import { useEffect, useMemo, useState } from "react";
import { CalendarDays, ChevronDown, ChevronUp, PackageCheck, RefreshCw, Search, Tags } from "lucide-react";
import { ozonPromotionProductAction, ozonPromotionProducts, ozonPromotions } from "./bridge";
import type { OzonPromotion, OzonPromotionProductsData, OzonPromotionsData } from "./types";
import "./promotion-center.css";

type Filter = "available" | "participating" | "upcoming" | "completed";
type Detail = { mode: "participating" | "candidates"; data: OzonPromotionProductsData };

const dateValue = (value: string) => {
  const time = Date.parse(value);
  return Number.isFinite(time) ? time : 0;
};
const formatDate = (value: string) => value ? new Intl.DateTimeFormat("zh-CN", { year: "numeric", month: "2-digit", day: "2-digit" }).format(new Date(value)) : "—";
const statusOf = (item: OzonPromotion): Filter => {
  const now = Date.now(), start = dateValue(item.dateStart), end = dateValue(item.dateEnd);
  if (end && end < now) return "completed";
  if (start && start > now) return "upcoming";
  return item.isParticipating ? "participating" : "available";
};
const statusText: Record<Filter, string> = { available: "可参加", participating: "正在参与", upcoming: "即将开始", completed: "已结束" };

