import { OzonBidObservations } from "./OzonBidObservations";
import {
  startTransition,
  useDeferredValue,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import type { InsightRow } from "./types";
import "./performance.css";
import "./layout-fixes.css";
import NetDashboard from "./NetDashboard";
import { PaymentRates } from "./PaymentRates";
import { WarehouseSwitch } from "./WarehouseSwitch";
import * as echarts from "./charts";
import {
  BarChart3,
  BadgePercent,
  BadgeRussianRuble,
  Box,
  BrainCircuit,
  CalendarDays,
  CircleAlert,
  ChevronDown,
  ChevronRight,
  Database,
  Download,
  FileText,
  FilePlus2,
  GitBranch,
  LayoutDashboard,
  Link2,
  ListTodo,
  Megaphone,
  PackageSearch,
  Network,
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
  campaignInventory,
  campaignMonitor,
  connectionStatus,
  dashboard,
  inventory,
  getInventoryAlertState,
  acknowledgeInventoryAlert,
  listShops,
  orders,
  syncFboOrders,
  openListingSupplierUrl,
  products,
  selectShop,
  syncAllData,
} from "./bridge";
import type {
  AdvertisingData,
  CampaignMonitorData,
  CampaignInventoryRow,
  ConnectionStatus,
  DashboardData,
  DateRange,
  InventoryRow,
  InventoryAlertState,
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
} from "./OperationsPages";
import { AdvertisingSeriesPanel } from "./AdvertisingSeriesPanel";
import { ProductInsights } from "./ProductInsights";
import { LocalWeeklyReportPage } from "./LocalWeeklyReportPage";
import { ProductDifferentiationPage } from "./ProductDifferentiationPage";
import { NewProductProfitPage } from "./NewProductProfitPage";
import { LocalSelectionPage } from "./LocalSelectionPage";
import { SelectionLibraryPage } from "./SelectionLibraryPage";
import { CrossBorderOperationsPage } from "./CrossBorderOperationsPage";
import { GrowthCenterPage } from "./GrowthCenterPage";
import { ProductAnalysisPage } from "./ProductAnalysisPage";
import { AdExperimentCenter, type ExperimentSeed } from "./AdExperimentCenter";
import { MindMapPage } from "./MindMapPage";
import { MercadoLibrePage } from "./MercadoLibrePage";
import { DailyTaskCenter } from "./DailyTaskCenter";
import { AdAttributionPage } from "./AdAttributionPage";
import { PurchaseContractPage } from "./PurchaseContractPage";
import { ProcurementBoard } from "./ProcurementBoard";
import { PurchaseOrderPage } from "./PurchaseOrderPage";
import { FreightQuotePage } from "./FreightQuotePage";
import { ProductMasterPage } from "./ProductMasterPage";
import { WbWorkspace, type WbSection } from "./WbWorkspace";
import { PackingDocumentsPage } from "./PackingDocumentsPage";

import { PromotionCenter } from "./PromotionCenter";
import { DeliveryFeeQueryPage } from "./DeliveryFeeQueryPage";
import { ProductTransferPage } from "./ProductTransferPage";
import { CompetitorShopPage } from "./CompetitorShopPage";
import { PriceIntelligencePage } from "./PriceIntelligencePage";

type Workspace = "ozon" | "product_library" | "wb" | "mercadolibre";
type WbPageKey = WbSection;

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
  clickSpend: 0,
  orderSpend: 0,
  unclassifiedSpend: 0,
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
  campaignTrend: {},
  campaigns: [],
  products: [],
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
  workspace: Workspace;
  setWorkspace: (value: Workspace) => void;
  wbPage: WbPageKey;
  setWbPage: (value: WbPageKey) => void;
  collapsed: boolean;
  setCollapsed: (value: boolean) => void;
}) {
  const groups: Array<{ id: string; label: string; icon: typeof LayoutDashboard; items: Array<[PageKey, string, typeof LayoutDashboard]> }> = [
    { id: "operations", label: "经营管理", icon: LayoutDashboard, items: [["dashboard", "经营总览", LayoutDashboard], ["daily_tasks", "每日任务", ListTodo], ["procurement_board", "采购履约看板", LayoutDashboard], ["purchase_orders", "采购单添加", FilePlus2], ["contracts", "采购合同", FileText], ["orders", "订单中心", ShoppingBag], ["products", "商品中心", Box], ["fbs", "FBS 管理", Truck]] },
    { id: "marketing", label: "营销与洞察", icon: Target, items: [["growth_center", "增长中心", BarChart3], ["product_analysis", "产品分析", Target], ["selection_library", "选品资料库", Database], ["local_selection", "Ozon 本土选品", Target], ["new_product_profit", "新品利润测算", BarChart3], ["competitor_shop", "竞品店铺拆解", Store], ["advertising", "广告运营", Megaphone], ["promotions", "广告促销", BadgePercent], ["ad_attribution", "系列广告归因", GitBranch], ["ad_experiments", "广告优化实验中心", Target], ["competitors", "竞品跟踪", PackageSearch], ["differentiation", "亚马逊差异化选品", Target], ["ai", "AI 分析", BrainCircuit]] },
    { id: "reports", label: "报表与利润", icon: BarChart3, items: [["price_intelligence", "价格与利润监控", BadgePercent], ["reports", "数据报告", BarChart3], ["monthly_profit", "月度盈亏", BarChart3], ["weekly_report", "经营周报", CalendarDays], ["cross_profit", "跨境店铺利润", BarChart3]] },
    { id: "inventory", label: "库存与供应链", icon: PackageSearch, items: [["inventory", "库存管理", PackageSearch], ["delivery_fee_query", "基础配送费查询", BadgeRussianRuble], ["freight_quotes", "物流询价配货表", Truck], ["packing", "补货装箱", FileText], ["supply", "约仓计划", Truck]] },
    { id: "cross", label: "跨境运营", icon: Truck, items: [["product_transfer", "商品搬家", Store], ["cross_border_ops", "俄罗斯跨境经营", Truck], ["listing", "产品台账", PackageSearch]] },
    { id: "data", label: "数据与协作", icon: Database, items: [["mind_map", "可视化报告", Network], ["sync", "数据同步", RefreshCw], ["feishu", "飞书协作", Database], ["migration", "数据迁移", Database]] },
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
        onClick={() => setWorkspace(workspace === "ozon" ? "wb" : workspace === "wb" ? "mercadolibre" : "ozon")}
      >
        <div className="brand-mark">
          <Database size={18} />
        </div>
        <div>
          <b>{workspace === "ozon" ? "Ozon ERP" : workspace === "product_library" ? "产品库" : workspace === "wb" ? "WB ERP" : "美客多 ERP"}</b>
          <small>
            {workspace === "ozon" ? "切换至 WB 工作区" : workspace === "wb" ? "切换至美客多工作区" : "返回 Ozon 工作区"}
          </small>
        </div>
      </button>
      <nav className="workspace-primary-nav">
        <button
          className={workspace === "product_library" ? "active" : ""}
          onClick={() => setWorkspace("product_library")}
          title="产品库"
        >
          <Box size={17} />
          <span>产品库</span>
        </button>
      </nav>
      {workspace !== "mercadolibre" && workspace !== "product_library" && <label className="shop-picker">
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
      </label>}
      {workspace === "ozon" && (
        <>
          <div className="nav-label">工作台</div>
          <nav className="grouped-nav">
            {groups.map((group) => {
              const visibleItems = group.items;
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
                ["orders", "订单中心", ShoppingBag],
                ["returns", "退货中心", Truck],
                ["pricing", "价格与利润监控", BarChart3],
                ["promotions", "促销活动", BadgePercent],
                ["products", "商品中心", Box],
                ["weekly", "经营周报", CalendarDays],
                ["monthly", "经营月报", BarChart3],
                ["inventory", "库存管理", PackageSearch],
                ["ad_optimization", "广告优化与评估", Megaphone],
              ] as Array<
                [WbPageKey, string, typeof LayoutDashboard]
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
      {workspace === "mercadolibre" && <><div className="nav-label">美客多工作台</div><nav><button className="active"><Store size={17}/>商品与经营</button></nav></>}
      {workspace === "product_library" && <><div className="nav-label">产品资料中心</div><nav><button className="active"><Database size={17}/><span>标准产品与多店铺铺货</span></button></nav></>}
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
  customRange,
  setCustomRange,
  dateLabel = "趋势",
}: {
  days: number;
  setDays: (days: number) => void;
  monthOffset?: number | null;
  setMonthOffset?: (offset: number | null) => void;
  customRange?: DateRange | null;
  setCustomRange?: (range: DateRange | null) => void;
  dateLabel?: string;
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
            className={!customRange && monthOffset == null && days === value ? "selected" : ""}
            onClick={() => {
              setMonthOffset?.(null);
              setCustomRange?.(null);
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
              className={!customRange && monthOffset === choice.offset ? "selected" : ""}
              onClick={() => { setCustomRange?.(null); setMonthOffset(choice.offset); }}
              key={`month-${choice.offset}`}
            >
              {choice.label}
            </button>
          ))}
      </div>
      {setCustomRange && <div className="dashboard-custom-range">
        <label>开始<input aria-label={dateLabel + "开始日期"} type="date" value={customRange?.from || ""} onChange={event => setCustomRange({from:event.target.value,to:customRange?.to && customRange.to >= event.target.value ? customRange.to : event.target.value})}/></label>
        <label>结束<input aria-label={dateLabel + "结束日期"} type="date" value={customRange?.to || ""} min={customRange?.from || undefined} onChange={event => setCustomRange({from:customRange?.from || event.target.value,to:event.target.value})}/></label>
      </div>}
      <div className="date-pill">
        <CalendarDays size={16} />
        {customRange ? `${customRange.from} 至 ${customRange.to}` : monthOffset == null ? `最近 ${days} 天` : `${calendarMonthRange(monthOffset).from} 至 ${calendarMonthRange(monthOffset).to}`}
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
}: {
  data: DashboardData["trend"];
}) {
  useEffect(() => {
    const el = document.getElementById("trend-chart");
    if (!el) return;
    const chart = echarts.init(el);
    chart.setOption({
      grid: { left: 20, right: 125, top: 46, bottom: 28, containLabel: true },
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
        { type: "value", position: "right", offset: 65, name: "销量/件", splitLine: { show: false }, axisLabel: { color: "#31a36b" } },
      ],
      legend: {
        left: "center",
        top: 6,
        itemGap: 24,
        data: ["销售额", "销量", "广告花费"],
      },
      series: [
        {
          name: "销售额",
          type: "line",
          smooth: true,
          symbol: "none",
          data: data.map((x) => x.revenue),
          lineStyle: { color: "#3478f6", width: 3 },
          areaStyle: {
            color: new echarts.graphic.LinearGradient(0, 0, 0, 1, [
              { offset: 0, color: "rgba(52,120,246,.22)" },
              { offset: 1, color: "rgba(52,120,246,0)" },
            ]),
          },
        },
        { name: "销量", type: "line", smooth: true, symbol: "none", yAxisIndex: 2, data: data.map((x) => x.units), lineStyle: { color: "#31a36b", width: 2 }, itemStyle: { color: "#31a36b" } },
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
  }, [data]);
  return <div id="trend-chart" className="chart" />;
}

type OverviewScope = { kind: "products" | "series"; id: string; name: string };
type OverviewScopeDay = DashboardData["trend"][number] & {
  adRevenue: number; adOrders: number; clicks: number; impressions: number;
  returns: number | null; cancellations: number | null; hasSales: boolean; hasAds: boolean;
};

function ScopedOverviewMetrics({ rows, currency }: { rows: OverviewScopeDay[]; currency: string }) {
  const total = (metric: "units" | "revenue" | "adSpend" | "adRevenue" | "adOrders" | "clicks" | "impressions" | "returns" | "cancellations") =>
    rows.reduce((sum, row) => sum + (row[metric] ?? 0), 0);
  const hasSales = rows.some((row) => row.hasSales);
  const hasAds = rows.some((row) => row.hasAds);
  const revenue = total("revenue"), units = total("units"), spend = total("adSpend"), adRevenue = total("adRevenue");
  const ratio = (ready: boolean, numerator: number, denominator: number) => pct(ready && denominator > 0 ? numerator / denominator * 100 : null);
  return <div className="metric-strip">
    <Stat tone="blue" label="销售额" value={hasSales ? money(revenue, currency) : "—"} note="所选范围" />
    <Stat tone="green" label="销量" value={hasSales ? String(units) : "—"} note="下单件数" />
    <Stat tone="blue" label="平均售价" value={hasSales && units > 0 ? money(revenue / units, currency) : "—"} note="销售额 ÷ 销量" />
    <Stat tone="orange" label="广告花费" value={hasAds ? money(spend, currency) : "—"} note="商品级归因" />
    <Stat tone="purple" label="广告销售额" value={hasAds ? money(adRevenue, currency) : "—"} note="商品级归因" />
    <Stat tone="purple" label="广告订单量" value={hasAds ? String(total("adOrders")) : "—"} note="商品级归因" />
    <Stat tone="purple" label="ACOS" value={ratio(hasAds, spend, adRevenue)} note="广告花费 ÷ 广告销售额" />
    <Stat tone="purple" label="TACOS" value={ratio(hasAds && hasSales, spend, revenue)} note="广告花费 ÷ 销售额" />
    <Stat tone="purple" label="CTR" value={ratio(hasAds, total("clicks"), total("impressions"))} note="点击 ÷ 曝光" />
    <Stat tone="green" label="退货件数" value={rows.some((row) => row.returns != null) ? String(total("returns")) : "—"} note="SKU 事件 / 销售统计" />
    <Stat tone="green" label="取消件数" value={rows.some((row) => row.cancellations != null) ? String(total("cancellations")) : "—"} note="SKU 事件 / 销售统计" />
  </div>;
}

function Dashboard({
  data,
  currency,
  days,
  setDays,
  monthOffset,
  setMonthOffset,
  customRange,
  setCustomRange,
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
  customRange: DateRange | null;
  setCustomRange: (range: DateRange | null) => void;
  refreshing: boolean;
  refresh: () => void;
  range: DateRange;
  to: string;
}) {
  const [syncRange, setSyncRange] = useState<DateRange>(range),
    [forceSync, setForceSync] = useState(false),
    [syncingAll, setSyncingAll] = useState(false),
    [syncMessage, setSyncMessage] = useState("");
  const [scope, setScope] = useState<OverviewScope | null>(null);
  const [scopeReport, setScopeReport] = useState<{ key: string; rows?: OverviewScopeDay[]; error?: string } | null>(null);
  const trendSection = useRef<HTMLElement>(null);
  const scopePicker = useRef<HTMLDivElement>(null);
  const scopeKey = JSON.stringify([scope?.kind, scope?.id, range.from, range.to]);
  useEffect(() => {
    if (!scope || refreshing || syncingAll) return;
    let current = true;
    setScopeReport(null);
    void invoke<OverviewScopeDay[]>("overview_scope_daily", {
      range: { from: range.from, to: range.to },
      sku: scope.kind === "products" ? scope.id : null,
      seriesId: scope.kind === "series" ? Number(scope.id) : null,
    }).then((rows) => {
      if (current) setScopeReport({ key: scopeKey, rows });
    }).catch((error) => {
      if (current) setScopeReport({ key: scopeKey, error: String(error) });
    });
    return () => { current = false; };
  }, [scope, scopeKey, range.from, range.to, refreshing, syncingAll]);
  const report = !refreshing && !syncingAll && scopeReport?.key === scopeKey ? scopeReport : null;
  const openOverview = (kind: OverviewScope["kind"], row: InsightRow) => {
    setScope({ kind, id: row.id, name: row.name || row.offerIds[0] || row.id });
    trendSection.current?.scrollIntoView({ block: "start", behavior: "smooth" });
  };
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
      {!scope && <>
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
      </>}
      <section className="card trend-card" ref={trendSection} aria-label="业绩趋势">
        <div className="section-heading">
          <div>
            <h2>{scope ? `${scope.kind === "series" ? "系列" : "单品"} · ${scope.name}` : "业绩趋势 · 整店"}</h2>
            <p>
              {scope ? `${range.from} 至 ${range.to}${scope.kind === "products" ? ` · SKU ${scope.id}` : ""} · 商品级广告归因` : "销售额、销量与广告花费的日度变化"}
            </p>
          </div>
          <div className="trend-actions">
            <div className="tabs">
              {scope && <button type="button" onClick={() => setScope(null)}>返回整店</button>}
              <button type="button" onClick={() => scopePicker.current?.scrollIntoView({ block: "start", behavior: "smooth" })}>选择单品或系列</button>
            </div>
            <RangeTabs days={days} setDays={setDays} monthOffset={monthOffset} setMonthOffset={setMonthOffset} customRange={customRange} setCustomRange={setCustomRange} />
          </div>
        </div>
        {scope ? (
          report?.error ? <div className="error-banner" role="alert">读取看板失败：{report.error}</div> :
          report?.rows ? <>
            <ScopedOverviewMetrics rows={report.rows} currency={currency} />
            {report.rows.some((row) => row.hasSales || row.hasAds || row.returns != null || row.cancellations != null)
              ? <TrendChart data={report.rows} />
              : <p role="status">所选日期内暂无该单品或系列的销售和广告数据。</p>}
          </> : <p role="status">正在读取看板数据…</p>
        ) : <>
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
        <TrendChart data={data.trend} />
        </>}
      </section>
      {!scope && <>
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
      <NetDashboard days={data.netDaily ?? []} to={range.to} currency={currency} />
      </>}
      <div ref={scopePicker}>
        <ProductInsights to={to} currency={currency} onOpenOverview={openOverview} range={range} refreshing={refreshing || syncingAll} />
      </div>
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
  pickupOnly,
  setPickupOnly,
  range,
  reload,
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
  pickupOnly: boolean;
  setPickupOnly: (value: boolean) => void;
  range: DateRange;
  reload: () => void;
}) {
  const [page, setPage] = useState(0),
    pages = Math.max(1, Math.ceil(rows.length / 50)),
    visible = rows.slice(page * 50, page * 50 + 50);
  const [syncing, setSyncing] = useState(false);
  const [syncMessage, setSyncMessage] = useState("");
  useEffect(() => setPage(0), [rows]);
  async function syncSources() {
    setSyncing(true);
    setSyncMessage("");
    try {
      await syncFboOrders(range);
      reload();
      setSyncMessage(`${range.from} 至 ${range.to} 的发货来源已同步。`);
    } catch (error) {
      setSyncMessage(`同步发货来源失败：${String(error)}`);
    } finally {
      setSyncing(false);
    }
  }
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
      <div className="order-origin-tools">
        <label>
          <input type="checkbox" checked={pickupOnly} onChange={e => setPickupOnly(e.target.checked)} />
          只看从取货点发货
        </label>
        <button className="date-pill" disabled={syncing || refreshing} onClick={() => void syncSources()}>
          <RefreshCw size={14} />
          {syncing ? "正在同步…" : "同步发货来源"}
        </button>
        <span role="status">{syncMessage || "历史订单可同步当前日期范围，补齐发货来源。"}</span>
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
                  {row.storageType === "pvz" && <span className="badge purple order-pickup-badge">从取货点</span>}
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


function SelectedCampaignTrend({daily, selected, range, currency}: {
  daily: AdvertisingData["campaignTrend"]; selected: Set<string>; range: DateRange; currency: string;
}) {
  const container = useRef<HTMLDivElement>(null);
  const {from, to} = range;
  const byDay = useMemo(()=>{
    const sums = new Map<string, AdvertisingData["trend"][number]>();
    for (const id of selected) {
      for (const row of daily[id] ?? []) {
        const total = sums.get(row.day);
        if (!total) sums.set(row.day, {...row});
        else {
          total.impressions += row.impressions;
          total.clicks += row.clicks;
          total.orders += row.orders;
          total.spend += row.spend;
          total.revenue += row.revenue;
        }
      }
    }
    return sums;
  },[daily,selected]);

  useEffect(()=>{
    if (!container.current) return;
    const chart = echarts.init(container.current);
    const days: string[] = [];
    for (let time=Date.parse(from); time<=Date.parse(to); time+=86_400_000) {
      days.push(new Date(time).toISOString().slice(0,10));
    }
    const symbol = currency==="CNY"?"¥":"₽";
    const metrics = [
      {key:"impressions",name:"曝光",axis:0,color:"#2869f5"},
      {key:"spend",name:"广告花费",axis:1,color:"#e57a20"},
      {key:"clicks",name:"点击",axis:0,color:"#0c9b79"},
      {key:"orders",name:"广告订单",axis:0,color:"#8257cf"},
      {key:"revenue",name:"归因销售额",axis:1,color:"#cf4770"},
    ] as const;
    chart.setOption({
      tooltip:{trigger:"axis",confine:true},
      legend:{top:0,selected:{"点击":false,"广告订单":false,"归因销售额":false}},
      grid:{left:88,right:96,top:58,bottom:38},
      xAxis:{type:"category",boundaryGap:false,data:days,axisLabel:{hideOverlap:true}},
      yAxis:[
        {type:"value",name:"数量",minInterval:1,axisLabel:{formatter:(value:number)=>value.toLocaleString("zh-CN")}},
        {type:"value",name:"金额 / "+symbol,splitLine:{show:false},axisLabel:{formatter:(value:number)=>value.toLocaleString("zh-CN")}},
      ],
      series:metrics.map(metric=>({
        name:metric.name,type:"line",yAxisIndex:metric.axis,showSymbol:days.length<=31,
        symbolSize:5,connectNulls:false,lineStyle:{width:2,color:metric.color},itemStyle:{color:metric.color},
        tooltip:{valueFormatter:(value:unknown)=>typeof value==="number" ?
          metric.axis===1 ? money(value,currency) : value.toLocaleString("zh-CN") : "—"},
        data:days.map(day=>byDay.get(day)?.[metric.key] ?? null),
      })),
    });
    const resize=new ResizeObserver(()=>chart.resize());
    resize.observe(container.current);
    return ()=>{resize.disconnect();chart.dispose();};
  },[byDay,from,to,currency]);
  return byDay.size ? <div ref={container} className="ozon-selection-chart" role="img" aria-label="所选广告活动每日投放趋势"/> :
    <p className="ozon-selection-coverage">所选活动在此区间暂无每日投放数据。</p>;
}

function Advertising({
  data,
  currency,
  days,
  setDays,
  refreshing,
  refresh,
  shopId,
  crossBorder,
  reportingRange,
  statsCurrent,
  customRange,
  setCustomRange,
}: {
  data: AdvertisingData;
  currency: string;
  days: number;
  setDays: (n: number) => void;
  refreshing: boolean;
  refresh: () => void;
  shopId: string;
  crossBorder: boolean;
  reportingRange: DateRange;
  statsCurrent: boolean;
  customRange: DateRange | null;
  setCustomRange: (range: DateRange | null) => void;
}) {
  const modules = [
    ["campaigns", "活动与预算"],
    ["overview", "投放概览"],
    ["series", "产品系列"],
    ["products", "单品分析"],
  ] as const;
  const [module, setModule] = useState<(typeof modules)[number][0]>("campaigns");
  const [campaignQuery, setCampaignQuery] = useState("");
  const [productQuery, setProductQuery] = useState("");
  const [monitorId, setMonitorId] = useState<string | null>(null);
  const [monitorCache, setMonitorCache] = useState<Record<string, { data: CampaignMonitorData; at: number }>>({});
  const [seriesSelection, setSeriesSelection] = useState<Set<string>>(new Set());
  const [inventoryRows, setInventoryRows] = useState<CampaignInventoryRow[]>([]);
  const [performanceSelection, setPerformanceSelection] = useState<Set<string>>(new Set());
  const [selectedCampaigns, setSelectedCampaigns] = useState<Set<string>>(new Set());
  const [inventoryBusy, setInventoryBusy] = useState(false);
  const [batchBusy, setBatchBusy] = useState(false);
  const [batchMessage, setBatchMessage] = useState("");
  const [batchProgress, setBatchProgress] = useState("");
  const loadCampaignInventory = async () => {
    setInventoryBusy(true);
    try { setInventoryRows(await campaignInventory()); setBatchMessage(""); }
    catch (error) { setBatchMessage(`读取活动清单失败：${String(error)}`); }
    finally { setInventoryBusy(false); }
  };
  useEffect(() => { void loadCampaignInventory(); }, [shopId]);
  const runCampaignBatch = async (action: "activate" | "deactivate", scope: "all" | "selected") => {
    if (batchBusy) return;
    setBatchBusy(true);
    try {
      const fresh = await campaignInventory();
      setInventoryRows(fresh);
      const targets = scope === "all" ? fresh : fresh.filter((item) => selectedCampaigns.has(item.id));
      if (!targets.length) { setBatchMessage(scope === "all" ? "当前店铺没有广告活动" : "所选活动已不在当前店铺，请重新选择"); return; }
      const verb = action === "deactivate" ? "暂停" : "重启";
      if (!window.confirm(`即将${verb}当前跨境店的 ${targets.length} 个广告活动，逐个写入 Ozon 并回读状态。确认执行吗？`)) return;
      let success = 0;
      const failures: string[] = [];
      for (const [index, item] of targets.entries()) {
        setBatchProgress(`${verb}中 ${index + 1}/${targets.length}：${item.name || item.id}`);
        try { await campaignControl({ campaignId: item.id, action, confirmation: "确认执行", expectedShopId: shopId }); success++; }
        catch (error) {
          failures.push(`${item.name || item.id}：${String(error)}`);
          if (String(error).includes("当前店铺已切换")) break;
        }
      }
      setBatchMessage(`${verb}完成：成功 ${success}/${targets.length}${failures.length ? `；失败 ${failures.length}：${failures.slice(0, 3).join("；")}` : ""}`);
      setSelectedCampaigns(new Set());
      try { setInventoryRows(await campaignInventory()); }
      catch (error) { setBatchMessage((old) => `${old}；刷新活动状态失败：${String(error)}`); }
      refresh();
    } catch (error) { setBatchMessage(`批量操作失败：${String(error)}`); }
    finally { setBatchProgress(""); setBatchBusy(false); }
  };
  const inventoryById = new Map(inventoryRows.map(row => [row.id,row]));
  const statsById = new Map((statsCurrent ? data.campaigns : []).map(row => [row.id,row]));
  const activityRows = [...new Set([...statsById.keys(),...inventoryById.keys()])].map(id=>{
    const stats=statsById.get(id), item=inventoryById.get(id);
    return {id,stats,name:item?.name || stats?.name,state:item?.state || stats?.state,budget:item?.budget};
  }).filter(row=>((row.name ?? "")+" "+row.id).toLowerCase().includes(campaignQuery.trim().toLowerCase()));
  const matchedStats = [...performanceSelection].flatMap(id => {
    const stats = statsById.get(id);
    return stats ? [stats] : [];
  });
  const totals = matchedStats.length ? matchedStats.reduce((sum, row) => ({
    impressions: sum.impressions + row.impressions, clicks: sum.clicks + row.clicks,
    orders: sum.orders + row.orders, spend: sum.spend + row.spend, revenue: sum.revenue + row.revenue,
  }), {impressions:0,clicks:0,orders:0,spend:0,revenue:0}) : null;
  const allVisibleSelected = activityRows.length > 0 && activityRows.every(row=>performanceSelection.has(row.id));
  const hiddenSelection = performanceSelection.size - activityRows.filter(row=>performanceSelection.has(row.id)).length;
  function selectPerformance(ids: string[], checked: boolean) {
    setPerformanceSelection(current=>{
      const next = new Set(current);
      for (const id of ids) {
        if (checked) next.add(id);
        else next.delete(id);
      }
      return next;
    });
  }
  const visibleProducts = data.products.filter((x) =>
    `${x.sku} ${x.offerId} ${x.name}`.toLowerCase().includes(productQuery.trim().toLowerCase()),
  );
  const toggleSeriesSku = (sku: string) => setSeriesSelection(current => {
    const next = new Set(current);
    next.has(sku) ? next.delete(sku) : next.add(sku);
    return next;
  });
  return (
    <>
      <Header
        eyebrow="ADVERTISING CENTER"
        title="广告数据"
        subtitle="广告消耗、归因销售额与 ROAS 按当前店铺真实筛选汇总"
        refreshing={refreshing}
        refresh={refresh}
      />
      <nav className="ozon-ad-modules" aria-label="广告运营功能">
        {modules.map(([id, label]) => (
          <button key={id} type="button" aria-pressed={module === id}
            onClick={() => setModule(id)}>{label}</button>
        ))}
      </nav>
      {module !== "series" && <RangeTabs days={days} setDays={setDays} customRange={customRange} setCustomRange={setCustomRange} dateLabel="广告统计" />}
      {module === "campaigns" && <>
      <section className="card ozon-selection-board">
        <div className="section-heading">
          <div><h2>所选活动看板</h2><p>{performanceSelection.size
            ? "已选 " + performanceSelection.size + " 个活动 · " + reportingRange.from + " 至 " + reportingRange.to
            : "勾选下方活动，查看合计投放表现。"}</p></div>
          <button className="ozon-ad-button" disabled={!performanceSelection.size} onClick={()=>setPerformanceSelection(new Set())}>清空选择</button>
        </div>
        {performanceSelection.size>0 && <>
          <p className="ozon-selection-coverage" role="status">{!statsCurrent
            ? refreshing ? "正在读取所选日期的数据…" : "所选日期的数据尚未加载，可刷新数据。"
            : matchedStats.length < performanceSelection.size
              ? "当前有 " + matchedStats.length + " 个所选活动返回统计。其余活动暂无此区间的数据，合计仅涵盖已返回的活动。"
              : "全部所选活动均有区间统计。"}
            {hiddenSelection>0 && " 当前搜索隐藏了 "+hiddenSelection+" 个已选活动。"}
          </p>
          {statsCurrent && <SelectedCampaignTrend daily={data.campaignTrend} selected={performanceSelection} range={reportingRange} currency={currency}/>}
          <dl className="ozon-selection-metrics">{[
            ["总曝光", totals?.impressions.toLocaleString("zh-CN")],
            ["广告花费", totals && money(totals.spend,currency)],
            ["总点击", totals?.clicks.toLocaleString("zh-CN")],
            ["广告订单", totals?.orders.toLocaleString("zh-CN")],
            ["归因销售额", totals && money(totals.revenue,currency)],
            ["CTR", totals?.impressions ? pct(100*totals.clicks/totals.impressions) : null],
            ["平均 CPC", totals?.clicks ? money(totals.spend/totals.clicks,currency) : null],
            ["ROAS", totals?.spend ? (totals.revenue/totals.spend).toFixed(2) : null],
            ["ACOS", totals?.revenue ? pct(100*totals.spend/totals.revenue) : null],
          ].map(([label,value])=><div key={label}><dt>{label}</dt><dd>{value ?? "—"}</dd></div>)}</dl>
        </>}
      </section>
      <section className="card campaigns ozon-campaign-list">
        <div className="card-title">
          广告活动
          <label className="search"><Search size={15}/><input value={campaignQuery} onChange={e=>setCampaignQuery(e.target.value)} placeholder="搜索活动名称或 ID"/></label>
          <button className="ozon-ad-button" disabled={inventoryBusy || batchBusy} onClick={()=>void loadCampaignInventory()}>刷新活动清单</button>
        </div>
        {!crossBorder && batchMessage && <p role="status">{batchMessage}</p>}
        <div className="ozon-ad-table"><table>
          <thead><tr><th><input type="checkbox" aria-label="选择当前搜索结果" title="选择当前搜索结果" disabled={!activityRows.length} checked={allVisibleSelected} onChange={e=>selectPerformance(activityRows.map(row=>row.id),e.target.checked)}/></th><th>ID</th><th>活动名称</th><th>状态</th><th>计费</th><th>周预算</th><th>曝光</th><th>点击</th><th>订单</th><th>消耗</th><th>归因销售额</th><th>ROAS</th><th>CTR / CPC</th><th>转化 / CPA</th><th>诊断与建议</th></tr></thead>
          <tbody>{activityRows.map(({id,name,state,budget,stats})=><tr key={id} data-selected={performanceSelection.has(id)} onDoubleClick={()=>setMonitorId(id)}>
            <td onDoubleClick={e=>e.stopPropagation()}><input type="checkbox" aria-label={"选择活动 "+(name || id)} checked={performanceSelection.has(id)} onChange={e=>selectPerformance([id],e.target.checked)}/></td>
            <td>{id}</td><td><button className="ozon-ad-link" onClick={()=>setMonitorId(id)}>{name || id}</button></td>
            <td>{state ? ({"CAMPAIGN_STATE_RUNNING":"投放中","CAMPAIGN_STATE_STOPPED":"已暂停","CAMPAIGN_STATE_ARCHIVED":"已归档"} as Record<string,string>)[state] ?? state : "—"}</td>
            <td>{stats?.paymentType || "—"}</td><td>{budget == null ? "—" : money(budget,"RUB")}</td>
            <td>{stats?.impressions ?? "—"}</td><td>{stats?.clicks ?? "—"}</td><td>{stats?.orders ?? "—"}</td>
            <td>{stats ? money(stats.spend,currency) : "—"}</td><td>{stats ? money(stats.revenue,currency) : "—"}</td>
            <td>{stats?.roas?.toFixed(2) ?? "—"}</td><td>{pct(stats?.ctr ?? null)}<small>{stats?.cpc == null ? "—" : money(stats.cpc,currency)}</small></td>
            <td>{pct(stats?.conversionRate ?? null)}<small>{stats?.cpa == null ? "—" : money(stats.cpa,currency)}</small></td>
            <td>{stats ? <div className={`ad-diagnosis ${stats.diagnosisLevel}`}><b>{stats.diagnosisText}</b><small>{stats.recommendedAction}</small></div> : "—"}</td>
          </tr>)}</tbody>
        </table></div>
        {!activityRows.length && <p className="empty">没有匹配的广告活动。</p>}
      </section>
      {crossBorder && <section className="card campaign-batch-panel">
        <div className="card-title">跨境店广告活动控制 <span>{inventoryRows.length} 个活动（账户完整清单）</span></div>
        <p>选择活动后批量暂停或重启；“暂停全部”会读取当前店铺最新活动清单，不受页面日期和搜索条件限制。每个活动单独记录操作结果。</p>
        <div className="campaign-batch-actions">
          <button disabled={inventoryBusy || batchBusy} onClick={() => void loadCampaignInventory()}>{inventoryBusy ? "读取中" : "刷新活动清单"}</button>
          <button disabled={batchBusy || inventoryBusy || !selectedCampaigns.size} onClick={() => void runCampaignBatch("deactivate", "selected")}>暂停所选（{selectedCampaigns.size}）</button>
          <button disabled={batchBusy || inventoryBusy || !selectedCampaigns.size} onClick={() => void runCampaignBatch("activate", "selected")}>重启所选（{selectedCampaigns.size}）</button>
          <button className="danger" disabled={batchBusy || inventoryBusy || !inventoryRows.length} onClick={() => void runCampaignBatch("deactivate", "all")}>一键暂停全部活动</button>
        </div>
        <div className="campaign-batch-list"><label><input type="checkbox" checked={inventoryRows.length > 0 && inventoryRows.every((item) => selectedCampaigns.has(item.id))} onChange={(event) => setSelectedCampaigns(event.target.checked ? new Set(inventoryRows.map((item) => item.id)) : new Set())}/>选择全部</label>{inventoryRows.map((item) => <label key={item.id}><input type="checkbox" checked={selectedCampaigns.has(item.id)} onChange={(event) => setSelectedCampaigns((old) => { const next = new Set(old); event.target.checked ? next.add(item.id) : next.delete(item.id); return next; })}/><span>{item.name || item.id}<small>{item.id} · {item.state || "状态未知"}</small></span></label>)}</div>
        {(batchProgress || batchMessage) && <div className="sync-message" role="status">{batchProgress || batchMessage}</div>}
      </section>}
      </>}
      <div hidden={module !== "series"}>
        <AdvertisingSeriesPanel shopId={shopId} selection={seriesSelection} setSelection={setSeriesSelection} />
      </div>
      {module === "overview" && <>
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
        <Stat tone="blue" label="按点击费用" value={money(data.clickSpend, currency)} note="CPC / 按点击计划" />
        <Stat tone="orange" label="按订单费用" value={money(data.orderSpend, currency)} note="CPO / 按订单推广" />
        {data.unclassifiedSpend > 0 && <Stat tone="pink" label="未分类广告费" value={money(data.unclassifiedSpend, currency)} note="计费类型未由 API 返回" />}
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
      </>}
      {module === "products" && <section className="card campaigns product-ad-metrics">
        <div className="card-title">
          单品 ACOS 与 TACOS
          <label className="search"><Search size={15} /><input value={productQuery} onChange={(e) => setProductQuery(e.target.value)} placeholder="搜索 SKU、货号或商品名" /></label>
          <span>{visibleProducts.length} 个单品</span>
        </div>
        <p className="metric-definition">同一所选周期计算：ACOS＝广告费 ÷ 广告归因销售额；TACOS＝广告费 ÷ 单品全部销售额。仅展示能匹配到 SKU 的广告明细，店铺级未分摊广告不强行归属。</p>
        {visibleProducts.length ? <table>
          <thead><tr><th>系列</th><th>单品</th><th>计费类型</th><th>曝光 / 点击</th><th>广告订单</th><th>总广告费</th><th>按点击</th><th>按订单</th><th>广告销售额</th><th>总销售额</th><th>归因占比</th><th>ACOS</th><th>TACOS</th><th>ROAS</th></tr></thead>
          <tbody>{visibleProducts.map((x)=><tr key={x.sku}>
            <td><input type="checkbox" checked={seriesSelection.has(x.sku)} onChange={() => toggleSeriesSku(x.sku)} aria-label={`选择 SKU ${x.sku}`} /></td>
            <td><b>{x.offerId || x.sku}</b><small>{x.sku}{x.name ? ` · ${x.name}` : ""}</small></td>
            <td>{x.billingTypes || "未分类"}{x.unclassifiedSpend > 0 && <small>未分类 {money(x.unclassifiedSpend,currency)}</small>}</td><td>{x.impressions} / {x.clicks}</td><td>{x.orders}</td><td>{money(x.spend,currency)}</td><td>{money(x.clickSpend,currency)}</td><td>{money(x.orderSpend,currency)}</td><td>{money(x.adRevenue,currency)}</td><td>{money(x.totalRevenue,currency)}</td><td>{x.totalRevenue > 0 ? pct(x.adRevenue / x.totalRevenue * 100) : "—"}</td>
            <td><b className={x.acos != null && x.acos > 30 ? "metric-risk" : ""}>{pct(x.acos)}</b></td>
            <td><b className={x.tacos != null && x.tacos > 15 ? "metric-risk" : ""}>{pct(x.tacos)}</b></td><td>{x.roas?.toFixed(2) ?? "—"}</td>
          </tr>)}</tbody>
        </table>:<div className="empty">当前周期没有可归属到 SKU 的广告明细。请同步商品级广告统计；店铺级广告汇总无法可靠计算单品 ACOS/TACOS。</div>}
      </section>}
      {monitorId && (
        <CampaignMonitorModal
          shopId={shopId}
          campaignId={monitorId}
          currency={currency}
          initialData={monitorCache[monitorId]?.data ?? null}
          cacheFresh={Date.now() - (monitorCache[monitorId]?.at ?? 0) < 15 * 60 * 1000}
          onLoaded={(next) => setMonitorCache((old) => ({ ...old, [monitorId]: { data: next, at: Date.now() } }))}
          close={() => setMonitorId(null)}
          refreshParent={() => {refresh(); void loadCampaignInventory();}}
        />
      )}
    </>
  );
}

function CampaignProductPreview({data}: {data: CampaignMonitorData | null}) {
  const [search,setSearch]=useState("");
  const [image,setImage]=useState("");
  const rows=(data?.products ?? []).filter(row=>[row.sku,row.offerId,row.name].join(" ").toLowerCase().includes(search.trim().toLowerCase()));
  return <section className="ozon-campaign-products">
    <div className="section-heading"><div><h3>活动商品</h3><p>{data ? [data.productSource,data.productFrom+" 至 "+data.productTo].join(" · ") : "正在读取活动商品…"}</p></div>
      <input aria-label="筛选活动商品" placeholder="搜索 SKU、货号或名称" value={search} onChange={e=>setSearch(e.target.value)}/></div>
    <div className="ozon-ad-table"><table><thead><tr><th>SKU / 货号</th><th>商品</th><th>平均 CPC</th><th>广告订单</th><th>归因销售额</th><th>消耗</th><th>ACOS</th><th>曝光</th><th>点击</th></tr></thead>
      <tbody>{rows.map(row=><tr key={row.sku}><td>{row.sku}<small>{row.offerId}</small></td>
        <td><div className="ozon-ad-product">{row.imageUrl?.startsWith("https://") && <button className="ozon-ad-thumb" onClick={()=>setImage(row.imageUrl!)} aria-label="预览商品图片"><img src={row.imageUrl} alt="" loading="lazy"/></button>}<b>{row.name || row.offerId || "—"}</b></div></td>
        <td>{row.clicks && row.spend != null ? money(row.spend/row.clicks,"RUB") : "—"}</td><td>{row.orders ?? "—"}</td>
        <td>{row.revenue == null ? "—" : money(row.revenue,"RUB")}</td><td>{row.spend == null ? "—" : money(row.spend,"RUB")}</td>
        <td>{row.revenue && row.spend != null ? pct(row.spend / row.revenue * 100) : "—"}</td><td>{row.impressions ?? "—"}</td><td>{row.clicks ?? "—"}</td></tr>)}</tbody>
    </table></div>
    {data && !rows.length && <p>暂无匹配商品。</p>}
    {image && <div className="ozon-ad-image-preview" role="dialog" aria-label="商品图片"><button type="button" onClick={()=>setImage("")}>关闭</button><img src={image} alt="商品图片"/></div>}
  </section>;
}
function CampaignMonitorModal({ shopId, campaignId, currency, initialData, cacheFresh, onLoaded, close, refreshParent }: {
  shopId: string; campaignId: string; currency: string; initialData: CampaignMonitorData | null; cacheFresh: boolean;
  onLoaded: (data: CampaignMonitorData) => void; close: () => void; refreshParent: () => void;
}) {
  const [data, setData] = useState<CampaignMonitorData | null>(initialData);
  const [scale, setScale] = useState<"day" | "week" | "month">("day");
  const [budget, setBudget] = useState(initialData?.budgetKnown ? String(initialData.budget) : "");
  const [confirmation, setConfirmation] = useState("");
  const [busy, setBusy] = useState(false);
  const [ai, setAi] = useState("");
  const loadVersion = useRef(0);
  const load = async () => {
    const version = ++loadVersion.current;
    const next = await campaignMonitor(campaignId);
    setData(next); setBudget(next.budgetKnown ? String(next.budget) : ""); onLoaded(next);
  };
  useEffect(() => { if (!cacheFresh) void load(); }, [campaignId, cacheFresh]);
  const series = useMemo(() => {
    const grouped = new Map<string, { label: string; spend: number; revenue: number; orders: number; clicks: number; impressions: number }>();
    for (const row of data?.daily ?? []) {
      const d = new Date(`${row.day}T00:00:00`);
      const key = scale === "day" ? row.day : scale === "month"
        ? row.day.slice(0, 7)
        : `${d.getFullYear()}-W${String(Math.ceil((((d.getTime() - new Date(d.getFullYear(), 0, 1).getTime()) / 86400000) + new Date(d.getFullYear(), 0, 1).getDay() + 1) / 7)).padStart(2, "0")}`;
      const old = grouped.get(key) ?? { label: key, spend: 0, revenue: 0, orders: 0, clicks: 0, impressions: 0 };
      old.spend += row.spend; old.revenue += row.revenue; old.orders += row.orders; old.clicks += row.clicks; old.impressions += row.impressions; grouped.set(key, old);
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
      <CampaignProductPreview data={data} />
      <OzonBidObservations products={data?.products} key={shopId+":"+campaignId} shopId={shopId} campaignId={campaignId} cpcDay={data?.cpcDay} skuCpc={data?.skuCpc} />
      <div className="monitor-kpis">
        <div><span>当前周预算</span><strong>{data ? (data.budgetKnown ? money(data.budget, currency) : "—") : "读取中"}</strong><small>{data?.budgetSource || "正在读取本地缓存与 Performance API"}</small></div>
        <div><span>区间花费</span><strong>{money(total.spend, currency)}</strong><small>{scale === "day" ? "日级" : scale === "week" ? "周级" : "月级"}汇总</small></div>
        <div><span>归因销售额</span><strong>{money(total.revenue, currency)}</strong><small>{total.orders} 个广告订单</small></div>
        <div><span>区间 ROAS</span><strong>{total.spend ? (total.revenue / total.spend).toFixed(2) : "—"}</strong><small>销售额 / 花费</small></div>
      </div>
      <div className="monitor-tabs">{(["day", "week", "month"] as const).map((x) => <button className={scale === x ? "active" : ""} onClick={() => setScale(x)} key={x}>{x === "day" ? "每日" : x === "week" ? "每周" : "每月"}</button>)}</div>
      <section className="monitor-chart"><CampaignEffectChart data={series} /><p className="metric-definition">平均 CPC = 所选期间广告花费 ÷ 点击量，无点击时留空。</p>{!series.length && <div className="empty">近两个月暂无日级缓存。</div>}</section>
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

function CampaignEffectChart({ data }: { data: Array<{ label: string; spend: number; revenue: number; orders: number; clicks: number; impressions: number }> }) {
  useEffect(() => {
    const el = document.getElementById("campaign-effect-chart");
    if (!el || !data.length) return;
    const chart = echarts.init(el);
    chart.setOption({
      grid: { left: 78, right: 110, top: 54, bottom: 34, containLabel: true },
      tooltip: { trigger: "axis" },
      legend: { top: 8, data: ["广告花费", "归因销售额", "ROAS", "平均 CPC", "曝光数"] },
      xAxis: { type: "category", boundaryGap: false, data: data.map((x) => x.label), axisLabel: { hideOverlap: true } },
      yAxis: [
        { type: "value", splitLine: { lineStyle: { color: "#edf1f7" } }, axisLabel: { color: "#8b98aa" } },
        { type: "value", name: "ROAS", position: "right", splitLine: { show: false }, axisLabel: { color: "#668343" } },
        { type: "value", name: "CPC / ₽", position: "right", offset: 55, min: 0, splitLine: { show: false }, axisLabel: { color: "#754cc1" } },
        { type: "value", name: "曝光 / 次", position: "left", offset: 62, min: 0, minInterval: 1, splitLine: { show: false }, axisLabel: { color: "#087e85" } },
      ],
      series: [
        { name: "广告花费", type: "line", smooth: true, symbol: "circle", symbolSize: 5, data: data.map((x) => x.spend), lineStyle: { color: "#3478f6", width: 3 }, areaStyle: { color: new echarts.graphic.LinearGradient(0, 0, 0, 1, [{ offset: 0, color: "rgba(52,120,246,.28)" }, { offset: 1, color: "rgba(52,120,246,0)" }]) } },
        { name: "归因销售额", type: "line", smooth: true, symbol: "none", data: data.map((x) => x.revenue), lineStyle: { color: "#f39a32", width: 2 } },
        { name: "ROAS", type: "line", smooth: true, symbol: "none", yAxisIndex: 1, data: data.map((x) => x.spend ? Number((x.revenue / x.spend).toFixed(2)) : null), lineStyle: { color: "#69b83e", width: 2, type: "dashed" } },
        { name: "平均 CPC", type: "line", yAxisIndex: 2, connectNulls: false,
          symbol: "circle", symbolSize: 5, itemStyle: { color: "#8054ce" }, lineStyle: { width: 2 },
          data: data.map(row => row.clicks > 0 ? Number((Math.abs(row.spend) / row.clicks).toFixed(4)) : null),
          tooltip: { valueFormatter: (value: unknown) => typeof value === "number" ? ("₽" + value.toFixed(4)) : "—" } },
        { name: "曝光数", type: "line", yAxisIndex: 3, symbol: "none",
          itemStyle: { color: "#10969d" }, lineStyle: { width: 2 }, data: data.map(row => row.impressions),
          tooltip: { valueFormatter: (value: unknown) => typeof value === "number" ? value.toLocaleString("zh-CN") + " 次" : "—" } },
      ],
    });
    const resize = () => chart.resize(); window.addEventListener("resize", resize);
    return () => { window.removeEventListener("resize", resize); chart.dispose(); };
  }, [data]);
  return <div id="campaign-effect-chart" className="campaign-effect-chart" />;
}

export function App() {
  const [experimentSeed, setExperimentSeed] = useState<ExperimentSeed | undefined>();
  const [inventoryAlert, setInventoryAlert] = useState<InventoryAlertState | null>(null);
  useEffect(() => {
    const open = (event: Event) => { setExperimentSeed((event as CustomEvent<ExperimentSeed>).detail); setPage("ad_experiments"); };
    window.addEventListener("create-ad-experiment", open);
    return () => window.removeEventListener("create-ad-experiment", open);
  }, []);
  const [page, setPage] = useState<PageKey>("dashboard"),
    [workspace, setWorkspace] = useState<Workspace>("ozon"),
    [sidebarCollapsed, setSidebarCollapsed] = useState(false),
    [wbPage, setWbPage] = useState<WbPageKey>("daily"),
    [shops, setShops] = useState<Shop[]>([]),
    [days, setDays] = useState(7),
    [dashboardMonthOffset, setDashboardMonthOffset] = useState<number | null>(null),
    [advertisingCustomRange, setAdvertisingCustomRange] = useState<DateRange | null>(null),
    [dashboardCustomRange, setDashboardCustomRange] = useState<DateRange | null>(null),
    [dash, setDash] = useState(emptyDashboard),
    [orderRows, setOrderRows] = useState<OrderRow[]>([]),
    [pickupOnly, setPickupOnly] = useState(false),
    [orderRevision, setOrderRevision] = useState(0),
    [ads, setAds] = useState(emptyAds),
    [adsScope, setAdsScope] = useState(""),
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
      () => page === "advertising" && advertisingCustomRange?.from && advertisingCustomRange?.to && advertisingCustomRange.from <= advertisingCustomRange.to ? advertisingCustomRange : page === "dashboard" && dashboardCustomRange?.from && dashboardCustomRange?.to && dashboardCustomRange.from <= dashboardCustomRange.to ? dashboardCustomRange : page === "dashboard" && dashboardMonthOffset != null ? calendarMonthRange(dashboardMonthOffset) : rangeFor(days),
      [page, days, dashboardMonthOffset, dashboardCustomRange, advertisingCustomRange],
    ),
    monthRange = useMemo(currentMonthRange, []),
    deferredQuery = useDeferredValue(query);
  useEffect(()=>{setExperimentSeed(undefined);},[activeShop?.id]);
  const currentAdsScope = JSON.stringify([activeShop?.id, range.from, range.to]);
  const loadVersion = useRef(0);
  const load = async () => {
    const version = ++loadVersion.current;
    setRefreshing(true);
    try {
      if (page === "dashboard") {
        const result = await dashboard(range);
        if (version === loadVersion.current) setDash(result);
      }
      if (page === "orders") {
        const key = JSON.stringify([activeShop?.id, range.from, range.to, deferredQuery, pickupOnly]);
        if (!orderCache.has(key)) orderCache.set(key, orders(range, deferredQuery, pickupOnly).catch(error => {
          orderCache.delete(key);
          throw error;
        }));
        const result = await orderCache.get(key)!;
        if (version === loadVersion.current) setOrderRows(result);
      }
      if (page === "advertising") {
        const result = await advertising(range);
        if (version === loadVersion.current) {
          setAds(result);
          setAdsScope(currentAdsScope);
        }
      }
      if (page === "products")
        setProductRows(await products(range, deferredQuery));
      if (page === "inventory") {
        const result = await inventory(deferredQuery, targetDays, leadTimeDays, safetyDays);
        if (version === loadVersion.current) setInventoryRows(result);
      }
      if (page === "settings") setConnection(await connectionStatus());
    } finally {
      if (version === loadVersion.current) setRefreshing(false);
    }
  };
  useEffect(() => {
    listShops().then(setShops);
  }, []);
  useEffect(() => {
    let active = true;
    const check = () => getInventoryAlertState().then((value) => {
      if (active) setInventoryAlert(value.pendingNotification ? value : null);
    }).catch(() => undefined);
    void check();
    const timer = window.setInterval(check, 20_000);
    return () => { active = false; window.clearInterval(timer); };
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
  }, [page, range.from, range.to, activeShop?.id, deferredQuery, pickupOnly, orderRevision, targetDays, leadTimeDays, safetyDays]);
  const changeShop = async (id: string) => {
    await selectShop(id);
    clearReportCache();
    orderCache.clear();
    setProductRows([]);
    setShops((xs) => xs.map((x) => ({ ...x, active: x.id === id })));
  };
  const reloadShops = async () => setShops(await listShops());
  return (
    <div className={`app ${sidebarCollapsed ? "sidebar-is-collapsed" : ""}`}>
      {inventoryAlert && (
        <div className="inventory-alert-popup-backdrop" role="alertdialog" aria-modal="true">
          <div className="inventory-alert-popup">
            <div className="inventory-alert-popup-icon"><CircleAlert size={26} /></div>
            <div>
              <span className="eyebrow">INVENTORY WARNING</span>
              <h2>库存已达到预警线</h2>
              <p>{inventoryAlert.lastMessage}</p>
            </div>
            <div className="inventory-alert-popup-list">
              {inventoryAlert.products.filter((item) => item.lastCheckedAt.slice(0, 10) === inventoryAlert.lastRunDay && item.lastSellableDays !== null && item.lastSellableDays < item.threshold).map((item) => (
                <div key={`${item.shopId}-${item.sku}`}>
                  <span><b>{item.offerId || item.sku}</b><small>{item.shopName} · {item.productName || `SKU ${item.sku}`}</small></span>
                  <span>库存 <strong>{item.lastStock}</strong></span>
                  <span>可售 <strong>{item.lastSellableDays?.toFixed(1)} 天</strong></span>
                  <span>预警 <strong>{item.threshold} 天</strong></span>
                </div>
              ))}
            </div>
            <div className="modal-actions">
              <button className="outline-button" onClick={() => { setWorkspace("ozon"); setPage("inventory"); }}>查看库存管理</button>
              <button className="dark-button" onClick={async () => { await acknowledgeInventoryAlert(); setInventoryAlert(null); }}>我知道了</button>
            </div>
          </div>
        </div>
      )}
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
      <main className={workspace !== "wb" && page === "advertising" ? "ozon-ad-main" : undefined}>
        {workspace === "product_library" && <ProductMasterPage />}
        {workspace === "wb" && <WbWorkspace section={wbPage} onSection={setWbPage} />}
        {workspace === "mercadolibre" && <MercadoLibrePage />}
        {workspace === "ozon" && (
          <>
            {page === "dashboard" && (
              <Dashboard
                key={activeShop?.id}
                data={dash}
                currency={currency}
                days={days}
                setDays={setDays}
                monthOffset={dashboardMonthOffset}
                setMonthOffset={setDashboardMonthOffset}
                customRange={dashboardCustomRange}
                setCustomRange={setDashboardCustomRange}
                refreshing={refreshing}
                refresh={load}
                range={range}
                to={range.to}
              />
            )}{" "}
            {page === "orders" && (
              <Orders
                key={activeShop?.id}
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
                pickupOnly={pickupOnly}
                setPickupOnly={setPickupOnly}
                range={range}
                reload={() => {
                  orderCache.clear();
                  setOrderRevision(revision => revision + 1);
                }}
              />
            )}{" "}
            {page === "fbs" && <FbsPage currency={currency} />}{" "}
            {page === "products" && (
              <>
              {activeShop?.kind === "cross_border" && <WarehouseSwitch key={`warehouse-switch-${activeShop.id}`} shopId={activeShop.id}/>}
              <ProductsPage
                rows={productRows}
                currency={currency}
                query={query}
                setQuery={setQuery}
                reload={load}
              />
              </>
            )}{" "}
            {page === "advertising" && (
              <Advertising
                key={activeShop?.id}
                shopId={activeShop?.id || ""}
                crossBorder={activeShop?.kind === "cross_border"}
                data={ads}
                reportingRange={range}
                statsCurrent={adsScope === currentAdsScope}
                customRange={advertisingCustomRange}
                setCustomRange={setAdvertisingCustomRange}
                currency={currency}
                days={days}
                setDays={setDays}
                refreshing={refreshing}
                refresh={load}
              />
            )}{" "}
            {page === "promotions" && <PromotionCenter key={activeShop?.id} shopId={activeShop?.id || ""} />}{" "}
            {page === "reports" && (
              <ReportsPage
                key={activeShop?.id}
                range={range}
                currency={currency}
                initialTab="daily"
              />
            )}{" "}
            {page === "monthly_profit" && (
              <>
              {activeShop?.kind === "cross_border" && <PaymentRates key={`payment-${activeShop.id}`} />}
              <ReportsPage
                key={`monthly-${activeShop?.id}`}
                range={monthRange}
                currency={currency}
                initialTab="summary"
              />
              </>
            )}{" "}
            {page === "weekly_report" && (
              activeShop?.kind === "local" ? <LocalWeeklyReportPage key={activeShop.id}/> :
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
            {page === "price_intelligence" && <PriceIntelligencePage key={activeShop?.id} shops={shops} activeShop={activeShop} onSelectShop={changeShop} />}{" "}
            {page === "ai" && <AiPage range={range} />}{" "}
            {page === "growth_center" && <GrowthCenterPage key={activeShop?.id} range={range} currency={currency} shopName={activeShop?.name || "当前店铺"} crossBorder={activeShop?.kind === "cross_border"} shopId={activeShop?.id || ""} />}{" "}
            {page === "product_analysis" && <ProductAnalysisPage currency={currency} shopId={activeShop?.id || ""} />}{" "}
            {page === "local_selection" && <LocalSelectionPage />}
            {page === "new_product_profit" && <NewProductProfitPage key={activeShop?.id} shopId={activeShop?.id || ""} range={range} />}
            {page === "selection_library" && <SelectionLibraryPage key={activeShop?.id} shopId={activeShop?.id || ""} />}{" "}
            {page === "competitor_shop" && <CompetitorShopPage key={activeShop?.id} />}{" "}
            {page === "ad_experiments" && <AdExperimentCenter key={activeShop?.id} shopId={activeShop?.id || ""} seed={experimentSeed} />}
            {page === "ad_attribution" && <AdAttributionPage key={activeShop?.id} />}
            {page === "daily_tasks" && <DailyTaskCenter key={activeShop?.id} />}
            {page === "contracts" && <PurchaseContractPage />}
            {page === "procurement_board" && <ProcurementBoard key={activeShop?.id} shopId={activeShop?.id || ""} />}{page === "purchase_orders" && <PurchaseOrderPage />}
            {page === "freight_quotes" && <FreightQuotePage />}
            {page === "mind_map" && <MindMapPage shopId={activeShop?.id || ""} />}{" "}
            {page === "inventory" && (
              <InventoryPage
                shopId={activeShop?.id || ""}
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
            {page === "product_transfer" && <ProductTransferPage shops={shops} />}
            {page === "delivery_fee_query" && <DeliveryFeeQueryPage key={activeShop?.id} />}{" "}
            {page === "packing" && <PackingDocumentsPage inventory={inventoryRows} shopName={activeShop?.name || "当前店铺"} />}{" "}
            {page === "sync" && <SyncPage range={range} />}{" "}
            {page === "feishu" && <FeishuPage range={range} />}{" "}
            {page === "wb" && <WbWorkspace section={wbPage} onSection={setWbPage} />}{" "}
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
