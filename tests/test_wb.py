from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from ozon_app.wb_api import WBApiClient
from ozon_app.wb_service import WBService, provisional_logistics_cny, weekly_card


class _FakeHttp:
    def request(self, method, url, **kwargs):
        if "/adv/v1/promotion/count" in url:
            return {"adverts": [{"advert_list": [{"advertId": 21414631}]}]}
        if "/adv/v3/fullstats" in url:
            return [
                {
                    "advertId": 21414631,
                    "days": [
                        {
                            "date": "2026-08-20T00:00:00",
                            "apps": [
                                {
                                    "nm": [
                                        {
                                            "nmId": 2654734207,
                                            "sum": 8544.74,
                                            "orders": 144,
                                            "sum_price": 263493,
                                            "views": 34353,
                                            "clicks": 1658,
                                        }
                                    ]
                                }
                            ],
                        }
                    ],
                }
            ]
        raise AssertionError(url)


class WBApiTests(unittest.TestCase):
    def test_product_ad_daily_is_exact_nm_and_not_double_counted(self):
        client = WBApiClient("token", http=_FakeHttp(), sleeper=lambda _: None)
        rows = client.product_ad_daily("2026-08-20", "2026-08-20")
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows[0]["nm_id"], 2654734207)
        self.assertEqual(rows[0]["campaign_id"], 21414631)
        self.assertAlmostEqual(rows[0]["spend_rub"], 8544.74)
        self.assertEqual(rows[0]["ad_orders"], 144)


class WBProfitTests(unittest.TestCase):
    def test_provisional_shipping_rules(self):
        self.assertAlmostEqual(provisional_logistics_cny("dongguan", 0.2, None, None, None), 13.6)
        self.assertAlmostEqual(provisional_logistics_cny("dongguan", 1.0, None, None, None), 51.0)
        self.assertAlmostEqual(provisional_logistics_cny("overseas", None, 20, 10, 10), 10.0)
        self.assertIsNone(provisional_logistics_cny("unknown", 1, 1, 1, 1))

    def test_daily_profit_and_weekly_card(self):
        # Windows can briefly retain the SQLite WAL handle after the assertion block.
        with tempfile.TemporaryDirectory(ignore_cleanup_errors=True) as folder:
            service = WBService(Path(folder))
            service.save_settings(
                {
                    "store_name": "WB 测试跨境店",
                    "rub_per_cny": "10",
                    "commission_percent": "10",
                }
            )
            service.db.replace_orders(
                [
                    {
                        "srid": "order-1",
                        "date": "2026-08-20T10:00:00",
                        "nmId": 1001,
                        "supplierArticle": "SKU-1",
                        "warehouseName": "东莞 DPG",
                        "finishedPrice": 1000,
                        "isCancel": False,
                    }
                ]
            )
            service.db.replace_ads(
                [
                    {
                        "date": "2026-08-20",
                        "nm_id": 1001,
                        "campaign_id": 88,
                        "spend_rub": 100,
                        "ad_orders": 1,
                    }
                ]
            )
            service.db.save_product_cost(
                {
                    "nm_id": 1001,
                    "article": "SKU-1",
                    "purchase_cost_cny": 20,
                    "weight_kg": 0.2,
                    "warehouse_mode": "auto",
                }
            )
            row = service.daily_rows("2026-08-20", "2026-08-20")[0]
            self.assertAlmostEqual(row["revenue_cny"], 100)
            self.assertAlmostEqual(row["ad_spend_cny"], 10)
            self.assertAlmostEqual(row["commission_cny"], 10)
            self.assertAlmostEqual(row["logistics_total_cny"], 13.6)
            self.assertAlmostEqual(row["profit_cny"], 46.4)

            summary = service.weekly_summary("2026-08-20", "2026-08-20")
            card = weekly_card(summary)
            self.assertEqual(summary["store_name"], "WB 测试跨境店")
            self.assertAlmostEqual(summary["profit_cny"], 46.4)
            self.assertEqual(card["header"]["title"]["content"], "WB 跨境店周利润")


if __name__ == "__main__":
    unittest.main()
