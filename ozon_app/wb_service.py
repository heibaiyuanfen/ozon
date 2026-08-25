from __future__ import annotations

from datetime import datetime, timedelta
from pathlib import Path
from typing import Any

from .feishu import FeishuClient
from .wb_api import WBApiClient
from .wb_database import WBDatabase, classify_warehouse


class WBService:
    def __init__(self, data_dir: str | Path, *, api_factory: Any = WBApiClient) -> None:
        self.data_dir = Path(data_dir)
        self.db = WBDatabase(self.data_dir / "wb" / "wb_analytics.db")
        self.api_factory = api_factory

    def settings(self) -> dict[str, str]:
        return self.db.settings()

    def save_settings(self, values: dict[str, Any]) -> None:
        self.db.save_settings(values)

    def client(self) -> WBApiClient:
        return self.api_factory(self.settings()["token"])

    def test_connection(self) -> str:
        return self.client().test_connection()

    def sync(self, date_from: str, date_to: str) -> dict[str, int]:
        client = self.client()
        orders = client.orders(date_from, date_to)
        ads = client.product_ad_daily(date_from, date_to)
        offices = client.wb_offices()
        office_by_id = {str(row.get("id") or ""): row for row in offices}
        seller_warehouses: list[dict[str, Any]] = []
        for source in client.seller_warehouses():
            row = dict(source)
            office = office_by_id.get(str(row.get("officeId") or ""), {})
            for field in ("address", "city", "country", "countryName"):
                if not row.get(field) and office.get(field):
                    row[field] = office[field]
            row["_source"] = "seller"
            seller_warehouses.append(row)
        warehouses = seller_warehouses + [dict(row, _source="office") for row in offices]
        return {
            "orders": self.db.replace_orders(orders),
            "ads": self.db.replace_ads(ads),
            "warehouses": self.db.replace_warehouses(warehouses),
        }

    def daily_rows(self, date_from: str, date_to: str) -> list[dict[str, Any]]:
        settings = self.settings()
        rub_per_cny = max(float(settings.get("rub_per_cny") or 0), 0.0001)
        commission_percent = max(float(settings.get("commission_percent") or 0), 0.0)
        output: list[dict[str, Any]] = []
        for source in self.db.daily_source(date_from, date_to):
            row = dict(source)
            quantity = int(row.get("quantity") or 0)
            revenue_rub = float(row.get("revenue_rub") or 0)
            spend_rub = float(row.get("spend_rub") or 0)
            purchase_cost = _float_or_none(row.get("purchase_cost_cny"))
            mode = str(row.get("warehouse_mode") or "auto")
            if mode == "auto":
                warehouse_name = str(row.get("warehouse_name") or "")
                mode = self.db.warehouse_mode(warehouse_name) or classify_warehouse(warehouse_name)
            logistics_each = provisional_logistics_cny(
                mode,
                _float_or_none(row.get("weight_kg")),
                _float_or_none(row.get("length_cm")),
                _float_or_none(row.get("width_cm")),
                _float_or_none(row.get("height_cm")),
            )
            complete = purchase_cost is not None and logistics_each is not None
            row.update(
                {
                    "warehouse_mode_resolved": mode,
                    "revenue_cny": revenue_rub / rub_per_cny,
                    "ad_spend_cny": spend_rub / rub_per_cny,
                    "purchase_total_cny": None if purchase_cost is None else purchase_cost * quantity,
                    "logistics_each_cny": logistics_each,
                    "logistics_total_cny": None if logistics_each is None else logistics_each * quantity,
                    "commission_cny": revenue_rub / rub_per_cny * commission_percent / 100.0,
                    "profit_cny": None,
                    "complete": complete,
                }
            )
            if complete:
                row["profit_cny"] = (
                    row["revenue_cny"] - row["ad_spend_cny"]
                    - row["commission_cny"] - row["purchase_total_cny"]
                    - row["logistics_total_cny"]
                )
            output.append(row)
        return output

    def weekly_summary(self, date_from: str, date_to: str) -> dict[str, Any]:
        rows = self.daily_rows(date_from, date_to)
        complete_rows = [row for row in rows if row["complete"]]
        return {
            "date_from": date_from,
            "date_to": date_to,
            "store_name": self.settings().get("store_name") or "WB 跨境店",
            "quantity": sum(int(row.get("quantity") or 0) for row in rows),
            "revenue_cny": sum(float(row.get("revenue_cny") or 0) for row in rows),
            "ad_spend_cny": sum(float(row.get("ad_spend_cny") or 0) for row in rows),
            "commission_cny": sum(float(row.get("commission_cny") or 0) for row in rows),
            "purchase_cny": sum(float(row.get("purchase_total_cny") or 0) for row in complete_rows),
            "logistics_cny": sum(float(row.get("logistics_total_cny") or 0) for row in complete_rows),
            "profit_cny": sum(float(row.get("profit_cny") or 0) for row in complete_rows),
            "products": len({int(row.get("nm_id") or 0) for row in rows}),
            "incomplete_products": len({int(row.get("nm_id") or 0) for row in rows if not row["complete"]}),
        }

    def send_weekly_feishu(self, date_from: str, date_to: str) -> str:
        settings = self.settings()
        client = FeishuClient(settings.get("feishu_app_id", ""), settings.get("feishu_app_secret", ""))
        summary = self.weekly_summary(date_from, date_to)
        return client.send_card(settings.get("feishu_chat_id", ""), weekly_card(summary))

    def test_feishu(self) -> str:
        settings = self.settings()
        client = FeishuClient(settings.get("feishu_app_id", ""), settings.get("feishu_app_secret", ""))
        return client.test_connection()


