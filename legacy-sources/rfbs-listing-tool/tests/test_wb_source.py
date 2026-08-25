from __future__ import annotations

import io
import tempfile
import unittest
from pathlib import Path

from PIL import Image

from wb_source import (
    download_wb_source_images,
    normalize_wb_source,
    parse_wb_source_html,
    prefer_wb_image_url,
)
from wb import WildberriesApi
from wb_ui import WbUiMixin
from wb_pricing import calculate_wb_price


class WbSourceParsingTests(unittest.TestCase):
    def test_normalizes_numeric_id_and_product_url(self):
        expected = "https://www.wildberries.ru/catalog/12345678/detail.aspx"

        self.assertEqual(normalize_wb_source("12345678"), (expected, "12345678"))
        self.assertEqual(
            normalize_wb_source("https://www.wildberries.ru/catalog/12345678/detail.aspx?targetUrl=GP"),
            (expected, "12345678"),
        )

    def test_rejects_non_wildberries_source(self):
        with self.assertRaisesRegex(ValueError, "wildberries.ru"):
            normalize_wb_source("https://www.ozon.ru/product/12345678/")

    def test_parses_product_json_ld_and_upgrades_images(self):
        page = """
        <html><head>
          <script type="application/ld+json">
          {
            "@type": "Product",
            "name": "Полотенце из микрофибры",
            "description": "Мягкое<br>полотенце",
            "brand": {"name": "Clean Home"},
            "image": [
              "https://basket-10.wbbasket.ru/vol123/part12345/12345678/images/c516x688/1.webp",
              "https://basket-10.wbbasket.ru/vol123/part12345/12345678/images/c516x688/2.webp"
            ],
            "additionalProperty": [{"name": "Цвет", "value": "синий"}]
          }
          </script>
        </head></html>
        """

        product = parse_wb_source_html("12345678", page)

        self.assertEqual(product.nm_id, "12345678")
        self.assertEqual(product.title, "Полотенце из микрофибры")
        self.assertEqual(product.description, "Мягкое\nполотенце")
        self.assertEqual(product.brand, "Clean Home")
        self.assertEqual(product.properties, {"Цвет": "синий"})
        self.assertEqual(len(product.images), 2)
        self.assertIn("/images/big/1.webp", product.images[0])

    def test_image_url_filter_is_wb_only(self):
        self.assertEqual(prefer_wb_image_url("https://example.com/images/1.jpg"), "")
        self.assertEqual(
            prefer_wb_image_url(
                "//basket-01.wbbasket.ru/vol1/part1/12345/images/c246x328/1.webp"
            ),
            "https://basket-01.wbbasket.ru/vol1/part1/12345/images/big/1.webp",
        )


class FakeImageResponse:
    status_code = 200

    def __init__(self, content: bytes):
        self.content = content


class FakeImageSession:
    def __init__(self, content: bytes):
        self.content = content
        self.calls = []

    def get(self, url, **kwargs):
        self.calls.append((url, kwargs))
        return FakeImageResponse(self.content)


class WbSourceDownloadTests(unittest.TestCase):
    def test_download_converts_source_image_to_valid_jpeg(self):
        stream = io.BytesIO()
        Image.new("RGB", (900, 1200), "white").save(stream, format="WEBP")
        session = FakeImageSession(stream.getvalue())
        url = "https://basket-01.wbbasket.ru/vol1/part1/12345/images/big/1.webp"

        with tempfile.TemporaryDirectory() as directory:
            paths = download_wb_source_images([url], directory, session=session)
            output = Path(paths[0])
            with Image.open(output) as image:
                size = image.size

        self.assertEqual(size, (900, 1200))
        self.assertEqual(output.name, "01-main.jpg")
        self.assertEqual(session.calls[0][1]["headers"]["Referer"], "https://www.wildberries.ru/")


