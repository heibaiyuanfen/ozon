import { WbAdOptimization } from "./WbAdOptimization";
import { WbReconciliation } from "./WbReconciliation";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ArrowDownLeft, BellRing, Boxes, CheckCheck, Download, RefreshCw, Search, Settings2, ShoppingBag, Truck, X } from "lucide-react";
import { wbShopCenter } from "./bridge";
import "./wb-workspace.css";
import { init as initChart } from "./charts";
import { WbPromotions } from "./WbPromotions";
import { WbProductEditor } from "./WbProductEditor";
import { WbWeeklyReport } from "./WbWeeklyReport";
import { WbInventory } from "./WbInventory";
import { WbPricing } from "./wb-pricing/WbPricing";

export type WbSection = "daily" | "orders" | "returns" | "pricing" | "promotions" | "products" | "weekly" | "monthly" | "inventory" | "ad_optimization";
type Json = Record<string, any>;
type Row = { id: string; raw: Json; local: Json; updatedAt: string; status: string; day: string; warehouseId?: string | number | null };
type Snapshot = { warehouses: Row[]; orders: Row[]; returns: Row[]; sync: {message?: string; updatedAt?: string}; running: boolean };
type Shop = {id:string;name:string;sellerType:string};
const empty:Snapshot={warehouses:[],orders:[],returns:[],sync:{},running:false};
const stages:Record<string,string>={awaiting_preparation:"等待备货",prepared:"已经备货",delivering:"配送中",awaiting_receipt:"等待签收",received:"已签收",returning:"退货 / 拒收",cancelled:"已取消",unknown:"状态待核实"};
const returnStages:Record<string,string>={buyer_return:"买家退货",application:"退货申请",returning:"退回途中",pickup:"待取件",expired:"已超期 / 待核实",collected:"已取件 / 完成"};
const today=()=>new Intl.DateTimeFormat("en-CA",{timeZone:"Europe/Moscow",year:"numeric",month:"2-digit",day:"2-digit"}).format(new Date());
const before=(days:number)=>{const d=new Date(`${today()}T12:00:00Z`);d.setUTCDate(d.getUTCDate()-days);return d.toISOString().slice(0,10)};
const stamp=(s?:string)=>s?new Date(s).toLocaleString("zh-CN"):"尚未同步";
const api=(action:string,payload:Json)=>invoke<Snapshot>("wb_operations",{action,payload});
const value=(x:unknown)=>x==null?"":String(x);
const sourceLabel=(s:unknown)=>s==="sales_return"?"买家退货":s==="item_return"?"退回卖家":s==="claim"?"退货申请":"其他来源";
type Currency = "CNY" | "RUB";
const currencySymbol = (currency: Currency) => currency === "CNY" ? "¥" : "₽";
const money = (n: number, currency: Currency) => `${currencySymbol(currency)}${n.toLocaleString("zh-CN", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`;

export function WbWorkspace({section,onSection}:{section:WbSection;onSection:(s:WbSection)=>void}) {
  const [shops,setShops]=useState<Shop[]>([]),[shop,setShop]=useState(()=>localStorage.getItem("wb-ops-shop")||"");
  const [settings,setSettings]=useState(false),[error,setError]=useState("");
  const [form,setForm]=useState({name:"",token:""}),[saving,setSaving]=useState(false);
  async function loadShops(){try{const d=await wbShopCenter<{shops:Shop[]}>("dashboard");setShops(d.shops);setShop(s=>d.shops.some(x=>x.id===s)?s:d.shops[0]?.id||"")}catch(e){setError(String(e))}}
  useEffect(()=>{void loadShops()},[]);
  useEffect(()=>{localStorage.setItem("wb-ops-shop",shop)},[shop]);
  async function saveConnection(){setSaving(true);setError("");try{if(form.name.trim()){const result=await wbShopCenter<{shopId:string}>("create_shop",{name:form.name,sellerType:"cross_border",currency:"RUB",timezone:"Europe/Moscow",token:form.token});setShop(result.shopId)}else{if(!shop)throw new Error("请输入新店铺名称");await wbShopCenter("rotate_token",{shopId:shop,token:form.token})}setForm({name:"",token:""});await loadShops();setSettings(false)}catch(e){setError(String(e))}finally{setSaving(false)}}
  const currency: Currency = shops.find(item => item.id === shop)?.sellerType === "cross_border" ? "CNY" : "RUB";
  return <div className="wb-workspace">
    <header className="wbo-header"><div><span className="wbo-eyebrow">WILDBERRIES · 跨境经营工作台</span><h1>{section==="weekly"?"经营周报":section==="monthly"?"经营月报":section==="products"?"商品中心":section==="promotions"?"促销活动":section==="ad_optimization"?"广告优化与评估":section==="inventory"?"库存管理":section==="pricing"?"价格与利润监控":section==="daily"?"经营总览":section==="orders"?"订单中心":"退货中心"}</h1><p>{section==="weekly"||section==="monthly"?"按莫斯科日期汇总已同步的 WB 订单。":section==="products"?"查看当前店铺的商品与图片资料。":section==="promotions"?"查看当前店铺的 Wildberries 促销日历。":section==="ad_optimization"?"评估搜索词组的转化与成本，筛选放量和止损候选。":"每日经营、履约进度与海外仓协同"}</p></div><div className="wbo-actions"><select aria-label="WB 店铺" value={shop} onChange={e=>setShop(e.target.value)}>{!shops.length&&<option value="">请接入 WB 店铺</option>}{shops.map(s=><option key={s.id} value={s.id}>{s.name}</option>)}</select><button onClick={()=>setSettings(true)}><Settings2 size={16}/>店铺连接</button></div></header>
    {error&&<div role="alert" className="wbo-error">{error}</div>}
    {shop?section==="ad_optimization"?<WbAdOptimization key={shop} shopId={shop} defaultFrom={before(7)} defaultTo={before(1)} maxDate={today()}/>:section==="inventory"?<WbInventory key={shop} shopId={shop}/>:section==="weekly"?<WbWeeklyReport key={shop+currency} shopId={shop} currency={currency} today={today()} orderRub={wbOrderAmount} exchange={wbRubPerCny}/>:section==="monthly"?<WbPeriodReport key={shop+section} shopId={shop} period={section} currency={currency}/>:section==="products"?<WbProductEditor key={shop} shopId={shop}/>:section==="promotions"?<WbPromotions key={shop} shopId={shop}/>:section==="pricing"?<WbPricing key={shop} shopId={shop}/>:<Operations key={shop} shopId={shop} currency={currency} section={section} onSection={onSection}/>:<section className="wbo-panel wbo-empty"><Boxes size={40}/><h2>连接店铺，开始管理多个海外仓</h2><p>同步 FBS、DBS 订单和平台退货记录，按仓库安排备货与取件。</p><button className="primary" onClick={()=>setSettings(true)}>接入 WB 店铺</button></section>}
    {settings&&<div className="wbo-overlay"><form className="wbo-dialog" onSubmit={e=>{e.preventDefault();void saveConnection()}}><div className="wbo-heading"><h2>店铺连接</h2><button type="button" aria-label="关闭" onClick={()=>setSettings(false)}><X size={18}/></button></div><p>已有店铺选择后更新 Token；新增店铺填写名称。凭据沿用本机加密保存。</p><label>当前店铺<select value={shop} onChange={e=>setShop(e.target.value)}><option value="">新增店铺</option>{shops.map(s=><option key={s.id} value={s.id}>{s.name}</option>)}</select></label><label>新增店铺名称（更新现有连接时留空）<input value={form.name} onChange={e=>setForm({...form,name:e.target.value})}/></label><label>API Token<input required type="password" autoComplete="new-password" value={form.token} onChange={e=>setForm({...form,token:e.target.value})}/></label><p>订单与仓库需要 Marketplace 权限；退货件需要 Analytics 权限；退货申请需要 Buyers returns 权限。仅执行数据读取。</p>{error&&<p className="wbo-error">{error}</p>}<button className="primary" disabled={saving}>{saving?"保存中…":"保存连接"}</button></form></div>}
  </div>
}

