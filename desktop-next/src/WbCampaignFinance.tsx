import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { canQueryCampaign, type WbAdCampaign } from "./wbAdCampaign";

type Budget = { advertId: number; total: number; currency: string };
type Balance = { balance: number; net: number; bonus: number; currency: string };
type Stat = { advertId: number; spend: number; currency: string; views?: number; clicks?: number; orders?: number };
const amount = (value: number, currency: string) => `${value.toLocaleString("zh-CN", { maximumFractionDigits: 2 })} ${currency}`;
const status: Record<number, string> = {4:"待启动",7:"已结束",9:"投放中",11:"已暂停"};

export function WbCampaignFinance({shopId, campaigns, defaultFrom, defaultTo, maxDate}: {
  shopId: string; campaigns: WbAdCampaign[]; defaultFrom: string; defaultTo: string; maxDate: string;
}) {
  const [search,setSearch]=useState("");
  const [page,setPage]=useState(0);
  const [from,setFrom]=useState(defaultFrom);
  const [to,setTo]=useState(defaultTo);
  const [budget,setBudget]=useState<Record<number,Budget>>({});
  const [balance,setBalance]=useState<Balance>();
  const [stats,setStats]=useState<Stat[]>([]);
  const [busy,setBusy]=useState(false);
  const [errors,setErrors]=useState<string[]>([]);
  const [notice,setNotice]=useState("");
  const [detail,setDetail]=useState<WbAdCampaign>();
  const [target,setTarget]=useState<WbAdCampaign>();
  const [sum,setSum]=useState("");
  const [source,setSource]=useState(0);
  const call = <T,>(action:string, payload:object={}) =>
    invoke<T>("wb_operations",{action,payload:{shopId,...payload}});
  const filtered=campaigns.filter(c=>`${c.id} ${c.name ?? ""}`.toLowerCase().includes(search.trim().toLowerCase()));
  const pageIndex=Math.min(page,Math.max(0,Math.ceil(filtered.length/20)-1));
  const visible=filtered.slice(pageIndex*20,pageIndex*20+20);
  const supportsBudget=(c:WbAdCampaign)=>[4,9,11].includes(c.status ?? 0);
  const valid=!!from && !!to && to<=maxDate && from<=to && (Date.parse(to)-Date.parse(from))/86400000<=30;

  async function refresh() {
    setBusy(true);
    setErrors([]);
    setNotice("");
    const ids=visible.filter(supportsBudget).map(c=>c.id);
    const statIds=visible.filter(canQueryCampaign).map(c=>c.id);
    const results=await Promise.allSettled([
      call<Balance>("ad_balance"),
      ids.length ? call<{adverts:Budget[]}>("ad_budgets",{ids}) : Promise.resolve({adverts:[]}),
      statIds.length ? call<Stat[]>("ad_cost_daily",{ids:statIds,from,to}) : Promise.resolve([]),
    ]);
    setBalance(results[0].status==="fulfilled" ? results[0].value : undefined);
    const budgets = results[1].status === "fulfilled" ? results[1].value.adverts : [];
    setBudget(previous=>{
      const next={...previous};
      ids.forEach(id=>delete next[id]);
      (budgets ?? []).forEach(row=>{next[row.advertId]=row;});
      return next;
    });
    setStats(results[2].status==="fulfilled" ? results[2].value : []);
    setErrors(results.flatMap((result,index)=>result.status==="rejected" ? [`${["推广账户","预算","费用"][index]}：${String(result.reason)}`] : []));
    setBusy(false);
  }

  async function deposit() {
    if (!target || busy || !Number.isSafeInteger(Number(sum)) || Number(sum)<=0) return;
    const current=budget[target.id];
    if (!current) return;
    const label=source===0?"推广账户":source===1?"结算余额":"奖金";
    if (!window.confirm(`确认从${label}向活动 ${target.name ?? target.id} 充值 ${amount(Number(sum),current.currency)}？`)) return;
    setBusy(true);
    setErrors([]);
    setNotice("");
    const id=target.id;
    setTarget(undefined);
    try {
      const result=await call<{total:number;currency:string}>("ad_deposit",{id,sum:Number(sum),source});
      if(typeof result.total!=="number" || !result.currency) throw Error("WB 未返回更新后的余额");
      setBudget(previous=>({...previous,[id]:{advertId:id,...result}}));
      setBalance(undefined);
      setNotice("充值成功，活动预算已按 WB 返回值更新。");
    } catch(reason) {
      setBudget(previous=>{const next={...previous};delete next[id];return next;});
      setBalance(undefined);
      setErrors([`充值结果待确认：${String(reason)}。请先刷新余额并核对记录，再决定是否重试。`]);
    } finally {setBusy(false);}
  }

  return <section className="wbo-panel">
    <div className="wbo-heading"><h2>活动预算与费用</h2><span>{balance ? `推广账户 ${amount(balance.balance,balance.currency)} · 结算余额 ${amount(balance.net,balance.currency)} · 奖金 ${amount(balance.bonus,balance.currency)}` : "推广账户余额待刷新"}</span></div>
    <div className="wbo-toolbar">
      <label>搜索活动<input value={search} disabled={busy} onChange={e=>{setSearch(e.target.value);setPage(0);}} placeholder="名称或 ID"/></label>
      <label>费用起始<input type="date" value={from} disabled={busy} onChange={e=>{setFrom(e.target.value);setStats([]);}}/></label>
      <label>费用结束<input type="date" value={to} max={maxDate} disabled={busy} onChange={e=>{setTo(e.target.value);setStats([]);}}/></label>
      <button className="primary" disabled={busy||!valid} onClick={()=>void refresh()}>{busy?"正在处理…":"刷新本页预算与费用"}</button>
    </div>
    {errors.map(error=><p className="wbo-error" role="alert" key={error}>{error}</p>)}
    {notice&&<p role="status">{notice}</p>}
    <div className="wbo-table-wrap"><table>
      <thead><tr><th>广告活动</th><th>状态</th><th>计费</th><th>预算余额</th><th>费用</th><th>展示次数</th><th>CTR</th><th>广告订单</th></tr></thead>
      <tbody>{visible.map(c=>{
        const rows=stats.filter(item=>item.advertId===c.id);
        const currencies=[...new Set(rows.map(row=>row.currency))];
        const spend=currencies.map(currency=>amount(rows.filter(row=>row.currency===currency).reduce((total,row)=>total+row.spend,0),currency)).join(" / ");
        const total=(key:"views"|"clicks"|"orders")=>rows.length && rows.every(row=>typeof row[key]==="number") ? rows.reduce((sum,row)=>sum+row[key]!,0) : null;
        const views=total("views"), clicks=total("clicks");
        return <tr key={c.id} onDoubleClick={()=>setDetail(c)}><td><button type="button" onClick={()=>setDetail(c)}>{c.name || "未命名活动"}</button><small>ID {c.id}</small></td>
          <td>{status[c.status ?? 0] ?? "—"}</td><td>{c.payment?.toUpperCase() ?? "—"}</td>
          <td>{budget[c.id] ? amount(budget[c.id].total,budget[c.id].currency) : "—"}
            {supportsBudget(c)&&<button type="button" disabled={busy||!budget[c.id]} onClick={()=>{setTarget(c);setSum("");setSource(0);}}>充值预算</button>}</td>
          <td>{spend||"—"}</td><td>{views??"—"}</td><td>{views&&clicks!==null ? `${(100*clicks/views).toFixed(2)}%` : "—"}</td><td>{total("orders")??"—"}</td></tr>;
      })}</tbody>
    </table></div>
    <div className="wbo-actions"><button disabled={busy||pageIndex===0} onClick={()=>setPage(pageIndex-1)}>上一页</button>
      <span>第 {pageIndex+1} 页 · {filtered.length} 个活动</span>
      <button disabled={busy||(pageIndex+1)*20>=filtered.length} onClick={()=>setPage(pageIndex+1)}>下一页</button></div>
    {detail&&<CampaignDetail key={shopId+":"+detail.id} shopId={shopId} campaign={detail} from={from} to={to} maxDate={maxDate}
      close={()=>setDetail(undefined)} budget={budget[detail.id]} updateBudget={value=>setBudget(previous=>({...previous,[value.advertId]:value}))}
      recharge={supportsBudget(detail)?()=>{setTarget(detail);setSum("");setSource(0);}:undefined}/>}
    {target&&<div className="wbo-overlay"><form className="wbo-dialog" onSubmit={e=>{e.preventDefault();void deposit();}}>
      <h2>充值活动预算</h2><p>{target.name} · ID {target.id}</p>
      <label>金额（{budget[target.id]?.currency}）<input required type="number" min="1" step="1" value={sum} onChange={e=>setSum(e.target.value)}/></label>
      <label>资金来源<select value={source} onChange={e=>setSource(Number(e.target.value))}><option value={0}>推广账户</option><option value={1}>结算余额</option><option value={3}>奖金</option></select></label>
      <div className="wbo-actions"><button type="button" onClick={()=>setTarget(undefined)}>取消</button><button className="primary" disabled={busy}>确认充值</button></div>
    </form></div>}
  </section>;
}

