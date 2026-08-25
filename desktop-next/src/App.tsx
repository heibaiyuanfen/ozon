import {
  lazy,
  Suspense,
  startTransition,
  useDeferredValue,
  useEffect,
  useMemo,
  useState,
} from "react";
import "./performance.css";
import "./layout-fixes.css";
import * as echarts from "echarts";
import {
  BarChart3,
  Box,
  BrainCircuit,
  CalendarDays,
  ChevronDown,
  Database,
  LayoutDashboard,
  Megaphone,
  PackageSearch,
  RefreshCw,
  Search,
  Settings2,
  ShoppingBag,
  Store,
  Truck,
} from "lucide-react";
import {
  advertising,
  connectionStatus,
  dashboard,
  inventory,
  listShops,
  orders,
  products,
  selectShop,
} from "./bridge";
import type {
  AdvertisingData,
  ConnectionStatus,
  DashboardData,
  DateRange,
  InventoryRow,
  OrderRow,
  PageKey,
  ProductRow,
  Shop,
} from "./types";
import { clearReportCache } from "./reportCache";

const InventoryPage = lazy(() => import("./Phase2Pages").then((m) => ({ default: m.InventoryPage })));
const ProductsPage = lazy(() => import("./Phase2Pages").then((m) => ({ default: m.ProductsPage })));
const SettingsPage = lazy(() => import("./Phase2Pages").then((m) => ({ default: m.SettingsPage })));
const ShopsPage = lazy(() => import("./Phase2Pages").then((m) => ({ default: m.ShopsPage })));
const AiPage = lazy(() => import("./OperationsPages").then((m) => ({ default: m.AiPage })));
const CompetitorsPage = lazy(() => import("./OperationsPages").then((m) => ({ default: m.CompetitorsPage })));
const FbsPage = lazy(() => import("./OperationsPages").then((m) => ({ default: m.FbsPage })));
const FeishuPage = lazy(() => import("./OperationsPages").then((m) => ({ default: m.FeishuPage })));
const ListingPage = lazy(() => import("./OperationsPages").then((m) => ({ default: m.ListingPage })));
const MigrationPage = lazy(() => import("./OperationsPages").then((m) => ({ default: m.MigrationPage })));
const ReportsPage = lazy(() => import("./OperationsPages").then((m) => ({ default: m.ReportsPage })));
const SupplyPage = lazy(() => import("./OperationsPages").then((m) => ({ default: m.SupplyPage })));
const SyncPage = lazy(() => import("./OperationsPages").then((m) => ({ default: m.SyncPage })));
const WbPage = lazy(() => import("./OperationsPages").then((m) => ({ default: m.WbPage })));
const ProductInsights = lazy(() => import("./ProductInsights").then((m) => ({ default: m.ProductInsights })));

const emptyDashboard: DashboardData = {
  revenue: 0,
  orders: 0,
  soldUnits: 0,
  activeProducts: 0,
  adSpend: 0,
  adRevenue: 0,
  adOrders: 0,
  conversionRate: null,
  trend: [],
  lastSync: null,
  acos: null,
  tacos: null,
  ctr: null,
  returnUnits: 0,
  cancellationUnits: 0,
  cancellationRate: null,
  views: 0,
  orderConversion: null,
};
const emptyAds: AdvertisingData = {
  impressions: 0,
  clicks: 0,
  cartAdds: 0,
  orders: 0,
  revenue: 0,
  spend: 0,
  ctr: null,
  cpc: null,
  roas: null,
  campaigns: [],
  trend: [],
};

function iso(date: Date) {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}
function rangeFor(days: number): DateRange {
  const to = new Date();
  const from = new Date();
  from.setDate(to.getDate() - days + 1);
  return { from: iso(from), to: iso(to) };
}
function currentMonthRange(): DateRange {
  const to = new Date(),
    from = new Date(to.getFullYear(), to.getMonth(), 1);
  return { from: iso(from), to: iso(to) };
}
function money(value: number, currency: string) {
  return `${currency === "CNY" ? "¥" : "₽"}${value.toLocaleString("zh-CN", { minimumFractionDigits: value % 1 ? 1 : 0, maximumFractionDigits: 2 })}`;
}
function pct(value: number | null) {
  return value == null ? "—" : `${value.toFixed(2)}%`;
}

