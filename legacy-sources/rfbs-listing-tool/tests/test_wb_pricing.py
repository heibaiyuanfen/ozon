from __future__ import annotations

import sys
import unittest
from decimal import Decimal
from pathlib import Path


APP_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(APP_DIR))

from wb_pricing import (
    calculate_cross_border_shipping_cny,
    calculate_wb_price,
    calculate_wb_tail_delivery_rub,
)


class WbPricingTests(unittest.TestCase):
    def test_standard_cross_border_shipping_rounds_up_each_100g(self):
        shipping, billable = calculate_cross_border_shipping_cny("0.27")
        self.assertEqual(billable, Decimal("0.3"))
        self.assertEqual(shipping, Decimal("19.4"))

        shipping, billable = calculate_cross_border_shipping_cny("3.05")
        self.assertEqual(billable, Decimal("3.1"))
        self.assertEqual(shipping, Decimal("141.3"))

    def test_official_tail_delivery_volume_bands(self):
        tail, volume = calculate_wb_tail_delivery_rub("10", "10", "5.5")
        self.assertEqual(volume, Decimal("0.55"))
        self.assertEqual(tail, Decimal("29"))

        tail, volume = calculate_wb_tail_delivery_rub(
            "20", "10", "9", logistics_coefficient_percent="110",
            localization_index="1.2",
        )
        self.assertEqual(volume, Decimal("1.8"))
        self.assertEqual(tail, Decimal("75.504"))

    def test_cross_border_price_reaches_target_margin_after_discount(self):
        result = calculate_wb_price(
            fulfillment_mode="cross_border",
            purchase_cost_cny="15", label_fee_cny="2",
            target_profit_margin_percent="20", sales_commission_percent="18",
            advertising_percent="15", cargo_loss_percent="10", cny_rub_rate="",
            weight_kg="0.27", length_cm="10", width_cm="10", height_cm="5.5",
            discount_percent="10",
        )
        self.assertEqual(result.cross_border_shipping_cny, Decimal("19.4"))
        self.assertEqual(result.currency_code, "CNY")
        self.assertEqual(result.cny_rub_rate, Decimal("0"))
        self.assertEqual(result.list_price, Decimal("95"))
        self.assertGreaterEqual(result.actual_profit_margin_percent, Decimal("20"))

    def test_overseas_price_includes_head_and_tail_delivery(self):
        result = calculate_wb_price(
            fulfillment_mode="overseas_warehouse",
            purchase_cost_cny="15", label_fee_cny="2",
            target_profit_margin_percent="20", sales_commission_percent="18",
            advertising_percent="15", cargo_loss_percent="10", cny_rub_rate="12",
            weight_kg="0.6", length_cm="10", width_cm="10", height_cm="5.5",
            first_mile_shipping_cny="50", warehouse_operation_fee_cny="5",
            logistics_coefficient_percent="100", localization_index="1",
            discount_percent="0",
        )
        self.assertEqual(result.first_mile_shipping_cny, Decimal("50"))
        self.assertEqual(result.warehouse_operation_fee_cny, Decimal("5"))
        self.assertEqual(result.tail_delivery_rub, Decimal("29"))
        self.assertEqual(result.currency_code, "RUB")
        self.assertEqual(result.logistics_total, Decimal("689"))
        self.assertEqual(result.list_price, Decimal("2090"))
        self.assertGreaterEqual(result.actual_profit_margin_percent, Decimal("20"))

    def test_exchange_rate_is_required_only_for_overseas_warehouse(self):
        cross = calculate_wb_price(
            fulfillment_mode="cross_border",
            purchase_cost_cny="15", label_fee_cny="2",
            target_profit_margin_percent="20", sales_commission_percent="18",
            advertising_percent="15", cargo_loss_percent="10", cny_rub_rate="",
            weight_kg="0.27", length_cm="10", width_cm="10", height_cm="5.5",
        )
        self.assertEqual(cross.currency_code, "CNY")
        with self.assertRaisesRegex(ValueError, "汇率"):
            calculate_wb_price(
                fulfillment_mode="overseas_warehouse",
                purchase_cost_cny="15", label_fee_cny="2",
                target_profit_margin_percent="20", sales_commission_percent="18",
                advertising_percent="15", cargo_loss_percent="10", cny_rub_rate="",
                weight_kg="0.6", length_cm="10", width_cm="10", height_cm="5.5",
            )

    def test_invalid_rates_and_standard_line_limits_are_rejected(self):
        with self.assertRaisesRegex(ValueError, "合计"):
            calculate_wb_price(
                fulfillment_mode="cross_border",
                purchase_cost_cny="1", label_fee_cny="0",
                target_profit_margin_percent="70", sales_commission_percent="20",
                advertising_percent="10", cargo_loss_percent="0", cny_rub_rate="12",
                weight_kg="1", length_cm="10", width_cm="10", height_cm="10",
            )
        with self.assertRaisesRegex(ValueError, "20kg"):
            calculate_cross_border_shipping_cny("20.01")


if __name__ == "__main__":
    unittest.main()
