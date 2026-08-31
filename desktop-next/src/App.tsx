import {
  startTransition,
  useDeferredValue,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import "./performance.css";
import "./layout-fixes.css";
import * as echarts from "./charts";
import {
  BarChart3,
  Box,
  BrainCircuit,
  CalendarDays,
  ChevronDown,
  ChevronRight,
  Database,
  LayoutDashboard,
  Megaphone,
  PackageSearch,
  PanelLeftClose,
  PanelLeftOpen,
  RefreshCw,
  Search,
  Settings2,
  ShoppingBag,
  Store,
  Target,
  Truck,
} from "lucide-react";
import {
  advertising,
  campaignAiAnalysis,
  campaignControl,
  campaignMonitor,
  connectionStatus,
  dashboard,
  inventory,
  listShops,
  orders,
  openListingSupplierUrl,
  products,
  selectShop,
  syncAllData,
} from "./bridge";
import type {
  AdvertisingData,
  CampaignMonitorData,
  ConnectionStatus,
  DashboardData,
  DateRange,
  InventoryRow,
  OrderRow,
  PageKey,
  ProductRow,
  Shop,
} from "./types";
import {
  InventoryPage,
  ProductsPage,
  SettingsPage,
  ShopsPage,
} from "./Phase2Pages";
import {
  AiPage,
  clearReportCache,
  CompetitorsPage,
  FbsPage,
  FeishuPage,
  ListingLedgerPage,
  MigrationPage,
  ReportsPage,
  SupplyPage,
  SyncPage,
  WbPage,
} from "./OperationsPages";
import { ProductInsights } from "./ProductInsights";
import { ProductDifferentiationPage } from "./ProductDifferentiationPage";
import { CrossBorderOperationsPage } from "./CrossBorderOperationsPage";
import { GrowthCenterPage } from "./GrowthCenterPage";
import { ProductAnalysisPage } from "./ProductAnalysisPage";

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
  returnRate: null,
  cancellationUnits: 0,
  cancellationRate: null,
  views: 0,
  orderConversion: null,
};
const orderCache = new Map<string, Promise<OrderRow[]>>();
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
  conversionRate: null,
  cpa: null,
  acos: null,
  breakEvenRoas: null,
  targetRoas: null,
  maxCpa: null,
  knownCostMargin: null,
  marginCoveragePercent: 0,
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
  // Ozon Seller “最近 30 天”报表按当前日向前跨 30 个日期边界，
  // 例如 8 月 26 日对应 7 月 27 日至 8 月 26 日（两端均包含）。
  from.setDate(to.getDate() - days);
  return { from: iso(from), to: iso(to) };
}
function currentMonthRange(): DateRange {
  const to = new Date(),
    from = new Date(to.getFullYear(), to.getMonth(), 1);
  return { from: iso(from), to: iso(to) };
}
function calendarMonthRange(offset: number): DateRange {
  const today = new Date(),
    from = new Date(today.getFullYear(), today.getMonth() - offset, 1),
    monthEnd = new Date(today.getFullYear(), today.getMonth() - offset + 1, 0),
    to = offset === 0 ? today : monthEnd;
  return { from: iso(from), to: iso(to) };
}
function recentMonthChoices() {
  const today = new Date();
  return [0, 1, 2].map((offset) => {
    const date = new Date(today.getFullYear(), today.getMonth() - offset, 1);
    return {
      offset,
      label: `${date.getFullYear()}年${date.getMonth() + 1}月`,
    };
  });
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
  collapsed,
  setCollapsed,
}: {
  page: PageKey;
  setPage: (page: PageKey) => void;
  shops: Shop[];
  activeShop?: Shop;
  changeShop: (id: string) => void;
  workspace: "ozon" | "wb";
  setWorkspace: (value: "ozon" | "wb") => void;
  wbPage: "daily" | "reports" | "orders" | "ads" | "inventory" | "costs" | "domestic_profit" | "cross_profit" | "settings";
  setWbPage: (value: "daily" | "reports" | "orders" | "ads" | "inventory" | "costs" | "domestic_profit" | "cross_profit" | "settings") => void;
  collapsed: boolean;
  setCollapsed: (value: boolean) => void;
}) {
  const groups: Array<{ id: string; label: string; icon: typeof LayoutDashboard; items: Array<[PageKey, string, typeof LayoutDashboard]> }> = [
    { id: "operations", label: "经营管理", icon: LayoutDashboard, items: [["dashboard", "经营总览", LayoutDashboard], ["orders", "订单中心", ShoppingBag], ["products", "商品中心", Box], ["fbs", "FBS 管理", Truck]] },
    { id: "marketing", label: "营销与洞察", icon: Target, items: [["growth_center", "增长中心", BarChart3], ["product_analysis", "产品分析", Target], ["advertising", "广告运营", Megaphone], ["competitors", "竞品跟踪", PackageSearch], ["differentiation", "亚马逊差异化选品", Target], ["ai", "AI 分析", BrainCircuit]] },
    { id: "reports", label: "报表与利润", icon: BarChart3, items: [["reports", "数据报告", BarChart3], ["monthly_profit", "月度盈亏", BarChart3], ["weekly_report", "经营周报", CalendarDays], ["cross_profit", "跨境店铺利润", BarChart3]] },
    { id: "inventory", label: "库存与供应链", icon: PackageSearch, items: [["inventory", "库存管理", PackageSearch], ["supply", "约仓计划", Truck]] },
    { id: "cross", label: "跨境运营", icon: Truck, items: [["cross_border_ops", "俄罗斯跨境经营", Truck], ["listing", "产品台账", PackageSearch]] },
    { id: "data", label: "数据与协作", icon: Database, items: [["sync", "数据同步", RefreshCw], ["feishu", "飞书协作", Database], ["migration", "数据迁移", Database]] },
    { id: "system", label: "系统设置", icon: Settings2, items: [["shops", "店铺管理", Store], ["settings", "连接设置", Settings2]] },
  ];
  const groupForPage = groups.find((group) => group.items.some(([key]) => key === page))?.id ?? "operations";
  const [openGroups, setOpenGroups] = useState<Record<string, boolean>>({ [groupForPage]: true });
  useEffect(() => setOpenGroups((old) => ({ ...old, [groupForPage]: true })), [groupForPage]);
  return (
    <aside className={`sidebar ${collapsed ? "collapsed" : ""}`}>
      <button className="sidebar-collapse" title={collapsed ? "展开导航栏" : "收起导航栏"} onClick={() => setCollapsed(!collapsed)}>
        {collapsed ? <PanelLeftOpen size={17} /> : <PanelLeftClose size={17} />}
      </button>
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
          <nav className="grouped-nav">
            {groups.map((group) => {
              const visibleItems = group.items.filter(([key]) => !(key === "monthly_profit" && activeShop?.kind === "cross_border"));
              const GroupIcon = group.icon;
              const opened = collapsed || openGroups[group.id];
              return <section className="nav-group" key={group.id}>
                <button className={`nav-group-toggle ${group.id === groupForPage ? "current" : ""}`} title={group.label} onClick={() => collapsed ? setCollapsed(false) : setOpenGroups((old) => ({ ...old, [group.id]: !old[group.id] }))}>
                  <GroupIcon size={17} /><span>{group.label}</span>{opened ? <ChevronDown className="nav-chevron" size={14} /> : <ChevronRight className="nav-chevron" size={14} />}
                </button>
                {opened && <div className="nav-group-items">{visibleItems.map(([key, label, Icon]) => <button key={key} title={label} className={key === page ? "active" : ""} onClick={() => startTransition(() => setPage(key))}><Icon size={16} /><span>{label}</span></button>)}</div>}
              </section>;
            })}
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
                ["reports", "报告中心", BarChart3],
                ["orders", "订单中心", ShoppingBag],
                ["ads", "广告运营", Megaphone],
                ["inventory", "仓库与库存", PackageSearch],
                ["costs", "商品与成本", Box],
                ["domestic_profit", "本土利润", BarChart3],
                ["cross_profit", "跨境利润", BarChart3],
                ["settings", "WB API 与汇率", Settings2],
              ] as Array<
                ["daily" | "reports" | "orders" | "ads" | "inventory" | "costs" | "domestic_profit" | "cross_profit" | "settings", string, typeof LayoutDashboard]
              >
            ).map(([key, label, Icon]) => (
              <button
                key={label}
                className={wbPage === key ? "active" : ""}
                onClick={() => setWbPage(key)}
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
  actions,
}: {
  eyebrow: string;
  title: string;
  subtitle: string;
  refreshing: boolean;
  refresh: () => void;
  actions?: ReactNode;
}) {
  return (
    <header className="page-header">
      <div>
        <span className="eyebrow">{eyebrow}</span>
        <h1>{title}</h1>
        <p>{subtitle}</p>
      </div>
      <div className="page-header-actions">
        {actions}
        <button className="dark-button" onClick={refresh} disabled={refreshing}>
          <RefreshCw size={16} className={refreshing ? "spin" : ""} />
          {refreshing ? "正在读取" : "刷新数据"}
        </button>
      </div>
    </header>
  );
}

