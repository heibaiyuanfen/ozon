from __future__ import annotations

import html
import difflib
import json
import os
import re
import tempfile
import time
from dataclasses import asdict, dataclass, field
from decimal import Decimal, InvalidOperation, ROUND_CEILING
from pathlib import Path
from typing import Any
from urllib.parse import urlparse

import requests

from runtime_paths import APP_DATA_DIR


USER_AGENT = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/126 Safari/537.36"
CJK_RE = re.compile(r"[\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff]")

LOGISTICS_COMMISSION_RATE = Decimal("0.02")
ADVERTISING_RATE = Decimal("0.15")
CARGO_LOSS_RATE = Decimal("0.10")


@dataclass
class ReferenceProduct:
    source_url: str
    title: str
    description: str = ""
    images: list[str] = field(default_factory=list)
    properties: dict[str, str] = field(default_factory=dict)
    tags: list[str] = field(default_factory=list)


@dataclass
class ProductInput:
    price: str
    weight: float
    depth: float
    width: float
    height: float
    category_id: int
    type_id: int
    offer_id: str
    currency_code: str = "RUB"
    weight_unit: str = "g"
    dimension_unit: str = "mm"
    vat: str = "0"
    old_price: str = "0"


@dataclass(frozen=True)
class RoiPricingBreakdown:
    price: Decimal
    shipping: Decimal
    sales_commission_rate: Decimal
    sales_commission: Decimal
    logistics_commission: Decimal
    advertising: Decimal
    cargo_loss: Decimal
    invested: Decimal
    profit: Decimal
    roi_percent: Decimal


def _pricing_decimal(value, label: str, *, allow_zero: bool = False) -> Decimal:
    try:
        number = Decimal(str(value).strip())
    except (InvalidOperation, ValueError) as error:
        raise ValueError(label + "必须是有效数字") from error
    if not number.is_finite() or number < 0 or (not allow_zero and number == 0):
        suffix = "不能小于 0" if allow_zero else "必须大于 0"
        raise ValueError(label + suffix)
    return number


def _shipping_price_bands(weight_kg: Decimal) -> list[tuple[Decimal, Decimal, Decimal]]:
    """Return (minimum price, exclusive maximum price, shipping) bands."""
    bands: list[tuple[Decimal, Decimal, Decimal]] = []
    if weight_kg < Decimal("0.5"):
        bands.append((Decimal("0"), Decimal("135"), Decimal("3.37") + weight_kg * Decimal("28.17")))
    elif weight_kg < Decimal("30"):
        bands.append((Decimal("0"), Decimal("135"), Decimal("25.83") + weight_kg * Decimal("19.17")))

    if weight_kg < Decimal("2"):
        bands.append((Decimal("135"), Decimal("635"), Decimal("17.97") + weight_kg * Decimal("28.17")))
    elif weight_kg < Decimal("30"):
        bands.append((Decimal("135"), Decimal("635"), Decimal("40.44") + weight_kg * Decimal("28.17")))

    if weight_kg < Decimal("5"):
        bands.append((Decimal("635"), Decimal("22525"), Decimal("24.17") + weight_kg * Decimal("28.17")))
    return bands


def calculate_shipping(price, weight_kg) -> Decimal:
    sale_price = _pricing_decimal(price, "售价")
    weight = _pricing_decimal(weight_kg, "实际重量")
    for minimum, maximum, shipping in _shipping_price_bands(weight):
        if minimum <= sale_price < maximum:
            return shipping
    raise ValueError("售价或实际重量超出运费公式支持区间")


def _sales_commission_rate(sales_commission_percent) -> Decimal:
    percent = _pricing_decimal(sales_commission_percent, "填写的销售佣金")
    if percent >= Decimal("100"):
        raise ValueError("填写的销售佣金必须小于 100%")
    return percent / Decimal("100")


def _sales_commission_discount_rate(discount_percent) -> Decimal:
    percent = _pricing_decimal(discount_percent, "销售佣金减免", allow_zero=True)
    if percent >= Decimal("100"):
        raise ValueError("销售佣金减免必须小于 100%")
    return percent / Decimal("100")


def _expense_rate(percent, label: str) -> Decimal:
    value = _pricing_decimal(percent, label, allow_zero=True)
    if value >= Decimal("100"):
        raise ValueError(label + "必须小于 100%")
    return value / Decimal("100")


def calculate_price_breakdown(
    price, purchase_cost, label_fee, weight_kg, sales_commission_percent,
    *, sales_commission_discount_percent=0,
    advertising_percent=ADVERTISING_RATE * Decimal("100"),
    cargo_loss_percent=CARGO_LOSS_RATE * Decimal("100"),
) -> RoiPricingBreakdown:
    sale_price = _pricing_decimal(price, "售价")
    purchase = _pricing_decimal(purchase_cost, "采购成本")
    label = _pricing_decimal(label_fee, "贴单费", allow_zero=True)
    weight = _pricing_decimal(weight_kg, "实际重量")
    sales_commission_rate = (
        _sales_commission_rate(sales_commission_percent)
        - _sales_commission_discount_rate(sales_commission_discount_percent)
    )
    if sales_commission_rate <= 0:
        raise ValueError("减免后的销售佣金必须大于 0%")
    advertising_rate = _expense_rate(advertising_percent, "广告费率")
    cargo_loss_rate = _expense_rate(cargo_loss_percent, "货损率")
    shipping = calculate_shipping(sale_price, weight)
    invested = purchase + label
    sales_commission = sale_price * sales_commission_rate
    logistics_commission = sale_price * LOGISTICS_COMMISSION_RATE
    advertising = sale_price * advertising_rate
    cargo_loss = (invested + shipping) * cargo_loss_rate
    profit = (
        sale_price - sales_commission - logistics_commission - advertising
        - invested - shipping - cargo_loss
    )
    return RoiPricingBreakdown(
        price=sale_price,
        shipping=shipping,
        sales_commission_rate=sales_commission_rate,
        sales_commission=sales_commission,
        logistics_commission=logistics_commission,
        advertising=advertising,
        cargo_loss=cargo_loss,
        invested=invested,
        profit=profit,
        roi_percent=profit / invested * Decimal("100"),
    )


