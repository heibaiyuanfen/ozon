from __future__ import annotations

import json
import mimetypes
import time
from decimal import Decimal, InvalidOperation
from pathlib import Path
from typing import Any, Callable

import requests
from PIL import Image


class WbApiError(RuntimeError):
    def __init__(self, status_code: int, endpoint: str, detail: str):
        self.status_code = int(status_code)
        self.endpoint = str(endpoint)
        self.detail = str(detail)
        super().__init__(f"WB API {self.endpoint} 返回 HTTP {self.status_code}: {self.detail}")


class WildberriesApi:
    CONTENT_URL = "https://content-api.wildberries.ru"
    CONTENT_SANDBOX_URL = "https://content-api-sandbox.wildberries.ru"
    PRICES_URL = "https://discounts-prices-api.wildberries.ru"
    PRICES_SANDBOX_URL = "https://discounts-prices-api-sandbox.wildberries.ru"
    MARKETPLACE_URL = "https://marketplace-api.wildberries.ru"
    COMMON_URL = "https://common-api.wildberries.ru"

    def __init__(self, token: str, *, sandbox: bool = False, timeout=60, session=None):
        normalized_token = str(token or "").strip()
        if normalized_token.casefold().startswith("bearer "):
            normalized_token = normalized_token[7:].strip()
        if not normalized_token:
            raise ValueError("请填写 WB API Token")
        self.token = normalized_token
        self.sandbox = bool(sandbox)
        self.timeout = (20, max(45, int(timeout)))
        self.session = session or requests.Session()
        self.content_url = self.CONTENT_SANDBOX_URL if self.sandbox else self.CONTENT_URL
        self.prices_url = self.PRICES_SANDBOX_URL if self.sandbox else self.PRICES_URL

    @property
    def authorization_header(self) -> str:
        return "Bearer " + self.token

    @staticmethod
    def _error_detail(payload, fallback: str) -> str:
        if isinstance(payload, dict):
            parts = []
            for key in (
                "title", "detail", "message", "errorText", "additionalErrors", "code",
            ):
                value = payload.get(key)
                if value not in (None, "", [], {}):
                    if isinstance(value, (dict, list)):
                        value = json.dumps(value, ensure_ascii=False)
                    parts.append(str(value))
            if parts:
                return " | ".join(parts)
        return fallback or "未提供错误详情"

    def _request(
        self, method: str, url: str, *, params=None, json_body=None, files=None,
        expected=(200,), allow_statuses=(), extra_headers=None,
    ) -> dict[str, Any] | list[Any]:
        headers = {
            "Authorization": self.authorization_header,
            "Accept": "application/json",
        }
        if files is None:
            headers["Content-Type"] = "application/json"
        headers.update(extra_headers or {})
        try:
            response = self.session.request(
                method, url, headers=headers, params=params, json=json_body,
                files=files, timeout=self.timeout,
            )
        except requests.exceptions.ConnectTimeout as error:
            raise RuntimeError("连接 WB API 超时，请检查网络、代理或防火墙") from error
        except requests.exceptions.ReadTimeout as error:
            raise RuntimeError(
                "WB API 已连接但等待响应超时；若正在创建商品，请先按货号查询结果，避免重复提交"
            ) from error
        except requests.exceptions.ConnectionError as error:
            raise RuntimeError("无法连接 WB API，请检查网络、DNS、代理或防火墙") from error

        payload: Any = {}
        if getattr(response, "content", b"") or getattr(response, "text", ""):
            try:
                payload = response.json()
            except (ValueError, json.JSONDecodeError):
                payload = {}
        if response.status_code in allow_statuses:
            if isinstance(payload, dict):
                return {**payload, "_http_status": response.status_code}
            return {"_http_status": response.status_code, "_body": payload}
        if response.status_code not in set(expected):
            detail = self._error_detail(payload, str(getattr(response, "text", ""))[:3000])
            raise WbApiError(response.status_code, url, detail)
        if isinstance(payload, dict) and payload.get("error") is True:
            detail = self._error_detail(payload, "WB API 返回业务错误")
            raise WbApiError(response.status_code, url, detail)
        return payload

    def ping(self) -> dict[str, dict]:
        hosts = {
            "Content": self.content_url,
            "Prices and Discounts": self.prices_url,
        }
        if not self.sandbox:
            hosts["Marketplace"] = self.MARKETPLACE_URL
        return {
            name: self._request("GET", host + "/ping")
            for name, host in hosts.items()
        }

    def seller_info(self) -> dict[str, Any]:
        if self.sandbox:
            return {"name": "WB Sandbox", "sid": "sandbox"}
        data = self._request("GET", self.COMMON_URL + "/api/v1/seller-info")
        return data if isinstance(data, dict) else {}

    def parent_categories(self, locale="zh") -> list[dict[str, Any]]:
        data = self._request(
            "GET", self.content_url + "/content/v2/object/parent/all",
            params={"locale": locale},
        )
        return [item for item in (data.get("data") or []) if isinstance(item, dict)]

    def subjects(
        self, *, parent_id: int | None = None, name="", locale="zh",
    ) -> list[dict[str, Any]]:
        results: list[dict[str, Any]] = []
        offset = 0
        while True:
            params: dict[str, Any] = {"locale": locale, "limit": 1000, "offset": offset}
            if parent_id:
                params["parentID"] = int(parent_id)
            if str(name or "").strip():
                params["name"] = str(name).strip()
            data = self._request(
                "GET", self.content_url + "/content/v2/object/all", params=params,
            )
            page = [item for item in (data.get("data") or []) if isinstance(item, dict)]
            results.extend(page)
            if len(page) < 1000:
                return results
            offset += 1000
            if offset >= 100000:
                raise RuntimeError("WB 子类目超过 100000 条，已停止以避免无限分页")

    def subject_characteristics(self, subject_id: int, locale="zh") -> list[dict[str, Any]]:
        data = self._request(
            "GET", self.content_url + f"/content/v2/object/charcs/{int(subject_id)}",
            params={"locale": locale},
        )
        return [item for item in (data.get("data") or []) if isinstance(item, dict)]

    def directory_values(self, kind: str, *, subject_id: int | None = None, locale="zh") -> list[Any]:
        paths = {
            "colors": "/content/v2/directory/colors",
            "kinds": "/content/v2/directory/kinds",
            "countries": "/content/v2/directory/countries",
            "seasons": "/content/v2/directory/seasons",
            "vat": "/content/v2/directory/vat",
            "tnved": "/content/v2/directory/tnved",
        }
        if kind not in paths:
            raise ValueError("该 WB 属性没有官方字典接口，请按属性要求手工填写")
        params: dict[str, Any] = {"locale": locale}
        if kind == "tnved":
            if not subject_id:
                raise ValueError("加载海关编码前请先选择 WB 子类目")
            params["subjectID"] = int(subject_id)
        data = self._request("GET", self.content_url + paths[kind], params=params)
        return list(data.get("data") or [])

    def generate_barcodes(self, count=1) -> list[str]:
        data = self._request(
            "POST", self.content_url + "/content/v2/barcodes",
            json_body={"count": int(count)},
        )
        return [str(value) for value in (data.get("data") or []) if str(value).strip()]

    def create_cards(self, payload: list[dict[str, Any]]) -> dict[str, Any]:
        data = self._request(
            "POST", self.content_url + "/content/v2/cards/upload",
            json_body=payload,
        )
        return data if isinstance(data, dict) else {}

    def cards_by_text(self, text: str) -> list[dict[str, Any]]:
        data = self._request(
            "POST", self.content_url + "/content/v2/get/cards/list",
            params={"locale": "zh"},
            json_body={
                "settings": {
                    "sort": {"ascending": False},
                    "filter": {
                        "textSearch": str(text), "allowedCategoriesOnly": False,
                        "withPhoto": -1,
                    },
                    "cursor": {"limit": 100},
                },
            },
        )
        return [item for item in (data.get("cards") or []) if isinstance(item, dict)]

    def find_card(self, vendor_code: str) -> dict[str, Any] | None:
        normalized = str(vendor_code or "").strip()
        return next((
            card for card in self.cards_by_text(normalized)
            if str(card.get("vendorCode") or "").strip() == normalized
        ), None)

    def failed_card_errors(self, vendor_code: str) -> list[str]:
        data = self._request(
            "POST", self.content_url + "/content/v2/cards/error/list",
            params={"locale": "zh"},
            json_body={"cursor": {"limit": 100}, "order": {"ascending": False}},
        )
        envelope = data.get("data") if isinstance(data.get("data"), dict) else data
        batches = envelope.get("items") or envelope.get("batches") or []
        normalized = str(vendor_code or "").strip()
        errors: list[str] = []
        for batch in batches:
            if not isinstance(batch, dict):
                continue
            values = (batch.get("errors") or {}).get(normalized) if isinstance(batch.get("errors"), dict) else None
            errors.extend(str(value) for value in (values or []) if str(value).strip())
        return errors

    def upload_media(self, nm_id: int, image_path: str | Path, photo_number: int) -> dict[str, Any]:
        path = Path(image_path)
        mime_type = mimetypes.guess_type(path.name)[0] or "application/octet-stream"
        with path.open("rb") as stream:
            data = self._request(
                "POST", self.content_url + "/content/v3/media/file",
                files={"uploadfile": (path.name, stream, mime_type)},
                extra_headers={
                    "X-Nm-Id": str(int(nm_id)),
                    "X-Photo-Number": str(int(photo_number)),
                },
            )
        return data if isinstance(data, dict) else {}

    def set_price(self, nm_id: int, price: int, discount=0) -> int:
        data = self._request(
            "POST", self.prices_url + "/api/v2/upload/task",
            json_body={"data": [{
                "nmID": int(nm_id), "price": int(price), "discount": int(discount),
            }]},
            expected=(200, 208),
        )
        upload_id = (data.get("data") or {}).get("id") if isinstance(data, dict) else None
        if upload_id is None:
            raise RuntimeError("WB 价格接口没有返回 uploadID")
        return int(upload_id)

    def price_task_status(self, upload_id: int) -> dict[str, Any] | None:
        params = {"uploadID": int(upload_id)}
        for index, path in enumerate(("/api/v2/buffer/tasks", "/api/v2/history/tasks")):
            try:
                data = self._request(
                    "GET", self.prices_url + path, params=params,
                    allow_statuses=(400, 404) if index == 0 else (),
                )
            except WbApiError as error:
                # A completed upload disappears from the buffer. Depending on the
                # service version, that is reported as HTTP 400 or error=true.
                if index == 0 and error.status_code in {200, 400, 404}:
                    continue
                raise
            if isinstance(data, dict) and data.get("_http_status") in {400, 404}:
                continue
            payload = data.get("data") if isinstance(data, dict) else None
            if isinstance(payload, dict) and payload.get("status") is not None:
                return payload
        return None

    def price_task_details(self, upload_id: int) -> list[dict[str, Any]]:
        data = self._request(
            "GET", self.prices_url + "/api/v2/history/goods/task",
            params={"uploadID": int(upload_id), "limit": 1000, "offset": 0},
        )
        payload = data.get("data") if isinstance(data, dict) else None
        return [item for item in ((payload or {}).get("historyGoods") or []) if isinstance(item, dict)]

    def warehouses(self) -> list[dict[str, Any]]:
        if self.sandbox:
            return []
        data = self._request("GET", self.MARKETPLACE_URL + "/api/v3/warehouses")
        return [item for item in data if isinstance(item, dict)] if isinstance(data, list) else []

    def update_stock(self, warehouse_id: int, chrt_id: int, amount: int):
        if self.sandbox:
            return {}
        return self._request(
            "PUT", self.MARKETPLACE_URL + f"/api/v3/stocks/{int(warehouse_id)}",
            json_body={"stocks": [{"chrtId": int(chrt_id), "amount": int(amount)}]},
            expected=(204,),
        )


