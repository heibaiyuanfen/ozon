import unittest

from competitor_collector import (
    article,
    blocked_page_reason,
    close_context_safely,
    failure_payload,
    high_resolution,
    should_fallback_immediately,
)


class CompetitorCollectorTests(unittest.TestCase):
    def test_article_uses_product_identifier_from_ozon_url(self):
        self.assertEqual(
            article("https://www.ozon.ru/product/example-3718744923/"),
            "3718744923",
        )

    def test_high_resolution_keeps_image_identity_and_upgrades_size(self):
        source = "https://cdn1.ozone.ru/s3/multimedia-a/wc250/item.jpg"
        self.assertEqual(
            high_resolution(source),
            "https://cdn1.ozone.ru/s3/multimedia-a/wc2000/item.jpg",
        )

    def test_blocked_page_reason_recognizes_ozon_synthetic_offline_page(self):
        self.assertEqual(
            blocked_page_reason("Похоже, нет соединения"),
            "похоже, нет соединения",
        )
        self.assertEqual(blocked_page_reason("Ozon", "Доступ ограничен"), "доступ ограничен")
        self.assertEqual(blocked_page_reason("Ozon product", "正常商品页"), "")
        self.assertTrue(should_fallback_immediately("похоже, нет соединения"))
        self.assertFalse(should_fallback_immediately("captcha"))

    def test_cleanup_does_not_mask_the_collection_error(self):
        class ClosedContext:
            def close(self):
                raise RuntimeError("Target page, context or browser has been closed")

        close_context_safely(ClosedContext())

    def test_failure_payload_explains_ozon_access_restriction(self):
        payload = failure_payload(RuntimeError("Ozon blocked all browser sessions: chrome captcha"))
        self.assertEqual(payload["status"], "blocked")
        self.assertIn("不是商品下架", payload["error"])
        self.assertIn("chrome captcha", payload["error"])


if __name__ == "__main__":
    unittest.main()
