from __future__ import annotations

import tempfile
import unittest
from pathlib import Path
from zipfile import ZipFile

from ozon_app.api import SellerApiClient
from ozon_app.database import Database
from ozon_app.exports import export_replenishment_xlsx
from ozon_app.services import AppService


class FakeHttp:
    def __init__(self) -> None:
        self.calls: list[tuple[str, dict]] = []

    def request(self, _method: str, url: str, **kwargs):
        body = kwargs.get("body") or {}
        self.calls.append((url, body))
        if url.endswith("/v1/analytics/stocks"):
            return {"items": [{"sku": sku} for sku in body["skus"]]}
        if url.endswith("/v2/cluster/list"):
            return {"result": [{"macrolocal_cluster_id": 7, "data": {}}]}
        if url.endswith("/v1/warehouse/fbo/seller/list"):
            return {"warehouses": [{"seller_warehouse_id": 9}]}
        return {}


class InventoryApiTests(unittest.TestCase):
    def test_inventory_batches_and_read_only_directories(self):
        http = FakeHttp()
        client = SellerApiClient("client", "key", http=http)
        rows = client.inventory_stocks([str(value) for value in range(1, 206)])
        self.assertEqual(len(rows), 205)
        stock_calls = [call for call in http.calls if call[0].endswith("/v1/analytics/stocks")]
        self.assertEqual([len(call[1]["skus"]) for call in stock_calls], [100, 100, 5])
        self.assertEqual(client.fbo_clusters()[0]["macrolocal_cluster_id"], 7)
        self.assertEqual(client.seller_supply_warehouses()[0]["seller_warehouse_id"], 9)


class InventoryDatabaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.database = Database(Path(self.temp.name) / "db.sqlite")
        self.database.upsert_products([{
            "sku": "1001", "offer_id": "OFFER-1", "product_id": "1001",
            "name": "测试商品", "source": "api",
        }])
        self.database.replace_ozon_clusters([
            {"macrolocal_cluster_id": 10, "data": {
                "macrolocal_cluster": {"name": "莫斯科", "country": {"name": "俄罗斯"}},
                "fulfillments": [],
            }},
            {"macrolocal_cluster_id": 20, "data": {
                "macrolocal_cluster": {"name": "喀山", "country": {"name": "俄罗斯"}},
                "fulfillments": [],
            }},
        ])
        self.database.replace_inventory_stock([{
            "sku": "1001", "offer_id": "OFFER-1", "product_name": "测试商品",
            "warehouse_id": 1, "warehouse_name": "仓库", "cluster_id": 1,
            "cluster_name": "莫斯科", "macrolocal_cluster_id": 10,
            "ads": 2, "ads_cluster": 5, "available_stock": 20,
            "valid_stock": 20, "requested_stock": 10, "transit_stock": 5,
            "idc_cluster": 12,
        }])

    def tearDown(self):
        self.temp.cleanup()

    def test_all_clusters_formulas_and_editable_plan(self):
        rows = self.database.inventory_cluster_plan("1001", 30)
        self.assertEqual([row["cluster_name"] for row in rows], ["莫斯科", "喀山"])
        moscow = rows[0]
        self.assertEqual(moscow["cluster_name_cn"], "莫斯科")
        self.assertEqual(moscow["monthly_estimated"], 150)
        self.assertEqual(moscow["recommended_qty"], 115)
        self.assertEqual(moscow["planned_qty"], 115)
        self.assertFalse(moscow["plan_saved"])
        self.assertEqual(moscow["sellable_days_after"], 30)
        self.assertIsNone(rows[1]["sellable_days_after"])
        self.database.save_replenishment_plan("1001", "10", 65, 30)
        edited = self.database.inventory_cluster_plan("1001", 30)[0]
        self.assertEqual(edited["planned_qty"], 65)
        self.assertTrue(edited["plan_saved"])
        self.assertEqual(edited["sellable_days_after"], 20)
        product = self.database.inventory_products(30)[0]
        self.assertEqual(product["planned_qty"], 65)
        self.assertEqual(product["planned_clusters"], 1)
        # Product cards use Ozon's SKU-level ADS field (2/day), while the
        # cluster detail uses ads_cluster (5/day).
        self.assertEqual(product["sellable_days_after"], 50)

    def test_single_sku_replace_preserves_other_cached_skus(self):
        self.database.replace_inventory_stock([{
            "sku": "2002", "warehouse_id": 2, "macrolocal_cluster_id": 20,
        }], replace_skus=["2002"])
        self.database.replace_inventory_stock([{
            "sku": "1001", "warehouse_id": 3, "macrolocal_cluster_id": 10,
        }], replace_skus=["1001"])
        with self.database.connect() as db:
            skus = {row[0] for row in db.execute("SELECT DISTINCT sku FROM inventory_stock")}
        self.assertEqual(skus, {"1001", "2002"})

    def test_duplicate_inventory_rows_are_merged_without_inflating_stock(self):
        count = self.database.replace_inventory_stock([
            {
                "sku": "3003", "warehouse_id": 77, "warehouse_name": "仓库 A",
                "macrolocal_cluster_id": 10, "available_stock": 5,
                "transit_stock": 2, "ads": 1.5,
            },
            {
                "sku": "3003", "warehouse_id": 77, "warehouse_name": "仓库 A",
                "macrolocal_cluster_id": 10, "available_stock": 8,
                "transit_stock": 1, "ads": 1.2,
            },
            {
                "sku": "3003", "warehouse_id": 0,
                "macrolocal_cluster_id": 20, "cluster_name": "喀山",
                "available_stock": 3,
            },
            {
                "sku": "3003", "warehouse_id": None,
                "macrolocal_cluster_id": 30, "cluster_name": "罗斯托夫",
                "available_stock": 4,
            },
        ])
        self.assertEqual(count, 3)
        metadata = self.database.last_inventory_write_metadata
        self.assertEqual(metadata["duplicate_rows_merged"], 1)
        self.assertEqual(metadata["fallback_warehouse_ids"], 2)
        with self.database.connect() as db:
            rows = db.execute(
                "SELECT warehouse_id, available_stock, transit_stock, ads "
                "FROM inventory_stock WHERE sku='3003' ORDER BY warehouse_id"
            ).fetchall()
        by_id = {row["warehouse_id"]: dict(row) for row in rows}
        self.assertEqual(by_id["77"]["available_stock"], 8)
        self.assertEqual(by_id["77"]["transit_stock"], 2)
        self.assertEqual(by_id["77"]["ads"], 1.5)
        self.assertIn("cluster:20", by_id)
        self.assertIn("cluster:30", by_id)

    def test_ai_single_product_context_is_not_top20(self):
        self.database.upsert_sales([{
            "day": "2026-08-01", "sku": "1001", "product_name": "测试商品",
            "revenue": 100, "ordered_units": 2,
        }])
        self.database.upsert_ads([
            {
                "day": "2026-08-01", "campaign_id": "A", "campaign_name": "全店活动",
                "sku": "", "impressions": 1000, "clicks": 100, "orders": 10,
                "revenue": 500, "spend": 50,
            },
            {
                "day": "2026-08-01", "campaign_id": "A", "campaign_name": "全店活动",
                "sku": "1001", "impressions": 200, "clicks": 20, "orders": 2,
                "revenue": 100, "spend": 20,
            },
            {
                "day": "2026-08-01", "campaign_id": "A", "campaign_name": "全店活动",
                "sku": "2002", "impressions": 9000, "clicks": 900, "orders": 90,
                "revenue": 9000, "spend": 999,
            },
        ])
        context = AppService(self.database).ai_data_context(
            "2026-08-01", "2026-08-31", product_sku="1001"
        )
        self.assertEqual(context["mode"], "single_product_plus_store_ad_efficiency")
        self.assertIsNone(context["data_scope"]["row_limit"])
        self.assertEqual(len(context["selected_product"]["sales_daily_all_rows"]), 1)
        self.assertEqual(len(context["selected_product"]["inventory_all_clusters"]), 2)
        self.assertEqual(
            context["store_overall_advertising_efficiency"]["summary"]["spend"], 50
        )
        product_ads = context["selected_product"]["advertising_daily_all_rows"]
        self.assertEqual(len(product_ads), 1)
        self.assertEqual(product_ads[0]["campaign_id"], "A")
        self.assertEqual(context["selected_product"]["advertising_summary"]["spend"], 20)
        self.assertEqual(
            context["selected_product"]["advertising_summary"]["match_status"],
            "matched_exact_sku",
        )
        self.assertNotIn("top_campaigns_by_spend", context)
        self.assertIn("其他 SKU", context["data_scope"]["excluded"])

    def test_ai_product_uses_only_clearly_matched_local_campaign_cache(self):
        self.database.upsert_sales([{
            "day": "2026-08-01", "sku": "1001", "product_name": "测试商品",
            "revenue": 1000, "ordered_units": 10,
        }])
        self.database.upsert_ads([
            {
                "day": "2026-08-01", "campaign_id": "MATCH",
                "campaign_name": "OFFER-1", "sku": "", "impressions": 100,
                "clicks": 10, "orders": 2, "revenue": 200, "spend": 20,
            },
            {
                "day": "2026-08-01", "campaign_id": "VARIANT",
                "campaign_name": "OFFER-10", "sku": "", "impressions": 500,
                "clicks": 50, "orders": 9, "revenue": 900, "spend": 90,
            },
            {
                "day": "2026-08-01", "campaign_id": "GENERIC",
                "campaign_name": "全店通用活动", "sku": "", "impressions": 1000,
                "clicks": 100, "orders": 20, "revenue": 2000, "spend": 200,
            },
        ])

        context = AppService(self.database).ai_data_context(
            "2026-08-01", "2026-08-31", product_sku="1001"
        )
        product = context["selected_product"]
        self.assertEqual(product["advertising_source"], "dedicated_campaign_name_local_cache")
        self.assertEqual(product["advertising_summary"]["spend"], 20)
        self.assertEqual(
            product["advertising_summary"]["match_status"],
            "matched_dedicated_campaign_name",
        )
        self.assertEqual(
            [row["campaign_id"] for row in product["advertising_daily_all_rows"]],
            ["MATCH"],
        )
        self.assertEqual(product["row_counts"]["advertising_matched_campaigns"], 1)
        self.assertIn("不会请求 Ozon", context["data_scope"]["source"])

    def test_excel_contains_editable_plan_and_formulas(self):
        rows = self.database.inventory_cluster_plan("1001", 30)
        path = Path(self.temp.name) / "plan.xlsx"
        export_replenishment_xlsx(
            path, rows, {"sku": "1001", "offer_id": "OFFER-1", "product_name": "测试商品"},
            30, "越库", {"city": "莫斯科", "address": "测试地址"},
        )
        with ZipFile(path) as archive:
            sheet = archive.read("xl/worksheets/sheet1.xml").decode("utf-8")
            workbook = archive.read("xl/workbook.xml").decode("utf-8")
        self.assertIn("集群中文名", sheet)
        self.assertIn("I6*30", sheet)
        self.assertIn("ROUNDUP(I6*K6-F6-G6-H6,0)", sheet)
        self.assertIn("(F6+G6+H6+M6)/I6", sheet)
        self.assertIn("fullCalcOnLoad", workbook)


if __name__ == "__main__":
    unittest.main()
