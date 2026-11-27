import { readWbData, wbProductCostKey, wbWarehouseCostKey, type WbProductCost, type WbWarehouseCost } from "./wbProductCosts";
﻿import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type Row = Record<string, unknown>;
type Summary = { reports: Row[]; previous: Row[] | null; updatedAt: string };
const displayValue = (value:unknown) => value == null ? "—" : typeof value === "object" ? JSON.stringify(value) : String(value);
const reportId = (row:Row) => String(row.reportId);
type Product = { id:string; sku:string; warehouseKey:string; knownWarehouse:boolean; name:string; warehouse:string; currency:string; sales:number; pay:number; delivery:number; returns:number; signed:number; quantityMissing:boolean; incomplete:boolean; orderKeys:Set<string>; orderMissing:boolean };
const fields = ["商品成本","头程运费","海外仓操作费","跨境运费"];
const numeric = (value: unknown) => {
  if ((typeof value !== "number" && typeof value !== "string") || String(value).trim() === "") return null;
  const result = Number(value);
  return Number.isFinite(result) ? result : null;
};
const normalizeCurrency = (value: unknown) => String(value ?? "").trim().toUpperCase();
const format = (value:number | null, currency = "") => value === null ? "待确认" :
  `${value.toLocaleString("zh-CN",{minimumFractionDigits:2,maximumFractionDigits:2})} ${currency}`.trim();
const summaryFields: [string,string][] = [
  ["报告编号","reportId"],["法人实体","sellerFinanceName"],["开始日期","dateFrom"],["结束日期","dateTo"],
  ["生成日期","createDate"],["报告类型","reportType"],["销售","retailAmountSum"],["应付商品结算金额","forPaySum"],
  ["协商折扣 %","avgSalePercent"],["物流费用","deliveryServiceSum"],["仓储费用","paidStorageSum"],
  ["商品处理费用","paidAcceptanceSum"],["其他扣款 / 付款","deductionSum"],["罚款总额","penaltySum"],
  ["WB佣金调整 / 补款","additionalPaymentSum"],["忠诚度折扣补偿","cashbackDiscountSum"],
  ["忠诚度参与费用","cashbackCommissionChangeSum"],["扣除积分总额","cashbackAmountSum"],
  ["应付总额","bankPaymentSum"],["币种","currency"],
];
const call = <T,>(shopId:string,action:string,payload:object={}) =>
  invoke<T>("wb_operations",{action,payload:{shopId,...payload}});

