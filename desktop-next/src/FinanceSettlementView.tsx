import { useEffect, useState } from "react";
import type { FinanceSettlementReport } from "./types";
import "./finance-settlement.css";

const money = (v: number | null) => v == null ? "待核对" : `¥${v.toLocaleString("zh-CN", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`;

export function FinanceSettlementView({ data, rate, from, to, onEditCost }: {data: FinanceSettlementReport; rate: number; from: string; to: string; onEditCost: (sku: string) => void}) {
  const [query, setQuery] = useState("");
  const [onlyIssues, setOnlyIssues] = useState(false);
  const [page, setPage] = useState(0);
  const rub = (v: number) => `${(v * rate).toLocaleString("zh-CN", { minimumFractionDigits: 2, maximumFractionDigits: 2 })} ₽`;
  const salesAndReturns = data.saleCny + data.refundsCny;
  useEffect(() => { setPage(0); }, [query, onlyIssues, data]);
  const rows = data.rows.filter((r) => (!onlyIssues || r.issues.length || r.returnReview) && `${r.postingNumber} ${r.relatedProducts.map((p) => `${p.sku} ${p.offerId} ${p.productName}`).join(" ")} ${r.entries.map((e) => `${e.operationId} ${e.operationType}`).join(" ")}`.toLowerCase().includes(query.toLowerCase().trim()));
  return <section className="card table-card finance-settlement">
    <div className="section-heading"><div><h2>Finance 入账利润核算</h2><p>{from} 至 {to} · 按平台入账日期 · {data.operationCount} 笔流水 · 尚未核实银行卡到账</p></div></div>
    <div className="report-hero">
      <div><span>销售和退货（Finance 应计口径）</span><strong>{rub(salesAndReturns)}</strong><small>{money(salesAndReturns)} · 含平台补贴及退款冲减</small></div>
      <div><span>应计费用及其他调整</span><strong>{rub(data.netCny - salesAndReturns)}</strong><small>{money(data.netCny - salesAndReturns)} · 平台流水实际扣费</small></div>
      <div><span>合计净额</span><strong>{rub(data.netCny)}</strong><small>{money(data.netCny)} · 卢布原额 ÷ {rate}</small></div>
    </div>
    <p className="sync-message">请将以上三项与 Ozon「商店经济状况」相同日期、相同店铺对照。下方“下单口径预估”的销售额按下单日期统计，不能直接与销售应计比较。此处为本地 Finance 缓存；若与后台仍有差额，请全量重读本期 Finance，应计数据可能在后续补记或调整。</p>
    <div className="report-hero">
      <div><span>Finance 入账净额</span><strong>{money(data.netCny)}</strong><small>已含本期退款及平台扣费</small></div>
      <div><span>平台实际运费 / 退货物流</span><strong>{money(data.deliveryCny + data.returnDeliveryCny)}</strong><small>已包含在净额中</small></div>
      <div><span>Finance 实扣广告推广</span><strong>{money(data.advertisingCny)}</strong><small>已包含在净额中，不再扣 Performance 花费</small></div>
      <div><span>对应入账订单采购成本</span><strong>{money(data.purchaseCny)}</strong><small>已匹配 {money(data.knownPurchaseCny)} · {data.missingCostOrders} 组待核对</small></div>
      <div><span>入账利润（按录入采购成本）</span><strong>{money(data.profitCny)}</strong><small>入账净额 − 对应采购成本</small></div>
      <div><span>退货成本未冲回前利润</span><strong>{money(data.profitBeforeReturnRecoveryCny)}</strong><small>{data.returnReviewOrders} 组退款待核实退货回仓</small></div>
    </div>
    <div className="sync-message">
      退款及取消产生的实际扣款按 Finance 入账计入；未成交取消订单不计销售成本，也不重复从净额扣退款。广告、佣金、平台运费已在净额内，仅作拆解展示。
      <br/>采购成本按 Finance 明确商品数量或匹配订单数量 × 当前录入的单位采购成本；不是历史采购批次成本。无销售入账的费用/退款行不再次扣商品成本。未入账费用、平台外费用及税费不在此表内。
      <br/>退货退款不等于商品已回仓：退款已扣入净额，采购成本暂不自动冲回；存在退货待核对时不标为最终利润。1 CNY = {rate} RUB，金额按当前汇率换算，尚未核实银行结汇损益。
    </div>
    <table><thead><tr><th>净额拆解（收入为正 / 扣费为负）</th><th>销售应计</th><th>退款冲减</th><th>佣金</th><th>正向物流</th><th>退货物流</th><th>广告推广</th><th>收单及其他调整</th><th>合计净额</th></tr></thead><tbody><tr><td>{data.operationCount} 笔流水</td>{[data.saleCny,data.refundsCny,data.commissionCny,data.deliveryCny,data.returnDeliveryCny,data.advertisingCny,data.otherCny,data.netCny].map((v,i)=><td key={i}>{money(v)}</td>)}</tr></tbody></table>
    <div className="section-heading"><input aria-label="查找入账订单或流水" placeholder="订单号 / SKU / 货号 / 流水 ID" value={query} onChange={(e)=>setQuery(e.target.value)}/><label><input type="checkbox" checked={onlyIssues} onChange={(e)=>setOnlyIssues(e.target.checked)}/>仅待核对</label><span>{rows.length} 组</span></div>
    {rows.slice(page*30,(page+1)*30).map((r)=><details key={r.key} style={{padding:"12px 16px",borderBottom:"1px solid #e5ebf4"}}>
      <summary style={{cursor:"pointer"}}><b>{r.postingNumber || "店铺级 / 未关联订单"}</b> · {r.firstDay}～{r.lastDay} · {r.entries.length} 笔 · 净额 {money(r.netCny)} · 采购 {money(r.purchaseCny)} · 利润 {money(r.profitCny)} {r.returnReview ? "· 退货回仓待核对" : r.issues.length ? "· 成本待核对" : ""}<small style={{display:"block"}}>对应商品：{r.relatedProducts.map(p=>`${p.offerId || p.sku}（SKU ${p.sku}）`).join("、") || "未关联商品，无法直接定位"}</small></summary>
      {r.relatedProducts.map(p=><div key={p.sku} className="section-heading" onDoubleClick={()=>onEditCost(p.sku)} title="双击打开该商品成本编辑"><b>{p.offerId || "货号未同步"} · SKU {p.sku}</b><span>{p.productName}</span><button onClick={()=>onEditCost(p.sku)}>查看 / 编辑成本</button></div>)}
      {r.issues.map((s)=><p className="sync-message" key={s}>{s}</p>)}
      {r.products.length ? <table><thead><tr><th>货号 / SKU</th><th>入账确认数量</th><th>数量依据</th><th>当前单位采购成本</th><th>本期计入成本</th></tr></thead><tbody>{r.products.map((p)=><tr key={p.sku}><td>{p.offerId || "—"}<small>{p.sku}</small></td><td>{p.quantity ?? "待核对"}</td><td>{p.quantitySource}</td><td>{money(p.unitCostCny)}</td><td>{money(p.purchaseCny)}</td></tr>)}</tbody></table> : <p>本期未新增确认该订单的商品成本；费用 / 退款已计入下方流水。</p>}
      {r.returnReview && <p className="sync-message">退款已入账；未自动恢复退货商品成本。退货成本未冲回前利润：{money(r.profitBeforeReturnRecoveryCny)}。</p>}
      <table><thead><tr><th>入账日 / 流水 ID</th><th>平台类型</th><th>销售 / 退款</th><th>佣金</th><th>物流</th><th>退货物流</th><th>广告</th><th>收单及其他</th><th>净额</th></tr></thead><tbody>{r.entries.map((e)=><tr key={e.operationId}><td>{e.day}<small>{e.operationId}</small></td><td>{e.operationType}</td>{[e.saleCny,e.commissionCny,e.deliveryCny,e.returnDeliveryCny,e.advertisingCny,e.otherCny,e.netCny].map((v,i)=><td key={i}>{money(v)}</td>)}</tr>)}</tbody></table>
    </details>)}
    {!rows.length && <p className="empty">没有符合条件的入账流水。</p>}
    <div className="section-heading"><button disabled={page===0} onClick={()=>setPage(p=>p-1)}>上一页</button><span>{page+1} / {Math.max(1,Math.ceil(rows.length/30))}</span><button disabled={(page+1)*30>=rows.length} onClick={()=>setPage(p=>p+1)}>下一页</button></div>
  </section>;
}

