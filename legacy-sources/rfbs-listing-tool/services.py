from __future__ import annotations

import base64
import hashlib
import io
import json
import mimetypes
import os
import re
import time
from pathlib import Path
from typing import Any
from urllib.parse import quote, urlsplit, urlunsplit

import requests
from PIL import Image, ImageOps, ImageStat
from requests.adapters import HTTPAdapter
from urllib3.util.retry import Retry

from core import (
    ReferenceProduct, USER_AGENT, atomic_write_json, load_json,
    format_ozon_hashtags, prefer_high_resolution_image_url, rank_categories,
    reference_fact_map, validate_ozon_hashtags,
)


class OzonApi:
    BASE_URL = "https://api-seller.ozon.ru"
    CATALOG_LANGUAGES = {"DEFAULT", "RU", "EN", "TR", "ZH_HANS"}

    def __init__(
        self, client_id: str, api_key: str, timeout=60, proxy_url: str = "",
        catalog_language: str = "ZH_HANS",
    ):
        if not client_id.strip() or not api_key.strip():
            raise ValueError("请填写 Ozon Client-Id 和 Api-Key")
        self.client_id = client_id.strip()
        self.api_key = api_key.strip()
        normalized_language = str(catalog_language or "ZH_HANS").strip().upper()
        if normalized_language not in self.CATALOG_LANGUAGES:
            raise ValueError("Ozon 类目语言必须是 DEFAULT、RU、EN、TR 或 ZH_HANS")
        self.catalog_language = normalized_language
        self.timeout = (20, max(45, int(timeout)))
        self.session = requests.Session()
        raw_proxy = str(proxy_url or "").strip()
        if raw_proxy and "://" not in raw_proxy:
            raw_proxy = "http://" + raw_proxy
        if raw_proxy:
            parsed_proxy = urlsplit(raw_proxy)
            if parsed_proxy.scheme not in {"http", "https"} or not parsed_proxy.hostname:
                raise ValueError("Ozon 代理地址格式错误，请填写例如 http://127.0.0.1:7890")
            self.session.proxies.update({"http": raw_proxy, "https": raw_proxy})
        self.proxy_url = raw_proxy
        # Retry only failures that happen while establishing the connection.
        # Read retries are disabled because a product import may already have
        # reached Ozon even when its response is lost.
        connect_retry = Retry(
            total=3, connect=3, read=0, redirect=0, status=0,
            backoff_factor=1.0, allowed_methods=frozenset({"POST"}),
        )
        self.session.mount("https://", HTTPAdapter(max_retries=connect_retry))

    def post(self, path: str, body: dict[str, Any]) -> dict[str, Any]:
        try:
            response = self.session.post(self.BASE_URL + path, headers={
                "Client-Id": self.client_id,
                "Api-Key": self.api_key,
                "Content-Type": "application/json",
                "Accept": "application/json",
            }, json=body, timeout=self.timeout)
        except requests.exceptions.ProxyError as error:
            raise RuntimeError(
                f"无法通过 Ozon HTTP 代理连接：{self.proxy_url or '未填写'}。请确认代理软件已启动、端口正确，"
                "并且该端口支持 HTTP 代理。"
            ) from error
        except requests.exceptions.ConnectTimeout as error:
            proxy_hint = (
                f"当前代理：{self.proxy_url}，请检查代理端口。"
                if self.proxy_url else
                "若浏览器能打开 Ozon 但程序仍超时，请在服务配置填写代理软件的 HTTP 代理地址，例如 http://127.0.0.1:7890。"
            )
            raise RuntimeError(
                "连接 Ozon Seller API 超时；已自动重试 3 次。" + proxy_hint
            ) from error
        except requests.exceptions.ReadTimeout as error:
            raise RuntimeError(
                "已连接 Ozon Seller API，但等待响应超时。为避免重复创建商品，导入请求不会自动进行读取重试；"
                "请先在 Ozon 卖家后台按 Offer ID 检查是否已经生成。"
            ) from error
        except requests.exceptions.ConnectionError as error:
            proxy_hint = f"当前代理：{self.proxy_url}。" if self.proxy_url else "当前未配置 Ozon HTTP 代理。"
            if "timed out" in str(error).casefold() or "timeout" in str(error).casefold():
                raise RuntimeError(
                    "连接 Ozon Seller API 超时；已自动重试 3 次。" + proxy_hint
                    + ("请检查代理软件和端口。" if self.proxy_url else "若浏览器可访问 Ozon，请填写代理软件的 HTTP 代理地址。")
                ) from error
            raise RuntimeError(
                "无法连接 api-seller.ozon.ru:443。请检查网络、代理/VPN、DNS 或防火墙。" + proxy_hint
            ) from error
        try:
            response.raise_for_status()
        except requests.HTTPError as error:
            detail = response.text[:2000]
            raise RuntimeError(f"Ozon API {path} 返回 HTTP {response.status_code}: {detail}") from error
        return response.json()

    def category_tree(self, language: str | None = None):
        selected_language = language or getattr(self, "catalog_language", "ZH_HANS")
        data = self.post("/v1/description-category/tree", {"language": selected_language})
        return data.get("result", data.get("items", []))

    def attributes(self, category_id: int, type_id: int, language: str | None = None):
        data = self.post("/v1/description-category/attribute", {
            "description_category_id": int(category_id), "type_id": int(type_id),
            "language": language or getattr(self, "catalog_language", "ZH_HANS"),
        })
        return data.get("result", [])

    def dictionary_values(
        self, category_id: int, type_id: int, attribute_id: int, query: str,
        language: str | None = None,
    ):
        data = self.post("/v1/description-category/attribute/values/search", {
            "description_category_id": int(category_id), "type_id": int(type_id),
            "attribute_id": int(attribute_id), "value": query, "limit": 100,
            "language": language or getattr(self, "catalog_language", "ZH_HANS"),
        })
        return data.get("result", [])

    def dictionary_values_all(
        self, category_id: int, type_id: int, attribute_id: int,
        language: str | None = None, *, page_limit: int = 2000, log_func=None,
    ) -> list[dict[str, Any]]:
        """Load the complete live dictionary using Ozon's last_value_id pagination."""
        selected_language = language or getattr(self, "catalog_language", "ZH_HANS")
        last_value_id = 0
        values: list[dict[str, Any]] = []
        seen_ids: set[int] = set()
        for page_number in range(1, 1001):
            data = self.post("/v1/description-category/attribute/values", {
                "description_category_id": int(category_id),
                "type_id": int(type_id),
                "attribute_id": int(attribute_id),
                "language": selected_language,
                "last_value_id": int(last_value_id),
                "limit": int(page_limit),
            })
            result = data.get("result", [])
            if isinstance(result, dict):
                page = result.get("values") or result.get("items") or []
            else:
                page = result
            page = [item for item in (page or []) if isinstance(item, dict) and item.get("id")]
            for item in page:
                value_id = int(item["id"])
                if value_id not in seen_ids:
                    values.append(item)
                    seen_ids.add(value_id)
            if log_func and (page_number == 1 or page_number % 5 == 0 or len(page) < page_limit):
                log_func(f"字典属性 {attribute_id}：已加载 {len(values)} 个选项（第 {page_number} 页）")
            if len(page) < page_limit:
                return values
            next_last_value_id = int(page[-1]["id"])
            if next_last_value_id == last_value_id:
                raise RuntimeError(f"Ozon 字典属性 {attribute_id} 返回了重复分页游标，已停止")
            last_value_id = next_last_value_id
        raise RuntimeError(f"Ozon 字典属性 {attribute_id} 超过 1000 页，已停止以避免无限请求")

    def import_product(self, item: dict[str, Any]) -> int:
        data = self.post("/v3/product/import", {"items": [item]})
        return int(data["result"]["task_id"])

    def import_info(self, task_id: int):
        return self.post("/v1/product/import/info", {"task_id": int(task_id)})

    def wait_import(
        self, task_id: int, attempts=30, interval=3, log_func=None, cancel_check=None,
    ):
        def interruptible_wait(seconds):
            deadline = time.monotonic() + max(0, seconds)
            while time.monotonic() < deadline:
                if cancel_check:
                    cancel_check()
                time.sleep(min(0.25, max(0, deadline - time.monotonic())))

        last = None
        last_error = None
        for attempt in range(1, attempts + 1):
            if cancel_check:
                cancel_check()
            try:
                last = self.import_info(task_id)
                last_error = None
            except RuntimeError as error:
                # Reading an existing import task is safe to retry.  The product
                # creation request itself is intentionally never repeated here.
                last_error = error
                if re.search(r"http\s+(400|401|403|404|405|409|413|415|422)", str(error).casefold()):
                    raise
                if log_func:
                    log_func(f"读取 Ozon 导入任务 {task_id} 状态失败（{attempt}/{attempts}）：{error}；将继续查询")
                interruptible_wait(interval)
                continue
            items = last.get("result", {}).get("items", [])
            statuses = {str(item.get("status", "")).casefold() for item in items}
            if any(item.get("errors") for item in items) or statuses & {"failed", "error", "imported"}:
                return last
            if log_func and (attempt == 1 or attempt % 5 == 0):
                rendered = "、".join(sorted(statuses)) or "等待 Ozon 返回状态"
                log_func(f"Ozon 导入任务 {task_id} 仍在处理（{attempt}/{attempts}）：{rendered}")
            interruptible_wait(interval)
        if last is None and last_error is not None:
            raise last_error
        return last

    def update_price(
        self, offer_id: str, price, *, old_price="0", currency_code="RUB", min_price="0",
    ) -> dict[str, Any]:
        price_item = {
            "auto_action_enabled": "UNKNOWN",
            "currency_code": str(currency_code or "RUB").strip().upper(),
            "min_price": str(min_price or "0"),
            "offer_id": str(offer_id or "").strip(),
            "old_price": str(old_price or "0"),
            "price": str(price),
        }
        data = self.post("/v1/product/import/prices", {"prices": [price_item]})
        results = data.get("result") if isinstance(data, dict) else None
        matching = next((
            entry for entry in (results or [])
            if isinstance(entry, dict)
            and str(entry.get("offer_id") or "").strip() == price_item["offer_id"]
        ), None)
        if matching is None:
            raise RuntimeError(
                f"Ozon 改价接口未返回商品 {price_item['offer_id']} 的处理结果"
            )
        if matching.get("errors") or matching.get("updated") is not True:
            raise RuntimeError(
                "Ozon 商品改价失败：" + json.dumps(matching, ensure_ascii=False)
            )
        return data

    def warehouses(self):
        warehouses: list[dict[str, Any]] = []
        cursor = ""
        seen_cursors = set()
        while True:
            body: dict[str, Any] = {"limit": 100}
            if cursor:
                body["cursor"] = cursor
            data = self.post("/v2/warehouse/list", body)
            # Current v2 response is top-level `warehouses`. Keep a guarded
            # fallback for accounts still receiving a transitional envelope.
            envelope = data.get("result") if isinstance(data.get("result"), dict) else data
            page = envelope.get("warehouses")
            if page is None:
                page = data.get("result") if isinstance(data.get("result"), list) else data.get("items", [])
            warehouses.extend(item for item in (page or []) if isinstance(item, dict))
            next_cursor = str(envelope.get("cursor") or data.get("cursor") or "")
            has_next = bool(envelope.get("has_next", data.get("has_next", False)))
            if not has_next or not next_cursor or next_cursor in seen_cursors:
                break
            seen_cursors.add(next_cursor)
            cursor = next_cursor
        return warehouses

    def update_stock(self, offer_id: str, warehouse_id: int, stock: int):
        return self.post("/v2/products/stocks", {"stocks": [{
            "offer_id": offer_id, "warehouse_id": int(warehouse_id), "stock": int(stock)
        }]})


