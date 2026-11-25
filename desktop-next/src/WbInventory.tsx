import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type StockRow = {
  nmId: number;
  sizeId: number;
  warehouseId: number;
  warehouse: string;
  quantity: number;
  vendorCode?: string;
  toCustomer: number | null;
  fromCustomer: number | null;
};
type StockSnapshot = { rows: StockRow[]; updatedAt?: string };
type Product = { nmId: number; sizeId: number; title: string; vendorCode: string; size: string };
type Warehouse = { id: number; name: string };
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
  const [busy, setBusy] = useState<Resource | "load" | "update" | null>("load");
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [edit, setEdit] = useState<StockRow | null>(null);
  const [adding, setAdding] = useState(false);
  const [sellerWarehouses, setSellerWarehouses] = useState<Warehouse[]>([]);
  const [productSearch, setProductSearch] = useState("");
  const [productsFound, setProductsFound] = useState<Product[]>([]);
  const [selectedSize, setSelectedSize] = useState("");
  const [selectedWarehouse, setSelectedWarehouse] = useState("");
  const [optionsBusy, setOptionsBusy] = useState(false);
  const [searched, setSearched] = useState(false);
  const [quantity, setQuantity] = useState("");
  const validQuantity = /^\d+$/.test(quantity) && Number(quantity) <= 999999;
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

  async function openAdd() {
    setAdding(true);
    setError("");
    setNotice("");
    setQuantity("");
    setProductSearch("");
    setProductsFound([]);
    setSelectedSize("");
    setSelectedWarehouse("");
    setSellerWarehouses([]);
    setSearched(false);
    setOptionsBusy(true);
    try {
      const options = await invoke<Warehouse[]>("wb_operations", {
        action: "inventory_options", payload: { shopId },
      });
      setSellerWarehouses(options);
      setSelectedWarehouse(String(options.find((item) => `seller:${item.id}` === warehouse)?.id ?? options[0]?.id ?? ""));
    } catch (reason) {
      setError(String(reason));
    } finally {
      setOptionsBusy(false);
    }
  }

  async function findProducts() {
    if (optionsBusy || !productSearch.trim()) return;
    setError("");
    setOptionsBusy(true);
    setProductsFound([]);
    setSelectedSize("");
    setSearched(false);
    try {
      setProductsFound(await invoke<Product[]>("wb_operations", {
        action: "inventory_options", payload: { shopId, search: productSearch.trim() },
      }));
      setSearched(true);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setOptionsBusy(false);
    }
  }

  async function saveStock(row: Pick<StockRow, "nmId" | "warehouseId" | "sizeId">, add = false) {
    if (busy || !validQuantity) return;
    setError("");
    setNotice("");
    setBusy("update");
    try {
      setData(await invoke<Snapshot>("wb_operations", {
        action: "inventory_update",
        payload: { shopId, warehouseId: row.warehouseId, sizeId: row.sizeId, nmId: row.nmId, amount: Number(quantity), add },
      }));
      setEdit(null);
      setAdding(false);
      if (add) { setQuery(String(row.nmId)); setWarehouse(`seller:${row.warehouseId}`); setPage(0); }
      setNotice(`商品 ${row.nmId} · 规格 ${row.sizeId} 的卖家仓库存已更新为 ${quantity} 件。`);
    } catch (reason) {
      setError(`库存更新未完成：${String(reason)}`);
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
  const fullStock = !!data.wb?.updatedAt && !!data.seller?.updatedAt;
  const selectedProduct = productsFound.find((item) => String(item.sizeId) === selectedSize);

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
        `${sources[source]}：${data[source]?.updatedAt ? new Date(data[source].updatedAt!).toLocaleString("zh-CN") : "尚未同步"}`).join(" · ")}</p>
      {notice && <p role="status">{notice}</p>}
      {error && !edit && !adding && <p role="alert" className="wbo-error">{error}</p>}
      <div className="wbo-actions">
        <button aria-pressed={view === "stock"} onClick={() => { setView("stock"); setPage(0); }}>库存明细</button>
        <button aria-pressed={view === "forecast"} onClick={() => { setView("forecast"); setPage(0); }}>销量预估</button>
        <input aria-label="搜索 WB 商品" placeholder="搜索商品编号或货号" value={query}
          onChange={(event) => { setQuery(event.target.value); setPage(0); }} />
      </div>
      {view === "stock" ? (
        <>
          <div className="wbo-actions">
            <button className="primary" disabled={busy !== null} onClick={() => void openAdd()}>加入库存</button>
            <select aria-label="筛选仓库" value={warehouse} onChange={(event) => { setWarehouse(event.target.value); setPage(0); }}>
              <option value="">全部仓库</option>
              {[...warehouses].map(([id, label]) => <option key={id} value={id}>{label}</option>)}
            </select>
            {data.wb || data.seller ? <strong>当前筛选可售合计 {format(filtered.reduce((sum, row) => sum + row.quantity, 0))} 件</strong> : null}
          </div>
          <div className="wbo-table-wrap">
            <table>
              <thead><tr><th>商品编号 / 货号</th><th>规格编号</th><th>仓库</th><th>可售</th><th>发往买家</th><th>买家退回途中</th><th>操作</th></tr></thead>
              <tbody>{filtered.slice(start, start + pageSize).map((row) => (
                <tr key={`${row.key}:${row.nmId}:${row.sizeId}`}>
                  <td>{row.nmId} {row.vendorCode || data.vendorCodes?.[row.nmId]}</td>
                  <td>{row.sizeId}</td><td>{row.label}</td><td>{format(row.quantity)}</td>
                  <td>{format(row.toCustomer ?? undefined)}</td><td>{format(row.fromCustomer ?? undefined)}</td>
                  <td>{row.source === "seller" ? (
                    <button disabled={busy !== null} onClick={() => {
                      setError(""); setNotice(""); setQuantity(String(row.quantity)); setEdit(row);
                    }}>修改库存</button>
                  ) : "—"}</td>
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
      {adding && <div className="wbo-overlay">
        <form className="wbo-dialog" role="dialog" aria-modal="true" aria-labelledby="wb-stock-add"
          onSubmit={(event) => {
            event.preventDefault();
            if (selectedProduct && selectedWarehouse) {
              void saveStock({ ...selectedProduct, warehouseId: Number(selectedWarehouse) }, true);
            }
          }}>
          <h2 id="wb-stock-add">加入卖家仓库存</h2>
          <fieldset className="wbo-stock-fields" disabled={optionsBusy || busy === "update"}>
            <label>卖家仓
              <select required value={selectedWarehouse} onChange={(event) => setSelectedWarehouse(event.target.value)}>
                <option value="">选择卖家仓</option>
                {sellerWarehouses.map((item) => <option key={item.id} value={item.id}>{item.name} · {item.id}</option>)}
              </select>
            </label>
            <label>搜索当前店铺商品
              <input autoFocus placeholder="输入 WB 商品编号或卖家货号" value={productSearch} onChange={(event) => {
                setProductSearch(event.target.value); setProductsFound([]); setSelectedSize(""); setSearched(false);
              }} onKeyDown={(event) => {
                if (event.key === "Enter") { event.preventDefault(); void findProducts(); }
              }} />
            </label>
            <button type="button" disabled={!productSearch.trim()} onClick={() => void findProducts()}>搜索商品</button>
            {searched && !productsFound.length && <p>没有找到匹配的商品规格。</p>}
            <label>商品规格
              <select required value={selectedSize} onChange={(event) => setSelectedSize(event.target.value)}>
                <option value="">选择商品规格</option>
                {productsFound.map((item) => <option key={item.sizeId} value={item.sizeId}>
                  {item.vendorCode || item.nmId} · {item.title} · {item.size || "默认规格"} · {item.sizeId}
                </option>)}
              </select>
            </label>
            <label>可售库存 <input type="number" min={1} max={999999} step={1} required
              value={quantity} onChange={(event) => setQuantity(event.target.value)} /></label>
          </fieldset>
          {optionsBusy && <p role="status">正在读取 WB 数据…</p>}
          {error && <p role="alert" className="wbo-error">{error}</p>}
          <div className="wbo-actions">
            <button type="button" disabled={optionsBusy || busy === "update"} onClick={() => { setAdding(false); setError(""); }}>取消</button>
            <button className="primary" disabled={optionsBusy || busy !== null || !selectedWarehouse || !selectedProduct || !validQuantity || Number(quantity) < 1}>
              {busy === "update" ? "正在提交…" : "确认加入 WB 库存"}
            </button>
          </div>
        </form>
      </div>}
      {edit && <div className="wbo-overlay">
        <form className="wbo-dialog" role="dialog" aria-modal="true" aria-labelledby="wb-stock-edit"
          onSubmit={(event) => { event.preventDefault(); void saveStock(edit); }}>
          <h2 id="wb-stock-edit">修改卖家仓库存</h2>
          <p>仓库 {edit.warehouse || edit.warehouseId} · 商品 {edit.nmId} · 规格 {edit.sizeId}</p>
          <p>当前表格库存 {edit.quantity} 件。保存将直接设置 WB 平台可售库存，填 0 可清零。</p>
          <label>新的可售库存 <input type="number" min={0} max={999999} step={1} required autoFocus
            disabled={busy === "update"} value={quantity} onChange={(event) => setQuantity(event.target.value)} /></label>
          {error && <p role="alert" className="wbo-error">{error}</p>}
          <div className="wbo-actions">
            <button type="button" disabled={busy === "update"} onClick={() => { setEdit(null); setError(""); }}>取消</button>
            <button className="primary" disabled={busy !== null || !validQuantity}>
              {busy === "update" ? "正在提交…" : "确认更新到 WB"}
            </button>
          </div>
        </form>
      </div>}
      {count > pageSize && <div className="wbo-actions">
        <button disabled={page === 0} onClick={() => setPage(page - 1)}>上一页</button>
        <span>第 {page + 1} 页 / 共 {Math.ceil(count / pageSize)} 页</span>
        <button disabled={start + pageSize >= count} onClick={() => setPage(page + 1)}>下一页</button>
      </div>}
    </section>
  );
}
