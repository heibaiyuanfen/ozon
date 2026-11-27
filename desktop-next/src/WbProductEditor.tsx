import { useEffect, useState } from "react";
import { readWbData, wbCostFields, wbProductCostKey, type WbProductCost } from "./wbProductCosts";
import { invoke } from "@tauri-apps/api/core";

type Card = {
  nmID: number;
  vendorCode: string;
  title?: string;
  description?: string;
  brand?: string;
  photos?: { big: string }[];
  updatedAt: string;
  sizes?: { skus?: string[] }[];
};
type Catalog = { cards: Card[]; updatedAt: string };
const imageLink = (url: string) => /^https:\/\/[^\s]+\.(jpe?g|png|webp|gif|bmp)([?#][^\s]*)?$/i.test(url);

export function WbProductEditor({ shopId }: { shopId: string }) {
  const [query, setQuery] = useState("");
  const [pageQuery, setPageQuery] = useState("");
  const [catalog,setCatalog] = useState<Catalog>({cards:[],updatedAt:""});
  const [page,setPage] = useState(0);
  const [cost,setCost] = useState<WbProductCost>({});
  const [prefix,setPrefix] = useState("");
  const [showMatches,setShowMatches] = useState(false);
  const costKey=wbProductCostKey(shopId);
  const results=catalog.cards.filter(item=>[item.nmID,item.vendorCode,item.title,...(item.sizes?.flatMap(size=>size.skus ?? []) ?? [])].join(" ").toLowerCase().includes(pageQuery.toLowerCase()));
  const cards=results.slice(page*50,page*50+50);
  const normalizedPrefix=prefix.trim().toLowerCase();
  const matches=normalizedPrefix ? catalog.cards.filter(item=>item.vendorCode?.trim().toLowerCase().startsWith(normalizedPrefix)) : [];
  const [card, setCard] = useState<Card | null>(null);
  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  const [photos, setPhotos] = useState<string[]>([]);
  const [newUrl, setNewUrl] = useState("");
  const [preview, setPreview] = useState("");
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  const call = <T,>(action: string, payload?: object) =>
    invoke<T>("wb_operations", { action, payload: { shopId, ...payload } });

  async function load(refresh = false) {
    setBusy(true);
    setError("");
    try {
      const result = await call<Catalog>(refresh ? "product_catalog_refresh" : "product_catalog");
      setCatalog(result);
      setPage(0);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => { void load(); }, [shopId]);

  function setDraft(result: Card) {
    setCard(result);
    setPrefix(result.vendorCode?.split("-")[0] ?? "");
    setShowMatches(false);
    setCost(readWbData<WbProductCost>(costKey)[String(result.nmID)] ?? {});
    setTitle(result.title ?? "");
    setDescription(result.description ?? "");
    setPhotos(result.photos?.map(photo => photo.big) ?? []);
    setNewUrl("");
  }

  async function open(nmId: number) {
    setBusy(true);
    setError("");
    setNotice("");
    try {
      setDraft(await call<Card>("product_detail", { nmId }));
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  }

  const seriesValues = wbCostFields.flatMap(([field,label,unit])=>{
    const value=cost[field]?.trim();
    return value ? [{field,label,unit,value}] : [];
  });
  const validSeriesValues=seriesValues.length>0 && seriesValues.every(({value})=>Number.isFinite(Number(value)) && Number(value)>=0);
  function copySeriesCost() {
    if (!validSeriesValues || !matches.length || !showMatches) return;
    const details=seriesValues.map(({label,unit,value})=>label+" "+value+" "+unit).join("、");
    if (!window.confirm("向 "+matches.length+" 个货号以「"+prefix.trim()+"」开头的商品同步："+details+"。确认覆盖这些字段？")) return;
    const patch=Object.fromEntries(seriesValues.map(({field,value})=>[field,value]));
    setError("");
    setNotice("");
    try {
      const current=readWbData<WbProductCost>(costKey);
      for (const item of matches) current[item.nmID]={...current[item.nmID],...patch};
      localStorage.setItem(costKey,JSON.stringify(current));
      setNotice("已同步 " + matches.length + " 个商品的成本与规格。");
      setShowMatches(false);
    } catch {setError("无法保存系列成本");}
  }

  const originalPhotos = card?.photos?.map(photo => photo.big) ?? [];
  const changedPhotos = JSON.stringify(photos) !== JSON.stringify(originalPhotos);
  const changedText = title.trim() !== (card?.title ?? "") || description !== (card?.description ?? "");

  async function save(media: boolean) {
    if (!card || busy) return;
    const action = media ? "product_media" : "product_update";
    const target = media ? `${photos.length} 张图片及当前排序` : "标题和描述";
    if (!window.confirm(`提交商品 ${card.nmID} 的${target}到 Wildberries？`)) return;
    setBusy(true);
    setError("");
    setNotice("");
    const identity = { nmId: card.nmID, updatedAt: card.updatedAt };
    try {
      await call(action, media ? { ...identity, photos, originalPhotos } : { ...identity, title: title.trim(), description });
      setNotice("WB 已接收，请稍后重新读取商品，核对平台生效结果。");
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  }

  function move(index: number, direction: number) {
    const next = [...photos];
    [next[index], next[index + direction]] = [next[index + direction], next[index]];
    setPhotos(next);
  }

  function addPhoto() {
    const url = newUrl.trim();
    if (!imageLink(url) || photos.includes(url) || photos.length >= 30) return;
    setPhotos([...photos, url]);
    setNewUrl("");
  }

  return <>
    {error && <p className="wbo-error" role="alert">{error}</p>}
    {notice && <p role="status">{notice}</p>}
    {!card ? <section className="wbo-panel">
      <div className="wbo-heading"><h2>商品列表</h2><div className="wbo-actions"><small>{catalog.updatedAt ? "缓存更新 " + new Date(catalog.updatedAt).toLocaleString("zh-CN") : "首次读取需内容权限"}</small><button disabled={busy} onClick={()=>void load(true)}>更新商品</button></div></div>
      <form className="wbo-actions" onSubmit={event => { event.preventDefault(); setPageQuery(query.trim());setPage(0); }}>
        <input aria-label="搜索商品" placeholder="输入 WB 商品 ID、货号或条码" value={query} onChange={event => setQuery(event.target.value)}/>
        <button disabled={busy}>搜索</button>
      </form>
      {busy && <p role="status">正在读取商品…</p>}
      <div className="wbo-table-wrap"><table>
        <thead><tr><th>商品</th><th>WB 商品 ID</th><th>货号</th><th>品牌</th><th>操作</th></tr></thead>
        <tbody>{cards.map(row => <tr key={row.nmID}>
          <td><div className="wbo-product-cell">{row.photos?.[0]?.big && <img src={row.photos[0].big} alt="" loading="lazy" referrerPolicy="no-referrer"/>}<b>{row.title || row.vendorCode}</b></div></td><td>{row.nmID}</td><td>{row.vendorCode}</td><td>{row.brand || "—"}</td>
          <td><button disabled={busy} onClick={() => {setNotice("");setError("");setDraft(row);}}>编辑商品</button></td>
        </tr>)}</tbody>
      </table></div>
      {!busy && !cards.length && <p>没有匹配的商品。</p>}
      <div className="wbo-actions">
        <button onClick={() => setPage(Math.max(0,page-1))} disabled={busy || page===0}>上一页</button>
        <button disabled={busy || (page+1)*50>=results.length}
          onClick={() => setPage(page+1)}>下一页</button>
      </div>
    </section> : <>
      <section className="wbo-panel">
        <div className="wbo-heading"><div><h2>{card.title || card.vendorCode}</h2><small>WB {card.nmID} · {card.vendorCode}</small></div>
          <div className="wbo-actions">
            <button disabled={busy} onClick={() => void open(card.nmID)}>重新读取</button>
            <button disabled={busy} onClick={() => { setCard(null); setNotice(""); setError(""); }}>返回列表</button>
          </div>
        </div>
        <div className="wbo-product-fields">
          <label>商品标题<input disabled={busy} maxLength={60} value={title} onChange={e => setTitle(e.target.value)}/></label>
          <label>商品描述<textarea disabled={busy} value={description} onChange={e => setDescription(e.target.value)}/></label>
          <div className="wbo-actions"><button disabled={busy || !changedText || !title.trim()} onClick={() => void save(false)}>提交商品资料</button></div>
        </div>
      </section>
      <section className="wbo-panel">
        <div className="wbo-heading"><h2>经营成本与包装规格</h2></div>
        <form className="wbo-toolbar" onSubmit={event=>{
          event.preventDefault();
          try {
            const current=readWbData<WbProductCost>(costKey);
            localStorage.setItem(costKey,JSON.stringify({...current,[card.nmID]:cost}));
            setNotice("成本与规格已保存，经营周报会读取该商品成本。");
          } catch {setError("无法保存本地成本");}
        }}>
          {wbCostFields.map(([field,label,unit])=><label key={field}>{label}（{unit}）<input type="number" min="0" step="any"
            value={cost[field] ?? ""} onChange={event=>setCost({...cost,[field]:event.target.value})}/></label>)}
          <button className="primary">保存成本与规格</button>
        </form>
        <div className="wbo-toolbar">
          <label>系列货号前缀<input placeholder="例如 F5067" value={prefix} onChange={event=>{setPrefix(event.target.value);setShowMatches(false);}}/></label>
          <button type="button" disabled={!matches.length} onClick={()=>setShowMatches(value=>!value)}>{showMatches?"收起匹配":"查看匹配"}（{matches.length}）</button>
          <button type="button" className="primary" disabled={!showMatches || !matches.length || !validSeriesValues} onClick={copySeriesCost}>同步系列成本与规格</button>
        </div>
        {showMatches&&<ul className="wbo-series-cost-matches">{matches.map(item=><li key={item.nmID}>{item.vendorCode} · WB {item.nmID}</li>)}</ul>}
      </section>
      <section className="wbo-panel">
        <div className="wbo-heading"><h2>商品图片</h2><small>{photos.length} / 30</small></div>
        <p className="wbo-muted">第一张为主图。图片可通过 HTTPS 图片直链添加，支持排序和移除。</p>
        <div className="wbo-product-media">{photos.map((url, index) => <article key={`${index}:${url}`}>
          <button onClick={() => setPreview(url)} aria-label={`预览图片 ${index + 1}`}>
            <img src={url} alt={index === 0 ? "主图" : `商品图片 ${index + 1}`} loading="lazy" referrerPolicy="no-referrer"/>
          </button>
          <div className="wbo-actions">
            <button disabled={busy || index === 0} onClick={() => move(index, -1)} aria-label="前移">←</button>
            <button disabled={busy || index === photos.length - 1} onClick={() => move(index, 1)} aria-label="后移">→</button>
            <button disabled={busy} onClick={() => setPhotos(photos.filter((_, i) => index !== i))}>移除</button>
          </div>
        </article>)}</div>
        <div className="wbo-actions">
          <input aria-label="图片直链" placeholder="https://…/image.jpg" value={newUrl} onChange={e => setNewUrl(e.target.value)}/>
          <button disabled={busy || !imageLink(newUrl.trim()) || photos.includes(newUrl.trim()) || photos.length >= 30} onClick={addPhoto}>添加图片</button>
          <button disabled={busy || !changedPhotos || !photos.length} onClick={() => void save(true)}>提交图片变更</button>
        </div>
      </section>
      {preview && <div className="wbo-overlay" onClick={() => setPreview("")}>
        <section className="wbo-dialog wbo-product-preview" role="dialog" aria-modal="true" aria-label="预览商品图片"
          onClick={event => event.stopPropagation()} onKeyDown={event => { if (event.key === "Escape") setPreview(""); }}>
          <div className="wbo-heading"><h2>图片预览</h2><button autoFocus onClick={() => setPreview("")}>关闭</button></div>
          <img alt="商品原图" src={preview} referrerPolicy="no-referrer"/>
        </section>
      </div>}
    </>}
  </>;
}
