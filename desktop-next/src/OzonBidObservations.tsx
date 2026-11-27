import { useEffect, useRef, useState } from "react";
import { init } from "./charts";

type Observation = { sku: string; price: number; bid: number; at: string; cpcDay?: string };

export function OzonBidObservations({shopId,campaignId,cpcDay,skuCpc,products}: {shopId:string;campaignId:string;cpcDay?:string;skuCpc?:Record<string,number>;products?:{sku:string;offerId?:string;name?:string}[]}) {
  const key = `ozon-bid-observations:${shopId}:${campaignId}`;
  const [rows,setRows]=useState<Observation[]>(()=>{
    try { const saved=JSON.parse(localStorage.getItem(key) ?? "[]"); return Array.isArray(saved) ? saved : []; }
    catch { return []; }
  });
  const [sku,setSku]=useState("");
  const [manualBid,setManualBid]=useState<string | null>(null);
  const observedCpc = Object.hasOwn(skuCpc ?? {}, sku.trim()) ? skuCpc?.[sku.trim()] : undefined;
  const bidValue = manualBid ?? (observedCpc === undefined ? "" : observedCpc.toFixed(4));
  const [shownSku,setShownSku]=useState("");
  const [error,setError]=useState("");
  const host=useRef<HTMLDivElement>(null);
  const activeSku=shownSku || rows.at(-1)?.sku;
  const points=rows.filter(row=>row.sku===activeSku);
  const current=points.at(-1), prior=points.at(-2);
  useEffect(()=>{
    if (!host.current || !points.length) return;
    const element=host.current;
    const chart=init(element);
    chart.setOption({
      animation:false,
      grid:{left:65,right:58,top:45,bottom:52},
      tooltip:{trigger:"axis"},
      legend:{top:0},
      xAxis:{type:"category", data:points.map(row=>new Date(row.at).toLocaleString("zh-CN",{timeZone:"Asia/Shanghai"})),axisLabel:{hideOverlap:true}},
      yAxis:[{type:"value",name:"售价 ₽",scale:true},{type:"value",name:"出价 ₽",scale:true,splitLine:{show:false}}],
      series:[
        {name:"商品售价",type:"line",data:points.map(row=>row.price)},
        {name:"观察值（CPC / 手填）",type:"line",yAxisIndex:1,data:points.map(row=>row.bid)},
      ],
    });
    const observer=new ResizeObserver(()=>chart.resize());
    observer.observe(element);
    return ()=>{observer.disconnect();chart.dispose();};
  },[rows,activeSku]);
  return <section className="ozon-campaign-products ozon-bid-card">
    <h3>SKU 售价与出价记录</h3>
    <p>输入 SKU 后自动带入该活动昨天的实际平均 CPC（花费 ÷ 点击），也可手填覆盖。金额使用卢布，记录保存在本机。</p>
    <form className="campaign-batch-actions" onSubmit={event=>{
      event.preventDefault();
      const data=new FormData(event.currentTarget);
      const price=Number(data.get("price")),bid=Number(data.get("bid")),code=sku.trim();
      if(!code||!Number.isFinite(price)||!Number.isFinite(bid)||price<=0||bid<0) return;
      const next=[...rows,{sku:code,price,bid,at:new Date().toISOString(),cpcDay:manualBid===null&&observedCpc!==undefined?cpcDay:undefined}];
      try {localStorage.setItem(key,JSON.stringify(next));setRows(next);setShownSku(code);setError("");}
      catch {setError("无法保存本机记录");}
    }}>
      <select required aria-label="商品 SKU" value={sku} onChange={e=>{setSku(e.target.value);setManualBid(null);}}><option value="">选择活动商品 SKU</option>{products?.map(item=><option key={item.sku} value={item.sku}>{[item.sku,item.offerId,item.name].filter(Boolean).join(" · ")}</option>)}</select>
      <input required name="price" type="number" step="0.01" min="0.01" aria-label="商品售价" placeholder="售价 ₽" />
      <input required name="bid" type="number" step="any" min="0" aria-label="昨日平均 CPC 或手填出价" placeholder="CPC / 出价 ₽" value={bidValue} onChange={e=>setManualBid(e.target.value)} />
      <button>保存观察</button>
    </form>
    {sku.trim() && <p role="status">{manualBid!==null ? "当前为手填值" : observedCpc!==undefined ? ("已带入 " + cpcDay + " 平均 CPC") : "昨日暂无该 SKU 的点击计费数据，可手填或同步广告后重新打开活动。"}</p>}
    {error && <p role="alert">{error}</p>}
    {activeSku && <>
      <label>对比商品<select value={activeSku} onChange={e=>setShownSku(e.target.value)}>
        {[...new Set(rows.map(row=>row.sku))].map(item=><option key={item}>{item}</option>)}
      </select></label>
      {current&&prior&&<p>最近两次：售价 ₽{prior.price} → ₽{current.price}，观察值 ₽{prior.bid}（{prior.cpcDay ? prior.cpcDay + " CPC" : "手填"}） → ₽{current.bid}（{current.cpcDay ? current.cpcDay + " CPC" : "手填"}）
        {prior.bid>0 && `（${((current.bid/prior.bid-1)*100).toFixed(2)}%）`}。</p>}
      <div ref={host} style={{height:240}} aria-label="SKU 售价与观察出价趋势"/>
    </>}
  </section>;
}
