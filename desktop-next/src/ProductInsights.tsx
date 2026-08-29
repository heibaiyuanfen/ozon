import { useDeferredValue, useEffect, useState } from "react";
import { Layers3, Plus, RefreshCw, Search, Trash2 } from "lucide-react";
import * as echarts from "./charts";
import {
  deleteProductSeries,
  productDetail,
  productInsights,
  refreshProductPrice,
  saveProductClusterWeights,
  saveProductSeries,
  seriesInsights,
  updateProductPrice,
} from "./bridge";
import type { InsightRow, ProductDetail, ProductPrice } from "./types";
import "./product-insights.css";

const cash = (value: number, currency: string) =>
  `${currency === "CNY" ? "¥" : "₽"}${value.toLocaleString("zh-CN", { maximumFractionDigits: 2 })}`;

export function ProductInsights({
  to,
  currency,
}: {
  to: string;
  currency: string;
}) {
  const [tab, setTab] = useState<"products" | "series">("products"),
    [products, setProducts] = useState<InsightRow[]>([]),
    [series, setSeries] = useState<InsightRow[]>([]),
    [query, setQuery] = useState(""),
    [selected, setSelected] = useState<string[]>([]),
    [name, setName] = useState(""),
    [busy, setBusy] = useState(false),
    [message, setMessage] = useState(""),
    [page, setPage] = useState(0),
    [detail, setDetail] = useState<ProductDetail | null>(null);
  const deferred = useDeferredValue(query);
  const load = async () => {
    const [p, s] = await Promise.all([
      productInsights(to, deferred),
      seriesInsights(to),
    ]);
    setProducts(p);
    setSeries(s);
  };
  useEffect(() => {
    void load();
  }, [to, deferred]);
  const create = async () => {
    setBusy(true);
    setMessage("");
    try {
      await saveProductSeries(null, name, selected);
      setName("");
      setSelected([]);
      setMessage("产品系列已保存，销量、广告和集群会按成员统一汇总。");
      await load();
    } catch (e) {
      setMessage(String(e));
    } finally {
      setBusy(false);
    }
  };
  const rows = tab === "products" ? products : series,
    pages = Math.max(1, Math.ceil(rows.length / 20)),
    visible = rows.slice(page * 20, page * 20 + 20);
  useEffect(() => setPage(0), [tab, deferred]);
  return (
    <section className="card product-insights">
      <div className="section-heading">
        <div>
          <h2>单品与产品系列经营</h2>
          <p>同时掌握日、周、月销量、销售额、广告以及订单集群分布</p>
        </div>
        <div className="tabs">
          <button
            className={tab === "products" ? "selected" : ""}
            onClick={() => setTab("products")}
          >
            单个产品
          </button>
          <button
            className={tab === "series" ? "selected" : ""}
            onClick={() => setTab("series")}
          >
            自定义系列
          </button>
        </div>
      </div>
      <div className="insight-tools">
        <label className="search">
          <Search size={15} />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="搜索货号或 SKU"
          />
        </label>
        <div className="series-builder">
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="新系列名称"
          />
          <span>已选 {selected.length} 个产品</span>
          <button
            disabled={busy || !name.trim() || !selected.length}
            onClick={create}
          >
            <Plus size={14} />
            创建系列
          </button>
        </div>
      </div>
      {message && <div className="sync-message">{message}</div>}
      <div className="insight-list">
        {visible.map((row) => (
          <article
            key={row.id}
            className="insight-row"
            onDoubleClick={() =>
              tab === "products" && productDetail(row.id, to).then(setDetail)
            }
            title={tab === "products" ? "双击打开单品经营看板" : undefined}
          >
            {tab === "products" ? (
              <label className="series-select">
                <input
                  type="checkbox"
                  checked={selected.includes(row.id)}
                  onChange={(e) =>
                    setSelected(
                      e.target.checked
                        ? [...selected, row.id]
                        : selected.filter((x) => x !== row.id),
                    )
                  }
                />
              </label>
            ) : (
              <button
                className="delete-series"
                title="删除系列"
                onClick={async () => {
                  await deleteProductSeries(Number(row.id));
                  await load();
                }}
              >
                <Trash2 size={14} />
              </button>
            )}
            <div className="insight-name">
              <Layers3 size={17} />
              <span>
                <b>{row.offerIds.filter(Boolean).join("、") || "无货号"}</b>
                <small>
                  {row.offerIds.filter(Boolean).join("、") ||
                    row.skus.join("、")}{" "}
                  · {row.skus.length} 个 SKU
                </small>
              </span>
            </div>
            <div className="period-metrics">
              <span>
                今日<b>{row.dayUnits} 件</b>
                <small>
                  {cash(row.dayRevenue, currency)} · 广告{" "}
                  {cash(row.dayAdSpend, currency)}
                </small>
              </span>
              <span>
                近 7 天<b>{row.weekUnits} 件</b>
                <small>
                  {cash(row.weekRevenue, currency)} · 广告{" "}
                  {cash(row.weekAdSpend, currency)}
                </small>
              </span>
              <span>
                近 30 天<b>{row.monthUnits} 件</b>
                <small>
                  {cash(row.monthRevenue, currency)} · 广告{" "}
                  {cash(row.monthAdSpend, currency)}
                </small>
              </span>
            </div>
            <div className="cluster-share">
              <b>集群分布</b>
              {row.clusters.slice(0, 4).map((cluster) => (
                <span key={cluster.cluster}>
                  <i style={{ width: `${Math.max(3, cluster.share)}%` }} />
                  <em>{cluster.cluster}</em>
                  <small>
                    {cluster.orders} 单 · {cluster.share.toFixed(1)}%
                  </small>
                </span>
              ))}
              {!row.clusters.length && <small>近 30 天无可识别订单集群</small>}
            </div>
          </article>
        ))}
      </div>
      {rows.length > 20 && (
        <div className="table-pagination">
          <button disabled={page === 0} onClick={() => setPage(page - 1)}>
            上一页
          </button>
          <span>
            第 {page + 1} / {pages} 页 · 共 {rows.length} 项
          </span>
          <button
            disabled={page + 1 >= pages}
            onClick={() => setPage(page + 1)}
          >
            下一页
          </button>
        </div>
      )}
      {!rows.length && (
        <div className="empty">
          当前没有可显示的数据；同步 Seller 销量与订单后会自动出现。
        </div>
      )}
      <p className="insight-source">
        单品广告仅使用可精确归属 SKU
        的真实广告缓存；全店广告不会按比例猜测分摊。
      </p>
      {detail && (
        <ProductDetailModal
          data={detail}
          currency={currency}
          close={() => setDetail(null)}
        />
      )}
    </section>
  );
}
function ProductDetailModal({
  data,
  currency,
  close,
}: {
  data: ProductDetail;
  currency: string;
  close: () => void;
}) {
  const [weights, setWeights] = useState<Record<string, number>>(() =>
      Object.fromEntries(
        data.clusters.map((x) => [x.cluster, x.configuredWeight]),
      ),
    ),
    [message, setMessage] = useState(""),
    [price, setPrice] = useState<ProductPrice | null>(data.price),
    [priceLogs, setPriceLogs] = useState(data.priceLogs),
    [priceDraft, setPriceDraft] = useState(() => data.price?.price ?? 0),
    [oldPriceDraft, setOldPriceDraft] = useState(() => data.price?.oldPrice ?? 0),
    [minPriceDraft, setMinPriceDraft] = useState(() => data.price?.minPrice ?? 0),
    [priceConfirm, setPriceConfirm] = useState(""),
    [priceBusy, setPriceBusy] = useState(false);
  useEffect(() => {
    const line = echarts.init(document.getElementById("product-detail-trend")!);
    line.setOption({
      tooltip: { trigger: "axis" },
      legend: { top: 0 },
      grid: { left: 50, right: 55, top: 40, bottom: 28 },
      xAxis: { type: "category", data: data.trend.map((x) => x.day.slice(5)) },
      yAxis: [
        { type: "value", name: "销量" },
        { type: "value", name: "金额" },
      ],
      series: [
        {
          name: "销量",
          type: "line",
          smooth: true,
          data: data.trend.map((x) => x.units),
          areaStyle: {},
        },
        {
          name: "销售额",
          type: "line",
          smooth: true,
          yAxisIndex: 1,
          data: data.trend.map((x) => x.revenue),
        },
        {
          name: "广告花费",
          type: "bar",
          yAxisIndex: 1,
          data: data.trend.map((x) => x.adSpend),
        },
      ],
    });
    const pie = echarts.init(document.getElementById("product-detail-pie")!);
    const topClusters = data.clusters.slice(0, 8);
    const otherOrders = data.clusters.slice(8).reduce((sum, x) => sum + x.orders, 0);
    pie.setOption({
      tooltip: { trigger: "item", formatter: "{b}: {c} 单 ({d}%)" },
      legend: { show: false },
      series: [
        {
          type: "pie",
          radius: ["42%", "70%"],
          data: [...topClusters, ...(otherOrders ? [{ cluster: "其他", orders: otherOrders }] : [])].map((x) => ({
            name: x.cluster,
            value: x.orders,
          })),
          label: { formatter: "{b}\\n{d}%" },
        },
      ],
    });
    return () => {
      line.dispose();
      pie.dispose();
    };
  }, [data]);
  const recent = data.trend.slice(-7).reduce((s, x) => s + x.units, 0),
    previous = data.trend.slice(-14, -7).reduce((s, x) => s + x.units, 0),
    change = previous ? ((recent - previous) / previous) * 100 : null;
  const save = async () => {
    await saveProductClusterWeights(data.sku, weights);
    setMessage("权重已归一化保存，将用于该产品的集群配送费估算。");
  };
  const applyPrice = (next: ProductPrice) => {
    setPrice(next);
    setPriceDraft(next.price);
    setOldPriceDraft(next.oldPrice);
    setMinPriceDraft(next.minPrice);
  };
  const refreshPrice = async () => {
    setPriceBusy(true);
    setMessage("");
    try {
      applyPrice(await refreshProductPrice(data.sku));
      setMessage("已从 Ozon Seller API 读取最新价格。");
    } catch (e) {
      setMessage(String(e));
    } finally {
      setPriceBusy(false);
    }
  };
  useEffect(() => {
    if (!data.price) void refreshPrice();
    // 首次无缓存时读取一次；已有缓存立即展示，避免每次打开弹窗都阻塞等待 API。
  }, [data.sku]);
  const savePrice = async () => {
    if (priceConfirm.trim() !== "确认改价") {
      setMessage("请输入“确认改价”后再提交。");
      return;
    }
    if (!window.confirm(`确认把 ${data.offerId || data.sku} 的售价修改为 ₽${priceDraft}？`)) return;
    setPriceBusy(true);
    setMessage("");
    try {
      applyPrice(await updateProductPrice({
        sku: data.sku,
        price: priceDraft,
        oldPrice: oldPriceDraft,
        minPrice: minPriceDraft,
        currencyCode: price?.currencyCode || "RUB",
      }));
      const refreshed = await productDetail(data.sku, data.trend.at(-1)?.day || "");
      setPriceLogs(refreshed.priceLogs);
      setPriceConfirm("");
      setMessage("价格修改请求已提交，并完成 Ozon 回读检查。");
    } catch (e) {
      setMessage(String(e));
      const refreshed = await productDetail(data.sku, data.trend.at(-1)?.day || "").catch(() => null);
      if (refreshed) setPriceLogs(refreshed.priceLogs);
    } finally {
      setPriceBusy(false);
    }
  };
  return (
    <div className="modal-backdrop product-detail-backdrop">
      <div className="product-detail-modal">
        <button className="modal-close" onClick={close}>
          ×
        </button>
        <header>
          <div>
            <small>
              {data.offerId} · SKU {data.sku}
            </small>
            <h2>{data.offerId || `SKU ${data.sku}`}</h2>
          </div>
          <strong
            className={
              change == null
                ? ""
                : change > 0
                  ? "up"
                  : change < 0
                    ? "down"
                    : "flat"
            }
          >
            {change == null
              ? "趋势待积累"
              : change > 0
                ? `近7天增长 ${change.toFixed(1)}%`
                : change < 0
                  ? `近7天下降 ${Math.abs(change).toFixed(1)}%`
                  : "近7天保持稳定"}
          </strong>
        </header>
        <section className="product-price-panel">
          <div className="product-price-heading">
            <div>
              <h3>Ozon 商品价格</h3>
              <p>{price ? `上次读取：${price.syncedAt}` : "尚未读取官方价格"}</p>
            </div>
            <button disabled={priceBusy} onClick={refreshPrice}>
              <RefreshCw size={14} />{priceBusy ? "读取中" : "读取最新价格"}
            </button>
          </div>
          <div className="product-price-metrics">
            <span>当前售价<b>{price ? `${price.currencyCode} ${price.price.toLocaleString()}` : "—"}</b></span>
            <span>划线价<b>{price ? price.oldPrice.toLocaleString() : "—"}</b></span>
            <span>最低价<b>{price ? price.minPrice.toLocaleString() : "—"}</b></span>
            <span>结算净价<b>{price?.netPrice == null ? "—" : price.netPrice.toLocaleString()}</b></span>
          </div>
          <div className="product-price-editor">
            <label>新售价<input type="number" min="0.01" step="0.01" value={priceDraft || ""} onChange={(e)=>setPriceDraft(Number(e.target.value))}/></label>
            <label>划线价<input type="number" min="0" step="0.01" value={oldPriceDraft} onChange={(e)=>setOldPriceDraft(Number(e.target.value))}/></label>
            <label>最低价<input type="number" min="0" step="0.01" value={minPriceDraft} onChange={(e)=>setMinPriceDraft(Number(e.target.value))}/></label>
            <label>安全确认<input value={priceConfirm} onChange={(e)=>setPriceConfirm(e.target.value)} placeholder="输入：确认改价"/></label>
            <button className="dark-button" disabled={priceBusy || !priceDraft || priceConfirm!=="确认改价"} onClick={savePrice}>提交价格修改</button>
          </div>
          <div className="product-price-logs">
            <h4>最近改价记录</h4>
            {priceLogs.slice(0,5).map((log)=><div key={log.id}><time>{log.createdAt}</time><span>₽{log.beforePrice} → ₽{log.requestedPrice}</span><b className={log.status}>{log.status}</b><small>{log.message}</small></div>)}
            {!priceLogs.length && <p>暂无 ERP 改价记录。</p>}
          </div>
        </section>
        <div className="product-detail-charts">
          <section>
            <h3>30天销量、销售额与广告趋势</h3>
            <div id="product-detail-trend" />
          </section>
          <section>
            <h3>订单集群权重分布</h3>
            <div id="product-detail-pie" />
          </section>
        </div>
        <section className="weight-editor">
          <h3>产品配送集群权重</h3>
          <p>历史占比用于参考；保存后的自定义权重会自动归一化为 100%。</p>
          {data.clusters.map((x) => (
            <label key={x.cluster}>
              <span>
                {x.cluster}
                <small>
                  历史 {x.historicalShare.toFixed(1)}% · {x.orders} 单
                </small>
              </span>
              <input
                type="number"
                min="0"
                step="0.1"
                value={weights[x.cluster] ?? 0}
                onChange={(e) =>
                  setWeights({
                    ...weights,
                    [x.cluster]: Number(e.target.value),
                  })
                }
              />
              <b>%</b>
            </label>
          ))}
          <button className="dark-button" onClick={save}>
            保存配送权重
          </button>
          {message && <span className="saved-message">{message}</span>}
        </section>
      </div>
    </div>
  );
}
