import base64
import json
import io
import queue
import sys
import tempfile
import threading
import tkinter as tk
import unittest
from decimal import Decimal
from pathlib import Path
from tkinter import ttk
from unittest.mock import patch

import requests
from PIL import Image, ImageDraw


APP_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(APP_DIR))

from core import ProductInput, ReferenceProduct, attribute_values_by_id, build_import_item, calculate_price_breakdown, calculate_roi_price, calculate_shipping, clean_attribute_payload, extract_hashtags, extract_reference_article, find_reference_fact, flatten_categories, format_ozon_hashtags, normalize_reference_input, parse_attributes, parse_complex_attributes, parse_ozon_hashtags, parse_reference_html, prefer_high_resolution_image_url, rank_categories, recommend_category, scrape_reference, set_attribute_value, unsupported_chinese_submission_fields, validate_ozon_hashtags, validate_required
from services import CopywritingService, ImageDownloadService, ImageGenerationService, OzonApi, OzonDictionaryCache, ProductVisionAnalysisService, WatermarkService, build_oss_object_keys, build_ozon_main_image_prompt, build_ozon_poster_text_lines, normalize_chat_completions_url, normalize_image_edits_url, visual_attribute_allowed
from app import AutoJobCancelled, RfbsListingApp, WORKSPACE_FIELD_KEYS