def calculate_roi_price(
    purchase_cost, label_fee, target_roi_percent, weight_kg,
    *, sales_commission_percent, sales_commission_discount_percent=0,
    advertising_percent=ADVERTISING_RATE * Decimal("100"),
    cargo_loss_percent=CARGO_LOSS_RATE * Decimal("100"), minimum_sale_price=0,
) -> RoiPricingBreakdown:
    purchase = _pricing_decimal(purchase_cost, "采购成本")
    label = _pricing_decimal(label_fee, "贴单费", allow_zero=True)
    target_roi = _pricing_decimal(target_roi_percent, "目标 ROI", allow_zero=True) / Decimal("100")
    weight = _pricing_decimal(weight_kg, "实际重量")
    minimum_price = _pricing_decimal(minimum_sale_price, "最低售价", allow_zero=True)
    invested = purchase + label
    manual_commission_rate = _sales_commission_rate(sales_commission_percent)
    commission_discount_rate = _sales_commission_discount_rate(
        sales_commission_discount_percent
    )
    advertising_rate = _expense_rate(advertising_percent, "广告费率")
    cargo_loss_rate = _expense_rate(cargo_loss_percent, "货损率")

    candidates: list[RoiPricingBreakdown] = []
    for minimum, maximum, shipping in _shipping_price_bands(weight):
        sales_commission_rate = manual_commission_rate - commission_discount_rate
        if sales_commission_rate <= 0:
            raise ValueError("减免后的销售佣金必须大于 0%")
        retained_rate = Decimal("1") - sales_commission_rate - LOGISTICS_COMMISSION_RATE - advertising_rate
        if retained_rate <= 0:
            raise ValueError("平台费率合计必须小于 100%")
        required_price = (
            target_roi * invested
            + (Decimal("1") + cargo_loss_rate) * (invested + shipping)
        ) / retained_rate
        price = max(
            Decimal("1"), minimum, minimum_price,
            required_price.to_integral_value(rounding=ROUND_CEILING),
        )
        if price >= maximum:
            continue
        sales_commission = price * sales_commission_rate
        logistics_commission = price * LOGISTICS_COMMISSION_RATE
        advertising = price * advertising_rate
        cargo_loss = (invested + shipping) * cargo_loss_rate
        profit = price - sales_commission - logistics_commission - advertising - invested - shipping - cargo_loss
        roi_percent = profit / invested * Decimal("100")
        if roi_percent < target_roi * Decimal("100"):
            continue
        candidates.append(RoiPricingBreakdown(
            price=price,
            shipping=shipping,
            sales_commission_rate=sales_commission_rate,
            sales_commission=sales_commission,
            logistics_commission=logistics_commission,
            advertising=advertising,
            cargo_loss=cargo_loss,
            invested=invested,
            profit=profit,
            roi_percent=roi_percent,
        ))
    if not candidates:
        raise ValueError("当前成本、重量和目标 ROI 超出运费公式支持的售价区间")
    return min(candidates, key=lambda item: item.price)


def extract_reference_article(value: str) -> str:
    raw = str(value or "").strip()
    direct = re.fullmatch(r"\d{6,}", raw)
    if direct:
        return direct.group(0)
    labelled = re.fullmatch(r"(?:артикул|sku|商品编号)\s*[:：#]?\s*(\d{6,})", raw, flags=re.I)
    if labelled:
        return labelled.group(1)
    try:
        parsed = urlparse(raw)
    except ValueError:
        return ""
    if parsed.scheme in {"http", "https"} and parsed.hostname and parsed.hostname.casefold().endswith("ozon.ru"):
        match = re.search(r"(?:-|/)(\d{6,})/?$", parsed.path)
        return match.group(1) if match else ""
    return ""


def normalize_reference_input(value: str) -> str:
    """Accept an Ozon product URL, a numeric Артикул, or a copied Артикул label."""
    raw = str(value or "").strip()
    article = extract_reference_article(raw)
    if re.fullmatch(r"\d{6,}", raw) or re.fullmatch(r"(?:артикул|sku|商品编号)\s*[:：#]?\s*\d{6,}", raw, flags=re.I):
        return f"https://www.ozon.ru/product/{article}/"
    parsed = urlparse(raw)
    if parsed.scheme not in {"http", "https"} or not parsed.hostname or not parsed.hostname.casefold().endswith("ozon.ru"):
        raise ValueError("请输入 Ozon 商品链接或纯数字 Артикул")
    return raw


def _attach_reference_article(reference: ReferenceProduct, article: str) -> ReferenceProduct:
    if not article:
        return reference
    properties = dict(reference.properties)
    properties["Артикул"] = article
    return ReferenceProduct(
        reference.source_url, reference.title, reference.description,
        list(reference.images), properties, list(reference.tags),
    )


def atomic_write_json(path: str | Path, value: Any) -> Path:
    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix=f".{target.name}.", suffix=".tmp", dir=str(target.parent))
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as handle:
            json.dump(value, handle, ensure_ascii=False, indent=2)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, target)
    except Exception:
        try:
            os.unlink(temporary)
        except OSError:
            pass
        raise
    return target


def load_json(path: str | Path, default: Any = None) -> Any:
    try:
        return json.loads(Path(path).read_text(encoding="utf-8-sig"))
    except (OSError, ValueError, TypeError):
        return {} if default is None else default


def contains_chinese(value: Any) -> bool:
    return bool(CJK_RE.search(str(value or "")))


OZON_HASHTAG_MIN_COUNT = 25
OZON_HASHTAG_MAX_COUNT = 30
OZON_HASHTAG_MAX_LENGTH = 30
_HASHTAG_TRAILING_STOPWORDS = {
    "без", "в", "для", "до", "и", "из", "к", "на", "от", "по", "под", "с",
}


