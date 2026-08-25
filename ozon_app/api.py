from __future__ import annotations

import csv
import io
import json
import re
import socket
import ssl
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from datetime import date, datetime, timedelta, timezone
from typing import Any, Callable, Iterable

from .config import PERFORMANCE_BASE_URL, SELLER_BASE_URL


class OzonApiError(RuntimeError):
    pass


class SyncCancelled(OzonApiError):
    pass


@dataclass
class SyncResult:
    source: str
    rows: int
    message: str


EventCallback = Callable[[str, str, str, dict[str, Any]], None]


_RATE_LOCK = threading.Lock()
_LAST_REQUEST_AT: dict[str, float] = {}


class JsonHttpClient:
    def request(
        self,
        method: str,
        url: str,
        *,
        headers: dict[str, str] | None = None,
        body: dict[str, Any] | None = None,
        timeout: int = 45,
        max_attempts: int = 4,
        progress: Callable[[str], None] | None = None,
        cancel_event: threading.Event | None = None,
        retry_429: bool = True,
    ) -> Any:
        data = None
        request_headers = {"Accept": "application/json", "User-Agent": "OzonAnalyticsDesktop/0.1"}
        if headers:
            request_headers.update(headers)
        if body is not None:
            data = json.dumps(body, ensure_ascii=False).encode("utf-8")
            request_headers.setdefault("Content-Type", "application/json")
        endpoint = urllib.parse.urlsplit(url).path
        for attempt in range(max(1, max_attempts)):
            _raise_if_cancelled(cancel_event)
            request = urllib.request.Request(url, data=data, headers=request_headers, method=method.upper())
            try:
                with urllib.request.urlopen(request, timeout=timeout) as response:
                    raw = response.read()
                    content_type = response.headers.get("Content-Type", "")
                break
            except urllib.error.HTTPError as error:
                payload = error.read().decode("utf-8", errors="replace")
                detail = _extract_error_message(payload)
                retryable = (error.code == 429 and retry_429) or error.code in {500, 502, 503, 504}
                if retryable and attempt + 1 < max_attempts:
                    if error.code == 429:
                        delay = _retry_after_seconds(error.headers, default=65)
                        message = f"接口 {endpoint} 触发限频，{delay} 秒后自动重试"
                    else:
                        delay = min(15, 2 ** (attempt + 1))
                        message = f"接口 {endpoint} 暂时不可用，{delay} 秒后重试"
                    if progress:
                        progress(message)
                    _wait(delay, cancel_event)
                    continue
                if error.code in (401, 403):
                    raise OzonApiError(
                        f"认证失败（HTTP {error.code}，接口 {endpoint}）：请检查 API 凭证和权限。{detail}"
                    ) from error
                if error.code == 429:
                    if retry_429:
                        message = (
                            f"接口 {endpoint} 仍被限频（HTTP 429），"
                            "自动重试后仍未恢复，请稍后再同步。"
                        )
                    else:
                        message = (
                            f"接口 {endpoint} 被限频（HTTP 429）；本次未继续长时间等待，"
                            "将使用本地缓存（如有）。"
                        )
                    raise OzonApiError(message) from error
                raise OzonApiError(f"Ozon API 返回 HTTP {error.code}（接口 {endpoint}）。{detail}") from error
            except (urllib.error.URLError, ssl.SSLError, socket.timeout, TimeoutError, ConnectionResetError) as error:
                if attempt + 1 < max_attempts:
                    delay = min(15, 2 ** (attempt + 1))
                    if progress:
                        progress(f"连接 {endpoint} 暂时中断，{delay} 秒后自动重试")
                    _wait(delay, cancel_event)
                    continue
                reason = getattr(error, "reason", error)
                raise OzonApiError(
                    f"无法连接 Ozon API（接口 {endpoint}，已自动重试 {max_attempts} 次）：{reason}"
                ) from error

        if not raw:
            return {}
        text = raw.decode("utf-8-sig", errors="replace")
        if "json" in content_type.lower() or text[:1] in "[{":
            try:
                return json.loads(text)
            except json.JSONDecodeError as error:
                raise OzonApiError("Ozon 返回了无法解析的 JSON 数据。") from error
        return text