class CoreTests(unittest.TestCase):
    def test_shipping_formula_matches_spreadsheet_and_price_boundaries(self):
        self.assertEqual(calculate_shipping("85", "0.6"), Decimal("37.332"))
        self.assertEqual(calculate_shipping("134", "0.4"), Decimal("14.638"))
        self.assertEqual(calculate_shipping("135", "0.4"), Decimal("29.238"))
        self.assertEqual(calculate_shipping("134", "0.5"), Decimal("35.415"))
        self.assertEqual(calculate_shipping("135", "2"), Decimal("96.78"))
        with self.assertRaisesRegex(ValueError, "超出运费公式"):
            calculate_shipping("635", "5")

    def test_roi_price_is_minimum_whole_price_that_reaches_target(self):
        breakdown = calculate_roi_price(
            "15", "2", "60", "0.6", sales_commission_percent="12",
        )
        self.assertEqual(breakdown.price, Decimal("99"))
        self.assertEqual(breakdown.shipping, Decimal("37.332"))
        self.assertEqual(breakdown.sales_commission_rate, Decimal("0.12"))
        self.assertEqual(breakdown.invested, Decimal("17"))
        self.assertGreaterEqual(breakdown.roi_percent, Decimal("60"))

        previous_price_profit = (
            Decimal("98") * Decimal("0.71")
            - breakdown.invested - breakdown.shipping - breakdown.cargo_loss
        )
        self.assertLess(previous_price_profit / breakdown.invested * Decimal("100"), Decimal("60"))

    def test_roi_price_rechecks_shipping_after_crossing_price_band(self):
        breakdown = calculate_roi_price(
            "50", "2", "60", "0.6", sales_commission_percent="14",
        )
        self.assertEqual(breakdown.price, Decimal("184"))
        self.assertEqual(breakdown.shipping, Decimal("34.872"))
        self.assertEqual(breakdown.sales_commission_rate, Decimal("0.14"))
        self.assertEqual(breakdown.sales_commission, Decimal("25.76"))
        with self.assertRaisesRegex(ValueError, "超出运费公式"):
            calculate_roi_price(
                "15", "2", "60", "30", sales_commission_percent="14",
            )

    def test_roi_price_uses_manually_entered_commission(self):
        breakdown = calculate_roi_price(
            "50", "2", "60", "0.6", sales_commission_percent="20",
        )
        self.assertEqual(breakdown.price, Decimal("202"))
        self.assertEqual(breakdown.sales_commission_rate, Decimal("0.2"))
        self.assertEqual(breakdown.sales_commission, Decimal("40.4"))
        self.assertGreaterEqual(breakdown.roi_percent, Decimal("60"))

        with self.assertRaisesRegex(ValueError, "填写的销售佣金"):
            calculate_roi_price(
                "50", "2", "60", "0.6", sales_commission_percent="",
            )

    def test_current_price_breakdown_uses_manually_entered_commission(self):
        breakdown = calculate_price_breakdown("112", "15", "2", "0.6", "20")
        self.assertEqual(breakdown.shipping, Decimal("37.332"))
        self.assertEqual(breakdown.sales_commission, Decimal("22.4"))
        self.assertEqual(
            breakdown.roi_percent,
            Decimal("63.49882352941176470588235294"),
        )

    def test_adjustable_advertising_and_cargo_loss_rates_affect_pricing(self):
        default = calculate_roi_price(
            "15", "2", "60", "0.6", sales_commission_percent="12",
        )
        adjusted = calculate_roi_price(
            "15", "2", "60", "0.6",
            sales_commission_percent="12",
            advertising_percent="20", cargo_loss_percent="15",
        )
        self.assertGreater(adjusted.price, default.price)
        current = calculate_price_breakdown(
            "100", "15", "2", "0.6", "12",
            advertising_percent="20", cargo_loss_percent="5",
        )
        self.assertEqual(current.advertising, Decimal("20"))
        self.assertEqual(current.cargo_loss, Decimal("2.71660"))
        with self.assertRaisesRegex(ValueError, "广告费率必须小于 100%"):
            calculate_roi_price(
                "15", "2", "60", "0.6", sales_commission_percent="12",
                advertising_percent="100",
            )
        with self.assertRaisesRegex(ValueError, "货损率不能小于 0"):
            calculate_roi_price(
                "15", "2", "60", "0.6", sales_commission_percent="12",
                cargo_loss_percent="-1",
            )

    def test_job_pricing_uses_adjustable_expense_rates(self):
        default = RfbsListingApp._job_price_breakdown({
            "purchase_cost": "15", "label_fee": "2", "target_roi": "60",
            "sales_commission_percent": "12", "weight": "600", "fbp_pricing": "0",
        })
        adjusted = RfbsListingApp._job_price_breakdown({
            "purchase_cost": "15", "label_fee": "2", "target_roi": "60",
            "advertising_percent": "20", "cargo_loss_percent": "15",
            "sales_commission_percent": "12", "weight": "600", "fbp_pricing": "0",
        })
        self.assertGreater(adjusted.price, default.price)

    def test_fbp_pricing_removes_label_fee_and_reduces_commission_one_point(self):
        breakdown = calculate_roi_price(
            "15", "0", "60", "0.6",
            sales_commission_percent="12",
            sales_commission_discount_percent="1",
        )
        self.assertEqual(breakdown.price, Decimal("93"))
        self.assertEqual(breakdown.sales_commission_rate, Decimal("0.11"))
        self.assertEqual(breakdown.invested, Decimal("15"))
        self.assertGreaterEqual(breakdown.roi_percent, Decimal("60"))

        actual = calculate_price_breakdown(
            "200", "30", "0", "0.6", "20",
            sales_commission_discount_percent="1",
        )
        self.assertEqual(actual.sales_commission_rate, Decimal("0.19"))
        self.assertEqual(actual.invested, Decimal("30"))

    def test_job_fbp_pricing_ignores_entered_label_fee(self):
        breakdown = RfbsListingApp._job_price_breakdown({
            "purchase_cost": "15", "label_fee": "99", "target_roi": "60",
            "sales_commission_percent": "12", "weight": "600", "fbp_pricing": "1",
        })
        self.assertEqual(breakdown.price, Decimal("93"))
        self.assertEqual(breakdown.invested, Decimal("15"))
        self.assertEqual(breakdown.sales_commission_rate, Decimal("0.11"))

    def test_fbp_checkbox_locks_zero_label_fee_and_restores_normal_value(self):
        class Value:
            def __init__(self, value):
                self.value = str(value)

            def get(self):
                return self.value

            def set(self, value):
                self.value = str(value)

        class Entry:
            def __init__(self):
                self.states = []

            def state(self, values):
                self.states.append(tuple(values))

        app = RfbsListingApp.__new__(RfbsListingApp)
        app.job_form_vars = {
            "fbp_pricing": Value("1"), "label_fee": Value("3"),
        }
        app.job_label_fee_entry = Entry()
        app._normal_label_fee_before_fbp = "2"
        app._update_job_price = lambda *_args: None

        app._on_fbp_pricing_changed()
        self.assertEqual(app.job_form_vars["label_fee"].get(), "0")
        self.assertEqual(app._normal_label_fee_before_fbp, "3")
        self.assertEqual(app.job_label_fee_entry.states[-1], ("disabled",))

        app.job_form_vars["fbp_pricing"].set("0")
        app._on_fbp_pricing_changed()
        self.assertEqual(app.job_form_vars["label_fee"].get(), "3")
        self.assertEqual(app.job_label_fee_entry.states[-1], ("!disabled",))

    def test_parses_product_json_ld_without_browser(self):
        page = '<script type="application/ld+json">{"@type":"Product","name":"Люстра","description":"LED #люстра #светодиодный_светильник","image":["https://cdn/x.jpg"],"additionalProperty":[{"name":"Мощность","value":"60 Вт"}]}</script>'
        product = parse_reference_html("https://ozon.ru/product/x", page)
        self.assertEqual(product.title, "Люстра")
        self.assertEqual(product.images, ["https://cdn/x.jpg"])
        self.assertEqual(product.properties, {"Мощность": "60 Вт"})
        self.assertEqual(product.tags, ["#люстра", "#светодиодный_светильник"])

    def test_reads_hashtags_outside_json_ld_and_decodes_unicode_escapes(self):
        page = '''
        <script type="application/ld+json">{"@type":"Product","name":"Люстра","description":"LED","image":[]}</script>
        <div data-widget="webSeoTags">#люстра #свет_для_дома</div>
        <script type="application/json">{"tags":"#\\u043f\\u043e\\u0442\\u043e\\u043b\\u043e\\u0447\\u043d\\u044b\\u0439_\\u0441\\u0432\\u0435\\u0442"}</script>
        <style>.x{color:#fff;background:#F0F2F5}</style>
        '''
        product = parse_reference_html("https://ozon.ru/product/123456789/", page)
        self.assertEqual(product.tags, ["#люстра", "#свет_для_дома", "#потолочный_свет"])

    def test_hashtag_parser_ignores_css_colors_and_page_anchors(self):
        self.assertEqual(
            extract_hashtags("#люстра #F0F2F5 #reviews #LED_свет"),
            ["#люстра", "#LED_свет"],
        )

    def test_accepts_numeric_or_labelled_ozon_article(self):
        expected = "https://www.ozon.ru/product/2379505289/"
        self.assertEqual(normalize_reference_input("2379505289"), expected)
        self.assertEqual(normalize_reference_input("Артикул: 2379505289"), expected)
        self.assertEqual(extract_reference_article("https://www.ozon.ru/product/lampa-2379505289/?sh=x"), "2379505289")

    def test_numeric_article_is_opened_as_single_variant_and_recorded(self):
        class Response:
            status_code = 200
            url = "https://www.ozon.ru/product/potolochnyy-svetilnik-2379505289/"
            text = '<script type="application/ld+json">{"@type":"Product","name":"Светильник","image":["https://cdn/x.jpg"]}</script>'
            def raise_for_status(self):
                return None

        class Session:
            requested_url = ""
            def get(self, url, **_kwargs):
                self.requested_url = url
                return Response()

        session = Session()
        product = scrape_reference("Артикул: 2379505289", session=session, browser_fallback=False)
        self.assertEqual(session.requested_url, "https://www.ozon.ru/product/2379505289/")
        self.assertEqual(product.properties["Артикул"], "2379505289")
        self.assertEqual(product.source_url, "https://www.ozon.ru/product/2379505289/")

    def test_direct_page_with_title_but_no_images_uses_browser_fallback(self):
        class Response:
            status_code = 200
            url = "https://www.ozon.ru/product/3389874670/"
            text = '<script type="application/ld+json">{"@type":"Product","name":"Игрушка"}</script>'
            def raise_for_status(self):
                return None
        class Session:
            def get(self, *_args, **_kwargs):
                return Response()
        browser_product = ReferenceProduct(
            "https://www.ozon.ru/product/3389874670/", "Игрушка", images=["https://ir.ozone.ru/s3/multimedia/a.jpg"],
        )
        logs = []
        with patch("core.scrape_reference_browser", return_value=browser_product) as fallback:
            product = scrape_reference(
                "3389874670", session=Session(), browser_fallback=True, log_func=logs.append,
            )
        self.assertEqual(product.images, browser_product.images)
        fallback.assert_called_once()
        self.assertIn("图库为空", logs[0])

    def test_failed_new_product_image_download_keeps_previous_product_state(self):
        class Value:
            def get(self):
                return "3389874670"
        app = RfbsListingApp.__new__(RfbsListingApp)
        previous = ReferenceProduct("https://www.ozon.ru/product/111111/", "Previous", images=["https://cdn/old.jpg"])
        current = ReferenceProduct("https://www.ozon.ru/product/3389874670/", "Current", images=["https://cdn/new.jpg"])
        app.reference = previous
        app.vars = {"source_url": Value()}
        app._log = lambda _message: None
        with patch("app.scrape_reference", return_value=current):
            app._download_reference_images = lambda _reference: (_ for _ in ()).throw(RuntimeError("download failed"))
            with self.assertRaisesRegex(RuntimeError, "download failed"):
                app._parse_url()
        self.assertIs(app.reference, previous)

    def test_flattens_current_official_leaf_shape_and_excludes_disabled(self):
        tree = [{
            "description_category_id": 100,
            "category_name": "Освещение",
            "children": [
                {"type_id": 200, "type_name": "Люстры", "children": []},
                {"type_id": 201, "type_name": "Лампы", "disabled": True, "children": []},
            ],
        }]
        result = flatten_categories(tree)
        self.assertEqual(len(result), 1)
        self.assertEqual(result[0]["description_category_id"], 100)
        self.assertEqual(result[0]["type_id"], 200)

    def test_recommends_russian_category_across_singular_plural_endings(self):
        categories = [
            {"display": "Одежда / Рюкзаки", "name": "Рюкзаки"},
            {"display": "Дом / Люстры", "name": "Люстры"},
        ]
        self.assertEqual(recommend_category("Черный городской рюкзак", categories)["name"], "Рюкзаки")
        self.assertEqual(recommend_category("Потолочная светодиодная люстра", categories)["name"], "Люстры")
        self.assertEqual(rank_categories("городской рюкзак", categories, 2)[0]["name"], "Рюкзаки")

    def test_russian_matching_works_with_chinese_category_display(self):
        categories = [{
            "display": "服装 / 双肩包", "display_ru": "Одежда / Рюкзаки", "name": "双肩包",
        }]
        self.assertEqual(recommend_category("Черный городской рюкзак", categories)["name"], "双肩包")

    def test_category_filter_matches_chinese_russian_and_full_paths(self):
        categories = [{
            "description_category_id": 100,
            "type_id": 200,
            "name": "双肩包",
            "name_ru": "Рюкзаки",
            "display": "服饰 / 箱包 / 双肩包",
            "display_ru": "Одежда / Сумки / Рюкзаки",
            "path": ["服饰", "箱包", "双肩包"],
            "path_ru": ["Одежда", "Сумки", "Рюкзаки"],
        }, {
            "description_category_id": 101,
            "type_id": 201,
            "name": "吊灯",
            "name_ru": "Люстры",
            "display": "家居 / 照明 / 吊灯",
            "display_ru": "Дом / Освещение / Люстры",
            "path": ["家居", "照明", "吊灯"],
            "path_ru": ["Дом", "Освещение", "Люстры"],
        }]
        displays = [item["display"] for item in categories]
        search_index = {
            item["display"]: RfbsListingApp._category_search_text(item)
            for item in categories
        }
        matcher = RfbsListingApp._matching_category_displays
        self.assertEqual(matcher("双肩", displays, search_index), [displays[0]])
        self.assertEqual(matcher("рюкзак", displays, search_index), [displays[0]])
        self.assertEqual(matcher("服饰 сумки", displays, search_index), [displays[0]])
        self.assertEqual(matcher("люстры", displays, search_index), [displays[1]])
        self.assertEqual(matcher("", displays, search_index), displays)

    def test_category_cache_round_trip_populates_local_choices(self):
        import app as app_module

        class Value:
            def __init__(self, value=""):
                self.value = value
            def get(self):
                return self.value

        class Combo(Value):
            def __init__(self):
                super().__init__("")
                self.values = []
            def configure(self, **kwargs):
                if "values" in kwargs:
                    self.values = list(kwargs["values"])

        class Root:
            @staticmethod
            def after(_delay, callback):
                callback()

        categories = [{
            "description_category_id": "100",
            "type_id": "200",
            "name": "双肩包",
            "name_ru": "Рюкзаки",
            "display": "箱包 / 双肩包",
            "display_ru": "Сумки / Рюкзаки",
            "path": ["箱包", "双肩包"],
            "path_ru": ["Сумки", "Рюкзаки"],
        }]
        with tempfile.TemporaryDirectory() as folder:
            cache_path = Path(folder) / "ozon-categories.json"
            with patch.object(app_module, "CATEGORY_CACHE_PATH", cache_path):
                writer = object.__new__(RfbsListingApp)
                writer._save_category_cache(categories)
                payload = json.loads(cache_path.read_text(encoding="utf-8"))
                self.assertEqual(payload["languages"], ["ZH_HANS", "RU"])

                reader = object.__new__(RfbsListingApp)
                reader.root = Root()
                reader.category_combo = Combo()
                reader.work_category_combo = Combo()
                reader.vars = {"category_display": Value()}
                reader.job_form_vars = {"category_display": Value()}
                reader.categories = []
                reader.category_displays = []
                reader.category_by_display = {}
                reader.category_search_text_by_display = {}
                reader._log = lambda _message: None

                self.assertTrue(reader._load_category_cache())
                self.assertEqual(reader.category_displays, ["箱包 / 双肩包"])
                self.assertEqual(reader.category_combo.values, ["箱包 / 双肩包"])
                self.assertEqual(reader.work_category_combo.values, ["箱包 / 双肩包"])
                self.assertIn("рюкзаки", reader.category_search_text_by_display["箱包 / 双肩包"])

    def test_attribute_assistant_uses_only_traceable_page_facts(self):
        reference = ReferenceProduct(
            "https://ozon.ru/product/x", "Рюкзак",
            properties={"Бренд": "Ritter", "页面商品参数": "Основные\nЦвет товара\nЧерный\nМатериал\nПолиэстер"},
        )
        self.assertEqual(find_reference_fact("Бренд товара", reference), ("Бренд", "Ritter"))
        self.assertEqual(find_reference_fact("Цвет", reference), ("Цвет товара", "Черный"))
        self.assertIsNone(find_reference_fact("Страна производства", reference))

    def test_attribute_assistant_groups_complex_values_and_cleans_ui_metadata(self):
        attributes = []
        complex_attributes = []
        simple_definition = {"id": 1, "name": "Бренд", "attribute_complex_id": 0}
        complex_definition = {"id": 2, "name": "Размер", "attribute_complex_id": 77}
        set_attribute_value(attributes, complex_attributes, simple_definition, value="Ritter", dictionary_value_id=42, source="page")
        set_attribute_value(attributes, complex_attributes, complex_definition, value="40 см", source="page")
        values = attribute_values_by_id(attributes, complex_attributes)
        self.assertEqual(values[1]["values"][0]["dictionary_value_id"], 42)
        self.assertEqual(values[2]["complex_id"], 77)
        cleaned = clean_attribute_payload(complex_attributes)
        self.assertNotIn("_complex_id", cleaned[0])
        self.assertNotIn("_name", cleaned[0]["attributes"][0])
        self.assertNotIn("_source", cleaned[0]["attributes"][0])

    def test_dictionary_collection_can_append_distinct_live_values(self):
        attributes = []
        definition = {"id": 7, "name": "Цвет", "is_collection": True, "max_value_count": 2}
        set_attribute_value(attributes, [], definition, value="Черный", dictionary_value_id=10, append=True)
        set_attribute_value(attributes, [], definition, value="Синий", dictionary_value_id=11, append=True)
        set_attribute_value(attributes, [], definition, value="Синий", dictionary_value_id=11, append=True)
        self.assertEqual(len(attributes[0]["values"]), 2)
        with self.assertRaises(ValueError):
            set_attribute_value(attributes, [], definition, value="Красный", dictionary_value_id=12, append=True)

    def test_payload_uses_current_product_import_shape(self):
        product = ProductInput("2999", 2000, 450, 450, 120, 100, 200, "TEST-1", old_price="3999")
        reference = ReferenceProduct("https://ozon.ru/product/x", "Люстра Ritter", "Описание")
        item = build_import_item(product, reference, ["https://cdn/main.jpg", "https://cdn/2.jpg"], [])
        self.assertEqual(item["primary_image"], "https://cdn/main.jpg")
        self.assertEqual(item["images"], ["https://cdn/2.jpg"])
        self.assertNotIn("images360", item)
        self.assertEqual(item["offer_id"], "TEST-1")
        self.assertEqual(item["price"], "2999")
        self.assertEqual(item["old_price"], "3999")
        with self.assertRaisesRegex(ValueError, "折扣前价格必须大于"):
            build_import_item(
                ProductInput("2999", 2000, 450, 450, 120, 100, 200, "TEST-2", old_price="2999"),
                reference, ["https://cdn/main.jpg"], [],
            )

    def test_formats_ozon_hashtags_with_hash_underscore_and_spaces(self):
        value = format_ozon_hashtags([
            "рюкзак", "туристический рюкзак", "#походныйрюкзак", "рюкзак для путешествий",
        ])
        self.assertEqual(
            value,
            "#рюкзак #туристический_рюкзак #походныйрюкзак #рюкзак_для_путешествий",
        )
        self.assertEqual(parse_ozon_hashtags(value), value.split())

    def test_hashtags_shorten_at_word_boundaries_and_require_25_to_30(self):
        formatted = format_ozon_hashtags(["прямоугольная форма для выпечки"])
        self.assertEqual(formatted, "#прямоугольная_форма")
        valid = [f"поисковый тег {index}" for index in range(25)]
        tags = validate_ozon_hashtags(valid)
        self.assertEqual(len(tags), 25)
        self.assertTrue(all(len(tag) <= 30 for tag in tags))
        with self.assertRaisesRegex(ValueError, "25–30"):
            validate_ozon_hashtags(valid[:24])

    def test_required_attributes_must_have_real_values(self):
        definitions = [{"id": 1, "name": "Бренд", "is_required": True}]
        self.assertEqual(validate_required([], definitions), ["Бренд"])
        attributes = parse_attributes(json.dumps([{"id": 1, "values": [{"dictionary_value_id": 42, "value": "Ritter"}]}]))
        self.assertEqual(validate_required(attributes, definitions), [])

    def test_complex_required_attribute_is_validated_and_sent(self):
        definitions = [{"id": 9, "name": "Вложенная характеристика", "is_required": True, "attribute_complex_id": 77}]
        complex_attributes = parse_complex_attributes(json.dumps([{"attributes": [
            {"complex_id": 77, "id": 9, "values": [{"value": "значение"}]}
        ]}]))
        self.assertEqual(validate_required([], definitions, complex_attributes), [])
        product = ProductInput("100", 10, 10, 10, 10, 1, 2, "X")
        item = build_import_item(product, ReferenceProduct("https://ozon.ru/x", "X"), ["https://cdn/x.jpg"], [], complex_attributes)
        self.assertEqual(item["complex_attributes"][0]["attributes"][0]["complex_id"], 77)

    def test_watermark_is_independent_and_outputs_jpeg(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source.png"
            mark = root / "mark.png"
            Image.new("RGB", (300, 300), "white").save(source)
            Image.new("RGBA", (80, 30), (255, 0, 0, 200)).save(mark)
            outputs = WatermarkService.apply([str(source)], str(mark), str(root / "out"))
            self.assertEqual(len(outputs), 1)
            with Image.open(outputs[0]) as result:
                self.assertEqual(result.format, "JPEG")
                self.assertEqual(result.size, (300, 300))

    def test_image_downloader_keeps_order_and_deduplicates_pixels(self):
        def image_bytes(color):
            data = io.BytesIO()
            Image.new("RGB", (800, 900), color).save(data, "PNG")
            return data.getvalue()

        class Response:
            def __init__(self, content):
                self.content = content
            def raise_for_status(self):
                return None

        class Session:
            def __init__(self):
                self.values = [image_bytes("red"), image_bytes("red"), image_bytes("blue")]
            def get(self, _url, **_kwargs):
                return Response(self.values.pop(0))

        with tempfile.TemporaryDirectory() as temporary:
            outputs = ImageDownloadService(Session()).download(
                ["https://cdn/1.png", "https://cdn/duplicate.png", "https://cdn/2.png"], temporary
            )
            self.assertEqual(len(outputs), 2)
            self.assertTrue(Path(outputs[0]).name.startswith("01_"))
            self.assertTrue(Path(outputs[1]).name.startswith("02_"))

    def test_image_downloader_deduplicates_same_visual_at_different_resolution(self):
        def image_bytes(size, image_format, quality=None):
            image = Image.new("RGB", size, "white")
            draw = ImageDraw.Draw(image)
            draw.rectangle((size[0] * 0.2, size[1] * 0.2, size[0] * 0.8, size[1] * 0.8), fill=(30, 30, 30))
            draw.ellipse((size[0] * 0.35, size[1] * 0.3, size[0] * 0.65, size[1] * 0.7), fill=(220, 40, 40))
            data = io.BytesIO()
            options = {"quality": quality} if quality else {}
            image.save(data, image_format, **options)
            return data.getvalue()

        class Response:
            def __init__(self, content):
                self.content = content
            def raise_for_status(self):
                return None

        class Session:
            def __init__(self):
                self.values = [image_bytes((800, 900), "PNG"), image_bytes((1200, 1350), "JPEG", 88)]
            def get(self, _url, **_kwargs):
                return Response(self.values.pop(0))

        with tempfile.TemporaryDirectory() as temporary:
            outputs = ImageDownloadService(Session()).download(
                ["https://cdn/first.png", "https://cdn/same-view.jpg"], temporary
            )
            self.assertEqual(len(outputs), 1)

    def test_ozon_thumbnail_url_is_upgraded_to_2000px_variant(self):
        source = "https://ir.ozone.ru/s3/multimedia-1-4/wc100/7243192120.jpg"
        self.assertEqual(
            prefer_high_resolution_image_url(source),
            "https://ir.ozone.ru/s3/multimedia-1-4/wc2000/7243192120.jpg",
        )

    def test_image_downloader_rejects_thumbnail_dimensions(self):
        data = io.BytesIO()
        Image.new("RGB", (100, 100), "red").save(data, "PNG")

        class Response:
            content = data.getvalue()
            def raise_for_status(self):
                return None

        class Session:
            def get(self, _url, **_kwargs):
                return Response()

        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaisesRegex(RuntimeError, "短边不足 600"):
                ImageDownloadService(Session()).download(["https://cdn/thumbnail.png"], temporary)

    def test_copywriting_uses_source_facts_and_parses_strict_json(self):
        class Response:
            status_code = 200
            text = ""
            def raise_for_status(self):
                return None
            def json(self):
                return {"choices": [{"message": {"content": json.dumps({
                    "title_ru": "Люстра LED 60 Вт",
                    "description_ru": "Потолочная светодиодная люстра.",
                    "tags_ru": ["люстра", "светодиодная люстра"] + [
                        f"освещение дома {index}" for index in range(1, 24)
                    ],
                }, ensure_ascii=False)}}]}

        class Session:
            payload = None
            url = None
            def post(self, url, **kwargs):
                self.url = url
                self.payload = kwargs["json"]
                return Response()

        session = Session()
        result = CopywritingService("https://api.example", "key", "model", session=session).generate(
            ReferenceProduct("https://ozon.ru/product/x", "Люстра", "LED", [], {"Мощность": "60 Вт"}, ["#люстра"])
        )
        self.assertEqual(result["title_ru"], "Люстра LED 60 Вт")
        self.assertEqual(session.url, "https://api.example/v1/chat/completions")
        self.assertFalse(session.payload["stream"])
        self.assertIn("60 Вт", session.payload["messages"][1]["content"])
        self.assertIn("#люстра", session.payload["messages"][1]["content"])
        self.assertIn("25-30 unique Russian search phrases", session.payload["messages"][1]["content"])
        self.assertEqual(len(result["tags_ru"]), 25)

    def test_copywriting_accepts_wrapped_json_and_common_key_aliases(self):
        class Response:
            status_code = 200
            text = ""
            def raise_for_status(self):
                return None
            def json(self):
                content = {
                    "title": "Люстра LED",
                    "description": "Описание.",
                    "tags": "; ".join(f"поисковый тег {index}" for index in range(25)),
                }
                return {"output_text": "Готово:\n```json\n" + json.dumps(content, ensure_ascii=False) + "\n```"}

        class Session:
            def post(self, _url, **_kwargs):
                return Response()

        result = CopywritingService("https://api.example/v1", "key", "model", session=Session()).generate(
            ReferenceProduct("https://ozon.ru/product/x", "Люстра")
        )
        self.assertEqual(result["title_ru"], "Люстра LED")
        self.assertEqual(result["description_ru"], "Описание.")
        self.assertEqual(result["tags_ru"], [f"поисковый тег {index}" for index in range(25)])
        self.assertEqual(normalize_chat_completions_url("https://api.gpt.ge/"), "https://api.gpt.ge/v1/chat/completions")

    def test_copywriting_rejects_fewer_than_25_normalized_tags(self):
        class Response:
            status_code = 200
            text = ""
            def raise_for_status(self):
                return None
            def json(self):
                return {"choices": [{"message": {"content": json.dumps({
                    "title_ru": "Люстра LED",
                    "description_ru": "Описание.",
                    "tags_ru": [f"тег {index}" for index in range(24)],
                }, ensure_ascii=False)}}]}

        class Session:
            def post(self, _url, **_kwargs):
                return Response()

        with self.assertRaisesRegex(RuntimeError, "25–30"):
            CopywritingService(
                "https://api.example", "key", "model", session=Session(),
            ).generate(ReferenceProduct("https://ozon.ru/product/x", "Люстра"))

    def test_ai_category_choice_is_restricted_to_live_candidates(self):
        class Response:
            status_code = 200
            text = ""
            def __init__(self, payload):
                self.payload = payload
            def raise_for_status(self):
                return None
            def json(self):
                return self.payload

        class Session:
            def __init__(self):
                self.responses = [
                    {"product_type_ru": ["городской рюкзак", "рюкзак"]},
                    {"description_category_id": 100, "type_id": 200, "reason_ru": "Это рюкзак"},
                ]
            def post(self, _url, **_kwargs):
                return Response(self.responses.pop(0))

        categories = [
            {"description_category_id": 100, "type_id": 200, "name": "Рюкзаки", "display": "Сумки / Рюкзаки"},
            {"description_category_id": 101, "type_id": 201, "name": "Люстры", "display": "Свет / Люстры"},
        ]
        selected, keywords = CopywritingService(
            "https://api.example", "key", "model", session=Session(),
        ).choose_category(ReferenceProduct("https://ozon.ru/x", "Черный городской рюкзак"), categories)
        self.assertEqual((selected["description_category_id"], selected["type_id"]), (100, 200))
        self.assertIn("рюкзак", keywords)
        invalid_session = Session()
        invalid_session.responses[1] = {"description_category_id": 999, "type_id": 999}
        with self.assertRaisesRegex(RuntimeError, "候选列表以外"):
            CopywritingService(
                "https://api.example", "key", "model", session=invalid_session,
            ).choose_category(ReferenceProduct("https://ozon.ru/x", "Черный городской рюкзак"), categories)

    def test_ai_attribute_mapping_and_dictionary_choice_reject_unknown_ids(self):
        class Response:
            status_code = 200
            text = ""
            def __init__(self, payload):
                self.payload = payload
            def raise_for_status(self):
                return None
            def json(self):
                return self.payload

        class Session:
            def __init__(self):
                self.responses = [
                    {"attributes": [
                        {"id": 1, "values": ["Черный", "Синий"], "evidence": "Цвет", "inferred": False},
                        {"id": 999, "values": ["invented"]},
                    ]},
                    {"selections": [
                        {"attribute_id": 1, "dictionary_value_ids": [10, 9999]},
                        {"attribute_id": 999, "dictionary_value_ids": [10]},
                    ]},
                ]
            def post(self, _url, **_kwargs):
                return Response(self.responses.pop(0))

        service = CopywritingService("https://api.example", "key", "model", session=Session())
        mapped = service.map_attributes(
            ReferenceProduct("https://ozon.ru/x", "Рюкзак", properties={"Цвет": "Черный"}),
            {"title_ru": "Рюкзак"},
            [{"id": 1, "name": "Цвет", "is_collection": False, "dictionary_id": 5}],
        )
        self.assertEqual(mapped, [{"id": 1, "values": ["Черный"], "evidence": "Цвет", "inferred": False}])
        chosen = service.choose_dictionary_values([{
            "attribute_id": 1, "attribute_name": "Цвет", "source_values": ["Черный"],
            "collection": False, "options": [{"id": 10, "value": "Черный"}],
        }])
        self.assertEqual(chosen, {1: [10]})

    def test_small_ozon_dictionary_is_fully_exposed_to_selection_ai(self):
        options = [
            {"id": 91945, "value": "装饰石材"},
            {"id": 93652, "value": "家用喷泉"},
            {"id": 970959847, "value": "室内装饰品"},
        ]
        selected = RfbsListingApp._dictionary_options_for_ai(
            options, ["空气凤梨"],
        )
        self.assertEqual(selected, options)
        self.assertTrue(all(item in options for item in selected))

        large = [
            {"id": index, "value": f"普通选项 {index}"}
            for index in range(1, 601)
        ]
        large[499] = {"id": 500, "value": "悬挂式吊架"}
        ranked = RfbsListingApp._dictionary_options_for_ai(
            large, ["悬挂式"],
        )
        self.assertLessEqual(len(ranked), 80)
        self.assertIn(500, {item["id"] for item in ranked})
        self.assertTrue(all(item in large for item in ranked))

    def test_dictionary_selection_ai_batches_real_options_and_returns_only_allowed_ids(self):
        class Response:
            status_code = 200
            text = ""
            def __init__(self, payload):
                self.payload = payload
            def raise_for_status(self):
                return None
            def json(self):
                return self.payload

        class Session:
            def __init__(self):
                self.calls = []
            def post(self, _url, **kwargs):
                self.calls.append(kwargs["json"])
                attribute_id = len(self.calls)
                return Response({"selections": [{
                    "attribute_id": attribute_id,
                    "dictionary_value_ids": [attribute_id * 1000 + 1, 999999],
                }]})

        session = Session()
        service = CopywritingService(
            "https://api.example", "key", "model", session=session,
        )
        requests_ = []
        for attribute_id in (1, 2):
            requests_.append({
                "attribute_id": attribute_id,
                "attribute_name": f"Attribute {attribute_id}",
                "source_values": ["internal search phrase"],
                "collection": False,
                "options": [{
                    "id": attribute_id * 1000 + index,
                    "value": ("Allowed option " + str(index) + " ") * 4,
                } for index in range(1, 301)],
            })
        chosen = service.choose_dictionary_values(requests_)
        self.assertEqual(chosen, {1: [1001], 2: [2001]})
        self.assertEqual(len(session.calls), 2)
        for payload in session.calls:
            prompt = payload["messages"][1]["content"]
            self.assertIn("source_values", prompt)
            self.assertIn("They are NOT allowed values", prompt)

    def test_vision_analysis_reads_multiple_images_and_filters_bad_evidence(self):
        class Response:
            status_code = 200
            text = ""
            def raise_for_status(self):
                return None
            def json(self):
                return {
                    "product_name_ru": "Игрушка для собак",
                    "poster_text_lines_ru": [
                        "Игрушка для собак",
                        "Заметная фактура",
                        "中文应过滤",
                    ],
                    "art_direction_en": "Bold editorial poster with a dark teal text zone.",
                    "visual_facts": [
                        {
                            "fact_name_ru": "Цвет",
                            "value_ru": "Бежевый и коричневый",
                            "reference_index": 2,
                            "evidence_ru": "На игрушке видны бежевые и коричневые детали",
                            "confidence": 0.96,
                        },
                        {
                            "fact_name_ru": "Форма",
                            "value_ru": "Фигура утки",
                            "reference_index": 9,
                            "evidence_ru": "Неверный номер изображения",
                            "confidence": 0.99,
                        },
                        {
                            "fact_name_ru": "Узор",
                            "value_ru": "Полосатый",
                            "reference_index": 1,
                            "evidence_ru": "Слабое предположение",
                            "confidence": 0.4,
                        },
                    ],
                }

        class Session:
            url = ""
            payload = None
            def post(self, url, **kwargs):
                self.url = url
                self.payload = kwargs["json"]
                return Response()

        with tempfile.TemporaryDirectory() as temporary:
            first = Path(temporary) / "first.png"
            second = Path(temporary) / "second.png"
            Image.new("RGB", (80, 60), "beige").save(first)
            Image.new("RGB", (60, 80), "brown").save(second)
            session = Session()
            result = ProductVisionAnalysisService(
                "https://api.example/v1", "key", "vision-model", session=session,
            ).analyze(
                ReferenceProduct("https://ozon.ru/x", "Игрушка"),
                [str(first), str(second)],
                listing_content={"title_ru": "Игрушка для собак «Утка»"},
            )

        user_content = session.payload["messages"][1]["content"]
        image_parts = [item for item in user_content if item.get("type") == "image_url"]
        self.assertEqual(session.url, "https://api.example/v1/chat/completions")
        self.assertEqual(len(image_parts), 2)
        self.assertIn("Игрушка для собак «Утка»", user_content[0]["text"])
        self.assertTrue(image_parts[0]["image_url"]["url"].startswith("data:image/jpeg;base64,"))
        self.assertEqual(result["poster_text_lines_ru"], ["Игрушка для собак", "Заметная фактура"])
        self.assertEqual(result["analyzed_image_count"], 2)
        self.assertEqual(len(result["visual_facts"]), 1)
        self.assertEqual(result["visual_facts"][0]["reference_index"], 2)

    def test_image_attribute_mapping_requires_traceable_high_confidence_and_safe_field(self):
        class Response:
            status_code = 200
            text = ""
            def raise_for_status(self):
                return None
            def json(self):
                return {"attributes": [
                    {
                        "id": 1,
                        "values": ["Бежевый"],
                        "evidence_type": "image",
                        "visual_fact_index": 1,
                    },
                    {
                        "id": 2,
                        "values": ["Хлопок"],
                        "evidence_type": "image",
                        "visual_fact_index": 1,
                    },
                    {
                        "id": 3,
                        "values": ["Овальная"],
                        "evidence_type": "image",
                        "visual_fact_index": 2,
                    },
                ]}

        visual_analysis = {"visual_facts": [
            {
                "fact_name_ru": "Цвет",
                "value_ru": "Бежевый",
                "reference_index": 2,
                "evidence_ru": "Основная поверхность изделия бежевая",
                "confidence": 0.96,
            },
            {
                "fact_name_ru": "Форма",
                "value_ru": "Овальная",
                "reference_index": 1,
                "evidence_ru": "Контур выглядит овальным",
                "confidence": 0.7,
            },
        ]}
        definitions = [
            {"id": 1, "name": "颜色", "name_ru": "Цвет", "is_collection": False, "dictionary_id": 5},
            {"id": 2, "name": "材质", "name_ru": "Материал", "is_collection": False, "dictionary_id": 0},
            {"id": 3, "name": "形状", "name_ru": "Форма", "is_collection": False, "dictionary_id": 5},
        ]
        service = CopywritingService("https://api.example", "key", "model", session=type(
            "Session", (), {"post": lambda _self, _url, **_kwargs: Response()},
        )())
        mapped = service.map_attributes(
            ReferenceProduct("https://ozon.ru/x", "Игрушка"),
            {"title_ru": "Игрушка"},
            definitions,
            visual_analysis=visual_analysis,
        )
        self.assertEqual(len(mapped), 1)
        self.assertEqual(mapped[0]["id"], 1)
        self.assertEqual(mapped[0]["evidence_type"], "image")
        self.assertEqual(mapped[0]["reference_index"], 2)
        self.assertEqual(mapped[0]["confidence"], 0.96)
        self.assertIn("图片2", mapped[0]["evidence"])
        self.assertTrue(visual_attribute_allowed(definitions[0]))
        self.assertFalse(visual_attribute_allowed(definitions[1]))

    def test_image_generation_normalizes_base_url_and_accepts_base64_image(self):
        generated_bytes = io.BytesIO()
        Image.new("RGB", (64, 64), "white").save(generated_bytes, "PNG")

        class Response:
            status_code = 200
            text = ""
            def raise_for_status(self):
                return None
            def json(self):
                return {"data": [{"b64_json": base64.b64encode(generated_bytes.getvalue()).decode()}]}

        class Session:
            url = None
            headers = None
            data = None
            file_count = 0
            def post(self, url, **kwargs):
                self.url = url
                self.headers = kwargs["headers"]
                self.data = kwargs["data"]
                self.file_count = len(kwargs["files"])
                return Response()

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "reference.png"
            Image.new("RGB", (64, 64), "black").save(source)
            session = Session()
            paths = ImageGenerationService(
                "https://www.micuapi.ai", "key", "gpt-image-2", session=session,
            ).generate([str(source)], "new main image", 1, str(root / "output"))
            self.assertEqual(session.url, "https://www.micuapi.ai/v1/images/edits")
            self.assertIn("User-Agent", session.headers)
            self.assertEqual(session.data["size"], "1792x2400")
            self.assertEqual(session.data["quality"], "high")
            self.assertEqual(session.data["response_format"], "b64_json")
            self.assertEqual(session.file_count, 1)
            self.assertEqual(len(paths), 1)
            self.assertTrue(Path(paths[0]).is_file())
        self.assertEqual(normalize_image_edits_url("https://www.micuapi.ai/v1"), "https://www.micuapi.ai/v1/images/edits")

    def test_main_image_prompt_forbids_collage_duplicates_and_unapproved_text(self):
        prompt = build_ozon_main_image_prompt(
            "Use an urban travel setting.",
            ["Городской рюкзак", "Вместительное отделение", "Для путешествий"],
        )
        self.assertIn("exactly ONE complete dominant product", prompt)
        self.assertIn("advertising poster", prompt)
        self.assertIn("Render exactly 3 added visible Russian text elements", prompt)
        self.assertIn('"Городской рюкзак"', prompt)
        self.assertIn('"Вместительное отделение"', prompt)
        self.assertIn("Do not create a dense collage", prompt)
        self.assertIn("Do not copy source seller watermarks", prompt)
        self.assertIn("Use an urban travel setting", prompt)
        self.assertLessEqual(len(prompt), 3800)
        longest = build_ozon_main_image_prompt(
            "Detailed art direction " * 1000,
            ["Очень длинный русский заголовок для карточки товара " * 3] * 4,
        )
        self.assertLessEqual(len(longest), 3800)

    def test_poster_text_uses_title_and_traceable_ozon_facts_only(self):
        reference = ReferenceProduct(
            "https://ozon.ru/product/3389874670/",
            "Игрушка для животных",
            properties={"页面商品参数": (
                "Характеристики\n"
                "Вид игрушки для животных\nПищалка, Интерактивная, Грызак\n"
                "Предназначено для\nДля собак\n"
                "Особенности игрушки\nЗвуковая, Шуршащая\n"
                "Артикул\n3389874670\n"
            )},
        )
        lines = build_ozon_poster_text_lines(
            "Игрушка для собак «Утка» с пищалкой и веревочным хвостом, 40 см",
            reference,
        )
        self.assertEqual(lines[0], "Игрушка для собак «Утка» с пищалкой и веревочным хвостом")
        self.assertIn("Звуковая, Шуршащая", lines)
        self.assertIn("Пищалка, Интерактивная, Грызак", lines)
        self.assertNotIn("3389874670", lines)

    def test_oss_keys_are_grouped_by_date_offer_and_stable_image_order(self):
        keys = build_oss_object_keys(
            ["C:/images/generated-main.jpg", "C:/images/detail.PNG", "C:/images/third.webp"],
            "rfbs-listing", "WXZ#003", "2026-07-31",
        )
        self.assertEqual(keys, [
            "rfbs-listing/2026-07-31/WXZ#003/WXZ#003主图.jpg",
            "rfbs-listing/2026-07-31/WXZ#003/附图(1).png",
            "rfbs-listing/2026-07-31/WXZ#003/附图(2).webp",
        ])
        self.assertEqual(
            keys,
            build_oss_object_keys(
                ["C:/images/generated-main.jpg", "C:/images/detail.PNG", "C:/images/third.webp"],
                "rfbs-listing", "WXZ#003", "2026-07-31",
            ),
        )

    def test_oss_offer_folder_sanitizes_unsafe_path_characters(self):
        key = build_oss_object_keys(
            ["C:/images/main.jpg"], "rfbs-listing", "../SKU / 01", "2026-07-31",
        )[0]
        self.assertTrue(key.startswith("rfbs-listing/2026-07-31/SKU_01-"))
        self.assertNotIn("../", key)
        self.assertTrue(key.endswith("主图.jpg"))

    def test_app_passes_current_offer_id_to_oss_uploader(self):
        class Value:
            def get(self):
                return "WXZ003"
        app = RfbsListingApp.__new__(RfbsListingApp)
        app.vars = {"offer_id": Value()}
        app.local_images = ["main.jpg", "detail.jpg"]
        app._config = lambda: {"oss": {"bucket": "test"}}
        app._log = lambda _message: None
        with patch("app.OssService") as service:
            service.return_value.upload.return_value = [
                "https://cdn/WXZ003主图.jpg", "https://cdn/附图(1).jpg",
            ]
            app._upload_images()
        service.return_value.upload.assert_called_once_with(
            ["main.jpg", "detail.jpg"], "WXZ003",
        )
        self.assertEqual(app.uploaded_urls[0], "https://cdn/WXZ003主图.jpg")

    def test_new_program_does_not_import_reference_modules(self):
        forbidden = ("参考代码", "ozon_core", "oss_uploader", "ozon生图", "图片添加水印", "ozon_pipeline")
        for path in APP_DIR.glob("*.py"):
            source = path.read_text(encoding="utf-8")
            for token in forbidden:
                self.assertNotIn(token, source, f"{path.name} must not depend on {token}")

    def test_tk_error_callback_captures_message_before_exception_cleanup(self):
        source = (APP_DIR / "app.py").read_text(encoding="utf-8")
        self.assertIn("lambda message=error_message:", source)
        self.assertNotIn('lambda: messagebox.showerror("执行失败", str(error))', source)

    def test_v2_warehouse_list_uses_top_level_paginated_response(self):
        api = object.__new__(OzonApi)
        calls = []
        pages = [
            {"warehouses": [{"warehouse_id": 1, "name": "A", "is_rfbs": True}], "has_next": True, "cursor": "next"},
            {"warehouses": [{"warehouse_id": 2, "name": "B", "is_rfbs": False}], "has_next": False, "cursor": ""},
        ]
        def fake_post(path, body):
            calls.append((path, body))
            return pages[len(calls) - 1]
        api.post = fake_post
        warehouses = api.warehouses()
        self.assertEqual([item["warehouse_id"] for item in warehouses], [1, 2])
        self.assertEqual(calls[0], ("/v2/warehouse/list", {"limit": 100}))
        self.assertEqual(calls[1], ("/v2/warehouse/list", {"limit": 100, "cursor": "next"}))

    def test_ozon_api_no_longer_exposes_commission_lookup(self):
        self.assertFalse(hasattr(OzonApi, "rfbs_commission_info"))
        self.assertFalse(hasattr(OzonApi, "product_prices"))

    def test_ozon_price_update_uses_supported_price_payload(self):
        api = object.__new__(OzonApi)
        calls = []
        def fake_post(path, body):
            calls.append((path, body))
            return {"result": [{
                "offer_id": "SKU-1", "product_id": 123, "updated": True, "errors": [],
            }]}
        api.post = fake_post
        api.update_price(
            "SKU-1", "160", old_price="200", currency_code="CNY", min_price="0",
        )
        self.assertEqual(calls, [("/v1/product/import/prices", {"prices": [{
            "auto_action_enabled": "UNKNOWN", "currency_code": "CNY",
            "min_price": "0", "offer_id": "SKU-1", "old_price": "200",
            "price": "160",
        }]})])

    def test_ozon_price_update_surfaces_item_level_errors(self):
        api = object.__new__(OzonApi)
        api.post = lambda _path, _body: {"result": [{
            "offer_id": "SKU-1", "updated": False,
            "errors": [{"code": "PRICE_ERROR", "message": "bad price"}],
        }]}
        with self.assertRaisesRegex(RuntimeError, "PRICE_ERROR"):
            api.update_price("SKU-1", "160")

    def test_app_uses_manual_commission_without_platform_lookup(self):
        class Value:
            def __init__(self, value):
                self.value = str(value)
            def get(self):
                return self.value
            def set(self, value):
                self.value = str(value)

        class Api:
            def __init__(self):
                self.updates = []
            def update_price(self, offer_id, price, **kwargs):
                self.updates.append((offer_id, str(price), dict(kwargs)))
                return {"result": [{"offer_id": offer_id, "updated": True, "errors": []}]}

        app = RfbsListingApp.__new__(RfbsListingApp)
        app.vars = {
            "purchase_cost": Value("30"), "label_fee": Value("2"),
            "target_roi": Value("60"), "weight": Value("600"),
            "price": Value("120"), "old_price": Value("150"),
        }
        app._ui_call = lambda callback, **_kwargs: callback()
        app._retry_step = lambda _label, operation, **_kwargs: operation()
        app._save_auto_jobs = lambda: None
        logs = []
        app._log = logs.append
        api = Api()
        item = {
            "offer_id": "SKU-1", "price": "120", "old_price": "150",
            "currency_code": "CNY", "description_category_id": 10, "type_id": 20,
        }
        job = {"inputs": {
            "purchase_cost": "30", "label_fee": "2", "target_roi": "60",
            "sales_commission_percent": "25",
            "weight": "600", "price": "120", "old_price": "150",
        }}

        result = app._apply_manual_commission_price(api, item, job_record=job)

        self.assertEqual([update[1] for update in api.updates], ["160"])
        self.assertEqual([update[2]["old_price"] for update in api.updates], ["200"])
        self.assertEqual(item["price"], "160")
        self.assertEqual(item["old_price"], "200")
        self.assertEqual(result["sales_percent_rfbs"], "25")
        self.assertEqual(result["advertising_percent"], "15")
        self.assertEqual(result["cargo_loss_percent"], "10")
        self.assertEqual(result["actual_roi_percent"], "60.13")
        self.assertEqual(result["adjustments"], 1)
        self.assertEqual(result["manual_sales_commission_percent"], "25")
        self.assertEqual(job["actual_sales_commission_percent"], "25")
        self.assertTrue(job["manual_commission_calculated"])
        self.assertTrue(any("ROI 60.13%" in message for message in logs))

    def test_app_uses_fbp_effective_commission_for_actual_roi_check(self):
        class Value:
            def __init__(self, value):
                self.value = str(value)

            def get(self):
                return self.value

            def set(self, value):
                self.value = str(value)

        class Api:
            def __init__(self):
                self.updates = []

            def update_price(self, *args, **kwargs):
                self.updates.append((args, kwargs))

        app = RfbsListingApp.__new__(RfbsListingApp)
        app.vars = {
            "purchase_cost": Value("30"), "label_fee": Value("2"),
            "target_roi": Value("60"), "weight": Value("600"),
            "fbp_pricing": Value("1"), "price": Value("200"),
            "old_price": Value("250"),
        }
        app._ui_call = lambda callback, **_kwargs: callback()
        app._retry_step = lambda _label, operation, **_kwargs: operation()
        app._save_auto_jobs = lambda: None
        app._log = lambda _message: None
        api = Api()
        item = {
            "offer_id": "SKU-FBP", "price": "200", "old_price": "250",
            "currency_code": "RUB", "description_category_id": 10, "type_id": 20,
        }
        job = {"inputs": {
            "purchase_cost": "30", "label_fee": "2", "target_roi": "60",
            "sales_commission_percent": "20",
            "advertising_percent": "20", "cargo_loss_percent": "5",
            "weight": "600", "fbp_pricing": "1", "price": "200", "old_price": "250",
        }}

        result = app._apply_manual_commission_price(api, item, job_record=job)

        self.assertEqual(api.updates, [])
        self.assertEqual(result["pricing_mode"], "FBP")
        self.assertEqual(result["manual_sales_commission_percent"], "20")
        self.assertEqual(result["sales_percent_rfbs"], "19")
        self.assertEqual(result["advertising_percent"], "20")
        self.assertEqual(result["cargo_loss_percent"], "5")
        self.assertEqual(result["actual_roi_percent"], "166.28")
        self.assertEqual(job["actual_sales_commission_percent"], "19")
        self.assertEqual(job["manual_sales_commission_percent"], "20")

    def test_dictionary_search_uses_description_category_and_type(self):
        api = object.__new__(OzonApi)
        calls = []
        def fake_post(path, body):
            calls.append((path, body))
            return {"result": [{"id": 99, "value": "Черный"}]}
        api.post = fake_post
        result = api.dictionary_values(10, 20, 30, "Черный")
        self.assertEqual(result[0]["id"], 99)
        self.assertEqual(calls, [("/v1/description-category/attribute/values/search", {
            "description_category_id": 10, "type_id": 20,
            "attribute_id": 30, "value": "Черный", "limit": 100,
            "language": "ZH_HANS",
        })])

    def test_complete_dictionary_uses_last_value_id_pagination(self):
        api = object.__new__(OzonApi)
        api.catalog_language = "ZH_HANS"
        calls = []
        pages = [
            {"result": [{"id": 10, "value": "A"}, {"id": 20, "value": "B"}]},
            {"result": [{"id": 30, "value": "C"}]},
        ]
        def fake_post(path, body):
            calls.append((path, body))
            return pages[len(calls) - 1]
        api.post = fake_post
        values = api.dictionary_values_all(1, 2, 3, page_limit=2)
        self.assertEqual([item["id"] for item in values], [10, 20, 30])
        self.assertEqual(calls[0][1]["last_value_id"], 0)
        self.assertEqual(calls[1][1]["last_value_id"], 20)
        self.assertEqual(calls[0][0], "/v1/description-category/attribute/values")

    def test_complete_dictionary_cache_is_reused_and_can_refresh(self):
        class Api:
            calls = 0
            def dictionary_values_all(self, *_args, **_kwargs):
                self.calls += 1
                return [{"id": self.calls, "value": "选项"}]
        with tempfile.TemporaryDirectory() as temporary:
            api = Api()
            cache = OzonDictionaryCache(temporary)
            first, first_cached, path = cache.load(api, 1, 2, 3)
            second, second_cached, _ = cache.load(api, 1, 2, 3)
            refreshed, refreshed_cached, _ = cache.load(api, 1, 2, 3, force_refresh=True)
            self.assertTrue(path.is_file())
            self.assertFalse(first_cached)
            self.assertTrue(second_cached)
            self.assertFalse(refreshed_cached)
            self.assertEqual(first, second)
            self.assertEqual(refreshed[0]["id"], 2)
            self.assertEqual(api.calls, 2)

    def test_generated_title_tags_and_no_brand_defaults_are_recognized(self):
        self.assertEqual(RfbsListingApp._content_attribute_kind("名称"), "title")
        self.assertEqual(RfbsListingApp._content_attribute_kind("Название"), "title")
        self.assertEqual(RfbsListingApp._content_attribute_kind("#主题标签"), "tags")
        self.assertEqual(
            RfbsListingApp._content_attribute_kind("型号名称（针对合并为一张商品卡片）"),
            "model_name",
        )
        self.assertEqual(
            RfbsListingApp._content_attribute_kind("Название модели (для объединения в одну карточку)"),
            "model_name",
        )
        no_brand = RfbsListingApp._locked_no_brand_dictionary_value()
        self.assertEqual(no_brand["id"], 126745801)
        self.assertEqual(no_brand["value"], "Нет бренда")

    def test_brand_detection_uses_live_id_and_localized_names(self):
        self.assertTrue(RfbsListingApp._is_brand_attribute({"id": 85, "name": "Other"}))
        self.assertTrue(RfbsListingApp._is_brand_attribute({"id": 1, "name": "品牌"}))
        self.assertTrue(RfbsListingApp._is_brand_attribute({"id": 1, "name_ru": "Бренд"}))
        self.assertFalse(RfbsListingApp._is_brand_attribute({"id": 1, "name": "型号名称"}))

    def test_locked_brand_overwrites_advanced_json_value(self):
        app = RfbsListingApp.__new__(RfbsListingApp)
        app.attribute_definitions = [{"id": 85, "name": "品牌", "dictionary_id": 28732849}]
        attributes = [{"id": 85, "values": [{"value": "Other", "dictionary_value_id": 999}]}]
        self.assertTrue(app._enforce_locked_brand(attributes, []))
        self.assertEqual(attributes[0]["values"], [{
            "value": "Нет бренда", "dictionary_value_id": 126745801,
        }])

    def test_brand_dictionary_load_is_skipped_before_api_access(self):
        app = RfbsListingApp.__new__(RfbsListingApp)
        app.attribute_definition_by_id = {85: {"id": 85, "name": "品牌", "dictionary_id": 28732849}}
        logs = []
        app._log = logs.append
        app._load_dictionary_values(85, force_refresh=True)
        self.assertIn("跳过 Ozon 品牌字典请求", logs[0])

    def test_submission_synchronizes_hashtags_without_commas(self):
        class Value:
            def get(self):
                return "900"
        app = RfbsListingApp.__new__(RfbsListingApp)
        app.vars = {"net_weight": Value()}
        app.attribute_definitions = [{"id": 23171, "name": "#主题标签", "dictionary_id": 0}]
        attributes = [{
            "id": 23171,
            "values": [{"value": "#рюкзак, #туристическийрюкзак"}],
        }]
        changed = app._enforce_generated_content_attributes(attributes, [], {
            "tags_ru": ["#рюкзак", "#туристическийрюкзак"],
        })
        self.assertEqual(changed, 1)
        self.assertEqual(attributes[0]["values"][0]["value"], "#рюкзак #туристическийрюкзак")

    def test_manual_net_weight_overrides_live_product_weight_attribute(self):
        class Value:
            def get(self):
                return "875"
        app = RfbsListingApp.__new__(RfbsListingApp)
        app.vars = {"net_weight": Value()}
        app.attribute_definitions = [{"id": 4383, "name": "商品重量，克", "dictionary_id": 0}]
        attributes = [{"id": 4383, "values": [{"value": "480"}]}]
        self.assertTrue(app._enforce_manual_net_weight(attributes, []))
        self.assertEqual(attributes[0]["values"][0]["value"], "875")

    def test_manual_model_name_overrides_attribute_for_variant_grouping(self):
        class Value:
            def get(self):
                return "RITTER-SQUARE-40"
        app = RfbsListingApp.__new__(RfbsListingApp)
        app.vars = {"model_name": Value()}
        app.attribute_definitions = [{
            "id": 9048,
            "name": "型号名称（针对合并为一张商品卡片）",
            "name_ru": "Название модели (для объединения в одну карточку)",
            "dictionary_id": 0,
        }]
        attributes = [{"id": 9048, "values": [{"value": "AI-WRONG-MODEL"}]}]
        self.assertTrue(app._enforce_manual_model_name(attributes, []))
        self.assertEqual(attributes[0]["values"][0]["value"], "RITTER-SQUARE-40")
        self.assertEqual(attributes[0]["_source"], "人工填写：型号名称（合并多变体）")

    def test_required_product_type_uses_the_only_live_dictionary_choice(self):
        class Value:
            def __init__(self, value):
                self.value = value
            def get(self):
                return self.value

        app = RfbsListingApp.__new__(RfbsListingApp)
        app.vars = {"category_id": Value("17028718"), "type_id": Value("96018")}
        app.attribute_definitions = [{
            "id": 8229, "name": "类型", "name_ru": "Тип",
            "dictionary_id": 1960, "is_required": True,
        }]
        app._api = lambda: object()
        app._cached_dictionary_values = lambda *_args, **_kwargs: ([
            {"id": 96018, "value": "清洁抹布"},
        ], True, Path("cache.json"))
        attributes = []

        filled = app._enforce_unique_required_product_type(attributes, [])

        self.assertEqual(filled, 1)
        self.assertEqual(attributes[0]["id"], 8229)
        self.assertEqual(attributes[0]["values"], [{
            "value": "清洁抹布", "dictionary_value_id": 96018,
        }])
        self.assertEqual(attributes[0]["_source"], "系统自动：当前类目唯一可选类型")

    def test_required_product_type_is_not_guessed_when_dictionary_has_multiple_choices(self):
        class Value:
            def __init__(self, value):
                self.value = value
            def get(self):
                return self.value

        app = RfbsListingApp.__new__(RfbsListingApp)
        app.vars = {"category_id": Value("10"), "type_id": Value("20")}
        app.attribute_definitions = [{
            "id": 8229, "name": "类型", "dictionary_id": 1960,
            "is_required": True,
        }]
        app._api = lambda: object()
        app._cached_dictionary_values = lambda *_args, **_kwargs: ([
            {"id": 1, "value": "类型一"}, {"id": 2, "value": "类型二"},
        ], True, Path("cache.json"))
        attributes = []

        self.assertEqual(app._enforce_unique_required_product_type(attributes, []), 0)
        self.assertEqual(attributes, [])

    def test_model_name_is_required_and_cannot_contain_chinese(self):
        app = RfbsListingApp.__new__(RfbsListingApp)
        app.ozon_shops = {}
        base = {"source_url": "2379505289", "offer_id": "SKU-1"}
        with self.assertRaisesRegex(ValueError, "人工填写型号名称"):
            app._validate_job_inputs({**base, "model_name": ""})
        with self.assertRaisesRegex(ValueError, "不能填写中文"):
            app._validate_job_inputs({**base, "model_name": "型号一"})
        with self.assertRaisesRegex(ValueError, "选择一个已保存的 Ozon 上品店铺"):
            app._validate_job_inputs({**base, "model_name": "MODEL-1"})

    def test_local_listing_requires_images_but_not_ozon_source_or_watermark(self):
        app = RfbsListingApp.__new__(RfbsListingApp)
        app.ozon_shops = {
            "shop-1": {
                "id": "shop-1", "name": "本地新品店",
                "client_id": "client", "api_key": "key",
            },
        }
        app.category_by_display = {
            "测试类目": {
                "display": "测试类目",
                "description_category_id": 10,
                "type_id": 20,
            },
        }
        with tempfile.TemporaryDirectory() as directory:
            image_path = Path(directory) / "main.jpg"
            image_path.write_bytes(b"image-placeholder")
            values = {
                "listing_mode": "local", "source_url": "",
                "local_image_paths": [str(image_path)],
                "offer_id": "AUTO-LOCAL-1", "model_name": "LOCAL-MODEL",
                "ozon_shop_id": "shop-1", "purchase_cost": "15",
                "sales_commission_percent": "18",
                "label_fee": "2", "target_roi": "60", "price": "100",
                "old_price": "0", "net_weight": "500", "weight": "600",
                "depth": "100", "width": "100", "height": "100",
                "category_display": "测试类目", "warehouse_id": "1001",
                "stock": "1", "watermark_path": "",
            }
            category = app._validate_job_inputs(values)
            self.assertEqual(category["type_id"], 20)
            self.assertGreater(Decimal(values["price"]), 0)
            with self.assertRaisesRegex(ValueError, "至少选择 1 张"):
                app._validate_job_inputs({**values, "local_image_paths": []})

    def test_local_job_images_are_copied_into_stable_program_storage(self):
        app = RfbsListingApp.__new__(RfbsListingApp)
        app._log = lambda _message: None
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source.png"
            source.write_bytes(b"local-product-image")
            with patch("app.OUTPUT_DIR", root / "output"):
                copied = app._snapshot_local_job_images(
                    [str(source)], "LOCAL/SKU", "job-1",
                )
            self.assertEqual(len(copied), 1)
            self.assertTrue(Path(copied[0]).is_file())
            self.assertEqual(Path(copied[0]).read_bytes(), b"local-product-image")
            self.assertIn("LOCAL_SKU", copied[0])

    def test_legacy_single_ozon_config_migrates_to_default_shop(self):
        shops, selected = RfbsListingApp._ozon_shops_from_config({
            "client_id": "legacy-client", "api_key": "legacy-key", "proxy_url": "http://127.0.0.1:7890",
        })
        self.assertEqual(selected, "legacy-default")
        self.assertEqual(shops[selected]["name"], "默认店铺")
        self.assertEqual(shops[selected]["client_id"], "legacy-client")
        self.assertEqual(shops[selected]["api_key"], "legacy-key")

    def test_running_job_uses_its_bound_shop_even_if_current_selection_changes(self):
        class Value:
            def __init__(self, value):
                self.value = value
            def get(self):
                return self.value

        app = RfbsListingApp.__new__(RfbsListingApp)
        app.ozon_shops = {
            "shop-a": {"id": "shop-a", "name": "店铺A", "client_id": "client-a", "api_key": "key-a", "proxy_url": ""},
            "shop-b": {"id": "shop-b", "name": "店铺B", "client_id": "client-b", "api_key": "key-b", "proxy_url": ""},
        }
        app.vars = {
            "ozon_shop_id": Value("shop-b"), "ozon_shop_name": Value("店铺B"),
            "ozon_client_id": Value("client-b"), "ozon_api_key": Value("key-b"),
            "ozon_proxy_url": Value(""),
        }
        app.active_job_id = "running-job"
        app.auto_jobs = {
            "running-job": {"inputs": {"ozon_shop_id": "shop-a", "ozon_shop_name": "店铺A"}},
        }
        api = app._api()
        self.assertEqual(api.client_id, "client-a")
        self.assertEqual(api.api_key, "key-a")

    def test_fixed_attribute_defaults_use_live_names_and_warranty_dictionary_id(self):
        app = RfbsListingApp.__new__(RfbsListingApp)
        unit_definition = {
            "id": 8962, "name": "一个商品中的件数",
            "name_ru": "Единиц в одном товаре", "dictionary_id": 0,
        }
        factory_definition = {
            "id": 11650, "name": "原厂包装数量",
            "name_ru": "Количество заводских упаковок", "dictionary_id": 0,
        }
        warranty_definition = {
            "id": 10400, "name": "保证", "name_ru": "Гарантия",
            "dictionary_id": 46700526,
        }
        country_definition = {
            "id": 4389, "name": "原产国", "name_ru": "Страна-изготовитель",
            "dictionary_id": 100,
        }
        attributes = []
        self.assertEqual(app._system_default_attribute_kind(unit_definition), "units_per_product")
        self.assertEqual(app._system_default_attribute_kind(factory_definition), "factory_package_count")
        self.assertEqual(app._system_default_attribute_kind(warranty_definition), "warranty")
        self.assertEqual(app._system_default_attribute_kind(country_definition), "country_of_origin")
        self.assertTrue(app._apply_system_attribute_default(attributes, [], unit_definition))
        self.assertTrue(app._apply_system_attribute_default(attributes, [], factory_definition))
        self.assertTrue(app._apply_system_attribute_default(
            attributes, [], warranty_definition,
            dictionary_options=[{"id": 970716397, "value": "1 年"}],
        ))
        self.assertTrue(app._apply_system_attribute_default(
            attributes, [], country_definition,
            dictionary_options=[{"id": 111, "value": "俄罗斯"}, {"id": 222, "value": "中国"}],
        ))
        values = attribute_values_by_id(attributes)
        self.assertEqual(values[8962]["values"], [{"value": "1"}])
        self.assertEqual(values[11650]["values"], [{"value": "1"}])
        self.assertEqual(values[10400]["values"], [{
            "value": "1 年", "dictionary_value_id": 970716397,
        }])
        self.assertEqual(values[4389]["values"], [{
            "value": "中国", "dictionary_value_id": 222,
        }])

    def test_submission_dictionary_values_must_come_from_live_options(self):
        class Value:
            def __init__(self, value):
                self.value = value
            def get(self):
                return self.value

        app = RfbsListingApp.__new__(RfbsListingApp)
        definition = {
            "id": 7, "name": "颜色", "name_ru": "Цвет",
            "dictionary_id": 70, "is_collection": False,
        }
        app.vars = {"category_id": Value("10"), "type_id": Value("20")}
        app.attribute_definitions = [definition]
        app._api = lambda: object()
        app._cached_dictionary_values = lambda *_args, **_kwargs: (
            [{"id": 701, "value": "黑色"}, {"id": 702, "value": "白色"}],
            True,
            Path("cache.json"),
        )

        attributes = [{"id": 7, "values": [{"value": "黑色"}], "_source": "AI映射"}]
        repaired = app._validate_dictionary_values_before_submission(attributes, [])
        self.assertEqual(repaired, 1)
        self.assertEqual(attributes[0]["values"], [{
            "value": "黑色", "dictionary_value_id": 701,
        }])

        invalid = [{"id": 7, "values": [{"value": "AI虚构颜色"}]}]
        with self.assertRaisesRegex(ValueError, "不是当前 Ozon 可供选择的内容"):
            app._validate_dictionary_values_before_submission(invalid, [])

    def test_invalid_country_dictionary_text_falls_back_to_live_china_option(self):
        class Value:
            def __init__(self, value):
                self.value = value
            def get(self):
                return self.value

        app = RfbsListingApp.__new__(RfbsListingApp)
        definition = {
            "id": 4389, "name": "原产国", "name_ru": "Страна-изготовитель",
            "dictionary_id": 100, "is_collection": False,
        }
        app.vars = {"category_id": Value("10"), "type_id": Value("20")}
        app.attribute_definitions = [definition]
        app._api = lambda: object()
        app._cached_dictionary_values = lambda *_args, **_kwargs: (
            [{"id": 111, "value": "俄罗斯"}, {"id": 222, "value": "中国"}],
            True,
            Path("cache.json"),
        )
        attributes = [{"id": 4389, "values": [{"value": "AI自由文本"}]}]
        repaired = app._validate_dictionary_values_before_submission(attributes, [])
        self.assertEqual(repaired, 1)
        self.assertEqual(attributes[0]["values"], [{
            "value": "中国", "dictionary_value_id": 222,
        }])

    def test_fixed_attribute_defaults_do_not_overwrite_explicit_repair(self):
        app = RfbsListingApp.__new__(RfbsListingApp)
        definition = {
            "id": 8962, "name": "一个商品中的件数",
            "name_ru": "Единиц в одном товаре", "dictionary_id": 0,
        }
        attributes = [{"id": 8962, "values": [{"value": "2"}], "_source": "人工修复"}]
        self.assertFalse(app._apply_system_attribute_default(attributes, [], definition))
        self.assertEqual(attributes[0]["values"][0]["value"], "2")

    def test_missing_one_year_warranty_option_is_rejected(self):
        app = RfbsListingApp.__new__(RfbsListingApp)
        definition = {
            "id": 10400, "name": "保证", "name_ru": "Гарантия",
            "dictionary_id": 46700526,
        }
        with self.assertRaisesRegex(ValueError, "没有找到“1 年”"):
            app._apply_system_attribute_default(
                [], [], definition,
                dictionary_options=[{"id": 970716398, "value": "2 年"}],
            )

    def test_safe_auto_step_retries_transient_errors_only(self):
        app = RfbsListingApp.__new__(RfbsListingApp)
        app._log = lambda _message: None
        calls = []
        def transient():
            calls.append(1)
            if len(calls) < 3:
                raise RuntimeError("temporary timeout")
            return "ok"
        self.assertEqual(app._retry_step("测试", transient, attempts=3, delay_seconds=0), "ok")
        self.assertEqual(len(calls), 3)
        validation_calls = []
        def invalid():
            validation_calls.append(1)
            raise ValueError("字段无效")
        with self.assertRaises(ValueError):
            app._retry_step("测试", invalid, attempts=3, delay_seconds=0)
        self.assertEqual(len(validation_calls), 1)

    def test_import_status_read_is_retried_without_reimporting(self):
        api = OzonApi.__new__(OzonApi)
        calls = []
        def import_info(task_id):
            calls.append(task_id)
            if len(calls) == 1:
                raise RuntimeError("temporary read timeout")
            return {"result": {"items": [{"status": "imported", "errors": []}]}}
        api.import_info = import_info
        logs = []
        result = api.wait_import(123, attempts=3, interval=0, log_func=logs.append)
        self.assertEqual(result["result"]["items"][0]["status"], "imported")
        self.assertEqual(calls, [123, 123])
        self.assertIn("将继续查询", logs[0])

    def test_auto_pipeline_runs_all_steps_in_order_and_keeps_selected_category(self):
        app = RfbsListingApp.__new__(RfbsListingApp)
        events = []
        app.auto_pipeline_running = False
        app.auto_progress_value = 0
        app._set_auto_progress = lambda _value, text: events.append(text.split(" ", 1)[-1])
        app._retry_step = lambda label, operation, **_kwargs: (events.append(label), operation())[1]
        app._ui_call = lambda callback, **_kwargs: callback()
        app._parse_url = lambda: events.append("parse")
        app._generate_copywriting = lambda: events.append("copy")
        app._generate_images = lambda: events.append("images")
        app._upload_images = lambda: events.append("oss")
        app._ai_fill_category_attributes = lambda use_selected_category=False: events.append(
            "selected-category" if use_selected_category else "ai-category"
        )
        app._save_draft = lambda notify=True: events.append("draft") or Path("draft.json")
        app._submit = lambda notify=True, auto_progress=False: events.append("submit") or Path("submitted.json")
        app._save_workspace_quietly = lambda: events.append("workspace")
        class Root:
            def after(self, *_args, **_kwargs):
                return None
        app.root = Root()
        app._auto_pipeline()
        ordered = [event for event in events if event in {
            "parse", "copy", "images", "oss", "selected-category", "draft", "submit",
        }]
        self.assertEqual(ordered, [
            "parse", "copy", "images", "oss", "selected-category", "draft", "submit",
        ])
        self.assertNotIn("ai-category", events)

    def test_local_queued_pipeline_skips_source_parse_and_image_generation(self):
        app = RfbsListingApp.__new__(RfbsListingApp)
        events = []
        app.auto_pipeline_running = False
        app._apply_job_context = lambda _job: events.append("apply")
        app._set_auto_progress = lambda _value, _text: None
        app._retry_step = lambda label, operation, **_kwargs: (events.append(label), operation())[1]
        app._ui_call = lambda callback, **_kwargs: callback()
        app._parse_url = lambda: events.append("parse")
        app._prepare_local_job_images = lambda _job, analyze=False: events.append(
            "local-analysis" if analyze else "local-images"
        )
        app._generate_copywriting = lambda: events.append("copy")
        app._generate_images = lambda: events.append("generate-images")
        app._prepare_local_images_for_upload = lambda: events.append("ready-local-images")
        app._upload_images = lambda: events.append("oss")
        app._ai_fill_category_attributes = lambda use_selected_category=False: events.append("attributes")
        app._save_draft = lambda notify=True: Path("draft.json")
        app._submit = lambda **_kwargs: events.append("submit") or Path("submitted.json")
        app._checkpoint_auto_job = lambda job, stage, _message: job.update(stage=stage)
        job = {
            "id": "local-job", "stage": 0,
            "inputs": {
                "listing_mode": "local", "offer_id": "LOCAL-1",
                "local_image_paths": ["main.jpg"],
            },
        }

        app._run_auto_job(job)

        self.assertIn("local-analysis", events)
        self.assertIn("ready-local-images", events)
        self.assertNotIn("parse", events)
        self.assertNotIn("generate-images", events)
        self.assertEqual(job["stage"], 7)

    def test_local_visual_facts_form_conservative_copywriting_reference(self):
        reference = RfbsListingApp._reference_from_local_vision({
            "product_name_ru": "Настольная лампа",
            "visual_facts": [
                {"fact_name_ru": "Цвет", "value_ru": "Белый"},
                {"fact_name_ru": "Форма", "value_ru": "Круглая"},
            ],
        })
        self.assertEqual(reference.source_url, "")
        self.assertEqual(reference.title, "Настольная лампа")
        self.assertEqual(reference.properties["Цвет"], "Белый")
        self.assertIn("Форма: Круглая", reference.description)

    def test_local_ready_images_keep_first_as_main_and_allow_no_watermark(self):
        class Value:
            def get(self):
                return ""

        app = RfbsListingApp.__new__(RfbsListingApp)
        app.source_images = ["first.jpg", "second.jpg", "third.jpg"]
        app.local_images = list(app.source_images)
        app.primary_reference_index = 1
        app.vars = {"watermark_path": Value()}
        app.uploaded_urls = ["old"]
        app._log = lambda message: setattr(app, "last_log", message)
        app._replace_images = lambda paths: setattr(app, "local_images", list(paths))

        app._prepare_local_images_for_upload()

        self.assertEqual(app.local_images, ["second.jpg", "first.jpg", "third.jpg"])
        self.assertEqual(app.uploaded_urls, [])
        self.assertIn("未调用图片生成服务", app.last_log)

    def test_queued_job_resumes_after_last_completed_stage(self):
        app = RfbsListingApp.__new__(RfbsListingApp)
        events = []
        app.auto_pipeline_running = False
        app._apply_job_context = lambda _job: events.append("apply")
        app._set_auto_progress = lambda _value, _text: None
        app._retry_step = lambda label, operation, **_kwargs: (events.append(label), operation())[1]
        app._ui_call = lambda callback, **_kwargs: callback()
        app._parse_url = lambda: events.append("parse")
        app._generate_copywriting = lambda: events.append("copy")
        app._generate_images = lambda: events.append("images")
        app._upload_images = lambda: events.append("oss")
        app._ai_fill_category_attributes = lambda use_selected_category=False: events.append("attributes")
        app._save_draft = lambda notify=True: Path("draft.json")
        app._submit = lambda **_kwargs: events.append("submit") or Path("submitted.json")
        app._checkpoint_auto_job = lambda job, stage, _message: job.update(stage=stage)
        job = {"id": "job", "stage": 4, "inputs": {"offer_id": "SKU-1"}}
        app._run_auto_job(job)
        self.assertEqual(events, ["apply", "类目属性填写", "attributes", "submit"])
        self.assertEqual(job["stage"], 7)

    def test_job_input_snapshot_is_independent_from_running_product_vars(self):
        class Value:
            def __init__(self, value):
                self.value = value
            def get(self):
                return self.value
        app = RfbsListingApp.__new__(RfbsListingApp)
        app.job_form_vars = {
            "source_url": Value("NEXT-ARTICLE"), "offer_id": Value("NEXT-OFFER"),
            "model_name": Value("NEXT-MODEL"), "category_display": Value("Next category"),
            "ozon_shop_id": Value("shop-next"), "ozon_shop_name": Value("下一家店"),
        }
        app.vars = {
            key: Value(value) for key, value in {
                "source_url": "RUNNING-ARTICLE", "offer_id": "RUNNING-OFFER",
                "price": "999", "old_price": "0", "net_weight": "900", "weight": "1000",
                "depth": "100", "width": "100", "height": "100", "category_id": "1",
                "type_id": "2", "warehouse_id": "3", "stock": "1", "watermark_path": "mark.png",
                "image_prompt": "prompt", "currency_code": "RUB", "weight_unit": "g",
                "dimension_unit": "mm", "vat": "0",
            }.items()
        }
        app.ozon_shops = {
            "shop-next": {
                "id": "shop-next", "name": "下一家店", "client_id": "client-next",
                "api_key": "must-not-enter-job", "proxy_url": "",
            },
        }
        app.category_by_display = {"Next category": {"description_category_id": 10, "type_id": 20}}
        snapshot = app._job_inputs_from_form()
        self.assertEqual(snapshot["source_url"], "NEXT-ARTICLE")
        self.assertEqual(snapshot["offer_id"], "NEXT-OFFER")
        self.assertEqual(snapshot["model_name"], "NEXT-MODEL")
        self.assertEqual(snapshot["category_id"], "10")
        self.assertEqual(snapshot["ozon_shop_id"], "shop-next")
        self.assertEqual(snapshot["ozon_shop_name"], "下一家店")
        self.assertNotIn("ozon_api_key", snapshot)
        self.assertNotIn("must-not-enter-job", json.dumps(snapshot, ensure_ascii=False))
        self.assertEqual(app.vars["source_url"].get(), "RUNNING-ARTICLE")

    def test_new_job_form_does_not_reuse_saved_model_name(self):
        class Value:
            def __init__(self, value):
                self.value = value
            def get(self):
                return self.value
            def set(self, value):
                self.value = value
        app = RfbsListingApp.__new__(RfbsListingApp)
        app.job_form_vars = {
            "model_name": Value("STALE-MODEL"),
            "price": Value("999"),
        }
        app.vars = {
            "model_name": Value("PREVIOUS-MODEL"),
            "price": Value("1299"),
        }
        app._job_category_selected = lambda: None
        app._initialize_job_form_from_workspace()
        self.assertEqual(app.job_form_vars["model_name"].get(), "")
        self.assertEqual(app.job_form_vars["price"].get(), "1299")

    def test_queue_continues_with_next_product_after_one_product_fails(self):
        class Value:
            def __init__(self):
                self.value = ""
            def set(self, value):
                self.value = value

        class Root:
            def after(self, _delay, callback):
                callback()

        app = RfbsListingApp.__new__(RfbsListingApp)
        app.busy = False
        app.active_job_id = ""
        app.auto_job_worker_running = True
        app.auto_job_lock = threading.RLock()
        app.auto_job_queue = queue.Queue()
        app.auto_status_var = Value()
        app.root = Root()
        app._save_auto_jobs = lambda: None
        app._refresh_job_tree = lambda: None
        app._ui_call = lambda callback, **_kwargs: callback()
        app._workspace_payload = lambda: {"fields": {"offer_id": "BROKEN"}}
        app._log = lambda _message: None
        completed = []
        app._run_auto_job = lambda job: (
            (_ for _ in ()).throw(RuntimeError("bad attribute"))
            if job["id"] == "bad" else completed.append(job["id"])
        )
        app._start_auto_job_worker = lambda: None
        app.auto_jobs = {
            "bad": {"id": "bad", "status": "queued", "stage": 4, "inputs": {"offer_id": "BAD"}},
            "good": {"id": "good", "status": "queued", "stage": 0, "inputs": {"offer_id": "GOOD"}},
        }
        app.auto_job_queue.put("bad")
        app.auto_job_queue.put("good")
        app._auto_job_worker()
        self.assertEqual(app.auto_jobs["bad"]["status"], "failed")
        self.assertEqual(app.auto_jobs["bad"]["failed_stage"], 5)
        self.assertIn("bad attribute", app.auto_jobs["bad"]["error"])
        self.assertEqual(app.auto_jobs["good"]["status"], "completed")
        self.assertEqual(completed, ["good"])
        self.assertEqual(app.auto_job_queue.unfinished_tasks, 0)

    def test_stop_button_cancels_only_selected_job(self):
        class Value:
            def __init__(self):
                self.value = ""
            def set(self, value):
                self.value = value

        selected_job = {
            "id": "selected", "status": "queued", "stage": 0,
            "inputs": {"offer_id": "STOP-ME"}, "task_id": 0,
        }
        other_job = {
            "id": "other", "status": "queued", "stage": 0,
            "inputs": {"offer_id": "KEEP-GOING"}, "task_id": 0,
        }
        app = RfbsListingApp.__new__(RfbsListingApp)
        app.auto_job_lock = threading.RLock()
        app.auto_jobs = {"selected": selected_job, "other": other_job}
        app._selected_job = lambda: selected_job
        app._save_auto_jobs = lambda: None
        app._refresh_job_tree = lambda: None
        app.auto_status_var = Value()
        with patch("app.messagebox.askyesno", return_value=True):
            app._stop_selected_job()
        self.assertEqual(selected_job["status"], "cancelled")
        self.assertFalse(selected_job["cancel_requested"])
        self.assertEqual(other_job["status"], "queued")
        self.assertIn("其他排队任务继续", app.auto_status_var.value)

    def test_clear_finished_jobs_removes_persisted_history_but_keeps_active_jobs(self):
        class Value:
            def __init__(self):
                self.value = ""
            def set(self, value):
                self.value = value

        app = RfbsListingApp.__new__(RfbsListingApp)
        app.auto_job_lock = threading.RLock()
        app.auto_jobs = {
            "done": {"id": "done", "status": "completed"},
            "failed": {"id": "failed", "status": "failed"},
            "queued": {"id": "queued", "status": "queued"},
            "running": {"id": "running", "status": "running"},
        }
        app.auto_job_order = ["done", "failed", "queued", "running"]
        app.auto_status_var = Value()
        saved = []
        refreshed = []
        app._save_auto_jobs = lambda: saved.append(True)
        app._refresh_job_tree = lambda: refreshed.append(True)

        with patch("app.messagebox.askyesno", return_value=True):
            app._clear_finished_jobs()

        self.assertEqual(set(app.auto_jobs), {"queued", "running"})
        self.assertEqual(app.auto_job_order, ["queued", "running"])
        self.assertEqual(saved, [True])
        self.assertEqual(refreshed, [True])
        self.assertIn("已清除 2 条", app.auto_status_var.value)

    def test_running_selected_job_stops_at_boundary_and_other_job_continues(self):
        class Value:
            def set(self, _value):
                return None

        selected_job = {
            "id": "selected", "status": "running", "stage": 2,
            "inputs": {"offer_id": "STOP-ME"}, "task_id": 0,
        }
        other_job = {
            "id": "other", "status": "queued", "stage": 0,
            "inputs": {"offer_id": "KEEP-GOING"}, "task_id": 0,
        }
        app = RfbsListingApp.__new__(RfbsListingApp)
        app.auto_job_lock = threading.RLock()
        app.auto_jobs = {"selected": selected_job, "other": other_job}
        app._selected_job = lambda: selected_job
        app._save_auto_jobs = lambda: None
        app._refresh_job_tree = lambda: None
        app.auto_status_var = Value()
        with patch("app.messagebox.askyesno", return_value=True):
            app._stop_selected_job()
        self.assertEqual(selected_job["status"], "stopping")
        self.assertTrue(selected_job["cancel_requested"])
        self.assertEqual(other_job["status"], "queued")
        with self.assertRaises(AutoJobCancelled):
            app._check_auto_job_cancelled(selected_job)

    def test_worker_marks_requested_job_cancelled_and_runs_next_job(self):
        class Value:
            def set(self, _value):
                return None

        class Root:
            def after(self, _delay, callback):
                callback()

        app = RfbsListingApp.__new__(RfbsListingApp)
        app.busy = False
        app.active_job_id = ""
        app.auto_job_worker_running = True
        app.auto_job_lock = threading.RLock()
        app.auto_job_queue = queue.Queue()
        app.auto_status_var = Value()
        app.root = Root()
        app._save_auto_jobs = lambda: None
        app._refresh_job_tree = lambda: None
        app._ui_call = lambda callback, **_kwargs: callback()
        app._workspace_payload = lambda: {}
        app._log = lambda _message: None
        completed = []
        app._run_auto_job = lambda job: (
            (_ for _ in ()).throw(AutoJobCancelled("selected only"))
            if job["id"] == "stop" else completed.append(job["id"])
        )
        app._start_auto_job_worker = lambda: None
        app.auto_jobs = {
            "stop": {"id": "stop", "status": "queued", "stage": 2, "inputs": {"offer_id": "STOP"}},
            "next": {"id": "next", "status": "queued", "stage": 0, "inputs": {"offer_id": "NEXT"}},
        }
        app.auto_job_queue.put("stop")
        app.auto_job_queue.put("next")
        app._auto_job_worker()
        self.assertEqual(app.auto_jobs["stop"]["status"], "cancelled")
        self.assertEqual(app.auto_jobs["next"]["status"], "completed")
        self.assertEqual(completed, ["next"])

    def test_import_wait_can_be_interrupted_without_new_import_request(self):
        api = OzonApi.__new__(OzonApi)
        calls = []
        api.import_info = lambda task_id: calls.append(task_id) or {"result": {"items": []}}
        def cancel():
            raise AutoJobCancelled("stop selected")
        with self.assertRaises(AutoJobCancelled):
            api.wait_import(123, attempts=30, interval=3, cancel_check=cancel)
        self.assertEqual(calls, [])

    def test_ozon_api_accepts_optional_http_proxy_and_uses_split_timeouts(self):
        api = OzonApi("client", "key", proxy_url="127.0.0.1:7890")
        self.assertEqual(api.proxy_url, "http://127.0.0.1:7890")
        self.assertEqual(api.session.proxies["https"], "http://127.0.0.1:7890")
        self.assertEqual(api.timeout, (20, 60))
        self.assertEqual(api.catalog_language, "ZH_HANS")
        with self.assertRaisesRegex(ValueError, "代理地址格式错误"):
            OzonApi("client", "key", proxy_url="socks5://127.0.0.1:1080")
        with self.assertRaisesRegex(ValueError, "类目语言"):
            OzonApi("client", "key", catalog_language="ZH_CN")

    def test_chinese_is_allowed_only_for_live_dictionary_values(self):
        definitions = [
            {"id": 1, "name": "颜色", "dictionary_id": 10},
            {"id": 2, "name": "型号", "dictionary_id": 0},
        ]
        dictionary_attributes = [{
            "id": 1, "values": [{"value": "黑色", "dictionary_value_id": 100}],
        }]
        self.assertEqual(unsupported_chinese_submission_fields(
            "Черный рюкзак", "Описание", ["рюкзак"], dictionary_attributes, [], definitions,
        ), [])
        text_attributes = dictionary_attributes + [{"id": 2, "values": [{"value": "测试型号"}]}]
        invalid = unsupported_chinese_submission_fields(
            "中文标题", "Описание", ["рюкзак"], text_attributes, [], definitions,
        )
        self.assertEqual(invalid, ["商品标题", "文本属性：型号"])

    def test_ozon_connection_timeout_has_actionable_proxy_message(self):
        class Session:
            def post(self, *_args, **_kwargs):
                raise requests.ConnectionError("ConnectTimeoutError: connection timed out")
        api = OzonApi("client", "key")
        api.session = Session()
        with self.assertRaisesRegex(RuntimeError, "已自动重试 3 次.*HTTP 代理地址"):
            api.category_tree()

    def test_workspace_payload_keeps_work_but_excludes_api_secrets(self):
        class Value:
            def __init__(self, value):
                self.value = value
            def get(self, *_args):
                return self.value

        app = object.__new__(RfbsListingApp)
        app.vars = {key: Value("saved-" + key) for key in WORKSPACE_FIELD_KEYS}
        app.vars["ozon_api_key"] = Value("must-not-be-written")
        app.vars["oss_access_key_secret"] = Value("must-not-be-written-either")
        app.description_text = Value("saved description")
        app.reference = ReferenceProduct("https://ozon.ru/product/123456/", "Saved title")
        app.source_images = ["C:/images/source.jpg"]
        app.local_images = ["C:/images/ready.jpg"]
        app.uploaded_urls = ["https://cdn.example/ready.jpg"]
        app.primary_reference_index = 0
        app.category_combo = Value("Category | 10/20")
        app.warehouse_combo = Value("Warehouse | ID=30")
        app.attribute_definitions = [{"id": 40, "name": "Цвет"}]
        app.attribute_fact_suggestions = {40: ("Цвет товара", "Черный")}
        app.vision_analysis = {
            "poster_text_lines_ru": ["Городской рюкзак"],
            "visual_facts": [],
        }
        app.attribute_text = Value('[{"id": 40}]')
        app.complex_attribute_text = Value("[]")

        payload = app._workspace_payload()
        raw = json.dumps(payload, ensure_ascii=False)
        self.assertEqual(payload["fields"]["title"], "saved-title")
        self.assertEqual(payload["description"], "saved description")
        self.assertEqual(payload["vision_analysis"]["poster_text_lines_ru"], ["Городской рюкзак"])
        self.assertNotIn("ozon_api_key", payload["fields"])
        self.assertNotIn("must-not-be-written", raw)

    def test_workspace_reference_restore_ignores_bad_shapes(self):
        restored = RfbsListingApp._reference_from_dict({
            "source_url": "https://ozon.ru/product/123456/",
            "title": "Рюкзак",
            "images": ["", "https://cdn.example/a.jpg"],
            "properties": {"Артикул": 123456},
            "tags": ["#рюкзак"],
        })
        self.assertEqual(restored.images, ["https://cdn.example/a.jpg"])
        self.assertEqual(restored.properties, {"Артикул": "123456"})
        self.assertEqual(restored.tags, ["#рюкзак"])

    def test_job_repair_window_has_interactive_attribute_editor(self):
        try:
            root = tk.Tk()
            root.withdraw()
        except tk.TclError as error:
            self.skipTest(f"Tk 不可用：{error}")
        try:
            app = object.__new__(RfbsListingApp)
            app.root = root
            app.category_displays = []
            app.category_by_display = {}
            job = {
                "id": "repair-test",
                "status": "failed",
                "stage": 5,
                "inputs": {
                    "offer_id": "SKU-REPAIR",
                    "category_id": "10",
                    "type_id": "20",
                },
                "error": '{"attribute_id": 700, "message": "bad dictionary value"}',
                "state": {
                    "fields": {},
                    "attributes_json": '[{"id":700,"values":[{"value":"bad"}]}]',
                    "complex_attributes_json": "[]",
                    "attribute_definitions": [{
                        "id": 700,
                        "name": "颜色",
                        "name_ru": "Цвет",
                        "dictionary_id": 70,
                        "is_required": True,
                    }],
                },
            }
            window = app._build_job_repair_window(job)
            root.update_idletasks()

            def descendants(widget):
                values = []
                for child in widget.winfo_children():
                    values.append(child)
                    values.extend(descendants(child))
                return values

            widgets = descendants(window)
            notebooks = [widget for widget in widgets if isinstance(widget, ttk.Notebook)]
            self.assertEqual(len(notebooks), 1)
            notebook = notebooks[0]
            tab_names = [notebook.tab(tab_id, "text") for tab_id in notebook.tabs()]
            self.assertIn("商品属性修复", tab_names)
            self.assertEqual(notebook.tab(notebook.select(), "text"), "商品属性修复")
            button_names = {
                widget.cget("text") for widget in widgets if isinstance(widget, ttk.Button)
            }
            self.assertTrue({
                "加载/刷新当前类目属性",
                "保存文本值",
                "加载全部选项",
                "采用选中值",
                "清空该属性",
            }.issubset(button_names))
            attribute_trees = [
                widget for widget in widgets
                if isinstance(widget, ttk.Treeview) and widget.exists("700")
            ]
            self.assertEqual(len(attribute_trees), 1)
            self.assertIn("problem", attribute_trees[0].item("700", "tags"))
            self.assertEqual(attribute_trees[0].selection(), ("700",))
            window.destroy()
        finally:
            root.destroy()

    def test_workspace_round_trip_restores_visible_work(self):
        import app as app_module

        original_workspace = app_module.WORKSPACE_PATH
        original_output = app_module.OUTPUT_DIR
        original_config = app_module.CONFIG_PATH
        try:
            with tempfile.TemporaryDirectory() as folder:
                temporary = Path(folder)
                image_path = temporary / "ready.jpg"
                image_path.write_bytes(b"test-image-placeholder")
                app_module.WORKSPACE_PATH = temporary / "workspace_state.json"
                app_module.OUTPUT_DIR = temporary / "output"
                app_module.CONFIG_PATH = temporary / "config.json"

                first_root = tk.Tk()
                first_root.withdraw()
                first = RfbsListingApp(first_root)
                first.vars["title"].set("Сохраненный рюкзак")
                first.vars["category_id"].set("17027467")
                first.description_text.delete("1.0", tk.END)
                first.description_text.insert("1.0", "Сохраненное описание")
                first.local_images = [str(image_path)]
                first.source_images = [str(image_path)]
                first.uploaded_urls = ["https://cdn.example/ready.jpg"]
                first.vision_analysis = {
                    "poster_text_lines_ru": ["Сохраненный рюкзак"],
                    "visual_facts": [],
                }
                first.attribute_text.delete("1.0", tk.END)
                first.attribute_text.insert("1.0", '[{"id": 85, "values": [{"value": "Черный"}]}]')
                first._save_workspace()
                first_root.destroy()

                second_root = tk.Tk()
                second_root.withdraw()
                second = RfbsListingApp(second_root)
                self.assertEqual(second.vars["title"].get(), "Сохраненный рюкзак")
                self.assertEqual(second.vars["category_id"].get(), "17027467")
                self.assertEqual(second.description_text.get("1.0", tk.END).strip(), "Сохраненное описание")
                self.assertEqual(second.local_images, [str(image_path)])
                self.assertEqual(second.uploaded_urls, ["https://cdn.example/ready.jpg"])
                self.assertEqual(second.vision_analysis["poster_text_lines_ru"], ["Сохраненный рюкзак"])
                self.assertIn('"id": 85', second.attribute_text.get("1.0", tk.END))
                second_root.destroy()
        except tk.TclError as error:
            self.skipTest(f"Tk 不可用：{error}")
        finally:
            app_module.WORKSPACE_PATH = original_workspace
            app_module.OUTPUT_DIR = original_output
            app_module.CONFIG_PATH = original_config


if __name__ == "__main__":
    unittest.main()
