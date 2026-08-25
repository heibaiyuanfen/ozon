from __future__ import annotations

import html
import io
import json
import re
import time
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any, Callable
from urllib.parse import urlparse

import requests
from PIL import Image, ImageOps


WB_PRODUCT_URL = "https://www.wildberries.ru/catalog/{nm_id}/detail.aspx"
WB_IMAGE_HOSTS = ("wbbasket.ru", "wbstatic.net")


@dataclass(frozen=True)
class WbSourceProduct:
    url: str
    nm_id: str
    title: str
    description: str
    brand: str
    images: list[str]
    properties: dict[str, str]

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)


def normalize_wb_source(value: str) -> tuple[str, str]:
    """Return the canonical WB product URL and nmID for a URL or numeric ID."""
    raw = str(value or "").strip()
    if not raw:
        raise ValueError("请填写 WB 来源链接或商品 ID")
    if re.fullmatch(r"\d{5,}", raw):
        nm_id = raw
        return WB_PRODUCT_URL.format(nm_id=nm_id), nm_id

    candidate = raw if "://" in raw else "https://" + raw
    try:
        parsed = urlparse(candidate)
    except ValueError as error:
        raise ValueError("WB 来源链接格式无效") from error
    host = str(parsed.hostname or "").casefold()
    if host != "wildberries.ru" and not host.endswith(".wildberries.ru"):
        raise ValueError("WB 跟卖来源必须是 wildberries.ru 商品链接或商品 ID")
    match = re.search(r"/catalog/(\d{5,})(?:/|$)", parsed.path, flags=re.I)
    if not match:
        match = re.search(r"(?:^|[?&])(?:nm|nm_id)=(\d{5,})(?:&|$)", parsed.query, flags=re.I)
    if not match:
        raise ValueError("无法从 WB 链接中识别商品 ID（nmID）")
    nm_id = match.group(1)
    return WB_PRODUCT_URL.format(nm_id=nm_id), nm_id


def _clean_text(value: Any) -> str:
    text = html.unescape(str(value or ""))
    text = re.sub(r"<br\s*/?>", "\n", text, flags=re.I)
    text = re.sub(r"</(?:p|li|div|h[1-6])\s*>", "\n", text, flags=re.I)
    text = re.sub(r"<[^>]+>", " ", text)
    text = re.sub(r"[ \t\r\f\v]+", " ", text)
    return re.sub(r"\n{3,}", "\n\n", text).strip()


def _json_ld(page: str):
    pattern = re.compile(
        r'<script[^>]+type=["\']application/ld\+json["\'][^>]*>(.*?)</script>',
        flags=re.I | re.S,
    )
    for raw in pattern.findall(str(page or "")):
        try:
            value = json.loads(html.unescape(raw).strip())
        except (TypeError, ValueError, json.JSONDecodeError):
            continue
        queue = list(value) if isinstance(value, list) else [value]
        while queue:
            item = queue.pop(0)
            if not isinstance(item, dict):
                continue
            yield item
            graph = item.get("@graph")
            if isinstance(graph, list):
                queue.extend(graph)


def _meta(page: str, key: str) -> str:
    for pattern in (
        rf'<meta[^>]+(?:property|name)=["\']{re.escape(key)}["\'][^>]+content=["\']([^"\']+)',
        rf'<meta[^>]+content=["\']([^"\']+)["\'][^>]+(?:property|name)=["\']{re.escape(key)}["\']',
    ):
        match = re.search(pattern, str(page or ""), flags=re.I)
        if match:
            return html.unescape(match.group(1)).strip()
    return ""


def prefer_wb_image_url(value: str) -> str:
    """Upgrade common WB thumbnail paths to the original-size image path."""
    raw = html.unescape(str(value or "").strip()).replace("\\/", "/")
    if raw.startswith("//"):
        raw = "https:" + raw
    if not raw.startswith(("http://", "https://")):
        return ""
    try:
        parsed = urlparse(raw)
    except ValueError:
        return ""
    host = str(parsed.hostname or "").casefold()
    if not any(host == suffix or host.endswith("." + suffix) for suffix in WB_IMAGE_HOSTS):
        return ""
    path = re.sub(
        r"/images/(?:c\d+x\d+|small|tm|tiny)/",
        "/images/big/",
        parsed.path,
        flags=re.I,
    )
    return parsed._replace(scheme="https", path=path, query="", fragment="").geturl()


