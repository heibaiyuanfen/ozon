import unittest

from competitor_collector import article, high_resolution


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


if __name__ == "__main__":
    unittest.main()