type Placement = "search" | "recommendations" | "combined";
type BidProduct = { nmId:number; name?:string; code?:string; image?:string; bids:Partial<Record<Placement,number>> };
type Detail = { currency:string; strategy?:string; placements?:Partial<Record<Placement,boolean>>; editable:boolean; products:BidProduct[] };
const placementLabels:Record<Placement,string> = {search:"搜索",recommendations:"推荐",combined:"统一竞价"};

function CampaignDetail({shopId,campaign,from,to,maxDate,close,budget,updateBudget,recharge}:{
  shopId:string;campaign:WbAdCampaign;from:string;to:string;maxDate:string;close:()=>void;
  budget?:Budget;updateBudget:(budget:Budget)=>void;recharge?:()=>void;
}) {
  const [detail,setDetail]=useState<Detail>();
  const [stats,setStats]=useState<Stat[]>([]);
  const [start,setStart]=useState(from);
  const [end,setEnd]=useState(to);
  const [drafts,setDrafts]=useState<Record<string,string>>({});
  const [pending,setPending]=useState<Record<string,boolean>>({});
  const [errors,setErrors]=useState<Record<string,string>>({});
  const [notice,setNotice]=useState("");
  const id=campaign.id;
  const valid=!!start && !!end && start<=end && end<=maxDate && (Date.parse(end)-Date.parse(start))/86400000<=30;
  const request=<T,>(action:string,payload:object={})=>invoke<T>("wb_operations",{action,payload:{shopId,id,...payload}});
  async function run(key:string, action:()=>Promise<void>) {
    setPending(old=>({...old,[key]:true}));
    setErrors(old=>({...old,[key]:""}));
    try {await action();}
    catch(error) {setErrors(old=>({...old,[key]:String(error)}));}
    finally {setPending(old=>({...old,[key]:false}));}
  }
  const loadDetails=()=>run("detail",async()=>{
    setDetail(undefined);
    setDrafts({});
    setDetail(await request<Detail>("ad_bid_details"));
  });
  const loadBudget=()=>run("budget",async()=>{
    const result=await request<{adverts:Budget[]}>("ad_budgets",{ids:[id]});
    const value=result.adverts.find(item=>item.advertId===id);
    if(!value) throw Error("WB 未返回活动预算");
    updateBudget(value);
  });
  const loadStats=()=>run("stats",async()=>{
    setStats([]);
    setStats(await request<Stat[]>("ad_cost_daily",{ids:[id],from:start,to:end}));
  });
  useEffect(()=>{
    void loadDetails();
    if(recharge && !budget) void loadBudget();
    if(valid && canQueryCampaign(campaign)) void loadStats();
  },[shopId,id]);

  function save(product:BidProduct,placement:Placement,bid:number) {
    const previous=product.bids[placement];
    if(!detail?.editable || pending.save || previous===undefined || bid<=0 || bid>2147483647 || bid===previous) return;
    if(!window.confirm([campaign.name ?? id,"商品 "+product.nmId,placementLabels[placement],amount(previous/100,detail.currency)+" → "+amount(bid/100,detail.currency),"确认提交给 WB？"].join(" · "))) return;
    setNotice("");
    void run("save",async()=>{
      try {
        const result=await request<{bid:number}>("ad_bid_save",{nmId:product.nmId,placement,previous,bid,currency:detail.currency});
        setDetail({...detail,products:detail.products.map(item=>item.nmId===product.nmId?{...item,bids:{...item.bids,[placement]:result.bid}}:item)});
        setNotice("WB 已确认新竞价。");
      } catch(error) {
        await loadDetails();
        throw Error("保存未确认，请核对 WB 当前竞价。 "+String(error));
      }
    });
  }
  const total=(key:"views"|"clicks"|"orders")=>stats.length && stats.every(row=>row[key]!=null)?stats.reduce((sum,row)=>sum+row[key]!,0):null;
  const views=total("views"),clicks=total("clicks");
  const currency=stats[0]?.currency;
  const spend=currency&&stats.every(row=>row.currency===currency)?stats.reduce((sum,row)=>sum+row.spend,0):null;
  const columns:Placement[]=detail?.strategy==="manual"?["search","recommendations"]:["combined"];
  const canEdit=detail?.editable && detail.products.some(item=>Object.keys(item.bids).length>0);
  return <div className="wbo-overlay"><section className="wbo-dialog wbo-ad-detail" role="dialog" aria-modal="true" aria-labelledby="wbo-ad-title"
    onKeyDown={e=>{if(e.key==="Escape"&&!pending.save)close();}}>
    <div className="wbo-heading"><div><h2 id="wbo-ad-title">{campaign.name || "广告活动"}</h2><p>ID {id} · {status[campaign.status ?? 0] ?? "状态未知"} · {campaign.payment?.toUpperCase() ?? "—"}</p></div>
      <button autoFocus type="button" disabled={pending.save} onClick={close}>关闭</button></div>
    <section className="wbo-panel"><div className="wbo-heading"><h2>活动预算</h2>
      {recharge&&<div className="wbo-actions"><button disabled={pending.budget} onClick={()=>void loadBudget()}>刷新预算</button><button disabled={!budget||pending.budget||!!errors.budget} onClick={recharge}>充值预算</button></div>}</div>
      <p>余额</p><strong>{budget?amount(budget.total,budget.currency):"—"}</strong>{errors.budget&&<p role="alert">{errors.budget}</p>}
    </section>
    <section className="wbo-panel"><h2>区间统计</h2><div className="wbo-toolbar">
      <label>起始日期<input type="date" value={start} disabled={pending.stats} onChange={e=>{setStart(e.target.value);setStats([]);}}/></label>
      <label>结束日期<input type="date" value={end} max={maxDate} disabled={pending.stats} onChange={e=>{setEnd(e.target.value);setStats([]);}}/></label>
      <button disabled={!valid||pending.stats||!canQueryCampaign(campaign)} onClick={()=>void loadStats()}>{pending.stats?"查询中…":"查询统计"}</button>
    </div>{errors.stats&&<p role="alert">{errors.stats}</p>}
    <dl className="wb-finance-kpis">{[
      ["曝光",views],["点击",clicks],["广告订单",total("orders")],
      ["费用",spend!==null?amount(spend,currency):null],
      ["平均 CPC",spend!==null&&clicks?amount(spend/clicks,currency):null],
      ["CTR",views&&clicks!==null?(100*clicks/views).toFixed(2)+"%":null],
    ].map(([label,value])=><div key={label}><dt>{label}</dt><dd>{value??"—"}</dd></div>)}</dl></section>
    <section className="wbo-panel"><h2>投放位置</h2><div className="wbo-ad-placements">
      {(["search","recommendations"] as const).map(key=><div key={key}><b>{placementLabels[key]}</b><span>{detail?.placements?.[key]===undefined?"—":detail.placements[key]?"已开启":"关闭"}</span></div>)}
    </div></section>
    <section className="wbo-panel"><div className="wbo-heading"><h2>商品竞价 {detail&&"· "+detail.products.length+" 件"}</h2>
      <button disabled={pending.detail||pending.save} onClick={()=>void loadDetails()}>刷新竞价</button></div>
      {pending.detail&&<p role="status">正在读取 WB 商品和竞价…</p>}
      {[errors.detail,errors.save].filter(Boolean).map(error=><p key={error} role="alert" className="wbo-error">{error}</p>)}
      {notice&&<p role="status">{notice}</p>}
      {detail&&<><p>{canEdit?"保存后会更新 WB 活动的实际竞价。":detail.editable?"WB 暂未返回可修改的竞价，请刷新后重试。":"当前活动状态下竞价只读。"} 金额单位 {detail.currency}。</p>
        <div className="wbo-table-wrap"><table><thead><tr><th>商品</th>{columns.map(key=><th key={key}>{placementLabels[key]}</th>)}</tr></thead>
        <tbody>{detail.products.map(item=><tr key={item.nmId}><td><div className="wbo-ad-product">
          {item.image?.startsWith("https://")&&<img src={item.image} alt="" loading="lazy"/>}
          <div><b>{item.name || "WB 商品 "+item.nmId}</b><small>{item.nmId}{item.code&&" · "+item.code}</small></div>
        </div></td>{columns.map(key=>{
          const current=item.bids[key], draftKey=item.nmId+":"+key;
          const draft=drafts[draftKey] ?? (current===undefined?"":(current/100).toFixed(2));
          const bid=/^\d+(?:\.\d{0,2})?$/.test(draft)?Math.round(Number(draft)*100):0;
          return <td key={key}>{current===undefined?"—":!detail.editable?amount(current/100,detail.currency):
            <form className="wbo-ad-bid" onSubmit={e=>{e.preventDefault();save(item,key,bid);}}>
              <input aria-label={item.nmId+" "+placementLabels[key]} inputMode="decimal" value={draft} disabled={pending.save} onChange={e=>setDrafts(old=>({...old,[draftKey]:e.target.value}))}/>
              <button type="submit" disabled={pending.save||bid<=0||bid>2147483647||bid===current}>保存</button>
            </form>}</td>;
        })}</tr>)}</tbody></table></div></>}
    </section></section></div>;
}
