from __future__ import annotations

from dataclasses import dataclass
from datetime import date, datetime, timedelta, timezone
import threading
import time
from typing import Any, Callable

from .ai import OpenAICompatibleClient
from .api import OzonApiError, PerformanceApiClient, SellerApiClient, SyncCancelled, SyncResult
from .database import Database
from .feishu import FeishuClient, FeishuError, field_number, field_text
from .secrets import protect, unprotect


SECRET_KEYS = {
    "seller_api_key", "performance_client_secret", "ai_api_key", "feishu_app_secret",
}
LogListener = Callable[[int, str, str, str, dict[str, Any]], None]


@dataclass
class Credentials:
    seller_client_id: str = ""
    seller_api_key: str = ""
    performance_client_id: str = ""
    performance_client_secret: str = ""
    ai_base_url: str = "https://api.gpt.ge/v1"
    ai_api_key: str = ""
    ai_model: str = "gpt-5.6-terra"
    feishu_base_url: str = "https://open.feishu.cn/open-apis"
    feishu_app_id: str = ""
    feishu_app_secret: str = ""
    feishu_app_token: str = ""
    feishu_product_table_id: str = ""
    feishu_weekly_table_id: str = ""
    feishu_tracking_table_id: str = ""
    feishu_series_table_id: str = ""
    feishu_chat_id: str = ""


