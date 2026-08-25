from pathlib import Path
from tempfile import TemporaryDirectory
import unittest
import zipfile
from xml.sax.saxutils import escape

from ozon_app.database import Database
from ozon_app.listing_ledger import ledger_shop_names, read_ozon_cost_records


HEADERS = [
    "平台", "上品店铺", "货号", "Ozon商品ID", "采购成本",
    "包装毛重(g)", "包装长度(mm)", "包装宽度(mm)", "包装高度(mm)",
]


def _column(index: int) -> str:
    result = ""
    value = index + 1
    while value:
        value, remainder = divmod(value - 1, 26)
        result = chr(65 + remainder) + result
    return result


def _write_ledger(path: Path) -> None:
    rows = [
        HEADERS,
        ["Ozon", "Cross One", "OFFER-1", "2578856473", "13.5", "240", "330", "270", "110"],
        ["Ozon", "Cross Two", "OFFER-2", "2578856474", "99", "500", "100", "200", "300"],
        ["Amazon", "Cross One", "IGNORE", "1", "1", "1", "1", "1", "1"],
    ]
    xml_rows = []
    for row_index, values in enumerate(rows, start=1):
        cells = []
        for column_index, value in enumerate(values):
            reference = f"{_column(column_index)}{row_index}"
            cells.append(
                f'<c r="{reference}" t="inlineStr"><is><t>{escape(value)}</t></is></c>'
            )
        xml_rows.append(f'<row r="{row_index}">{"".join(cells)}</row>')
    worksheet = (
        '<?xml version="1.0" encoding="UTF-8"?>'
        '<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">'
        f'<sheetData>{"".join(xml_rows)}</sheetData></worksheet>'
    )
    workbook = (
        '<?xml version="1.0" encoding="UTF-8"?>'
        '<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" '
        'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">'
        '<sheets><sheet name="产品台账" sheetId="1" r:id="rId1"/></sheets></workbook>'
    )
    relationships = (
        '<?xml version="1.0" encoding="UTF-8"?>'
        '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        '<Relationship Id="rId1" '
        'Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" '
        'Target="worksheets/sheet1.xml"/></Relationships>'
    )
    with zipfile.ZipFile(path, "w") as archive:
        archive.writestr("xl/workbook.xml", workbook)
        archive.writestr("xl/_rels/workbook.xml.rels", relationships)
        archive.writestr("xl/worksheets/sheet1.xml", worksheet)


class ListingLedgerTests(unittest.TestCase):
    def test_exact_shop_filter_and_cost_import(self) -> None:
        with TemporaryDirectory() as folder:
            ledger = Path(folder) / "产品台账.xlsx"
            _write_ledger(ledger)

            self.assertEqual(ledger_shop_names(ledger), ["Cross One", "Cross Two"])
            records = read_ozon_cost_records(ledger, "Cross One")
            self.assertEqual(len(records), 1)
            self.assertEqual(records[0]["offer_id"], "OFFER-1")
            self.assertAlmostEqual(records[0]["unit_cost_cny"], 13.5)
            self.assertAlmostEqual(records[0]["weight_kg"], 0.24)
            self.assertAlmostEqual(records[0]["length_cm"], 33.0)

            database = Database(Path(folder) / "cross-one.db")
            database.upsert_products([{
                "sku": "2578856473", "offer_id": "OFFER-1", "product_id": "1001",
            }])
            result = database.import_listing_ledger_costs(records)
            self.assertEqual(result["updated_skus"], 1)
            imported = database.product_cost_rows("OFFER-1")[0]
            self.assertAlmostEqual(imported["unit_cost_cny"], 13.5)
            self.assertAlmostEqual(imported["weight_kg"], 0.24)
            self.assertAlmostEqual(imported["width_cm"], 27.0)
            self.assertAlmostEqual(imported["height_cm"], 11.0)


if __name__ == "__main__":
    unittest.main()
