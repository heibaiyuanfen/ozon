from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from PIL import Image

from services import WbCardAiService


class FakeResponse:
    status_code = 200
    text = ""

    def __init__(self, payload):
        self.payload = payload

    def raise_for_status(self):
        return None

    def json(self):
        return self.payload


class FakeSession:
    def __init__(self, payload):
        self.response = FakeResponse(payload)
        self.calls = []

    def post(self, url, **kwargs):
        self.calls.append((url, kwargs))
        return self.response


class WbCardAiServiceTests(unittest.TestCase):
    definitions = [
        {"charcID": 10, "name": "颜色", "required": True, "charcType": 0},
        {"charcID": 20, "name": "数量", "required": False, "charcType": 4},
    ]

    @staticmethod
    def current_document():
        return {
            "subjectID": 123,
            "variants": [{
                "vendorCode": "WB-KEEP-ME",
                "title": "",
                "description": "",
                "characteristics": [],
                "sizes": [{"skus": ["2030000000012"]}],
            }],
        }

    def test_image_ai_uses_live_ids_and_preserves_seller_identifiers(self):
        payload = {
            "subjectID": 123,
            "variants": [{
                "vendorCode": "AI-MUST-NOT-CHANGE",
                "title": "Салфетка для уборки",
                "description": "Мягкая салфетка для ежедневной уборки.",
                "characteristics": [
                    {"id": 10, "value": ["Синий"]},
                    {"id": 20, "value": 3},
                ],
                "sizes": [{
                    "skus": ["AI-MUST-NOT-CHANGE"], "techSize": "M", "wbSize": "44",
                }],
            }],
        }
        session = FakeSession(payload)
        service = WbCardAiService("https://example.test/v1", "secret", "vision-model", session=session)
        with tempfile.TemporaryDirectory() as directory:
            image = Path(directory) / "product.jpg"
            Image.new("RGB", (800, 1000), "blue").save(image)

            result = service.generate(
                subject_id=123, subject_name="清洁抹布", definitions=self.definitions,
                image_paths=[str(image)], current_document=self.current_document(),
            )

        variant = result["variants"][0]
        self.assertEqual(variant["vendorCode"], "WB-KEEP-ME")
        self.assertEqual(variant["sizes"][0]["skus"], ["2030000000012"])
        self.assertEqual(variant["characteristics"][0], {"id": 10, "value": ["Синий"]})
        request_content = session.calls[0][1]["json"]["messages"][1]["content"]
        self.assertTrue(any(item.get("type") == "image_url" for item in request_content))

    def test_image_ai_rejects_characteristic_id_outside_api_schema(self):
        payload = {
            "subjectID": 123,
            "variants": [{
                "title": "Название", "description": "Описание",
                "characteristics": [{"id": 999, "value": ["Ошибка"]}],
                "sizes": [{}],
            }],
        }
        service = WbCardAiService(
            "https://example.test/v1", "secret", "vision-model", session=FakeSession(payload),
        )
        with tempfile.TemporaryDirectory() as directory:
            image = Path(directory) / "product.jpg"
            Image.new("RGB", (800, 1000), "white").save(image)
            with self.assertRaisesRegex(RuntimeError, "类目外属性 ID 999"):
                service.generate(
                    subject_id=123, subject_name="测试类目", definitions=self.definitions,
                    image_paths=[str(image)], current_document=self.current_document(),
                )


if __name__ == "__main__":
    unittest.main()