class AppService:
    def __init__(
        self,
        database: Database,
        *,
        shop_name: str = "",
        shop_kind: str = "local",
    ):
        self.database = database
        self.shop_name = str(shop_name or "").strip() or "未命名店铺"
        self.shop_kind = "cross_border" if shop_kind == "cross_border" else "local"

    @property
    def shop_kind_label(self) -> str:
        return "跨境店" if self.shop_kind == "cross_border" else "本土店"

    def load_credentials(self) -> Credentials:
        values: dict[str, str] = {}
        defaults = Credentials()
        for key in Credentials.__dataclass_fields__:
            # Preserve dataclass defaults for settings introduced in newer
            # versions.  This lets an existing local database immediately use
            # the default relay URL/model without a migration or forced save.
            raw = self.database.get_setting(key, getattr(defaults, key))
            if key in SECRET_KEYS:
                try:
                    raw = unprotect(raw)
                except Exception:
                    raw = ""
            values[key] = raw
        return Credentials(**values)

    def save_credentials(self, credentials: Credentials) -> None:
        values = credentials.__dict__.copy()
        for key in SECRET_KEYS:
            values[key] = protect(values[key])
        self.database.save_settings(values)

    def seller_client(self, credentials: Credentials | None = None) -> SellerApiClient:
        credentials = credentials or self.load_credentials()
        return SellerApiClient(credentials.seller_client_id, credentials.seller_api_key)

    def performance_client(self, credentials: Credentials | None = None) -> PerformanceApiClient:
        credentials = credentials or self.load_credentials()
        return PerformanceApiClient(
            credentials.performance_client_id, credentials.performance_client_secret
        )

    def ai_client(self, credentials: Credentials | None = None) -> OpenAICompatibleClient:
        credentials = credentials or self.load_credentials()
        return OpenAICompatibleClient(
            base_url=credentials.ai_base_url,
            api_key=credentials.ai_api_key,
            model=credentials.ai_model,
        )

    def feishu_client(self, credentials: Credentials | None = None) -> FeishuClient:
        credentials = credentials or self.load_credentials()
        return FeishuClient(
            credentials.feishu_app_id,
            credentials.feishu_app_secret,
            base_url=credentials.feishu_base_url,
        )

    def test_seller(self, credentials: Credentials) -> str:
        return self.seller_client(credentials).test_connection()

    def test_performance(self, credentials: Credentials) -> str:
        return self.performance_client(credentials).test_connection()

    def test_ai(self, credentials: Credentials) -> str:
        return self.ai_client(credentials).test_connection()

    def test_feishu(self, credentials: Credentials) -> str:
        table_id = next((value for value in (
            credentials.feishu_product_table_id,
            credentials.feishu_weekly_table_id,
            credentials.feishu_tracking_table_id,
            credentials.feishu_series_table_id,
        ) if value), "")
        return self.feishu_client(credentials).test_connection(
            credentials.feishu_app_token,
            table_id,
        )

    def sync_inventory(
        self,
        sku: str | None = None,
        progress: Callable[[str], None] | None = None,
        cancel_event: threading.Event | None = None,
        event_listener: LogListener | None = None,
    ) -> dict[str, Any]:
        """Refresh stock, cluster and FBO shipping-address directories."""
        products = self.database.product_sync_rows()
        numeric_skus = [
            str(row.get("sku") or "") for row in products
            if str(row.get("sku") or "").isdigit()
        ]
        if sku:
            numeric_skus = [str(sku)] if str(sku).isdigit() else []
        if not numeric_skus:
            raise ValueError("没有可同步的 Ozon SKU；请先在数据中心同步 Seller 商品目录。")

        client = self.seller_client()
        log_id = self.database.start_log("Seller Inventory")
        emit, tracked_progress = self._log_callbacks(log_id, progress, event_listener)
        try:
            emit("INFO", "开始", "开始读取库存、集群与 FBO 发货地址", {
                "stock_endpoint": "/v1/analytics/stocks",
                "cluster_endpoint": "/v2/cluster/list",
                "warehouse_endpoint": "/v1/warehouse/fbo/seller/list",
                "sku_count": len(numeric_skus),
                "scope": "selected_product" if sku else "all_products",
            })
            stock_items = client.inventory_stocks(
                numeric_skus, progress=tracked_progress, cancel_event=cancel_event
            )
            clusters = client.fbo_clusters(
                progress=tracked_progress, cancel_event=cancel_event
            )
            warehouses = client.seller_supply_warehouses(
                progress=tracked_progress, cancel_event=cancel_event
            )
            stock_rows = []
            for item in stock_items:
                stock_rows.append({
                    "sku": str(item.get("sku") or ""),
                    "offer_id": str(item.get("offer_id") or ""),
                    "product_name": str(item.get("name") or ""),
                    "warehouse_id": str(item.get("warehouse_id") or ""),
                    "warehouse_name": str(item.get("warehouse_name") or ""),
                    "cluster_id": str(item.get("cluster_id") or ""),
                    "cluster_name": str(item.get("cluster_name") or ""),
                    "macrolocal_cluster_id": str(item.get("macrolocal_cluster_id") or ""),
                    "ads": item.get("ads") or 0,
                    "ads_cluster": item.get("ads_cluster") or 0,
                    "available_stock": item.get("available_stock_count") or 0,
                    "valid_stock": item.get("valid_stock_count") or 0,
                    "requested_stock": item.get("requested_stock_count") or 0,
                    "transit_stock": item.get("transit_stock_count") or 0,
                    "days_without_sales": item.get("days_without_sales") or 0,
                    "idc_cluster": item.get("idc_cluster") or 0,
                    "turnover_grade_cluster": item.get("turnover_grade_cluster") or "",
                })
            stock_count = self.database.replace_inventory_stock(
                stock_rows, replace_skus=numeric_skus if sku else None
            )
            write_metadata = dict(self.database.last_inventory_write_metadata)
            if (
                write_metadata.get("duplicate_rows_merged")
                or write_metadata.get("fallback_warehouse_ids")
                or write_metadata.get("invalid_rows_skipped")
            ):
                emit(
                    "WARNING", "库存去重", "库存接口重复/缺失仓库标识的记录已安全合并",
                    write_metadata,
                )
            cluster_count = self.database.replace_ozon_clusters(clusters)
            warehouse_count = self.database.replace_seller_supply_warehouses(warehouses)
            result = {
                "stock_rows": stock_count, "clusters": cluster_count,
                "warehouses": warehouse_count, "log_id": log_id,
                "inventory_write": write_metadata,
            }
            emit("SUCCESS", "写入数据库", "库存与补货基础数据已保存", result)
            self.database.finish_log(
                log_id, "success", stock_count,
                f"库存 {stock_count} 行，集群 {cluster_count} 个，发货地址 {warehouse_count} 个",
            )
            return result
        except Exception as error:
            emit("ERROR", "同步失败", str(error), {"error_type": type(error).__name__})
            self.database.finish_log(log_id, "failed", 0, str(error))
            raise

    def supply_appointments(
        self,
        progress: Callable[[str], None] | None = None,
        cancel_event: threading.Event | None = None,
        event_listener: LogListener | None = None,
    ) -> dict[str, Any]:
        client = self.seller_client()
        log_id = self.database.start_log("FBO Supply Orders")
        emit, tracked_progress = self._log_callbacks(log_id, progress, event_listener)
        try:
            emit("INFO", "开始", "开始读取 FBO 供应单和越库信息", {
                "list_endpoint": "/v3/supply-order/list",
                "detail_endpoint": "/v3/supply-order/get",
                "states": [1, 2, 3, 4, 5, 6, 7],
            })
            orders = client.supply_orders(
                progress=tracked_progress, cancel_event=cancel_event
            )
            rows = [_normalize_supply_order(item) for item in orders]
            rows.sort(key=lambda item: str(item.get("created_date") or ""), reverse=True)
            emit("SUCCESS", "解析结果", "供应单与越库集群解析完成", {
                "orders": len(rows),
                "crossdock_orders": sum(1 for item in rows if item["is_crossdock"]),
                "clusters": len({
                    cluster for item in rows for cluster in item.get("cluster_ids", [])
                }),
            })
            self.database.finish_log(
                log_id, "success", len(rows), f"读取 {len(rows)} 个活动供应单"
            )
            return {"orders": rows, "log_id": log_id}
        except Exception as error:
            emit("ERROR", "失败", "读取供应单失败", {"error": str(error)})
            self.database.finish_log(log_id, "failed", 0, str(error))
            raise

    def supply_timeslots(
        self,
        order_id: int,
        date_from: str,
        date_to: str,
        timezone_offset_seconds: int = 0,
        progress: Callable[[str], None] | None = None,
        cancel_event: threading.Event | None = None,
        event_listener: LogListener | None = None,
    ) -> dict[str, Any]:
        start = date.fromisoformat(date_from)
        end = date.fromisoformat(date_to)
        if start > end:
            raise ValueError("时间窗开始日期不能晚于结束日期。")
        if (end - start).days > 60:
            raise ValueError("单次最多查询 60 天的约仓时间窗。")
        client = self.seller_client()
        log_id = self.database.start_log("FBO Timeslots")
        emit, tracked_progress = self._log_callbacks(log_id, progress, event_listener)
        try:
            emit("INFO", "开始", "开始查询可用约仓时间窗", {
                "order_id": int(order_id), "date_from": date_from, "date_to": date_to,
                "endpoint": "/v2/supply-order/timeslot/list",
            })
            values = client.supply_order_timeslots(
                order_id, date_from, date_to,
                progress=tracked_progress, cancel_event=cancel_event,
            )
            rows = []
            for item in values:
                local_from = _supply_local_datetime(
                    item["from"], timezone_offset_seconds
                )
                local_to = _supply_local_datetime(item["to"], timezone_offset_seconds)
                rows.append({
                    "from": item["from"], "to": item["to"],
                    "local_from": local_from, "local_to": local_to,
                    "local_day": local_from[:10],
                    "local_time": f"{local_from[11:16]}–{local_to[11:16]}",
                })
            rows.sort(key=lambda item: item["from"])
            emit("SUCCESS", "返回结果", "可用时间窗查询完成", {
                "order_id": int(order_id), "timeslots": len(rows),
                "timezone_offset_seconds": int(timezone_offset_seconds),
            })
            self.database.finish_log(
                log_id, "success", len(rows), f"读取 {len(rows)} 个可用时间窗"
            )
            return {"timeslots": rows, "log_id": log_id}
        except Exception as error:
            emit("ERROR", "失败", "查询约仓时间窗失败", {"error": str(error)})
            self.database.finish_log(log_id, "failed", 0, str(error))
            raise

    def book_supply_timeslot(
        self,
        supply_order_id: int,
        timeslot_from: str,
        timeslot_to: str,
        event_listener: LogListener | None = None,
    ) -> dict[str, Any]:
        """Submit exactly once; ambiguous write responses are never auto-retried."""
        client = self.seller_client()
        log_id = self.database.start_log("FBO Timeslot Update")
        emit, _ = self._log_callbacks(log_id, None, event_listener)
        try:
            emit("WARNING", "提交预约", "用户已确认提交约仓时间窗", {
                "supply_order_id": int(supply_order_id),
                "timeslot_from": timeslot_from, "timeslot_to": timeslot_to,
                "endpoint": "/v1/supply-order/timeslot/update",
                "automatic_retry": False,
            })
            response = client.update_supply_order_timeslot(
                supply_order_id, timeslot_from, timeslot_to
            )
            operation_id = str(
                response.get("operation_id")
                or (response.get("result") or {}).get("operation_id")
                or ""
            )
            emit("SUCCESS", "已受理", "Ozon 已受理约仓时间窗请求", {
                "operation_id": operation_id,
                "response_keys": sorted(response.keys()),
            })
            self.database.finish_log(
                log_id, "success", 1,
                "约仓请求已提交" + (f"，操作 ID {operation_id}" if operation_id else ""),
            )
            return {
                "operation_id": operation_id, "response": response, "log_id": log_id,
            }
        except Exception as error:
            emit("ERROR", "提交失败", "约仓时间窗提交失败", {"error": str(error)})
            self.database.finish_log(log_id, "failed", 0, str(error))
            raise

    def search_supply_dropoffs(
        self,
        search: str,
        supply_type: str,
        progress: Callable[[str], None] | None = None,
        cancel_event: threading.Event | None = None,
        event_listener: LogListener | None = None,
        force_refresh: bool = False,
    ) -> dict[str, Any]:
        log_id = self.database.start_log("FBO Draft Warehouses")
        emit, tracked_progress = self._log_callbacks(log_id, progress, event_listener)
        try:
            cached = self.database.cached_supply_dropoffs(search, supply_type)
            if cached and not force_refresh:
                emit("INFO", "使用缓存", "已从当前店铺的本地缓存读取发货点", {
                    "search": search, "supply_type": supply_type,
                    "warehouses": len(cached), "network_request": False,
                })
                self.database.finish_log(
                    log_id, "success", len(cached), "发货点已从本地缓存读取"
                )
                return {"warehouses": cached, "log_id": log_id, "cached": True}
            client = self.seller_client()
            emit("INFO", "查询发货点", "开始搜索新约仓草稿可用发货点", {
                "endpoint": "/v1/warehouse/fbo/list", "search": search,
                "supply_type": supply_type, "force_refresh": bool(force_refresh),
            })
            rows = client.search_supply_dropoff_warehouses(
                search, supply_type, progress=tracked_progress,
                cancel_event=cancel_event,
            )
            cached_rows = self.database.cache_supply_dropoffs(rows, supply_type)
            emit("SUCCESS", "返回结果", "发货点搜索完成并已写入本店缓存", {
                "warehouses": len(rows), "cached_rows": cached_rows,
            })
            self.database.finish_log(log_id, "success", len(rows), "发货点搜索完成")
            return {"warehouses": rows, "log_id": log_id, "cached": False}
        except Exception as error:
            emit("ERROR", "查询失败", str(error), {"error_type": type(error).__name__})
            self.database.finish_log(log_id, "failed", 0, str(error))
            raise

    def create_supply_draft(
        self,
        sku: str,
        quantity: int,
        macrolocal_cluster_id: int,
        supply_type: str,
        delivery_info: dict[str, Any] | None = None,
        event_listener: LogListener | None = None,
    ) -> dict[str, Any]:
        """Create the remote draft once, then read its current calculation state."""
        client = self.seller_client()
        log_id = self.database.start_log("FBO Draft Create")
        emit, _ = self._log_callbacks(log_id, None, event_listener)
        try:
            endpoint = (
                "/v1/draft/direct/create" if supply_type.upper() == "DIRECT"
                else "/v1/draft/crossdock/create"
            )
            emit("WARNING", "创建草稿", "用户已确认创建新的 Ozon 约仓草稿", {
                "endpoint": endpoint, "sku": sku, "quantity": int(quantity),
                "macrolocal_cluster_id": int(macrolocal_cluster_id),
                "supply_type": supply_type.upper(), "automatic_retry": False,
            })
            response = client.create_supply_draft(
                sku, quantity, macrolocal_cluster_id, supply_type,
                delivery_info=delivery_info,
            )
            draft_id = int(response.get("draft_id") or 0)
            errors = list(response.get("errors") or [])
            if not draft_id:
                raise OzonApiError(
                    "Ozon 未返回草稿 ID。" + _draft_error_text(errors)
                )
            emit("SUCCESS", "草稿已创建", "Ozon 已返回新草稿 ID", {
                "draft_id": draft_id, "errors": errors,
            })
            # This is a read request.  A freshly-created draft may still be
            # calculating, so returning its status is safer than repeating the write.
            time.sleep(0.8)
            info = client.supply_draft_info(draft_id)
            emit("INFO", "草稿校验", "已读取当前草稿计算状态", {
                "draft_id": draft_id, "status": info.get("status"),
                "clusters": len(info.get("clusters") or []),
                "errors": info.get("errors") or [],
            })
            self.database.finish_log(log_id, "success", 1, f"草稿 {draft_id} 已创建")
            return {
                "draft_id": draft_id, "create_response": response,
                "info": info, "log_id": log_id,
            }
        except Exception as error:
            emit("ERROR", "创建失败", str(error), {"error_type": type(error).__name__})
            self.database.finish_log(log_id, "failed", 0, str(error))
            raise

    def supply_draft_info(
        self,
        draft_id: int,
        progress: Callable[[str], None] | None = None,
        cancel_event: threading.Event | None = None,
        event_listener: LogListener | None = None,
    ) -> dict[str, Any]:
        client = self.seller_client()
        log_id = self.database.start_log("FBO Draft Info")
        emit, tracked_progress = self._log_callbacks(log_id, progress, event_listener)
        try:
            emit("INFO", "读取草稿", "开始读取草稿校验结果与目的仓", {
                "endpoint": "/v2/draft/create/info", "draft_id": int(draft_id),
            })
            info = client.supply_draft_info(
                draft_id, progress=tracked_progress, cancel_event=cancel_event,
            )
            clusters = list(info.get("clusters") or [])
            emit("SUCCESS", "返回结果", "草稿校验结果读取完成", {
                "status": info.get("status"), "clusters": len(clusters),
                "errors": info.get("errors") or [],
            })
            self.database.finish_log(log_id, "success", len(clusters), "草稿校验已刷新")
            return {"draft_id": int(draft_id), "info": info, "log_id": log_id}
        except Exception as error:
            emit("ERROR", "读取失败", str(error), {"error_type": type(error).__name__})
            self.database.finish_log(log_id, "failed", 0, str(error))
            raise

    def supply_draft_timeslots(
        self,
        draft_id: int,
        supply_type: str,
        macrolocal_cluster_id: int,
        storage_warehouse_id: int,
        date_from: str,
        date_to: str,
        progress: Callable[[str], None] | None = None,
        cancel_event: threading.Event | None = None,
        event_listener: LogListener | None = None,
    ) -> dict[str, Any]:
        start = date.fromisoformat(date_from)
        end = date.fromisoformat(date_to)
        if start > end or (end - start).days > 60:
            raise ValueError("约仓时间范围应为 0–60 天，且开始日期不能晚于结束日期。")
        client = self.seller_client()
        log_id = self.database.start_log("FBO Draft Timeslots")
        emit, tracked_progress = self._log_callbacks(log_id, progress, event_listener)
        try:
            emit("INFO", "查询时段", "开始查询新草稿可用约仓时段", {
                "endpoint": "/v2/draft/timeslot/info", "draft_id": int(draft_id),
                "macrolocal_cluster_id": int(macrolocal_cluster_id),
                "storage_warehouse_id": int(storage_warehouse_id),
                "date_from": date_from, "date_to": date_to,
            })
            response = client.supply_draft_timeslots(
                draft_id, supply_type, macrolocal_cluster_id, storage_warehouse_id,
                date_from, date_to, progress=tracked_progress,
                cancel_event=cancel_event,
            )
            result = response.get("result") or {}
            warehouse_timeslots = result.get("drop_off_warehouse_timeslots") or {}
            rows: list[dict[str, str]] = []
            for day in warehouse_timeslots.get("days") or []:
                for slot in day.get("timeslots") or []:
                    start_at = str(slot.get("from_in_timezone") or "")
                    end_at = str(slot.get("to_in_timezone") or "")
                    if start_at and end_at:
                        rows.append({
                            "day": str(day.get("date_in_timezone") or start_at[:10]),
                            "from": start_at, "to": end_at,
                            "timezone": str(warehouse_timeslots.get("warehouse_timezone") or ""),
                        })
            rows.sort(key=lambda item: item["from"])
            emit("SUCCESS", "返回结果", "新草稿可用时段读取完成", {
                "timeslots": len(rows), "error_reason": response.get("error_reason"),
            })
            self.database.finish_log(log_id, "success", len(rows), "草稿时段查询完成")
            return {"timeslots": rows, "raw": response, "log_id": log_id}
        except Exception as error:
            emit("ERROR", "查询失败", str(error), {"error_type": type(error).__name__})
            self.database.finish_log(log_id, "failed", 0, str(error))
            raise

    def submit_supply_from_draft(
        self,
        draft_id: int,
        supply_type: str,
        macrolocal_cluster_id: int,
        storage_warehouse_id: int,
        timeslot_from: str,
        timeslot_to: str,
        event_listener: LogListener | None = None,
    ) -> dict[str, Any]:
        """Create the real Ozon supply once; never retry an ambiguous write."""
        client = self.seller_client()
        log_id = self.database.start_log("FBO Supply Create")
        emit, _ = self._log_callbacks(log_id, None, event_listener)
        try:
            emit("WARNING", "正式提交", "用户已二次确认创建真实 Ozon 供应申请", {
                "endpoint": "/v2/draft/supply/create", "draft_id": int(draft_id),
                "supply_type": supply_type.upper(),
                "macrolocal_cluster_id": int(macrolocal_cluster_id),
                "storage_warehouse_id": int(storage_warehouse_id),
                "timeslot_from": timeslot_from, "timeslot_to": timeslot_to,
                "automatic_retry": False,
            })
            response = client.create_supply_from_draft(
                draft_id, supply_type, macrolocal_cluster_id,
                storage_warehouse_id, timeslot_from, timeslot_to,
            )
            errors = list(response.get("error_reasons") or [])
            if errors:
                raise OzonApiError("Ozon 拒绝创建供应申请：" + "；".join(map(str, errors)))
            time.sleep(0.8)
            status = client.supply_from_draft_status(draft_id)
            status_errors = list(status.get("error_reasons") or [])
            if status_errors:
                raise OzonApiError(
                    "Ozon 已受理写请求，但生成供应单失败："
                    + "；".join(map(str, status_errors))
                )
            emit("SUCCESS", "已受理", "Ozon 已受理真实供应申请", {
                "draft_id": int(draft_id), "status": status.get("status"),
                "order_id": status.get("order_id"),
                "error_reasons": status.get("error_reasons") or [],
            })
            self.database.finish_log(
                log_id, "success", 1,
                f"供应申请已受理，草稿 {draft_id}，供应单 {status.get('order_id') or '生成中'}",
            )
            return {"response": response, "status": status, "log_id": log_id}
        except Exception as error:
            emit("ERROR", "提交失败", str(error), {"error_type": type(error).__name__})
            self.database.finish_log(log_id, "failed", 0, str(error))
            raise

    def sync_feishu_products(
        self,
        direction: str = "both",
        credentials: Credentials | None = None,
        event_listener: LogListener | None = None,
    ) -> dict[str, Any]:
        """Synchronize product identity/cost fields, using SKU as the stable key."""
        if direction not in {"both", "pull", "push"}:
            raise ValueError("飞书商品同步方向无效。")
        credentials = credentials or self.load_credentials()
        if not credentials.feishu_app_token or not credentials.feishu_product_table_id:
            raise FeishuError("请先填写飞书 App Token 和商品表 Table ID。")
        client = self.feishu_client(credentials)
        log_id = self.database.start_log("Feishu Products")

        def emit(level: str, stage: str, message: str, details: dict[str, Any]) -> None:
            self.database.append_log_event(log_id, level, stage, message, details)
            if event_listener:
                event_listener(log_id, level, stage, message, details)

        try:
            emit("INFO", "开始", "开始同步飞书商品多维表格", {"direction": direction})
            definitions = {
                "SKU": 1, "货号": 1, "商品名称": 1,
                "采购成本 CNY": 2, "头程 RUB": 2,
                # 保留旧字段，以便已有飞书表无需一次性迁移。
                "单件成本": 2, "单件头程": 2,
                "备注": 1, "本地更新时间": 1,
            }
            _, primary_name = client.ensure_fields(
                credentials.feishu_app_token,
                credentials.feishu_product_table_id,
                definitions,
            )
            remote_records = client.list_records(
                credentials.feishu_app_token, credentials.feishu_product_table_id
            )
            remote_by_sku: dict[str, dict[str, Any]] = {}
            duplicate_count = 0
            for record in remote_records:
                fields = record.get("fields") or {}
                sku = field_text(fields.get("SKU"))
                if not sku and primary_name:
                    sku = field_text(fields.get(primary_name))
                if not sku:
                    continue
                if sku in remote_by_sku:
                    duplicate_count += 1
                    continue
                remote_by_sku[sku] = record
            emit(
                "INFO", "读取飞书", "已读取飞书商品记录",
                {"records": len(remote_records), "keyed_records": len(remote_by_sku),
                 "duplicate_skus": duplicate_count},
            )

            pulled = 0
            partial = 0
            if direction in {"both", "pull"}:
                local_before = {
                    str(row["sku"]): row for row in self.database.product_sync_rows()
                }
                for sku, record in remote_by_sku.items():
                    fields = record.get("fields") or {}
                    remote_unit_cny = field_number(fields.get("采购成本 CNY"))
                    remote_first_rub = field_number(fields.get("头程 RUB"))
                    remote_unit = field_number(fields.get("单件成本"))
                    remote_first = field_number(fields.get("单件头程"))
                    if (
                        remote_unit_cny is None and remote_first_rub is None
                        and remote_unit is None and remote_first is None
                    ):
                        continue
                    previous = local_before.get(sku) or {}
                    if remote_unit_cny is not None or remote_first_rub is not None:
                        unit_cost_cny = (
                            remote_unit_cny if remote_unit_cny is not None
                            else previous.get("unit_cost_cny")
                        )
                        first_mile_rub = (
                            remote_first_rub if remote_first_rub is not None
                            else previous.get("first_mile_cost")
                        )
                        if remote_unit_cny is None or remote_first_rub is None:
                            partial += 1
                        self.database.save_product_cost_mixed(
                            sku, unit_cost_cny, first_mile_rub,
                            length_cm=previous.get("length_cm"),
                            width_cm=previous.get("width_cm"),
                            height_cm=previous.get("height_cm"),
                            weight_kg=previous.get("weight_kg"),
                            note=field_text(fields.get("备注")),
                        )
                    else:
                        # 旧版字段继续沿用 RUB/RUB 口径，避免破坏历史表。
                        unit_cost = (
                            remote_unit if remote_unit is not None
                            else previous.get("unit_cost")
                        )
                        first_mile = (
                            remote_first if remote_first is not None
                            else previous.get("first_mile_cost")
                        )
                        if remote_unit is None or remote_first is None:
                            partial += 1
                        self.database.save_product_cost(
                            sku, unit_cost, first_mile,
                            field_text(fields.get("备注")),
                        )
                    pulled += 1
                emit(
                    "SUCCESS", "写入本地", "飞书商品成本已写入本地",
                    {"pulled_rows": pulled, "partial_cost_rows": partial},
                )

            created: list[dict[str, Any]] = []
            updated: list[dict[str, Any]] = []
            unchanged = 0
            if direction in {"both", "push"}:
                for row in self.database.product_sync_rows():
                    sku = str(row.get("sku") or "").strip()
                    if not sku:
                        continue
                    desired: dict[str, Any] = {
                        "SKU": sku,
                        "货号": str(row.get("offer_id") or ""),
                        "商品名称": str(row.get("product_name") or ""),
                        "备注": str(row.get("note") or ""),
                        "本地更新时间": str(row.get("local_updated_at") or ""),
                    }
                    if row.get("unit_cost") is not None:
                        desired["单件成本"] = float(row["unit_cost"])
                    if row.get("first_mile_cost") is not None:
                        desired["单件头程"] = float(row["first_mile_cost"])
                        desired["头程 RUB"] = float(row["first_mile_cost"])
                    if row.get("unit_cost_cny") is not None:
                        desired["采购成本 CNY"] = float(row["unit_cost_cny"])
                    if primary_name and primary_name not in desired:
                        desired[primary_name] = sku
                    existing = remote_by_sku.get(sku)
                    if existing:
                        if _same_feishu_fields(existing.get("fields") or {}, desired):
                            unchanged += 1
                        else:
                            updated.append({"record_id": existing.get("record_id"), "fields": desired})
                    else:
                        created.append({"fields": desired})
                client.batch_create(
                    credentials.feishu_app_token, credentials.feishu_product_table_id, created
                )
                client.batch_update(
                    credentials.feishu_app_token, credentials.feishu_product_table_id, updated
                )
                emit(
                    "SUCCESS", "写入飞书", "本地商品已增量写入飞书",
                    {"created": len(created), "updated": len(updated),
                     "unchanged": unchanged},
                )
            result = {
                "pulled": pulled, "created": len(created), "updated": len(updated),
                "unchanged": unchanged, "duplicates": duplicate_count, "partial": partial,
                "log_id": log_id,
            }
            affected = pulled + len(created) + len(updated)
            self.database.finish_log(
                log_id, "success", affected,
                f"商品双向同步完成：读取 {pulled}，新增 {len(created)}，更新 {len(updated)}",
            )
            emit("SUCCESS", "结束", "飞书商品同步完成", result)
            return result
        except Exception as error:
            emit("ERROR", "失败", "飞书商品同步失败", {"error": str(error)})
            self.database.finish_log(log_id, "failed", 0, str(error))
            raise

    def sync_feishu_shipments(
        self,
        credentials: Credentials | None = None,
        event_listener: LogListener | None = None,
    ) -> dict[str, Any]:
        """Read shipment rows from Bitable without sending group messages."""
        credentials = credentials or self.load_credentials()
        if not credentials.feishu_app_token or not credentials.feishu_tracking_table_id:
            raise FeishuError("请先填写飞书 App Token 和发货跟踪 Table ID。")
        client = self.feishu_client(credentials)
        log_id = self.database.start_log("Feishu Shipments")

        def emit(level: str, stage: str, message: str, details: dict[str, Any]) -> None:
            self.database.append_log_event(log_id, level, stage, message, details)
            if event_listener:
                event_listener(log_id, level, stage, message, details)

        try:
            emit("INFO", "开始", "开始读取飞书发货跟踪表", {
                "shop_name": self.shop_name,
                "table_id": credentials.feishu_tracking_table_id,
            })
            records = client.list_records(
                credentials.feishu_app_token, credentials.feishu_tracking_table_id,
            )
            rows: list[dict[str, Any]] = []
            for record in records:
                fields = record.get("fields") or {}
                tracking_id = _first_feishu_text(
                    fields, "采购单号", "跟踪编号", "发货单号", "批次号",
                )
                if not tracking_id:
                    continue
                quantity = _first_feishu_number(fields, "数量", "发货数量")
                rows.append({
                    "tracking_id": tracking_id,
                    "product_name": _first_feishu_text(fields, "品名", "产品名称", "商品名称"),
                    "batch_no": _first_feishu_text(fields, "批次号", "批次"),
                    "shop_name": _first_feishu_text(fields, "店铺", "店铺名称") or self.shop_name,
                    "quantity": int(quantity or 0),
                    "cargo_status": _first_feishu_text(fields, "货物状态", "状态"),
                    "channel": _first_feishu_text(fields, "渠道", "物流渠道"),
                    "domestic_arrival": _first_feishu_date(fields, "国内到库", "国内到仓"),
                    "foreign_arrival": _first_feishu_date(fields, "国外到库", "国外到仓"),
                    "remote_record_id": str(record.get("record_id") or ""),
                })
            arrivals = self.database.upsert_shipment_tracking(rows, source="feishu")
            emit("SUCCESS", "本地缓存", "发货跟踪表已写入本地缓存", {
                "remote_records": len(records), "valid_rows": len(rows),
                "new_arrivals": len(arrivals),
            })
            notifications = 0
            if arrivals:
                emit("INFO", "待人工确认", "发现到库变化，仅更新本地缓存，不自动发群", {
                    "pending_notifications": len(arrivals),
                })
            result = {
                "rows": len(rows), "arrivals": len(arrivals),
                "notifications": notifications, "log_id": log_id,
            }
            self.database.finish_log(
                log_id, "success", len(rows),
                f"发货跟踪已更新 {len(rows)} 条，发送到库通知 {notifications} 条",
            )
            emit("SUCCESS", "结束", "飞书发货跟踪同步完成", result)
            return result
        except Exception as error:
            emit("ERROR", "失败", "飞书发货跟踪同步失败", {"error": str(error)})
            self.database.finish_log(log_id, "failed", 0, str(error))
            raise

    def send_feishu_shipment_notification(
        self, tracking_id: str, credentials: Credentials | None = None,
    ) -> str:
        credentials = credentials or self.load_credentials()
        if not credentials.feishu_chat_id:
            raise FeishuError("请先填写接收到库通知的飞书群 Chat ID。")
        row = self.database.shipment_tracking_row(tracking_id)
        if not row:
            raise FeishuError("未找到该发货跟踪记录。")
        foreign_arrival = str(row.get("foreign_arrival") or "").strip()
        if not foreign_arrival:
            raise FeishuError("该记录尚无国外到库日期，不能发送到库通知。")
        message_id = self.feishu_client(credentials).send_card(
            credentials.feishu_chat_id,
            _shipment_arrival_feishu_card(row, self.shop_name),
        )
        self.database.mark_shipment_notified(str(tracking_id), foreign_arrival)
        return message_id

    def send_feishu_product_series(
        self, rows: list[dict[str, Any]], period_type: str,
        credentials: Credentials | None = None,
    ) -> dict[str, Any]:
        """Manually send the currently displayed product-series summary to a group."""
        credentials = credentials or self.load_credentials()
        if not credentials.feishu_chat_id:
            raise FeishuError("请先填写接收产品系列分析的飞书群 Chat ID。")
        if not rows:
            raise FeishuError("没有可发送的产品系列统计。")
        client = self.feishu_client(credentials)
        chunk_size = 18
        chunks = [rows[index:index + chunk_size] for index in range(0, len(rows), chunk_size)]
        message_ids: list[str] = []
        for index, chunk in enumerate(chunks, start=1):
            message_ids.append(client.send_card(
                credentials.feishu_chat_id,
                _product_series_feishu_card(
                    chunk, rows, period_type, self.shop_name,
                    part=index, total_parts=len(chunks),
                ),
            ))
        return {
            "message_id": message_ids[0],
            "message_ids": message_ids,
            "messages": len(message_ids),
            "rows": len(rows),
            "shop_name": self.shop_name,
        }

    def sync_feishu_product_series(
        self, rows: list[dict[str, Any]], period_type: str,
        credentials: Credentials | None = None,
    ) -> dict[str, Any]:
        """兼容旧版调用；现在只发送飞书群卡片，不再写多维表格。"""
        return self.send_feishu_product_series(rows, period_type, credentials)

    def sync_feishu_weekly(
        self,
        week_end: str,
        *,
        write_table: bool = True,
        send_message: bool = True,
        credentials: Credentials | None = None,
        event_listener: LogListener | None = None,
    ) -> dict[str, Any]:
        """Upsert one weekly row and/or send its summary to a Feishu group."""
        if not write_table and not send_message:
            raise ValueError("至少选择写入周报表或发送群消息中的一项。")
        credentials = credentials or self.load_credentials()
        if write_table and (
            not credentials.feishu_app_token or not credentials.feishu_weekly_table_id
        ):
            raise FeishuError("请先填写飞书 App Token 和周报表 Table ID。")
        if send_message and not credentials.feishu_chat_id:
            raise FeishuError("请先填写接收周报的飞书群 Chat ID。")
        client = self.feishu_client(credentials)
        log_id = self.database.start_log("Feishu Weekly")

        def emit(level: str, stage: str, message: str, details: dict[str, Any]) -> None:
            self.database.append_log_event(log_id, level, stage, message, details)
            if event_listener:
                event_listener(log_id, level, stage, message, details)

        try:
            report = self.database.weekly_report(week_end)
            current = report["current"]
            comparison = report["comparison"]
            period = f"{report['current_start']} ~ {report['current_end']}"
            emit(
                "INFO", "开始", "开始同步飞书周报",
                {"period": period, "write_table": write_table,
                 "send_message": send_message},
            )
            table_action = "未写入"
            if write_table:
                definitions = {
                    "周报周期": 1, "店铺名称": 1, "店铺类型": 1,
                    "开始日期": 1, "结束日期": 1,
                    "销售额": 2, "销量": 2, "妥投": 2, "退货": 2,
                    "取消": 2, "浏览": 2, "加购": 2,
                    "广告花费": 2, "广告销售额": 2, "广告订单": 2,
                    "曝光": 2, "点击": 2, "CTR": 2, "ACOS": 2,
                    "TACOS": 2, "广告单占比": 2, "ACoTS": 2,
                    "销售额环比": 2,
                    "销量环比": 2, "广告花费环比": 2, "广告订单环比": 2,
                    "每日明细": 1, "同步时间": 1,
                }
                _, primary_name = client.ensure_fields(
                    credentials.feishu_app_token,
                    credentials.feishu_weekly_table_id,
                    definitions,
                )
                desired: dict[str, Any] = {
                    "周报周期": period,
                    "店铺名称": self.shop_name,
                    "店铺类型": self.shop_kind_label,
                    "开始日期": report["current_start"],
                    "结束日期": report["current_end"],
                    "销售额": float(current.get("revenue") or 0),
                    "销量": float(current.get("orders") or 0),
                    "妥投": float(current.get("delivered") or 0),
                    "退货": float(current.get("returns") or 0),
                    "取消": float(current.get("cancellations") or 0),
                    "浏览": float(current.get("views") or 0),
                    "加购": float(current.get("cart_adds") or 0),
                    "广告花费": float(current.get("spend") or 0),
                    "广告销售额": float(current.get("ad_revenue") or 0),
                    "广告订单": float(current.get("ad_orders") or 0),
                    "曝光": float(current.get("impressions") or 0),
                    "点击": float(current.get("clicks") or 0),
                    "CTR": float(current.get("ctr") or 0),
                    "ACOS": float(current.get("acos") or 0),
                    "TACOS": float(current.get("tacos") or 0),
                    "广告单占比": float(current.get("ad_order_share") or 0),
                    "ACoTS": float(current.get("acots") or 0),
                    "每日明细": _weekly_daily_text(report),
                    "同步时间": datetime.now().isoformat(timespec="seconds"),
                }
                for local_key, field_name in (
                    ("revenue", "销售额环比"), ("orders", "销量环比"),
                    ("spend", "广告花费环比"), ("ad_orders", "广告订单环比"),
                ):
                    if comparison.get(local_key) is not None:
                        desired[field_name] = float(comparison[local_key])
                if primary_name and primary_name not in desired:
                    desired[primary_name] = period
                records = client.list_records(
                    credentials.feishu_app_token, credentials.feishu_weekly_table_id
                )
                existing = next(
                    (
                        item for item in records
                        if field_text((item.get("fields") or {}).get("周报周期")) == period
                        and field_text((item.get("fields") or {}).get("店铺名称"))
                        in {"", self.shop_name}
                    ),
                    None,
                )
                if existing:
                    # 同步时间本身会变化，因此周报手动重发时也会留下最新刷新时间。
                    client.batch_update(
                        credentials.feishu_app_token,
                        credentials.feishu_weekly_table_id,
                        [{"record_id": existing.get("record_id"), "fields": desired}],
                    )
                    table_action = "更新"
                else:
                    client.batch_create(
                        credentials.feishu_app_token,
                        credentials.feishu_weekly_table_id,
                        [{"fields": desired}],
                    )
                    table_action = "新增"
                emit(
                    "SUCCESS", "写入飞书", f"周报表已{table_action}",
                    {"period": period},
                )

            message_id = ""
            if send_message:
                card = _weekly_feishu_card(
                    report, self.shop_name, self.shop_kind_label,
                )
                message_id = client.send_card(credentials.feishu_chat_id, card)
                emit(
                    "SUCCESS", "发送群消息", "周报已发送到飞书群",
                    {"message_id": message_id, "period": period},
                )
            result = {
                "period": period, "table_action": table_action,
                "message_id": message_id, "log_id": log_id,
                "shop_name": self.shop_name,
            }
            self.database.finish_log(log_id, "success", 1, "飞书周报同步完成")
            emit("SUCCESS", "结束", "飞书周报同步完成", result)
            return result
        except Exception as error:
            emit("ERROR", "失败", "飞书周报同步失败", {"error": str(error)})
            self.database.finish_log(log_id, "failed", 0, str(error))
            raise

    def send_feishu_monthly_profit(
        self,
        month: str,
        *,
        credentials: Credentials | None = None,
        event_listener: LogListener | None = None,
    ) -> dict[str, Any]:
        """Send the selected month P&L from the local cache as a Feishu card."""
        credentials = credentials or self.load_credentials()
        if not credentials.feishu_chat_id:
            raise FeishuError("请先填写接收报表的飞书群 Chat ID。")
        try:
            start = date.fromisoformat(str(month).strip() + "-01")
        except ValueError as error:
            raise ValueError("核算月份格式应为 YYYY-MM。") from error
        end = (start.replace(day=28) + timedelta(days=4)).replace(day=1) - timedelta(days=1)
        date_from, date_to = start.isoformat(), end.isoformat()
        client = self.feishu_client(credentials)
        log_id = self.database.start_log("Feishu Monthly Profit")

        def emit(level: str, stage: str, message: str, details: dict[str, Any]) -> None:
            self.database.append_log_event(log_id, level, stage, message, details)
            if event_listener:
                event_listener(log_id, level, stage, message, details)

        try:
            emit("INFO", "开始", "开始生成飞书月度盈亏卡片", {
                "shop_name": self.shop_name, "month": month,
            })
            summary = self.database.monthly_profit_summary(date_from, date_to)
            breakdown = self.database.monthly_finance_breakdown(date_from, date_to)
            card = _monthly_profit_feishu_card(
                summary, breakdown, self.shop_name, self.shop_kind_label,
                date_from, date_to,
            )
            message_id = client.send_card(credentials.feishu_chat_id, card)
            result = {
                "period": month, "message_id": message_id, "log_id": log_id,
                "shop_name": self.shop_name,
            }
            emit("SUCCESS", "发送群消息", "月度盈亏表已发送到飞书群", result)
            self.database.finish_log(log_id, "success", 1, "飞书月度盈亏表发送完成")
            return result
        except Exception as error:
            emit("ERROR", "失败", "飞书月度盈亏表发送失败", {"error": str(error)})
            self.database.finish_log(log_id, "failed", 0, str(error))
            raise

    def send_feishu_cross_border_profit(
        self,
        date_from: str,
        date_to: str,
        *,
        credentials: Credentials | None = None,
        event_listener: LogListener | None = None,
    ) -> dict[str, Any]:
        """Send weekly cross-border profit rows for a selected local range."""
        if self.shop_kind != "cross_border":
            raise ValueError("跨境周利润只能从跨境店铺的数据空间发送。")
        start, end = date.fromisoformat(date_from), date.fromisoformat(date_to)
        if start > end:
            raise ValueError("开始日期不能晚于结束日期。")
        credentials = credentials or self.load_credentials()
        if not credentials.feishu_chat_id:
            raise FeishuError("请先填写接收报表的飞书群 Chat ID。")
        client = self.feishu_client(credentials)
        log_id = self.database.start_log("Feishu Cross Border Profit")

        def emit(level: str, stage: str, message: str, details: dict[str, Any]) -> None:
            self.database.append_log_event(log_id, level, stage, message, details)
            if event_listener:
                event_listener(log_id, level, stage, message, details)

        try:
            emit("INFO", "开始", "开始生成飞书跨境周利润卡片", {
                "shop_name": self.shop_name,
                "date_from": date_from, "date_to": date_to,
            })
            weeks = self.database.cross_border_weekly_summary(date_from, date_to)
            summary = self.database.cross_border_period_summary(date_from, date_to)
            card = _cross_border_profit_feishu_card(
                summary, weeks, self.shop_name, date_from, date_to,
            )
            message_id = client.send_card(credentials.feishu_chat_id, card)
            result = {
                "period": f"{date_from} ~ {date_to}", "message_id": message_id,
                "log_id": log_id, "shop_name": self.shop_name,
            }
            emit("SUCCESS", "发送群消息", "跨境周利润表已发送到飞书群", result)
            self.database.finish_log(log_id, "success", 1, "飞书跨境周利润表发送完成")
            return result
        except Exception as error:
            emit("ERROR", "失败", "飞书跨境周利润表发送失败", {"error": str(error)})
            self.database.finish_log(log_id, "failed", 0, str(error))
            raise

    def ai_analysis(
        self,
        date_from: str,
        date_to: str,
        question: str,
        analysis_type: str = "custom",
        credentials: Credentials | None = None,
        product_sku: str | None = None,
    ) -> str:
        """Analyze a bounded, aggregate snapshot of the selected local data.

        No credential, sync log, raw API response, or row outside the selected
        date range is included in ``data_context``.
        """

        context = self.ai_data_context(date_from, date_to, product_sku=product_sku)
        prompt = _ai_analysis_prompt(question, analysis_type, bool(product_sku))
        return self.ai_client(credentials).analyze(prompt, data_context=context)

    def ai_data_context(
        self, date_from: str, date_to: str, product_sku: str | None = None,
    ) -> dict[str, Any]:
        """Build the JSON-serializable aggregate context sent to the AI relay."""

        start = date.fromisoformat(date_from)
        end = date.fromisoformat(date_to)
        if start > end:
            raise ValueError("开始日期不能晚于结束日期。")

        if product_sku:
            store_summary = self.database.dashboard_summary(date_from, date_to)
            store_trend = self.database.daily_trend(date_from, date_to)
            store_ad_keys = (
                "revenue", "orders", "spend", "ad_revenue", "ad_orders",
                "impressions", "clicks", "ctr", "roas", "acos", "tacos",
            )
            store_ad_daily: list[dict[str, Any]] = []
            for row in store_trend:
                spend = float(row.get("spend") or 0)
                ad_revenue = float(row.get("ad_revenue") or 0)
                seller_revenue = float(row.get("revenue") or 0)
                impressions = int(row.get("impressions") or 0)
                clicks = int(row.get("clicks") or 0)
                store_ad_daily.append({
                    key: row.get(key) for key in (
                        "day", "revenue", "orders", "spend", "ad_revenue",
                        "ad_orders", "impressions", "clicks",
                    )
                } | {
                    "ctr": clicks / impressions * 100 if impressions else None,
                    "roas": ad_revenue / spend if spend else None,
                    "acos": spend / ad_revenue * 100 if ad_revenue else None,
                    "tacos": spend / seller_revenue * 100 if seller_revenue else None,
                })
            return {
                "period": {"date_from": date_from, "date_to": date_to},
                "mode": "single_product_plus_store_ad_efficiency",
                "store_overall_advertising_efficiency": {
                    "summary": {
                        key: store_summary.get(key) for key in store_ad_keys
                    },
                    "daily_trend": store_ad_daily,
                    "scope": (
                        "全店活动级广告汇总；用于店铺整体 ACOS/TACOS/ROAS/CTR，"
                        "不包含其他 SKU 的广告明细"
                    ),
                },
                "selected_product": self.database.product_ai_detail(
                    date_from, date_to, str(product_sku)
                ),
                "metric_definitions": {
                    "ACOS": "广告花费 / 广告归因销售额 * 100%",
                    "TACOS": "广告花费 / Seller 总销售额 * 100%",
                    "ROAS": "广告归因销售额 / 广告花费",
                    "daily_sales": "Ozon /v1/analytics/stocks 返回的 ads_cluster",
                    "monthly_estimated": "集群日均销量 * 30",
                    "recommended_replenishment": (
                        "max(0, 向上取整(日均销量 * 目标天数 - 可售库存 - 在途 - 已申请))"
                    ),
                },
                "data_scope": {
                    "product_sku": str(product_sku),
                    "row_limit": None,
                    "source": (
                        "全部数据只读取数据中心已保存的本地 SQLite 缓存，AI 分析过程"
                        "不会请求 Ozon；全店只发送活动级广告效率汇总，商品部分只发送"
                        "所选 SKU 在日期区间内的全部本地明细和全部库存集群"
                    ),
                    "excluded": "其他 SKU 的广告明细与商品明细",
                    "product_advertising_priority": (
                        "优先使用 exact_sku_local_cache；若不存在，仅可使用"
                        "dedicated_campaign_name_local_cache，并标注为活动级推算；"
                        "不得把通用活动归因给商品，也不得把两种来源相加"
                    ),
                    "empty_data_rule": "空列表表示本地数据不足，不能据此推断平台真实为 0",
                },
            }

        summary = self.database.dashboard_summary(date_from, date_to)
        trend = self.database.daily_trend(date_from, date_to)
        campaigns = self.database.top_campaigns(date_from, date_to, limit=20)
        products = self.database.top_products(date_from, date_to, limit=20)
        return {
            "period": {"date_from": date_from, "date_to": date_to},
            "metric_definitions": {
                "ACOS": "广告花费 / 广告归因销售额 * 100%",
                "TACOS": "广告花费 / Seller 总销售额 * 100%",
                "ROAS": "广告归因销售额 / 广告花费",
            },
            "summary": summary,
            "daily_trend": trend,
            "top_campaigns_by_spend": campaigns,
            "top_products_by_revenue": products,
            "data_scope": {
                "campaign_limit": 20,
                "product_limit": 20,
                "source": "用户本地 SQLite 数据库的所选日期区间聚合结果",
                "empty_data_rule": "0 或空列表表示本地数据不足，不能据此推断平台真实为 0",
            },
        }

    def sync_finance_accruals(
        self,
        date_from: str,
        date_to: str,
        progress: Callable[[str], None] | None = None,
        cancel_event: threading.Event | None = None,
        event_listener: LogListener | None = None,
    ) -> dict[str, Any]:
        """Refresh shop-economy totals and SKU-level accrual transactions."""
        client = self.seller_client()
        log_id = self.database.start_log("Seller Accruals")
        emit, tracked_progress = self._log_callbacks(log_id, progress, event_listener)
        try:
            emit("INFO", "开始", "开始同步商店经济与应计费用明细", {
                "date_from": date_from, "date_to": date_to,
                "summary_endpoint": "/v1/finance/cash-flow-statement/list",
                "detail_endpoint": "/v3/finance/transaction/list",
                "allocation_rule": "仅单一 SKU 的应计记录归属到商品；多 SKU 与店铺级费用保留未分摊",
            })
            report = client.finance_cash_flow_statement(
                date_from, date_to, tracked_progress, cancel_event
            )
            emit("SUCCESS", "商店经济", "商店经济汇总读取完成", {
                "cash_flow_rows": len(report.get("cash_flows", [])),
                "detail_rows": len(report.get("details", [])),
            })
            statement_rows = self.database.replace_finance_cash_flow(
                date_from, date_to, report
            )
            emit("INFO", "逐笔应计", "正在读取逐笔应计费用", {
                "endpoint": "/v3/finance/transaction/list",
                "page_size": 1000,
            })
            transactions = client.finance_transactions(
                date_from, date_to, tracked_progress, cancel_event
            )
            transaction_rows = self.database.upsert_finance(transactions)
            summary = self.database.monthly_profit_summary(date_from, date_to)
            now = datetime.now().isoformat(timespec="seconds")
            self.database.save_settings({
                "seller_finance_last_success_at": now,
                "seller_finance_last_date_to": date_to,
                "seller_finance_verified_through": min(
                    date.fromisoformat(date_to), date.today()
                ).isoformat(),
                "seller_shop_economy_last_success_at": now,
            })
            emit("SUCCESS", "对账", "应计费用已缓存并完成店铺级对账", {
                "statement_rows": statement_rows,
                "transaction_rows": transaction_rows,
                "shop_economy_total": summary.get("platform_payout"),
                "transaction_total": summary.get("transaction_total"),
                "difference": summary.get("reconciliation_difference"),
                "unallocated_operations": summary.get("unallocated_operations"),
                "unallocated_amount": summary.get("unallocated_amount"),
            })
            self.database.finish_log(
                log_id, "success", transaction_rows,
                f"商店经济 {statement_rows} 行、逐笔应计 {transaction_rows} 行同步完成",
            )
            return {
                "statement_rows": statement_rows,
                "transaction_rows": transaction_rows,
                "summary": summary,
            }
        except Exception as error:
            emit("ERROR", "同步失败", str(error), {"error_type": type(error).__name__})
            self.database.finish_log(log_id, "failed", 0, str(error))
            raise

    def sync_monthly_sales(
        self,
        date_from: str,
        date_to: str,
        progress: Callable[[str], None] | None = None,
        cancel_event: threading.Event | None = None,
        event_listener: LogListener | None = None,
    ) -> dict[str, Any]:
        """Fill one month's Seller Analytics cache without unrelated requests.

        This is intentionally separate from ``sync_seller``: the monthly P&L
        page needs the official order-date metrics, but it must not also read
        products, postings, returns and finance.  A verified complete month is
        served from SQLite and therefore costs no Ozon request.
        """

        log_id = self.database.start_log("Seller Monthly Sales")
        emit, tracked_progress = self._log_callbacks(log_id, progress, event_listener)
        coverage = self.database.sales_coverage_info(date_from, date_to)
        if coverage["complete"]:
            message = (
                f"Seller 月销量已完整缓存 {coverage['requested_from']} 至 "
                f"{coverage['effective_to']}，本次未调用 Ozon 接口"
            )
            emit("INFO", "使用缓存", message, coverage)
            self.database.finish_log(log_id, "cached", coverage["rows_count"], message)
            return {"status": "cached", "rows": coverage["rows_count"], "coverage": coverage}

        state = self.database.sales_sync_state(date_from, date_to)
        next_allowed = _optional_datetime(state.get("next_allowed_at"))
        if next_allowed and datetime.now() < next_allowed:
            message = (
                f"Seller Analytics 正在限频冷却，请于 "
                f"{next_allowed.isoformat(timespec='seconds')} 后再次同步本月销量"
            )
            emit("WARNING", "限频冷却", message, {**coverage, "next_allowed_at": state.get("next_allowed_at")})
            self.database.finish_log(log_id, "cached", coverage["rows_count"], message)
            return {
                "status": "cooldown", "rows": coverage["rows_count"],
                "coverage": coverage, "message": message,
            }

        client = self.seller_client()
        try:
            emit("INFO", "开始", "开始补齐完整月份 Seller 销量", {
                "date_from": date_from,
                "date_to": date_to,
                "endpoint": "/v1/analytics/data",
                "dimensions": ["sku", "day"],
                "metrics": ["revenue", "ordered_units"],
                "reason": "月度订单金额与已订购商品必须按订单日期与 Ozon 后台保持一致",
                "existing_cache": coverage,
            })
            now = datetime.now()
            self.database.save_settings({
                "seller_analytics_next_allowed_at": (
                    now + timedelta(seconds=65)
                ).isoformat(timespec="seconds")
            })
            rows = client.analytics(date_from, date_to, tracked_progress, cancel_event)
            count = self.database.upsert_sales(rows)
            self.database.mark_coverage(date_from, date_to, "sales", "seller_analytics")
            metadata = dict(getattr(client, "last_analytics_metadata", {}) or {})
            if metadata.get("traffic_available"):
                self.database.mark_coverage(date_from, date_to, "traffic", "seller_analytics")
                self.database.save_settings({"seller_traffic_status": "available"})
            else:
                self.database.save_settings({"seller_traffic_status": "subscription_required"})
            now = datetime.now()
            verified_through = min(
                date.fromisoformat(date_to), date.today() - timedelta(days=1)
            ).isoformat()
            self.database.save_settings({
                "seller_analytics_last_success_at": now.isoformat(timespec="seconds"),
                "seller_analytics_last_date_to": date_to,
                "seller_analytics_verified_through": verified_through,
                "seller_analytics_next_allowed_at": (
                    now + timedelta(seconds=65)
                ).isoformat(timespec="seconds"),
            })
            refreshed = self.database.sales_coverage_info(date_from, date_to)
            totals = {
                "order_amount": sum(float(row.get("revenue") or 0) for row in rows),
                "ordered_products": sum(int(float(row.get("ordered_units") or 0)) for row in rows),
            }
            emit("SUCCESS", "写入数据库", "完整月份 Seller 销量已保存", {
                "written_rows": count, "coverage": refreshed, **totals,
            })
            self.database.finish_log(
                log_id, "success", count,
                f"Seller 月销量同步完成：{totals['ordered_products']} 件，"
                f"订单金额 {totals['order_amount']:.2f} RUB",
            )
            return {
                "status": "success", "rows": count, "coverage": refreshed,
                **totals,
            }
        except Exception as error:
            emit("ERROR", "同步失败", str(error), {"error_type": type(error).__name__})
            self.database.finish_log(log_id, "failed", 0, str(error))
            raise

    def sync_seller(
        self, date_from: str, date_to: str, progress: Callable[[str], None] | None = None,
        cancel_event: threading.Event | None = None,
        event_listener: LogListener | None = None,
    ) -> list[SyncResult]:
        client = self.seller_client()
        results: list[SyncResult] = []

        products_method = getattr(client, "products", None)
        if callable(products_method):
            product_log_id = self.database.start_log("Seller Products")
            emit, tracked_progress = self._log_callbacks(
                product_log_id, progress, event_listener
            )
            try:
                emit("INFO", "开始", "开始同步商品目录与商家货号", {
                    "endpoint": "/v3/product/list",
                })
                product_rows = products_method(tracked_progress, cancel_event)
                emit("SUCCESS", "获取数据", "商品目录读取完成", {
                    "parsed_rows": len(product_rows),
                })
                product_count = self.database.upsert_products(product_rows)
                emit("SUCCESS", "写入数据库", "SKU 与货号映射已保存", {
                    "written_rows": product_count,
                })
                self.database.finish_log(
                    product_log_id, "success", product_count, "商品货号同步完成"
                )
                results.append(SyncResult("Seller Products", product_count, "商品货号同步完成"))
            except Exception as error:
                emit("ERROR", "同步失败", str(error), {"error_type": type(error).__name__})
                self.database.finish_log(product_log_id, "failed", 0, str(error))
                if isinstance(error, SyncCancelled):
                    raise
                results.append(SyncResult("Seller Products", 0, f"商品货号同步失败：{error}"))

        log_id = self.database.start_log("Seller Analytics")
        emit, tracked_progress = self._log_callbacks(log_id, progress, event_listener)
        cache = self.database.sales_sync_state(date_from, date_to)
        plan = _sales_sync_plan(date_from, date_to, cache)
        cached_rows = int(cache.get("rows_count") or 0)
        if plan["skip_reason"]:
            message = str(plan["skip_reason"])
            emit("INFO", "智能缓存", message, {
                "requested_date_from": date_from,
                "requested_date_to": date_to,
                "cached_rows": cached_rows,
                "cached_min_day": cache.get("min_day"),
                "cached_max_day": cache.get("max_day"),
                "cached_updated_at": cache.get("updated_at"),
                "verified_through": cache.get("verified_through"),
                "last_success_at": cache.get("last_success_at"),
                "next_allowed_at": cache.get("next_allowed_at"),
                "api_requests": 0,
            })
            self.database.finish_log(log_id, "cached", cached_rows, message)
            results.append(SyncResult("Seller Analytics", cached_rows, message))
        else:
            analytics_from = str(plan["api_date_from"])
            try:
                emit("INFO", "开始", "开始同步 Seller 销量分析", {
                    "requested_date_from": date_from, "requested_date_to": date_to,
                    "api_date_from": analytics_from, "api_date_to": date_to,
                    "endpoint": "/v1/analytics/data",
                    "mode": plan["mode"],
                    "cached_rows": cached_rows,
                })
                if analytics_from != date_from:
                    emit("INFO", "最小增量", "复用历史缓存，仅补齐末尾缺失或未结算日期", {
                        "cached_min_day": cache.get("min_day"),
                        "cached_max_day": cache.get("max_day"),
                        "cached_updated_at": cache.get("updated_at"),
                        "refresh_from": analytics_from,
                        "refresh_to": date_to,
                        "saved_days": (
                            date.fromisoformat(analytics_from)
                            - date.fromisoformat(date_from)
                        ).days,
                    })
                if progress:
                    progress("正在拉取 Seller 销量分析…")
                # Persist the endpoint cooldown before the request so closing and
                # reopening the application cannot accidentally bypass it.
                self.database.save_settings({
                    "seller_analytics_next_allowed_at": (
                        datetime.now() + timedelta(seconds=65)
                    ).isoformat(timespec="seconds")
                })
                rows = client.analytics(analytics_from, date_to, tracked_progress, cancel_event)
                emit("SUCCESS", "获取数据", "Seller 销量分析返回完成", {
                    "parsed_rows": len(rows),
                    "date_from": analytics_from,
                    "date_to": date_to,
                })
                analytics_meta = dict(getattr(client, "last_analytics_metadata", {}) or {})
                if analytics_meta.get("traffic_available"):
                    self.database.mark_coverage(
                        analytics_from, date_to, "traffic", "seller_analytics"
                    )
                    self.database.save_settings({
                        "seller_traffic_status": "available",
                        "seller_traffic_checked_at": datetime.now().isoformat(timespec="seconds"),
                    })
                    emit("SUCCESS", "流量字段", "浏览与加购指标已由 Seller API 返回", analytics_meta)
                else:
                    self.database.save_settings({
                        "seller_traffic_status": "subscription_required",
                        "seller_traffic_checked_at": datetime.now().isoformat(timespec="seconds"),
                    })
                    emit(
                        "WARNING", "流量字段",
                        "Seller API 本次只返回基础销量指标，未返回浏览/加购；"
                        "这些指标受店铺 Analytics 订阅/API 权限限制，界面保持为“—”",
                        analytics_meta,
                    )
                count = self.database.upsert_sales(rows)
                self.database.mark_coverage(
                    analytics_from, date_to, "sales", "seller_analytics"
                )
                now = datetime.now()
                verified_through = min(
                    date.fromisoformat(date_to), date.today() - timedelta(days=1)
                ).isoformat()
                self.database.save_settings({
                    "seller_analytics_last_success_at": now.isoformat(timespec="seconds"),
                    "seller_analytics_last_date_to": date_to,
                    "seller_analytics_verified_through": verified_through,
                    "seller_analytics_next_allowed_at": (
                        now + timedelta(seconds=65)
                    ).isoformat(timespec="seconds"),
                })
                emit("SUCCESS", "写入数据库", "销量分析已保存到本地数据库", {
                    "written_rows": count,
                    "verified_through": verified_through,
                    "next_refresh_earliest": (
                        now + timedelta(seconds=65)
                    ).isoformat(timespec="seconds"),
                })
                self.database.finish_log(log_id, "success", count, "Seller 销量分析同步完成")
                results.append(SyncResult("Seller Analytics", count, "销量分析同步完成"))
            except Exception as error:
                if isinstance(error, SyncCancelled):
                    emit("ERROR", "同步取消", str(error), {"error_type": type(error).__name__})
                    self.database.finish_log(log_id, "failed", 0, str(error))
                    raise
                cache = self.database.sales_sync_state(date_from, date_to)
                cached_rows = int(cache.get("rows_count") or 0)
                if _is_rate_limit_error(error):
                    next_allowed = (
                        datetime.now() + timedelta(seconds=65)
                    ).isoformat(timespec="seconds")
                    self.database.save_settings({
                        "seller_analytics_next_allowed_at": next_allowed
                    })
                if cached_rows and _is_rate_limit_error(error):
                    message = (
                        f"Seller 接口限频；已自动等待并重试 1 次，仍未恢复。"
                        f"保留本地缓存 {cached_rows} 行，{next_allowed} 后可再次补齐"
                    )
                    emit("WARNING", "使用缓存", message, {
                        **cache,
                        "attempted_date_from": analytics_from,
                        "attempted_date_to": date_to,
                        "next_allowed_at": next_allowed,
                    })
                    self.database.finish_log(log_id, "cached", cached_rows, message)
                    results.append(SyncResult("Seller Analytics", cached_rows, message))
                else:
                    message = f"Seller 销量同步失败，已继续财务和广告同步：{error}"
                    emit("ERROR", "同步失败", message, {"error_type": type(error).__name__})
                    self.database.finish_log(log_id, "failed", 0, str(error))
                    results.append(SyncResult("Seller Analytics", 0, message))

        cancellations_method = getattr(client, "cancellations", None)
        if callable(cancellations_method):
            cancellation_log_id = self.database.start_log("Seller Cancellations")
            emit, tracked_progress = self._log_callbacks(
                cancellation_log_id, progress, event_listener
            )
            try:
                emit("INFO", "开始", "开始同步 FBO/FBS 取消商品明细", {
                    "date_from": date_from, "date_to": date_to,
                    "endpoints": ["/v4/posting/fbs/list", "/v3/posting/fbo/list"],
                })
                cancellation_rows = cancellations_method(
                    date_from, date_to, tracked_progress, cancel_event
                )
                count = self.database.replace_cancellations(
                    date_from, date_to, cancellation_rows
                )
                self.database.upsert_products(
                    {
                        "sku": row.get("sku"), "offer_id": row.get("offer_id"),
                        "name": row.get("product_name"), "source": "api",
                    }
                    for row in cancellation_rows
                )
                emit("SUCCESS", "写入数据库", "取消商品明细已保存", {
                    "written_rows": count,
                    "quantity": sum(int(row.get("quantity") or 0) for row in cancellation_rows),
                })
                self.database.finish_log(
                    cancellation_log_id, "success", count, "Seller 取消商品同步完成"
                )
                results.append(SyncResult(
                    "Seller Cancellations", count, "取消商品明细同步完成"
                ))
            except Exception as error:
                emit("WARNING", "字段暂不可用", str(error), {
                    "error_type": type(error).__name__,
                    "display_behavior": "取消件数与取消率显示为暂无数据，不伪装成 0",
                })
                self.database.finish_log(cancellation_log_id, "failed", 0, str(error))
                if isinstance(error, SyncCancelled):
                    raise
                results.append(SyncResult(
                    "Seller Cancellations", 0, f"取消商品同步失败：{error}"
                ))

        deliveries_method = getattr(client, "deliveries", None)
        if callable(deliveries_method):
            delivery_log_id = self.database.start_log("Seller Deliveries")
            emit, tracked_progress = self._log_callbacks(
                delivery_log_id, progress, event_listener
            )
            try:
                emit("INFO", "开始", "开始解析 FBO/FBS 妥投商品明细", {
                    "date_from": date_from, "date_to": date_to,
                    "request_behavior": "复用取消同步已读取的发货列表，不重复请求",
                })
                delivery_rows = deliveries_method(
                    date_from, date_to, tracked_progress, cancel_event
                )
                count = self.database.replace_deliveries(
                    date_from, date_to, delivery_rows
                )
                self.database.upsert_products(
                    {
                        "sku": row.get("sku"), "offer_id": row.get("offer_id"),
                        "name": row.get("product_name"), "source": "api",
                    }
                    for row in delivery_rows
                )
                emit("SUCCESS", "写入数据库", "妥投商品明细已保存", {
                    "written_rows": count,
                    "quantity": sum(int(row.get("quantity") or 0) for row in delivery_rows),
                })
                self.database.finish_log(
                    delivery_log_id, "success", count, "Seller 妥投商品同步完成"
                )
                results.append(SyncResult("Seller Deliveries", count, "妥投商品明细同步完成"))
            except Exception as error:
                emit("WARNING", "字段暂不可用", str(error), {
                    "error_type": type(error).__name__,
                    "display_behavior": "妥投件数显示为暂无数据，不伪装成 0",
                })
                self.database.finish_log(delivery_log_id, "failed", 0, str(error))
                if isinstance(error, SyncCancelled):
                    raise
                results.append(SyncResult(
                    "Seller Deliveries", 0, f"妥投商品同步失败：{error}"
                ))

        routes_method = getattr(client, "posting_routes", None)
        if callable(routes_method):
            route_log_id = self.database.start_log("Seller Posting Routes")
            emit, tracked_progress = self._log_callbacks(
                route_log_id, progress, event_listener
            )
            try:
                emit("INFO", "开始", "开始缓存全部 FBO/FBS 订单配送路线", {
                    "date_from": date_from, "date_to": date_to,
                    "request_behavior": "复用取消/妥投同步已读取的发货列表",
                    "statuses": "all",
                })
                route_rows = routes_method(
                    date_from, date_to, tracked_progress, cancel_event
                )
                count = self.database.replace_posting_routes(
                    date_from, date_to, route_rows
                )
                emit("SUCCESS", "写入数据库", "订单配送路线已缓存", {
                    "written_rows": count,
                    "open_and_closed_statuses": True,
                    "api_requests_added": 0,
                })
                self.database.finish_log(
                    route_log_id, "success", count, "Seller 配送路线同步完成"
                )
                results.append(SyncResult(
                    "Seller Posting Routes", count, "配送起始与目的地已缓存"
                ))
            except Exception as error:
                emit("WARNING", "字段暂不可用", str(error), {
                    "error_type": type(error).__name__,
                    "display_behavior": "产品利润表保留历史路线缓存",
                })
                self.database.finish_log(route_log_id, "failed", 0, str(error))
                if isinstance(error, SyncCancelled):
                    raise
                results.append(SyncResult(
                    "Seller Posting Routes", 0, f"配送路线同步失败：{error}"
                ))

        finance_log_id = self.database.start_log("Seller Finance")
        emit, tracked_progress = self._log_callbacks(finance_log_id, progress, event_listener)
        finance_ready = False
        finance_cache = self.database.finance_sync_state(date_from, date_to)
        finance_plan = _sales_sync_plan(date_from, date_to, finance_cache)
        finance_cached_rows = int(finance_cache.get("rows_count") or 0)
        if finance_plan["skip_reason"]:
            message = str(finance_plan["skip_reason"]).replace(
                "Seller 缓存", "Seller 财务缓存"
            )
            emit("INFO", "智能缓存", message, {
                "requested_date_from": date_from,
                "requested_date_to": date_to,
                "cached_rows": finance_cached_rows,
                "cached_min_day": finance_cache.get("min_day"),
                "cached_max_day": finance_cache.get("max_day"),
                "verified_through": finance_cache.get("verified_through"),
                "api_requests": 0,
            })
            self.database.finish_log(
                finance_log_id, "cached", finance_cached_rows, message
            )
            results.append(SyncResult("Seller Finance", finance_cached_rows, message))
            finance_ready = True
        else:
            finance_from = str(finance_plan["api_date_from"])
            try:
                emit("INFO", "开始", "开始同步 Seller 财务流水", {
                    "requested_date_from": date_from,
                    "requested_date_to": date_to,
                    "api_date_from": finance_from,
                    "api_date_to": date_to,
                    "mode": finance_plan["mode"],
                    "endpoint": "/v3/finance/transaction/list",
                    "cached_rows": finance_cached_rows,
                })
                if finance_from != date_from:
                    emit("INFO", "最小增量", "复用历史财务流水，仅刷新最后缓存日及缺失日期", {
                        "cached_min_day": finance_cache.get("min_day"),
                        "cached_max_day": finance_cache.get("max_day"),
                        "refresh_from": finance_from,
                        "refresh_to": date_to,
                    })
                if progress:
                    progress("正在拉取 Seller 财务流水…")
                cash_flow_method = getattr(client, "finance_cash_flow_statement", None)
                cash_flow_count = 0
                if callable(cash_flow_method):
                    emit("INFO", "商店经济", "正在读取商店经济应计费用汇总", {
                        "endpoint": "/v1/finance/cash-flow-statement/list",
                        "date_from": finance_from, "date_to": date_to,
                        "with_details": True,
                    })
                    cash_flow_report = cash_flow_method(
                        finance_from, date_to, tracked_progress, cancel_event
                    )
                    cash_flow_count = self.database.replace_finance_cash_flow(
                        finance_from, date_to, cash_flow_report
                    )
                    emit("SUCCESS", "商店经济", "商店经济汇总与费用明细已缓存", {
                        "cash_flow_rows": len(cash_flow_report.get("cash_flows", [])),
                        "detail_rows": len(cash_flow_report.get("details", [])),
                        "written_rows": cash_flow_count,
                    })
                rows = client.finance_transactions(
                    finance_from, date_to, tracked_progress, cancel_event
                )
                emit("SUCCESS", "获取数据", "Seller 财务流水返回完成", {
                    "parsed_rows": len(rows),
                    "date_from": finance_from,
                    "date_to": date_to,
                })
                count = self.database.upsert_finance(rows)
                now = datetime.now()
                verified_through = min(
                    date.fromisoformat(date_to), date.today() - timedelta(days=1)
                ).isoformat()
                self.database.save_settings({
                    "seller_finance_last_success_at": now.isoformat(timespec="seconds"),
                    "seller_finance_last_date_to": date_to,
                    "seller_finance_verified_through": verified_through,
                })
                emit("SUCCESS", "写入数据库", "财务流水已保存到本地数据库", {
                    "written_rows": count,
                    "shop_economy_rows": cash_flow_count,
                    "verified_through": verified_through,
                })
                self.database.finish_log(finance_log_id, "success", count, "财务流水同步完成")
                results.append(SyncResult("Seller Finance", count, "财务流水同步完成"))
                finance_ready = True
            except Exception as error:
                emit("ERROR", "同步失败", str(error), {"error_type": type(error).__name__})
                self.database.finish_log(finance_log_id, "failed", 0, str(error))
                if isinstance(error, SyncCancelled):
                    raise
                results.append(SyncResult("Seller Finance", 0, f"财务同步失败：{error}"))

        if finance_ready:
            returns_log_id = self.database.start_log("Seller Returns")
            emit, _ = self._log_callbacks(returns_log_id, progress, event_listener)
            try:
                emit("INFO", "统计口径", "按原始下单日期计算财务确认的买家实际退货", {
                    "date_from": date_from,
                    "date_to": date_to,
                    "source": "finance_transactions",
                    "operation_type": "ClientReturnAgentOperation",
                    "date_field": "posting.order_date",
                    "excluded_source": "/v1/returns/list 物流退货池",
                    "api_requests": 0,
                })
                return_rows = self.database.finance_customer_returns(date_from, date_to)
                return_count = self.database.replace_returns(
                    date_from, date_to, return_rows, coverage_source="finance"
                )
                return_quantity = sum(
                    int(row.get("quantity") or 0) for row in return_rows
                )
                emit("SUCCESS", "写入数据库", "财务确认退货已按下单日期保存", {
                    "written_rows": return_count,
                    "quantity": return_quantity,
                    "api_requests": 0,
                })
                message = (
                    f"按下单日期统计财务确认退货 {return_quantity} 件（辅助口径）"
                )
                self.database.finish_log(
                    returns_log_id, "success", return_count, message
                )
                results.append(SyncResult("Seller Returns", return_count, message))
            except Exception as error:
                emit("WARNING", "字段暂不可用", str(error), {
                    "error_type": type(error).__name__,
                    "display_behavior": "退货件数显示为暂无数据，不伪装成 0",
                })
                self.database.finish_log(returns_log_id, "failed", 0, str(error))
                results.append(SyncResult(
                    "Seller Returns", 0, f"财务确认退货统计失败：{error}"
                ))
        return results

    @staticmethod
    def _performance_product_days(
        date_from: str, date_to: str, *, current_day: date | None = None,
    ) -> list[str]:
        """Return the days accepted by Ozon's product/SKU statistics endpoint."""
        today = current_day or date.today()
        start = date.fromisoformat(str(date_from)[:10])
        end = date.fromisoformat(str(date_to)[:10])
        return [
            value.isoformat()
            for value in (today - timedelta(days=1), today)
            if start <= value <= end
        ]

    def sync_performance(
        self, date_from: str, date_to: str, progress: Callable[[str], None] | None = None,
        cancel_event: threading.Event | None = None,
        event_listener: LogListener | None = None,
    ) -> list[SyncResult]:
        client = self.performance_client()
        log_id = self.database.start_log("Performance API")
        emit, tracked_progress = self._log_callbacks(log_id, progress, event_listener)
        started_at = time.monotonic()
        try:
            emit("INFO", "开始", "开始快速同步广告数据", {
                "mode": "daily_json_fast",
                "date_from": date_from,
                "date_to": date_to,
            })
            emit("INFO", "认证", "正在请求 Performance Access Token", {
                "endpoint": "/api/client/token",
            })
            client.authenticate(tracked_progress, cancel_event)
            emit("SUCCESS", "认证", "Performance API 认证成功", {})

            tracked_progress("正在读取广告活动…")
            emit("INFO", "读取活动", "请求广告活动列表", {
                "endpoint": "/api/client/campaign",
            })
            campaigns = client.campaigns(tracked_progress, cancel_event)
            state_counts: dict[str, int] = {}
            for campaign in campaigns:
                state = str(campaign.get("state") or "UNKNOWN")
                state_counts[state] = state_counts.get(state, 0) + 1
            emit("SUCCESS", "读取活动", "广告活动列表获取完成", {
                "campaign_count": len(campaigns),
                "state_counts": state_counts,
            })
            campaign_count = self.database.upsert_campaigns(campaigns)
            emit("SUCCESS", "写入数据库", "广告活动已保存到本地数据库", {
                "written_campaigns": campaign_count,
            })
            if not campaigns:
                emit("WARNING", "结束", "账号中没有可同步的广告活动", {})
                self.database.finish_log(log_id, "success", 0, "没有可同步的广告活动")
                return [SyncResult("Performance API", 0, "没有发现广告活动")]

            # 默认使用官方的即时每日统计接口。旧的 statistics/json 会为每 10 个活动
            # 创建一个异步报告，大店铺很容易触发 429 或等待数分钟。
            rows = client.statistics_daily(
                campaigns, date_from, date_to, tracked_progress, cancel_event, emit
            )
            totals = {
                "impressions": sum(float(row.get("impressions") or 0) for row in rows),
                "clicks": sum(float(row.get("clicks") or 0) for row in rows),
                "orders": sum(float(row.get("orders") or 0) for row in rows),
                "revenue": round(sum(float(row.get("revenue") or 0) for row in rows), 2),
                "spend": round(sum(float(row.get("spend") or 0) for row in rows), 2),
            }
            emit("INFO", "数据校验", "已汇总本次广告数据", {
                "parsed_rows": len(rows), **totals,
            })
            count = self.database.upsert_ads(rows)
            # Ozon currently accepts only today or yesterday on the product/SKU
            # endpoint.  Query those days individually instead of sending the
            # whole UI range (which returned HTTP 400 for 7/30-day ranges).
            sku_count = 0
            product_days = self._performance_product_days(date_from, date_to)
            if not product_days:
                emit("INFO", "商品广告缓存", "所选区间不含今天或昨天，跳过受限的 SKU 明细接口", {
                    "date_from": date_from, "date_to": date_to,
                    "cache_fallback": "unique_campaign_title",
                })
            for product_day in product_days:
                try:
                    sku_rows = client.product_statistics_by_sku(
                        campaigns, product_day, product_day,
                        tracked_progress, cancel_event, emit,
                    )
                    written = self.database.replace_ad_product_details(
                        product_day, product_day, sku_rows,
                    )
                    sku_count += written
                    emit("SUCCESS", "商品广告缓存", "精确 SKU 广告明细已按日保存", {
                        "day": product_day, "written_rows": written,
                        "endpoint": "/api/client/statistics/products/sku",
                    })
                except Exception as detail_error:
                    emit("WARNING", "商品广告缓存", "活动数据已保存，但该日 SKU 明细未更新", {
                        "day": product_day, "error": str(detail_error),
                    })
            elapsed = round(time.monotonic() - started_at, 2)
            emit("SUCCESS", "写入数据库", "广告统计已保存到本地数据库", {
                "written_rows": count,
                "elapsed_seconds": elapsed,
            })
            message = f"快速同步 {campaign_count} 个广告活动，SKU 明细 {sku_count} 条，耗时 {elapsed:.1f} 秒"
            self.database.finish_log(log_id, "success", count, message)
            emit("SUCCESS", "结束", "广告快速同步完成", {
                "campaign_count": campaign_count,
                "written_rows": count,
                "elapsed_seconds": elapsed,
            })
            return [SyncResult("Performance API", count, message)]
        except Exception as error:
            emit("ERROR", "同步失败", str(error), {
                "error_type": type(error).__name__,
                "elapsed_seconds": round(time.monotonic() - started_at, 2),
            })
            self.database.finish_log(log_id, "failed", 0, str(error))
            raise

    def sync_performance_product_detail(
        self,
        date_from: str,
        date_to: str,
        product_sku: str,
        progress: Callable[[str], None] | None = None,
        cancel_event: threading.Event | None = None,
        event_listener: LogListener | None = None,
        credentials: Credentials | None = None,
    ) -> SyncResult:
        """Reuse or fetch the official daily CPC statistics for exact SKUs."""

        sku = str(product_sku or "").strip()
        if not sku:
            raise ValueError("请选择需要补充广告明细的 Ozon SKU。")
        log_id = self.database.start_log("Performance SKU")
        emit, tracked_progress = self._log_callbacks(
            log_id, progress, event_listener
        )
        started_at = time.monotonic()
        try:
            effective_rows = self.database.effective_product_ad_rows(
                date_from, date_to, sku=sku,
            )
            if effective_rows:
                exact_count = sum(
                    1 for row in effective_rows
                    if row.get("attribution_source") in {"official_sku", "mixed"}
                )
                mapped_count = sum(
                    1 for row in effective_rows
                    if row.get("attribution_source") in {"unique_campaign_title", "mixed"}
                )
                message = (
                    f"已使用本地广告缓存，SKU {sku} 匹配 {len(effective_rows)} 个日期"
                    f"（官方 SKU {exact_count}，专属活动名映射 {mapped_count}），未调用 Ozon 接口"
                )
                emit("SUCCESS", "使用缓存", message, {
                    "product_sku": sku, "matched_days": len(effective_rows),
                    "official_days": exact_count, "mapped_days": mapped_count,
                })
                self.database.finish_log(log_id, "cached", len(effective_rows), message)
                return SyncResult("Performance SKU Cache", len(effective_rows), message)

            cache = self.database.ad_detail_cache_info(date_from, date_to, sku)
            if cache["complete"]:
                selected_count = int(cache["selected_sku_rows"] or 0)
                message = (
                    f"已使用本地广告明细缓存（全量 {cache['row_count']} 条，"
                    f"SKU {sku} 匹配 {selected_count} 条），未调用 Ozon 接口"
                )
                emit("SUCCESS", "使用缓存", message, cache)
                self.database.finish_log(log_id, "cached", selected_count, message)
                return SyncResult("Performance SKU Cache", selected_count, message)

            product_days = self._performance_product_days(date_from, date_to)
            if not product_days:
                message = (
                    "当前日期范围没有可调用的官方 SKU 明细日期。Ozon 该接口只允许今天或昨天；"
                    "历史数据仅在广告活动名称唯一等于货号或 Ozon SKU 时从本地活动缓存映射。"
                )
                emit("WARNING", "接口日期限制", message, {
                    "date_from": date_from, "date_to": date_to,
                    "product_sku": sku,
                })
                self.database.finish_log(log_id, "cached", 0, message)
                return SyncResult("Performance SKU Cache", 0, message)

            client = self.performance_client(credentials)
            emit("INFO", "开始", "开始同步所选 SKU 广告明细", {
                "product_sku": sku,
                "date_from": date_from,
                "date_to": date_to,
                "endpoint": "/api/client/statistics/products/sku",
                "cache_rule": "cache_all_sku_details_filter_selected_before_ai",
            })
            client.authenticate(tracked_progress, cancel_event)
            emit("SUCCESS", "认证", "Performance API 认证成功", {})
            campaigns = client.campaigns(tracked_progress, cancel_event)
            self.database.upsert_campaigns(campaigns)
            emit("INFO", "读取商品统计", "正在读取官方活动商品 SKU 统计", {
                "campaign_count": len(campaigns),
                "product_sku": sku,
                "note": "按广告活动读取精确 Ozon SKU；全部明细缓存本机，AI 仅筛选所选 SKU",
            })
            report_rows: list[dict[str, Any]] = []
            for product_day in product_days:
                report_rows.extend(client.product_statistics_by_sku(
                    campaigns, product_day, product_day,
                    tracked_progress, cancel_event, emit,
                ))
            detail_rows = [
                {**row, "sku": str(row.get("sku") or "").strip(), "source": "api"}
                for row in report_rows
                if str(row.get("sku") or "").strip()
            ]
            selected_count = sum(
                1 for row in detail_rows if str(row.get("sku") or "").strip() == sku
            )
            aggregate_rows = sum(
                1 for row in report_rows if not str(row.get("sku") or "").strip()
            )
            if report_rows and not detail_rows:
                raise OzonApiError(
                    "Ozon 明细报告已返回数据，但没有 SKU 字段；本次不建立空缓存，"
                    "请在数据中心复制运行明细用于校准。"
                )
            emit("INFO", "本地缓存", "广告商品明细已准备写入全量本地缓存", {
                "report_rows": len(report_rows),
                "detail_rows": len(detail_rows),
                "selected_sku_rows": selected_count,
                "discarded_aggregate_rows": aggregate_rows,
                "product_sku": sku,
                "ai_send_rule": "only_selected_sku",
            })
            cached_count = 0
            for product_day in product_days:
                cached_count += self.database.replace_ad_product_details(
                    product_day, product_day,
                    [row for row in detail_rows if str(row.get("day") or "")[:10] == product_day],
                )
            elapsed = round(time.monotonic() - started_at, 2)
            if selected_count:
                message = (
                    f"已缓存全部 SKU 广告明细 {cached_count} 条；"
                    f"SKU {sku} 匹配 {selected_count} 条，耗时 {elapsed:.1f} 秒"
                )
                emit("SUCCESS", "写入数据库", "广告商品明细缓存已保存", {
                    "product_sku": sku, "cached_rows": cached_count,
                    "selected_sku_rows": selected_count,
                    "elapsed_seconds": elapsed,
                })
            else:
                message = (
                    f"已缓存全部 SKU 广告明细 {cached_count} 条，但未匹配到 SKU {sku}；"
                    "后续选择其他 SKU 可直接筛选本地缓存，无需再次调用接口"
                )
                emit("WARNING", "未匹配", message, {
                    "product_sku": sku, "report_rows": len(report_rows),
                    "cached_rows": cached_count,
                })
            self.database.finish_log(log_id, "success", cached_count, message)
            emit("SUCCESS", "结束", "广告商品明细缓存同步完成", {
                "product_sku": sku, "cached_rows": cached_count,
                "selected_sku_rows": selected_count,
                "elapsed_seconds": elapsed,
            })
            return SyncResult("Performance SKU", selected_count, message)
        except Exception as error:
            emit("ERROR", "同步失败", str(error), {
                "product_sku": sku,
                "error_type": type(error).__name__,
                "elapsed_seconds": round(time.monotonic() - started_at, 2),
            })
            self.database.finish_log(log_id, "failed", 0, str(error))
            raise

    def sync_all(
        self, date_from: str, date_to: str, progress: Callable[[str], None] | None = None,
        cancel_event: threading.Event | None = None,
        event_listener: LogListener | None = None,
    ) -> list[SyncResult]:
        results: list[SyncResult] = []
        try:
            results.extend(self.sync_seller(
                date_from, date_to, progress, cancel_event, event_listener
            ))
        except SyncCancelled:
            raise
        except Exception as error:
            results.append(SyncResult("Seller", 0, f"Seller 同步失败，已继续同步广告：{error}"))
        results.extend(self.sync_performance(
            date_from, date_to, progress, cancel_event, event_listener
        ))
        return results

    def _log_callbacks(
        self,
        log_id: int,
        progress: Callable[[str], None] | None,
        event_listener: LogListener | None,
    ) -> tuple[
        Callable[[str, str, str, dict[str, Any]], None],
        Callable[[str], None],
    ]:
        def emit(level: str, stage: str, message: str, details: dict[str, Any]) -> None:
            self.database.append_log_event(log_id, level, stage, message, details)
            if event_listener:
                event_listener(log_id, level, stage, message, details)

        def tracked_progress(message: str) -> None:
            if progress:
                progress(message)
            emit("INFO", "网络请求", message, {})

        return emit, tracked_progress


