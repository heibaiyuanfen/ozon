import { useEffect, useMemo, useState } from "react";
import type { NetDashboardDay } from "./types";
import "./net-dashboard.css";

const pct = (n: number | null) => n == null ? "—" : `${n.toFixed(2)}%`;
export default function NetDashboard({ days, currency, to }: { days: NetDashboardDay[]; currency: string; to: string }) {
  const [selected, setSelected] = useState(to);
  const [query, setQuery] = useState("");
  const [page, setPage] = useState(0);
  useEffect(() => { setSelected(to); setQuery(""); setPage(0); }, [to]);
  useEffect(() => setPage(0), [selected, query]);
  const row = days.find(d => d.day === selected);
  const money = (n: number) => `${currency === "CNY" ? "¥" : "₽"}${n.toLocaleString("zh-CN", { maximumFractionDigits: 2 })}`;
  const products = useMemo(() => (row?.products ?? []).filter(p => `${p.sku} ${p.offerId} ${p.name}`.toLowerCase().includes(query.trim().toLowerCase())).sort((a,b) => b.cancellationUnits-a.cancellationUnits || b.revenue-a.revenue), [row, query]);
  const maxPage = Math.max(0, Math.ceil(products.length / 100)-1);
  const currentPage = Math.min(page, maxPage);
  const cards = row ? [
    ["扣除取消后销售额", money(row.netRevenue)],
    ["扣除取消后销量", `${row.netUnits} 件`],
    ["取消件数 / 扣减金额", row.cancellationComplete ? `${row.cancellationUnits} 件 / ${money(row.cancellationAmount)}` : `待核对（缓存 ${row.cancellationUnits} 件）`],
    ["广告花费", money(row.adSpend)],
    ["扣除取消后广告销售额（估算）", money(row.estimatedNetAdRevenue)],
    ["广告 ACOS（估算）", pct(row.estimatedAcos)],
    ["扣除取消后 TACOS", pct(row.tacos)],
    ["净件均销售额", row.netUnits > 0 ? money(row.netRevenue/row.netUnits) : "—"],
  ] : [];
  return <section className="card net-dashboard">
    <div className="section-heading"><div><h2>扣除取消后的经营数据</h2><p>按订单所属日期计算；取消状态会随同步更新。</p></div>
      <label>查看日期 <select value={selected} onChange={e => setSelected(e.target.value)} aria-label="扣除取消看板日期">
        {[...new Set([to, ...days.map(d=>d.day)])].sort().reverse().map(d=><option key={d} value={d}>{d}</option>)}
      </select></label>
    </div>
    {row ? <>{!row.cancellationComplete && <p className="net-warning">此日期的 FBS/FBO 订单状态尚未完整核对，缓存中的0不代表没有取消；净销售额、净销量和广告比例暂按现有缓存估算。请同步订单状态并检查同步错误。</p>}<div className="net-metrics">{cards.map(([label,value])=><div key={label}><span>{label}</span><strong>{value}</strong></div>)}</div>
      <p className="net-basis">原销售额 {money(row.revenue)} · 原销量 {row.units} 件 · 平台广告销售额 {money(row.adRevenue)}。
        订单状态核对：{row.cancellationSyncedAt ? new Date(row.cancellationSyncedAt).toLocaleString("zh-CN") : "尚未完成"}。取消金额优先使用对应订单价格；{row.estimatedProducts} 个商品使用均价估算或缺少可匹配数据。
        广告净销售额按当天净销售额 / 原销售额同比例估算，ACOS = 广告花费 / 估算广告净销售额；TACOS = 广告花费 / 净销售额。广告花费不因取消而扣减，退货不在本看板扣除。</p>
      {row.unmatchedUnits > 0 && <p className="net-warning">有 {row.unmatchedUnits} 件取消无法匹配当天销量，未重复扣减；请查看商品明细并同步对应订单日期的数据。</p>}
    </> : <p className="net-basis">{selected} 暂无销售或广告缓存，请同步此日期的数据。</p>}
    <details className="net-details" open><summary>逐日明细（点击日期查看商品）</summary><div className="net-table-wrap"><table><thead><tr><th>日期</th><th>原销售额</th><th>取消件数</th><th>扣减金额</th><th>净销售额</th><th>净销量</th><th>广告花费</th><th>广告净销售额（估算）</th><th>ACOS（估算）</th><th>净 TACOS</th></tr></thead><tbody>
      {[...days].reverse().map(d=><tr key={d.day} className={selected===d.day ? "net-selected" : ""}><td><button className="net-day-button" onClick={()=>setSelected(d.day)}>{d.day}</button></td><td>{money(d.revenue)}</td><td>{d.cancellationComplete ? d.cancellationUnits : `待核对（缓存 ${d.cancellationUnits}）`}</td><td>{d.cancellationComplete ? money(d.cancellationAmount) : "待核对"}</td><td>{money(d.netRevenue)}</td><td>{d.netUnits}</td><td>{money(d.adSpend)}</td><td>{money(d.estimatedNetAdRevenue)}</td><td>{pct(d.estimatedAcos)}</td><td>{pct(d.tacos)}</td></tr>)}
      {!days.length && <tr><td colSpan={10}>当前日期范围暂无数据</td></tr>}
    </tbody></table></div></details>
    <details className="net-details"><summary>{selected} 商品明细 · {row?.products.length ?? 0} 个商品</summary>
      <div className="net-product-tools"><input placeholder="搜索货号、SKU 或商品名" aria-label="净销售商品搜索" value={query} onChange={e=>setQuery(e.target.value)} /><span>{products.length} 条</span><button disabled={currentPage===0} onClick={()=>setPage(currentPage-1)}>上一页</button><span>{currentPage+1}/{maxPage+1}</span><button disabled={currentPage===maxPage} onClick={()=>setPage(currentPage+1)}>下一页</button></div>
      <div className="net-table-wrap"><table><thead><tr><th>商品 / SKU</th><th>原销售额</th><th>原销量</th><th>取消件数</th><th>扣减金额</th><th>净销售额</th><th>净销量</th><th>金额依据</th></tr></thead><tbody>
        {products.slice(currentPage*100,currentPage*100+100).map(p=><tr key={p.sku}><td className="net-product"><b>{p.offerId || p.sku}</b><small>{p.name}</small><small>SKU {p.sku}</small></td><td>{money(p.revenue)}</td><td>{p.units}</td><td>{p.cancellationUnits}</td><td>{money(p.cancellationAmount)}</td><td>{money(p.netRevenue)}</td><td>{p.netUnits}</td><td>{p.amountBasis}</td></tr>)}
        {!products.length && <tr><td colSpan={8}>暂无匹配商品</td></tr>}
      </tbody></table></div>
    </details>
  </section>;
}
