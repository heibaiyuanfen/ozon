import { useEffect, useState } from "react";
import { localWeeklyReport } from "./bridge";
import type { LocalWeeklyReport } from "./types";
import "./local-weekly-report.css";

const dateText = (d: Date) => `${d.getFullYear()}-${String(d.getMonth()+1).padStart(2,"0")}-${String(d.getDate()).padStart(2,"0")}`;
const latestCompleteSunday = () => {
  const d = new Date();
  d.setDate(d.getDate()-1);
  d.setDate(d.getDate()-d.getDay());
  return dateText(d);
};
const rub = (v: number) => `₽${v.toLocaleString("zh-CN",{minimumFractionDigits:2,maximumFractionDigits:2})}`;
const pct = (v: number | null) => v == null ? "—" : `${v.toFixed(2)}%`;

export function LocalWeeklyReportPage() {
  const [weekEnd,setWeekEnd]=useState(latestCompleteSunday);
  const [report,setReport]=useState<LocalWeeklyReport|null>(null);
  const [error,setError]=useState("");
  const [revision,setRevision]=useState(0);
  const [detailTab,setDetailTab]=useState<"daily"|"products"|"profit">("daily");
  const [query,setQuery]=useState("");
  const [page,setPage]=useState(0);
  useEffect(()=>setPage(0),[weekEnd,query]);
  const latest=latestCompleteSunday();
  useEffect(()=>{
    let alive=true;
    setReport(null);setError("");
    localWeeklyReport(weekEnd).then(value=>{if(alive)setReport(value);}).catch(e=>{if(alive)setError(String(e));});
    return ()=>{alive=false;};
  },[weekEnd,revision]);
  const shift=(days:number)=>{
    const d=new Date(`${weekEnd}T12:00:00`);d.setDate(d.getDate()+days);
    setWeekEnd(dateText(d)>latest?latest:dateText(d));
  };
  const start=new Date(`${weekEnd}T12:00:00`);start.setDate(start.getDate()-6);
  const metrics=report ? [
    {label:"一周销售总额",value:rub(report.revenue),note:"Seller 销售额合计"},
    {label:"广告销售总额",value:rub(report.adRevenue),note:"广告归因销售额合计"},
    {label:"销售数量",value:`${report.units.toLocaleString("zh-CN")} 件`,note:"Seller 下单销量合计"},
    {label:"广告销售数量",value:`${report.adUnits.toLocaleString("zh-CN")} 单`,note:"Performance API 广告归因订单数"},
    {label:"广告 ACOS",value:pct(report.acos),note:"本周广告费 ÷ 广告销售总额"},
    {label:"TACOS",value:pct(report.tacos),note:"本周广告费 ÷ 销售总额"},
    {label:"一周利润",value:report.profit==null?"待确认":rub(report.profit),note:"已结算税后利润",title:report.profitNote},
  ] : [];
  const productRows=(report?.products??[]).filter(r=>`${r.sku} ${r.offerId} ${r.name}`.toLowerCase().includes(query.trim().toLowerCase()));
  const pages=Math.max(1,Math.ceil(productRows.length/100));
  const visibleProducts=productRows.slice(Math.min(page,pages-1)*100,(Math.min(page,pages-1)+1)*100);
  const optionalRub=(v:number|null)=>v==null?"—":rub(v);
  return <main className="local-weekly-report">
    <header className="page-header"><div><span className="eyebrow">WEEKLY BUSINESS REPORT</span><h1>经营周报</h1><p>本土店 · 周一至周日 · 卢布</p></div></header>
    <div className="month-toolbar"><b>周结束日（周日）</b><input type="date" value={weekEnd} max={latest} onChange={e=>{if(!e.target.value)return;const d=new Date(`${e.target.value}T12:00:00`);d.setDate(d.getDate()+((7-d.getDay())%7));setWeekEnd(dateText(d)>latest?latest:dateText(d));}}/><button onClick={()=>shift(-7)}>上一周</button><button disabled={weekEnd>=latest} onClick={()=>shift(7)}>下一周</button><button onClick={()=>setWeekEnd(latest)}>最新完整周</button><button onClick={()=>setRevision(v=>v+1)}>刷新周报</button><span>{dateText(start)} 至 {weekEnd}</span></div>
    {error && <div className="error-banner">{error}</div>}
    {!report && !error && <div className="empty">正在读取本周销售、广告及利润数据…</div>}
    <section className="local-weekly-metrics">{metrics.map(metric=><article className={`card ${metric.label==="一周利润"?"weekly-profit":""}`} key={metric.label} title={metric.title||metric.note}><span>{metric.label}</span><strong>{metric.value}</strong><small>{metric.note}</small></article>)}</section>
    {report && <p className="local-weekly-note">{report.profitNote}。广告销售数量沿用广告接口归因订单数；ACOS、TACOS 由整周合计计算，分母为零显示“—”。销售缓存覆盖 {report.salesDays}/7 天，广告缓存覆盖 {report.adDays}/7 天。</p>}
    {report && <section className="card weekly-details">
      <div className="weekly-detail-tabs">{(["daily","products","profit"] as const).map(tab=><button key={tab} className={detailTab===tab?"active":""} onClick={()=>setDetailTab(tab)}>{tab==="daily"?"每日明细":tab==="products"?"商品明细":"利润明细"}</button>)}</div>
      {detailTab==="daily" && <><p>所选周的 7 天数据；广告费用于核对 ACOS 与 TACOS，日金额及数量合计与上方周汇总一致。</p><div className="weekly-detail-scroll"><table><thead><tr><th>日期</th><th>销售额</th><th>广告销售额</th><th>销售数量</th><th>广告销售数量</th><th>广告费</th><th>ACOS</th><th>TACOS</th></tr></thead><tbody>{report.daily.map(r=><tr key={r.day}><td>{r.day}</td><td>{rub(r.revenue)}</td><td>{rub(r.adRevenue)}</td><td>{r.units} 件</td><td>{r.adUnits} 单</td><td>{rub(r.adSpend)}</td><td>{pct(r.acos)}</td><td>{pct(r.tacos)}</td></tr>)}</tbody><tfoot><tr><th>本周合计</th><th>{rub(report.revenue)}</th><th>{rub(report.adRevenue)}</th><th>{report.units} 件</th><th>{report.adUnits} 单</th><th>{rub(report.daily.reduce((s,r)=>s+r.adSpend,0))}</th><th>{pct(report.acos)}</th><th>{pct(report.tacos)}</th></tr></tfoot></table></div></>}
      {detailTab==="products" && <><div className="weekly-product-search"><input value={query} onChange={e=>setQuery(e.target.value)} placeholder="搜索货号、SKU 或商品名称"/><span>{productRows.length} 个商品</span></div><p>商品广告使用 SKU 归因明细。店铺级未归属广告不分摊到商品；商品广告合计可能与店铺级周汇总不同，“—”表示没有可归属数据。</p><div className="weekly-detail-scroll"><table><thead><tr><th>商品 / SKU</th><th>销售额</th><th>广告销售额</th><th>销售数量</th><th>广告销售数量</th><th>广告费</th><th>ACOS</th><th>TACOS</th></tr></thead><tbody>{visibleProducts.map(r=><tr key={r.sku}><td><b>{r.offerId||r.sku}</b><small>{r.name}</small><small>SKU {r.sku}</small></td><td>{rub(r.revenue)}</td><td>{optionalRub(r.adRevenue)}</td><td>{r.units} 件</td><td>{r.adUnits==null?"—":`${r.adUnits} 单`}</td><td>{optionalRub(r.adSpend)}</td><td>{pct(r.acos)}</td><td>{pct(r.tacos)}</td></tr>)}</tbody></table></div>{!productRows.length&&<div className="empty">本周没有符合条件的商品数据</div>}{pages>1&&<div className="table-pagination"><button disabled={page===0} onClick={()=>setPage(v=>v-1)}>上一页</button><span>{Math.min(page,pages-1)+1} / {pages}</span><button disabled={page>=pages-1} onClick={()=>setPage(v=>v+1)}>下一页</button></div>}</>}
      {detailTab==="profit" && <><p>{report.profitNote}。正数为收入，负数为成本或费用。</p><div className="weekly-detail-scroll"><table><thead><tr><th>项目</th><th>金额（RUB）</th><th>计算说明</th></tr></thead><tbody>{report.profitBreakdown.map(r=><tr key={r.label}><td>{r.label}</td><td>{rub(r.amount)}</td><td>{r.note}</td></tr>)}</tbody><tfoot><tr><th>本周税后利润</th><th>{report.profit==null?"待确认":rub(report.profit)}</th><th>{report.profit==null?"数据未齐，不能将已知费用合计作为完整利润":"结算净额减成本、税费与回款手续费"}</th></tr></tfoot></table></div></>}
    </section>}
  </main>;
}