class SellerApiClient:
    def __init__(self, client_id: str, api_key: str, http: JsonHttpClient | None = None):
        self.client_id = client_id.strip()
        self.api_key = api_key.strip()
        self.http = http or JsonHttpClient()
        self._posting_cache: dict[tuple[str, str], dict[str, list[dict[str, Any]]]] = {}
        self.last_analytics_metadata: dict[str, Any] = {}

    @property
    def headers(self) -> dict[str, str]:
        return {
            "Client-Id": self.client_id,
            "Api-Key": self.api_key,
            "Content-Type": "application/json",
        }

    def _post(
        self,
        path: str,
        body: dict[str, Any],
        *,
        progress: Callable[[str], None] | None = None,
        cancel_event: threading.Event | None = None,
        max_attempts: int = 4,
        retry_429: bool = True,
    ) -> Any:
        if not self.client_id or not self.api_key:
            raise OzonApiError("请先填写 Seller Client ID 和 Seller API Key。")
        return self.http.request(
            "POST", SELLER_BASE_URL + path, headers=self.headers, body=body,
            progress=progress, cancel_event=cancel_event,
            max_attempts=max_attempts, retry_429=retry_429,
        )

    def test_connection(self) -> str:
        # /v1/roles 是 Ozon 提供的 API Key 信息接口，适合用于最小化连接测试。
        # 旧版 MVP 曾使用 /v1/warehouse/list，该接口已被 Ozon 停用。
        response = self._post("/v1/roles", {})
        if isinstance(response, dict):
            return "Seller API 连接成功"
        return "Seller API 已响应"

    def inventory_stocks(
        self,
        skus: Iterable[str | int],
        *,
        progress: Callable[[str], None] | None = None,
        cancel_event: threading.Event | None = None,
    ) -> list[dict[str, Any]]:
        """Return Ozon's current FBO stock analytics for up to 100 SKUs/request."""
        normalized = []
        for value in skus:
            text = str(value).strip()
            if text.isdigit() and int(text) > 0:
                normalized.append(int(text))
        normalized = list(dict.fromkeys(normalized))
        rows: list[dict[str, Any]] = []
        for start in range(0, len(normalized), 100):
            batch = normalized[start:start + 100]
            if progress:
                progress(
                    f"正在读取库存第 {start // 100 + 1} 批"
                    f"（SKU {start + 1}–{start + len(batch)} / {len(normalized)}）…"
                )
            response = self._post(
                "/v1/analytics/stocks", {"skus": batch},
                progress=progress, cancel_event=cancel_event,
                max_attempts=2, retry_429=True,
            )
            rows.extend((response or {}).get("items") or [])
        return rows

    def fbo_clusters(
        self,
        *,
        progress: Callable[[str], None] | None = None,
        cancel_event: threading.Event | None = None,
    ) -> list[dict[str, Any]]:
        """Return the current macro-cluster and fulfillment warehouse directory."""
        response = self._post(
            "/v2/cluster/list", {}, progress=progress, cancel_event=cancel_event,
            max_attempts=2, retry_429=True,
        )
        return list((response or {}).get("result") or [])

    def seller_supply_warehouses(
        self,
        *,
        progress: Callable[[str], None] | None = None,
        cancel_event: threading.Event | None = None,
    ) -> list[dict[str, Any]]:
        """Return the seller's FBO shipping warehouses, including postal addresses."""
        response = self._post(
            "/v1/warehouse/fbo/seller/list", {},
            progress=progress, cancel_event=cancel_event,
            max_attempts=2, retry_429=True,
        )
        return list((response or {}).get("warehouses") or [])

    def search_supply_dropoff_warehouses(
        self,
        search: str,
        supply_type: str = "CROSSDOCK",
        *,
        progress: Callable[[str], None] | None = None,
        cancel_event: threading.Event | None = None,
    ) -> list[dict[str, Any]]:
        """Search current FBO drop-off points for a new supply draft."""
        query = str(search or "").strip()
        if len(query) < 4:
            raise ValueError("发货点搜索词至少需要 4 个字符。")
        normalized_type = str(supply_type or "").strip().upper()
        if normalized_type not in {"DIRECT", "CROSSDOCK"}:
            raise ValueError("供应方式必须是 DIRECT 或 CROSSDOCK。")
        response = self._post(
            "/v1/warehouse/fbo/list",
            {
                "filter_by_supply_type": [f"CREATE_TYPE_{normalized_type}"],
                "search": query,
            },
            progress=progress,
            cancel_event=cancel_event,
            max_attempts=2,
            retry_429=True,
        )
        rows: list[dict[str, Any]] = []
        for raw in (response or {}).get("search") or []:
            row = dict(raw or {})
            warehouse_type = str(row.get("warehouse_type") or "").strip().upper()
            if normalized_type == "CROSSDOCK" and warehouse_type in {
                "", "0", "UNSPECIFIED", "UNKNOWN", "WAREHOUSE_TYPE_UNSPECIFIED",
            }:
                warehouse_type = "CROSS_DOCK"
            row["warehouse_type"] = warehouse_type
            rows.append(row)
        return rows

    def create_supply_draft(
        self,
        sku: int | str,
        quantity: int,
        macrolocal_cluster_id: int | str,
        supply_type: str,
        *,
        delivery_info: dict[str, Any] | None = None,
    ) -> dict[str, Any]:
        """Create one current typed FBO draft.  This write is never retried."""
        normalized_type = str(supply_type or "").strip().upper()
        if normalized_type not in {"DIRECT", "CROSSDOCK"}:
            raise ValueError("供应方式必须是 DIRECT 或 CROSSDOCK。")
        if int(quantity) <= 0:
            raise ValueError("草稿商品数量必须大于 0。")
        cluster_info = {
            "items": [{"sku": int(sku), "quantity": int(quantity)}],
            "macrolocal_cluster_id": int(macrolocal_cluster_id),
        }
        body: dict[str, Any] = {
            "cluster_info": cluster_info,
            "deletion_sku_mode": "PARTIAL",
        }
        if normalized_type == "CROSSDOCK":
            if not delivery_info:
                raise ValueError("越库草稿必须选择发货点。")
            normalized_delivery = dict(delivery_info)
            dropoff = dict(normalized_delivery.get("drop_off_warehouse") or {})
            warehouse_id = int(dropoff.get("warehouse_id") or 0)
            if warehouse_id <= 0:
                raise ValueError("越库草稿的发货点 ID 无效，请重新选择发货点。")
            # /warehouse/fbo/list may return the protobuf default enum as the
            # string "0".  /draft/crossdock/create explicitly rejects that
            # unspecified value; the selected point is necessarily cross-dock.
            dropoff["warehouse_id"] = warehouse_id
            dropoff["warehouse_type"] = "CROSS_DOCK"
            normalized_delivery["type"] = "DROPOFF"
            normalized_delivery["drop_off_warehouse"] = dropoff
            body["delivery_info"] = normalized_delivery
        response = self._post(
            f"/v1/draft/{'direct' if normalized_type == 'DIRECT' else 'crossdock'}/create",
            body,
            max_attempts=1,
            retry_429=False,
        )
        return dict(response or {})

    def supply_draft_info(
        self,
        draft_id: int,
        *,
        progress: Callable[[str], None] | None = None,
        cancel_event: threading.Event | None = None,
    ) -> dict[str, Any]:
        response = self._post(
            "/v2/draft/create/info", {"draft_id": int(draft_id)},
            progress=progress, cancel_event=cancel_event,
            max_attempts=2, retry_429=True,
        )
        return dict(response or {})

    def supply_draft_timeslots(
        self,
        draft_id: int,
        supply_type: str,
        macrolocal_cluster_id: int,
        storage_warehouse_id: int,
        date_from: str,
        date_to: str,
        *,
        progress: Callable[[str], None] | None = None,
        cancel_event: threading.Event | None = None,
    ) -> dict[str, Any]:
        response = self._post(
            "/v2/draft/timeslot/info",
            {
                "date_from": date_from,
                "date_to": date_to,
                "draft_id": int(draft_id),
                "supply_type": str(supply_type).upper(),
                "selected_cluster_warehouses": [{
                    "macrolocal_cluster_id": int(macrolocal_cluster_id),
                    "storage_warehouse_id": int(storage_warehouse_id),
                }],
            },
            progress=progress, cancel_event=cancel_event,
            max_attempts=2, retry_429=True,
        )
        return dict(response or {})

    def create_supply_from_draft(
        self,
        draft_id: int,
        supply_type: str,
        macrolocal_cluster_id: int,
        storage_warehouse_id: int,
        timeslot_from: str,
        timeslot_to: str,
    ) -> dict[str, Any]:
        """Create a real supply order from a draft exactly once."""
        response = self._post(
            "/v2/draft/supply/create",
            {
                "draft_id": int(draft_id),
                "supply_type": str(supply_type).upper(),
                "selected_cluster_warehouses": [{
                    "macrolocal_cluster_id": int(macrolocal_cluster_id),
                    "storage_warehouse_id": int(storage_warehouse_id),
                }],
                "timeslot": {
                    "from_in_timezone": timeslot_from,
                    "to_in_timezone": timeslot_to,
                },
            },
            max_attempts=1,
            retry_429=False,
        )
        return dict(response or {})

    def supply_from_draft_status(self, draft_id: int) -> dict[str, Any]:
        response = self._post(
            "/v2/draft/supply/create/status", {"draft_id": int(draft_id)},
            max_attempts=2, retry_429=True,
        )
        return dict(response or {})

    def supply_order_status_counts(self) -> list[dict[str, Any]]:
        response = self._post(
            "/v1/supply-order/status/counter", {}, max_attempts=2, retry_429=True
        )
        return list((response or {}).get("items") or [])

    def supply_orders(
        self,
        states: list[int] | None = None,
        *,
        progress: Callable[[str], None] | None = None,
        cancel_event: threading.Event | None = None,
    ) -> list[dict[str, Any]]:
        """Return active FBO supply orders using the current v3 list/get flow."""
        # Enum values 1..7 are all non-terminal supply-order states.  Ozon's
        # protobuf JSON gateway currently rejects the symbolic enum names.
        states = states or [1, 2, 3, 4, 5, 6, 7]
        order_ids: list[int] = []
        last_id = ""
        for page in range(20):
            if progress:
                progress(f"正在读取 FBO 供应单第 {page + 1} 页…")
            response = self._post(
                "/v3/supply-order/list",
                {
                    "filter": {"states": states},
                    "last_id": last_id,
                    "limit": 100,
                    "sort_by": 1,
                    "sort_order": 1,
                },
                progress=progress,
                cancel_event=cancel_event,
                max_attempts=2,
                retry_429=True,
            )
            page_ids = [int(value) for value in (response or {}).get("order_ids") or []]
            order_ids.extend(page_ids)
            next_id = str((response or {}).get("last_id") or "")
            if not page_ids or not next_id or next_id == last_id:
                break
            last_id = next_id
        orders: list[dict[str, Any]] = []
        for start in range(0, len(order_ids), 50):
            batch = order_ids[start:start + 50]
            response = self._post(
                "/v3/supply-order/get",
                {"order_ids": batch},
                progress=progress,
                cancel_event=cancel_event,
                max_attempts=2,
                retry_429=True,
            )
            orders.extend((response or {}).get("orders") or [])
        return orders

    def supply_order_timeslots(
        self,
        order_id: int,
        date_from: str,
        date_to: str,
        *,
        progress: Callable[[str], None] | None = None,
        cancel_event: threading.Event | None = None,
    ) -> list[dict[str, str]]:
        response = self._post(
            "/v2/supply-order/timeslot/list",
            {"order_id": int(order_id), "date_from": date_from, "date_to": date_to},
            progress=progress,
            cancel_event=cancel_event,
            max_attempts=2,
            retry_429=True,
        )
        values = ((response or {}).get("timeslots_info") or {}).get("timeslots") or []
        return [
            {"from": str(item.get("from") or ""), "to": str(item.get("to") or "")}
            for item in values
            if item.get("from") and item.get("to")
        ]

    def update_supply_order_timeslot(
        self,
        supply_order_id: int,
        timeslot_from: str,
        timeslot_to: str,
    ) -> dict[str, Any]:
        response = self._post(
            "/v1/supply-order/timeslot/update",
            {
                "supply_order_id": int(supply_order_id),
                "timeslot": {"from": timeslot_from, "to": timeslot_to},
            },
            # A write request must never be retried automatically: after a
            # network interruption its server-side result can be ambiguous.
            max_attempts=1,
            retry_429=False,
        )
        return dict(response or {})

    def supply_order_timeslot_update_status(self, operation_id: str) -> dict[str, Any]:
        response = self._post(
            "/v1/supply-order/timeslot/status",
            {"operation_id": operation_id},
            max_attempts=2,
            retry_429=True,
        )
        return dict(response or {})

    def analytics(
        self,
        date_from: str,
        date_to: str,
        progress: Callable[[str], None] | None = None,
        cancel_event: threading.Event | None = None,
    ) -> list[dict[str, Any]]:
        metrics = [
            "revenue", "ordered_units", "delivered_units", "returns", "cancellations",
            "hits_view", "hits_tocart",
        ]
        rows: list[dict[str, Any]] = []
        offset = 0
        returned_metric_counts: list[int] = []
        while True:
            # Ozon 对 /v1/analytics/data 的限制是每个 Seller 账号每分钟 1 次。
            _wait_for_rate_limit("seller.analytics", 65, progress, cancel_event)
            page = offset // 1000 + 1
            if progress:
                progress(
                    f"正在读取 Seller 销量分析第 {page} 页"
                    f"（{date_from} 至 {date_to}，offset={offset}）…"
                )
            payload = {
                "date_from": date_from,
                "date_to": date_to,
                "metrics": metrics,
                "dimension": ["sku", "day"],
                "filters": [],
                "sort": [{"key": "revenue", "order": "DESC"}],
                "limit": 1000,
                "offset": offset,
            }
            response = self._post(
                "/v1/analytics/data", payload, progress=progress, cancel_event=cancel_event,
                # 只自动重试一次，最长额外等待约 65 秒；避免旧版连续等待
                # 4 次、耗时数分钟，同时给偶发的账号级限频一次恢复机会。
                max_attempts=2, retry_429=True,
            )
            _mark_rate_limit("seller.analytics")
            result = (response or {}).get("result") or {}
            data = result.get("data") or []
            totals = result.get("totals") or []
            page_metric_count = len((data[0] or {}).get("metrics") or []) if data else len(totals)
            returned_metric_counts.append(page_metric_count)
            for item in data:
                dimensions = item.get("dimensions") or []
                values = item.get("metrics") or []
                metric_map = {metric: values[index] if index < len(values) else 0 for index, metric in enumerate(metrics)}
                sku, product_name, day = _read_dimensions(dimensions)
                rows.append(
                    {
                        "day": day or date_to,
                        "sku": sku,
                        "product_name": product_name,
                        "revenue": metric_map["revenue"],
                        "ordered_units": metric_map["ordered_units"],
                        "delivered_units": metric_map["delivered_units"],
                        "returns": metric_map["returns"],
                        "cancellations": metric_map["cancellations"],
                        "views": metric_map["hits_view"],
                        "cart_adds": metric_map["hits_tocart"],
                        "source": "api",
                    }
                )
            if progress:
                progress(
                    f"Seller 销量分析第 {page} 页返回 {len(data)} 行，"
                    f"累计解析 {len(rows)} 行"
                )
            if len(data) < 1000:
                break
            offset += 1000
        returned_metric_count = min(returned_metric_counts) if returned_metric_counts else 0
        self.last_analytics_metadata = {
            "requested_metrics": list(metrics),
            "requested_metric_count": len(metrics),
            "returned_metric_count": returned_metric_count,
            "returned_metrics": metrics[:returned_metric_count],
            "omitted_metrics": metrics[returned_metric_count:],
            "traffic_available": returned_metric_count >= len(metrics),
            "pages": len(returned_metric_counts),
        }
        return [row for row in rows if row["sku"]]

    def products(
        self,
        progress: Callable[[str], None] | None = None,
        cancel_event: threading.Event | None = None,
    ) -> list[dict[str, Any]]:
        """Return the seller article number for every Ozon SKU in the catalog."""
        rows: list[dict[str, Any]] = []
        last_id = ""
        page = 1
        while True:
            _raise_if_cancelled(cancel_event)
            if progress:
                progress(f"正在读取商品目录与货号（第 {page} 页）…")
            response = self._post(
                "/v3/product/list",
                {
                    "filter": {"visibility": "ALL"},
                    "last_id": last_id,
                    "limit": 1000,
                },
                progress=progress,
                cancel_event=cancel_event,
            )
            result = (response or {}).get("result") or {}
            items = result.get("items") or []
            for item in items:
                sku = item.get("sku")
                if sku in (None, "", 0, "0"):
                    continue
                images = item.get("images") or []
                first_image = images[0] if isinstance(images, list) and images else ""
                if isinstance(first_image, dict):
                    first_image = first_image.get("url") or first_image.get("file_name") or ""
                image_url = str(
                    item.get("primary_image") or item.get("primary_image_url")
                    or first_image
                    or ""
                )
                rows.append(
                    {
                        "sku": str(sku),
                        "offer_id": str(item.get("offer_id") or ""),
                        "product_id": str(item.get("product_id") or item.get("id") or ""),
                        "name": str(item.get("name") or ""),
                        "image_url": image_url,
                        "source": "api",
                    }
                )
            next_id = str(result.get("last_id") or "")
            total = int(result.get("total") or 0)
            if not items or not next_id or next_id == last_id or (total and len(rows) >= total):
                break
            last_id = next_id
            page += 1
        return rows

    def returns(
        self,
        date_from: str,
        date_to: str,
        progress: Callable[[str], None] | None = None,
        cancel_event: threading.Event | None = None,
    ) -> list[dict[str, Any]]:
        """Read customer-return quantities from the unified returns endpoint.

        The endpoint also returns ``Cancellation`` records (refusals, failed
        deliveries, and other logistics returns).  Seller Analytics' “returned
        products” column is the ``ClientReturn`` business type, so mixing both
        types greatly overstates returns and also duplicates cancellation data.
        """
        rows: list[dict[str, Any]] = []
        last_id = 0
        page = 1
        raw_count = 0
        excluded_count = 0
        time_from, time_to = _utc_range(date_from, date_to)
        while True:
            _raise_if_cancelled(cancel_event)
            if progress:
                progress(f"正在读取退货明细（第 {page} 页）…")
            response = self._post(
                "/v1/returns/list",
                {
                    "filter": {
                        "logistic_return_date": {
                            "time_from": time_from,
                            "time_to": time_to,
                        }
                    },
                    "limit": 500,
                    "last_id": last_id,
                },
                progress=progress,
                cancel_event=cancel_event,
            )
            items = (response or {}).get("returns") or []
            for item in items:
                product = item.get("product") or {}
                quantity = int(product.get("quantity") or 1)
                raw_count += quantity
                return_type = str(item.get("type") or item.get("return_type") or "")
                if return_type.lower() != "clientreturn":
                    excluded_count += quantity
                    continue
                logistic = item.get("logistic") or {}
                visual = item.get("visual") or {}
                event_day = _iso_day(
                    logistic.get("return_date")
                    or logistic.get("final_moment")
                    or visual.get("change_moment")
                    or date_to
                )
                return_id = item.get("id") or item.get("return_id")
                if return_id in (None, ""):
                    return_id = f"{item.get('posting_number', '')}:{product.get('sku', '')}:{event_day}"
                rows.append(
                    {
                        "return_id": str(return_id),
                        "day": event_day,
                        "sku": str(product.get("sku") or ""),
                        "offer_id": str(product.get("offer_id") or ""),
                        "product_name": str(product.get("name") or ""),
                        "quantity": quantity,
                        "source": "api",
                    }
                )
            if progress:
                progress(
                    f"退货明细第 {page} 页：原始 {len(items)} 条，"
                    f"累计实际买家退货 {sum(int(row['quantity']) for row in rows)} 件，"
                    f"已排除取消/拒收等物流退回 {excluded_count} 件"
                )
            has_next = bool((response or {}).get("has_next"))
            if not has_next or not items:
                break
            next_id = int(items[-1].get("id") or items[-1].get("return_id") or last_id)
            if next_id == last_id:
                break
            last_id = next_id
            page += 1
        if progress:
            progress(
                f"退货口径校验完成：原始 {raw_count} 件，实际买家退货 "
                f"{sum(int(row['quantity']) for row in rows)} 件，排除 {excluded_count} 件"
            )
        return rows

    def cancellations(
        self,
        date_from: str,
        date_to: str,
        progress: Callable[[str], None] | None = None,
        cancel_event: threading.Event | None = None,
    ) -> list[dict[str, Any]]:
        """Read cancelled products from the cached FBS/FBO posting lists."""
        return self._posting_events(
            date_from, date_to, "cancellations", progress, cancel_event
        )

    def deliveries(
        self,
        date_from: str,
        date_to: str,
        progress: Callable[[str], None] | None = None,
        cancel_event: threading.Event | None = None,
    ) -> list[dict[str, Any]]:
        """Read delivered products, reusing the same posting response as cancellations."""
        return self._posting_events(
            date_from, date_to, "deliveries", progress, cancel_event
        )

    def posting_routes(
        self,
        date_from: str,
        date_to: str,
        progress: Callable[[str], None] | None = None,
        cancel_event: threading.Event | None = None,
    ) -> list[dict[str, Any]]:
        """Return routes for every posting status from the shared page cache."""
        return self._posting_events(
            date_from, date_to, "routes", progress, cancel_event
        )

    def _posting_collections(
        self,
        date_from: str,
        date_to: str,
        progress: Callable[[str], None] | None,
        cancel_event: threading.Event | None,
    ) -> dict[str, list[dict[str, Any]]]:
        cache_key = (date_from, date_to)
        if cache_key in self._posting_cache:
            if progress:
                progress("复用已读取的 FBO/FBS 发货列表…")
            return self._posting_cache[cache_key]
        collections: dict[str, list[dict[str, Any]]] = {}
        for scheme, current_path, fallback_path in (
            ("FBS", "/v4/posting/fbs/list", "/v3/posting/fbs/list"),
            ("FBO", "/v3/posting/fbo/list", "/v2/posting/fbo/list"),
        ):
            try:
                postings = self._posting_pages(
                    current_path, date_from, date_to, scheme, progress, cancel_event
                )
            except OzonApiError:
                # Ozon rolls out posting versions account-by-account.  The fallback
                # keeps existing accounts working during the official transition.
                postings = self._posting_pages(
                    fallback_path, date_from, date_to, scheme, progress, cancel_event
                )
            collections[scheme] = postings
        self._posting_cache[cache_key] = collections
        return collections

    def _posting_events(
        self,
        date_from: str,
        date_to: str,
        metric: str,
        progress: Callable[[str], None] | None,
        cancel_event: threading.Event | None,
    ) -> list[dict[str, Any]]:
        rows: list[dict[str, Any]] = []
        for scheme, postings in self._posting_collections(
            date_from, date_to, progress, cancel_event
        ).items():
            for posting in postings:
                status = str(posting.get("status") or "").lower()
                if metric == "cancellations" and "cancel" not in status:
                    continue
                if metric == "deliveries" and not (
                    status == "delivered" or status.endswith("_delivered")
                ):
                    continue
                posting_number = str(
                    posting.get("posting_number") or posting.get("order_number") or ""
                )
                analytics = posting.get("analytics_data") or {}
                if not isinstance(analytics, dict):
                    analytics = {}
                origin = str(
                    analytics.get("warehouse_name")
                    or analytics.get("warehouse")
                    or posting.get("warehouse_name") or ""
                )
                destination = str(
                    analytics.get("region") or analytics.get("city")
                    or analytics.get("delivery_city") or ""
                )
                financial = posting.get("financial_data") or {}
                if not isinstance(financial, dict):
                    financial = {}
                financial_products = financial.get("products") or []
                event_day = _iso_day(
                    posting.get("in_process_at")
                    or posting.get("created_at")
                    or posting.get("shipment_date")
                    or date_to
                )
                for index, product in enumerate(posting.get("products") or []):
                    sku = str(product.get("sku") or product.get("product_id") or "")
                    quantity = int(product.get("quantity") or 1)
                    financial_product: dict[str, Any] = {}
                    for candidate in financial_products:
                        if not isinstance(candidate, dict):
                            continue
                        candidate_sku = str(
                            candidate.get("product_id") or candidate.get("sku")
                            or candidate.get("offer_id") or ""
                        )
                        if candidate_sku in {sku, str(product.get("offer_id") or "")}:
                            financial_product = candidate
                            break
                    rows.append(
                        {
                            "event_id": f"{metric}:{scheme}:{posting_number}:{sku}:{index}",
                            "day": event_day,
                            "sku": sku,
                            "offer_id": str(product.get("offer_id") or ""),
                            "product_name": str(product.get("name") or ""),
                            "quantity": quantity,
                            "scheme": scheme,
                            "status": status,
                            "posting_number": posting_number,
                            "origin": origin,
                            "destination": destination,
                            "order_price": _money(
                                financial_product.get("price") or product.get("price")
                            ) * quantity,
                            "commission": _money(
                                financial_product.get("commission_amount")
                                or financial_product.get("commission")
                            ) * quantity,
                            "payout": _money(financial_product.get("payout")) * quantity,
                            "source": "api",
                        }
                    )
        return rows

    def _posting_pages(
        self,
        path: str,
        date_from: str,
        date_to: str,
        scheme: str,
        progress: Callable[[str], None] | None,
        cancel_event: threading.Event | None,
    ) -> list[dict[str, Any]]:
        rows: list[dict[str, Any]] = []
        offset = 0
        page = 1
        time_from, time_to = _utc_range(date_from, date_to)
        while True:
            _raise_if_cancelled(cancel_event)
            if progress:
                progress(f"正在读取 {scheme} 发货状态（第 {page} 页）…")
            response = self._post(
                path,
                {
                    "dir": "ASC",
                    "filter": {"since": time_from, "to": time_to},
                    "limit": 1000,
                    "offset": offset,
                    "with": {
                        "analytics_data": True,
                        "barcodes": False,
                        "financial_data": True,
                        "translit": False,
                    },
                },
                progress=progress,
                cancel_event=cancel_event,
            )
            container = (response or {}).get("result")
            if isinstance(container, list):
                postings = container
                has_next = len(postings) >= 1000
            else:
                container = container or response or {}
                postings = container.get("postings") or []
                has_next = bool(container.get("has_next"))
            rows.extend(postings)
            if not postings or not has_next:
                break
            offset += len(postings)
            page += 1
        return rows

    def finance_transactions(
        self,
        date_from: str,
        date_to: str,
        progress: Callable[[str], None] | None = None,
        cancel_event: threading.Event | None = None,
    ) -> list[dict[str, Any]]:
        rows: list[dict[str, Any]] = []
        page = 1
        cursor = date.fromisoformat(date_from)
        end = date.fromisoformat(date_to)
        while cursor <= end:
            month_end = min(_next_month(cursor) - timedelta(days=1), end)
            page = 1
            while True:
                payload = {
                    "filter": {
                        "date": {
                            "from": datetime.combine(cursor, datetime.min.time(), tzinfo=timezone.utc).isoformat().replace("+00:00", "Z"),
                            "to": datetime.combine(month_end, datetime.max.time(), tzinfo=timezone.utc).isoformat().replace("+00:00", "Z"),
                        },
                        "operation_type": [],
                        "posting_number": "",
                        "transaction_type": "all",
                    },
                    "page": page,
                    "page_size": 1000,
                }
                response = self._post(
                    "/v3/finance/transaction/list", payload,
                    progress=progress, cancel_event=cancel_event,
                )
                result = (response or {}).get("result") or response or {}
                operations = result.get("operations") or []
                rows.extend(operations)
                page_count = int(result.get("page_count") or 1)
                if page >= page_count or not operations:
                    break
                page += 1
            cursor = month_end + timedelta(days=1)
        return rows

    def finance_cash_flow_statement(
        self,
        date_from: str,
        date_to: str,
        progress: Callable[[str], None] | None = None,
        cancel_event: threading.Event | None = None,
    ) -> dict[str, list[dict[str, Any]]]:
        """Read the Seller ``商店经济`` cash-flow summary and fee details.

        Ozon returns this report by settlement period.  We keep both the
        summarized cash-flow rows and the nested detail rows because the former
        is the store-level reconciliation source while the latter exposes fees
        which cannot always be attributed to one SKU.
        """
        cash_flows: list[dict[str, Any]] = []
        details: list[dict[str, Any]] = []
        page = 1
        while True:
            _raise_if_cancelled(cancel_event)
            if progress:
                progress(f"正在读取商店经济应计费用（第 {page} 页）…")
            payload = {
                "date": {
                    "from": f"{date_from}T00:00:00Z",
                    "to": f"{date_to}T23:59:59Z",
                },
                "with_details": True,
                "page": page,
                "page_size": 100,
            }
            response = self._post(
                "/v1/finance/cash-flow-statement/list", payload,
                progress=progress, cancel_event=cancel_event,
            )
            result = (response or {}).get("result") or response or {}
            page_cash_flows = result.get("cash_flows") or []
            page_details = result.get("details") or []
            cash_flows.extend(row for row in page_cash_flows if isinstance(row, dict))
            details.extend(row for row in page_details if isinstance(row, dict))
            page_count = int(result.get("page_count") or 1)
            if page >= page_count or (not page_cash_flows and not page_details):
                break
            page += 1
        return {"cash_flows": cash_flows, "details": details}