class OzonDictionaryCache:
    VERSION = 1

    def __init__(self, cache_dir: str | Path):
        self.cache_dir = Path(cache_dir)

    def path_for(self, category_id: int, type_id: int, attribute_id: int, language: str) -> Path:
        safe_language = re.sub(r"[^A-Z_]", "", str(language or "ZH_HANS").upper()) or "ZH_HANS"
        return self.cache_dir / f"{int(category_id)}-{int(type_id)}-{int(attribute_id)}-{safe_language}.json"

    def load(
        self, api: OzonApi, category_id: int, type_id: int, attribute_id: int,
        language: str = "ZH_HANS", *, force_refresh=False, log_func=None,
    ) -> tuple[list[dict[str, Any]], bool, Path]:
        path = self.path_for(category_id, type_id, attribute_id, language)
        if not force_refresh and path.is_file():
            cached = load_json(path, {})
            values = cached.get("values") if isinstance(cached, dict) else None
            if cached.get("version") == self.VERSION and isinstance(values, list):
                cleaned = [item for item in values if isinstance(item, dict) and item.get("id")]
                if log_func:
                    log_func(f"已从本地缓存读取字典属性 {attribute_id}，共 {len(cleaned)} 个选项")
                return cleaned, True, path
        values = api.dictionary_values_all(
            category_id, type_id, attribute_id, language, log_func=log_func,
        )
        atomic_write_json(path, {
            "version": self.VERSION,
            "saved_at": time.strftime("%Y-%m-%d %H:%M:%S"),
            "description_category_id": int(category_id),
            "type_id": int(type_id),
            "attribute_id": int(attribute_id),
            "language": language,
            "values": values,
        })
        if log_func:
            log_func(f"完整字典已缓存：{path}（{len(values)} 个选项）")
        return values, False, path


class WatermarkService:
    @staticmethod
    def apply(image_paths: list[str], watermark_path: str, output_dir: str, scale=0.18, opacity=0.72) -> list[str]:
        watermark_file = Path(watermark_path)
        if not watermark_file.is_file():
            raise FileNotFoundError("水印文件不存在")
        target_dir = Path(output_dir)
        target_dir.mkdir(parents=True, exist_ok=True)
        with Image.open(watermark_file) as raw_mark:
            mark_source = raw_mark.convert("RGBA")
            results = []
            for raw_path in image_paths:
                source_path = Path(raw_path)
                with Image.open(source_path) as raw_image:
                    image = raw_image.convert("RGBA")
                    width = max(1, round(image.width * float(scale)))
                    height = max(1, round(mark_source.height * width / mark_source.width))
                    mark = mark_source.resize((width, height), Image.Resampling.LANCZOS)
                    alpha = mark.getchannel("A").point(lambda value: round(value * float(opacity)))
                    mark.putalpha(alpha)
                    margin = max(12, round(min(image.size) * 0.025))
                    image.alpha_composite(mark, (image.width - width - margin, image.height - height - margin))
                    output = target_dir / f"{source_path.stem}_watermarked.jpg"
                    image.convert("RGB").save(output, "JPEG", quality=95, optimize=True)
                    results.append(str(output))
        return results


class ImageDownloadService:
    """Download and normalize storefront images without depending on the old project."""

    def __init__(self, session=None, timeout=90):
        self.session = session or requests.Session()
        self.timeout = timeout

    @staticmethod
    def _visual_signature(image: Image.Image) -> tuple[int, tuple[float, float, float]]:
        grayscale = image.convert("L").resize((16, 16), Image.Resampling.LANCZOS)
        pixels = list(grayscale.get_flattened_data())
        average = sum(pixels) / len(pixels)
        bits = 0
        for value in pixels:
            bits = (bits << 1) | int(value >= average)
        color_mean = tuple(ImageStat.Stat(image.resize((32, 32), Image.Resampling.LANCZOS)).mean[:3])
        return bits, color_mean

    @staticmethod
    def _visually_same(
        left: tuple[int, tuple[float, float, float]],
        right: tuple[int, tuple[float, float, float]],
    ) -> bool:
        hash_distance = (left[0] ^ right[0]).bit_count()
        color_distance = max(abs(a - b) for a, b in zip(left[1], right[1]))
        return hash_distance <= 6 and color_distance <= 12

    def download(self, urls: list[str], output_dir: str, limit=15, log_func=None) -> list[str]:
        if not urls:
            raise ValueError("商品链接中没有解析到可下载图片")
        target_dir = Path(output_dir)
        target_dir.mkdir(parents=True, exist_ok=True)
        results: list[str] = []
        fingerprints: set[str] = set()
        visual_signatures: list[tuple[int, tuple[float, float, float]]] = []
        duplicate_count = 0
        failures: list[str] = []
        max_images = max(1, int(limit))
        for index, url in enumerate(urls, start=1):
            if len(results) >= max_images:
                break
            try:
                high_resolution_url = prefer_high_resolution_image_url(str(url))
                response = self.session.get(high_resolution_url, headers={
                    "User-Agent": USER_AGENT,
                    "Referer": "https://www.ozon.ru/",
                    "Accept": "image/avif,image/webp,image/apng,image/*,*/*;q=0.8",
                }, timeout=self.timeout)
                response.raise_for_status()
                if len(response.content) > 30 * 1024 * 1024:
                    raise RuntimeError("图片超过 30MB")
                with Image.open(io.BytesIO(response.content)) as raw_image:
                    image = ImageOps.exif_transpose(raw_image).convert("RGBA")
                    if min(image.size) < 600:
                        raise RuntimeError(f"高清图短边不足 600 像素：{image.width}x{image.height}")
                    background = Image.new("RGBA", image.size, "white")
                    background.alpha_composite(image)
                    rgb = background.convert("RGB")
                    fingerprint = hashlib.sha256(
                        f"{rgb.width}x{rgb.height}".encode("ascii") + rgb.tobytes()
                    ).hexdigest()
                    visual_signature = self._visual_signature(rgb)
                    if fingerprint in fingerprints or any(
                        self._visually_same(visual_signature, existing) for existing in visual_signatures
                    ):
                        duplicate_count += 1
                        if log_func:
                            log_func(f"第 {index} 张与前面的图片视觉重复，已跳过")
                        continue
                    fingerprints.add(fingerprint)
                    visual_signatures.append(visual_signature)
                    output = target_dir / f"{len(results) + 1:02d}_{fingerprint[:10]}.jpg"
                    rgb.save(output, "JPEG", quality=96, optimize=True)
                    results.append(str(output))
                    if log_func:
                        log_func(f"已下载原图 {len(results)}：{rgb.width}x{rgb.height}")
            except Exception as error:
                failures.append(f"第 {index} 张：{error}")
        if not results:
            detail = "；".join(failures[:3])
            raise RuntimeError("原图全部下载失败" + ("：" + detail if detail else ""))
        if failures and log_func:
            log_func(f"有 {len(failures)} 张图片下载失败，已跳过")
        if duplicate_count and log_func:
            log_func(f"已自动跳过 {duplicate_count} 张视觉重复图片")
        return results