def _supply_timezone_offset(value: Any) -> int:
    text = str(value or "0").strip()
    if text.endswith("s"):
        text = text[:-1]
    try:
        return int(float(text))
    except (TypeError, ValueError):
        return 0


def _supply_local_datetime(value: Any, offset_seconds: int) -> str:
    text = str(value or "").strip()
    if not text:
        return ""
    try:
        parsed = datetime.fromisoformat(text.replace("Z", "+00:00"))
        if parsed.tzinfo is None:
            parsed = parsed.replace(tzinfo=timezone.utc)
        target = timezone(timedelta(seconds=int(offset_seconds or 0)))
        return parsed.astimezone(target).isoformat(timespec="minutes")
    except (TypeError, ValueError, OverflowError):
        return text


def _normalize_supply_order(order: dict[str, Any]) -> dict[str, Any]:
    supplies = list(order.get("supplies") or [])
    timeslot_info = order.get("timeslot") or {}
    timeslot = timeslot_info.get("timeslot") or {}
    timezone_info = timeslot_info.get("timezone_info") or {}
    offset_seconds = _supply_timezone_offset(timezone_info.get("offset"))
    cluster_ids = sorted({
        str(item.get("macrolocal_cluster_id") or "").strip()
        for item in supplies
        if str(item.get("macrolocal_cluster_id") or "").strip()
    })
    supply_states = sorted({
        str(item.get("state") or "").strip()
        for item in supplies
        if str(item.get("state") or "").strip()
    })
    storage_names = sorted({
        str((item.get("storage_warehouse") or {}).get("name") or "").strip()
        for item in supplies
        if str((item.get("storage_warehouse") or {}).get("name") or "").strip()
    })
    crossdock_count = sum(1 for item in supplies if bool(item.get("is_crossdock")))
    dropoff = order.get("drop_off_warehouse") or {}
    raw_from = str(timeslot.get("from") or "")
    raw_to = str(timeslot.get("to") or "")
    return {
        "order_id": int(order.get("order_id") or 0),
        "order_number": str(order.get("order_number") or ""),
        "state": str(order.get("state") or ""),
        "created_date": str(order.get("created_date") or ""),
        "data_filling_deadline": str(order.get("data_filling_deadline") or ""),
        "dropoff_name": str(dropoff.get("name") or ""),
        "dropoff_address": str(dropoff.get("address") or ""),
        "timeslot_from": raw_from,
        "timeslot_to": raw_to,
        "local_timeslot_from": _supply_local_datetime(raw_from, offset_seconds),
        "local_timeslot_to": _supply_local_datetime(raw_to, offset_seconds),
        "timezone_offset_seconds": offset_seconds,
        "timezone_name": str(timezone_info.get("iana_name") or ""),
        "supplies_count": len(supplies),
        "crossdock_supplies": crossdock_count,
        "is_crossdock": crossdock_count > 0,
        "supply_type": "越库" if crossdock_count else "直送",
        "cluster_ids": cluster_ids,
        "clusters": "、".join(cluster_ids),
        "storage_warehouses": "、".join(storage_names),
        "supply_states": "、".join(supply_states),
    }


