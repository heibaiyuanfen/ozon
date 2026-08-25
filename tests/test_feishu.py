import json
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from ozon_app.database import Database
from ozon_app.feishu import FeishuClient, field_number, field_text
from ozon_app.services import AppService, Credentials


class FakeHttp:
    def __init__(self) -> None:
        self.calls = []

    def request(self, method, url, *, headers=None, body=None, timeout=30):
        self.calls.append((method, url, headers or {}, body, timeout))
        if "/auth/v3/tenant_access_token/internal/" in url:
            return {"code": 0, "tenant_access_token": "tenant-token", "expire": 7200}
        if "/fields" in url:
            return {
                "code": 0,
                "data": {"items": [{"field_name": "SKU", "is_primary": True}]},
            }
        if "/records" in url and method == "GET":
            return {"code": 0, "data": {"items": [], "has_more": False}}
        if "/im/v1/messages" in url:
            return {"code": 0, "data": {"message_id": "om_123"}}
        return {"code": 0, "data": {}}


class FakeFeishuClient:
    def __init__(self, records=None) -> None:
        self.records = list(records or [])
        self.created = []
        self.updated = []
        self.messages = []
        self.cards = []

    def ensure_fields(self, app_token, table_id, definitions):
        return ({name: {} for name in definitions}, "主字段")

    def list_records(self, app_token, table_id):
        return list(self.records)

    def batch_create(self, app_token, table_id, records):
        values = list(records)
        self.created.extend(values)
        return len(values)

    def batch_update(self, app_token, table_id, records):
        values = list(records)
        self.updated.extend(values)
        return len(values)

    def send_text(self, chat_id, text):
        self.messages.append((chat_id, text))
        return "om_weekly"

    def send_card(self, chat_id, card):
        self.cards.append((chat_id, card))
        return "om_card"


class FeishuClientTests(unittest.TestCase):
    def test_auth_table_read_and_group_message_request(self) -> None:
        http = FakeHttp()
        client = FeishuClient("cli_1", "secret", http=http)

        self.assertIn("1 个字段", client.test_connection("app_token", "tbl_product"))
        self.assertEqual(client.list_records("app_token", "tbl_product"), [])
        self.assertEqual(client.send_text("oc_group", "周报内容"), "om_123")
        card = {
            "elements": [{"tag": "div", "text": {"tag": "lark_md", "content": "周报"}}]
        }
        self.assertEqual(client.send_card("oc_group", card), "om_123")

        token_calls = [call for call in http.calls if "/tenant_access_token/" in call[1]]
        self.assertEqual(len(token_calls), 1)
        message_calls = [call for call in http.calls if "/im/v1/messages" in call[1]]
        message_call = message_calls[0]
        self.assertIn("receive_id_type=chat_id", message_call[1])
        self.assertEqual(message_call[2]["Authorization"], "Bearer tenant-token")
        self.assertEqual(message_call[3]["receive_id"], "oc_group")
        self.assertEqual(json.loads(message_call[3]["content"])["text"], "周报内容")
        self.assertEqual(message_calls[1][3]["msg_type"], "interactive")
        self.assertEqual(json.loads(message_calls[1][3]["content"]), card)

    def test_field_value_helpers_accept_bitable_shapes(self) -> None:
        self.assertEqual(field_text([{"text": "SKU-"}, {"text": "1"}]), "SKU-1")
        self.assertEqual(field_number("1 234,50"), 1234.5)
        self.assertIsNone(field_number("—"))


