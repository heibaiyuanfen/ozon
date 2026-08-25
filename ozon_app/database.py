from __future__ import annotations

import csv
import json
import math
import random
import re
import sqlite3
from contextlib import contextmanager
from datetime import date, datetime, timedelta
from pathlib import Path
from typing import Any, Iterable, Iterator

from .delivery_tariffs import normalize_cluster, read_tariff_workbook


def cross_border_shipping_cny(price_cny: float, weight_kg: float) -> float | None:
    """Return cross-border freight in CNY using the user's ordered IFS rules."""
    price = float(price_cny)
    weight = float(weight_kg)
    if price < 0 or weight < 0:
        return None
    if price < 135 and weight < 0.5:
        return 3.37 + weight * 28.17
    if 135 <= price < 635 and weight < 2:
        return 17.97 + weight * 28.17
    if 635 <= price < 22525 and weight < 5:
        return 24.17 + weight * 28.17
    if price < 135 and 0.5 <= weight < 30:
        return 25.83 + weight * 19.17
    if 135 <= price < 635 and 2 <= weight < 30:
        return 40.44 + weight * 28.17
    return None


class Database:
    def __init__(self, path: Path):
        self.path = Path(path)
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.last_inventory_write_metadata: dict[str, Any] = {}
        # Parsed finance operations are expensive (10k+ JSON rows per month).
        # Keep a small per-process cache and invalidate it whenever finance data
        # changes.  This avoids parsing the same month three times per refresh.
        self._finance_revision = 0
        self._finance_snapshot_cache: dict[tuple[str, str, int], dict[str, Any]] = {}
        self._realtime_fee_profile_cache: tuple[int, dict[str, Any]] | None = None
        self._fulfillment_order_cache: dict[
            tuple[str, str, int], dict[str, dict[str, Any]]
        ] = {}
        self._fulfillment_order_index_revision = -1
        self._fulfillment_order_index: list[tuple[str, str, str]] = []
        # The official route workbook contains tens of thousands of rows.
        # Keep the parsed rows in memory after the first order-page refresh so
        # typing in the order search box does not repeatedly scan SQLite.
        self._delivery_tariff_cache: dict[
            tuple[str, str], list[dict[str, float | str]]
        ] | None = None
        self.initialize()

    @contextmanager
    def connect(self) -> Iterator[sqlite3.Connection]:
        connection = sqlite3.connect(self.path, timeout=30)
        connection.row_factory = sqlite3.Row
        try:
            yield connection
            connection.commit()
        finally:
            connection.close()

    def initialize(self) -> None:
        with self.connect() as db:
            db.executescript(
                """
                PRAGMA journal_mode=WAL;

                CREATE TABLE IF NOT EXISTS settings (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL DEFAULT ''
                );

                CREATE TABLE IF NOT EXISTS sales_daily (
                    day TEXT NOT NULL,
                    sku TEXT NOT NULL,
                    product_name TEXT NOT NULL DEFAULT '',
                    revenue REAL NOT NULL DEFAULT 0,
                    ordered_units INTEGER NOT NULL DEFAULT 0,
                    delivered_units INTEGER NOT NULL DEFAULT 0,
                    returns INTEGER NOT NULL DEFAULT 0,
                    cancellations INTEGER NOT NULL DEFAULT 0,
                    views INTEGER NOT NULL DEFAULT 0,
                    cart_adds INTEGER NOT NULL DEFAULT 0,
                    source TEXT NOT NULL DEFAULT 'api',
                    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    PRIMARY KEY (day, sku)
                );

                CREATE TABLE IF NOT EXISTS products (
                    sku TEXT PRIMARY KEY,
                    offer_id TEXT NOT NULL DEFAULT '',
                    product_id TEXT NOT NULL DEFAULT '',
                    name TEXT NOT NULL DEFAULT '',
                    image_url TEXT NOT NULL DEFAULT '',
                    source TEXT NOT NULL DEFAULT 'api',
                    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );

                CREATE TABLE IF NOT EXISTS inventory_stock (
                    sku TEXT NOT NULL,
                    offer_id TEXT NOT NULL DEFAULT '',
                    product_name TEXT NOT NULL DEFAULT '',
                    warehouse_id TEXT NOT NULL,
                    warehouse_name TEXT NOT NULL DEFAULT '',
                    cluster_id TEXT NOT NULL DEFAULT '',
                    cluster_name TEXT NOT NULL DEFAULT '',
                    macrolocal_cluster_id TEXT NOT NULL DEFAULT '',
                    ads REAL NOT NULL DEFAULT 0,
                    ads_cluster REAL NOT NULL DEFAULT 0,
                    available_stock INTEGER NOT NULL DEFAULT 0,
                    valid_stock INTEGER NOT NULL DEFAULT 0,
                    requested_stock INTEGER NOT NULL DEFAULT 0,
                    transit_stock INTEGER NOT NULL DEFAULT 0,
                    days_without_sales INTEGER NOT NULL DEFAULT 0,
                    idc_cluster INTEGER NOT NULL DEFAULT 0,
                    turnover_grade_cluster TEXT NOT NULL DEFAULT '',
                    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    PRIMARY KEY (sku, warehouse_id)
                );

                CREATE TABLE IF NOT EXISTS ozon_clusters (
                    macrolocal_cluster_id TEXT PRIMARY KEY,
                    cluster_name TEXT NOT NULL DEFAULT '',
                    country TEXT NOT NULL DEFAULT '',
                    fulfillments_json TEXT NOT NULL DEFAULT '[]',
                    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );

                CREATE TABLE IF NOT EXISTS seller_supply_warehouses (
                    seller_warehouse_id TEXT PRIMARY KEY,
                    warehouse_name TEXT NOT NULL DEFAULT '',
                    is_active INTEGER NOT NULL DEFAULT 0,
                    is_pickup INTEGER NOT NULL DEFAULT 0,
                    region TEXT NOT NULL DEFAULT '',
                    city TEXT NOT NULL DEFAULT '',
                    address TEXT NOT NULL DEFAULT '',
                    timezone TEXT NOT NULL DEFAULT '',
                    macrolocal_cluster_id TEXT NOT NULL DEFAULT '',
                    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );

                CREATE TABLE IF NOT EXISTS supply_dropoff_cache (
                    warehouse_id TEXT NOT NULL,
                    warehouse_type TEXT NOT NULL,
                    supply_type TEXT NOT NULL,
                    name TEXT NOT NULL DEFAULT '',
                    address TEXT NOT NULL DEFAULT '',
                    raw_json TEXT NOT NULL DEFAULT '{}',
                    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    PRIMARY KEY (warehouse_id, warehouse_type, supply_type)
                );

                CREATE TABLE IF NOT EXISTS replenishment_plan (
                    sku TEXT NOT NULL,
                    macrolocal_cluster_id TEXT NOT NULL,
                    planned_qty INTEGER NOT NULL DEFAULT 0,
                    target_days INTEGER NOT NULL DEFAULT 30,
                    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    PRIMARY KEY (sku, macrolocal_cluster_id)
                );

                CREATE TABLE IF NOT EXISTS return_events (
                    return_id TEXT PRIMARY KEY,
                    day TEXT NOT NULL,
                    sku TEXT NOT NULL DEFAULT '',
                    offer_id TEXT NOT NULL DEFAULT '',
                    product_name TEXT NOT NULL DEFAULT '',
                    quantity INTEGER NOT NULL DEFAULT 0,
                    source TEXT NOT NULL DEFAULT 'api',
                    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );

                CREATE TABLE IF NOT EXISTS cancellation_events (
                    event_id TEXT PRIMARY KEY,
                    day TEXT NOT NULL,
                    sku TEXT NOT NULL DEFAULT '',
                    offer_id TEXT NOT NULL DEFAULT '',
                    product_name TEXT NOT NULL DEFAULT '',
                    quantity INTEGER NOT NULL DEFAULT 0,
                    scheme TEXT NOT NULL DEFAULT '',
                    source TEXT NOT NULL DEFAULT 'api',
                    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );

                CREATE TABLE IF NOT EXISTS delivery_events (
                    event_id TEXT PRIMARY KEY,
                    day TEXT NOT NULL,
                    sku TEXT NOT NULL DEFAULT '',
                    offer_id TEXT NOT NULL DEFAULT '',
                    product_name TEXT NOT NULL DEFAULT '',
                    quantity INTEGER NOT NULL DEFAULT 0,
                    scheme TEXT NOT NULL DEFAULT '',
                    posting_number TEXT NOT NULL DEFAULT '',
                    origin TEXT NOT NULL DEFAULT '',
                    destination TEXT NOT NULL DEFAULT '',
                    order_price REAL NOT NULL DEFAULT 0,
                    commission REAL NOT NULL DEFAULT 0,
                    payout REAL NOT NULL DEFAULT 0,
                    source TEXT NOT NULL DEFAULT 'api',
                    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );

                CREATE TABLE IF NOT EXISTS posting_routes (
                    event_id TEXT PRIMARY KEY,
                    day TEXT NOT NULL,
                    sku TEXT NOT NULL DEFAULT '',
                    offer_id TEXT NOT NULL DEFAULT '',
                    product_name TEXT NOT NULL DEFAULT '',
                    quantity INTEGER NOT NULL DEFAULT 0,
                    scheme TEXT NOT NULL DEFAULT '',
                    status TEXT NOT NULL DEFAULT '',
                    posting_number TEXT NOT NULL DEFAULT '',
                    origin TEXT NOT NULL DEFAULT '',
                    destination TEXT NOT NULL DEFAULT '',
                    order_price REAL NOT NULL DEFAULT 0,
                    commission REAL NOT NULL DEFAULT 0,
                    payout REAL NOT NULL DEFAULT 0,
                    source TEXT NOT NULL DEFAULT 'api',
                    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );

                CREATE TABLE IF NOT EXISTS data_coverage (
                    day TEXT NOT NULL,
                    metric TEXT NOT NULL,
                    source TEXT NOT NULL DEFAULT 'api',
                    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    PRIMARY KEY (day, metric)
                );

                CREATE TABLE IF NOT EXISTS ad_daily (
                    day TEXT NOT NULL,
                    campaign_id TEXT NOT NULL,
                    campaign_name TEXT NOT NULL DEFAULT '',
                    sku TEXT NOT NULL DEFAULT '',
                    impressions INTEGER NOT NULL DEFAULT 0,
                    clicks INTEGER NOT NULL DEFAULT 0,
                    cart_adds INTEGER NOT NULL DEFAULT 0,
                    orders INTEGER NOT NULL DEFAULT 0,
                    revenue REAL NOT NULL DEFAULT 0,
                    spend REAL NOT NULL DEFAULT 0,
                    source TEXT NOT NULL DEFAULT 'api',
                    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    PRIMARY KEY (day, campaign_id, sku)
                );

                CREATE TABLE IF NOT EXISTS ad_detail_cache (
                    date_from TEXT NOT NULL,
                    date_to TEXT NOT NULL,
                    row_count INTEGER NOT NULL DEFAULT 0,
                    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    PRIMARY KEY (date_from, date_to)
                );

                CREATE TABLE IF NOT EXISTS campaigns (
                    campaign_id TEXT PRIMARY KEY,
                    name TEXT NOT NULL DEFAULT '',
                    state TEXT NOT NULL DEFAULT '',
                    payment_type TEXT NOT NULL DEFAULT '',
                    budget REAL NOT NULL DEFAULT 0,
                    source TEXT NOT NULL DEFAULT 'api',
                    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );

                CREATE TABLE IF NOT EXISTS finance_transactions (
                    operation_id TEXT PRIMARY KEY,
                    operation_date TEXT NOT NULL,
                    operation_type TEXT NOT NULL DEFAULT '',
                    posting_number TEXT NOT NULL DEFAULT '',
                    sku TEXT NOT NULL DEFAULT '',
                    amount REAL NOT NULL DEFAULT 0,
                    delivery_charge REAL NOT NULL DEFAULT 0,
                    return_delivery_charge REAL NOT NULL DEFAULT 0,
                    accruals_for_sale REAL NOT NULL DEFAULT 0,
                    sale_commission REAL NOT NULL DEFAULT 0,
                    raw_json TEXT NOT NULL DEFAULT '{}'
                );

                CREATE TABLE IF NOT EXISTS finance_cash_flows (
                    row_id TEXT PRIMARY KEY,
                    period_from TEXT NOT NULL DEFAULT '',
                    period_to TEXT NOT NULL DEFAULT '',
                    currency_code TEXT NOT NULL DEFAULT '',
                    raw_json TEXT NOT NULL DEFAULT '{}',
                    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );

                CREATE TABLE IF NOT EXISTS finance_cash_flow_details (
                    row_id TEXT PRIMARY KEY,
                    period_from TEXT NOT NULL DEFAULT '',
                    period_to TEXT NOT NULL DEFAULT '',
                    raw_json TEXT NOT NULL DEFAULT '{}',
                    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );

                CREATE TABLE IF NOT EXISTS product_costs (
                    sku TEXT PRIMARY KEY,
                    unit_cost REAL,
                    first_mile_cost REAL,
                    unit_cost_cny REAL,
                    first_mile_cost_cny REAL,
                    length_cm REAL,
                    width_cm REAL,
                    height_cm REAL,
                    weight_kg REAL,
                    note TEXT NOT NULL DEFAULT '',
                    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );

                CREATE TABLE IF NOT EXISTS delivery_tariffs (
                    origin TEXT NOT NULL,
                    destination TEXT NOT NULL,
                    volume_min_l REAL NOT NULL,
                    volume_max_l REAL NOT NULL,
                    price_le_300 REAL NOT NULL DEFAULT 0,
                    price_gt_300 REAL NOT NULL DEFAULT 0,
                    source_file TEXT NOT NULL DEFAULT '',
                    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    PRIMARY KEY (origin, destination, volume_min_l, volume_max_l)
                );

                CREATE TABLE IF NOT EXISTS shipment_tracking (
                    tracking_id TEXT PRIMARY KEY,
                    product_name TEXT NOT NULL DEFAULT '',
                    batch_no TEXT NOT NULL DEFAULT '',
                    shop_name TEXT NOT NULL DEFAULT '',
                    quantity INTEGER NOT NULL DEFAULT 0,
                    cargo_status TEXT NOT NULL DEFAULT '',
                    channel TEXT NOT NULL DEFAULT '',
                    domestic_arrival TEXT NOT NULL DEFAULT '',
                    foreign_arrival TEXT NOT NULL DEFAULT '',
                    notified_foreign_arrival TEXT NOT NULL DEFAULT '',
                    source TEXT NOT NULL DEFAULT 'local',
                    remote_record_id TEXT NOT NULL DEFAULT '',
                    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );

                CREATE TABLE IF NOT EXISTS sync_logs (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    started_at TEXT NOT NULL,
                    finished_at TEXT,
                    source TEXT NOT NULL,
                    status TEXT NOT NULL,
                    rows_count INTEGER NOT NULL DEFAULT 0,
                    message TEXT NOT NULL DEFAULT ''
                );

                CREATE TABLE IF NOT EXISTS sync_log_events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    log_id INTEGER NOT NULL,
                    event_at TEXT NOT NULL,
                    level TEXT NOT NULL DEFAULT 'INFO',
                    stage TEXT NOT NULL DEFAULT '',
                    message TEXT NOT NULL DEFAULT '',
                    details_json TEXT NOT NULL DEFAULT '{}',
                    FOREIGN KEY(log_id) REFERENCES sync_logs(id) ON DELETE CASCADE
                );

                CREATE INDEX IF NOT EXISTS idx_sync_log_events_log_id
                    ON sync_log_events(log_id, id);
                CREATE INDEX IF NOT EXISTS idx_return_events_day_sku
                    ON return_events(day, sku);
                CREATE INDEX IF NOT EXISTS idx_cancellation_events_day_sku
                    ON cancellation_events(day, sku);
                CREATE INDEX IF NOT EXISTS idx_delivery_events_day_sku
                    ON delivery_events(day, sku);
                CREATE INDEX IF NOT EXISTS idx_posting_routes_day_sku
                    ON posting_routes(day, sku);
                CREATE INDEX IF NOT EXISTS idx_inventory_stock_sku_cluster
                    ON inventory_stock(sku, macrolocal_cluster_id);
                CREATE INDEX IF NOT EXISTS idx_supply_dropoff_cache_type_name
                    ON supply_dropoff_cache(supply_type, name);
                CREATE INDEX IF NOT EXISTS idx_finance_cash_flow_period
                    ON finance_cash_flows(period_from, period_to);
                CREATE INDEX IF NOT EXISTS idx_finance_cash_flow_detail_period
                    ON finance_cash_flow_details(period_from, period_to);
                CREATE INDEX IF NOT EXISTS idx_delivery_tariffs_route_volume
                    ON delivery_tariffs(origin, destination, volume_min_l, volume_max_l);
                CREATE INDEX IF NOT EXISTS idx_shipment_tracking_foreign_arrival
                    ON shipment_tracking(foreign_arrival, notified_foreign_arrival);
                """
            )
            campaign_columns = {
                row["name"] for row in db.execute("PRAGMA table_info(campaigns)").fetchall()
            }
            if "source" not in campaign_columns:
                db.execute("ALTER TABLE campaigns ADD COLUMN source TEXT NOT NULL DEFAULT 'api'")
            cost_columns = {
                row["name"] for row in db.execute("PRAGMA table_info(product_costs)").fetchall()
            }
            for name in (
                "unit_cost_cny", "first_mile_cost_cny", "length_cm", "width_cm",
                "height_cm", "weight_kg",
            ):
                if name not in cost_columns:
                    db.execute(f"ALTER TABLE product_costs ADD COLUMN {name} REAL")
            delivery_columns = {
                row["name"] for row in db.execute("PRAGMA table_info(delivery_events)").fetchall()
            }
            for name, declaration in (
                ("posting_number", "TEXT NOT NULL DEFAULT ''"),
                ("origin", "TEXT NOT NULL DEFAULT ''"),
                ("destination", "TEXT NOT NULL DEFAULT ''"),
                ("order_price", "REAL NOT NULL DEFAULT 0"),
                ("commission", "REAL NOT NULL DEFAULT 0"),
                ("payout", "REAL NOT NULL DEFAULT 0"),
            ):
                if name not in delivery_columns:
                    db.execute(f"ALTER TABLE delivery_events ADD COLUMN {name} {declaration}")
            product_columns = {
                str(row[1]) for row in db.execute("PRAGMA table_info(products)").fetchall()
            }
            if "image_url" not in product_columns:
                db.execute("ALTER TABLE products ADD COLUMN image_url TEXT NOT NULL DEFAULT ''")
            # 兼容 0.1.0 已生成的演示活动：旧表尚无 campaigns.source 字段。
            db.execute(
                """
                UPDATE campaigns SET source='demo'
                WHERE campaign_id IN (
                    SELECT DISTINCT campaign_id FROM ad_daily WHERE source='demo'
                )
                AND campaign_id NOT IN (
                    SELECT DISTINCT campaign_id FROM ad_daily WHERE source='api'
                )
                """
            )
            # v0.3.0 treated every row from /v1/returns/list as a customer
            # return.  That endpoint also contains Cancellation/logistics
            # records, so the derived cache cannot be repaired without reading
            # the source again.  Clear it once and let the next Seller sync
            # rebuild only ClientReturn rows.
            return_migration = db.execute(
                "SELECT value FROM settings WHERE key='return_events_business_type_version'"
            ).fetchone()
            if not return_migration:
                db.execute("DELETE FROM return_events WHERE source='api'")
                db.execute(
                    "DELETE FROM data_coverage WHERE source='api' AND metric='returns'"
                )
                db.execute(
                    "INSERT INTO settings(key, value) VALUES"
                    "('return_events_business_type_version', '2')"
                )
            return_order_date_migration = db.execute(
                "SELECT value FROM settings WHERE key='return_events_order_date_version'"
            ).fetchone()
            if not return_order_date_migration:
                db.execute(
                    "DELETE FROM return_events WHERE source IN ('api', 'finance')"
                )
                db.execute(
                    "DELETE FROM data_coverage WHERE source='api' AND metric='returns'"
                )
                db.execute(
                    "INSERT INTO settings(key, value) VALUES"
                    "('return_events_order_date_version', '1')"
                )

    def get_setting(self, key: str, default: str = "") -> str:
        with self.connect() as db:
            row = db.execute("SELECT value FROM settings WHERE key = ?", (key,)).fetchone()
        return row["value"] if row else default

    def save_settings(self, values: dict[str, str]) -> None:
        with self.connect() as db:
            db.executemany(
                """INSERT INTO settings(key, value) VALUES(?, ?)
                   ON CONFLICT(key) DO UPDATE SET value = excluded.value""",
                values.items(),
            )

    def mark_coverage(
        self, date_from: str, date_to: str, metric: str, source: str = "api",
    ) -> None:
        """Record that an API really returned a metric for every day in a range."""
        with self.connect() as db:
            self._mark_coverage_in_connection(db, date_from, date_to, metric, source)

    def save_product_cost(
        self,
        sku: str,
        unit_cost: float | None,
        first_mile_cost: float | None,
        note: str = "",
    ) -> None:
        with self.connect() as db:
            db.execute(
                """
                INSERT INTO product_costs(sku, unit_cost, first_mile_cost, note)
                VALUES (?, ?, ?, ?)
                ON CONFLICT(sku) DO UPDATE SET
                    unit_cost=excluded.unit_cost,
                    first_mile_cost=excluded.first_mile_cost,
                    note=excluded.note,
                    updated_at=CURRENT_TIMESTAMP
                """,
                (sku, unit_cost, first_mile_cost, note),
            )

    def save_product_costs(
        self,
        skus: Iterable[str],
        unit_cost: float | None,
        first_mile_cost: float | None,
        note: str = "",
    ) -> int:
        """Save the same local cost values for a deduplicated SKU collection."""
        unique_skus = tuple(dict.fromkeys(
            str(sku or "").strip() for sku in skus if str(sku or "").strip()
        ))
        if not unique_skus:
            return 0
        with self.connect() as db:
            db.executemany(
                """
                INSERT INTO product_costs(sku, unit_cost, first_mile_cost, note)
                VALUES (?, ?, ?, ?)
                ON CONFLICT(sku) DO UPDATE SET
                    unit_cost=excluded.unit_cost,
                    first_mile_cost=excluded.first_mile_cost,
                    note=CASE
                        WHEN excluded.note = '' THEN product_costs.note
                        ELSE excluded.note
                    END,
                    updated_at=CURRENT_TIMESTAMP
                """,
                ((sku, unit_cost, first_mile_cost, note) for sku in unique_skus),
            )
        return len(unique_skus)

    def rub_per_cny(self) -> float:
        try:
            value = float(self.get_setting("rub_per_cny", "11.5") or 11.5)
        except (TypeError, ValueError):
            value = 11.5
        return value if value > 0 else 11.5

    def save_rub_per_cny(self, value: float) -> float:
        rate = float(value)
        if not math.isfinite(rate) or rate <= 0:
            raise ValueError("人民币兑卢布汇率必须大于 0。")
        self.save_settings({"rub_per_cny": f"{rate:.8f}".rstrip("0").rstrip(".")})
        return rate

    def save_product_cost_cny(
        self, sku: str, unit_cost_cny: float | None,
        first_mile_cost_cny: float | None, *, length_cm: float | None = None,
        width_cm: float | None = None, height_cm: float | None = None,
        weight_kg: float | None = None, note: str = "",
    ) -> None:
        sku = str(sku or "").strip()
        if not sku:
            raise ValueError("SKU 不能为空。")
        with self.connect() as db:
            db.execute(
                """
                INSERT INTO product_costs(
                    sku, unit_cost_cny, first_mile_cost_cny,
                    length_cm, width_cm, height_cm, weight_kg, note
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(sku) DO UPDATE SET
                    unit_cost_cny=excluded.unit_cost_cny,
                    first_mile_cost_cny=excluded.first_mile_cost_cny,
                    length_cm=excluded.length_cm,
                    width_cm=excluded.width_cm,
                    height_cm=excluded.height_cm,
                    weight_kg=excluded.weight_kg,
                    note=excluded.note,
                    updated_at=CURRENT_TIMESTAMP
                """,
                (sku, unit_cost_cny, first_mile_cost_cny, length_cm, width_cm,
                 height_cm, weight_kg, note),
            )

    def save_product_cost_mixed(
        self, sku: str, unit_cost_cny: float | None,
        first_mile_cost_rub: float | None, *, length_cm: float | None = None,
        width_cm: float | None = None, height_cm: float | None = None,
        weight_kg: float | None = None, note: str = "",
    ) -> None:
        """Save purchasing cost in CNY and first-mile cost directly in RUB."""
        sku = str(sku or "").strip()
        if not sku:
            raise ValueError("SKU 不能为空。")
        with self.connect() as db:
            db.execute(
                """
                INSERT INTO product_costs(
                    sku, unit_cost_cny, first_mile_cost, first_mile_cost_cny,
                    length_cm, width_cm, height_cm, weight_kg, note
                ) VALUES (?, ?, ?, NULL, ?, ?, ?, ?, ?)
                ON CONFLICT(sku) DO UPDATE SET
                    unit_cost_cny=excluded.unit_cost_cny,
                    first_mile_cost=excluded.first_mile_cost,
                    first_mile_cost_cny=NULL,
                    length_cm=excluded.length_cm,
                    width_cm=excluded.width_cm,
                    height_cm=excluded.height_cm,
                    weight_kg=excluded.weight_kg,
                    note=excluded.note,
                    updated_at=CURRENT_TIMESTAMP
                """,
                (sku, unit_cost_cny, first_mile_cost_rub, length_cm, width_cm,
                 height_cm, weight_kg, note),
            )

    def save_product_costs_mixed(
        self, skus: Iterable[str], unit_cost_cny: float | None,
        first_mile_cost_rub: float | None, note: str = "",
        length_cm: float | None = None, width_cm: float | None = None,
        height_cm: float | None = None, weight_kg: float | None = None,
    ) -> int:
        unique_skus = tuple(dict.fromkeys(
            str(sku or "").strip() for sku in skus if str(sku or "").strip()
        ))
        if not unique_skus:
            return 0
        with self.connect() as db:
            db.executemany(
                """
                INSERT INTO product_costs(
                    sku, unit_cost_cny, first_mile_cost, first_mile_cost_cny, note,
                    length_cm, width_cm, height_cm, weight_kg
                ) VALUES (?, ?, ?, NULL, ?, ?, ?, ?, ?)
                ON CONFLICT(sku) DO UPDATE SET
                    unit_cost_cny=excluded.unit_cost_cny,
                    first_mile_cost=excluded.first_mile_cost,
                    first_mile_cost_cny=NULL,
                    length_cm=excluded.length_cm,
                    width_cm=excluded.width_cm,
                    height_cm=excluded.height_cm,
                    weight_kg=excluded.weight_kg,
                    note=CASE WHEN excluded.note='' THEN product_costs.note ELSE excluded.note END,
                    updated_at=CURRENT_TIMESTAMP
                """,
                ((sku, unit_cost_cny, first_mile_cost_rub, note, length_cm,
                  width_cm, height_cm, weight_kg) for sku in unique_skus),
            )
        return len(unique_skus)

    def save_product_costs_cny(
        self, skus: Iterable[str], unit_cost_cny: float | None,
        first_mile_cost_cny: float | None, note: str = "",
    ) -> int:
        unique_skus = tuple(dict.fromkeys(
            str(sku or "").strip() for sku in skus if str(sku or "").strip()
        ))
        if not unique_skus:
            return 0
        with self.connect() as db:
            db.executemany(
                """
                INSERT INTO product_costs(sku, unit_cost_cny, first_mile_cost_cny, note)
                VALUES (?, ?, ?, ?)
                ON CONFLICT(sku) DO UPDATE SET
                    unit_cost_cny=excluded.unit_cost_cny,
                    first_mile_cost_cny=excluded.first_mile_cost_cny,
                    note=CASE WHEN excluded.note='' THEN product_costs.note ELSE excluded.note END,
                    updated_at=CURRENT_TIMESTAMP
                """,
                ((sku, unit_cost_cny, first_mile_cost_cny, note) for sku in unique_skus),
            )
        return len(unique_skus)

    def import_listing_ledger_costs(
        self, records: Iterable[dict[str, Any]], source_label: str = "上品工具台账",
    ) -> dict[str, Any]:
        """Match ledger offers to this shop only and merge their CNY cost fields."""
        source_records = [dict(record or {}) for record in records]
        local_products = self.product_sync_rows()
        by_offer: dict[str, list[str]] = {}
        known_skus: set[str] = set()
        for product in local_products:
            sku = str(product.get("sku") or "").strip()
            offer_id = str(product.get("offer_id") or "").strip().casefold()
            if sku:
                known_skus.add(sku)
            if sku and offer_id:
                by_offer.setdefault(offer_id, []).append(sku)
        values: list[tuple[Any, ...]] = []
        unmatched: list[str] = []
        matched_offers: set[str] = set()
        for record in source_records:
            offer_id = str(record.get("offer_id") or "").strip()
            product_id = str(record.get("ozon_product_id") or "").strip()
            targets = list(by_offer.get(offer_id.casefold(), ()))
            if not targets and product_id in known_skus:
                targets = [product_id]
            if not targets:
                unmatched.append(offer_id or product_id or "（无货号）")
                continue
            matched_offers.add(offer_id)
            for sku in dict.fromkeys(targets):
                values.append((
                    sku, record.get("unit_cost_cny"), record.get("length_cm"),
                    record.get("width_cm"), record.get("height_cm"),
                    record.get("weight_kg"), source_label,
                ))
        if values:
            with self.connect() as db:
                db.executemany(
                    """
                    INSERT INTO product_costs(
                        sku, unit_cost_cny, length_cm, width_cm, height_cm,
                        weight_kg, note
                    ) VALUES (?, ?, ?, ?, ?, ?, ?)
                    ON CONFLICT(sku) DO UPDATE SET
                        unit_cost_cny=COALESCE(excluded.unit_cost_cny, product_costs.unit_cost_cny),
                        length_cm=COALESCE(excluded.length_cm, product_costs.length_cm),
                        width_cm=COALESCE(excluded.width_cm, product_costs.width_cm),
                        height_cm=COALESCE(excluded.height_cm, product_costs.height_cm),
                        weight_kg=COALESCE(excluded.weight_kg, product_costs.weight_kg),
                        note=CASE WHEN product_costs.note='' THEN excluded.note ELSE product_costs.note END,
                        updated_at=CURRENT_TIMESTAMP
                    """,
                    values,
                )
        return {
            "ledger_rows": len(source_records),
            "matched_offers": len(matched_offers),
            "updated_skus": len({str(value[0]) for value in values}),
            "unmatched_offers": tuple(dict.fromkeys(unmatched)),
        }

    def find_product_cost_targets(
        self, query: str, *, prefix: bool = False,
    ) -> list[dict[str, Any]]:
        """Find local products by offer/SKU for fuzzy selection or family cost updates."""
        normalized = str(query or "").strip().casefold()
        if not normalized:
            return []
        result: list[dict[str, Any]] = []
        for row in self.product_sync_rows():
            offer_id = str(row.get("offer_id") or "").strip().casefold()
            sku = str(row.get("sku") or "").strip().casefold()
            values = (offer_id, sku)
            matched = (
                any(value.startswith(normalized) for value in values if value)
                if prefix else any(normalized in value for value in values if value)
            )
            if matched:
                result.append(row)
        result.sort(key=lambda row: (
            str(row.get("offer_id") or "").casefold(), str(row.get("sku") or "")
        ))
        return result

    def product_sync_rows(self) -> list[dict[str, Any]]:
        """Return one Feishu-ready row for every locally known product SKU."""
        with self.connect() as db:
            rows = db.execute(
                """
                WITH product_keys AS (
                    SELECT sku FROM products WHERE sku <> ''
                    UNION SELECT sku FROM sales_daily WHERE sku <> ''
                    UNION SELECT sku FROM product_costs WHERE sku <> ''
                ), sales AS (
                    SELECT sku, MAX(product_name) product_name,
                           MAX(updated_at) updated_at
                    FROM sales_daily WHERE sku <> '' GROUP BY sku
                )
                SELECT k.sku,
                       COALESCE(p.offer_id, '') offer_id,
                       CASE WHEN COALESCE(p.name, '') <> '' THEN p.name
                            ELSE COALESCE(s.product_name, '') END product_name,
                       pc.unit_cost, pc.first_mile_cost,
                       pc.unit_cost_cny, pc.first_mile_cost_cny,
                       pc.length_cm, pc.width_cm, pc.height_cm, pc.weight_kg,
                       COALESCE(pc.note, '') note,
                       MAX(COALESCE(p.updated_at, ''),
                           COALESCE(s.updated_at, ''),
                           COALESCE(pc.updated_at, '')) local_updated_at
                FROM product_keys k
                LEFT JOIN products p ON p.sku = k.sku
                LEFT JOIN sales s ON s.sku = k.sku
                LEFT JOIN product_costs pc ON pc.sku = k.sku
                ORDER BY k.sku
                """
            ).fetchall()
        return [dict(row) for row in rows]

    def product_cost_rows(self, query: str = "") -> list[dict[str, Any]]:
        """All known SKUs with CNY source values and live RUB conversions."""
        normalized = str(query or "").strip().casefold()
        rate = self.rub_per_cny()
        result: list[dict[str, Any]] = []
        for raw in self.product_sync_rows():
            row = dict(raw)
            searchable = " ".join((
                str(row.get("offer_id") or ""), str(row.get("sku") or ""),
            )).casefold()
            if normalized and normalized not in searchable:
                continue
            unit_cny = row.get("unit_cost_cny")
            first_cny = row.get("first_mile_cost_cny")
            row["rub_per_cny"] = rate
            row["unit_cost_rub"] = (
                float(unit_cny) * rate if unit_cny is not None else row.get("unit_cost")
            )
            row["first_mile_cost_rub"] = (
                row.get("first_mile_cost") if row.get("first_mile_cost") is not None
                else (float(first_cny) * rate if first_cny is not None else None)
            )
            row["cost_currency"] = "CNY" if unit_cny is not None else (
                "RUB（旧数据）" if row.get("unit_cost") is not None else ""
            )
            result.append(row)
        result.sort(key=lambda row: (
            str(row.get("offer_id") or "").casefold(), str(row.get("sku") or "")
        ))
        return result

    def replace_inventory_stock(
        self, rows: Iterable[dict[str, Any]], replace_skus: Iterable[str] | None = None,
    ) -> int:
        source_rows = list(rows)
        merged: dict[tuple[str, str], dict[str, Any]] = {}
        duplicate_rows = 0
        fallback_warehouse_ids = 0
        invalid_rows = 0
        numeric_fields = (
            "ads", "ads_cluster", "available_stock", "valid_stock",
            "requested_stock", "transit_stock", "days_without_sales", "idc_cluster",
        )
        text_fields = (
            "offer_id", "product_name", "warehouse_name", "cluster_id",
            "cluster_name", "macrolocal_cluster_id", "turnover_grade_cluster",
        )
        for raw in source_rows:
            sku = str(raw.get("sku") or "").strip()
            if not sku:
                invalid_rows += 1
                continue
            warehouse_id = str(raw.get("warehouse_id") or "").strip()
            if not warehouse_id or warehouse_id == "0":
                # Some accounts return virtual cluster stock rows without a
                # physical warehouse ID.  Keep them under a stable cluster key
                # instead of collapsing every such row to the empty string.
                fallback_part = next((
                    str(raw.get(field) or "").strip()
                    for field in (
                        "macrolocal_cluster_id", "cluster_id",
                        "warehouse_name", "cluster_name",
                    )
                    if str(raw.get(field) or "").strip()
                ), "unknown")
                warehouse_id = f"cluster:{fallback_part}"
                fallback_warehouse_ids += 1
            item: dict[str, Any] = {
                "sku": sku,
                "warehouse_id": warehouse_id,
                "offer_id": str(raw.get("offer_id") or ""),
                "product_name": str(raw.get("product_name") or raw.get("name") or ""),
                "warehouse_name": str(raw.get("warehouse_name") or ""),
                "cluster_id": str(raw.get("cluster_id") or ""),
                "cluster_name": str(raw.get("cluster_name") or ""),
                "macrolocal_cluster_id": str(raw.get("macrolocal_cluster_id") or ""),
                "ads": _number(raw.get("ads")),
                "ads_cluster": _number(raw.get("ads_cluster")),
                "available_stock": _integer(raw.get("available_stock")),
                "valid_stock": _integer(raw.get("valid_stock")),
                "requested_stock": _integer(raw.get("requested_stock")),
                "transit_stock": _integer(raw.get("transit_stock")),
                "days_without_sales": _integer(raw.get("days_without_sales")),
                "idc_cluster": _integer(raw.get("idc_cluster")),
                "turnover_grade_cluster": str(raw.get("turnover_grade_cluster") or ""),
            }
            key = (sku, warehouse_id)
            current = merged.get(key)
            if current is None:
                merged[key] = item
                continue
            duplicate_rows += 1
            # The endpoint may repeat the same SKU/warehouse snapshot.  Taking
            # maxima prevents duplicated API rows from inflating real stock.
            for field in numeric_fields:
                current[field] = max(current[field], item[field])
            for field in text_fields:
                if not current[field] and item[field]:
                    current[field] = item[field]

        values = [
            (
                item["sku"], item["offer_id"], item["product_name"],
                item["warehouse_id"], item["warehouse_name"], item["cluster_id"],
                item["cluster_name"], item["macrolocal_cluster_id"], item["ads"],
                item["ads_cluster"], item["available_stock"], item["valid_stock"],
                item["requested_stock"], item["transit_stock"],
                item["days_without_sales"], item["idc_cluster"],
                item["turnover_grade_cluster"],
            )
            for item in merged.values()
        ]
        self.last_inventory_write_metadata = {
            "api_rows": len(source_rows),
            "written_rows": len(values),
            "duplicate_rows_merged": duplicate_rows,
            "fallback_warehouse_ids": fallback_warehouse_ids,
            "invalid_rows_skipped": invalid_rows,
        }
        with self.connect() as db:
            if replace_skus is None:
                db.execute("DELETE FROM inventory_stock")
            else:
                targets = [str(value) for value in replace_skus if str(value)]
                if targets:
                    placeholders = ",".join("?" for _ in targets)
                    db.execute(
                        f"DELETE FROM inventory_stock WHERE sku IN ({placeholders})", targets
                    )
            db.executemany(
                """
                INSERT INTO inventory_stock(
                    sku, offer_id, product_name, warehouse_id, warehouse_name,
                    cluster_id, cluster_name, macrolocal_cluster_id, ads, ads_cluster,
                    available_stock, valid_stock, requested_stock, transit_stock,
                    days_without_sales, idc_cluster, turnover_grade_cluster
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(sku, warehouse_id) DO UPDATE SET
                    offer_id=excluded.offer_id,
                    product_name=excluded.product_name,
                    warehouse_name=excluded.warehouse_name,
                    cluster_id=excluded.cluster_id,
                    cluster_name=excluded.cluster_name,
                    macrolocal_cluster_id=excluded.macrolocal_cluster_id,
                    ads=excluded.ads,
                    ads_cluster=excluded.ads_cluster,
                    available_stock=excluded.available_stock,
                    valid_stock=excluded.valid_stock,
                    requested_stock=excluded.requested_stock,
                    transit_stock=excluded.transit_stock,
                    days_without_sales=excluded.days_without_sales,
                    idc_cluster=excluded.idc_cluster,
                    turnover_grade_cluster=excluded.turnover_grade_cluster,
                    updated_at=CURRENT_TIMESTAMP
                """,
                values,
            )
        return len(values)

    def replace_ozon_clusters(self, rows: Iterable[dict[str, Any]]) -> int:
        values = []
        for row in rows:
            macro_id = str(row.get("macrolocal_cluster_id") or "")
            if not macro_id:
                continue
            data = row.get("data") or {}
            macro = data.get("macrolocal_cluster") or {}
            country = macro.get("country") or {}
            values.append((
                macro_id, str(macro.get("name") or row.get("cluster_name") or ""),
                str(country.get("name") or country.get("uid") or ""),
                json.dumps(data.get("fulfillments") or [], ensure_ascii=False),
            ))
        with self.connect() as db:
            db.execute("DELETE FROM ozon_clusters")
            db.executemany(
                """
                INSERT INTO ozon_clusters(
                    macrolocal_cluster_id, cluster_name, country, fulfillments_json
                ) VALUES (?, ?, ?, ?)
                """,
                values,
            )
        return len(values)

    def replace_seller_supply_warehouses(self, rows: Iterable[dict[str, Any]]) -> int:
        values = []
        for row in rows:
            warehouse_id = str(row.get("seller_warehouse_id") or "")
            if not warehouse_id:
                continue
            address = row.get("address") or {}
            values.append((
                warehouse_id, str(row.get("seller_warehouse_name") or ""),
                int(bool(row.get("is_active"))), int(bool(row.get("is_pickup"))),
                str(address.get("region") or ""), str(address.get("city") or ""),
                str(address.get("address") or ""), str(address.get("timezone") or ""),
                str(address.get("macrolocal_cluster_id") or ""),
            ))
        with self.connect() as db:
            db.execute("DELETE FROM seller_supply_warehouses")
            db.executemany(
                """
                INSERT INTO seller_supply_warehouses(
                    seller_warehouse_id, warehouse_name, is_active, is_pickup,
                    region, city, address, timezone, macrolocal_cluster_id
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                """,
                values,
            )
        return len(values)

    def cache_supply_dropoffs(
        self, rows: Iterable[dict[str, Any]], supply_type: str,
    ) -> int:
        """Persist searched FBO drop-off points in the current shop database."""
        normalized_type = str(supply_type or "").strip().upper()
        values: list[tuple[str, str, str, str, str, str]] = []
        for raw in rows:
            row = dict(raw or {})
            warehouse_id = str(row.get("warehouse_id") or "").strip()
            if not warehouse_id:
                continue
            warehouse_type = str(row.get("warehouse_type") or "").strip().upper()
            if normalized_type == "CROSSDOCK" and warehouse_type in {
                "", "0", "UNSPECIFIED", "UNKNOWN", "WAREHOUSE_TYPE_UNSPECIFIED",
            }:
                warehouse_type = "CROSS_DOCK"
            values.append((
                warehouse_id, warehouse_type, normalized_type,
                str(row.get("name") or ""), str(row.get("address") or ""),
                json.dumps(row, ensure_ascii=False),
            ))
        if not values:
            return 0
        with self.connect() as db:
            db.executemany(
                """
                INSERT INTO supply_dropoff_cache(
                    warehouse_id, warehouse_type, supply_type, name, address, raw_json
                ) VALUES (?, ?, ?, ?, ?, ?)
                ON CONFLICT(warehouse_id, warehouse_type, supply_type) DO UPDATE SET
                    name=excluded.name,
                    address=excluded.address,
                    raw_json=excluded.raw_json,
                    updated_at=CURRENT_TIMESTAMP
                """,
                values,
            )
        return len(values)

    def cached_supply_dropoffs(
        self, search: str = "", supply_type: str = "CROSSDOCK",
    ) -> list[dict[str, Any]]:
        """Read cached drop-off points without calling Ozon again."""
        query = str(search or "").strip().casefold()
        normalized_type = str(supply_type or "").strip().upper()
        with self.connect() as db:
            records = db.execute(
                """
                SELECT warehouse_id, warehouse_type, supply_type, name, address,
                       raw_json, updated_at
                FROM supply_dropoff_cache
                WHERE supply_type=?
                ORDER BY name COLLATE NOCASE, address COLLATE NOCASE
                """,
                (normalized_type,),
            ).fetchall()
        result: list[dict[str, Any]] = []
        for record in records:
            searchable = (
                f"{record['name']} {record['address']} {record['warehouse_id']}"
            ).casefold()
            if query and query not in searchable:
                continue
            try:
                row = json.loads(record["raw_json"] or "{}")
            except (TypeError, ValueError):
                row = {}
            row.update({
                "warehouse_id": record["warehouse_id"],
                "warehouse_type": record["warehouse_type"],
                "name": record["name"],
                "address": record["address"],
                "cached_at": record["updated_at"],
            })
            result.append(row)
        return result

    def inventory_products(self, target_days: int = 30) -> list[dict[str, Any]]:
        target_days = max(1, int(target_days))
        with self.connect() as db:
            rows = db.execute(
                """
                WITH keys AS (
                    SELECT sku FROM products WHERE sku <> ''
                    UNION SELECT sku FROM sales_daily WHERE sku <> ''
                    UNION SELECT sku FROM inventory_stock WHERE sku <> ''
                ), stocks AS (
                    SELECT sku, MAX(offer_id) offer_id, MAX(product_name) product_name,
                           SUM(available_stock) available_stock,
                           SUM(transit_stock) transit_stock,
                           SUM(requested_stock) requested_stock,
                           MAX(ads) ads
                    FROM inventory_stock GROUP BY sku
                ), plans AS (
                    SELECT sku, SUM(planned_qty) planned_qty,
                           SUM(CASE WHEN planned_qty > 0 THEN 1 ELSE 0 END) planned_clusters
                    FROM replenishment_plan GROUP BY sku
                )
                SELECT k.sku, COALESCE(NULLIF(p.offer_id, ''), s.offer_id, '') offer_id,
                       COALESCE(NULLIF(p.name, ''), s.product_name, '') product_name,
                       COALESCE(s.available_stock, 0) available_stock,
                       COALESCE(s.transit_stock, 0) transit_stock,
                       COALESCE(s.requested_stock, 0) requested_stock,
                       COALESCE(s.ads, 0) daily_sales,
                       COALESCE(rp.planned_qty, 0) planned_qty,
                       COALESCE(rp.planned_clusters, 0) planned_clusters
                FROM keys k LEFT JOIN products p ON p.sku=k.sku
                LEFT JOIN stocks s ON s.sku=k.sku
                LEFT JOIN plans rp ON rp.sku=k.sku
                ORDER BY offer_id, k.sku
                """
            ).fetchall()
        result = []
        for raw in rows:
            row = dict(raw)
            daily_sales = float(row.get("daily_sales") or 0)
            existing = (
                int(row.get("available_stock") or 0)
                + int(row.get("transit_stock") or 0)
                + int(row.get("requested_stock") or 0)
            )
            row["monthly_estimated"] = daily_sales * 30
            row["recommended_qty"] = max(0, math.ceil(daily_sales * target_days - existing))
            planned = int(row.get("planned_qty") or 0)
            row["sellable_days_after"] = (
                (existing + planned) / daily_sales if daily_sales > 0 else None
            )
            result.append(row)
        return result

    def inventory_cluster_plan(self, sku: str, target_days: int = 30) -> list[dict[str, Any]]:
        target_days = max(1, int(target_days))
        with self.connect() as db:
            rows = db.execute(
                """
                WITH cluster_keys AS (
                    SELECT macrolocal_cluster_id, cluster_name FROM ozon_clusters
                    UNION
                    SELECT macrolocal_cluster_id, MAX(cluster_name)
                    FROM inventory_stock WHERE sku=? AND macrolocal_cluster_id<>''
                    GROUP BY macrolocal_cluster_id
                ), stock AS (
                    SELECT macrolocal_cluster_id, MAX(cluster_name) stock_cluster_name,
                           SUM(available_stock) available_stock,
                           SUM(transit_stock) transit_stock,
                           SUM(requested_stock) requested_stock,
                           MAX(ads_cluster) daily_sales,
                           MAX(idc_cluster) idc_cluster,
                           MAX(turnover_grade_cluster) turnover_grade
                    FROM inventory_stock WHERE sku=? GROUP BY macrolocal_cluster_id
                )
                SELECT ck.macrolocal_cluster_id,
                       COALESCE(NULLIF(ck.cluster_name, ''), s.stock_cluster_name, '未命名集群') cluster_name,
                       COALESCE(s.available_stock, 0) available_stock,
                       COALESCE(s.transit_stock, 0) transit_stock,
                       COALESCE(s.requested_stock, 0) requested_stock,
                       COALESCE(s.daily_sales, 0) daily_sales,
                       COALESCE(s.idc_cluster, 0) idc_cluster,
                       COALESCE(s.turnover_grade, '') turnover_grade,
                       rp.planned_qty saved_planned_qty, rp.target_days saved_target_days
                FROM cluster_keys ck LEFT JOIN stock s
                  ON s.macrolocal_cluster_id=ck.macrolocal_cluster_id
                LEFT JOIN replenishment_plan rp
                  ON rp.sku=? AND rp.macrolocal_cluster_id=ck.macrolocal_cluster_id
                ORDER BY daily_sales DESC, cluster_name
                """,
                (sku, sku, sku),
            ).fetchall()
        result = []
        for raw in rows:
            row = dict(raw)
            row["cluster_name_cn"] = _cluster_name_cn(row.get("cluster_name"))
            daily_sales = float(row.get("daily_sales") or 0)
            existing = sum(int(row.get(key) or 0) for key in (
                "available_stock", "transit_stock", "requested_stock"
            ))
            recommended = max(0, math.ceil(daily_sales * target_days - existing))
            planned = row.pop("saved_planned_qty")
            row.pop("saved_target_days", None)
            row["plan_saved"] = planned is not None
            row["monthly_estimated"] = daily_sales * 30
            row["target_days"] = target_days
            row["recommended_qty"] = recommended
            row["planned_qty"] = recommended if planned is None else int(planned)
            row["sellable_days_after"] = (
                (existing + row["planned_qty"]) / daily_sales if daily_sales > 0 else None
            )
            result.append(row)
        return result

    def save_replenishment_plan(
        self, sku: str, macrolocal_cluster_id: str, planned_qty: int, target_days: int,
    ) -> None:
        with self.connect() as db:
            db.execute(
                """
                INSERT INTO replenishment_plan(
                    sku, macrolocal_cluster_id, planned_qty, target_days
                ) VALUES (?, ?, ?, ?)
                ON CONFLICT(sku, macrolocal_cluster_id) DO UPDATE SET
                    planned_qty=excluded.planned_qty,
                    target_days=excluded.target_days,
                    updated_at=CURRENT_TIMESTAMP
                """,
                (str(sku), str(macrolocal_cluster_id), max(0, int(planned_qty)), max(1, int(target_days))),
            )

    def supply_warehouse_options(self) -> list[dict[str, Any]]:
        with self.connect() as db:
            rows = db.execute(
                """
                SELECT * FROM seller_supply_warehouses
                ORDER BY is_active DESC, city, warehouse_name
                """
            ).fetchall()
        return [dict(row) for row in rows]

    def product_ai_detail(
        self, date_from: str, date_to: str, sku: str, target_days: int = 30,
    ) -> dict[str, Any]:
        """Return every local row for one SKU in the selected period."""
        with self.connect() as db:
            product = db.execute(
                """
                SELECT p.sku, p.offer_id, p.product_id, p.name product_name,
                       pc.unit_cost, pc.first_mile_cost, pc.note
                FROM products p LEFT JOIN product_costs pc ON pc.sku=p.sku
                WHERE p.sku=?
                """,
                (sku,),
            ).fetchone()
            sales = db.execute(
                """
                SELECT day, revenue, ordered_units, delivered_units, returns,
                       cancellations, views, cart_adds, source
                FROM sales_daily WHERE sku=? AND day BETWEEN ? AND ? ORDER BY day
                """,
                (sku, date_from, date_to),
            ).fetchall()
            advertising = db.execute(
                """
                SELECT day, campaign_id, campaign_name, impressions, clicks,
                       cart_adds, orders, revenue, spend, source
                FROM ad_daily WHERE sku=? AND day BETWEEN ? AND ?
                ORDER BY day, campaign_id
                """,
                (sku, date_from, date_to),
            ).fetchall()
            campaign_advertising = db.execute(
                """
                SELECT day, campaign_id, campaign_name, impressions, clicks,
                       cart_adds, orders, revenue, spend, source
                FROM ad_daily WHERE sku='' AND day BETWEEN ? AND ?
                ORDER BY day, campaign_id
                """,
                (date_from, date_to),
            ).fetchall()
            returns = db.execute(
                """
                SELECT return_id, day, offer_id, product_name, quantity, source
                FROM return_events WHERE sku=? AND day BETWEEN ? AND ? ORDER BY day, return_id
                """,
                (sku, date_from, date_to),
            ).fetchall()
            cancellations = db.execute(
                """
                SELECT event_id, day, offer_id, product_name, quantity, scheme, source
                FROM cancellation_events WHERE sku=? AND day BETWEEN ? AND ?
                ORDER BY day, event_id
                """,
                (sku, date_from, date_to),
            ).fetchall()
            deliveries = db.execute(
                """
                SELECT event_id, day, offer_id, product_name, quantity, scheme, source
                FROM delivery_events WHERE sku=? AND day BETWEEN ? AND ?
                ORDER BY day, event_id
                """,
                (sku, date_from, date_to),
            ).fetchall()
            finance = db.execute(
                """
                SELECT COUNT(*) transaction_count, COALESCE(SUM(amount), 0) amount,
                       COALESCE(SUM(delivery_charge), 0) delivery_charge,
                       COALESCE(SUM(return_delivery_charge), 0) return_delivery_charge,
                       COALESCE(SUM(accruals_for_sale), 0) accruals_for_sale,
                       COALESCE(SUM(sale_commission), 0) sale_commission
                FROM finance_transactions
                WHERE sku=? AND substr(operation_date, 1, 10) BETWEEN ? AND ?
                """,
                (sku, date_from, date_to),
            ).fetchone()
        identity = dict(product) if product else {
            "sku": sku, "offer_id": "", "product_id": "", "product_name": "",
            "unit_cost": None, "first_mile_cost": None, "note": "",
        }
        exact_advertising_rows = [dict(row) for row in advertising]
        identifiers = tuple(filter(None, (
            str(identity.get("offer_id") or "").strip(),
            str(identity.get("sku") or sku or "").strip(),
        )))
        matched_campaign_rows = [
            dict(row) for row in campaign_advertising
            if any(
                _campaign_identifier_match(row["campaign_name"], identifier)
                for identifier in identifiers
            )
        ]
        seller_revenue = sum(float(row["revenue"] or 0) for row in sales)
        exact_summary = _advertising_summary(
            exact_advertising_rows, seller_revenue,
            "matched_exact_sku" if exact_advertising_rows else "no_exact_sku_rows_in_local_cache",
        )
        matched_summary = _advertising_summary(
            matched_campaign_rows, seller_revenue,
            (
                "matched_dedicated_campaign_name"
                if matched_campaign_rows else "no_dedicated_campaign_name_match"
            ),
        )
        effective_rows = exact_advertising_rows or matched_campaign_rows
        effective_summary = exact_summary if exact_advertising_rows else matched_summary
        advertising_source = (
            "exact_sku_local_cache" if exact_advertising_rows
            else "dedicated_campaign_name_local_cache" if matched_campaign_rows
            else "no_product_advertising_match"
        )
        matched_campaigns = sorted({
            (str(row.get("campaign_id") or ""), str(row.get("campaign_name") or ""))
            for row in matched_campaign_rows
        })
        return {
            "identity": identity,
            "sales_daily_all_rows": [dict(row) for row in sales],
            "advertising_source": advertising_source,
            "advertising_summary": effective_summary,
            "advertising_daily_all_rows": effective_rows,
            "advertising_exact_sku": {
                "summary": exact_summary,
                "daily_all_rows": exact_advertising_rows,
            },
            "locally_matched_campaign_advertising": {
                "summary": matched_summary,
                "daily_all_rows": matched_campaign_rows,
                "matched_campaigns": [
                    {"campaign_id": campaign_id, "campaign_name": campaign_name}
                    for campaign_id, campaign_name in matched_campaigns
                ],
                "match_identifiers": list(identifiers),
                "scope": (
                    "仅使用数据中心已同步到本地的活动级缓存，并仅匹配活动名称中"
                    "明确包含所选 Ozon SKU 或商家货号的专属活动；通用活动不会归因给商品"
                ),
            },
            "return_event_all_rows": [dict(row) for row in returns],
            "cancellation_event_all_rows": [dict(row) for row in cancellations],
            "delivery_event_all_rows": [dict(row) for row in deliveries],
            "finance_period_summary": dict(finance) if finance else {},
            "inventory_all_clusters": self.inventory_cluster_plan(sku, target_days),
            "row_counts": {
                "sales_daily": len(sales),
                "advertising_daily": len(effective_rows),
                "advertising_exact_sku": len(exact_advertising_rows),
                "advertising_matched_campaign": len(matched_campaign_rows),
                "advertising_matched_campaigns": len(matched_campaigns),
                "returns": len(returns), "cancellations": len(cancellations),
                "deliveries": len(deliveries),
            },
            "advertising_attribution_rule": (
                "优先使用 ad_daily.sku 与所选 Ozon SKU 完全一致的本地缓存；"
                "没有精确明细时，才使用活动名称明确匹配 SKU/货号的本地活动级缓存，"
                "并视为活动级推算而非 Ozon 官方 SKU 归因；空列表不代表真实指标为 0"
            ),
        }

    def ad_detail_cache_info(
        self, date_from: str, date_to: str, sku: str = "",
    ) -> dict[str, Any]:
        """Return exact-range cache state for the full Performance SKU report."""
        with self.connect() as db:
            cache = db.execute(
                """
                SELECT row_count, updated_at FROM ad_detail_cache
                WHERE date_from=? AND date_to=?
                """,
                (date_from, date_to),
            ).fetchone()
            selected_rows = db.execute(
                """
                SELECT COUNT(*) FROM ad_daily
                WHERE sku=? AND day BETWEEN ? AND ?
                """,
                (str(sku), date_from, date_to),
            ).fetchone()[0] if sku else 0
        return {
            "complete": cache is not None,
            "row_count": int(cache["row_count"] or 0) if cache else 0,
            "selected_sku_rows": int(selected_rows or 0),
            "updated_at": str(cache["updated_at"] or "") if cache else "",
            "date_from": date_from,
            "date_to": date_to,
        }

    def replace_ad_product_details(
        self, date_from: str, date_to: str, rows: Iterable[dict[str, Any]],
    ) -> int:
        """Atomically replace all locally cached SKU-detail rows for one report range."""
        values = [
            (
                str(row.get("day") or date_to),
                str(row.get("campaign_id") or ""),
                str(row.get("campaign_name") or ""),
                str(row.get("sku") or ""),
                _integer(row.get("impressions")), _integer(row.get("clicks")),
                _integer(row.get("cart_adds")), _integer(row.get("orders")),
                _number(row.get("revenue")), _number(row.get("spend")),
                str(row.get("source") or "api"),
            )
            for row in rows
            if str(row.get("sku") or "").strip()
            and str(row.get("campaign_id") or "").strip()
        ]
        with self.connect() as db:
            db.execute(
                "DELETE FROM ad_daily WHERE sku<>'' AND day BETWEEN ? AND ?",
                (date_from, date_to),
            )
            db.executemany(
                """
                INSERT INTO ad_daily(
                    day, campaign_id, campaign_name, sku, impressions, clicks,
                    cart_adds, orders, revenue, spend, source
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(day, campaign_id, sku) DO UPDATE SET
                    campaign_name=excluded.campaign_name,
                    impressions=excluded.impressions,
                    clicks=excluded.clicks,
                    cart_adds=excluded.cart_adds,
                    orders=excluded.orders,
                    revenue=excluded.revenue,
                    spend=excluded.spend,
                    source=excluded.source,
                    updated_at=CURRENT_TIMESTAMP
                """,
                values,
            )
            db.execute(
                """
                INSERT INTO ad_detail_cache(date_from, date_to, row_count)
                VALUES (?, ?, ?)
                ON CONFLICT(date_from, date_to) DO UPDATE SET
                    row_count=excluded.row_count,
                    updated_at=CURRENT_TIMESTAMP
                """,
                (date_from, date_to, len(values)),
            )
        return len(values)

    def upsert_sales(self, rows: Iterable[dict[str, Any]]) -> int:
        values = [
            (
                str(row.get("day", "")),
                str(row.get("sku", "")),
                str(row.get("product_name", "")),
                _number(row.get("revenue")),
                _integer(row.get("ordered_units")),
                _integer(row.get("delivered_units")),
                _integer(row.get("returns")),
                _integer(row.get("cancellations")),
                _integer(row.get("views")),
                _integer(row.get("cart_adds")),
                str(row.get("source", "api")),
            )
            for row in rows
            if row.get("day") and row.get("sku")
        ]
        if not values:
            return 0
        with self.connect() as db:
            db.executemany(
                """
                INSERT INTO sales_daily(
                    day, sku, product_name, revenue, ordered_units,
                    delivered_units, returns, cancellations, views, cart_adds, source
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(day, sku) DO UPDATE SET
                    product_name=excluded.product_name,
                    revenue=excluded.revenue,
                    ordered_units=excluded.ordered_units,
                    delivered_units=excluded.delivered_units,
                    returns=excluded.returns,
                    cancellations=excluded.cancellations,
                    views=excluded.views,
                    cart_adds=excluded.cart_adds,
                    source=excluded.source,
                    updated_at=CURRENT_TIMESTAMP
                """,
                values,
            )
        return len(values)

    def upsert_products(self, rows: Iterable[dict[str, Any]]) -> int:
        values = [
            (
                str(row.get("sku", "")), str(row.get("offer_id", "")),
                str(row.get("product_id", "")), str(row.get("name", "")),
                str(row.get("image_url", "")),
                str(row.get("source", "api")),
            )
            for row in rows if row.get("sku")
        ]
        if not values:
            return 0
        with self.connect() as db:
            db.executemany(
                """
                INSERT INTO products(sku, offer_id, product_id, name, image_url, source)
                VALUES (?, ?, ?, ?, ?, ?)
                ON CONFLICT(sku) DO UPDATE SET
                    offer_id=excluded.offer_id,
                    product_id=excluded.product_id,
                    name=CASE WHEN excluded.name<>'' THEN excluded.name ELSE products.name END,
                    image_url=CASE WHEN excluded.image_url<>'' THEN excluded.image_url ELSE products.image_url END,
                    source=excluded.source,
                    updated_at=CURRENT_TIMESTAMP
                """,
                values,
            )
        return len(values)

    def replace_returns(
        self, date_from: str, date_to: str, rows: Iterable[dict[str, Any]],
        *, coverage_source: str | None = None,
    ) -> int:
        values = [
            (
                str(row.get("return_id", "")), str(row.get("day", "")),
                str(row.get("sku", "")), str(row.get("offer_id", "")),
                str(row.get("product_name", "")), max(0, _integer(row.get("quantity"))),
                str(row.get("source", "api")),
            )
            for row in rows if row.get("return_id") and row.get("day")
        ]
        with self.connect() as db:
            db.execute(
                "DELETE FROM return_events WHERE day BETWEEN ? AND ? "
                "AND source IN ('api', 'finance')",
                (date_from, date_to),
            )
            if values:
                db.executemany(
                    """
                    INSERT INTO return_events(
                        return_id, day, sku, offer_id, product_name, quantity, source
                    ) VALUES (?, ?, ?, ?, ?, ?, ?)
                    ON CONFLICT(return_id) DO UPDATE SET
                        day=excluded.day, sku=excluded.sku, offer_id=excluded.offer_id,
                        product_name=excluded.product_name, quantity=excluded.quantity,
                        source=excluded.source, updated_at=CURRENT_TIMESTAMP
                    """,
                    values,
                )
            if coverage_source is None:
                coverage_source = (
                    "finance" if values and all(value[6] == "finance" for value in values)
                    else "api"
                )
            self._mark_coverage_in_connection(
                db, date_from, date_to, "returns", coverage_source
            )
        return len(values)

    def replace_cancellations(
        self, date_from: str, date_to: str, rows: Iterable[dict[str, Any]],
    ) -> int:
        values = [
            (
                str(row.get("event_id", "")), str(row.get("day", "")),
                str(row.get("sku", "")), str(row.get("offer_id", "")),
                str(row.get("product_name", "")), max(0, _integer(row.get("quantity"))),
                str(row.get("scheme", "")), str(row.get("source", "api")),
            )
            for row in rows if row.get("event_id") and row.get("day")
        ]
        with self.connect() as db:
            db.execute(
                "DELETE FROM cancellation_events WHERE day BETWEEN ? AND ? AND source='api'",
                (date_from, date_to),
            )
            if values:
                db.executemany(
                    """
                    INSERT INTO cancellation_events(
                        event_id, day, sku, offer_id, product_name, quantity, scheme, source
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                    ON CONFLICT(event_id) DO UPDATE SET
                        day=excluded.day, sku=excluded.sku, offer_id=excluded.offer_id,
                        product_name=excluded.product_name, quantity=excluded.quantity,
                        scheme=excluded.scheme, source=excluded.source,
                        updated_at=CURRENT_TIMESTAMP
                    """,
                    values,
                )
            self._mark_coverage_in_connection(db, date_from, date_to, "cancellations")
        return len(values)

    def replace_deliveries(
        self, date_from: str, date_to: str, rows: Iterable[dict[str, Any]],
    ) -> int:
        values = [
            (
                str(row.get("event_id", "")), str(row.get("day", "")),
                str(row.get("sku", "")), str(row.get("offer_id", "")),
                str(row.get("product_name", "")), max(0, _integer(row.get("quantity"))),
                str(row.get("scheme", "")), str(row.get("posting_number", "")),
                str(row.get("origin", "")), str(row.get("destination", "")),
                _number(row.get("order_price")), _number(row.get("commission")),
                _number(row.get("payout")), str(row.get("source", "api")),
            )
            for row in rows if row.get("event_id") and row.get("day")
        ]
        with self.connect() as db:
            db.execute(
                "DELETE FROM delivery_events WHERE day BETWEEN ? AND ? AND source='api'",
                (date_from, date_to),
            )
            if values:
                db.executemany(
                    """
                    INSERT INTO delivery_events(
                        event_id, day, sku, offer_id, product_name, quantity, scheme,
                        posting_number, origin, destination, order_price, commission,
                        payout, source
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                    ON CONFLICT(event_id) DO UPDATE SET
                        day=excluded.day, sku=excluded.sku, offer_id=excluded.offer_id,
                        product_name=excluded.product_name, quantity=excluded.quantity,
                        scheme=excluded.scheme, posting_number=excluded.posting_number,
                        origin=excluded.origin, destination=excluded.destination,
                        order_price=excluded.order_price, commission=excluded.commission,
                        payout=excluded.payout, source=excluded.source,
                        updated_at=CURRENT_TIMESTAMP
                    """,
                    values,
                )
            self._mark_coverage_in_connection(db, date_from, date_to, "deliveries")
        return len(values)

    def replace_posting_routes(
        self, date_from: str, date_to: str, rows: Iterable[dict[str, Any]],
    ) -> int:
        """Cache routes for all FBO/FBS postings without marking them delivered."""
        values = [
            (
                str(row.get("event_id", "")), str(row.get("day", "")),
                str(row.get("sku", "")), str(row.get("offer_id", "")),
                str(row.get("product_name", "")), max(0, _integer(row.get("quantity"))),
                str(row.get("scheme", "")), str(row.get("status", "")),
                str(row.get("posting_number", "")), str(row.get("origin", "")),
                str(row.get("destination", "")), _number(row.get("order_price")),
                _number(row.get("commission")), _number(row.get("payout")),
                str(row.get("source", "api")),
            )
            for row in rows if row.get("event_id") and row.get("day")
        ]
        with self.connect() as db:
            db.execute(
                "DELETE FROM posting_routes WHERE day BETWEEN ? AND ? AND source='api'",
                (date_from, date_to),
            )
            if values:
                db.executemany(
                    """
                    INSERT INTO posting_routes(
                        event_id, day, sku, offer_id, product_name, quantity, scheme,
                        status, posting_number, origin, destination, order_price,
                        commission, payout, source
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                    ON CONFLICT(event_id) DO UPDATE SET
                        day=excluded.day, sku=excluded.sku, offer_id=excluded.offer_id,
                        product_name=excluded.product_name, quantity=excluded.quantity,
                        scheme=excluded.scheme, status=excluded.status,
                        posting_number=excluded.posting_number, origin=excluded.origin,
                        destination=excluded.destination, order_price=excluded.order_price,
                        commission=excluded.commission, payout=excluded.payout,
                        source=excluded.source, updated_at=CURRENT_TIMESTAMP
                    """,
                    values,
                )
        return len(values)

    @staticmethod
    def _mark_coverage_in_connection(
        db: sqlite3.Connection, date_from: str, date_to: str, metric: str,
        source: str = "api",
    ) -> None:
        cursor = date.fromisoformat(date_from)
        end = date.fromisoformat(date_to)
        values: list[tuple[str, str, str]] = []
        while cursor <= end:
            values.append((cursor.isoformat(), metric, source))
            cursor += timedelta(days=1)
        db.executemany(
            """
            INSERT INTO data_coverage(day, metric, source) VALUES (?, ?, ?)
            ON CONFLICT(day, metric) DO UPDATE SET
                source=excluded.source, updated_at=CURRENT_TIMESTAMP
            """,
            values,
        )

    def upsert_ads(self, rows: Iterable[dict[str, Any]]) -> int:
        values = [
            (
                str(row.get("day", "")),
                str(row.get("campaign_id", "")),
                str(row.get("campaign_name", "")),
                str(row.get("sku", "")),
                _integer(row.get("impressions")),
                _integer(row.get("clicks")),
                _integer(row.get("cart_adds")),
                _integer(row.get("orders")),
                _number(row.get("revenue")),
                _number(row.get("spend")),
                str(row.get("source", "api")),
            )
            for row in rows
            if row.get("day") and row.get("campaign_id")
        ]
        if not values:
            return 0
        with self.connect() as db:
            db.executemany(
                """
                INSERT INTO ad_daily(
                    day, campaign_id, campaign_name, sku, impressions, clicks,
                    cart_adds, orders, revenue, spend, source
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(day, campaign_id, sku) DO UPDATE SET
                    campaign_name=excluded.campaign_name,
                    impressions=excluded.impressions,
                    clicks=excluded.clicks,
                    cart_adds=excluded.cart_adds,
                    orders=excluded.orders,
                    revenue=excluded.revenue,
                    spend=excluded.spend,
                    source=excluded.source,
                    updated_at=CURRENT_TIMESTAMP
                """,
                values,
            )
        return len(values)

    def upsert_campaigns(self, rows: Iterable[dict[str, Any]]) -> int:
        values = [
            (
                str(row.get("campaign_id", "")),
                str(row.get("name", "")),
                str(row.get("state", "")),
                str(row.get("payment_type", "")),
                _number(row.get("budget")),
                str(row.get("source", "api")),
            )
            for row in rows
            if row.get("campaign_id")
        ]
        if not values:
            return 0
        with self.connect() as db:
            db.executemany(
                """
                INSERT INTO campaigns(campaign_id, name, state, payment_type, budget, source)
                VALUES (?, ?, ?, ?, ?, ?)
                ON CONFLICT(campaign_id) DO UPDATE SET
                    name=excluded.name, state=excluded.state,
                    payment_type=excluded.payment_type, budget=excluded.budget,
                    source=excluded.source,
                    updated_at=CURRENT_TIMESTAMP
                """,
                values,
            )
        return len(values)

    def upsert_finance(self, rows: Iterable[dict[str, Any]]) -> int:
        values = []
        for index, row in enumerate(rows):
            operation_id = str(row.get("operation_id") or f"generated-{index}-{row.get('operation_date', '')}")
            items = row.get("items") or []
            item_skus = {
                str(item.get("sku") or "").strip()
                for item in items if isinstance(item, dict) and item.get("sku")
            }
            # An operation can contain several products.  Storing the first SKU
            # would silently assign the whole fee to that product, so only keep
            # an exact SKU when the operation contains one unique product.
            sku = next(iter(item_skus)) if len(item_skus) == 1 else str(row.get("sku", ""))
            posting = row.get("posting") or {}
            values.append(
                (
                    operation_id,
                    str(row.get("operation_date", "")),
                    str(row.get("operation_type", "")),
                    str(posting.get("posting_number", row.get("posting_number", ""))),
                    sku,
                    _number(row.get("amount")),
                    _number(row.get("delivery_charge")),
                    _number(row.get("return_delivery_charge")),
                    _number(row.get("accruals_for_sale")),
                    _number(row.get("sale_commission")),
                    json.dumps(row, ensure_ascii=False),
                )
            )
        if not values:
            return 0
        with self.connect() as db:
            db.executemany(
                """
                INSERT INTO finance_transactions(
                    operation_id, operation_date, operation_type, posting_number, sku,
                    amount, delivery_charge, return_delivery_charge,
                    accruals_for_sale, sale_commission, raw_json
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(operation_id) DO UPDATE SET
                    operation_date=excluded.operation_date,
                    operation_type=excluded.operation_type,
                    posting_number=excluded.posting_number,
                    sku=excluded.sku,
                    amount=excluded.amount,
                    delivery_charge=excluded.delivery_charge,
                    return_delivery_charge=excluded.return_delivery_charge,
                    accruals_for_sale=excluded.accruals_for_sale,
                    sale_commission=excluded.sale_commission,
                    raw_json=excluded.raw_json
                """,
                values,
            )
        self._finance_revision += 1
        self._finance_snapshot_cache.clear()
        self._realtime_fee_profile_cache = None
        self._fulfillment_order_cache.clear()
        self._fulfillment_order_index_revision = -1
        self._fulfillment_order_index = []
        return len(values)

    def replace_finance_cash_flow(
        self,
        date_from: str,
        date_to: str,
        report: dict[str, Any],
    ) -> int:
        """Cache the store-economy report returned by Seller API."""
        cash_flows = [row for row in report.get("cash_flows", []) if isinstance(row, dict)]
        details = [row for row in report.get("details", []) if isinstance(row, dict)]
        cash_values: list[tuple[Any, ...]] = []
        detail_values: list[tuple[Any, ...]] = []
        for index, row in enumerate(cash_flows):
            period_from, period_to = _finance_period_bounds(row)
            currency = str(row.get("currency_code") or "")
            row_id = f"{period_from}|{period_to}|{currency}|{index}"
            cash_values.append((
                row_id, period_from, period_to, currency,
                json.dumps(row, ensure_ascii=False),
            ))
        for index, row in enumerate(details):
            period_from, period_to = _finance_period_bounds(row)
            row_id = f"{period_from}|{period_to}|{index}"
            detail_values.append((
                row_id, period_from, period_to, json.dumps(row, ensure_ascii=False),
            ))
        with self.connect() as db:
            # Ozon splits the result into settlement periods.  Replace periods
            # touched by this refresh so a partial current-month report cannot
            # coexist with an older version of the same period.
            db.execute(
                "DELETE FROM finance_cash_flows WHERE period_to>=? AND period_from<=?",
                (date_from, date_to),
            )
            db.execute(
                "DELETE FROM finance_cash_flow_details WHERE period_to>=? AND period_from<=?",
                (date_from, date_to),
            )
            if cash_values:
                db.executemany(
                    """
                    INSERT INTO finance_cash_flows(
                        row_id, period_from, period_to, currency_code, raw_json
                    ) VALUES (?, ?, ?, ?, ?)
                    ON CONFLICT(row_id) DO UPDATE SET
                        period_from=excluded.period_from,
                        period_to=excluded.period_to,
                        currency_code=excluded.currency_code,
                        raw_json=excluded.raw_json,
                        updated_at=CURRENT_TIMESTAMP
                    """,
                    cash_values,
                )
            if detail_values:
                db.executemany(
                    """
                    INSERT INTO finance_cash_flow_details(
                        row_id, period_from, period_to, raw_json
                    ) VALUES (?, ?, ?, ?)
                    ON CONFLICT(row_id) DO UPDATE SET
                        period_from=excluded.period_from,
                        period_to=excluded.period_to,
                        raw_json=excluded.raw_json,
                        updated_at=CURRENT_TIMESTAMP
                    """,
                    detail_values,
                )
        return len(cash_values) + len(detail_values)

    def finance_customer_returns(
        self, date_from: str, date_to: str,
    ) -> list[dict[str, Any]]:
        """Build Seller-style returned products from confirmed finance rows.

        Ozon's product analytics attributes returns to the original order date.
        ``/v1/returns/list`` is an operational logistics feed and cannot be
        aggregated by its return date for that column.  A
        ``ClientReturnAgentOperation`` contains the original ``order_date`` and
        represents the confirmed financial reversal, so it is the closest
        available non-Premium source for the same per-product business metric.
        """

        with self.connect() as db:
            rows = db.execute(
                """
                SELECT operation_id, raw_json
                FROM finance_transactions
                WHERE operation_type='ClientReturnAgentOperation'
                """
            ).fetchall()
        result: list[dict[str, Any]] = []
        for row in rows:
            try:
                raw = json.loads(row["raw_json"] or "{}")
            except (TypeError, json.JSONDecodeError):
                continue
            posting = raw.get("posting") or {}
            order_day = str(posting.get("order_date") or "")[:10]
            if not order_day or not (date_from <= order_day <= date_to):
                continue
            posting_number = str(posting.get("posting_number") or "")
            for index, item in enumerate(raw.get("items") or []):
                sku = str(item.get("sku") or "")
                if not sku:
                    continue
                result.append(
                    {
                        "return_id": (
                            f"finance:{row['operation_id']}:{posting_number}:{sku}:{index}"
                        ),
                        "day": order_day,
                        "sku": sku,
                        "offer_id": "",
                        "product_name": str(item.get("name") or ""),
                        "quantity": max(1, _integer(item.get("quantity") or 1)),
                        "source": "finance",
                    }
                )
        return result

    def start_log(self, source: str) -> int:
        with self.connect() as db:
            cursor = db.execute(
                "INSERT INTO sync_logs(started_at, source, status) VALUES(?, ?, 'running')",
                (datetime.now().isoformat(timespec="seconds"), source),
            )
            return int(cursor.lastrowid)

    def finish_log(self, log_id: int, status: str, rows_count: int, message: str) -> None:
        with self.connect() as db:
            db.execute(
                """UPDATE sync_logs SET finished_at=?, status=?, rows_count=?, message=?
                   WHERE id=?""",
                (datetime.now().isoformat(timespec="seconds"), status, rows_count, message[:1000], log_id),
            )

    def append_log_event(
        self,
        log_id: int,
        level: str,
        stage: str,
        message: str,
        details: dict[str, Any] | None = None,
    ) -> int:
        safe_details = _sanitize_log_details(details or {})
        with self.connect() as db:
            cursor = db.execute(
                """
                INSERT INTO sync_log_events(
                    log_id, event_at, level, stage, message, details_json
                ) VALUES (?, ?, ?, ?, ?, ?)
                """,
                (
                    log_id,
                    datetime.now().isoformat(timespec="milliseconds"),
                    level.upper()[:16],
                    stage[:80],
                    message[:4000],
                    json.dumps(safe_details, ensure_ascii=False, default=str),
                ),
            )
            return int(cursor.lastrowid)

    def dashboard_summary(self, date_from: str, date_to: str) -> dict[str, Any]:
        with self.connect() as db:
            sales = db.execute(
                """
                SELECT COALESCE(SUM(revenue), 0) revenue,
                       COALESCE(SUM(ordered_units), 0) orders,
                       COALESCE(SUM(delivered_units), 0) delivered,
                       COALESCE(SUM(returns), 0) returns,
                       COALESCE(SUM(cancellations), 0) cancellations,
                       COALESCE(SUM(views), 0) views,
                       COALESCE(SUM(cart_adds), 0) cart_adds,
                       MAX(CASE WHEN source='demo' THEN 1 ELSE 0 END) has_demo
                FROM sales_daily WHERE day BETWEEN ? AND ?
                """,
                (date_from, date_to),
            ).fetchone()
            event_totals = db.execute(
                """
                SELECT
                    COALESCE((SELECT SUM(quantity) FROM return_events
                              WHERE day BETWEEN ? AND ?), 0) event_returns,
                    COALESCE((SELECT SUM(quantity) FROM cancellation_events
                              WHERE day BETWEEN ? AND ?), 0) event_cancellations,
                    COALESCE((SELECT SUM(quantity) FROM delivery_events
                              WHERE day BETWEEN ? AND ?), 0) event_deliveries,
                    COALESCE((SELECT COUNT(*) FROM data_coverage
                              WHERE day BETWEEN ? AND ? AND metric='returns'), 0) return_days,
                    COALESCE((SELECT COUNT(*) FROM data_coverage
                              WHERE day BETWEEN ? AND ? AND metric='cancellations'), 0) cancellation_days,
                    COALESCE((SELECT COUNT(*) FROM data_coverage
                              WHERE day BETWEEN ? AND ? AND metric='deliveries'), 0) delivery_days,
                    COALESCE((SELECT MAX(source) FROM data_coverage
                              WHERE day BETWEEN ? AND ? AND metric='returns'), '') return_source,
                    COALESCE((SELECT COUNT(*) FROM data_coverage
                              WHERE day BETWEEN ? AND ? AND metric='traffic'), 0) traffic_days
                """,
                (date_from, date_to, date_from, date_to,
                 date_from, date_to, date_from, date_to,
                 date_from, date_to, date_from, date_to,
                 date_from, date_to, date_from, date_to),
            ).fetchone()
            ads = db.execute(
                """
                SELECT COALESCE(SUM(spend), 0) spend,
                       COALESCE(SUM(revenue), 0) ad_revenue,
                       COALESCE(SUM(orders), 0) ad_orders,
                       COALESCE(SUM(impressions), 0) impressions,
                       COALESCE(SUM(clicks), 0) clicks
                FROM ad_daily WHERE day BETWEEN ? AND ? AND sku=''
                """,
                (date_from, date_to),
            ).fetchone()
        result = dict(sales)
        result.update(dict(ads))
        expected_days = (date.fromisoformat(date_to) - date.fromisoformat(date_from)).days + 1
        return_coverage = int(event_totals["return_days"] or 0) >= expected_days
        cancellation_coverage = int(event_totals["cancellation_days"] or 0) >= expected_days
        delivery_coverage = int(event_totals["delivery_days"] or 0) >= expected_days
        if int(event_totals["return_days"] or 0):
            result["returns"] = int(event_totals["event_returns"] or 0)
        if int(event_totals["cancellation_days"] or 0):
            result["cancellations"] = int(event_totals["event_cancellations"] or 0)
        if int(event_totals["delivery_days"] or 0):
            result["delivered"] = int(event_totals["event_deliveries"] or 0)
        demo_available = bool(result.pop("has_demo", 0))
        result["returns_available"] = return_coverage or demo_available or result["returns"] > 0
        result["returns_source"] = (
            "finance_proxy" if event_totals["return_source"] == "finance"
            else "seller_analytics"
        )
        result["returns_exact"] = (
            demo_available or result["returns_source"] != "finance_proxy"
        )
        result["cancellations_available"] = (
            cancellation_coverage or demo_available or result["cancellations"] > 0
        )
        result["delivered_available"] = (
            delivery_coverage or demo_available or result["delivered"] > 0 or result["orders"] == 0
        )
        traffic_coverage = int(event_totals["traffic_days"] or 0) >= expected_days
        result["traffic_available"] = (
            traffic_coverage or demo_available or result["views"] > 0
            or result["cart_adds"] > 0 or result["orders"] == 0
        )
        result["roas"] = result["ad_revenue"] / result["spend"] if result["spend"] else 0
        result["acos"] = result["spend"] / result["ad_revenue"] * 100 if result["ad_revenue"] else 0
        result["tacos"] = result["spend"] / result["revenue"] * 100 if result["revenue"] else 0
        result["drr"] = result["spend"] / result["revenue"] * 100 if result["revenue"] else 0
        result["ctr"] = result["clicks"] / result["impressions"] * 100 if result["impressions"] else 0
        result["conversion"] = (
            result["orders"] / result["views"] * 100
            if result["traffic_available"] and result["views"] else None
        )
        result["cancellation_rate"] = (
            result["cancellations"] / result["orders"] * 100
            if result["cancellations_available"] and result["orders"] else None
        )
        return result

    def daily_trend(self, date_from: str, date_to: str) -> list[dict[str, Any]]:
        with self.connect() as db:
            rows = db.execute(
                """
                WITH days AS (
                    SELECT day FROM sales_daily WHERE day BETWEEN ? AND ?
                    UNION SELECT day FROM ad_daily WHERE day BETWEEN ? AND ? AND sku=''
                ), s AS (
                    SELECT day, SUM(revenue) revenue, SUM(ordered_units) orders,
                           SUM(delivered_units) delivered, SUM(returns) returns,
                           SUM(cancellations) cancellations, SUM(views) views,
                           SUM(cart_adds) cart_adds
                    FROM sales_daily WHERE day BETWEEN ? AND ? GROUP BY day
                ), r AS (
                    SELECT day, SUM(quantity) returns
                    FROM return_events WHERE day BETWEEN ? AND ? GROUP BY day
                ), c AS (
                    SELECT day, SUM(quantity) cancellations
                    FROM cancellation_events WHERE day BETWEEN ? AND ? GROUP BY day
                ), d AS (
                    SELECT day, SUM(quantity) delivered
                    FROM delivery_events WHERE day BETWEEN ? AND ? GROUP BY day
                ), coverage AS (
                    SELECT day,
                           MAX(CASE WHEN metric='returns' THEN 1 ELSE 0 END) returns_available,
                           MAX(CASE WHEN metric='cancellations' THEN 1 ELSE 0 END) cancellations_available,
                           MAX(CASE WHEN metric='deliveries' THEN 1 ELSE 0 END) deliveries_available
                    FROM data_coverage WHERE day BETWEEN ? AND ? GROUP BY day
                ), a AS (
                    SELECT day, SUM(spend) spend, SUM(revenue) ad_revenue,
                           SUM(orders) ad_orders, SUM(impressions) impressions,
                           SUM(clicks) clicks
                    FROM ad_daily WHERE day BETWEEN ? AND ? AND sku='' GROUP BY day
                )
                SELECT days.day, COALESCE(s.revenue, 0) revenue,
                       COALESCE(s.orders, 0) orders,
                       CASE WHEN COALESCE(coverage.deliveries_available, 0)=1
                            THEN COALESCE(d.delivered, 0) ELSE COALESCE(s.delivered, 0) END delivered,
                       CASE WHEN COALESCE(coverage.returns_available, 0)=1
                            THEN COALESCE(r.returns, 0) ELSE COALESCE(s.returns, 0) END returns,
                       CASE WHEN COALESCE(coverage.cancellations_available, 0)=1
                            THEN COALESCE(c.cancellations, 0) ELSE COALESCE(s.cancellations, 0) END cancellations,
                       COALESCE(s.views, 0) views,
                       COALESCE(s.cart_adds, 0) cart_adds,
                       COALESCE(a.spend, 0) spend,
                       COALESCE(a.ad_revenue, 0) ad_revenue,
                       COALESCE(a.ad_orders, 0) ad_orders,
                       COALESCE(a.impressions, 0) impressions,
                       COALESCE(a.clicks, 0) clicks
                FROM days
                LEFT JOIN s ON s.day=days.day
                LEFT JOIN r ON r.day=days.day
                LEFT JOIN c ON c.day=days.day
                LEFT JOIN d ON d.day=days.day
                LEFT JOIN coverage ON coverage.day=days.day
                LEFT JOIN a ON a.day=days.day
                ORDER BY days.day
                """,
                (
                    date_from, date_to, date_from, date_to,
                    date_from, date_to, date_from, date_to,
                    date_from, date_to, date_from, date_to,
                    date_from, date_to, date_from, date_to,
                ),
            ).fetchall()
        return [dict(row) for row in rows]

    def top_products(self, date_from: str, date_to: str, limit: int = 100) -> list[dict[str, Any]]:
        with self.connect() as db:
            rows = db.execute(
                """
                WITH s AS (
                    SELECT sku, MAX(product_name) product_name, SUM(revenue) revenue,
                           SUM(ordered_units) orders, SUM(delivered_units) delivered,
                           SUM(returns) analytics_returns,
                           SUM(cancellations) analytics_cancellations,
                           SUM(views) views, SUM(cart_adds) cart_adds,
                           MAX(CASE WHEN source='demo' THEN 1 ELSE 0 END) has_demo
                    FROM sales_daily WHERE day BETWEEN ? AND ? GROUP BY sku
                ), r AS (
                    SELECT sku, SUM(quantity) returns
                    FROM return_events WHERE day BETWEEN ? AND ? GROUP BY sku
                ), c AS (
                    SELECT sku, SUM(quantity) cancellations
                    FROM cancellation_events WHERE day BETWEEN ? AND ? GROUP BY sku
                ), d AS (
                    SELECT sku, SUM(quantity) delivered
                    FROM delivery_events WHERE day BETWEEN ? AND ? GROUP BY sku
                )
                SELECT s.sku, COALESCE(p.offer_id, '') offer_id,
                       CASE WHEN COALESCE(p.name, '')<>'' THEN p.name ELSE s.product_name END product_name,
                       s.revenue, s.orders, s.delivered, s.analytics_returns,
                       s.analytics_cancellations, s.views, s.cart_adds, s.has_demo,
                       COALESCE(r.returns, 0) event_returns,
                       COALESCE(c.cancellations, 0) event_cancellations,
                       COALESCE(d.delivered, 0) event_delivered
                FROM s LEFT JOIN products p ON p.sku=s.sku
                LEFT JOIN r ON r.sku=s.sku LEFT JOIN c ON c.sku=s.sku
                LEFT JOIN d ON d.sku=s.sku
                ORDER BY s.revenue DESC LIMIT ?
                """,
                (
                    date_from, date_to, date_from, date_to,
                    date_from, date_to, date_from, date_to, limit,
                ),
            ).fetchall()
            coverage = db.execute(
                """
                SELECT metric, COUNT(*) days, MAX(source) source FROM data_coverage
                WHERE day BETWEEN ? AND ?
                  AND metric IN ('returns', 'cancellations', 'deliveries', 'traffic')
                GROUP BY metric
                """,
                (date_from, date_to),
            ).fetchall()
        expected_days = (date.fromisoformat(date_to) - date.fromisoformat(date_from)).days + 1
        coverage_by_metric = {row["metric"]: int(row["days"]) for row in coverage}
        coverage_source = {
            row["metric"]: str(row["source"] or "") for row in coverage
        }
        result: list[dict[str, Any]] = []
        for raw in rows:
            row = dict(raw)
            demo = bool(row.pop("has_demo", 0))
            return_available = coverage_by_metric.get("returns", 0) >= expected_days or demo
            cancellation_available = coverage_by_metric.get("cancellations", 0) >= expected_days or demo
            row["returns_available"] = return_available or row["analytics_returns"] > 0
            row["returns_source"] = (
                "finance_proxy" if coverage_source.get("returns") == "finance"
                else "seller_analytics"
            )
            row["returns_exact"] = demo or row["returns_source"] != "finance_proxy"
            row["cancellations_available"] = (
                cancellation_available or row["analytics_cancellations"] > 0
            )
            delivery_available = coverage_by_metric.get("deliveries", 0) >= expected_days or demo
            row["delivered_available"] = (
                delivery_available or row["delivered"] > 0 or row["orders"] == 0
            )
            if coverage_by_metric.get("deliveries", 0):
                row["delivered"] = row.pop("event_delivered")
            else:
                row.pop("event_delivered", None)
            traffic_available = coverage_by_metric.get("traffic", 0) >= expected_days
            row["traffic_available"] = (
                traffic_available or demo or row["views"] > 0
                or row["cart_adds"] > 0 or row["orders"] == 0
            )
            row["returns"] = (
                row.pop("event_returns") if coverage_by_metric.get("returns", 0)
                else row.pop("analytics_returns")
            )
            row.pop("analytics_returns", None)
            row.pop("event_returns", None)
            row["cancellations"] = (
                row.pop("event_cancellations") if coverage_by_metric.get("cancellations", 0)
                else row.pop("analytics_cancellations")
            )
            row.pop("analytics_cancellations", None)
            row.pop("event_cancellations", None)
            row["conversion"] = (
                row["orders"] * 100.0 / row["views"]
                if row["traffic_available"] and row["views"] else None
            )
            row["cancellation_rate"] = (
                row["cancellations"] * 100.0 / row["orders"]
                if row["cancellations_available"] and row["orders"] else None
            )
            result.append(row)
        return result

    def weekly_report(self, week_end: str) -> dict[str, Any]:
        """Return a completed Monday-Sunday week and its prior-week comparison."""
        current_end = date.fromisoformat(week_end)
        current_start = current_end - timedelta(days=6)
        previous_end = current_start - timedelta(days=1)
        previous_start = previous_end - timedelta(days=6)

        current = self.dashboard_summary(current_start.isoformat(), current_end.isoformat())
        previous = self.dashboard_summary(previous_start.isoformat(), previous_end.isoformat())

        def enrich(values: dict[str, Any]) -> dict[str, Any]:
            row = dict(values)
            orders = float(row.get("orders") or 0)
            revenue = float(row.get("revenue") or 0)
            row["ad_order_share"] = (
                float(row.get("ad_orders") or 0) / orders * 100 if orders else 0
            )
            row["acots"] = float(row.get("spend") or 0) / revenue * 100 if revenue else 0
            # Inventory and sellable days need a stock snapshot history.  Keep
            # them explicitly unavailable until that source is added.
            row["inventory"] = None
            row["sellable_days"] = None
            return row

        current = enrich(current)
        previous = enrich(previous)
        comparison: dict[str, float | None] = {}
        for key in ("revenue", "orders", "spend", "ad_orders", "ad_order_share", "acots"):
            old = float(previous.get(key) or 0)
            new = float(current.get(key) or 0)
            comparison[key] = (new / old - 1) * 100 if old else None
        comparison["inventory"] = None
        comparison["sellable_days"] = None

        trend = {
            row["day"]: row
            for row in self.daily_trend(current_start.isoformat(), current_end.isoformat())
        }
        daily: list[dict[str, Any]] = []
        cursor = current_start
        while cursor <= current_end:
            row = dict(trend.get(cursor.isoformat()) or {})
            row.setdefault("day", cursor.isoformat())
            for key in (
                "revenue", "orders", "delivered", "returns", "cancellations",
                "views", "cart_adds", "spend", "ad_revenue", "ad_orders",
            ):
                row.setdefault(key, 0)
            row["ad_order_share"] = (
                float(row["ad_orders"]) / float(row["orders"]) * 100
                if row["orders"] else 0
            )
            row["acots"] = (
                float(row["spend"]) / float(row["revenue"]) * 100
                if row["revenue"] else 0
            )
            row["inventory"] = None
            row["sellable_days"] = None
            daily.append(row)
            cursor += timedelta(days=1)
        return {
            "current_start": current_start.isoformat(),
            "current_end": current_end.isoformat(),
            "previous_start": previous_start.isoformat(),
            "previous_end": previous_end.isoformat(),
            "current": current,
            "previous": previous,
            "comparison": comparison,
            "daily": daily,
        }

    def _legacy_monthly_profit_rows(
        self, date_from: str, date_to: str, limit: int = 500,
    ) -> list[dict[str, Any]]:
        """Initial SKU-level P&L based on sales, finance net payout and local costs."""
        with self.connect() as db:
            rows = db.execute(
                """
                WITH s AS (
                    SELECT sku, MAX(product_name) product_name,
                           SUM(revenue) revenue, SUM(ordered_units) orders
                    FROM sales_daily WHERE day BETWEEN ? AND ? GROUP BY sku
                ), d AS (
                    SELECT sku, SUM(quantity) delivered
                    FROM delivery_events WHERE day BETWEEN ? AND ? GROUP BY sku
                ), f AS (
                    SELECT sku, COUNT(*) finance_rows, SUM(amount) platform_payout,
                           SUM(accruals_for_sale) sales_accrual,
                           SUM(sale_commission) commission,
                           SUM(delivery_charge + return_delivery_charge) logistics
                    FROM finance_transactions
                    WHERE substr(operation_date, 1, 10) BETWEEN ? AND ? AND sku<>''
                    GROUP BY sku
                ), a AS (
                    SELECT sku, COUNT(*) ad_rows, SUM(spend) ad_spend
                    FROM ad_daily WHERE day BETWEEN ? AND ? AND sku<>'' GROUP BY sku
                )
                SELECT s.sku, COALESCE(p.offer_id, '') offer_id,
                       CASE WHEN COALESCE(p.name, '')<>'' THEN p.name ELSE s.product_name END product_name,
                       s.revenue, s.orders, COALESCE(d.delivered, s.orders) cost_units,
                       COALESCE(f.finance_rows, 0) finance_rows,
                       COALESCE(f.platform_payout, 0) platform_payout,
                       COALESCE(f.sales_accrual, 0) sales_accrual,
                       COALESCE(f.commission, 0) commission,
                       COALESCE(f.logistics, 0) logistics,
                       COALESCE(a.ad_rows, 0) ad_rows, COALESCE(a.ad_spend, 0) ad_spend,
                       pc.unit_cost, pc.first_mile_cost, COALESCE(pc.note, '') note
                FROM s
                LEFT JOIN products p ON p.sku=s.sku
                LEFT JOIN d ON d.sku=s.sku
                LEFT JOIN f ON f.sku=s.sku
                LEFT JOIN a ON a.sku=s.sku
                LEFT JOIN product_costs pc ON pc.sku=s.sku
                ORDER BY s.revenue DESC LIMIT ?
                """,
                (
                    date_from, date_to, date_from, date_to,
                    date_from, date_to, date_from, date_to, limit,
                ),
            ).fetchall()
        result: list[dict[str, Any]] = []
        for raw in rows:
            row = dict(raw)
            row["other_fees"] = (
                float(row["platform_payout"] or 0)
                - float(row["sales_accrual"] or 0)
                - float(row["commission"] or 0)
            )
            complete = row["unit_cost"] is not None and row["first_mile_cost"] is not None
            row["cost_complete"] = complete
            if complete:
                row["product_cost_total"] = -float(row["unit_cost"]) * int(row["cost_units"] or 0)
                row["first_mile_total"] = -float(row["first_mile_cost"]) * int(row["cost_units"] or 0)
                row["profit"] = (
                    float(row["platform_payout"] or 0)
                    + row["product_cost_total"] + row["first_mile_total"]
                )
                row["profit_rate"] = (
                    row["profit"] / float(row["revenue"]) * 100 if row["revenue"] else None
                )
            else:
                row["product_cost_total"] = None
                row["first_mile_total"] = None
                row["profit"] = None
                row["profit_rate"] = None
            result.append(row)
        return result

    def _legacy_monthly_profit_summary(self, date_from: str, date_to: str) -> dict[str, Any]:
        rows = self._legacy_monthly_profit_rows(date_from, date_to, limit=100000)
        with self.connect() as db:
            finance = db.execute(
                """
                SELECT COUNT(*) rows_count, COALESCE(SUM(amount), 0) platform_payout
                FROM finance_transactions
                WHERE substr(operation_date, 1, 10) BETWEEN ? AND ?
                """,
                (date_from, date_to),
            ).fetchone()
            ads = db.execute(
                "SELECT COALESCE(SUM(spend), 0) spend FROM ad_daily "
                "WHERE day BETWEEN ? AND ? AND sku=''",
                (date_from, date_to),
            ).fetchone()
        missing = sum(1 for row in rows if not row["cost_complete"] and row["orders"])
        complete_rows = [row for row in rows if row["cost_complete"]]
        result: dict[str, Any] = {
            "revenue": sum(float(row["revenue"] or 0) for row in rows),
            "orders": sum(int(row["orders"] or 0) for row in rows),
            "platform_payout": float(finance["platform_payout"] or 0),
            "finance_available": int(finance["rows_count"] or 0) > 0,
            "ad_spend": float(ads["spend"] or 0),
            "product_cost": sum(float(row["product_cost_total"] or 0) for row in complete_rows),
            "first_mile": sum(float(row["first_mile_total"] or 0) for row in complete_rows),
            "missing_cost_skus": missing,
            "configured_cost_skus": len(complete_rows),
            "product_count": len(rows),
        }
        if not missing and rows and result["finance_available"]:
            result["profit"] = (
                result["platform_payout"] + result["product_cost"] + result["first_mile"]
            )
            result["profit_rate"] = (
                result["profit"] / result["revenue"] * 100 if result["revenue"] else None
            )
        else:
            result["profit"] = None
            result["profit_rate"] = None
        return result

    def _legacy_monthly_finance_breakdown(self, date_from: str, date_to: str) -> list[dict[str, Any]]:
        with self.connect() as db:
            rows = db.execute(
                """
                SELECT operation_type, COUNT(*) rows_count, SUM(amount) amount
                FROM finance_transactions
                WHERE substr(operation_date, 1, 10) BETWEEN ? AND ?
                GROUP BY operation_type ORDER BY ABS(SUM(amount)) DESC
                """,
                (date_from, date_to),
            ).fetchall()
        return [dict(row) for row in rows]

    def monthly_profit_rows(
        self, date_from: str, date_to: str, limit: int = 500,
    ) -> list[dict[str, Any]]:
        """SKU P&L based on cached Seller accrual transactions.

        Only a transaction containing one unique SKU is assigned to that SKU.
        Store-level and multi-SKU rows remain unallocated for reconciliation.
        """
        snapshot = self._monthly_finance_snapshot(date_from, date_to)
        with self.connect() as db:
            sales_rows = db.execute(
                "SELECT sku, MAX(product_name) product_name, SUM(revenue) revenue, "
                "SUM(ordered_units) orders FROM sales_daily "
                "WHERE day BETWEEN ? AND ? GROUP BY sku",
                (date_from, date_to),
            ).fetchall()
            delivered_rows = db.execute(
                "SELECT sku, SUM(quantity) delivered FROM delivery_events "
                "WHERE day BETWEEN ? AND ? GROUP BY sku",
                (date_from, date_to),
            ).fetchall()
            product_rows = db.execute("SELECT sku, offer_id, name FROM products").fetchall()
            cost_rows = db.execute(
                "SELECT sku, unit_cost, first_mile_cost, unit_cost_cny, "
                "first_mile_cost_cny, length_cm, width_cm, height_cm, weight_kg, "
                "note FROM product_costs"
            ).fetchall()
        sales = {str(row["sku"]): dict(row) for row in sales_rows}
        delivered = {str(row["sku"]): int(row["delivered"] or 0) for row in delivered_rows}
        products = {str(row["sku"]): dict(row) for row in product_rows}
        costs = {str(row["sku"]): dict(row) for row in cost_rows}
        result: list[dict[str, Any]] = []
        rate = self.rub_per_cny()
        for sku in set(sales) | set(snapshot["by_sku"]):
            sale = sales.get(sku, {})
            finance = snapshot["by_sku"].get(sku, {})
            product = products.get(sku, {})
            cost = costs.get(sku, {})
            orders = int(sale.get("orders") or 0)
            cost_units = int(delivered.get(sku, orders))
            unit_cost_cny = cost.get("unit_cost_cny")
            first_mile_cost_cny = cost.get("first_mile_cost_cny")
            effective_unit_cost = (
                float(unit_cost_cny) * rate
                if unit_cost_cny is not None else cost.get("unit_cost")
            )
            effective_first_mile = (
                cost.get("first_mile_cost")
                if cost.get("first_mile_cost") is not None else (
                    float(first_mile_cost_cny) * rate
                    if first_mile_cost_cny is not None else None
                )
            )
            row: dict[str, Any] = {
                "sku": sku, "offer_id": str(product.get("offer_id") or ""),
                "product_name": str(product.get("name") or sale.get("product_name") or ""),
                "revenue": float(sale.get("revenue") or 0), "orders": orders,
                "cost_units": cost_units,
                "finance_rows": int(finance.get("finance_rows") or 0),
                "platform_payout": float(finance.get("platform_payout") or 0),
                "sales_accrual": float(finance.get("sales") or 0),
                "returns_accrual": float(finance.get("returns") or 0),
                "commission": float(finance.get("commission") or 0),
                "ad_spend": float(finance.get("advertising") or 0),
                "delivery": float(finance.get("delivery") or 0),
                "last_mile": float(finance.get("last_mile") or 0),
                "return_logistics": float(finance.get("return_logistics") or 0),
                "acquiring": float(finance.get("acquiring") or 0),
                "fbo_storage": float(finance.get("fbo_storage") or 0),
                "penalties_compensations": float(finance.get("penalties_compensations") or 0),
                "other_fees": float(finance.get("other") or 0),
                "unit_cost": effective_unit_cost,
                "first_mile_cost": effective_first_mile,
                "unit_cost_cny": unit_cost_cny,
                "first_mile_cost_cny": first_mile_cost_cny,
                "rub_per_cny": rate,
                "length_cm": cost.get("length_cm"), "width_cm": cost.get("width_cm"),
                "height_cm": cost.get("height_cm"), "weight_kg": cost.get("weight_kg"),
                "note": str(cost.get("note") or ""),
            }
            row["sales_returns"] = row["sales_accrual"] + row["returns_accrual"]
            row["logistics"] = row["delivery"] + row["last_mile"] + row["return_logistics"]
            row["average_delivery_cost"] = (
                abs(float(row["logistics"])) / cost_units if cost_units else None
            )
            unit_cost_available = row["unit_cost"] is not None
            first_mile_available = row["first_mile_cost"] is not None
            row["product_cost_total"] = (
                -float(row["unit_cost"]) * cost_units if unit_cost_available else None
            )
            row["first_mile_total"] = (
                -float(row["first_mile_cost"]) * cost_units
                if first_mile_available else None
            )
            complete = unit_cost_available and first_mile_available
            row["cost_complete"] = complete
            if complete:
                row["profit"] = row["platform_payout"] + row["product_cost_total"] + row["first_mile_total"]
                row["profit_rate"] = (
                    row["profit"] / float(row["revenue"]) * 100 if row["revenue"] else None
                )
            else:
                row.update({
                    "profit": None, "profit_rate": None,
                })
            result.append(row)
        result.sort(
            key=lambda row: (abs(float(row["platform_payout"])), float(row["revenue"])),
            reverse=True,
        )
        return result[:limit]

    def monthly_profit_summary(self, date_from: str, date_to: str) -> dict[str, Any]:
        rows = self.monthly_profit_rows(date_from, date_to, limit=100000)
        snapshot = self._monthly_finance_snapshot(date_from, date_to)
        cash_flow = self.finance_cash_flow_summary(date_from, date_to)
        seller_coverage = self.sales_coverage_info(date_from, date_to)
        missing = sum(1 for row in rows if not row["cost_complete"] and row["orders"])
        complete_rows = [row for row in rows if row["cost_complete"]]
        product_cost_rows = [row for row in rows if row["product_cost_total"] is not None]
        first_mile_rows = [row for row in rows if row["first_mile_total"] is not None]
        finance_available = int(snapshot["operation_count"] or 0) > 0
        payout = (
            float(cash_flow["platform_payout"])
            if cash_flow["available"] else float(snapshot["platform_payout"])
        )
        sales_returns = (
            float(cash_flow["sales_returns"])
            if cash_flow["available"]
            else float(snapshot["sales"] + snapshot["returns"])
        )
        # ``cash_flows.services_amount`` can contain settlement-transfer
        # adjustments which do not represent the Shop Economy total shown in
        # Seller.  The operation list is the auditable source and, for real
        # July data, reconciles exactly to the UI total while the raw summary
        # was overstated by more than five million RUB.
        payout = (
            float(snapshot["platform_payout"])
            if finance_available else float(cash_flow["platform_payout"])
        )
        other_adjustments = float(cash_flow.get("other_adjustments") or 0)
        result: dict[str, Any] = {
            "revenue": sum(float(row["revenue"] or 0) for row in rows),
            "orders": sum(int(row["orders"] or 0) for row in rows),
            "sales_returns": sales_returns,
            "accrual_fees": payout - sales_returns - other_adjustments,
            "other_adjustments": other_adjustments,
            "platform_payout": payout,
            "finance_available": finance_available or bool(cash_flow["available"]),
            "shop_economy_available": bool(cash_flow["available"]),
            "ad_spend": float(snapshot["advertising"] or 0),
            "product_cost": sum(float(row["product_cost_total"] or 0) for row in product_cost_rows),
            "first_mile": sum(float(row["first_mile_total"] or 0) for row in first_mile_rows),
            "costed_units": sum(int(row["cost_units"] or 0) for row in product_cost_rows),
            "configured_product_cost_skus": len(product_cost_rows),
            "missing_product_cost_skus": sum(
                1 for row in rows if row["orders"] and row["product_cost_total"] is None
            ),
            "missing_cost_skus": missing,
            "configured_cost_skus": len(complete_rows), "product_count": len(rows),
            "operation_count": int(snapshot["operation_count"] or 0),
            "exact_sku_operations": int(snapshot["exact_sku_operations"] or 0),
            "unallocated_operations": int(snapshot["unallocated_operations"] or 0),
            "unallocated_amount": float(snapshot["unallocated_amount"] or 0),
            "transaction_total": float(snapshot["platform_payout"] or 0),
            "cash_flow_reported_total": float(cash_flow.get("platform_payout") or 0),
            "reconciliation_difference": (
                float(cash_flow.get("platform_payout") or 0)
                - float(snapshot["platform_payout"] or 0)
                if cash_flow["available"] and finance_available else None
            ),
            "currency_code": cash_flow.get("currency_code") or "RUB",
            "seller_sales_coverage": seller_coverage,
            "seller_sales_complete": bool(seller_coverage["complete"]),
        }
        if not missing and rows and result["finance_available"]:
            result["profit"] = payout + result["product_cost"] + result["first_mile"]
            result["profit_rate"] = (
                result["profit"] / result["revenue"] * 100 if result["revenue"] else None
            )
        else:
            result["profit"] = None
            result["profit_rate"] = None
        tax_rate = max(0.0, _number(self.get_setting("local_tax_rate", "6")))
        payout_fee_rate = max(
            0.0, _number(self.get_setting("local_payout_fee_rate", "1"))
        )
        result["tax_rate"] = tax_rate
        result["payout_fee_rate"] = payout_fee_rate
        result["tax_amount"] = -max(0.0, float(result["revenue"])) * tax_rate / 100
        result["payout_fee"] = -max(0.0, float(result["platform_payout"])) * payout_fee_rate / 100
        result["pre_tax_profit"] = result["profit"]
        result["after_tax_profit"] = (
            float(result["profit"]) + result["tax_amount"] + result["payout_fee"]
            if result["profit"] is not None else None
        )
        return result

    def product_profit_rows(
        self, date_from: str, date_to: str, *, query: str = "", limit: int = 1000,
    ) -> list[dict[str, Any]]:
        """Local-shop product estimate with cached ads, routes and Finance costs.

        This is an operational estimate.  The monthly accrual P&L remains the
        authoritative settlement view because order and Finance dates differ.
        """
        base_rows = self.monthly_profit_rows(date_from, date_to, limit=100000)
        effective_ad_rows = self.effective_product_ad_rows(date_from, date_to)
        ad_rows_by_sku: dict[str, dict[str, Any]] = {}
        for row in effective_ad_rows:
            sku = str(row.get("sku") or "")
            target = ad_rows_by_sku.setdefault(sku, {
                "sku": sku, "ad_spend": 0.0, "ad_orders": 0,
                "ad_revenue": 0.0, "impressions": 0, "clicks": 0,
                "attribution_sources": set(),
            })
            target["ad_spend"] += _number(row.get("spend"))
            target["ad_orders"] += _integer(row.get("orders"))
            target["ad_revenue"] += _number(row.get("revenue"))
            target["impressions"] += _integer(row.get("impressions"))
            target["clicks"] += _integer(row.get("clicks"))
            target["attribution_sources"].add(str(row.get("attribution_source") or ""))
        with self.connect() as db:
            shop_ad_spend = _number(db.execute(
                """
                SELECT CASE WHEN EXISTS(
                    SELECT 1 FROM ad_daily
                    WHERE day BETWEEN ? AND ? AND sku=''
                ) THEN COALESCE((
                    SELECT SUM(ABS(spend)) FROM ad_daily
                    WHERE day BETWEEN ? AND ? AND sku=''
                ), 0) ELSE COALESCE((
                    SELECT SUM(ABS(spend)) FROM ad_daily
                    WHERE day BETWEEN ? AND ? AND sku<>''
                ), 0) END
                """,
                (
                    date_from, date_to, date_from, date_to,
                    date_from, date_to,
                ),
            ).fetchone()[0])
            route_rows = db.execute(
                """
                WITH route_source AS (
                    SELECT day, sku, posting_number, origin, destination
                    FROM posting_routes WHERE day BETWEEN ? AND ?
                    UNION ALL
                    SELECT day, sku, posting_number, origin, destination
                    FROM delivery_events WHERE day BETWEEN ? AND ?
                )
                SELECT sku, GROUP_CONCAT(DISTINCT NULLIF(origin, '')) origins,
                       GROUP_CONCAT(DISTINCT NULLIF(destination, '')) destinations,
                       COUNT(DISTINCT NULLIF(posting_number, '')) route_orders
                FROM route_source GROUP BY sku
                """,
                (date_from, date_to, date_from, date_to),
            ).fetchall()
        ads = ad_rows_by_sku
        routes = {str(row["sku"]): dict(row) for row in route_rows}
        tax_rate = max(0.0, _number(self.get_setting("local_tax_rate", "6")))
        payout_fee_rate = max(
            0.0, _number(self.get_setting("local_payout_fee_rate", "1"))
        )
        fallback_delivery = max(
            0.0, _number(self.get_setting("local_default_delivery_fee", "0"))
        )
        needle = str(query or "").strip().casefold()
        result: list[dict[str, Any]] = []
        for source in base_rows:
            sku = str(source.get("sku") or "")
            if needle and needle not in sku.casefold() and needle not in str(
                source.get("offer_id") or ""
            ).casefold():
                continue
            ad = ads.get(sku, {})
            route = routes.get(sku, {})
            orders = int(source.get("orders") or 0)
            revenue = float(source.get("revenue") or 0)
            ad_spend = abs(float(ad.get("ad_spend") or 0))
            # Finance advertising is a useful fallback when the product report
            # has not been synchronized yet; never add the two sources.
            if not ad_spend:
                ad_spend = abs(float(source.get("ad_spend") or 0))
            ad_orders = int(ad.get("ad_orders") or 0)
            unit_delivery = source.get("average_delivery_cost")
            # A zero calculated from *no Finance rows* is not evidence of free
            # delivery.  Use the editable fallback until real logistics
            # accruals are available; keep a genuine Finance zero unchanged.
            if (unit_delivery is None or not int(source.get("finance_rows") or 0)) \
                    and fallback_delivery:
                unit_delivery = fallback_delivery
            delivery_total = (
                float(unit_delivery) * orders if unit_delivery is not None else None
            )
            product_cost_total = (
                abs(float(source["product_cost_total"]))
                if source.get("product_cost_total") is not None else None
            )
            commission_total = abs(float(source.get("commission") or 0))
            acquiring_total = abs(float(source.get("acquiring") or 0))
            if product_cost_total is None or delivery_total is None:
                pre_tax = None
            else:
                pre_tax = (
                    revenue - product_cost_total - delivery_total
                    - commission_total - acquiring_total - ad_spend
                )
            tax = revenue * tax_rate / 100
            settlement_base = max(
                0.0, revenue - commission_total - acquiring_total - (delivery_total or 0)
            )
            payout_fee = settlement_base * payout_fee_rate / 100
            after_tax = pre_tax - tax - payout_fee if pre_tax is not None else None
            result.append({
                **source,
                "ad_spend_estimate": ad_spend,
                "ad_orders": ad_orders,
                "ad_revenue": float(ad.get("ad_revenue") or 0),
                "ad_attribution_source": ",".join(sorted(
                    value for value in ad.get("attribution_sources", set()) if value
                )),
                "ad_cost_per_order": ad_spend / ad_orders if ad_orders else None,
                "ad_spend_share": (
                    ad_spend / shop_ad_spend * 100 if shop_ad_spend else None
                ),
                "ad_sales_share": (
                    float(ad.get("ad_revenue") or 0) / revenue * 100 if revenue else None
                ),
                "tacos": ad_spend / revenue * 100 if revenue else None,
                "origins": str(route.get("origins") or ""),
                "destinations": str(route.get("destinations") or ""),
                "route_orders": int(route.get("route_orders") or 0),
                "estimated_delivery_unit": unit_delivery,
                "estimated_delivery_total": delivery_total,
                "commission_total": commission_total,
                "acquiring_total": acquiring_total,
                "tax_amount_estimate": tax,
                "payout_fee_estimate": payout_fee,
                "pre_tax_profit_estimate": pre_tax,
                "after_tax_profit_estimate": after_tax,
                "tax_rate": tax_rate,
                "payout_fee_rate": payout_fee_rate,
            })
        result.sort(key=lambda row: float(row.get("revenue") or 0), reverse=True)
        return result[:max(1, int(limit))]

    def product_ad_daily_rows(
        self, date_from: str, date_to: str, sku: str,
    ) -> list[dict[str, Any]]:
        """Daily Seller + safely attributed product Performance metrics.

        Official SKU rows are preferred.  Historical campaign-only rows are
        used only when a campaign title uniquely equals one offer ID or Ozon
        SKU; shared/generic campaigns remain store-level and are not allocated.
        """
        effective_ads = {
            str(row["day"]): row
            for row in self.effective_product_ad_rows(date_from, date_to, sku=sku)
        }
        with self.connect() as db:
            rows = db.execute(
                """
                WITH days AS (
                    SELECT day FROM sales_daily
                    WHERE sku=? AND day BETWEEN ? AND ?
                ), sales AS (
                    SELECT day, SUM(revenue) seller_revenue,
                           SUM(ordered_units) ordered_units
                    FROM sales_daily
                    WHERE sku=? AND day BETWEEN ? AND ? GROUP BY day
                ), store_ads AS (
                    SELECT day,
                           CASE WHEN SUM(CASE WHEN sku='' THEN 1 ELSE 0 END)>0
                           THEN SUM(CASE WHEN sku='' THEN ABS(spend) ELSE 0 END)
                           ELSE SUM(CASE WHEN sku<>'' THEN ABS(spend) ELSE 0 END)
                           END store_ad_spend
                    FROM ad_daily WHERE day BETWEEN ? AND ? GROUP BY day
                )
                SELECT days.day,
                       COALESCE(sales.seller_revenue, 0) seller_revenue,
                       COALESCE(sales.ordered_units, 0) ordered_units,
                       COALESCE(store_ads.store_ad_spend, 0) store_ad_spend
                FROM days LEFT JOIN sales USING(day)
                LEFT JOIN store_ads USING(day)
                ORDER BY days.day
                """,
                (
                    sku, date_from, date_to, sku, date_from, date_to,
                    date_from, date_to,
                ),
            ).fetchall()
        result: list[dict[str, Any]] = []
        for stored in rows:
            row = dict(stored)
            ad = effective_ads.get(str(row["day"]), {})
            row.update({
                "ad_spend": _number(ad.get("spend")),
                "ad_orders": _integer(ad.get("orders")),
                "ad_revenue": _number(ad.get("revenue")),
                "impressions": _integer(ad.get("impressions")),
                "clicks": _integer(ad.get("clicks")),
                "ad_attribution_source": str(ad.get("attribution_source") or ""),
            })
            seller_revenue = float(row["seller_revenue"] or 0)
            ad_spend = abs(float(row["ad_spend"] or 0))
            ad_orders = int(row["ad_orders"] or 0)
            ad_revenue = float(row["ad_revenue"] or 0)
            impressions = int(row["impressions"] or 0)
            clicks = int(row["clicks"] or 0)
            store_ad_spend = abs(float(row["store_ad_spend"] or 0))
            row.update({
                "ad_cost_per_order": ad_spend / ad_orders if ad_orders else None,
                "ad_spend_share": ad_spend / store_ad_spend * 100
                if store_ad_spend else None,
                "ad_sales_share": ad_revenue / seller_revenue * 100
                if seller_revenue else None,
                "acos": ad_spend / ad_revenue * 100 if ad_revenue else None,
                "tacos": ad_spend / seller_revenue * 100 if seller_revenue else None,
                "ctr": clicks / impressions * 100 if impressions else None,
            })
            result.append(row)
        return result

    def effective_product_ad_rows(
        self, date_from: str, date_to: str, *, sku: str = "",
    ) -> list[dict[str, Any]]:
        """Return safe product-level ads from the local Performance cache.

        Official rows with a non-empty SKU always win.  Historical campaign
        rows can only be attributed when the complete campaign title exactly
        equals one unique merchant offer ID or Ozon SKU.  Generic and
        multi-product campaigns therefore remain store-level data.
        """
        filters = ["day BETWEEN ? AND ?"]
        params: list[Any] = [date_from, date_to]
        if str(sku or "").strip():
            filters.append("sku=?")
            params.append(str(sku).strip())
        with self.connect() as db:
            rows = db.execute(
                f"""
                WITH candidate_map AS (
                    SELECT DISTINCT a.campaign_id, p.sku
                    FROM ad_daily a
                    LEFT JOIN campaigns c ON c.campaign_id=a.campaign_id
                    JOIN products p ON (
                        LOWER(TRIM(COALESCE(NULLIF(a.campaign_name,''), c.name))) =
                            LOWER(TRIM(p.offer_id))
                        OR TRIM(COALESCE(NULLIF(a.campaign_name,''), c.name)) =
                            TRIM(p.sku)
                    )
                    WHERE a.day BETWEEN ? AND ? AND a.sku=''
                          AND TRIM(COALESCE(NULLIF(a.campaign_name,''), c.name))<>''
                ), unique_map AS (
                    SELECT campaign_id, MIN(sku) sku
                    FROM candidate_map GROUP BY campaign_id
                    HAVING COUNT(DISTINCT sku)=1
                ), exact_rows AS (
                    SELECT day, campaign_id, sku, impressions, clicks, cart_adds,
                           orders, revenue, spend, 'official_sku' attribution_source
                    FROM ad_daily
                    WHERE day BETWEEN ? AND ? AND sku<>''
                ), mapped_rows AS (
                    SELECT a.day, a.campaign_id, m.sku, a.impressions, a.clicks,
                           a.cart_adds, a.orders, a.revenue, a.spend,
                           'unique_campaign_title' attribution_source
                    FROM ad_daily a JOIN unique_map m USING(campaign_id)
                    WHERE a.day BETWEEN ? AND ? AND a.sku=''
                      AND NOT EXISTS (
                          SELECT 1 FROM exact_rows e
                          WHERE e.day=a.day AND e.campaign_id=a.campaign_id
                                AND e.sku=m.sku
                      )
                ), effective AS (
                    SELECT * FROM exact_rows UNION ALL SELECT * FROM mapped_rows
                )
                SELECT day, sku, SUM(impressions) impressions, SUM(clicks) clicks,
                       SUM(cart_adds) cart_adds, SUM(orders) orders,
                       SUM(revenue) revenue, SUM(spend) spend,
                       CASE
                         WHEN MIN(attribution_source)=MAX(attribution_source)
                         THEN MIN(attribution_source) ELSE 'mixed'
                       END attribution_source
                FROM effective WHERE {' AND '.join(filters)}
                GROUP BY day, sku ORDER BY day, sku
                """,
                [date_from, date_to, date_from, date_to, date_from, date_to, *params],
            ).fetchall()
        return [dict(row) for row in rows]

    def import_delivery_tariffs(self, path: str | Path) -> dict[str, Any]:
        """Replace the local Ozon route/volume tariff cache from an xlsx file."""
        source = Path(path)
        rows = read_tariff_workbook(source)
        with self.connect() as db:
            db.execute("DELETE FROM delivery_tariffs")
            db.executemany(
                """
                INSERT INTO delivery_tariffs(
                    origin, destination, volume_min_l, volume_max_l,
                    price_le_300, price_gt_300, source_file
                ) VALUES (?, ?, ?, ?, ?, ?, ?)
                """,
                (
                    (
                        row["origin"], row["destination"], row["volume_min_l"],
                        row["volume_max_l"], row["price_le_300"],
                        row["price_gt_300"], source.name,
                    )
                    for row in rows
                ),
            )
        self.save_settings({
            "delivery_tariff_source": str(source),
            "delivery_tariff_imported_at": datetime.now().isoformat(timespec="seconds"),
        })
        self._delivery_tariff_cache = None
        return {
            "row_count": len(rows),
            "source_file": str(source),
            "routes": len({(row["origin"], row["destination"]) for row in rows}),
        }

    def delivery_tariff_info(self) -> dict[str, Any]:
        with self.connect() as db:
            row = db.execute(
                """
                SELECT COUNT(*) row_count,
                       COUNT(DISTINCT origin || '→' || destination) routes,
                       MAX(source_file) source_file, MAX(updated_at) updated_at
                FROM delivery_tariffs
                """
            ).fetchone()
        return dict(row) if row else {
            "row_count": 0, "routes": 0, "source_file": "", "updated_at": "",
        }

    def _delivery_tariff_price(
        self, origin: str, destination: str, volume_l: float,
        unit_price_rub: float,
    ) -> float | None:
        normalized_origin = normalize_cluster(origin)
        normalized_destination = normalize_cluster(destination)
        if not normalized_origin or not normalized_destination or volume_l <= 0:
            return None
        with self.connect() as db:
            row = db.execute(
                """
                SELECT price_le_300, price_gt_300
                FROM delivery_tariffs
                WHERE origin=? AND destination=?
                  AND volume_min_l <= ? AND volume_max_l >= ?
                ORDER BY (volume_max_l-volume_min_l), volume_min_l DESC
                LIMIT 1
                """,
                (normalized_origin, normalized_destination, volume_l, volume_l),
            ).fetchone()
        if not row:
            return None
        return float(row["price_le_300"] if unit_price_rub <= 300 else row["price_gt_300"])

    def order_detail_rows(
        self, date_from: str, date_to: str, *, query: str = "",
        scheme: str = "", limit: int = 5000,
    ) -> list[dict[str, Any]]:
        """One cached Seller posting item per row with estimated base delivery."""
        filters = ["r.day BETWEEN ? AND ?"]
        parameters: list[Any] = [date_from, date_to]
        normalized_query = str(query or "").strip()
        if normalized_query:
            filters.append(
                "(r.posting_number LIKE ? OR r.offer_id LIKE ? OR r.sku LIKE ?)"
            )
            wildcard = f"%{normalized_query}%"
            parameters.extend((wildcard, wildcard, wildcard))
        normalized_scheme = str(scheme or "").strip()
        if normalized_scheme and normalized_scheme not in {"全部", "全部履约"}:
            filters.append("UPPER(r.scheme) LIKE ?")
            parameters.append(f"%{normalized_scheme.upper()}%")
        parameters.append(max(1, int(limit)))
        with self.connect() as db:
            rows = db.execute(
                f"""
                SELECT r.*, COALESCE(p.offer_id, r.offer_id) resolved_offer_id,
                       pc.length_cm, pc.width_cm, pc.height_cm, pc.weight_kg
                FROM posting_routes r
                LEFT JOIN products p ON p.sku=r.sku
                LEFT JOIN product_costs pc ON pc.sku=r.sku
                WHERE {' AND '.join(filters)}
                ORDER BY r.day DESC, r.posting_number DESC, r.sku
                LIMIT ?
                """,
                parameters,
            ).fetchall()
            if self._delivery_tariff_cache is None:
                tariff_rows = db.execute(
                    """
                    SELECT origin, destination, volume_min_l, volume_max_l,
                           price_le_300, price_gt_300
                    FROM delivery_tariffs
                    ORDER BY origin, destination, volume_min_l, volume_max_l
                    """
                ).fetchall()
                tariffs: dict[
                    tuple[str, str], list[dict[str, float | str]]
                ] = {}
                for tariff in tariff_rows:
                    value = dict(tariff)
                    tariffs.setdefault(
                        (str(value["origin"]), str(value["destination"])), []
                    ).append(value)
                self._delivery_tariff_cache = tariffs
            # Posting APIs may return a physical warehouse name/id while the
            # official delivery workbook is keyed by macrolocal cluster.  The
            # two read-only Seller directories are cached during inventory
            # sync, so build an alias map locally without another API call.
            cluster_aliases: dict[str, str] = {}

            def remember_alias(value: Any, cluster_name: str) -> None:
                alias = normalize_cluster(value)
                target = normalize_cluster(cluster_name)
                if not alias or not target:
                    return
                if alias.casefold() in {
                    "fbo", "fbs", "rfbs", "true", "false", "russia", "俄罗斯",
                }:
                    return
                cluster_aliases.setdefault(alias.casefold(), target)

            cluster_rows = db.execute(
                """
                SELECT macrolocal_cluster_id, cluster_name, fulfillments_json
                FROM ozon_clusters
                """
            ).fetchall()
            cluster_names_by_id: dict[str, str] = {}
            for cluster_row in cluster_rows:
                cluster_name = str(cluster_row["cluster_name"] or "")
                macro_id = str(cluster_row["macrolocal_cluster_id"] or "")
                cluster_names_by_id[macro_id] = cluster_name
                remember_alias(cluster_name, cluster_name)
                remember_alias(macro_id, cluster_name)
                try:
                    fulfillment_data = json.loads(
                        cluster_row["fulfillments_json"] or "[]"
                    )
                except (TypeError, ValueError):
                    fulfillment_data = []

                def collect_directory_values(value: Any) -> None:
                    if isinstance(value, dict):
                        for nested in value.values():
                            collect_directory_values(nested)
                    elif isinstance(value, list):
                        for nested in value:
                            collect_directory_values(nested)
                    elif isinstance(value, (str, int)):
                        remember_alias(value, cluster_name)

                collect_directory_values(fulfillment_data)

            warehouse_rows = db.execute(
                """
                SELECT seller_warehouse_id, warehouse_name, region, city,
                       address, macrolocal_cluster_id
                FROM seller_supply_warehouses
                """
            ).fetchall()
            for warehouse in warehouse_rows:
                cluster_name = cluster_names_by_id.get(
                    str(warehouse["macrolocal_cluster_id"] or ""), ""
                )
                if not cluster_name:
                    continue
                for key in (
                    "seller_warehouse_id", "warehouse_name", "region", "city",
                    "address",
                ):
                    remember_alias(warehouse[key], cluster_name)
        tariffs = self._delivery_tariff_cache or {}

        def resolve_directory_cluster(value: Any) -> str:
            normalized = normalize_cluster(value)
            if not normalized:
                return ""
            direct = cluster_aliases.get(normalized.casefold())
            if direct:
                return direct
            # Warehouse values sometimes include a suffix such as `_RFЦ` or a
            # full street address.  Prefer a sufficiently distinctive cached
            # alias contained in that text; do not infer from very short names.
            folded = normalized.casefold()
            candidates = [
                (alias, target) for alias, target in cluster_aliases.items()
                if len(alias) >= 5 and (alias in folded or folded in alias)
            ]
            if candidates:
                return max(candidates, key=lambda item: len(item[0]))[1]
            return normalized

        result: list[dict[str, Any]] = []
        for stored in rows:
            row = dict(stored)
            quantity = max(1, int(row.get("quantity") or 1))
            order_price = float(row.get("order_price") or 0)
            unit_price = order_price / quantity if quantity else order_price
            dimensions = (row.get("length_cm"), row.get("width_cm"), row.get("height_cm"))
            volume_l: float | None = None
            if all(value is not None and float(value) > 0 for value in dimensions):
                volume_l = math.prod(float(value) for value in dimensions) / 1000
            normalized_origin = resolve_directory_cluster(row.get("origin"))
            normalized_destination = resolve_directory_cluster(
                row.get("destination")
            )
            delivery_unit = None
            if volume_l is not None:
                for tariff in tariffs.get(
                    (normalized_origin, normalized_destination), []
                ):
                    if float(tariff["volume_min_l"]) <= volume_l <= float(
                        tariff["volume_max_l"]
                    ):
                        delivery_unit = float(
                            tariff["price_le_300"] if unit_price <= 300
                            else tariff["price_gt_300"]
                        )
                        break
            if volume_l is None:
                match_status = "缺少尺寸"
            elif not normalized_origin or not normalized_destination:
                match_status = "缺少集群"
            elif delivery_unit is None:
                match_status = "未匹配路线/体积"
            else:
                match_status = "已匹配"
            row.update({
                "offer_id": str(row.get("resolved_offer_id") or row.get("offer_id") or ""),
                "origin_normalized": normalized_origin,
                "destination_normalized": normalized_destination,
                "cluster_directory_used": bool(cluster_aliases),
                "unit_price": unit_price,
                "volume_l": volume_l,
                "estimated_delivery_unit": delivery_unit,
                "estimated_delivery_total": (
                    delivery_unit * quantity if delivery_unit is not None else None
                ),
                "tariff_status": match_status,
            })
            result.append(row)
        return result

    def product_unit_profit_rows(
        self, date_from: str, date_to: str, *, query: str = "", limit: int = 2000,
    ) -> list[dict[str, Any]]:
        """Per-unit local product forecast; period data only supplies rate references."""
        source_rows = self.product_profit_rows(
            date_from, date_to, query=query, limit=max(10000, limit),
        )
        order_rows = self.order_detail_rows(
            date_from, date_to, query=query, limit=20000,
        )
        route_costs: dict[str, list[tuple[float, int]]] = {}
        for order in order_rows:
            fee = order.get("estimated_delivery_unit")
            if fee is None:
                continue
            route_costs.setdefault(str(order.get("sku") or ""), []).append(
                (float(fee), max(1, int(order.get("quantity") or 1)))
            )
        result: list[dict[str, Any]] = []
        for source in source_rows:
            row = dict(source)
            units = int(row.get("orders") or 0)
            divisor = units if units > 0 else 1
            selling_price = float(row.get("revenue") or 0) / divisor if units else 0.0
            ad_unit = float(row.get("ad_spend_estimate") or 0) / divisor if units else 0.0
            commission_unit = float(row.get("commission_total") or 0) / divisor if units else 0.0
            acquiring_unit = float(row.get("acquiring_total") or 0) / divisor if units else 0.0
            matches = route_costs.get(str(row.get("sku") or ""), [])
            matched_quantity = sum(quantity for _fee, quantity in matches)
            route_delivery = (
                sum(fee * quantity for fee, quantity in matches) / matched_quantity
                if matched_quantity else None
            )
            delivery_unit = route_delivery
            if delivery_unit is None:
                delivery_unit = row.get("estimated_delivery_unit")
            unit_cost = row.get("unit_cost")
            first_mile = row.get("first_mile_cost")
            tax = selling_price * float(row.get("tax_rate") or 0) / 100
            settlement_base = max(
                0.0, selling_price - commission_unit - acquiring_unit
                - float(delivery_unit or 0)
            )
            payout_fee = settlement_base * float(row.get("payout_fee_rate") or 0) / 100
            complete = unit_cost is not None and first_mile is not None and delivery_unit is not None
            pre_tax = (
                selling_price - float(unit_cost) - float(first_mile)
                - float(delivery_unit) - commission_unit - acquiring_unit - ad_unit
                if complete else None
            )
            after_tax = pre_tax - tax - payout_fee if pre_tax is not None else None
            row.update({
                "reference_units": units,
                "selling_price_unit": selling_price,
                "ad_spend_unit": ad_unit,
                "commission_unit": commission_unit,
                "acquiring_unit": acquiring_unit,
                "delivery_unit": delivery_unit,
                "delivery_source": "集群配送表" if route_delivery is not None else "Finance/默认值",
                "route_match_units": matched_quantity,
                "tax_unit": tax,
                "payout_fee_unit": payout_fee,
                "pre_tax_profit_unit": pre_tax,
                "after_tax_profit_unit": after_tax,
                "unit_profit_complete": complete,
            })
            result.append(row)
        result.sort(key=lambda item: float(item.get("selling_price_unit") or 0), reverse=True)
        return result[:max(1, int(limit))]

    def daily_product_profit_rows(
        self, date_from: str, date_to: str, *, query: str = "", limit: int = 10000,
    ) -> list[dict[str, Any]]:
        """Daily SKU sales, safely attributed ads and estimated operating profit."""
        profiles = {
            str(row.get("sku") or ""): row for row in self.product_unit_profit_rows(
                date_from, date_to, query=query, limit=10000,
            )
        }
        filters = ["s.day BETWEEN ? AND ?"]
        params: list[Any] = [date_from, date_to]
        needle = str(query or "").strip()
        if needle:
            filters.append("(s.sku LIKE ? OR COALESCE(p.offer_id, '') LIKE ?)")
            params.extend((f"%{needle}%", f"%{needle}%"))
        params.append(max(1, int(limit)))
        effective_ads = {
            (str(row["day"]), str(row["sku"])): row
            for row in self.effective_product_ad_rows(date_from, date_to)
        }
        with self.connect() as db:
            rows = db.execute(
                f"""
                SELECT s.day, s.sku, COALESCE(p.offer_id, '') offer_id,
                       SUM(s.ordered_units) units, SUM(s.revenue) revenue
                FROM sales_daily s
                LEFT JOIN products p ON p.sku=s.sku
                WHERE {' AND '.join(filters)}
                GROUP BY s.day, s.sku, p.offer_id
                HAVING SUM(s.ordered_units)<>0 OR SUM(s.revenue)<>0
                ORDER BY s.day DESC, SUM(s.revenue) DESC
                LIMIT ?
                """,
                params,
            ).fetchall()
        result: list[dict[str, Any]] = []
        for stored in rows:
            row = dict(stored)
            ad = effective_ads.get((str(row.get("day") or ""), str(row.get("sku") or "")), {})
            row.update({
                "ad_spend": abs(_number(ad.get("spend"))),
                "ad_orders": _integer(ad.get("orders")),
                "ad_revenue": _number(ad.get("revenue")),
                "ad_attribution_source": str(ad.get("attribution_source") or ""),
            })
            profile = profiles.get(str(row.get("sku") or ""), {})
            units = int(row.get("units") or 0)
            revenue = float(row.get("revenue") or 0)
            ad_spend = abs(float(row.get("ad_spend") or 0))
            variable_non_ad = sum(float(profile.get(key) or 0) for key in (
                "unit_cost", "first_mile_cost", "delivery_unit",
                "commission_unit", "acquiring_unit", "tax_unit", "payout_fee_unit",
            ))
            profit = revenue - ad_spend - variable_non_ad * units if profile else None
            row.update({
                "tacos": ad_spend / revenue * 100 if revenue else None,
                "ad_cost_per_order": ad_spend / units if units else None,
                "ad_cost_per_ad_order": (
                    ad_spend / int(row.get("ad_orders") or 0)
                    if int(row.get("ad_orders") or 0) else None
                ),
                "estimated_profit": profit,
                "profit_rate": profit / revenue * 100 if profit is not None and revenue else None,
                "cost_profile_complete": bool(profile.get("unit_profit_complete")),
            })
            result.append(row)
        return result

    def product_series_rows(
        self, date_from: str, date_to: str, *, period: str = "day", query: str = "",
    ) -> list[dict[str, Any]]:
        """Aggregate Seller sales by a stable offer-id series prefix.

        A prefix such as ``GJYB001`` groups colour/size variants while the
        original SKU remains untouched.  This is intentionally computed from
        local cached Seller rows and never triggers an API request.
        """
        needle = str(query or "").strip()
        params: list[Any] = [date_from, date_to]
        filters = ["s.day BETWEEN ? AND ?"]
        if needle:
            filters.append("(s.sku LIKE ? OR COALESCE(p.offer_id, '') LIKE ?)")
            params.extend((f"%{needle}%", f"%{needle}%"))
        with self.connect() as db:
            rows = db.execute(
                f"""
                SELECT s.day, s.sku, COALESCE(NULLIF(p.offer_id, ''), s.sku) offer_id,
                       SUM(s.ordered_units) units, SUM(s.revenue) revenue
                FROM sales_daily s
                LEFT JOIN products p ON p.sku=s.sku
                WHERE {' AND '.join(filters)}
                GROUP BY s.day, s.sku, p.offer_id
                HAVING SUM(s.ordered_units)<>0 OR SUM(s.revenue)<>0
                """,
                params,
            ).fetchall()

        grouped: dict[tuple[str, str], dict[str, Any]] = {}
        for stored in rows:
            row = dict(stored)
            offer_id = str(row.get("offer_id") or row.get("sku") or "").strip()
            match = re.match(r"^([A-Za-z]+\d+)", offer_id)
            series = match.group(1).upper() if match else (offer_id.split("-")[0] or offer_id)
            day_value = date.fromisoformat(str(row["day"])[:10])
            if period == "month":
                period_label = day_value.strftime("%Y-%m")
            elif period == "week":
                week_start = day_value - timedelta(days=day_value.weekday())
                week_end = week_start + timedelta(days=6)
                period_label = f"{week_start.isoformat()} 至 {week_end.isoformat()}"
            else:
                period_label = day_value.isoformat()
            key = (period_label, series)
            target = grouped.setdefault(key, {
                "period": period_label, "series": series, "skus": set(),
                "units": 0, "revenue": 0.0,
            })
            target["skus"].add(str(row.get("sku") or ""))
            target["units"] += int(row.get("units") or 0)
            target["revenue"] += float(row.get("revenue") or 0)
        result: list[dict[str, Any]] = []
        for value in grouped.values():
            result.append({
                "period": value["period"], "series": value["series"],
                "sku_count": len(value["skus"]), "units": value["units"],
                "revenue": value["revenue"],
            })
        return sorted(result, key=lambda item: (item["period"], item["series"]), reverse=True)

    def shipment_tracking_rows(self, query: str = "") -> list[dict[str, Any]]:
        needle = str(query or "").strip()
        with self.connect() as db:
            rows = db.execute(
                """
                SELECT * FROM shipment_tracking
                WHERE ?='' OR tracking_id LIKE ? OR product_name LIKE ?
                      OR batch_no LIKE ? OR shop_name LIKE ?
                ORDER BY CASE WHEN foreign_arrival='' THEN 0 ELSE 1 END,
                         foreign_arrival DESC, updated_at DESC
                """,
                (needle, *(f"%{needle}%",) * 4),
            ).fetchall()
        return [dict(row) for row in rows]

    def shipment_tracking_row(self, tracking_id: str) -> dict[str, Any] | None:
        with self.connect() as db:
            row = db.execute(
                "SELECT * FROM shipment_tracking WHERE tracking_id=?", (str(tracking_id),)
            ).fetchone()
        return dict(row) if row else None

    def upsert_shipment_tracking(
        self, rows: Iterable[dict[str, Any]], *, source: str = "feishu",
    ) -> list[dict[str, Any]]:
        """Upsert tracking rows and return entries that just reached a foreign warehouse."""
        arrivals: list[dict[str, Any]] = []
        with self.connect() as db:
            for raw in rows:
                tracking_id = str(raw.get("tracking_id") or "").strip()
                if not tracking_id:
                    continue
                existing = db.execute(
                    "SELECT foreign_arrival, notified_foreign_arrival FROM shipment_tracking "
                    "WHERE tracking_id=?", (tracking_id,),
                ).fetchone()
                foreign_arrival = str(raw.get("foreign_arrival") or "").strip()
                notified = str(existing["notified_foreign_arrival"] or "") if existing else ""
                should_notify = bool(foreign_arrival and notified != foreign_arrival)
                db.execute(
                    """
                    INSERT INTO shipment_tracking(
                        tracking_id, product_name, batch_no, shop_name, quantity,
                        cargo_status, channel, domestic_arrival, foreign_arrival,
                        notified_foreign_arrival, source, remote_record_id
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                    ON CONFLICT(tracking_id) DO UPDATE SET
                        product_name=excluded.product_name, batch_no=excluded.batch_no,
                        shop_name=excluded.shop_name, quantity=excluded.quantity,
                        cargo_status=excluded.cargo_status, channel=excluded.channel,
                        domestic_arrival=excluded.domestic_arrival,
                        foreign_arrival=excluded.foreign_arrival,
                        source=excluded.source, remote_record_id=excluded.remote_record_id,
                        updated_at=CURRENT_TIMESTAMP
                    """,
                    (
                        tracking_id, str(raw.get("product_name") or ""),
                        str(raw.get("batch_no") or ""), str(raw.get("shop_name") or ""),
                        int(_number(raw.get("quantity"))), str(raw.get("cargo_status") or ""),
                        str(raw.get("channel") or ""), str(raw.get("domestic_arrival") or ""),
                        foreign_arrival, notified, source,
                        str(raw.get("remote_record_id") or ""),
                    ),
                )
                if should_notify:
                    arrivals.append({**raw, "tracking_id": tracking_id})
        return arrivals

    def mark_shipment_notified(self, tracking_id: str, foreign_arrival: str) -> None:
        with self.connect() as db:
            db.execute(
                "UPDATE shipment_tracking SET notified_foreign_arrival=?, "
                "updated_at=CURRENT_TIMESTAMP WHERE tracking_id=?",
                (foreign_arrival, tracking_id),
            )

    def cross_border_fulfillment_orders(
        self, date_from: str, date_to: str,
    ) -> dict[str, dict[str, Any]]:
        """Classify cached Finance order postings by their Ozon delivery schema.

        Seller Analytics remains the source for sales amount and ordered units.
        Finance has no reliable item quantity here, so these values are labelled
        as fulfillment *orders* and are used for classification/reconciliation.
        """
        cache_key = (date_from, date_to, self._finance_revision)
        cached = self._fulfillment_order_cache.get(cache_key)
        if cached is not None:
            return cached
        if self._fulfillment_order_index_revision != self._finance_revision:
            with self.connect() as db:
                stored_rows = db.execute(
                    "SELECT operation_id, raw_json FROM finance_transactions "
                    "WHERE raw_json<>'' ORDER BY operation_id"
                ).fetchall()
            indexed: dict[tuple[str, str], tuple[str, str]] = {}
            for stored in stored_rows:
                try:
                    raw = json.loads(stored["raw_json"] or "{}")
                except (TypeError, json.JSONDecodeError):
                    continue
                if not isinstance(raw, dict):
                    continue
                raw_type = str(raw.get("type") or "").casefold()
                operation_type = str(raw.get("operation_type") or "")
                if raw_type != "orders" and not (
                    "deliveredtocustomer" in operation_type.casefold()
                    and _number(raw.get("accruals_for_sale")) > 0
                ):
                    continue
                posting = raw.get("posting") or {}
                if not isinstance(posting, dict):
                    posting = {}
                order_day = str(
                    posting.get("order_date") or raw.get("order_date")
                    or raw.get("operation_date") or ""
                )[:10]
                posting_number = str(
                    posting.get("posting_number") or raw.get("posting_number") or ""
                ).strip()
                if not posting_number or len(order_day) != 10:
                    continue
                schema = _normalize_delivery_schema(posting.get("delivery_schema"))
                item_skus = {
                    str(item.get("sku") or "").strip()
                    for item in (raw.get("items") or []) if isinstance(item, dict)
                }
                item_skus.discard("")
                for sku in item_skus:
                    key = (posting_number, sku)
                    previous = indexed.get(key)
                    # A posting can appear in several Finance operations. Count
                    # it once and prefer a real schema over an unknown value.
                    if previous is None or previous[1] == "UNKNOWN":
                        indexed[key] = (order_day, schema)
            self._fulfillment_order_index = [
                (sku, order_day, schema)
                for (_posting_number, sku), (order_day, schema) in indexed.items()
            ]
            self._fulfillment_order_index_revision = self._finance_revision

        result: dict[str, dict[str, Any]] = {}
        for sku, order_day, schema in self._fulfillment_order_index:
            if not (date_from <= order_day <= date_to):
                continue
            row = result.setdefault(sku, {
                "sku": sku, "fulfillment_orders": 0, "fbp_orders": 0,
                "rfbs_orders": 0, "whd_orders": 0, "fbo_orders": 0,
                "other_fulfillment_orders": 0, "fulfillment_schemes": set(),
            })
            row["fulfillment_orders"] += 1
            if schema == "FBP":
                row["fbp_orders"] += 1
            elif schema == "RFBS":
                row["rfbs_orders"] += 1
            elif schema == "WHD":
                row["whd_orders"] += 1
            elif schema == "FBO":
                row["fbo_orders"] += 1
            else:
                row["other_fulfillment_orders"] += 1
            row["fulfillment_schemes"].add(schema)
        for row in result.values():
            schemes = row["fulfillment_schemes"]
            row["fulfillment_schemes"] = ", ".join(sorted(schemes - {"UNKNOWN"})) \
                or "未识别"
        self._fulfillment_order_cache[cache_key] = result
        while len(self._fulfillment_order_cache) > 6:
            self._fulfillment_order_cache.pop(next(iter(self._fulfillment_order_cache)))
        return result

    def cross_border_profit_rows(
        self, date_from: str, date_to: str, query: str = "",
    ) -> list[dict[str, Any]]:
        """Return a near-real-time sold-SKU profitability estimate.

        Sales use Seller Analytics' order date instead of waiting for Finance
        settlement.  Only historically observed commission and acquiring rates
        are estimated as platform fees; Finance logistics is deliberately not
        mixed into this live view.  The user's cross-border freight formula is
        included in product cost accounting.  Shop advertising stays unallocated
        at SKU level and is deducted only in the period/weekly summary.
        """
        normalized = str(query or "").strip().casefold()
        rate = self.rub_per_cny()
        fulfillment = self.cross_border_fulfillment_orders(date_from, date_to)
        fee_profile = self.cross_border_realtime_fee_profile()
        shop_rates = fee_profile["shop"]
        result: list[dict[str, Any]] = []
        for source in self.monthly_profit_rows(date_from, date_to, limit=100000):
            orders = int(source.get("orders") or 0)
            if orders <= 0:
                continue
            searchable = f"{source.get('offer_id', '')} {source.get('sku', '')}".casefold()
            if normalized and normalized not in searchable:
                continue
            row = dict(source)
            revenue = float(row.get("revenue") or 0)
            selling_price_rub = revenue / orders if orders else 0.0
            selling_price_cny = selling_price_rub / rate if rate else 0.0
            weight = row.get("weight_kg")
            freight_cny = (
                cross_border_shipping_cny(selling_price_cny, float(weight))
                if weight is not None else None
            )
            purchase_cny = row.get("unit_cost_cny")
            purchase_rub_total = (
                float(purchase_cny) * rate * orders if purchase_cny is not None else None
            )
            freight_rub_total = (
                float(freight_cny) * rate * orders if freight_cny is not None else None
            )
            sku_rates = fee_profile["by_sku"].get(str(row.get("sku") or ""), {})
            commission_rate = sku_rates.get("commission_rate")
            commission_source = "SKU 历史已结算"
            if commission_rate is None:
                commission_rate = shop_rates.get("commission_rate")
                commission_source = "全店历史已结算"
            acquiring_rate = shop_rates.get("acquiring_rate")
            fee_rate_available = commission_rate is not None and acquiring_rate is not None
            estimated_commission = (
                -revenue * float(commission_rate) if commission_rate is not None else None
            )
            estimated_acquiring = (
                -revenue * float(acquiring_rate) if acquiring_rate is not None else None
            )
            estimated_platform_fees = (
                float(estimated_commission) + float(estimated_acquiring)
                if estimated_commission is not None and estimated_acquiring is not None
                else None
            )
            cost_complete = (
                purchase_rub_total is not None
                and freight_rub_total is not None
                and fee_rate_available
            )
            contribution = (
                revenue + float(estimated_platform_fees)
                - float(purchase_rub_total) - float(freight_rub_total)
                if cost_complete and estimated_platform_fees is not None else None
            )
            finance_base = float(row.get("platform_payout") or 0)
            revenue_cny = revenue / rate if rate else 0.0
            estimated_platform_fees_cny = (
                estimated_platform_fees / rate
                if estimated_platform_fees is not None and rate else None
            )
            purchase_cny_total = (
                purchase_rub_total / rate if purchase_rub_total is not None and rate else None
            )
            freight_cny_total = (
                freight_rub_total / rate if freight_rub_total is not None and rate else None
            )
            formula_cny = "数据不足"
            if contribution is not None and estimated_platform_fees_cny is not None:
                formula_cny = (
                    f"{revenue_cny:.2f} - {abs(estimated_platform_fees_cny):.2f}"
                    f" - {float(purchase_cny_total):.2f} - {float(freight_cny_total):.2f}"
                    f" = {contribution / rate:.2f}"
                )
            row.update({
                "selling_price_rub": selling_price_rub,
                "selling_price_cny": selling_price_cny,
                "revenue_cny": revenue_cny,
                "purchase_cost_cny": purchase_cny,
                "freight_unit_cny": freight_cny,
                "purchase_rub_total": purchase_rub_total,
                "freight_rub_total": freight_rub_total,
                "purchase_cny_total": purchase_cny_total,
                "freight_cny_total": freight_cny_total,
                "estimated_commission_rate": commission_rate,
                "estimated_acquiring_rate": acquiring_rate,
                "commission_rate_source": commission_source,
                "fee_rate_available": fee_rate_available,
                "estimated_commission": estimated_commission,
                "estimated_acquiring": estimated_acquiring,
                "estimated_platform_fees": estimated_platform_fees,
                "estimated_platform_fees_cny": estimated_platform_fees_cny,
                "finance_base": finance_base,
                "finance_base_cny": finance_base / rate if rate else 0.0,
                "contribution_before_shop_ads": contribution,
                "contribution_before_shop_ads_cny": (
                    contribution / rate if contribution is not None and rate else None
                ),
                "realtime_profit_formula_cny": formula_cny,
                "cross_border_cost_complete": cost_complete,
                "freight_in_realtime_profit": True,
            })
            row.update(fulfillment.get(str(row.get("sku") or ""), {
                "fulfillment_orders": 0, "fbp_orders": 0, "rfbs_orders": 0,
                "whd_orders": 0, "fbo_orders": 0,
                "other_fulfillment_orders": 0, "fulfillment_schemes": "无缓存分类",
            }))
            result.append(row)
        result.sort(key=lambda row: float(row.get("revenue") or 0), reverse=True)
        return result

    def cross_border_realtime_fee_profile(self) -> dict[str, Any]:
        """Estimate commission/acquiring rates from all cached settled history.

        A broad historical window avoids using the current unfinished Finance
        cycle.  Commission prefers the product's own history; acquiring uses the
        full shop because its rows often arrive before the matching sale accrual.
        """
        cached = self._realtime_fee_profile_cache
        if cached is not None and cached[0] == self._finance_revision:
            return cached[1]
        with self.connect() as db:
            stored_rows = db.execute(
                "SELECT operation_id, operation_date, raw_json FROM finance_transactions"
            ).fetchall()
        operation_days = [
            date.fromisoformat(str(row["operation_date"] or "")[:10])
            for row in stored_rows
            if re.fullmatch(r"\d{4}-\d{2}-\d{2}", str(row["operation_date"] or "")[:10])
        ]
        history_cutoff: date | None = None
        if operation_days and (max(operation_days) - min(operation_days)).days >= 14:
            history_cutoff = max(operation_days) - timedelta(days=7)
        shop = {"sales": 0.0, "commission": 0.0, "acquiring": 0.0}
        by_sku: dict[str, dict[str, float]] = {}
        for stored in stored_rows:
            stored_day_text = str(stored["operation_date"] or "")[:10]
            if history_cutoff is not None and re.fullmatch(
                r"\d{4}-\d{2}-\d{2}", stored_day_text
            ) and date.fromisoformat(stored_day_text) > history_cutoff:
                continue
            try:
                raw = json.loads(stored["raw_json"] or "{}")
            except (TypeError, json.JSONDecodeError):
                continue
            operation = _normalize_finance_operation(raw, str(stored["operation_id"]))
            sku = str(operation.get("sku") or "")
            sku_total = by_sku.setdefault(
                sku, {"sales": 0.0, "commission": 0.0, "acquiring": 0.0}
            ) if sku else None
            for component in operation.get("components", []):
                category = str(component.get("category") or "")
                value = float(component.get("amount") or 0)
                if category == "sales" and value > 0:
                    shop["sales"] += value
                    if sku_total is not None:
                        sku_total["sales"] += value
                elif category in {"commission", "acquiring"} and value < 0:
                    shop[category] += abs(value)
                    if sku_total is not None:
                        sku_total[category] += abs(value)

        def rate_for(total: dict[str, float], key: str, maximum: float) -> float | None:
            sales = float(total.get("sales") or 0)
            value = float(total.get(key) or 0)
            if sales <= 0 or value <= 0:
                return None
            candidate = value / sales
            return candidate if 0 < candidate <= maximum else None

        shop_result: dict[str, Any] = dict(shop)
        shop_result["commission_rate"] = rate_for(shop, "commission", 0.60)
        shop_result["acquiring_rate"] = rate_for(shop, "acquiring", 0.10)
        sku_result: dict[str, dict[str, Any]] = {}
        for sku, total in by_sku.items():
            sku_result[sku] = {
                **total,
                "commission_rate": rate_for(total, "commission", 0.60),
                "acquiring_rate": rate_for(total, "acquiring", 0.10),
            }
        result = {
            "shop": shop_result,
            "by_sku": sku_result,
            "history_through": history_cutoff.isoformat() if history_cutoff else "全部缓存",
        }
        self._realtime_fee_profile_cache = (self._finance_revision, result)
        return result

    def cross_border_period_summary(
        self, date_from: str, date_to: str,
        rows: list[dict[str, Any]] | None = None,
    ) -> dict[str, Any]:
        rows = self.cross_border_profit_rows(date_from, date_to) if rows is None else rows
        dashboard = self.dashboard_summary(date_from, date_to)
        monthly = self.monthly_profit_summary(date_from, date_to)
        fulfillment = self.cross_border_fulfillment_orders(date_from, date_to)
        fulfillment_rows = list(fulfillment.values())
        configured = [row for row in rows if row["cross_border_cost_complete"]]
        purchase = sum(
            float(row.get("purchase_rub_total") or 0)
            for row in rows if row.get("purchase_rub_total") is not None
        )
        freight = sum(
            float(row.get("freight_rub_total") or 0)
            for row in rows if row.get("freight_rub_total") is not None
        )
        estimated_platform_fees = sum(
            float(row.get("estimated_platform_fees") or 0)
            for row in rows if row.get("estimated_platform_fees") is not None
        )
        revenue = sum(float(row.get("revenue") or 0) for row in rows)
        ads = float(dashboard.get("spend") or 0)
        finance_available = bool(monthly.get("finance_available"))
        missing = sum(1 for row in rows if not row["cross_border_cost_complete"])
        profit = (
            revenue + estimated_platform_fees - ads - purchase - freight
            if rows and missing == 0 else None
        )
        rate = self.rub_per_cny()
        fee_profile_all = self.cross_border_realtime_fee_profile()
        fee_profile = fee_profile_all["shop"]
        return {
            "date_from": date_from, "date_to": date_to,
            "revenue": revenue,
            "orders": sum(int(row.get("orders") or 0) for row in rows),
            "fulfillment_orders": sum(
                int(row.get("fulfillment_orders") or 0) for row in fulfillment_rows
            ),
            "fbp_orders": sum(int(row.get("fbp_orders") or 0) for row in fulfillment_rows),
            "rfbs_orders": sum(int(row.get("rfbs_orders") or 0) for row in fulfillment_rows),
            "whd_orders": sum(int(row.get("whd_orders") or 0) for row in fulfillment_rows),
            "other_fulfillment_orders": sum(
                int(row.get("other_fulfillment_orders") or 0) for row in fulfillment_rows
            ),
            "ad_spend": ads,
            "estimated_platform_fees": estimated_platform_fees,
            "estimated_commission_rate": fee_profile.get("commission_rate"),
            "estimated_acquiring_rate": fee_profile.get("acquiring_rate"),
            "fee_history_through": fee_profile_all.get("history_through", "全部缓存"),
            "platform_payout": float(monthly.get("platform_payout") or 0),
            "settled_finance_net": float(monthly.get("platform_payout") or 0),
            "other_expenses": estimated_platform_fees,
            "purchase_cost": -purchase,
            "cross_border_freight": -freight,
            "reference_cross_border_freight": freight,
            "purchase_and_freight_cost": -(purchase + freight),
            "freight_in_realtime_profit": True,
            "profit": profit,
            "missing_cost_skus": missing,
            "sold_skus": len(rows),
            "finance_available": finance_available,
            "rub_per_cny": rate,
            "revenue_cny": revenue / rate if rate else 0.0,
            "ad_spend_cny": ads / rate if rate else 0.0,
            "platform_payout_cny": (
                float(monthly.get("platform_payout") or 0) / rate if rate else 0.0
            ),
            "settled_finance_net_cny": (
                float(monthly.get("platform_payout") or 0) / rate if rate else 0.0
            ),
            "estimated_platform_fees_cny": (
                estimated_platform_fees / rate if rate else 0.0
            ),
            "other_expenses_cny": estimated_platform_fees / rate if rate else 0.0,
            "purchase_cost_cny": -purchase / rate if rate else 0.0,
            "cross_border_freight_cny": -freight / rate if rate else 0.0,
            "reference_cross_border_freight_cny": (
                freight / rate if rate else 0.0
            ),
            "purchase_and_freight_cost_cny": -(purchase + freight) / rate if rate else 0.0,
            "profit_cny": profit / rate if profit is not None and rate else None,
        }

    def cross_border_weekly_summary(
        self, date_from: str, date_to: str,
    ) -> list[dict[str, Any]]:
        start = date.fromisoformat(date_from)
        end = date.fromisoformat(date_to)
        cursor = start
        result: list[dict[str, Any]] = []
        while cursor <= end:
            week_end = min(end, cursor + timedelta(days=6 - cursor.weekday()))
            result.append(self.cross_border_period_summary(
                cursor.isoformat(), week_end.isoformat(),
            ))
            cursor = week_end + timedelta(days=1)
        return result

    def monthly_finance_breakdown(self, date_from: str, date_to: str) -> list[dict[str, Any]]:
        snapshot = self._monthly_finance_snapshot(date_from, date_to)
        rows = list(snapshot["breakdown"].values())
        rows.sort(key=lambda row: abs(float(row["amount"])), reverse=True)
        return rows

    def monthly_finance_detail(
        self, sku: str, date_from: str, date_to: str,
    ) -> dict[str, Any]:
        snapshot = self._monthly_finance_snapshot(date_from, date_to)
        operations = [row for row in snapshot["operations"] if row.get("sku") == str(sku)]
        breakdown: dict[tuple[str, str, str], dict[str, Any]] = {}
        for operation in operations:
            for component in operation.get("components", []):
                key = (
                    str(component["category"]), str(component["label"]),
                    str(component.get("api_name") or ""),
                )
                item = breakdown.setdefault(key, {
                    "category": key[0], "category_label": _FINANCE_CATEGORY_LABELS.get(key[0], key[0]),
                    "name": key[1], "api_name": key[2], "rows_count": 0, "amount": 0.0,
                })
                item["rows_count"] += 1
                item["amount"] += float(component.get("amount") or 0)
        categories = sorted(
            breakdown.values(), key=lambda row: abs(float(row["amount"])), reverse=True
        )
        return {
            "sku": str(sku), "operations": operations, "categories": categories,
            "summary": snapshot["by_sku"].get(str(sku), {}),
        }

    def monthly_finance_store_detail(self, date_from: str, date_to: str) -> dict[str, Any]:
        """Expose all cached store-economy and unallocated data for audit."""
        snapshot = self._monthly_finance_snapshot(date_from, date_to)
        with self.connect() as db:
            cash_rows = db.execute(
                "SELECT raw_json FROM finance_cash_flows "
                "WHERE period_to>=? AND period_from<=? ORDER BY period_from, row_id",
                (date_from, date_to),
            ).fetchall()
            detail_rows = db.execute(
                "SELECT raw_json FROM finance_cash_flow_details "
                "WHERE period_to>=? AND period_from<=? ORDER BY period_from, row_id",
                (date_from, date_to),
            ).fetchall()

        def decoded(rows: Iterable[sqlite3.Row]) -> list[dict[str, Any]]:
            result: list[dict[str, Any]] = []
            for row in rows:
                try:
                    value = json.loads(row["raw_json"] or "{}")
                except (TypeError, json.JSONDecodeError):
                    continue
                if isinstance(value, dict):
                    result.append(value)
            return result

        breakdown = list(snapshot["breakdown"].values())
        breakdown.sort(key=lambda row: abs(float(row["amount"])), reverse=True)
        unallocated_operations = [
            row for row in snapshot["operations"] if not row.get("sku")
        ]
        unallocated_components: list[dict[str, Any]] = []
        for operation in unallocated_operations:
            components = operation.get("components") or [{
                "category": "other",
                "label": operation.get("operation_type_name") or "未拆分应计记录",
                "api_name": operation.get("operation_type") or "",
                "amount": operation.get("amount") or 0,
            }]
            for component in components:
                category = str(component.get("category") or "other")
                unallocated_components.append({
                    "operation_date": operation.get("operation_date") or "",
                    "operation_id": operation.get("operation_id") or "",
                    "operation_type_name": operation.get("operation_type_name") or "",
                    "posting_number": operation.get("posting_number") or "",
                    "item_skus": operation.get("item_skus") or "",
                    "allocation_status": operation.get("allocation_status") or "",
                    "category": category,
                    "category_label": _FINANCE_CATEGORY_LABELS.get(category, category),
                    "name": component.get("label") or "",
                    "api_name": component.get("api_name") or "",
                    "amount": float(component.get("amount") or 0),
                })
        unallocated_components.sort(
            key=lambda row: (str(row["operation_date"]), abs(float(row["amount"]))),
            reverse=True,
        )
        return {
            "summary": self.monthly_profit_summary(date_from, date_to),
            "breakdown": breakdown,
            "unallocated_operations": unallocated_operations,
            "unallocated_components": unallocated_components,
            "cash_flows": decoded(cash_rows),
            "cash_flow_details": decoded(detail_rows),
        }

    def monthly_unallocated_finance_rows(
        self, date_from: str, date_to: str,
    ) -> list[dict[str, Any]]:
        return self.monthly_finance_store_detail(date_from, date_to)["unallocated_components"]

    def finance_cash_flow_summary(self, date_from: str, date_to: str) -> dict[str, Any]:
        with self.connect() as db:
            rows = db.execute(
                "SELECT raw_json FROM finance_cash_flows "
                "WHERE period_to>=? AND period_from<=? ORDER BY period_from, row_id",
                (date_from, date_to),
            ).fetchall()
            detail_rows = db.execute(
                "SELECT raw_json FROM finance_cash_flow_details "
                "WHERE period_to>=? AND period_from<=? ORDER BY period_from, row_id",
                (date_from, date_to),
            ).fetchall()
        result: dict[str, Any] = {
            "available": bool(rows), "rows_count": len(rows), "currency_code": "",
            "orders_amount": 0.0, "returns_amount": 0.0, "commission_amount": 0.0,
            "item_delivery_and_return_amount": 0.0, "services_amount": 0.0,
            "sales_returns": 0.0, "platform_payout": 0.0,
            "other_adjustments": 0.0,
        }
        for stored in rows:
            try:
                raw = json.loads(stored["raw_json"] or "{}")
            except (TypeError, json.JSONDecodeError):
                continue
            result["currency_code"] = result["currency_code"] or str(raw.get("currency_code") or "")
            for key in (
                "orders_amount", "returns_amount", "commission_amount",
                "item_delivery_and_return_amount", "services_amount",
            ):
                result[key] += _number(raw.get(key))
        result["sales_returns"] = result["orders_amount"] + result["returns_amount"]
        result["platform_payout"] = sum(
            float(result[key]) for key in (
                "orders_amount", "returns_amount", "commission_amount",
                "item_delivery_and_return_amount", "services_amount",
            )
        )
        for stored in detail_rows:
            try:
                raw = json.loads(stored["raw_json"] or "{}")
            except (TypeError, json.JSONDecodeError):
                continue
            others = raw.get("others") or {}
            if isinstance(others, dict) and others.get("total") is not None:
                result["other_adjustments"] += _number(others.get("total"))
            elif isinstance(others, dict):
                result["other_adjustments"] += sum(
                    _number(item.get("price"))
                    for item in (others.get("items") or []) if isinstance(item, dict)
                )
        return result

    def _monthly_finance_snapshot(self, date_from: str, date_to: str) -> dict[str, Any]:
        cache_key = (date_from, date_to, self._finance_revision)
        cached = self._finance_snapshot_cache.get(cache_key)
        if cached is not None:
            return cached
        with self.connect() as db:
            stored_rows = db.execute(
                "SELECT operation_id, raw_json FROM finance_transactions "
                "WHERE substr(operation_date, 1, 10) BETWEEN ? AND ? "
                "ORDER BY operation_date, operation_id",
                (date_from, date_to),
            ).fetchall()
        result: dict[str, Any] = {key: 0.0 for key in _FINANCE_CATEGORY_LABELS}
        result.update({
            "platform_payout": 0.0, "operation_count": 0, "exact_sku_operations": 0,
            "unallocated_operations": 0, "unallocated_amount": 0.0,
            "by_sku": {}, "breakdown": {}, "operations": [],
        })
        for stored in stored_rows:
            try:
                raw = json.loads(stored["raw_json"] or "{}")
            except (TypeError, json.JSONDecodeError):
                continue
            operation = _normalize_finance_operation(raw, str(stored["operation_id"]))
            amount = float(operation["amount"])
            result["operation_count"] += 1
            result["platform_payout"] += amount
            sku = str(operation.get("sku") or "")
            sku_summary: dict[str, Any] | None = None
            if sku:
                result["exact_sku_operations"] += 1
                sku_summary = result["by_sku"].setdefault(
                    sku, {**{key: 0.0 for key in _FINANCE_CATEGORY_LABELS},
                          "finance_rows": 0, "platform_payout": 0.0}
                )
                sku_summary["finance_rows"] += 1
                sku_summary["platform_payout"] += amount
            else:
                result["unallocated_operations"] += 1
                result["unallocated_amount"] += amount
            for component in operation["components"]:
                category, value = str(component["category"]), float(component["amount"])
                result[category] = float(result.get(category) or 0) + value
                if sku_summary is not None:
                    sku_summary[category] = float(sku_summary.get(category) or 0) + value
                group_key = f"{category}|{component['label']}|{component.get('api_name', '')}"
                group = result["breakdown"].setdefault(group_key, {
                    "category": category,
                    "category_label": _FINANCE_CATEGORY_LABELS.get(category, category),
                    "name": component["label"], "api_name": component.get("api_name", ""),
                    "rows_count": 0, "amount": 0.0,
                })
                group["rows_count"] += 1
                group["amount"] += value
            result["operations"].append(operation)
        self._finance_snapshot_cache[cache_key] = result
        while len(self._finance_snapshot_cache) > 4:
            self._finance_snapshot_cache.pop(next(iter(self._finance_snapshot_cache)))
        return result

    def advertising_summary(self, date_from: str, date_to: str) -> dict[str, Any]:
        """Aggregate trustworthy campaign-total advertising data for a date range."""
        with self.connect() as db:
            row = db.execute(
                """
                SELECT COALESCE(SUM(impressions), 0) AS impressions,
                       COALESCE(SUM(clicks), 0) AS clicks,
                       COALESCE(SUM(cart_adds), 0) AS cart_adds,
                       COALESCE(SUM(orders), 0) AS orders,
                       COALESCE(SUM(revenue), 0) AS revenue,
                       COALESCE(SUM(spend), 0) AS spend,
                       MIN(day) AS available_from,
                       MAX(day) AS available_to,
                       COUNT(*) AS row_count
                FROM ad_daily
                WHERE day BETWEEN ? AND ? AND sku=''
                """,
                (date_from, date_to),
            ).fetchone()
        result = dict(row)
        impressions = float(result.get("impressions") or 0)
        clicks = float(result.get("clicks") or 0)
        cart_adds = float(result.get("cart_adds") or 0)
        orders = float(result.get("orders") or 0)
        spend = float(result.get("spend") or 0)
        revenue = float(result.get("revenue") or 0)
        result.update({
            "ctr": clicks * 100.0 / impressions if impressions else 0.0,
            "cpc": spend / clicks if clicks else 0.0,
            "cart_rate": cart_adds * 100.0 / clicks if clicks else 0.0,
            "conversion": orders * 100.0 / clicks if clicks else 0.0,
            "cpa": spend / orders if orders else 0.0,
            "roas": revenue / spend if spend else 0.0,
            "acos": spend * 100.0 / revenue if revenue else 0.0,
        })
        return result

    def top_campaigns(
        self,
        date_from: str,
        date_to: str,
        limit: int = 100,
        *,
        query: str = "",
        state: str = "",
        payment_type: str = "",
        min_spend: float | None = None,
        max_acos: float | None = None,
    ) -> list[dict[str, Any]]:
        conditions: list[str] = []
        parameters: list[Any] = [date_from, date_to]

        query = query.strip()
        state = state.strip()
        payment_type = payment_type.strip()
        if query:
            conditions.append(
                "(enriched.campaign_id LIKE ? COLLATE NOCASE "
                "OR enriched.campaign_name LIKE ? COLLATE NOCASE)"
            )
            pattern = f"%{query}%"
            parameters.extend((pattern, pattern))
        if state:
            conditions.append("enriched.state = ? COLLATE NOCASE")
            parameters.append(state)
        if payment_type:
            conditions.append("enriched.payment_type = ? COLLATE NOCASE")
            parameters.append(payment_type)
        if min_spend is not None:
            conditions.append("enriched.spend >= ?")
            parameters.append(float(min_spend))
        if max_acos is not None:
            # A campaign that spent money without attributed revenue has an
            # undefined (effectively infinite) ACOS and must not pass a cap.
            conditions.append(
                "((enriched.revenue > 0 AND enriched.spend * 100.0 / enriched.revenue <= ?) "
                "OR enriched.spend = 0)"
            )
            parameters.append(float(max_acos))

        where_clause = f"WHERE {' AND '.join(conditions)}" if conditions else ""
        parameters.append(limit)
        with self.connect() as db:
            rows = db.execute(
                f"""
                WITH aggregated AS (
                    SELECT campaign_id, MAX(campaign_name) campaign_name,
                           SUM(impressions) impressions, SUM(clicks) clicks,
                           SUM(cart_adds) cart_adds, SUM(orders) orders,
                           SUM(revenue) revenue, SUM(spend) spend
                    FROM ad_daily WHERE day BETWEEN ? AND ? AND sku=''
                    GROUP BY campaign_id
                ), enriched AS (
                    SELECT aggregated.campaign_id,
                           COALESCE(NULLIF(aggregated.campaign_name, ''),
                                    NULLIF(campaigns.name, ''),
                                    aggregated.campaign_id) campaign_name,
                           COALESCE(campaigns.state, '') state,
                           COALESCE(campaigns.payment_type, '') payment_type,
                           COALESCE(campaigns.budget, 0) budget,
                           aggregated.impressions, aggregated.clicks, aggregated.cart_adds,
                           aggregated.orders, aggregated.revenue, aggregated.spend
                    FROM aggregated
                    LEFT JOIN campaigns ON campaigns.campaign_id=aggregated.campaign_id
                )
                SELECT campaign_id, campaign_name, state, payment_type, budget,
                       impressions, clicks, cart_adds, orders, revenue, spend,
                       CASE WHEN impressions>0 THEN clicks*100.0/impressions ELSE 0 END ctr,
                       CASE WHEN clicks>0 THEN spend/clicks ELSE 0 END cpc,
                       CASE WHEN clicks>0 THEN cart_adds*100.0/clicks ELSE 0 END cart_rate,
                       CASE WHEN clicks>0 THEN orders*100.0/clicks ELSE 0 END conversion,
                       CASE WHEN orders>0 THEN spend/orders ELSE 0 END cpa,
                       CASE WHEN spend>0 THEN revenue/spend ELSE 0 END roas,
                       CASE WHEN revenue>0 THEN spend*100.0/revenue ELSE 0 END acos
                FROM enriched
                {where_clause}
                ORDER BY spend DESC LIMIT ?
                """,
                parameters,
            ).fetchall()
        return [dict(row) for row in rows]

    def campaign_filter_options(
        self,
        date_from: str | None = None,
        date_to: str | None = None,
    ) -> dict[str, list[str]]:
        """Return campaign metadata values available to the advertising filters.

        Supplying a complete date range limits the values to campaigns that have
        advertising rows in that range.  With no range, all saved campaigns are
        considered so callers that do not need date-aware options remain simple.
        """
        parameters: list[Any] = []
        date_condition = ""
        if date_from is not None and date_to is not None:
            date_condition = (
                "AND EXISTS (SELECT 1 FROM ad_daily "
                "WHERE ad_daily.campaign_id=campaigns.campaign_id "
                "AND ad_daily.day BETWEEN ? AND ? AND ad_daily.sku='')"
            )
            parameters.extend((date_from, date_to))
        with self.connect() as db:
            rows = db.execute(
                f"""
                SELECT state, payment_type FROM campaigns
                WHERE (state <> '' OR payment_type <> '') {date_condition}
                """,
                parameters,
            ).fetchall()
        return {
            "states": sorted({str(row["state"]) for row in rows if row["state"]}),
            "payment_types": sorted(
                {str(row["payment_type"]) for row in rows if row["payment_type"]}
            ),
        }

    def recent_logs(self, limit: int = 30) -> list[dict[str, Any]]:
        with self.connect() as db:
            rows = db.execute(
                "SELECT * FROM sync_logs ORDER BY id DESC LIMIT ?", (limit,)
            ).fetchall()
        return [dict(row) for row in rows]

    def sales_cache_info(self, date_from: str, date_to: str) -> dict[str, Any]:
        with self.connect() as db:
            row = db.execute(
                """
                SELECT COUNT(*) rows_count, MIN(day) min_day, MAX(day) max_day,
                       MAX(updated_at) updated_at
                FROM sales_daily
                WHERE source='api' AND day BETWEEN ? AND ?
                """,
                (date_from, date_to),
            ).fetchone()
        return dict(row)

    def sales_sync_state(self, date_from: str, date_to: str) -> dict[str, Any]:
        """Return cache coverage plus the persisted Seller Analytics freshness."""

        result = self.sales_cache_info(date_from, date_to)
        result.update(
            {
                "last_success_at": self.get_setting("seller_analytics_last_success_at"),
                "last_date_to": self.get_setting("seller_analytics_last_date_to"),
                "verified_through": self.get_setting("seller_analytics_verified_through"),
                "next_allowed_at": self.get_setting("seller_analytics_next_allowed_at"),
            }
        )
        return result

    def sales_coverage_info(self, date_from: str, date_to: str) -> dict[str, Any]:
        """Describe whether Seller Analytics covers the requested closed days.

        New successful syncs write explicit daily coverage markers.  Older
        databases did not have those markers, so their first/last cached sales
        dates remain a conservative fallback until the range is synced again.
        """

        requested_start = date.fromisoformat(date_from)
        requested_end = date.fromisoformat(date_to)
        latest_closed_day = date.today() - timedelta(days=1)
        effective_end = min(requested_end, latest_closed_day)
        if effective_end < requested_start:
            effective_end = requested_start
        effective_to = effective_end.isoformat()
        with self.connect() as db:
            coverage = db.execute(
                """
                SELECT COUNT(*) days_count, MIN(day) min_day, MAX(day) max_day
                FROM data_coverage
                WHERE metric='sales' AND day BETWEEN ? AND ?
                """,
                (date_from, effective_to),
            ).fetchone()
        cache = self.sales_cache_info(date_from, effective_to)
        expected_days = (effective_end - requested_start).days + 1
        marked_days = int(coverage["days_count"] or 0)
        if marked_days:
            complete = marked_days >= expected_days
            coverage_min = coverage["min_day"]
            coverage_max = coverage["max_day"]
            mode = "explicit"
        else:
            cache_min = str(cache.get("min_day") or "")
            cache_max = str(cache.get("max_day") or "")
            complete = bool(
                int(cache.get("rows_count") or 0)
                and cache_min and cache_max
                and cache_min <= date_from and cache_max >= effective_to
            )
            coverage_min = cache_min or None
            coverage_max = cache_max or None
            mode = "legacy_cache_range"
        return {
            "requested_from": date_from,
            "requested_to": date_to,
            "effective_to": effective_to,
            "expected_days": expected_days,
            "covered_days": marked_days,
            "min_day": coverage_min,
            "max_day": coverage_max,
            "rows_count": int(cache.get("rows_count") or 0),
            "complete": complete,
            "mode": mode,
        }

    def finance_sync_state(self, date_from: str, date_to: str) -> dict[str, Any]:
        with self.connect() as db:
            row = db.execute(
                """
                SELECT COUNT(*) rows_count,
                       MIN(substr(operation_date, 1, 10)) min_day,
                       MAX(substr(operation_date, 1, 10)) max_day
                FROM finance_transactions
                WHERE substr(operation_date, 1, 10) BETWEEN ? AND ?
                """,
                (date_from, date_to),
            ).fetchone()
        result = dict(row)
        result.update(
            {
                "last_success_at": self.get_setting("seller_finance_last_success_at"),
                "last_date_to": self.get_setting("seller_finance_last_date_to"),
                "verified_through": self.get_setting("seller_finance_verified_through"),
            }
        )
        return result

    def log_events(self, log_id: int, limit: int = 2000) -> list[dict[str, Any]]:
        with self.connect() as db:
            rows = db.execute(
                """
                SELECT id, log_id, event_at, level, stage, message, details_json
                FROM sync_log_events
                WHERE log_id=? ORDER BY id ASC LIMIT ?
                """,
                (log_id, limit),
            ).fetchall()
        result: list[dict[str, Any]] = []
        for row in rows:
            item = dict(row)
            try:
                item["details"] = json.loads(item.pop("details_json") or "{}")
            except json.JSONDecodeError:
                item["details"] = {"raw": item.pop("details_json", "")}
            result.append(item)
        return result

    def seed_demo_data(self, days: int = 45) -> tuple[int, int]:
        rng = random.Random(20260812)
        products = [
            ("100001", "无线蓝牙耳机 Pro", 3290),
            ("100002", "便携式充电宝 20000mAh", 2490),
            ("100003", "家用空气加湿器", 3790),
            ("100004", "人体工学办公椅", 12990),
            ("100005", "厨房收纳盒套装", 1890),
            ("100006", "智能手表 S8", 6990),
            ("100007", "儿童积木玩具 500片", 2890),
            ("100008", "车载吸尘器", 4590),
        ]
        campaigns = [
            ("90001", "核心爆款-搜索推广"),
            ("90002", "新品测试-商品推广"),
            ("90003", "家居品类-自动广告"),
            ("90004", "高客单商品-CPO"),
        ]
        end = date.today()
        start = end - timedelta(days=days - 1)
        sales_rows: list[dict[str, Any]] = []
        ad_rows: list[dict[str, Any]] = []

        for day_index in range(days):
            current = start + timedelta(days=day_index)
            weekday_factor = 1.18 if current.weekday() >= 5 else 1.0
            trend_factor = 0.90 + 0.20 * day_index / max(days - 1, 1)
            for product_index, (sku, name, price) in enumerate(products):
                base_orders = 7 + product_index * 1.3
                seasonal = 1 + 0.18 * math.sin((day_index + product_index) / 4)
                orders = max(0, round(base_orders * weekday_factor * trend_factor * seasonal + rng.uniform(-2, 3)))
                delivered = max(0, orders - rng.randint(0, max(1, orders // 7)))
                returns = rng.randint(0, 1 if delivered else 0)
                cancellations = max(0, orders - delivered - returns)
                views = max(orders, round(orders * rng.uniform(24, 38)))
                carts = max(orders, round(views * rng.uniform(0.075, 0.13)))
                sales_rows.append(
                    {
                        "day": current.isoformat(), "sku": sku, "product_name": name,
                        "revenue": delivered * price, "ordered_units": orders,
                        "delivered_units": delivered, "returns": returns,
                        "cancellations": cancellations, "views": views,
                        "cart_adds": carts, "source": "demo",
                    }
                )

            for campaign_index, (campaign_id, campaign_name) in enumerate(campaigns):
                impressions = round((8200 + campaign_index * 2300) * weekday_factor * rng.uniform(0.82, 1.18))
                ctr = 0.012 + campaign_index * 0.0025 + rng.uniform(-0.0015, 0.0015)
                clicks = max(1, round(impressions * ctr))
                ad_orders = max(0, round(clicks * rng.uniform(0.055, 0.105)))
                cpc = 18 + campaign_index * 5 + rng.uniform(-2, 4)
                spend = round(clicks * cpc, 2)
                avg_price = products[(campaign_index * 2) % len(products)][2]
                ad_revenue = round(ad_orders * avg_price * rng.uniform(0.88, 1.2), 2)
                ad_rows.append(
                    {
                        "day": current.isoformat(), "campaign_id": campaign_id,
                        "campaign_name": campaign_name, "sku": "",
                        "impressions": impressions, "clicks": clicks,
                        "cart_adds": round(clicks * rng.uniform(0.15, 0.28)),
                        "orders": ad_orders, "revenue": ad_revenue,
                        "spend": spend, "source": "demo",
                    }
                )

        self.upsert_campaigns(
            {
                "campaign_id": campaign_id, "name": name, "state": "RUNNING",
                "payment_type": "CPC" if index < 3 else "CPO", "budget": 50000 + index * 20000,
                "source": "demo",
            }
            for index, (campaign_id, name) in enumerate(campaigns)
        )
        self.upsert_products(
            {
                "sku": sku, "offer_id": f"DEMO-{index + 1:03d}",
                "product_id": sku, "name": name, "source": "demo",
            }
            for index, (sku, name, _price) in enumerate(products)
        )
        return self.upsert_sales(sales_rows), self.upsert_ads(ad_rows)

    def demo_counts(self) -> dict[str, int]:
        with self.connect() as db:
            return {
                "sales": int(db.execute("SELECT COUNT(*) FROM sales_daily WHERE source='demo'").fetchone()[0]),
                "ads": int(db.execute("SELECT COUNT(*) FROM ad_daily WHERE source='demo'").fetchone()[0]),
                "campaigns": int(db.execute("SELECT COUNT(*) FROM campaigns WHERE source='demo'").fetchone()[0]),
            }

    def clear_demo_data(self) -> dict[str, int]:
        counts = self.demo_counts()
        with self.connect() as db:
            db.execute("DELETE FROM sales_daily WHERE source='demo'")
            db.execute("DELETE FROM ad_daily WHERE source='demo'")
            db.execute("DELETE FROM campaigns WHERE source='demo'")
            db.execute("DELETE FROM products WHERE source='demo'")
        return counts

    def close_stale_running_logs(self, older_than_minutes: int = 30) -> int:
        cutoff = (datetime.now() - timedelta(minutes=older_than_minutes)).isoformat(timespec="seconds")
        with self.connect() as db:
            cursor = db.execute(
                """
                UPDATE sync_logs
                SET finished_at=?, status='failed', message='程序退出或任务中断，未正常完成。'
                WHERE status='running' AND started_at < ?
                """,
                (datetime.now().isoformat(timespec="seconds"), cutoff),
            )
            return int(cursor.rowcount)

    def export_csv_bundle(self, folder: Path, date_from: str, date_to: str) -> list[Path]:
        folder.mkdir(parents=True, exist_ok=True)
        exports = [
            ("sales.csv", self.top_products(date_from, date_to, 100000)),
            ("advertising.csv", self.top_campaigns(date_from, date_to, 100000)),
            ("daily_trend.csv", self.daily_trend(date_from, date_to)),
        ]
        paths: list[Path] = []
        for filename, rows in exports:
            path = folder / filename
            if rows:
                with path.open("w", newline="", encoding="utf-8-sig") as stream:
                    writer = csv.DictWriter(stream, fieldnames=list(rows[0].keys()))
                    writer.writeheader()
                    writer.writerows(rows)
            else:
                path.write_text("", encoding="utf-8-sig")
            paths.append(path)
        return paths


_FINANCE_CATEGORY_LABELS = {
    "sales": "销售收入",
    "returns": "退货退款",
    "commission": "Ozon 佣金",
    "advertising": "推广与广告",
    "delivery": "正向配送",
    "last_mile": "后段配送",
    "return_logistics": "退货/逆向物流",
    "acquiring": "收单与支付",
    "fbo_storage": "FBO、仓储与包装",
    "penalties_compensations": "赔偿、罚款与调整",
    "other": "其他应计项目",
}

_FINANCE_SERVICE_LABELS = {
    "MarketplaceServiceCostPerClick": "点击付费广告",
    "MarketplaceServicePromotionWithCostPerOrder": "按订单付费推广",
    "MarketplaceServiceExternalPromotion": "外部推广",
    "ItemAgentServiceStarsMembership": "星级商品/会员推广服务",
    "MarketplaceServiceItemCollectingFirstProductReviews": "首批评价服务",
    "MarketplaceElectronicServiceAcceleratedProductReviews": "加速评价服务",
    "OperationMarketplaceAcceleratedProductReviews": "加速评价服务",
    "OperationMarketplaceCostPerClick": "点击付费广告",
    "OperationMarketplaceExternalPromotion": "外部推广",
    "OperationPromotionWithCostPerOrder": "按订单付费推广",
    "MarketplaceServiceItemDeliveryToHandoverPlaceOzon": "配送至 Ozon 交接点",
    "MarketplaceServiceItemRedistributionLastMileCourier": "后段快递配送",
    "MarketplaceServiceItemDirectFlowLogistic": "正向物流",
    "MarketplaceServiceItemDirectFlowLogisticSum": "正向物流合计",
    "MarketplaceServiceItemReturnFlowLogistic": "退货逆向物流",
    "MarketplaceServiceItemRedistributionReturnsPVZ": "取货点退货处理",
    "MarketplaceRedistributionOfAcquiringOperation": "收单手续费",
    "MarketplaceServiceStorageItem": "仓储服务",
    "MarketplaceServiceItemTemporaryStorageRedistribution": "临时仓储",
    "MarketplaceServiceItemPackageRedistribution": "商品包装",
    "MarketplaceServiceItemPackageMaterialsProvision": "包装材料",
    "MarketplaceServiceItemAdditionalPackagingAtWarehouse": "仓库附加包装",
    "MarketplaceServiceItemSupplyInboundCargoSurplus": "入库货物盈余处理",
    "MarketplaceServiceItemSupplyInboundCargoShortage": "入库货物短缺处理",
    "MarketplaceServiceItemDisposalDetailed": "商品处置",
    "MarketplaceServiceItemCrossdocking": "越库服务",
    "MarketplaceServiceItemReplenishment": "补货服务",
    "OperationMarketplaceItemAdditionalPackagingAtWarehouse": "仓库附加包装",
    "OperationMarketplaceServiceStorage": "仓储服务",
    "OperationMarketplaceServiceSupplyInboundCargoShortage": "入库货物短缺处理",
    "OperationMarketplaceFlexiblePaymentSchedule": "灵活付款计划服务",
    "OperationMarketplaceServiceEarlyPaymentAccrual": "提前付款服务",
    "OperationSubscriptionPremiumLite": "Premium Lite 订阅",
    "AccrualConsigWriteOff": "寄售核销",
    "AccrualInternalClaim": "内部索赔应计",
    "MarketplaceSellerDecompensationItemByTypeDocOperation": "商品扣回/反向赔偿",
    "InsuranceServiceSellerItem": "商品保险",
}


def _finance_period_bounds(row: dict[str, Any]) -> tuple[str, str]:
    period = row.get("period") or {}
    if not isinstance(period, dict):
        period = {}
    begin = str(
        period.get("begin") or period.get("from") or period.get("since")
        or row.get("period_from") or row.get("date_from") or ""
    )[:10]
    end = str(
        period.get("end") or period.get("to") or period.get("until")
        or row.get("period_to") or row.get("date_to") or begin
    )[:10]
    return begin, end


def _finance_service_category(name: Any) -> str:
    normalized = str(name or "").casefold()
    if any(marker in normalized for marker in (
        "promotion", "costperclick", "externalpromotion", "review", "starsmembership",
        "advert", "реклам", "продвиж",
    )):
        return "advertising"
    if any(marker in normalized for marker in (
        "returnflow", "returnspvz", "reverse", "возврат",
    )):
        return "return_logistics"
    if any(marker in normalized for marker in ("lastmile", "handoverplace", "courier")):
        return "last_mile"
    if any(marker in normalized for marker in ("acquiring", "payment", "эквайр")):
        return "acquiring"
    if any(marker in normalized for marker in (
        "storage", "package", "supply", "disposal", "warehouse", "fbo",
        "crossdock", "replenishment",
    )):
        return "fbo_storage"
    if any(marker in normalized for marker in ("logistic", "delivery", "достав")):
        return "delivery"
    if any(marker in normalized for marker in (
        "penalt", "compensation", "shortage", "surplus", "insurance", "штраф", "компенсац",
    )):
        return "penalties_compensations"
    return "other"


def _finance_operation_category(operation_type: Any, operation_name: Any, amount: float) -> str:
    normalized = f"{operation_type or ''} {operation_name or ''}".casefold()
    if "return" in normalized or "возврат" in normalized:
        return "returns"
    service_category = _finance_service_category(normalized)
    if service_category != "other":
        return service_category
    if any(marker in normalized for marker in ("sale", "delivered", "order", "продаж")):
        return "sales" if amount >= 0 else "returns"
    if any(marker in normalized for marker in ("compensation", "penalty", "штраф", "компенсац")):
        return "penalties_compensations"
    return "other"


def _normalize_finance_operation(raw: dict[str, Any], fallback_id: str) -> dict[str, Any]:
    items = [item for item in (raw.get("items") or []) if isinstance(item, dict)]
    item_skus = sorted({str(item.get("sku") or "").strip() for item in items if item.get("sku")})
    sku = item_skus[0] if len(item_skus) == 1 else ""
    allocation_status = (
        "精确归属单个 SKU" if sku else
        ("多 SKU 应计，保留为未分摊" if item_skus else "店铺级应计，未分摊到 SKU")
    )
    operation_type = str(raw.get("operation_type") or "")
    operation_name = str(raw.get("operation_type_name") or operation_type)
    amount = _number(raw.get("amount"))
    accrual = _number(raw.get("accruals_for_sale"))
    commission = _number(raw.get("sale_commission"))
    components: list[dict[str, Any]] = []
    if abs(accrual) >= 0.005:
        category = _finance_operation_category(operation_type, operation_name, accrual)
        if category not in {"sales", "returns"}:
            category = "sales" if accrual >= 0 else "returns"
        components.append({
            "category": category,
            "label": "商品销售应计" if category == "sales" else "商品退货/退款",
            "api_name": "accruals_for_sale", "amount": accrual,
        })
    if abs(commission) >= 0.005:
        components.append({
            "category": "commission", "label": "Ozon 销售佣金",
            "api_name": "sale_commission", "amount": commission,
        })
    services = [service for service in (raw.get("services") or []) if isinstance(service, dict)]
    service_total = 0.0
    service_descriptions: list[str] = []
    for service in services:
        api_name = str(service.get("name") or "未命名服务")
        price = _number(service.get("price"))
        service_total += price
        label = _FINANCE_SERVICE_LABELS.get(api_name, api_name)
        service_descriptions.append(f"{label}: {price:.2f}")
        components.append({
            "category": _finance_service_category(api_name), "label": label,
            "api_name": api_name, "amount": price,
        })
    residual = amount - accrual - commission - service_total
    if abs(residual) >= 0.005:
        residual_label = _FINANCE_SERVICE_LABELS.get(operation_type, operation_name)
        components.append({
            "category": _finance_operation_category(operation_type, operation_name, residual),
            "label": residual_label or "未拆分应计差额",
            "api_name": operation_type, "amount": residual,
        })
    posting = raw.get("posting") or {}
    if not isinstance(posting, dict):
        posting = {}
    return {
        "operation_id": str(raw.get("operation_id") or fallback_id),
        "operation_date": str(raw.get("operation_date") or ""),
        "operation_type": operation_type,
        "operation_type_name": operation_name,
        "posting_number": str(posting.get("posting_number") or raw.get("posting_number") or ""),
        "sku": sku,
        "item_skus": ", ".join(item_skus),
        "allocation_status": allocation_status,
        "amount": amount,
        "accruals_for_sale": accrual,
        "sale_commission": commission,
        # These two fields are exposed for audit, but are not added again: Ozon
        # already repeats their amounts in services[].
        "delivery_charge": _number(raw.get("delivery_charge")),
        "return_delivery_charge": _number(raw.get("return_delivery_charge")),
        "services": service_descriptions,
        "services_text": "；".join(service_descriptions),
        "components": components,
        "raw": raw,
    }


def _number(value: Any) -> float:
    try:
        if value in (None, ""):
            return 0.0
        return float(str(value).replace(" ", "").replace(",", "."))
    except (TypeError, ValueError):
        return 0.0


def _normalize_delivery_schema(value: Any) -> str:
    normalized = re.sub(r"[^A-Z0-9]", "", str(value or "").upper())
    if normalized in {"RFBS", "REALFBS", "FBS"}:
        return "RFBS"
    if normalized in {"FBP", "FBO", "WHD"}:
        return normalized
    return normalized or "UNKNOWN"


def _integer(value: Any) -> int:
    return int(round(_number(value)))


def _campaign_identifier_match(campaign_name: Any, identifier: Any) -> bool:
    """Match a dedicated campaign without confusing variant-like identifiers."""
    name = str(campaign_name or "").strip()
    token = str(identifier or "").strip()
    if not name or not token:
        return False
    return re.search(
        rf"(?<![\w-]){re.escape(token)}(?![\w-])", name,
        flags=re.IGNORECASE,
    ) is not None


def _advertising_summary(
    rows: list[dict[str, Any]], seller_revenue: float, match_status: str,
) -> dict[str, Any]:
    impressions = sum(int(row.get("impressions") or 0) for row in rows)
    clicks = sum(int(row.get("clicks") or 0) for row in rows)
    cart_adds = sum(int(row.get("cart_adds") or 0) for row in rows)
    orders = sum(int(row.get("orders") or 0) for row in rows)
    revenue = sum(float(row.get("revenue") or 0) for row in rows)
    spend = sum(float(row.get("spend") or 0) for row in rows)
    return {
        "impressions": impressions,
        "clicks": clicks,
        "cart_adds": cart_adds,
        "orders": orders,
        "revenue": revenue,
        "spend": spend,
        "seller_revenue": seller_revenue,
        "ctr": clicks / impressions * 100 if impressions else None,
        "roas": revenue / spend if spend else None,
        "acos": spend / revenue * 100 if revenue else None,
        "tacos": spend / seller_revenue * 100 if seller_revenue else None,
        "match_status": match_status,
    }


def _sanitize_log_details(value: Any) -> Any:
    sensitive_markers = ("api_key", "apikey", "secret", "token", "authorization", "password")
    if isinstance(value, dict):
        result: dict[str, Any] = {}
        for key, item in value.items():
            normalized = str(key).lower().replace("-", "_")
            if any(marker in normalized for marker in sensitive_markers):
                result[str(key)] = "***"
            else:
                result[str(key)] = _sanitize_log_details(item)
        return result
    if isinstance(value, (list, tuple)):
        return [_sanitize_log_details(item) for item in value]
    return value


def _cluster_name_cn(value: Any) -> str:
    """Return a stable Chinese label for the macro clusters Ozon currently exposes."""
    text = str(value or "").strip()
    normalized = text.casefold()
    names = (
        ("莫斯科", "莫斯科"), ("喀山", "喀山"),
        ("москва", "莫斯科、莫斯科州及偏远地区"),
        ("санкт-петербург", "圣彼得堡及西北联邦区"),
        ("дальний восток", "远东"),
        ("воронеж", "沃罗涅日"), ("ростов", "罗斯托夫"),
        ("казань", "喀山"), ("уфа", "乌法"),
        ("ярославль", "雅罗斯拉夫尔"), ("саратов", "萨拉托夫"),
        ("новосибирск", "新西伯利亚"), ("екатеринбург", "叶卡捷琳堡"),
        ("пермь", "彼尔姆"), ("оренбург", "奥伦堡"),
        ("самара", "萨马拉"), ("краснодар", "克拉斯诺达尔"),
        ("омск", "鄂木斯克"), ("нижний новгород", "下诺夫哥罗德"),
        ("тюмень", "秋明"), ("красноярск", "克拉斯诺亚尔斯克"),
        ("калининград", "加里宁格勒"), ("беларус", "白俄罗斯"),
        ("астана", "阿斯塔纳"), ("алматы", "阿拉木图"),
        ("махачкала", "马哈奇卡拉"), ("невинномысск", "涅温诺梅斯克"),
        ("тверь", "特维尔"), ("хабаровск", "哈巴罗夫斯克"),
        ("иркутск", "伊尔库茨克"), ("волгоград", "伏尔加格勒"),
    )
    for marker, translated in names:
        if marker in normalized:
            return translated
    return "待补充翻译" if text else "—"
