import { useDeferredValue, useEffect, useState } from "react";
import { Layers3, Plus, Search, Trash2 } from "lucide-react";
import * as echarts from "echarts";
import {
  deleteProductSeries,
  productDetail,
  productInsights,
  saveProductClusterWeights,
  saveProductSeries,
  seriesInsights,
} from "./bridge";
import type { InsightRow, ProductDetail } from "./types";
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
    [message, setMessage] = useState("");
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
