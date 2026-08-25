import { invoke } from "@tauri-apps/api/core";
import type {
  AdvertisingData,
  AnalyticsDetail,
  BusinessReport,
  CrossBorderReport,
  CompetitorRow,
  ConnectionStatus,
  CredentialsForm,
  DashboardData,
  DataCoverageRow,
  DateRange,
  FbsOrderRow,
  FinanceBreakdownRow,
  InsightRow,
  InventoryRow,
  ListingRow,
  ListingJob,
  ListingDraftInput,
  ListingPriceBreakdown,
  ListingPriceInput,
  ListingSettings,
  MissingCostRow,
  OrderRow,
  ProductCostInput,
  ProductDetail,
  ProductRow,
  ShipmentTracking,
  Shop,
  SupplyOrder,
  SupplyTimeslot,
  SyncLog,
  WarehouseMapping,
  WbCost,
  WbDaily,
  WbSettings,
} from "./types";

const isTauri = () => "__TAURI_INTERNALS__" in window;

const demoTrend = Array.from({ length: 7 }, (_, index) => ({
  day: `08/${18 + index}`,
  revenue: [1320, 1570, 1640, 1590, 1410, 820, 220][index],
  units: [21, 27, 25, 24, 19, 11, 3][index],
  adSpend: [130, 160, 145, 170, 155, 120, 45][index],
}));

export async function listShops(): Promise<Shop[]> {
  if (isTauri()) return invoke("list_shops");
  return [
    {
      id: "preview",
      name: "主店铺",
      kind: "cross_border",
      apiName: "Seller + Performance API",
      active: true,
    },
  ];
}

export async function selectShop(shopId: string): Promise<void> {
  if (isTauri()) await invoke("select_shop", { shopId });
}

export async function dashboard(range: DateRange): Promise<DashboardData> {
  if (isTauri()) return invoke("dashboard", { range });
  return {
    revenue: 6349.9,
    orders: 100,
    soldUnits: 119,
    activeProducts: 24,
    adSpend: 925,
    adRevenue: 2800,
    adOrders: 22,
    conversionRate: 3.2,
    trend: demoTrend,
    lastSync: "2026-08-24 14:42:30",
    acos: 33.04,
    tacos: 14.57,
    ctr: 2.8,
    returnUnits: 3,
    cancellationUnits: 4,
    cancellationRate: 4,
    views: 3200,
    orderConversion: 3.13,
  };
}

export async function orders(
  range: DateRange,
  query: string,
): Promise<OrderRow[]> {
  if (isTauri()) return invoke("orders", { range, query });
  const products = ["马克杯收纳网包", "双层加固焊接夹", "男士旅行洗漱包"];
  return Array.from({ length: 14 }, (_, i) => ({
    eventId: `event-${i}`,
    postingNumber: `${31011390 + i}-00${63 + i}-1`,
    sku: `SKU-${1000 + i}`,
    offerId: `GJYB${String(i + 1).padStart(3, "0")}`,
    productName: products[i % products.length],
    quantity: 1,
    scheme: i % 4 === 0 ? "FBO" : "FBS",
    status: i % 3 === 0 ? "delivered" : "delivering",
    amount: [35, 25, 25, 60][i % 4],
    createdAt: `2026/07/30 ${String(19 - (i % 10)).padStart(2, "0")}:39:01`,
    updatedAt: "2026/08/11 10:00:00",
    origin: "莫斯科仓",
    destination: "库尔斯克",
    estimatedDelivery: 12.4,
    estimateBasis: "集群权重预估",
    imageUrl: "",
  }));
}

export async function advertising(range: DateRange): Promise<AdvertisingData> {
  if (isTauri()) return invoke("advertising", { range });
  return {
    impressions: 0,
    clicks: 0,
    cartAdds: 0,
    orders: 0,
    revenue: 0,
    spend: 0,
    ctr: null,
    cpc: null,
    roas: 0,
    campaigns: [],
    trend: [],
  };
}