def _is_rate_limit_error(error: Exception) -> bool:
    text = str(error).lower()
    return isinstance(error, OzonApiError) and ("429" in text or "限频" in text)


def _draft_error_text(errors: list[Any]) -> str:
    if not errors:
        return ""
    messages: list[str] = []
    for item in errors:
        if isinstance(item, dict):
            message = (
                item.get("error_message") or item.get("message")
                or "；".join(map(str, item.get("error_reasons") or []))
            )
            messages.append(str(message or item))
        else:
            messages.append(str(item))
    return " 错误：" + "；".join(messages)


def _sales_sync_plan(
    date_from: str,
    date_to: str,
    cache: dict[str, Any],
    *,
    now: datetime | None = None,
    current_day_refresh_minutes: int = 30,
) -> dict[str, str]:
    """Plan the smallest safe Seller Analytics request.

    Historical data that has already been verified is reused without an API
    call.  For an evolving current-day range, a successful snapshot is reused
    for 30 minutes.  Otherwise only the last cached day plus genuinely missing
    tail dates are requested, which also keeps the response below the 1000-row
    pagination threshold for normal daily use.
    """

    now = now or datetime.now()
    requested_start = date.fromisoformat(date_from)
    requested_end = date.fromisoformat(date_to)
    rows_count = int(cache.get("rows_count") or 0)
    cached_min = _optional_date(cache.get("min_day"))
    cached_max = _optional_date(cache.get("max_day"))
    cache_covers_range = bool(
        rows_count and cached_min and cached_max
        and cached_min <= requested_start and cached_max >= requested_end
    )

    verified_through = _optional_date(cache.get("verified_through"))
    if cache_covers_range and verified_through and verified_through >= requested_end:
        return {
            "api_date_from": "",
            "mode": "verified_cache",
            "skip_reason": (
                f"所选历史区间已核验至 {verified_through.isoformat()}，"
                "本次不请求 Ozon，直接使用本地缓存"
            ),
        }

    last_success = _optional_datetime(cache.get("last_success_at"))
    last_date_to = _optional_date(cache.get("last_date_to"))
    if (
        cache_covers_range
        and requested_end >= now.date()
        and last_success
        and last_date_to
        and last_date_to >= requested_end
        and now - last_success < timedelta(minutes=current_day_refresh_minutes)
    ):
        next_refresh = last_success + timedelta(minutes=current_day_refresh_minutes)
        return {
            "api_date_from": "",
            "mode": "fresh_current_cache",
            "skip_reason": (
                f"当天 Seller 缓存刚于 {last_success.isoformat(timespec='seconds')} 更新；"
                f"为减少接口频率，{next_refresh.isoformat(timespec='seconds')} 后再刷新"
            ),
        }

    next_allowed = _optional_datetime(cache.get("next_allowed_at"))
    if rows_count and next_allowed and now < next_allowed:
        return {
            "api_date_from": "",
            "mode": "rate_limit_cooldown",
            "skip_reason": (
                f"Seller Analytics 正在限频冷却，{next_allowed.isoformat(timespec='seconds')} "
                "后再请求；本次直接使用本地缓存"
            ),
        }

    api_date_from = _incremental_sales_start(date_from, date_to, cache)
    return {
        "api_date_from": api_date_from,
        "mode": "minimal_tail" if api_date_from != date_from else "full_range",
        "skip_reason": "",
    }


