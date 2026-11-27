import { WbWeeklyFinance } from "./WbWeeklyFinance";
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type Sheet = { name: string; firstRow: number; firstColumn: number; width: number; rows: string[][] };
type Workbook = { name: string; source?: "api"; sheets: Sheet[] };
const apiLabels: Record<string, string> = { reportId:"报告编号",dateFrom:"报告开始日期",dateTo:"报告结束日期",currency:"币种",rrdId:"明细行 ID",nmId:"WB商品编号",vendorCode:"卖家商品编号",docTypeName:"文档类型",sellerOperName:"付款依据",orderDt:"买家下单日期",saleDt:"销售日期",quantity:"数量",retailAmount:"WB销售商品",forPay:"支付给卖家的已售商品金额",deliveryService:"向买家交付货物的服务",storageFee:"仓储",deduction:"扣款" };
const pageSize = 50;

const definitions: Record<string, string> = {
  "供货编号": "从 WB 仓库销售时显示供货编号。卖家仓库销售或盘点时显示 0。",
  "商品品类": "商品所属的类目或分类。",
  "WB商品编号": "Wildberries 商品详情页上的编号。",
  "卖家商品编号": "卖家在商品详情中设置的货号。",
  "文档类型": "商品操作类型，例如销售或退货。",
  "付款依据": "对应交易或扣费的类型，例如销售、退货、配送、罚款或补款。",
  "买家下单日期": "买家下单的日期。",
  "销售日期": "商品签收日期。",
  "数量": "报告期间内商品的销售或退货数量。",
  "零售价": "卖家设定的未折扣价格。",
  "WB销售商品": "报告中的销售金额，含商品适用的折扣。",
  "零售价格（包含最终折扣）": "包含卖家设定的最终折扣后的零售价。",
  "平台折扣（%）": "由 Wildberries 提供的常客折扣。",
  "支付给卖家的已售商品金额": "WB 报告列示的应付卖家商品金额。",
  "交付数量": "交付给买家的数量。",
  "退货数量": "买家退回仓库的数量。",
  "向买家交付货物的服务": "WB 在该列记录的配送及相关服务金额。",
  "配送类型、罚款及Wildberries佣金调整类型": "配送、罚款或佣金调整的原因或类型。",
  "FBS标签": "卖家仓库履约时的单位商品唯一代码。",
  "仓库标签": "贴在仓库商品上的单位商品识别码。",
  "Srid": "买家订单中商品的唯一标识。",
  "Rid": "买家订单中商品的唯一标识。",
  "仓储": "在 WB 仓库储存商品产生的费用。",
  "扣款": "WB 的营销服务费、罚款或其他扣付项目。",
  "商品处理服务": "在 WB 仓库处理商品的费用。",
};

function columnName(index: number) {
  let name = "";
  for (let value = index; value > 0; value = Math.floor((value - 1) / 26)) {
    name = String.fromCharCode(65 + (value - 1) % 26) + name;
  }
  return name;
}

