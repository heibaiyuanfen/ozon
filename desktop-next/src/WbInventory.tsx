import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type StockRow = {
  nmId: number;
  sizeId: number;
  warehouseId: number;
  warehouse: string;
  quantity: number;
  toCustomer: number | null;
  fromCustomer: number | null;
};
type StockSnapshot = { rows: StockRow[]; updatedAt: string };
type Snapshot = {
  wb?: StockSnapshot;
  seller?: StockSnapshot;
  sales?: { totals: Record<string, [number, number]>; through: string; updatedAt: string };
  vendorCodes?: Record<string, string>;
};
type Resource = "wb" | "seller" | "sales";
const sources = { wb: "WB 仓", seller: "卖家仓", sales: "历史销量" };
const pageSize = 50;
const format = (value: number | undefined, digits = 0) =>
  value === undefined ? "—" : value.toLocaleString("zh-CN", { maximumFractionDigits: digits });

export function WbInventory({ shopId }: { shopId: string }) {
  const [data, setData] = useState<Snapshot>({});
  const [busy, setBusy] = useState<Resource | "load" | null>("load");
  const [error, setError] = useState("");
  const [query, setQuery] = useState("");
  const [warehouse, setWarehouse] = useState("");
  const [view, setView] = useState<"stock" | "forecast">("stock");
  const [windowDays, setWindowDays] = useState(30);
  const [forecastDays, setForecastDays] = useState(14);
  const [page, setPage] = useState(0);

  useEffect(() => {
    let alive = true;
    void invoke<Snapshot>("wb_operations", { action: "inventory_list", payload: { shopId } })
      .then((snapshot) => { if (alive) setData(snapshot); })
      .catch((reason) => { if (alive) setError(String(reason)); })
      .finally(() => { if (alive) setBusy(null); });
    return () => { alive = false; };
  }, [shopId]);

  async function sync(resource: Resource) {
    setError("");
    setBusy(resource);
    try {
      setData(await invoke<Snapshot>("wb_operations", {
        action: "inventory_sync", payload: { shopId, resource },
      }));
      setPage(0);
    } catch (reason) {
      setError(`${sources[resource]}同步失败：${String(reason)}`);
    } finally {
      setBusy(null);
    }
  }

  const stock = useMemo(() => (["wb", "seller"] as const).flatMap((source) =>
    (data[source]?.rows ?? []).map((row) => ({
      ...row, source, key: `${source}:${row.warehouseId}`,
      label: `${sources[source]} · ${row.warehouse || row.warehouseId}`,
    }))), [data.wb, data.seller]);
  const warehouses = new Map(stock.map((row) => [row.key, row.label]));
  const search = query.trim().toLowerCase();
  const matches = (id: number) => String(id).includes(search) ||
    (data.vendorCodes?.[id] ?? "").toLowerCase().includes(search);
  const filtered = stock.filter((row) => matches(row.nmId) && (!warehouse || row.key === warehouse));
  const available = new Map<number, number>();
  for (const row of stock) available.set(row.nmId, (available.get(row.nmId) ?? 0) + row.quantity);
  const totals = data.sales?.totals;
  const products = [...new Set([...available.keys(), ...Object.keys(totals ?? {}).map(Number), ...Object.keys(data.vendorCodes ?? {}).map(Number)])]
    .filter(matches).sort((a, b) => (totals?.[b]?.[1] ?? 0) - (totals?.[a]?.[1] ?? 0) || a - b);
  const count = view === "stock" ? filtered.length : products.length;
  const start = page * pageSize;
  const fullStock = !!data.wb && !!data.seller;

  return (
    <section className="wbo-panel">
      <div className="wbo-actions">
        {(["wb", "seller", "sales"] as const).map((resource) => (
          <button key={resource} disabled={busy !== null} onClick={() => void sync(resource)}>
            {busy === resource ? `正在同步${sources[resource]}…` : `同步${sources[resource]}`}
          </button>
        ))}
      </div>
      <p>{(["wb", "seller", "sales"] as const).map((source) =>
        `${sources[source]}：${data[source] ? new Date(data[source].updatedAt).toLocaleString("zh-CN") : "尚未同步"}`).join(" · ")}</p>
      {error && <p role="alert" className="wbo-error">{error}</p>}
      <div className="wbo-actions">
        <button aria-pressed={view === "stock"} onClick={() => { setView("stock"); setPage(0); }}>库存明细</button>
        <button aria-pressed={view === "forecast"} onClick={() => { setView("forecast"); setPage(0); }}>销量预估</button>
        <input aria-label="搜索 WB 商品" placeholder="搜索商品编号或货号" value={query}
          onChange={(event) => { setQuery(event.target.value); setPage(0); }} />
      </div>
      {view === "stock" ? (
        <>
          <div className="wbo-actions">
            <select aria-label="筛选仓库" value={warehouse} onChange={(event) => { setWarehouse(event.target.value); setPage(0); }}>
              <option value="">全部仓库</option>
              {[...warehouses].map(([id, label]) => <option key={id} value={id}>{label}</option>)}
            </select>
            {data.wb || data.seller ? <strong>当前筛选可售合计 {format(filtered.reduce((sum, row) => sum + row.quantity, 0))} 件</strong> : null}
          </div>
          <div className="wbo-table-wrap">
            <table>
              <thead><tr><th>商品编号 / 货号</th><th>规格编号</th><th>仓库</th><th>可售</th><th>发往买家</th><th>买家退回途中</th></tr></thead>
              <tbody>{filtered.slice(start, start + pageSize).map((row) => (
                <tr key={`${row.key}:${row.nmId}:${row.sizeId}`}>
                  <td>{row.nmId} {data.vendorCodes?.[row.nmId]}</td>
                  <td>{row.sizeId}</td><td>{row.label}</td><td>{format(row.quantity)}</td>
                  <td>{format(row.toCustomer ?? undefined)}</td><td>{format(row.fromCustomer ?? undefined)}</td>
                </tr>
              ))}</tbody>
            </table>
          </div>
          {!count && <p>{busy === "load" ? "正在读取库存…" : "当前筛选没有库存明细。可同步 WB 仓或卖家仓。"}</p>}
        </>
      ) : (
        <>
          <div className="wbo-actions">
            <label>日均销量口径 <select value={windowDays} onChange={(event) => setWindowDays(Number(event.target.value))}>
              <option value={7}>近 7 天</option><option value={30}>近 30 天</option>
            </select></label>
            <label>预估周期 <select value={forecastDays} onChange={(event) => setForecastDays(Number(event.target.value))}>
              {[7, 14, 30].map((days) => <option key={days} value={days}>未来 {days} 天</option>)}
            </select></label>
          </div>
          <p>{data.sales ? `历史销量截至 ${data.sales.through}（莫斯科时间）。` : "请先同步历史销量。"}按已完成自然日的成交件数计算日均，未来销量按日均乘以预估天数。可售天数使用已同步仓库的可售量。{fullStock ? "" : "尚未同步全部仓库来源。"}</p>
          <div className="wbo-table-wrap">
            <table>
              <thead><tr><th>商品编号 / 货号</th><th>可售库存</th><th>近 7 天销量</th><th>近 30 天销量</th><th>日均销量</th><th>预计 {forecastDays} 天销量</th><th>预计可售天数</th></tr></thead>
              <tbody>{products.slice(start, start + pageSize).map((id) => {
                const counts = totals ? totals[id] ?? [0, 0] : undefined;
                const daily = counts && counts[windowDays === 7 ? 0 : 1] / windowDays;
                const quantity = available.get(id) ?? (fullStock ? 0 : undefined);
                const cover = quantity !== undefined && daily && daily > 0 ? quantity / daily : undefined;
                return (
                  <tr key={id}>
                    <td>{id} {data.vendorCodes?.[id]}</td><td>{format(quantity)}</td>
                    <td>{format(counts?.[0])}</td><td>{format(counts?.[1])}</td><td>{format(daily, 2)}</td>
                    <td>{format(daily === undefined ? undefined : Math.ceil(daily * forecastDays))}</td><td>{format(cover, 1)}</td>
                  </tr>
                );
              })}</tbody>
            </table>
          </div>
          {!count && <p>当前筛选没有商品。</p>}
        </>
      )}
      {count > pageSize && <div className="wbo-actions">
        <button disabled={page === 0} onClick={() => setPage(page - 1)}>上一页</button>
        <span>第 {page + 1} 页 / 共 {Math.ceil(count / pageSize)} 页</span>
        <button disabled={start + pageSize >= count} onClick={() => setPage(page + 1)}>下一页</button>
      </div>}
    </section>
  );
}