def _optional_date(value: Any) -> date | None:
    text = str(value or "").strip()
    if not text:
        return None
    try:
        return date.fromisoformat(text[:10])
    except ValueError:
        return None


def _optional_datetime(value: Any) -> datetime | None:
    text = str(value or "").strip()
    if not text:
        return None
    try:
        return datetime.fromisoformat(text.replace("Z", "+00:00")).replace(tzinfo=None)
    except ValueError:
        return None


def _same_feishu_fields(existing: dict[str, Any], desired: dict[str, Any]) -> bool:
    for key, expected in desired.items():
        current = existing.get(key)
        if isinstance(expected, (int, float)) and not isinstance(expected, bool):
            number = field_number(current)
            if number is None or abs(number - float(expected)) > 0.000001:
                return False
        elif field_text(current) != str(expected).strip():
            return False
    return True


def _weekly_feishu_message(report: dict[str, Any]) -> str:
    current = report["current"]
    comparison = report["comparison"]

    def number(value: Any) -> str:
        return f"{float(value or 0):,.0f}".replace(",", " ")

    def change(key: str) -> str:
        value = comparison.get(key)
        if value is None:
            return "无可比上期"
        return f"环比 {float(value):+.1f}%"

    return "\n".join([
        f"Ozon 店铺周报｜{report['current_start']} 至 {report['current_end']}",
        "",
        f"销售额：{number(current.get('revenue'))} ₽（{change('revenue')}）",
        f"销量：{number(current.get('orders'))} 件（{change('orders')}）",
        f"广告花费：{number(current.get('spend'))} ₽（{change('spend')}）",
        f"广告订单：{number(current.get('ad_orders'))} 单（{change('ad_orders')}）",
        f"广告单占比：{float(current.get('ad_order_share') or 0):.2f}%",
        f"ACoTS：{float(current.get('acots') or 0):.2f}%",
        "",
        "由 Ozon Seller Analytics 自动生成并发送。",
    ])