const wbRubPerCny = 12.5;
function wbOrderAmount({ raw }: Pick<Row, "raw">) {
  const amounts = [
    { currency: value(raw.convertedCurrencyCode), amount: raw.convertedFinalPrice ?? raw.convertedPrice },
    { currency: value(raw.currencyCode), amount: raw.finalPrice ?? raw.price },
  ];
  for (const [currency, rate] of [["643", 1], ["156", wbRubPerCny]] as const) {
    const quote = amounts.find(item => item.currency === currency &&
      Number.isFinite(Number(item.amount)) && Number(item.amount) > 0);
    if (quote) return Number(quote.amount) / 100 * rate;
  }
  return 0;
}


type AdDay = {day:string; spend:number; attributedSales:number; detail?: {orders:number|null; products:Record<string,[number,number,number]>|null}|null};

type WbDaily = [string, { total: number; cancelled: number; amount: number; missing: number }][];

function WbAdvertising({currency,shopId,from,to,orders,busy,salesKeys,daily,onDay,onRange,activeSeries}:{shopId:string;from:string;to:string;orders:Row[];busy:boolean;salesKeys:string[];daily:WbDaily;onDay:(day:string)=>void;onRange:(days:number)=>void;activeSeries:string;currency:Currency}) {
  const [ads,setAds]=useState<AdDay[]>([]);
  const [pending,setPending]=useState(false);
  const [error,setError]=useState("");
  useEffect(()=>{
    let active=true;
    invoke<AdDay[]>("wb_operations",{action:"ads_list",payload:{shopId}})
      .then(rows=>{if(active)setAds(rows)})
      .catch(e=>{if(active)setError(String(e))});
    return ()=>{active=false};
  },[shopId]);
  async function sync(start:string,end:string) {
    setPending(true);
    setError("");
    try {
      setAds(await invoke<AdDay[]>("wb_operations",{action:"ads_sync",payload:{shopId,from:start,to:end}}));
    } catch(e) {
      setError(String(e));
    } finally {
      setPending(false);
    }
  }
  const now=today(), week=before(6);
  const selected = new Set(salesKeys);
  const productIds = salesKeys.filter(key => key.startsWith("id:")).map(key => key.slice(3));
  const valid=(start:string,end:string)=>!!start&&!!end&&start<=end&&end<=now&&(Date.parse(end)-Date.parse(start))/86400000<=29;
  const ratio=(spend:number|null,sales:number|null)=>spend!=null&&sales!=null&&sales>0
    ? (100*spend/sales).toFixed(2)+"%" : "—";
  const adsByDay = new Map(ads.map(row => [row.day, row]));
  function adValues(day: string): [number | null, number | null, number | null] {
    const row = adsByDay.get(day);
    if (!row) return [null, null, null];
    if (!selected.size) return [row.spend, row.attributedSales, row.detail?.orders ?? null];
    const products = row.detail?.products;
    if (!products || productIds.length !== salesKeys.length) return [null, null, null];
    return productIds.reduce<[number, number, number]>((sum, id) => {
      const values = products[id] ?? [0, 0, 0];
      return [sum[0] + values[0], sum[1] + values[1], sum[2] + values[2]];
    }, [0, 0, 0]);
  }
  const salesFactor = currency === "CNY" ? 1 / wbRubPerCny : 1;
  const displaySales = (value: number) => money(value * salesFactor, currency);
  const displayAd = (value: number | null) => value === null ? "—" : money(value, currency);
  const spendByDay = daily.map(([day]) => adValues(day)[0]);
  const adTotals = [0, 1, 2].map(index => {
    const values = daily.map(([day]) => adValues(day)[index]);
    return values.length && values.every(value => value !== null)
      ? values.reduce<number>((sum, value) => sum + (value ?? 0), 0)
      : null;
  });
  const sales = daily.reduce((sum, [, day]) => sum + day.amount, 0);
  const hasMissingAmount = daily.some(([, day]) => day.missing > 0);
  const sold = daily.reduce((sum, [, day]) => sum + day.total - day.cancelled, 0);
  const total = daily.reduce((sum, [, day]) => sum + day.total, 0);
  const tacos = ratio(adTotals[0], hasMissingAmount ? null : sales * salesFactor);
  return <>
    <div className="wbo-summary-grid">
      <WbDashboardMetrics title="实时销量" metrics={[
        ["销量", sold], ["销售额", hasMissingAmount ? "—" : displaySales(sales)], ["订单量", total], ["客单价", sold && !hasMissingAmount ? displaySales(sales / sold) : "—"],
      ]}/>
      <WbDashboardMetrics title="广告表现" metrics={[
        ["广告花费", displayAd(adTotals[0])], ["广告销售额", displayAd(adTotals[1])], ["广告订单量", adTotals[2] ?? "—"],
        ["TACOS", tacos],
      ]}/>
    </div>
    <section className="wbo-panel">
      <div className="wbo-heading">
        <div><h2>业绩趋势 · {activeSeries || (salesKeys.length ? "已选商品" : "整店")}</h2><span>{from} 至 {to}</span></div>
        <div className="wbo-actions">
          <button onClick={() => onRange(7)}>最近 7 天</button>
          <button onClick={() => onRange(30)}>最近 30 天</button>
          <a href="#wb-sales-filter">选择单品或系列</a>
        </div>
      </div>
      <WbDashboardMetrics metrics={[
        ["销售额", hasMissingAmount ? "—" : displaySales(sales)], ["销量", sold], ["订单量", total], ["广告花费", displayAd(adTotals[0])],
      ]}/>
      <WbOrderTrend currency={currency} daily={daily.map(([day, row]) => [day, { ...row, amount: row.amount * salesFactor }])} spend={spendByDay} onDay={onDay}/>
      <p className="wbo-muted">点击日期或数据点查看订单。金额以 {currency} 显示，换算按 ¥1 = ₽12.5 估算。广告缺失日期留空。</p>
    </section>
    <section className="wbo-panel">
    <div className="wbo-heading"><h2>广告明细</h2><div className="wbo-actions">
      <button disabled={pending||busy} onClick={()=>void sync(week,now)}>同步近 7 天广告</button>
      <button disabled={pending||busy||!valid(from,to)} onClick={()=>void sync(from,to)}>同步所选日期广告</button>
    </div></div>
    <p className="wbo-muted">莫斯科日期，近 7 天含今日。销售和广告随商品或系列筛选，按全仓 FBS + DBS 汇总。商品广告按 WB 商品 ID 归因，旧缓存需重新同步广告。</p>
    {pending&&<p role="status">正在同步广告统计…</p>}
    {error&&<p role="alert" className="wbo-error">{error}</p>}
    <div className="wbo-table-wrap"><table><thead><tr><th>期间</th><th>总销售额（{currency}）</th><th>总销量（件）</th><th>广告订单</th><th>广告订单占比</th><th>广告费（{currency}）</th><th>ACOS</th><th>TACOS</th><th>广告同步覆盖</th></tr></thead><tbody>
      {([["今日",now,now],["近 7 天",week,now],["所选日期",from,to]]).map(([label,start,end])=>{
        const days=Math.round((Date.parse(end)-Date.parse(start))/86400000)+1;
        const rows=ads.filter(r=>r.day>=start&&r.day<=end);
        const complete=days>0&&rows.length===days;
        const productComplete = complete && productIds.length === salesKeys.length &&
          rows.every(row => row.detail?.products != null);
        const productStats = productComplete ? rows.flatMap(row => productIds.map(id =>
          row.detail?.products?.[id] ?? [0, 0, 0])) : [];
        const spend = selected.size
          ? productComplete ? productStats.reduce((sum, item) => sum + item[0], 0) : null
          : complete ? rows.reduce((sum, row) => sum + row.spend, 0) : null;
        const attributed = selected.size
          ? productStats.reduce((sum, item) => sum + item[1], 0)
          : rows.reduce((sum, row) => sum + row.attributedSales, 0);
        const adOrders = selected.size
          ? productComplete ? productStats.reduce((sum, item) => sum + item[2], 0) : null
          : complete && rows.every(row => row.detail?.orders != null)
            ? rows.reduce((sum, row) => sum + (row.detail?.orders ?? 0), 0) : null;
        const eligible = orders.filter(row => row.day >= start && row.day <= end &&
          !["cancelled", "returning"].includes(row.status) && (!selected.size || selected.has(salesProductKey(row))));
        const amounts=eligible.map(wbOrderAmount);
        const sales=amounts.every(n=>Number.isFinite(n)&&n>0)?amounts.reduce((a,b)=>a+b,0):null;
        return <tr key={label}><td><b>{label}</b><small>{start} 至 {end}</small></td>
          <td>{sales === null ? "—" : displaySales(sales)}</td><td>{eligible.length}</td>
          <td>{adOrders ?? "—"}</td><td>{ratio(adOrders, eligible.length)}</td>
          <td>{displayAd(spend)}</td><td>{ratio(spend,attributed)}</td><td>{ratio(spend,sales === null ? null : sales * salesFactor)}</td>
          <td>{days>0?`${rows.length} / ${days} 天`:"日期待选择"}</td></tr>;
      })}
    </tbody></table></div>
    <p className="wbo-muted">销量按有效订单件数，销售额扣除取消与拒收。广告订单占比 = WB 广告归因订单 ÷ 有效订单数，两者统计口径可能不同。ACOS = 广告费 ÷ 广告归因销售额。TACOS = 同币种广告费 ÷ 有效订单销售额。缺少卢布金额时，人民币订单额按 ¥1 = ₽12.5 估算。广告覆盖不足、金额仍缺失或分母为零时显示“—”。</p>
    </section>
  </>;
}