class PerformanceApiClient:
    def __init__(self, client_id: str, client_secret: str, http: JsonHttpClient | None = None):
        self.client_id = client_id.strip()
        self.client_secret = client_secret.strip()
        self.http = http or JsonHttpClient()
        self.access_token = ""

    def authenticate(
        self,
        progress: Callable[[str], None] | None = None,
        cancel_event: threading.Event | None = None,
    ) -> str:
        if not self.client_id or not self.client_secret:
            raise OzonApiError("请先填写 Performance Client ID 和 Client Secret。")
        response = self.http.request(
            "POST",
            PERFORMANCE_BASE_URL + "/api/client/token",
            headers={"Content-Type": "application/json"},
            body={
                "client_id": self.client_id,
                "client_secret": self.client_secret,
                "grant_type": "client_credentials",
            },
            progress=progress,
            cancel_event=cancel_event,
        )
        self.access_token = str((response or {}).get("access_token") or "")
        if not self.access_token:
            raise OzonApiError("Performance API 未返回 Access Token。")
        return self.access_token

    @property
    def headers(self) -> dict[str, str]:
        if not self.access_token:
            self.authenticate()
        return {"Authorization": f"Bearer {self.access_token}", "Content-Type": "application/json"}

    def test_connection(self) -> str:
        self.authenticate()
        self.campaigns()
        return "Performance API 连接成功"

    def campaigns(
        self,
        progress: Callable[[str], None] | None = None,
        cancel_event: threading.Event | None = None,
    ) -> list[dict[str, Any]]:
        response = self.http.request(
            "GET", PERFORMANCE_BASE_URL + "/api/client/campaign", headers=self.headers,
            progress=progress, cancel_event=cancel_event,
        )
        source_rows = response.get("list") if isinstance(response, dict) else response
        if isinstance(source_rows, dict):
            source_rows = source_rows.get("list") or source_rows.get("campaigns") or []
        if not isinstance(source_rows, list):
            source_rows = []
        return [
            {
                "campaign_id": str(row.get("id", "")),
                "name": row.get("title") or row.get("name") or f"活动 {row.get('id', '')}",
                "state": row.get("state", ""),
                "payment_type": row.get("paymentType", ""),
                "budget": _money(row.get("budget") or row.get("dailyBudget") or row.get("weeklyBudget")),
                "date_from": str(row.get("fromDate") or ""),
                "date_to": str(row.get("toDate") or ""),
                "source": "api",
            }
            for row in source_rows
            if row.get("id") is not None
        ]

    def statistics(
        self,
        campaigns: list[dict[str, Any]],
        date_from: str,
        date_to: str,
        progress: Callable[[str], None] | None = None,
        cancel_event: threading.Event | None = None,
        event: EventCallback | None = None,
    ) -> list[dict[str, Any]]:
        relevant_campaigns = [
            row for row in campaigns
            if row.get("campaign_id") and _campaign_overlaps(row, date_from, date_to)
        ]
        campaign_ids = [row["campaign_id"] for row in relevant_campaigns]
        names = {row["campaign_id"]: row.get("name", "") for row in campaigns}
        rows: list[dict[str, Any]] = []
        batches = [campaign_ids[index:index + 10] for index in range(0, len(campaign_ids), 10)]
        pending: list[dict[str, Any]] = []
        _emit(
            event, "INFO", "筛选活动", "完成广告活动筛选",
            {
                "all_campaigns": len(campaigns),
                "matched_campaigns": len(campaign_ids),
                "skipped_campaigns": len(campaigns) - len(campaign_ids),
                "date_from": date_from,
                "date_to": date_to,
                "batch_count": len(batches),
            },
        )
        if not batches:
            _emit(event, "WARNING", "筛选活动", "所选日期范围内没有相关广告活动", {})
            return []

        # 先提交所有批次，让 Ozon 并行生成报告；旧实现逐个等待会随活动数线性变慢。
        for batch_index, batch in enumerate(batches, start=1):
            _raise_if_cancelled(cancel_event)
            if progress:
                progress(
                    f"正在提交广告报告 {batch_index}/{len(batches)}"
                    f"（相关活动 {len(campaign_ids)}/{len(campaigns)}）"
                )
            _emit(
                event, "INFO", "提交报告", f"提交广告报告批次 {batch_index}/{len(batches)}",
                {
                    "batch": batch_index,
                    "batch_total": len(batches),
                    "campaign_count": len(batch),
                    "campaign_ids": batch,
                },
            )
            response = self.http.request(
                "POST",
                PERFORMANCE_BASE_URL + "/api/client/statistics/json",
                headers=self.headers,
                body={"campaigns": batch, "dateFrom": date_from, "dateTo": date_to, "groupBy": "DATE"},
                progress=progress,
                cancel_event=cancel_event,
            )
            report_uuid = str((response or {}).get("UUID") or (response or {}).get("uuid") or "")
            if not report_uuid:
                _emit(
                    event, "ERROR", "提交报告", "Ozon 未返回报告 UUID",
                    {"batch": batch_index, "campaign_ids": batch},
                )
                raise OzonApiError("Performance API 未返回广告统计报告 UUID。")
            pending.append({"uuid": report_uuid, "campaign_ids": batch})
            _emit(
                event, "SUCCESS", "提交报告", "广告报告已提交",
                {
                    "batch": batch_index,
                    "report_uuid": report_uuid,
                    "campaign_count": len(batch),
                },
            )

        started_at = time.monotonic()
        completed = 0
        poll_round = 0
        last_states: dict[str, str] = {}
        while pending and time.monotonic() - started_at < 900:
            _raise_if_cancelled(cancel_event)
            poll_round += 1
            made_progress = False
            for item in list(pending):
                _raise_if_cancelled(cancel_event)
                response = self.http.request(
                    "GET",
                    PERFORMANCE_BASE_URL + "/api/client/statistics/" + urllib.parse.quote(item["uuid"]),
                    headers=self.headers,
                    progress=progress,
                    cancel_event=cancel_event,
                )
                state = str((response or {}).get("state") or (response or {}).get("status") or "").upper()
                previous_state = last_states.get(item["uuid"])
                if state != previous_state or poll_round == 1 or poll_round % 6 == 0:
                    _emit(
                        event, "INFO", "查询状态", "查询广告报告状态",
                        {
                            "poll_round": poll_round,
                            "report_uuid": item["uuid"],
                            "state": state or "UNKNOWN",
                            "campaign_count": len(item["campaign_ids"]),
                            "remaining_batches": len(pending),
                        },
                    )
                    last_states[item["uuid"]] = state
                if state in {"ERROR", "FAILED", "CANCELLED"}:
                    detail = (response or {}).get("error") or (response or {}).get("message") or "未知原因"
                    _emit(
                        event, "ERROR", "查询状态", "Ozon 广告报告生成失败",
                        {"report_uuid": item["uuid"], "state": state, "error": detail},
                    )
                    raise OzonApiError(f"Ozon 广告报告生成失败：{detail}")
                if not _report_ready(response):
                    continue
                completed += 1
                if progress:
                    progress(f"正在下载广告报告 {completed}/{len(batches)}")
                _emit(
                    event, "SUCCESS", "报告就绪", "广告报告生成完成，开始下载",
                    {
                        "report_uuid": item["uuid"],
                        "completed_batches": completed,
                        "batch_total": len(batches),
                    },
                )
                query = urllib.parse.urlencode({"UUID": item["uuid"]})
                report = self.http.request(
                    "GET",
                    PERFORMANCE_BASE_URL + "/api/client/statistics/report?" + query,
                    headers=self.headers,
                    timeout=120,
                    progress=progress,
                    cancel_event=cancel_event,
                )
                default_id = item["campaign_ids"][0] if len(item["campaign_ids"]) == 1 else ""
                parsed_rows = _parse_performance_report(
                    report, names, default_campaign_id=default_id, default_day=date_to
                )
                rows.extend(parsed_rows)
                _emit(
                    event, "SUCCESS", "解析报告", "广告报告下载并解析完成",
                    {
                        "report_uuid": item["uuid"],
                        "parsed_rows": len(parsed_rows),
                        "campaign_count": len(item["campaign_ids"]),
                        "total_parsed_rows": len(rows),
                    },
                )
                pending.remove(item)
                made_progress = True
            if pending:
                elapsed = int(time.monotonic() - started_at)
                if progress:
                    progress(
                        f"广告报告进度 {completed}/{len(batches)}，"
                        f"剩余 {len(pending)} 批，已等待 {elapsed // 60}分{elapsed % 60:02d}秒"
                    )
                _wait(2 if made_progress else 5, cancel_event)
        if pending:
            _emit(
                event, "ERROR", "等待超时", "广告报告等待超过 15 分钟",
                {"pending_batches": len(pending), "report_uuids": [item["uuid"] for item in pending]},
            )
            raise OzonApiError(
                f"仍有 {len(pending)} 批广告报告等待超过 15 分钟，可缩短日期范围后重试。"
            )
        _emit(
            event, "SUCCESS", "广告同步", "所有广告报告处理完成",
            {"batch_count": len(batches), "parsed_rows": len(rows)},
        )
        return rows

    def product_statistics_by_sku(
        self,
        campaigns: list[dict[str, Any]],
        date_from: str,
        date_to: str,
        progress: Callable[[str], None] | None = None,
        cancel_event: threading.Event | None = None,
        event: EventCallback | None = None,
    ) -> list[dict[str, Any]]:
        """Return the official product-level CPC statistics.

        Unlike ``statistics/daily/json`` this endpoint includes the exact Ozon
        SKU and date, so its expense must not be allocated from campaign totals.
        """
        relevant = [
            row for row in campaigns
            if row.get("campaign_id") and _campaign_overlaps(row, date_from, date_to)
        ]
        names = {str(row.get("campaign_id") or ""): str(row.get("name") or "")
                 for row in campaigns}
        ids = [str(row.get("campaign_id") or "") for row in relevant]
        if not ids:
            return []
        result_rows: list[dict[str, Any]] = []
        # Keep request bodies bounded for shops with hundreds of campaigns.
        batches = [ids[index:index + 100] for index in range(0, len(ids), 100)]
        for index, batch in enumerate(batches, start=1):
            _raise_if_cancelled(cancel_event)
            if progress:
                progress(f"正在读取商品广告明细 {index}/{len(batches)}")
            payload = self.http.request(
                "POST",
                PERFORMANCE_BASE_URL + "/api/client/statistics/products/sku",
                headers=self.headers,
                body={"campaignIds": batch, "dateFrom": date_from, "dateTo": date_to},
                progress=progress,
                cancel_event=cancel_event,
            )
            source = payload.get("rows") if isinstance(payload, dict) else payload
            if isinstance(source, dict):
                source = source.get("rows") or source.get("items") or source.get("list") or []
            if not isinstance(source, list):
                source = []
            for raw in source:
                if not isinstance(raw, dict):
                    continue
                campaign_id = str(raw.get("campaignId") or raw.get("campaign_id") or "")
                sku = str(raw.get("sku") or "").strip()
                day = str(raw.get("date") or raw.get("day") or date_to)[:10]
                if not campaign_id or not sku or not day:
                    continue
                result_rows.append({
                    "day": day,
                    "campaign_id": campaign_id,
                    "campaign_name": names.get(campaign_id, ""),
                    "sku": sku,
                    "impressions": _money(raw.get("views")),
                    "clicks": _money(raw.get("clicks")),
                    "cart_adds": _money(raw.get("toCart")),
                    "orders": _money(raw.get("modelOrders") if raw.get("modelOrders") is not None else raw.get("orders")),
                    "revenue": _money(raw.get("modelSales") if raw.get("modelSales") is not None else raw.get("sales")),
                    "spend": _money(raw.get("expense")),
                    "source": "api_product_sku",
                })
            _emit(event, "SUCCESS", "商品广告", "已读取商品 SKU 广告明细", {
                "batch": index, "batch_total": len(batches), "campaigns": len(batch),
                "rows": len(source), "endpoint": "/api/client/statistics/products/sku",
            })
        return _merge_ad_rows(result_rows)

    def statistics_daily(
        self,
        campaigns: list[dict[str, Any]],
        date_from: str,
        date_to: str,
        progress: Callable[[str], None] | None = None,
        cancel_event: threading.Event | None = None,
        event: EventCallback | None = None,
    ) -> list[dict[str, Any]]:
        """快速读取按活动/日期汇总的统计，不创建异步报告。"""
        names = {row["campaign_id"]: row.get("name", "") for row in campaigns}
        matched = [
            row for row in campaigns
            if row.get("campaign_id") and _campaign_overlaps(row, date_from, date_to)
        ]
        # campaignIds 是可选参数。不拼接数百个活动 ID，可以保持 URL 很短并一次读取账号下
        # 全部活动；这正是该接口相对于异步分批报告的速度优势。
        query = urllib.parse.urlencode({"dateFrom": date_from, "dateTo": date_to})
        url = PERFORMANCE_BASE_URL + "/api/client/statistics/daily/json?" + query
        if progress:
            progress("正在通过即时接口读取广告每日统计…")
        _emit(
            event, "INFO", "快速统计", "请求广告每日统计即时接口",
            {
                "endpoint": "/api/client/statistics/daily/json",
                "date_from": date_from,
                "date_to": date_to,
                "all_campaigns": len(campaigns),
                "matched_campaigns": len(matched),
                "campaign_filter": "all",
            },
        )
        started_at = time.monotonic()
        response = self.http.request(
            "GET", url, headers=self.headers, timeout=120,
            max_attempts=2,
            progress=progress, cancel_event=cancel_event,
        )
        elapsed = round(time.monotonic() - started_at, 2)
        raw_rows = _extract_json_rows(response)
        parsed_rows = _parse_performance_report(response, names, default_day=date_to)
        parsed_valid_rows = [
            row for row in parsed_rows if row.get("campaign_id") and row.get("day")
        ]
        dropped_rows = [
            row for row in parsed_rows if not row.get("campaign_id") or not row.get("day")
        ]
        valid_rows = _merge_ad_rows(parsed_valid_rows)
        raw_keys = sorted({str(key) for row in raw_rows[:50] for key in row.keys()})
        if dropped_rows:
            missing_campaign = sum(not row.get("campaign_id") for row in dropped_rows)
            missing_day = sum(not row.get("day") for row in dropped_rows)
            _emit(
                event, "WARNING", "字段校验", "部分广告统计缺少必要字段",
                {
                    "dropped_rows": len(dropped_rows),
                    "missing_campaign_id": missing_campaign,
                    "missing_day": missing_day,
                    "raw_row_keys": raw_keys,
                    "raw_sample_rows": raw_rows[:3],
                    "normalized_dropped_samples": dropped_rows[:3],
                },
            )
        _emit(
            event, "SUCCESS", "快速统计", "广告每日统计获取并解析完成",
            {
                "elapsed_seconds": elapsed,
                "response_type": type(response).__name__,
                "raw_rows": len(raw_rows),
                "parsed_rows": len(parsed_rows),
                "merged_rows": len(valid_rows),
                "valid_rows": len(valid_rows),
                "dropped_rows": len(dropped_rows),
                "response_keys": list(response.keys()) if isinstance(response, dict) else [],
                "raw_row_keys": raw_keys,
                "raw_sample_rows": raw_rows[:3],
                "sample_rows": valid_rows[:5],
            },
        )
        if raw_rows and not valid_rows:
            _emit(
                event, "ERROR", "字段校验", "Ozon 已返回数据，但没有可写入的记录",
                {
                    "raw_rows": len(raw_rows),
                    "raw_row_keys": raw_keys,
                    "raw_sample_rows": raw_rows[:3],
                },
            )
            raise OzonApiError(
                f"Ozon 返回了 {len(raw_rows)} 条广告统计，但字段结构无法识别；"
                "请复制本任务的运行明细用于校准，数据未写入。"
            )
        return valid_rows

    def _wait_report(
        self,
        report_uuid: str,
        progress: Callable[[str], None] | None,
        cancel_event: threading.Event | None = None,
    ) -> Any:
        # 广告报告为异步任务。大型店铺可能需要数分钟，因此最多等待 15 分钟。
        max_attempts = 180
        for attempt in range(max_attempts):
            _raise_if_cancelled(cancel_event)
            if progress:
                elapsed = attempt * 5
                progress(f"等待广告报告生成（已等待 {elapsed // 60}分{elapsed % 60:02d}秒，可取消）")
            response = self.http.request(
                "GET",
                PERFORMANCE_BASE_URL + "/api/client/statistics/" + urllib.parse.quote(report_uuid),
                headers=self.headers,
                progress=progress,
                cancel_event=cancel_event,
            )
            state = str((response or {}).get("state") or (response or {}).get("status") or "").upper()
            if state in {"ERROR", "FAILED", "CANCELLED"}:
                detail = (response or {}).get("error") or (response or {}).get("message") or "未知原因"
                raise OzonApiError(f"Ozon 广告报告生成失败：{detail}")
            if _report_ready(response):
                if progress:
                    progress("广告报告已生成，正在下载…")
                query = urllib.parse.urlencode({"UUID": report_uuid})
                return self.http.request(
                    "GET",
                    PERFORMANCE_BASE_URL + "/api/client/statistics/report?" + query,
                    headers=self.headers,
                    timeout=120,
                    progress=progress,
                    cancel_event=cancel_event,
                )
            _wait(5, cancel_event)
        raise OzonApiError("广告统计报告等待超过 15 分钟。可缩短日期范围后重试。")