def _feishu_number(value: Any, decimals: int = 0) -> str:
    if value is None:
        return "—"
    formatted = f"{float(value):,.{decimals}f}".replace(",", " ")
    return formatted


def _feishu_money(value: Any, currency: str = "₽") -> str:
    return "—" if value is None else f"{_feishu_number(value)} {currency}"


def _feishu_percent(value: Any) -> str:
    return "—" if value is None else f"{float(value):.2f}%"


def _feishu_change(value: Any) -> str:
    if value is None:
        return "—"
    number = float(value)
    if number > 0:
        return f"▲ +{number:.1f}%"
    if number < 0:
        return f"▼ {number:.1f}%"
    return "● 0.0%"


def _comparison(current: dict[str, Any], previous: dict[str, Any], key: str) -> float | None:
    old = float(previous.get(key) or 0)
    new = float(current.get(key) or 0)
    return (new / old - 1) * 100 if old else None


def _card_table_elements(
    title: str,
    headers: list[str],
    rows: list[list[str]],
    *,
    weights: list[int] | None = None,
) -> list[dict[str, Any]]:
    """Build a compact, wide-screen table from compatible column sets."""
    weights = weights or [1] * len(headers)

    def column_set(values: list[str], header: bool = False) -> dict[str, Any]:
        return {
            "tag": "column_set",
            "flex_mode": "none",
            "background_style": "grey" if header else "default",
            "columns": [
                {
                    "tag": "column",
                    "width": "weighted",
                    "weight": weights[index],
                    "elements": [{
                        "tag": "div",
                        "text": {
                            "tag": "lark_md",
                            "content": f"**{value}**" if header else str(value),
                        },
                    }],
                }
                for index, value in enumerate(values)
            ],
        }

    elements: list[dict[str, Any]] = [{
        "tag": "div", "text": {"tag": "lark_md", "content": f"**{title}**"},
    }]
    elements.append(column_set(headers, header=True))
    if rows:
        elements.extend(column_set(row) for row in rows)
    else:
        elements.append({
            "tag": "div",
            "text": {"tag": "lark_md", "content": "_当前日期范围暂无可展示数据_"},
        })
    return elements


