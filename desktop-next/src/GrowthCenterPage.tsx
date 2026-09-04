import { useEffect, useMemo, useState } from "react";
import { AlertTriangle, CheckCircle2, RefreshCw, Save, Target, TrendingUp } from "lucide-react";
import { advertising, businessReport, competitors, dashboard, inventory } from "./bridge";
import type { AdvertisingData, BusinessReport, CompetitorRow, DashboardData, DateRange, InventoryRow } from "./types";
import "./growth-center.css";

type GrowthInputs = { targetRevenue: string; repeatRate: string; purchaseFrequency: string; customerLifespan: string; teamCapacity: string; growthPreference: string; notes: string };
const initialInputs: GrowthInputs = { targetRevenue: "", repeatRate: "", purchaseFrequency: "", customerLifespan: "", teamCapacity: "", growthPreference: "optimize", notes: "" };
const emptyDashboard: DashboardData = { revenue:0, orders:0, soldUnits:0, activeProducts:0, adSpend:0, adRevenue:0, adOrders:0, conversionRate:null, trend:[], lastSync:null, acos:null, tacos:null, ctr:null, returnUnits:0, returnRate:null, cancellationUnits:0, cancellationRate:null, views:0, orderConversion:null };
const emptyAds: AdvertisingData = { impressions:0, clicks:0, cartAdds:0, orders:0, revenue:0, spend:0, clickSpend:0, orderSpend:0, unclassifiedSpend:0, ctr:null, cpc:null, roas:null, conversionRate:null, cpa:null, acos:null, breakEvenRoas:null, targetRoas:null, maxCpa:null, knownCostMargin:null, marginCoveragePercent:0, campaigns:[], products:[], trend:[] };
const emptyReport: BusinessReport = { revenue:0,orders:0,adSpend:0,financeNet:0,salesReturns:0,accrualFees:0,otherAdjustments:0,commission:0,financeAdvertising:0,deliveryFees:0,returnFees:0,purchaseCost:0,firstMileCost:0,estimatedProfit:0,settledProfit:null,taxRate:0,taxAmount:0,payoutFeeRate:0,payoutFee:0,afterTaxProfit:null,acquiring:0,storagePackaging:0,penaltiesAdjustments:0,otherFinanceFees:0,unallocatedFinanceAmount:0,financeOperations:0,exactSkuOperations:0,unallocatedOperations:0,cashFlowReportedTotal:0,reconciliationDifference:null,missingCostSkus:0,costedUnits:0,missingCostUnits:0,daily:[] };
const percent = (value: number | null) => value == null || !Number.isFinite(value) ? "—" : `${value.toFixed(1)}%`;