export function WbWeeklyReport({ shopId, currency }: { shopId: string; currency: "RUB" | "CNY" }) {
  const [local, setLocal] = useState(false);
  const [apiReport, setApiReport] = useState<Workbook | null>(null);
  const [localReport, setLocalReport] = useState<Workbook | null>(null);
  const report = local ? localReport : apiReport;
  const [offset, setOffset] = useState(0);
  const [attempt, setAttempt] = useState(0);
  const [force, setForce] = useState(false);
  const date = new Date(new Intl.DateTimeFormat("sv-SE",{timeZone:"Europe/Moscow",year:"numeric",month:"2-digit",day:"2-digit"}).format(new Date()) + "T12:00:00Z");
  date.setUTCDate(date.getUTCDate() - (date.getUTCDay() + 6) % 7 - 7 * (offset + 1));
  const from = date.toISOString().slice(0,10);
  date.setUTCDate(date.getUTCDate() + 6);
  const to = date.toISOString().slice(0,10);
  const [sheetIndex, setSheetIndex] = useState(0);
  const [page, setPage] = useState(0);
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState("");

  useEffect(() => {
    let active = true;
    setBusy(true);
    setError("");
    const action = local ? "weekly_report_load" : "weekly_report_api";
    invoke<Workbook | null>("wb_operations", {action, payload:{shopId,from,refresh:force}})
      .then(result => { if (active) (local ? setLocalReport : setApiReport)(result); })
      .catch(reason => { if (active) setError(String(reason)); })
      .finally(() => { if (active) setBusy(false); });
    return () => { active = false; };
  }, [shopId,local,from,attempt]);

  function showSource(value: boolean) {
    setForce(false);
    setLocal(value);
    setPage(0);
    setSheetIndex(0);
  }

  function changeWeek(value: number) {
    setForce(false);
    setApiReport(null);
    setOffset(value);
    setPage(0);
    setSheetIndex(0);
  }

  async function importFile(file: File) {
    setError("");
    setBusy(true);
    try {
      if (file.size > 16 * 1024 * 1024) throw Error("文件超过 16 MB");
      const bytes = Array.from(new Uint8Array(await file.arrayBuffer()));
      const result = await invoke<Workbook>("wb_operations", {
        action: "weekly_report_import", payload: {shopId, name: file.name, bytes},
      });
      setLocalReport(result);
      setSheetIndex(0);
      setPage(0);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  }

  const sheet = report?.sheets[sheetIndex];
  const first = sheet?.rows[0] ?? [];
  const reportHeader = report?.source === "api" || (first.includes("付款依据") && first.includes("WB商品编号"));
  const rows = sheet?.rows.slice(reportHeader ? 1 : 0) ?? [];
  const pageCount = Math.max(1, Math.ceil(rows.length / pageSize));
  const columns = Array.from({length: sheet?.width ?? 0}, (_, i) => i);
  const reportNumber = report?.name.match(/№\s*(\d+)/)?.[1];

  return <>
    <section className="wbo-panel">
    <div className="wbo-heading">
      <div><h2>官方周度销售报告</h2>{report && <p>{report.name}</p>}</div>
      {local && <label><span>导入本地工作簿</span> <input type="file" accept=".xlsx" disabled={busy} onChange={event => {
        const file = event.currentTarget.files?.[0];
        event.currentTarget.value = "";
        if (file) void importFile(file);
      }}/></label>}
    </div>
    <div className="wbo-actions">
      <button type="button" disabled={busy || !local} onClick={() => showSource(false)}>官方 API 报告</button>
      <button type="button" disabled={busy || local} onClick={() => showSource(true)}>本地工作簿</button>
    </div>
    {!local && <div className="wbo-toolbar">
      <button disabled={busy} onClick={() => changeWeek(offset + 1)}>上一周</button>
      <span>{from} 至 {to} · 莫斯科时间</span>
      <button disabled={busy || offset === 0} onClick={() => changeWeek(offset - 1)}>下一周</button>
      <button className="primary" disabled={busy} onClick={() => {setForce(true);setAttempt(value => value + 1);setPage(0);}}>从 WB 刷新</button>
    </div>}
    {!local && <p className="wbo-muted">通过当前店铺的 Finance（财务）Token 读取完整周销售明细。</p>}
    {busy && <p role="status">{local ? "正在读取工作簿…" : "正在读取 WB 官方销售报告…"}</p>}
    {error && <p className="wbo-error" role="alert">{error}</p>}
    {!busy && !report && !error && local && <p>选择本地 XLSX，查看整理表或原始明细。</p>}
    {!local && <WbWeeklyFinance key={shopId+":"+from} shopId={shopId} adCurrency={currency} from={from} to={to} headers={first} rows={apiReport?.sheets[0]?.rows.slice(1) ?? []}/>}
    {report && sheet && <>
      <p className="wbo-muted">{reportNumber ? `报告编号 ${reportNumber} · ` : ""}{report.sheets.length} 张工作表{local ? " · 导入新文件将替换当前本地工作簿" : " · 来源为 WB 财务接口"}</p>
      <div className="wbo-actions" aria-label="工作表">{report.sheets.map((item, index) =>
        <button key={item.name} aria-pressed={index === sheetIndex} className={index === sheetIndex ? "primary" : ""}
          onClick={() => {setSheetIndex(index);setPage(0);}}>{item.name}</button>)}</div>
      {reportHeader && <details className="wbo-weekly-definitions">
        <summary>字段说明</summary>
        <dl>{first.map(title => report?.source === "api" ? apiLabels[title] || title : title).filter(title => definitions[title]).map((title, i) =>
          <div key={i}><dt>{title}</dt><dd>{definitions[title]}</dd></div>)}</dl>
      </details>}
      <div className="wbo-table-wrap wbo-weekly-grid" tabIndex={0} aria-label={sheet.name + "完整明细"}>
        <table>
          <thead><tr><th scope="col">原表行号</th>{columns.map(index => {
            const label = columnName(sheet.firstColumn + index);
            const field = reportHeader ? first[index] : "";
            const title = (report.source === "api" && apiLabels[field]) || field;
            return <th scope="col" key={index} title={definitions[title]}>{title || label}<small>{title ? (report.source === "api" && title !== field ? field : label) : ""}</small></th>;
          })}</tr></thead>
          <tbody>{rows.slice(page * pageSize, (page + 1) * pageSize).map((row, index) => {
            const rowNumber = sheet.firstRow + (reportHeader ? 1 : 0) + page * pageSize + index;
            return <tr key={rowNumber}><th scope="row">{rowNumber}</th>{columns.map(col => <td key={col}>{row[col]}</td>)}</tr>;
          })}</tbody>
        </table>
      </div>
      {!rows.length && <p>{local ? "此工作表没有明细行。" : "WB 尚未返回这一周的报告明细。"}</p>}
      <div className="wbo-actions">
        <button disabled={page === 0} onClick={() => setPage(page - 1)}>上一页</button>
        <span>{rows.length} 行 · {sheet.width} 列 · 第 {page + 1} / {pageCount} 页</span>
        <button disabled={page + 1 >= pageCount} onClick={() => setPage(page + 1)}>下一页</button>
      </div>
    </>}
  </section></>;
}
