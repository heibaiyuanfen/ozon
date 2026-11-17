import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { settlement } from "./wbSettlementCheck";

type Fields = Record<string, string | number | null | undefined>;
type Result = { updatedAt: string; matched: number; reportRows: number; rows: { order: Fields; details: Fields[] }[] };
const amount = (value: unknown, currency: unknown, divisor = 1) =>
  value == null ? "—" : (Number(value) / divisor).toFixed(2) + " " + (currency === 156 ? "CNY" : currency === 643 ? "RUB" : currency ?? "");

export function WbReconciliation({shopId, from, to, disabled}: {shopId: string; from: string; to: string; disabled: boolean}) {
  const [data, setData] = useState<Result | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [query, setQuery] = useState("");
  const call = (action: string) => invoke<Result | null>("wb_operations", { action, payload: { shopId, from, to } });
  useEffect(() => {
    let active = true;
    void call("reconciliation_list").then(result => { if (active) setData(result); })
      .catch(e => { if (active) setError(String(e)); });
    return () => { active = false; };
  }, [shopId, from, to]);
  async function sync() {
    setBusy(true);
    setError("");
    try { setData(await call("reconciliation_sync")); }
    catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  }
  return <section className="wbo-panel">
    <div className="wbo-heading"><div><h2>订单与周报核对</h2>
      <p>按上方日期同步 FBS 历史订单和周销售明细。跨周结算的订单可能在本期没有明细。按每条销售或退款复算商品应付，允许 0.01 同币种舍入差。估算净额只计入本期能关联到订单的已识别费用，请用财务打款记录核对实际到账。</p></div>
      <button disabled={disabled || busy || !from || !to} onClick={() => void sync()}>{busy ? "正在核对…" : "同步并核对"}</button></div>
    {error && <p role="alert" className="wbo-error">{error}</p>}
    {data && <>
      <p>{data.rows.length} 笔订单 · {data.matched} 笔匹配 · {data.rows.length - data.matched} 笔无匹配 · 周报 {data.reportRows} 条 · {new Date(data.updatedAt).toLocaleString("zh-CN")}</p>
      <input aria-label="搜索核对结果" placeholder="输入订单号或货号" value={query} onChange={e => setQuery(e.target.value)}/>
      <div className="wbo-table-wrap" style={{maxHeight: 420, overflow: "auto"}}><table><thead><tr><th>订单 / 货号</th><th>订单原币 / 换算金额</th><th>周报操作</th><th>卖家零售价</th><th>买家实付</th><th>商品应付</th><th>实收核对（按订单）</th></tr></thead>
      <tbody>{data.rows.filter(({order}) => [order.id, order.article].some(v => String(v ?? "").toLowerCase().includes(query.trim().toLowerCase()))).flatMap(({order, details}) =>
        (details.length ? details : [null]).map((row, index) => {
          const currency = row?.currency ?? row?.currencyName ?? row?.currency_name;
          return <tr key={order.id + "/" + index}><td>{order.id}<small>{order.article}</small></td>
            <td>{amount(order.price, order.currencyCode, 100)}<small>{amount(order.convertedPrice, order.convertedCurrencyCode, 100)}</small></td>
            <td>{row ? row.sellerOperName ?? row.supplierOperName ?? row.supplier_oper_name ?? row.docTypeName ?? row.doc_type_name ?? "已匹配" : order.rid ? "本期无匹配明细" : "订单缺少关联号"}</td>
            <td>{amount(row?.retailPriceWithDisc ?? row?.retail_price_withdisc_rub ?? row?.retailPrice ?? row?.retail_price, currency)}</td>
            <td>{amount(row?.retailAmount ?? row?.retail_amount, currency)}</td>
            <td>{amount(row?.forPay ?? row?.ppvz_for_pay, currency)}</td>{index === 0 && <td rowSpan={Math.max(1, details.length)}>{!details.length ? "待周报入账" : settlement(details).map(g => { const m = (n: number) => amount(n, g.currency, 100); return <div key={g.currency}><b>{g.failed ? "商品应付存在差额" : g.pending ? "字段不足，待核实" : !g.checked ? "尚无可核对的销售或退款" : "商品应付核对通过"}</b>{g.checked > 0 && <><small>卖家价 {m(g.basis)} · 佣金 {m(g.commission)} · 支付费 {m(g.acquiring)}</small><small>计算应付 {m(g.expected)} · 周报商品应付 {m(g.reported)}{g.difference != null && ` · 差额 ${m(g.difference)}`}</small></>}<small>配送 {m(g.delivery)} · 其他扣费 {m(g.other)} · 补款 {m(g.adjustment)}</small><small>已关联明细估算净额 {m(g.net)}{g.unclassified > 0 && ` · ${g.unclassified} 条待分类`}</small></div>; })}</td>}</tr>;
        }))}</tbody></table></div>
    </>}
  </section>;
}