def normalize_chat_completions_url(value: str) -> str:
    """Accept either a provider base URL or a complete Chat Completions endpoint."""
    raw = str(value or "").strip()
    try:
        parsed = urlsplit(raw)
    except ValueError:
        return raw
    if not parsed.scheme or not parsed.netloc:
        return raw
    path = parsed.path.rstrip("/")
    if path in {"", "/v1"}:
        path = "/v1/chat/completions"
    return urlunsplit((parsed.scheme, parsed.netloc, path, parsed.query, parsed.fragment))


def normalize_image_edits_url(value: str) -> str:
    """Accept either an OpenAI-compatible base URL or a complete image-edits endpoint."""
    raw = str(value or "").strip()
    try:
        parsed = urlsplit(raw)
    except ValueError:
        return raw
    if not parsed.scheme or not parsed.netloc:
        return raw
    path = parsed.path.rstrip("/")
    if path in {"", "/v1"}:
        path = "/v1/images/edits"
    return urlunsplit((parsed.scheme, parsed.netloc, path, parsed.query, parsed.fragment))


def _short_poster_line(value: str, maximum=68) -> str:
    text = re.sub(r"\s+", " ", str(value or "")).strip()
    if not text or re.search(r"[\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff]", text):
        return ""
    if len(text) <= maximum:
        return text
    for separator in (", ", " — ", "; "):
        first = text.split(separator, 1)[0].strip()
        if 12 <= len(first) <= maximum:
            return first
    shortened = text[:maximum + 1].rsplit(" ", 1)[0].rstrip(" ,;:-")
    return shortened if len(shortened) >= 12 else ""


def build_ozon_poster_text_lines(title: str, reference: ReferenceProduct) -> list[str]:
    """Select compact Russian poster copy from generated title and traceable Ozon facts."""
    lines: list[str] = []

    def add(value, maximum=68):
        line = _short_poster_line(value, maximum)
        if line and line.casefold() not in {item.casefold() for item in lines}:
            lines.append(line)

    headline = re.sub(r"\s+", " ", str(title or "")).strip()
    if ", " in headline:
        concise_headline = headline.split(", ", 1)[0].strip()
        if len(concise_headline) >= 12:
            headline = concise_headline
    add(headline, 68)
    facts = reference_fact_map(reference)
    preferred_names = (
        "особенности игрушки", "вид игрушки", "предназначено для",
        "назначение", "особенности", "тип", "материал", "размеры",
        "особенность конструкции", "целевое назначение",
    )
    used_fact_keys = set()
    for wanted in preferred_names:
        match = next((
            (key, source_value)
            for key, source_value in facts.items()
            if key not in used_fact_keys and wanted in key
        ), None)
        if not match:
            continue
        key, (_source_name, raw_value) = match
        value = re.sub(r"\s+", " ", str(raw_value or "")).strip()
        if not value or len(value) > 120 or re.search(r"[\u3400-\u9fff]", value):
            continue
        if len(value) > 52 and "," in value:
            parts = [part.strip() for part in value.split(",") if part.strip()]
            value = ", ".join(parts[:2])
        if len(value) > 52:
            continue
        value = value[:1].upper() + value[1:] if value else ""
        add(value, 52)
        used_fact_keys.add(key)
        if len(lines) >= 4:
            break
    return lines[:4]


def build_ozon_main_image_prompt(
    creative_direction: str = "",
    poster_text_lines: list[str] | None = None,
) -> str:
    """Compile the reference program's SKU-lock and Russian poster policies."""
    max_prompt_chars = 3800
    direction = re.sub(r"\s+", " ", str(creative_direction or "")).strip()
    text_lines = []
    for raw_line in poster_text_lines or []:
        line = _short_poster_line(raw_line)
        if line and line not in text_lines:
            text_lines.append(line)
    text_lines = text_lines[:4]
    whitelist = "\n".join(f'{index}. "{line}"' for index, line in enumerate(text_lines, start=1))
    if len(text_lines) == 1:
        text_layout = (
            "Render exactly one added visible Russian text element: line 1 as a large primary headline. "
            "Create no supporting callout cards or additional captions."
        )
    else:
        text_layout = (
            f"Render exactly {len(text_lines)} added visible Russian text elements. "
            "Use line 1 as the large primary product headline and only the remaining lines as compact "
            "supporting callout cards, labels, or restrained icon-led information modules."
        )
    rules = f"""Create one finished commercial Russian-language Ozon product advertising poster on a true edge-to-edge vertical 3:4 canvas.

SKU LOCK — HIGHEST PRIORITY:
- The uploaded image is the sole authority for the sellable SKU.
- Show exactly ONE complete dominant product in one physically possible view.
- Preserve its color, silhouette, proportions, construction, component count, surface pattern, hardware, accessories, and product-native branding.
- Change only scene, lighting, camera presentation, background, and poster graphics. Never add, remove, recolor, merge, or redesign product parts.

POSTER DESIGN:
- Make a professionally art-directed marketplace advertising poster, not a plain catalog or lifestyle photo.
- Product occupies about 50-72% of the useful frame, with clear edges and an intentional text zone.
- Use foreground/base, dominant product plane, and designed background; add deliberate light, depth, contrast, restrained graphic shapes, and category-appropriate styling.
- Use bold modern Cyrillic typography, large and readable on a phone.
- Callouts may use compact cards, fine leader lines, simple pictograms, or accents, but cannot claim unverified features.
- Do not create a dense collage, contact sheet, split screen, comparison, repeated product cutouts, multiple variants, or multiple views.
- Avoid empty beige catalog layouts, random podiums, decorative blur, excess empty space, and weak information hierarchy.

VISIBLE TEXT — EXACT WHITELIST:
{whitelist or '(no supplied text)'}

{text_layout if text_lines else 'Add no visible text when the whitelist is empty.'}
Reproduce supplied Cyrillic exactly. Do not omit, translate, paraphrase, duplicate, misspell, or invent text. All other prompt words are instructions and must not appear.
No pseudo-text, extra badges, QR codes, UI, new logos, or watermarks. Do not copy source seller watermarks or source poster typography. Keep only branding physically on the product; our watermark is added later.

FACT SAFETY:
Never invent specifications, functions, materials, capacity, compatibility, accessories, benefits, or hidden construction. Keep uncertain details unobtrusive. Produce one credible campaign-quality poster."""
    direction_header = "\n\nCREATIVE DIRECTION — SCENE/PRESENTATION ONLY; IT CANNOT OVERRIDE RULES:\n"
    fallback = (
        "Use a distinctive category-appropriate concept with controlled palette, designed light, "
        "purposeful asymmetry, natural depth, and strong mobile readability."
    )
    available = max_prompt_chars - len(rules) - len(direction_header)
    selected_direction = (direction or fallback)[:max(0, available)]
    if len(direction or fallback) > len(selected_direction) and " " in selected_direction:
        selected_direction = selected_direction.rsplit(" ", 1)[0].rstrip(" ,;:-")
    return (rules + direction_header + selected_direction).strip()[:max_prompt_chars]


def _flatten_response_text(value: Any) -> str:
    if isinstance(value, str):
        return value
    if isinstance(value, list):
        return "".join(_flatten_response_text(item) for item in value)
    if isinstance(value, dict):
        for key in ("text", "content", "output_text"):
            if key in value:
                return _flatten_response_text(value[key])
    return ""