export function GrowthCenterPage({ range, currency, shopName }: { range: DateRange; currency: string; shopName: string }) {
  const [dash,setDash]=useState(emptyDashboard),[ads,setAds]=useState(emptyAds),[report,setReport]=useState(emptyReport),[stocks,setStocks]=useState<InventoryRow[]>([]),[rivals,setRivals]=useState<CompetitorRow[]>([]),[loading,setLoading]=useState(false),[saved,setSaved]=useState(false);
  const [inputs,setInputs]=useState<GrowthInputs>(()=>{try{return JSON.parse(localStorage.getItem("ozon-growth-center-inputs")||"null")||initialInputs}catch{return initialInputs}});
  const load=async()=>{setLoading(true);try{const [d,a,b,i,c]=await Promise.all([dashboard(range),advertising(range),businessReport(range),inventory("",45,21,7),competitors()]);setDash(d);setAds(a);setReport(b);setStocks(i);setRivals(c)}finally{setLoading(false)}};
  useEffect(()=>{void load()},[range.from,range.to]);
  const data=useMemo(()=>{
    const aov=dash.orders?dash.revenue/dash.orders:null;
    const contribution=report.revenue>0&&report.missingCostUnits===0?report.estimatedProfit/report.revenue*100:null;
    const adOrderProxy=ads.orders>0?ads.spend/ads.orders:null;
    const repeat=Number(inputs.repeatRate)>0?Number(inputs.repeatRate):null, frequency=Number(inputs.purchaseFrequency)>0?Number(inputs.purchaseFrequency):null, lifespan=Number(inputs.customerLifespan)>0?Number(inputs.customerLifespan):null;
    const ltv=aov!=null&&repeat!=null&&frequency!=null&&lifespan!=null&&contribution!=null?aov*frequency*lifespan*(contribution/100):null;
    const stockRisk=stocks.filter(x=>["stockout","critical","warning"].includes(x.healthStatus)).length;
    const competitorAlerts=rivals.filter(x=>["critical","warning"].includes(x.priceAlertLevel)).length;
    const levers=[
      {id:"traffic",name:"流量",metric:dash.views?`${dash.views.toLocaleString()} 浏览`:"浏览量缺失",score:dash.views&&dash.orderConversion!=null?(dash.orderConversion>=2?62:75):45,reason:ads.impressions?`广告曝光 ${ads.impressions.toLocaleString()}，CTR ${percent(ads.ctr)}`:"广告曝光数据不足"},
      {id:"conversion",name:"转化",metric:percent(dash.orderConversion),score:dash.orderConversion==null?48:dash.orderConversion<1.5?92:dash.orderConversion<2.5?72:48,reason:`退货 ${percent(dash.returnRate)} · 取消 ${percent(dash.cancellationRate)}`},
      {id:"aov",name:"客单价",metric:aov==null?"—":money(aov,currency),score:aov==null?45:60,reason:"当前 API 无类目客单价基准，需结合商品组合判断"},
      {id:"retention",name:"留存",metric:repeat==null?"待填写":`${repeat}%`,score:repeat==null?68:repeat<15?88:repeat<28?70:45,reason:repeat==null?"Ozon 当前缓存无法可靠识别复购客户":"使用人工录入复购率"},
      {id:"expansion",name:"扩张",metric:`${dash.activeProducts} 活跃商品`,score:contribution!=null&&contribution>0&&stockRisk===0?58:35,reason:`库存风险 ${stockRisk} · 竞品预警 ${competitorAlerts}`},
    ];
    const opportunities=[
      {name:"修复商品页转化漏斗",lever:"转化",impact:dash.orderConversion!=null&&dash.orderConversion<2?5:3,effort:2,why:`浏览下单转化 ${percent(dash.orderConversion)}`},
      {name:"降低低效广告并放大达标计划",lever:"流量",impact:ads.roas!=null&&ads.targetRoas!=null&&ads.roas<ads.targetRoas?5:3,effort:2,why:`ROAS ${ads.roas?.toFixed(2)??"—"} / 目标 ${ads.targetRoas?.toFixed(2)??"—"}`},
      {name:"处理断货与低库存 SKU",lever:"扩张",impact:stockRisk?5:2,effort:3,why:`${stockRisk} 个库存风险 SKU`},
      {name:"建立复购与老客运营基线",lever:"留存",impact:repeat==null||repeat<20?4:2,effort:3,why:repeat==null?"复购率尚未录入":`当前复购率 ${repeat}%`},
      {name:"组合装与关联销售测试",lever:"客单价",impact:4,effort:2,why:aov==null?"客单价缺失":`当前客单价 ${money(aov,currency)}`},
      {name:"根据竞品预警调整价格价值表达",lever:"转化",impact:competitorAlerts?4:2,effort:2,why:`${competitorAlerts} 个竞品价格预警`},
    ].map(x=>({...x,priority:x.impact*2-x.effort})).sort((a,b)=>b.priority-a.priority);
    return{aov,contribution,adOrderProxy,ltv,repeat,stockRisk,competitorAlerts,levers,opportunities};
  },[dash,ads,report,stocks,rivals,inputs,currency]);
  const top=data.opportunities.slice(0,3);
  const save=()=>{localStorage.setItem("ozon-growth-center-inputs",JSON.stringify(inputs));setSaved(true);window.setTimeout(()=>setSaved(false),1600)};
  return <div className="growth-center-page">
    <header className="page-header"><div><span className="eyebrow">GROWTH STRATEGY</span><h1>电商增长中心</h1><p>{shopName} · 从真实经营数据识别优先增长杠杆，并形成 30/60/90 天路线图</p></div><div className="growth-actions"><button onClick={save}>{saved?<CheckCircle2 size={16}/>:<Save size={16}/>} {saved?"已保存":"保存增长目标"}</button><button className="dark-button" disabled={loading} onClick={()=>void load()}><RefreshCw size={16} className={loading?"spin":""}/>{loading?"诊断中":"刷新诊断"}</button></div></header>
    <section className="growth-truth"><AlertTriangle size={16}/><span>CAC 需要“获客花费 ÷ 新客户数”，当前 Ozon 缓存没有可靠新客标识，因此只显示广告归因订单成本代理值；LTV 仅在你补充复购参数且成本完整时计算。</span></section>
    <div className="growth-kpis"><div><span>区间销售额</span><strong>{money(dash.revenue,currency)}</strong><small>{dash.orders} 单 · {dash.soldUnits} 件</small></div><div><span>平均订单金额 AOV</span><strong>{data.aov==null?"—":money(data.aov,currency)}</strong><small>销售额 ÷ 订单数</small></div><div><span>贡献利润率</span><strong>{percent(data.contribution)}</strong><small>{data.contribution==null?`缺 ${report.missingCostUnits} 件成本或口径不完整`:"经营预估口径"}</small></div><div><span>广告订单成本代理值</span><strong>{data.adOrderProxy==null?"—":money(data.adOrderProxy,currency)}</strong><small>不是严格 CAC</small></div><div><span>估算 LTV</span><strong>{data.ltv==null?"—":money(data.ltv,currency)}</strong><small>依赖人工复购参数</small></div><div><span>目标销售额差距</span><strong>{Number(inputs.targetRevenue)>0?money(Math.max(0,Number(inputs.targetRevenue)-dash.revenue),currency):"待填写"}</strong><small>以当前筛选周期为口径</small></div></div>
    <section className="card growth-inputs"><div className="card-title">增长目标与经营假设 <span className="badge blue">人工输入</span></div><label>周期目标销售额<input type="number" min="0" value={inputs.targetRevenue} onChange={e=>setInputs({...inputs,targetRevenue:e.target.value})}/></label><label>复购客户占比 %<input type="number" min="0" max="100" value={inputs.repeatRate} onChange={e=>setInputs({...inputs,repeatRate:e.target.value})}/></label><label>年购买频次<input type="number" min="0" step="0.1" value={inputs.purchaseFrequency} onChange={e=>setInputs({...inputs,purchaseFrequency:e.target.value})}/></label><label>客户生命周期（年）<input type="number" min="0" step="0.5" value={inputs.customerLifespan} onChange={e=>setInputs({...inputs,customerLifespan:e.target.value})}/></label><label>增长方向<select value={inputs.growthPreference} onChange={e=>setInputs({...inputs,growthPreference:e.target.value})}><option value="optimize">优化现有经营</option><option value="products">扩展产品</option><option value="channels">扩展平台</option><option value="markets">扩展市场</option></select></label><label>团队与资源<input value={inputs.teamCapacity} onChange={e=>setInputs({...inputs,teamCapacity:e.target.value})} placeholder="负责人、预算、供应链能力"/></label></section>
    <section className="growth-levers"><div className="growth-section-title"><div><h2>五个增长杠杆</h2><p>优先度越高，越值得先投入验证</p></div></div><div className="lever-grid">{data.levers.map(x=><div className="card lever-card" key={x.id}><div><span>{x.name}</span><b>{x.score}</b></div><strong>{x.metric}</strong><i><em style={{width:`${x.score}%`}}/></i><small>{x.reason}</small></div>)}</div></section>
    <div className="growth-two-cols"><section className="card opportunity-card"><div className="card-title">增长机会矩阵 <span>影响 × 工作量</span></div><table><thead><tr><th>机会</th><th>杠杆</th><th>影响</th><th>工作量</th><th>优先级</th></tr></thead><tbody>{data.opportunities.map(x=><tr key={x.name}><td><b>{x.name}</b><small>{x.why}</small></td><td>{x.lever}</td><td>{x.impact}/5</td><td>{x.effort}/5</td><td><span className={x.priority>=7?"priority-high":x.priority>=4?"priority-mid":"priority-low"}>{x.priority>=7?"立即":x.priority>=4?"随后":"观察"}</span></td></tr>)}</tbody></table></section>
      <section className="card ansoff-card"><div className="card-title">增长路径判断</div><div className="ansoff-grid"><div className="recommended"><b>市场渗透</b><span>现有产品 × 现有 Ozon 市场</span><p>先处理转化、广告、库存和客单价，风险最低。</p></div><div><b>产品扩展</b><span>新产品 × 现有市场</span><p>用竞品痛点和现有商品表现选择相邻产品。</p></div><div><b>渠道/市场扩展</b><span>现有产品 × 新平台或地区</span><p>仅在贡献利润为正、库存同步稳定后启动。</p></div><div><b>多元化</b><span>新产品 × 新市场</span><p>风险最高，当前不应作为默认优先项。</p></div></div></section></div>
    <section className="card growth-roadmap"><div className="card-title">30 / 60 / 90 天增长路线图</div><div className="growth-phases">{[{period:"1–30 天",title:"基础修复",item:top[0],milestone:"完成数据口径、成本和关键漏斗修复"},{period:"31–60 天",title:"构建实验",item:top[1],milestone:"运行至少一个可对照的增长实验"},{period:"61–90 天",title:"放大有效动作",item:top[2],milestone:"只扩大已证明改善利润或转化的动作"}].map((phase,i)=><div key={phase.period}><i>{i+1}</i><span>{phase.period}</span><h3>{phase.title}</h3><b>{phase.item?.name??"继续观察数据"}</b><p>{phase.item?.why??"当前样本不足"}</p><small>里程碑：{phase.milestone}</small></div>)}</div></section>
  </div>;
}
function money(value:number,currency:string){return `${currency==="CNY"?"¥":"₽"}${value.toLocaleString("zh-CN",{maximumFractionDigits:2})}`}