function Sidebar({
  page,
  setPage,
  shops,
  activeShop,
  changeShop,
  workspace,
  setWorkspace,
  wbPage,
  setWbPage,
}: {
  page: PageKey;
  setPage: (page: PageKey) => void;
  shops: Shop[];
  activeShop?: Shop;
  changeShop: (id: string) => void;
  workspace: "ozon" | "wb";
  setWorkspace: (value: "ozon" | "wb") => void;
  wbPage: "daily" | "costs" | "settings";
  setWbPage: (value: "daily" | "costs" | "settings") => void;
}) {
  const items: Array<[PageKey | "disabled", string, typeof LayoutDashboard]> = [
    ["dashboard", "经营总览", LayoutDashboard],
    ["orders", "订单中心", ShoppingBag],
    ["fbs", "FBS 管理", Truck],
    ["products", "商品中心", Box],
    ["advertising", "广告运营", Megaphone],
    ["reports", "数据报告", BarChart3],
    ["monthly_profit", "月度盈亏", BarChart3],
    ["weekly_report", "经营周报", CalendarDays],
    ["cross_profit", "跨境店铺利润", BarChart3],
    ["ai", "AI 分析", BrainCircuit],
    ["inventory", "库存管理", PackageSearch],
    ["supply", "约仓计划", Truck],
    ["sync", "数据同步", RefreshCw],
    ["feishu", "飞书协作", Database],
    ["migration", "数据迁移", Database],
    ["listing", "跨境上品", PackageSearch],
    ["competitors", "竞品跟踪", PackageSearch],
    ["shops", "店铺管理", Store],
    ["settings", "连接设置", Settings2],
  ];
  return (
    <aside className="sidebar">
      <button
        className="brand workspace-switch"
        onClick={() => setWorkspace(workspace === "ozon" ? "wb" : "ozon")}
      >
        <div className="brand-mark">
          <Database size={18} />
        </div>
        <div>
          <b>{workspace === "ozon" ? "Ozon ERP" : "WB ERP"}</b>
          <small>
            {workspace === "ozon" ? "切换至 WB 工作区" : "返回 Ozon 工作区"}
          </small>
        </div>
      </button>
      <label className="shop-picker">
        <Store size={16} />
        <select
          value={activeShop?.id ?? ""}
          onChange={(e) => changeShop(e.target.value)}
        >
          {shops.map((s) => (
            <option key={s.id} value={s.id}>
              {s.name}
            </option>
          ))}
        </select>
        <ChevronDown size={14} />
        <small>{activeShop?.apiName}</small>
      </label>
      {workspace === "ozon" && (
        <>
          <div className="nav-label">工作台</div>
          <nav>
            {items.map(([key, label, Icon]) => (
              <button
                key={label}
                disabled={key === "disabled"}
                className={key === page ? "active" : ""}
                onClick={() =>
                  key !== "disabled" && startTransition(() => setPage(key))
                }
              >
                <Icon size={17} />
                {label}
              </button>
            ))}
          </nav>
        </>
      )}
      {workspace === "wb" && (
        <>
          <div className="nav-label">WB 工作台</div>
          <nav>
            {(
              [
                ["daily", "经营总览", LayoutDashboard],
                ["costs", "商品与成本", Box],
                ["settings", "WB API 与汇率", Settings2],
              ] as Array<
                ["daily" | "costs" | "settings", string, typeof LayoutDashboard]
              >
            ).map(([key, label, Icon]) => (
              <button
                key={label}
                className={wbPage === key ? "active" : ""}
                onClick={() => setWbPage(key as "daily" | "costs" | "settings")}
              >
                <Icon size={17} />
                {label}
              </button>
            ))}
          </nav>
        </>
      )}
      <div className="sidebar-foot">
        <div className="avatar">黑</div>
        <div>
          <b>黑白缘分</b>
          <small>本地安全工作区</small>
        </div>
      </div>
    </aside>
  );
}