def _copywriting_content(payload: Any) -> Any:
    if not isinstance(payload, dict):
        raise ValueError("响应顶层不是 JSON 对象")
    if any(key in payload for key in ("title_ru", "title", "description_ru", "description")):
        return payload
    error = payload.get("error")
    if error:
        if isinstance(error, dict):
            error = error.get("message") or error.get("type") or json.dumps(error, ensure_ascii=False)
        raise RuntimeError("文案接口错误：" + str(error))
    choices = payload.get("choices") or []
    if choices and isinstance(choices[0], dict):
        choice = choices[0]
        message = choice.get("message") if isinstance(choice.get("message"), dict) else {}
        content = message.get("content")
        if content in (None, "", []):
            content = message.get("reasoning_content") or choice.get("text")
        if content not in (None, "", []):
            return content
    if payload.get("output_text") not in (None, "", []):
        return payload["output_text"]
    for output in payload.get("output") or []:
        text = _flatten_response_text(output)
        if text:
            return text
    raise ValueError("响应中没有可识别的模型文本")


def _json_object_from_content(content: Any) -> dict[str, Any]:
    if isinstance(content, dict):
        return content
    raw = _flatten_response_text(content).strip()
    if not raw:
        raise ValueError("模型文本为空")
    cleaned = re.sub(r"```(?:json)?", "", raw, flags=re.I).replace("```", "").strip()
    candidate: Any = cleaned
    for _ in range(2):
        try:
            candidate = json.loads(candidate) if isinstance(candidate, str) else candidate
        except (TypeError, ValueError, json.JSONDecodeError):
            break
        if isinstance(candidate, dict):
            return candidate
    decoder = json.JSONDecoder()
    for index, character in enumerate(cleaned):
        if character != "{":
            continue
        try:
            candidate, _ = decoder.raw_decode(cleaned[index:])
        except json.JSONDecodeError:
            continue
        if isinstance(candidate, dict):
            return candidate
    raise ValueError("模型文本中没有完整 JSON 对象")


def _contains_cjk(value: Any) -> bool:
    return bool(re.search(r"[\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff]", str(value or "")))


def _vision_image_data_url(path: str, maximum=1400) -> str:
    """Prepare a bounded JPEG data URL for an OpenAI-compatible vision request."""
    source = Path(path)
    if not source.is_file():
        raise FileNotFoundError(f"视觉分析参考图不存在：{source}")
    try:
        with Image.open(source) as opened:
            image = ImageOps.exif_transpose(opened).convert("RGB")
            image.thumbnail((maximum, maximum), Image.Resampling.LANCZOS)
            buffer = io.BytesIO()
            image.save(buffer, format="JPEG", quality=84, optimize=True)
    except Exception as error:
        raise RuntimeError(f"无法读取视觉分析参考图：{source.name}；{error}") from error
    encoded = base64.b64encode(buffer.getvalue()).decode("ascii")
    return "data:image/jpeg;base64," + encoded


def visual_attribute_allowed(definition: dict[str, Any]) -> bool:
    """Return whether appearance alone can safely support this Ozon attribute."""
    name = " ".join((
        str(definition.get("name") or ""),
        str(definition.get("name_ru") or ""),
        str(definition.get("description") or ""),
    )).casefold()
    blocked_fragments = (
        "бренд", "brand", "品牌",
        "модел", "model", "型号", "артикул", "货号", "штрихкод", "条形码",
        "вес", "weight", "重量", "масса",
        "размер", "dimension", "尺寸", "габарит", "длина", "ширина", "высота", "长度", "宽度", "高度",
        "материал", "material", "材质", "состав", "composition", "成分", "наполнитель", "填充",
        "ткан", "fabric", "面料",
        "сертифик", "certif", "认证", "декларац", "лиценз", "许可证",
        "страна", "country", "产地", "制造商", "производител",
        "гарант", "warranty", "保证",
        "упаков", "package", "包装", "комплект", "included", "套装", "配件",
        "объем", "volume", "容量", "совместим", "compatib", "兼容",
    )
    return bool(name.strip()) and not any(fragment in name for fragment in blocked_fragments)


class WbCardAiService:
    """Fill one WB card from selected images using only live category fields."""

    MAX_IMAGES = 6

    def __init__(self, api_url: str, api_key: str, model: str, timeout=240, session=None):
        self.api_url = normalize_chat_completions_url(api_url)
        self.api_key = str(api_key or "").strip()
        self.model = str(model or "").strip()
        self.timeout = timeout
        self.session = session or requests.Session()

    def generate(
        self, *, subject_id: int, subject_name: str,
        definitions: list[dict[str, Any]], image_paths: list[str],
        current_document: dict[str, Any], source_context: dict[str, Any] | None = None,
    ) -> dict[str, Any]:
        if not self.api_url or not self.api_key or not self.model:
            raise ValueError("请先在服务配置中填写支持图片输入的 AI 接口、密钥和模型")
        selected_paths = [str(path) for path in image_paths if str(path).strip()][:self.MAX_IMAGES]
        if not selected_paths:
            raise ValueError("AI 填写 WB 内容前，请先在“图片与库存”中选择至少一张商品图片")
        allowed: dict[int, dict[str, Any]] = {}
        simplified = []
        for item in definitions:
            if not isinstance(item, dict) or item.get("charcID") in (None, ""):
                continue
            charc_id = int(item["charcID"])
            allowed[charc_id] = item
            simplified.append({
                "id": charc_id,
                "name": str(item.get("name") or ""),
                "required": bool(item.get("required")),
                "type": int(item.get("charcType") or 0),
                "unit": str(item.get("unitName") or ""),
                "maxCount": int(item.get("maxCount") or 0),
                "popular": bool(item.get("popular")),
            })
        if not allowed:
            raise ValueError("当前没有 WB API 返回的类目属性，请先点击“读取类目属性”")

        prompt = (
            "Analyze all supplied product images and fill the editable Wildberries card JSON. "
            "Return exactly one JSON object with subjectID and one item in variants, and no markdown. "
            "The variant may contain only vendorCode, title, description, brand, characteristics and sizes. "
            "Do not return dimensions: length, width, height and weight remain manual fields in the application. "
            "Write a factual Russian title of at most 60 characters and a factual Russian description of at most "
            "5000 characters. Preserve vendorCode and every barcode in sizes.skus exactly. Use brand only when it "
            "is supplied in current_card/source_context or is clearly readable on the product; otherwise omit it. "
            "techSize is the seller's real size label, such as S or 42. wbSize is Wildberries' standardized display "
            "size. Fill them only for a genuinely sized product when supported by the supplied evidence. For a "
            "product without size, keep one sizes object with skus only and omit techSize and wbSize. "
            "For characteristics, use only IDs from live_characteristics. Never invent or alter an ID. Attribute "
            "values must follow the listed type and be suitable for the WB API; ordinary text characteristics use "
            "arrays of concise Russian strings, and numeric type 4 uses a JSON number. Fill a field only when it is "
            "supported by a visible fact or supplied source/current data. Never guess dimensions, weight, material, "
            "composition, certification, country, manufacturer, warranty, package quantity, hidden accessories, "
            "compatibility or technical specifications. It is acceptable to leave unsupported required fields absent "
            "for later manual review.\n"
            f"Selected subject: {subject_name} [{int(subject_id)}]\n"
            "Live characteristics:\n" + json.dumps(simplified, ensure_ascii=False)
            + "\nCurrent editable card:\n" + json.dumps(current_document, ensure_ascii=False)
            + "\nSupplied source context:\n" + json.dumps(source_context or {}, ensure_ascii=False)[:20000]
        )
        content: list[dict[str, Any]] = [{"type": "text", "text": prompt}]
        for index, path in enumerate(selected_paths, start=1):
            content.append({"type": "text", "text": f"PRODUCT IMAGE {index}"})
            content.append({
                "type": "image_url",
                "image_url": {"url": _vision_image_data_url(path), "detail": "high"},
            })
        response = self.session.post(self.api_url, headers={
            "Authorization": f"Bearer {self.api_key}",
            "Content-Type": "application/json",
        }, json={
            "model": self.model,
            "temperature": 0.1,
            "stream": False,
            "messages": [
                {
                    "role": "system",
                    "content": (
                        "You prepare conservative Russian Wildberries product-card data from images and live API "
                        "schemas. You distinguish evidence from guesses and return strict JSON."
                    ),
                },
                {"role": "user", "content": content},
            ],
        }, timeout=self.timeout)
        try:
            response.raise_for_status()
        except requests.HTTPError as error:
            raise RuntimeError(f"WB 图片 AI 返回 HTTP {response.status_code}: {response.text[:1500]}") from error
        try:
            payload = response.json()
        except (TypeError, ValueError, json.JSONDecodeError) as error:
            preview = re.sub(r"\s+", " ", str(response.text or "")).strip()[:500]
            raise RuntimeError(f"WB 图片 AI 未返回 JSON；响应预览：{preview or '空响应'}") from error
        content_value = (
            payload if isinstance(payload, dict) and "variants" in payload and not payload.get("error")
            else _copywriting_content(payload)
        )
        document = _json_object_from_content(content_value)
        try:
            returned_subject_id = int(document.get("subjectID"))
        except (TypeError, ValueError) as error:
            raise RuntimeError("WB 图片 AI 返回结果缺少有效 subjectID") from error
        if returned_subject_id != int(subject_id):
            raise RuntimeError("WB 图片 AI 改动了当前类目 ID，程序已拒绝采用")
        variants = document.get("variants")
        if not isinstance(variants, list) or len(variants) != 1 or not isinstance(variants[0], dict):
            raise RuntimeError("WB 图片 AI 返回的 variants 必须只包含一个商品对象")

        current_variant = ((current_document.get("variants") or [{}])[0])
        raw_variant = variants[0]
        characteristics = []
        seen = set()
        for raw in raw_variant.get("characteristics") or []:
            if not isinstance(raw, dict):
                continue
            try:
                charc_id = int(raw.get("id"))
            except (TypeError, ValueError):
                continue
            if charc_id not in allowed:
                raise RuntimeError(f"WB 图片 AI 返回了类目外属性 ID {charc_id}，程序已拒绝采用")
            if charc_id in seen or raw.get("value") in (None, "", []):
                continue
            value = raw.get("value")
            if int(allowed[charc_id].get("charcType") or 0) == 4:
                if isinstance(value, list) and len(value) == 1:
                    value = value[0]
                if not isinstance(value, (int, float)) or isinstance(value, bool):
                    continue
            elif isinstance(value, str):
                value = [value]
            characteristics.append({"id": charc_id, "value": value})
            seen.add(charc_id)

        raw_sizes = raw_variant.get("sizes") or [{}]
        raw_size = raw_sizes[0] if isinstance(raw_sizes, list) and raw_sizes and isinstance(raw_sizes[0], dict) else {}
        current_sizes = current_variant.get("sizes") or [{}]
        current_size = current_sizes[0] if isinstance(current_sizes, list) and current_sizes else {}
        size: dict[str, Any] = {}
        current_skus = current_size.get("skus") or []
        if current_skus:
            size["skus"] = current_skus
        for key in ("techSize", "wbSize"):
            size_value = str(raw_size.get(key) or current_size.get(key) or "").strip()
            if size_value:
                size[key] = size_value[:50]

        normalized_variant: dict[str, Any] = {
            "vendorCode": str(current_variant.get("vendorCode") or "").strip(),
            "title": re.sub(r"\s+", " ", str(
                raw_variant.get("title") or current_variant.get("title") or ""
            )).strip()[:60],
            "description": str(
                raw_variant.get("description") or current_variant.get("description") or ""
            ).strip()[:5000],
            "characteristics": characteristics,
            "sizes": [size],
        }
        brand = re.sub(r"\s+", " ", str(
            raw_variant.get("brand") or current_variant.get("brand") or ""
        )).strip()[:50]
        if brand:
            normalized_variant["brand"] = brand
        return {"subjectID": int(subject_id), "variants": [normalized_variant]}


