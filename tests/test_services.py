from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from ozon_app.database import Database
from ozon_app.api import OzonApiError
from ozon_app.secrets import unprotect
from datetime import date, datetime

from ozon_app.services import (
    AppService,
    Credentials,
    _incremental_sales_start,
    _sales_sync_plan,
)


class ServiceTests(unittest.TestCase):
    def test_new_ai_settings_keep_dataclass_defaults_and_encrypt_key(self) -> None:
        with TemporaryDirectory() as folder:
            database = Database(Path(folder) / "test.db")
            service = AppService(database)

            defaults = service.load_credentials()
            self.assertEqual(defaults.ai_base_url, "https://api.gpt.ge/v1")
            self.assertEqual(defaults.ai_model, "gpt-5.6-terra")
            self.assertEqual(defaults.ai_api_key, "")

            credentials = Credentials(ai_api_key="sk-local-secret")
            service.save_credentials(credentials)
            stored = database.get_setting("ai_api_key")
            self.assertNotEqual(stored, "sk-local-secret")
            self.assertEqual(unprotect(stored), "sk-local-secret")
            self.assertEqual(service.load_credentials().ai_api_key, "sk-local-secret")

    def test_ai_analysis_sends_only_bounded_aggregate_context(self) -> None:
        class FakeDatabase:
            def __init__(self) -> None:
                self.calls = []

            def dashboard_summary(self, date_from, date_to):
                self.calls.append(("summary", date_from, date_to))
                return {"revenue": 1000, "spend": 100, "ad_revenue": 500}

            def daily_trend(self, date_from, date_to):
                self.calls.append(("trend", date_from, date_to))
                return [{"day": date_from, "revenue": 1000}]

            def top_campaigns(self, date_from, date_to, limit=100):
                self.calls.append(("campaigns", date_from, date_to, limit))
                return [{"campaign_id": "1", "spend": 100}]

            def top_products(self, date_from, date_to, limit=100):
                self.calls.append(("products", date_from, date_to, limit))
                return [{"sku": "SKU-1", "revenue": 1000}]

        class FakeAIClient:
            def __init__(self) -> None:
                self.prompt = ""
                self.context = None

            def analyze(self, prompt, data_context=None):
                self.prompt = prompt
                self.context = data_context
                return "分析完成"

        database = FakeDatabase()
        client = FakeAIClient()
        service = AppService(database)  # type: ignore[arg-type]
        service.ai_client = lambda credentials=None: client  # type: ignore[method-assign]

        result = service.ai_analysis(
            "2026-08-01", "2026-08-13", "请给出投放建议", "advertising"
        )

        self.assertEqual(result, "分析完成")
        self.assertIn("ACOS = 广告花费 / 广告归因销售额", client.prompt)
        self.assertIn("TACOS = 广告花费 / Seller 总销售额", client.prompt)
        self.assertEqual(client.context["period"]["date_from"], "2026-08-01")
        self.assertEqual(client.context["period"]["date_to"], "2026-08-13")
        self.assertEqual(client.context["top_campaigns_by_spend"][0]["campaign_id"], "1")
        self.assertEqual(client.context["top_products_by_revenue"][0]["sku"], "SKU-1")
        self.assertEqual(
            database.calls,
            [
                ("summary", "2026-08-01", "2026-08-13"),
                ("trend", "2026-08-01", "2026-08-13"),
                ("campaigns", "2026-08-01", "2026-08-13", 20),
                ("products", "2026-08-01", "2026-08-13", 20),
            ],
        )

    def test_incremental_sales_start_refreshes_only_last_cached_day(self) -> None:
        start = _incremental_sales_start(
            "2026-07-15", "2026-08-13",
            {"rows_count": 2371, "min_day": "2026-07-15", "max_day": "2026-08-13"},
        )
        self.assertEqual(start, "2026-08-13")

    def test_monthly_sales_sync_fills_full_range_then_reuses_cache(self) -> None:
        class FakeSellerClient:
            def __init__(self) -> None:
                self.calls = 0
                self.last_analytics_metadata = {
                    "traffic_available": False,
                    "returned_metric_count": 2,
                }

            def analytics(self, date_from, date_to, progress=None, cancel_event=None):
                self.calls += 1
                self.asserted_range = (date_from, date_to)
                return [{
                    "day": "2026-07-01", "sku": "2578856473",
                    "product_name": "Tool Bag", "revenue": 1221690,
                    "ordered_units": 559, "source": "api",
                }]

        with TemporaryDirectory() as folder:
            database = Database(Path(folder) / "test.db")
            service = AppService(database)
            client = FakeSellerClient()
            service.seller_client = lambda credentials=None: client  # type: ignore[method-assign]

            first = service.sync_monthly_sales("2026-07-01", "2026-07-31")
            second = service.sync_monthly_sales("2026-07-01", "2026-07-31")

            self.assertEqual(first["status"], "success")
            self.assertEqual(first["ordered_products"], 559)
            self.assertEqual(first["order_amount"], 1221690)
            self.assertEqual(second["status"], "cached")
            self.assertEqual(client.calls, 1)
            self.assertEqual(client.asserted_range, ("2026-07-01", "2026-07-31"))
            summary = database.monthly_profit_summary("2026-07-01", "2026-07-31")
            self.assertEqual(summary["orders"], 559)
            self.assertEqual(summary["revenue"], 1221690)
            self.assertTrue(summary["seller_sales_complete"])

    def test_incremental_sales_start_keeps_full_range_for_incomplete_cache(self) -> None:
        start = _incremental_sales_start(
            "2026-07-15", "2026-08-13",
            {"rows_count": 400, "min_day": "2026-08-01", "max_day": "2026-08-13"},
        )
        self.assertEqual(start, "2026-07-15")

    def test_sales_sync_plan_skips_verified_history_and_recent_current_cache(self) -> None:
        historical = _sales_sync_plan(
            "2026-07-16", "2026-08-13",
            {
                "rows_count": 2300, "min_day": "2026-07-16", "max_day": "2026-08-13",
                "verified_through": "2026-08-13",
            },
            now=datetime(2026, 8, 14, 12, 0, 0),
        )
        self.assertEqual(historical["mode"], "verified_cache")
        self.assertIn("不请求 Ozon", historical["skip_reason"])

        current = _sales_sync_plan(
            "2026-07-16", "2026-08-14",
            {
                "rows_count": 2380, "min_day": "2026-07-16", "max_day": "2026-08-14",
                "last_success_at": "2026-08-14T11:50:00",
                "last_date_to": "2026-08-14",
            },
            now=datetime(2026, 8, 14, 12, 0, 0),
        )
        self.assertEqual(current["mode"], "fresh_current_cache")

    def test_sales_sync_plan_refreshes_incomplete_tail(self) -> None:
        plan = _sales_sync_plan(
            "2026-07-16", "2026-08-14",
            {
                "rows_count": 2299, "min_day": "2026-07-16", "max_day": "2026-08-13",
                "updated_at": "2026-08-13 09:45:47",
            },
            now=datetime(2026, 8, 14, 12, 0, 0),
        )
        self.assertEqual(plan["mode"], "minimal_tail")
        self.assertEqual(plan["api_date_from"], "2026-08-13")

    def test_seller_rate_limit_returns_existing_cache_without_finance_failure(self) -> None:
        class FakeSellerClient:
            def analytics(self, date_from, date_to, progress=None, cancel_event=None):
                raise OzonApiError("接口 /v1/analytics/data 仍被限频（HTTP 429）")

            def finance_transactions(self, date_from, date_to, progress=None, cancel_event=None):
                return []

        with TemporaryDirectory() as folder:
            database = Database(Path(folder) / "test.db")
            database.upsert_sales([{
                "day": "2026-08-10", "sku": "1", "product_name": "商品",
                "ordered_units": 2, "source": "api",
            }])
            service = AppService(database)
            service.seller_client = lambda credentials=None: FakeSellerClient()  # type: ignore[method-assign]
            results = service.sync_seller("2026-08-01", "2026-08-13")
            self.assertEqual(results[0].rows, 1)
            self.assertIn("本地缓存", results[0].message)
            self.assertEqual(results[1].source, "Seller Finance")
            analytics_log = next(
                row for row in database.recent_logs(5)
                if row["source"] == "Seller Analytics"
            )
            self.assertEqual(analytics_log["status"], "cached")

    def test_performance_sync_uses_fast_daily_endpoint_and_saves_events(self) -> None:
        class FakePerformanceClient:
            def __init__(self) -> None:
                self.daily_calls = 0

            def authenticate(self, progress=None, cancel_event=None):
                return "test-token"

            def campaigns(self, progress=None, cancel_event=None):
                return [{
                    "campaign_id": "12", "name": "广告 12", "state": "RUNNING",
                    "payment_type": "CPC", "budget": 100, "date_from": "2026-01-01",
                    "date_to": "", "source": "api",
                }]

            def statistics_daily(
                self, campaigns, date_from, date_to, progress=None,
                cancel_event=None, event=None,
            ):
                self.daily_calls += 1
                event("INFO", "快速统计", "已调用 daily/json", {"rows": 1})
                return [{
                    "day": "2026-08-01", "campaign_id": "12",
                    "campaign_name": "广告 12", "sku": "", "impressions": 100,
                    "clicks": 10, "cart_adds": 3, "orders": 2,
                    "revenue": 200, "spend": 20, "source": "api",
                }]

            def statistics(self, *args, **kwargs):
                raise AssertionError("不应调用旧异步 statistics/json 接口")

        with TemporaryDirectory() as folder:
            database = Database(Path(folder) / "test.db")
            service = AppService(database)
            client = FakePerformanceClient()
            service.performance_client = lambda credentials=None: client  # type: ignore[method-assign]
            live_events = []
            results = service.sync_performance(
                "2026-08-01", "2026-08-13",
                event_listener=lambda *values: live_events.append(values),
            )

            self.assertEqual(client.daily_calls, 1)
            self.assertEqual(results[0].rows, 1)
            self.assertIn("快速同步", results[0].message)
            self.assertGreaterEqual(len(live_events), 8)
            log = database.recent_logs(1)[0]
            self.assertEqual(log["status"], "success")
            self.assertEqual(log["rows_count"], 1)
            stages = [item["stage"] for item in database.log_events(log["id"])]
            self.assertIn("快速统计", stages)
            self.assertIn("写入数据库", stages)

    def test_product_ad_sync_caches_all_skus_and_reuses_exact_range(self) -> None:
        current_day = date.today().isoformat()

        class FakePerformanceClient:
            def __init__(self):
                self.product_statistics_calls = 0

            def authenticate(self, progress=None, cancel_event=None):
                return "test-token"

            def campaigns(self, progress=None, cancel_event=None):
                return [{
                    "campaign_id": "12", "name": "广告 12", "state": "RUNNING",
                    "payment_type": "CPC", "budget": 100, "source": "api",
                }]

            def product_statistics_by_sku(
                self, campaigns, date_from, date_to, progress=None,
                cancel_event=None, event=None,
            ):
                self.product_statistics_calls += 1
                return [
                    {
                        "day": current_day, "campaign_id": "12",
                        "campaign_name": "广告 12", "sku": "1001",
                        "impressions": 100, "clicks": 10, "orders": 2,
                        "revenue": 200, "spend": 20,
                    },
                    {
                        "day": current_day, "campaign_id": "12",
                        "campaign_name": "广告 12", "sku": "2002",
                        "impressions": 900, "clicks": 90, "orders": 20,
                        "revenue": 2000, "spend": 200,
                    },
                    {
                        "day": current_day, "campaign_id": "12",
                        "campaign_name": "广告 12", "sku": "",
                        "impressions": 1000, "clicks": 100, "orders": 22,
                        "revenue": 2200, "spend": 220,
                    },
                ]

        with TemporaryDirectory() as folder:
            database = Database(Path(folder) / "test.db")
            service = AppService(database)
            client = FakePerformanceClient()
            service.performance_client = (  # type: ignore[method-assign]
                lambda credentials=None: client
            )
            result = service.sync_performance_product_detail(
                current_day, current_day, "1001"
            )
            cached_result = service.sync_performance_product_detail(
                current_day, current_day, "2002"
            )

            self.assertEqual(result.rows, 1)
            self.assertEqual(cached_result.rows, 1)
            self.assertIn("未调用 Ozon 接口", cached_result.message)
            self.assertEqual(client.product_statistics_calls, 1)
            with database.connect() as db:
                rows = db.execute(
                    "SELECT sku, spend FROM ad_daily ORDER BY sku"
                ).fetchall()
            self.assertEqual(
                [(row["sku"], row["spend"]) for row in rows],
                [("1001", 20.0), ("2002", 200.0)],
            )
            cache = database.ad_detail_cache_info(
                current_day, current_day, "2002"
            )
            self.assertTrue(cache["complete"])
            self.assertEqual(cache["row_count"], 2)
            self.assertEqual(cache["selected_sku_rows"], 1)
            latest_log = database.recent_logs(1)[0]
            self.assertEqual(latest_log["status"], "cached")
            events = database.log_events(latest_log["id"])
            self.assertIn("使用缓存", [row["stage"] for row in events])


if __name__ == "__main__":
    unittest.main()