function Header({
  eyebrow,
  title,
  subtitle,
  refreshing,
  refresh,
}: {
  eyebrow: string;
  title: string;
  subtitle: string;
  refreshing: boolean;
  refresh: () => void;
}) {
  return (
    <header className="page-header">
      <div>
        <span className="eyebrow">{eyebrow}</span>
        <h1>{title}</h1>
        <p>{subtitle}</p>
      </div>
      <button className="dark-button" onClick={refresh} disabled={refreshing}>
        <RefreshCw size={16} className={refreshing ? "spin" : ""} />
        {refreshing ? "正在读取" : "刷新数据"}
      </button>
    </header>
  );
}

function RangeTabs({
  days,
  setDays,
}: {
  days: number;
  setDays: (days: number) => void;
}) {
  return (
    <div className="range-row">
      <div className="tabs">
        {[
          [7, "最近7天"],
          [30, "最近30天"],
          [90, "本季度"],
        ].map(([value, label]) => (
          <button
            className={days === value ? "selected" : ""}
            onClick={() => setDays(value as number)}
            key={value}
          >
            {label}
          </button>
        ))}
      </div>
      <div className="date-pill">
        <CalendarDays size={16} />
        最近 {days} 天
      </div>
    </div>
  );
}

function Stat({
  tone,
  label,
  value,
  note,
}: {
  tone: string;
  label: string;
  value: string;
  note: string;
}) {
  return (
    <div className={`stat ${tone}`}>
      <span>{label}</span>
      <strong>{value}</strong>
      <small>↗ {note}</small>
    </div>
  );
}

function TrendChart({
  data,
  mode,
}: {
  data: DashboardData["trend"];
  mode: "revenue" | "units";
}) {
  useEffect(() => {
    const el = document.getElementById("trend-chart");
    if (!el) return;
    const chart = echarts.init(el);
    chart.setOption({
      grid: { left: 20, right: 62, top: 46, bottom: 28, containLabel: true },
      tooltip: { trigger: "axis" },
      xAxis: {
        type: "category",
        boundaryGap: false,
        data: data.map((x) => x.day),
        axisLine: { show: false },
        axisTick: { show: false },
        axisLabel: { hideOverlap: true, interval: "auto", margin: 12 },
      },
      yAxis: [
        {
          type: "value",
          splitLine: { lineStyle: { color: "#eef2f7" } },
          axisLabel: { color: "#9aa6b5" },
        },
        {
          type: "value",
          splitLine: { show: false },
          axisLabel: { color: "#c28b46" },
        },
      ],
      legend: {
        left: "center",
        top: 6,
        itemGap: 24,
        data: [mode === "revenue" ? "销售额" : "销量", "广告花费"],
      },
      series: [
        {
          name: mode === "revenue" ? "销售额" : "销量",
          type: "line",
          smooth: true,
          symbol: "none",
          data: data.map((x) => (mode === "revenue" ? x.revenue : x.units)),
          lineStyle: { color: "#3478f6", width: 3 },
          areaStyle: {
            color: new echarts.graphic.LinearGradient(0, 0, 0, 1, [
              { offset: 0, color: "rgba(52,120,246,.22)" },
              { offset: 1, color: "rgba(52,120,246,0)" },
            ]),
          },
        },
        {
          name: "广告花费",
          type: "line",
          smooth: true,
          symbol: "none",
          yAxisIndex: 1,
          data: data.map((x) => x.adSpend),
          lineStyle: { color: "#f39a32", width: 2 },
        },
      ],
    });
    const resize = () => chart.resize();
    window.addEventListener("resize", resize);
    return () => {
      window.removeEventListener("resize", resize);
      chart.dispose();
    };
  }, [data, mode]);
  return <div id="trend-chart" className="chart" />;
}

