import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type Row = Record<string, any>;
const call = <T,>(action: string, payload: Row = {}) =>
  invoke<T>("product_master", { action, payload });

export function ProductDistributionPanel({
  shops,
  admin,
}: {
  shops: Row[];
  admin: boolean;
}) {
  const [rows, setRows] = useState<Row[]>([]);
  const [skus, setSkus] = useState<Row[]>([]);
  const [total, setTotal] = useState(0);
  const [query, setQuery] = useState("");
  const [shopId, setShopId] = useState("");
  const [status, setStatus] = useState("");
  const [page, setPage] = useState(0);
  const [revision, setRevision] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [draft, setDraft] = useState<Row | null>(null);

  useEffect(() => {
    const timer = setTimeout(() => {
      setBusy(true);
      Promise.all([
        call<Row>("list_shop_products", { query, shopId, status, page }),
        call<Row>("list_v2", { kind: "skus", query: "", status: "active" }),
      ])
        .then(([plans, products]) => {
          setRows(plans.rows || []);
          setTotal(plans.total || 0);
          setSkus(products.rows || []);
        })
        .catch((e) => setError(String(e)))
        .finally(() => setBusy(false));
    }, 180);
    return () => clearTimeout(timer);
  }, [query, shopId, status, page, revision]);

  const openDraft = () => {
    const first = skus[0]?.id || "";
    setDraft({
      skuId: first,
      targets: Object.fromEntries(shops.map((s) => [s.id, { enabled: false, offerId: "" }])),
    });
  };
  const save = async () => {
    if (!draft?.skuId) return setError("请选择产品 SKU");
    const targets = shops
      .filter((s) => draft.targets[s.id]?.enabled)
      .map((s) => ({ shopId: s.id, offerId: draft.targets[s.id].offerId.trim() }));
    if (!targets.length) return setError("请至少选择一个店铺");
    if (targets.some((x) => !x.offerId)) return setError("所选店铺都必须填写货号");
    setBusy(true);
    setError("");
    try {
      await call("upsert_shop_products", { skuId: draft.skuId, targets });
      setDraft(null);
      setNotice(`已为 ${targets.length} 个店铺生成铺货计划`);
      setRevision((x) => x + 1);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      {error && <div className="wb-center-error">{error}<button onClick={() => setError("")}>关闭</button></div>}
      {notice && <div className="pm-notice">{notice}</div>}
      <div className="sync-actions pm-filters">
        <input placeholder="SKU / 产品名 / 货号 / 店铺" value={query} onChange={(e) => { setQuery(e.target.value); setPage(0); }} />
        <select value={shopId} onChange={(e) => { setShopId(e.target.value); setPage(0); }}>
          <option value="">全部店铺</option>
          {shops.map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}
        </select>
        <select value={status} onChange={(e) => { setStatus(e.target.value); setPage(0); }}>
          <option value="">全部状态</option>
          <option value="ready">待上品</option>
          <option value="publishing">上品中</option>
          <option value="published">已上品</option>
          <option value="failed">失败</option>
        </select>
        <button className="primary" disabled={!admin || busy || !shops.length || !skus.length} onClick={openDraft}>一键铺货</button>
        <button onClick={() => setRevision((x) => x + 1)}>刷新</button>
      </div>
      <article className="wb-panel">
        <h2>本地铺货计划</h2>
        <p>同一产品可分配到多个店铺；每个店铺内货号唯一。当前只生成待上品记录，不会直接提交平台。</p>
        <table>
          <thead><tr><th>产品</th><th>店铺</th><th>店铺货号</th><th>资料摘要</th><th>状态</th><th>操作</th></tr></thead>
          <tbody>
            {rows.map((r) => <tr key={r.id}>
              <td><strong>{r.skuCode}</strong><small>{r.name}</small></td>
              <td>{r.shop}</td>
              <td><strong>{r.offerId}</strong></td>
              <td>采购成本 {r.purchaseCost || "未填"}<small>{r.metadata?.supplierUrl || "未填采购链接"} · {r.media?.length || 0} 张图</small></td>
              <td><span className="status">{r.status}</span>{r.lastError && <small>{r.lastError}</small>}</td>
              <td><button disabled={!admin || busy} onClick={async () => { if (!confirm(`移除 ${r.shop} / ${r.offerId} 的铺货计划？`)) return; try { await call("remove_shop_product", { id: r.id }); setRevision((x) => x + 1); } catch (e) { setError(String(e)); } }}>移除</button></td>
            </tr>)}
            {!rows.length && <tr><td className="pm-empty" colSpan={6}>{busy ? "正在加载…" : "暂无铺货计划，可点击“一键铺货”建立。"}</td></tr>}
          </tbody>
        </table>
        <div className="pm-pagination"><span>共 {total} 条 · 第 {page + 1} 页</span><button disabled={!page || busy} onClick={() => setPage(page - 1)}>上一页</button><button disabled={(page + 1) * 50 >= total || busy} onClick={() => setPage(page + 1)}>下一页</button></div>
      </article>
      {draft && <div className="wb-modal"><div className="pm-dialog pm-distribution-dialog">
        <h2>一次建品，分配多店铺</h2>
        <label>标准产品 SKU<select value={draft.skuId} onChange={(e) => setDraft({ ...draft, skuId: e.target.value })}>{skus.map((s) => <option key={s.id} value={s.id}>{s.code} · {s.name}</option>)}</select></label>
        <div className="pm-target-list">
          {shops.map((s) => { const target = draft.targets[s.id]; return <label className="pm-target" key={s.id}>
            <input type="checkbox" checked={target.enabled} onChange={(e) => setDraft({ ...draft, targets: { ...draft.targets, [s.id]: { ...target, enabled: e.target.checked } } })} />
            <span><strong>{s.name}</strong><small>{s.id}</small></span>
            <input placeholder="该店铺货号" disabled={!target.enabled} value={target.offerId} onChange={(e) => setDraft({ ...draft, targets: { ...draft.targets, [s.id]: { ...target, offerId: e.target.value } } })} />
          </label>; })}
        </div>
        <div className="sync-actions"><button onClick={() => setDraft(null)}>取消</button><button className="primary" disabled={busy} onClick={() => void save()}>生成铺货计划</button></div>
      </div></div>}
    </>
  );
}