def _decimal(value, label: str, *, allow_zero=False) -> Decimal:
    try:
        number = Decimal(str(value).strip())
    except (InvalidOperation, ValueError) as error:
        raise ValueError(label + "必须是有效数字") from error
    if not number.is_finite() or number < 0 or (not allow_zero and number == 0):
        raise ValueError(label + ("不能小于 0" if allow_zero else "必须大于 0"))
    return number


def parse_characteristic_value(raw_value, charc_type=0):
    if isinstance(raw_value, (list, dict, int, float, bool)):
        return raw_value
    text = str(raw_value or "").strip()
    if not text:
        raise ValueError("WB 属性值不能为空")
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        pass
    if int(charc_type or 0) == 4:
        number = _decimal(text, "WB 数值属性")
        return int(number) if number == number.to_integral_value() else float(number)
    return [text]


def build_card_content_document(inputs: dict[str, Any]) -> dict[str, Any]:
    """Build the editable WB card JSON while dimensions stay in dedicated inputs."""
    try:
        subject_id = int(str(inputs.get("subject_id") or "").strip())
    except ValueError as error:
        raise ValueError("请选择有效的 WB 子类目") from error
    definitions = {
        str(item.get("charcID")): item
        for item in (inputs.get("characteristic_definitions") or [])
        if isinstance(item, dict) and item.get("charcID") not in (None, "")
    }
    characteristics = []
    for charc_id, raw_value in (inputs.get("characteristic_values") or {}).items():
        key = str(charc_id)
        if key not in definitions or str(raw_value).strip() == "":
            continue
        characteristics.append({
            "id": int(key),
            "value": parse_characteristic_value(raw_value, definitions[key].get("charcType", 0)),
        })

    size: dict[str, Any] = {}
    barcode = str(inputs.get("barcode") or "").strip()
    if barcode:
        size["skus"] = [barcode]
    tech_size = str(inputs.get("tech_size") or "").strip()
    wb_size = str(inputs.get("wb_size") or "").strip()
    if tech_size:
        size["techSize"] = tech_size
    if wb_size:
        size["wbSize"] = wb_size

    variant: dict[str, Any] = {
        "vendorCode": str(inputs.get("vendor_code") or "").strip(),
        "title": str(inputs.get("title") or "").strip(),
        "description": str(inputs.get("description") or "").strip(),
        "characteristics": characteristics,
        "sizes": [size],
    }
    brand = str(inputs.get("brand") or "").strip()
    if brand:
        variant["brand"] = brand
    return {"subjectID": subject_id, "variants": [variant]}


