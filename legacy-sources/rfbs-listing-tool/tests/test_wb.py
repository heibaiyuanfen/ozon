from __future__ import annotations

import copy
import tempfile
import unittest
from pathlib import Path

from PIL import Image

from wb import (
    WbApiError, WbListingWorkflow, WildberriesApi, build_card_content_document,
    build_card_payload, parse_card_content_document, parse_characteristic_value,
    validate_image_paths,
)


class FakeResponse:
    def __init__(self, status_code=200, payload=None, text=""):
        self.status_code = status_code
        self.payload = {} if payload is None else payload
        self.text = text
        self.content = text.encode("utf-8") if text else (b"{}" if payload is not None else b"")

    def json(self):
        return self.payload


class FakeSession:
    def __init__(self, responses):
        self.responses = list(responses)
        self.calls = []

    def request(self, method, url, **kwargs):
        self.calls.append((method, url, kwargs))
        if not self.responses:
            raise AssertionError("unexpected HTTP request")
        return self.responses.pop(0)


def valid_inputs():
    return {
        "subject_id": "123",
        "vendor_code": "WB-TEST-1",
        "title": "测试商品",
        "description": "这是一段商品描述",
        "brand": "",
        "barcode": "2030000000012",
        "length_cm": "10",
        "width_cm": "20",
        "height_cm": "30",
        "weight_kg": "0.6",
        "tech_size": "",
        "wb_size": "",
        "price": "999",
        "discount": "10",
        "warehouse_id": "88",
        "stock": "7",
        "characteristic_definitions": [
            {"charcID": 1, "name": "颜色", "required": True, "charcType": 0},
            {"charcID": 2, "name": "数量", "required": False, "charcType": 4},
        ],
        "characteristic_values": {"1": '["红色"]', "2": "3"},
    }


class WildberriesApiTests(unittest.TestCase):
    def test_bearer_header_and_parent_endpoint(self):
        session = FakeSession([FakeResponse(payload={"data": [{"id": 1, "name": "家居"}]})])
        api = WildberriesApi("Bearer secret-token", session=session)

        result = api.parent_categories()

        self.assertEqual(result[0]["id"], 1)
        method, url, kwargs = session.calls[0]
        self.assertEqual(method, "GET")
        self.assertTrue(url.endswith("/content/v2/object/parent/all"))
        self.assertEqual(kwargs["headers"]["Authorization"], "Bearer secret-token")
        self.assertEqual(kwargs["params"], {"locale": "zh"})

    def test_http_error_exposes_wb_detail(self):
        session = FakeSession([
            FakeResponse(401, {"title": "unauthorized", "detail": "token expired"}),
        ])
        api = WildberriesApi("secret", session=session)

        with self.assertRaises(WbApiError) as context:
            api.parent_categories()

        self.assertEqual(context.exception.status_code, 401)
        self.assertIn("token expired", str(context.exception))

    def test_price_status_falls_back_from_buffer_to_history(self):
        session = FakeSession([
            FakeResponse(404, {"detail": "not found"}),
            FakeResponse(200, {"data": {"id": 12, "status": 3}}),
        ])
        api = WildberriesApi("secret", session=session)

        status = api.price_task_status(12)

        self.assertEqual(status["status"], 3)
        self.assertIn("/api/v2/buffer/tasks", session.calls[0][1])
        self.assertIn("/api/v2/history/tasks", session.calls[1][1])


class PayloadTests(unittest.TestCase):
    def test_build_card_payload_converts_dimensions_and_characteristics(self):
        payload = build_card_payload(valid_inputs())

        variant = payload[0]["variants"][0]
        self.assertEqual(payload[0]["subjectID"], 123)
        self.assertEqual(variant["vendorCode"], "WB-TEST-1")
        self.assertEqual(variant["dimensions"]["weightBrutto"], 0.6)
        self.assertEqual(variant["characteristics"], [
            {"id": 1, "value": ["红色"]},
            {"id": 2, "value": 3},
        ])
        self.assertEqual(variant["sizes"], [{"skus": ["2030000000012"]}])

    def test_sized_product_keeps_technical_and_wb_sizes(self):
        inputs = valid_inputs()
        inputs["tech_size"] = "S"
        inputs["wb_size"] = "42"

        variant = build_card_payload(inputs)[0]["variants"][0]

        self.assertEqual(
            variant["sizes"],
            [{"skus": ["2030000000012"], "techSize": "S", "wbSize": "42"}],
        )

    def test_editable_content_json_round_trips_api_fields(self):
        inputs = valid_inputs()
        document = build_card_content_document(inputs)

        values = parse_card_content_document(
            document, inputs["characteristic_definitions"], expected_subject_id=123,
        )

        self.assertNotIn("dimensions", document["variants"][0])
        self.assertEqual(values["barcode"], "2030000000012")
        self.assertEqual(values["characteristic_values"]["1"], ["红色"])
        self.assertEqual(values["characteristic_values"]["2"], 3)

    def test_editable_content_json_rejects_attribute_outside_live_schema(self):
        inputs = valid_inputs()
        document = build_card_content_document(inputs)
        document["variants"][0]["characteristics"].append({"id": 999, "value": ["错误"]})

        with self.assertRaisesRegex(ValueError, "不属于当前 WB 子类目"):
            parse_card_content_document(document, inputs["characteristic_definitions"])

    def test_missing_required_characteristic_is_rejected(self):
        inputs = valid_inputs()
        inputs["characteristic_values"] = {}

        with self.assertRaisesRegex(ValueError, "颜色"):
            build_card_payload(inputs)

    def test_characteristic_json_and_number_parsing(self):
        self.assertEqual(parse_characteristic_value('["A", "B"]'), ["A", "B"])
        self.assertEqual(parse_characteristic_value("2.5", 4), 2.5)
        self.assertEqual(parse_characteristic_value("棉"), ["棉"])

    def test_image_validation_preserves_primary_order(self):
        with tempfile.TemporaryDirectory() as directory:
            first = Path(directory) / "first.jpg"
            second = Path(directory) / "second.png"
            Image.new("RGB", (700, 900), "white").save(first)
            Image.new("RGB", (900, 1000), "blue").save(second)

            paths = validate_image_paths([second, first])

            self.assertEqual(paths, [str(second), str(first)])

    def test_small_image_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "small.jpg"
            Image.new("RGB", (699, 900), "white").save(path)
            with self.assertRaisesRegex(ValueError, "最低分辨率"):
                validate_image_paths([path])