def _read_dimensions(dimensions: list[dict[str, Any]]) -> tuple[str, str, str]:
    sku = ""
    name = ""
    day = ""
    for dimension in dimensions:
        key = str(dimension.get("key") or dimension.get("id") or "").lower()
        value = dimension.get("id") or dimension.get("value") or dimension.get("name") or ""
        if "day" in key or _looks_like_date(str(value)):
            day = str(value)[:10]
        elif "sku" in key or (not sku and str(value).isdigit()):
            sku = str(value)
            name = str(dimension.get("name") or name)
    if not sku and dimensions:
        sku = str(dimensions[0].get("id") or dimensions[0].get("value") or "")
        name = str(dimensions[0].get("name") or "")
    return sku, name, day


def _utc_range(date_from: str, date_to: str) -> tuple[str, str]:
    return f"{date_from}T00:00:00Z", f"{date_to}T23:59:59Z"


def _iso_day(value: Any) -> str:
    text = str(value or "")
    return text[:10] if len(text) >= 10 else text


def _parse_performance_report(
    report: Any,
    names: dict[str, str],
    default_campaign_id: str = "",
    default_day: str = "",
) -> list[dict[str, Any]]:
    if isinstance(report, list):
        rows = [_normalize_ad_row(row, names) for row in report if isinstance(row, dict)]
        return _apply_ad_defaults(rows, names, default_campaign_id, default_day)
    if isinstance(report, dict):
        for key in ("rows", "data", "report", "result"):
            value = report.get(key)
            if isinstance(value, list):
                rows = [_normalize_ad_row(row, names) for row in value if isinstance(row, dict)]
                return _apply_ad_defaults(rows, names, default_campaign_id, default_day)
            if isinstance(value, dict):
                nested_rows = _parse_performance_report(
                    value, names, default_campaign_id, default_day
                )
                if nested_rows:
                    return nested_rows
            if isinstance(value, str) and value.strip():
                return _parse_performance_report(
                    value, names, default_campaign_id, default_day
                )
        if all(key in report for key in ("campaignId", "date")):
            return _apply_ad_defaults(
                [_normalize_ad_row(report, names)], names, default_campaign_id, default_day
            )
        return []

    if not isinstance(report, str):
        return []
    text = report.lstrip("\ufeff").strip()
    if not text:
        return []
    try:
        return _parse_performance_report(
            json.loads(text), names, default_campaign_id, default_day
        )
    except json.JSONDecodeError:
        pass
    return _parse_performance_csv(text, names, default_campaign_id, default_day)


