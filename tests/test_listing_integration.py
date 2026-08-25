from pathlib import Path
from tempfile import TemporaryDirectory
import json
import unittest

from ozon_app.listing_integration import (
    find_listing_tool_executable, sync_listing_shop_profile,
)


class ListingIntegrationTests(unittest.TestCase):
    def test_profile_upsert_preserves_other_settings_and_selects_shop(self) -> None:
        with TemporaryDirectory() as folder:
            config = Path(folder) / "config.json"
            config.write_text(json.dumps({
                "ozon": {
                    "selected_shop_id": "old",
                    "shops": [{"id": "old", "name": "Old", "client_id": "1"}],
                },
                "image": {"model": "keep-me"},
            }), encoding="utf-8")
            first = sync_listing_shop_profile(
                config, profile_id="analytics-cross-1", name="Cross One",
                client_id="200", api_key="secret-one",
            )
            self.assertEqual(first["shop_count"], 2)
            sync_listing_shop_profile(
                config, profile_id="analytics-cross-1", name="Cross Renamed",
                client_id="201", api_key="secret-two",
            )
            saved = json.loads(config.read_text(encoding="utf-8"))
            self.assertEqual(saved["image"], {"model": "keep-me"})
            self.assertEqual(saved["ozon"]["selected_shop_id"], "analytics-cross-1")
            self.assertEqual(len(saved["ozon"]["shops"]), 2)
            current = next(
                row for row in saved["ozon"]["shops"]
                if row["id"] == "analytics-cross-1"
            )
            self.assertEqual(current["name"], "Cross Renamed")
            self.assertEqual(current["client_id"], "201")
            self.assertEqual(current["api_key"], "secret-two")

    def test_executable_discovery_prefers_packaged_copy(self) -> None:
        with TemporaryDirectory() as folder:
            root = Path(folder)
            packaged = root / "package" / "tools" / "Ozon_RFBS上品工具"
            external = root / "external" / "发布" / "Ozon_RFBS上品工具"
            packaged.mkdir(parents=True)
            external.mkdir(parents=True)
            package_exe = packaged / "Ozon_RFBS上品工具.exe"
            external_exe = external / "Ozon_RFBS上品工具.exe"
            package_exe.write_bytes(b"package")
            external_exe.write_bytes(b"external")
            self.assertEqual(
                find_listing_tool_executable(root / "external", root / "package"),
                package_exe,
            )


if __name__ == "__main__":
    unittest.main()
