import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Copy, RefreshCw, Search, Pause, Download } from "lucide-react";
import type { Shop } from "./types";
import "./product-transfer.css";
import { PostImportStockPage } from "./PostImportStockPage";

type Field = { id: number; name: string; description?: string; dictionary_id?: number; attribute_complex_id?: number; options?: {id: number; value: string}[] };
type Source = { id: number; offer_id: string; name: string; price: string; currency_code: string; primary_image?: string[]; images?: string[] };
type StockPlan = { warehouseId: number; warehouseName: string; quantity: number };
type Transfer = { id: string; source: string; target: string; sourceOffer: string; offer: string; name: string; images: string[]; price: string; currency: string; status: string; issues: string[]; warnings: string[]; taskId: number | null; targetProductId: number | null; updatedAt: string; payload: unknown; stockPlan?: StockPlan | null; stockStatus?: string; stockMessage?: string };
const labels: Record<string, string> = { ready: "预检通过", blocked: "需补充资料", skipped: "已有商品 · 跳过", submitting: "提交中 / 待核对", uncertain: "结果待核对", submitted: "平台处理中", moderating: "审核中", imported: "导入完成", failed: "平台拒绝", found: "目标已有 · 待核对" };
const stockLabels: Record<string, string> = { pending: "旧计划未执行，请到导入后库存设置", applying: "写入中 / 中断请核对", success: "库存已写入", failed: "库存写入失败", uncertain: "库存结果待核对" };
const stockSummary = (r: Transfer) => r.stockPlan ? `${r.stockPlan.warehouseName || r.stockPlan.warehouseId} · 每件 ${r.stockPlan.quantity} · ${stockLabels[r.stockStatus ?? ""] ?? "待处理"}` : "不设置库存";
const recorded = (r: Transfer) => ["imported", "submitted", "moderating", "submitting", "uncertain", "found", "skipped"].includes(r.status);
const call = <T,>(command: string, args: Record<string, unknown>) => {
  if (!("__TAURI_INTERNALS__" in window)) return Promise.reject(new Error("请在桌面正式程序中连接店铺后使用"));
  return invoke<T>(command, args);
};
export function ProductTransferPage({ shops }: { shops: Shop[] }) {
  const available = shops.filter(s => s.kind === "cross_border");
  const [source, setSource] = useState(available[0]?.id ?? "");
  const [target, setTarget] = useState(available[1]?.id ?? "");
  const [mode, setMode] = useState<"transfer" | "stock">("transfer");
  const [stockBusy, setStockBusy] = useState(false);
  const [items, setItems] = useState<Source[]>([]);
  const [records, setRecords] = useState<Transfer[]>([]);
  const [recordsLoaded, setRecordsLoaded] = useState(false);
  const [excludeScope, setExcludeScope] = useState("target");
  const [recordSource, setRecordSource] = useState("");
  const [recordTarget, setRecordTarget] = useState("");
  const [recordQuery, setRecordQuery] = useState("");
  const [recordStatus, setRecordStatus] = useState("all");
  const history = useMemo(() => records.filter(r => r.source === source && r.target === target), [records, source, target]);
  const shopName = (id: string) => shops.find(s => s.id === id)?.name ?? `已移除店铺 (${id})`;
  const productRecords = useMemo(() => {
    const result = new Map<string, Transfer[]>();
    records.filter(r => r.source === source && recorded(r)).forEach(r => result.set(r.sourceOffer, [...(result.get(r.sourceOffer) ?? []), r]));
    return result;
  }, [records, source]);
  const excluded = (offer: string) => excludeScope !== "none" && (productRecords.get(offer) ?? []).some(r => excludeScope === "any" || r.target === target);
  const recordRows = records.filter(r => (!recordSource || r.source === recordSource) && (!recordTarget || r.target === recordTarget) && (recordStatus === "all" || (recordStatus === "recorded" ? recorded(r) : r.status === recordStatus)) && `${r.name} ${r.sourceOffer} ${r.offer}`.toLowerCase().includes(recordQuery.toLowerCase()));
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
  const [fields, setFields] = useState<Field[]>([]);
  const [edits, setEdits] = useState<Record<number, string>>({});
  const [aiHelp, setAiHelp] = useState("");
  const [detailError, setDetailError] = useState("");
  const [editing, setEditing] = useState(false);
  const detailVersion = useRef(0);
  const openDetail = async (row: Transfer) => {
    const version = ++detailVersion.current;
    setDetail(row); setFields([]); setEdits({}); setAiHelp(""); setDetailError(""); setEditing(true);
    try {
      const result = await call<Field[]>("transfer_edit", {id: row.id, action: "fields", values: []});
      if (version === detailVersion.current) setFields(result);
    } catch (e) { if (version === detailVersion.current) setDetailError(String(e)); }
    finally { if (version === detailVersion.current) setEditing(false); }
  };
  const editDetail = async (action: "ai" | "save") => {
    if (!detail || editing) return;
    setEditing(true); setDetailError("");
    try {
      if (action === "ai") setAiHelp(await call<string>("transfer_edit", {id: detail.id, action, values: []}));
      else {
        const row = await call<Transfer>("transfer_edit", {id: detail.id, action, values: Object.entries(edits).map(([id, value]) => ({id: Number(id), value}))});
        update(row); setDetail(row); setEdits({}); setDetailError(row.status === "ready" ? "已保存，预检通过，可以关闭窗口后发布。" : "已保存，仍有待处理的问题。");
      }
    } catch (e) { setDetailError(String(e)); }
    finally { setEditing(false); }
  };
  const [confirm, setConfirm] = useState(false);
  const stop = useRef(false);
  const mounted = useRef(true);
  const scope = useRef(0);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; stop.current = true; }; }, []);
  const reload = () => call<Transfer[]>("transfer_history", { source: "", target: "" });
  useEffect(() => {
    const version = ++scope.current;
    setConfirm(false); setDetail(null); setSelected(new Set());
    void reload().then(v => { if (version === scope.current && mounted.current) { setRecords(v); setRecordsLoaded(true); } }).catch(e => { if (version === scope.current && mounted.current) setMessage(String(e)); });
  }, [source, target]);
  const visible = items.filter(p => !excluded(p.offer_id) && `${p.offer_id} ${p.name}`.toLowerCase().includes(query.toLowerCase()));
  const excludedCount = items.filter(p => excluded(p.offer_id)).length;
  const selectedOffers = [...selected].filter(id => !excluded(id));
  const ready = history.filter(r => r.status === "ready" && !excluded(r.sourceOffer));
  const update = (row: Transfer) => { if (mounted.current) setRecords(old => [row, ...old.filter(r => r.id !== row.id)]); };
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
    const offers = selectedOffers; let done = 0; const errors: string[] = [];
    setConfirm(false);
    for (const offerId of offers) {
      if (stop.current) break;
      try { update(await call<Transfer>("transfer_prepare", { input: { source, target, offerId, prefix, multiplier: Number(multiplier), currency, excludeScope } })); }
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
      try {
        let result = row.status === "imported" ? row : await call<Transfer>("transfer_refresh", { id: row.id }); update(result);
      } catch (e) { failed++; setMessage(`${row.offer}：${String(e)}`); }
    }
    setMessage(failed ? `刷新完成，${failed} 件需处理；请查看商品和库存状态 / 原因。` : "平台状态已刷新；商品创建后请进入“导入后库存”独立识别和设置库存。");
  });
  const refreshRecords = () => run(async () => { setRecords(await reload()); setRecordsLoaded(true); setMessage("本机搬家记录已刷新"); });
  const exportRecords = () => {
    const rows = [["源店铺", "目标店铺", "商品", "源货号", "目标货号", "状态", "目标仓库 / 库存", "库存结果", "更新时间"], ...recordRows.map(r => [shopName(r.source), shopName(r.target), r.name, r.sourceOffer, r.offer, labels[r.status] ?? r.status, stockSummary(r), r.stockMessage ?? "", r.updatedAt])];
    const escape = (value: string) => '"' + (/^[=+@\-\t\r]/.test(value) ? "'" : "") + value.replace(/"/g, '""') + '"';
    const url = URL.createObjectURL(new Blob(["\uFEFF" + rows.map(r => r.map(escape).join(",")).join("\r\n")], {type: "text/csv;charset=utf-8"}));
    const a = document.createElement("a"); a.href=url; a.download="各店铺商品搬家记录.csv"; a.click(); setTimeout(() => URL.revokeObjectURL(url),1000);
  };
  const exportData = () => {
    const url = URL.createObjectURL(new Blob([JSON.stringify(history, null, 2)], { type: "application/json" }));
    const a = document.createElement("a"); a.href = url; a.download = "商品搬家-迁移草稿与结果.json"; a.click(); setTimeout(() => URL.revokeObjectURL(url), 1000);
  };
  const changeSource = (id: string) => { setSource(id); setItems([]); setSelected(new Set()); setCursor(""); setLoaded(false); setTotal(0); setMessage(""); };
  const toggle = (id: string) => setSelected(old => { const next = new Set(old); next.has(id) ? next.delete(id) : next.add(id); return next; });
  const eligible = source && target && source !== target;
  return <div className="transfer-page">
    <header><div><span className="eyebrow">CROSS-SHOP PRODUCT TRANSFER</span><h1><Copy size={26} /> 商品搬家</h1><p>跨境店铺之间复制商品资料，先预检，再发布，逐件追踪平台结果。</p></div><button disabled={!history.length || busy} onClick={exportData}><Download size={16} />导出草稿与结果</button></header>
    <div className="transfer-actions" role="tablist" aria-label="商品与库存模块"><button role="tab" aria-selected={mode==="transfer"} className={mode==="transfer" ? "primary" : ""} disabled={busy || stockBusy || editing} onClick={()=>setMode("transfer")}>商品搬家与记录</button><button role="tab" aria-selected={mode==="stock"} className={mode==="stock" ? "primary" : ""} disabled={busy || stockBusy || editing} onClick={()=>setMode("stock")}>导入后库存（独立模块）</button></div>
    {mode==="stock" && <PostImportStockPage shops={shops} records={records} onBusy={setStockBusy}/>}
    <div className="transfer-workspace" style={{display:mode==="transfer" ? "grid" : "none"}}>
    <section className="transfer-card"><div className="transfer-form">
      <label>源店铺<select value={source} disabled={busy} onChange={e => changeSource(e.target.value)}><option value="">选择源店铺</option>{available.map(s => <option key={s.id} value={s.id}>{s.name}</option>)}</select></label>
      <span className="transfer-arrow">→</span><label>目标店铺<select value={target} disabled={busy} onChange={e => { setTarget(e.target.value); setMessage(""); }}><option value="">选择目标店铺</option>{available.filter(s => s.id !== source).map(s => <option key={s.id} value={s.id}>{s.name}</option>)}</select></label>
      <label>目标货号前缀<input value={prefix} disabled={busy} onChange={e => setPrefix(e.target.value)} placeholder="留空保留原货号" maxLength={40}/></label>
      <label>售价倍率<input type="number" min="0.01" step="0.01" value={multiplier} disabled={busy} onChange={e => setMultiplier(e.target.value)}/></label>
      <label>目标账户币种<select value={currency} disabled={busy} onChange={e => setCurrency(e.target.value)}>{["CNY", "RUB", "USD", "EUR"].map(c => <option key={c}>{c}</option>)}</select></label>
    </div>
    <p className="transfer-note">默认跳过目标店已有货号，源店库存不复制。币种须与目标账户一致；与源商品币种不一致时预检会阻止发布。倍率和前缀只影响新生成的草稿，发布以草稿明细为准。</p>
    {available.length < 2 && <p className="transfer-warning">请先在店铺管理中添加至少两个跨境店，并分别配置 Seller API。</p>}
    <div className="transfer-actions"><button disabled={busy || !source} onClick={() => void read(false, true)}><RefreshCw size={15}/>读取首批商品</button><button disabled={busy || !source || (loaded && !cursor)} onClick={() => void read(true, !loaded)}>读取全部商品</button>{cursor && <button disabled={busy} onClick={() => void read(false)}>读取下一批</button>}<button className="primary" disabled={busy || !eligible || !selectedOffers.length || !recordsLoaded || !(Number(multiplier) > 0)} onClick={() => void prepare()}>预检所选 {selectedOffers.length} 件</button>{busy && <button onClick={() => { stop.current = true; setMessage("将在当前商品处理完成后暂停，进度已保存。"); }}><Pause size={15}/>暂停</button>}</div>
    </section>
    {message && <div className="transfer-message" role="status">{message}</div>}
    <section className="transfer-card"><div className="transfer-section"><h2>源店商品 <small>{items.length} / {total} 件</small></h2><label className="transfer-search"><Search size={16}/><input placeholder="搜索货号或标题" value={query} onChange={e => setQuery(e.target.value)}/></label></div>
      <div className="transfer-actions transfer-filter"><label>重复商品筛选 <select aria-label="重复商品筛选" disabled={busy} value={excludeScope} onChange={e => { setExcludeScope(e.target.value); setSelected(new Set()); setConfirm(false); }}><option value="target">剔除已搬到当前目标店的商品</option><option value="any">剔除已搬到任意店铺的商品</option><option value="none">不过滤，显示全部</option></select></label><span>已读取 {items.length} 件 · 剔除 {excludedCount} 件 · 显示 {visible.length} 件</span></div><p>处理中、待核对及目标已存在也计入剔除；失败和仅预检草稿可重试。更换目标货号前缀仍按源货号识别。</p>
      <div className="transfer-table"><table><thead><tr><th><input aria-label="选择当前筛选的全部商品" type="checkbox" disabled={busy || !visible.length} checked={!!visible.length && visible.every(p => selected.has(p.offer_id))} onChange={e => { const checked = e.target.checked; setSelected(old => {const next = new Set(old); visible.forEach(p => checked ? next.add(p.offer_id) : next.delete(p.offer_id)); return next;}); }}/></th><th>图片</th><th>商品 / 货号</th><th>源售价</th><th>已搬到的店铺 / 状态</th></tr></thead><tbody>{visible.map(p => <tr key={p.offer_id}><td><input aria-label={`选择 ${p.offer_id}`} disabled={busy} type="checkbox" checked={selected.has(p.offer_id)} onChange={() => toggle(p.offer_id)}/></td><td>{(p.primary_image?.[0] || p.images?.[0]) ? <img loading="lazy" src={p.primary_image?.[0] || p.images?.[0]} alt="商品"/> : "—"}</td><td><b>{p.name}</b><small>{p.offer_id}</small></td><td>{p.price} {p.currency_code}</td><td>{productRecords.get(p.offer_id)?.map(r => <small key={r.id}>{shopName(r.target)} · {labels[r.status]} · {r.offer}</small>) ?? <small>暂无本机搬家记录</small>}</td></tr>)}</tbody></table>{!items.length && <div className="transfer-empty">选择源店并读取商品。这里只展示实际 API 返回的数据。</div>}</div>
    </section>
    <section className="transfer-card"><div className="transfer-section"><div><h2>迁移草稿与发布结果 <small>{history.length} 件 · 可发布 {ready.length} 件</small></h2><p>记录保存在本机，重新打开可继续。平台拒绝的商品可重新选择并预检；待核对任务禁止重复提交。</p></div><div className="transfer-actions"><button disabled={busy || !history.length} onClick={() => void refresh()}><RefreshCw size={15}/>刷新平台结果</button><button className="primary" disabled={busy || !ready.length || !eligible} onClick={() => setConfirm(true)}>发布预检通过的 {ready.length} 件</button></div></div>
      <div className="transfer-table"><table><thead><tr><th>商品</th><th>目标货号</th><th>草稿价格</th><th>仓库 / 库存</th><th>状态 / 原因</th><th>详情</th></tr></thead><tbody>{history.map(r => <tr key={r.id}><td><b>{r.name}</b><small>来源：{r.sourceOffer}</small></td><td>{r.offer}</td><td>{r.price} {r.currency}</td><td><small>{stockSummary(r)}</small>{r.stockMessage && <small className="transfer-error">{r.stockMessage}</small>}<button disabled={busy} onClick={()=>setMode("stock")}>进入导入后库存</button></td><td><span className={`transfer-status ${r.status}`}>{labels[r.status] ?? r.status}</span>{r.issues.map((e, i) => <small className="transfer-error" key={i}>{e}</small>)}{r.taskId && <small>任务 {r.taskId} {r.targetProductId ? `· 商品 ${r.targetProductId}` : ""}</small>}</td><td><button disabled={busy} onClick={() => void openDetail(r)}>补充资料 / AI</button></td></tr>)}</tbody></table>{!history.length && <div className="transfer-empty">勾选商品并预检后，在这里查看迁移内容和发布结果。</div>}</div>
    </section>
    <section className="transfer-card"><div className="transfer-section"><div><h2>各店铺搬家记录 <small>{recordRows.length} 条</small></h2><p>本机记录：源店 → 目标店、商品、货号、状态和时间。导入完成不等于审核通过；目标已有仅表示发现同货号。未通过本软件搬家的商品不在此记录中。</p></div><div className="transfer-actions"><button disabled={busy} onClick={() => void refreshRecords()}>刷新记录</button><button disabled={!recordRows.length} onClick={exportRecords}>导出筛选记录 CSV</button></div></div>
      <div className="transfer-form transfer-filter"><label>源店筛选<select value={recordSource} onChange={e => setRecordSource(e.target.value)}><option value="">全部源店</option>{shops.map(s => <option key={s.id} value={s.id}>{s.name}</option>)}</select></label><label>目标店筛选<select value={recordTarget} onChange={e => setRecordTarget(e.target.value)}><option value="">全部目标店</option>{shops.map(s => <option key={s.id} value={s.id}>{s.name}</option>)}</select></label><label>状态筛选<select value={recordStatus} onChange={e => setRecordStatus(e.target.value)}><option value="all">全部状态</option><option value="recorded">已搬 / 处理中 / 已存在 / 待核对</option>{Object.entries(labels).map(([key, label]) => <option key={key} value={key}>{label}</option>)}</select></label><label>搜索记录<input placeholder="商品标题、源货号或目标货号" value={recordQuery} onChange={e => setRecordQuery(e.target.value)}/></label></div>
      <div className="transfer-table"><table><thead><tr><th>源店 → 目标店</th><th>商品 / 源货号</th><th>目标货号</th><th>状态</th><th>仓库 / 库存</th><th>更新时间</th></tr></thead><tbody>{recordRows.map(r => <tr key={r.id}><td>{shopName(r.source)} → {shopName(r.target)}</td><td><b>{r.name}</b><small>{r.sourceOffer}</small></td><td>{r.offer}</td><td><span className={`transfer-status ${r.status}`}>{labels[r.status] ?? r.status}</span></td><td><small>{stockSummary(r)}</small><small>{r.stockMessage}</small></td><td>{new Date(r.updatedAt).toLocaleString()}</td></tr>)}</tbody></table>{!recordRows.length && <div className="transfer-empty">暂无符合条件的本机搬家记录</div>}</div></section>
    {confirm && <div className="transfer-overlay"><section className="transfer-dialog"><h2>发布 {ready.length} 件商品到目标店</h2><p>目标店：<b>{available.find(s => s.id === target)?.name}</b></p><p>仅发布“预检通过”的已保存草稿，目标已有货号会再次核对。此操作会在目标店创建商品。库存请在产品创建完成后进入“导入后库存”模块独立设置。旧草稿库存设置不会自动执行。</p><div className="transfer-actions"><button onClick={() => setConfirm(false)}>返回检查</button><button className="primary" onClick={() => void submit()}>确认发布 {ready.length} 件</button></div></section></div>}
    {detail && <div className="transfer-overlay"><section className="transfer-dialog wide"><h2>{detail.name}</h2><p>{labels[detail.status]} · {detail.price} {detail.currency} · 更新于 {detail.updatedAt}</p><div className="transfer-images">{detail.images.map(url => <a key={url} href={url} target="_blank" rel="noreferrer"><img src={url} alt="迁移图片"/></a>)}</div>{detail.warnings.map(w => <p key={w} className="transfer-warning">{w}</p>)}<h2>补充必填属性</h2><p>字典属性从平台选项中选择（最多显示前 500 项）。AI 会读取此商品草稿与属性，给出建议；请核实后填写并保存。已有属性不修改时会保留。</p>
      {detail.issues.map((issue, i) => <p className="transfer-warning" key={i}>{issue}</p>)}
      <div className="transfer-field-grid">{fields.map(f => <label key={f.id}><b>{f.name} ({f.id})</b>{f.description && <small>{f.description}</small>}{f.dictionary_id ? <select disabled={editing || !!f.attribute_complex_id || !["ready", "blocked"].includes(detail.status)} value={edits[f.id] ?? "__keep"} onChange={e => setEdits(old => ({...old, [f.id]: e.target.value}))}><option value="__keep" disabled>保留现有值 / 请选择</option><option value="">清空此属性</option>{f.options?.map(o => <option key={o.id} value={String(o.id)}>{o.value} ({o.id})</option>)}</select> : <input disabled={editing || !!f.attribute_complex_id || !["ready", "blocked"].includes(detail.status)} placeholder="填写属性值；未编辑则保留原值" value={edits[f.id] ?? ""} onChange={e => setEdits(old => ({...old, [f.id]: e.target.value}))}/ >}{!!f.attribute_complex_id && <small>复合属性请在目标后台补充</small>}</label>)}</div>
      <div className="transfer-actions"><button disabled={editing} onClick={() => void editDetail("ai")}>{editing ? "处理中…" : "AI 分析缺失资料"}</button><button className="primary" disabled={editing || !Object.keys(edits).length || !["ready", "blocked"].includes(detail.status)} onClick={() => void editDetail("save")}>保存并重新预检</button><button disabled={editing} onClick={() => { detailVersion.current++; setDetail(null); }}>关闭详情</button></div>
      {detailError && <div role="status" className="transfer-message">{detailError}</div>}{aiHelp && <div className="transfer-message"><b>AI 建议（请人工核实）</b><div>{aiHelp}</div></div>}
      <details><summary>原始草稿 / 当前属性</summary><pre>{JSON.stringify(detail.payload, null, 2)}</pre></details></section></div>}
    </div>
  </div>;
}