function WbDashboardMetrics({ title, metrics }: { title?: string; metrics: [string, string | number][] }) {
  return <section className={title ? "wbo-panel" : "wbo-trend-metrics"}>
    {title && <div className="wbo-heading"><h2>{title}</h2></div>}
    <dl className="wbo-metric-row">{metrics.map(([label, value]) => <div key={label}><dt>{label}</dt><dd title={String(value)}>{value}</dd></div>)}</dl>
  </section>;
}

type SalesSeries = { name: string; keys: string[] };

function salesProductKey(row: Row) {
  const id = value(row.raw.nmId ?? row.raw.nm_id).trim();
  if (id && id !== "0") return "id:" + id;
  const article = value(row.raw.article || row.raw.supplierArticle).trim();
  return article ? "article:" + article : "";
}

function Operations({shopId,currency,section,onSection}:{shopId:string;currency:Currency;section:WbSection;onSection:(s:WbSection)=>void}){
  const [data,setData]=useState<Snapshot>(empty),[error,setError]=useState(""),[busy,setBusy]=useState(false),[loading,setLoading]=useState(true);
  const [from,setFrom]=useState(before(6)),[to,setTo]=useState(today()),[warehouse,setWarehouse]=useState(""),[model,setModel]=useState(""),[status,setStatus]=useState(""),[query,setQuery]=useState(""),[page,setPage]=useState(0);
  const [auto,setAuto]=useState(()=>localStorage.getItem(`wb-auto-${shopId}`)==="true"),[edit,setEdit]=useState<Row|null>(null),[draft,setDraft]=useState<Json>({}),[saving,setSaving]=useState(false),[allReturns,setAllReturns]=useState(true),[returnSource,setReturnSource]=useState("");
  const seriesStorage = "wb-sales-series:" + shopId;
  const [series, setSeries] = useState<SalesSeries[]>(() => {
    try {
      const saved = JSON.parse(localStorage.getItem(seriesStorage) ?? "[]");
      return Array.isArray(saved) ? saved.filter(item => typeof item?.name === "string" &&
        Array.isArray(item.keys) && item.keys.every((key: unknown) => typeof key === "string")) : [];
    } catch {
      return [];
    }
  });
  const [salesKeys, setSalesKeys] = useState<string[]>([]);
  const [salesQuery, setSalesQuery] = useState("");
  const [seriesName, setSeriesName] = useState("");
  const [activeSeries, setActiveSeries] = useState("");
  const [seriesMessage, setSeriesMessage] = useState("");
  const products = useMemo(() => {
    const options = new Map<string, string>();
    for (const row of data.orders) {
      const key = salesProductKey(row);
      if (!key || options.has(key)) continue;
      const article = value(row.raw.article || row.raw.supplierArticle);
      options.set(key, [article, key.startsWith("id:") ? "WB " + key.slice(3) : ""].filter(Boolean).join(" · "));
    }
    return [...options].sort((a, b) => a[1].localeCompare(b[1]));
  }, [data.orders]);

  function saveSalesSeries() {
    const name = seriesName.trim();
    if (!name || !salesKeys.length) return;
    if (series.some(item => item.name === name)) {
      setSeriesMessage("已有同名系列，请使用其他名称。");
      return;
    }
    const next = [...series, { name, keys: salesKeys }];
    try {
      localStorage.setItem(seriesStorage, JSON.stringify(next));
      setSeries(next);
      setActiveSeries(name);
      setSeriesName("");
      setSeriesMessage("系列已保存。");
    } catch {
      setSeriesMessage("无法保存系列，本机存储不可用。");
    }
  }

  const alive=useRef(true),syncing=useRef(false),version=useRef(0);
  const running=busy||data.running;
  async function refresh(){const v=++version.current;try{const d=await api("list",{shopId});if(alive.current&&v===version.current)setData(d)}catch(e){if(alive.current)setError(String(e))}finally{if(alive.current)setLoading(false)}}
  useEffect(()=>{alive.current=true;void refresh();return()=>{alive.current=false}},[shopId]);
  useEffect(()=>{if(!running)return;const timer=setInterval(()=>void refresh(),2500);return()=>clearInterval(timer)},[running,shopId]);
  async function sync(automatic=false){if(syncing.current||data.running)return;syncing.current=true;setBusy(true);setError("");try{await api("sync",{shopId,from:automatic?before(6):from,to:automatic?today():to});await refresh()}catch(e){if(alive.current)setError(String(e))}finally{syncing.current=false;if(alive.current)setBusy(false)}}
  useEffect(()=>{localStorage.setItem(`wb-auto-${shopId}`,String(auto));if(!auto)return;const timer=setInterval(()=>void sync(true),60*60*1000);return()=>clearInterval(timer)},[auto,from,to,shopId,data.running]);
  useEffect(()=>{setPage(0)},[warehouse,model,status,query,from,to,section,allReturns,returnSource,salesKeys]);
  useEffect(()=>{setStatus(s=>!s||(section==="returns"?returnStages:stages)[s]?s:"");setQuery("")},[section]);
  const whName=(id:unknown)=>{const s=value(id);return data.warehouses.find(w=>value(w.raw.id)===s)?.raw.name||(s?`仓库 ${s}`:"待关联仓库")};
  const byWh=(r:Row,returns=false)=>!warehouse||(warehouse==="unknown"?!value(returns?r.warehouseId:r.raw.warehouseId):value(returns?r.warehouseId:r.raw.warehouseId)===warehouse);
  const scopedOrders=useMemo(()=>data.orders.filter(r=>byWh(r)&&(!model||r.raw._model===model)&&r.day>=from&&r.day<=to),[data.orders,warehouse,model,from,to]);
  const orders = useMemo(() => {
    if (!salesKeys.length) return scopedOrders;
    const selected = new Set(salesKeys);
    return scopedOrders.filter(row => selected.has(salesProductKey(row)));
  }, [scopedOrders, salesKeys]);
  const returns=useMemo(()=>data.returns.filter(r=>byWh(r,true)&&(allReturns||r.day>=from&&r.day<=to)),[data.returns,warehouse,allReturns,from,to]);
  const reminders=data.returns.filter(r=>byWh(r,true)&&["pickup","expired"].includes(r.status));
  const activeOrders=orders.filter(r=>!["cancelled","returning"].includes(r.status));
  const amount=wbOrderAmount;
  const amountMissing=activeOrders.filter(r=>!Number.isFinite(amount(r))||amount(r)===0).length;
  const orderAmount=activeOrders.reduce((s,r)=>s+(Number.isFinite(amount(r))?amount(r):0),0);
  const list=(section==="returns"?returns:orders).filter(r=>(!status||r.status===status)&&(section!=="returns"||!returnSource||r.raw._source===returnSource)&&`${r.id} ${r.raw.id||r.raw.saleID||""} ${r.raw.orderId||""} ${r.raw.srid||""} ${r.raw.article||r.raw.supplierArticle||""} ${r.raw.nmId||r.raw.nm_id||""} ${r.raw.imt_name||r.raw.subjectName||r.raw.subject||""}`.toLowerCase().includes(query.trim().toLowerCase()));
  const pages=Math.max(1,Math.ceil(list.length/50)),currentPage=Math.min(page,pages-1),shown=list.slice(currentPage*50,currentPage*50+50);
  const daily=useMemo(()=>{const ds:Record<string,{total:number;cancelled:number;amount:number;missing:number}>={};if(!from||!to||!Number.isFinite(Date.parse(from)))return [];for(let d=new Date(`${from}T12:00:00Z`);d.toISOString().slice(0,10)<=to&&Object.keys(ds).length<366;d.setUTCDate(d.getUTCDate()+1))ds[d.toISOString().slice(0,10)]={total:0,cancelled:0,amount:0,missing:0};for(const r of orders){if(ds[r.day]){ds[r.day].total++;if(["cancelled","returning"].includes(r.status))ds[r.day].cancelled++;else { const orderValue = amount(r); if (orderValue > 0) ds[r.day].amount += orderValue; else ds[r.day].missing++; }}}return Object.entries(ds)},[orders,from,to]);
  async function save(row:Row,local:Json,kind="return"){setSaving(true);setError("");try{await api("save_local",{shopId,id:row.id,kind,local});await refresh();setEdit(null)}catch(e){setError(String(e))}finally{setSaving(false)}}
  function exportRows(){const heading=section==="returns"?["退货编号","原订单号/SRID","来源","仓库","状态","日期","取件地址","取件截止日"]:["订单号","仓库","配送模式","状态","创建日期","货号","平台状态"];
    const rows=list.map(r=>section==="returns"?[r.raw.id||r.raw.saleID||r.raw.shkId,r.raw.orderId||r.raw.srid,sourceLabel(r.raw._source),whName(r.warehouseId),returnStages[r.status],r.day,r.local.address||r.raw.dstOfficeAddress,r.local.due]:[r.raw.id,whName(r.raw.warehouseId),r.raw._model,stages[r.status],r.day,r.raw.article,`${r.raw.supplierStatus||""}/${r.raw.wbStatus||""}`]);
    const escape=(v:unknown)=>`"${value(v).replace(/^[=+@-]/,"'$&").replaceAll('"','""')}"`;const blob=new Blob(["\ufeff"+[heading,...rows].map(r=>r.map(escape).join(",")).join("\r\n")],{type:"text/csv;charset=utf-8"});const url=URL.createObjectURL(blob);const a=document.createElement("a");a.href=url;a.download=`WB-${section}-${from}-${to}.csv`;a.click();setTimeout(()=>URL.revokeObjectURL(url),1000)}
  const openOrderDay = useCallback((day: string) => {
    setFrom(day);
    setTo(day);
    onSection("orders");
  }, [onSection]);
  return <>
    <section className="wbo-toolbar wbo-panel"><label>开始日期<input type="date" value={from} max={to} onChange={e=>setFrom(e.target.value)}/></label><label>结束日期<input type="date" value={to} min={from} max={today()} onChange={e=>setTo(e.target.value)}/></label><label>海外仓<select value={warehouse} onChange={e=>setWarehouse(e.target.value)}><option value="">全部仓库</option>{data.warehouses.map(w=><option key={w.id} value={value(w.raw.id)}>{w.raw.name} · {w.raw.id}</option>)}<option value="unknown">待关联仓库</option></select></label><label>配送模式<select value={model} disabled={section==="returns"} onChange={e=>setModel(e.target.value)}><option value="">FBS + DBS</option><option value="fbs">FBS</option><option value="dbs">DBS</option></select></label><button className="primary" disabled={running||!from||!to||to<from||(Date.parse(to)-Date.parse(from))/86400000>29} onClick={()=>void sync()}><RefreshCw size={16} className={running?"spin":""}/>{running?"正在同步…":"同步订单与退货"}</button><label className="wbo-check"><input type="checkbox" checked={auto} onChange={e=>setAuto(e.target.checked)}/>每小时自动同步</label><small>莫斯科日期 · 订单单次最多 30 天 · 退货独立回补近 90 天 · 自动同步在 WB 工作台打开时运行</small></section>
    {section === "daily" && <WbAdvertising currency={currency} shopId={shopId} from={from} to={to} orders={data.orders} busy={running} salesKeys={salesKeys} daily={daily} onDay={openOrderDay} onRange={days => { setFrom(before(days - 1)); setTo(today()); }} activeSeries={activeSeries}/>}
    {(section === "daily" || (section === "orders" && salesKeys.length > 0)) &&
      <section className="wbo-panel" id="wb-sales-filter">
        <div className="wbo-heading"><h2>商品销售筛选</h2><button disabled={!salesKeys.length}
          onClick={() => { setSalesKeys([]); setActiveSeries(""); setSeriesMessage(""); }}>恢复整店</button></div>
        {section === "daily" && <>
          <div className="wbo-toolbar">
            <label>系列<select value={activeSeries} onChange={event => {
              const name = event.target.value;
              setActiveSeries(name);
              setSalesKeys(series.find(item => item.name === name)?.keys ?? []);
              setSeriesMessage("");
            }}><option value="">选择已保存系列</option>
              {series.map(item => <option key={item.name} value={item.name}>{item.name} · {item.keys.length} 款</option>)}
            </select></label>
            <label>搜索商品<input placeholder="货号或 WB 商品 ID" value={salesQuery}
              onChange={event => setSalesQuery(event.target.value)} /></label>
            <label>新系列名称<input value={seriesName} maxLength={80} onChange={event => setSeriesName(event.target.value)} /></label>
            <button disabled={!salesKeys.length || !seriesName.trim()} onClick={saveSalesSeries}>保存为系列</button>
          </div>
          <details><summary>选择商品（可多选）</summary>
            <div style={{ maxHeight: 200, overflowY: "auto" }}>
              {products.filter(([, label]) => label.toLowerCase().includes(salesQuery.trim().toLowerCase())).map(([key, label]) =>
                <label className="wbo-check" key={key}><input type="checkbox" checked={salesKeys.includes(key)}
                  onChange={event => {
                    const checked = event.target.checked;
                    setSalesKeys(previous => checked ? [...previous, key] : previous.filter(item => item !== key));
                    setActiveSeries("");
                    setSeriesMessage("");
                  }} />{label}</label>)}
            </div>
            {!products.length && <p>同步订单后可选择商品。</p>}
          </details>
        </>}
        <p role="status">{activeSeries ? `系列「${activeSeries}」 · ` : ""}{salesKeys.length ? `已选 ${salesKeys.length} 款商品` : "全部商品"}{seriesMessage && ` · ${seriesMessage}`}</p>
      </section>}
    {error&&<div className="wbo-error" role="alert">{error}</div>}
    {section==="orders" && <WbReconciliation key={from+"/"+to} shopId={shopId} from={from} to={to} disabled={running}/>}
    {reminders.length>0&&<div className="wbo-reminder" role="status"><BellRing size={22}/><div><b>{reminders.length} 件退货需要安排取件或核实</b><p>{reminders.slice(0,3).map(r=>`${r.raw.orderId||r.raw.srid||r.raw.id||r.raw.shkId} · ${whName(r.warehouseId)}${(r.local.due||r.raw.expiredDt)?` · 截止 ${r.local.due||value(r.raw.expiredDt).slice(0,10)}`:""}`).join("；")}</p></div><button onClick={()=>{onSection("returns");setStatus("pickup");setAllReturns(true)}}>查看取件清单</button></div>}
    {section==="daily"?<> <section className="wbo-panel"><div className="wbo-heading"><h2>履约进度</h2><button onClick={()=>onSection("orders")}>进入订单中心 →</button></div><div className="wbo-stages">{Object.entries(stages).map(([s,label])=><button key={s} onClick={()=>{onSection("orders");setTimeout(()=>setStatus(s),0)}}><span className={`wbo-dot ${s}`}/><span>{label}</span><b>{orders.filter(r=>r.status===s).length}</b></button>)}</div></section>
      <div className="wbo-kpis">{[["周期订单量",orders.length,"平台订单明细",ShoppingBag],["扣除取消 / 拒收后订单额",money(orderAmount / (currency === "CNY" ? wbRubPerCny : 1), currency),amountMissing?`${amountMissing} 单金额或 RUB 币种缺失`:"订单额，非结算收入",Boxes],["待备货订单",orders.filter(r=>r.status==="awaiting_preparation").length,"按所选仓库安排备货",Truck],["取件 / 超期待核实",reminders.length,"跨日期待办，避免漏取",ArrowDownLeft]].map(([label,v,note,Icon])=>{const I=Icon as typeof Boxes;return <section className="wbo-panel wbo-kpi" key={String(label)}><span><I size={18}/>{String(label)}</span><strong>{String(v)}</strong><small>{String(note)}</small></section>})}</div>
      <section className="wbo-panel"><div className="wbo-heading"><h2>每日经营明细</h2><span>按订单创建日归属</span></div><div className="wbo-table-wrap"><table><thead><tr><th>日期</th><th>订单量</th><th>取消 / 拒收</th><th>扣除取消 / 拒收后订单量</th><th>扣除取消 / 拒收后订单额</th><th>查看</th></tr></thead><tbody>{[...daily].reverse().map(([day,d])=><tr key={day}><td>{day}</td><td>{d.total}</td><td>{d.cancelled}</td><td>{d.total-d.cancelled}</td><td>{money(d.amount / (currency === "CNY" ? wbRubPerCny : 1), currency)}</td><td><button onClick={()=>{setFrom(day);setTo(day);onSection("orders")}}>订单明细</button></td></tr>)}</tbody></table></div></section>
      <section className="wbo-panel"><div className="wbo-heading"><h2>海外仓协同</h2><span>{data.warehouses.length} 个平台仓库</span></div><div className="wbo-table-wrap"><table><thead><tr><th>仓库</th><th>订单量</th><th>等待备货</th><th>已经备货</th><th>配送中</th><th>等待签收</th><th>待取退货</th></tr></thead><tbody>{[...data.warehouses.map(w=>({id:value(w.raw.id),name:w.raw.name})),{id:"",name:"待关联仓库"}].map(w=>{const os=orders.filter(r=>value(r.raw.warehouseId)===w.id);return <tr key={w.id}><td><button onClick={()=>{setWarehouse(w.id||"unknown");onSection("orders")}}>{w.name}</button><small>{w.id}</small></td><td>{os.length}</td>{["awaiting_preparation","prepared","delivering","awaiting_receipt"].map(s=><td key={s}>{os.filter(r=>r.status===s).length}</td>)}<td>{reminders.filter(r=>value(r.warehouseId)===w.id).length}</td></tr>})}</tbody></table></div></section>
    </>:<section className="wbo-panel">
      <div className="wbo-heading"><div><h2>{section==="orders"?"每日订单明细":"退货与取件清单"}</h2><p>{section==="orders"?"平台状态自动更新；备货标记仅用于本地协同，不会向 WB 提交发货。":"买家退货、退回卖家按近90天独立回补，退货申请读取近14天；买家退货不代表实物已到可取件。仓库仅精确关联，未匹配时可手动指定。"}</p></div><button onClick={exportRows}><Download size={16}/>导出筛选结果</button></div>
      {section==="returns"&&<div className="wbo-source-summary">{["sales_return","item_return","claim"].map(source=><button key={source} className={returnSource===source?"selected":""} onClick={()=>{setReturnSource(returnSource===source?"":source);setStatus("")}}>{sourceLabel(source)} · {returns.filter(r=>r.raw._source===source).length} 条</button>)}<small>不同来源为不同业务记录，不直接相加为实物退货件数。</small></div>}
      <div className="wbo-tabs"><button className={!status?"selected":""} onClick={()=>setStatus("")}>全部 {section==="orders"?orders.length:returns.length}</button>{Object.entries(section==="orders"?stages:returnStages).map(([s,label])=><button className={status===s?"selected":""} key={s} onClick={()=>setStatus(s)}>{label} {(section==="orders"?orders:returns).filter(r=>r.status===s).length}</button>)}</div>
      <div className="wbo-list-tools"><div className="wbo-search"><Search size={16}/><input aria-label="搜索订单" placeholder="搜索订单号、退货编号、SRID、货号或商品" value={query} onChange={e=>setQuery(e.target.value)}/></div>{section==="returns"&&<label className="wbo-check"><input type="checkbox" checked={allReturns} onChange={e=>setAllReturns(e.target.checked)}/>查看全部已缓存退货（不限制日期）</label>}<span>{list.length} 条</span></div>
      <div className="wbo-table-wrap"><table><thead><tr><th>{section==="orders"?"订单 / 商品":"退货编号 / 原订单"}</th><th>所属仓库</th><th>{section==="orders"?"配送模式":"取件地点 / 截止日"}</th><th>状态</th><th>日期</th><th>操作</th></tr></thead><tbody>{shown.map(r=><tr key={r.id}><td><b>{section==="orders"?r.raw.id:r.raw.id||r.raw.saleID||r.raw.shkId}</b><small>{section==="orders"?`${r.raw.article||r.raw.supplierArticle||""} · 商品 ${r.raw.nmId||"—"}`:`原订单 ${r.raw.orderId||"未提供"} · SRID ${r.raw.srid||"未提供"}`}</small><small>{section==="returns"?sourceLabel(r.raw._source):""}</small><small>{section==="orders"?r.raw.rid:r.raw.imt_name||r.raw.subjectName||r.raw.subject}</small></td><td>{section==="returns"&&!value(r.warehouseId)&&r.raw.warehouseName?`${r.raw.warehouseName}（店铺仓待关联）`:whName(section==="orders"?r.raw.warehouseId:r.warehouseId)}<small>{value(section==="orders"?r.raw.warehouseId:r.warehouseId)||"平台未提供，待关联"}</small></td><td>{section==="orders"?value(r.raw._model).toUpperCase():<>{r.local.address||r.raw.dstOfficeAddress||"平台未提供地址"}<small className={(r.local.due||value(r.raw.expiredDt).slice(0,10))&&(r.local.due||value(r.raw.expiredDt).slice(0,10))<today()&&r.status!=="collected"?"wbo-overdue":""}>{(r.local.due||r.raw.expiredDt)?`取件截止 ${r.local.due||value(r.raw.expiredDt).slice(0,10)}`:"未设置取件截止日"}</small></>}</td><td><span className={`wbo-status ${r.status}`}>{(section==="orders"?stages:returnStages)[r.status]}</span><small>{section==="orders"?`${r.raw.supplierStatus||"未返回"} / ${r.raw.wbStatus||"未返回"}`:r.raw.returnStatus||(r.raw._source==="sales_return"?"销售记录确认买家退货；取件物流待核实":`申请状态 ${r.raw.status??"—"} / ${r.raw.status_ex??"—"}`)}</small></td><td>{r.day||"平台未提供"}<small>同步 {stamp(r.updatedAt)}</small></td><td>{section==="orders"?(["awaiting_preparation","prepared"].includes(r.status)?<button disabled={saving} onClick={()=>void save(r,{...r.local,prepared:!r.local.prepared},"order")}><CheckCheck size={15}/>{r.local.prepared?"撤销备货":"标记已备货"}</button>:"—"):<button onClick={()=>{setEdit(r);setDraft({...r.local})}}>取件 / 关联仓库</button>}</td></tr>)}{!shown.length&&<tr><td className="wbo-empty" colSpan={6}>{loading?"正在读取缓存…":running?"数据正在同步，完成的记录会自动显示":"暂无匹配记录，请调整筛选或同步数据。"}</td></tr>}</tbody></table></div>
      <div className="wbo-pagination"><span>每页 50 条</span><button disabled={currentPage===0} onClick={()=>setPage(currentPage-1)}>上一页</button>{currentPage+1} / {pages}<button disabled={currentPage>=pages-1} onClick={()=>setPage(currentPage+1)}>下一页</button></div>
    </section>}
    <details className="wbo-panel wbo-sync" open={running||undefined}><summary>同步状态 · {stamp(data.sync.updatedAt)}{running?" · 进行中":""}</summary><pre>{data.sync.message||"暂无同步记录。请选择日期后同步；每个数据源分别记录结果，失败时保留已有缓存。"}</pre><p>实物退货与买家退货回补近 90 天；退货申请接口仅提供近 14 天记录。接口返回0条只代表该来源当前查询无记录，不能代表店铺没有退货。取件提醒在本工作台内显示，需保持程序运行并及时同步。</p></details>
    {edit&&<div className="wbo-overlay"><form className="wbo-dialog" onSubmit={e=>{e.preventDefault();void save(edit,draft)}}><div className="wbo-heading"><h2>退货取件安排</h2><button aria-label="关闭" type="button" onClick={()=>setEdit(null)}><X size={18}/></button></div><b>退货 {edit.raw.id||edit.raw.shkId}</b><p>原订单 {edit.raw.orderId||edit.raw.srid||"平台未提供"}</p><label>关联海外仓<select value={value(draft.warehouseId??edit.warehouseId)} onChange={e=>setDraft({...draft,warehouseId:e.target.value})}><option value="">待关联仓库</option>{data.warehouses.map(w=><option key={w.id} value={value(w.raw.id)}>{w.raw.name} · {w.raw.id}</option>)}</select></label><label>取件地址<input value={draft.address??edit.raw.dstOfficeAddress??""} onChange={e=>setDraft({...draft,address:e.target.value})}/></label><label>取件截止日期<input type="date" value={draft.due??value(edit.raw.expiredDt).slice(0,10)} onChange={e=>setDraft({...draft,due:e.target.value})}/></label><label className="wbo-check"><input type="checkbox" checked={!!draft.ready} onChange={e=>setDraft({...draft,ready:e.target.checked})}/>已核实实物到达，加入取件提醒</label><label className="wbo-check"><input type="checkbox" checked={!!draft.collected} onChange={e=>setDraft({...draft,collected:e.target.checked,collectedAt:e.target.checked?new Date().toISOString():null})}/>我已取件</label><label>备注<textarea value={draft.note||""} onChange={e=>setDraft({...draft,note:e.target.value})}/></label><p>这些安排保存在本机，不会审批退款或改变 WB 平台订单状态。</p>{error&&<p className="wbo-error">{error}</p>}<button className="primary" disabled={saving}>{saving?"保存中…":"保存取件安排"}</button></form></div>}
  </>
}

