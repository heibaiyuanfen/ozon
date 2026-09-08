import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { AlertTriangle, CheckCircle2, FileUp, GitBranch, RefreshCw } from "lucide-react";
import { openExperiment } from "./AdExperimentCenter";
import "./ad-attribution.css";

type Data = { series: { id: number; name: string }[]; rows: any[]; flows?: any[]; recommendations?: any[]; summary: any; coverage: any };
const iso = (d: Date) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
const money = (v: number) => `₽${(v || 0).toLocaleString("zh-CN", { maximumFractionDigits: 2 })}`;
const ratio = (v: number | null, s = "") => v == null ? "—" : v.toFixed(2) + s;
const roleName: Record<string, string> = { traffic_driver: "引流款", main_converter: "主成交款", profit_variant: "利润款", upgrade_receiver: "升级承接款", balanced_hub: "双向枢纽", cannibalizing_variant: "内部抢单风险", low_efficiency: "低效款", insufficient_data: "数据不足" };
const decisionName: Record<string, string> = { INCREASE_10: "预算增加 10%", INCREASE_5: "预算增加 5%", REDUCE_15: "预算减少 15%", PRICE_UP_3: "价格测试 +3%", INSUFFICIENT_DATA: "样本不足", HOLD_3_DAYS: "保持观察 3 天" };
const confidenceName: Record<string, string> = { HIGH: "高置信", MEDIUM: "中置信", LOW: "低置信" };
const experimentAction = (decision: string) => decision?.startsWith("INCREASE") ? "increase_budget" : decision?.startsWith("REDUCE") ? "reduce_budget" : decision === "PRICE_UP_3" ? "price_change" : "hold";

function VariantMvp({ rows, flows }: { rows: any[]; flows: any[] }) {
  const priced = [...rows].filter(x => x.price != null).sort((a, b) => a.price - b.price);
  return <>
    {!!rows.length && <section className="variant-role-grid">
      {rows.map(x => <article className={`role-card ${x.role || "unknown"}`} key={x.sku}>
        <div><b>{x.offerId}</b><span>{roleName[x.role] || "数据不足"}</span></div>
        <strong>{x.budgetOpportunityScore ?? 0}<small>/100 预算机会分</small></strong>
        <dl><div><dt>Outbound</dt><dd>{money(x.outboundCrossRevenue)}</dd></div><div><dt>Inbound</dt><dd>{money(x.inboundAssistedRevenue)}</dd></div><div><dt>Net Flow</dt><dd>{money(x.netFlow)}</dd></div><div><dt>系列弹性</dt><dd>{ratio(x.seriesScaleElasticity)}</dd></div><div><dt>投入成交差</dt><dd>{ratio(x.trafficSalesGap, "%")}</dd></div><div><dt>Own依赖</dt><dd>{ratio(x.ownAdDependency, "%")}</dd></div></dl>
        <footer><span className={`confidence ${x.confidence?.toLowerCase()}`}>{confidenceName[x.confidence] || "低置信"}</span><b>{decisionName[x.decision] || x.decision}</b></footer>
        <button className="experiment-link" onClick={() => openExperiment({ skus: [x.sku], name: `${x.offerId} ${decisionName[x.decision] || "优化"}`, experimentType: x.decision === "PRICE_UP_3" ? "price_test" : "budget_reallocation", observationDays: 3, actions: { [x.sku]: experimentAction(x.decision) }, notes: `角色：${roleName[x.role]}；置信度：${confidenceName[x.confidence]}；机会分：${x.budgetOpportunityScore}；成功条件：Series Units 与 Revenue 不低于基线，Series TACOS 不升高；回退条件：Series Units 下降超过 5%。` })}>创建实验</button>
      </article>)}
    </section>}
    <section className="variant-panels">
      <article className="card flow-panel"><header><div><h2>跨尺寸成交网络</h2><p>广告入口 SKU → 最终成交 SKU</p></div></header>{flows.length ? <div className="flow-list">{flows.map((f, i) => <div key={`${f.entrySku}-${f.purchasedSku}-${i}`}><b>{f.entrySku}</b><i>→</i><b>{f.purchasedSku}</b><span>{f.units} 件 · {money(f.revenue)}</span></div>)}</div> : <div className="empty">导入含 Union 工作表的推广分析报告后显示真实流向。</div>}</article>
      <article className="card ladder-panel"><header><div><h2>价格阶梯</h2><p>检查升级承接与价格断层</p></div></header>{priced.length ? <div className="price-ladder">{priced.map((x, i) => <div key={x.sku}><span>{x.offerId}</span><b>{money(x.price)}</b><small>{i ? `价差 ${money(x.price - priced[i - 1].price)}` : "最低价格"} · {x.units || 0} 件</small></div>)}</div> : <div className="empty">同步商品价格后显示价格阶梯。</div>}</article>
    </section>
  </>;
}

