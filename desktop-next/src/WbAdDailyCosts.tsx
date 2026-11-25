import { campaignLabel, canQueryCampaign, type WbAdCampaign } from "./wbAdCampaign";
import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type Daily = {
  day: string;
  payment: "cpm" | "cpc";
  currency: string;
  spend: number;
  views: number | null;
  clicks: number | null;
};

const format = (value: number | null, digits = 2) =>
  value === null ? "—" : value.toLocaleString("zh-CN", { minimumFractionDigits: digits, maximumFractionDigits: digits });

function summarize(rows: Daily[]) {
  const totals = new Map<string, Daily>();
  for (const row of rows) {
    const key = JSON.stringify([row.day, row.payment, row.currency]);
    const total = totals.get(key);
    if (!total) {
      totals.set(key, { ...row });
      continue;
    }
    total.spend += row.spend;
    total.views = total.views === null || row.views === null ? null : total.views + row.views;
    total.clicks = total.clicks === null || row.clicks === null ? null : total.clicks + row.clicks;
  }
  return [...totals.values()].sort((a, b) => b.day.localeCompare(a.day) ||
    a.currency.localeCompare(b.currency) || b.payment.localeCompare(a.payment));
}

export function WbAdDailyCosts({ shopId, campaigns, defaultFrom, defaultTo, maxDate }: {
  shopId: string; campaigns: WbAdCampaign[]; defaultFrom: string; defaultTo: string; maxDate: string;
}) {
  const [selected, setSelected] = useState<number[]>([]);
  const [from, setFrom] = useState(defaultFrom);
  const [to, setTo] = useState(defaultTo);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [report, setReport] = useState<{ from: string; to: string; count: number; rows: Daily[] } | null>(null);
  const ids = campaigns.filter(item => canQueryCampaign(item) && selected.includes(item.id)).map(item => item.id);
  const valid = ids.length > 0 && ids.length <= 50 &&
    from && to && from <= to && to <= maxDate && Date.parse(to) - Date.parse(from) <= 30 * 86400000;

  async function query() {
    if (!valid || busy) return;
    setBusy(true);
    setError("");
    try {
      const rows = await invoke<Daily[]>("wb_operations", {
        action: "ad_cost_daily", payload: { shopId, ids, from, to },
      });
      setReport({ from, to, count: ids.length, rows: summarize(rows) });
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  }

  const summary = summarize((report?.rows ?? []).map(row => ({ ...row, day: "所选期间" })));
  const table = (rows: Daily[]) => <div className="wbo-table-wrap"><table>
    <thead><tr>{["日期", "计费方式", "币种", "按展示扣费", "按点击扣费", "展示量", "点击量", "CPM", "CPC"].map(label =>
      <th key={label}>{label}</th>)}</tr></thead>
    <tbody>{rows.map(row => <tr key={JSON.stringify([row.day, row.payment, row.currency])}>
      <td>{row.day}</td><td>{row.payment === "cpm" ? "按展示（CPM）" : "按点击（CPC）"}</td><td>{row.currency}</td>
      <td>{row.payment === "cpm" ? format(row.spend) : "—"}</td>
      <td>{row.payment === "cpc" ? format(row.spend) : "—"}</td>
      <td>{format(row.views, 0)}</td><td>{format(row.clicks, 0)}</td>
      <td>{format(row.views ? 1000 * row.spend / row.views : null)}</td>
      <td>{format(row.clicks ? row.spend / row.clicks : null)}</td>
    </tr>)}</tbody>
  </table></div>;

  return <section className="wbo-panel wb-keyword-panel" aria-labelledby="wb-ad-cost-title">
    <div className="wbo-heading"><h2 id="wb-ad-cost-title">逐日广告花费</h2></div>
    <form className="wbo-toolbar" onSubmit={event => { event.preventDefault(); void query(); }}>
      <fieldset className="wb-ad-campaign-picker">
        <legend>选择广告活动（已选 {ids.length}）</legend>
        {campaigns.map(item => <label key={item.id}>
          <input type="checkbox" checked={ids.includes(item.id)}
            disabled={!canQueryCampaign(item) || (ids.length >= 50 && !ids.includes(item.id))}
            onChange={event => setSelected(event.target.checked ? [...ids, item.id] : ids.filter(id => id !== item.id))} />
          {campaignLabel(item)}
        </label>)}
      </fieldset>
      <label>开始日期<input type="date" value={from} max={to || maxDate} onChange={event => setFrom(event.target.value)} /></label>
      <label>结束日期<input type="date" value={to} min={from} max={maxDate} onChange={event => setTo(event.target.value)} /></label>
      <button type="submit" className="primary" disabled={!valid || busy}>{busy ? "查询中…" : "查询逐日花费"}</button>
    </form>
    <p>单次最多 50 个活动、31 天，按 WB 返回的日期和币种展示。CPM 为每千次展示成本，CPC 为每次点击平均成本。花费只归入活动实际计费方式，缺少展示量、点击量或分母为零时显示「—」。</p>
    {error && <p className="wbo-error" role="alert">{error}</p>}
    {report && <div>
      <p>{report.from} 至 {report.to} · 已查询 {report.count} 个活动。仅列出 WB 返回明细的日期。</p>
      {report.rows.length ? <>
        <h3>期间汇总</h3>{table(summary)}
        <h3>逐日明细</h3>{table(report.rows)}
      </> : <p>WB 未返回这段时间的活动数据。</p>}
    </div>}
  </section>;
}