class WorkflowApi:
    sandbox = False

    def __init__(self):
        self.cards = []
        self.created_payload = None
        self.uploads = []
        self.price_calls = []
        self.stock_calls = []

    def find_card(self, vendor_code):
        if self.created_payload is None:
            return None
        return {
            "vendorCode": vendor_code,
            "nmID": 456,
            "sizes": [{"chrtID": 789}],
        }

    def generate_barcodes(self, count):
        return ["2030000000012"]

    def create_cards(self, payload):
        self.created_payload = payload
        return {}

    def failed_card_errors(self, _vendor_code):
        return []

    def upload_media(self, nm_id, path, number):
        self.uploads.append((nm_id, Path(path).name, number))
        return {}

    def set_price(self, nm_id, price, discount):
        self.price_calls.append((nm_id, price, discount))
        return 321

    def price_task_status(self, upload_id):
        return {"id": upload_id, "status": 3}

    def price_task_details(self, _upload_id):
        return []

    def update_stock(self, warehouse_id, chrt_id, amount):
        self.stock_calls.append((warehouse_id, chrt_id, amount))
        return {}


class WaitingWorkflowApi(WorkflowApi):
    def find_card(self, _vendor_code):
        return None


class WorkflowTests(unittest.TestCase):
    def _state(self, image_paths):
        inputs = valid_inputs()
        inputs["barcode"] = ""
        return {
            "stage": "new",
            "status": "queued",
            "inputs": inputs,
            "image_paths": image_paths,
        }

    def test_full_workflow_saves_each_external_stage(self):
        with tempfile.TemporaryDirectory() as directory:
            image = Path(directory) / "main.jpg"
            Image.new("RGB", (700, 900), "white").save(image)
            api = WorkflowApi()
            saved = []
            workflow = WbListingWorkflow(
                api, save_state=lambda state: saved.append(copy.deepcopy(state)),
                sleep=lambda _seconds: None,
            )

            result = workflow.advance(
                self._state([str(image)]), card_attempts=1, price_attempts=1,
                sync_min_age_seconds=0,
            )

        self.assertEqual(result["stage"], "completed")
        self.assertEqual(result["nm_id"], 456)
        self.assertEqual(result["chrt_id"], 789)
        self.assertEqual(api.uploads, [(456, "main.jpg", 1)])
        self.assertEqual(api.price_calls, [(456, 999, 10)])
        self.assertEqual(api.stock_calls, [(88, 789, 7)])
        stages = [entry["stage"] for entry in saved]
        self.assertLess(stages.index("card_submitted"), stages.index("card_ready"))
        self.assertLess(stages.index("media_uploaded"), stages.index("price_submitted"))
        self.assertEqual(stages[-1], "completed")

    def test_card_sync_waiting_state_can_be_resumed_without_resubmit(self):
        api = WaitingWorkflowApi()
        saved = []
        workflow = WbListingWorkflow(
            api, save_state=lambda state: saved.append(copy.deepcopy(state)),
            sleep=lambda _seconds: None,
        )
        state = self._state(["unused-until-card-is-ready.jpg"])

        first = workflow.advance(state, card_attempts=1)
        second = workflow.advance(first, card_attempts=1)

        self.assertEqual(first["stage"], "card_submitted")
        self.assertEqual(second["stage"], "card_submitted")
        self.assertEqual(second["status"], "waiting")
        self.assertEqual(api.created_payload[0]["variants"][0]["vendorCode"], "WB-TEST-1")

    def test_new_card_waits_for_cross_service_sync_before_price(self):
        with tempfile.TemporaryDirectory() as directory:
            image = Path(directory) / "main.jpg"
            Image.new("RGB", (700, 900), "white").save(image)
            api = WorkflowApi()
            workflow = WbListingWorkflow(
                api, save_state=lambda _state: None, sleep=lambda _seconds: None,
            )

            result = workflow.advance(
                self._state([str(image)]), card_attempts=1, price_attempts=1,
            )

        self.assertEqual(result["stage"], "media_uploaded")
        self.assertEqual(result["status"], "waiting")
        self.assertIn("同步价格和仓库", result["message"])
        self.assertEqual(api.price_calls, [])
        self.assertEqual(api.stock_calls, [])


if __name__ == "__main__":
    unittest.main()
