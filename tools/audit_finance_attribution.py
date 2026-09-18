import collections
import json
import sqlite3
import sys


db_path = sys.argv[1]
month = sys.argv[2]
connection = sqlite3.connect(db_path)
rows = connection.execute(
    """
    SELECT operation_id, operation_date, operation_type, posting_number, sku, raw_json
    FROM finance_transactions
    WHERE substr(operation_date, 1, 7) = ?
    """,
    (month,),
).fetchall()
payloads = [json.loads(row[5]) for row in rows]
ids = [row[0] for row in rows]
accruals = [payload.get("accrual_raw") or {} for payload in payloads]
type_name_pairs = collections.Counter()
for payload, accrual in zip(payloads, accruals):
    fee_ids = []
    for product in ((accrual.get("posting") or {}).get("products") or []):
        fee_ids.extend(fee.get("type_id") for fee in ((product.get("delivery") or {}).get("services") or []))
    for group in ((accrual.get("item_fees") or {}).get("fees") or []):
        fee_ids.extend(fee.get("type_id") for fee in (group.get("fees") or []))
    if accrual.get("non_item_fee"):
        fee_ids.append(accrual["non_item_fee"].get("type_id"))
    names = [service.get("name") for service in (payload.get("services") or [])]
    type_name_pairs.update(zip(fee_ids, names))

print(json.dumps({
    "rows": len(rows),
    "distinct_ids": len(set(ids)),
    "duplicate_ids": len(ids) - len(set(ids)),
    "dates": collections.Counter(row[1][:10] for row in rows),
    "sku_nonempty": sum(bool(row[4]) for row in rows),
    "posting_nonempty": sum(bool(row[3]) for row in rows),
    "top_types": collections.Counter(row[2] for row in rows).most_common(20),
    "type_name_pairs": type_name_pairs.most_common(),
    "accrual_top_level_keys": collections.Counter(
        tuple(sorted(accrual.keys())) for accrual in accruals
    ).most_common(5),
    "posting_value_types": collections.Counter(
        type(accrual.get("posting")).__name__ for accrual in accruals
    ),
    "posting_product_summary": {
        "rows_with_products": sum(bool(((accrual.get("posting") or {}).get("products") or [])) for accrual in accruals),
        "products": sum(len(((accrual.get("posting") or {}).get("products") or [])) for accrual in accruals),
        "quantity": sum(
            int(product.get("quantity") or 0)
            for accrual in accruals
            for product in ((accrual.get("posting") or {}).get("products") or [])
        ),
        "with_commission": sum(
            product.get("commission") is not None
            for accrual in accruals
            for product in ((accrual.get("posting") or {}).get("products") or [])
        ),
        "seller_price_positive": sum(
            float(((product.get("commission") or {}).get("seller_price") or {}).get("amount") or 0) > 0
            for accrual in accruals
            for product in ((accrual.get("posting") or {}).get("products") or [])
        ),
        "seller_price_negative": sum(
            float(((product.get("commission") or {}).get("seller_price") or {}).get("amount") or 0) < 0
            for accrual in accruals
            for product in ((accrual.get("posting") or {}).get("products") or [])
        ),
    },
    "accrued_categories": collections.Counter(
        accrual.get("accrued_category") for accrual in accruals
    ).most_common(),
    "item_fee_type_ids": collections.Counter(
        fee.get("type_id")
        for accrual in accruals
        for group in ((accrual.get("item_fees") or {}).get("fees") or [])
        for fee in (group.get("fees") or [])
    ).most_common(),
    "item_fee_type_totals": {
        str(type_id): {
            "rows": len(values),
            "units": sum(quantity for _, _, quantity, _ in values),
            "unique_unit_sku": len({(unit, sku) for unit, sku, _, _ in values}),
            "amount": round(sum(amount for _, _, _, amount in values), 2),
        }
        for type_id, values in {
            type_id: [
                (
                    str(accrual.get("unit_number") or ""),
                    str(group.get("sku")),
                    int(group.get("quantity") or 0),
                    float((fee.get("accrued") or {}).get("amount") or 0),
                )
                for accrual in accruals
                for group in ((accrual.get("item_fees") or {}).get("fees") or [])
                for fee in (group.get("fees") or [])
                if fee.get("type_id") == type_id
            ]
            for type_id in {
                fee.get("type_id")
                for accrual in accruals
                for group in ((accrual.get("item_fees") or {}).get("fees") or [])
                for fee in (group.get("fees") or [])
            }
        }.items()
    },
    "item_fee_skus": len({
        str(group.get("sku"))
        for accrual in accruals
        for group in ((accrual.get("item_fees") or {}).get("fees") or [])
        if group.get("sku") is not None
    }),
    "item_fee_rows_with_sku": sum(
        1
        for accrual in accruals
        if any(
            group.get("sku") is not None
            for group in ((accrual.get("item_fees") or {}).get("fees") or [])
        )
    ),
    "unique_item_units": len({
        (str(accrual.get("unit_number") or ""), str(group.get("sku")))
        for accrual in accruals
        for group in ((accrual.get("item_fees") or {}).get("fees") or [])
        if group.get("sku") is not None
    }),
    "unique_item_quantities": sum({
        (str(accrual.get("unit_number") or ""), str(group.get("sku"))): int(group.get("quantity") or 0)
        for accrual in accruals
        for group in ((accrual.get("item_fees") or {}).get("fees") or [])
        if group.get("sku") is not None
    }.values()),
    "sales": connection.execute(
        """
        SELECT count(*), coalesce(sum(ordered_units), 0),
               coalesce(sum(delivered_units), 0), coalesce(sum(revenue), 0)
        FROM sales_daily
        WHERE substr(day, 1, 7) = ?
        """,
        (month,),
    ).fetchone(),
    "cost_coverage": connection.execute(
        """
        SELECT count(distinct s.sku),
               count(distinct case when c.sku is not null then s.sku end),
               coalesce(sum(case when c.sku is not null then s.delivered_units else 0 end), 0)
        FROM sales_daily s
        LEFT JOIN product_costs c ON c.sku = s.sku
        WHERE substr(s.day, 1, 7) = ?
        """,
        (month,),
    ).fetchone(),
    "delivery_events": connection.execute(
        """
        SELECT count(*), coalesce(sum(quantity), 0)
        FROM delivery_events
        WHERE substr(day, 1, 7) = ?
        """,
        (month,),
    ).fetchone(),
}, ensure_ascii=False, indent=2, default=list))

seen_categories = set()
for accrual in accruals:
    category = accrual.get("accrued_category")
    if accrual and category not in seen_categories:
        seen_categories.add(category)
        print(f"SAMPLE_{category}=" + json.dumps(accrual, ensure_ascii=False)[:5000])
