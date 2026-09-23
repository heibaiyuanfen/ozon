import { useEffect, useMemo, useRef, useState } from "react";
import { AlertTriangle, Calculator, RefreshCw, Search, Tags } from "lucide-react";
import { priceIntelligence, refreshPriceIntelligence, saveProfitMonitorSettings } from "./bridge";
import type { PriceIntelligenceData } from "./types";
import { RepriceDialog } from "./RepriceDialog";
import "./price-intelligence.css";

const rub = (value: number | null) => value == null ? "—" : `${value.toLocaleString("zh-CN", { maximumFractionDigits: 2 })} ₽`;
const cny = (value: number | null) => value == null ? "—" : `¥${value.toLocaleString("zh-CN", { maximumFractionDigits: 2 })}`;
const percent = (value: number | null) => value == null ? "—" : `${(value * 100).toFixed(1)}%`;

export function PriceIntelligencePage() {
  const [data, setData] = useState<PriceIntelligenceData | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [query, setQuery] = useState("");
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<{ done: number; total: number; refreshed: number; failed: number } | null>(null);
  const stopRequested = useRef(false);
  const [message, setMessage] = useState("");
  const [warning, setWarning] = useState("15");
  const [repriceSkus, setRepriceSkus] = useState<string[] | null>(null);
  const load = () => priceIntelligence().then((value) => { setData(value); setWarning(String(value.warningMargin)); }).catch((e) => setMessage(String(e)));
  useEffect(() => { void load(); }, []);
  const rows = useMemo(() => (data?.rows ?? []).filter((row) => `${row.sku} ${row.offerId} ${row.productName}`.toLowerCase().includes(query.trim().toLowerCase())), [data, query]);
  const refresh = async (chosen: string[]) => {
    if (busy || !data) return;
    const targets = chosen.length ? chosen : data.rows.filter((row) => !row.syncedAt).map((row) => row.sku);
    if (!targets.length) { setMessage(data.rows.length ? "所有商品已有本地缓存；重新打开软件会直接读取缓存。若需更新价格，请勾选商品后点击“更新所选”。" : "当前没有可扫描的商品，请先同步商品资料"); return; }
    stopRequested.current = false;
    setBusy(true); setMessage("");
    let refreshed = 0;
    let failed = 0;
    const errors: string[] = [];
    let done = 0;
    setProgress({ done, total: targets.length, refreshed, failed });
    try {
      // Paint the progress panel before dispatching the first native command.
      await new Promise((resolve) => window.setTimeout(resolve, 20));
      // Selected updates use smaller batches so progress and stop are responsive.
      const batchSize = chosen.length ? 5 : 25;
      for (let offset = 0; offset < targets.length; offset += batchSize) {
        if (stopRequested.current) break;
        const batch = targets.slice(offset, offset + batchSize);
        try {
          const result = await refreshPriceIntelligence(batch);
          refreshed += result.refreshed;
          failed += result.failed;
          errors.push(...result.errors);
          setData(result.data);
          if (result.errors.some((error) => /限频|HTTP 429|认证失败|HTTP 401|HTTP 403/.test(error))) {
            stopRequested.current = true;
          }
        } catch (e) {
          failed += batch.length;
          errors.push(String(e));
          stopRequested.current = true;
        }
        done += batch.length;
        setProgress({ done, total: targets.length, refreshed, failed });
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      }
      setSelected(new Set());
      const stopped = done < targets.length;
      setMessage(`${stopped ? "扫描已停止；" : "扫描完成；"}已处理 ${done}/${targets.length}，成功 ${refreshed}，失败 ${failed}${errors.length ? `。${errors.slice(0, 2).join("；")}` : ""}`);
    } finally { setBusy(false); }
  };
  const saveLimit = async () => {
    try { const value = await saveProfitMonitorSettings(Number(warning)); setData(value); setMessage("利润率预警线已保存"); }
    catch (e) { setMessage(String(e)); }
  };
  const complete = data?.rows.filter((r) => r.profitMargin != null).length ?? 0;
  const warnings = data?.rows.filter((r) => r.warning).length ?? 0;
  const shownPrice = (value: number | null, currencyCode: string) => {
    if (!data?.isCrossBorder) return rub(value);
    if (value == null) return "—";
    return cny(currencyCode.toUpperCase() === "CNY" ? value : value / data.rubPerCny);
  };
  return <main className="price-intelligence-page">
    <header className="pi-head">
      <div><span>OZON PRICE INTELLIGENCE</span><h1>价格、补贴与跨境利润监控</h1><p>独立读取前台价、促销价和原价；跨境店按完整商品资料核算并缓存利润。</p></div>
      <button className="primary" disabled={busy} onClick={() => refresh([])}><RefreshCw size={16} className={busy ? "spin" : ""}/>{busy ? "正在分批扫描" : "扫描未缓存商品"}</button>
    </header>
    <section className="pi-summary">
      <article><Tags/><span>已缓存商品</span><strong>{data?.rows.filter((r) => !!r.syncedAt).length ?? 0}</strong></article>
      <article><Calculator/><span>可完整核算</span><strong>{complete}</strong></article>
      <article className={warnings ? "danger" : ""}><AlertTriangle/><span>低利润预警</span><strong>{warnings}</strong></article>
      <article><span>当前汇率</span><strong>1 CNY = {data?.rubPerCny.toFixed(4) ?? "—"} RUB</strong><small>跨境利润统一换算成人民币</small></article>
    </section>
    {busy && progress && <section className="pi-progress" aria-live="polite"><div><strong>正在同步价格</strong><span>已处理 {progress.done}/{progress.total} · 成功 {progress.refreshed} · 失败 {progress.failed}</span><button onClick={() => { stopRequested.current = true; }}>完成当前批次后停止</button></div><progress max={progress.total} value={progress.done}/><small>更新所选每批最多 5 个商品，后台执行；已完成的结果会保留。遇到限频或权限错误会自动停止。</small></section>}
    <section className="pi-toolbar card">
      <label className="pi-search"><Search size={16}/><input value={query} onChange={(e)=>setQuery(e.target.value)} placeholder="搜索 SKU、货号或商品名"/></label>
      <button disabled={!selected.size || busy} onClick={()=>refresh([...selected])}>更新所选（{selected.size}）</button>
      {data?.isCrossBorder && <><button disabled={busy || ![...selected].some((sku) => data.rows.some((row) => row.sku === sku && row.warning))} onClick={() => setRepriceSkus([...selected].filter((sku) => data.rows.some((row) => row.sku === sku && row.warning)))}>所选预警商品改价试算</button><button disabled={!warnings || busy} onClick={() => setSelected(new Set(data.rows.filter((row) => row.warning).map((row) => row.sku)))}>选择全部预警</button></>}
      {data?.isCrossBorder && <div className="pi-limit"><label>利润率低于</label><input type="number" value={warning} onChange={(e)=>setWarning(e.target.value)}/><span>% 时预警</span><button onClick={saveLimit}>保存</button></div>}
    </section>
    {message && <div className="sync-message">{message}</div>}
    <section className="pi-assumptions">
      {data?.isCrossBorder ? <><b>跨境预估规则</b><span>阶梯跨境运费已包含全部运费，不加头程</span><span>货损 (成本 + 贴单 + 跨境运费) × 3%</span><span>广告 10%</span><span>贴单 ¥2</span><span>佣金 ≤¥134：13%，&gt;¥134：15%</span><span>物流佣金 2%</span></> : <><b>本土店</b><span>不使用跨境运费阶梯；物流费用以本土实际结算为准</span></>}
    </section>
    <section className="card pi-table-wrap"><table className="pi-table"><thead><tr>
      <th><input type="checkbox" checked={!!rows.length && rows.every(r=>selected.has(r.sku))} onChange={(e)=>setSelected(e.target.checked?new Set(rows.map(r=>r.sku)):new Set())}/></th><th>商品</th><th>前台价<br/><small>补贴后</small></th><th>促销价</th><th>设置原价</th><th>最终定价</th><th>Ozon 补贴</th><th>成本资料</th>{data?.isCrossBorder && <><th>费用拆解</th><th>利润 / 利润率</th></>}<th>状态</th>
    </tr></thead><tbody>{rows.map((row)=><tr key={row.sku} className={row.warning?"warning-row":""}>
      <td><input type="checkbox" checked={selected.has(row.sku)} onChange={(e)=>setSelected(old=>{const next=new Set(old);e.target.checked?next.add(row.sku):next.delete(row.sku);return next;})}/></td>
      <td><b>{row.offerId || row.sku}</b><span>{row.productName || "未读取商品名"}</span><small>SKU {row.sku}</small></td>
      <td className="front-price">{shownPrice(row.frontendPrice, row.currencyCode)}</td><td>{shownPrice(row.promotionPrice, row.currencyCode)}</td><td>{shownPrice(row.originalPrice, row.currencyCode)}</td><td><b>{shownPrice(row.finalPricing, row.currencyCode)}</b></td>
      <td><b>{percent(row.subsidyRate)}</b><small>{shownPrice(row.subsidyAmount, row.currencyCode)}</small></td>
      <td><span>成本 {cny(row.purchaseCostCny)}</span><small>体积 {row.volumeL==null?"—":`${row.volumeL.toFixed(2)} L`} · {row.weightKg==null?"—":`${row.weightKg} kg`}</small>{data?.isCrossBorder ? <small>跨境运费（全部运费）{cny(row.freightCny)}</small> : <small>本土物流费按实际结算，不套用跨境阶梯</small>}</td>
      {data?.isCrossBorder && <><td><span>广告 {cny(row.advertisingCny)} · 货损 {cny(row.damageCny)}</span><small>平台佣金 {cny(row.commissionCny)} · 物流佣金 {cny(row.logisticsCommissionCny)}</small><small>贴单 {cny(row.labelFeeCny)}</small></td><td className={row.warning?"profit danger-text":"profit"}><b>{cny(row.profitCny)}</b><small>{percent(row.profitMargin)}</small></td></>}
      <td>{row.warning && <div className="warning-badge">需要改价</div>}{row.missingFields.length ? <div className="missing">缺：{row.missingFields.join("、")}</div> : !row.warning && <div className="ok-badge">正常</div>}<small>{row.syncedAt||"尚未读取"}</small></td>
    </tr>)}</tbody></table>{!rows.length&&<div className="empty">没有符合条件的商品</div>}</section>
    {repriceSkus && data && <RepriceDialog skus={repriceSkus} warningMargin={data.warningMargin} onClose={() => setRepriceSkus(null)} onChanged={() => { void load(); }}/>} 
  </main>;
}