class ProductVisionAnalysisService:
    """Analyze several product images before poster generation and attribute mapping."""

    MAX_IMAGES = 6
    MIN_SAVED_CONFIDENCE = 0.65

    def __init__(self, api_url: str, api_key: str, model: str, timeout=240, session=None):
        self.api_url = normalize_chat_completions_url(api_url)
        self.api_key = str(api_key or "").strip()
        self.model = str(model or "").strip()
        self.timeout = timeout
        self.session = session or requests.Session()

    def analyze(
        self,
        reference: ReferenceProduct,
        image_paths: list[str],
        listing_content: dict[str, Any] | None = None,
    ) -> dict[str, Any]:
        if not self.api_url or not self.api_key or not self.model:
            raise ValueError(
                "请填写产品视觉分析 AI 的接口地址、密钥和模型；留空时会复用文案 AI 配置，"
                "复用的模型必须支持图片输入"
            )
        selected_paths = [str(path) for path in image_paths if str(path).strip()][:self.MAX_IMAGES]
        if not selected_paths:
            raise ValueError("视觉分析至少需要一张商品参考图")

        facts = CopywritingService._reference_facts(reference)
        prompt = (
            "Analyze the product in all supplied reference images together with the supplied Ozon page facts. "
            "Return one JSON object and no markdown with this exact shape:\n"
            '{"product_name_ru":"...",'
            '"poster_text_lines_ru":["headline","callout"],'
            '"art_direction_en":"...",'
            '"visual_facts":[{"fact_name_ru":"...","value_ru":"...",'
            '"reference_index":1,"evidence_ru":"...","confidence":0.95}]}\n'
            "poster_text_lines_ru must contain 1-4 concise, correctly spelled Russian lines: one product headline "
            "followed by at most three factual callouts. Every line must be supported by the page facts or clearly "
            "visible product evidence. art_direction_en describes only a professional 3:4 Russian marketplace poster "
            "layout, background, lighting, palette, typography zones and callout placement. It must demand one dominant "
            "product, not a collage or repeated product copies. visual_facts must describe only attributes directly "
            "visible in a numbered image, such as color, shape, pattern, visible construction or obvious product type. "
            "Each visual fact must cite exactly one 1-based reference_index and a short Russian visual observation. "
            "Never infer brand, model, dimensions, weight, capacity, material/composition, country, manufacturer, "
            "certification, warranty, package quantity, package contents, compatibility or hidden construction from "
            "appearance. Do not use filenames or hidden metadata. If uncertain, omit the fact. Use confidence from 0 to 1. "
            "Do not output Chinese text.\nOzon page facts:\n"
            + json.dumps(facts, ensure_ascii=False)
            + "\nGenerated Russian listing copy (secondary context; do not repeat any unsupported claim):\n"
            + json.dumps(listing_content or {}, ensure_ascii=False)
        )
        content: list[dict[str, Any]] = [{"type": "text", "text": prompt}]
        for index, path in enumerate(selected_paths, start=1):
            content.append({"type": "text", "text": f"REFERENCE IMAGE {index}"})
            content.append({
                "type": "image_url",
                "image_url": {"url": _vision_image_data_url(path), "detail": "high"},
            })
        response = self.session.post(self.api_url, headers={
            "Authorization": f"Bearer {self.api_key}",
            "Content-Type": "application/json",
        }, json={
            "model": self.model,
            "temperature": 0.1,
            "stream": False,
            "messages": [
                {
                    "role": "system",
                    "content": (
                        "You are a conservative product-vision analyst and Russian marketplace art director. "
                        "You distinguish visible evidence from guesses and return strict JSON."
                    ),
                },
                {"role": "user", "content": content},
            ],
        }, timeout=self.timeout)
        try:
            response.raise_for_status()
        except requests.HTTPError as error:
            raise RuntimeError(f"视觉分析 AI 返回 HTTP {response.status_code}: {response.text[:1500]}") from error
        try:
            payload = response.json()
        except (TypeError, ValueError, json.JSONDecodeError) as error:
            preview = re.sub(r"\s+", " ", str(response.text or "")).strip()[:500]
            raise RuntimeError(f"视觉分析 AI 未返回 JSON；响应预览：{preview or '空响应'}") from error
        try:
            direct_keys = {"poster_text_lines_ru", "visual_facts", "art_direction_en", "product_name_ru"}
            content_value = (
                payload if isinstance(payload, dict) and not payload.get("error") and direct_keys.intersection(payload)
                else _copywriting_content(payload)
            )
            value = _json_object_from_content(content_value)
        except RuntimeError:
            raise
        except (TypeError, ValueError, json.JSONDecodeError) as error:
            preview = re.sub(r"\s+", " ", json.dumps(payload, ensure_ascii=False)).strip()[:700]
            raise RuntimeError(f"视觉分析 AI 内容无法解析为 JSON 对象：{preview}") from error

        product_name = re.sub(r"\s+", " ", str(value.get("product_name_ru") or "")).strip()[:160]
        if _contains_cjk(product_name) or (product_name and not re.search(r"[А-Яа-яЁё]", product_name)):
            product_name = ""
        raw_lines = value.get("poster_text_lines_ru") or []
        if isinstance(raw_lines, str):
            raw_lines = [raw_lines]
        poster_lines: list[str] = []
        for raw_line in raw_lines if isinstance(raw_lines, list) else []:
            line = _short_poster_line(raw_line)
            if (
                line and re.search(r"[А-Яа-яЁё]", line)
                and line.casefold() not in {item.casefold() for item in poster_lines}
            ):
                poster_lines.append(line)
            if len(poster_lines) >= 4:
                break
        art_direction = re.sub(r"\s+", " ", str(value.get("art_direction_en") or "")).strip()[:1600]
        if _contains_cjk(art_direction):
            art_direction = ""

        visual_facts: list[dict[str, Any]] = []
        seen_facts = set()
        raw_facts = value.get("visual_facts") or []
        for raw_fact in raw_facts if isinstance(raw_facts, list) else []:
            if not isinstance(raw_fact, dict):
                continue
            try:
                reference_index = int(raw_fact.get("reference_index"))
                confidence = float(raw_fact.get("confidence"))
            except (TypeError, ValueError):
                continue
            if not 1 <= reference_index <= len(selected_paths):
                continue
            confidence = min(1.0, max(0.0, confidence))
            if confidence < self.MIN_SAVED_CONFIDENCE:
                continue
            fact_name = re.sub(r"\s+", " ", str(
                raw_fact.get("fact_name_ru") or raw_fact.get("attribute_name_ru") or ""
            )).strip()[:120]
            fact_value = re.sub(r"\s+", " ", str(
                raw_fact.get("value_ru") or raw_fact.get("value") or ""
            )).strip()[:240]
            evidence = re.sub(r"\s+", " ", str(
                raw_fact.get("evidence_ru") or raw_fact.get("evidence") or ""
            )).strip()[:300]
            if not fact_name or not fact_value or not evidence:
                continue
            if _contains_cjk(fact_name) or _contains_cjk(fact_value) or _contains_cjk(evidence):
                continue
            if not re.search(r"[А-Яа-яЁё]", fact_name) or not re.search(r"[А-Яа-яЁё]", evidence):
                continue
            key = (fact_name.casefold(), fact_value.casefold(), reference_index)
            if key in seen_facts:
                continue
            seen_facts.add(key)
            visual_facts.append({
                "fact_name_ru": fact_name,
                "value_ru": fact_value,
                "reference_index": reference_index,
                "evidence_ru": evidence,
                "confidence": round(confidence, 3),
            })
            if len(visual_facts) >= 30:
                break
        return {
            "product_name_ru": product_name,
            "poster_text_lines_ru": poster_lines,
            "art_direction_en": art_direction,
            "visual_facts": visual_facts,
            "analyzed_image_count": len(selected_paths),
        }