def _first_feishu_text(fields: dict[str, Any], *names: str) -> str:
    for name in names:
        value = field_text(fields.get(name))
        if value:
            return value
    return ""


def _first_feishu_number(fields: dict[str, Any], *names: str) -> float | None:
    for name in names:
        value = field_number(fields.get(name))
        if value is not None:
            return value
    return None


def _first_feishu_date(fields: dict[str, Any], *names: str) -> str:
    for name in names:
        raw = fields.get(name)
        if raw in (None, "", []):
            continue
        if isinstance(raw, (int, float)):
            timestamp = float(raw)
            if timestamp > 100_000_000_000:
                timestamp /= 1000
            try:
                return datetime.fromtimestamp(timestamp, timezone.utc).date().isoformat()
            except (OverflowError, OSError, ValueError):
                pass
        text = field_text(raw).strip()
        if not text:
            continue
        normalized = text.replace("年", "-").replace("月", "-").replace("日", "")
        normalized = normalized.replace("/", "-").replace(".", "-")
        candidate = normalized.split(" ", 1)[0]
        try:
            return date.fromisoformat(candidate).isoformat()
        except ValueError:
            return text
    return ""


def _base_feishu_card(
    title: str, subtitle: str, *, template: str = "blue",
) -> dict[str, Any]:
    return {
        "config": {"wide_screen_mode": True, "enable_forward": True},
        "header": {
            "template": template,
            "title": {"tag": "plain_text", "content": title},
            "subtitle": {"tag": "plain_text", "content": subtitle},
        },
        "elements": [],
    }


def _shipment_arrival_feishu_card(
    row: dict[str, Any], default_shop_name: str,
) -> dict[str, Any]:
    shop_name = str(row.get("shop_name") or default_shop_name or "未命名店铺")
    tracking_id = str(row.get("tracking_id") or "—")
    foreign_arrival = str(row.get("foreign_arrival") or "—")
    card = _base_feishu_card(
        f"{shop_name}｜货物已到国外仓",
        f"到库日期：{foreign_arrival}",
        template="green",
    )
    details = [
        f"**采购/跟踪单号：** {tracking_id}",
        f"**品名：** {row.get('product_name') or '—'}",
        f"**批次号：** {row.get('batch_no') or '—'}",
        f"**数量：** {_feishu_number(row.get('quantity'))}",
        f"**渠道：** {row.get('channel') or '—'}",
        f"**货物状态：** {row.get('cargo_status') or '—'}",
        f"**国内到库：** {row.get('domestic_arrival') or '—'}",
        f"**国外到库：** {foreign_arrival}",
    ]
    card["elements"].append({
        "tag": "div",
        "text": {"tag": "lark_md", "content": "\n".join(details)},
    })
    card["elements"].append({
        "tag": "note",
        "elements": [{
            "tag": "plain_text",
            "content": "由 Ozon Seller Analytics 发货跟踪自动检测并发送",
        }],
    })
    return card