function WbOrderTrend({ currency, daily, spend, onDay }: {
  currency: Currency;
  daily: WbDaily;
  spend: (number | null)[];
  onDay: (day: string) => void;
}) {
  const container = useRef<HTMLDivElement>(null);
  const selected = useRef(0);
  const chart = useRef<ReturnType<typeof initChart> | null>(null);

  useEffect(() => {
    const element = container.current;
    if (!element || !daily.length) return;
    const instance = initChart(element);
    chart.current = instance;
    selected.current = 0;
    const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    instance.setOption({
      animation: !reduceMotion,
      animationDuration: 1200,
      animationEasing: "cubicOut",
      grid: { left: 75, right: 135, top: 48, bottom: 36 },
      legend: { top: 5 },
      tooltip: { trigger: "axis" },
      xAxis: {
        type: "category",
        data: daily.map(([day]) => day),
        triggerEvent: true,
        axisLabel: { formatter: (day: string) => day.slice(5), hideOverlap: true, color: "#64748b" },
      },
      yAxis: [
        { type: "value", name: `销售额 / ${currencySymbol(currency)}`, min: 0, splitLine: { lineStyle: { color: "#e7eef8" } } },
        { type: "value", name: `广告 / ${currencySymbol(currency)}`, min: 0, splitLine: { show: false } },
        { type: "value", name: "销量 / 件", min: 0, minInterval: 1, offset: 65, splitLine: { show: false } },
      ],
      series: [
        { name: "销售额", type: "line", smooth: true, smoothMonotone: "x",
          data: daily.map(([, day]) => day.missing ? null : day.amount), itemStyle: { color: "#2469ff" },
          tooltip: { valueFormatter: (value: unknown) => typeof value === "number" ? money(value, currency) : "—" },
          lineStyle: { width: 3 }, areaStyle: { opacity: 0.12 } },
        { name: "销量", type: "line", smooth: true, smoothMonotone: "x", yAxisIndex: 2,
          data: daily.map(([, day]) => day.total - day.cancelled), itemStyle: { color: "#269d68" } },
        { name: "广告花费", type: "line", smooth: true, smoothMonotone: "x", yAxisIndex: 1,
          data: spend, tooltip: { valueFormatter: (value: unknown) => typeof value === "number" ? money(value, currency) : "—" }, itemStyle: { color: "#e98925" } },
      ],
    });
    instance.on("click", ({ dataIndex, value, componentType }) => {
      const day = componentType === "xAxis" ? String(value) : daily[dataIndex]?.[0];
      if (day) onDay(day);
    });
    const observer = new ResizeObserver(() => instance.resize());
    observer.observe(element);
    return () => {
      observer.disconnect();
      instance.dispose();
      chart.current = null;
    };
  }, [currency, daily, spend, onDay]);

  if (!daily.length) return <p className="wbo-muted">请选择有效的日期范围。</p>;

  return <div ref={container} className="wbo-order-curve" tabIndex={0}
    aria-label="业绩趋势，使用左右方向键选择日期，按 Enter 查看订单"
    onFocus={() => chart.current?.dispatchAction({ type: "showTip", seriesIndex: 0, dataIndex: selected.current })}
    onBlur={() => chart.current?.dispatchAction({ type: "hideTip" })}
    onKeyDown={event => {
      if (event.key === "Enter") {
        onDay(daily[selected.current][0]);
      } else if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
        event.preventDefault();
        selected.current = Math.min(daily.length - 1, Math.max(0, selected.current + (event.key === "ArrowLeft" ? -1 : 1)));
        chart.current?.dispatchAction({ type: "showTip", seriesIndex: 0, dataIndex: selected.current });
      }
    }}/>;
}