export async function products(
  range: DateRange,
  query: string,
): Promise<ProductRow[]> {
  if (isTauri()) return invoke("products", { range, query });
  return Array.from({ length: 18 }, (_, i) => ({
    sku: String(2526402232 + i),
    offerId: `GJYB${String(i + 1).padStart(4, "0")}`,
    productId: "",
    name: ["多功能旅行收纳包", "双层相机保护包", "轻便户外腰包"][i % 3],
    revenue: 1280 - i * 37,
    orderedUnits: 24 - (i % 8),
    deliveredUnits: 20 - (i % 6),
    returns: i % 3,
    cancellations: i % 2,
    unitCost: i % 4 ? 18.5 : null,
    firstMileCost: i % 4 ? 6.2 : null,
    lengthCm: 30,
    widthCm: 20,
    heightCm: 8,
    weightKg: 0.6,
    note: "",
    updatedAt: "2026-08-24 12:31:19",
  }));
}

export async function saveProductCost(input: ProductCostInput): Promise<void> {
  if (isTauri()) await invoke("save_product_cost", { input });
}
export async function matchProductCosts(
  input: ProductCostInput,
  pattern: string,
): Promise<number> {
  if (isTauri()) return invoke("match_product_costs", { input, pattern });
  return 1;
}
export async function exportProductCosts(): Promise<string> {
  if (isTauri()) return invoke("export_product_costs");
  return "preview/product_costs.csv";
}

export async function inventory(
  query: string,
  targetDays = 30,
): Promise<InventoryRow[]> {
  if (isTauri()) return invoke("inventory", { query, targetDays });
  return Array.from({ length: 12 }, (_, i) => ({
    sku: String(2526402232 + i),
    offerId: `GJYB${String(i + 1).padStart(4, "0")}`,
    productName: "多功能旅行收纳包",
    availableStock: 92 - i * 5,
    portalStock: 100 - i * 5,
    reservedStock: 8,
    transitStock: i * 2,
    requestedStock: i,
    warehouseCount: 3 + (i % 4),
    dailySales: 2.4 + i / 10,
    estimatedDays: 28 - i,
    suggestedQty: i > 7 ? 20 + i : 0,
    plannedQty: i % 3 ? 0 : 12,
    updatedAt: "2026-08-24 12:31:19",
  }));
}
export async function syncInventory(): Promise<number> {
  return invoke("sync_inventory");
}