export function PromotionCenter({ shopId }: { shopId: string }) {
  const [data, setData] = useState<OzonPromotionsData>({ promotions: [], cachedAt: "", source: "cache" });
  const [filter, setFilter] = useState<Filter>("available");
  const [query, setQuery] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [openId, setOpenId] = useState<number | null>(null);
  const [details, setDetails] = useState<Record<number, Detail>>({});
  const [detailBusy, setDetailBusy] = useState<number | null>(null);
  const [actionBusy, setActionBusy] = useState("");
  const [edits, setEdits] = useState<Record<string, { price: string; stock: string }>>({});
  const [notice, setNotice] = useState("");

  const load = async (refresh: boolean) => {
    setBusy(true); setError("");
    try { setData(await ozonPromotions(refresh)); }
    catch (reason) { setError(String(reason)); }
    finally { setBusy(false); }
  };
  useEffect(() => { void load(false); }, [shopId]);

  const counts = useMemo(() => data.promotions.reduce((all, item) => {
    all[statusOf(item)] += 1; return all;
  }, { available: 0, participating: 0, upcoming: 0, completed: 0 } as Record<Filter, number>), [data.promotions]);
  const visible = useMemo(() => data.promotions.filter((item) => {
    const text = `${item.title} ${item.description} ${item.actionType} ${item.id}`.toLowerCase();
    return statusOf(item) === filter && text.includes(query.trim().toLowerCase());
  }).sort((a, b) => dateValue(a.dateEnd) - dateValue(b.dateEnd)), [data.promotions, filter, query]);

  const loadProducts = async (item: OzonPromotion, mode: Detail["mode"]) => {
    setOpenId(item.id); setDetailBusy(item.id); setError("");
    try {
      const productData = await ozonPromotionProducts(item.id, mode);
      setDetails((old) => ({ ...old, [item.id]: { mode, data: productData } }));
    }
    catch (reason) { setError(String(reason)); }
    finally { setDetailBusy(null); }
  };

  const changeEdit = (key: string, field: "price" | "stock", value: string) => setEdits((old) => ({ ...old, [key]: { price: old[key]?.price ?? "", stock: old[key]?.stock ?? "", [field]: value } }));
  const submitProduct = async (item: OzonPromotion, productId: number, mode: Detail["mode"], defaults: { price: number; stock: number }, remove = false) => {
    const key = `${item.id}:${productId}`;
    const price = Number(edits[key]?.price ?? defaults.price), stock = Number(edits[key]?.stock ?? defaults.stock);
    if (!remove && (!(price > 0) || !Number.isInteger(stock) || stock < 0)) { setError("请填写大于 0 的活动价，以及不小于 0 的整数活动数量。"); return; }
    const question = remove ? `确认将商品 ${productId} 移出“${item.title}”吗？` : `确认提交商品 ${productId}：活动价 ${price} ₽、活动数量 ${stock}？`;
    if (!window.confirm(question)) return;
    setActionBusy(key); setError(""); setNotice("");
    try {
      const result = await ozonPromotionProductAction({ actionId: item.id, action: remove ? "deactivate" : "activate", productId, actionPrice: remove ? undefined : price, stock: remove ? undefined : stock });
      if (!result.success) throw new Error(result.message);
      setNotice(result.message);
      const productData = await ozonPromotionProducts(item.id, mode);
      setDetails((old) => ({ ...old, [item.id]: { mode, data: productData } }));
      setData(await ozonPromotions(true));
    } catch (reason) { setError(String(reason)); }
    finally { setActionBusy(""); }
  };

  return <div className="promotion-center">
    <header className="promotion-header">
      <div><span>OZON SELLER PROMOTIONS</span><h1>广告促销</h1><p>读取当前店铺可参加、正在参与和已结束的 Ozon 促销；每项促销独立管理。</p></div>
      <button className="dark-button" disabled={busy} onClick={() => void load(true)}><RefreshCw className={busy ? "spin" : ""} size={16}/>{busy ? "正在读取" : "刷新 Ozon 促销"}</button>
    </header>
    <section className="promotion-summary">
      <article><Tags/><span>全部促销<b>{data.promotions.length}</b></span></article>
      <article><PackageCheck/><span>正在参与<b>{counts.participating}</b></span></article>
      <article><CalendarDays/><span>即将开始<b>{counts.upcoming}</b></span></article>
      <div><small>{data.cachedAt ? `最近读取：${data.cachedAt} · ${data.source === "api" ? "Ozon API" : "本地缓存"}` : "尚未读取促销"}</small></div>
    </section>
    {error && <div className="promotion-error">{error}</div>}
    {notice && <div className="promotion-notice">{notice}</div>}
    <section className="promotion-toolbar">
      <label><Search size={16}/><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="搜索促销名称、类型或 ID"/></label>
      <nav>{(["available", "participating", "upcoming", "completed"] as Filter[]).map((key) => <button key={key} className={filter === key ? "active" : ""} onClick={() => setFilter(key)}>{statusText[key]} <b>{counts[key]}</b></button>)}</nav>
    </section>
    <section className="promotion-grid">
      {visible.map((item) => {
        const status = statusOf(item), open = openId === item.id, detail = details[item.id];
        return <article className={`promotion-card status-${status}`} key={item.id}>
          <header><div><small>{formatDate(item.dateStart)} — {formatDate(item.dateEnd)} · {item.actionType || "Ozon 促销"}</small><h2>{item.title || `促销 ${item.id}`}</h2></div><span>{statusText[status]}</span></header>
          {item.description && <p>{item.description}</p>}
          <div className="promotion-card-metrics">
            <span><small>已参加商品</small><b>{item.participatingProductsCount}</b></span>
            <span><small>可添加商品</small><b>{item.potentialProductsCount}</b></span>
            <span><small>不可参加</small><b>{item.bannedProductsCount}</b></span>
            <span><small>优惠</small><b>{item.discountValue ? `${item.discountValue}${item.discountType?.includes("PERCENT") ? "%" : " ₽"}` : "按活动规则"}</b></span>
          </div>
          <footer>
            <span>ID {item.id}{item.freezeDate ? ` · 冻结 ${formatDate(item.freezeDate)}` : ""}{item.withTargeting ? " · 定向" : ""}</span>
            <div><button disabled={detailBusy === item.id} onClick={() => void loadProducts(item, "participating")}>参与商品</button><button disabled={detailBusy === item.id} onClick={() => void loadProducts(item, "candidates")}>候选商品</button><button className="promotion-expand" onClick={() => setOpenId(open ? null : item.id)}>{open ? <ChevronUp/> : <ChevronDown/>}</button></div>
          </footer>
          {open && <div className="promotion-detail">
            {detailBusy === item.id ? <div className="promotion-detail-empty">正在读取商品…</div> : detail ? <><div className="promotion-detail-head"><b>{detail.mode === "participating" ? "参与商品管理" : "候选商品管理"}</b><span>共 {detail.data.total} 件，本页 {detail.data.products.length} 件</span></div><div className="promotion-products">{detail.data.products.slice(0, 100).map((product) => {
              const key = `${item.id}:${product.id}`, defaultPrice = product.actionPrice || product.maxActionPrice || product.price, defaultStock = product.stock || product.minStock;
              return <span key={product.id}><b>商品 ID {product.id}</b><small>现价 {product.price || "—"} ₽ · 最高活动价 {product.maxActionPrice || "—"} ₽ · {product.addMode || "—"}</small><div className="promotion-product-edit"><label>活动价 ₽<input type="number" min="0.01" step="0.01" max={product.maxActionPrice || undefined} value={edits[key]?.price ?? String(defaultPrice || "")} onChange={(event) => changeEdit(key, "price", event.target.value)}/></label><label>活动数量<input type="number" min="0" step="1" value={edits[key]?.stock ?? String(defaultStock || 0)} onChange={(event) => changeEdit(key, "stock", event.target.value)}/></label></div><div className="promotion-product-actions"><button disabled={actionBusy === key} onClick={() => void submitProduct(item, product.id, detail.mode, { price: defaultPrice, stock: defaultStock })}>{detail.mode === "participating" ? "保存修改" : "加入活动"}</button>{detail.mode === "participating" && <button className="danger" disabled={actionBusy === key} onClick={() => void submitProduct(item, product.id, detail.mode, { price: defaultPrice, stock: defaultStock }, true)}>移出活动</button>}</div></span>;
            })}</div>{detail.data.products.length > 100 && <small className="promotion-limit">为保证界面流畅，仅展示前 100 件。</small>}</> : <div className="promotion-detail-empty">点击“参与商品”或“候选商品”读取明细。</div>}
          </div>}
        </article>;
      })}
      {!busy && !visible.length && <div className="promotion-empty">当前筛选条件下没有促销活动。</div>}
    </section>
  </div>;
}
