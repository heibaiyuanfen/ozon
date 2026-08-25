export type PageKey =
  | "dashboard"
  | "orders"
  | "products"
  | "advertising"
  | "reports"
  | "monthly_profit"
  | "weekly_report"
  | "cross_profit"
  | "ai"
  | "inventory"
  | "fbs"
  | "supply"
  | "sync"
  | "feishu"
  | "wb"
  | "migration"
  | "listing"
  | "competitors"
  | "shops"
  | "settings";

export interface Shop {
  id: string;
  name: string;
  kind: "local" | "cross_border";
  apiName: string;
  active: boolean;
}

export interface DateRange {
  from: string;
  to: string;
}

export interface DashboardData {
  revenue: number;
  orders: number;
  soldUnits: number;
  activeProducts: number;
  adSpend: number;
  adRevenue: number;
  adOrders: number;
  conversionRate: number | null;
  trend: Array<{
    day: string;
    revenue: number;
    units: number;
    adSpend: number;
  }>;
  lastSync: string | null;
  acos: number | null;
  tacos: number | null;
  ctr: number | null;
  returnUnits: number;
  cancellationUnits: number;
  cancellationRate: number | null;
  views: number;
  orderConversion: number | null;
}

export interface OrderRow {
  eventId: string;
  postingNumber: string;
  sku: string;
  offerId: string;
  productName: string;
  quantity: number;
  scheme: string;
  status: string;
  amount: number;
  createdAt: string;
  updatedAt: string;
  origin: string;
  destination: string;
  estimatedDelivery: number | null;
  estimateBasis: string;
  imageUrl: string;
}

export interface AdvertisingData {
  impressions: number;
  clicks: number;
  cartAdds: number;
  orders: number;
  revenue: number;
  spend: number;
  ctr: number | null;
  cpc: number | null;
  roas: number | null;
  campaigns: Array<{
    id: string;
    name: string;
    state: string;
    paymentType: string;
    impressions: number;
    clicks: number;
    orders: number;
    spend: number;
    revenue: number;
    roas: number | null;
  }>;
  trend: Array<{
    day: string;
    impressions: number;
    clicks: number;
    orders: number;
    spend: number;
    revenue: number;
  }>;
}

export interface ProductRow {
  sku: string;
  offerId: string;
  productId: string;
  name: string;
  revenue: number;
  orderedUnits: number;
  deliveredUnits: number;
  returns: number;
  cancellations: number;
  unitCost: number | null;
  firstMileCost: number | null;
  lengthCm: number | null;
  widthCm: number | null;
  heightCm: number | null;
  weightKg: number | null;
  note: string;
  updatedAt: string;
}

export interface ProductCostInput {
  sku: string;
  unitCost: number | null;
  firstMileCost: number | null;
  lengthCm: number | null;
  widthCm: number | null;
  heightCm: number | null;
  weightKg: number | null;
  note: string;
}

export interface InventoryRow {
  sku: string;
  offerId: string;
  productName: string;
  availableStock: number;
  portalStock: number | null;
  reservedStock: number | null;
  transitStock: number;
  requestedStock: number;
  warehouseCount: number;
  dailySales: number;
  estimatedDays: number | null;
  suggestedQty: number;
  plannedQty: number;
  updatedAt: string;
}

