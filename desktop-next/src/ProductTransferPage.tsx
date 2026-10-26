import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Copy, RefreshCw, Search, Pause, Download } from "lucide-react";
import type { Shop } from "./types";
import "./product-transfer.css";

type Source = { id: number; offer_id: string; name: string; price: string; currency_code: string; primary_image?: string[]; images?: string[] };
type Transfer = { id: string; source: string; target: string; sourceOffer: string; offer: string; name: string; images: string[]; price: string; currency: string; status: string; issues: string[]; warnings: string[]; taskId: number | null; targetProductId: number | null; updatedAt: string; payload: unknown };
const labels: Record<string, string> = { ready: "预检通过", blocked: "需补充资料", skipped: "已有商品 · 跳过", submitting: "提交中 / 待核对", uncertain: "结果待核对", submitted: "平台处理中", moderating: "审核中", imported: "导入完成", failed: "平台拒绝", found: "目标已有 · 待核对" };
const call = <T,>(command: string, args: Record<string, unknown>) => {
  if (!("__TAURI_INTERNALS__" in window)) return Promise.reject(new Error("请在桌面正式程序中连接店铺后使用"));
  return invoke<T>(command, args);
};
export function ProductTransferPage({ shops }: { shops: Shop[] }) {
  const available = shops.filter(s => s.kind === "cross_border");
  const [source, setSource] = useState(available[0]?.id ?? "");
  const [target, setTarget] = useState(available[1]?.id ?? "");
  const [items, setItems] = useState<Source[]>([]);
  const [history, setHistory] = useState<Transfer[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [query, setQuery] = useState("");
  const [prefix, setPrefix] = useState("");
  const [multiplier, setMultiplier] = useState("1");
  const [currency, setCurrency] = useState("CNY");
  const [cursor, setCursor] = useState("");
  const [loaded, setLoaded] = useState(false);
  const [total, setTotal] = useState(0);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [detail, setDetail] = useState<Transfer | null>(null);
  const [confirm, setConfirm] = useState(false);
  const stop = useRef(false);
  const mounted = useRef(true);
  const scope = useRef(0);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; stop.current = true; }; }, []);
  const reload = () => call<Transfer[]>("transfer_history", { source, target });
  useEffect(() => {
    const version = ++scope.current;
    setHistory([]); setConfirm(false); setDetail(null);
    if (source && target) void reload().then(v => { if (version === scope.current && mounted.current) setHistory(v); }).catch(e => { if (version === scope.current && mounted.current) setMessage(String(e)); });
  }, [source, target]);
  const visible = useMemo(() => items.filter(p => `${p.offer_id} ${p.name}`.toLowerCase().includes(query.toLowerCase())), [items, query]);
  const ready = history.filter(r => r.status === "ready");
  const update = (row: Transfer) => { if (mounted.current) setHistory(old => [row, ...old.filter(r => r.id !== row.id)]); };
  const run = async (work: () => Promise<void>) => {
    if (busy) return;
    stop.current = false; setBusy(true); setMessage("");
    try { await work(); } catch (e) { if (mounted.current) setMessage(String(e)); }
    finally { if (mounted.current) setBusy(false); }
  };
  const read = (all: boolean, reset = false) => run(async () => {
    let next = reset ? "" : cursor;
    let accumulated: Source[] = reset ? [] : [...items];
    const seen = new Set<string>();
    do {
      const result = await call<{ items: Source[]; lastId: string; total: number }>("transfer_source_page", { source, lastId: next });
      const combined = new Map(accumulated.map(p => [p.offer_id, p]));
      result.items.forEach(p => combined.set(p.offer_id, p)); accumulated = [...combined.values()];
      setItems(accumulated); setTotal(result.total); setLoaded(true); setCursor(result.lastId);
      setMessage(`已读取 ${accumulated.length} / ${result.total} 件商品`);
      if (result.lastId === next || seen.has(result.lastId)) { setCursor(""); break; }
      seen.add(result.lastId); next = result.lastId;
      if (accumulated.length >= result.total) { setCursor(""); break; }
    } while (all && next && !stop.current);
  });
  const prepare = () => run(async () => {
    const offers = [...selected]; let done = 0; const errors: string[] = [];
    setConfirm(false);
    for (const offerId of offers) {
      if (stop.current) break;
      try { update(await call<Transfer>("transfer_prepare", { input: { source, target, offerId, prefix, multiplier: Number(multiplier), currency } })); }
      catch (e) { errors.push(`${offerId}：${String(e)}`); }
      done++; setMessage(`预检 ${done} / ${offers.length}${errors.length ? `；读取失败 ${errors.length} 件` : ""}`);
    }
    setMessage(`预检已处理 ${done} / ${offers.length} 件。${errors.join("\n")}`);
  });
  const submit = () => run(async () => {
    setConfirm(false); const rows = [...ready]; let done = 0;
    for (const row of rows) {
      if (stop.current) break;
      const result = await call<Transfer>("transfer_submit", { id: row.id }); update(result); done++;
      setMessage(`已提交 ${done} / ${rows.length} 件；请刷新平台结果查看导入与审核状态`);
      if (result.status === "uncertain") { setMessage(`货号 ${result.offer} 的响应待核对，批量提交已暂停。请刷新状态或查看目标后台。`); break; }
    }
  });
  const refresh = () => run(async () => {
    let failed = 0;
    for (const row of history.filter(r => ["submitted", "moderating", "uncertain", "submitting"].includes(r.status))) {
      if (stop.current) break;
      try { update(await call<Transfer>("transfer_refresh", { id: row.id })); } catch (e) { failed++; setMessage(`${row.offer}：${String(e)}`); }
    }
    if (!failed) setMessage("平台状态已刷新；审核仍在进行的商品可稍后再次刷新。");
  });
  const exportData = () => {
    const url = URL.createObjectURL(new Blob([JSON.stringify(history, null, 2)], { type: "application/json" }));
    const a = document.createElement("a"); a.href = url; a.download = "商品搬家-迁移草稿与结果.json"; a.click(); setTimeout(() => URL.revokeObjectURL(url), 1000);
  };
  const changeSource = (id: string) => { setSource(id); setItems([]); setSelected(new Set()); setCursor(""); setLoaded(false); setTotal(0); setMessage(""); };
  const toggle = (id: string) => setSelected(old => { const next = new Set(old); next.has(id) ? next.delete(id) : next.add(id); return next; });
  const eligible = source && target && source !== target;
  return <div className="transfer-page">
    <header><div><span className="eyebrow">CROSS-SHOP PRODUCT TRANSFER</span><h1><Copy size={26} /> 商品搬家</h1><p>跨境店铺之间复制商品资料，先预检，再发布，逐件追踪平台结果。</p></div><button disabled={!history.length || busy} onClick={exportData}><Download size={16} />导出草稿与结果</button></header>
    <section className="transfer-card"><div className="transfer-form">
      <label>源店铺<select value={source} disabled={busy} onChange={e => changeSource(e.target.value)}><option value="">选择源店铺</option>{available.map(s => <option key={s.id} value={s.id}>{s.name}</option>)}</select></label>
      <span className="transfer-arrow">→</span><label>目标店铺<select value={target} disabled={busy} onChange={e => { setTarget(e.target.value); setMessage(""); }}><option value="">选择目标店铺</option>{available.filter(s => s.id !== source).map(s => <option key={s.id} value={s.id}>{s.name}</option>)}</select></label>
      <label>目标货号前缀<input value={prefix} disabled={busy} onChange={e => setPrefix(e.target.value)} placeholder="留空保留原货号" maxLength={40}/></label>
      <label>售价倍率<input type="number" min="0.01" step="0.01" value={multiplier} disabled={busy} onChange={e => setMultiplier(e.target.value)}/></label>
      <label>目标账户币种<select value={currency} disabled={busy} onChange={e => setCurrency(e.target.value)}>{["CNY", "RUB", "USD", "EUR"].map(c => <option key={c}>{c}</option>)}</select></label>
    </div><p className="transfer-note">默认跳过目标店已有货号，不迁移库存。币种须与目标账户一致；与源商品币种不一致时预检会阻止发布。倍率和前缀只影响新生成的草稿，发布以草稿明细为准。</p>
    {available.length < 2 && <p className="transfer-warning">请先在店铺管理中添加至少两个跨境店，并分别配置 Seller API。</p>}
    <div className="transfer-actions"><button disabled={busy || !source} onClick={() => void read(false, true)}><RefreshCw size={15}/>读取首批商品</button><button disabled={busy || !source || (loaded && !cursor)} onClick={() => void read(true, !loaded)}>读取全部商品</button>{cursor && <button disabled={busy} onClick={() => void read(false)}>读取下一批</button>}<button className="primary" disabled={busy || !eligible || !selected.size || !(Number(multiplier) > 0)} onClick={() => void prepare()}>预检所选 {selected.size} 件</button>{busy && <button onClick={() => { stop.current = true; setMessage("将在当前商品处理完成后暂停，进度已保存。"); }}><Pause size={15}/>暂停</button>}</div>
    </section>
    {message && <div className="transfer-message" role="status">{message}</div>}
    <section className="transfer-card"><div className="transfer-section"><h2>源店商品 <small>{items.length} / {total} 件</small></h2><label className="transfer-search"><Search size={16}/><input placeholder="搜索货号或标题" value={query} onChange={e => setQuery(e.target.value)}/></label></div>
      <div className="transfer-table"><table><thead><tr><th><input aria-label="选择当前筛选的全部商品" type="checkbox" disabled={busy || !visible.length} checked={!!visible.length && visible.every(p => selected.has(p.offer_id))} onChange={e => { const checked = e.target.checked; setSelected(old => {const next = new Set(old); visible.forEach(p => checked ? next.add(p.offer_id) : next.delete(p.offer_id)); return next;}); }}/></th><th>图片</th><th>商品 / 货号</th><th>源售价</th></tr></thead><tbody>{visible.map(p => <tr key={p.offer_id}><td><input aria-label={`选择 ${p.offer_id}`} disabled={busy} type="checkbox" checked={selected.has(p.offer_id)} onChange={() => toggle(p.offer_id)}/></td><td>{(p.primary_image?.[0] || p.images?.[0]) ? <img loading="lazy" src={p.primary_image?.[0] || p.images?.[0]} alt="商品"/> : "—"}</td><td><b>{p.name}</b><small>{p.offer_id}</small></td><td>{p.price} {p.currency_code}</td></tr>)}</tbody></table>{!items.length && <div className="transfer-empty">选择源店并读取商品。这里只展示实际 API 返回的数据。</div>}</div>
    </section>
    <section className="transfer-card"><div className="transfer-section"><div><h2>迁移草稿与发布结果 <small>{history.length} 件 · 可发布 {ready.length} 件</small></h2><p>记录保存在本机，重新打开可继续。平台拒绝的商品可重新选择并预检；待核对任务禁止重复提交。</p></div><div className="transfer-actions"><button disabled={busy || !history.length} onClick={() => void refresh()}><RefreshCw size={15}/>刷新平台结果</button><button className="primary" disabled={busy || !ready.length || !eligible} onClick={() => setConfirm(true)}>发布预检通过的 {ready.length} 件</button></div></div>
      <div className="transfer-table"><table><thead><tr><th>商品</th><th>目标货号</th><th>草稿价格</th><th>状态 / 原因</th><th>详情</th></tr></thead><tbody>{history.map(r => <tr key={r.id}><td><b>{r.name}</b><small>来源：{r.sourceOffer}</small></td><td>{r.offer}</td><td>{r.price} {r.currency}</td><td><span className={`transfer-status ${r.status}`}>{labels[r.status] ?? r.status}</span>{r.issues.map((e, i) => <small className="transfer-error" key={i}>{e}</small>)}{r.taskId && <small>任务 {r.taskId} {r.targetProductId ? `· 商品 ${r.targetProductId}` : ""}</small>}</td><td><button onClick={() => setDetail(r)}>查看草稿</button></td></tr>)}</tbody></table>{!history.length && <div className="transfer-empty">勾选商品并预检后，在这里查看迁移内容和发布结果。</div>}</div>
    </section>
    {confirm && <div className="transfer-overlay"><section className="transfer-dialog"><h2>发布 {ready.length} 件商品到目标店</h2><p>目标店：<b>{available.find(s => s.id === target)?.name}</b></p><p>仅发布“预检通过”的已保存草稿，目标已有货号会再次核对。此操作会在目标店创建商品，库存不迁移。</p><div className="transfer-actions"><button onClick={() => setConfirm(false)}>返回检查</button><button className="primary" onClick={() => void submit()}>确认发布 {ready.length} 件</button></div></section></div>}
    {detail && <div className="transfer-overlay"><section className="transfer-dialog wide"><h2>{detail.name}</h2><p>{labels[detail.status]} · {detail.price} {detail.currency} · 更新于 {detail.updatedAt}</p><div className="transfer-images">{detail.images.map(url => <a key={url} href={url} target="_blank" rel="noreferrer"><img src={url} alt="迁移图片"/></a>)}</div>{detail.warnings.map(w => <p key={w} className="transfer-warning">{w}</p>)}<pre>{JSON.stringify(detail.payload, null, 2)}</pre><button onClick={() => setDetail(null)}>关闭详情</button></section></div>}
  </div>;
}