def parse_card_content_document(
    document: Any, definitions: list[dict[str, Any]], *, expected_subject_id: int | None = None,
) -> dict[str, Any]:
    """Validate editable/AI WB JSON and return values consumable by the form."""
    if not isinstance(document, dict):
        raise ValueError("WB 内容 JSON 顶层必须是对象")
    try:
        subject_id = int(document.get("subjectID"))
    except (TypeError, ValueError) as error:
        raise ValueError("WB 内容 JSON 缺少有效 subjectID") from error
    if expected_subject_id is not None and subject_id != int(expected_subject_id):
        raise ValueError("JSON 的 subjectID 与当前选择的 WB 子类目不一致")
    variants = document.get("variants")
    if not isinstance(variants, list) or len(variants) != 1 or not isinstance(variants[0], dict):
        raise ValueError("WB 内容 JSON 的 variants 必须包含且只包含一个商品对象")
    variant = variants[0]
    allowed = {
        int(item.get("charcID")): item
        for item in definitions
        if isinstance(item, dict) and item.get("charcID") not in (None, "")
    }
    values: dict[str, Any] = {}
    characteristics = variant.get("characteristics") or []
    if not isinstance(characteristics, list):
        raise ValueError("WB 内容 JSON 的 characteristics 必须是数组")
    for item in characteristics:
        if not isinstance(item, dict):
            raise ValueError("WB characteristics 中的每一项都必须是对象")
        try:
            charc_id = int(item.get("id"))
        except (TypeError, ValueError) as error:
            raise ValueError("WB characteristics 中存在无效属性 ID") from error
        if charc_id not in allowed:
            raise ValueError(f"属性 ID {charc_id} 不属于当前 WB 子类目，已拒绝采用")
        value = item.get("value")
        if value in (None, "", []):
            continue
        values[str(charc_id)] = value

    sizes = variant.get("sizes") or [{}]
    if not isinstance(sizes, list) or not sizes or not isinstance(sizes[0], dict):
        raise ValueError("WB 内容 JSON 的 sizes 必须是对象数组")
    size = sizes[0]
    skus = size.get("skus") or []
    if not isinstance(skus, list):
        raise ValueError("WB sizes.skus 必须是条码数组")
    return {
        "subject_id": subject_id,
        "vendor_code": str(variant.get("vendorCode") or "").strip(),
        "title": str(variant.get("title") or "").strip(),
        "description": str(variant.get("description") or "").strip(),
        "brand": str(variant.get("brand") or "").strip(),
        "barcode": str(skus[0] if skus else "").strip(),
        "tech_size": str(size.get("techSize") or "").strip(),
        "wb_size": str(size.get("wbSize") or "").strip(),
        "characteristic_values": values,
    }


