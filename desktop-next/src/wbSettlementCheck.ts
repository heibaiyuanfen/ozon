type Fields = Record<string, string | number | null | undefined>;

function number(row: Fields, ...keys: string[]): number | null {
  for (const key of keys) {
    const value = row[key];
    if (value != null && String(value).trim() && Number.isFinite(Number(value))) return Number(value);
  }
  return null;
}

const cents = (value: number) => Math.sign(value) * Math.round((Math.abs(value) + 1e-9) * 100);

export function settlement(details: Fields[]) {
  const groups = new Map<string, { checked: number; failed: number; pending: number; basis: number; commission: number; acquiring: number; expected: number; reported: number; delivery: number; other: number; adjustment: number; unclassified: number }>();
  for (const row of details) {
    const currency = String(row.currency ?? row.currencyName ?? row.currency_name ?? "").trim().toUpperCase() || "币种未知";
    const group = groups.get(currency) ?? { checked: 0, failed: 0, pending: 0, basis: 0, commission: 0, acquiring: 0, expected: 0, reported: 0, delivery: 0, other: 0, adjustment: 0, unclassified: 0 };
    groups.set(currency, group);
    const type = String(row.docTypeName ?? row.doc_type_name ?? "");
    const operation = String(row.sellerOperName ?? row.supplierOperName ?? row.supplier_oper_name ?? "");
    const sale = /^(销售|Продажа|sale)$/i;
    const refund = /^(退货|退款|Возврат|return)$/i;
    const sign = sale.test(operation) ? 1 : refund.test(operation) ? -1 : !operation && sale.test(type) ? 1 : !operation && refund.test(type) ? -1 : 0;
    const pay = number(row, "forPay", "ppvz_for_pay");
    const delivery = number(row, "deliveryService", "delivery_rub") ?? 0;
    const other = (number(row, "paidStorage", "storage_fee") ?? 0)
      + (number(row, "paidAcceptance", "acceptance") ?? 0)
      + (number(row, "penalty") ?? 0) + (number(row, "deduction") ?? 0);
    const additional = number(row, "additionalPayment", "additional_payment") ?? 0;
    group.delivery += cents(delivery);
    group.other += cents(other);
    group.adjustment += cents(additional);
    if (!sign) {
      if (pay || (!delivery && !other && !additional) || /补偿|补贴|调整|коррекц|компенсац/i.test(operation)) group.unclassified++;
      continue;
    }
    if (pay != null) group.reported += sign * cents(Math.abs(pay));
    // A WB sale / return line is for one item. Other quantities need separate verification.
    const quantity = number(row, "quantity");
    const retail = number(row, "retailPriceWithDisc", "retail_price_withdisc_rub");
    const rate = number(row, "commissionPercent", "commission_percent");
    const acquiring = number(row, "acquiringFee", "acquiring_fee");
    if (quantity !== 1 || retail == null || rate == null || acquiring == null || pay == null || currency === "币种未知") {
      group.pending++;
      continue;
    }
    const expected = cents(retail * (1 - rate / 100) - acquiring);
    group.checked++;
    if (Math.abs(cents(Math.abs(pay)) - expected) > 1) group.failed++;
    group.basis += sign * cents(retail);
    group.commission += sign * cents(retail * rate / 100);
    group.acquiring += sign * cents(acquiring);
    group.expected += sign * expected;
  }
  return [...groups].map(([currency, group]) => ({
    currency, ...group,
    difference: group.pending ? null : group.reported - group.expected,
    net: group.reported - group.delivery - group.other + group.adjustment,
  }));
}

