import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type Card = {
  nmID: number;
  vendorCode: string;
  title?: string;
  description?: string;
  brand?: string;
  photos?: { big: string }[];
  updatedAt: string;
};
type Cursor = { nmID: number; updatedAt: string };
type Page = { cards: Card[]; cursor?: Cursor };
const imageLink = (url: string) => /^https:\/\/[^\s]+\.(jpe?g|png|webp|gif|bmp)([?#][^\s]*)?$/i.test(url);

export function WbProductEditor({ shopId }: { shopId: string }) {
  const [query, setQuery] = useState("");
  const [pageQuery, setPageQuery] = useState("");
  const [page, setPage] = useState<Page>({ cards: [] });
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

  async function load(search: string, cursor?: Cursor) {
    setBusy(true);
    setError("");
    try {
      const result = await call<Page>("product_cards", { query: search, cursor });
      setPage(result);
      setPageQuery(search);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => { void load(""); }, [shopId]);

  function setDraft(result: Card) {
    setCard(result);
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
      <div className="wbo-heading"><h2>商品列表</h2><small>需要具有内容权限的 WB Token</small></div>
      <form className="wbo-actions" onSubmit={event => { event.preventDefault(); void load(query.trim()); }}>
        <input aria-label="搜索商品" placeholder="输入 WB 商品 ID、货号或条码" value={query} onChange={event => setQuery(event.target.value)}/>
        <button disabled={busy}>搜索</button>
      </form>
      {busy && <p role="status">正在读取商品…</p>}
      <div className="wbo-table-wrap"><table>
        <thead><tr><th>商品</th><th>WB 商品 ID</th><th>货号</th><th>品牌</th><th>操作</th></tr></thead>
        <tbody>{page.cards.map(row => <tr key={row.nmID}>
          <td><div className="wbo-product-cell">{row.photos?.[0]?.big && <img src={row.photos[0].big} alt="" loading="lazy" referrerPolicy="no-referrer"/>}<b>{row.title || row.vendorCode}</b></div></td><td>{row.nmID}</td><td>{row.vendorCode}</td><td>{row.brand || "—"}</td>
          <td><button disabled={busy} onClick={() => void open(row.nmID)}>编辑商品</button></td>
        </tr>)}</tbody>
      </table></div>
      {!busy && !page.cards.length && <p>没有匹配的商品。</p>}
      <div className="wbo-actions">
        <button disabled={busy} onClick={() => void load(pageQuery)}>返回首页</button>
        <button disabled={busy || !page.cursor?.updatedAt || page.cards.length < 50}
          onClick={() => void load(pageQuery, page.cursor)}>下一页</button>
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