def provisional_logistics_cny(
    mode: str,
    weight_kg: float | None,
    length_cm: float | None,
    width_cm: float | None,
    height_cm: float | None,
) -> float | None:
    """Temporary tariff supplied in screenshots; intentionally isolated for replacement."""
    if mode == "dongguan":
        if weight_kg is None or weight_kg <= 0:
            return None
        return weight_kg * (58.0 if weight_kg <= 0.3 else 43.0) + (2.0 if weight_kg <= 0.3 else 8.0)
    if mode == "overseas":
        if None in (length_cm, width_cm, height_cm):
            return None
        volume_l = float(length_cm) * float(width_cm) * float(height_cm) / 1000.0
        return 8.0 + max(volume_l - 1.0, 0.0) * 2.0
    return None


def weekly_card(summary: dict[str, Any]) -> dict[str, Any]:
    def money(value: Any) -> str:
        return f"¥{float(value or 0):,.2f}"

    fields = [
        ("销量", f"{int(summary['quantity']):,} 件"),
        ("销售额", money(summary["revenue_cny"])),
        ("广告费", money(summary["ad_spend_cny"])),
        ("暂估平台费", money(summary["commission_cny"])),
        ("采购成本", money(summary["purchase_cny"])),
        ("暂估物流", money(summary["logistics_cny"])),
        ("暂估利润", money(summary["profit_cny"])),
    ]
    elements: list[dict[str, Any]] = [
        {
            "tag": "markdown",
            "content": f"**店铺：** {summary['store_name']}\n**周期：** {summary['date_from']} 至 {summary['date_to']}",
        },
        {"tag": "hr"},
        {
            "tag": "column_set",
            "flex_mode": "none",
            "columns": [
                {
                    "tag": "column", "width": "weighted", "weight": 1,
                    "elements": [{"tag": "markdown", "content": f"**{label}**\n{value}"}],
                }
                for label, value in fields[:3]
            ],
        },
        {
            "tag": "column_set",
            "flex_mode": "none",
            "columns": [
                {
                    "tag": "column", "width": "weighted", "weight": 1,
                    "elements": [{"tag": "markdown", "content": f"**{label}**\n{value}"}],
                }
                for label, value in fields[3:6]
            ],
        },
        {"tag": "markdown", "content": f"**{fields[6][0]}**\n{fields[6][1]}"},
        {"tag": "hr"},
        {
            "tag": "note",
            "elements": [{
                "tag": "plain_text",
                "content": (
                    f"共 {summary['products']} 个出单商品；{summary['incomplete_products']} 个商品缺成本、尺寸或仓库归类。"
                    " 当前物流为暂行估算，最终利润以完整运费规则和 WB 财务结算为准。"
                ),
            }],
        },
    ]
    return {
        "config": {"wide_screen_mode": True},
        "header": {"template": "purple", "title": {"tag": "plain_text", "content": "WB 跨境店周利润"}},
        "elements": elements,
    }


def previous_full_week(today: datetime | None = None) -> tuple[str, str]:
    current = (today or datetime.now()).date()
    monday = current - timedelta(days=current.weekday())
    end = monday - timedelta(days=1)
    start = end - timedelta(days=6)
    return start.isoformat(), end.isoformat()


def _float_or_none(value: Any) -> float | None:
    if value in (None, ""):
        return None
    return float(value)