export function WbWeeklyFinance({shopId,adCurrency,from,to,headers,rows}:{
  shopId:string;adCurrency:"CNY"|"RUB";from:string;to:string;headers:string[];rows:string[][];
}) {
  const [summary,setSummary]=useState<Summary>();
  const [loadingSummary,setLoadingSummary]=useState(true);
  const summaries=summary?.reports ?? [];
  const changes=useMemo(()=>{
    if (!summary?.previous) return [];
    const before=new Map(summary.previous.map(row=>[reportId(row),row]));
    const after=new Map(summary.reports.map(row=>[reportId(row),row]));
    return [...new Set([...before.keys(),...after.keys()])].flatMap(id=>{
      const old=before.get(id), current=after.get(id);
      return [...new Set([...Object.keys(old ?? {}),...Object.keys(current ?? {})])].flatMap(field=>{
        const previous=displayValue(old?.[field]), next=displayValue(current?.[field]);
        return previous===next ? [] : [{id,field,previous,next,kind:old ? current ? "更新" : "移除" : "新增"}];
      });
    });
  },[summary]);
  const [ads,setAds]=useState<{day:string;spend:number}[]>([]);
  const [snapshot,setSnapshot]=useState<{orders:{raw:Row}[];warehouses:{raw:Row}[]}>({orders:[],warehouses:[]});
  const [pending,setPending]=useState("");
  const [errors,setErrors]=useState<Record<string,string>>({});
  const key=`wb-report-cost-v1:${shopId}:${from}`;
  const productCosts=readWbData<WbProductCost>(wbProductCostKey(shopId));
  const warehouseKey=wbWarehouseCostKey(shopId);
  const [warehouseCosts,setWarehouseCosts]=useState(()=>readWbData<WbWarehouseCost>(warehouseKey));
  const [included,setIncluded]=useState(()=>localStorage.getItem(key+":ad") ?? "");

  async function refresh(kind:"summary"|"ads", sync=false) {
    setPending(kind);
    setErrors(previous=>({...previous,[kind]:""}));
    try {
      if (kind==="summary") setSummary(await call<Summary>(shopId,"weekly_report_summary",{from,refresh:true}));
      else setAds(await call(shopId,sync ? "ads_sync" : "ads_list",{from,to}));
    } catch (error) {
      setErrors(previous=>({...previous,[kind]:String(error)}));
    } finally {setPending("");}
  }
  useEffect(()=>{
    let active=true;
    call<Summary>(shopId,"weekly_report_summary",{from}).then(data=>{if(active)setSummary(data);})
      .catch(error=>{if(active)setErrors(old=>({...old,summary:String(error)}));})
      .finally(()=>{if(active)setLoadingSummary(false);});
    call<typeof ads>(shopId,"ads_list").then(data=>{if(active)setAds(data);})
      .catch(error=>{if(active)setErrors(old=>({...old,ads:String(error)}));});
    call<typeof snapshot>(shopId,"list").then(data=>{if(active)setSnapshot(data);})
      .catch(error=>{if(active)setErrors(old=>({...old,warehouse:String(error)}));});
    return ()=>{active=false;};
  },[shopId,from,to]);

  const {products,unknown}=useMemo(()=>{
    const warehouses=new Map(snapshot.warehouses.map(row=>[String(row.raw.id ?? ""),String(row.raw.name ?? "")]));
    const orderWarehouses=new Map<string,Set<string>>();
    for(const {raw} of snapshot.orders) {
      const rid=String(raw.rid ?? raw.srid ?? ""), id=String(raw.warehouseId ?? "");
      if(!rid||!id) continue;
      const matches=orderWarehouses.get(rid) ?? new Set<string>();
      matches.add(id);
      orderWarehouses.set(rid,matches);
    }
    const groups=new Map<string,Product>();
    let unknown=0;
    for(const values of rows) {
      const row=Object.fromEntries(headers.map((field,index)=>[field,values[index]]));
      const sku=row.nmId?.trim(), currency=normalizeCurrency(row.currency) || "币种未知";
      const matches=orderWarehouses.get(row.srid);
      const warehouseId=matches?.size===1?[...matches][0]:"";
      const warehouse=warehouseId?`${warehouses.get(warehouseId) || "卖家仓"} · ${warehouseId}`:
        row.officeName?`WB 报告仓：${row.officeName}`:"发货仓待确认";
      const operation=row.sellerOperName || row.docTypeName || "";
      const sign=/^(Продажа|销售|sale)$/i.test(operation)?1:/^(Возврат|退货|退款|return)$/i.test(operation)?-1:0;
      const shipping=numeric(row.deliveryService);
      if(!sku || sku==="0") continue;
      const id=JSON.stringify([sku,warehouse,currency]);
      const item=groups.get(id) ?? {id,sku,warehouseKey:warehouseId ? "id:"+warehouseId : "name:"+row.officeName,knownWarehouse:!!warehouseId || !!row.officeName,name:[row.vendorCode,row.title].filter(Boolean).join(" · ") || sku,warehouse,currency,
        sales:0,pay:0,delivery:0,returns:0,signed:0,quantityMissing:false,incomplete:false,orderKeys:new Set<string>(),orderMissing:false};
      if(!warehouseId && sign) unknown++;
      item.delivery+=shipping ?? 0;
      if(sign) {
        const qty=numeric(row.quantity), retail=numeric(row.retailAmount), pay=numeric(row.forPay);
        if(qty===null) item.quantityMissing=true;
        if(qty===null || retail===null || pay===null) item.incomplete=true;
        item.sales+=sign*Math.abs(retail ?? 0);
        item.pay+=sign*Math.abs(pay ?? 0);
        if(sign>0) {
          item.signed+=Math.abs(qty ?? 0);
          const orderKey=[row.orderId,row.srid,row.rid].map(value=>value?.trim()).find(value=>value && value!=="0");
          if(orderKey) item.orderKeys.add(orderKey);
          else item.orderMissing=true;
        } else item.returns+=Math.abs(qty ?? 0);
      }
      groups.set(id,item);
    }
    return {products:[...groups.values()],unknown};
  },[headers,rows,snapshot]);

  const weekAds=ads.filter(row=>row.day>=from&&row.day<=to);
  const adSpend=new Set(weekAds.map(row=>row.day)).size===7?weekAds.reduce((sum,row)=>sum+row.spend,0):null;
  const currencies=[...new Set(summaries.map(row=>normalizeCurrency(row.currency)))];
  const currency=currencies.length===1?currencies[0]:"";
  const payout=summaries.length && currency && summaries.every(row=>numeric(row.bankPaymentSum)!==null)
    ? summaries.reduce((sum,row)=>sum+Number(row.bankPaymentSum),0):null;
  function expenseLines(row:Product) {
    const rule=row.knownWarehouse ? warehouseCosts[row.warehouseKey] : undefined;
    const product=productCosts[row.sku];
    const rate=row.currency==="CNY"?1:row.currency==="RUB"?12.5:null;
    const line=(value:string|undefined,quantity:number)=>{
      if(quantity===0)return 0;
      const unit=numeric(value);
      return unit===null||unit<0||rate===null?null:unit*quantity*rate;
    };
    if (row.quantityMissing) return [null,null,null,0];
    if (!row.signed && !row.returns) return [0,0,0,0];
    const purchase=line(product?.purchase,Math.max(0,row.signed-row.returns));
    if (!rule && row.signed) return [purchase,null,null,0];
    if (!rule) return [purchase,0,0,0];
    return rule.kind==="domestic" ? [purchase,0,0,0] :
      [purchase,line(product?.firstMile,row.signed),line(rule.handling,row.signed),0];
  }
  function expenses(row:Product) {
    const entries=expenseLines(row);
    return entries.every(value=>value!==null)?entries.reduce<number>((sum,value)=>sum+value!,0):null;
  }
  const complete=products.length>0 && products.every(row=>row.currency===currency && expenses(row)!==null);
  const costs=products.map(expenseLines);
  const costTotal=(index:number)=>products.length && currency && products.every((row,i)=>row.currency===currency && costs[i][index]!==null)
    ? costs.reduce((sum,line)=>sum+line[index]!,0):null;
  const purchaseTotal=costTotal(0), firstMileTotal=costTotal(1), handlingTotal=costTotal(2);
  const channel=(row:Product)=>row.knownWarehouse ? warehouseCosts[row.warehouseKey]?.kind : undefined;
  const relevant=(row:Product)=>row.signed!==0 || row.returns!==0 || row.delivery!==0 || row.incomplete || row.orderKeys.size>0;
  const classified=products.length>0 && products.every(row=>!relevant(row) || (channel(row) && row.currency===currency));
  const overseas=products.filter(row=>channel(row)==="overseas");
  const domestic=products.filter(row=>channel(row)==="domestic");
  const overseasDelivery=classified?overseas.reduce((sum,row)=>sum+row.delivery,0):null;
  const groupTotals=(group:Product[])=>{
    const valid=classified && group.every(row=>!row.incomplete && expenses(row)!==null);
    const cost=valid?group.reduce((sum,row)=>sum+expenses(row)!+row.delivery,0):null;
    const gross=valid?group.reduce((sum,row)=>sum+row.pay-row.delivery-expenses(row)!,0):null;
    return {cost,gross,orders:new Set(group.flatMap(row=>[...row.orderKeys]))};
  };
  const overseasTotals=groupTotals(overseas), domesticTotals=groupTotals(domestic);
  const orderCount=overseasTotals.orders.size+domesticTotals.orders.size;
  const canAllocate=classified && orderCount>0 && products.every(row=>!row.orderMissing) &&
    [...overseasTotals.orders].every(id=>!domesticTotals.orders.has(id)) && currency===adCurrency && adSpend!==null;
  const overseasAds=canAllocate?Math.round(adSpend!*overseasTotals.orders.size/orderCount*100)/100:null;
  const domesticAds=overseasAds===null?null:adSpend!-overseasAds;
  const overseasNet=overseasTotals.gross!==null&&overseasAds!==null?overseasTotals.gross-overseasAds:null;
  const domesticNet=domesticTotals.gross!==null&&domesticAds!==null?domesticTotals.gross-domesticAds:null;
  const linkedCost=complete?products.reduce((sum,row)=>sum+expenses(row)!,0):null;
  const invested=linkedCost!==null&&overseasDelivery!==null?linkedCost+overseasDelivery:null;
  const sales=summaries.length && currency && summaries.every(row=>numeric(row.retailAmountSum)!==null)
    ? summaries.reduce((sum,row)=>sum+Number(row.retailAmountSum),0):null;
  const profit=payout!==null&&linkedCost!==null?payout-linkedCost:null;
  const deducted=numeric(included);
  const net=profit!==null && currency===adCurrency && adSpend!==null && deducted!==null && deducted>=0 && deducted<=adSpend
    ? profit-(adSpend-deducted):null;

  const ratio=(value:number|null,base:number|null)=>value!==null && base!==null && base>0
    ? new Intl.NumberFormat("zh-CN",{style:"percent",maximumFractionDigits:2}).format(value/base):"待确认";

  function setWarehouseRule(id:string, rule:WbWarehouseCost) {
    const next={...warehouseCosts,[id]:rule};
    try {localStorage.setItem(warehouseKey,JSON.stringify(next));setWarehouseCosts(next);}
    catch {setErrors(old=>({...old,local:"仓库规则无法保存"}));}
  }

  return <div className="wb-finance-report">
    <section className="wbo-panel">
      <div className="wbo-heading"><h2>WB 官方汇总核对</h2><button disabled={loadingSummary || !!pending} onClick={()=>void refresh("summary")}>{pending==="summary" ? "正在更新…" : "更新官方汇总"}</button></div>
      {loadingSummary ? <p role="status">正在读取官方汇总…</p> : !summaries.length && <p>暂无已读取的官方汇总。</p>}
      {summary && summaries.length>0 && <p className="wbo-muted">上次成功读取 {new Date(summary.updatedAt).toLocaleString("zh-CN",{timeZone:"Asia/Shanghai"})}（中国时间）</p>}
      {summaries.map((row,index)=><dl className="wb-finance-kpis" key={String(row.reportId ?? index)}>
        {summaryFields.map(([label,field])=><div key={field}><dt>{label}</dt><dd>{String(row[field] ?? "—")}</dd></div>)}
      </dl>)}
      {summary?.previous && <div>
        <h3>本次更新变动明细</h3>
        {changes.length ? <div className="wbo-table-wrap wbo-weekly-grid"><table>
          <thead><tr><th>报告编号</th><th>类型</th><th>字段</th><th>更新前</th><th>更新后</th></tr></thead>
          <tbody>{changes.map(change=><tr key={JSON.stringify([change.id,change.field])}><td>{change.id}</td><td>{change.kind}</td>
            <td>{summaryFields.find(([,field])=>field===change.field)?.[0] ?? change.field}</td><td>{change.previous}</td><td>{change.next}</td></tr>)}</tbody>
        </table></div> : <p>与上次读取相比，官方汇总无变动。</p>}
      </div>}
    </section>
    <section className="wbo-panel">
      <div className="wbo-heading"><h2>同周广告与利润</h2><button disabled={!!pending} onClick={()=>void refresh("ads",true)}>同步同周广告</button></div>
      <dl className="wb-finance-kpis">
        <div><dt>官方应付总额</dt><dd>{format(payout,currency)}</dd></div>
        <div><dt>扣联动成本后利润</dt><dd>{format(profit,currency)}</dd></div>
        <div><dt>整体广告费 · {weekAds.length}/7 天</dt><dd>{format(adSpend,adCurrency)}</dd></div>
        <div><dt>扣广告后净利润</dt><dd>{format(net,currency)}</dd></div>
      </dl>
      <label>已计入 WB 扣款的同周广告费（确认后填写，未扣填 0） <input type="number" min="0" step="0.01" value={included} onChange={event=>{
        const value=event.target.value;
        try {localStorage.setItem(key+":ad",value);setIncluded(value);} catch {setErrors(old=>({...old,local:"广告扣款金额无法保存"}));}
      }}/></label>
      <p className="wbo-muted">净利润 = 官方应付总额 − 联动成本 − 尚未计入 WB 扣款的广告费。广告按店铺币种显示，和官方报告币种不一致时净利润待确认。</p>
    </section>
    <section className="wbo-panel">
      <div className="wbo-heading"><h2>实际投入与回报</h2></div>
      <dl className="wb-finance-kpis">
        <div><dt>总实际投入</dt><dd>{format(invested,currency)}</dd></div>
        <div><dt>商品成本</dt><dd>{format(purchaseTotal,currency)}</dd></div>
        <div><dt>头程运费</dt><dd>{format(firstMileTotal,currency)}</dd></div>
        <div><dt>海外仓操作费</dt><dd>{format(handlingTotal,currency)}</dd></div>
        <div><dt>海外仓 WB 运费</dt><dd>{format(overseasDelivery,currency)}</dd></div>
        <div><dt>海外仓直发 ROI</dt><dd>{ratio(overseasNet,overseasTotals.cost)}</dd></div>
        <div><dt>净利润率</dt><dd>{ratio(net,sales)}</dd></div>
        <div><dt>结算回款率</dt><dd>{ratio(payout,sales)}</dd></div>
      </dl>
      <p>海外仓直发 ROI = 海外仓直发净利润 ÷ 海外仓实际投入。净利润率 = 净利润 ÷ 官方销售额。结算回款率 = 官方应付总额 ÷ 官方销售额。</p>
      <p>总实际投入包含商品成本、海外仓头程、操作费和 WB 运费。WB 运费已反映在官方应付总额中，国内直发运费已由财报扣除，总实际投入排除这部分运费。结算回款率以 WB 应付金额为准。</p>
    </section>
    <section className="wbo-panel">
      <div className="wbo-heading"><h2>按发货渠道核算利润</h2></div>
      <div className="wbo-table-wrap"><table>
        <thead><tr><th>发货渠道</th><th>结算销售订单</th><th>毛利润</th><th>分摊广告费</th><th>净利润</th></tr></thead>
        <tbody>{[
          {name:"海外仓直发",totals:overseasTotals,ads:overseasAds,net:overseasNet},
          {name:"国内直发",totals:domesticTotals,ads:domesticAds,net:domesticNet},
        ].map(row=><tr key={row.name}><td>{row.name}</td><td>{classified && products.every(item=>!item.orderMissing)?row.totals.orders.size:"待确认"}</td>
          <td>{format(row.totals.gross,currency)}</td><td>{format(row.ads,currency)}</td><td>{format(row.net,currency)}</td></tr>)}</tbody>
      </table></div>
      <dl className="wb-finance-kpis">
        <div><dt>海外仓实际投入</dt><dd>{format(overseasTotals.cost,currency)}</dd></div>
        <div><dt>整周净利润与渠道净利润合计的差额</dt><dd>{format(net!==null&&overseasNet!==null&&domesticNet!==null?net-overseasNet-domesticNet:null,currency)}</dd></div>
      </dl>
      <p>渠道毛利润 = 商品应付金额 − 财报物流费 − 联动成本。广告费按同周结算销售订单比例分摊，优先按订单号去重，缺少订单号时使用 SRID 或 RID。两渠道订单重叠或归属不完整时，广告分摊待确认。</p>
      <p>渠道净利润 = 渠道毛利润 − 分摊广告费。整周净利润另以官方应付总额为准，差额用于核对未归到渠道的扣款、补款及广告扣款。</p>
    </section>
    {Object.entries(errors).filter(([,message])=>message).map(([kind,message])=><p key={kind} role="alert" className="wbo-error">{message}</p>)}
    <section className="wbo-panel">
      <div className="wbo-heading"><h2>商品 × 发货仓</h2><span>{products.length} 组</span></div>
      <p className="wbo-muted">按 SRID 匹配已同步卖家订单的发货仓。{unknown} 条销售或退货明细未匹配到具体卖家仓。报告仓为 WB 原始字段。已保存的商品成本独立显示。请确认仓库类型以计算仓配费用。商品成本按签收减退货的非负净件数计算。海外仓头程和操作费按签收件数计算。国内直发运费由财报扣除。卢布报告按 ¥1 = ₽12.5 换算成本。</p>
      <div className="wbo-table-wrap wbo-weekly-grid"><table><thead><tr>
        {["商品","发货仓","仓库规则","销售额","WB 运费","退货件数","签收件数","商品应付金额",...fields,"商品利润（扣广告前）"].map(field=><th key={field}>{field}</th>)}
      </tr></thead><tbody>{products.map(row=>{
        const cost=expenses(row);
        return <tr key={row.id}><td>{row.name}</td><td>{row.warehouse}</td><td>{row.knownWarehouse ? <><select aria-label={row.warehouse+" 发货类型"} value={warehouseCosts[row.warehouseKey]?.kind ?? ""} onChange={event=>{const kind=event.target.value;if(kind==="domestic"||kind==="overseas")setWarehouseRule(row.warehouseKey,{kind,handling:warehouseCosts[row.warehouseKey]?.handling ?? ""});}}><option value="" disabled>选择类型</option><option value="domestic">国内直发</option><option value="overseas">海外仓</option></select>{warehouseCosts[row.warehouseKey]?.kind==="overseas"&&<input aria-label={row.warehouse+" 单件操作费 CNY"} placeholder="操作费 CNY/件" type="number" min="0" step="0.01" value={warehouseCosts[row.warehouseKey].handling} onChange={event=>setWarehouseRule(row.warehouseKey,{kind:"overseas",handling:event.target.value})}/>}</> : "待关联"}</td>
          <td>{format(row.incomplete?null:row.sales,row.currency)}</td><td>{format(row.delivery,row.currency)}</td>
          <td>{row.returns}</td><td>{row.signed}</td><td>{format(row.incomplete?null:row.pay,row.currency)}</td>
          {expenseLines(row).map((value,index)=><td key={fields[index]}>{format(value,row.currency)}</td>)}
          <td>{format(cost===null||row.incomplete?null:row.pay-row.delivery-cost,row.currency)}</td>
        </tr>;
      })}</tbody></table></div>
    </section>
  </div>;
}