def format_ozon_hashtags(values: Any) -> str:
    """Return valid Ozon hashtags without cutting a multi-word tag in mid-word."""
    if isinstance(values, str):
        raw_values = re.split(r"[,;\n]+|\s+(?=#)", values)
    else:
        raw_values = list(values or [])
    result = []
    seen = set()
    for raw in raw_values:
        value = str(raw or "").strip().lstrip("#").casefold().replace("ё", "е")
        value = re.sub(r"[^\w]+", "_", value, flags=re.UNICODE).strip("_")
        if not value:
            continue
        parts = [part for part in value.split("_") if part]
        kept: list[str] = []
        for part in parts:
            candidate = "_".join([*kept, part])
            if len(candidate) > OZON_HASHTAG_MAX_LENGTH - 1:
                break
            kept.append(part)
        while len(kept) > 1 and kept[-1] in _HASHTAG_TRAILING_STOPWORDS:
            kept.pop()
        if not kept:
            continue
        tag = "#" + "_".join(kept)
        identity = tag.casefold()
        if len(tag) > 1 and identity not in seen:
            result.append(tag)
            seen.add(identity)
        if len(result) >= OZON_HASHTAG_MAX_COUNT:
            break
    return " ".join(result)


def parse_ozon_hashtags(value: Any) -> list[str]:
    formatted = format_ozon_hashtags(value)
    return formatted.split() if formatted else []


def validate_ozon_hashtags(
    value: Any,
    minimum: int = OZON_HASHTAG_MIN_COUNT,
    maximum: int = OZON_HASHTAG_MAX_COUNT,
) -> list[str]:
    """Normalize and validate the Ozon hashtag count, syntax, and 30-char limit."""
    tags = parse_ozon_hashtags(value)
    if len(tags) < minimum or len(tags) > maximum:
        raise ValueError(f"商品标签必须有 {minimum}–{maximum} 个；当前为 {len(tags)} 个")
    invalid = [
        tag for tag in tags
        if len(tag) > OZON_HASHTAG_MAX_LENGTH
        or not re.fullmatch(r"#[^\W_]+(?:_[^\W_]+)*", tag, flags=re.UNICODE)
    ]
    if invalid:
        raise ValueError(
            "商品标签只能包含字母、数字和连接多词的下划线，且每个最多 "
            f"{OZON_HASHTAG_MAX_LENGTH} 个字符：" + " ".join(invalid[:3])
        )
    return tags


def unsupported_chinese_submission_fields(
    title: str,
    description: str,
    tags: list[str],
    attributes: list[dict[str, Any]],
    complex_attributes: list[dict[str, Any]],
    definitions: list[dict[str, Any]],
) -> list[str]:
    """Chinese is valid for Ozon dictionary display values, not seller-entered free text."""
    invalid: list[str] = []
    if contains_chinese(title):
        invalid.append("商品标题")
    if contains_chinese(description):
        invalid.append("商品描述")
    if any(contains_chinese(tag) for tag in tags):
        invalid.append("搜索标签")
    definition_by_id = {
        int(item["id"]): item for item in definitions
        if isinstance(item, dict) and item.get("id")
    }
    all_attributes = list(attributes)
    for group in complex_attributes:
        all_attributes.extend(group.get("attributes", []))
    for item in all_attributes:
        try:
            attribute_id = int(item.get("id") or 0)
        except (TypeError, ValueError):
            attribute_id = 0
        definition = definition_by_id.get(attribute_id, {})
        has_dictionary_value_id = any(
            int(value.get("dictionary_value_id") or 0) for value in item.get("values", [])
        )
        if int(definition.get("dictionary_id") or 0) or has_dictionary_value_id:
            continue
        if any(contains_chinese(value.get("value")) for value in item.get("values", [])):
            label = str(definition.get("name") or item.get("_name") or attribute_id or "未知属性")
            invalid.append("文本属性：" + label)
    return list(dict.fromkeys(invalid))


def _json_ld(page: str):
    pattern = re.compile(r'<script[^>]+type=["\']application/ld\+json["\'][^>]*>(.*?)</script>', re.I | re.S)
    for raw in pattern.findall(page):
        try:
            values = json.loads(html.unescape(raw).strip())
        except (TypeError, ValueError):
            continue
        queue = values if isinstance(values, list) else [values]
        for value in queue:
            if isinstance(value, dict):
                yield value
                graph = value.get("@graph", [])
                if isinstance(graph, list):
                    queue.extend(graph)


def _meta(page: str, key: str) -> str:
    for pattern in (
        rf'<meta[^>]+(?:property|name)=["\']{re.escape(key)}["\'][^>]+content=["\']([^"\']+)',
        rf'<meta[^>]+content=["\']([^"\']+)["\'][^>]+(?:property|name)=["\']{re.escape(key)}["\']',
    ):
        match = re.search(pattern, page, re.I)
        if match:
            return html.unescape(match.group(1)).strip()
    return ""


def _clean_product_text(value: Any) -> str:
    text = html.unescape(str(value or ""))
    text = re.sub(r"<br\s*/?>", "\n", text, flags=re.I)
    text = re.sub(r"</(?:p|li|div|h[1-6])\s*>", "\n", text, flags=re.I)
    text = re.sub(r"<[^>]+>", " ", text)
    text = re.sub(r"[ \t\r\f\v]+", " ", text)
    return re.sub(r"\n{3,}", "\n\n", text).strip()


def extract_hashtags(*values: Any) -> list[str]:
    tags: list[str] = []
    ignored = {"#reviews", "#questions", "#description", "#characteristics", "#comments"}
    for value in values:
        if isinstance(value, (list, tuple, set)):
            text = " ".join(str(item) for item in value)
        elif isinstance(value, dict):
            text = " ".join(str(item) for item in value.values())
        else:
            text = str(value or "")
        text = html.unescape(text)
        text = re.sub(
            r"\\u([0-9a-fA-F]{4})",
            lambda match: chr(int(match.group(1), 16)),
            text,
        )
        for body in re.findall(r"(?<![\w#])#[ \t\u00a0]*([\w\-]{2,80})", text, flags=re.UNICODE):
            tag = "#" + body
            if re.fullmatch(r"#[0-9a-fA-F]{3,8}", tag) or tag.casefold() in ignored:
                continue
            tags.append(tag)
    return list(dict.fromkeys(tags))


