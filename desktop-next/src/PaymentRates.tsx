import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type RecordRow = { from: string; to: string; rub: number; cny: number | null; rate: number | null; fetchedAt: string };
const amount = (n: number) => n.toLocaleString("zh-CN", {minimumFractionDigits:2,maximumFractionDigits:2});
export function PaymentRates() {
  const now = new Date();
  const [from,setFrom] = useState(`${now.getFullYear()}-${String(now.getMonth()+1).padStart(2,"0")}-01`);
  const [to,setTo] = useState(`${now.getFullYear()}-${String(now.getMonth()+1).padStart(2,"0")}-${String(now.getDate()).padStart(2,"0")}`);
  const [rows,setRows] = useState<RecordRow[]>([]);
  const [draft,setDraft] = useState<Record<string,string>>({});
  const [busy,setBusy] = useState(false);
  const [message,setMessage] = useState("");
  const update = (values: RecordRow[]) => {setRows(values);setDraft(Object.fromEntries(values.map(r=>[`${r.from}/${r.to}`,r.cny == null ? "" : String(r.cny)])));};
  useEffect(()=>{let active=true;invoke<RecordRow[]>("payment_rate_list").then(v=>{if(active)update(v);}).catch(e=>{if(active)setMessage(String(e));});return()=>{active=false;};},[]);
  const run = async (command:string,args:Record<string,unknown>) => {setBusy(true);setMessage("");try{update(await invoke<RecordRow[]>(command,args));setMessage(command==="payment_rate_fetch"?"付款卢布已获取，请填写对应人民币付款合计。":"人民币金额与结算汇率已保存。");}catch(e){setMessage(String(e));}finally{setBusy(false);}};
  return <section className="card table-card" style={{padding:20,marginBottom:20}}>
    <div className="section-heading"><div><h2>付款与结算汇率</h2><p>获取 Ozon 付款卢布，填写同一批付款的人民币合计，计算 1 CNY 对应的 RUB。</p></div></div>
    <div className="section-heading"><label>开始日期 <input aria-label="付款开始日期" type="date" disabled={busy} value={from} onChange={e=>setFrom(e.target.value)}/></label><label>结束日期 <input aria-label="付款结束日期" type="date" disabled={busy} value={to} onChange={e=>setTo(e.target.value)}/></label><button disabled={busy || !from || !to || from>to} onClick={()=>void run("payment_rate_fetch",{from,to})}>{busy?"处理中…":"获取本期付款卢布"}</button></div>
    <p>人民币请填写所选期间内已完成付款的对应金额，排除待付款。多笔付款填写合计；日期范围重叠的记录请分别核对。此处保存仅用于付款对账，不自动修改利润核算汇率。</p>
    {message && <p className="sync-message" role="status">{message}</p>}
    <table><thead><tr><th>付款期间</th><th>API 付款卢布</th><th>人民币付款合计（手填）</th><th>结算汇率（RUB/CNY）</th><th>操作 / 获取时间</th></tr></thead><tbody>{rows.map(r=>{const key=`${r.from}/${r.to}`;const text=draft[key]??"";const cny=Number(text);const valid=Number.isFinite(cny)&&cny>0&&r.rub>0;return <tr key={key}><td>{r.from} 至 {r.to}</td><td>{amount(r.rub)} ₽{r.rub===0&&<small>本期无净付款</small>}</td><td><input aria-label={`${r.from}人民币付款合计`} type="number" min="0.01" step="0.01" disabled={busy||r.rub<=0} placeholder="填写对应人民币金额" value={text} onChange={e=>setDraft(d=>({...d,[key]:e.target.value}))}/></td><td><strong>{valid?(r.rub/cny).toFixed(6):"—"}</strong><small>{r.rate!=null?`已保存：${r.rate.toFixed(6)}`:"尚未保存"}</small></td><td><button disabled={busy||!valid} onClick={()=>void run("payment_rate_save",{from:r.from,to:r.to,cny})}>保存金额与汇率</button><small>{new Date(r.fetchedAt).toLocaleString("zh-CN")}</small></td></tr>;})}</tbody></table>
    {!rows.length&&<p className="empty">选择付款日期范围，获取第一条记录。</p>}
  </section>;
}