def build_card_payload(inputs: dict[str, Any]) -> list[dict[str, Any]]:
    try:
        subject_id = int(str(inputs.get("subject_id") or "").strip())
    except ValueError as error:
        raise ValueError("请选择有效的 WB 子类目") from error
    if subject_id <= 0:
        raise ValueError("请选择有效的 WB 子类目")
    vendor_code = str(inputs.get("vendor_code") or "").strip()
    title = str(inputs.get("title") or "").strip()
    description = str(inputs.get("description") or "").strip()
    barcode = str(inputs.get("barcode") or "").strip()
    if not vendor_code:
        raise ValueError("请填写 WB 卖家货号")
    if not title:
        raise ValueError("请填写 WB 商品标题")
    if len(title) > 60:
        raise ValueError("WB 商品标题不能超过 60 个字符")
    if not description:
        raise ValueError("请填写 WB 商品描述")
    if len(description) > 5000:
        raise ValueError("WB 商品描述不能超过 5000 个字符")
    if not barcode:
        raise ValueError("请生成或填写 WB 商品条码")

    definitions = [item for item in (inputs.get("characteristic_definitions") or []) if isinstance(item, dict)]
    raw_values = inputs.get("characteristic_values") or {}
    values = {str(key): value for key, value in raw_values.items() if str(value).strip()}
    missing = [
        str(item.get("name") or item.get("charcID"))
        for item in definitions
        if item.get("required") and not values.get(str(item.get("charcID") or ""))
    ]
    if missing:
        raise ValueError("仍缺少 WB 必填属性：" + "、".join(missing))
    definition_by_id = {str(item.get("charcID")): item for item in definitions}
    characteristics = []
    for charc_id, raw_value in values.items():
        try:
            numeric_id = int(charc_id)
        except ValueError as error:
            raise ValueError(f"WB 属性 ID 无效：{charc_id}") from error
        definition = definition_by_id.get(charc_id, {})
        characteristics.append({
            "id": numeric_id,
            "value": parse_characteristic_value(raw_value, definition.get("charcType", 0)),
        })

    dimensions = {
        "length": float(_decimal(inputs.get("length_cm"), "WB 包装长度")),
        "width": float(_decimal(inputs.get("width_cm"), "WB 包装宽度")),
        "height": float(_decimal(inputs.get("height_cm"), "WB 包装高度")),
        "weightBrutto": float(_decimal(inputs.get("weight_kg"), "WB 包装重量")),
    }
    size = {"skus": [barcode]}
    tech_size = str(inputs.get("tech_size") or "").strip()
    wb_size = str(inputs.get("wb_size") or "").strip()
    if tech_size:
        size["techSize"] = tech_size
    if wb_size:
        size["wbSize"] = wb_size
    variant: dict[str, Any] = {
        "vendorCode": vendor_code,
        "title": title,
        "description": description,
        "dimensions": dimensions,
        "characteristics": characteristics,
        "sizes": [size],
    }
    brand = str(inputs.get("brand") or "").strip()
    if brand:
        variant["brand"] = brand
    return [{"subjectID": subject_id, "variants": [variant]}]