class WbModeValidationTests(unittest.TestCase):
    @staticmethod
    def valid_inputs(**updates):
        values = {
            "listing_mode": "local",
            "fulfillment_mode": "cross_border",
            "sales_commission_percent": "18",
            "purchase_cost": "15",
            "label_fee": "2",
            "target_profit_margin_percent": "20",
            "advertising_percent": "15",
            "cargo_loss_percent": "10",
            "cny_rub_rate": "12",
            "first_mile_shipping": "50",
            "warehouse_operation_fee": "5",
            "logistics_coefficient_percent": "100",
            "localization_index": "1",
            "subject_id": "123",
            "vendor_code": "WB-MODE-1",
            "title": "测试商品",
            "description": "商品描述",
            "brand": "",
            "barcode": "",
            "length_cm": "10",
            "width_cm": "10",
            "height_cm": "10",
            "weight_kg": "1",
            "tech_size": "0",
            "wb_size": "",
            "price": "999",
            "discount": "0",
            "warehouse_id": "",
            "stock": "0",
            "characteristic_definitions": [],
            "characteristic_values": {},
        }
        values.update(updates)
        return values

    def test_local_and_both_fulfillment_modes_calculate_price(self):
        cross_border = self.valid_inputs()
        overseas = self.valid_inputs(fulfillment_mode="overseas_warehouse")

        WbUiMixin._wb_validate_listing_inputs(cross_border, sandbox=True)
        WbUiMixin._wb_validate_listing_inputs(overseas, sandbox=True)

        self.assertNotEqual(cross_border["price"], "999")
        self.assertNotEqual(overseas["price"], "999")
        self.assertIn("pricing_breakdown", cross_border)
        self.assertIn("pricing_breakdown", overseas)

    def test_manual_commission_is_required(self):
        with self.assertRaisesRegex(ValueError, "人工佣金"):
            WbUiMixin._wb_validate_listing_inputs(
                self.valid_inputs(sales_commission_percent=""), sandbox=True,
            )

    def test_wb_api_no_longer_exposes_commission_lookup(self):
        self.assertFalse(hasattr(WildberriesApi, "commission_report"))
        self.assertFalse(hasattr(WildberriesApi, "commission_for_subject"))

    def test_reading_current_subject_id_does_not_clear_loaded_attributes(self):
        class Variable:
            def __init__(self, value):
                self.value = value

            def get(self):
                return self.value

            def set(self, value):
                self.value = value

        view = object.__new__(WbUiMixin)
        view.vars = {
            "wb_subject_display": Variable("清洁抹布 [123]"),
            "wb_subject_id": Variable("123"),
        }
        view.wb_subject_by_display = {
            "清洁抹布 [123]": {"subjectID": 123, "subjectName": "清洁抹布"},
        }
        view.wb_characteristics = [{"charcID": 10, "name": "颜色"}]
        view.wb_characteristic_values = {"10": ["Синий"]}

        selected = view._wb_selected_subject_id()

        self.assertEqual(selected, 123)
        self.assertEqual(view.wb_characteristic_values, {"10": ["Синий"]})

    def test_follow_mode_requires_wb_source(self):
        with self.assertRaisesRegex(ValueError, "来源链接"):
            WbUiMixin._wb_validate_listing_inputs(
                self.valid_inputs(listing_mode="follow", source_url=""), sandbox=True,
            )

    def test_follow_mode_normalizes_source_identity(self):
        inputs = self.valid_inputs(
            listing_mode="follow",
            source_url="12345678",
            source_nm_id="12345678",
            source_reference={"nm_id": "12345678"},
        )

        WbUiMixin._wb_validate_listing_inputs(inputs, sandbox=True)

        self.assertEqual(
            inputs["source_url"],
            "https://www.wildberries.ru/catalog/12345678/detail.aspx",
        )

    def test_wb_ledger_record_keeps_profit_margin_cost_details(self):
        inputs = self.valid_inputs(fulfillment_mode="overseas_warehouse")
        pricing = calculate_wb_price(
            fulfillment_mode=inputs["fulfillment_mode"],
            purchase_cost_cny=inputs["purchase_cost"],
            label_fee_cny=inputs["label_fee"],
            target_profit_margin_percent=inputs["target_profit_margin_percent"],
            sales_commission_percent=inputs["sales_commission_percent"],
            advertising_percent=inputs["advertising_percent"],
            cargo_loss_percent=inputs["cargo_loss_percent"],
            cny_rub_rate=inputs["cny_rub_rate"],
            weight_kg=inputs["weight_kg"], length_cm=inputs["length_cm"],
            width_cm=inputs["width_cm"], height_cm=inputs["height_cm"],
            first_mile_shipping_cny=inputs["first_mile_shipping"],
            warehouse_operation_fee_cny=inputs["warehouse_operation_fee"],
            logistics_coefficient_percent=inputs["logistics_coefficient_percent"],
            localization_index=inputs["localization_index"],
        )
        inputs["price"] = format(pricing.list_price, "f")
        inputs["pricing_breakdown"] = pricing.json_values()

        record = WbUiMixin._wb_product_ledger_record(
            object(), {"inputs": inputs, "nm_id": 1, "chrt_id": 2},
        )

        self.assertEqual(record["target_profit_margin_percent"], "20")
        self.assertEqual(record["tail_delivery_rub"], "32")
        self.assertEqual(record["first_mile_shipping_cny"], "50")
        self.assertIn("利润率", record["pricing_mode"])

    def test_cross_border_validation_uses_cny_without_exchange_rate(self):
        inputs = self.valid_inputs(cny_rub_rate="")

        WbUiMixin._wb_validate_listing_inputs(inputs, sandbox=True)

        self.assertEqual(inputs["currency_code"], "CNY")
        self.assertEqual(inputs["pricing_breakdown"]["currency_code"], "CNY")


if __name__ == "__main__":
    unittest.main()