class FeishuServiceTests(unittest.TestCase):
    def credentials(self) -> Credentials:
        return Credentials(
            feishu_app_id="cli_1",
            feishu_app_secret="secret",
            feishu_app_token="app_token",
            feishu_product_table_id="tbl_product",
            feishu_weekly_table_id="tbl_weekly",
            feishu_tracking_table_id="tbl_tracking",
            feishu_chat_id="oc_group",
        )

    def test_product_both_uses_remote_cost_and_preserves_missing_remote_value(self) -> None:
        with TemporaryDirectory() as folder:
            database = Database(Path(folder) / "test.db")
            database.upsert_products([
                {"sku": "SKU-1", "offer_id": "OFFER-1", "name": "商品一"},
                {"sku": "SKU-3", "offer_id": "OFFER-3", "name": "商品三"},
            ])
            database.save_product_cost("SKU-1", 10, 3, "本地备注")
            database.save_product_cost("SKU-3", 30, 6, "")
            client = FakeFeishuClient([
                {
                    "record_id": "rec_1",
                    "fields": {"SKU": "SKU-1", "单件成本": 11, "备注": "飞书备注"},
                },
                {
                    "record_id": "rec_2",
                    "fields": {"SKU": "SKU-2", "单件成本": 20, "单件头程": 4},
                },
            ])
            service = AppService(database, shop_name="莫斯科一店", shop_kind="local")
            service.feishu_client = lambda credentials=None: client  # type: ignore[method-assign]

            result = service.sync_feishu_products("both", self.credentials())

            rows = {row["sku"]: row for row in database.product_sync_rows()}
            self.assertEqual(rows["SKU-1"]["unit_cost"], 11)
            self.assertEqual(rows["SKU-1"]["first_mile_cost"], 3)
            self.assertEqual(rows["SKU-1"]["note"], "飞书备注")
            self.assertEqual(rows["SKU-2"]["unit_cost"], 20)
            self.assertEqual(result["pulled"], 2)
            self.assertEqual(result["partial"], 1)
            self.assertEqual(len(client.created), 1)
            self.assertEqual(client.created[0]["fields"]["SKU"], "SKU-3")
            self.assertEqual(
                {item["record_id"] for item in client.updated}, {"rec_1", "rec_2"}
            )

    def test_product_sync_accepts_cny_purchase_and_rub_first_mile_fields(self) -> None:
        with TemporaryDirectory() as folder:
            database = Database(Path(folder) / "test.db")
            database.upsert_products([{
                "sku": "SKU-1", "offer_id": "OFFER-1", "name": "商品一",
            }])
            database.save_product_cost_mixed(
                "SKU-1", 10, 30, length_cm=12, width_cm=8,
                height_cm=5, weight_kg=0.6,
            )
            client = FakeFeishuClient([{
                "record_id": "rec_1",
                "fields": {
                    "SKU": "SKU-1", "采购成本 CNY": 12.5,
                    "头程 RUB": 35, "备注": "新币种字段",
                },
            }])
            service = AppService(database, shop_name="莫斯科一店", shop_kind="local")
            service.feishu_client = lambda credentials=None: client  # type: ignore[method-assign]

            service.sync_feishu_products("both", self.credentials())

            row = database.product_sync_rows()[0]
            self.assertEqual(row["unit_cost_cny"], 12.5)
            self.assertEqual(row["first_mile_cost"], 35)
            self.assertEqual(row["length_cm"], 12)
            self.assertEqual(row["weight_kg"], 0.6)
            self.assertEqual(row["note"], "新币种字段")
            self.assertEqual(client.updated[0]["fields"]["采购成本 CNY"], 12.5)
            self.assertEqual(client.updated[0]["fields"]["头程 RUB"], 35)

    def test_weekly_upserts_table_and_sends_group_summary(self) -> None:
        with TemporaryDirectory() as folder:
            database = Database(Path(folder) / "test.db")
            database.upsert_sales([
                {"day": "2026-08-08", "sku": "SKU-1", "revenue": 1000,
                 "ordered_units": 10},
                {"day": "2026-08-01", "sku": "SKU-1", "revenue": 500,
                 "ordered_units": 5},
            ])
            database.upsert_ads([
                {"day": "2026-08-08", "campaign_id": "C-1", "spend": 100,
                 "revenue": 400, "orders": 4},
                {"day": "2026-08-01", "campaign_id": "C-1", "spend": 50,
                 "revenue": 200, "orders": 2},
            ])
            client = FakeFeishuClient()
            service = AppService(database, shop_name="莫斯科一店", shop_kind="local")
            service.feishu_client = lambda credentials=None: client  # type: ignore[method-assign]

            result = service.sync_feishu_weekly(
                "2026-08-09", credentials=self.credentials()
            )

            self.assertEqual(result["table_action"], "新增")
            self.assertEqual(result["message_id"], "om_card")
            fields = client.created[0]["fields"]
            self.assertEqual(fields["周报周期"], "2026-08-03 ~ 2026-08-09")
            self.assertEqual(fields["店铺名称"], "莫斯科一店")
            self.assertEqual(fields["销售额"], 1000)
            self.assertEqual(fields["ACoTS"], 10)
            self.assertEqual(client.cards[0][0], "oc_group")
            card = client.cards[0][1]
            self.assertIn("莫斯科一店", card["header"]["title"]["content"])
            rendered = json.dumps(card, ensure_ascii=False)
            self.assertIn("核心指标", rendered)
            self.assertIn("每日经营数据", rendered)
            self.assertIn("1 000 ₽", rendered)

    def test_shipment_tracking_waits_for_manual_notification(self) -> None:
        with TemporaryDirectory() as folder:
            database = Database(Path(folder) / "test.db")
            client = FakeFeishuClient([{
                "record_id": "rec_track",
                "fields": {
                    "采购单号": "CG-20260820", "品名": "工具腰包",
                    "批次号": "BATCH-1", "店铺": "莫斯科一店",
                    "数量": 300, "货物状态": "已送仓", "渠道": "超光速",
                    "国内到库": 1787155200000,
                },
            }])
            service = AppService(database, shop_name="莫斯科一店", shop_kind="local")
            service.feishu_client = lambda credentials=None: client  # type: ignore[method-assign]

            baseline = service.sync_feishu_shipments(self.credentials())
            self.assertEqual(baseline["notifications"], 0)
            client.records[0]["fields"]["国外到库"] = "2026/08/27"
            arrived = service.sync_feishu_shipments(self.credentials())
            repeated = service.sync_feishu_shipments(self.credentials())

            self.assertEqual(arrived["arrivals"], 1)
            self.assertEqual(arrived["notifications"], 0)
            self.assertEqual(repeated["notifications"], 0)
            self.assertEqual(len(client.cards), 0)
            message_id = service.send_feishu_shipment_notification(
                "CG-20260820", credentials=self.credentials(),
            )
            self.assertEqual(message_id, "om_card")
            self.assertEqual(len(client.cards), 1)
            rendered = json.dumps(client.cards[0][1], ensure_ascii=False)
            self.assertIn("货物已到国外仓", rendered)
            self.assertIn("CG-20260820", rendered)
            row = database.shipment_tracking_rows()[0]
            self.assertEqual(row["foreign_arrival"], "2026-08-27")
            self.assertEqual(row["notified_foreign_arrival"], "2026-08-27")

    def test_monthly_and_cross_border_cards_use_local_report_data(self) -> None:
        with TemporaryDirectory() as folder:
            database = Database(Path(folder) / "test.db")
            database.upsert_sales([
                {"day": "2026-08-03", "sku": "SKU-1", "revenue": 1080,
                 "ordered_units": 10},
            ])
            client = FakeFeishuClient()
            local_service = AppService(database, shop_name="本土一店", shop_kind="local")
            local_service.feishu_client = lambda credentials=None: client  # type: ignore[method-assign]
            monthly = local_service.send_feishu_monthly_profit(
                "2026-08", credentials=self.credentials()
            )
            self.assertEqual(monthly["message_id"], "om_card")
            self.assertIn("本土一店｜月度盈亏表", client.cards[-1][1]["header"]["title"]["content"])

            cross_service = AppService(
                database, shop_name="跨境一店", shop_kind="cross_border",
            )
            cross_service.feishu_client = lambda credentials=None: client  # type: ignore[method-assign]
            cross = cross_service.send_feishu_cross_border_profit(
                "2026-08-03", "2026-08-09", credentials=self.credentials()
            )
            self.assertEqual(cross["message_id"], "om_card")
            self.assertIn("跨境一店｜跨境店周利润表", client.cards[-1][1]["header"]["title"]["content"])

    def test_product_series_is_manually_sent_to_group_not_bitable(self) -> None:
        with TemporaryDirectory() as folder:
            database = Database(Path(folder) / "test.db")
            client = FakeFeishuClient()
            service = AppService(database, shop_name="莫斯科一店", shop_kind="local")
            service.feishu_client = lambda credentials=None: client  # type: ignore[method-assign]
            rows = [
                {
                    "period": f"2026-08-{index:02d} 至 2026-08-{index + 6:02d}",
                    "series": f"WZW{index:03d}", "sku_count": index,
                    "units": index * 10, "revenue": index * 1000,
                }
                for index in range(1, 21)
            ]

            result = service.send_feishu_product_series(
                rows, "每7天", credentials=self.credentials(),
            )

            self.assertEqual(result["rows"], 20)
            self.assertEqual(result["messages"], 2)
            self.assertEqual(len(client.cards), 2)
            self.assertEqual(client.created, [])
            self.assertEqual(client.updated, [])
            self.assertTrue(all(chat_id == "oc_group" for chat_id, _card in client.cards))
            rendered = json.dumps(client.cards[0][1], ensure_ascii=False)
            self.assertIn("莫斯科一店｜产品系列分析", rendered)
            self.assertIn("系列表现", rendered)
            self.assertIn("WZW001", rendered)
            self.assertIn("2 100 件", rendered)
            self.assertIn("不会自动写入多维表格", rendered)


if __name__ == "__main__":
    unittest.main()
