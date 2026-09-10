import { useEffect, useMemo, useRef, useState } from "react";
import { CloudUpload, FileSpreadsheet, FileText, PackagePlus, Plus, Save, Trash2 } from "lucide-react";
import { deletePackingDraft, exportPackingDocuments, packingDrafts, savePackingDraft, uploadPackingDocumentsToFeishu, type PackingDraft, type PackingExportInput, type PackingExportResult, type PackingItemInput } from "./bridge";
import type { InventoryRow } from "./types";
import "./packing-documents.css";

const day = () => new Date().toISOString().slice(0, 10);
const blankItem = (): PackingItemInput => ({ sku: "", productName: "", barcodeLabel: "", battery: "否", cartons: 1, quantityPerCarton: 1, weightKg: 1, lengthCm: 40, widthCm: 30, heightCm: 30, remark: "" });
const savedExport = () => { try { return JSON.parse(localStorage.getItem("packing-last-export") || "null") as { batchCode: string; result: PackingExportResult } | null; } catch { return null; } };

export function PackingDocumentsPage({ inventory, shopName }: { inventory: InventoryRow[]; shopName: string }) {
  const [batchCode, setBatchCode] = useState(`CZ-OZON-${day().slice(5).replace("-", "")}`);
  const [title, setTitle] = useState("补货产品发货装箱单");
  const [platform, setPlatform] = useState("Ozon");
  const [shipDate, setShipDate] = useState(day());
  const [notice, setNotice] = useState("");
  const [items, setItems] = useState<PackingItemInput[]>([blankItem()]);
  const [drafts, setDrafts] = useState<PackingDraft[]>([]), [draftId, setDraftId] = useState<number | null>(null), [draftName, setDraftName] = useState("新补货装箱单");
  const [lastExport, setLastExport] = useState<PackingExportResult | null>(null), [uploading, setUploading] = useState(false);
  const [busy, setBusy] = useState(false), [message, setMessage] = useState(""), [error, setError] = useState("");
  const autosaveReady = useRef(false);
  const products = useMemo(() => Array.from(new Map(inventory.map((x) => [x.sku, x])).values()), [inventory]);
  const cartons = items.reduce((sum, x) => sum + Math.max(0, Number(x.cartons) || 0), 0);
  const quantity = items.reduce((sum, x) => sum + Math.max(0, Number(x.cartons) || 0) * Math.max(0, Number(x.quantityPerCarton) || 0), 0);
  const update = (index: number, patch: Partial<PackingItemInput>) => setItems((old) => old.map((x, i) => i === index ? { ...x, ...patch } : x));
  const payload = useMemo<PackingExportInput>(() => ({ batchCode, title, platform, shopName, shippingDate: shipDate, notice, items }), [batchCode, title, platform, shopName, shipDate, notice, items]);
  const refreshDrafts = () => void packingDrafts().then(setDrafts).catch((e) => setError(String(e)));
  useEffect(() => { refreshDrafts(); const recent=savedExport(); if (recent) setLastExport(recent.result); const timer = window.setTimeout(() => { autosaveReady.current = true; }, 500); return () => window.clearTimeout(timer); }, []);
  useEffect(() => { if (!autosaveReady.current) return; const timer = window.setTimeout(async () => { try { const id = await savePackingDraft(draftId, draftName, payload); if (draftId == null) setDraftId(id); refreshDrafts(); } catch (e) { setError(`自动保存失败：${String(e)}`); } }, 900); return () => window.clearTimeout(timer); }, [payload, draftName, draftId]);
  const loadDraft = (draft: PackingDraft) => { autosaveReady.current = false; setDraftId(draft.id); setDraftName(draft.name); setBatchCode(draft.payload.batchCode); setTitle(draft.payload.title); setPlatform(draft.payload.platform); setShipDate(draft.payload.shippingDate); setNotice(draft.payload.notice); setItems(draft.payload.items.length ? draft.payload.items : [blankItem()]); const recent=savedExport(); setLastExport(recent?.batchCode===draft.payload.batchCode ? recent.result : null); setMessage(`已载入草稿：${draft.name}`); window.setTimeout(() => { autosaveReady.current = true; }, 500); };
  const newDraft = () => { autosaveReady.current = false; setDraftId(null); setDraftName("新补货装箱单"); setBatchCode(`CZ-OZON-${day().slice(5).replace("-", "")}`); setTitle("补货产品发货装箱单"); setPlatform("Ozon"); setShipDate(day()); setNotice(""); setItems([blankItem()]); setLastExport(null); setMessage("已新建草稿"); window.setTimeout(() => { autosaveReady.current = true; }, 500); };
  return <>
    <header className="page-header"><div><span className="eyebrow">REPLENISHMENT PACKING</span><h1>补货装箱与箱唛</h1><p>设置补货产品和每箱参数，一次生成发货装箱单与逐箱箱唛</p></div></header>
    {error && <div className="error-banner">{error}</div>}{message && <div className="packing-success">{message}</div>}
    <section className="card packing-drafts"><div className="card-title">草稿缓存<span>修改后约 1 秒自动保存到本机</span></div><div className="packing-draft-toolbar"><label>草稿名称<input value={draftName} onChange={(e) => setDraftName(e.target.value)} /></label><label>载入历史草稿<select value={draftId ?? ""} onChange={(e) => { const draft = drafts.find((x) => x.id === Number(e.target.value)); if (draft) loadDraft(draft); }}><option value="">当前新草稿</option>{drafts.map((draft) => <option key={draft.id} value={draft.id}>{draft.name} · {draft.updatedAt}</option>)}</select></label><button className="outline-button" onClick={newDraft}><Plus size={15}/>新建</button><button className="outline-button" onClick={async () => { const id = await savePackingDraft(draftId, draftName, payload); setDraftId(id); refreshDrafts(); setMessage("草稿已保存"); }}><Save size={15}/>立即保存</button><button className="draft-delete" disabled={draftId == null} onClick={async () => { if (draftId == null || !confirm(`删除草稿“${draftName}”？`)) return; await deletePackingDraft(draftId); newDraft(); refreshDrafts(); }}><Trash2 size={15}/>删除</button></div></section>
    <section className="card packing-settings">
      <div className="card-title">批次信息<span>箱唛格式参考 CZ7046-OZON-0909-01</span></div>
      <div className="packing-form-grid">
        <label>批次号<input value={batchCode} onChange={(e) => setBatchCode(e.target.value.toUpperCase())} placeholder="CZ7046-OZON-0909" /></label>
        <label>装箱单标题<input value={title} onChange={(e) => setTitle(e.target.value)} /></label>
        <label>平台<input value={platform} onChange={(e) => setPlatform(e.target.value)} /></label>
        <label>店名<input value={shopName} readOnly /></label>
        <label>出货日期<input type="date" value={shipDate} onChange={(e) => setShipDate(e.target.value)} /></label>
        <label className="wide">注意事项<input value={notice} onChange={(e) => setNotice(e.target.value)} placeholder="可留空" /></label>
      </div>
    </section>
    <section className="card packing-products">
      <div className="card-title">补货产品<span>同一产品的箱数会自动展开为逐箱明细</span><button className="outline-button" onClick={() => setItems((old) => [...old, blankItem()])}><Plus size={15}/>添加产品</button></div>
      <div className="packing-product-list">{items.map((item, index) => <article className="packing-product" key={index}>
        <div className="packing-product-head"><b>产品 {index + 1}</b>{items.length > 1 && <button title="删除" onClick={() => setItems((old) => old.filter((_, i) => i !== index))}><Trash2 size={16}/></button>}</div>
        <div className="packing-form-grid product-fields">
          <label className="wide">库存商品<select value={item.sku} onChange={(e) => { const p = products.find((x) => x.sku === e.target.value); update(index, { sku: e.target.value, productName: p?.productName || "", barcodeLabel: p?.offerId || e.target.value }); }}><option value="">手动填写 / 请选择</option>{products.map((p) => <option key={p.sku} value={p.sku}>{p.offerId || p.sku} · {p.productName}</option>)}</select></label>
          <label>商品名称<input value={item.productName} onChange={(e) => update(index, { productName: e.target.value })}/></label>
          <label>条码标签<input value={item.barcodeLabel} onChange={(e) => update(index, { barcodeLabel: e.target.value })}/></label>
          <label>箱数<input type="number" min="1" value={item.cartons} onChange={(e) => update(index, { cartons: Number(e.target.value) })}/></label>
          <label>单箱数量<input type="number" min="1" value={item.quantityPerCarton} onChange={(e) => update(index, { quantityPerCarton: Number(e.target.value) })}/></label>
          <label>单箱重量 KG<input type="number" min="0.01" step="0.01" value={item.weightKg} onChange={(e) => update(index, { weightKg: Number(e.target.value) })}/></label>
          <label>长 CM<input type="number" min="0.1" step="0.1" value={item.lengthCm} onChange={(e) => update(index, { lengthCm: Number(e.target.value) })}/></label>
          <label>宽 CM<input type="number" min="0.1" step="0.1" value={item.widthCm} onChange={(e) => update(index, { widthCm: Number(e.target.value) })}/></label>
          <label>高 CM<input type="number" min="0.1" step="0.1" value={item.heightCm} onChange={(e) => update(index, { heightCm: Number(e.target.value) })}/></label>
          <label>是否带电<select value={item.battery} onChange={(e) => update(index, { battery: e.target.value })}><option>否</option><option>是</option></select></label>
          <label className="wide">备注<input value={item.remark} onChange={(e) => update(index, { remark: e.target.value })} placeholder="颜色、款式等"/></label>
        </div>
      </article>)}</div>
    </section>
    <section className="card packing-export"><div><PackagePlus size={22}/><p><b>{items.length} 个产品 · {cartons} 箱 · {quantity} 件</b><span>{lastExport ? "已记住最近生成的文件，可直接上传飞书" : "装箱单为 Excel，箱唛为 7×5 英寸横向 PDF，每箱一页"}</span></p></div><div className="packing-export-actions"><button className="dark-button" disabled={busy || !cartons} onClick={async () => { setBusy(true); setError(""); setMessage(""); try { const savedId = await savePackingDraft(draftId, draftName, payload); setDraftId(savedId); const result = await exportPackingDocuments(payload); setLastExport(result); localStorage.setItem("packing-last-export", JSON.stringify({ batchCode, result })); refreshDrafts(); setMessage(`已生成 ${result.cartonCount} 页箱唛和装箱单，共 ${result.totalQuantity} 件。文件夹已打开。`); } catch (e) { setError(String(e)); } finally { setBusy(false); } }}><FileSpreadsheet size={16}/><FileText size={16}/>{busy ? "正在生成…" : "生成装箱单和箱唛"}</button><button className="feishu-upload" disabled={uploading} onClick={async () => { if (!lastExport) { setError("尚未找到可上传的文件，请先点击“生成装箱单和箱唛”"); return; } setUploading(true); setError(""); setMessage("正在连接飞书并上传 Excel 与 PDF，请稍候…"); try { await uploadPackingDocumentsToFeishu(lastExport.xlsxPath, lastExport.pdfPath); setMessage("Excel 装箱单和 PDF 箱唛已上传到飞书“发货装箱单”文件夹"); } catch (e) { setMessage(""); setError(`飞书上传失败：${String(e)}`); } finally { setUploading(false); } }}><CloudUpload size={16}/>{uploading ? "上传中…" : "上传到飞书"}</button></div></section>
  </>;
}
