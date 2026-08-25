from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from ozon_app.api import SellerApiClient
from ozon_app.database import Database
from ozon_app.services import AppService


class SupplyApiTests(unittest.TestCase):
    def test_supply_list_details_timeslots_and_single_write(self) -> None:
        class FakeHttp:
            def __init__(self) -> None:
                self.calls = []

            def request(self, method, url, **kwargs):
                body = kwargs.get("body") or {}
                self.calls.append((url, body, kwargs))
                if url.endswith("/v3/supply-order/list"):
                    return {"order_ids": [101], "last_id": ""}
                if url.endswith("/v3/supply-order/get"):
                    self.assertEqual(body, {"order_ids": [101]})
                    return {"orders": [{"order_id": 101, "supplies": []}]}
                if url.endswith("/v2/supply-order/timeslot/list"):
                    return {"timeslots_info": {"timeslots": [
                        {"from": "2026-08-20T06:00:00Z", "to": "2026-08-20T07:00:00Z"},
                    ]}}
                if url.endswith("/v1/supply-order/timeslot/update"):
                    self.assertEqual(body["supply_order_id"], 101)
                    self.assertEqual(body["timeslot"]["from"], "2026-08-20T06:00:00Z")
                    self.assertEqual(kwargs.get("max_attempts"), 1)
                    self.assertFalse(kwargs.get("retry_429"))
                    return {"operation_id": "op-1"}
                raise AssertionError(url)

            def assertEqual(self, left, right):
                if left != right:
                    raise AssertionError(f"{left!r} != {right!r}")

            def assertFalse(self, value):
                if value:
                    raise AssertionError(value)

        http = FakeHttp()
        client = SellerApiClient("1", "key", http=http)
        orders = client.supply_orders()
        slots = client.supply_order_timeslots(101, "2026-08-20", "2026-08-30")
        update = client.update_supply_order_timeslot(
            101, "2026-08-20T06:00:00Z", "2026-08-20T07:00:00Z"
        )
        self.assertEqual(orders[0]["order_id"], 101)
        self.assertEqual(slots[0]["from"], "2026-08-20T06:00:00Z")
        self.assertEqual(update["operation_id"], "op-1")

    def test_new_supply_draft_flow_uses_current_endpoints_and_single_writes(self) -> None:
        class FakeHttp:
            def __init__(self) -> None:
                self.calls = []

            def request(self, method, url, **kwargs):
                body = kwargs.get("body") or {}
                self.calls.append((url, body, kwargs))
                if url.endswith("/v1/warehouse/fbo/list"):
                    return {"search": [{
                        # Ozon may return the protobuf default enum as 0.  It
                        # must never be sent back to the create endpoint.
                        "warehouse_id": 55, "warehouse_type": "0",
                        "name": "Test dropoff",
                    }]}
                if url.endswith("/v1/draft/crossdock/create"):
                    self._single_write(kwargs)
                    self._equal(body["cluster_info"]["items"], [{"sku": 1001, "quantity": 25}])
                    self._equal(body["delivery_info"]["drop_off_warehouse"]["warehouse_id"], 55)
                    self._equal(
                        body["delivery_info"]["drop_off_warehouse"]["warehouse_type"],
                        "CROSS_DOCK",
                    )
                    return {"draft_id": 901, "errors": []}
                if url.endswith("/v1/draft/direct/create"):
                    self._single_write(kwargs)
                    if "delivery_info" in body:
                        raise AssertionError("direct request must not include delivery_info")
                    return {"draft_id": 902, "errors": []}
                if url.endswith("/v2/draft/create/info"):
                    return {"status": "SUCCESS", "clusters": [{
                        "macrolocal_cluster_id": 77,
                        "warehouses": [{"storage_warehouse": {"warehouse_id": 88}}],
                    }]}
                if url.endswith("/v2/draft/timeslot/info"):
                    self._equal(body["selected_cluster_warehouses"], [{
                        "macrolocal_cluster_id": 77, "storage_warehouse_id": 88,
                    }])
                    return {"result": {"drop_off_warehouse_timeslots": {"days": [{
                        "date_in_timezone": "2026-08-20",
                        "timeslots": [{
                            "from_in_timezone": "2026-08-20T10:00:00+03:00",
                            "to_in_timezone": "2026-08-20T11:00:00+03:00",
                        }],
                    }]}}}
                if url.endswith("/v2/draft/supply/create"):
                    self._single_write(kwargs)
                    self._equal(body["draft_id"], 901)
                    self._equal(body["timeslot"]["from_in_timezone"], "2026-08-20T10:00:00+03:00")
                    return {"draft_id": 901, "error_reasons": []}
                if url.endswith("/v2/draft/supply/create/status"):
                    return {"status": "SUCCESS", "order_id": 777}
                raise AssertionError(url)

            @staticmethod
            def _equal(left, right):
                if left != right:
                    raise AssertionError(f"{left!r} != {right!r}")

            @staticmethod
            def _single_write(kwargs):
                if kwargs.get("max_attempts") != 1 or kwargs.get("retry_429") is not False:
                    raise AssertionError("remote write must be sent once without 429 retry")

        http = FakeHttp()
        client = SellerApiClient("1", "key", http=http)
        dropoff = client.search_supply_dropoff_warehouses("Test", "CROSSDOCK")[0]
        self.assertEqual(dropoff["warehouse_id"], 55)
        self.assertEqual(dropoff["warehouse_type"], "CROSS_DOCK")
        crossdock = client.create_supply_draft(
            1001, 25, 77, "CROSSDOCK",
            delivery_info={
                "type": "DROPOFF",
                "drop_off_warehouse": {
                    "warehouse_id": 55, "warehouse_type": "CROSS_DOCK",
                },
            },
        )
        direct = client.create_supply_draft(1001, 25, 77, "DIRECT")
        self.assertEqual(crossdock["draft_id"], 901)
        self.assertEqual(direct["draft_id"], 902)
        self.assertEqual(client.supply_draft_info(901)["status"], "SUCCESS")
        slots = client.supply_draft_timeslots(
            901, "CROSSDOCK", 77, 88, "2026-08-20", "2026-08-30"
        )
        self.assertIn("result", slots)
        created = client.create_supply_from_draft(
            901, "CROSSDOCK", 77, 88,
            "2026-08-20T10:00:00+03:00", "2026-08-20T11:00:00+03:00",
        )
        self.assertEqual(created["draft_id"], 901)
        self.assertEqual(client.supply_from_draft_status(901)["order_id"], 777)