export async function connectionStatus(): Promise<ConnectionStatus> {
  if (isTauri()) return invoke("connection_status");
  return {
    sellerClientId: "3193042",
    sellerApiConfigured: true,
    performanceClientId: "",
    performanceApiConfigured: false,
    aiBaseUrl: "",
    aiModel: "",
    aiConfigured: false,
    feishuConfigured: false,
    lastSuccessfulSync: "2026-08-24T20:34:50",
  };
}
export async function warehouseMappings(): Promise<WarehouseMapping[]> {
  if (isTauri()) return invoke("warehouse_mappings");
  return [];
}
export async function saveWarehouseMapping(
  warehouseName: string,
  clusterName: string,
): Promise<void> {
  if (isTauri())
    await invoke("save_warehouse_mapping", { warehouseName, clusterName });
}
export async function fbsOrders(query: string): Promise<FbsOrderRow[]> {
  if (isTauri()) return invoke("fbs_orders", { query });
  return [];
}
export async function syncFbsOrders(range: DateRange): Promise<number> {
  return invoke("sync_fbs_orders", { range });
}
export async function saveFbsThreshold(
  hours: number,
  warningHours: number,
): Promise<void> {
  if (isTauri()) await invoke("save_fbs_threshold", { hours, warningHours });
}
export async function competitors(): Promise<CompetitorRow[]> {
  if (isTauri()) return invoke("competitors");
  return [];
}
export async function addCompetitor(productUrl: string): Promise<number> {
  if (isTauri()) return invoke("add_competitor", { productUrl });
  return 1;
}
export async function refreshCompetitor(id: number): Promise<void> {
  if (isTauri()) await invoke("refresh_competitor", { id });
}
export async function refreshCompetitorsDue(): Promise<number> {
  return invoke("refresh_competitors_due");
}
export async function removeCompetitor(id: number): Promise<void> {
  if (isTauri()) await invoke("remove_competitor", { id });
}
export async function businessReport(
  range: DateRange,
): Promise<BusinessReport> {
  if (isTauri()) return invoke("business_report", { range });
  return {
    revenue: 0,
    orders: 0,
    adSpend: 0,
    financeNet: 0,
    salesReturns: 0,
    accrualFees: 0,
    otherAdjustments: 0,
    commission: 0,
    financeAdvertising: 0,
    deliveryFees: 0,
    returnFees: 0,
    purchaseCost: 0,
    firstMileCost: 0,
    estimatedProfit: 0,
    settledProfit: null,
    taxRate: 6,
    taxAmount: 0,
    payoutFeeRate: 1,
    payoutFee: 0,
    afterTaxProfit: null,
    acquiring: 0,
    storagePackaging: 0,
    penaltiesAdjustments: 0,
    otherFinanceFees: 0,
    unallocatedFinanceAmount: 0,
    financeOperations: 0,
    exactSkuOperations: 0,
    unallocatedOperations: 0,
    cashFlowReportedTotal: 0,
    reconciliationDifference: null,
    missingCostSkus: 0,
    costedUnits: 0,
    missingCostUnits: 0,
    daily: [],
  };
}
export async function missingCostRows(
  range: DateRange,
): Promise<MissingCostRow[]> {
  return isTauri() ? invoke("missing_cost_rows", { range }) : [];
}
export async function analyticsDetail(
  range: DateRange,
): Promise<AnalyticsDetail> {
  if (isTauri()) return invoke("analytics_detail", { range });
  return {
    products: [],
    dailyProducts: [],
    series: [],
    weekly: [],
    weeklyDaily: [],
  };
}
export async function crossBorderReport(
  range: DateRange,
): Promise<CrossBorderReport> {
  return invoke("cross_border_report", { range });
}
export async function financeBreakdown(
  range: DateRange,
): Promise<FinanceBreakdownRow[]> {
  return isTauri() ? invoke("finance_breakdown", { range }) : [];
}
export async function dataCoverage(): Promise<DataCoverageRow[]> {
  return isTauri() ? invoke("data_coverage") : [];
}
export async function pruneCache(before: string): Promise<number> {
  return invoke("prune_cache", { before });
}
export async function loadCredentialsForm(): Promise<CredentialsForm> {
  return invoke("load_credentials_form");
}
export async function saveCredentialsForm(
  form: CredentialsForm,
): Promise<void> {
  await invoke("save_credentials_form", { form });
}
export async function aiAnalysis(
  range: DateRange,
  question: string,
): Promise<string> {
  if (isTauri()) return invoke("ai_analysis", { range, question });
  return "预览模式不会向 AI 服务发送经营数据。请在桌面版的连接设置中配置 AI 服务后使用。";
}
export async function supplyOrders(): Promise<SupplyOrder[]> {
  return isTauri() ? invoke("supply_orders") : [];
}
export async function supplyTimeslots(
  orderId: number,
  dateFrom: string,
  dateTo: string,
): Promise<SupplyTimeslot[]> {
  return isTauri()
    ? invoke("supply_timeslots", { orderId, dateFrom, dateTo })
    : [];
}
export async function bookSupplyTimeslot(
  supplyOrderId: number,
  timeslotFrom: string,
  timeslotTo: string,
  confirmation: string,
): Promise<string> {
  return invoke("book_supply_timeslot", {
    supplyOrderId,
    timeslotFrom,
    timeslotTo,
    confirmation,
  });
}
export async function syncLogs(): Promise<SyncLog[]> {
  return isTauri() ? invoke("sync_logs") : [];
}
export async function syncSeller(range: DateRange): Promise<number> {
  return invoke("sync_seller_sales", { range });
}
export async function syncPerformance(range: DateRange): Promise<number> {
  return invoke("sync_performance_ads", { range });
}
export async function syncFinance(range: DateRange): Promise<number> {
  return invoke("sync_finance", { range });
}
export async function testFeishu(): Promise<string> {
  return invoke("test_feishu");
}
export async function syncFeishuProducts(
  direction: "pull" | "push" | "both",
): Promise<string> {
  return invoke("sync_feishu_products", { direction });
}
export async function sendFeishuWeekly(range: DateRange): Promise<string> {
  return invoke("send_feishu_weekly", { range });
}
export async function shipmentTracking(): Promise<ShipmentTracking[]> {
  return invoke("shipment_tracking");
}
export async function syncFeishuShipments(): Promise<number> {
  return invoke("sync_feishu_shipments");
}
export async function notifyShipment(trackingId: string): Promise<string> {
  return invoke("notify_feishu_shipment", { trackingId });
}
export async function wbSettings(): Promise<WbSettings> {
  return invoke("wb_settings");
}
export async function exportWbApiBundle(): Promise<string> {
  return invoke("export_wb_api_bundle");
}
export async function importWbApiBundle(path: string): Promise<void> {
  return invoke("import_wb_api_bundle", { path });
}
export async function saveWbSettings(form: WbSettings): Promise<void> {
  return invoke("save_wb_settings", { form });
}
export async function wbCosts(): Promise<WbCost[]> {
  return invoke("wb_costs");
}
export async function saveWbCost(input: WbCost): Promise<void> {
  return invoke("save_wb_cost", { input });
}
export async function wbDaily(range: DateRange): Promise<WbDaily[]> {
  return invoke("wb_daily", { range });
}
export async function syncWb(range: DateRange): Promise<string> {
  return invoke("sync_wb", { range });
}
export async function testWbFeishu(): Promise<string> {
  return invoke("test_wb_feishu");
}
export async function sendWbWeekly(range: DateRange): Promise<string> {
  return invoke("send_wb_weekly", { range });
}
export async function exportDataset(
  kind: string,
  range: DateRange,
): Promise<string> {
  return invoke("export_dataset", { kind, range });
}
export async function importProductCostsCsv(path: string): Promise<number> {
  return invoke("import_product_costs_csv", { path });
}
export async function exportApiBundle(): Promise<string> {
  return invoke("export_api_bundle");
}
export async function importApiBundle(path: string): Promise<number> {
  return invoke("import_api_bundle", { path });
}
export async function createShop(
  name: string,
  kind: "local" | "cross_border",
  apiName: string,
): Promise<string> {
  return invoke("create_shop", { name, kind, apiName });
}
export async function updateShop(
  shopId: string,
  name: string,
  kind: "local" | "cross_border",
  apiName: string,
): Promise<void> {
  return invoke("update_shop", { shopId, name, kind, apiName });
}
export async function deleteShop(
  shopId: string,
  confirmation: string,
): Promise<string> {
  return invoke("delete_shop", { shopId, confirmation });
}
export async function listingSettings(): Promise<ListingSettings> {
  return invoke("listing_settings");
}
export async function saveListingSettings(
  form: ListingSettings,
): Promise<void> {
  return invoke("save_listing_settings", { form });
}
export async function listingRows(query: string): Promise<ListingRow[]> {
  return invoke("listing_rows", { query });
}
export async function listingJobs():Promise<ListingJob[]>{return invoke("listing_jobs");}
export async function createListingDraft(reference:string):Promise<number>{return invoke("create_listing_draft",{reference});}
export async function saveListingDraft(form:ListingDraftInput):Promise<number>{return invoke("save_listing_draft",{form});}
export async function retryListingJob(id:number):Promise<void>{return invoke("retry_listing_job",{id});}
export async function collectListingReference(id:number):Promise<number>{return invoke("collect_listing_reference",{id});}
export async function syncListingCosts(): Promise<number> {
  return invoke("sync_listing_costs");
}
export async function calculateListingPrice(
  form: ListingPriceInput,
): Promise<ListingPriceBreakdown> {
  return invoke("calculate_listing_price", { form });
}
export async function launchListingTool(): Promise<string> {
  return invoke("launch_listing_tool");
}
export async function productInsights(
  to: string,
  query: string,
): Promise<InsightRow[]> {
  return invoke("product_insights", { to, query });
}
export async function seriesInsights(to: string): Promise<InsightRow[]> {
  return invoke("series_insights", { to });
}
export async function saveProductSeries(
  id: number | null,
  name: string,
  skus: string[],
): Promise<number> {
  return invoke("save_product_series", { id, name, skus });
}
export async function deleteProductSeries(id: number): Promise<void> {
  return invoke("delete_product_series", { id });
}
export async function productDetail(
  sku: string,
  to: string,
): Promise<ProductDetail> {
  return invoke("product_detail", { sku, to });
}
export async function saveProductClusterWeights(
  sku: string,
  weights: Record<string, number>,
): Promise<void> {
  return invoke("save_product_cluster_weights", { sku, weights });
}
