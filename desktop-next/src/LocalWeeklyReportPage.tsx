import {LocalProfitDetail} from "./LocalProfitDetail";
import { useEffect, useState } from "react";
import { localWeeklyReport, seriesInsights } from "./bridge";
import type { InsightRow, LocalWeeklyReport } from "./types";
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

function WeeklySeriesDetail({ report }: { report: LocalWeeklyReport }) {
  const [series, setSeries] = useState<InsightRow[] | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    let active = true;
    seriesInsights(report.to).then(
      rows => { if (active) setSeries(rows); },
      reason => { if (active) setError(String(reason)); },
    );
    return () => { active = false; };
  }, [report]);
  if (error) return <p role="alert">读取系列失败：{error}</p>;
  if (!series) return <p>正在读取系列…</p>;
  const postingsBySku = new Map<string, string[]>();
  for (const [sku, posting] of report.orderPostings ?? []) {
    const postings = postingsBySku.get(sku);
    if (postings) postings.push(posting);
    else postingsBySku.set(sku, [posting]);
  }
  const assigned = new Set(series.flatMap(row => row.skus.map(String)));
  const groups = [
    ...series.map(row => ({ key: row.id, name: row.name, skus: new Set(row.skus.map(String)) })),
    { key: "unassigned", name: "未分组", skus: new Set([...report.products, ...report.previousProducts].filter(row => !assigned.has(String(row.sku))).map(row => String(row.sku))) },
  ];
  const rows = groups.map(group => {
    const products = report.products.filter(row => group.skus.has(String(row.sku)));
    const sum = (key: "revenue" | "units") => products.reduce((total, row) => total + row[key], 0);
    const optional = (key: "adRevenue" | "adUnits" | "adSpend") => {
      const values = products.flatMap(row => row[key] == null ? [] : [row[key]]);
      return values.length ? values.reduce((total, value) => total + value, 0) : null;
    };
    const orderCount = report.orderPostings == null ? null :
      new Set([...group.skus].flatMap(sku => postingsBySku.get(sku) ?? [])).size;
    return { ...group, orderCount, count: products.length, revenue: sum("revenue"), units: sum("units"),
      adRevenue: optional("adRevenue"), adUnits: optional("adUnits"), adSpend: optional("adSpend") };
  }).filter(row => row.count).sort((a, b) => b.revenue - a.revenue);
  const previousRevenue = new Map(report.previousProducts.map(row => [String(row.sku), row.revenue]));
  const weekChange = (row: typeof rows[number]) => {
    const baseline = [...row.skus].reduce((total, sku) => total + (previousRevenue.get(sku) ?? 0), 0);
    if (report.salesDays < 7 || report.previousSalesDays < 7 || baseline <= 0) return "—";
    const change = (row.revenue / baseline - 1) * 100;
    return (change > 0 ? "+" : "") + pct(change);
  };
  const ratio = (spend: number | null, revenue: number | null) =>
    spend != null && revenue != null && revenue > 0 ? spend / revenue * 100 : null;
  return <><p>按所选周的商品明细汇总。商品属于多个系列时会计入各所属系列，系列之间的金额可能重叠。销售额环比与上一完整周比较，两周销售数据覆盖不足 7 天或上周销售额为零时显示“—”。广告订单占比＝系列广告归因订单数 ÷ 该系列总订单数（按发货单号去重）。订单未同步齐全或总订单数为零时显示“—”。平均单价＝销售额 ÷ 销售件数。</p>
    <div className="weekly-detail-scroll"><table><thead><tr>
      <th>系列</th><th>商品数</th><th>销售额</th><th>销售额环比</th><th>广告销售额</th><th>销售数量</th><th>广告订单占比</th><th>广告费</th><th>平均单价</th><th>TACOS</th>
    </tr></thead><tbody>{rows.map(row => <tr key={row.key}>
      <td>{row.name}</td><td>{row.count}</td><td>{rub(row.revenue)}</td><td>{weekChange(row)}</td>
      <td>{row.adRevenue == null ? "—" : rub(row.adRevenue)}</td><td>{row.units} 件</td>
      <td>{pct(ratio(row.adUnits, row.orderCount))}</td>
      <td>{row.adSpend == null ? "—" : rub(row.adSpend)}</td>
      <td>{row.units > 0 ? rub(row.revenue / row.units) : "—"}</td><td>{pct(ratio(row.adSpend, row.revenue))}</td>
    </tr>)}</tbody></table></div>{!rows.length && <div className="empty">本周没有商品明细</div>}</>;
}

