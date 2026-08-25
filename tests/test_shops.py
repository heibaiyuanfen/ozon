import tempfile
import unittest
from pathlib import Path

from ozon_app.shops import ShopRegistry


class ShopRegistryTests(unittest.TestCase):
    def test_legacy_database_is_default_shop(self):
        with tempfile.TemporaryDirectory() as folder:
            registry = ShopRegistry(Path(folder))
            shop = registry.active()
            self.assertEqual(shop.id, "default")
            self.assertEqual(registry.database_path(shop).name, "ozon_analytics.db")

    def test_add_switch_update_and_safe_remove(self):
        with tempfile.TemporaryDirectory() as folder:
            registry = ShopRegistry(Path(folder))
            shop = registry.add(
                " 跨境一店 ", "cross_border", api_name="跨境一店 Seller API",
            )
            self.assertEqual(shop.name, "跨境一店")
            self.assertEqual(shop.resolved_api_name, "跨境一店 Seller API")
            self.assertEqual(shop.kind_label, "跨境店")
            registry.set_active(shop.id)
            self.assertEqual(ShopRegistry(Path(folder)).active().id, shop.id)
            updated = registry.update(
                shop.id, name="跨境旗舰店", kind="cross_border",
                api_name="跨境旗舰 API",
            )
            self.assertEqual(updated.name, "跨境旗舰店")
            self.assertEqual(updated.resolved_api_name, "跨境旗舰 API")
            self.assertEqual(updated.kind, "cross_border")
            previous_database = updated.database_file
            changed = registry.update(
                shop.id, name="本土旗舰店", kind="local", api_name="本土旗舰 API",
            )
            self.assertEqual(changed.kind, "local")
            self.assertNotEqual(changed.database_file, previous_database)
            state = registry._read()
            raw = next(item for item in state["shops"] if item["id"] == shop.id)
            self.assertEqual(raw["database_history"][-1]["database_file"], previous_database)
            registry.remove(shop.id)
            self.assertEqual(registry.active().id, "default")

    def test_cannot_remove_last_shop(self):
        with tempfile.TemporaryDirectory() as folder:
            registry = ShopRegistry(Path(folder))
            with self.assertRaises(ValueError):
                registry.remove("default")

    def test_api_profile_names_are_unique(self):
        with tempfile.TemporaryDirectory() as folder:
            registry = ShopRegistry(Path(folder))
            registry.add("本土二店", api_name="二店 API")
            with self.assertRaisesRegex(ValueError, "API 配置名称"):
                registry.add("跨境二店", "cross_border", api_name="二店 api")


if __name__ == "__main__":
    unittest.main()