def validate_image_paths(image_paths) -> list[str]:
    paths = [Path(path) for path in image_paths if str(path).strip()]
    if not paths:
        raise ValueError("请至少选择 1 张 WB 商品图片")
    if len(paths) > 30:
        raise ValueError("WB 商品图片最多 30 张")
    validated = []
    for path in paths:
        if not path.is_file():
            raise FileNotFoundError("WB 商品图片不存在：" + str(path))
        if path.suffix.casefold() not in {".jpg", ".jpeg", ".png", ".bmp", ".gif", ".webp"}:
            raise ValueError("WB 不支持该图片格式：" + str(path))
        if path.stat().st_size > 32 * 1024 * 1024:
            raise ValueError("WB 单张图片不能超过 32 MB：" + path.name)
        with Image.open(path) as image:
            width, height = image.size
        if width < 700 or height < 900:
            raise ValueError(
                f"WB 图片最低分辨率为 700×900：{path.name} 当前为 {width}×{height}"
            )
        validated.append(str(path))
    return validated


class WbListingWorkflow:
    STAGES = {
        "new": 0,
        "card_submitted": 1,
        "card_ready": 2,
        "media_uploaded": 3,
        "price_submitted": 4,
        "price_ready": 5,
        "completed": 6,
    }

    def __init__(
        self, api: WildberriesApi, *, save_state: Callable[[dict], None],
        log: Callable[[str], None] | None = None, sleep: Callable[[float], None] = time.sleep,
    ):
        self.api = api
        self.save_state = save_state
        self.log = log or (lambda _message: None)
        self.sleep = sleep

    def _save(self, state: dict, message: str | None = None):
        if message is not None:
            state["message"] = message
            self.log(message)
        state["updated_at"] = time.strftime("%Y-%m-%d %H:%M:%S")
        self.save_state(state)

    @staticmethod
    def _card_identity(card: dict[str, Any]) -> tuple[int, int] | None:
        try:
            nm_id = int(card.get("nmID"))
        except (TypeError, ValueError):
            return None
        for size in card.get("sizes") or []:
            try:
                chrt_id = int(size.get("chrtID"))
            except (AttributeError, TypeError, ValueError):
                continue
            if chrt_id > 0:
                return nm_id, chrt_id
        return None

    def _adopt_card(self, state: dict, card: dict[str, Any]) -> bool:
        identity = self._card_identity(card)
        if not identity:
            return False
        state["nm_id"], state["chrt_id"] = identity
        state["stage"] = "card_ready"
        state["status"] = "running"
        self._save(
            state,
            f"WB 商品卡片已同步：nmID={identity[0]}，chrtID={identity[1]}",
        )
        return True

    def advance(
        self, state: dict, *, card_attempts=6, card_interval=5,
        price_attempts=6, price_interval=2, sync_min_age_seconds=1800,
    ) -> dict:
        inputs = state.get("inputs") if isinstance(state.get("inputs"), dict) else {}
        vendor_code = str(inputs.get("vendor_code") or "").strip()
        if not vendor_code:
            raise ValueError("WB 任务缺少卖家货号")
        state.setdefault("stage", "new")
        state["status"] = "running"
        state["error"] = ""
        self._save(state)

        if state["stage"] == "new":
            existing = self.api.find_card(vendor_code)
            if existing:
                state["card_preexisting"] = True
            if existing and self._adopt_card(state, existing):
                pass
            else:
                if not str(inputs.get("barcode") or "").strip():
                    barcodes = self.api.generate_barcodes(1)
                    if not barcodes:
                        raise RuntimeError("WB 条码接口没有返回可用条码")
                    inputs["barcode"] = barcodes[0]
                    state["inputs"] = inputs
                payload = build_card_payload(inputs)
                state["payload"] = payload
                state["stage"] = "card_submitted"
                state["submitted_at"] = time.strftime("%Y-%m-%d %H:%M:%S")
                state["submitted_at_epoch"] = time.time()
                self._save(state, "正在提交 WB 商品卡片；该请求不会自动重复发送")
                try:
                    self.api.create_cards(payload)
                except WbApiError as error:
                    if error.status_code in {400, 401, 402, 403, 413, 422}:
                        state["stage"] = "new"
                        self._save(state)
                    raise

        if state["stage"] == "card_submitted":
            for attempt in range(1, max(1, int(card_attempts)) + 1):
                card = self.api.find_card(vendor_code)
                if card and self._adopt_card(state, card):
                    break
                errors = self.api.failed_card_errors(vendor_code)
                if errors:
                    message = "WB 商品卡片创建失败：" + "；".join(errors)
                    state["stage"] = "new"
                    state["status"] = "failed"
                    state["error"] = message
                    self._save(state, "商品卡片被 WB 拒绝；已允许修正后重新提交")
                    raise RuntimeError(message)
                if attempt < card_attempts:
                    self.log(f"WB 商品卡片仍在同步（{attempt}/{card_attempts}），稍后继续查询")
                    self.sleep(max(0, card_interval))
            if state["stage"] == "card_submitted":
                state["status"] = "waiting"
                self._save(state, "WB 商品卡片仍在异步同步，请稍后点击“继续当前任务”")
                return state

        if state["stage"] == "card_ready":
            image_paths = validate_image_paths(state.get("image_paths") or [])
            uploaded = int(state.get("uploaded_image_count") or 0)
            for index, path in enumerate(image_paths, start=1):
                if index <= uploaded:
                    continue
                self.api.upload_media(state["nm_id"], path, index)
                state["uploaded_image_count"] = index
                self._save(state, f"WB 商品图片已上传 {index}/{len(image_paths)}")
            state["stage"] = "media_uploaded"
            self._save(state, "WB 商品图片上传完成")

        if state["stage"] == "media_uploaded":
            submitted_at = float(state.get("submitted_at_epoch") or 0)
            required_age = max(0, int(sync_min_age_seconds))
            if not state.get("card_preexisting") and submitted_at:
                remaining = required_age - max(0, time.time() - submitted_at)
                if remaining > 0:
                    state["status"] = "waiting"
                    minutes = max(1, int((remaining + 59) // 60))
                    self._save(
                        state,
                        f"WB 新卡片仍在同步价格和仓库服务；约 {minutes} 分钟后点击“继续当前任务”",
                    )
                    return state
            price = _decimal(inputs.get("price"), "WB 售价")
            if price != price.to_integral_value():
                raise ValueError("WB 售价必须是整数")
            discount = int(_decimal(inputs.get("discount", 0), "WB 折扣", allow_zero=True))
            if discount >= 100:
                raise ValueError("WB 折扣必须小于 100%")
            upload_id = self.api.set_price(state["nm_id"], int(price), discount)
            state["price_upload_id"] = upload_id
            state["stage"] = "price_submitted"
            self._save(state, f"WB 价格任务已提交：uploadID={upload_id}")

        if state["stage"] == "price_submitted":
            result = None
            for attempt in range(1, max(1, int(price_attempts)) + 1):
                result = self.api.price_task_status(state["price_upload_id"])
                status = int((result or {}).get("status") or 0)
                if status in {3, 4, 5, 6}:
                    break
                if attempt < price_attempts:
                    self.sleep(max(0, price_interval))
            status = int((result or {}).get("status") or 0)
            if status == 3:
                state["stage"] = "price_ready"
                self._save(state, "WB 售价已经生效")
            elif status in {4, 5, 6}:
                details = self.api.price_task_details(state["price_upload_id"])
                rendered = "；".join(
                    str(item.get("errorText") or item) for item in details
                    if item.get("errorText")
                )
                message = f"WB 价格任务状态为 {status}" + (
                    "：" + rendered if rendered else ""
                )
                state["stage"] = "media_uploaded"
                state["status"] = "failed"
                state["error"] = message
                self._save(state, "WB 价格任务失败；已允许修改价格后重新提交")
                raise RuntimeError(message)
            else:
                state["status"] = "waiting"
                self._save(state, "WB 价格仍在处理，请稍后点击“继续当前任务”")
                return state

        if state["stage"] == "price_ready":
            if not self.api.sandbox:
                try:
                    warehouse_id = int(str(inputs.get("warehouse_id") or "").strip())
                    stock = int(str(inputs.get("stock") or "").strip())
                except ValueError as error:
                    raise ValueError("WB 仓库 ID 和库存必须是整数") from error
                if warehouse_id <= 0 or stock < 0:
                    raise ValueError("WB 仓库 ID 必须大于 0，库存不能小于 0")
                self.api.update_stock(warehouse_id, state["chrt_id"], stock)
                state["stock_written"] = True
            state["stage"] = "completed"
            state["status"] = "completed"
            suffix = "；Sandbox 模式未写入真实仓库库存" if self.api.sandbox else "，库存已写入"
            self._save(state, "WB 商品上架流程完成" + suffix)
        return state