class CopywritingService:
    """Generate Russian listing copy from captured page facts via Chat Completions."""

    def __init__(self, api_url: str, api_key: str, model: str, timeout=180, session=None):
        self.api_url = normalize_chat_completions_url(api_url)
        self.api_key = api_key.strip()
        self.model = model.strip()
        self.timeout = timeout
        self.session = session or requests.Session()

    def _structured_json(self, system: str, prompt: str, *, temperature: float = 0.1) -> dict[str, Any]:
        if not self.api_url or not self.api_key or not self.model:
            raise ValueError("请先填写文案 API 地址、密钥和模型")
        response = self.session.post(self.api_url, headers={
            "Authorization": f"Bearer {self.api_key}",
            "Content-Type": "application/json",
        }, json={
            "model": self.model,
            "temperature": temperature,
            "stream": False,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": prompt},
            ],
        }, timeout=self.timeout)
        try:
            response.raise_for_status()
        except requests.HTTPError as error:
            raise RuntimeError(f"AI 文案接口返回 HTTP {response.status_code}: {response.text[:1500]}") from error
        try:
            payload = response.json()
        except (TypeError, ValueError, json.JSONDecodeError) as error:
            preview = re.sub(r"\s+", " ", str(response.text or "")).strip()[:500]
            raise RuntimeError(
                f"AI 文案接口未返回 JSON。实际请求地址：{self.api_url}；响应预览：{preview or '空响应'}"
            ) from error
        try:
            direct_keys = {"product_type_ru", "keywords", "description_category_id", "type_id", "attributes", "mapped_attributes", "selections"}
            content = (
                payload if isinstance(payload, dict) and not payload.get("error") and direct_keys.intersection(payload)
                else _copywriting_content(payload)
            )
            return _json_object_from_content(content)
        except RuntimeError:
            raise
        except (TypeError, ValueError, json.JSONDecodeError) as error:
            preview = re.sub(r"\s+", " ", json.dumps(payload, ensure_ascii=False)).strip()[:700]
            raise RuntimeError(f"AI 返回内容无法解析为 JSON 对象：{preview}") from error

    @staticmethod
    def _reference_facts(reference: ReferenceProduct) -> dict[str, Any]:
        return {
            "source_title": reference.title,
            "source_description": str(reference.description or "")[:16000],
            "source_properties": reference.properties,
            "source_hashtags": reference.tags,
        }

    def choose_category(
        self, reference: ReferenceProduct, categories: list[dict[str, Any]],
    ) -> tuple[dict[str, Any], list[str]]:
        """Choose one category only from live Ozon leaf/type candidates."""
        facts = self._reference_facts(reference)
        keyword_result = self._structured_json(
            "You extract Russian Ozon taxonomy keywords and return strict JSON.",
            "From these product facts, return one JSON object with product_type_ru: a list of 3-10 short "
            "Russian product type/category phrases. Use nouns describing what the product actually is, not benefits, "
            "audience, color, material, or style. Facts:\n" + json.dumps(facts, ensure_ascii=False),
        )
        raw_keywords = keyword_result.get("product_type_ru") or keyword_result.get("keywords") or []
        if isinstance(raw_keywords, str):
            raw_keywords = re.split(r"[,;\n]+", raw_keywords)
        keywords = list(dict.fromkeys(
            re.sub(r"\s+", " ", str(value)).strip()
            for value in raw_keywords if str(value).strip()
        ))[:10]
        category_hint = " ".join(filter(None, (
            reference.title, str(reference.properties.get("Ozon页面类目") or ""), " ".join(keywords),
        )))
        candidates = rank_categories(category_hint, categories, 80)
        if not candidates:
            raise RuntimeError("AI 已识别商品类型，但无法在 Ozon 实时类目树中形成可靠候选")
        candidate_payload = [{
            "description_category_id": int(item["description_category_id"]),
            "type_id": int(item["type_id"]),
            "display": item["display"],
            "display_ru": item.get("display_ru", ""),
        } for item in candidates]
        choice = self._structured_json(
            "You select one exact Ozon product category from an allowed candidate list and return strict JSON.",
            "Select the single most specific correct category/type for the supplied product. You MUST copy both IDs "
            "from one candidate; never create an ID. Return exactly one JSON object with description_category_id "
            "(integer), type_id (integer), reason_ru (short string). Product facts:\n"
            + json.dumps(facts, ensure_ascii=False)
            + "\nAllowed live candidates:\n" + json.dumps(candidate_payload, ensure_ascii=False),
        )
        try:
            key = (int(choice["description_category_id"]), int(choice["type_id"]))
        except (KeyError, TypeError, ValueError) as error:
            raise RuntimeError("AI 类目结果缺少有效的 description_category_id/type_id") from error
        allowed = {
            (int(item["description_category_id"]), int(item["type_id"])): item
            for item in candidates
        }
        if key not in allowed:
            raise RuntimeError("AI 返回了候选列表以外的类目 ID，程序已拒绝采用")
        selected = dict(allowed[key])
        selected["ai_reason"] = str(choice.get("reason_ru") or choice.get("reason") or "").strip()
        return selected, keywords

    def map_attributes(
        self,
        reference: ReferenceProduct,
        listing_content: dict[str, Any],
        definitions: list[dict[str, Any]],
        visual_analysis: dict[str, Any] | None = None,
    ) -> list[dict[str, Any]]:
        """Map page and validated visual facts without allowing AI-made IDs."""
        simplified_definitions = [{
            "id": int(item["id"]),
            "name": str(item.get("name") or ""),
            "name_ru": str(item.get("name_ru") or ""),
            "description": re.sub(r"\s+", " ", str(item.get("description") or "")).strip()[:600],
            "type": str(item.get("type") or ""),
            "required": bool(item.get("is_required")),
            "dictionary": bool(int(item.get("dictionary_id") or 0)),
            "collection": bool(item.get("is_collection")),
            "max_value_count": int(item.get("max_value_count") or 0),
        } for item in definitions]
        visual_facts = [
            item for item in (visual_analysis or {}).get("visual_facts", [])
            if isinstance(item, dict)
        ][:30]
        result = self._structured_json(
            "You map factual Ozon product data to supplied live Ozon attributes and return strict JSON.",
            "Map the source product facts to the live attribute definitions. Return one JSON object with attributes: "
            "an array of objects {id: integer, values: array of strings, evidence: short string, inferred: boolean, "
            'evidence_type: "page" or "image", visual_fact_index: integer or null}. '
            "Use only IDs from live_attributes. Attribute names may be Simplified Chinese. For dictionary attributes return "
            "short Simplified Chinese semantic search terms so the application can resolve them in a separate step through "
            "the localized Ozon dictionary. These terms are internal retrieval hints, not valid attribute values, and must "
            "never be treated as selected options. Never create dictionary IDs. For non-dictionary free-text attributes "
            "return Russian values only. "
            "Fill every required attribute when the answer "
            "is stated or strongly entailed by the source. You may infer ordinary product classification (for example use "
            "or style) when obvious, and mark inferred=true. Never invent brand, model, dimensions, weight, material, "
            "composition, country of manufacture, certification/declaration data, warranty, package quantity, or included "
            "accessories. Omit an attribute when no defensible value exists. Keep distinct values separate for collections. "
            "Do not copy the source seller's article as our model or offer ID. A visual fact may be used only when its "
            "confidence is at least 0.85. When visual evidence is used, set evidence_type=image and copy the exact 1-based "
            "visual_fact_index from validated_visual_facts. Never use images for brand, model, dimensions, weight, "
            "material/composition, country/manufacturer, certification, warranty, package quantity/contents, capacity or "
            "compatibility. Otherwise set evidence_type=page and visual_fact_index=null.\nProduct facts:\n"
            + json.dumps(self._reference_facts(reference), ensure_ascii=False)
            + "\nGenerated listing content (secondary evidence only):\n" + json.dumps(listing_content, ensure_ascii=False)
            + "\nValidated visual facts (each array position is its 1-based visual_fact_index):\n"
            + json.dumps(visual_facts, ensure_ascii=False)
            + "\nLive attributes:\n" + json.dumps(simplified_definitions, ensure_ascii=False),
        )
        allowed = {int(item["id"]): item for item in definitions}
        raw_items = result.get("attributes") or result.get("mapped_attributes") or []
        if not isinstance(raw_items, list):
            raise RuntimeError("AI 属性结果中的 attributes 不是数组")
        mapped: list[dict[str, Any]] = []
        seen = set()
        for raw in raw_items:
            if not isinstance(raw, dict):
                continue
            try:
                attribute_id = int(raw.get("id"))
            except (TypeError, ValueError):
                continue
            if attribute_id not in allowed or attribute_id in seen:
                continue
            raw_values = raw.get("values") if "values" in raw else raw.get("value")
            if isinstance(raw_values, str) or not isinstance(raw_values, list):
                raw_values = [raw_values]
            normalized_values = []
            for raw_value in raw_values:
                if isinstance(raw_value, dict):
                    raw_value = raw_value.get("value") or raw_value.get("text")
                text_value = re.sub(r"\s+", " ", str(raw_value or "")).strip()
                if text_value:
                    normalized_values.append(text_value)
            values = list(dict.fromkeys(normalized_values))
            maximum = int(allowed[attribute_id].get("max_value_count") or 0)
            if not allowed[attribute_id].get("is_collection"):
                values = values[:1]
            elif maximum:
                values = values[:maximum]
            if not values:
                continue
            evidence_type = str(raw.get("evidence_type") or "page").strip().casefold()
            visual_fact_index = None
            evidence = re.sub(r"\s+", " ", str(raw.get("evidence") or "")).strip()[:300]
            confidence = None
            reference_index = None
            if evidence_type == "image":
                try:
                    visual_fact_index = int(raw.get("visual_fact_index"))
                except (TypeError, ValueError):
                    continue
                if not 1 <= visual_fact_index <= len(visual_facts):
                    continue
                visual_fact = visual_facts[visual_fact_index - 1]
                try:
                    confidence = float(visual_fact.get("confidence"))
                    reference_index = int(visual_fact.get("reference_index"))
                except (TypeError, ValueError):
                    continue
                if confidence < 0.85 or not visual_attribute_allowed(allowed[attribute_id]):
                    continue
                observation = re.sub(
                    r"\s+", " ", str(visual_fact.get("evidence_ru") or "")
                ).strip()[:220]
                evidence = f"图片{reference_index}：{observation}"
            else:
                evidence_type = "page"
            mapped_item = {
                "id": attribute_id,
                "values": values,
                "evidence": evidence,
                "inferred": (
                    False if evidence_type == "image" else
                    raw.get("inferred") is True
                    or str(raw.get("inferred") or "").casefold() in {"true", "yes", "1"}
                ),
            }
            if evidence_type == "image":
                mapped_item.update({
                    "evidence_type": "image",
                    "visual_fact_index": visual_fact_index,
                    "confidence": confidence,
                    "reference_index": reference_index,
                })
            mapped.append(mapped_item)
            seen.add(attribute_id)
        return mapped

    def choose_dictionary_values(self, requests_: list[dict[str, Any]]) -> dict[int, list[int]]:
        """Select only IDs supplied by Ozon; any invented ID is discarded."""
        if not requests_:
            return {}
        request_by_id = {int(item["attribute_id"]): item for item in requests_}
        prepared_requests = []
        for request in requests_:
            prepared = dict(request)
            prepared["options"] = [{
                "id": int(option["id"]),
                "value": str(option.get("value") or "")[:240],
            } for option in request.get("options", []) if option.get("id")]
            prepared_requests.append(prepared)

        batches: list[list[dict[str, Any]]] = []
        current_batch: list[dict[str, Any]] = []
        current_size = 0
        maximum_batch_chars = 24000
        for request in prepared_requests:
            request_size = len(json.dumps(request, ensure_ascii=False))
            if current_batch and current_size + request_size > maximum_batch_chars:
                batches.append(current_batch)
                current_batch = []
                current_size = 0
            current_batch.append(request)
            current_size += request_size
        if current_batch:
            batches.append(current_batch)

        selections: dict[int, list[int]] = {}
        for batch in batches:
            result = self._structured_json(
                "You choose exact Ozon dictionary values only from supplied options and return strict JSON.",
                "For each request, interpret source_values and source_evidence only as product evidence. They are NOT "
                "allowed values. Choose exclusively from that request's options array and return one JSON object with "
                "selections: [{attribute_id: integer, dictionary_value_ids: array of integers}]. Every returned ID must "
                "be copied from the same request's options. Never create an option ID or return option text. Choose no "
                "ID when none of the supplied options is semantically correct. For collection=false choose at most one. "
                "Requests:\n" + json.dumps(batch, ensure_ascii=False),
            )
            for raw in result.get("selections") or []:
                if not isinstance(raw, dict):
                    continue
                try:
                    attribute_id = int(raw.get("attribute_id"))
                except (TypeError, ValueError):
                    continue
                request = request_by_id.get(attribute_id)
                if not request:
                    continue
                allowed_ids = {int(option["id"]) for option in request.get("options", [])}
                raw_ids = raw.get("dictionary_value_ids") or raw.get("ids") or []
                if not isinstance(raw_ids, list):
                    raw_ids = [raw_ids]
                chosen = []
                for raw_id in raw_ids:
                    try:
                        value_id = int(raw_id)
                    except (TypeError, ValueError):
                        continue
                    if value_id in allowed_ids and value_id not in chosen:
                        chosen.append(value_id)
                if not request.get("collection"):
                    chosen = chosen[:1]
                else:
                    maximum = int(request.get("max_value_count") or 0)
                    if maximum:
                        chosen = chosen[:maximum]
                if chosen:
                    selections[attribute_id] = chosen
        return selections

    def generate(self, reference: ReferenceProduct) -> dict[str, Any]:
        if not self.api_url or not self.api_key or not self.model:
            raise ValueError("请先填写文案 API 地址、密钥和模型")
        facts = {
            "source_title": reference.title,
            "source_description": reference.description,
            "source_properties": reference.properties,
            "source_hashtags": reference.tags,
        }
        prompt = (
            "Based only on the supplied Ozon product facts, write sale-ready Russian copy. "
            "Never invent specifications, certifications, materials, brand, warranty, package contents, "
            "compatibility, or performance. Preserve factual model/brand/numbers. Return one JSON object "
            "and no markdown with exactly: title_ru (string, clear and concise), description_ru "
            "(string, readable product description), tags_ru (array of 25-30 unique Russian search phrases). "
            "Build tags for Russian marketplace search intent: start with exact product-type queries, then "
            "common synonyms, use cases, audience/room/style, and factual material or features only when the "
            "facts support them. Use natural phrases a Russian buyer would type, not random grammatical-case "
            "variants or near-duplicates. Do not put # or underscores in tags_ru; the program formats multi-word "
            "hashtags later. Each formatted hashtag, including # and underscores, must be at most 30 characters. "
            "Never include any brand, seller, competitor, model/article, dimensions, quantities, or other product "
            "parameters in tags. Do not include competitor claims or unsupported superlatives. Facts:\n" +
            json.dumps(facts, ensure_ascii=False)
        )
        response = self.session.post(self.api_url, headers={
            "Authorization": f"Bearer {self.api_key}",
            "Content-Type": "application/json",
        }, json={
            "model": self.model,
            "temperature": 0.2,
            "stream": False,
            "messages": [
                {"role": "system", "content": "You create factual Russian marketplace product listings and return strict JSON."},
                {"role": "user", "content": prompt},
            ],
        }, timeout=self.timeout)
        try:
            response.raise_for_status()
        except requests.HTTPError as error:
            raise RuntimeError(f"文案接口返回 HTTP {response.status_code}: {response.text[:1500]}") from error
        try:
            payload = response.json()
        except (TypeError, ValueError, json.JSONDecodeError) as error:
            preview = re.sub(r"\s+", " ", str(response.text or "")).strip()[:500]
            raise RuntimeError(
                f"文案接口未返回 JSON。实际请求地址：{self.api_url}；响应预览：{preview or '空响应'}"
            ) from error
        content: Any = ""
        try:
            content = _copywriting_content(payload)
            value = _json_object_from_content(content)
            title = re.sub(r"\s+", " ", str(value.get("title_ru") or value.get("title") or "")).strip()
            description = str(value.get("description_ru") or value.get("description") or "").strip()
            raw_tags = value.get("tags_ru") or value.get("tags") or value.get("keywords") or []
            if isinstance(raw_tags, str):
                raw_tags = re.split(r"[,;\n]+", raw_tags)
            elif isinstance(raw_tags, dict):
                raw_tags = list(raw_tags.values())
            formatted_tags = format_ozon_hashtags(raw_tags)
            try:
                validated_tags = validate_ozon_hashtags(formatted_tags)
            except ValueError as error:
                raise RuntimeError(str(error)) from error
            tags = [tag[1:].replace("_", " ") for tag in validated_tags]
        except RuntimeError:
            raise
        except (KeyError, IndexError, TypeError, ValueError, json.JSONDecodeError) as error:
            preview_source = _flatten_response_text(content) or json.dumps(payload, ensure_ascii=False)
            preview = re.sub(r"\s+", " ", preview_source).strip()[:700]
            raise RuntimeError(
                f"文案接口已返回内容，但无法提取标题、描述 JSON。响应预览：{preview or '空内容'}"
            ) from error
        if not title or not description:
            raise RuntimeError("文案接口返回的标题或描述为空")
        return {"title_ru": title[:500], "description_ru": description, "tags_ru": tags}