function RangeTabs({
  days,
  setDays,
  monthOffset,
  setMonthOffset,
}: {
  days: number;
  setDays: (days: number) => void;
  monthOffset?: number | null;
  setMonthOffset?: (offset: number | null) => void;
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
            className={monthOffset == null && days === value ? "selected" : ""}
            onClick={() => {
              setMonthOffset?.(null);
              setDays(value as number);
            }}
            key={value}
          >
            {label}
          </button>
        ))}
        {setMonthOffset &&
          recentMonthChoices().map((choice) => (
            <button
              className={monthOffset === choice.offset ? "selected" : ""}
              onClick={() => setMonthOffset(choice.offset)}
              key={`month-${choice.offset}`}
            >
              {choice.label}
            </button>
          ))}
      </div>
      <div className="date-pill">
        <CalendarDays size={16} />
        {monthOffset == null ? `最近 ${days} 天` : `${calendarMonthRange(monthOffset).from} 至 ${calendarMonthRange(monthOffset).to}`}
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
  monthOffset,
  setMonthOffset,
  refreshing,
  refresh,
  range,
  to,
}: {
  data: DashboardData;
  currency: string;
  days: number;
  setDays: (n: number) => void;
  monthOffset: number | null;
  setMonthOffset: (offset: number | null) => void;
  refreshing: boolean;
  refresh: () => void;
  range: DateRange;
  to: string;
}) {
  const [chartMode, setChartMode] = useState<"revenue" | "units">("revenue"),
    [syncRange, setSyncRange] = useState<DateRange>(range),
    [forceSync, setForceSync] = useState(false),
    [syncingAll, setSyncingAll] = useState(false),
    [syncMessage, setSyncMessage] = useState("");
  useEffect(() => setSyncRange(range), [range.from, range.to]);
  const today = new Date().toISOString().slice(0, 10),
    syncRangeValid = !!syncRange.from && !!syncRange.to && syncRange.from <= syncRange.to && syncRange.to <= today;
  const syncEverything = async () => {
    if (!syncRangeValid) {
      setSyncMessage("请选择有效日期：结束日期不能早于开始日期，也不能晚于今天。");
      return;
    }
    setSyncingAll(true);
    setSyncMessage("Seller、Performance 和 Finance 正在并行同步…");
    try {
      const result = await syncAllData(syncRange, forceSync);
      clearReportCache();
      const items = [
        result.sellerError ? `Seller 失败：${result.sellerError}` : result.sellerRows === 0 ? "Seller 复用本地缓存" : `Seller ${result.sellerRows ?? 0} 行`,
        result.performanceError ? `Performance 失败：${result.performanceError}` : result.performanceRows === 0 ? "Performance 复用本地缓存" : `Performance ${result.performanceRows ?? 0} 行`,
        result.financeError ? `Finance 失败：${result.financeError}` : result.financeRows === 0 ? "Finance 复用本地缓存" : `Finance ${result.financeRows ?? 0} 行`,
      ];
      setSyncMessage(`同步完成：${items.join("；")}`);
      await refresh();
    } catch (error) {
      setSyncMessage(`同步启动失败：${String(error)}`);
    } finally {
      setSyncingAll(false);
    }
  };
  const avg = data.orders ? data.revenue / data.orders : 0;
  return (
    <>
      <Header
        eyebrow="运营中心"
        title="经营总览"
        subtitle="当前店铺经营表现与本地缓存快照"
        refreshing={refreshing}
        refresh={refresh}
        actions={
          <div className="dashboard-sync-actions">
            <div className="sync-mode-tabs compact">
              <button className={!forceSync ? "selected" : ""} onClick={() => setForceSync(false)}>智能</button>
              <button className={forceSync ? "selected danger" : ""} onClick={() => setForceSync(true)}>强制</button>
            </div>
            <label>开始<input type="date" value={syncRange.from} max={syncRange.to || today} onChange={(event) => setSyncRange((current) => ({ ...current, from: event.target.value }))} /></label>
            <label>结束<input type="date" value={syncRange.to} min={syncRange.from} max={today} onChange={(event) => setSyncRange((current) => ({ ...current, to: event.target.value }))} /></label>
            <button className="sync-all-button" disabled={syncingAll || refreshing || !syncRangeValid} onClick={() => void syncEverything()}>
              <RefreshCw size={16} className={syncingAll ? "spin" : ""} />
              {syncingAll ? "同步中" : "同步所有数据"}
            </button>
          </div>
        }
      />
      {syncMessage && <div className={`dashboard-sync-message ${syncMessage.includes("失败") ? "error" : ""}`}>{syncMessage}</div>}
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
            <RangeTabs days={days} setDays={setDays} monthOffset={monthOffset} setMonthOffset={setMonthOffset} />
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
              产品退货率<b>{pct(data.returnRate)}</b>
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
  crossBorder,
}: {
  rows: OrderRow[];
  currency: string;
  days: number;
  setDays: (n: number) => void;
  refreshing: boolean;
  refresh: () => void;
  query: string;
  setQuery: (q: string) => void;
  crossBorder: boolean;
}) {
  const [page, setPage] = useState(0),
    pages = Math.max(1, Math.ceil(rows.length / 50)),
    visible = rows.slice(page * 50, page * 50 + 50);
  useEffect(() => setPage(0), [rows]);
  return (
    <>
      <Header
        eyebrow="ORDER OPERATIONS"
        title={crossBorder ? "跨境订单中心" : "本土订单中心"}
        subtitle={crossBorder ? "RFBS / FBP / WHD 订单状态、履约与采购台账" : "FBS / FBO 订单状态、履约、商品、集群与基础配送"}
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
                        alt=""
                        loading="lazy"
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
                      {row.supplierUrl && (
                        <button
                          className="link-button"
                          onClick={() => void openListingSupplierUrl(row.supplierUrl)}
                        >
                          打开 1688
                        </button>
                      )}
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
  const [monitorId, setMonitorId] = useState<string | null>(null);
  const [monitorCache, setMonitorCache] = useState<Record<string, { data: CampaignMonitorData; at: number }>>({});
  const visibleCampaigns = data.campaigns.filter((x) =>
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
        <Stat tone="orange" label="广告 ACOS" value={pct(data.acos)} note="消耗 / 归因销售额" />
        <Stat tone="green" label="点击转化率" value={pct(data.conversionRate)} note="归因订单 / 点击" />
        <Stat tone="pink" label="单笔归因成本 CPA" value={data.cpa == null ? "—" : money(data.cpa, currency)} note="消耗 / 归因订单" />
      </div>
      <section className="card ad-financial-framework">
        <div className="card-title">利润约束与投放目标 <span className="badge orange">⚠ 已知成本估算</span></div>
        <p>依据已填写的采购成本和头程成本计算，不包含尚未归集的平台佣金、仓储、退货和其他费用，因此只作为广告上限参考。</p>
        <div className="ad-financial-grid">
          <div><span>成本覆盖率</span><b>{data.marginCoveragePercent.toFixed(1)}%</b><small>低于 80% 时不生成利润目标</small></div>
          <div><span>已知成本毛利率</span><b>{pct(data.knownCostMargin == null ? null : data.knownCostMargin * 100)}</b><small>销售额减采购与头程</small></div>
          <div><span>盈亏平衡 ROAS</span><b>{data.breakEvenRoas?.toFixed(2) ?? "—"}</b><small>1 ÷ 已知成本毛利率</small></div>
          <div><span>可持续目标 ROAS</span><b>{data.targetRoas?.toFixed(2) ?? "—"}</b><small>盈亏线 × 1.5 利润缓冲</small></div>
          <div><span>最大 CPA</span><b>{data.maxCpa == null ? "—" : money(data.maxCpa, currency)}</b><small>归因客单价 × 已知成本毛利率</small></div>
        </div>
      </section>
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
              订单<small>{pct(data.conversionRate)}</small>
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
                <th>CTR / CPC</th>
                <th>转化 / CPA</th>
                <th>诊断与建议</th>
              </tr>
            </thead>
            <tbody>
              {visibleCampaigns.map((x) => (
                <tr key={x.id} onDoubleClick={() => setMonitorId(x.id)} title="双击打开活动监控">
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
                  <td>{pct(x.ctr)}<small>{x.cpc == null ? "—" : money(x.cpc, currency)}</small></td>
                  <td>{pct(x.conversionRate)}<small>{x.cpa == null ? "—" : money(x.cpa, currency)}</small></td>
                  <td><div className={`ad-diagnosis ${x.diagnosisLevel}`}><b>{x.diagnosisText}</b><small>{x.recommendedAction}</small></div></td>
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
      {monitorId && (
        <CampaignMonitorModal
          campaignId={monitorId}
          currency={currency}
          initialData={monitorCache[monitorId]?.data ?? null}
          cacheFresh={Date.now() - (monitorCache[monitorId]?.at ?? 0) < 15 * 60 * 1000}
          onLoaded={(next) => setMonitorCache((old) => ({ ...old, [monitorId]: { data: next, at: Date.now() } }))}
          close={() => setMonitorId(null)}
          refreshParent={refresh}
        />
      )}
    </>
  );
}

function CampaignMonitorModal({ campaignId, currency, initialData, cacheFresh, onLoaded, close, refreshParent }: {
  campaignId: string; currency: string; initialData: CampaignMonitorData | null; cacheFresh: boolean;
  onLoaded: (data: CampaignMonitorData) => void; close: () => void; refreshParent: () => void;
}) {
  const [data, setData] = useState<CampaignMonitorData | null>(initialData);
  const [scale, setScale] = useState<"day" | "week" | "month">("day");
  const [budget, setBudget] = useState(initialData?.budgetKnown ? String(initialData.budget) : "");
  const [confirmation, setConfirmation] = useState("");
  const [busy, setBusy] = useState(false);
  const [ai, setAi] = useState("");
  const load = async () => {
    const next = await campaignMonitor(campaignId);
    setData(next); setBudget(next.budgetKnown ? String(next.budget) : ""); onLoaded(next);
  };
  useEffect(() => { if (!cacheFresh) void load(); }, [campaignId, cacheFresh]);
  const series = useMemo(() => {
    const grouped = new Map<string, { label: string; spend: number; revenue: number; orders: number }>();
    for (const row of data?.daily ?? []) {
      const d = new Date(`${row.day}T00:00:00`);
      const key = scale === "day" ? row.day : scale === "month"
        ? row.day.slice(0, 7)
        : `${d.getFullYear()}-W${String(Math.ceil((((d.getTime() - new Date(d.getFullYear(), 0, 1).getTime()) / 86400000) + new Date(d.getFullYear(), 0, 1).getDay() + 1) / 7)).padStart(2, "0")}`;
      const old = grouped.get(key) ?? { label: key, spend: 0, revenue: 0, orders: 0 };
      old.spend += row.spend; old.revenue += row.revenue; old.orders += row.orders; grouped.set(key, old);
    }
    return [...grouped.values()];
  }, [data, scale]);
  const execute = async (action: "activate" | "deactivate" | "budget") => {
    if (confirmation !== "确认执行") { alert("请输入“确认执行”后再操作。 "); return; }
    if (!window.confirm("该操作会写入 Ozon 广告账户，确定继续吗？")) return;
    setBusy(true);
    try {
      await campaignControl({ campaignId, action, weeklyBudget: action === "budget" ? Number(budget) : undefined, confirmation });
      setConfirmation(""); await load(); refreshParent();
    } catch (error) { alert(String(error)); } finally { setBusy(false); }
  };
  const total = series.reduce((a, x) => ({ spend: a.spend + x.spend, revenue: a.revenue + x.revenue, orders: a.orders + x.orders }), { spend: 0, revenue: 0, orders: 0 });
  return <div className="campaign-monitor-backdrop" onMouseDown={(e) => e.target === e.currentTarget && close()}>
    <div className="campaign-monitor">
      <button className="monitor-close" onClick={close}>×</button>
      <h2>{data?.name || campaignId}</h2><p>活动 ID {campaignId} · {data?.state || "读取中"}</p>
      <div className="monitor-kpis">
        <div><span>当前周预算</span><strong>{data ? (data.budgetKnown ? money(data.budget, currency) : "—") : "读取中"}</strong><small>{data?.budgetSource || "正在读取本地缓存与 Performance API"}</small></div>
        <div><span>区间花费</span><strong>{money(total.spend, currency)}</strong><small>{scale === "day" ? "日级" : scale === "week" ? "周级" : "月级"}汇总</small></div>
        <div><span>归因销售额</span><strong>{money(total.revenue, currency)}</strong><small>{total.orders} 个广告订单</small></div>
        <div><span>区间 ROAS</span><strong>{total.spend ? (total.revenue / total.spend).toFixed(2) : "—"}</strong><small>销售额 / 花费</small></div>
      </div>
      <div className="monitor-tabs">{(["day", "week", "month"] as const).map((x) => <button className={scale === x ? "active" : ""} onClick={() => setScale(x)} key={x}>{x === "day" ? "每日" : x === "week" ? "每周" : "每月"}</button>)}</div>
      <section className="monitor-chart"><CampaignEffectChart data={series} />{!series.length && <div className="empty">近两个月暂无日级缓存。</div>}</section>
      <section className="monitor-controls">
        <b>广告账户控制</b><input type="number" min="0" value={budget} onChange={(e) => setBudget(e.target.value)} placeholder="周预算" />
        <input value={confirmation} onChange={(e) => setConfirmation(e.target.value)} placeholder="输入：确认执行" />
        <button disabled={busy} onClick={() => void execute("budget")}>保存周预算</button><button disabled={busy} onClick={() => void execute("activate")}>开启</button><button disabled={busy} onClick={() => void execute("deactivate")}>关闭</button>
      </section>
      <section className="monitor-ai"><button disabled={busy} onClick={async () => { setBusy(true); try { setAi(await campaignAiAnalysis(campaignId)); } catch (e) { setAi(String(e)); } finally { setBusy(false); } }}>AI 分析调整效果与建议</button>{ai && <pre>{ai}</pre>}</section>
      <section className="monitor-logs"><h3>独立操作日志与调整后效果</h3>{data?.logs.length ? <table><thead><tr><th>时间</th><th>操作</th><th>前 → 后</th><th>操作前 7 天</th><th>操作后</th><th>状态</th></tr></thead><tbody>{data.logs.map((x) => <tr key={x.id}><td>{x.createdAt}</td><td>{x.action} {x.requestedValue}</td><td>{x.beforeState} / {x.beforeBudget} → {x.afterState} / {x.afterBudget}</td><td>{money(x.beforeSpend, currency)} · ROAS {x.beforeSpend ? (x.beforeRevenue / x.beforeSpend).toFixed(2) : "—"}</td><td>{money(x.afterSpend, currency)} · ROAS {x.afterSpend ? (x.afterRevenue / x.afterSpend).toFixed(2) : "—"}</td><td>{x.status}</td></tr>)}</tbody></table> : <div className="empty">暂无 ERP 操作记录。</div>}</section>
    </div>
  </div>;
}

function CampaignEffectChart({ data }: { data: Array<{ label: string; spend: number; revenue: number; orders: number }> }) {
  useEffect(() => {
    const el = document.getElementById("campaign-effect-chart");
    if (!el || !data.length) return;
    const chart = echarts.init(el);
    chart.setOption({
      grid: { left: 22, right: 62, top: 52, bottom: 34, containLabel: true },
      tooltip: { trigger: "axis" },
      legend: { top: 8, data: ["广告花费", "归因销售额", "ROAS"] },
      xAxis: { type: "category", boundaryGap: false, data: data.map((x) => x.label), axisLabel: { hideOverlap: true } },
      yAxis: [
        { type: "value", splitLine: { lineStyle: { color: "#edf1f7" } }, axisLabel: { color: "#8b98aa" } },
        { type: "value", name: "ROAS", splitLine: { show: false }, axisLabel: { color: "#8b98aa" } },
      ],
      series: [
        { name: "广告花费", type: "line", smooth: true, symbol: "circle", symbolSize: 5, data: data.map((x) => x.spend), lineStyle: { color: "#3478f6", width: 3 }, areaStyle: { color: new echarts.graphic.LinearGradient(0, 0, 0, 1, [{ offset: 0, color: "rgba(52,120,246,.28)" }, { offset: 1, color: "rgba(52,120,246,0)" }]) } },
        { name: "归因销售额", type: "line", smooth: true, symbol: "none", data: data.map((x) => x.revenue), lineStyle: { color: "#f39a32", width: 2 } },
        { name: "ROAS", type: "line", smooth: true, symbol: "none", yAxisIndex: 1, data: data.map((x) => x.spend ? Number((x.revenue / x.spend).toFixed(2)) : null), lineStyle: { color: "#69b83e", width: 2, type: "dashed" } },
      ],
    });
    const resize = () => chart.resize(); window.addEventListener("resize", resize);
    return () => { window.removeEventListener("resize", resize); chart.dispose(); };
  }, [data]);
  return <div id="campaign-effect-chart" className="campaign-effect-chart" />;
}

export function App() {
  const [page, setPage] = useState<PageKey>("dashboard"),
    [workspace, setWorkspace] = useState<"ozon" | "wb">("ozon"),
    [sidebarCollapsed, setSidebarCollapsed] = useState(false),
    [wbPage, setWbPage] = useState<"daily" | "reports" | "orders" | "ads" | "inventory" | "costs" | "domestic_profit" | "cross_profit" | "settings">("daily"),
    [shops, setShops] = useState<Shop[]>([]),
    [days, setDays] = useState(7),
    [dashboardMonthOffset, setDashboardMonthOffset] = useState<number | null>(null),
    [dash, setDash] = useState(emptyDashboard),
    [orderRows, setOrderRows] = useState<OrderRow[]>([]),
    [ads, setAds] = useState(emptyAds),
    [refreshing, setRefreshing] = useState(false),
    [query, setQuery] = useState(""),
    [productRows, setProductRows] = useState<ProductRow[]>([]),
    [inventoryRows, setInventoryRows] = useState<InventoryRow[]>([]),
    [connection, setConnection] = useState<ConnectionStatus | null>(null),
    [targetDays, setTargetDays] = useState(45),
    [leadTimeDays, setLeadTimeDays] = useState(21),
    [safetyDays, setSafetyDays] = useState(7);
  const activeShop = shops.find((s) => s.active) ?? shops[0],
    currency = activeShop?.kind === "cross_border" ? "CNY" : "RUB",
    range = useMemo(
      () => dashboardMonthOffset == null ? rangeFor(days) : calendarMonthRange(dashboardMonthOffset),
      [days, dashboardMonthOffset],
    ),
    monthRange = useMemo(currentMonthRange, []),
    deferredQuery = useDeferredValue(query);
  const load = async () => {
    setRefreshing(true);
    try {
      if (page === "dashboard") setDash(await dashboard(range));
      if (page === "orders") {
        const key = `${activeShop?.id || ""}|${range.from}|${range.to}|${deferredQuery}`;
        if (!orderCache.has(key)) orderCache.set(key, orders(range, deferredQuery));
        setOrderRows(await orderCache.get(key)!);
      }
      if (page === "advertising") setAds(await advertising(range));
      if (page === "products")
        setProductRows(await products(range, deferredQuery));
      if (page === "inventory")
        setInventoryRows(await inventory(deferredQuery, targetDays, leadTimeDays, safetyDays));
      if (page === "settings") setConnection(await connectionStatus());
    } finally {
      setRefreshing(false);
    }
  };
  useEffect(() => {
    listShops().then(setShops);
  }, []);
  useEffect(() => {
    const navigate = (event: Event) => {
      const detail = (event as CustomEvent<{ page?: PageKey; query?: string }>).detail;
      if (!detail?.page) return;
      if (detail.query != null) setQuery(detail.query);
      setWorkspace("ozon");
      setPage(detail.page);
    };
    window.addEventListener("ozon:navigate", navigate);
    return () => window.removeEventListener("ozon:navigate", navigate);
  }, []);
  useEffect(() => {
    void load();
  }, [page, range.from, range.to, activeShop?.id, deferredQuery, targetDays, leadTimeDays, safetyDays]);
  const changeShop = async (id: string) => {
    await selectShop(id);
    clearReportCache();
    orderCache.clear();
    setProductRows([]);
    const nextShop = shops.find((shop) => shop.id === id);
    if (nextShop?.kind === "cross_border" && page === "monthly_profit") {
      setPage("cross_profit");
    }
    setShops((xs) => xs.map((x) => ({ ...x, active: x.id === id })));
  };
  const reloadShops = async () => setShops(await listShops());
  return (
    <div className={`app ${sidebarCollapsed ? "sidebar-is-collapsed" : ""}`}>
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
        collapsed={sidebarCollapsed}
        setCollapsed={setSidebarCollapsed}
      />
      <main>
        {workspace === "wb" && <WbPage range={range} section={wbPage} days={days} setDays={setDays} />}
        {workspace === "ozon" && (
          <>
            {page === "dashboard" && (
              <Dashboard
                data={dash}
                currency={currency}
                days={days}
                setDays={setDays}
                monthOffset={dashboardMonthOffset}
                setMonthOffset={setDashboardMonthOffset}
                refreshing={refreshing}
                refresh={load}
                range={range}
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
                refresh={() => {
                  orderCache.clear();
                  void load();
                }}
                query={query}
                setQuery={setQuery}
                crossBorder={activeShop?.kind === "cross_border"}
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
                key={activeShop?.id}
                range={range}
                currency={currency}
                initialTab="daily"
              />
            )}{" "}
            {page === "monthly_profit" && activeShop?.kind !== "cross_border" && (
              <ReportsPage
                key={activeShop?.id}
                range={monthRange}
                currency={currency}
                initialTab="summary"
              />
            )}{" "}
            {page === "weekly_report" && (
              <ReportsPage
                key={activeShop?.id}
                range={range}
                currency={currency}
                initialTab="weekly"
              />
            )}{" "}
            {page === "cross_profit" && (
              <ReportsPage
                key={activeShop?.id}
                range={monthRange}
                currency={currency}
                initialTab="cross"
              />
            )}{" "}
            {page === "ai" && <AiPage range={range} />}{" "}
            {page === "growth_center" && <GrowthCenterPage range={range} currency={currency} shopName={activeShop?.name || "当前店铺"} />}{" "}
            {page === "product_analysis" && <ProductAnalysisPage currency={currency} />}{" "}
            {page === "inventory" && (
              <InventoryPage
                rows={inventoryRows}
                query={query}
                setQuery={setQuery}
                targetDays={targetDays}
                setTargetDays={setTargetDays}
                leadTimeDays={leadTimeDays}
                setLeadTimeDays={setLeadTimeDays}
                safetyDays={safetyDays}
                setSafetyDays={setSafetyDays}
                reload={load}
              />
            )}{" "}
            {page === "supply" && <SupplyPage />}{" "}
            {page === "sync" && <SyncPage range={range} />}{" "}
            {page === "feishu" && <FeishuPage range={range} />}{" "}
            {page === "wb" && <WbPage range={range} section="daily" days={days} setDays={setDays} />}{" "}
            {page === "migration" && <MigrationPage range={range} />}{" "}
            {page === "listing" && <ListingLedgerPage />}{" "}
            {page === "cross_border_ops" && <CrossBorderOperationsPage range={range} shopName={activeShop?.name || "当前店铺"} enabled={activeShop?.kind === "cross_border"} />}{" "}
            {page === "competitors" && (
              <CompetitorsPage key={activeShop?.id} />
            )}{" "}
            {page === "differentiation" && <ProductDifferentiationPage />}{" "}
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
      </main>
    </div>
  );
}