def extract_page_hashtags(page: str) -> list[str]:
    """Extract visible and embedded product hashtags without treating CSS colors as tags."""
    raw = str(page or "")
    visible_html = re.sub(r"<(?:script|style)\b[^>]*>.*?</(?:script|style)\s*>", " ", raw, flags=re.I | re.S)
    sources: list[Any] = [_clean_product_text(visible_html)]
    json_scripts = re.findall(
        r'<script[^>]+type=["\'][^"\']*json[^"\']*["\'][^>]*>(.*?)</script>',
        raw,
        flags=re.I | re.S,
    )
    sources.extend(json_scripts)
    return extract_hashtags(*sources)


def prefer_high_resolution_image_url(value: str) -> str:
    """Upgrade Ozon multimedia thumbnail paths to the 2000px CDN variant."""
    raw = str(value or "").strip()
    if not raw:
        return ""
    try:
        parsed = urlparse(raw)
        if parsed.hostname and parsed.hostname.casefold().endswith("ozone.ru") and "/s3/multimedia" in parsed.path:
            path = re.sub(r"/wc\d+/", "/wc2000/", parsed.path, flags=re.I)
            return parsed._replace(path=path).geturl()
    except ValueError:
        pass
    return raw


def parse_reference_html(url: str, page: str) -> ReferenceProduct:
    product = next((item for item in _json_ld(page) if str(item.get("@type", "")).casefold() == "product"), {})
    title = str(product.get("name") or _meta(page, "og:title")).strip()
    description = _clean_product_text(product.get("description") or _meta(page, "og:description"))
    raw_images = product.get("image") or _meta(page, "og:image")
    images = raw_images if isinstance(raw_images, list) else [raw_images]
    images = list(dict.fromkeys(
        prefer_high_resolution_image_url(str(item)) for item in images if str(item).startswith("http")
    ))
    raw_properties = product.get("additionalProperty") or []
    if isinstance(raw_properties, dict):
        raw_properties = [raw_properties]
    properties = {}
    for item in raw_properties:
        if not isinstance(item, dict):
            continue
        name = str(item.get("name") or item.get("propertyID") or "").strip()
        value = item.get("value")
        if isinstance(value, (list, dict)):
            value = json.dumps(value, ensure_ascii=False)
        value = str(value or "").strip()
        if name and value:
            properties[name] = value
    raw_category = product.get("category")
    if isinstance(raw_category, dict):
        raw_category = raw_category.get("name") or raw_category.get("@id")
    if str(raw_category or "").strip():
        properties.setdefault("Ozon页面类目", str(raw_category).strip())
    tags = extract_hashtags(description, product.get("keywords"), properties, extract_page_hashtags(page))
    if not title:
        raise RuntimeError("商品页面没有返回可识别标题")
    return ReferenceProduct(url, title, description, images, properties, tags)