function Dashboard({
  data,
  currency,
  days,
  setDays,
  refreshing,
  refresh,
  to,
}: {
  data: DashboardData;
  currency: string;
  days: number;
  setDays: (n: number) => void;
  refreshing: boolean;
  refresh: () => void;
  to: string;
}) {
  const [chartMode, setChartMode] = useState<"revenue" | "units">("revenue");
  const avg = data.orders ? data.revenue / data.orders : 0;
  return (
    <>
      <Header
        eyebrow="运营中心"
        title="经营总览"
        subtitle="当前店铺经营表现与本地缓存快照"
        refreshing={refreshing}
        refresh={refresh}
      />
      <div className="hero-grid">
        <section className="card summary">
          <div className="card-title">
            实时销量 <span className="badge green">已同步</span>
          </div>
          <div className="four-cols">
            <Stat
              tone="blue"
              label="销量"
              value={String(data.soldUnits)}
              note="当前周期"
            />
            <Stat
              tone="blue"
              label="销售额"
              value={money(data.revenue, currency)}
              note="真实汇总"
            />
            <Stat
              tone="blue"
              label="订单量"
              value={String(data.orders)}
              note="真实汇总"
            />
            <Stat
              tone="blue"
              label="客单价"
              value={money(avg, currency)}
              note="平均订单金额"
            />
          </div>
        </section>
        <section className="card summary">
          <div className="card-title">
            广告表现 <span className="badge purple">本地广告缓存</span>
          </div>
          <div className="four-cols">
            <Stat
              tone="purple"
              label="广告花费"
              value={money(data.adSpend, currency)}
              note="活动总计"
            />
            <Stat
              tone="purple"
              label="广告销售额"
              value={money(data.adRevenue, currency)}
              note="归因销售"
            />
            <Stat
              tone="purple"
              label="广告订单量"
              value={String(data.adOrders)}
              note="真实汇总"
            />
            <Stat
              tone="purple"
              label="转化率"
              value={pct(data.conversionRate)}
              note="点击至下单"
            />
          </div>
        </section>
      </div>
      {data.adSpend === 0 && data.adOrders > 0 && (
        <div className="error-banner ad-resync-warning">
          当前范围已有广告曝光或订单，但旧缓存的广告花费为
          0。请到“数据同步”重新同步 Performance 广告，新版会读取 moneySpent /
          expense 等真实花费字段。
        </div>
      )}
      <section className="card trend-card">
        <div className="section-heading">
          <div>
            <h2>业绩趋势</h2>
            <p>
              {chartMode === "revenue" ? "销售额" : "销量"}与广告花费的日度变化
            </p>
          </div>
          <div className="trend-actions">
            <div className="tabs">
              <button
                className={chartMode === "revenue" ? "selected" : ""}
                onClick={() => setChartMode("revenue")}
              >
                销售额
              </button>
              <button
                className={chartMode === "units" ? "selected" : ""}
                onClick={() => setChartMode("units")}
              >
                销量
              </button>
            </div>
            <RangeTabs days={days} setDays={setDays} />
          </div>
        </div>
        <div className="metric-strip">
          <Stat
            tone="blue selected-stat"
            label="销售额"
            value={money(data.revenue, currency)}
            note="已汇总"
          />
          <Stat
            tone="green"
            label="订单量"
            value={String(data.orders)}
            note="已汇总"
          />
          <Stat
            tone="orange"
            label="广告花费"
            value={money(data.adSpend, currency)}
            note="活动总计"
          />
          <Stat
            tone="green"
            label="在售商品"
            value={String(data.activeProducts)}
            note="本地快照"
          />
        </div>
        <TrendChart data={data.trend} mode={chartMode} />
      </section>
      <div className="health-grid">
        <section className="card health-card">
          <div className="card-title">
            经营健康度 <span className="badge blue">真实指标</span>
          </div>
          <div className="health-list">
            <span>
              广告 ACOS<b>{pct(data.acos)}</b>
            </span>
            <span>
              整体 TACOS / DRR<b>{pct(data.tacos)}</b>
            </span>
            <span>
              广告 CTR<b>{pct(data.ctr)}</b>
            </span>
            <span>
              浏览－下单转化<b>{pct(data.orderConversion)}</b>
            </span>
            <span>
              退货件数<b>{data.returnUnits}</b>
            </span>
            <span>
              取消件数<b>{data.cancellationUnits}</b>
            </span>
            <span>
              产品取消率<b>{pct(data.cancellationRate)}</b>
            </span>
          </div>
        </section>
        <section className="card sync">
          <div className="card-title">
            同步状态 <span className="badge green">● 已连接</span>
          </div>
          <p>Ozon 数据已同步并保存至本地快照。</p>
          <div>
            <span>
              Ozon 连接<b>已配置</b>
            </span>
            <span>
              最近同步<b>{data.lastSync ?? "尚未同步"}</b>
            </span>
            <span>
              数据来源<b>Ozon 快照</b>
            </span>
          </div>
        </section>
      </div>
      <ProductInsights to={to} currency={currency} />
    </>
  );
}

