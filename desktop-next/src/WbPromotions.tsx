import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { RefreshCw } from "lucide-react";
import { displayTime } from "./time";

type Promotion = {
  id: string;
  externalPromotionId: string;
  kind: string | null;
  name: string;
  startsAt: string | null;
  endsAt: string | null;
  lastSyncedAt: string | null;
};

const statusLabels = {
  all: "全部状态",
  upcoming: "即将开始",
  active: "进行中",
  ended: "已结束",
  unknown: "时间待确认",
};

function statusOf(row: Promotion): Exclude<keyof typeof statusLabels, "all"> {
  const start = Date.parse(row.startsAt ?? "");
  const end = Date.parse(row.endsAt ?? "");
  const now = Date.now();
  if (!Number.isFinite(start) || !Number.isFinite(end) || start > end) return "unknown";
  if (now >= end) return "ended";
  return now < start ? "upcoming" : "active";
}

export function WbPromotions({ shopId }: { shopId: string }) {
  const [selected, setSelected] = useState<Promotion | null>(null);
  const [rows, setRows] = useState<Promotion[]>([]);
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [filter, setFilter] = useState("all");
  const [search, setSearch] = useState("");

  async function load(sync: boolean) {
    setBusy(true);
    setError("");
    setNotice("");
    const payload = { shopId };
    try {
      const result = sync
        ? await invoke<{ records: number }>("price_center", { action: "sync_promotions", payload })
        : null;
      setRows(await invoke<Promotion[]>("price_center", { action: "list_promotions", payload }));
      if (result) setNotice(`本次同步 ${result.records} 项活动`);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => { void load(false); }, [shopId]);

  const lastSynced = rows.reduce((last, row) =>
    row.lastSyncedAt && row.lastSyncedAt > last ? row.lastSyncedAt : last, "");
  const query = search.trim().toLowerCase();
  const visible = rows.map(row => ({ ...row, status: statusOf(row) })).filter(row =>
    (filter === "all" || row.status === filter) &&
    (row.name.toLowerCase().includes(query) || row.externalPromotionId.includes(query)));

  return <section className="wbo-panel">
    <div className="wbo-heading">
      <h2>活动列表</h2>
      <div className="wbo-actions">
        <button disabled={busy} onClick={() => void load(true)}>
          <RefreshCw size={16}/>同步 WB 促销
        </button>
      </div>
    </div>
    <p className="wbo-muted">同步范围为过去 30 天至未来 180 天。店铺 Token 需要「价格与折扣」权限。</p>
    {lastSynced && <p className="wbo-muted">最近活动同步 {displayTime(lastSynced)}</p>}
    {busy && <p role="status">正在读取促销活动…</p>}
    {error && <p role="alert" className="wbo-error">{error}</p>}
    {notice && <p role="status">{notice}</p>}
    <div className="wbo-actions">
      <input aria-label="搜索活动" placeholder="搜索活动名称或 ID" value={search} onChange={e => setSearch(e.target.value)}/>
      <select aria-label="活动状态" value={filter} onChange={e => setFilter(e.target.value)}>
        {Object.entries(statusLabels).map(([key, label]) => <option key={key} value={key}>{label}</option>)}
      </select>
    </div>
    <div className="wbo-table-wrap">
      <table>
        <thead><tr><th>活动名称</th><th>ID</th><th>开始时间</th><th>结束时间</th><th>状态</th><th>商品管理</th></tr></thead>
        <tbody>{visible.map(row => <tr key={row.id}>
          <td>{row.name}</td><td>{row.externalPromotionId}</td><td>{displayTime(row.startsAt)}</td>
          <td>{displayTime(row.endsAt)}</td><td>{statusLabels[row.status]}</td><td><div className="wbo-actions"><button onClick={() => setSelected(row)}>管理商品</button></div></td>
        </tr>)}</tbody>
      </table>
    </div>
    {selected && <PromotionProducts key={selected.id} shopId={shopId} promotion={selected} onClose={() => setSelected(null)}/>}
    {!busy && !error && !visible.length && <p className="wbo-muted">{rows.length ? "没有符合筛选条件的活动。" : notice ? "WB 当前未返回活动。" : "暂无活动记录，请同步 WB 促销。"}</p>}
  </section>;
}

type Product = {
  id: number;
  price: number;
  planPrice: number;
  planDiscount: number;
  currencyCode: string;
};

function PromotionProducts({shopId, promotion, onClose}: {
  shopId: string;
  promotion: Promotion;
  onClose: () => void;
}) {
  const [view, setView] = useState({ participating: false, offset: 0 });
  const [rows, setRows] = useState<Product[]>([]);
  const [chosen, setChosen] = useState<number[]>([]);
  const [busy, setBusy] = useState(true);
  const [uploadNow, setUploadNow] = useState(true);
  const [preview, setPreview] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const regular = promotion.kind === "regular";
  const status = statusOf(promotion);
  const canSubmit = status === "active" || status === "upcoming";
  const promotionId = Number(promotion.externalPromotionId);

  useEffect(() => {
    if (!regular) return;
    let current = true;
    setBusy(true);
    setError("");
    setRows([]);
    setChosen([]);
    setPreview(false);
    invoke<Product[]>("price_center", {
      action: "promotion_products", payload: { shopId, promotionId, ...view },
    }).then(result => { if (current) setRows(result); })
      .catch(reason => { if (current) setError(String(reason)); })
      .finally(() => { if (current) setBusy(false); });
    return () => { current = false; };
  }, [shopId, promotionId, regular, view]);

  async function submit() {
    setPreview(false);
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const result = await invoke<{uploadId: number; alreadyExists: boolean}>("price_center", {
        action: "promotion_submit", payload: { shopId, promotionId, productIds: chosen, offset: view.offset, uploadNow: status === "active" || uploadNow,
          planPrices: Object.fromEntries(selectedRows.map(row => [row.id, row.planPrice])) },
      });
      setNotice(`${result.alreadyExists ? "已有相同任务" : "WB 已接收报名"} · 任务号 ${result.uploadId}。请刷新商品列表核对参与状态。`);
      setChosen([]);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  }

  const selectedRows = rows.filter(row => chosen.includes(row.id));
  return <section className="wbo-panel">
    <div className="wbo-heading">
      <h2>{promotion.name} · 商品管理</h2>
      <div className="wbo-actions"><button onClick={onClose}>关闭</button></div>
    </div>
    <p className="wbo-muted">移出商品或自定义活动价请在 WB 卖家后台的「商品与价格 → 促销日历」处理，完成后可回到这里刷新参与列表。</p>
    {!regular ? <p>{promotion.kind === "auto" ? "自动促销的商品名单在 WB 卖家后台管理。" : "请同步活动以确认类型，普通促销支持在 ERP 报名。"}</p> : <>
      <div className="wbo-actions">
        <button disabled={busy || !view.participating} onClick={() => setView({ participating: false, offset: 0 })}>可报名商品</button>
        <button disabled={busy || view.participating} onClick={() => setView({ participating: true, offset: 0 })}>已参与商品</button>
        <button disabled={busy} onClick={() => setView({ ...view })}>刷新商品</button>
      </div>
      {busy && <p role="status">正在读取或提交…</p>}
      {error && <p role="alert" className="wbo-error">{error}</p>}
      {notice && <p role="status">{notice}</p>}
      <div className="wbo-table-wrap">
        <table>
          <thead><tr><th>报名</th><th>WB 商品 ID</th><th>现价</th><th>WB 活动价</th><th>活动折扣</th></tr></thead>
          <tbody>{rows.map(row => <tr key={row.id}>
            <td>{!view.participating && canSubmit && <input type="checkbox" aria-label={`报名商品 ${row.id}`}
              checked={chosen.includes(row.id)} disabled={busy || preview || !Number.isFinite(row.planPrice) || row.planPrice <= 0}
              onChange={event => setChosen(old => event.target.checked ? [...old, row.id] : old.filter(id => id !== row.id))}/>}</td>
            <td>{row.id}</td><td>{row.price} {row.currencyCode}</td>
            <td>{row.planPrice} {row.currencyCode}</td><td>{row.planDiscount}%</td>
          </tr>)}</tbody>
        </table>
      </div>
      {!busy && !error && rows.length === 0 && <p>本页暂无商品。</p>}
      <div className="wbo-actions">
        <button disabled={busy || preview || view.offset === 0} onClick={() => setView({ ...view, offset: view.offset - 100 })}>上一页</button>
        <button disabled={busy || preview || rows.length < 100} onClick={() => setView({ ...view, offset: view.offset + 100 })}>下一页</button>
        {!view.participating && canSubmit && <button disabled={busy || preview || !chosen.length} onClick={() => setPreview(true)}>预览报名（{chosen.length}）</button>}
      </div>
      {preview && <section className="wbo-panel">
        <h3>确认按 WB 活动价报名</h3>
        <label>生效时间 <select value={status === "active" || uploadNow ? "now" : "start"} onChange={e => setUploadNow(e.target.value === "now")}>
          <option value="now">立即应用折扣</option>
          {status === "upcoming" && <option value="start">活动开始时应用折扣</option>}
        </select></label>
        <ul>{selectedRows.map(row => <li key={row.id}>商品 {row.id}，{row.price} → {row.planPrice} {row.currencyCode}</li>)}</ul>
        <div className="wbo-actions">
          <button onClick={() => setPreview(false)}>返回</button>
          <button onClick={() => void submit()}>确认提交报名</button>
        </div>
      </section>}
    </>}
  </section>;
}
