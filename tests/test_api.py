import io
import threading
import urllib.error
import unittest
from unittest.mock import patch

from ozon_app.api import (
    JsonHttpClient,
    OzonApiError,
    PerformanceApiClient,
    SellerApiClient,
    SyncCancelled,
    _normalize_ad_row,
    _merge_ad_rows,
    _parse_performance_report,
)


class ApiParserTests(unittest.TestCase):
    def test_normalize_json_ad_row(self) -> None:
        row = _normalize_ad_row(
            {
                "campaignId": "12", "date": "2026-08-01", "impressions": 1000,
                "clicks": 50, "orders": 4, "revenue": 12000, "spend": 1300,
            },
            {"12": "测试广告"},
        )
        self.assertEqual(row["campaign_name"], "测试广告")
        self.assertEqual(row["clicks"], 50)
        self.assertEqual(row["spend"], 1300)

    def test_normalize_daily_statistics_fields(self) -> None:
        row = _normalize_ad_row(
            {
                "campaignId": "12", "date": "2026-08-01", "views": "1000",
                "clicks": "50", "toCart": "9", "modelOrders": "4",
                "modelSales": "12000.50", "expense": "1300.25",
            },
            {"12": "测试广告"},
        )
        self.assertEqual(row["impressions"], 1000)
        self.assertEqual(row["cart_adds"], 9)
        self.assertEqual(row["orders"], 4)
        self.assertEqual(row["revenue"], 12000.50)
        self.assertEqual(row["spend"], 1300.25)

    def test_normalize_daily_statistics_id_and_title_fields(self) -> None:
        row = _normalize_ad_row(
            {
                "id": "321", "title": "真实日报活动", "date": "2026-08-01",
                "views": "2845", "clicks": "40", "moneySpent": "2820,00",
                "orders": "23", "ordersMoney": "28200,00",
            },
            {"321": "数据库活动名"},
        )
        self.assertEqual(row["campaign_id"], "321")
        self.assertEqual(row["campaign_name"], "真实日报活动")
        self.assertEqual(row["day"], "2026-08-01")
        self.assertEqual(row["impressions"], 2845)
        self.assertEqual(row["orders"], 23)
        self.assertEqual(row["spend"], 2820)
        self.assertEqual(row["revenue"], 28200)

    def test_normalize_nested_campaign_fields(self) -> None:
        row = _normalize_ad_row(
            {
                "campaign": {"id": "456", "title": "嵌套活动"},
                "day": "2026-08-02", "views": "200",
            },
            {},
        )
        self.assertEqual(row["campaign_id"], "456")
        self.assertEqual(row["campaign_name"], "嵌套活动")

    def test_merge_duplicate_daily_campaign_rows(self) -> None:
        rows = _merge_ad_rows([
            {"day": "2026-08-01", "campaign_id": "1", "sku": "", "clicks": 2, "spend": 10},
            {"day": "2026-08-01", "campaign_id": "1", "sku": "", "clicks": 3, "spend": 20},
        ])
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows[0]["clicks"], 5)
        self.assertEqual(rows[0]["spend"], 30)

    def test_parse_russian_csv(self) -> None:
        text = "Дата;Кампания;Показы;Клики;Заказы;Выручка;Расход с НДС\n2026-08-01;99;1000;40;3;9000;1000\n"
        rows = _parse_performance_report(text, {"99": "活动"})
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows[0]["orders"], 3)
        self.assertEqual(rows[0]["revenue"], 9000)

    def test_parse_official_csv_with_title_preamble(self) -> None:
        text = (
            "; Кампания по продвижению товаров № 13883040, период 01.08.2026-01.08.2026\n"
            "sku;Название товара;Показы;Клики;Расход, Р, с НДС;Заказы;Выручка, Р\n"
            "10001;Товар;1000;40;1000,50;3;9000,25\n"
        )
        rows = _parse_performance_report(
            text, {"13883040": "测试活动"}, default_day="2026-08-01"
        )
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows[0]["campaign_id"], "13883040")
        self.assertEqual(rows[0]["campaign_name"], "测试活动")
        self.assertEqual(rows[0]["day"], "2026-08-01")
        self.assertEqual(rows[0]["spend"], 1000.50)

    def test_seller_connection_uses_roles_endpoint(self) -> None:
        class FakeHttp:
            def __init__(self) -> None:
                self.url = ""

            def request(self, method, url, **kwargs):
                self.url = url
                return {"roles": []}

        http = FakeHttp()
        result = SellerApiClient("1", "key", http=http).test_connection()
        self.assertIn("连接成功", result)
        self.assertTrue(http.url.endswith("/v1/roles"))

    def test_seller_cash_flow_statement_reads_summary_and_details(self) -> None:
        class FakeHttp:
            def __init__(self) -> None:
                self.body = {}

            def request(self, method, url, **kwargs):
                self.body = kwargs.get("body") or {}
                self.url = url
                return {"result": {
                    "cash_flows": [{"currency_code": "RUB", "orders_amount": 10}],
                    "details": [{"services": {"items": [], "total": -1}}],
                    "page_count": 1,
                }}

        http = FakeHttp()
        report = SellerApiClient("1", "key", http=http).finance_cash_flow_statement(
            "2026-08-01", "2026-08-15"
        )
        self.assertTrue(http.url.endswith("/v1/finance/cash-flow-statement/list"))
        self.assertTrue(http.body["with_details"])
        self.assertEqual(http.body["date"]["from"], "2026-08-01T00:00:00Z")
        self.assertEqual(len(report["cash_flows"]), 1)
        self.assertEqual(len(report["details"]), 1)

    @patch("ozon_app.api._wait_for_rate_limit", return_value=None)
    @patch("ozon_app.api._mark_rate_limit", return_value=None)
    def test_seller_analytics_records_omitted_subscription_metrics(
        self, _mark_rate, _wait_rate,
    ) -> None:
        class FakeHttp:
            def request(self, method, url, **kwargs):
                return {"result": {"totals": [1000, 10], "data": [{
                    "dimensions": [
                        {"id": "123", "name": "商品 A"},
                        {"id": "2026-08-01", "name": "2026-08-01"},
                    ],
                    "metrics": [1000, 10],
                }]}}

        client = SellerApiClient("1", "key", http=FakeHttp())
        rows = client.analytics("2026-08-01", "2026-08-01")
        self.assertEqual(rows[0]["revenue"], 1000)
        self.assertEqual(rows[0]["views"], 0)
        self.assertFalse(client.last_analytics_metadata["traffic_available"])
        self.assertEqual(client.last_analytics_metadata["returned_metric_count"], 2)
        self.assertIn("hits_view", client.last_analytics_metadata["omitted_metrics"])

    def test_seller_product_return_and_cancellation_parsers(self) -> None:
        class FakeHttp:
            def request(self, method, url, **kwargs):
                if url.endswith("/v3/product/list"):
                    return {"result": {"items": [{
                        "sku": 123, "offer_id": "SELLER-A", "product_id": 99,
                    }], "total": 1, "last_id": "done"}}
                if url.endswith("/v1/returns/list"):
                    return {"returns": [
                        {
                            "id": 7, "type": "ClientReturn", "product": {
                                "sku": 123, "offer_id": "SELLER-A",
                                "name": "商品 A", "quantity": 2,
                            }, "logistic": {"return_date": "2026-08-03T10:00:00Z"},
                        },
                        {
                            "id": 8, "type": "Cancellation", "product": {
                                "sku": 123, "offer_id": "SELLER-A",
                                "name": "商品 A", "quantity": 9,
                            }, "logistic": {"return_date": "2026-08-03T11:00:00Z"},
                        },
                    ], "has_next": False}
                if url.endswith("/v4/posting/fbs/list"):
                    return {"postings": [{
                        "posting_number": "P-1", "status": "cancelled",
                        "in_process_at": "2026-08-02T08:00:00Z",
                        "analytics_data": {"warehouse": "Moscow", "region": "Tula"},
                        "products": [{
                            "sku": 123, "offer_id": "SELLER-A", "name": "商品 A", "quantity": 1,
                        }],
                    }], "has_next": False}
                if url.endswith("/v3/posting/fbo/list"):
                    return {"postings": [{
                        "posting_number": "P-2", "status": "delivered",
                        "in_process_at": "2026-08-02T09:00:00Z",
                        "products": [{
                            "sku": 123, "offer_id": "SELLER-A", "name": "商品 A", "quantity": 3,
                        }],
                    }], "has_next": False}
                raise AssertionError(url)

        client = SellerApiClient("1", "key", http=FakeHttp())
        products = client.products()
        returns = client.returns("2026-08-01", "2026-08-13")
        cancellations = client.cancellations("2026-08-01", "2026-08-13")
        deliveries = client.deliveries("2026-08-01", "2026-08-13")
        routes = client.posting_routes("2026-08-01", "2026-08-13")
        self.assertEqual(products[0]["offer_id"], "SELLER-A")
        self.assertEqual(len(returns), 1)
        self.assertEqual(returns[0]["quantity"], 2)
        self.assertEqual(returns[0]["day"], "2026-08-03")
        self.assertEqual(cancellations[0]["scheme"], "FBS")
        self.assertEqual(cancellations[0]["quantity"], 1)
        self.assertEqual(deliveries[0]["scheme"], "FBO")
        self.assertEqual(deliveries[0]["quantity"], 3)
        self.assertEqual(len(routes), 2)
        self.assertEqual(routes[0]["origin"], "Moscow")
        self.assertEqual(routes[0]["destination"], "Tula")

    @patch("ozon_app.api.time.sleep", return_value=None)
    def test_performance_report_status_then_download(self, _sleep) -> None:
        class FakeHttp:
            def __init__(self) -> None:
                self.urls = []

            def request(self, method, url, **kwargs):
                self.urls.append(url)
                if "/statistics/report?" in url:
                    return {"rows": [{"campaignId": "12", "date": "2026-08-01", "expense": "10"}]}
                return {"state": "OK", "UUID": "abc"}

        http = FakeHttp()
        client = PerformanceApiClient("id", "secret", http=http)
        client.access_token = "token"
        report = client._wait_report("abc", None)
        self.assertEqual(report["rows"][0]["expense"], "10")
        self.assertIn("/api/client/statistics/abc", http.urls[0])
        self.assertIn("/api/client/statistics/report?UUID=abc", http.urls[1])

    @patch("urllib.request.urlopen")
    def test_http_error_shows_endpoint(self, urlopen) -> None:
        urlopen.side_effect = urllib.error.HTTPError(
            "https://api-seller.ozon.ru/v1/old", 400, "Bad Request", {},
            io.BytesIO(b'{"message":"obsolete method cannot be used"}')
        )
        with self.assertRaisesRegex(OzonApiError, r"接口 /v1/old"):
            JsonHttpClient().request("POST", "https://api-seller.ozon.ru/v1/old", body={})

    @patch("urllib.request.urlopen")
    @patch("ozon_app.api._wait", return_value=None)
    def test_http_429_retries_automatically(self, _wait, urlopen) -> None:
        rate_error = urllib.error.HTTPError(
            "https://api-seller.ozon.ru/v1/analytics/data", 429, "Too Many Requests", {},
            io.BytesIO(b'{}')
        )

        class Response:
            headers = {"Content-Type": "application/json"}

            def __enter__(self):
                return self

            def __exit__(self, *args):
                return False

            def read(self):
                return b'{"ok":true}'

        urlopen.side_effect = [rate_error, Response()]
        result = JsonHttpClient().request(
            "POST", "https://api-seller.ozon.ru/v1/analytics/data", body={}, max_attempts=2
        )
        self.assertTrue(result["ok"])
        self.assertEqual(urlopen.call_count, 2)

    @patch("urllib.request.urlopen")
    def test_http_429_can_fail_fast_without_waiting(self, urlopen) -> None:
        urlopen.side_effect = urllib.error.HTTPError(
            "https://api-seller.ozon.ru/v1/analytics/data", 429,
            "Too Many Requests", {}, io.BytesIO(b'{}'),
        )
        with self.assertRaisesRegex(OzonApiError, "429"):
            JsonHttpClient().request(
                "POST", "https://api-seller.ozon.ru/v1/analytics/data",
                body={}, max_attempts=4, retry_429=False,
            )
        self.assertEqual(urlopen.call_count, 1)

    def test_wait_report_can_be_cancelled(self) -> None:
        cancel = threading.Event()
        cancel.set()
        client = PerformanceApiClient("id", "secret")
        with self.assertRaises(SyncCancelled):
            client._wait_report("abc", None, cancel)

    @patch("ozon_app.api._wait", return_value=None)
    def test_statistics_submits_campaigns_in_batches_of_ten(self, _wait) -> None:
        class FakeHttp:
            def __init__(self) -> None:
                self.submitted = []
                self.counter = 0

            def request(self, method, url, **kwargs):
                if method == "POST" and url.endswith("/statistics/json"):
                    self.counter += 1
                    self.submitted.append(kwargs["body"]["campaigns"])
                    return {"UUID": f"report-{self.counter}"}
                if "/statistics/report?" in url:
                    return {"rows": [{
                        "campaignId": "1", "date": "2026-08-01",
                        "impressions": 100, "clicks": 10, "expense": 20,
                    }]}
                return {"state": "OK"}

        campaigns = [
            {"campaign_id": str(index), "name": f"活动 {index}", "date_from": "2026-01-01", "date_to": ""}
            for index in range(1, 26)
        ]
        http = FakeHttp()
        client = PerformanceApiClient("id", "secret", http=http)
        client.access_token = "token"
        rows = client.statistics(campaigns, "2026-08-01", "2026-08-13")
        self.assertEqual([len(batch) for batch in http.submitted], [10, 10, 5])
        self.assertEqual(len(rows), 3)

    def test_daily_statistics_uses_one_fast_request_and_emits_details(self) -> None:
        class FakeHttp:
            def __init__(self) -> None:
                self.calls = []

            def request(self, method, url, **kwargs):
                self.calls.append((method, url, kwargs))
                return {"rows": [{
                    "campaignId": "12", "date": "2026-08-01", "views": "100",
                    "clicks": "10", "expense": "20", "modelOrders": "2",
                    "modelSales": "200",
                }]}

        http = FakeHttp()
        events = []
        client = PerformanceApiClient("id", "secret", http=http)
        client.access_token = "token"
        rows = client.statistics_daily(
            [{
                "campaign_id": "12", "name": "广告 12",
                "date_from": "2026-01-01", "date_to": "",
            }],
            "2026-08-01", "2026-08-13",
            event=lambda *values: events.append(values),
        )
        self.assertEqual(len(http.calls), 1)
        self.assertEqual(http.calls[0][0], "GET")
        self.assertIn("/api/client/statistics/daily/json?", http.calls[0][1])
        self.assertNotIn("campaignIds=", http.calls[0][1])
        self.assertEqual(rows[0]["impressions"], 100)
        self.assertEqual(rows[0]["campaign_name"], "广告 12")
        self.assertEqual(events[-1][0], "SUCCESS")
        self.assertEqual(events[-1][3]["sample_rows"][0]["campaign_id"], "12")

    def test_daily_statistics_rejects_unrecognized_nonempty_rows(self) -> None:
        class FakeHttp:
            def request(self, method, url, **kwargs):
                return {"rows": [{"unknown": "value", "views": "100"}]}

        events = []
        client = PerformanceApiClient("id", "secret", http=FakeHttp())
        client.access_token = "token"
        with self.assertRaisesRegex(OzonApiError, "字段结构无法识别"):
            client.statistics_daily(
                [{"campaign_id": "12", "name": "广告 12"}],
                "2026-08-01", "2026-08-13",
                event=lambda *values: events.append(values),
            )
        warning_details = [item[3] for item in events if item[0] == "WARNING"]
        self.assertEqual(warning_details[0]["raw_row_keys"], ["unknown", "views"])
        self.assertEqual(events[-1][0], "ERROR")

    def test_product_statistics_by_sku_uses_exact_official_endpoint(self) -> None:
        class FakeHttp:
            def __init__(self) -> None:
                self.calls = []

            def request(self, method, url, **kwargs):
                self.calls.append((method, url, kwargs))
                return {"rows": [{
                    "campaignId": "21414631", "sku": "2654734207",
                    "date": "2026-08-20", "views": 34353, "clicks": 1658,
                    "toCart": 144, "modelOrders": 12,
                    "modelSales": 263493, "expense": 8544.74,
                }]}

        http = FakeHttp()
        client = PerformanceApiClient("id", "secret", http=http)
        client.access_token = "token"
        rows = client.product_statistics_by_sku(
            [{
                "campaign_id": "21414631", "name": "WZW 沙漠数码",
                "date_from": "2026-01-01", "date_to": "",
            }],
            "2026-08-20", "2026-08-20",
        )

        self.assertEqual(len(http.calls), 1)
        self.assertEqual(http.calls[0][0], "POST")
        self.assertTrue(http.calls[0][1].endswith(
            "/api/client/statistics/products/sku"
        ))
        self.assertEqual(http.calls[0][2]["body"]["campaignIds"], ["21414631"])
        self.assertEqual(rows[0]["sku"], "2654734207")
        self.assertEqual(rows[0]["campaign_name"], "WZW 沙漠数码")
        self.assertEqual(rows[0]["spend"], 8544.74)


if __name__ == "__main__":
    unittest.main()
