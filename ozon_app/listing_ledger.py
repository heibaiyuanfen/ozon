from __future__ import annotations

import re
import zipfile
from pathlib import Path
from xml.etree import ElementTree as ET


class ListingLedgerError(RuntimeError):
    pass


_MAIN_NS = "http://schemas.openxmlformats.org/spreadsheetml/2006/main"
_REL_NS = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
_PKG_REL_NS = "http://schemas.openxmlformats.org/package/2006/relationships"


def _column_index(reference: str) -> int:
    letters = re.match(r"[A-Z]+", str(reference or "").upper())
    if not letters:
        return -1
    result = 0
    for character in letters.group(0):
        result = result * 26 + ord(character) - 64
    return result - 1


def _shared_strings(archive: zipfile.ZipFile) -> list[str]:
    try:
        root = ET.fromstring(archive.read("xl/sharedStrings.xml"))
    except KeyError:
        return []
    result: list[str] = []
    for item in root.findall(f"{{{_MAIN_NS}}}si"):
        result.append("".join(node.text or "" for node in item.iter(f"{{{_MAIN_NS}}}t")))
    return result


def _worksheet_path(archive: zipfile.ZipFile, preferred_name: str) -> str:
    workbook = ET.fromstring(archive.read("xl/workbook.xml"))
    relationships = ET.fromstring(archive.read("xl/_rels/workbook.xml.rels"))
    targets = {
        item.attrib.get("Id", ""): item.attrib.get("Target", "")
        for item in relationships.findall(f"{{{_PKG_REL_NS}}}Relationship")
    }
    sheets = workbook.find(f"{{{_MAIN_NS}}}sheets")
    if sheets is None:
        raise ListingLedgerError("产品台账中没有工作表。")
    candidates = list(sheets)
    selected = next(
        (item for item in candidates if item.attrib.get("name") == preferred_name),
        candidates[0] if candidates else None,
    )
    if selected is None:
        raise ListingLedgerError("产品台账中没有工作表。")
    relation_id = selected.attrib.get(f"{{{_REL_NS}}}id", "")
    target = targets.get(relation_id, "")
    if not target:
        raise ListingLedgerError("无法定位产品台账工作表。")
    target = target.replace("\\", "/").lstrip("/")
    if target.startswith("xl/"):
        return target
    return "xl/" + target


def _cell_value(cell: ET.Element, shared: list[str]) -> str:
    cell_type = cell.attrib.get("t", "")
    if cell_type == "inlineStr":
        return "".join(node.text or "" for node in cell.iter(f"{{{_MAIN_NS}}}t"))
    value = cell.find(f"{{{_MAIN_NS}}}v")
    text = value.text if value is not None and value.text is not None else ""
    if cell_type == "s" and text:
        try:
            return shared[int(text)]
        except (IndexError, ValueError):
            return ""
    if cell_type == "b":
        return "1" if text == "1" else "0"
    return text


def read_xlsx_rows(path: str | Path, sheet_name: str = "产品台账") -> list[dict[str, str]]:
    ledger_path = Path(path).expanduser()
    if not ledger_path.is_file():
        raise ListingLedgerError(f"找不到产品台账：{ledger_path}")
    try:
        with zipfile.ZipFile(ledger_path) as archive:
            shared = _shared_strings(archive)
            worksheet = ET.fromstring(archive.read(_worksheet_path(archive, sheet_name)))
    except (OSError, KeyError, zipfile.BadZipFile, ET.ParseError) as error:
        raise ListingLedgerError(f"无法读取产品台账：{error}") from error
    raw_rows: list[list[str]] = []
    for row in worksheet.iter(f"{{{_MAIN_NS}}}row"):
        values: list[str] = []
        for cell in row.findall(f"{{{_MAIN_NS}}}c"):
            index = _column_index(cell.attrib.get("r", ""))
            if index < 0:
                continue
            while len(values) <= index:
                values.append("")
            values[index] = _cell_value(cell, shared).strip()
        if any(values):
            raw_rows.append(values)
    if not raw_rows:
        return []
    headers = raw_rows[0]
    result: list[dict[str, str]] = []
    for values in raw_rows[1:]:
        row = {
            header: values[index] if index < len(values) else ""
            for index, header in enumerate(headers) if header
        }
        if any(row.values()):
            result.append(row)
    return result


def ledger_shop_names(path: str | Path) -> list[str]:
    return sorted({
        str(row.get("上品店铺") or "").strip()
        for row in read_xlsx_rows(path)
        if str(row.get("上品店铺") or "").strip()
        and str(row.get("平台") or "Ozon").strip().casefold().startswith("ozon")
    }, key=str.casefold)


def _number(value: object) -> float | None:
    text = str(value or "").strip().replace(",", "")
    if not text:
        return None
    try:
        return float(text)
    except ValueError:
        return None


def read_ozon_cost_records(path: str | Path, shop_name: str) -> list[dict[str, object]]:
    selected_shop = str(shop_name or "").strip().casefold()
    if not selected_shop:
        raise ListingLedgerError("请选择台账中的上品店铺，避免把多个店铺成本混在一起。")
    result: list[dict[str, object]] = []
    for row in read_xlsx_rows(path):
        platform = str(row.get("平台") or "Ozon").strip().casefold()
        ledger_shop = str(row.get("上品店铺") or "").strip()
        if not platform.startswith("ozon") or ledger_shop.casefold() != selected_shop:
            continue
        offer_id = str(row.get("货号") or "").strip()
        if not offer_id:
            continue
        result.append({
            "offer_id": offer_id,
            "ozon_product_id": str(row.get("Ozon商品ID") or "").strip(),
            "unit_cost_cny": _number(row.get("采购成本")),
            "weight_kg": (
                _number(row.get("包装毛重(g)")) / 1000
                if _number(row.get("包装毛重(g)")) is not None else None
            ),
            "length_cm": (
                _number(row.get("包装长度(mm)")) / 10
                if _number(row.get("包装长度(mm)")) is not None else None
            ),
            "width_cm": (
                _number(row.get("包装宽度(mm)")) / 10
                if _number(row.get("包装宽度(mm)")) is not None else None
            ),
            "height_cm": (
                _number(row.get("包装高度(mm)")) / 10
                if _number(row.get("包装高度(mm)")) is not None else None
            ),
            "ledger_shop": ledger_shop,
        })
    return result


def find_listing_ledger(root: str | Path) -> Path | None:
    """Find only the product ledger without probing or launching the listing app."""
    base = Path(root).expanduser()
    candidates = (
        base / "发布" / "Ozon_RFBS上品工具" / "程序数据" / "产品台账.xlsx",
        base / "RFBS上品工具" / "产品台账.xlsx",
        base / "产品台账.xlsx",
    )
    return next((item for item in candidates if item.is_file()), None)