export function AdAttributionPage() {
  const now = new Date(), before = new Date(now); before.setDate(now.getDate() - 6);
  const [from, setFrom] = useState(iso(before)), [to, setTo] = useState(iso(now)), [sid, setSid] = useState<number | null>(null), [data, setData] = useState<Data>({ series: [], rows: [], flows: [], recommendations: [], summary: null, coverage: {} }), [busy, setBusy] = useState(false), [msg, setMsg] = useState(""), [path, setPath] = useState("");
  const load = async (id = sid) => { setBusy(true); try { const v = await invoke<Data>("ad_attribution_command", { from, to, seriesId: id }); setData(v); if (id == null && v.series[0]) setSid(v.series[0].id); } catch (e) { setMsg(String(e)); } finally { setBusy(false); } };
  useEffect(() => { void load(null); }, []);
  useEffect(() => { if (sid != null) void load(sid); }, [sid, from, to]);
  const importExcel = async () => { setBusy(true); try { const v = await invoke<any>("import_variant_report", { path }); setFrom(v.periodFrom); setTo(v.periodTo); setMsg(`导入成功：广告统计 ${v.statisticsRows} 行，跨尺寸成交 ${v.flowRows} 行`); await load(); } catch (e) { setMsg(String(e)); } finally { setBusy(false); } };
  const direct = data.coverage?.mode === "direct", s = data.summary;
  return <div className="attribution-page">
    <header><div><span><GitBranch />变体引流网络</span><h1>系列广告归因</h1><p>识别广告入口尺寸、最终成交尺寸、系列增量与内部抢单。</p></div><div className="attr-filters"><select value={sid ?? ""} onChange={e => setSid(Number(e.target.value))}><option value="">选择产品系列</option>{data.series.map(x => <option value={x.id} key={x.id}>{x.name}</option>)}</select><input type="date" value={from} onChange={e => setFrom(e.target.value)} /><span>至</span><input type="date" value={to} onChange={e => setTo(e.target.value)} /><button onClick={() => void load()} disabled={busy}><RefreshCw className={busy ? "spin" : ""} />刷新</button></div></header>
    <section className="excel-import card"><div><FileUp /><div><b>导入 Ozon 推广分析报告</b><span>支持 Statistics + Union；重复导入会覆盖同期同活动数据。</span></div></div><input value={path} onChange={e => setPath(e.target.value)} placeholder="粘贴 .xlsx 文件完整路径" /><button disabled={busy || !path.trim()} onClick={() => void importExcel()}>导入 Excel</button></section>
    <section className="coverage-strip"><article className="ok"><CheckCircle2 /><div><b>API SKU ROAS</b><span>Performance 商品报表可计算</span></div></article><article className={direct ? "ok" : ""}>{direct ? <CheckCircle2 /> : <AlertTriangle />}<div><b>Own / Assisted ROAS</b><span>{direct ? "已按入口与成交 SKU 直接计算" : "导入 Excel 后可计算"}</span></div></article><article className={direct ? "ok" : ""}>{direct ? <CheckCircle2 /> : <AlertTriangle />}<div><b>Cross-size Halo</b><span>{direct ? "Direct Attribution 已启用" : "等待 Union 工作表"}</span></div></article></section>
    {s && <><section className="health-hero"><div><span>Series Health</span><strong>{s.seriesHealthScore}</strong><b>{s.seriesHealthLabel}</b></div><dl><div><dt>Series TACOS</dt><dd>{ratio(s.seriesTacos, "%")}</dd></div><div><dt>销量增长</dt><dd>{ratio(s.seriesGrowth == null ? null : s.seriesGrowth * 100, "%")}</dd></div><div><dt>销售额增长</dt><dd>{ratio(s.revenueGrowth == null ? null : s.revenueGrowth * 100, "%")}</dd></div><div><dt>ASP</dt><dd>{money(s.averageSellingPrice)}</dd></div><div><dt>阶段</dt><dd>{s.growthStage}</dd></div></dl></section><section className="attr-kpis"><article><span>本期系列销售额</span><strong>{money(s.totalSales)}</strong><small>{s.totalUnits || 0} 件 · 前期 {money(s.previousSales)}</small></article><article><span>边际新增销量</span><strong>{ratio(s.marginalSeriesUnits)}</strong><small>每 1000 RUB：{ratio(s.marginalUnitsPer1000)} 件</small></article><article><span>系列销售额增量</span><strong>{money(s.incrementalSeriesRevenue)}</strong><small>相对等长前周期</small></article><article className="focus"><span>Series Marginal ROAS</span><strong>{ratio(s.marginalSeriesRoas)}</strong><small>销售额增量 ÷ 花费增量</small></article></section><div className="method-note"><b>Data Mode：{direct ? "Direct Attribution" : "Experimental Inference"}</b>。{direct ? "箭头表示报告中的真实 Revenue / Order Flow。" : "不展示精确跨 SKU 路径，只提供估算关系。"} 基线：{s.baselineFrom} 至 {s.baselineTo}。</div></>}
    {!!data.recommendations?.length && <section className="budget-rec card"><header><div><h2>预算迁移建议</h2><p>先判断系列健康，再按边际效率迁移预算</p></div></header>{data.recommendations.map((r, i) => <article key={i}><div><span>从</span><b>{r.fromOfferId}</b><strong>-{r.reducePercent}%</strong></div><i>→</i><div><span>转向</span><b>{r.toOfferId}</b><strong>+{r.increasePercent}%</strong></div><div className="rec-copy"><b>保留 {r.reservePercent}% · 观察 {r.observationDays} 个完整日</b><p>{r.reason}</p><small>成功：{r.successConditions.join("；")}</small><small>回退：{r.rollbackConditions.join("；")}</small></div><button onClick={() => openExperiment({ skus: [r.fromSku, r.toSku], name: `${r.fromOfferId} → ${r.toOfferId} 预算迁移`, seriesId: sid ?? undefined, experimentType: "budget_reallocation", observationDays: r.observationDays, actions: { [r.fromSku]: "reduce_budget", [r.toSku]: "increase_budget" }, notes: `${r.reason}。计划：${r.fromOfferId} -${r.reducePercent}%，${r.toOfferId} +${r.increasePercent}%，保留 ${r.reservePercent}%。成功：${r.successConditions.join("；")}。回退：${r.rollbackConditions.join("；")}。` })}>创建预算迁移实验</button></article>)}</section>}
    <section className="attr-table card"><header><div><h2>Variant Portfolio 明细</h2><p>角色与预算建议不会仅按 Assisted ROAS 排名；低置信样本禁止进入优先放量。</p></div></header><table><thead><tr><th>产品 / SKU</th><th>角色</th><th>点击/订单</th><th>Own ROAS</th><th>Assisted</th><th>Inbound Share</th><th>Outbound Halo</th><th>Net Flow</th><th>投入成交差</th><th>系列弹性</th><th>机会分</th><th>置信度</th><th>Next Best Action</th></tr></thead><tbody>{data.rows.map(x => <tr key={x.sku}><td><b>{x.offerId}</b><small>{x.sku} · {x.name}</small></td><td>{roleName[x.role]}</td><td>{x.clicks ?? "—"} / {x.adOrders ?? "—"}</td><td>{ratio(x.ownRoas)}</td><td>{ratio(x.assistedRoas)}</td><td>{ratio(x.inboundAssistShare, "%")}</td><td>{ratio(x.outboundHaloRate, "%")}</td><td>{money(x.netFlow)}</td><td>{ratio(x.trafficSalesGap, "%")}</td><td>{ratio(x.seriesScaleElasticity)}</td><td>{x.doNotRank ? `${x.budgetOpportunityScore} · 不排名` : x.budgetOpportunityScore}</td><td><span className={`confidence ${x.confidence?.toLowerCase()}`}>{confidenceName[x.confidence]}</span></td><td><b>{decisionName[x.decision]}</b></td></tr>)}</tbody></table>{!data.rows.length && <div className="empty">请先建立产品系列，并同步或导入广告数据。</div>}</section>
    <VariantMvp rows={data.rows} flows={data.flows || []} />
    {msg && <div className="error-banner">{msg}</div>}
  </div>;
}