def _image_urls(*values: Any) -> list[str]:
    result: list[str] = []

    def add(value: Any):
        if isinstance(value, (list, tuple, set)):
            for item in value:
                add(item)
            return
        if isinstance(value, dict):
            for item in value.values():
                add(item)
            return
        url = prefer_wb_image_url(str(value or ""))
        if url and "/images/" in urlparse(url).path.casefold() and url not in result:
            result.append(url)

    for value in values:
        add(value)
    return result[:30]


def parse_wb_source_html(source: str, page: str) -> WbSourceProduct:
    url, nm_id = normalize_wb_source(source)
    product = next(
        (item for item in _json_ld(page) if str(item.get("@type") or "").casefold() == "product"),
        {},
    )
    title = _clean_text(product.get("name") or _meta(page, "og:title"))
    description = _clean_text(product.get("description") or _meta(page, "og:description"))
    raw_brand = product.get("brand")
    if isinstance(raw_brand, dict):
        raw_brand = raw_brand.get("name") or raw_brand.get("brand")
    brand = _clean_text(raw_brand)

    decoded_page = str(page or "").replace("\\/", "/").replace("\\u0026", "&")
    embedded_images = re.findall(
        r'https?://[^\s"\'<>\\]+(?:wbbasket\.ru|wbstatic\.net)[^\s"\'<>\\]*',
        decoded_page,
        flags=re.I,
    )
    images = _image_urls(product.get("image"), _meta(page, "og:image"), embedded_images)

    raw_properties = product.get("additionalProperty") or []
    if isinstance(raw_properties, dict):
        raw_properties = [raw_properties]
    properties: dict[str, str] = {}
    for item in raw_properties:
        if not isinstance(item, dict):
            continue
        name = _clean_text(item.get("name") or item.get("propertyID"))
        value = item.get("value")
        if isinstance(value, (dict, list)):
            value = json.dumps(value, ensure_ascii=False)
        cleaned_value = _clean_text(value)
        if name and cleaned_value:
            properties[name] = cleaned_value
    if not title:
        raise RuntimeError("WB 商品页面没有返回可识别的标题")
    return WbSourceProduct(url, nm_id, title, description, brand, images, properties)


def _browser_snapshot(browser_page) -> dict[str, Any]:
    return browser_page.evaluate(
        r"""
        () => {
          const text = (selector) => document.querySelector(selector)?.innerText?.trim() || '';
          const meta = (key) => document.querySelector(`meta[property="${key}"],meta[name="${key}"]`)?.content || '';
          const images = [];
          const add = (raw) => {
            let value = String(raw || '').trim();
            if (value.startsWith('//')) value = `https:${value}`;
            if (/^https?:\/\//i.test(value) && /(wbbasket\.ru|wbstatic\.net)/i.test(value)) images.push(value);
          };
          for (const image of document.querySelectorAll('img')) {
            add(image.currentSrc || image.src || image.getAttribute('data-src'));
            for (const item of String(image.getAttribute('srcset') || '').split(',')) {
              add(item.trim().split(/\s+/)[0]);
            }
          }
          for (const entry of performance.getEntriesByType('resource')) add(entry.name);
          return {
            title: text('h1') || meta('og:title'),
            description: text('[class*="product-page__description"]') ||
              text('[data-testid*="description"]') || meta('og:description'),
            images: [...new Set(images)],
          };
        }
        """
    )