export function LocalWeeklyReportPage() {
  const [weekEnd,setWeekEnd]=useState(latestCompleteSunday);
  const [report,setReport]=useState<LocalWeeklyReport|null>(null);
  const [error,setError]=useState("");
  const [revision,setRevision]=useState(0);
  const [detailTab,setDetailTab]=useState<"daily"|"products"|"profit"|"log"|"series">("daily");
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
    {label:"财务销售和退货",value:rub(report.financeSales),note:"Finance 入账，税费基数"},
    {label:"下单销售额",value:rub(report.revenue),note:"Seller 销售统计"},
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
    {report && <p className="local-weekly-note">{report.profitNote}。广告销售数量沿用广告接口归因订单数；ACOS、TACOS 仍以下单销售额为分母，与财务销售和退货口径不同；ACOS、TACOS 由整周合计计算，分母为零显示“—”。销售缓存覆盖 {report.salesDays}/7 天，广告缓存覆盖 {report.adDays}/7 天。</p>}
    {report && <section className="card weekly-details">
      <div className="weekly-detail-tabs">{(["daily","products","profit","log","series"] as const).map(tab=><button key={tab} className={detailTab===tab?"active":""} onClick={()=>setDetailTab(tab)}>{tab==="daily"?"每日明细":tab==="products"?"商品明细":tab==="profit"?"利润明细":tab==="log"?"利润变动记录":"系列明细"}</button>)}</div>
      {detailTab==="series" && <WeeklySeriesDetail key={report.to + ":" + revision} report={report}/> }
      {detailTab==="daily" && <><p>所选周的 7 天数据；广告费用于核对 ACOS 与 TACOS，日金额及数量合计与上方周汇总一致。</p><div className="weekly-detail-scroll"><table><thead><tr><th>日期</th><th>销售额</th><th>广告销售额</th><th>销售数量</th><th>广告销售数量</th><th>广告费</th><th>ACOS</th><th>TACOS</th></tr></thead><tbody>{report.daily.map(r=><tr key={r.day}><td>{r.day}</td><td>{rub(r.revenue)}</td><td>{rub(r.adRevenue)}</td><td>{r.units} 件</td><td>{r.adUnits} 单</td><td>{rub(r.adSpend)}</td><td>{pct(r.acos)}</td><td>{pct(r.tacos)}</td></tr>)}</tbody><tfoot><tr><th>本周合计</th><th>{rub(report.revenue)}</th><th>{rub(report.adRevenue)}</th><th>{report.units} 件</th><th>{report.adUnits} 单</th><th>{rub(report.daily.reduce((s,r)=>s+r.adSpend,0))}</th><th>{pct(report.acos)}</th><th>{pct(report.tacos)}</th></tr></tfoot></table></div></>}
      {detailTab==="products" && <><div className="weekly-product-search"><input value={query} onChange={e=>setQuery(e.target.value)} placeholder="搜索货号、SKU 或商品名称"/><span>{productRows.length} 个商品</span></div><p>商品广告使用 SKU 归因明细。店铺级未归属广告不分摊到商品；商品广告合计可能与店铺级周汇总不同，“—”表示没有可归属数据。</p><div className="weekly-detail-scroll"><table><thead><tr><th>商品 / SKU</th><th>销售额</th><th>广告销售额</th><th>销售数量</th><th>广告销售数量</th><th>广告费</th><th>ACOS</th><th>TACOS</th></tr></thead><tbody>{visibleProducts.map(r=><tr key={r.sku}><td><b>{r.offerId||r.sku}</b><small>{r.name}</small><small>SKU {r.sku}</small></td><td>{rub(r.revenue)}</td><td>{optionalRub(r.adRevenue)}</td><td>{r.units} 件</td><td>{r.adUnits==null?"—":`${r.adUnits} 单`}</td><td>{optionalRub(r.adSpend)}</td><td>{pct(r.acos)}</td><td>{pct(r.tacos)}</td></tr>)}</tbody></table></div>{!productRows.length&&<div className="empty">本周没有符合条件的商品数据</div>}{pages>1&&<div className="table-pagination"><button disabled={page===0} onClick={()=>setPage(v=>v-1)}>上一页</button><span>{Math.min(page,pages-1)+1} / {pages}</span><button disabled={page>=pages-1} onClick={()=>setPage(v=>v+1)}>下一页</button></div>}</>}
      {detailTab==="log" && <>
        <p>打开或刷新周报时自动对比。首次读取建立基线，相同结果不会重复记录。结算组成和费用明细用于追查差额，不要与结算净额重复相加。</p>
        <div className="weekly-detail-scroll"><table><thead><tr><th>记录时间（北京时间）</th><th>变动项目</th><th>变动前</th><th>变动后</th><th>差额</th></tr></thead><tbody>
          {report.profitLog.flatMap(({ id, recordedAt, snapshot }, index) => {
            const previous = report.profitLog[index + 1]?.snapshot;
            const time = new Date(recordedAt).toLocaleString("zh-CN", {timeZone:"Asia/Shanghai"});
            const fmt = (cents: number | null | undefined) => cents == null ? "待确认" : rub(cents / 100);
            const fields: Array<[string, number | null | undefined, number | null | undefined]> = [
              ["税后利润", previous?.profit, snapshot.profit],
              ...Object.entries(snapshot.items).filter(([name, value]) => previous && previous.items[name] !== value)
                .map(([name, value]): [string, number | undefined, number] => [name, previous?.items[name], value]),
            ];
            return fields.map(([name, before, after], row) => <tr key={id + "/" + name}>
              {row === 0 && <td rowSpan={fields.length}>{time}{!previous && <small>初始基线</small>}</td>}
              <td>{name}</td><td>{previous ? fmt(before) : "—"}</td><td>{fmt(after)}</td>
              <td>{before == null || after == null ? "—" : fmt(after - before)}</td>
            </tr>);
          })}
        </tbody></table></div>
      </>}
      {detailTab==="profit" && <><p>{report.profitNote}。正数为收入，负数为成本或费用。</p><div className="weekly-detail-scroll"><table><thead><tr><th>项目</th><th>金额（RUB）</th><th>计算说明</th></tr></thead><tbody>{report.profitBreakdown.map(r=><tr key={r.label}><td>{r.label}</td><td>{rub(r.amount)}</td><td>{r.note}</td></tr>)}</tbody><tfoot><tr><th>本周税后利润</th><th>{report.profit==null?"待确认":rub(report.profit)}</th><th>{report.profit==null?"数据未齐，不能将已知费用合计作为完整利润":"结算净额减成本、税费与回款手续费"}</th></tr></tfoot></table></div></>}
    </section>}
    {report && <LocalProfitDetail from={report.from} to={report.to}/>}
  </main>;
}
