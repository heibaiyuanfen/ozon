import { FileBlob, SpreadsheetFile } from "@oai/artifact-tool";

const path = process.argv[2];
if (!path) throw new Error("missing workbook path");
const input = await FileBlob.load(path);
const workbook = await SpreadsheetFile.importXlsx(input);
const summary = await workbook.inspect({
  kind: "workbook,sheet,table",
  maxChars: 20000,
  tableMaxRows: 12,
  tableMaxCols: 24,
  tableMaxCellChars: 160,
});
console.log(summary.ndjson);
const sheets = await workbook.inspect({ kind: "sheet", include: "id,name", maxChars: 5000 });
console.log(sheets.ndjson);
for (const line of sheets.ndjson.split(/\r?\n/).filter(Boolean)) {
  const row = JSON.parse(line);
  const name = row.name || row.sheetName;
  if (!name) continue;
  const region = await workbook.inspect({ kind: "region", sheetId: name, range: "A1:Z40", maxChars: 18000 });
  console.log(JSON.stringify({ sheet: name, region: region.ndjson }));
}
const ledgerSheet = workbook.worksheets.getItem("产品台账");
const values = ledgerSheet.getUsedRange(true).values;
const headers = values[0].map(String);
const wanted = ["上品店铺", "货号", "1688采购链接", "Ozon商品ID", "商品标题", "币种", "采购成本", "包装毛重(g)", "包装长度(mm)", "包装宽度(mm)", "包装高度(mm)", "平台"];
const indexes = Object.fromEntries(wanted.map((name) => [name, headers.indexOf(name)]));
const rows = values.slice(1).map((row) => Object.fromEntries(wanted.map((name) => [name, indexes[name] >= 0 ? row[indexes[name]] : null])));
console.log(JSON.stringify({ kind: "ledgerRows", rows }));