def scrape_wb_source_browser(
    source: str,
    *,
    profile_dir: str | Path,
    timeout: int = 180,
    log_func: Callable[[str], None] | None = None,
) -> WbSourceProduct:
    try:
        from playwright.sync_api import sync_playwright
    except ImportError as error:
        raise RuntimeError("请安装浏览器解析组件：pip install playwright") from error

    url, nm_id = normalize_wb_source(source)
    profile = Path(profile_dir)
    profile.mkdir(parents=True, exist_ok=True)
    if log_func:
        log_func("WB 直连页面信息不完整，正在打开独立浏览器窗口解析来源商品")
    with sync_playwright() as playwright:
        context = None
        last_error: Exception | None = None
        for channel in ("msedge", "chrome"):
            try:
                context = playwright.chromium.launch_persistent_context(
                    str(profile),
                    channel=channel,
                    headless=False,
                    locale="ru-RU",
                    viewport={"width": 1360, "height": 900},
                    args=["--disable-blink-features=AutomationControlled"],
                )
                break
            except Exception as error:
                last_error = error
        if context is None:
            raise RuntimeError(f"无法启动 Edge/Chrome 浏览器：{last_error}")
        try:
            browser_page = context.pages[0] if context.pages else context.new_page()
            try:
                browser_page.goto(url, wait_until="domcontentloaded", timeout=min(timeout * 1000, 120000))
            except Exception:
                pass
            deadline = time.monotonic() + timeout
            best: WbSourceProduct | None = None
            stable_rounds = 0
            previous_images: tuple[str, ...] = ()
            while time.monotonic() < deadline:
                if browser_page.is_closed():
                    raise RuntimeError("浏览器窗口已关闭，未完成 WB 来源解析")
                snapshot = _browser_snapshot(browser_page)
                try:
                    parsed = parse_wb_source_html(url, browser_page.content())
                except RuntimeError:
                    parsed = WbSourceProduct(
                        url=url,
                        nm_id=nm_id,
                        title=_clean_text(snapshot.get("title")),
                        description=_clean_text(snapshot.get("description")),
                        brand="",
                        images=[],
                        properties={},
                    )
                images = _image_urls(parsed.images, snapshot.get("images"))
                current = WbSourceProduct(
                    url=url,
                    nm_id=nm_id,
                    title=parsed.title or _clean_text(snapshot.get("title")),
                    description=parsed.description or _clean_text(snapshot.get("description")),
                    brand=parsed.brand,
                    images=images,
                    properties=parsed.properties,
                )
                if current.title and current.images:
                    best = current
                    image_key = tuple(current.images)
                    stable_rounds = stable_rounds + 1 if image_key == previous_images else 0
                    previous_images = image_key
                    if stable_rounds >= 2:
                        return current
                browser_page.mouse.wheel(0, 700)
                browser_page.wait_for_timeout(1000)
            if best is not None:
                return best
            raise RuntimeError("未能从 WB 页面读取完整标题和商品图片；请在浏览器中完成人机验证后重试")
        finally:
            context.close()


def scrape_wb_source(
    source: str,
    *,
    profile_dir: str | Path,
    timeout: int = 180,
    log_func: Callable[[str], None] | None = None,
    session=None,
) -> WbSourceProduct:
    url, _nm_id = normalize_wb_source(source)
    http = session or requests.Session()
    try:
        response = http.get(
            url,
            headers={
                "User-Agent": (
                    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) "
                    "AppleWebKit/537.36 Chrome/126.0 Safari/537.36"
                ),
                "Accept-Language": "ru-RU,ru;q=0.9,en;q=0.7",
            },
            timeout=(20, 60),
        )
        if int(getattr(response, "status_code", 0)) == 200:
            parsed = parse_wb_source_html(url, response.text)
            if parsed.images:
                return parsed
    except (requests.RequestException, RuntimeError, ValueError) as error:
        if log_func:
            log_func(f"WB 来源直连解析未完成：{error}")
    return scrape_wb_source_browser(
        url, profile_dir=profile_dir, timeout=timeout, log_func=log_func,
    )


def download_wb_source_images(
    image_urls: list[str],
    output_dir: str | Path,
    *,
    session=None,
    log_func: Callable[[str], None] | None = None,
) -> list[str]:
    urls = _image_urls(image_urls)
    if not urls:
        raise ValueError("WB 来源商品没有可下载的图片")
    target = Path(output_dir)
    target.mkdir(parents=True, exist_ok=True)
    http = session or requests.Session()
    downloaded: list[str] = []
    errors: list[str] = []
    for index, image_url in enumerate(urls[:30], start=1):
        try:
            response = http.get(
                image_url,
                headers={
                    "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/126.0",
                    "Referer": "https://www.wildberries.ru/",
                },
                timeout=(20, 90),
            )
            if int(getattr(response, "status_code", 0)) != 200:
                raise RuntimeError(f"HTTP {getattr(response, 'status_code', 0)}")
            content = bytes(response.content)
            if not content or len(content) > 32 * 1024 * 1024:
                raise RuntimeError("文件为空或超过 32 MB")
            with Image.open(io.BytesIO(content)) as source_image:
                image = ImageOps.exif_transpose(source_image).convert("RGB")
                width, height = image.size
                if width < 700 or height < 900:
                    raise RuntimeError(f"分辨率 {width}x{height} 低于 WB 要求 700x900")
                name = f"{index:02d}-main.jpg" if index == 1 else f"{index:02d}.jpg"
                destination = target / name
                image.save(destination, format="JPEG", quality=94, optimize=True)
            downloaded.append(str(destination))
            if log_func:
                log_func(f"WB 来源图片已下载 {index}/{len(urls)}：{destination.name}")
        except Exception as error:
            errors.append(f"第 {index} 张：{error}")
            if log_func:
                log_func(f"WB 来源图片下载失败：第 {index} 张，{error}")
    if not downloaded:
        detail = "；".join(errors[:3])
        raise RuntimeError("WB 来源图片全部下载失败" + ("：" + detail if detail else ""))
    return downloaded