def scrape_reference_browser(url: str, *, timeout=180, log_func=None) -> ReferenceProduct:
    """Load a blocked storefront page in a visible, dedicated Edge profile."""
    try:
        from playwright.sync_api import sync_playwright
    except ImportError as error:
        raise RuntimeError("Ozon 拒绝了直接解析；请先安装浏览器解析组件：pip install playwright") from error

    profile_dir = APP_DATA_DIR / "browser_profile"
    requested_article = extract_reference_article(url)
    profile_dir.mkdir(parents=True, exist_ok=True)
    if log_func:
        log_func("Ozon 直连页面未返回完整商品图库，正在打开专用 Edge 窗口补抓。若出现验证页面，请完成验证并等待自动解析。")
    with sync_playwright() as playwright:
        last_error = None
        context = None
        for channel in ("msedge", "chrome"):
            try:
                context = playwright.chromium.launch_persistent_context(
                    str(profile_dir), channel=channel, headless=False, locale="ru-RU",
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
                # A verification page may keep loading; continue polling the DOM.
                pass
            deadline = time.monotonic() + timeout
            ready_since = None
            last_image_count = -1
            stable_rounds = 0
            content_hydration_started = False
            best_images: list[str] = []
            best_gallery_mode = ""
            best_gallery_slot_count = 0
            while time.monotonic() < deadline:
                if browser_page.is_closed():
                    raise RuntimeError("浏览器窗口已关闭，未完成 Ozon 链接解析")
                data = browser_page.evaluate(r"""
                    () => {
                      let product = {};
                      for (const script of document.querySelectorAll('script[type="application/ld+json"]')) {
                        try {
                          const raw = JSON.parse(script.textContent || '{}');
                          const queue = Array.isArray(raw) ? raw : [raw];
                          for (const item of queue) {
                            if (item && String(item['@type'] || '').toLowerCase() === 'product') product = item;
                            if (item && Array.isArray(item['@graph'])) queue.push(...item['@graph']);
                          }
                        } catch (_) {}
                      }
                      const meta = (key) => document.querySelector(`meta[property="${key}"],meta[name="${key}"]`)?.content || '';
                      const cleanText = (raw) => {
                        const node = document.createElement('div');
                        node.innerHTML = String(raw || '');
                        return String(node.innerText || node.textContent || '').trim();
                      };
                      const heading = document.querySelector('h1')?.innerText?.trim() || '';
                      const images = [];
                      const addImage = (raw) => {
                        let value = String(raw || '').trim();
                        if (value.startsWith('//')) value = `https:${value}`;
                        if (!/^https:\/\//.test(value) || !/ozone\.ru/i.test(value)) return;
                        if (/abt-challenge|captcha|\/icons?\//i.test(value)) return;
                        try {
                          const parsed = new URL(value);
                          if (/\/s3\/multimedia/i.test(parsed.pathname)) {
                            parsed.pathname = parsed.pathname.replace(/\/wc\d+\//i, '/wc2000/');
                            value = parsed.toString();
                          }
                        } catch (_) {}
                        images.push(value);
                      };
                      const imageSource = (image) => {
                        const candidates = String(image.getAttribute('srcset') || '').split(',')
                          .map(x => x.trim().split(/\s+/)[0]).filter(Boolean);
                        return candidates.at(-1) || image.currentSrc || image.src || image.getAttribute('data-src') || '';
                      };
                      const gallerySelectors = [
                        '[data-widget="webGallery"] img', '[data-widget*="Gallery"] img',
                        '[data-widget*="gallery"] img'
                      ];
                      const galleryElements = new Set();
                      for (const selector of gallerySelectors) {
                        for (const image of document.querySelectorAll(selector)) galleryElements.add(image);
                      }
                      const galleryRecords = [...galleryElements].map((image, order) => {
                        const rect = image.getBoundingClientRect();
                        return {
                          source: imageSource(image), order,
                          x: rect.x, y: rect.y, width: rect.width, height: rect.height,
                        };
                      }).filter(item => item.source);
                      // Ozon renders the selected photo twice: once in the fixed
                      // thumbnail rail and once as the large central preview.
                      // Select the small images positioned to the left of the largest
                      // gallery image, then sort by their screen coordinates.  That
                      // fixed rail is the authoritative ordered list of image slots.
                      const visibleGallery = galleryRecords.filter(item => item.width > 0 && item.height > 0);
                      const preview = visibleGallery.reduce((largest, item) =>
                        !largest || item.width * item.height > largest.width * largest.height ? item : largest
                      , null);
                      const smallImages = visibleGallery.filter(item => Math.max(item.width, item.height) <= 180);
                      const leftRail = preview ? smallImages.filter(item =>
                        item.x + item.width <= preview.x + Math.max(24, preview.width * 0.12)
                      ) : [];
                      const thumbnailSlots = leftRail.length ? leftRail : smallImages;
                      const positionedGallery = thumbnailSlots.length
                        ? thumbnailSlots.sort((a, b) =>
                            Math.abs(a.y - b.y) > 2 ? a.y - b.y : (a.x - b.x || a.order - b.order)
                          )
                        : galleryRecords.sort((a, b) => a.order - b.order);
                      const galleryMode = leftRail.length
                        ? 'fixed-left-thumbnail-rail'
                        : (smallImages.length ? 'thumbnail-size-fallback' : 'gallery-dom-fallback');
                      for (const item of positionedGallery) addImage(item.source);
                      if (!images.length) {
                        const rawImages = product.image || [];
                        for (const value of (Array.isArray(rawImages) ? rawImages : [rawImages])) addImage(value);
                        addImage(meta('og:image'));
                      }
                      if (images.length < 2) {
                        for (const image of document.querySelectorAll('img[src],img[srcset],img[data-src]')) {
                          const alt = String(image.alt || '').trim();
                          if (alt && (alt === heading || heading.includes(alt) || alt.includes(heading))) addImage(imageSource(image));
                        }
                      }
                      const uniqueImages = [];
                      const seenImages = new Set();
                      for (const value of images) {
                        let key = value;
                        try {
                          const parsed = new URL(value);
                          const media = parsed.pathname.match(/\/s3\/multimedia[^/]*\/wc\d+\/([^/]+)$/i);
                          key = media ? media[1] : parsed.origin + parsed.pathname;
                        } catch (_) {}
                        if (!seenImages.has(key)) { seenImages.add(key); uniqueImages.push(value); }
                      }
                      const properties = {};
                      const additional = product.additionalProperty || [];
                      for (const item of (Array.isArray(additional) ? additional : [additional])) {
                        const key = String(item?.name || item?.propertyID || '').trim();
                        const value = String(item?.value || '').trim();
                        if (key && value) properties[key] = value;
                      }
                      const productCategory = typeof product.category === 'object'
                        ? String(product.category?.name || product.category?.['@id'] || '').trim()
                        : String(product.category || '').trim();
                      if (productCategory) properties['Ozon页面类目'] = productCategory;
                      const descriptionText = document.querySelector('[data-widget="webDescription"]')?.innerText?.trim() || '';
                      const characteristicsText = document.querySelector('[data-widget="webCharacteristics"]')?.innerText?.trim() || '';
                      if (characteristicsText) properties['页面商品参数'] = characteristicsText;
                      const structuredDescription = cleanText(product.description || '');
                      const pageDescription = cleanText(descriptionText);
                      const description = pageDescription.length >= structuredDescription.length ? pageDescription : structuredDescription;
                      const decodeEscapes = (raw) => String(raw || '').replace(/\\u([0-9a-fA-F]{4})/g,
                        (_, code) => String.fromCharCode(parseInt(code, 16))
                      );
                      const tagSource = [structuredDescription, pageDescription, meta('keywords')];
                      const keywords = product.keywords || [];
                      tagSource.push(Array.isArray(keywords) ? keywords.join(' ') : String(keywords));
                      const tagWidgetNames = [];
                      for (const widget of document.querySelectorAll('[data-widget]')) {
                        const widgetName = String(widget.getAttribute('data-widget') || '');
                        if (!/(description|tag|keyword|seo)/i.test(widgetName)) continue;
                        const text = String(widget.innerText || widget.textContent || '').trim();
                        if (text.includes('#')) {
                          tagWidgetNames.push(widgetName);
                          tagSource.push(text);
                        }
                      }
                      // Hashtag chips are sometimes rendered outside webDescription.
                      // Read only short, product-page text nodes so reviews and the
                      // rest of the document are not copied as description content.
                      const productRoot = document.querySelector('main') || document.body;
                      for (const node of productRoot.querySelectorAll('a, span, p, li')) {
                        const text = String(node.innerText || node.textContent || '').trim();
                        if (text.includes('#') && text.length <= 400) tagSource.push(text);
                      }
                      const article = (location.pathname.match(/(\d{6,})(?:\/?$)/) || [])[1] || '';
                      for (const script of document.querySelectorAll('script[type*="json"]')) {
                        const raw = String(script.textContent || '');
                        if (!raw.includes('#') || raw.length > 8000000) continue;
                        if (!article || raw.includes(article) || (heading && raw.includes(heading.slice(0, 40)))) {
                          tagSource.push(raw);
                        }
                      }
                      const tags = [];
                      const seenTags = new Set();
                      const ignoredTags = new Set(['#reviews', '#questions', '#description', '#characteristics', '#comments']);
                      for (const source of tagSource) {
                        for (const match of decodeEscapes(source).matchAll(/#[ \t\u00a0]*([\p{L}\p{N}_-]{2,80})/gu)) {
                          const tag = `#${match[1]}`;
                          if (/^#[0-9a-fA-F]{3,8}$/.test(tag) || ignoredTags.has(tag.toLowerCase())) continue;
                          if (!seenTags.has(tag)) { seenTags.add(tag); tags.push(tag); }
                        }
                      }
                      return {
                        title: String(product.name || meta('og:title') || heading || '').trim(),
                        description: description || cleanText(meta('og:description') || ''),
                        images: uniqueImages.slice(0, 20),
                        properties,
                        tags,
                        tagWidgetNames: [...new Set(tagWidgetNames)],
                        galleryMode,
                        gallerySlotCount: positionedGallery.length,
                        pageTitle: document.title || ''
                      };
                    }
                """)
                title = str(data.get("title") or "").strip()
                page_title = str(data.get("pageTitle") or "").casefold()
                challenge = any(text in page_title for text in ("похоже", "доступ ограничен", "captcha", "нет соединения"))
                if title and not challenge and title.casefold() not in {"ozon", "озон"}:
                    current_images = [str(item) for item in data.get("images", []) if str(item).startswith("https://")]
                    if len(current_images) > len(best_images):
                        best_images = current_images
                        best_gallery_mode = str(data.get("galleryMode") or "")
                        best_gallery_slot_count = int(data.get("gallerySlotCount") or len(current_images))
                    if not content_hydration_started:
                        if not best_images:
                            # Keep the gallery viewport visible until at least
                            # one real product image has hydrated.
                            browser_page.evaluate("() => window.scrollTo(0, 0)")
                            browser_page.wait_for_timeout(1200)
                            continue
                        browser_page.evaluate(r"""
                            () => {
                              const description = document.querySelector('[data-widget="webDescription"]');
                              if (description) description.scrollIntoView({block: 'center'});
                              else window.scrollTo(0, Math.min(document.body.scrollHeight * 0.55, 3500));
                            }
                        """)
                        content_hydration_started = True
                        browser_page.wait_for_timeout(1200)
                        continue
                    image_count = len(best_images)
                    now = time.monotonic()
                    if ready_since is None:
                        ready_since = now
                        if log_func:
                            log_func("商品页面已打开，正在等待高清图库完整加载……")
                    if image_count == last_image_count:
                        stable_rounds += 1
                    else:
                        stable_rounds = 0
                        last_image_count = image_count
                    # A title alone is not a complete parse. Ozon sometimes
                    # returns the product text before hydrating the gallery;
                    # never return an empty image list to the listing pipeline.
                    if image_count > 0 and now - ready_since >= 4 and (stable_rounds >= 2 or now - ready_since >= 8):
                        actual_article = extract_reference_article(browser_page.url)
                        if requested_article and actual_article and actual_article != requested_article:
                            raise RuntimeError(
                                f"Ozon 跳转到了另一个变体 Артикул {actual_article}，目标是 {requested_article}；已停止解析"
                            )
                        images = list(dict.fromkeys(
                            prefer_high_resolution_image_url(str(item))
                            for item in best_images if str(item).startswith("https://")
                        ))
                        properties = {str(key): str(value) for key, value in (data.get("properties") or {}).items() if str(value).strip()}
                        tags = list(dict.fromkeys(str(item).strip() for item in data.get("tags", []) if str(item).startswith("#")))
                        if log_func:
                            mode = best_gallery_mode
                            slot_count = best_gallery_slot_count or len(images)
                            if mode == "fixed-left-thumbnail-rail":
                                log_func(f"已按页面固定缩略图位置读取 {slot_count} 个图库槽位，并排除中央预览图")
                            else:
                                log_func(f"页面固定缩略图轨道未完整识别，已用图库结构读取 {slot_count} 个槽位")
                            if tags:
                                log_func("已读取页面 #标签：" + ", ".join(tags))
                            else:
                                widgets = ", ".join(str(item) for item in data.get("tagWidgetNames", []) if str(item).strip())
                                detail = f"；检查到组件：{widgets}" if widgets else ""
                                log_func("未在商品描述、标签组件或页面商品数据中发现 #标签" + detail)
                        reference = ReferenceProduct(url, title, str(data.get("description") or "").strip(), images, properties, tags)
                        return _attach_reference_article(reference, requested_article)
                browser_page.wait_for_timeout(1000)
            raise RuntimeError("等待 Ozon 页面或人工验证超时；请确认浏览器中能正常打开该商品")
        finally:
            context.close()


def scrape_reference(url: str, *, session=None, timeout=30, browser_fallback=True, log_func=None) -> ReferenceProduct:
    normalized_url = normalize_reference_input(url)
    requested_article = extract_reference_article(normalized_url)
    client = session or requests.Session()
    response = client.get(normalized_url, headers={"User-Agent": USER_AGENT, "Accept-Language": "ru-RU,ru;q=0.9"}, timeout=timeout)
    if response.status_code == 403 and browser_fallback:
        return scrape_reference_browser(normalized_url, timeout=max(120, timeout), log_func=log_func)
    response.raise_for_status()
    try:
        final_url = str(getattr(response, "url", normalized_url) or normalized_url)
        actual_article = extract_reference_article(final_url)
        if requested_article and actual_article and actual_article != requested_article:
            raise RuntimeError(
                f"Ozon 跳转到了另一个变体 Артикул {actual_article}，目标是 {requested_article}；已停止解析"
            )
        reference = parse_reference_html(normalized_url, response.text)
        if browser_fallback and not reference.images:
            if log_func:
                log_func("Ozon 直连响应已读取到标题，但图库为空；切换到浏览器重新加载商品图库")
            return scrape_reference_browser(normalized_url, timeout=max(120, timeout), log_func=log_func)
        return _attach_reference_article(reference, requested_article)
    except RuntimeError:
        if browser_fallback:
            return scrape_reference_browser(normalized_url, timeout=max(120, timeout), log_func=log_func)
        raise


def flatten_categories(tree: list[dict[str, Any]]) -> list[dict[str, Any]]:
    result: list[dict[str, Any]] = []

    def visit(node: dict[str, Any], path: list[str], inherited_category_id=None):
        if node.get("disabled") or node.get("is_disabled"):
            return
        category_id = node.get("description_category_id") or node.get("category_id") or inherited_category_id
        category_name = str(node.get("category_name") or "").strip()
        type_id = node.get("type_id")
        type_name = str(node.get("type_name") or "").strip()
        current = path + ([category_name] if category_name else [])
        if category_id and type_id:
            display_path = current + ([type_name] if type_name and type_name != category_name else [])
            result.append({
                "description_category_id": int(category_id),
                "type_id": int(type_id),
                "name": type_name or category_name,
                "path": display_path,
                "display": " / ".join(display_path),
            })
        for legacy in node.get("type", node.get("types", [])) or []:
            if legacy.get("disabled") or legacy.get("is_disabled"):
                continue
            legacy_id = legacy.get("id") or legacy.get("type_id")
            legacy_name = str(legacy.get("name") or legacy.get("type_name") or "").strip()
            if category_id and legacy_id:
                display_path = current + ([legacy_name] if legacy_name else [])
                result.append({
                    "description_category_id": int(category_id), "type_id": int(legacy_id),
                    "name": legacy_name, "path": display_path, "display": " / ".join(display_path),
                })
        for child in node.get("children", []) or []:
            if isinstance(child, dict):
                visit(child, current, category_id)

    for root in tree or []:
        if isinstance(root, dict):
            visit(root, [])
    unique = {(item["description_category_id"], item["type_id"]): item for item in result}
    return sorted(unique.values(), key=lambda item: item["display"].casefold())


def rank_categories(title: str, categories: list[dict[str, Any]], limit: int = 60) -> list[dict[str, Any]]:
    ignored = {"товар", "товары", "новый", "новая", "новое", "для", "with"}
    words = set(re.findall(r"[a-zа-яё0-9]{4,}", title.casefold())) - ignored
    if not words:
        return []

    def stem(word: str) -> str:
        value = word.replace("ё", "е")
        for suffix in ("иями", "ями", "ами", "ого", "ему", "ому", "ыми", "ими", "ов", "ев", "ей", "ой", "ый", "ий", "ая", "яя", "ое", "ее", "ы", "и", "а", "я"):
            if value.endswith(suffix) and len(value) - len(suffix) >= 5:
                return value[:-len(suffix)]
        return value

    ranked: list[tuple[float, dict[str, Any]]] = []
    for category in categories:
        searchable_display = " ".join(filter(None, (
            str(category.get("display") or ""), str(category.get("display_ru") or ""),
        )))
        category_words = set(re.findall(r"[a-zа-яё0-9]{4,}", searchable_display.casefold())) - ignored
        score: float = sum(len(word) ** 2 for word in words & category_words)
        stems = {stem(word) for word in words}
        category_stems = {stem(word) for word in category_words}
        score += sum(len(word) ** 2 for word in stems & category_stems)
        # Catch close transliterations and inflections while keeping a fairly
        # high threshold so unrelated catalogue branches are not promoted.
        for source_word in words:
            for category_word in category_words:
                ratio = difflib.SequenceMatcher(None, stem(source_word), stem(category_word)).ratio()
                if ratio >= 0.72:
                    score += ratio * min(len(source_word), len(category_word))
        if score:
            ranked.append((score, category))
    ranked.sort(key=lambda item: (-item[0], item[1]["display"].casefold()))
    return [category for _, category in ranked[:max(1, int(limit))]]


def recommend_category(title: str, categories: list[dict[str, Any]]) -> dict[str, Any] | None:
    ranked = rank_categories(title, categories, 1)
    return ranked[0] if ranked else None


def normalize_attribute_name(value: Any) -> str:
    """Normalize Russian attribute/fact names for conservative exact matching."""
    text = html.unescape(str(value or "")).casefold().replace("ё", "е")
    text = re.sub(r"\([^)]*\)", " ", text)
    text = re.sub(r"[^a-zа-я0-9\u3400-\u4dbf\u4e00-\u9fff]+", " ", text)
    words = [word for word in text.split() if word not in {
        "товар", "товара", "товары", "изделие", "изделия", "продукт", "продукта",
    }]
    return " ".join(words).strip()


def reference_fact_map(reference: ReferenceProduct) -> dict[str, tuple[str, str]]:
    """Return only traceable page facts; later duplicates keep the first source."""
    facts: dict[str, tuple[str, str]] = {}
    raw_characteristics = ""
    for raw_name, raw_value in (reference.properties or {}).items():
        name = str(raw_name or "").strip()
        value = str(raw_value or "").strip()
        if not name or not value:
            continue
        if name == "页面商品参数":
            raw_characteristics = value
            continue
        normalized = normalize_attribute_name(name)
        if normalized:
            facts.setdefault(normalized, (name, value))

    # Ozon's characteristics widget commonly renders label/value pairs on
    # adjacent lines. Only accept a pair when the label is short and distinct;
    # this is intentionally conservative so section headings are not invented
    # as values.
    lines = [re.sub(r"\s+", " ", line).strip() for line in raw_characteristics.splitlines()]
    lines = [line for line in lines if line]
    for index in range(len(lines) - 1):
        name, value = lines[index], lines[index + 1]
        normalized = normalize_attribute_name(name)
        if not normalized or len(name) > 120 or len(value) > 1000:
            continue
        if normalize_attribute_name(value) == normalized:
            continue
        facts.setdefault(normalized, (name, value))
    return facts


def find_reference_fact(attribute_name: str, reference: ReferenceProduct) -> tuple[str, str] | None:
    """Find an exact/canonical page fact without semantic guessing."""
    wanted = normalize_attribute_name(attribute_name)
    if not wanted:
        return None
    return reference_fact_map(reference).get(wanted)


def attribute_values_by_id(
    attributes: list[dict[str, Any]],
    complex_attributes: list[dict[str, Any]] | None = None,
) -> dict[int, dict[str, Any]]:
    result: dict[int, dict[str, Any]] = {}
    for item in attributes:
        if item.get("id"):
            result[int(item["id"])] = item
    for group in complex_attributes or []:
        for item in group.get("attributes", []):
            if item.get("id"):
                result[int(item["id"])] = item
    return result


def set_attribute_value(
    attributes: list[dict[str, Any]],
    complex_attributes: list[dict[str, Any]],
    definition: dict[str, Any],
    *,
    value: str,
    dictionary_value_id: int = 0,
    source: str = "manual",
    append: bool = False,
) -> dict[str, Any]:
    """Create or update an Ozon attribute while preserving its live grouping."""
    attribute_id = int(definition["id"])
    complex_id = int(definition.get("attribute_complex_id") or 0)
    existing = attribute_values_by_id(attributes, complex_attributes).get(attribute_id)
    if existing is None:
        existing = {
            "complex_id": complex_id,
            "id": attribute_id,
            "values": [],
            "_name": str(definition.get("name") or attribute_id),
        }
        if complex_id:
            group = next((
                item for item in complex_attributes
                if int(item.get("_complex_id") or 0) == complex_id
                or any(int(attribute.get("complex_id") or 0) == complex_id for attribute in item.get("attributes", []))
            ), None)
            if group is None:
                group = {"attributes": [], "_complex_id": complex_id}
                complex_attributes.append(group)
            group["attributes"].append(existing)
        else:
            attributes.append(existing)
    payload = {"value": str(value).strip()}
    if dictionary_value_id:
        payload["dictionary_value_id"] = int(dictionary_value_id)
    if append and definition.get("is_collection"):
        values = existing.setdefault("values", [])
        identity = (payload.get("dictionary_value_id", 0), normalize_attribute_name(payload.get("value")))
        present = {
            (item.get("dictionary_value_id", 0), normalize_attribute_name(item.get("value")))
            for item in values
        }
        if identity not in present:
            maximum = int(definition.get("max_value_count") or 0)
            if maximum and len(values) >= maximum:
                raise ValueError(f"属性 {definition.get('name') or attribute_id} 最多允许 {maximum} 个值")
            values.append(payload)
    else:
        existing["values"] = [payload]
    existing["_source"] = source
    return existing


def clean_attribute_payload(items: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Remove UI-only metadata before sending attributes to Ozon."""
    cleaned: list[dict[str, Any]] = []
    for item in items:
        if "attributes" in item:
            cleaned.append({
                key: value for key, value in item.items()
                if not str(key).startswith("_") and key != "attributes"
            } | {"attributes": clean_attribute_payload(item.get("attributes", []))})
        else:
            cleaned.append({key: value for key, value in item.items() if not str(key).startswith("_")})
    return cleaned


def parse_attributes(raw: str) -> list[dict[str, Any]]:
    value = json.loads(raw or "[]")
    if not isinstance(value, list):
        raise ValueError("属性 JSON 必须是数组")
    for item in value:
        if not isinstance(item, dict) or not item.get("id") or not isinstance(item.get("values"), list):
            raise ValueError("每个属性必须包含 id 和 values 数组")
    return value


def parse_complex_attributes(raw: str) -> list[dict[str, Any]]:
    value = json.loads(raw or "[]")
    if not isinstance(value, list):
        raise ValueError("组合属性 JSON 必须是数组")
    for group in value:
        if not isinstance(group, dict) or not isinstance(group.get("attributes"), list):
            raise ValueError("每组组合属性必须包含 attributes 数组")
        for item in group["attributes"]:
            if not isinstance(item, dict) or not item.get("id") or not isinstance(item.get("values"), list):
                raise ValueError("组合属性项必须包含 id 和 values 数组")
    return value


def validate_required(attributes: list[dict[str, Any]], definitions: list[dict[str, Any]], complex_attributes: list[dict[str, Any]] | None = None) -> list[str]:
    all_attributes = list(attributes)
    for group in complex_attributes or []:
        all_attributes.extend(group.get("attributes", []))
    present = {int(item.get("id", 0)) for item in all_attributes if item.get("values") and any(v.get("value") or v.get("dictionary_value_id") for v in item["values"])}
    return [str(item.get("name") or item.get("id")) for item in definitions if item.get("is_required") and int(item.get("id", 0)) not in present]


def build_import_item(product: ProductInput, reference: ReferenceProduct, image_urls: list[str], attributes: list[dict[str, Any]], complex_attributes: list[dict[str, Any]] | None = None) -> dict[str, Any]:
    if not image_urls or any(not url.startswith("https://") for url in image_urls):
        raise ValueError("至少需要一张可公开访问的 HTTPS 商品图片")
    if not product.offer_id.strip():
        raise ValueError("offer_id 不能为空")
    price_text = str(product.price).strip()
    old_price_text = str(product.old_price or "0").strip() or "0"
    try:
        price_number = Decimal(price_text)
        old_price_number = Decimal(old_price_text)
    except (InvalidOperation, ValueError) as error:
        raise ValueError("售价和折扣前价格必须是有效数字") from error
    if price_number <= 0:
        raise ValueError("售价必须大于 0")
    if old_price_number < 0:
        raise ValueError("折扣前价格不能小于 0")
    if old_price_number and old_price_number <= price_number:
        raise ValueError("折扣前价格必须大于当前售价；不设置折扣时填写 0")
    return {
        "attributes": attributes,
        "barcode": "",
        "description_category_id": int(product.category_id),
        "new_description_category_id": 0,
        "color_image": "",
        "complex_attributes": complex_attributes or [],
        "currency_code": product.currency_code,
        "depth": product.depth,
        "dimension_unit": product.dimension_unit,
        "height": product.height,
        "images": image_urls[1:15],
        "name": re.sub(r"\s+", " ", reference.title).strip()[:500],
        "offer_id": product.offer_id.strip(),
        "old_price": old_price_text,
        "pdf_list": [],
        "price": price_text,
        "primary_image": image_urls[0],
        "type_id": int(product.type_id),
        "vat": str(product.vat),
        "weight": product.weight,
        "weight_unit": product.weight_unit,
        "width": product.width,
    }


def export_reference(reference: ReferenceProduct) -> dict[str, Any]:
    return asdict(reference)