function WbPeriodReport({ shopId, period, currency }: {
  shopId: string;
  period: "weekly" | "monthly";
  currency: Currency;
}) {
  const [orders, setOrders] = useState<Row[]>([]);
  const [offset, setOffset] = useState(0);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");

  useEffect(() => {
    let active = true;
    api("list", { shopId }).then(snapshot => {
      if (active) setOrders(snapshot.orders);
    }).catch(reason => {
      if (active) setError(String(reason));
    }).finally(() => {
      if (active) setLoading(false);
    });
    return () => { active = false; };
  }, [shopId]);

  const current = today();
  const start = new Date(`${current}T12:00:00Z`);
  const end = new Date(start);
  const weekly = period === "weekly";
  if (weekly) {
    start.setUTCDate(start.getUTCDate() - (start.getUTCDay() + 6) % 7 - offset * 7);
    end.setTime(start.getTime());
    end.setUTCDate(end.getUTCDate() + 6);
  } else {
    start.setUTCMonth(start.getUTCMonth() - offset, 1);
    end.setTime(start.getTime());
    end.setUTCMonth(end.getUTCMonth() + 1, 0);
  }
  const dateKey = (date: Date) => date.toISOString().slice(0, 10);
  const from = dateKey(start);
  const to = offset ? dateKey(end) : current;
  const totals = { orders: 0, excluded: 0, sales: 0, missing: 0 };
  const days = new Map<string, typeof totals>();
  for (const row of orders) {
    if (row.day < from || row.day > to) continue;
    const day = days.get(row.day) ?? { orders: 0, excluded: 0, sales: 0, missing: 0 };
    day.orders++;
    totals.orders++;
    if (row.status === "cancelled" || row.status === "returning") {
      day.excluded++;
      totals.excluded++;
    } else {
      const value = wbOrderAmount(row);
      if (value > 0) {
        day.sales += value;
        totals.sales += value;
      } else {
        day.missing++;
        totals.missing++;
      }
    }
    days.set(row.day, day);
  }
  const formatSales = (row: typeof totals) => row.missing ? "—" :
    money(currency === "CNY" ? row.sales / wbRubPerCny : row.sales, currency);

  return <section className="wbo-panel">
    <div className="wbo-heading">
      <div><h2>{weekly ? "周度经营汇总" : "月度经营汇总"}</h2><span>{from} 至 {to}</span></div>
      <div className="wbo-actions">
        <button onClick={() => setOffset(value => value + 1)}>{weekly ? "上一周" : "上一月"}</button>
        <button disabled={!offset} onClick={() => setOffset(value => value - 1)}>{weekly ? "下一周" : "下一月"}</button>
        <button disabled={!offset} onClick={() => setOffset(0)}>{weekly ? "本周" : "本月"}</button>
      </div>
    </div>
    <p className="wbo-muted">数据来自本机已同步订单。{currency === "CNY" ? "人民币金额按 ¥1 = ₽12.5 估算。" : ""}</p>
    {error && <p className="wbo-error" role="alert">{error}</p>}
    {loading ? <p role="status">正在读取已同步订单…</p> : !error && <>
      <WbDashboardMetrics metrics={[
        ["订单量", totals.orders], ["有效订单", totals.orders - totals.excluded],
        ["取消 / 拒收", totals.excluded], ["销售额", days.size ? formatSales(totals) : "—"],
      ]}/>
      {days.size ? <div className="wbo-table-wrap"><table>
        <thead><tr><th>日期</th><th>订单量</th><th>有效订单</th><th>取消 / 拒收</th><th>销售额</th></tr></thead>
        <tbody>{[...days.keys()].sort().reverse().map(day => {
          const row = days.get(day)!;
          return <tr key={day}><td>{day}</td><td>{row.orders}</td><td>{row.orders - row.excluded}</td>
            <td>{row.excluded}</td><td>{formatSales(row)}</td></tr>;
        })}</tbody>
      </table></div> : <p>该期间暂无已同步订单。</p>}
    </>}
  </section>;
}