class SupplyServiceTests(unittest.TestCase):
    def test_dropoff_search_uses_per_shop_cache_until_forced(self) -> None:
        class FakeSeller:
            calls = 0

            def search_supply_dropoff_warehouses(
                self, search, supply_type, progress=None, cancel_event=None,
            ):
                self.calls += 1
                return [{
                    "warehouse_id": 55, "warehouse_type": "0",
                    "name": "Test cached dropoff", "address": "Moscow",
                }]

        with TemporaryDirectory() as folder:
            database = Database(Path(folder) / "test.db")
            database.cache_supply_dropoffs([{
                "warehouse_id": 55, "warehouse_type": "0",
                "name": "Test cached dropoff", "address": "Moscow",
            }], "CROSSDOCK")
            service = AppService(database)
            fake = FakeSeller()
            service.seller_client = lambda credentials=None: fake  # type: ignore[method-assign]

            cached = service.search_supply_dropoffs("Test", "CROSSDOCK")
            self.assertTrue(cached["cached"])
            self.assertEqual(fake.calls, 0)
            self.assertEqual(cached["warehouses"][0]["warehouse_type"], "CROSS_DOCK")

            refreshed = service.search_supply_dropoffs(
                "Test", "CROSSDOCK", force_refresh=True
            )
            self.assertFalse(refreshed["cached"])
            self.assertEqual(fake.calls, 1)

    def test_crossdock_normalization_local_time_and_booking_log(self) -> None:
        class FakeSeller:
            def supply_orders(self, progress=None, cancel_event=None):
                return [{
                    "order_id": 101,
                    "order_number": "FBO-101",
                    "state": "ORDER_STATE_DATA_FILLING",
                    "created_date": "2026-08-14T01:00:00Z",
                    "drop_off_warehouse": {"name": "供货仓 A"},
                    "timeslot": {
                        "timeslot": {
                            "from": "2026-08-20T06:00:00Z",
                            "to": "2026-08-20T07:00:00Z",
                        },
                        "timezone_info": {
                            "offset": "10800s", "iana_name": "Europe/Moscow",
                        },
                    },
                    "supplies": [{
                        "supply_id": 1,
                        "is_crossdock": True,
                        "macrolocal_cluster_id": 77,
                        "state": "SUPPLY_STATE_DATA_FILLING",
                        "storage_warehouse": {"name": "存储仓 B"},
                    }],
                }]

            def supply_order_timeslots(
                self, order_id, date_from, date_to, progress=None, cancel_event=None,
            ):
                return [{
                    "from": "2026-08-20T06:00:00Z",
                    "to": "2026-08-20T07:00:00Z",
                }]

            def update_supply_order_timeslot(self, order_id, start, end):
                return {"operation_id": "operation-1"}

        with TemporaryDirectory() as folder:
            database = Database(Path(folder) / "test.db")
            service = AppService(database)
            service.seller_client = lambda credentials=None: FakeSeller()  # type: ignore[method-assign]

            appointment = service.supply_appointments()["orders"][0]
            self.assertTrue(appointment["is_crossdock"])
            self.assertEqual(appointment["clusters"], "77")
            self.assertEqual(appointment["local_timeslot_from"], "2026-08-20T09:00+03:00")

            timeslot = service.supply_timeslots(
                101, "2026-08-20", "2026-08-30", 10800
            )["timeslots"][0]
            self.assertEqual(timeslot["local_time"], "09:00–10:00")

            result = service.book_supply_timeslot(
                101, timeslot["from"], timeslot["to"]
            )
            self.assertEqual(result["operation_id"], "operation-1")
            log = next(
                row for row in database.recent_logs(10)
                if row["source"] == "FBO Timeslot Update"
            )
            self.assertEqual(log["status"], "success")


if __name__ == "__main__":
    unittest.main()
