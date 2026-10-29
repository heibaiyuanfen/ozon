import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Shop } from "./types";

type Plan = { warehouseId: number; warehouseName: string; quantity: number };
type Product = { target: string; offer: string; name: string; productId: number | null; exists: boolean; archived: boolean; platformStatus: string; checkedAt: string; plan: Plan | null; stockStatus: string; message: string; updatedAt: string };
type Stock = {offer: string; warehouseId: number; quantity: number | null; free: number | null; reserved: number | null; checkedAt: string; recordFound: boolean};
type Warehouse = { warehouse_id: number; name: string };
type Record = {target: string; offer: string; status: string};
const status: {[key: string]: string} = {success:"库存已写入",failed:"写入失败",uncertain:"结果待核对",applying:"写入中",skipped_stock:"目标仓库已有库存 · 已跳过"};
const call = <T,>(name: string, args: object) => invoke<T>(name,args as {[key:string]:unknown});
export function PostImportStockPage({shops,records,onBusy}: {shops: Shop[]; records: Record[]; onBusy: (busy:boolean)=>void}) {
  const [target,setTarget]=useState(shops.find(s=>s.kind==="cross_border")?.id ?? "");
  const [rows,setRows]=useState<Product[]>([]);
  const [selected,setSelected]=useState<Set<string>>(new Set());
  const [offers,setOffers]=useState("");
  const [search,setSearch]=useState("");
  const [warehouses,setWarehouses]=useState<Warehouse[]>([]);
  const [warehouseId,setWarehouseId]=useState("");
  const [excludeExisting,setExcludeExisting]=useState(true);
  const [stocks,setStocks]=useState<{[offer:string]:Stock}>({});
  const [stockScope,setStockScope]=useState("");
  const [quantity,setQuantity]=useState("1");
  const [cursor,setCursor]=useState("");
  const [total,setTotal]=useState<number | null>(null);
  const [busy,setBusy]=useState(false);
  const [message,setMessage]=useState("");
  const [confirmation,setConfirmation]=useState<{products:Product[];plan:Plan;target:string;excludeExisting:boolean}|null>(null);
  const version=useRef(0);
  const running=useRef(false);
  const stop=useRef(false);
  const mount=useRef(true);
  useEffect(()=>{mount.current=true;return ()=>{mount.current=false;stop.current=true;onBusy(false);};},[]);
  useEffect(()=>{
    const current=++version.current;
    setRows([]);setStocks({});setStockScope("");setSelected(new Set());setWarehouses([]);setWarehouseId("");setCursor("");setTotal(null);setMessage("");setOffers("");setConfirmation(null);
    if(target) void call<Product[]>("inventory_history",{target}).then(v=>{if(current===version.current && mount.current)setRows(v);}).catch(e=>{if(current===version.current && mount.current)setMessage(String(e));});
  },[target]);
  const run=async(work:()=>Promise<void>)=>{
    if(running.current)return;++version.current;running.current=true;stop.current=false;setBusy(true);onBusy(true);setMessage("");
    try{await work();}catch(e){if(mount.current)setMessage(String(e));}
    finally{running.current=false;if(mount.current){setBusy(false);onBusy(false);}}
  };
  const merge=(products:Product[])=>{if(mount.current)setRows(old=>{const map=new Map(old.map(p=>[p.offer,p]));products.forEach(p=>map.set(p.offer,p));return [...map.values()];});};
  const identify=async(ids:string[])=>{
    const unique=[...new Set(ids.map(o=>o.trim()).filter(Boolean))];if(!unique.length)throw Error("请输入目标货号，或先导入商品再从搬家记录识别。");
    let count=0;
    for(let i=0;i<unique.length;i+=100){if(stop.current)break;const products=await call<Product[]>("inventory_lookup",{target,offers:unique.slice(i,i+100)});merge(products);setStocks(old=>{const next={...old};products.forEach(p=>delete next[p.offer]);return next;});count+=products.length;if(mount.current)setMessage(`已识别 ${count} / ${unique.length} 件；未找到的商品可稍后重新识别。`);}
  };
  const read=()=>run(async()=>{
    const result=await call<{items:{offer_id:string}[];lastId:string;total:number}>("transfer_source_page",{source:target,lastId:cursor});
    if(result.lastId===cursor && result.lastId)throw Error("平台分页未前进，请稍后重试");
    await identify(result.items.map(p=>p.offer_id));setCursor(result.lastId);setTotal(result.total);
  });
  const loadWarehouses=()=>run(async()=>{const v=await call<Warehouse[]>("transfer_warehouses",{target});setWarehouses(v);setWarehouseId("");setStocks({});setStockScope("");setConfirmation(null);if(!v.length)setMessage("目标店没有可用仓库，请检查店铺后台和 API 权限。");});
  const warehouse=warehouses.find(w=>String(w.warehouse_id)===warehouseId);
  const valid=!!warehouse && quantity.trim()!=="" && Number.isSafeInteger(Number(quantity)) && Number(quantity)>=0 && Number(quantity)<=2147483647;
  const stockFor=(p:Product)=>stockScope===`${target}:${warehouseId}` ? stocks[p.offer] : undefined;
  const allowed=(p:Product)=>p.exists && !p.archived && p.stockStatus!=="applying" && (!excludeExisting || stockFor(p)?.quantity===0);
  const hasStock=(p:Product)=>(stockFor(p)?.quantity ?? 0)>0;
  const picked=rows.filter(p=>selected.has(p.offer) && allowed(p));
  const visible=rows.filter(p=>(!excludeExisting || !hasStock(p)) && `${p.offer} ${p.name}`.toLowerCase().includes(search.toLowerCase()));
  const selectable=visible.filter(allowed);
  const excludedCount=rows.filter(hasStock).length;
  const queryWarehouse=async(id:string)=>{
    if(!id)return;setStocks({});setStockScope(`${target}:${id}`);setConfirmation(null);
    const offers=rows.filter(p=>p.exists && !p.archived).map(p=>p.offer);let done=0;let excluded=0;let unknown=0;
    for(let i=0;i<offers.length;i+=100){if(stop.current)break;
      const result=await call<Stock[]>("inventory_warehouse_stocks",{target,warehouseId:Number(id),offers:offers.slice(i,i+100)});
      const values=Object.fromEntries(result.map(v=>[v.offer,v]));setStocks(old=>({...old,...values}));
      setSelected(old=>new Set([...old].filter(offer=>!(offer in values) || values[offer].quantity===0)));
      done+=result.length;excluded+=result.filter(v=>(v.quantity ?? 0)>0).length;unknown+=result.filter(v=>v.quantity===null).length;
      setMessage(`已核对目标仓库库存 ${done} / ${offers.length} 件；已有库存 ${excluded} 件，未查明 ${unknown} 件。`);
    }
  };
  const changeWarehouse=(id:string)=>{setWarehouseId(id);setStocks({});setStockScope("");setConfirmation(null);if(id)void run(()=>queryWarehouse(id));};
  const execute=()=>run(async()=>{
    const snapshot=confirmation;if(!snapshot)return;setConfirmation(null);let done=0;let failed=0;const errors:string[]=[];
    for(const p of snapshot.products){if(stop.current)break;try{const result=await call<Product>("inventory_apply",{target:snapshot.target,offer:p.offer,plan:snapshot.plan,revision:p.updatedAt,excludeExisting:snapshot.excludeExisting});merge([result]);if(!["success","skipped_stock"].includes(result.stockStatus))failed++;}catch(e){failed++;errors.push(`${p.offer}：${String(e)}`);}done++;setMessage(`已处理 ${done} / ${snapshot.products.length} 件，需核对 ${failed} 件。`);}
    setMessage(`已处理 ${done} / ${snapshot.products.length} 件，需核对 ${failed} 件。${errors.length ? "\n"+errors.join("\n") : ""}`);setSelected(new Set());setStocks({});setStockScope("");
  });
  return <div className="transfer-workspace">
    <section className="transfer-card"><h2>导入后库存</h2><p>① 选择店铺并识别商品 → ② 勾选已存在商品 → ③ 读取仓库并设置数量 → ④ 确认写入。查询和刷新均不会修改库存。</p>
      <div className="transfer-form"><label>目标店铺<select aria-label="库存目标店铺" disabled={busy} value={target} onChange={e=>setTarget(e.target.value)}><option value="">请选择店铺</option>{shops.filter(s=>s.kind==="cross_border").map(s=><option key={s.id} value={s.id}>{s.name}</option>)}</select></label><label>搜索已识别商品<input value={search} placeholder="货号或商品名称" onChange={e=>setSearch(e.target.value)}/></label></div>
      <label className="inventory-offers">目标货号（每行一个，也可用逗号分隔）<textarea disabled={busy} value={offers} onChange={e=>setOffers(e.target.value)} placeholder="输入目标店已导入或已存在的货号"/></label>
      <div className="transfer-actions"><button disabled={busy || !target || !offers.trim()} onClick={()=>void run(()=>identify(offers.split(/[\n,，;；]+/)))}>识别输入的货号</button><button disabled={busy || !target || !records.some(r=>r.target===target && !["ready","blocked"].includes(r.status))} onClick={()=>void run(()=>identify(records.filter(r=>r.target===target && !["ready","blocked"].includes(r.status)).map(r=>r.offer)))}>识别本店搬家记录商品</button><button disabled={busy || !target || !rows.length} onClick={()=>void run(()=>identify(rows.map(r=>r.offer)))}>重新识别列表商品</button><button disabled={busy || !target || (total!==null && !cursor)} onClick={()=>void read()}>{total===null ? "读取目标店已有商品" : "读取下一批已有商品"}</button>{busy && <button onClick={()=>{stop.current=true;setMessage("将在当前请求完成后停止，已完成结果会保留。");}}>停止后续处理</button>}</div>
      {total!==null && <p>目标店返回共 {total} 件商品；本机列表包含已识别记录。</p>}
    </section>
    {message && <div className="transfer-message" role="status">{message}</div>}
    <section className="transfer-card"><div className="transfer-section"><h2>商品识别结果 <small>{rows.length} 件 · 已选 {picked.length} 件 · 所选仓库已有库存 {excludedCount} 件</small></h2></div><p>“已存在”表示平台返回有效商品 ID；不等于审核通过或库存一定可写。暂未找到可能仍在创建，请稍后重新识别；已归档商品不能在此设置库存。历史识别时间会保留，写入前将再次核对商品。</p>
      <div className="transfer-table"><table><thead><tr><th><input type="checkbox" aria-label="选择已找到的全部商品" disabled={busy || !selectable.length} checked={!!selectable.length && selectable.every(p=>selected.has(p.offer))} onChange={e=>{const checked=e.target.checked;setSelected(old=>{const next=new Set(old);selectable.forEach(p=>checked ? next.add(p.offer) : next.delete(p.offer));return next;});}}/></th><th>商品 / 目标货号</th><th>创建识别</th><th>平台状态 / 识别时间</th><th>所选仓库实际库存</th><th>上次仓库 / 数量</th><th>库存结果</th></tr></thead><tbody>{visible.map(p=><tr key={p.offer}><td><input type="checkbox" aria-label={`选择库存商品 ${p.offer}`} disabled={busy || !allowed(p)} checked={selected.has(p.offer)} onChange={e=>{const checked=e.target.checked;setSelected(old=>{const n=new Set(old);checked?n.add(p.offer):n.delete(p.offer);return n;});}}/></td><td><b>{p.name || p.offer}</b><small>{p.offer}</small></td><td><span className={`transfer-status ${p.exists && !p.archived ? "ready" : "blocked"}`}>{p.archived ? "已归档" : p.exists ? "已存在 / 已创建" : "暂未找到"}</span>{p.productId && <small>商品 ID：{p.productId}</small>}</td><td><small>{p.platformStatus}</small><small>{p.checkedAt ? new Date(p.checkedAt).toLocaleString() : "尚未识别"}</small></td><td>{stockFor(p)?.quantity != null ? <><b>{stockFor(p)?.recordFound ? `${stockFor(p)?.quantity} 件` : "尚未设置该仓库库存"}</b>{!stockFor(p)?.recordFound && <small>新品可首次设置库存</small>}<small>可售 {stockFor(p)?.free ?? "—"} · 预留 {stockFor(p)?.reserved ?? "—"}</small><small>{new Date(stockFor(p)!.checkedAt).toLocaleString()}</small></> : <small>{warehouseId ? "待查询 / 查询异常，请刷新" : "请先选择目标仓库"}</small>}</td><td>{p.plan ? `${p.plan.warehouseName || p.plan.warehouseId} · ${p.plan.quantity} 件` : "未设置"}</td><td><small>{status[p.stockStatus] ?? "尚未写入"}</small><small className="transfer-error">{p.message}</small></td></tr>)}</tbody></table>{!rows.length && <div className="transfer-empty">先识别导入商品或读取店铺已有商品</div>}</div>
    </section>
    <section className="transfer-card"><h2>仓库与库存设置</h2><div className="transfer-form"><button disabled={busy || !target || !rows.some(p=>p.exists && !p.archived)} onClick={()=>void loadWarehouses()}>读取本店仓库</button><label>目标仓库<select aria-label="库存目标仓库" disabled={busy || !warehouses.length} value={warehouseId} onChange={e=>changeWarehouse(e.target.value)}><option value="">请选择仓库</option>{warehouses.map(w=><option key={w.warehouse_id} value={String(w.warehouse_id)}>{w.name} ({w.warehouse_id})</option>)}</select></label><label>每件商品可售库存<input aria-label="独立库存数量" disabled={busy} type="number" min="0" max="2147483647" step="1" value={quantity} onChange={e=>setQuantity(e.target.value)}/></label><button className="primary" disabled={busy || !valid || !picked.length} onClick={()=>{if(warehouse)setConfirmation({products:[...picked],plan:{warehouseId:warehouse.warehouse_id,warehouseName:warehouse.name,quantity:Number(quantity)},target,excludeExisting});}}>设置所选 {picked.length} 件商品库存</button></div><div className="transfer-actions transfer-filter"><label><input type="checkbox" checked={excludeExisting} disabled={busy} onChange={e=>{setExcludeExisting(e.target.checked);setSelected(new Set());setConfirmation(null);}}/> 剔除目标仓库已有库存的商品（含预留）</label><button disabled={busy || !warehouseId || !rows.length} onClick={()=>void run(()=>queryWarehouse(warehouseId))}>刷新目标仓库库存</button><span>已有库存 {excludedCount} 件；{excludeExisting ? "已剔除有库存商品；新品未设置库存时可正常提交" : "当前不过滤已有库存"}</span></div><p>选择仓库会自动查询该仓库的实际库存；查询成功且完整时，零库存或没有该仓库库存记录的商品均可设置库存；新品无需先有仓库库存记录。查询失败、分页不完整或库存数据异常时会报错，请重试。写入前还会再次查询，已有库存则自动跳过。</p><p>数量为该仓库的可售库存设置值，不是累加。支持 0；平台拒绝或结果待核对时保留原因，可核对后重新选择商品操作。商品导入和库存写入完全独立。</p></section>
    {confirmation && <div className="transfer-overlay"><section className="transfer-dialog"><h2>确认设置 {confirmation.products.length} 件商品库存</h2><p>店铺：{shops.find(s=>s.id===confirmation.target)?.name} · 仓库：{confirmation.plan.warehouseName}</p><p>每件商品设置为 {confirmation.plan.quantity} 件可售库存。{confirmation.excludeExisting ? "已启用已有库存剔除；提交前再次查询所选仓库，有库存将自动跳过。" : "已关闭已有库存剔除；会覆盖所选商品的可售库存。"}写入前再次检查商品是否存在。若上次结果待核对，请先检查后台当前库存。</p><ul>{confirmation.products.map(p=><li key={p.offer}>{p.offer}</li>)}</ul><div className="transfer-actions"><button onClick={()=>setConfirmation(null)}>返回检查</button><button className="primary" onClick={()=>void execute()}>确认写入库存</button></div></section></div>}
  </div>;
}
