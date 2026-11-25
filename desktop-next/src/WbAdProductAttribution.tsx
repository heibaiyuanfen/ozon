import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type Conversion = { nmId: number; kind: string; currency: string; orders: number; sales: number; spend: number };
const names: Record<string, string> = { direct: "直接成交", associated: "关联成交", multicard: "同卡跨商品成交" };
const money = (n: number) => n.toLocaleString("zh-CN", { maximumFractionDigits: 2 });

export function WbAdProductAttribution({ shopId, campaign, end, target, scale }: {
  shopId: string; campaign: string; end: string; target: number; scale: number;
}) {
  const [report, setReport] = useState<{ campaign: string; from: string; to: string; rows: Conversion[] } | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const valid = /^\d+$/.test(campaign) && Number.isSafeInteger(Number(campaign)) && Number(campaign) > 0 && !!end;

  async function query() {
    if (busy || !valid) return;
    setBusy(true);
    setError("");
    const from = new Date(Date.parse(end) - 13 * 86400000).toISOString().slice(0, 10);
    try {
      const rows = await invoke<Conversion[]>("wb_operations", {
        action: "ad_product_attribution", payload: { shopId, campaignId: Number(campaign), from, to: end },
      });
      setReport({ campaign, from, to: end, rows });
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  }

  const products = new Map<string, Conversion>();
  const totals = new Map<string, { direct: number; cross: number; spend: number }>();
  for (const row of report?.rows ?? []) {
    const total = totals.get(row.currency) ?? { direct: 0, cross: 0, spend: 0 };
    total.spend += row.spend;
    if (row.kind === "direct") total.direct += row.orders;
    if (row.kind === "associated" || row.kind === "multicard") total.cross += row.orders;
    totals.set(row.currency, total);
    const key = JSON.stringify([row.nmId, row.kind, row.currency]);
    const prior = products.get(key);
    if (prior) {
      prior.orders += row.orders;
      prior.sales += row.sales;
    } else {
      products.set(key, { ...row });
    }
  }

  return <section aria-labelledby="wb-ad-product-attribution">
    <div className="wbo-heading"><h3 id="wb-ad-product-attribution">搜索广告成交商品与活动评分</h3>
      <button type="button" disabled={!valid || busy} onClick={() => void query()}>
        {busy ? "正在查询…" : "查询近 14 天成交商品"}
      </button>
    </div>
    <p>使用上方活动编号和截止日。活动评分计入 WB 确认的直接、关联及同卡跨商品订单。成交商品无法对应到具体搜索词，词组评分继续使用 WB 的词组统计。</p>
    {error && <p role="alert" className="wbo-error">{error}</p>}
    {report && <>
      <p>活动 {report.campaign} · {report.from} 至 {report.to} · 搜索广告位</p>
      {!report.rows.length && <p>WB 未返回可单独归为搜索广告位的数据。</p>}
      {[...totals].map(([currency, total]) => {
        const orders = total.direct + total.cross;
        const cpa = orders > 0 ? total.spend / orders : null;
        const score = cpa !== null && target > 0 && scale > 0
          ? Math.round(80 * (cpa === 0 ? 1 : Math.min(1, target / cpa)) + 20 * Math.min(1, orders / scale))
          : null;
        return <p key={currency}><strong>活动评分 {score ?? "—"}</strong> · 直接 {total.direct} 单 · 跨商品 {total.cross} 单 ·
          搜索花费 {money(total.spend)} {currency} · 含跨商品 CPA {cpa === null ? "—" : money(cpa)}</p>;
      })}
      {!!report.rows.length && <>
        <p>活动评分采用目标 CPA 达成度 80%、成交规模 20%，沿用上方目标 CPA 和成交满分单量。</p>
        <div className="wbo-table-wrap"><table>
          <thead><tr><th>成交商品编号</th><th>归因类型</th><th>归因订单</th><th>归因销售额</th></tr></thead>
          <tbody>{[...products].filter(([, row]) => row.orders > 0).sort((a, b) => b[1].orders - a[1].orders)
            .map(([key, row]) => <tr key={key}><td>{row.nmId}</td><td>{names[row.kind] ?? row.kind}</td>
              <td>{row.orders}</td><td>{money(row.sales)} {row.currency}</td></tr>)}</tbody>
        </table></div>
      </>}
    </>}
  </section>;
}
