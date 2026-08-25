from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from ozon_app.api_bundle import build_api_bundle, read_api_bundle, write_api_bundle
from ozon_app.services import Credentials
from ozon_app.shops import ShopProfile


class ApiBundleTests(unittest.TestCase):
    def test_named_local_and_cross_border_profiles_roundtrip(self) -> None:
        profiles = [
            (
                ShopProfile("a", "本土一店", "local", "a.db", "本土一店 API"),
                Credentials(seller_client_id="100", seller_api_key="secret-a"),
            ),
            (
                ShopProfile("b", "跨境一店", "cross_border", "b.db", "跨境一店 API"),
                Credentials(seller_client_id="200", seller_api_key="secret-b"),
            ),
        ]
        with TemporaryDirectory() as folder:
            path = Path(folder) / "profiles.ozon-api.json"
            write_api_bundle(path, build_api_bundle(profiles))
            rows = read_api_bundle(path)
        self.assertEqual([row["api_name"] for row in rows], ["本土一店 API", "跨境一店 API"])
        self.assertEqual([row["shop_kind"] for row in rows], ["local", "cross_border"])
        self.assertEqual(rows[0]["credentials"].seller_api_key, "secret-a")
        self.assertEqual(rows[1]["credentials"].seller_client_id, "200")


if __name__ == "__main__":
    unittest.main()