function Orders({
  rows,
  currency,
  days,
  setDays,
  refreshing,
  refresh,
  query,
  setQuery,
}: {
  rows: OrderRow[];
  currency: string;
  days: number;
  setDays: (n: number) => void;
  refreshing: boolean;
  refresh: () => void;
  query: string;
  setQuery: (q: string) => void;
}) {
  const [page, setPage] = useState(0),
    pages = Math.max(1, Math.ceil(rows.length / 50)),
    visible = rows.slice(page * 50, page * 50 + 50);
  useEffect(() => setPage(0), [rows]);
  return (
    <>
      <Header
        eyebrow="ORDER OPERATIONS"
        title="订单中心"
        subtitle="FBS / FBO 订单状态、履约、商品、集群与基础配送"
        refreshing={refreshing}
        refresh={refresh}
      />
      <div className="orders-tools">
        <RangeTabs days={days} setDays={setDays} />
        <label className="search">
          <Search size={16} />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="订单号、货号、SKU 或状态"
          />
        </label>
        <span>匹配 {rows.length} 笔订单</span>
      </div>
      <section className="card table-card">
        <table>
          <thead>
            <tr>
              <th>订单</th>
              <th>履约</th>
              <th>状态</th>
              <th>货号 / SKU</th>
              <th>金额</th>
              <th>运输 / 配送</th>
              <th>基础配送</th>
              <th>创建 / 更新</th>
            </tr>
          </thead>
          <tbody>
            {visible.map((row) => (
              <tr key={row.eventId}>
                <td>
                  <b>{row.postingNumber}</b>
                  <small>{row.offerId || row.sku}</small>
                </td>
                <td>
                  <span className="scheme">{row.scheme || "—"}</span>
                </td>
                <td>
                  <b>{row.status || "—"}</b>
                  <small>
                    {row.status === "delivered"
                      ? "posting_received"
                      : "posting_in_transit"}
                  </small>
                </td>
                <td>
                  <div className="order-product">
                    {row.imageUrl ? (
                      <img
                        src={row.imageUrl}
                        alt={row.productName || row.offerId || row.sku || "商品图片"}
                        loading="lazy"
                        decoding="async"
                        onError={(e) => {
                          e.currentTarget.style.display = "none";
                        }}
                      />
                    ) : (
                      <span className="order-image-placeholder">◇</span>
                    )}
                    <div>
                      <b className="product">
                        {row.offerId || "—"} ×{row.quantity}
                      </b>
                      <small>{row.sku}</small>
                    </div>
                  </div>
                </td>
                <td>
                  <b>{money(row.amount, currency)}</b>
                </td>
                <td>
                  <b>{row.origin || "—"}</b>
                  <small>{row.destination || "—"}</small>
                </td>
                <td>
                  <b>
                    {row.estimatedDelivery == null
                      ? "—"
                      : money(row.estimatedDelivery, currency)}
                  </b>
                  <small>{row.estimateBasis}</small>
                </td>
                <td>
                  <span>{row.createdAt}</span>
                  <small>{row.updatedAt}</small>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {!rows.length && <div className="empty">当前条件下暂无订单</div>}
      </section>
      <div className="table-pagination">
        <button disabled={page === 0} onClick={() => setPage(page - 1)}>
          上一页
        </button>
        <span>
          第 {page + 1} / {pages} 页 · 每页 50 条
        </span>
        <button disabled={page + 1 >= pages} onClick={() => setPage(page + 1)}>
          下一页
        </button>
      </div>
    </>
  );
}

function AdvertisingTrend({ data }: { data: AdvertisingData["trend"] }) {
  useEffect(() => {
    const el = document.getElementById("advertising-trend");
    if (!el) return;
    const chart = echarts.init(el);
    chart.setOption({
      tooltip: { trigger: "axis" },
      legend: { top: 0 },
      grid: { left: 55, right: 55, top: 42, bottom: 28 },
      xAxis: {
        type: "category",
        data: data.map((x) => x.day),
        axisLabel: { hideOverlap: true },
      },
      yAxis: [
        { type: "value", name: "金额" },
        { type: "value", name: "订单" },
      ],
      series: [
        { name: "广告消耗", type: "bar", data: data.map((x) => x.spend) },
        {
          name: "广告销售额",
          type: "line",
          smooth: true,
          data: data.map((x) => x.revenue),
        },
        {
          name: "广告订单",
          type: "line",
          smooth: true,
          yAxisIndex: 1,
          data: data.map((x) => x.orders),
        },
      ],
    });
    const resize = () => chart.resize();
    window.addEventListener("resize", resize);
    return () => {
      window.removeEventListener("resize", resize);
      chart.dispose();
    };
  }, [data]);
  return <div id="advertising-trend" className="chart" />;
}

function Advertising({
  data,
  currency,
  days,
  setDays,
  refreshing,
  refresh,
}: {
  data: AdvertisingData;
  currency: string;
  days: number;
  setDays: (n: number) => void;
  refreshing: boolean;
  refresh: () => void;
}) {
  const [campaignQuery, setCampaignQuery] = useState("");
  const conversion = data.clicks ? (data.orders / data.clicks) * 100 : null,
    visibleCampaigns = data.campaigns.filter((x) =>
      `${x.name} ${x.id}`
        .toLowerCase()
        .includes(campaignQuery.trim().toLowerCase()),
    );
  return (
    <>
      <Header
        eyebrow="ADVERTISING CENTER"
        title="广告数据"
        subtitle="广告消耗、归因销售额与 ROAS 按当前店铺真实筛选汇总"
        refreshing={refreshing}
        refresh={refresh}
      />
      <RangeTabs days={days} setDays={setDays} />
      <section className="card trend-card">
        <div className="section-heading">
          <div>
            <h2>广告表现趋势</h2>
            <p>按所选日期尺度观察消耗、归因销售额和广告订单</p>
          </div>
        </div>
        <AdvertisingTrend data={data.trend} />
      </section>
      <div className="ad-stats">
        <Stat
          tone="purple"
          label="广告消耗"
          value={money(data.spend, currency)}
          note="活动总计"
        />
        <Stat
          tone="blue"
          label="广告销售额"
          value={money(data.revenue, currency)}
          note="真实归因"
        />
        <Stat
          tone="green"
          label="广告订单"
          value={String(data.orders)}
          note="归因订单量"
        />
        <Stat
          tone="orange"
          label="平均 CTR"
          value={pct(data.ctr)}
          note="曝光至点击"
        />
        <Stat
          tone="pink"
          label="平均 CPC"
          value={data.cpc == null ? "—" : money(data.cpc, currency)}
          note="单次点击费用"
        />
        <Stat
          tone="cyan"
          label="投产比 ROAS"
          value={data.roas?.toFixed(2) ?? "—"}
          note="销售额 / 消耗"
        />
      </div>
      <div className="ads-grid">
        <section className="card data-scope">
          <div className="card-title">
            真实广告数据范围 <span className="badge blue">真实汇总</span>
          </div>
          <p>
            经营导入已使用当前店铺、日期与 SKU 筛选；活动总计不会与 SKU
            明细重复计算。
          </p>
          <div className="scope-total">
            当前广告数据：消耗 {money(data.spend, currency)}，归因销售额{" "}
            {money(data.revenue, currency)}，ROAS {data.roas?.toFixed(2) ?? "—"}
            。
          </div>
        </section>
        <section className="card funnel">
          <h2>广告转化漏斗</h2>
          <p>由真实曝光与点击字段生成</p>
          <div className="funnel-step f1">
            <span>
              曝光<small>100%</small>
            </span>
            <b>{data.impressions || "—"}</b>
          </div>
          <div className="funnel-step f2">
            <span>
              点击<small>{pct(data.ctr)}</small>
            </span>
            <b>{data.clicks || "—"}</b>
          </div>
          <div className="funnel-step f3">
            <span>
              订单<small>{pct(conversion)}</small>
            </span>
            <b>{data.orders}</b>
          </div>
        </section>
      </div>
      <section className="card campaigns">
        <div className="card-title">
          广告计划表现
          <label className="search">
            <Search size={15} />
            <input
              value={campaignQuery}
              onChange={(e) => setCampaignQuery(e.target.value)}
              placeholder="按广告名称或计划 ID 筛选"
            />
          </label>
          <span>{visibleCampaigns.length} 个计划</span>
        </div>
        {visibleCampaigns.length ? (
          <table>
            <thead>
              <tr>
                <th>计划</th>
                <th>状态</th>
                <th>曝光</th>
                <th>点击</th>
                <th>订单</th>
                <th>消耗</th>
                <th>销售额</th>
                <th>ROAS</th>
              </tr>
            </thead>
            <tbody>
              {visibleCampaigns.map((x) => (
                <tr key={x.id}>
                  <td>
                    <b>{x.name || x.id}</b>
                    <small>{x.id}</small>
                  </td>
                  <td>{x.state || "—"}</td>
                  <td>{x.impressions}</td>
                  <td>{x.clicks}</td>
                  <td>{x.orders}</td>
                  <td>{money(x.spend, currency)}</td>
                  <td>{money(x.revenue, currency)}</td>
                  <td>{x.roas?.toFixed(2) ?? "—"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        ) : (
          <div className="empty">
            当前真实范围没有计划明细，系统不会用演示计划替代真实数据。
          </div>
        )}
      </section>
    </>
  );
}

function LazyFallback() {
  return <div className="card page-loading">正在加载模块…</div>;
}

export function App() {
  const [page, setPage] = useState<PageKey>("dashboard"),
    [workspace, setWorkspace] = useState<"ozon" | "wb">("ozon"),
    [wbPage, setWbPage] = useState<"daily" | "costs" | "settings">("daily"),
    [shops, setShops] = useState<Shop[]>([]),
    [days, setDays] = useState(7),
    [dash, setDash] = useState(emptyDashboard),
    [orderRows, setOrderRows] = useState<OrderRow[]>([]),
    [ads, setAds] = useState(emptyAds),
    [refreshing, setRefreshing] = useState(false),
    [query, setQuery] = useState(""),
    [productRows, setProductRows] = useState<ProductRow[]>([]),
    [inventoryRows, setInventoryRows] = useState<InventoryRow[]>([]),
    [connection, setConnection] = useState<ConnectionStatus | null>(null),
    [targetDays, setTargetDays] = useState(30);
  const activeShop = shops.find((s) => s.active) ?? shops[0],
    currency = activeShop?.kind === "cross_border" ? "CNY" : "RUB",
    range = useMemo(() => rangeFor(days), [days]),
    monthRange = useMemo(currentMonthRange, []),
    deferredQuery = useDeferredValue(query);
  const load = async () => {
    setRefreshing(true);
    try {
      if (page === "dashboard") setDash(await dashboard(range));
      if (page === "orders") setOrderRows(await orders(range, deferredQuery));
      if (page === "advertising") setAds(await advertising(range));
      if (page === "products")
        setProductRows(await products(range, deferredQuery));
      if (page === "inventory")
        setInventoryRows(await inventory(deferredQuery, targetDays));
      if (page === "settings") setConnection(await connectionStatus());
    } finally {
      setRefreshing(false);
    }
  };
  useEffect(() => {
    listShops().then(setShops);
  }, []);
  useEffect(() => {
    void load();
  }, [page, range.from, range.to, activeShop?.id, deferredQuery, targetDays]);
  const changeShop = async (id: string) => {
    await selectShop(id);
    clearReportCache();
    setShops((xs) => xs.map((x) => ({ ...x, active: x.id === id })));
  };
  const reloadShops = async () => setShops(await listShops());
  return (
    <div className="app">
      <Sidebar
        page={page}
        setPage={setPage}
        shops={shops}
        activeShop={activeShop}
        changeShop={changeShop}
        workspace={workspace}
        setWorkspace={setWorkspace}
        wbPage={wbPage}
        setWbPage={setWbPage}
      />
      <main>
        <Suspense fallback={<LazyFallback />}>
        {workspace === "wb" && <WbPage range={range} section={wbPage} />}
        {workspace === "ozon" && (
          <>
            {page === "dashboard" && (
              <Dashboard
                data={dash}
                currency={currency}
                days={days}
                setDays={setDays}
                refreshing={refreshing}
                refresh={load}
                to={range.to}
              />
            )}{" "}
            {page === "orders" && (
              <Orders
                rows={orderRows}
                currency={currency}
                days={days}
                setDays={setDays}
                refreshing={refreshing}
                refresh={load}
                query={query}
                setQuery={setQuery}
              />
            )}{" "}
            {page === "fbs" && <FbsPage currency={currency} />}{" "}
            {page === "products" && (
              <ProductsPage
                rows={productRows}
                currency={currency}
                query={query}
                setQuery={setQuery}
                reload={load}
              />
            )}{" "}
            {page === "advertising" && (
              <Advertising
                data={ads}
                currency={currency}
                days={days}
                setDays={setDays}
                refreshing={refreshing}
                refresh={load}
              />
            )}{" "}
            {page === "reports" && (
              <ReportsPage
                range={range}
                currency={currency}
                initialTab="daily"
              />
            )}{" "}
            {page === "monthly_profit" && (
              <ReportsPage
                range={monthRange}
                currency={currency}
                initialTab="summary"
              />
            )}{" "}
            {page === "weekly_report" && (
              <ReportsPage
                range={range}
                currency={currency}
                initialTab="weekly"
              />
            )}{" "}
            {page === "cross_profit" && (
              <ReportsPage
                range={monthRange}
                currency={currency}
                initialTab="cross"
              />
            )}{" "}
            {page === "ai" && <AiPage range={range} />}{" "}
            {page === "inventory" && (
              <InventoryPage
                rows={inventoryRows}
                query={query}
                setQuery={setQuery}
                targetDays={targetDays}
                setTargetDays={setTargetDays}
                reload={load}
              />
            )}{" "}
            {page === "supply" && <SupplyPage />}{" "}
            {page === "sync" && <SyncPage range={range} />}{" "}
            {page === "feishu" && <FeishuPage range={range} />}{" "}
            {page === "wb" && <WbPage range={range} section="daily" />}{" "}
            {page === "migration" && <MigrationPage range={range} />}{" "}
            {page === "listing" && <ListingPage />}{" "}
            {page === "competitors" && <CompetitorsPage />}{" "}
            {page === "shops" && (
              <ShopsPage
                shops={shops}
                activeShop={activeShop}
                select={changeShop}
                reload={reloadShops}
              />
            )}{" "}
            {page === "settings" && <SettingsPage status={connection} />}
          </>
        )}
        </Suspense>
      </main>
    </div>
  );
}
