import type { ProductAnalysisRow, ProductDetail } from "./types";

export type DecisionEvent={day:string;type:"price"|"stockout"|"promotion"|"budget";label:string;before:number|null;after:number|null;source:string};
export type DecisionProfile={
  sku:string;lifecycle:string;trend:string;activeDays:number;expectedUnits:number|null;actualUnits:number;deltaPercent:number|null;sustainedDays:number;
  confidence:number;events:DecisionEvent[];attribution:string;inventoryMatch:number|null;lsi:number|null;lsiConfidence:number;lsiMissing:string[];
  marginalCpa:number|null;marginalRoas:number|null;marginalCvr:number|null;adElasticity:number|null;experimentDecision:string;
  nextGate:string;stopConditions:string[];vetoes:string[];
};

const clamp=(v:number)=>Math.max(0,Math.min(100,v));
const ratio=(a:number,b:number)=>b>0?a/b:null;
const pct=(now:number,before:number)=>before>0?(now-before)/before:null;
const sum=(rows:ProductAnalysisRow[])=>rows.reduce((a,r)=>({units:a.units+r.totalUnits,orders:a.orders+r.adOrders,clicks:a.clicks+r.clicks,spend:a.spend+r.adSpend,adRevenue:a.adRevenue+r.adRevenue}),{units:0,orders:0,clicks:0,spend:0,adRevenue:0});

function weekdayExpectation(history:ProductAnalysisRow[],target:ProductAnalysisRow[]){
  const byDay=new Map(history.map(r=>[r.day,r]));
  let expected=0,covered=0;
  for(const row of target){const date=new Date(`${row.day}T00:00:00`),values:number[]=[];for(let w=1;w<=4;w++){const d=new Date(date);d.setDate(d.getDate()-7*w);const prior=byDay.get(d.toISOString().slice(0,10));if(prior)values.push(prior.totalUnits);}if(values.length){const weights=[.4,.3,.2,.1].slice(0,values.length),weight=weights.reduce((a,b)=>a+b,0);expected+=values.reduce((s,v,i)=>s+v*weights[i],0)/weight;covered++;}}
  return covered===target.length&&covered>0?expected:null;
}

function detectEvents(rows:ProductAnalysisRow[],detail?:ProductDetail):DecisionEvent[]{
  const events:DecisionEvent[]=[];
  for(let i=1;i<rows.length;i++){const before=rows[i-1],after=rows[i];
    if(before.adSpend>0&&Math.abs(after.adSpend-before.adSpend)/before.adSpend>=.2)events.push({day:after.day,type:"budget",label:`广告费${after.adSpend>before.adSpend?"上升":"下降"} ${Math.abs((after.adSpend-before.adSpend)/before.adSpend*100).toFixed(0)}%`,before:before.adSpend,after:after.adSpend,source:"按日广告费变化推断，需结合预算日志确认"});
  }
  for(const log of detail?.priceLogs||[]){const day=log.createdAt.slice(0,10);if(!events.some(e=>e.day===day&&e.type==="price"))events.push({day,type:log.requestedPrice<log.beforePrice?"promotion":"price",label:`价格操作 ${log.beforePrice} → ${log.requestedPrice}（${log.status}）`,before:log.beforePrice,after:log.verifiedPrice??log.requestedPrice,source:"产品改价审计日志"});}
  const latest=rows.at(-1);if(latest?.stock===0)events.push({day:latest.day,type:"stockout",label:"期末库存为 0",before:null,after:0,source:"当前库存快照；缺少历史库存，不能确定断货起始日"});
  return events.sort((a,b)=>a.day.localeCompare(b.day));
}

function localization(detail?:ProductDetail){
  if(!detail?.clusters.length)return{match:null,lsi:null,confidence:0,missing:["集群订单分布","集群库存分布","本地配送率","配送时效","集群可售率"]};
  const totalOrders=detail.clusters.reduce((s,c)=>s+c.orders,0),totalStockWeight=detail.clusters.reduce((s,c)=>s+c.configuredWeight,0);
  if(totalOrders<=0||totalStockWeight<=0)return{match:null,lsi:null,confidence:0,missing:["有效集群订单或库存配置"]};
  const mismatch=detail.clusters.reduce((s,c)=>s+Math.abs(c.orders/totalOrders-c.configuredWeight/totalStockWeight),0),match=clamp((1-.5*mismatch)*100);
  return{match,lsi:match,confidence:35,missing:["本地配送率","配送时效","集群可售率"]};
}