export interface ConnectionStatus {
  sellerClientId: string;
  sellerApiConfigured: boolean;
  performanceClientId: string;
  performanceApiConfigured: boolean;
  aiBaseUrl: string;
  aiModel: string;
  aiConfigured: boolean;
  feishuConfigured: boolean;
  lastSuccessfulSync: string | null;
}
export interface CredentialsForm {
  sellerClientId: string;
  sellerApiKey: string;
  performanceClientId: string;
  performanceClientSecret: string;
  aiBaseUrl: string;
  aiApiKey: string;
  aiModel: string;
  feishuBaseUrl: string;
  feishuAppId: string;
  feishuAppSecret: string;
  feishuAppToken: string;
  feishuProductTableId: string;
  feishuWeeklyTableId: string;
  feishuTrackingTableId: string;
  feishuSeriesTableId: string;
  feishuChatId: string;
  localTaxRate: string;
  localPayoutFeeRate: string;
  localRubPerCny: string;
  crossBorderRubPerCny: string;
}
export interface WarehouseMapping {
  warehouseName: string;
  clusterName: string;
  orderCount: number;
}
export interface FbsOrderRow {
  postingNumber: string;
  offerId: string;
  sku: string;
  productName: string;
  orderedAt: string;
  status: string;
  origin: string;
  destination: string;
  deadline: string;
  alertLevel: "overdue" | "warning" | "pending" | "shipped";
  estimatedDelivery: number | null;
  estimateBasis: string;
}
export interface CompetitorSnapshot {
  capturedAt: string;
  price: number | null;
  salesTotal: number | null;
}
export interface CompetitorRow {
  id: number;
  productUrl: string;
  productCode: string;
  name: string;
  imageUrl: string;
  latestPrice: number | null;
  dailySales: number | null;
  weeklySales: number | null;
  monthlySales: number | null;
  snapshots: CompetitorSnapshot[];
}
export interface BusinessReport {
  revenue: number;
  orders: number;
  adSpend: number;
  financeNet: number;
  salesReturns: number;
  accrualFees: number;
  otherAdjustments: number;
  commission: number;
  financeAdvertising: number;
  deliveryFees: number;
  returnFees: number;
  purchaseCost: number;
  firstMileCost: number;
  estimatedProfit: number;
  settledProfit: number | null;
  taxRate: number;
  taxAmount: number;
  payoutFeeRate: number;
  payoutFee: number;
  afterTaxProfit: number | null;
  acquiring: number;
  storagePackaging: number;
  penaltiesAdjustments: number;
  otherFinanceFees: number;
  unallocatedFinanceAmount: number;
  financeOperations: number;
  exactSkuOperations: number;
  unallocatedOperations: number;
  cashFlowReportedTotal: number;
  reconciliationDifference: number | null;
  missingCostSkus: number;
  costedUnits: number;
  missingCostUnits: number;
  daily: Array<{
    day: string;
    revenue: number;
    orders: number;
    adSpend: number;
  }>;
}
export interface MissingCostRow {
  sku: string;
  offerId: string;
  productName: string;
  units: number;
  missingPurchase: boolean;
  missingFirstMile: boolean;
  missingWeight: boolean;
  missingDimensions: boolean;
  unitCost: number | null;
  firstMileCost: number | null;
  weightKg: number | null;
  lengthCm: number | null;
  widthCm: number | null;
  heightCm: number | null;
  note: string;
}
export interface AnalyticsDetail {
  products: Array<{
    sku: string;
    offerId: string;
    productName: string;
    units: number;
    revenue: number;
    adSpend: number;
    purchaseCost: number | null;
    firstMileCost: number | null;
    platformFees: number;
    estimatedProfit: number | null;
    profitRate: number | null;
    crossBorderFreight: number | null;
    costComplete: boolean;
  }>;
  dailyProducts: Array<{
    day: string;
    sku: string;
    offerId: string;
    productName: string;
    units: number;
    revenue: number;
    returns: number;
    cancellations: number;
    views: number;
    adSpend: number;
    adOrders: number;
    tacos: number | null;
    adCostPerOrder: number | null;
    estimatedProfit: number | null;
    costComplete: boolean;
  }>;
  series: Array<{
    periodType: string;
    period: string;
    series: string;
    skuCount: number;
    units: number;
    revenue: number;
  }>;
  weekly: Array<{
    period: string;
    revenue: number;
    units: number;
    adSpend: number;
    adOrders: number;
    adOrderShare: number;
    acots: number;
    returns: number;
    cancellations: number;
    estimatedProfit: number;
  }>;
  weeklyDaily: Array<{
    day: string;
    revenue: number;
    units: number;
    adSpend: number;
    adOrders: number;
    adOrderShare: number;
    acots: number;
    returns: number;
    cancellations: number;
  }>;
}
export interface CrossBorderReport {
  dateFrom: string;
  dateTo: string;
  rubPerCny: number;
  revenueCny: number;
  units: number;
  adSpendCny: number;
  estimatedPlatformFeesCny: number;
  purchaseAndFreightCny: number;
  profitCny: number | null;
  settledFinanceNetCny: number;
  financeAvailable: boolean;
  commissionRate: number | null;
  acquiringRate: number | null;
  missingCostSkus: number;
  fbpOrders: number;
  rfbsOrders: number;
  whdOrders: number;
  daily: Array<{
    day: string;
    units: number;
    revenueCny: number;
    adSpendCny: number;
    purchaseAndFreightCny: number;
    profitCny: number | null;
  }>;
  rows: Array<{
    sku: string;
    offerId: string;
    productName: string;
    units: number;
    fulfillmentOrders: number;
    fbpOrders: number;
    rfbsOrders: number;
    whdOrders: number;
    revenueCny: number;
    sellingPriceCny: number;
    purchaseCostCny: number | null;
    weightKg: number | null;
    freightUnitCny: number | null;
    purchaseTotalCny: number | null;
    freightTotalCny: number | null;
    estimatedPlatformFeesCny: number | null;
    contributionCny: number | null;
    financeSettledCny: number | null;
    commissionRate: number | null;
    acquiringRate: number | null;
    costComplete: boolean;
  }>;
}
export interface FinanceBreakdownRow {
  category: string;
  categoryLabel: string;
  name: string;
  apiName: string;
  rowsCount: number;
  amount: number;
}
export interface DataCoverageRow {
  source: string;
  rowsCount: number;
  dateFrom: string;
  dateTo: string;
  lastSuccess: string;
}
export interface ListingSettings {
  ledgerPath: string;
  ledgerShopName: string;
  toolExecutable: string;
  toolDataDir: string;
}
export interface ListingRow {
  shopName: string;
  platform: string;
  offerId: string;
  productId: string;
  unitCostCny: number | null;
  weightKg: number | null;
  lengthCm: number | null;
  widthCm: number | null;
  heightCm: number | null;
  status: string;
  listingMode: string;
  pricingMode: string;
  price: number | null;
  profit: number | null;
  roiPercent: number | null;
  category: string;
  importTaskId: string;
  updatedAt: string;
}
export interface ListingJob { id:number; sourceUrl:string; article:string; offerId:string; title:string; categoryId:string; categoryDisplay:string; status:string; stage:number; error:string; payload:Record<string,unknown>; updatedAt:string; }
export interface ListingCategory { descriptionCategoryId:number; typeId:number; name:string; display:string; score:number; }
export interface ListingDraftInput { id:number; offerId:string; title:string; categoryId:string; categoryDisplay:string; typeId:string; price:string; weight:number; depth:number; width:number; height:number; description:string; images:string[]; attributes:unknown[]; complexAttributes:unknown[]; }
export interface ListingPriceInput {
  purchaseCost: number;
  labelFee: number;
  targetRoiPercent: number;
  weightKg: number;
  salesCommissionPercent: number;
  salesCommissionDiscountPercent: number;
  advertisingPercent: number;
  cargoLossPercent: number;
  minimumSalePrice: number;
}
export interface ListingPriceBreakdown {
  price: number;
  shipping: number;
  salesCommission: number;
  logisticsCommission: number;
  advertising: number;
  cargoLoss: number;
  invested: number;
  profit: number;
  roiPercent: number;
}
export interface InsightRow {
  id: string;
  name: string;
  skus: string[];
  offerIds: string[];
  dayUnits: number;
  weekUnits: number;
  monthUnits: number;
  dayRevenue: number;
  weekRevenue: number;
  monthRevenue: number;
  dayAdSpend: number;
  weekAdSpend: number;
  monthAdSpend: number;
  dayAdOrders: number;
  weekAdOrders: number;
  monthAdOrders: number;
  clusters: Array<{ cluster: string; orders: number; share: number }>;
  adSource: string;
}
export interface ProductDetail {
  sku: string;
  offerId: string;
  name: string;
  trend: Array<{
    day: string;
    units: number;
    revenue: number;
    adSpend: number;
  }>;
  clusters: Array<{
    cluster: string;
    historicalShare: number;
    configuredWeight: number;
    orders: number;
  }>;
}
export interface SupplyOrder {
  orderId: number;
  orderNumber: string;
  state: string;
  createdDate: string;
  dataFillingDeadline: string;
  dropoffName: string;
  dropoffAddress: string;
  timeslotFrom: string;
  timeslotTo: string;
  timezoneName: string;
  supplyType: string;
  clusters: string;
  storageWarehouses: string;
  supplyStates: string;
  suppliesCount: number;
}
export interface SupplyTimeslot {
  from: string;
  to: string;
}
export interface SyncLog {
  id: number;
  startedAt: string;
  finishedAt: string;
  source: string;
  status: string;
  rowsCount: number;
  message: string;
}
export interface ShipmentTracking {
  trackingId: string;
  productName: string;
  batchNo: string;
  shopName: string;
  quantity: number;
  cargoStatus: string;
  channel: string;
  domesticArrival: string;
  foreignArrival: string;
  notifiedForeignArrival: string;
  source: string;
  updatedAt: string;
  needsNotification: boolean;
}
export interface WbSettings {
  storeName: string;
  token: string;
  rubPerCny: number;
  commissionPercent: number;
  feishuAppId: string;
  feishuAppSecret: string;
  feishuChatId: string;
}
export interface WbCost {
  nmId: number;
  article: string;
  purchaseCostCny: number | null;
  lengthCm: number | null;
  widthCm: number | null;
  heightCm: number | null;
  weightKg: number | null;
  warehouseMode: string;
}
export interface WbDaily {
  day: string;
  nmId: number;
  article: string;
  warehouseName: string;
  quantity: number;
  revenueCny: number;
  adSpendCny: number;
  commissionCny: number;
  purchaseTotalCny: number | null;
  logisticsTotalCny: number | null;
  profitCny: number | null;
  complete: boolean;
}
export interface WbOrderRow {
  srid: string;
  day: string;
  changedAt: string;
  nmId: number;
  article: string;
  warehouseName: string;
  revenueCny: number;
  cancelled: boolean;
}
export interface WbAdRow {
  day: string;
  nmId: number;
  campaignId: number;
  spendCny: number;
  orders: number;
  salesCny: number;
  views: number;
  clicks: number;
  ctr: number | null;
}
export interface WbWarehouseRow {
  warehouseKey: string;
  name: string;
  address: string;
  city: string;
  country: string;
  mode: string;
}
export interface WbStockRow {
  nmId: number;
  chrtId: number;
  warehouseId: number;
  warehouseName: string;
  regionName: string;
  quantity: number;
  inWayToClient: number;
  inWayFromClient: number;
  updatedAt: string;
}