def build_oss_object_keys(
    image_paths: list[str],
    object_prefix: str,
    offer_id: str,
    date_text: str | None = None,
) -> list[str]:
    """Build readable, retry-stable OSS keys grouped by date and Ozon Offer ID."""
    raw_offer = str(offer_id or "").strip()
    if not raw_offer:
        raise ValueError("上传 OSS 前必须填写货号（Offer ID）")
    safe_offer = re.sub(r"[^A-Za-z0-9._#-]+", "_", raw_offer).strip("._-")
    if not safe_offer:
        safe_offer = "offer"
    if safe_offer != raw_offer:
        safe_offer += "-" + hashlib.sha256(raw_offer.encode("utf-8")).hexdigest()[:8]
    upload_date = str(date_text or time.strftime("%Y-%m-%d")).strip()
    if not re.fullmatch(r"\d{4}-\d{2}-\d{2}", upload_date):
        raise ValueError("OSS 上传日期必须使用 YYYY-MM-DD 格式")
    prefix = str(object_prefix or "rfbs-listing").strip("/")
    folder = "/".join(part for part in (prefix, upload_date, safe_offer) if part)
    keys = []
    for index, raw_path in enumerate(image_paths, start=1):
        suffix = Path(raw_path).suffix.lower()
        if not re.fullmatch(r"\.[a-z0-9]{1,8}", suffix):
            suffix = ".jpg"
        filename = f"{safe_offer}主图{suffix}" if index == 1 else f"附图({index - 1}){suffix}"
        keys.append(f"{folder}/{filename}")
    return keys