def _extract_json_rows(report: Any) -> list[dict[str, Any]]:
    """提取原始 JSON 行，仅用于字段诊断日志，不修改返回数据。"""
    if isinstance(report, list):
        return [row for row in report if isinstance(row, dict)]
    if isinstance(report, dict):
        for key in ("rows", "data", "report", "result"):
            value = report.get(key)
            if isinstance(value, list):
                return [row for row in value if isinstance(row, dict)]
            if isinstance(value, dict):
                nested = _extract_json_rows(value)
                if nested:
                    return nested
    return []


def _merge_ad_rows(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """合并同一日期/活动/SKU 的重复行，避免数据库 UPSERT 后只保留最后一条。"""
    merged: dict[tuple[str, str, str], dict[str, Any]] = {}
    for row in rows:
        key = (
            str(row.get("day") or ""),
            str(row.get("campaign_id") or ""),
            str(row.get("sku") or ""),
        )
        current = merged.get(key)
        if current is None:
            merged[key] = row.copy()
            continue
        if not current.get("campaign_name") and row.get("campaign_name"):
            current["campaign_name"] = row["campaign_name"]
        for field in ("impressions", "clicks", "cart_adds", "orders", "revenue", "spend"):
            current[field] = float(current.get(field) or 0) + float(row.get(field) or 0)
    return list(merged.values())


def _parse_performance_csv(
    text: str,
    names: dict[str, str],
    default_campaign_id: str,
    default_day: str,
) -> list[dict[str, Any]]:
    lines = [line for line in text.splitlines() if line.strip()]
    rows: list[dict[str, Any]] = []
    current_campaign_id = default_campaign_id
    index = 0
    while index < len(lines):
        line = lines[index]
        campaign_match = re.search(r"№\s*(\d+)", line)
        if campaign_match:
            current_campaign_id = campaign_match.group(1)
            index += 1
            continue
        delimiter = ";" if line.count(";") >= line.count(",") else ","
        normalized_header = _norm_key(line)
        if delimiter in line and ("sku" in normalized_header or "показы" in normalized_header or "impressions" in normalized_header):
            section = [line]
            index += 1
            while index < len(lines):
                candidate = lines[index]
                if re.search(r"№\s*(\d+)", candidate):
                    break
                candidate_norm = _norm_key(candidate)
                if delimiter in candidate and ("sku" in candidate_norm or "показы" in candidate_norm or "impressions" in candidate_norm):
                    break
                section.append(candidate)
                index += 1
            reader = csv.DictReader(io.StringIO("\n".join(section)), delimiter=delimiter)
            parsed = [_normalize_ad_row(item, names) for item in reader]
            rows.extend(
                _apply_ad_defaults(parsed, names, current_campaign_id, default_day)
            )
            continue
        index += 1
    return rows


def _apply_ad_defaults(
    rows: list[dict[str, Any]],
    names: dict[str, str],
    campaign_id: str,
    day: str,
) -> list[dict[str, Any]]:
    for row in rows:
        if not row.get("campaign_id"):
            row["campaign_id"] = campaign_id
        if not row.get("campaign_name"):
            row["campaign_name"] = names.get(str(row.get("campaign_id", "")), "")
        if not row.get("day"):
            row["day"] = day
    return rows


def _normalize_ad_row(row: dict[str, Any], names: dict[str, str]) -> dict[str, Any]:
    normalized = {_norm_key(str(key)): value for key, value in row.items()}
    campaign_value = _first(
        normalized, "campaignid", "campaignids", "campaign", "кампания", "idкампании"
    )
    campaign_name = ""
    if isinstance(campaign_value, dict):
        nested = {_norm_key(str(key)): value for key, value in campaign_value.items()}
        campaign_id = str(_first(nested, "campaignid", "id", "value"))
        campaign_name = str(_first(nested, "campaignname", "title", "name", "label"))
    else:
        campaign_id = str(campaign_value or "")

    # /statistics/daily/json 的真实响应使用 id/title；异步商品报告则使用
    # campaignId。仅在活动日报行中将通用 id 作为活动 ID。
    if not campaign_id:
        campaign_id = str(_first(normalized, "id", "campaignnumber", "campaignnumberid"))
    explicit_name = _first(
        normalized, "campaignname", "campaigntitle", "title", "name", "названиекампании"
    )
    if explicit_name:
        campaign_name = str(explicit_name)
    if not campaign_id and campaign_name:
        matching_ids = [key for key, value in names.items() if value == campaign_name]
        if len(matching_ids) == 1:
            campaign_id = matching_ids[0]
    day = str(_first(normalized, "date", "day", "дата"))[:10]
    return {
        "day": day,
        "campaign_id": campaign_id,
        "campaign_name": campaign_name or names.get(campaign_id, ""),
        "sku": str(_first(normalized, "sku", "озонid", "ozonid")),
        "impressions": _value(normalized, "impressions", "views", "shows", "показы"),
        "clicks": _value(normalized, "clicks", "клики"),
        "cart_adds": _value(normalized, "cartadds", "addtocart", "tocart", "вкорзину"),
        "orders": _value(normalized, "orders", "заказы", "modelorders", "заказымодели"),
        "revenue": _value(
            normalized, "revenue", "ordersmoney", "ordermoney", "sales", "modelsales",
            "выручка", "выручкар", "modelrevenue",
            "выручкасзаказовмодели", "выручкасзаказовмоделир",
        ),
        "spend": _value(
            normalized, "spend", "moneyspent", "expense", "cost", "расход", "расходсндс",
            "расходрсндс", "расходр",
        ),
        "source": "api",
    }


def _report_ready(response: Any) -> bool:
    if isinstance(response, str):
        return bool(response.strip())
    if not isinstance(response, dict):
        return False
    state = str(response.get("state") or response.get("status") or "").upper()
    if state in {"OK", "READY", "SUCCESS", "COMPLETED", "DONE"}:
        return True
    return any(isinstance(response.get(key), (list, str)) and response.get(key) for key in ("rows", "data", "report", "result"))


def _first(row: dict[str, Any], *keys: str) -> Any:
    for key in keys:
        if key in row and row[key] not in (None, ""):
            return row[key]
    return ""


def _value(row: dict[str, Any], *keys: str) -> float:
    return _money(_first(row, *keys))


def _norm_key(value: str) -> str:
    return "".join(char.lower() for char in value if char.isalnum())


def _money(value: Any) -> float:
    if value in (None, ""):
        return 0.0
    text = str(value).replace("\xa0", "").replace(" ", "").replace("₽", "").replace("%", "")
    if text.count(",") == 1 and text.count(".") == 0:
        text = text.replace(",", ".")
    else:
        text = text.replace(",", "")
    try:
        return float(text)
    except ValueError:
        return 0.0


def _extract_error_message(payload: str) -> str:
    try:
        data = json.loads(payload)
        message = data.get("message") or data.get("error") or data.get("details") or ""
        return f" {message}" if message else ""
    except (json.JSONDecodeError, AttributeError):
        return f" {payload[:240]}" if payload else ""


def _looks_like_date(value: str) -> bool:
    try:
        date.fromisoformat(value[:10])
        return True
    except ValueError:
        return False


def _next_month(value: date) -> date:
    if value.month == 12:
        return date(value.year + 1, 1, 1)
    return date(value.year, value.month + 1, 1)


def _campaign_overlaps(campaign: dict[str, Any], date_from: str, date_to: str) -> bool:
    campaign_from = str(campaign.get("date_from") or "")[:10]
    campaign_to = str(campaign.get("date_to") or "")[:10]
    if campaign_from and campaign_from > date_to:
        return False
    if campaign_to and campaign_to < date_from:
        return False
    return True


def _retry_after_seconds(headers: Any, default: int) -> int:
    try:
        value = headers.get("Retry-After") if headers else None
        return max(1, min(300, int(value))) if value else default
    except (TypeError, ValueError):
        return default


def _raise_if_cancelled(cancel_event: threading.Event | None) -> None:
    if cancel_event is not None and cancel_event.is_set():
        raise SyncCancelled("同步已由用户取消。")


def _wait(seconds: float, cancel_event: threading.Event | None = None) -> None:
    if cancel_event is None:
        time.sleep(seconds)
        return
    if cancel_event.wait(seconds):
        raise SyncCancelled("同步已由用户取消。")


def _wait_for_rate_limit(
    key: str,
    interval: float,
    progress: Callable[[str], None] | None,
    cancel_event: threading.Event | None,
) -> None:
    with _RATE_LOCK:
        last_at = _LAST_REQUEST_AT.get(key, 0.0)
    remaining = interval - (time.monotonic() - last_at)
    if remaining > 0:
        seconds = int(remaining + 0.999)
        if progress:
            progress(f"遵循 Ozon 接口限频，{seconds} 秒后继续请求销量分析")
        _wait(remaining, cancel_event)


def _mark_rate_limit(key: str) -> None:
    with _RATE_LOCK:
        _LAST_REQUEST_AT[key] = time.monotonic()


def _emit(
    callback: EventCallback | None,
    level: str,
    stage: str,
    message: str,
    details: dict[str, Any] | None = None,
) -> None:
    if callback:
        callback(level, stage, message, details or {})