def _product_series_feishu_card(
    rows: list[dict[str, Any]],
    all_rows: list[dict[str, Any]],
    period_type: str,
    shop_name: str,
    *,
    part: int,
    total_parts: int,
) -> dict[str, Any]:
    """构建手动发送的产品系列分析卡片。"""
    page_label = f" · 第 {part}/{total_parts} 张" if total_parts > 1 else ""
    card = _base_feishu_card(
        f"{shop_name}｜产品系列分析",
        f"统计粒度：{period_type} · 手动发送{page_label}",
        template="blue",
    )
    series_names = {
        str(row.get("series") or "未分系列") for row in all_rows
    }
    total_units = sum(int(row.get("units") or 0) for row in all_rows)
    total_revenue = sum(float(row.get("revenue") or 0) for row in all_rows)
    card["elements"].extend(_card_table_elements(
        "汇总",
        ["系列数", "周期记录", "销量合计", "销售额合计"],
        [[
            f"{len(series_names)} 个", f"{len(all_rows)} 条",
            f"{_feishu_number(total_units)} 件", _feishu_money(total_revenue),
        ]],
        weights=[2, 2, 2, 3],
    ))
    detail_rows = [
        [
            str(row.get("period") or "—"),
            str(row.get("series") or "—"),
            _feishu_number(row.get("sku_count") or 0),
            _feishu_number(row.get("units") or 0),
            _feishu_money(row.get("revenue") or 0),
        ]
        for row in rows
    ]
    card["elements"].extend(_card_table_elements(
        "系列表现",
        ["周期", "产品系列", "SKU", "销量", "销售额"],
        detail_rows,
        weights=[4, 3, 1, 2, 3],
    ))
    card["elements"].append({
        "tag": "note",
        "elements": [{
            "tag": "plain_text",
            "content": "数据来自当前店铺本地缓存；由用户手动发送，不会自动写入多维表格。",
        }],
    })
    return card


def _weekly_daily_text(report: dict[str, Any]) -> str:
    lines = []
    for row in report.get("daily") or []:
        lines.append(
            f"{row.get('day', '')}｜销售额 {_feishu_number(row.get('revenue'))}｜"
            f"销量 {_feishu_number(row.get('orders'))}｜妥投 {_feishu_number(row.get('delivered'))}｜"
            f"退货 {_feishu_number(row.get('returns'))}｜取消 {_feishu_number(row.get('cancellations'))}｜"
            f"浏览 {_feishu_number(row.get('views'))}｜加购 {_feishu_number(row.get('cart_adds'))}｜"
            f"广告花费 {_feishu_number(row.get('spend'))}｜广告销售 {_feishu_number(row.get('ad_revenue'))}｜"
            f"广告订单 {_feishu_number(row.get('ad_orders'))}"
        )
    return "\n".join(lines)


def _weekly_feishu_card(
    report: dict[str, Any], shop_name: str, shop_kind_label: str,
) -> dict[str, Any]:
    current = report["current"]
    previous = report["previous"]
    comparison = report["comparison"]
    period = f"{report['current_start']} 至 {report['current_end']}"
    card = _base_feishu_card(
        f"{shop_name}｜Ozon 店铺周报",
        f"{shop_kind_label} · {period}",
        template="blue",
    )
    summary_specs = [
        ("销售额", "revenue", lambda value: _feishu_money(value)),
        ("销量", "orders", lambda value: f"{_feishu_number(value)} 件"),
        ("妥投", "delivered", lambda value: f"{_feishu_number(value)} 件"),
        ("退货", "returns", lambda value: f"{_feishu_number(value)} 件"),
        ("取消", "cancellations", lambda value: f"{_feishu_number(value)} 件"),
        ("广告花费", "spend", lambda value: _feishu_money(value)),
        ("广告销售额", "ad_revenue", lambda value: _feishu_money(value)),
        ("广告订单", "ad_orders", lambda value: f"{_feishu_number(value)} 单"),
        ("CTR", "ctr", _feishu_percent),
        ("ACOS", "acos", _feishu_percent),
        ("TACOS / ACoTS", "tacos", _feishu_percent),
        ("广告单占比", "ad_order_share", _feishu_percent),
    ]
    summary_rows: list[list[str]] = []
    for label, key, formatter in summary_specs:
        change = comparison.get(key)
        if change is None:
            change = _comparison(current, previous, key)
        summary_rows.append([
            label, formatter(current.get(key)), formatter(previous.get(key)),
            _feishu_change(change),
        ])
    card["elements"].extend(_card_table_elements(
        "核心指标（本期 / 上期 / 环比）",
        ["指标", "本期", "上期", "环比"], summary_rows,
        weights=[2, 2, 2, 2],
    ))
    daily = list(report.get("daily") or [])
    operation_rows = [[
        str(row.get("day") or "")[5:], _feishu_money(row.get("revenue")),
        _feishu_number(row.get("orders")), _feishu_number(row.get("delivered")),
        f"{_feishu_number(row.get('returns'))} / {_feishu_number(row.get('cancellations'))}",
    ] for row in daily]
    traffic_rows = [[
        str(row.get("day") or "")[5:], _feishu_number(row.get("views")),
        _feishu_number(row.get("cart_adds")), _feishu_number(row.get("impressions")),
        _feishu_number(row.get("clicks")),
    ] for row in daily]
    ad_rows = []
    for row in daily:
        revenue = float(row.get("revenue") or 0)
        ad_rows.append([
            str(row.get("day") or "")[5:], _feishu_money(row.get("spend")),
            _feishu_money(row.get("ad_revenue")), _feishu_number(row.get("ad_orders")),
            _feishu_percent(float(row.get("spend") or 0) / revenue * 100 if revenue else 0),
        ])
    card["elements"].append({"tag": "hr"})
    card["elements"].extend(_card_table_elements(
        "每日经营数据", ["日期", "销售额", "销量", "妥投", "退货 / 取消"],
        operation_rows, weights=[2, 3, 2, 2, 3],
    ))
    card["elements"].extend(_card_table_elements(
        "每日流量数据", ["日期", "浏览", "加购", "曝光", "点击"],
        traffic_rows, weights=[2, 2, 2, 3, 2],
    ))
    card["elements"].extend(_card_table_elements(
        "每日广告数据", ["日期", "花费", "广告销售", "广告订单", "ACoTS"],
        ad_rows, weights=[2, 3, 3, 2, 2],
    ))
    card["elements"].append({
        "tag": "note", "elements": [{
            "tag": "plain_text",
            "content": "数据来自本店铺本地缓存；金额单位 RUB。由 Ozon Seller Analytics 自动生成。",
        }],
    })
    return card


def _monthly_profit_feishu_card(
    summary: dict[str, Any], breakdown: list[dict[str, Any]],
    shop_name: str, shop_kind_label: str, date_from: str, date_to: str,
) -> dict[str, Any]:
    card = _base_feishu_card(
        f"{shop_name}｜月度盈亏表",
        f"{shop_kind_label} · {date_from} 至 {date_to}",
        template="orange",
    )
    summary_rows = [
        ["订单金额", _feishu_money(summary.get("revenue")), "Seller 下单日期"],
        ["已订购商品", f"{_feishu_number(summary.get('orders'))} 件", "Seller 下单日期"],
        ["销售与退货（应计）", _feishu_money(summary.get("sales_returns")), "Finance"],
        ["应计费用", _feishu_money(summary.get("accrual_fees")), "Finance"],
        ["其他收入 / 调整", _feishu_money(summary.get("other_adjustments")), "Finance"],
        ["应计净额", _feishu_money(summary.get("platform_payout")), "Finance"],
        ["成本核算金额", _feishu_money(summary.get("product_cost")), "本地成本"],
        ["头程核算金额", _feishu_money(summary.get("first_mile")), "本地成本"],
        ["暂估利润", _feishu_money(summary.get("profit")), "成本完整后计算"],
    ]
    card["elements"].extend(_card_table_elements(
        "本月汇总", ["项目", "金额 / 数量", "口径"], summary_rows,
        weights=[3, 3, 3],
    ))
    category_rows = [[
        str(row.get("category_label") or row.get("category") or "其它"),
        str(row.get("name") or row.get("api_name") or "—"),
        _feishu_number(row.get("rows_count")), _feishu_money(row.get("amount")),
    ] for row in breakdown[:15]]
    card["elements"].append({"tag": "hr"})
    card["elements"].extend(_card_table_elements(
        f"应计费用分类（按绝对金额前 {min(len(breakdown), 15)} 项）",
        ["大类", "费用项目", "记录", "金额"], category_rows,
        weights=[3, 4, 2, 3],
    ))
    missing = int(summary.get("missing_cost_skus") or 0)
    card["elements"].append({
        "tag": "note", "elements": [{
            "tag": "plain_text",
            "content": (
                f"逐笔应计 {int(summary.get('operation_count') or 0)} 条；"
                f"未分摊 {int(summary.get('unallocated_operations') or 0)} 条；"
                f"待填写成本 SKU {missing} 个。应计净额 = 销售/退货应计 + 应计费用 + 其他调整。"
            ),
        }],
    })
    return card


def _cross_border_profit_feishu_card(
    summary: dict[str, Any], weeks: list[dict[str, Any]], shop_name: str,
    date_from: str, date_to: str,
) -> dict[str, Any]:
    visible_weeks = weeks[:12]
    card = _base_feishu_card(
        f"{shop_name}｜跨境店周利润表",
        f"跨境店 · {date_from} 至 {date_to} · 1 CNY = {float(summary.get('rub_per_cny') or 0):g} RUB",
        template="green",
    )
    summary_rows = [
        ["销售额", _feishu_money(summary.get("revenue_cny"), "¥")],
        ["销量", f"{_feishu_number(summary.get('orders'))} 件"],
        ["全店广告", _feishu_money(summary.get("ad_spend_cny"), "¥")],
        ["预估平台费", _feishu_money(summary.get("estimated_platform_fees_cny"), "¥")],
        ["采购成本", _feishu_money(summary.get("purchase_cost_cny"), "¥")],
        ["期间利润", _feishu_money(summary.get("profit_cny"), "¥")],
        ["履约单 FBP / FBS / WHD", (
            f"{int(summary.get('fbp_orders') or 0)} / "
            f"{int(summary.get('rfbs_orders') or 0)} / "
            f"{int(summary.get('whd_orders') or 0)}"
        )],
    ]
    card["elements"].extend(_card_table_elements(
        "期间汇总", ["项目", "数值"], summary_rows, weights=[3, 4],
    ))
    weekly_rows = [[
        f"{row.get('date_from')} ~ {row.get('date_to')}",
        _feishu_number(row.get("orders")), _feishu_money(row.get("revenue_cny"), "¥"),
        _feishu_money(row.get("ad_spend_cny"), "¥"),
        _feishu_money(row.get("profit_cny"), "¥"),
    ] for row in visible_weeks]
    card["elements"].append({"tag": "hr"})
    card["elements"].extend(_card_table_elements(
        "每周利润", ["周期", "销量", "销售额", "广告", "利润"], weekly_rows,
        weights=[4, 2, 3, 3, 3],
    ))
    weekly_cost_rows = [[
        f"{row.get('date_from')} ~ {row.get('date_to')}",
        _feishu_money(row.get("estimated_platform_fees_cny"), "¥"),
        _feishu_money(row.get("purchase_cost_cny"), "¥"),
        _feishu_money(row.get("reference_cross_border_freight_cny"), "¥"),
    ] for row in visible_weeks]
    card["elements"].extend(_card_table_elements(
        "每周费用参考", ["周期", "平台费", "采购成本", "估算跨境运费"],
        weekly_cost_rows, weights=[4, 3, 3, 3],
    ))
    card["elements"].append({
        "tag": "note", "elements": [{
            "tag": "plain_text",
            "content": (
                f"仅显示出单商品；待补成本/重量 SKU {int(summary.get('missing_cost_skus') or 0)} 个。"
                "跨境运费仅作定价参考，不计入实时利润。"
                + (f" 卡片展示最近 12 个周期，共 {len(weeks)} 个周期。" if len(weeks) > 12 else "")
            ),
        }],
    })
    return card


def _ai_analysis_prompt(
    question: str, analysis_type: str, single_product: bool = False,
) -> str:
    question = str(question or "").strip()
    analysis_type = str(analysis_type or "custom").strip().lower()
    focus_by_type = {
        "overview": "诊断整体经营表现、趋势异常、主要风险和下一步行动。",
        "advertising": "分析广告活动的花费、归因销售、ROAS、ACOS，并给出预算与停启建议。",
        "products": "分析商品销售额、销量、转化、退货和流量表现，并给出商品优化建议。",
        "custom": "回答用户的自定义经营分析问题。",
    }
    focus = focus_by_type.get(analysis_type, focus_by_type["custom"])
    if not question:
        if analysis_type == "custom" or analysis_type not in focus_by_type:
            raise ValueError("请输入要让 AI 分析的问题。")
        question = focus

    scope_rule = (
        "本次是单商品模式：JSON 同时包含全店活动级广告效率汇总，以及所选 SKU "
        "在日期区间内的全部本地明细、全部库存集群和期间财务汇总；不得声称它只来自 Top 20。"
        "这些经营与广告数据全部来自数据中心本地缓存，本次 AI 分析不会请求 Ozon。"
        "全店广告数据只能用于店铺整体效率判断，不能归因给所选 SKU。商品广告结论应优先使用"
        " selected_product.advertising_exact_sku；若精确明细为空，只能使用"
        " locally_matched_campaign_advertising 中活动名称明确匹配 SKU/货号的本地活动数据，"
        "必须称为‘活动级推算’，不能称为 Ozon 官方 SKU 归因。不得混合两类数据，也不得将"
        "通用活动归因给所选 SKU。"
        if single_product else
        "本次是全店模式：商品和广告活动排名各限制为 Top 20，结论不得冒充全量明细。"
    )
    return (
        "请用中文分析以下 Ozon 经营数据。\n"
        f"分析重点：{focus}\n"
        f"用户问题：{question}\n\n"
        "必须遵守以下口径：\n"
        "1. ACOS = 广告花费 / 广告归因销售额 * 100%。\n"
        "2. TACOS = 广告花费 / Seller 总销售额 * 100%。\n"
        "3. 广告归因销售额与 Seller 总销售额不可混用。\n"
        "4. 只能使用所附 JSON 中的数据，不得编造任何数字、原因或平台事实。\n"
        "5. 数据为空、分母为 0 或指标口径不足时，必须明确标注‘数据不足’，不能将其解释为真实业务为 0。\n"
        f"6. {scope_rule}\n"
        "7. 先给出结论摘要，再给出数据依据、优先级建议和需要补充的数据。"
    )


def _incremental_sales_start(
    date_from: str, date_to: str, cache: dict[str, Any], days: int | None = None,
) -> str:
    requested_start = date.fromisoformat(date_from)
    requested_end = date.fromisoformat(date_to)
    cached_rows = int(cache.get("rows_count") or 0)
    cached_min_text = str(cache.get("min_day") or "")
    cached_max_text = str(cache.get("max_day") or "")
    if not cached_rows or not cached_min_text or not cached_max_text:
        return date_from
    try:
        cached_min = date.fromisoformat(cached_min_text)
        cached_max = date.fromisoformat(cached_max_text)
    except ValueError:
        return date_from
    # 历史起点已覆盖时，只重取最后一个缓存日期，再追加其后的缺失日期。
    # 最后一日可能是在当天早晨保存的不完整快照，因此不能从 cached_max + 1
    # 开始，否则会永久保留用户本次遇到的“8 月 13 日半天数据”。
    if cached_min <= requested_start and cached_max >= requested_start:
        return min(cached_max, requested_end).isoformat()
    return date_from
