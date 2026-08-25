from __future__ import annotations

from dataclasses import asdict, dataclass
from decimal import Decimal, InvalidOperation, ROUND_CEILING


WB_TAIL_TARIFF_VERSION = "2026-07-15"
WB_TAIL_VOLUME_BANDS = (
    (Decimal("0.2"), Decimal("23")),
    (Decimal("0.4"), Decimal("26")),
    (Decimal("0.6"), Decimal("29")),
    (Decimal("0.8"), Decimal("30")),
    (Decimal("1"), Decimal("32")),
)

@dataclass(frozen=True)
class WbPricingBreakdown:
    fulfillment_mode: str
    currency_code: str
    list_price: Decimal
    discounted_price: Decimal
    target_profit_margin_percent: Decimal
    actual_profit_margin_percent: Decimal
    purchase_cost_cny: Decimal
    label_fee_cny: Decimal
    cny_rub_rate: Decimal
    cross_border_shipping_cny: Decimal
    first_mile_shipping_cny: Decimal
    warehouse_operation_fee_cny: Decimal
    tail_delivery_rub: Decimal
    logistics_total: Decimal
    fixed_cost: Decimal
    sales_commission: Decimal
    advertising: Decimal
    cargo_loss: Decimal
    profit: Decimal
    billable_weight_kg: Decimal
    volume_liters: Decimal

    def json_values(self) -> dict[str, str]:
        return {
            key: value if isinstance(value, str) else format(value, "f")
            for key, value in asdict(self).items()
        }


def _number(value, label: str, *, allow_zero=False) -> Decimal:
    try:
        number = Decimal(str(value).strip())
    except (InvalidOperation, ValueError) as error:
        raise ValueError(label + "必须是有效数字") from error
    if not number.is_finite() or number < 0 or (not allow_zero and number == 0):
        raise ValueError(label + ("不能小于 0" if allow_zero else "必须大于 0"))
    return number


def _rate(value, label: str, *, allow_zero=True) -> Decimal:
    percent = _number(value, label, allow_zero=allow_zero)
    if percent >= 100:
        raise ValueError(label + "必须小于 100%")
    return percent / Decimal("100")


def _validate_standard_dimensions(length_cm, width_cm, height_cm, weight_kg):
    dimensions = (
        _number(length_cm, "WB 包装长度"),
        _number(width_cm, "WB 包装宽度"),
        _number(height_cm, "WB 包装高度"),
    )
    weight = _number(weight_kg, "WB 包装重量")
    if max(dimensions) > Decimal("120") or sum(dimensions) > Decimal("200"):
        raise ValueError("当前 WB 核价公式仅支持普通商品：单边不超过 120cm 且三边和不超过 200cm")
    return dimensions, weight


def calculate_cross_border_shipping_cny(weight_kg) -> tuple[Decimal, Decimal]:
    weight = _number(weight_kg, "WB 包装重量")
    billable_weight = (
        weight * Decimal("10")
    ).to_integral_value(rounding=ROUND_CEILING) / Decimal("10")
    if billable_weight > Decimal("20"):
        raise ValueError("WB 标准跨境线路只支持不超过 20kg 的商品")
    if billable_weight <= Decimal("0.3"):
        return billable_weight * Decimal("58") + Decimal("2"), billable_weight
    return billable_weight * Decimal("43") + Decimal("8"), billable_weight


def calculate_wb_tail_delivery_rub(
    length_cm, width_cm, height_cm, *, logistics_coefficient_percent=100,
    localization_index=1,
) -> tuple[Decimal, Decimal]:
    length = _number(length_cm, "WB 包装长度")
    width = _number(width_cm, "WB 包装宽度")
    height = _number(height_cm, "WB 包装高度")
    coefficient = _number(logistics_coefficient_percent, "WB 仓库物流系数") / Decimal("100")
    localization = _number(localization_index, "WB 本地化指数")
    volume = length * width * height / Decimal("1000")
    base = next(
        (tariff for maximum, tariff in WB_TAIL_VOLUME_BANDS if volume <= maximum),
        None,
    )
    if base is None:
        base = Decimal("46") + (volume - Decimal("1")) * Decimal("14")
    return base * coefficient * localization, volume