export function buildDecisionProfiles(historySets:ProductAnalysisRow[][],current:ProductAnalysisRow[],previous:ProductAnalysisRow[],details:Map<string,ProductDetail>,period:{from:string;to:string}):Map<string,DecisionProfile>{
  const histories=new Map<string,ProductAnalysisRow[]>();historySets.flat().forEach(r=>histories.set(r.sku,[...(histories.get(r.sku)||[]),r]));
  const previousMap=new Map(previous.map(r=>[r.sku,r])),result=new Map<string,DecisionProfile>();
  for(const row of current){const history=histories.get(row.sku)||[],trendDays=Math.min(7,history.length),target=history.slice(-trendDays),stateActual=sum(target),prior=previousMap.get(row.sku),actual={units:row.totalUnits,orders:row.adOrders,clicks:row.clicks,spend:row.adSpend,adRevenue:row.adRevenue},before=prior?{units:prior.totalUnits,orders:prior.adOrders,clicks:prior.clicks,spend:prior.adSpend,adRevenue:prior.adRevenue}:sum(history.slice(-trendDays*2,-trendDays));
    const activeDays=history.filter(r=>r.totalUnits>0).length,lifecycle=activeDays<=6?"冷启动":activeDays<=28?"新品":"成长期（30天窗口内）",expected=weekdayExpectation(history,target),delta=expected&&expected>0?(stateActual.units-expected)/expected:null;
    let sustainedDays=0;for(const r of [...target].reverse()){const base=history.filter(x=>new Date(`${x.day}T00:00:00`).getDay()===new Date(`${r.day}T00:00:00`).getDay()&&x.day<r.day).slice(-4);const avg=base.length?base.reduce((s,x)=>s+x.totalUnits,0)/base.length:null;if(avg!=null&&r.totalUnits<avg*.85)sustainedDays++;else break;}
    const trend=expected==null?"样本不足":expected<15?"低销量样本":delta!=null&&sustainedDays>=3&&((expected<50&&delta<=-.25&&expected-stateActual.units>=5)||(expected>=50&&delta<=-.15))?"持续下滑":delta!=null&&delta>=.15?"真实增长":"稳定";
    const confidence=Math.min(1,actual.clicks/200)*40+Math.min(1,actual.orders/30)*40+Math.min(1,history.filter(r=>r.stock>0||r.totalUnits>0).length/14)*20,events=detectEvents(history,details.get(row.sku)),periodEvents=events.filter(e=>e.day>=period.from&&e.day<=period.to),types=new Set(periodEvents.map(e=>e.type)),attribution=!periodEvents.length?"周期内未检测到价格、断货、促销或广告费突变":types.size>1?"多变量同时变化，实验已污染，不能给出单一因果结论":`检测到 ${periodEvents[0].label}；仅标记时间相关性，不直接声明因果`;
    const loc=localization(details.get(row.sku)),deltaOrders=actual.orders-before.orders,deltaSpend=actual.spend-before.spend,deltaRevenue=actual.adRevenue-before.adRevenue,deltaClicks=actual.clicks-before.clicks,marginalCpa=deltaOrders>0&&deltaSpend>0?deltaSpend/deltaOrders:null,marginalRoas=deltaSpend>0?deltaRevenue/deltaSpend:null,marginalCvr=deltaClicks>0?deltaOrders/deltaClicks:null,spendGrowth=pct(actual.spend,before.spend),orderGrowth=pct(actual.orders,before.orders),adElasticity=spendGrowth&&spendGrowth>0&&orderGrowth!=null?orderGrowth/spendGrowth:null;
    const saturated=spendGrowth!=null&&spendGrowth>=.10&&actual.clicks>before.clicks&&(orderGrowth??0)<.05&&row.cpaLimit!=null&&marginalCpa!=null&&marginalCpa>row.cpaLimit,vetoes:string[]=[];if(row.cpaLimit==null)vetoes.push("成本或平台费率不完整");if(row.cpaLimit!=null&&marginalCpa!=null&&marginalCpa>row.cpaLimit)vetoes.push("边际 CPA 超过目标/保本线");if(row.inventoryDays!=null&&row.inventoryDays<21)vetoes.push("库存覆盖低于 21 天");if(confidence<60)vetoes.push("数据可信度低于 60%");if(loc.confidence<80)vetoes.push("主要集群本地化数据覆盖不足 80%");if(types.size>1)vetoes.push("实验存在多变量污染");if(saturated)vetoes.push("广告已出现边际饱和");
    const experimentDecision=saturated?"停止继续加预算：广告费和点击增长，但链接订单增长不足 5%，且边际 CPA 超线":vetoes.length?`暂不放量：${vetoes[0]}`:marginalCvr!=null&&row.cvr!=null&&marginalCvr>=row.cvr/100*.8&&orderGrowth!=null&&orderGrowth>=.08?"通过边际关口，可进行下一次 +10% 单变量预算实验":"维持当前预算，继续观察边际承接";
    const nextGate=row.cpaLimit==null?"先补齐成本，建立目标 CPA 与保本 ROAS":confidence<60?`累计达到 200 点击、30 广告订单和 14 个有效日（当前可信度 ${confidence.toFixed(0)}%）`:loc.confidence<80?"补齐主要集群库存、本地配送率、时效和可售率":`预算仅增加 10%，观察 48–72 小时；链接订单增长需 ≥8%，边际 CPA ≤ ${row.cpaLimit?.toFixed(2)??"目标值"}`;
    const stopConditions=[`链接订单增长低于 5%`,row.cpaLimit!=null?`边际 CPA 高于 ${row.cpaLimit.toFixed(2)}`:"成本缺失时禁止扩量",row.breakEvenRoas!=null?`边际 ROAS 低于 ${row.breakEvenRoas.toFixed(2)}`:"保本 ROAS 不可计算",`边际 CVR 低于基线的 80%`,`主要集群库存覆盖低于 21 天`,`出现价格、促销、库存等第二变量`];
    result.set(row.sku,{sku:row.sku,lifecycle,trend,activeDays,expectedUnits:expected,actualUnits:stateActual.units,deltaPercent:delta==null?null:delta*100,sustainedDays,confidence,events:periodEvents,attribution,inventoryMatch:loc.match,lsi:loc.lsi,lsiConfidence:loc.confidence,lsiMissing:loc.missing,marginalCpa,marginalRoas,marginalCvr,adElasticity,experimentDecision,nextGate,stopConditions,vetoes});
  }
  return result;
}

export function adjustedScore(raw:number,confidence:number){return Math.round(50+(raw-50)*Math.min(1,confidence/100));}
