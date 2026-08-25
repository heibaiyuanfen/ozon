from __future__ import annotations

import json
import sqlite3
from pathlib import Path
from typing import Any, Iterable

from .secrets import protect, unprotect


class WBDatabase:
    """Independent local store for WB; it never opens an Ozon database."""

    def __init__(self, path: str | Path) -> None:
        self.path = Path(path)
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self._init_schema()

    def connect(self) -> sqlite3.Connection:
        connection = sqlite3.connect(self.path)
        connection.row_factory = sqlite3.Row
        return connection

    def _init_schema(self) -> None:
        with self.connect() as db:
            db.executescript(
                """
                PRAGMA journal_mode=WAL;
                CREATE TABLE IF NOT EXISTS settings (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL DEFAULT ''
                );
                CREATE TABLE IF NOT EXISTS orders (
                    srid TEXT PRIMARY KEY,
                    day TEXT NOT NULL,
                    changed_at TEXT,
                    nm_id INTEGER NOT NULL DEFAULT 0,
                    article TEXT,
                    warehouse_name TEXT,
                    quantity INTEGER NOT NULL DEFAULT 1,
                    revenue_rub REAL NOT NULL DEFAULT 0,
                    is_cancelled INTEGER NOT NULL DEFAULT 0,
                    raw_json TEXT NOT NULL DEFAULT '{}'
                );
                CREATE INDEX IF NOT EXISTS idx_wb_orders_day ON orders(day);
                CREATE INDEX IF NOT EXISTS idx_wb_orders_product ON orders(nm_id, day);
                CREATE TABLE IF NOT EXISTS ad_daily (
                    day TEXT NOT NULL,
                    nm_id INTEGER NOT NULL,
                    campaign_id INTEGER NOT NULL,
                    spend_rub REAL NOT NULL DEFAULT 0,
                    ad_orders INTEGER NOT NULL DEFAULT 0,
                    ad_sales_rub REAL NOT NULL DEFAULT 0,
                    views INTEGER NOT NULL DEFAULT 0,
                    clicks INTEGER NOT NULL DEFAULT 0,
                    PRIMARY KEY(day, nm_id, campaign_id)
                );
                CREATE TABLE IF NOT EXISTS product_costs (
                    nm_id INTEGER PRIMARY KEY,
                    article TEXT NOT NULL DEFAULT '',
                    purchase_cost_cny REAL,
                    length_cm REAL,
                    width_cm REAL,
                    height_cm REAL,
                    weight_kg REAL,
                    warehouse_mode TEXT NOT NULL DEFAULT 'auto'
                );
                CREATE TABLE IF NOT EXISTS warehouses (
                    warehouse_key TEXT PRIMARY KEY,
                    name TEXT NOT NULL DEFAULT '',
                    address TEXT NOT NULL DEFAULT '',
                    city TEXT NOT NULL DEFAULT '',
                    country TEXT NOT NULL DEFAULT '',
                    mode TEXT NOT NULL DEFAULT 'unknown',
                    raw_json TEXT NOT NULL DEFAULT '{}'
                );
                """
            )

    def settings(self) -> dict[str, str]:
        defaults = {
            "store_name": "WB 跨境店",
            "token": "",
            "rub_per_cny": "12.0",
            "commission_percent": "15.0",
            "feishu_app_id": "",
            "feishu_app_secret": "",
            "feishu_chat_id": "",
        }
        with self.connect() as db:
            for row in db.execute("SELECT key, value FROM settings"):
                defaults[str(row["key"])] = str(row["value"])
        for key in ("token", "feishu_app_secret"):
            defaults[key] = unprotect(defaults.get(key, ""))
        return defaults

    def save_settings(self, values: dict[str, Any]) -> None:
        with self.connect() as db:
            for key, value in values.items():
                text = str(value or "").strip()
                if key in {"token", "feishu_app_secret"}:
                    text = protect(text)
                db.execute(
                    "INSERT INTO settings(key, value) VALUES(?, ?) "
                    "ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                    (key, text),
                )

    def replace_orders(self, rows: Iterable[dict[str, Any]]) -> int:
        count = 0
        with self.connect() as db:
            for row in rows:
                srid = str(row.get("srid") or row.get("gNumber") or "").strip()
                if not srid:
                    continue
                is_cancelled = bool(row.get("isCancel"))
                revenue = float(row.get("finishedPrice") or row.get("priceWithDisc") or 0)
                db.execute(
                    """
                    INSERT INTO orders(
                        srid, day, changed_at, nm_id, article, warehouse_name,
                        quantity, revenue_rub, is_cancelled, raw_json
                    ) VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                    ON CONFLICT(srid) DO UPDATE SET
                        day=excluded.day, changed_at=excluded.changed_at,
                        nm_id=excluded.nm_id, article=excluded.article,
                        warehouse_name=excluded.warehouse_name,
                        quantity=excluded.quantity, revenue_rub=excluded.revenue_rub,
                        is_cancelled=excluded.is_cancelled, raw_json=excluded.raw_json
                    """,
                    (
                        srid,
                        str(row.get("date") or "")[:10],
                        str(row.get("lastChangeDate") or ""),
                        int(row.get("nmId") or 0),
                        str(row.get("supplierArticle") or ""),
                        str(row.get("warehouseName") or ""),
                        1,
                        revenue,
                        int(is_cancelled),
                        json.dumps(row, ensure_ascii=False),
                    ),
                )
                count += 1
        return count

    def replace_ads(self, rows: Iterable[dict[str, Any]]) -> int:
        count = 0
        with self.connect() as db:
            for row in rows:
                db.execute(
                    """
                    INSERT INTO ad_daily(
                        day, nm_id, campaign_id, spend_rub, ad_orders,
                        ad_sales_rub, views, clicks
                    ) VALUES(?, ?, ?, ?, ?, ?, ?, ?)
                    ON CONFLICT(day, nm_id, campaign_id) DO UPDATE SET
                        spend_rub=excluded.spend_rub, ad_orders=excluded.ad_orders,
                        ad_sales_rub=excluded.ad_sales_rub, views=excluded.views,
                        clicks=excluded.clicks
                    """,
                    (
                        row["date"], int(row["nm_id"]), int(row["campaign_id"]),
                        float(row.get("spend_rub") or 0), int(row.get("ad_orders") or 0),
                        float(row.get("ad_sales_rub") or 0), int(row.get("views") or 0),
                        int(row.get("clicks") or 0),
                    ),
                )
                count += 1
        return count

    def replace_warehouses(self, rows: Iterable[dict[str, Any]]) -> int:
        count = 0
        with self.connect() as db:
            for row in rows:
                source = str(row.get("_source") or "warehouse")
                raw_key = str(row.get("id") or row.get("officeId") or row.get("name") or "")
                key = f"{source}:{raw_key}"
                if not key:
                    continue
                name = str(row.get("name") or "")
                address = str(row.get("address") or "")
                city = str(row.get("city") or "")
                country = str(row.get("country") or row.get("countryName") or "")
                mode = classify_warehouse(" ".join((name, address, city, country)))
                db.execute(
                    """
                    INSERT INTO warehouses(warehouse_key, name, address, city, country, mode, raw_json)
                    VALUES(?, ?, ?, ?, ?, ?, ?)
                    ON CONFLICT(warehouse_key) DO UPDATE SET name=excluded.name,
                    address=excluded.address, city=excluded.city, country=excluded.country,
                    mode=excluded.mode, raw_json=excluded.raw_json
                    """,
                    (key, name, address, city, country, mode, json.dumps(row, ensure_ascii=False)),
                )
                count += 1
        return count

    def warehouse_mode(self, name: str) -> str | None:
        normalized = str(name or "").strip().lower()
        if not normalized:
            return None
        with self.connect() as db:
            row = db.execute(
                "SELECT mode FROM warehouses "
                "WHERE lower(name)=? OR lower(address) LIKE ? OR lower(city)=? "
                "ORDER BY CASE WHEN lower(name)=? THEN 0 ELSE 1 END LIMIT 1",
                (normalized, f"%{normalized}%", normalized, normalized),
            ).fetchone()
        if row and str(row["mode"]) != "unknown":
            return str(row["mode"])
        return None

    def product_costs(self) -> list[dict[str, Any]]:
        with self.connect() as db:
            return [dict(row) for row in db.execute("SELECT * FROM product_costs ORDER BY article, nm_id")]

    def save_product_cost(self, row: dict[str, Any]) -> None:
        with self.connect() as db:
            db.execute(
                """
                INSERT INTO product_costs(
                    nm_id, article, purchase_cost_cny, length_cm, width_cm,
                    height_cm, weight_kg, warehouse_mode
                ) VALUES(?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(nm_id) DO UPDATE SET article=excluded.article,
                purchase_cost_cny=excluded.purchase_cost_cny, length_cm=excluded.length_cm,
                width_cm=excluded.width_cm, height_cm=excluded.height_cm,
                weight_kg=excluded.weight_kg, warehouse_mode=excluded.warehouse_mode
                """,
                (
                    int(row["nm_id"]), str(row.get("article") or ""),
                    _nullable_float(row.get("purchase_cost_cny")),
                    _nullable_float(row.get("length_cm")), _nullable_float(row.get("width_cm")),
                    _nullable_float(row.get("height_cm")), _nullable_float(row.get("weight_kg")),
                    str(row.get("warehouse_mode") or "auto"),
                ),
            )

    def daily_source(self, date_from: str, date_to: str) -> list[dict[str, Any]]:
        sql = """
            WITH order_rows AS (
                SELECT day, nm_id, MAX(article) article, MAX(warehouse_name) warehouse_name,
                       SUM(CASE WHEN is_cancelled=0 THEN quantity ELSE 0 END) quantity,
                       SUM(CASE WHEN is_cancelled=0 THEN revenue_rub ELSE 0 END) revenue_rub
                FROM orders WHERE day BETWEEN ? AND ? GROUP BY day, nm_id
            ), ad_rows AS (
                SELECT day, nm_id, SUM(spend_rub) spend_rub, SUM(ad_orders) ad_orders,
                       SUM(ad_sales_rub) ad_sales_rub, SUM(views) views, SUM(clicks) clicks
                FROM ad_daily WHERE day BETWEEN ? AND ? GROUP BY day, nm_id
            )
            SELECT o.*, COALESCE(a.spend_rub,0) spend_rub, COALESCE(a.ad_orders,0) ad_orders,
                   COALESCE(a.ad_sales_rub,0) ad_sales_rub, COALESCE(a.views,0) views,
                   COALESCE(a.clicks,0) clicks, c.purchase_cost_cny, c.length_cm, c.width_cm,
                   c.height_cm, c.weight_kg, COALESCE(c.warehouse_mode,'auto') warehouse_mode
            FROM order_rows o
            LEFT JOIN ad_rows a ON a.day=o.day AND a.nm_id=o.nm_id
            LEFT JOIN product_costs c ON c.nm_id=o.nm_id
            ORDER BY o.day DESC, o.revenue_rub DESC
        """
        with self.connect() as db:
            return [dict(row) for row in db.execute(sql, (date_from, date_to, date_from, date_to))]


def classify_warehouse(text: str) -> str:
    normalized = (text or "").lower()
    if any(value in normalized for value in ("东莞", "dongguan", "dpg", "guangdong", "广东")):
        return "dongguan"
    if any(value in normalized for value in ("russia", "russian", "россия", "москва", "moscow")):
        return "overseas"
    if any("а" <= char <= "я" or "А" <= char <= "Я" for char in text or ""):
        return "overseas"
    return "unknown"


def _nullable_float(value: Any) -> float | None:
    if value in (None, ""):
        return None
    return float(value)