class OssService:
    def __init__(self, config: dict[str, Any]):
        self.config = config

    def upload(self, image_paths: list[str], offer_id: str) -> list[str]:
        try:
            import alibabacloud_oss_v2 as oss
        except ImportError as error:
            raise RuntimeError("缺少 OSS SDK，请运行 pip install alibabacloud-oss-v2") from error
        required = ("region", "bucket", "endpoint", "access_key_id", "access_key_secret")
        missing = [key for key in required if not str(self.config.get(key, "")).strip()]
        if missing:
            raise ValueError("OSS 配置缺少：" + "、".join(missing))
        os.environ["OSS_ACCESS_KEY_ID"] = str(self.config["access_key_id"])
        os.environ["OSS_ACCESS_KEY_SECRET"] = str(self.config["access_key_secret"])
        client_config = oss.config.load_default()
        client_config.credentials_provider = oss.credentials.EnvironmentVariableCredentialsProvider()
        client_config.region = self.config["region"]
        client_config.endpoint = self.config["endpoint"]
        client = oss.Client(client_config)
        base = str(self.config.get("public_base_url") or f"https://{self.config['bucket']}.{self.config['endpoint']}").rstrip("/")
        prefix = str(self.config.get("object_prefix", "rfbs-listing")).strip("/")
        keys = build_oss_object_keys(image_paths, prefix, offer_id)
        urls = []
        for raw_path, key in zip(image_paths, keys):
            path = Path(raw_path)
            content_type = mimetypes.guess_type(path.name)[0] or "application/octet-stream"
            with path.open("rb") as body:
                client.put_object(oss.PutObjectRequest(bucket=self.config["bucket"], key=key, body=body, content_type=content_type))
            urls.append(f"{base}/{quote(key, safe='/')}")
        return urls


class ImageGenerationService:
    """Small independent adapter for an OpenAI-compatible Images Edits endpoint."""

    def __init__(self, api_url: str, api_key: str, model: str, timeout=600, session=None):
        self.api_url = normalize_image_edits_url(api_url)
        self.api_key = api_key.strip()
        self.model = model.strip()
        self.timeout = timeout
        self.session = session or requests.Session()

    def generate(self, reference_paths: list[str], prompt: str, count: int, output_dir: str) -> list[str]:
        if not self.api_url or not self.api_key or not self.model:
            raise ValueError("请先填写生图 API 地址、密钥和模型")
        if not reference_paths:
            raise ValueError("至少选择一张本地商品参考图")
        output = Path(output_dir)
        output.mkdir(parents=True, exist_ok=True)
        files = []
        handles = []
        try:
            for path in reference_paths:
                handle = Path(path).open("rb")
                handles.append(handle)
                files.append(("image", (Path(path).name, handle, mimetypes.guess_type(path)[0] or "image/jpeg")))
            response = self.session.post(self.api_url, headers={
                "Authorization": f"Bearer {self.api_key}",
                "User-Agent": USER_AGENT,
            }, files=files, data={
                "model": self.model,
                "prompt": prompt,
                "n": int(count),
                "size": "1792x2400",
                "quality": "high",
                "response_format": "b64_json",
            }, timeout=self.timeout)
            try:
                response.raise_for_status()
            except requests.HTTPError as error:
                preview = re.sub(r"\s+", " ", str(response.text or "")).strip()[:1200]
                raise RuntimeError(
                    f"生图接口返回 HTTP {response.status_code}。实际请求地址：{self.api_url}；响应：{preview or '空响应'}"
                ) from error
            try:
                payload = response.json()
            except (TypeError, ValueError, json.JSONDecodeError) as error:
                preview = re.sub(r"\s+", " ", str(response.text or "")).strip()[:800]
                raise RuntimeError(
                    f"生图接口未返回 JSON。实际请求地址：{self.api_url}；响应：{preview or '空响应'}"
                ) from error
            if not isinstance(payload, dict):
                raise RuntimeError("生图接口返回的 JSON 顶层不是对象")
            if payload.get("error"):
                api_error = payload["error"]
                if isinstance(api_error, dict):
                    api_error = api_error.get("message") or api_error.get("type") or json.dumps(api_error, ensure_ascii=False)
                raise RuntimeError("生图接口错误：" + str(api_error))
            data = payload.get("data") or payload.get("images") or []
        finally:
            for handle in handles:
                handle.close()
        results = []
        for index, item in enumerate(data, start=1):
            if item.get("b64_json"):
                content = base64.b64decode(item["b64_json"])
            elif item.get("url"):
                download = self.session.get(item["url"], headers={"User-Agent": USER_AGENT}, timeout=120)
                download.raise_for_status()
                content = download.content
            else:
                continue
            with Image.open(io.BytesIO(content)) as image:
                target = output / f"generated_{int(time.time())}_{index}.jpg"
                image.convert("RGB").save(target, "JPEG", quality=95)
                results.append(str(target))
        if not results:
            raise RuntimeError("生图接口没有返回可用图片")
        return results
