from datetime import date, timedelta
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from ozon_app.database import Database, cross_border_shipping_cny
from ozon_app.secrets import protect, unprotect


class DatabaseTests(unittest.TestCase):
    def test_demo_data_and_summary(self) -> None:
        with TemporaryDirectory() as folder:
            database = Database(Path(folder) / "test.db")
            sales_count, ad_count = database.seed_demo_data(days=10)
            self.assertEqual(sales_count, 80)
            self.assertEqual(ad_count, 40)
            end = date.today()
            start = end - timedelta(days=9)
            summary = database.dashboard_summary(start.isoformat(), end.isoformat())
            self.assertGreater(summary["revenue"], 0)
            self.assertGreater(summary["spend"], 0)
            self.assertGreater(summary["orders"], 0)
            self.assertGreater(summary["roas"], 0)
            self.assertGreater(summary["acos"], 0)
            self.assertEqual(summary["tacos"], summary["drr"])
            self.assertEqual(len(database.daily_trend(start.isoformat(), end.isoformat())), 10)
            self.assertEqual(len(database.top_products(start.isoformat(), end.isoformat())), 8)
            self.assertEqual(len(database.top_campaigns(start.isoformat(), end.isoformat())), 4)
            counts = database.demo_counts()
            self.assertEqual(counts, {"sales": 80, "ads": 40, "campaigns": 4})
            removed = database.clear_demo_data()
            self.assertEqual(removed, counts)
            self.assertEqual(database.demo_counts(), {"sales": 0, "ads": 0, "campaigns": 0})
            empty = database.dashboard_summary(start.isoformat(), end.isoformat())
            self.assertEqual(empty["revenue"], 0)
            self.assertEqual(empty["acos"], 0)
            self.assertEqual(empty["tacos"], 0)

    def test_summary_metrics_and_daily_sales_units(self) -> None:
        with TemporaryDirectory() as folder:
            database = Database(Path(folder) / "test.db")
            database.upsert_sales(
                [
                    {
                        "day": "2026-08-01", "sku": "SKU-1", "revenue": 1000,
                        "ordered_units": 12, "delivered_units": 10, "returns": 2,
                        "cancellations": 1, "views": 100, "cart_adds": 20,
                    }
                ]
            )
            database.upsert_ads(
                [
                    {
                        "day": "2026-08-01", "campaign_id": "C-1",
                        "spend": 100, "revenue": 400, "orders": 4,
                    }
                ]
            )

            summary = database.dashboard_summary("2026-08-01", "2026-08-01")
            self.assertAlmostEqual(summary["acos"], 25.0)
            self.assertAlmostEqual(summary["tacos"], 10.0)
            self.assertAlmostEqual(summary["drr"], 10.0)

            trend = database.daily_trend("2026-08-01", "2026-08-01")
            self.assertEqual(len(trend), 1)
            self.assertEqual(trend[0]["orders"], 12)
            self.assertEqual(trend[0]["delivered"], 10)
            self.assertEqual(trend[0]["returns"], 2)
            self.assertEqual(trend[0]["cancellations"], 1)
            self.assertEqual(trend[0]["ad_orders"], 4)

    def test_finance_returns_use_original_order_date(self) -> None:
        with TemporaryDirectory() as folder:
            database = Database(Path(folder) / "test.db")
            database.upsert_finance(
                [
                    {
                        "operation_id": "RET-1",
                        "operation_type": "ClientReturnAgentOperation",
                        "operation_date": "2026-08-10 00:00:00",
                        "posting": {
                            "posting_number": "P-1",
                            "order_date": "2026-08-01 12:00:00",
                        },
                        "items": [{"sku": 123, "name": "商品 A"}],
                    },
                    {
                        "operation_id": "LOG-1",
                        "operation_type": "OperationItemReturn",
                        "operation_date": "2026-08-02 00:00:00",
                        "posting": {
                            "posting_number": "P-2",
                            "order_date": "2026-08-01 12:00:00",
                        },
                        "items": [{"sku": 123, "name": "商品 A"}],
                    },
                ]
            )

            rows = database.finance_customer_returns("2026-08-01", "2026-08-01")
            self.assertEqual(len(rows), 1)
            self.assertEqual(rows[0]["day"], "2026-08-01")
            self.assertEqual(rows[0]["sku"], "123")
            self.assertEqual(rows[0]["source"], "finance")

    def test_product_offer_and_real_fulfillment_events_override_missing_analytics(self) -> None:
        with TemporaryDirectory() as folder:
            database = Database(Path(folder) / "test.db")
            database.upsert_sales([{
                "day": "2026-08-01", "sku": "123", "product_name": "商品 A",
                "revenue": 1000, "ordered_units": 10,
            }])
            database.upsert_products([{
                "sku": "123", "offer_id": "SELLER-A", "product_id": "99",
            }])
            before = database.dashboard_summary("2026-08-01", "2026-08-01")
            self.assertFalse(before["returns_available"])
            self.assertFalse(before["cancellations_available"])
            self.assertFalse(before["traffic_available"])

            database.replace_returns("2026-08-01", "2026-08-01", [{
                "return_id": "R-1", "day": "2026-08-01", "sku": "123", "quantity": 2,
            }])
            database.replace_cancellations("2026-08-01", "2026-08-01", [{
                "event_id": "FBS:P-1:123", "day": "2026-08-01", "sku": "123",
                "quantity": 1, "scheme": "FBS",
            }])
            database.replace_deliveries("2026-08-01", "2026-08-01", [{
                "event_id": "FBO:P-2:123", "day": "2026-08-01", "sku": "123",
                "quantity": 8, "scheme": "FBO",
            }])
            summary = database.dashboard_summary("2026-08-01", "2026-08-01")
            self.assertEqual(summary["returns"], 2)
            self.assertEqual(summary["cancellations"], 1)
            self.assertEqual(summary["delivered"], 8)
            self.assertAlmostEqual(summary["cancellation_rate"], 10.0)
            products = database.top_products("2026-08-01", "2026-08-01")
            self.assertEqual(products[0]["offer_id"], "SELLER-A")
            self.assertEqual(products[0]["returns"], 2)
            self.assertEqual(products[0]["cancellations"], 1)
            self.assertEqual(products[0]["delivered"], 8)
            self.assertAlmostEqual(products[0]["cancellation_rate"], 10.0)

    def test_advertising_summary_and_funnel_metrics(self) -> None:
        with TemporaryDirectory() as folder:
            database = Database(Path(folder) / "test.db")
            database.upsert_ads([
                {
                    "day": "2026-08-01", "campaign_id": "C-FUNNEL",
                    "campaign_name": "Funnel Campaign", "impressions": 1000,
                    "clicks": 100, "cart_adds": 25, "orders": 10,
                    "spend": 200, "revenue": 800,
                },
            ])
            summary = database.advertising_summary("2026-08-01", "2026-08-01")
            self.assertEqual(summary["impressions"], 1000)
            self.assertEqual(summary["clicks"], 100)
            self.assertEqual(summary["cart_adds"], 25)
            self.assertEqual(summary["orders"], 10)
            self.assertAlmostEqual(summary["ctr"], 10.0)
            self.assertAlmostEqual(summary["cpc"], 2.0)
            self.assertAlmostEqual(summary["cart_rate"], 25.0)
            self.assertAlmostEqual(summary["conversion"], 10.0)
            self.assertAlmostEqual(summary["cpa"], 20.0)
            self.assertAlmostEqual(summary["roas"], 4.0)
            self.assertAlmostEqual(summary["acos"], 25.0)

            rows = database.top_campaigns("2026-08-01", "2026-08-01")
            self.assertEqual(len(rows), 1)
            self.assertEqual(rows[0]["cart_adds"], 25)
            self.assertAlmostEqual(rows[0]["cpc"], 2.0)
            self.assertAlmostEqual(rows[0]["conversion"], 10.0)
            self.assertAlmostEqual(rows[0]["cpa"], 20.0)

    def test_campaign_filters_and_options(self) -> None:
        with TemporaryDirectory() as folder:
            database = Database(Path(folder) / "test.db")
            database.upsert_campaigns(
                [
                    {
                        "campaign_id": "C-SEARCH", "name": "Search Alpha",
                        "state": "RUNNING", "payment_type": "CPC", "budget": 500,
                    },
                    {
                        "campaign_id": "C-BETA", "name": "Beta Campaign",
                        "state": "PAUSED", "payment_type": "CPO", "budget": 800,
                    },
                    {
                        "campaign_id": "C-OLD", "name": "Old Campaign",
                        "state": "ARCHIVED", "payment_type": "CPC", "budget": 100,
                    },
                ]
            )
            database.upsert_ads(
                [
                    {
                        "day": "2026-08-01", "campaign_id": "C-SEARCH",
                        "campaign_name": "Search Alpha", "spend": 120,
                        "revenue": 600, "impressions": 1000, "clicks": 100,
                    },
                    {
                        "day": "2026-08-01", "campaign_id": "C-BETA",
                        "campaign_name": "Beta Campaign", "spend": 50,
                        "revenue": 100, "impressions": 500, "clicks": 25,
                    },
                    {
                        "day": "2026-08-01", "campaign_id": "C-NOREVENUE",
                        "campaign_name": "No Revenue", "spend": 200,
                        "revenue": 0,
                    },
                    {
                        "day": "2026-07-01", "campaign_id": "C-OLD",
                        "campaign_name": "Old Campaign", "spend": 10,
                        "revenue": 100,
                    },
                ]
            )

            # The original positional limit call remains supported.
            self.assertEqual(len(database.top_campaigns("2026-08-01", "2026-08-01", 2)), 2)

            query_rows = database.top_campaigns(
                "2026-08-01", "2026-08-01", query="alpha"
            )
            self.assertEqual([row["campaign_id"] for row in query_rows], ["C-SEARCH"])
            self.assertEqual(query_rows[0]["state"], "RUNNING")
            self.assertEqual(query_rows[0]["payment_type"], "CPC")

            metadata_rows = database.top_campaigns(
                "2026-08-01", "2026-08-01", state="PAUSED", payment_type="CPO"
            )
            self.assertEqual([row["campaign_id"] for row in metadata_rows], ["C-BETA"])

            spend_rows = database.top_campaigns(
                "2026-08-01", "2026-08-01", min_spend=100
            )
            self.assertEqual(
                {row["campaign_id"] for row in spend_rows},
                {"C-SEARCH", "C-NOREVENUE"},
            )

            acos_rows = database.top_campaigns(
                "2026-08-01", "2026-08-01", max_acos=25
            )
            self.assertEqual([row["campaign_id"] for row in acos_rows], ["C-SEARCH"])

            all_options = database.campaign_filter_options()
            self.assertEqual(all_options["states"], ["ARCHIVED", "PAUSED", "RUNNING"])
            self.assertEqual(all_options["payment_types"], ["CPC", "CPO"])
            ranged_options = database.campaign_filter_options("2026-08-01", "2026-08-01")
            self.assertEqual(ranged_options["states"], ["PAUSED", "RUNNING"])
            self.assertEqual(ranged_options["payment_types"], ["CPC", "CPO"])

    def test_settings_roundtrip(self) -> None:
        with TemporaryDirectory() as folder:
            database = Database(Path(folder) / "test.db")
            database.save_settings({"a": "1", "b": "中文"})
            self.assertEqual(database.get_setting("a"), "1")
            self.assertEqual(database.get_setting("b"), "中文")

    def test_weekly_report_matches_daily_and_wow_formulas(self) -> None:
        with TemporaryDirectory() as folder:
            database = Database(Path(folder) / "test.db")
            sales = []
            ads = []
            for index in range(14):
                day = date(2026, 7, 27) + timedelta(days=index)
                current = index >= 7
                sales.append({
                    "day": day.isoformat(), "sku": "SKU-1",
                    "revenue": 200 if current else 100,
                    "ordered_units": 20 if current else 10,
                })
                ads.append({
                    "day": day.isoformat(), "campaign_id": "C-1",
                    "spend": 20 if current else 10,
                    "revenue": 100 if current else 50,
                    "orders": 10 if current else 5,
                })
            database.upsert_sales(sales)
            database.upsert_ads(ads)

            report = database.weekly_report("2026-08-09")
            self.assertEqual(report["current_start"], "2026-08-03")
            self.assertEqual(report["current_end"], "2026-08-09")
            self.assertEqual(len(report["daily"]), 7)
            self.assertEqual(report["current"]["revenue"], 1400)
            self.assertAlmostEqual(report["current"]["ad_order_share"], 50.0)
            self.assertAlmostEqual(report["current"]["acots"], 10.0)
            self.assertAlmostEqual(report["comparison"]["revenue"], 100.0)

    def test_monthly_profit_waits_for_costs_then_calculates(self) -> None:
        with TemporaryDirectory() as folder:
            database = Database(Path(folder) / "test.db")
            database.upsert_sales([{
                "day": "2026-08-03", "sku": "123", "product_name": "商品 A",
                "revenue": 1000, "ordered_units": 10,
            }])
            database.upsert_products([{"sku": "123", "offer_id": "SELLER-A"}])
            database.upsert_finance([{
                "operation_id": "F-1", "operation_date": "2026-08-04 10:00:00",
                "operation_type": "OperationAgentDeliveredToCustomer",
                "amount": 700, "accruals_for_sale": 1000,
                "sale_commission": -200, "items": [{"sku": 123}],
            }])
            before = database.monthly_profit_summary("2026-08-01", "2026-08-31")
            self.assertIsNone(before["profit"])
            self.assertEqual(before["missing_cost_skus"], 1)

            database.save_product_cost("123", 20, 5)
            rows = database.monthly_profit_rows("2026-08-01", "2026-08-31")
            self.assertEqual(rows[0]["offer_id"], "SELLER-A")
            self.assertEqual(rows[0]["product_cost_total"], -200)
            self.assertEqual(rows[0]["first_mile_total"], -50)
            self.assertEqual(rows[0]["profit"], 450)
            after = database.monthly_profit_summary("2026-08-01", "2026-08-31")
            # The unexplained -100 residual in this synthetic transaction is
            # normalized as a return, so the auditable Finance identity is
            # 900 sales/returns - 200 fees = 700 net accrual.
            self.assertEqual(after["sales_returns"], 900)
            self.assertEqual(after["accrual_fees"], -200)
            self.assertEqual(after["product_cost"], -200)
            self.assertEqual(after["costed_units"], 10)
            self.assertEqual(after["configured_product_cost_skus"], 1)
            self.assertEqual(after["missing_product_cost_skus"], 0)
            self.assertEqual(after["profit"], 450)
            self.assertAlmostEqual(after["profit_rate"], 45.0)

    def test_product_family_costs_can_be_fuzzy_found_and_saved_together(self) -> None:
        with TemporaryDirectory() as folder:
            database = Database(Path(folder) / "test.db")
            database.upsert_products([
                {"sku": "101", "offer_id": "GJYB001-YELLOW", "name": "Yellow"},
                {"sku": "102", "offer_id": "GJYB001-RED", "name": "Red"},
                {"sku": "103", "offer_id": "XSB002", "name": "Other"},
            ])

            fuzzy = database.find_product_cost_targets("jyb001")
            family = database.find_product_cost_targets("GJYB001", prefix=True)
            self.assertEqual({row["sku"] for row in fuzzy}, {"101", "102"})
            self.assertEqual({row["sku"] for row in family}, {"101", "102"})

            database.save_product_cost("101", 10, 2, "保留单品备注")
            saved = database.save_product_costs(
                (row["sku"] for row in family), 364, 93.04
            )
            self.assertEqual(saved, 2)
            costs = {
                row["sku"]: row for row in database.product_sync_rows()
                if row["sku"] in {"101", "102", "103"}
            }
            self.assertEqual(costs["101"]["unit_cost"], 364)
            self.assertEqual(costs["102"]["first_mile_cost"], 93.04)
            self.assertEqual(costs["101"]["note"], "保留单品备注")
            self.assertIsNone(costs["103"]["unit_cost"])

            database.save_product_costs_mixed(
                ("101", "102"), 28, 93.04, length_cm=34, width_cm=30,
                height_cm=4, weight_kg=0.535,
            )
            synchronized = {
                row["sku"]: row for row in database.product_sync_rows()
                if row["sku"] in {"101", "102"}
            }
            for row in synchronized.values():
                self.assertEqual(row["length_cm"], 34)
                self.assertEqual(row["width_cm"], 30)
                self.assertEqual(row["height_cm"], 4)
                self.assertEqual(row["weight_kg"], 0.535)

    def test_product_series_supports_day_week_and_month(self) -> None:
        with TemporaryDirectory() as folder:
            database = Database(Path(folder) / "test.db")
            database.upsert_products([
                {"sku": "101", "offer_id": "WZW001-YELLOW-3"},
                {"sku": "102", "offer_id": "WZW001-WHITE-6"},
            ])
            database.upsert_sales([
                {"day": "2026-08-03", "sku": "101", "revenue": 100, "ordered_units": 2},
                {"day": "2026-08-04", "sku": "102", "revenue": 200, "ordered_units": 3},
            ])

            daily = database.product_series_rows(
                "2026-08-01", "2026-08-31", period="day"
            )
            weekly = database.product_series_rows(
                "2026-08-01", "2026-08-31", period="week"
            )
            monthly = database.product_series_rows(
                "2026-08-01", "2026-08-31", period="month"
            )

            self.assertEqual(len(daily), 2)
            self.assertEqual(weekly[0]["series"], "WZW001")
            self.assertEqual(weekly[0]["units"], 5)
            self.assertEqual(weekly[0]["sku_count"], 2)
            self.assertEqual(monthly[0]["period"], "2026-08")
            self.assertEqual(monthly[0]["revenue"], 300)

    def test_cny_costs_follow_shop_exchange_rate_without_rewriting_source(self) -> None:
        with TemporaryDirectory() as folder:
            database = Database(Path(folder) / "test.db")
            database.upsert_sales([{
                "day": "2026-08-01", "sku": "101", "revenue": 1000,
                "ordered_units": 2,
            }])
            database.save_rub_per_cny(10)
            database.save_product_cost_cny(
                "101", 20, 3, length_cm=30, width_cm=20,
                height_cm=10, weight_kg=0.4,
            )
            first = database.monthly_profit_rows("2026-08-01", "2026-08-31")[0]
            self.assertEqual(first["unit_cost_cny"], 20)
            self.assertEqual(first["unit_cost"], 200)
            database.save_rub_per_cny(12)
            second = database.monthly_profit_rows("2026-08-01", "2026-08-31")[0]
            self.assertEqual(second["unit_cost_cny"], 20)
            self.assertEqual(second["unit_cost"], 240)
            self.assertEqual(second["weight_kg"], 0.4)

    def test_cross_border_shipping_formula_boundaries(self) -> None:
        self.assertAlmostEqual(cross_border_shipping_cny(100, 0.4), 14.61)
        self.assertAlmostEqual(cross_border_shipping_cny(200, 1.0), 46.07)
        self.assertAlmostEqual(cross_border_shipping_cny(700, 2.0), 64.03)
        self.assertAlmostEqual(cross_border_shipping_cny(100, 0.5), 38.76)
        self.assertAlmostEqual(cross_border_shipping_cny(200, 2.0), 96.64)
        self.assertAlmostEqual(cross_border_shipping_cny(700, 5.0), 193.00)
        self.assertAlmostEqual(cross_border_shipping_cny(10, 0.01), 6.18)
        self.assertIsNone(cross_border_shipping_cny(30000, 1))

    def test_cross_border_report_contains_only_sold_products(self) -> None:
        with TemporaryDirectory() as folder:
            database = Database(Path(folder) / "test.db")
            database.upsert_products([
                {"sku": "101", "offer_id": "SOLD"},
                {"sku": "102", "offer_id": "NO-SALE"},
            ])
            database.upsert_sales([{
                "day": "2026-08-01", "sku": "101", "revenue": 1200,
                "ordered_units": 3,
            }])
            database.save_rub_per_cny(10)
            database.save_product_cost_cny("101", 20, 0, weight_kg=1)
            database.save_product_cost_cny("102", 30, 0, weight_kg=1)
            database.upsert_finance([
                {
                    "operation_id": "settled-sale", "operation_date": "2026-07-15",
                    "operation_type": "OperationAgentDeliveredToCustomer",
                    "amount": 900, "accruals_for_sale": 1000,
                    "sale_commission": -100, "items": [{"sku": "101"}],
                },
                {
                    "operation_id": "settled-acquiring", "operation_date": "2026-07-15",
                    "operation_type": "OperationMarketplaceRedistributionOfAcquiringOperation",
                    "amount": -20, "items": [{"sku": "101"}],
                    "services": [{
                        "name": "MarketplaceRedistributionOfAcquiringOperation",
                        "price": -20,
                    }],
                },
            ])
            rows = database.cross_border_profit_rows("2026-08-01", "2026-08-07")
            self.assertEqual([row["sku"] for row in rows], ["101"])
            self.assertEqual(rows[0]["purchase_rub_total"], 600)
            self.assertIsNotNone(rows[0]["freight_unit_cny"])
            self.assertAlmostEqual(rows[0]["estimated_commission_rate"], 0.10)
            self.assertAlmostEqual(rows[0]["estimated_acquiring_rate"], 0.02)
            # Live CNY formula: 120 sales - 14.4 commission/acquiring
            # - 60 purchase - 135 estimated freight = -89.4.
            self.assertAlmostEqual(
                rows[0]["contribution_before_shop_ads_cny"], -89.40, places=2,
            )
            self.assertIn("120.00 - 14.40 - 60.00 - 135.00", rows[0]["realtime_profit_formula_cny"])
            summary = database.cross_border_period_summary(
                "2026-08-01", "2026-08-07", rows,
            )
            self.assertAlmostEqual(summary["profit_cny"], -89.40, places=2)
            self.assertGreater(summary["reference_cross_border_freight"], 0)
            self.assertTrue(summary["freight_in_realtime_profit"])

    def test_cross_border_realtime_profit_requires_weight_and_fee_history(self) -> None:
        with TemporaryDirectory() as folder:
            database = Database(Path(folder) / "test.db")
            database.upsert_products([{"sku": "101", "offer_id": "SOLD"}])
            database.upsert_sales([{
                "day": "2026-08-01", "sku": "101", "revenue": 1000,
                "ordered_units": 5,
            }])
            database.save_rub_per_cny(10)
            database.save_product_cost_cny("101", 10, 0)
            rows = database.cross_border_profit_rows("2026-08-01", "2026-08-07")
            self.assertIsNone(rows[0]["freight_unit_cny"])
            self.assertFalse(rows[0]["cross_border_cost_complete"])
            summary = database.cross_border_period_summary(
                "2026-08-01", "2026-08-07", rows,
            )
            self.assertEqual(summary["missing_cost_skus"], 1)
            self.assertIsNone(summary["profit"])

    def test_cross_border_orders_use_cached_finance_delivery_schema(self) -> None:
        with TemporaryDirectory() as folder:
            database = Database(Path(folder) / "test.db")
            database.upsert_sales([
                {"day": "2026-08-03", "sku": "101", "revenue": 1000,
                 "ordered_units": 5},
                {"day": "2026-08-03", "sku": "102", "revenue": 500,
                 "ordered_units": 2},
            ])
            finance_rows = []
            for index, (posting, schema, skus, order_day) in enumerate((
                ("P-1", "FBP", ["101"], "2026-08-03T10:00:00Z"),
                ("P-2", "RFBS", ["101", "102"], "2026-08-03T11:00:00Z"),
                ("P-3", "WHD", ["101"], "2026-08-04T10:00:00Z"),
                ("P-OUT", "FBP", ["101"], "2026-07-31T10:00:00Z"),
            )):
                finance_rows.append({
                    "operation_id": f"F-{index}",
                    "operation_date": "2026-08-10T10:00:00Z",
                    "operation_type": "OperationAgentDeliveredToCustomer",
                    "type": "orders", "amount": 100, "accruals_for_sale": 100,
                    "posting": {
                        "posting_number": posting, "delivery_schema": schema,
                        "order_date": order_day,
                    },
                    "items": [{"sku": sku} for sku in skus],
                })
            # Another Finance operation for the same posting/SKU must not
            # double-count the fulfillment order.
            finance_rows.append({
                **finance_rows[0], "operation_id": "F-DUP",
                "operation_type": "OperationMarketplaceServiceItemFulfillment",
                "type": "orders", "amount": -10, "accruals_for_sale": 0,
            })
            database.upsert_finance(finance_rows)

            classified = database.cross_border_fulfillment_orders(
                "2026-08-01", "2026-08-07",
            )
            self.assertEqual(classified["101"]["fulfillment_orders"], 3)
            self.assertEqual(classified["101"]["fbp_orders"], 1)
            self.assertEqual(classified["101"]["rfbs_orders"], 1)
            self.assertEqual(classified["101"]["whd_orders"], 1)
            self.assertEqual(classified["102"]["rfbs_orders"], 1)

            rows = {
                row["sku"]: row for row in database.cross_border_profit_rows(
                    "2026-08-01", "2026-08-07",
                )
            }
            self.assertEqual(rows["101"]["orders"], 5)
            self.assertEqual(rows["101"]["fulfillment_orders"], 3)
            summary = database.cross_border_period_summary(
                "2026-08-01", "2026-08-07", rows=[],
            )
            self.assertEqual(summary["fulfillment_orders"], 4)
            self.assertEqual(summary["fbp_orders"], 1)
            self.assertEqual(summary["rfbs_orders"], 2)
            self.assertEqual(summary["whd_orders"], 1)

    def test_monthly_profit_keeps_official_orders_separate_from_cost_units(self) -> None:
        with TemporaryDirectory() as folder:
            database = Database(Path(folder) / "test.db")
            database.upsert_sales([{
                "day": "2026-07-01", "sku": "2578856473",
                "revenue": 1221690, "ordered_units": 559,
            }])
            database.replace_deliveries("2026-07-01", "2026-07-31", [{
                "event_id": "delivery-1", "day": "2026-07-15",
                "sku": "2578856473", "quantity": 204,
            }])
            row = database.monthly_profit_rows("2026-07-01", "2026-07-31")[0]
            self.assertEqual(row["orders"], 559)
            self.assertEqual(row["revenue"], 1221690)
            self.assertEqual(row["cost_units"], 204)

            incomplete = database.monthly_profit_summary("2026-07-01", "2026-07-31")
            self.assertFalse(incomplete["seller_sales_complete"])
            database.mark_coverage(
                "2026-07-01", "2026-07-31", "sales", "seller_analytics"
            )
            complete = database.monthly_profit_summary("2026-07-01", "2026-07-31")
            self.assertTrue(complete["seller_sales_complete"])

    def test_monthly_accruals_split_services_and_keep_multi_sku_unallocated(self) -> None:
        with TemporaryDirectory() as folder:
            database = Database(Path(folder) / "test.db")
            database.upsert_products([{"sku": "123", "offer_id": "SELLER-A"}])
            database.upsert_sales([{
                "day": "2026-08-03", "sku": "123", "revenue": 1000,
                "ordered_units": 2,
            }])
            database.upsert_finance([
                {
                    "operation_id": "F-DETAIL", "operation_date": "2026-08-04T10:00:00Z",
                    "operation_type": "OperationAgentDeliveredToCustomer",
                    "operation_type_name": "销售",
                    "amount": 700, "accruals_for_sale": 1000, "sale_commission": -200,
                    "items": [{"sku": 123}],
                    "services": [
                        {"name": "MarketplaceServiceCostPerClick", "price": -50},
                        {"name": "MarketplaceServiceItemDirectFlowLogistic", "price": -50},
                    ],
                },
                {
                    "operation_id": "F-MULTI", "operation_date": "2026-08-04T11:00:00Z",
                    "operation_type": "OtherOperation", "amount": -30,
                    "items": [{"sku": 123}, {"sku": 456}],
                },
                {
                    "operation_id": "F-CROSSDOCK", "operation_date": "2026-08-05T11:00:00Z",
                    "operation_type": "MarketplaceServiceItemCrossdocking", "amount": -20,
                    "items": [],
                },
                {
                    "operation_id": "F-CPO", "operation_date": "2026-08-06T11:00:00Z",
                    "operation_type": "OperationPromotionWithCostPerOrder", "amount": -10,
                    "items": [],
                },
            ])
            database.replace_finance_cash_flow("2026-08-01", "2026-08-31", {
                "cash_flows": [{
                    "period": {"begin": "2026-08-01", "end": "2026-08-31"},
                    "currency_code": "RUB", "orders_amount": 1000,
                    "returns_amount": 0, "commission_amount": -200,
                    "item_delivery_and_return_amount": -50, "services_amount": -80,
                }],
                "details": [],
            })
            row = database.monthly_profit_rows("2026-08-01", "2026-08-31")[0]
            self.assertEqual(row["offer_id"], "SELLER-A")
            self.assertEqual(row["ad_spend"], -50)
            self.assertEqual(row["delivery"], -50)
            self.assertEqual(row["platform_payout"], 700)
            summary = database.monthly_profit_summary("2026-08-01", "2026-08-31")
            self.assertTrue(summary["shop_economy_available"])
            self.assertEqual(summary["platform_payout"], 640)
            self.assertEqual(summary["cash_flow_reported_total"], 670)
            self.assertEqual(summary["reconciliation_difference"], 30)
            self.assertEqual(summary["unallocated_operations"], 3)
            self.assertEqual(summary["unallocated_amount"], -60)
            detail = database.monthly_finance_detail("123", "2026-08-01", "2026-08-31")
            self.assertEqual(len(detail["operations"]), 1)
            self.assertIn("点击付费广告", {item["name"] for item in detail["categories"]})
            unallocated = database.monthly_unallocated_finance_rows(
                "2026-08-01", "2026-08-31"
            )
            self.assertIn("越库服务", {item["name"] for item in unallocated})
            self.assertIn("按订单付费推广", {item["name"] for item in unallocated})
            self.assertIn("FBO、仓储与包装", {
                item["category_label"] for item in unallocated
            })
            self.assertIn("推广与广告", {
                item["category_label"] for item in unallocated
            })

    def test_monthly_summary_uses_transactions_when_cash_flow_summary_is_anomalous(self) -> None:
        with TemporaryDirectory() as folder:
            database = Database(Path(folder) / "test.db")
            database.replace_finance_cash_flow("2026-07-01", "2026-07-31", {
                "cash_flows": [{
                    "period": {"begin": "2026-07-01", "end": "2026-07-31"},
                    "currency_code": "RUB", "orders_amount": 10971648.33,
                    "returns_amount": -140364, "commission_amount": -4781717.35,
                    "item_delivery_and_return_amount": -993836.87,
                    # Mirrors the real anomalous settlement summary field.
                    "services_amount": 3624927.06,
                }],
                "details": [{
                    "period": {"begin": "2026-07-01", "end": "2026-07-31"},
                    "others": {"total": 17868.76, "items": []},
                }],
            })
            database.upsert_finance([{
                "operation_id": "ALL-JULY", "operation_date": "2026-07-31T23:00:00Z",
                "operation_type": "OtherOperation", "amount": 3345905.85,
                "items": [],
            }])

            summary = database.monthly_profit_summary("2026-07-01", "2026-07-31")
            self.assertAlmostEqual(summary["sales_returns"], 10831284.33)
            self.assertAlmostEqual(summary["cash_flow_reported_total"], 8680657.17)
            self.assertAlmostEqual(summary["platform_payout"], 3345905.85)
            self.assertAlmostEqual(summary["other_adjustments"], 17868.76)
            self.assertAlmostEqual(summary["accrual_fees"], -7503247.24)
            self.assertAlmostEqual(summary["reconciliation_difference"], 5334751.32)

    def test_monthly_finance_cache_is_invalidated_after_sync(self) -> None:
        with TemporaryDirectory() as folder:
            database = Database(Path(folder) / "test.db")
            database.upsert_finance([{
                "operation_id": "F-1", "operation_date": "2026-08-01T10:00:00Z",
                "operation_type": "Sale", "amount": 10, "items": [{"sku": 123}],
            }])
            first = database.monthly_profit_summary("2026-08-01", "2026-08-31")
            self.assertEqual(first["transaction_total"], 10)
            database.upsert_finance([{
                "operation_id": "F-2", "operation_date": "2026-08-02T10:00:00Z",
                "operation_type": "Sale", "amount": 20, "items": [{"sku": 123}],
            }])
            second = database.monthly_profit_summary("2026-08-01", "2026-08-31")
            self.assertEqual(second["transaction_total"], 30)

    def test_local_product_profit_and_daily_ad_efficiency(self) -> None:
        with TemporaryDirectory() as folder:
            database = Database(Path(folder) / "test.db")
            database.save_settings({
                "rub_per_cny": "10", "local_tax_rate": "6",
                "local_payout_fee_rate": "1", "local_default_delivery_fee": "50",
            })
            database.upsert_products([{"sku": "101", "offer_id": "GJYB001"}])
            database.save_product_cost_cny("101", 10, 0)
            database.upsert_sales([{
                "day": "2026-08-01", "sku": "101", "revenue": 1000,
                "ordered_units": 2,
            }])
            database.replace_posting_routes("2026-08-01", "2026-08-01", [{
                "event_id": "R-1", "day": "2026-08-01", "sku": "101",
                "quantity": 2, "posting_number": "P-1", "origin": "Moscow",
                "destination": "Voronezh", "status": "awaiting_packaging",
            }])
            database.replace_ad_product_details("2026-08-01", "2026-08-01", [{
                "day": "2026-08-01", "campaign_id": "C-1", "sku": "101",
                "spend": 100, "revenue": 400, "orders": 1,
                "impressions": 1000, "clicks": 50,
            }])
            database.upsert_ads([{
                "day": "2026-08-01", "campaign_id": "SHOP", "sku": "",
                "spend": 400,
            }])

            row = database.product_profit_rows("2026-08-01", "2026-08-01")[0]
            self.assertEqual(row["origins"], "Moscow")
            self.assertEqual(row["destinations"], "Voronezh")
            self.assertAlmostEqual(row["ad_cost_per_order"], 100)
            self.assertAlmostEqual(row["ad_spend_share"], 25)
            self.assertAlmostEqual(row["ad_sales_share"], 40)
            self.assertAlmostEqual(row["tacos"], 10)
            self.assertAlmostEqual(row["pre_tax_profit_estimate"], 600)
            self.assertAlmostEqual(row["tax_amount_estimate"], 60)
            self.assertAlmostEqual(row["payout_fee_estimate"], 9)
            self.assertAlmostEqual(row["after_tax_profit_estimate"], 531)

            daily = database.product_ad_daily_rows(
                "2026-08-01", "2026-08-01", "101"
            )[0]
            self.assertAlmostEqual(daily["ad_cost_per_order"], 100)
            self.assertAlmostEqual(daily["ad_spend_share"], 25)
            self.assertAlmostEqual(daily["ad_sales_share"], 40)
            self.assertAlmostEqual(daily["acos"], 25)
            self.assertAlmostEqual(daily["tacos"], 10)
            self.assertAlmostEqual(daily["ctr"], 5)

    def test_effective_product_ads_map_only_unique_campaign_titles(self) -> None:
        with TemporaryDirectory() as folder:
            database = Database(Path(folder) / "test.db")
            database.upsert_products([
                {"sku": "101", "offer_id": "GJYB001-YELLOW"},
                {"sku": "202", "offer_id": "XSB002"},
            ])
            database.upsert_campaigns([
                {"campaign_id": "EXACT", "name": "GJYB001-YELLOW"},
                {"campaign_id": "GENERIC", "name": "全店推广"},
            ])
            database.upsert_ads([
                {
                    "day": "2026-08-20", "campaign_id": "EXACT", "sku": "",
                    "spend": 30, "revenue": 300, "orders": 3,
                },
                {
                    "day": "2026-08-20", "campaign_id": "GENERIC", "sku": "",
                    "spend": 100, "revenue": 1000, "orders": 10,
                },
                {
                    "day": "2026-08-21", "campaign_id": "EXACT", "sku": "",
                    "spend": 40, "revenue": 400, "orders": 4,
                },
                {
                    "day": "2026-08-21", "campaign_id": "EXACT", "sku": "101",
                    "spend": 20, "revenue": 200, "orders": 2,
                },
            ])

            rows = database.effective_product_ad_rows(
                "2026-08-20", "2026-08-21"
            )

            self.assertEqual(len(rows), 2)
            by_day = {row["day"]: row for row in rows}
            self.assertEqual(by_day["2026-08-20"]["sku"], "101")
            self.assertAlmostEqual(by_day["2026-08-20"]["spend"], 30)
            self.assertEqual(
                by_day["2026-08-20"]["attribution_source"],
                "unique_campaign_title",
            )
            self.assertAlmostEqual(by_day["2026-08-21"]["spend"], 20)
            self.assertEqual(
                by_day["2026-08-21"]["attribution_source"], "official_sku"
            )

    def test_secret_roundtrip(self) -> None:
        encrypted = protect("test-secret-中文")
        self.assertNotEqual(encrypted, "test-secret-中文")
        self.assertEqual(unprotect(encrypted), "test-secret-中文")

    def test_route_tariff_unit_profit_and_daily_profit_are_separate(self) -> None:
        with TemporaryDirectory() as folder:
            database = Database(Path(folder) / "test.db")
            database.save_settings({
                "rub_per_cny": "10", "local_tax_rate": "6",
                "local_payout_fee_rate": "1",
            })
            database.upsert_products([{
                "sku": "101", "offer_id": "GJYB001", "name": "测试商品",
            }])
            database.save_product_cost_mixed(
                "101", 10, 20, length_cm=10, width_cm=10,
                height_cm=10, weight_kg=1,
            )
            database.upsert_sales([{
                "day": "2026-08-20", "sku": "101", "revenue": 1000,
                "ordered_units": 2,
            }])
            database.upsert_ads([{
                "day": "2026-08-20", "campaign_id": "C-1", "sku": "101",
                "spend": 100, "revenue": 400, "orders": 1,
            }])
            database.replace_posting_routes(
                "2026-08-20", "2026-08-20", [{
                    "event_id": "route-1", "day": "2026-08-20",
                    "sku": "101", "offer_id": "GJYB001", "quantity": 2,
                    "scheme": "FBO", "status": "awaiting_packaging",
                    "posting_number": "POST-1", "origin": "Moscow",
                    "destination": "Voronezh", "order_price": 1000,
                }],
            )
            with database.connect() as connection:
                connection.execute(
                    """
                    INSERT INTO delivery_tariffs(
                        origin, destination, volume_min_l, volume_max_l,
                        price_le_300, price_gt_300, source_file
                    ) VALUES (?, ?, ?, ?, ?, ?, ?)
                    """,
                    (
                        "莫斯科、莫斯科州和周边地区", "沃罗涅日",
                        0.5, 1.5, 50, 80, "test.xlsx",
                    ),
                )

            order = database.order_detail_rows(
                "2026-08-20", "2026-08-20",
            )[0]
            self.assertEqual(order["tariff_status"], "已匹配")
            self.assertAlmostEqual(order["volume_l"], 1)
            self.assertAlmostEqual(order["estimated_delivery_unit"], 80)
            self.assertAlmostEqual(order["estimated_delivery_total"], 160)

            unit = database.product_unit_profit_rows(
                "2026-08-20", "2026-08-20",
            )[0]
            self.assertEqual(unit["reference_units"], 2)
            self.assertAlmostEqual(unit["selling_price_unit"], 500)
            self.assertAlmostEqual(unit["unit_cost"], 100)
            self.assertAlmostEqual(unit["first_mile_cost"], 20)
            self.assertAlmostEqual(unit["ad_spend_unit"], 50)
            self.assertAlmostEqual(unit["delivery_unit"], 80)
            self.assertAlmostEqual(unit["pre_tax_profit_unit"], 250)
            self.assertAlmostEqual(unit["tax_unit"], 30)
            self.assertAlmostEqual(unit["payout_fee_unit"], 4.2)
            self.assertAlmostEqual(unit["after_tax_profit_unit"], 215.8)

            daily = database.daily_product_profit_rows(
                "2026-08-20", "2026-08-20",
            )[0]
            self.assertEqual(daily["day"], "2026-08-20")
            self.assertAlmostEqual(daily["tacos"], 10)
            self.assertAlmostEqual(daily["ad_cost_per_order"], 50)
            self.assertAlmostEqual(daily["estimated_profit"], 431.6)

    def test_detailed_log_events_are_saved_and_secrets_redacted(self) -> None:
        with TemporaryDirectory() as folder:
            database = Database(Path(folder) / "test.db")
            log_id = database.start_log("Performance API")
            database.append_log_event(
                log_id, "info", "认证", "认证完成",
                {
                    "endpoint": "/api/client/token",
                    "access_token": "must-not-be-stored",
                    "nested": {"client_secret": "also-secret", "rows": 10},
                },
            )
            events = database.log_events(log_id)
            self.assertEqual(len(events), 1)
            self.assertEqual(events[0]["level"], "INFO")
            self.assertEqual(events[0]["details"]["access_token"], "***")
            self.assertEqual(events[0]["details"]["nested"]["client_secret"], "***")
            self.assertEqual(events[0]["details"]["nested"]["rows"], 10)


if __name__ == "__main__":
    unittest.main()
