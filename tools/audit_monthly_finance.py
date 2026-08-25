"""Audit monthly Finance totals independently from either desktop UI."""

from __future__ import annotations

import argparse
import json
import sqlite3
from pathlib import Path


def number(value: object) -> float:
    try:
        return float(value or 0)
    except (TypeError, ValueError):
        return 0.0


def audit(database: Path, month: str) -> dict[str, object]:
    date_from = f"{month}-01"
    year, month_number = map(int, month.split("-"))
    next_year, next_month = (year + 1, 1) if month_number == 12 else (year, month_number + 1)
    date_to_exclusive = f"{next_year:04d}-{next_month:02d}-01"
    connection = sqlite3.connect(database)
    connection.row_factory = sqlite3.Row
    rows = connection.execute(
        "SELECT operation_id, sku, amount, sale_commission, raw_json "
        "FROM finance_transactions WHERE substr(operation_date,1,10)>=? "
        "AND substr(operation_date,1,10)<? ORDER BY operation_date, operation_id",
        (date_from, date_to_exclusive),
    ).fetchall()
    result: dict[str, object] = {
        "database": str(database), "month": month, "operations": len(rows),
        "valid_raw_json": 0, "invalid_raw_json": 0, "stored_amount": 0.0,
        "raw_amount": 0.0, "stored_commission": 0.0, "raw_commission": 0.0,
        "component_total": 0.0, "exact_sku_operations": 0,
        "multi_sku_operations": 0, "store_level_operations": 0,
    }
    for row in rows:
        result["stored_amount"] += number(row["amount"])
        result["stored_commission"] += number(row["sale_commission"])
        try:
            raw = json.loads(row["raw_json"] or "{}")
        except (TypeError, json.JSONDecodeError):
            result["invalid_raw_json"] += 1
            continue
        result["valid_raw_json"] += 1
        amount = number(raw.get("amount"))
        accrual = number(raw.get("accruals_for_sale"))
        commission = number(raw.get("sale_commission"))
        service_total = sum(
            number(service.get("price"))
            for service in (raw.get("services") or [])
            if isinstance(service, dict)
        )
        residual = amount - accrual - commission - service_total
        result["raw_amount"] += amount
        result["raw_commission"] += commission
        result["component_total"] += accrual + commission + service_total + residual
        skus = {
            str(item.get("sku") or "").strip()
            for item in (raw.get("items") or [])
            if isinstance(item, dict) and item.get("sku")
        }
        key = (
            "exact_sku_operations" if len(skus) == 1 else
            "multi_sku_operations" if len(skus) > 1 else "store_level_operations"
        )
        result[key] += 1
    for key in (
        "stored_amount", "raw_amount", "stored_commission", "raw_commission",
        "component_total",
    ):
        result[key] = round(float(result[key]), 2)
    result["amount_storage_difference"] = round(
        float(result["stored_amount"]) - float(result["raw_amount"]), 2
    )
    result["commission_storage_difference"] = round(
        float(result["stored_commission"]) - float(result["raw_commission"]), 2
    )
    result["component_reconciliation_difference"] = round(
        float(result["component_total"]) - float(result["raw_amount"]), 2
    )
    result["legacy_rounded_commission"] = round(float(result["raw_commission"]))
    return result


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("database", type=Path)
    parser.add_argument("month", help="YYYY-MM")
    args = parser.parse_args()
    print(json.dumps(audit(args.database, args.month), ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