def calculate_wb_price(
    *, fulfillment_mode, purchase_cost_cny, label_fee_cny,
    target_profit_margin_percent, sales_commission_percent,
    advertising_percent, cargo_loss_percent, cny_rub_rate,
    weight_kg, length_cm, width_cm, height_cm, discount_percent=0,
    first_mile_shipping_cny=0, warehouse_operation_fee_cny=0,
    logistics_coefficient_percent=100, localization_index=1,
) -> WbPricingBreakdown:
    mode = str(fulfillment_mode or "").strip()
    if mode not in {"cross_border", "overseas_warehouse"}:
        raise ValueError("WB 核价方式无效")
    dimensions, weight = _validate_standard_dimensions(
        length_cm, width_cm, height_cm, weight_kg,
    )
    if mode == "overseas_warehouse" and weight >= Decimal("25"):
        raise ValueError("当前 WB 海外仓尾程公式仅支持包装重量小于 25kg 的普通商品")

    purchase = _number(purchase_cost_cny, "WB 采购成本", allow_zero=True)
    label = _number(label_fee_cny, "WB 贴标费", allow_zero=True)
    exchange_rate = (
        _number(cny_rub_rate, "CNY→RUB 汇率")
        if mode == "overseas_warehouse" else Decimal("0")
    )
    target_margin = _rate(target_profit_margin_percent, "WB 目标利润率", allow_zero=True)
    commission_rate = _rate(sales_commission_percent, "WB 人工佣金", allow_zero=False)
    advertising_rate = _rate(advertising_percent, "WB 广告费率", allow_zero=True)
    cargo_loss_rate = _rate(cargo_loss_percent, "WB 货损率", allow_zero=True)
    discount_rate = _rate(discount_percent, "WB 折扣", allow_zero=True)

    retained_rate = Decimal("1") - commission_rate - advertising_rate - target_margin
    if retained_rate <= 0:
        raise ValueError("WB 佣金、广告费率与目标利润率合计必须小于 100%")

    cross_border_shipping = Decimal("0")
    billable_weight = weight
    first_mile = Decimal("0")
    warehouse_operation = Decimal("0")
    tail_delivery = Decimal("0")
    volume = dimensions[0] * dimensions[1] * dimensions[2] / Decimal("1000")
    if mode == "cross_border":
        cross_border_shipping, billable_weight = calculate_cross_border_shipping_cny(weight)
        currency_code = "CNY"
        logistics_total = cross_border_shipping
        fixed_cost = purchase + label + logistics_total
    else:
        first_mile = _number(first_mile_shipping_cny, "WB 头程运费", allow_zero=True)
        warehouse_operation = _number(
            warehouse_operation_fee_cny, "WB 海外仓操作费", allow_zero=True,
        )
        tail_delivery, volume = calculate_wb_tail_delivery_rub(
            *dimensions,
            logistics_coefficient_percent=logistics_coefficient_percent,
            localization_index=localization_index,
        )
        currency_code = "RUB"
        logistics_total = (first_mile + warehouse_operation) * exchange_rate + tail_delivery
        fixed_cost = (purchase + label) * exchange_rate + logistics_total

    cargo_loss = fixed_cost * cargo_loss_rate
    required_discounted_price = (fixed_cost + cargo_loss) / retained_rate
    list_price = (
        required_discounted_price / (Decimal("1") - discount_rate)
    ).to_integral_value(rounding=ROUND_CEILING)
    discounted_price = list_price * (Decimal("1") - discount_rate)
    sales_commission = discounted_price * commission_rate
    advertising = discounted_price * advertising_rate
    profit = discounted_price - sales_commission - advertising - fixed_cost - cargo_loss
    actual_margin = profit / discounted_price * Decimal("100")

    return WbPricingBreakdown(
        fulfillment_mode=mode,
        currency_code=currency_code,
        list_price=list_price,
        discounted_price=discounted_price,
        target_profit_margin_percent=target_margin * Decimal("100"),
        actual_profit_margin_percent=actual_margin,
        purchase_cost_cny=purchase,
        label_fee_cny=label,
        cny_rub_rate=exchange_rate,
        cross_border_shipping_cny=cross_border_shipping,
        first_mile_shipping_cny=first_mile,
        warehouse_operation_fee_cny=warehouse_operation,
        tail_delivery_rub=tail_delivery,
        logistics_total=logistics_total,
        fixed_cost=fixed_cost,
        sales_commission=sales_commission,
        advertising=advertising,
        cargo_loss=cargo_loss,
        profit=profit,
        billable_weight_kg=billable_weight,
        volume_liters=volume,
    )
