use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        LazyLock, Mutex,
    },
};
use tauri::{Manager, State};
mod insights;
mod listing;
mod secrets;
mod wb;
static INVENTORY_SYNC_LOCK: Mutex<()> = Mutex::new(());
static SELLER_SYNC_LOCK: Mutex<()> = Mutex::new(());
static PERFORMANCE_SYNC_LOCK: Mutex<()> = Mutex::new(());
static FINANCE_SYNC_LOCK: Mutex<()> = Mutex::new(());
static COMPETITOR_COLLECTION_STOP: AtomicBool = AtomicBool::new(false);
static COMPETITOR_TASK_STOPS: LazyLock<Mutex<HashSet<i64>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));
static COMPETITOR_COLLECTION_PROGRESS: LazyLock<Mutex<CompetitorCollectionProgress>> =
    LazyLock::new(|| Mutex::new(CompetitorCollectionProgress::default()));

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct CompetitorCollectionProgress {
    running: bool,
    run_id: String,
    total: i64,
    completed: i64,
    succeeded: i64,
    failed: i64,
    current_id: Option<i64>,
    current_code: String,
    stage: String,
    message: String,
    stop_requested: bool,
    tasks: Vec<CompetitorCollectionTask>,
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct CompetitorCollectionTask {
    id: i64,
    product_code: String,
    product_url: String,
    status: String,
    stage: String,
    message: String,
    retry_count: i64,
    started_at: String,
    finished_at: String,
    stop_requested: bool,
}

fn competitor_task_stop_requested(id: i64) -> bool {
    COMPETITOR_COLLECTION_STOP.load(Ordering::SeqCst)
        || COMPETITOR_TASK_STOPS
            .lock()
            .map(|stops| stops.contains(&id))
            .unwrap_or(false)
}

fn update_competitor_task(id: i64, update: impl FnOnce(&mut CompetitorCollectionTask)) {
    if let Ok(mut progress) = COMPETITOR_COLLECTION_PROGRESS.lock() {
        if let Some(task) = progress.tasks.iter_mut().find(|task| task.id == id) {
            update(task);
        }
    }
}

pub(crate) struct AppState {
    pub(crate) data_dir: PathBuf,
    active_shop_id: Mutex<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct ShopFile {
    active_shop_id: String,
    shops: Vec<RawShop>,
}
#[derive(Clone, Serialize, Deserialize)]
struct RawShop {
    id: String,
    name: String,
    kind: String,
    database_file: String,
    #[serde(default)]
    api_name: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Shop {
    id: String,
    name: String,
    kind: String,
    api_name: String,
    active: bool,
    database_size_bytes: u64,
}
#[derive(Clone, Deserialize)]
pub(crate) struct DateRange {
    pub(crate) from: String,
    pub(crate) to: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TrendRow {
    day: String,
    revenue: f64,
    units: i64,
    ad_spend: f64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DashboardData {
    revenue: f64,
    orders: i64,
    sold_units: i64,
    active_products: i64,
    ad_spend: f64,
    ad_revenue: f64,
    ad_orders: i64,
    conversion_rate: Option<f64>,
    trend: Vec<TrendRow>,
    last_sync: Option<String>,
    acos: Option<f64>,
    tacos: Option<f64>,
    ctr: Option<f64>,
    return_units: i64,
    return_rate: Option<f64>,
    cancellation_units: i64,
    cancellation_rate: Option<f64>,
    views: i64,
    order_conversion: Option<f64>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OrderRow {
    event_id: String,
    posting_number: String,
    sku: String,
    offer_id: String,
    product_name: String,
    quantity: i64,
    scheme: String,
    status: String,
    amount: f64,
    created_at: String,
    updated_at: String,
    origin: String,
    destination: String,
    estimated_delivery: Option<f64>,
    estimate_basis: String,
    image_url: String,
    supplier_url: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CampaignRow {
    id: String,
    name: String,
    state: String,
    payment_type: String,
    impressions: i64,
    clicks: i64,
    orders: i64,
    spend: f64,
    revenue: f64,
    roas: Option<f64>,
    ctr: Option<f64>,
    cpc: Option<f64>,
    conversion_rate: Option<f64>,
    cpa: Option<f64>,
    acos: Option<f64>,
    diagnosis_level: String,
    diagnosis_text: String,
    recommended_action: String,
    budget: f64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AdvertisingTrendRow {
    day: String,
    impressions: i64,
    clicks: i64,
    orders: i64,
    spend: f64,
    revenue: f64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AdvertisingData {
    impressions: i64,
    clicks: i64,
    cart_adds: i64,
    orders: i64,
    revenue: f64,
    spend: f64,
    ctr: Option<f64>,
    cpc: Option<f64>,
    roas: Option<f64>,
    conversion_rate: Option<f64>,
    cpa: Option<f64>,
    acos: Option<f64>,
    break_even_roas: Option<f64>,
    target_roas: Option<f64>,
    max_cpa: Option<f64>,
    known_cost_margin: Option<f64>,
    margin_coverage_percent: f64,
    campaigns: Vec<CampaignRow>,
    trend: Vec<AdvertisingTrendRow>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CampaignActionLogRow {
    id: i64,
    action: String,
    requested_value: String,
    before_state: String,
    before_budget: f64,
    after_state: String,
    after_budget: f64,
    status: String,
    message: String,
    created_at: String,
    before_spend: f64,
    before_revenue: f64,
    after_spend: f64,
    after_revenue: f64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CampaignMonitorData {
    id: String,
    name: String,
    state: String,
    budget: f64,
    budget_source: String,
    budget_known: bool,
    daily: Vec<AdvertisingTrendRow>,
    logs: Vec<CampaignActionLogRow>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CampaignControlInput {
    campaign_id: String,
    action: String,
    weekly_budget: Option<f64>,
    confirmation: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProductRow {
    sku: String,
    offer_id: String,
    product_id: String,
    name: String,
    revenue: f64,
    ordered_units: i64,
    delivered_units: i64,
    returns: i64,
    cancellations: i64,
    unit_cost: Option<f64>,
    first_mile_cost: Option<f64>,
    length_cm: Option<f64>,
    width_cm: Option<f64>,
    height_cm: Option<f64>,
    weight_kg: Option<f64>,
    note: String,
    updated_at: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProductCostInput {
    sku: String,
    unit_cost: Option<f64>,
    first_mile_cost: Option<f64>,
    length_cm: Option<f64>,
    width_cm: Option<f64>,
    height_cm: Option<f64>,
    weight_kg: Option<f64>,
    note: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InventoryRow {
    sku: String,
    offer_id: String,
    product_name: String,
    available_stock: i64,
    portal_stock: Option<i64>,
    reserved_stock: Option<i64>,
    transit_stock: i64,
    requested_stock: i64,
    domestic_production_stock: i64,
    domestic_warehouse_stock: i64,
    overseas_transit_stock: i64,
    overseas_arrived_stock: i64,
    warehouse_count: i64,
    daily_sales: f64,
    daily_sales_7d: f64,
    demand_trend_percent: Option<f64>,
    estimated_days: Option<f64>,
    health_status: String,
    health_text: String,
    suggested_qty: i64,
    planned_qty: i64,
    return_units_30d: i64,
    return_rate_30d: Option<f64>,
    return_logistics_cost_30d: f64,
    updated_at: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionStatus {
    seller_client_id: String,
    seller_api_configured: bool,
    performance_client_id: String,
    performance_api_configured: bool,
    ai_base_url: String,
    ai_model: String,
    ai_configured: bool,
    feishu_configured: bool,
    last_successful_sync: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CredentialsForm {
    seller_client_id: String,
    seller_api_key: String,
    performance_client_id: String,
    performance_client_secret: String,
    ai_base_url: String,
    ai_api_key: String,
    ai_model: String,
    feishu_base_url: String,
    feishu_app_id: String,
    feishu_app_secret: String,
    feishu_app_token: String,
    feishu_product_table_id: String,
    feishu_weekly_table_id: String,
    feishu_tracking_table_id: String,
    feishu_series_table_id: String,
    feishu_chat_id: String,
    local_tax_rate: String,
    local_payout_fee_rate: String,
    local_rub_per_cny: String,
    cross_border_rub_per_cny: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WarehouseMapping {
    warehouse_name: String,
    cluster_name: String,
    order_count: i64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FbsOrderRow {
    posting_number: String,
    offer_id: String,
    sku: String,
    product_name: String,
    ordered_at: String,
    status: String,
    origin: String,
    destination: String,
    deadline: String,
    alert_level: String,
    estimated_delivery: Option<f64>,
    estimate_basis: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CompetitorSnapshot {
    captured_at: String,
    price: Option<f64>,
    sales_total: Option<i64>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CompetitorRow {
    id: i64,
    is_demo: bool,
    product_url: String,
    product_code: String,
    name: String,
    image_url: String,
    latest_price: Option<f64>,
    previous_price: Option<f64>,
    price_change: Option<f64>,
    price_change_percent: Option<f64>,
    price_min_30d: Option<f64>,
    price_max_30d: Option<f64>,
    price_avg_30d: Option<f64>,
    price_alert_level: String,
    price_alert_text: String,
    price_changes_30d: i64,
    promotion_suspected: bool,
    daily_sales: Option<i64>,
    weekly_sales: Option<i64>,
    monthly_sales: Option<i64>,
    latest_status: String,
    latest_observed_at: String,
    latest_retry_count: i64,
    latest_notes: String,
    snapshots: Vec<CompetitorSnapshot>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CompetitorRunSummary {
    run_id: String,
    started_at: String,
    finished_at: String,
    requested: i64,
    completed: i64,
    ok: i64,
    blocked: i64,
    changed_layout: i64,
    inaccessible: i64,
    ambiguous_match: i64,
    incomplete: i64,
    status: String,
    notes: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReportDay {
    day: String,
    revenue: f64,
    orders: i64,
    ad_spend: f64,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BusinessReport {
    revenue: f64,
    orders: i64,
    ad_spend: f64,
    finance_net: f64,
    sales_returns: f64,
    accrual_fees: f64,
    other_adjustments: f64,
    commission: f64,
    finance_advertising: f64,
    delivery_fees: f64,
    return_fees: f64,
    purchase_cost: f64,
    first_mile_cost: f64,
    estimated_profit: f64,
    settled_profit: Option<f64>,
    tax_rate: f64,
    tax_amount: f64,
    payout_fee_rate: f64,
    payout_fee: f64,
    after_tax_profit: Option<f64>,
    acquiring: f64,
    storage_packaging: f64,
    penalties_adjustments: f64,
    other_finance_fees: f64,
    unallocated_finance_amount: f64,
    finance_operations: i64,
    exact_sku_operations: i64,
    unallocated_operations: i64,
    cash_flow_reported_total: f64,
    reconciliation_difference: Option<f64>,
    missing_cost_skus: i64,
    costed_units: i64,
    missing_cost_units: i64,
    daily: Vec<ReportDay>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProductProfitRow {
    sku: String,
    offer_id: String,
    product_name: String,
    units: i64,
    revenue: f64,
    ad_spend: f64,
    purchase_cost: Option<f64>,
    first_mile_cost: Option<f64>,
    platform_fees: f64,
    estimated_profit: Option<f64>,
    profit_rate: Option<f64>,
    cross_border_freight: Option<f64>,
    cost_complete: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SeriesAnalysisRow {
    period_type: String,
    period: String,
    series: String,
    sku_count: i64,
    units: i64,
    revenue: f64,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DailyProductRow {
    day: String,
    sku: String,
    offer_id: String,
    product_name: String,
    units: i64,
    revenue: f64,
    returns: i64,
    cancellations: i64,
    views: i64,
    ad_spend: f64,
    ad_orders: i64,
    tacos: Option<f64>,
    ad_cost_per_order: Option<f64>,
    estimated_profit: Option<f64>,
    cost_complete: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FinanceBreakdownRow {
    category: String,
    category_label: String,
    name: String,
    api_name: String,
    rows_count: i64,
    amount: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DataCoverageRow {
    source: String,
    rows_count: i64,
    date_from: String,
    date_to: String,
    last_success: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MissingCostRow {
    sku: String,
    offer_id: String,
    product_name: String,
    units: i64,
    missing_purchase: bool,
    missing_first_mile: bool,
    missing_weight: bool,
    missing_dimensions: bool,
    unit_cost: Option<f64>,
    first_mile_cost: Option<f64>,
    weight_kg: Option<f64>,
    length_cm: Option<f64>,
    width_cm: Option<f64>,
    height_cm: Option<f64>,
    note: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WeeklyAnalysisRow {
    period: String,
    revenue: f64,
    units: i64,
    ad_spend: f64,
    ad_orders: i64,
    ad_order_share: f64,
    acots: f64,
    returns: i64,
    cancellations: i64,
    estimated_profit: f64,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WeeklyDailyRow {
    day: String,
    revenue: f64,
    units: i64,
    ad_spend: f64,
    ad_orders: i64,
    ad_order_share: f64,
    acots: f64,
    returns: i64,
    cancellations: i64,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnalyticsDetail {
    products: Vec<ProductProfitRow>,
    daily_products: Vec<DailyProductRow>,
    series: Vec<SeriesAnalysisRow>,
    weekly: Vec<WeeklyAnalysisRow>,
    weekly_daily: Vec<WeeklyDailyRow>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CrossBorderProfitRow {
    sku: String,
    offer_id: String,
    product_name: String,
    units: i64,
    fulfillment_orders: i64,
    fbp_orders: i64,
    rfbs_orders: i64,
    whd_orders: i64,
    revenue_cny: f64,
    selling_price_cny: f64,
    purchase_cost_cny: Option<f64>,
    weight_kg: Option<f64>,
    freight_unit_cny: Option<f64>,
    purchase_total_cny: Option<f64>,
    freight_total_cny: Option<f64>,
    estimated_platform_fees_cny: Option<f64>,
    contribution_cny: Option<f64>,
    finance_settled_cny: Option<f64>,
    commission_rate: Option<f64>,
    acquiring_rate: Option<f64>,
    cost_complete: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CrossBorderDayRow {
    day: String,
    units: i64,
    revenue_cny: f64,
    ad_spend_cny: f64,
    purchase_and_freight_cny: f64,
    profit_cny: Option<f64>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CrossBorderReport {
    date_from: String,
    date_to: String,
    rub_per_cny: f64,
    revenue_cny: f64,
    units: i64,
    ad_spend_cny: f64,
    estimated_platform_fees_cny: f64,
    purchase_and_freight_cny: f64,
    profit_cny: Option<f64>,
    settled_finance_net_cny: f64,
    finance_available: bool,
    commission_rate: Option<f64>,
    acquiring_rate: Option<f64>,
    missing_cost_skus: i64,
    fbp_orders: i64,
    rfbs_orders: i64,
    whd_orders: i64,
    daily: Vec<CrossBorderDayRow>,
    rows: Vec<CrossBorderProfitRow>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SupplyOrderRow {
    order_id: i64,
    order_number: String,
    state: String,
    created_date: String,
    data_filling_deadline: String,
    dropoff_name: String,
    dropoff_address: String,
    timeslot_from: String,
    timeslot_to: String,
    timezone_name: String,
    supply_type: String,
    clusters: String,
    storage_warehouses: String,
    supply_states: String,
    supplies_count: usize,
}
#[derive(Serialize)]
struct SupplyTimeslot {
    from: String,
    to: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SupplyClusterPlanRow {
    sku: String,
    offer_id: String,
    product_name: String,
    macrolocal_cluster_id: String,
    cluster_name: String,
    available_stock: i64,
    transit_stock: i64,
    requested_stock: i64,
    daily_sales: f64,
    recommended_qty: i64,
    planned_qty: i64,
    target_days: i64,
    plan_saved: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SyncLogRow {
    id: i64,
    started_at: String,
    finished_at: String,
    source: String,
    status: String,
    rows_count: i64,
    message: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ShipmentRow {
    tracking_id: String,
    product_name: String,
    batch_no: String,
    shop_name: String,
    quantity: i64,
    cargo_status: String,
    channel: String,
    domestic_arrival: String,
    foreign_arrival: String,
    notified_foreign_arrival: String,
    source: String,
    updated_at: String,
    needs_notification: bool,
    sku_allocations: Vec<ShipmentSkuAllocation>,
    settlement_completed: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ShipmentSkuAllocation {
    sku: String,
    quantity: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ShipmentSkuOption {
    sku: String,
    offer_id: String,
    name: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ShipmentSettlementItem {
    sku: String,
    batch_quantity: i64,
    requested_stock: i64,
    fbo_quantity: i64,
    fbs_quantity: i64,
    overseas_remaining_quantity: i64,
    loss_quantity: i64,
    other_quantity: i64,
    note: String,
}

fn read_registry(data_dir: &Path) -> Result<ShopFile, String> {
    let text = fs::read_to_string(data_dir.join("shops.json")).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SyncAllResult {
    seller_rows: Option<i64>,
    performance_rows: Option<i64>,
    finance_rows: Option<i64>,
    seller_error: String,
    performance_error: String,
    finance_error: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompetitorAlertSettings {
    warning_drop_percent: f64,
    critical_drop_percent: f64,
    opportunity_rise_percent: f64,
}

fn snapshot_database(source: &Path, target: &Path) -> Result<(), String> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if target.exists() {
        return Err(format!(
            "新版数据库目标已存在，拒绝覆盖：{}",
            target.display()
        ));
    }
    let source_db = Connection::open(source)
        .map_err(|e| format!("无法读取旧数据库 {}：{e}", source.display()))?;
    let _ = source_db.execute_batch("PRAGMA wal_checkpoint(PASSIVE);");
    let target_text = target.to_string_lossy().to_string();
    source_db
        .execute("VACUUM INTO ?1", [&target_text])
        .map_err(|e| format!("建立新版数据库快照失败：{e}"))?;
    let destination = Connection::open(target).map_err(|e| e.to_string())?;
    destination
        .execute_batch(
            "DELETE FROM business_report_cache;
             DELETE FROM analytics_detail_cache;
             DELETE FROM sync_progress;",
        )
        .or_else(|_| {
            // Older source databases may not contain every derived cache.
            for table in [
                "business_report_cache",
                "analytics_detail_cache",
                "sync_progress",
            ] {
                let _ = destination.execute(&format!("DELETE FROM {table}"), []);
            }
            Ok::<(), rusqlite::Error>(())
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn initialize_next_data_dir(legacy_dir: &Path, next_dir: &Path) -> Result<(), String> {
    let legacy_registry = read_registry(legacy_dir)?;
    fs::create_dir_all(next_dir.join("shops")).map_err(|e| e.to_string())?;
    let mut next_registry = legacy_registry.clone();
    for shop in &mut next_registry.shops {
        let source = legacy_dir.join(&shop.database_file);
        let safe_id: String = shop
            .id
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
            .collect();
        let relative = if shop.id == "default" {
            "ozon_next_default.db".to_string()
        } else {
            format!(
                "shops/shop_next_{}.db",
                if safe_id.is_empty() {
                    "store"
                } else {
                    &safe_id
                }
            )
        };
        snapshot_database(&source, &next_dir.join(&relative))?;
        shop.database_file = relative;
    }
    let legacy_wb = legacy_dir.join("wb").join("wb_analytics.db");
    if legacy_wb.is_file() {
        snapshot_database(&legacy_wb, &next_dir.join("wb").join("wb_analytics.db"))?;
    }
    write_registry(next_dir, &next_registry)?;
    fs::write(
        next_dir.join("database-generation.json"),
        format!(
            "{{\n  \"generation\": \"desktop-next-v2\",\n  \"created_at\": \"{}\",\n  \"legacy_runtime_reuse\": false\n}}\n",
            chrono::Utc::now().to_rfc3339()
        ),
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn copy_directory(source: &Path, target: &Path) -> Result<(), String> {
    fs::create_dir_all(target).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let destination = target.join(entry.file_name());
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            copy_directory(&entry.path(), &destination)?;
        } else {
            fs::copy(entry.path(), destination).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn locate_data_dir(
    executable: &Path,
    local_app_dir: &Path,
    resource_dir: &Path,
) -> Result<PathBuf, String> {
    let executable_dir = executable.parent().ok_or("无法确定程序目录")?;
    for root in executable_dir.ancestors() {
        let next = root.join("data-next");
        if next.join("shops.json").is_file() && next.join("database-generation.json").is_file() {
            return Ok(next);
        }
    }
    for root in executable_dir.ancestors() {
        let legacy = root.join("data");
        if legacy.join("shops.json").is_file() {
            let next = root.join("data-next");
            initialize_next_data_dir(&legacy, &next)?;
            return Ok(next);
        }
    }
    let local_data = local_app_dir.join("data-next");
    if local_data.join("shops.json").is_file() {
        return Ok(local_data);
    }
    for template in [
        resource_dir.join("data-next-template"),
        resource_dir.join("resources").join("data-next-template"),
    ] {
        if template.join("shops.json").is_file() {
            copy_directory(&template, &local_data)?;
            return Ok(local_data);
        }
    }
    Err(format!(
        "无法初始化本地数据目录。安装资源缺少 data-next-template；预期本地目录：{}",
        local_data.display()
    ))
}
fn initialize_extensions(c: &Connection) -> Result<(), String> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS warehouse_cluster_mappings(warehouse_name TEXT PRIMARY KEY,cluster_name TEXT NOT NULL DEFAULT '',updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);CREATE TABLE IF NOT EXISTS competitor_products(id INTEGER PRIMARY KEY AUTOINCREMENT,product_url TEXT NOT NULL UNIQUE,product_code TEXT NOT NULL DEFAULT '',name TEXT NOT NULL DEFAULT '',image_url TEXT NOT NULL DEFAULT '',active INTEGER NOT NULL DEFAULT 1,created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);CREATE TABLE IF NOT EXISTS competitor_snapshots(id INTEGER PRIMARY KEY AUTOINCREMENT,competitor_id INTEGER NOT NULL,captured_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,price REAL,sales_total INTEGER,source TEXT NOT NULL DEFAULT 'web',UNIQUE(competitor_id,captured_at),FOREIGN KEY(competitor_id) REFERENCES competitor_products(id));CREATE TABLE IF NOT EXISTS shipment_tracking(tracking_id TEXT PRIMARY KEY,product_name TEXT NOT NULL DEFAULT '',batch_no TEXT NOT NULL DEFAULT '',shop_name TEXT NOT NULL DEFAULT '',quantity INTEGER NOT NULL DEFAULT 0,cargo_status TEXT NOT NULL DEFAULT '',channel TEXT NOT NULL DEFAULT '',domestic_arrival TEXT NOT NULL DEFAULT '',foreign_arrival TEXT NOT NULL DEFAULT '',notified_foreign_arrival TEXT NOT NULL DEFAULT '',source TEXT NOT NULL DEFAULT 'local',remote_record_id TEXT NOT NULL DEFAULT '',updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);CREATE INDEX IF NOT EXISTS idx_posting_routes_day ON posting_routes(day);CREATE INDEX IF NOT EXISTS idx_posting_routes_sku_destination ON posting_routes(sku,destination);CREATE INDEX IF NOT EXISTS idx_sales_daily_day_sku ON sales_daily(day,sku);CREATE INDEX IF NOT EXISTS idx_ad_daily_day_sku ON ad_daily(day,sku);CREATE INDEX IF NOT EXISTS idx_finance_operation_date ON finance_transactions(operation_date);").map_err(|e|e.to_string())?;
    c.execute_batch("CREATE TABLE IF NOT EXISTS product_cluster_weights(sku TEXT NOT NULL,cluster_name TEXT NOT NULL,weight REAL NOT NULL DEFAULT 0,updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,PRIMARY KEY(sku,cluster_name));").map_err(|e|e.to_string())?;
    c.execute_batch("CREATE TABLE IF NOT EXISTS inventory_totals(sku TEXT PRIMARY KEY,offer_id TEXT NOT NULL DEFAULT '',present_stock INTEGER NOT NULL DEFAULT 0,reserved_stock INTEGER NOT NULL DEFAULT 0,updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);").map_err(|e|e.to_string())?;
    for column in [
        "sku TEXT NOT NULL DEFAULT ''",
        "production_qty INTEGER NOT NULL DEFAULT 0",
        "domestic_stock_qty INTEGER NOT NULL DEFAULT 0",
        "overseas_transit_qty INTEGER NOT NULL DEFAULT 0",
        "overseas_arrived_qty INTEGER NOT NULL DEFAULT 0",
        "foreign_appointment TEXT NOT NULL DEFAULT ''",
        "supply_source_shop_id TEXT NOT NULL DEFAULT ''",
        "status_formula_version TEXT NOT NULL DEFAULT ''",
    ] {
        let _ = c.execute(
            &format!("ALTER TABLE shipment_tracking ADD COLUMN {column}"),
            [],
        );
    }
    let _ = c.execute(
        "CREATE INDEX IF NOT EXISTS idx_shipment_tracking_sku ON shipment_tracking(sku)",
        [],
    );
    c.execute_batch("CREATE TABLE IF NOT EXISTS feishu_supply_chain_product_mappings(product_name TEXT NOT NULL,shop_name TEXT NOT NULL DEFAULT '',sku TEXT NOT NULL,source TEXT NOT NULL DEFAULT 'manual',updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,PRIMARY KEY(product_name,shop_name));CREATE TABLE IF NOT EXISTS feishu_supply_chain_sync_runs(id INTEGER PRIMARY KEY AUTOINCREMENT,synced_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,source_shop_id TEXT NOT NULL DEFAULT '',source_shop_name TEXT NOT NULL DEFAULT '',records_count INTEGER NOT NULL DEFAULT 0,matched_count INTEGER NOT NULL DEFAULT 0,unmatched_count INTEGER NOT NULL DEFAULT 0,unshipped_qty INTEGER NOT NULL DEFAULT 0,transit_qty INTEGER NOT NULL DEFAULT 0,overseas_arrived_qty INTEGER NOT NULL DEFAULT 0,delivered_qty INTEGER NOT NULL DEFAULT 0,status_formula_version TEXT NOT NULL DEFAULT 'cargo-status-v1');").map_err(|e|e.to_string())?;
    c.execute_batch("CREATE TABLE IF NOT EXISTS shipment_sku_allocations(tracking_id TEXT NOT NULL,sku TEXT NOT NULL,quantity INTEGER NOT NULL CHECK(quantity>0),updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,PRIMARY KEY(tracking_id,sku),FOREIGN KEY(tracking_id) REFERENCES shipment_tracking(tracking_id) ON DELETE CASCADE);CREATE INDEX IF NOT EXISTS idx_shipment_sku_allocations_sku ON shipment_sku_allocations(sku);").map_err(|e|e.to_string())?;
    for column in [
        "fbo_quantity INTEGER NOT NULL DEFAULT 0",
        "fbs_quantity INTEGER NOT NULL DEFAULT 0",
        "overseas_remaining_quantity INTEGER NOT NULL DEFAULT 0",
        "loss_quantity INTEGER NOT NULL DEFAULT 0",
        "other_quantity INTEGER NOT NULL DEFAULT 0",
        "settled INTEGER NOT NULL DEFAULT 0",
        "settlement_note TEXT NOT NULL DEFAULT ''",
        "settled_at TEXT NOT NULL DEFAULT ''",
    ] {
        let _ = c.execute(
            &format!("ALTER TABLE shipment_sku_allocations ADD COLUMN {column}"),
            [],
        );
    }
    c.execute_batch("CREATE TABLE IF NOT EXISTS campaign_action_logs(id INTEGER PRIMARY KEY AUTOINCREMENT,campaign_id TEXT NOT NULL,action TEXT NOT NULL,requested_value TEXT NOT NULL DEFAULT '',before_state TEXT NOT NULL DEFAULT '',before_budget REAL NOT NULL DEFAULT 0,after_state TEXT NOT NULL DEFAULT '',after_budget REAL NOT NULL DEFAULT 0,status TEXT NOT NULL DEFAULT 'pending',message TEXT NOT NULL DEFAULT '',before_from TEXT NOT NULL DEFAULT '',before_to TEXT NOT NULL DEFAULT '',before_spend REAL NOT NULL DEFAULT 0,before_revenue REAL NOT NULL DEFAULT 0,created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);CREATE INDEX IF NOT EXISTS idx_campaign_action_logs_campaign ON campaign_action_logs(campaign_id,created_at DESC);").map_err(|e|e.to_string())?;
    c.execute_batch("CREATE TABLE IF NOT EXISTS competitor_collection_runs(run_id TEXT PRIMARY KEY,workflow TEXT NOT NULL DEFAULT 'product_snapshot',platform TEXT NOT NULL DEFAULT 'ozon',locale TEXT NOT NULL DEFAULT 'ru-RU',started_at TEXT NOT NULL,finished_at TEXT NOT NULL DEFAULT '',search_context TEXT NOT NULL DEFAULT '',requested_scope TEXT NOT NULL DEFAULT '',completed_scope TEXT NOT NULL DEFAULT '',requested INTEGER NOT NULL DEFAULT 0,completed INTEGER NOT NULL DEFAULT 0,ok INTEGER NOT NULL DEFAULT 0,blocked INTEGER NOT NULL DEFAULT 0,changed_layout INTEGER NOT NULL DEFAULT 0,inaccessible INTEGER NOT NULL DEFAULT 0,ambiguous_match INTEGER NOT NULL DEFAULT 0,incomplete INTEGER NOT NULL DEFAULT 0,status TEXT NOT NULL DEFAULT 'running',notes TEXT NOT NULL DEFAULT '');CREATE TABLE IF NOT EXISTS competitor_observations(id INTEGER PRIMARY KEY AUTOINCREMENT,run_id TEXT NOT NULL,competitor_id INTEGER NOT NULL,observed_at TEXT NOT NULL,status TEXT NOT NULL,retry_count INTEGER NOT NULL DEFAULT 0,source_url TEXT NOT NULL DEFAULT '',final_url TEXT NOT NULL DEFAULT '',locale TEXT NOT NULL DEFAULT 'ru-RU',search_context TEXT NOT NULL DEFAULT '',price_raw TEXT NOT NULL DEFAULT '',sales_raw TEXT NOT NULL DEFAULT '',evidence TEXT NOT NULL DEFAULT '',notes TEXT NOT NULL DEFAULT '',FOREIGN KEY(run_id) REFERENCES competitor_collection_runs(run_id),FOREIGN KEY(competitor_id) REFERENCES competitor_products(id),UNIQUE(run_id,competitor_id));CREATE TABLE IF NOT EXISTS competitor_manual_metrics(competitor_id INTEGER PRIMARY KEY,daily_sales INTEGER,weekly_sales INTEGER,monthly_sales INTEGER,updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,FOREIGN KEY(competitor_id) REFERENCES competitor_products(id));CREATE INDEX IF NOT EXISTS idx_competitor_observations_competitor ON competitor_observations(competitor_id,observed_at DESC);").map_err(|e|e.to_string())?;
    let _ = c.execute(
        "ALTER TABLE campaigns ADD COLUMN budget_known INTEGER NOT NULL DEFAULT 0",
        [],
    );
    let _ = c.execute(
        "ALTER TABLE campaigns ADD COLUMN budget_updated_at TEXT NOT NULL DEFAULT ''",
        [],
    );
    let _ = c.execute(
        "ALTER TABLE campaigns ADD COLUMN budget_scale_version INTEGER NOT NULL DEFAULT 0",
        [],
    );
    c.execute("UPDATE campaigns SET budget=CASE WHEN budget_known=1 THEN budget/1000000.0 ELSE budget END,budget_scale_version=1 WHERE budget_scale_version=0", []).map_err(|e|e.to_string())?;
    c.execute_batch("CREATE INDEX IF NOT EXISTS idx_inventory_stock_sku ON inventory_stock(sku);CREATE INDEX IF NOT EXISTS idx_replenishment_plan_sku ON replenishment_plan(sku);").map_err(|e|e.to_string())?;
    let has_image = c
        .prepare("PRAGMA table_info(products)")
        .and_then(|mut s| {
            s.query_map([], |r| r.get::<_, String>(1))?
                .collect::<Result<Vec<_>, _>>()
        })
        .map_err(|e| e.to_string())?
        .iter()
        .any(|name| name == "image_url");
    if !has_image {
        c.execute(
            "ALTER TABLE products ADD COLUMN image_url TEXT NOT NULL DEFAULT ''",
            [],
        )
        .map_err(|e| e.to_string())?;
    }
    // Seller Analytics is a valid product source even when the optional
    // catalog endpoint returns no metadata or the token lacks catalog access.
    // Backfill both existing databases and newly synchronized shops so the
    // product center never stays empty while sales_daily already has SKUs.
    if setting(c, "seller_product_backfill_version") != "1" {
        c.execute(
            "INSERT INTO products(sku,name,source,updated_at)
             SELECT s.sku,COALESCE(MAX(NULLIF(s.product_name,'')),''),'seller_analytics',CURRENT_TIMESTAMP
             FROM sales_daily s WHERE s.sku<>'' GROUP BY s.sku
             ON CONFLICT(sku) DO UPDATE SET
               name=CASE WHEN products.name='' AND excluded.name<>'' THEN excluded.name ELSE products.name END,
               updated_at=CASE WHEN products.source='seller_analytics' THEN CURRENT_TIMESTAMP ELSE products.updated_at END",
            [],
        )
        .map_err(|e| e.to_string())?;
        save_setting(c, "seller_product_backfill_version", "1")?;
    }
    Ok(())
}
fn active_shop_database_path(state: &AppState) -> Result<PathBuf, String> {
    let registry = read_registry(&state.data_dir)?;
    let id = state
        .active_shop_id
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    let shop = registry
        .shops
        .iter()
        .find(|x| x.id == id)
        .or_else(|| registry.shops.first())
        .ok_or("未配置店铺")?;
    Ok(state.data_dir.join(&shop.database_file))
}

pub(crate) fn db(state: &AppState) -> Result<Connection, String> {
    let c = Connection::open(active_shop_database_path(state)?).map_err(|e| e.to_string())?;
    c.busy_timeout(std::time::Duration::from_secs(30))
        .map_err(|e| e.to_string())?;
    initialize_extensions(&c)?;
    Ok(c)
}
pub(crate) fn background_state(state: &AppState) -> Result<AppState, String> {
    Ok(AppState {
        data_dir: state.data_dir.clone(),
        active_shop_id: Mutex::new(
            state
                .active_shop_id
                .lock()
                .map_err(|e| e.to_string())?
                .clone(),
        ),
    })
}
fn setting(conn: &Connection, key: &str) -> String {
    conn.query_row("SELECT value FROM settings WHERE key=?1", [key], |r| {
        r.get(0)
    })
    .unwrap_or_default()
}

fn active_shop_kind(state: &AppState) -> Result<String, String> {
    let registry = read_registry(&state.data_dir)?;
    let id = state
        .active_shop_id
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    Ok(registry
        .shops
        .iter()
        .find(|x| x.id == id)
        .or_else(|| registry.shops.first())
        .map(|x| x.kind.clone())
        .unwrap_or_else(|| "local".into()))
}

fn active_shop_identity(state: &AppState) -> Result<(String, String), String> {
    let registry = read_registry(&state.data_dir)?;
    let id = state
        .active_shop_id
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    let shop = registry
        .shops
        .iter()
        .find(|x| x.id == id)
        .ok_or("当前店铺不存在")?;
    Ok((shop.id.clone(), shop.name.clone()))
}

fn rub_per_cny_for(state: &AppState, c: &Connection) -> Result<f64, String> {
    let key = if active_shop_kind(state)? == "cross_border" {
        "cross_border_rub_per_cny"
    } else {
        "local_rub_per_cny"
    };
    let preferred = setting(c, key);
    let legacy = setting(c, "rub_per_cny");
    Ok(preferred
        .parse::<f64>()
        .ok()
        .filter(|v| *v > 0.0)
        .or_else(|| legacy.parse::<f64>().ok().filter(|v| *v > 0.0))
        .unwrap_or(if key == "cross_border_rub_per_cny" {
            14.0
        } else {
            11.5
        }))
}
fn display_amount(value_rub: f64, cross_border: bool, rub_per_cny: f64) -> f64 {
    if cross_border && rub_per_cny > 0.0 {
        value_rub / rub_per_cny
    } else {
        value_rub
    }
}
fn save_setting(c: &Connection, key: &str, value: &str) -> Result<(), String> {
    c.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,value]).map_err(|e|e.to_string())?;
    Ok(())
}
fn secret_setting(c: &Connection, key: &str) -> Result<String, String> {
    secrets::unprotect(&setting(c, key)).map_err(|error| {
        if error.contains("-2146893813") {
            "本机保存的 API 密钥已无法由 Windows DPAPI 解密。请打开“连接设置”，重新输入 Seller API Key 和 Performance Client Secret 后保存；历史业务数据不会丢失。".to_string()
        } else { error }
    })
}

pub(crate) fn seller_post(
    c: &Connection,
    path: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let client_id = setting(c, "seller_client_id");
    let api_key = secret_setting(c, "seller_api_key")?;
    if client_id.is_empty() || api_key.is_empty() {
        return Err("请先在连接设置中配置 Seller Client ID 和 API Key".into());
    }
    let url = format!("https://api-seller.ozon.ru{path}");
    let body_text = body.to_string();
    let mut response = None;
    let max_attempts = if path == "/v1/analytics/data" { 2 } else { 4 };
    for attempt in 0..max_attempts {
        match ureq::post(&url).set("Client-Id",&client_id).set("Api-Key",&api_key).set("Content-Type","application/json").set("Accept","application/json").set("User-Agent","OzonERPDesktop/0.1").send_string(&body_text){
            Ok(value)=>{response=Some(value);break},
            Err(ureq::Error::Status(status,value)) if status==429||matches!(status,500|502|503|504)=>{
                if attempt+1==max_attempts{return Err(if status==429{format!("Ozon Seller API 接口 {path} 自动等待并重试后仍被限频（HTTP 429）。请不要连续点击同步，稍后再试；已有本地缓存不会被清除。") }else{format!("Ozon Seller API 接口 {path} 暂时不可用（HTTP {status}），自动重试后仍未恢复。")})}
                let retry_after=value.header("Retry-After").and_then(|v|v.trim().parse::<u64>().ok());
                let delay=if status==429{retry_after.unwrap_or(65).clamp(1,180)}else{2u64.pow(attempt+1).min(15)};
                std::thread::sleep(std::time::Duration::from_secs(delay));
            },
            Err(ureq::Error::Status(status,_)) if matches!(status,401|403)=>return Err(format!("Ozon Seller API 认证失败（HTTP {status}，接口 {path}），请检查当前店铺 API 凭证与权限。")),
            Err(error)=>return Err(format!("Ozon Seller API 请求失败（{path}）：{error}")),
        }
    }
    let response = response.ok_or_else(|| format!("Ozon Seller API 接口 {path} 未返回响应"))?;
    let raw = response.into_string().map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| format!("Ozon Seller API 返回无法解析的 JSON：{e}"))
}

fn performance_token(c: &Connection) -> Result<String, String> {
    let id = setting(c, "performance_client_id");
    let secret = secret_setting(c, "performance_client_secret")?;
    if id.is_empty() || secret.is_empty() {
        return Err("请先在连接设置中配置 Performance Client ID 和 Client Secret".into());
    }
    let body = serde_json::json!({"client_id":id,"client_secret":secret,"grant_type":"client_credentials"});
    let response = ureq::post("https://api-performance.ozon.ru/api/client/token")
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| format!("Performance 认证失败：{e}"))?;
    let raw = response.into_string().map_err(|e| e.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let token = json_text(value.get("access_token"));
    if token.is_empty() {
        Err("Performance API 未返回 Access Token".into())
    } else {
        Ok(token)
    }
}
fn performance_get(path_and_query: &str, token: &str) -> Result<serde_json::Value, String> {
    let url = format!("https://api-performance.ozon.ru{path_and_query}");
    let response = ureq::get(&url)
        .set("Authorization", &format!("Bearer {token}"))
        .set("Accept", "application/json")
        .call()
        .map_err(|e| format!("Performance API 请求失败（{path_and_query}）：{e}"))?;
    let raw = response.into_string().map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| format!("Performance API 返回无法解析：{e}"))
}
fn performance_post(
    path: &str,
    token: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let url = format!("https://api-performance.ozon.ru{path}");
    let response = ureq::post(&url)
        .set("Authorization", &format!("Bearer {token}"))
        .set("Accept", "application/json")
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| format!("Performance API 请求失败（{path}）：{e}"))?;
    let raw = response.into_string().map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| format!("Performance API 返回无法解析：{e}"))
}
fn performance_mutation(
    method: &str,
    path: &str,
    token: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let url = format!("https://api-performance.ozon.ru{path}");
    let request = if method == "PATCH" {
        ureq::patch(&url)
    } else {
        ureq::post(&url)
    };
    let response = request
        .set("Authorization", &format!("Bearer {token}"))
        .set("Accept", "application/json")
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| format!("Performance API 写操作失败（{path}）：{e}"))?;
    let raw = response.into_string().map_err(|e| e.to_string())?;
    if raw.trim().is_empty() {
        Ok(serde_json::json!({}))
    } else {
        serde_json::from_str(&raw).map_err(|e| format!("Performance API 返回无法解析：{e}"))
    }
}
fn feishu_base(c: &Connection) -> String {
    let value = setting(c, "feishu_base_url");
    if value.is_empty() {
        "https://open.feishu.cn/open-apis".into()
    } else {
        value.trim_end_matches('/').into()
    }
}
fn feishu_raw(
    method: &str,
    url: &str,
    token: Option<&str>,
    body: Option<&serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let mut request =
        ureq::request(method, url).set("Content-Type", "application/json; charset=utf-8");
    if let Some(value) = token {
        request = request.set("Authorization", &format!("Bearer {value}"));
    }
    let response = if let Some(value) = body {
        request.send_string(&value.to_string())
    } else {
        request.call()
    }
    .map_err(|e| format!("飞书 API 请求失败：{e}"))?;
    let raw = response.into_string().map_err(|e| e.to_string())?;
    let payload: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("飞书 API 返回无法解析：{e}"))?;
    let code = payload.get("code").and_then(|v| v.as_i64()).unwrap_or(0);
    if code != 0 {
        return Err(format!(
            "飞书 API 错误 {code}：{}",
            json_text(payload.get("msg"))
        ));
    }
    Ok(payload)
}
fn feishu_token(c: &Connection) -> Result<String, String> {
    let id = setting(c, "feishu_app_id");
    let secret = secret_setting(c, "feishu_app_secret")?;
    if id.is_empty() || secret.is_empty() {
        return Err("请先配置飞书 App ID 和 App Secret".into());
    }
    let payload = feishu_raw(
        "POST",
        &format!("{}/auth/v3/tenant_access_token/internal/", feishu_base(c)),
        None,
        Some(&serde_json::json!({"app_id":id,"app_secret":secret})),
    )?;
    let token = json_text(payload.get("tenant_access_token"));
    if token.is_empty() {
        Err("飞书认证响应缺少 tenant_access_token".into())
    } else {
        Ok(token)
    }
}
fn feishu_table_path(c: &Connection) -> Result<String, String> {
    let app = setting(c, "feishu_app_token");
    let table = setting(c, "feishu_product_table_id");
    if app.is_empty() || table.is_empty() {
        Err("请先配置飞书 App Token 和商品表 Table ID".into())
    } else {
        Ok(format!(
            "{}/bitable/v1/apps/{app}/tables/{table}",
            feishu_base(c)
        ))
    }
}
fn feishu_field_text(value: Option<&serde_json::Value>) -> String {
    match value {
        None | Some(serde_json::Value::Null) => String::new(),
        Some(serde_json::Value::String(s)) => s.trim().into(),
        Some(serde_json::Value::Number(n)) => n.to_string(),
        Some(serde_json::Value::Array(a)) => a
            .iter()
            .map(|v| feishu_field_text(Some(v)))
            .collect::<Vec<_>>()
            .join(""),
        Some(serde_json::Value::Object(o)) => ["text", "name", "value"]
            .iter()
            .find_map(|k| o.get(*k))
            .map(|v| feishu_field_text(Some(v)))
            .unwrap_or_default(),
        Some(v) => v.to_string(),
    }
}
fn feishu_field_number(value: Option<&serde_json::Value>) -> Option<f64> {
    value.and_then(|v| v.as_f64()).or_else(|| {
        let text = feishu_field_text(value).replace(' ', "").replace(',', ".");
        text.parse().ok()
    })
}
fn collect_objects(
    value: &serde_json::Value,
    out: &mut Vec<serde_json::Map<String, serde_json::Value>>,
) {
    match value {
        serde_json::Value::Array(a) => {
            for v in a {
                collect_objects(v, out)
            }
        }
        serde_json::Value::Object(o) => {
            if o.keys()
                .any(|k| matches!(k.as_str(), "id" | "campaignId" | "campaign_id"))
                && o.keys().any(|k| {
                    matches!(
                        k.as_str(),
                        "date" | "day" | "views" | "clicks" | "expense" | "orders"
                    )
                })
            {
                out.push(o.clone())
            } else {
                for v in o.values() {
                    collect_objects(v, out)
                }
            }
        }
        _ => {}
    }
}
fn object_text(o: &serde_json::Map<String, serde_json::Value>, keys: &[&str]) -> String {
    for key in keys {
        if let Some(v) = o.get(*key) {
            if let Some(s) = v.as_str() {
                if !s.is_empty() {
                    return s.into();
                }
            } else if v.is_number() {
                return v.to_string();
            }
        }
    }
    String::new()
}
fn object_number(o: &serde_json::Map<String, serde_json::Value>, keys: &[&str]) -> f64 {
    for key in keys {
        if let Some(v) = o.get(*key) {
            if let Some(n) = v.as_f64() {
                return n;
            }
            if let Some(n) = v
                .as_str()
                .and_then(|s| s.replace(' ', "").replace(',', ".").parse().ok())
            {
                return n;
            }
        }
    }
    0.0
}
#[tauri::command]
fn list_shops(state: State<AppState>) -> Result<Vec<Shop>, String> {
    let registry = read_registry(&state.data_dir)?;
    let active = state
        .active_shop_id
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    Ok(registry
        .shops
        .into_iter()
        .map(|s| Shop {
            database_size_bytes: fs::metadata(state.data_dir.join(&s.database_file))
                .map(|value| value.len())
                .unwrap_or(0),
            id: s.id.clone(),
            name: s.name,
            kind: s.kind,
            api_name: s.api_name,
            active: s.id == active,
        })
        .collect())
}
#[tauri::command]
fn select_shop(shop_id: String, state: State<AppState>) -> Result<(), String> {
    let registry = read_registry(&state.data_dir)?;
    if !registry.shops.iter().any(|s| s.id == shop_id) {
        return Err("店铺不存在".into());
    }
    *state.active_shop_id.lock().map_err(|e| e.to_string())? = shop_id;
    Ok(())
}

fn write_registry(data_dir: &Path, registry: &ShopFile) -> Result<(), String> {
    let text = serde_json::to_string_pretty(registry).map_err(|e| e.to_string())?;
    fs::write(data_dir.join("shops.json"), format!("{text}\n")).map_err(|e| e.to_string())
}

#[tauri::command]
fn create_shop(
    name: String,
    kind: String,
    api_name: String,
    state: State<AppState>,
) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("店铺名称不能为空".into());
    }
    if !matches!(kind.as_str(), "local" | "cross_border") {
        return Err("店铺类型无效".into());
    }
    let mut registry = read_registry(&state.data_dir)?;
    let id = format!("{:x}", chrono::Local::now().timestamp_micros());
    let relative = format!("shops/shop_{id}.db");
    let target = state.data_dir.join(&relative);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let source_shop = registry
        .shops
        .iter()
        .find(|x| x.id == registry.active_shop_id)
        .or_else(|| registry.shops.first())
        .ok_or("没有可用于初始化结构的店铺数据库")?;
    let source = Connection::open(state.data_dir.join(&source_shop.database_file))
        .map_err(|e| e.to_string())?;
    let destination = Connection::open(&target).map_err(|e| e.to_string())?;
    let schemas = {
        let mut stmt=source.prepare("SELECT sql FROM sqlite_master WHERE sql IS NOT NULL AND type IN('table','index','trigger') ORDER BY CASE type WHEN 'table' THEN 0 WHEN 'index' THEN 1 ELSE 2 END,name").map_err(|e|e.to_string())?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        rows
    };
    for sql in schemas {
        if !sql.starts_with("CREATE TABLE sqlite_") {
            destination
                .execute_batch(&sql)
                .map_err(|e| format!("初始化店铺数据库失败：{e}"))?;
        }
    }
    registry.shops.push(RawShop {
        id: id.clone(),
        name: name.into(),
        kind,
        database_file: relative,
        api_name: if api_name.trim().is_empty() {
            name.into()
        } else {
            api_name.trim().into()
        },
    });
    write_registry(&state.data_dir, &registry)?;
    Ok(id)
}

#[tauri::command]
fn update_shop(
    shop_id: String,
    name: String,
    kind: String,
    api_name: String,
    state: State<AppState>,
) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("店铺名称不能为空".into());
    }
    if !matches!(kind.as_str(), "local" | "cross_border") {
        return Err("店铺类型无效".into());
    }
    let mut registry = read_registry(&state.data_dir)?;
    let shop = registry
        .shops
        .iter_mut()
        .find(|x| x.id == shop_id)
        .ok_or("店铺不存在")?;
    let stable_id = shop.id.clone();
    let stable_database_file = shop.database_file.clone();
    shop.name = name.trim().into();
    shop.kind = kind;
    shop.api_name = api_name.trim().into();
    if shop.id != stable_id || shop.database_file != stable_database_file {
        return Err("店铺重命名不得改变专属 ID 或数据库文件".into());
    }
    write_registry(&state.data_dir, &registry)
}

#[tauri::command]
fn delete_shop(
    shop_id: String,
    confirmation: String,
    state: State<AppState>,
) -> Result<String, String> {
    let mut registry = read_registry(&state.data_dir)?;
    if registry.shops.len() <= 1 {
        return Err("至少保留一个店铺".into());
    }
    if registry.active_shop_id == shop_id {
        return Err("不能删除当前店铺；请先切换到其他店铺".into());
    }
    let index = registry
        .shops
        .iter()
        .position(|x| x.id == shop_id)
        .ok_or("店铺不存在")?;
    if confirmation != registry.shops[index].name {
        return Err("确认文字与店铺名称不一致".into());
    }
    let shop = registry.shops.remove(index);
    let source = state.data_dir.join(&shop.database_file);
    let trash = state.data_dir.join("trash");
    fs::create_dir_all(&trash).map_err(|e| e.to_string())?;
    let backup = trash.join(format!(
        "{}_{}_{}.db",
        shop.id,
        chrono::Local::now().format("%Y%m%d_%H%M%S"),
        "deleted"
    ));
    if source.exists() {
        fs::rename(&source, &backup).map_err(|e| format!("移动数据库到回收目录失败：{e}"))?;
    }
    write_registry(&state.data_dir, &registry)?;
    Ok(format!(
        "店铺已删除，数据库可从 {} 恢复",
        backup.to_string_lossy()
    ))
}

#[tauri::command]
fn dashboard(range: DateRange, state: State<AppState>) -> Result<DashboardData, String> {
    let c = db(&state)?;
    let cross_border = active_shop_kind(&state)? == "cross_border";
    let rate = rub_per_cny_for(&state, &c)?;
    let (revenue,orders,views):(f64,i64,i64)=c.query_row("SELECT COALESCE(SUM(revenue),0),COALESCE(SUM(ordered_units),0),COALESCE(SUM(views),0) FROM sales_daily WHERE day BETWEEN ?1 AND ?2",params![range.from,range.to],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e|e.to_string())?;
    let return_units: i64 = c.query_row("SELECT CASE WHEN EXISTS(SELECT 1 FROM return_events WHERE day BETWEEN ?1 AND ?2) THEN COALESCE((SELECT SUM(quantity) FROM return_events WHERE day BETWEEN ?1 AND ?2),0) ELSE COALESCE((SELECT SUM(returns) FROM sales_daily WHERE day BETWEEN ?1 AND ?2),0) END",params![range.from,range.to],|r|r.get(0)).map_err(|e|e.to_string())?;
    let cancellation_units: i64 = c.query_row("SELECT CASE WHEN EXISTS(SELECT 1 FROM cancellation_events WHERE day BETWEEN ?1 AND ?2) THEN COALESCE((SELECT SUM(quantity) FROM cancellation_events WHERE day BETWEEN ?1 AND ?2),0) ELSE COALESCE((SELECT SUM(cancellations) FROM sales_daily WHERE day BETWEEN ?1 AND ?2),0) END",params![range.from,range.to],|r|r.get(0)).map_err(|e|e.to_string())?;
    let sold_units = orders;
    let active_products: i64 = c
        .query_row("SELECT COUNT(*) FROM products", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let (ad_spend,ad_revenue,ad_orders,clicks,impressions):(f64,f64,i64,i64,i64)=c.query_row("WITH x AS(SELECT * FROM ad_daily WHERE day BETWEEN ?1 AND ?2),m AS(SELECT EXISTS(SELECT 1 FROM x WHERE sku='') store)SELECT COALESCE(SUM(spend),0),COALESCE(SUM(revenue),0),COALESCE(SUM(orders),0),COALESCE(SUM(clicks),0),COALESCE(SUM(impressions),0)FROM x,m WHERE (m.store=1 AND x.sku='')OR(m.store=0 AND x.sku<>'')",params![range.from,range.to],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).map_err(|e|e.to_string())?;
    let mut stmt=c.prepare("SELECT s.day,COALESCE(SUM(s.revenue),0),COALESCE(SUM(s.ordered_units),0),COALESCE((SELECT CASE WHEN EXISTS(SELECT 1 FROM ad_daily z WHERE z.day=s.day AND z.sku='') THEN SUM(CASE WHEN a.sku='' THEN a.spend ELSE 0 END) ELSE SUM(CASE WHEN a.sku<>'' THEN a.spend ELSE 0 END) END FROM ad_daily a WHERE a.day=s.day),0) FROM sales_daily s WHERE s.day BETWEEN ?1 AND ?2 GROUP BY s.day ORDER BY s.day").map_err(|e|e.to_string())?;
    let mut trend = stmt
        .query_map(params![range.from, range.to], |r| {
            Ok(TrendRow {
                day: r.get(0)?,
                revenue: r.get(1)?,
                units: r.get(2)?,
                ad_spend: r.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    if cross_border {
        for row in &mut trend {
            row.revenue = display_amount(row.revenue, true, rate);
            row.ad_spend = display_amount(row.ad_spend, true, rate);
        }
    }
    let last_sync = c
        .query_row(
            "SELECT finished_at FROM sync_logs WHERE status='success' ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .ok();
    Ok(DashboardData {
        revenue: display_amount(revenue, cross_border, rate),
        orders,
        sold_units,
        active_products,
        ad_spend: display_amount(ad_spend, cross_border, rate),
        ad_revenue: display_amount(ad_revenue, cross_border, rate),
        ad_orders,
        conversion_rate: if clicks > 0 {
            Some(ad_orders as f64 / clicks as f64 * 100.0)
        } else {
            None
        },
        trend,
        last_sync,
        acos: if ad_revenue > 0.0 {
            Some(ad_spend / ad_revenue * 100.0)
        } else {
            None
        },
        tacos: if revenue > 0.0 {
            Some(ad_spend / revenue * 100.0)
        } else {
            None
        },
        ctr: if impressions > 0 {
            Some(clicks as f64 / impressions as f64 * 100.0)
        } else {
            None
        },
        return_units,
        return_rate: if orders > 0 {
            Some(return_units as f64 / orders as f64 * 100.0)
        } else {
            None
        },
        cancellation_units,
        cancellation_rate: if orders > 0 {
            Some(cancellation_units as f64 / orders as f64 * 100.0)
        } else {
            None
        },
        views,
        order_conversion: if views > 0 {
            Some(orders as f64 / views as f64 * 100.0)
        } else {
            None
        },
    })
}

#[tauri::command]
fn orders(
    range: DateRange,
    query: String,
    state: State<AppState>,
) -> Result<Vec<OrderRow>, String> {
    let c = db(&state)?;
    let cross_border = active_shop_kind(&state)? == "cross_border";
    let rate = rub_per_cny_for(&state, &c)?;
    let needle = format!("%{}%", query.trim());
    let mut stmt = c.prepare(
        "SELECT r.event_id,r.posting_number,r.sku,r.offer_id,r.product_name,r.quantity,r.scheme,r.status,r.order_price,r.day,r.updated_at,r.origin,r.destination,COALESCE(p.image_url,'')
         FROM posting_routes r LEFT JOIN products p ON p.sku=r.sku
         WHERE r.day BETWEEN ?1 AND ?2
           AND ((?4=1 AND UPPER(r.scheme) IN ('RFBS','FBP','WHD','FBS','FBO')) OR (?4=0 AND UPPER(r.scheme) IN ('FBO','FBS')))
           AND (?3='%%' OR r.posting_number LIKE ?3 OR r.sku LIKE ?3 OR r.offer_id LIKE ?3 OR r.product_name LIKE ?3 OR r.status LIKE ?3)
         ORDER BY r.day DESC,r.updated_at DESC LIMIT 500"
    ).map_err(|e| e.to_string())?;
    let raw = stmt
        .query_map(
            params![
                range.from,
                range.to,
                needle,
                if cross_border { 1 } else { 0 }
            ],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, String>(7)?,
                    r.get::<_, f64>(8)?,
                    r.get::<_, String>(9)?,
                    r.get::<_, String>(10)?,
                    r.get::<_, String>(11)?,
                    r.get::<_, String>(12)?,
                    r.get::<_, String>(13)?,
                ))
            },
        )
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut fee_cache = std::collections::HashMap::<String, (Option<f64>, String)>::new();
    let rows = raw
        .into_iter()
        .map(
            |(
                event_id,
                posting_number,
                sku,
                offer_id,
                product_name,
                quantity,
                scheme,
                status,
                amount,
                created_at,
                updated_at,
                origin,
                destination,
                image_url,
            )| {
                let cache_key =
                    format!("{sku}\u{1f}{origin}\u{1f}{destination}\u{1f}{:.2}", amount);
                let (fee, basis) = fee_cache
                    .entry(cache_key)
                    .or_insert_with(|| estimate_delivery(&c, &sku, &origin, &destination, amount))
                    .clone();
                let display_scheme = if cross_border {
                    match scheme.to_uppercase().as_str() {
                        "FBS" => "RFBS".to_string(),
                        "FBO" => "FBP".to_string(),
                        _ => scheme,
                    }
                } else {
                    scheme
                };
                OrderRow {
                    event_id,
                    posting_number,
                    sku,
                    supplier_url: listing::supplier_link_for_offer(&c, &offer_id),
                    offer_id,
                    product_name,
                    quantity,
                    scheme: display_scheme,
                    status,
                    amount: display_amount(amount, cross_border, rate),
                    created_at,
                    updated_at,
                    origin,
                    destination,
                    estimated_delivery: fee.map(|value| display_amount(value, cross_border, rate)),
                    estimate_basis: basis,
                    image_url,
                }
            },
        )
        .collect();
    Ok(rows)
}

#[tauri::command]
fn advertising(range: DateRange, state: State<AppState>) -> Result<AdvertisingData, String> {
    let c = db(&state)?;
    let cross_border = active_shop_kind(&state)? == "cross_border";
    let rate = rub_per_cny_for(&state, &c)?;
    let (impressions,clicks,cart_adds,orders,revenue,spend):(i64,i64,i64,i64,f64,f64)=c.query_row("WITH x AS(SELECT * FROM ad_daily WHERE day BETWEEN ?1 AND ?2),m AS(SELECT EXISTS(SELECT 1 FROM x WHERE sku='') store)SELECT COALESCE(SUM(impressions),0),COALESCE(SUM(clicks),0),COALESCE(SUM(cart_adds),0),COALESCE(SUM(orders),0),COALESCE(SUM(revenue),0),COALESCE(SUM(spend),0)FROM x,m WHERE (m.store=1 AND x.sku='')OR(m.store=0 AND x.sku<>'')",params![range.from,range.to],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).map_err(|e|e.to_string())?;
    let to_date = chrono::NaiveDate::parse_from_str(&range.to, "%Y-%m-%d")
        .unwrap_or_else(|_| chrono::Local::now().date_naive());
    let active_from = to_date
        .checked_sub_months(chrono::Months::new(2))
        .unwrap_or(to_date)
        .format("%Y-%m-%d")
        .to_string();
    let (costed_revenue,total_revenue,known_cost):(f64,f64,f64)=c.query_row("SELECT COALESCE(SUM(CASE WHEN pc.sku IS NOT NULL THEN s.revenue ELSE 0 END),0),COALESCE(SUM(s.revenue),0),COALESCE(SUM(CASE WHEN pc.sku IS NOT NULL THEN s.ordered_units*(COALESCE(pc.unit_cost,pc.unit_cost_cny*?3,0)+COALESCE(pc.first_mile_cost,pc.first_mile_cost_cny*?3,0)) ELSE 0 END),0) FROM sales_daily s LEFT JOIN product_costs pc ON pc.sku=s.sku WHERE s.day BETWEEN ?1 AND ?2",params![range.from,range.to,rate],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e|e.to_string())?;
    let margin_coverage_percent = if total_revenue > 0.0 {
        costed_revenue / total_revenue * 100.0
    } else {
        0.0
    };
    let known_cost_margin = (costed_revenue > 0.0 && margin_coverage_percent >= 80.0)
        .then_some(((costed_revenue - known_cost) / costed_revenue).clamp(0.0, 1.0));
    let break_even_roas = known_cost_margin.filter(|m| *m > 0.0).map(|m| 1.0 / m);
    let target_roas = break_even_roas.map(|v| v * 1.5);
    let average_order_value = (orders > 0).then_some(revenue / orders as f64);
    let max_cpa = average_order_value
        .zip(known_cost_margin)
        .map(|(aov, margin)| aov * margin);
    let mut stmt=c.prepare("WITH x AS(SELECT * FROM ad_daily WHERE day BETWEEN ?1 AND ?2),m AS(SELECT EXISTS(SELECT 1 FROM x WHERE sku='') store)SELECT a.campaign_id,COALESCE(NULLIF(MAX(a.campaign_name),''),MAX(c.name),a.campaign_id),COALESCE(MAX(c.state),''),COALESCE(MAX(c.payment_type),''),SUM(a.impressions),SUM(a.clicks),SUM(a.orders),SUM(a.spend),SUM(a.revenue),COALESCE(MAX(c.budget),0) FROM x a CROSS JOIN m LEFT JOIN campaigns c ON c.campaign_id=a.campaign_id WHERE (m.store=1 AND a.sku='')OR(m.store=0 AND a.sku<>'')GROUP BY a.campaign_id HAVING SUM(a.spend)>0 OR SUM(a.impressions)>0 ORDER BY SUM(a.spend)DESC").map_err(|e|e.to_string())?;
    let campaigns = stmt
        .query_map(params![active_from, range.to], |r| {
            let spend: f64 = r.get(7)?;
            let revenue: f64 = r.get(8)?;
            let impressions: i64 = r.get(4)?;
            let clicks: i64 = r.get(5)?;
            let campaign_orders: i64 = r.get(6)?;
            let campaign_roas = (spend > 0.0).then_some(revenue / spend);
            let campaign_acos = (revenue > 0.0).then_some(spend / revenue * 100.0);
            let (diagnosis_level, diagnosis_text, recommended_action) = if impressions == 0 {
                (
                    "pending",
                    "无曝光，计划尚未形成有效投放",
                    "检查计划状态、商品审核、预算和投放日期",
                )
            } else if clicks == 0 {
                (
                    "critical",
                    "有曝光但没有点击",
                    "检查主图、标题、价格和活动竞争力",
                )
            } else if clicks >= 20 && campaign_orders == 0 {
                (
                    "critical",
                    "已有点击但没有归因订单",
                    "暂停扩量，检查商品页转化、价格、评价与库存",
                )
            } else if clicks as f64 / impressions as f64 * 100.0 < 1.0 {
                (
                    "warning",
                    "点击率低于 1%",
                    "优先优化素材、商品卡和投放商品组合",
                )
            } else if campaign_orders > 0 && campaign_orders as f64 / clicks as f64 * 100.0 < 2.0 {
                (
                    "warning",
                    "点击到订单转化率低于 2%",
                    "检查落地商品、售价、配送时效和评论质量",
                )
            } else if break_even_roas
                .zip(campaign_roas)
                .is_some_and(|(be, actual)| actual < be)
            {
                (
                    "critical",
                    "ROAS 低于已知成本盈亏线",
                    "降低预算或暂停低效商品，先修复转化",
                )
            } else if target_roas
                .zip(campaign_roas)
                .is_some_and(|(target, actual)| actual >= target)
            {
                (
                    "good",
                    "ROAS 达到可持续目标",
                    "在库存和边际利润允许时分阶段增加 20% 预算",
                )
            } else {
                (
                    "observe",
                    "计划处于观察区间",
                    "保持预算，累计至少 7 天数据后再调整",
                )
            };
            Ok(CampaignRow {
                id: r.get(0)?,
                name: r.get(1)?,
                state: r.get(2)?,
                payment_type: r.get(3)?,
                impressions,
                clicks,
                orders: campaign_orders,
                spend: display_amount(spend, cross_border, rate),
                revenue: display_amount(revenue, cross_border, rate),
                roas: campaign_roas,
                ctr: (impressions > 0).then_some(clicks as f64 / impressions as f64 * 100.0),
                cpc: (clicks > 0).then_some(display_amount(
                    spend / clicks as f64,
                    cross_border,
                    rate,
                )),
                conversion_rate: (clicks > 0)
                    .then_some(campaign_orders as f64 / clicks as f64 * 100.0),
                cpa: (campaign_orders > 0).then_some(display_amount(
                    spend / campaign_orders as f64,
                    cross_border,
                    rate,
                )),
                acos: campaign_acos,
                diagnosis_level: diagnosis_level.into(),
                diagnosis_text: diagnosis_text.into(),
                recommended_action: recommended_action.into(),
                // Campaign budgets are editable platform configuration and
                // remain in Ozon's native RUB unit. Read-only performance
                // amounts above follow the shop display currency.
                budget: r.get(9)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut trend_stmt=c.prepare("WITH x AS(SELECT * FROM ad_daily WHERE day BETWEEN ?1 AND ?2),m AS(SELECT EXISTS(SELECT 1 FROM x WHERE sku='') store)SELECT a.day,SUM(a.impressions),SUM(a.clicks),SUM(a.orders),SUM(a.spend),SUM(a.revenue)FROM x a CROSS JOIN m WHERE (m.store=1 AND a.sku='')OR(m.store=0 AND a.sku<>'')GROUP BY a.day ORDER BY a.day").map_err(|e|e.to_string())?;
    let mut trend = trend_stmt
        .query_map(params![range.from, range.to], |r| {
            Ok(AdvertisingTrendRow {
                day: r.get(0)?,
                impressions: r.get(1)?,
                clicks: r.get(2)?,
                orders: r.get(3)?,
                spend: r.get(4)?,
                revenue: r.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    if cross_border {
        for row in &mut trend {
            row.spend = display_amount(row.spend, true, rate);
            row.revenue = display_amount(row.revenue, true, rate);
        }
    }
    Ok(AdvertisingData {
        impressions,
        clicks,
        cart_adds,
        orders,
        revenue: display_amount(revenue, cross_border, rate),
        spend: display_amount(spend, cross_border, rate),
        ctr: if impressions > 0 {
            Some(clicks as f64 / impressions as f64 * 100.0)
        } else {
            None
        },
        cpc: if clicks > 0 {
            Some(display_amount(spend / clicks as f64, cross_border, rate))
        } else {
            None
        },
        roas: if spend > 0.0 {
            Some(revenue / spend)
        } else {
            Some(0.0)
        },
        conversion_rate: (clicks > 0).then_some(orders as f64 / clicks as f64 * 100.0),
        cpa: (orders > 0).then_some(display_amount(spend / orders as f64, cross_border, rate)),
        acos: (revenue > 0.0).then_some(spend / revenue * 100.0),
        break_even_roas,
        target_roas,
        max_cpa: max_cpa.map(|value| display_amount(value, cross_border, rate)),
        known_cost_margin,
        margin_coverage_percent,
        campaigns,
        trend,
    })
}

fn campaign_metrics(
    c: &Connection,
    campaign_id: &str,
    from: &str,
    to: &str,
) -> Result<(f64, f64), String> {
    c.query_row("WITH x AS(SELECT * FROM ad_daily WHERE campaign_id=?1 AND day BETWEEN ?2 AND ?3),m AS(SELECT EXISTS(SELECT 1 FROM x WHERE sku='') store)SELECT COALESCE(SUM(spend),0),COALESCE(SUM(revenue),0)FROM x,m WHERE (m.store=1 AND sku='')OR(m.store=0 AND sku<>'')",params![campaign_id,from,to],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|e|e.to_string())
}

fn campaign_monitor_data(
    campaign_id: String,
    state: &AppState,
) -> Result<CampaignMonitorData, String> {
    let c = db(state)?;
    let (name, mut current_state, mut budget, mut budget_known, budget_fresh): (String, String, f64, bool, bool) = c
        .query_row(
            "SELECT name,state,budget,budget_known=1,budget_known=1 AND datetime(budget_updated_at)>=datetime('now','-15 minutes') FROM campaigns WHERE campaign_id=?1",
            [&campaign_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap_or_else(|_| (campaign_id.clone(), String::new(), 0.0, false, false));
    let mut budget_source = if budget_known {
        "本地缓存"
    } else {
        "尚未获取"
    }
    .to_string();
    if !budget_fresh {
        if let Ok(token) = performance_token(&c) {
            if let Ok(payload) = performance_get(
                &format!("/api/client/campaign?campaignIds={campaign_id}"),
                &token,
            ) {
                if let Some(item) = payload
                    .get("list")
                    .and_then(|v| v.as_array())
                    .and_then(|v| v.first())
                {
                    if let Some(value) = performance_budget_rub(
                        item.get("weeklyBudget").or_else(|| item.get("budget")),
                    ) {
                        budget = value;
                        budget_known = true;
                        budget_source = "Performance API 实时值".to_string();
                    }
                    let api_state = json_text(item.get("state"));
                    if !api_state.is_empty() {
                        current_state = api_state;
                    }
                    let _ = c.execute(
                    "UPDATE campaigns SET state=?1,budget=CASE WHEN ?2 THEN ?3 ELSE budget END,budget_known=CASE WHEN ?2 THEN 1 ELSE budget_known END,budget_updated_at=CASE WHEN ?2 THEN CURRENT_TIMESTAMP ELSE budget_updated_at END,budget_scale_version=1,updated_at=CURRENT_TIMESTAMP WHERE campaign_id=?4",
                    params![current_state, budget_known, budget, campaign_id],
                );
                }
            }
        }
    } else {
        budget_source = "本地缓存（15 分钟内）".to_string();
    }
    let today = chrono::Local::now().date_naive();
    let from = today
        .checked_sub_months(chrono::Months::new(2))
        .unwrap_or(today)
        .format("%Y-%m-%d")
        .to_string();
    let to = today.format("%Y-%m-%d").to_string();
    let mut daily_stmt=c.prepare("WITH x AS(SELECT * FROM ad_daily WHERE campaign_id=?1 AND day BETWEEN ?2 AND ?3),m AS(SELECT EXISTS(SELECT 1 FROM x WHERE sku='') store)SELECT day,SUM(impressions),SUM(clicks),SUM(orders),SUM(spend),SUM(revenue)FROM x,m WHERE (m.store=1 AND sku='')OR(m.store=0 AND sku<>'')GROUP BY day ORDER BY day").map_err(|e|e.to_string())?;
    let daily = daily_stmt
        .query_map(params![campaign_id, from, to], |r| {
            Ok(AdvertisingTrendRow {
                day: r.get(0)?,
                impressions: r.get(1)?,
                clicks: r.get(2)?,
                orders: r.get(3)?,
                spend: r.get(4)?,
                revenue: r.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut log_stmt=c.prepare("SELECT id,action,requested_value,before_state,before_budget,after_state,after_budget,status,message,created_at,before_spend,before_revenue FROM campaign_action_logs WHERE campaign_id=?1 ORDER BY id DESC LIMIT 200").map_err(|e|e.to_string())?;
    let base = log_stmt
        .query_map([&campaign_id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, f64>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, f64>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, String>(8)?,
                r.get::<_, String>(9)?,
                r.get::<_, f64>(10)?,
                r.get::<_, f64>(11)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut logs = Vec::new();
    for row in base {
        let after_from = row.9.chars().take(10).collect::<String>();
        let (after_spend, after_revenue) = campaign_metrics(&c, &campaign_id, &after_from, &to)?;
        logs.push(CampaignActionLogRow {
            id: row.0,
            action: row.1,
            requested_value: row.2,
            before_state: row.3,
            before_budget: row.4,
            after_state: row.5,
            after_budget: row.6,
            status: row.7,
            message: row.8,
            created_at: row.9,
            before_spend: row.10,
            before_revenue: row.11,
            after_spend,
            after_revenue,
        });
    }
    Ok(CampaignMonitorData {
        id: campaign_id,
        name,
        state: current_state,
        budget,
        budget_source,
        budget_known,
        daily,
        logs,
    })
}

#[tauri::command]
async fn campaign_monitor(
    campaign_id: String,
    state: State<'_, AppState>,
) -> Result<CampaignMonitorData, String> {
    let owned = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || campaign_monitor_data(campaign_id, &owned))
        .await
        .map_err(|e| e.to_string())?
}

fn campaign_control_blocking(
    input: CampaignControlInput,
    state: &AppState,
) -> Result<String, String> {
    if input.confirmation.trim() != "确认执行" {
        return Err("请输入“确认执行”后再提交广告写操作".into());
    }
    let c = db(state)?;
    let (before_state, before_budget): (String, f64) = c
        .query_row(
            "SELECT state,budget FROM campaigns WHERE campaign_id=?1",
            [&input.campaign_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| "广告活动不存在，请先同步 Performance 广告".to_string())?;
    let today = chrono::Local::now().date_naive();
    let before_to = today.pred_opt().unwrap_or(today);
    let before_from = before_to - chrono::Duration::days(6);
    let before_from_text = before_from.format("%Y-%m-%d").to_string();
    let before_to_text = before_to.format("%Y-%m-%d").to_string();
    let (before_spend, before_revenue) =
        campaign_metrics(&c, &input.campaign_id, &before_from_text, &before_to_text)?;
    c.execute("INSERT INTO campaign_action_logs(campaign_id,action,requested_value,before_state,before_budget,before_from,before_to,before_spend,before_revenue,status)VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'running')",params![input.campaign_id,input.action,input.weekly_budget.map(|v|v.to_string()).unwrap_or_default(),before_state,before_budget,before_from_text,before_to_text,before_spend,before_revenue]).map_err(|e|e.to_string())?;
    let log_id = c.last_insert_rowid();
    let result = (|| -> Result<(String, f64, String), String> {
        let token = performance_token(&c)?;
        match input.action.as_str() {
            "activate" => {
                performance_mutation(
                    "POST",
                    &format!("/api/client/campaign/{}/activate", input.campaign_id),
                    &token,
                    &serde_json::json!({}),
                )?;
            }
            "deactivate" => {
                performance_mutation(
                    "POST",
                    &format!("/api/client/campaign/{}/deactivate", input.campaign_id),
                    &token,
                    &serde_json::json!({}),
                )?;
            }
            "budget" => {
                let value = input
                    .weekly_budget
                    .filter(|v| *v > 0.0)
                    .ok_or("周预算必须大于 0")?;
                performance_mutation(
                    "PATCH",
                    &format!("/api/client/campaign/{}", input.campaign_id),
                    &token,
                    &serde_json::json!({"weeklyBudget":(value * 1_000_000.0).round() as i64}),
                )?;
            }
            _ => return Err("不支持的广告操作".into()),
        }
        let payload = performance_get(
            &format!("/api/client/campaign?campaignIds={}", input.campaign_id),
            &token,
        )?;
        let item = payload
            .get("list")
            .and_then(|v| v.as_array())
            .and_then(|v| v.first())
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        let state_value = json_text(item.get("state"));
        let budget_value = item
            .get("weeklyBudget")
            .or_else(|| item.get("budget"))
            .and_then(|v| performance_budget_rub(Some(v)))
            .unwrap_or_else(|| input.weekly_budget.unwrap_or(before_budget));
        Ok((
            state_value,
            budget_value,
            "Performance API 已执行并完成活动状态回读".to_string(),
        ))
    })();
    match result {
        Ok((after_state, after_budget, message)) => {
            c.execute("UPDATE campaigns SET state=?1,budget=?2,budget_known=1,budget_updated_at=CURRENT_TIMESTAMP,budget_scale_version=1,updated_at=CURRENT_TIMESTAMP WHERE campaign_id=?3",params![after_state,after_budget,input.campaign_id]).map_err(|e|e.to_string())?;
            c.execute("UPDATE campaign_action_logs SET after_state=?1,after_budget=?2,status='success',message=?3 WHERE id=?4",params![after_state,after_budget,message,log_id]).map_err(|e|e.to_string())?;
            Ok(message)
        }
        Err(error) => {
            let _=c.execute("UPDATE campaign_action_logs SET after_state=before_state,after_budget=before_budget,status='failed',message=?1 WHERE id=?2",params![error,log_id]);
            Err(error)
        }
    }
}

#[tauri::command]
async fn campaign_control(
    input: CampaignControlInput,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let owned = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || campaign_control_blocking(input, &owned))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn campaign_ai_analysis(
    campaign_id: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let owned = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move||{
        let data=campaign_monitor_data(campaign_id,&owned)?;let c=db(&owned)?;let base=setting(&c,"ai_base_url");let model=setting(&c,"ai_model");let key=secret_setting(&c,"ai_api_key")?;
        if base.is_empty()||model.is_empty()||key.is_empty(){return Err("请先在连接设置中配置 AI Base URL、模型和 API Key".into())}
        let context=serde_json::to_string(&data.daily).map_err(|e|e.to_string())?;let logs=serde_json::to_string(&data.logs).map_err(|e|e.to_string())?;
        let prompt=format!("广告活动 {}（{}），当前状态 {}，周预算 {}。近两个月逐日数据：{}。预算/开关操作及效果日志：{}。分析调整后的真实变化，区分样本不足与可确认结论，并给出是否启停、预算调整幅度及观察周期建议。",data.name,data.id,data.state,data.budget,context,logs);
        let body=serde_json::json!({"model":model,"messages":[{"role":"system","content":"你是 Ozon 广告分析助手。只能依据提供的数据分析；不得声称已执行任何操作；明确区分事实、推断和建议。输出中文。"},{"role":"user","content":prompt}]});
        let response=ureq::post(&chat_endpoint(&base)).set("Authorization",&format!("Bearer {key}")).set("Content-Type","application/json").send_string(&body.to_string()).map_err(|e|format!("AI 请求失败：{e}"))?;
        let raw=response.into_string().map_err(|e|e.to_string())?;let payload:serde_json::Value=serde_json::from_str(&raw).map_err(|e|format!("AI 返回不是有效 JSON：{e}"))?;
        payload.pointer("/choices/0/message/content").and_then(|v|v.as_str()).map(str::to_string).ok_or_else(||"AI 返回缺少正文".into())
    }).await.map_err(|e|e.to_string())?
}

#[tauri::command]
fn products(
    range: DateRange,
    query: String,
    state: State<AppState>,
) -> Result<Vec<ProductRow>, String> {
    let c = db(&state)?;
    let cross_border = active_shop_kind(&state)? == "cross_border";
    let rate = rub_per_cny_for(&state, &c)?;
    let needle = format!("%{}%", query.trim());
    let mut stmt = c.prepare("SELECT p.sku,p.offer_id,p.product_id,p.name,
        COALESCE(SUM(s.revenue),0),COALESCE(SUM(s.ordered_units),0),COALESCE(SUM(s.delivered_units),0),
        CASE WHEN EXISTS(SELECT 1 FROM return_events re WHERE re.day BETWEEN ?1 AND ?2) THEN COALESCE((SELECT SUM(re.quantity) FROM return_events re WHERE re.sku=p.sku AND re.day BETWEEN ?1 AND ?2),0) ELSE COALESCE(SUM(s.returns),0) END,CASE WHEN EXISTS(SELECT 1 FROM cancellation_events ce WHERE ce.day BETWEEN ?1 AND ?2) THEN COALESCE((SELECT SUM(ce.quantity) FROM cancellation_events ce WHERE ce.sku=p.sku AND ce.day BETWEEN ?1 AND ?2),0) ELSE COALESCE(SUM(s.cancellations),0) END,
        COALESCE(pc.unit_cost_cny,pc.unit_cost),COALESCE(pc.first_mile_cost_cny,pc.first_mile_cost),pc.length_cm,pc.width_cm,pc.height_cm,pc.weight_kg,COALESCE(pc.note,''),p.updated_at
        FROM products p LEFT JOIN sales_daily s ON s.sku=p.sku AND s.day BETWEEN ?1 AND ?2
        LEFT JOIN product_costs pc ON pc.sku=p.sku
        WHERE (?3='%%' OR p.sku LIKE ?3 OR p.offer_id LIKE ?3 OR p.name LIKE ?3)
        GROUP BY p.sku ORDER BY COALESCE(SUM(s.revenue),0) DESC,p.offer_id LIMIT 2000").map_err(|e|e.to_string())?;
    let rows = stmt
        .query_map(params![range.from, range.to, needle], |r| {
            Ok(ProductRow {
                sku: r.get(0)?,
                offer_id: r.get(1)?,
                product_id: r.get(2)?,
                name: r.get(3)?,
                revenue: {
                    let value: f64 = r.get(4)?;
                    if cross_border {
                        value / rate
                    } else {
                        value
                    }
                },
                ordered_units: r.get(5)?,
                delivered_units: r.get(6)?,
                returns: r.get(7)?,
                cancellations: r.get(8)?,
                unit_cost: r.get(9)?,
                first_mile_cost: r.get(10)?,
                length_cm: r.get(11)?,
                width_cm: r.get(12)?,
                height_cm: r.get(13)?,
                weight_kg: r.get(14)?,
                note: r.get(15)?,
                updated_at: r.get(16)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

fn upsert_cost(c: &Connection, input: &ProductCostInput) -> Result<(), String> {
    c.execute("INSERT INTO product_costs(sku,unit_cost_cny,first_mile_cost,length_cm,width_cm,height_cm,weight_kg,note,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,CURRENT_TIMESTAMP) ON CONFLICT(sku) DO UPDATE SET unit_cost_cny=excluded.unit_cost_cny,first_mile_cost=excluded.first_mile_cost,length_cm=excluded.length_cm,width_cm=excluded.width_cm,height_cm=excluded.height_cm,weight_kg=excluded.weight_kg,note=excluded.note,updated_at=CURRENT_TIMESTAMP",params![input.sku,input.unit_cost,input.first_mile_cost,input.length_cm,input.width_cm,input.height_cm,input.weight_kg,input.note]).map_err(|e|e.to_string())?;
    Ok(())
}

#[tauri::command]
fn save_product_cost(input: ProductCostInput, state: State<AppState>) -> Result<(), String> {
    let c = db(&state)?;
    upsert_cost(&c, &input)
}

#[tauri::command]
fn match_product_costs(
    input: ProductCostInput,
    pattern: String,
    state: State<AppState>,
) -> Result<i64, String> {
    let mut c = db(&state)?;
    let needle = format!("%{}%", pattern.trim());
    let tx = c.transaction().map_err(|e| e.to_string())?;
    let skus = {
        let mut stmt = tx
            .prepare("SELECT sku FROM products WHERE sku LIKE ?1 OR offer_id LIKE ?1")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([needle], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        rows
    };
    let mut count = 0;
    for sku in skus {
        let matched = ProductCostInput {
            sku,
            unit_cost: input.unit_cost,
            first_mile_cost: input.first_mile_cost,
            length_cm: input.length_cm,
            width_cm: input.width_cm,
            height_cm: input.height_cm,
            weight_kg: input.weight_kg,
            note: input.note.clone(),
        };
        upsert_cost(&tx, &matched)?;
        count += 1;
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(count)
}

fn csv_cell(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}
#[tauri::command]
fn export_product_costs(state: State<AppState>) -> Result<String, String> {
    let c = db(&state)?;
    let folder = state
        .data_dir
        .parent()
        .unwrap_or(&state.data_dir)
        .join("exports");
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let path = folder.join(format!(
        "product_costs_{}.csv",
        chrono::Local::now().format("%Y%m%d_%H%M%S")
    ));
    let mut text=String::from("\u{feff}SKU,货号,商品名称,采购成本_CNY,头程_RUB,长度_cm,宽度_cm,高度_cm,重量_kg,备注,更新时间\r\n");
    let mut stmt=c.prepare("SELECT p.sku,p.offer_id,p.name,pc.unit_cost_cny,pc.first_mile_cost,pc.length_cm,pc.width_cm,pc.height_cm,pc.weight_kg,COALESCE(pc.note,''),COALESCE(pc.updated_at,'') FROM products p LEFT JOIN product_costs pc ON pc.sku=p.sku ORDER BY p.offer_id,p.sku").map_err(|e|e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<f64>>(3)?,
                r.get::<_, Option<f64>>(4)?,
                r.get::<_, Option<f64>>(5)?,
                r.get::<_, Option<f64>>(6)?,
                r.get::<_, Option<f64>>(7)?,
                r.get::<_, Option<f64>>(8)?,
                r.get::<_, String>(9)?,
                r.get::<_, String>(10)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    for row in rows {
        let (sku, offer, name, a, b, l, w, h, kg, note, updated) =
            row.map_err(|e| e.to_string())?;
        let num = |x: Option<f64>| x.map(|v| v.to_string()).unwrap_or_default();
        text.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{}\r\n",
            csv_cell(&sku),
            csv_cell(&offer),
            csv_cell(&name),
            num(a),
            num(b),
            num(l),
            num(w),
            num(h),
            num(kg),
            csv_cell(&note),
            csv_cell(&updated)
        ));
    }
    fs::write(&path, text.as_bytes()).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

fn sqlite_cell(value: rusqlite::types::ValueRef<'_>) -> String {
    match value {
        rusqlite::types::ValueRef::Null => String::new(),
        rusqlite::types::ValueRef::Integer(v) => v.to_string(),
        rusqlite::types::ValueRef::Real(v) => v.to_string(),
        rusqlite::types::ValueRef::Text(v) => String::from_utf8_lossy(v).into_owned(),
        rusqlite::types::ValueRef::Blob(v) => {
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, v)
        }
    }
}

#[tauri::command]
fn export_dataset(
    kind: String,
    range: DateRange,
    state: State<AppState>,
) -> Result<String, String> {
    let (name,headers,sql,ranged,wb)=match kind.as_str(){
        "sales"=>("sales",vec!["日期","SKU","商品名称","销售额","销量","妥投","退货","取消","浏览","加购","来源","更新时间"],"SELECT day,sku,product_name,revenue,ordered_units,delivered_units,returns,cancellations,views,cart_adds,source,updated_at FROM sales_daily WHERE day BETWEEN ?1 AND ?2 ORDER BY day,sku",true,false),
        "orders"=>("orders",vec!["事件ID","订单号","SKU","货号","商品名称","数量","履约","状态","金额","下单时间","更新时间","运输集群","配送集群"],"SELECT event_id,posting_number,sku,offer_id,product_name,quantity,scheme,status,order_price,created_at,updated_at,origin,destination FROM posting_routes WHERE day BETWEEN ?1 AND ?2 ORDER BY day,posting_number",true,false),
        "advertising"=>("advertising",vec!["日期","活动ID","活动名称","SKU","曝光","点击","加购","订单","广告销售额","花费","来源","更新时间"],"SELECT day,campaign_id,campaign_name,sku,impressions,clicks,cart_adds,orders,revenue,spend,source,updated_at FROM ad_daily WHERE day BETWEEN ?1 AND ?2 ORDER BY day,campaign_id,sku",true,false),
        "inventory"=>("inventory",vec!["SKU","货号","商品名称","仓库ID","仓库名","集群ID","集群名","可售","在途","待供货","更新时间"],"SELECT sku,offer_id,product_name,warehouse_id,warehouse_name,macrolocal_cluster_id,cluster_name,available_stock,transit_stock,requested_stock,updated_at FROM inventory_stock ORDER BY sku,warehouse_name",false,false),
        "finance"=>("finance",vec!["操作ID","操作日期","类型","订单号","SKU","金额","配送费","退货配送费","销售应计","佣金","原始JSON"],"SELECT operation_id,operation_date,operation_type,posting_number,sku,amount,delivery_charge,return_delivery_charge,accruals_for_sale,sale_commission,raw_json FROM finance_transactions WHERE substr(operation_date,1,10) BETWEEN ?1 AND ?2 ORDER BY operation_date",true,false),
        "costs"=>("product_costs",vec!["SKU","货号","商品名称","采购成本_CNY","头程_RUB","长度_cm","宽度_cm","高度_cm","重量_kg","备注","更新时间"],"WITH known AS(SELECT sku FROM products UNION SELECT sku FROM product_costs)SELECT k.sku,COALESCE(p.offer_id,''),COALESCE(p.name,''),pc.unit_cost_cny,pc.first_mile_cost,pc.length_cm,pc.width_cm,pc.height_cm,pc.weight_kg,COALESCE(pc.note,''),COALESCE(pc.updated_at,'')FROM known k LEFT JOIN products p ON p.sku=k.sku LEFT JOIN product_costs pc ON pc.sku=k.sku ORDER BY p.offer_id,k.sku",false,false),
        "products"=>("products_portable",vec!["SKU","货号","Ozon商品ID","商品名称","图片URL","采购成本_CNY","头程_RUB","长度_cm","宽度_cm","高度_cm","重量_kg","备注","更新时间"],"WITH known AS(SELECT sku FROM products UNION SELECT sku FROM product_costs)SELECT k.sku,COALESCE(p.offer_id,''),COALESCE(p.product_id,''),COALESCE(p.name,''),COALESCE(p.image_url,''),pc.unit_cost_cny,pc.first_mile_cost,pc.length_cm,pc.width_cm,pc.height_cm,pc.weight_kg,COALESCE(pc.note,''),COALESCE(pc.updated_at,p.updated_at,'')FROM known k LEFT JOIN products p ON p.sku=k.sku LEFT JOIN product_costs pc ON pc.sku=k.sku ORDER BY p.offer_id,k.sku",false,false),
        "warehouses"=>("warehouse_clusters",vec!["仓库名称","所属集群","更新时间"],"SELECT warehouse_name,cluster_name,updated_at FROM warehouse_cluster_mappings ORDER BY warehouse_name",false,false),
        "shipments"=>("shipments",vec!["跟踪单号","品名","批次","店铺","数量","状态","渠道","国内到库","国外到库","已通知日期","来源","远程记录ID","更新时间"],"SELECT tracking_id,product_name,batch_no,shop_name,quantity,cargo_status,channel,domestic_arrival,foreign_arrival,notified_foreign_arrival,source,remote_record_id,updated_at FROM shipment_tracking ORDER BY updated_at",false,false),
        "competitors"=>("competitors",vec!["竞品ID","链接","商品编码","名称","图片","采集时间","售价","累计销量","来源"],"SELECT p.id,p.product_url,p.product_code,p.name,p.image_url,s.captured_at,s.price,s.sales_total,s.source FROM competitor_products p LEFT JOIN competitor_snapshots s ON s.competitor_id=p.id ORDER BY p.id,s.captured_at",false,false),
        "wb_costs"=>("wb_product_costs",vec!["nmId","货号","采购成本_CNY","长度_cm","宽度_cm","高度_cm","重量_kg","仓库模式"],"SELECT nm_id,article,purchase_cost_cny,length_cm,width_cm,height_cm,weight_kg,warehouse_mode FROM product_costs ORDER BY article,nm_id",false,true),
        _=>return Err("未知导出数据类型".into())};
    let connection = if wb {
        Connection::open(state.data_dir.join("wb").join("wb_analytics.db"))
            .map_err(|e| e.to_string())?
    } else {
        db(&state)?
    };
    let folder = state
        .data_dir
        .parent()
        .unwrap_or(&state.data_dir)
        .join("exports");
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let path = folder.join(format!(
        "{}_{}.csv",
        name,
        chrono::Local::now().format("%Y%m%d_%H%M%S")
    ));
    let mut output = String::from("\u{feff}");
    output.push_str(
        &headers
            .iter()
            .map(|v| csv_cell(v))
            .collect::<Vec<_>>()
            .join(","),
    );
    output.push_str("\r\n");
    let mut stmt = connection.prepare(sql).map_err(|e| e.to_string())?;
    let column_count = stmt.column_count();
    let mut rows = if ranged {
        stmt.query(params![range.from, range.to])
            .map_err(|e| e.to_string())?
    } else {
        stmt.query([]).map_err(|e| e.to_string())?
    };
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let mut values = Vec::with_capacity(column_count);
        for index in 0..column_count {
            values.push(csv_cell(&sqlite_cell(
                row.get_ref(index).map_err(|e| e.to_string())?,
            )));
        }
        output.push_str(&values.join(","));
        output.push_str("\r\n");
    }
    fs::write(&path, output.as_bytes()).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

fn parse_csv_line(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut value = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '"' {
            if quoted && chars.peek() == Some(&'"') {
                value.push('"');
                chars.next();
            } else {
                quoted = !quoted
            }
        } else if ch == ',' && !quoted {
            out.push(value);
            value = String::new()
        } else {
            value.push(ch)
        }
    }
    out.push(value);
    out
}

#[tauri::command]
fn import_product_costs_csv(path: String, state: State<AppState>) -> Result<i64, String> {
    let text =
        fs::read_to_string(PathBuf::from(path.trim())).map_err(|e| format!("无法读取 CSV：{e}"))?;
    let mut lines = text.lines();
    let headers = parse_csv_line(
        lines
            .next()
            .ok_or("CSV 文件为空")?
            .trim_start_matches('\u{feff}'),
    );
    let index = |names: &[&str]| {
        headers
            .iter()
            .position(|h| names.iter().any(|n| h.trim() == *n))
    };
    let sku_i = index(&["SKU", "sku"]).ok_or("CSV 缺少 SKU 列")?;
    let offer_i = index(&["货号", "offer_id"]);
    let product_id_i = index(&["Ozon商品ID", "product_id"]);
    let name_i = index(&["商品名称", "name"]);
    let image_i = index(&["图片URL", "image_url"]);
    let idx = [
        index(&["采购成本_CNY", "采购成本 CNY", "unit_cost_cny"]),
        index(&["头程_RUB", "头程 RUB", "first_mile_cost"]),
        index(&["长度_cm", "长度 (cm)", "length_cm"]),
        index(&["宽度_cm", "宽度 (cm)", "width_cm"]),
        index(&["高度_cm", "高度 (cm)", "height_cm"]),
        index(&["重量_kg", "重量 (kg)", "weight_kg"]),
        index(&["备注", "note"]),
    ];
    let mut c = db(&state)?;
    let tx = c.transaction().map_err(|e| e.to_string())?;
    let mut count = 0;
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let row = parse_csv_line(line);
        let sku = row.get(sku_i).map(|v| v.trim()).unwrap_or_default();
        if sku.is_empty() {
            continue;
        }
        let num = |i: Option<usize>| {
            i.and_then(|x| row.get(x))
                .map(|v| v.trim())
                .filter(|v| !v.is_empty())
                .and_then(|v| v.parse::<f64>().ok())
        };
        let note = idx[6].and_then(|x| row.get(x)).cloned().unwrap_or_default();
        let input = ProductCostInput {
            sku: sku.into(),
            unit_cost: num(idx[0]),
            first_mile_cost: num(idx[1]),
            length_cm: num(idx[2]),
            width_cm: num(idx[3]),
            height_cm: num(idx[4]),
            weight_kg: num(idx[5]),
            note,
        };
        let text_at = |i: Option<usize>| {
            i.and_then(|x| row.get(x))
                .map(|v| v.trim())
                .unwrap_or_default()
        };
        if offer_i.is_some() || product_id_i.is_some() || name_i.is_some() || image_i.is_some() {
            tx.execute("INSERT INTO products(sku,offer_id,product_id,name,image_url,source)VALUES(?1,?2,?3,?4,?5,'import')ON CONFLICT(sku)DO UPDATE SET offer_id=CASE WHEN excluded.offer_id<>''THEN excluded.offer_id ELSE products.offer_id END,product_id=CASE WHEN excluded.product_id<>''THEN excluded.product_id ELSE products.product_id END,name=CASE WHEN excluded.name<>''THEN excluded.name ELSE products.name END,image_url=CASE WHEN excluded.image_url<>''THEN excluded.image_url ELSE products.image_url END,updated_at=CURRENT_TIMESTAMP",params![sku,text_at(offer_i),text_at(product_id_i),text_at(name_i),text_at(image_i)]).map_err(|e|e.to_string())?;
        }
        upsert_cost(&tx, &input)?;
        count += 1;
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(count)
}

#[cfg(test)]
mod migration_tests {
    use super::*;
    #[test]
    fn csv_parser_preserves_commas_and_quotes() {
        assert_eq!(
            parse_csv_line("\"SKU-1\",\"备注,含逗号\",\"双\"\"引号\""),
            vec!["SKU-1", "备注,含逗号", "双\"引号"]
        );
    }
    #[test]
    fn installed_build_initializes_writable_local_data_from_template() {
        let root = std::env::temp_dir().join(format!(
            "ozon-local-install-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let exe_dir = root.join("program");
        let local = root.join("local");
        let resources = root.join("resources");
        fs::create_dir_all(&exe_dir).unwrap();
        fs::create_dir_all(resources.join("data-next-template")).unwrap();
        fs::write(
            resources.join("data-next-template/shops.json"),
            r#"{"active_shop_id":"default","shops":[]}"#,
        )
        .unwrap();
        fs::write(
            resources.join("data-next-template/database-generation.json"),
            "{}",
        )
        .unwrap();
        let result = locate_data_dir(&exe_dir.join("app.exe"), &local, &resources).unwrap();
        assert_eq!(result, local.join("data-next"));
        assert!(result.join("shops.json").is_file());
        fs::remove_dir_all(root).unwrap();
    }
}

#[tauri::command]
fn warehouse_mappings(state: State<AppState>) -> Result<Vec<WarehouseMapping>, String> {
    let c = db(&state)?;
    let mut stmt=c.prepare("WITH names AS(SELECT origin name FROM posting_routes WHERE origin<>'' UNION ALL SELECT destination FROM posting_routes WHERE destination<>'') SELECT n.name,COALESCE(m.cluster_name,''),COUNT(*) FROM names n LEFT JOIN warehouse_cluster_mappings m ON m.warehouse_name=n.name GROUP BY n.name ORDER BY COUNT(*) DESC,n.name").map_err(|e|e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok(WarehouseMapping {
                warehouse_name: r.get(0)?,
                cluster_name: r.get(1)?,
                order_count: r.get(2)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}
#[tauri::command]
fn save_warehouse_mapping(
    warehouse_name: String,
    cluster_name: String,
    state: State<AppState>,
) -> Result<(), String> {
    let c = db(&state)?;
    c.execute("INSERT INTO warehouse_cluster_mappings(warehouse_name,cluster_name,updated_at) VALUES(?1,?2,CURRENT_TIMESTAMP) ON CONFLICT(warehouse_name) DO UPDATE SET cluster_name=excluded.cluster_name,updated_at=CURRENT_TIMESTAMP",params![warehouse_name,cluster_name]).map_err(|e|e.to_string())?;
    Ok(())
}

fn tariff_for(
    c: &Connection,
    origin: &str,
    destination: &str,
    volume: f64,
    price: f64,
) -> Option<f64> {
    c.query_row("SELECT AVG(CASE WHEN ?4<=300 THEN price_le_300 ELSE price_gt_300 END) FROM delivery_tariffs WHERE origin=?1 AND destination=?2 AND ?3>=volume_min_l AND ?3<=volume_max_l",params![origin,destination,volume,price],|r|r.get::<_,Option<f64>>(0)).ok().flatten()
}
fn estimate_delivery(
    c: &Connection,
    sku: &str,
    origin: &str,
    destination: &str,
    price: f64,
) -> (Option<f64>, String) {
    let dims = c
        .query_row(
            "SELECT length_cm,width_cm,height_cm FROM product_costs WHERE sku=?1",
            [sku],
            |r| {
                Ok((
                    r.get::<_, Option<f64>>(0)?,
                    r.get::<_, Option<f64>>(1)?,
                    r.get::<_, Option<f64>>(2)?,
                ))
            },
        )
        .ok();
    let volume = match dims {
        Some((Some(l), Some(w), Some(h))) if l > 0.0 && w > 0.0 && h > 0.0 => l * w * h / 1000.0,
        _ => return (None, "缺少尺寸".into()),
    };
    let map = |name: &str| {
        c.query_row(
            "SELECT cluster_name FROM warehouse_cluster_mappings WHERE warehouse_name=?1",
            [name],
            |r| r.get::<_, String>(0),
        )
        .unwrap_or_else(|_| name.to_string())
    };
    let o = map(origin);
    let d = map(destination);
    if let Some(v) = tariff_for(c, &o, &d, volume, price) {
        return (Some(v), "仓库映射精确匹配".into());
    }
    let configured = c
        .prepare(
            "SELECT cluster_name,weight FROM product_cluster_weights WHERE sku=?1 AND weight>0",
        )
        .and_then(|mut s| {
            s.query_map([sku], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)))?
                .collect::<Result<Vec<_>, _>>()
        })
        .unwrap_or_default();
    let (groups, basis) = if configured.is_empty() {
        let mut stmt=match c.prepare("SELECT COALESCE(m.cluster_name,p.destination),COUNT(*)*1.0 FROM posting_routes p LEFT JOIN warehouse_cluster_mappings m ON m.warehouse_name=p.destination WHERE p.sku=?1 AND p.destination<>'' GROUP BY 1"){Ok(v)=>v,Err(_)=>return(None,"无匹配运价".into())};
        let values =
            match stmt.query_map([sku], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))) {
                Ok(v) => v.filter_map(Result::ok).collect(),
                Err(_) => vec![],
            };
        (values, "历史集群权重预估")
    } else {
        (configured, "产品自定义集群权重预估")
    };
    let mut total = 0.0;
    let mut weighted = 0.0;
    for (cluster, weight) in groups {
        if let Some(v) = tariff_for(c, &o, &cluster, volume, price) {
            total += weight;
            weighted += v * weight
        }
    }
    if total > 0.0 {
        (Some(weighted / total), basis.into())
    } else {
        (None, "无匹配运价".into())
    }
}

#[tauri::command]
fn save_fbs_threshold(
    hours: i64,
    warning_hours: i64,
    state: State<AppState>,
) -> Result<(), String> {
    let c = db(&state)?;
    c.execute("INSERT INTO settings(key,value) VALUES('fbs_shipping_hours',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[hours.to_string()]).map_err(|e|e.to_string())?;
    c.execute("INSERT INTO settings(key,value) VALUES('fbs_warning_hours',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[warning_hours.to_string()]).map_err(|e|e.to_string())?;
    Ok(())
}
fn sync_fbs_orders_blocking(range: DateRange, state: &AppState) -> Result<i64, String> {
    let mut c = db(state)?;
    let mut offset = 0;
    let mut postings = vec![];
    loop {
        let response = seller_post(
            &c,
            "/v3/posting/fbs/list",
            &serde_json::json!({"dir":"ASC","filter":{"since":format!("{}T00:00:00Z",range.from),"to":format!("{}T23:59:59Z",range.to),"status":""},"limit":1000,"offset":offset,"with":{"analytics_data":true,"financial_data":true}}),
        )?;
        let batch = response
            .pointer("/result/postings")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let count = batch.len();
        postings.extend(batch);
        if count < 1000 {
            break;
        }
        offset += count as i64;
        if offset > 50000 {
            return Err("FBS 订单超过 50000 条，请缩小日期范围".into());
        }
    }
    let tx = c.transaction().map_err(|e| e.to_string())?;
    let mut written = 0;
    for posting in postings {
        let number = json_text(posting.get("posting_number"));
        let status = json_text(posting.get("status"));
        let created = json_text(posting.get("in_process_at"));
        let day = created.get(0..10).unwrap_or("").to_string();
        let origin = json_text(posting.pointer("/analytics_data/warehouse_name"))
            .or_else_empty(|| json_text(posting.pointer("/delivery_method/warehouse")));
        let destination = json_text(posting.pointer("/analytics_data/region"))
            .or_else_empty(|| json_text(posting.pointer("/analytics_data/city")));
        for (index, item) in posting
            .get("products")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .enumerate()
        {
            let sku = item
                .get("sku")
                .map(|v| {
                    if let Some(n) = v.as_i64() {
                        n.to_string()
                    } else {
                        json_text(Some(v))
                    }
                })
                .unwrap_or_default();
            let offer = json_text(item.get("offer_id"));
            let name = json_text(item.get("name"));
            let quantity = item.get("quantity").and_then(|v| v.as_i64()).unwrap_or(0);
            let price = json_text(item.get("price")).parse::<f64>().unwrap_or(0.0);
            let id = format!("seller_fbs:{number}:{sku}:{index}");
            tx.execute("INSERT INTO posting_routes(event_id,day,sku,offer_id,product_name,quantity,scheme,status,posting_number,origin,destination,order_price,source,updated_at)VALUES(?1,?2,?3,?4,?5,?6,'FBS',?7,?8,?9,?10,?11,'seller_fbs',CURRENT_TIMESTAMP)ON CONFLICT(event_id)DO UPDATE SET day=excluded.day,product_name=excluded.product_name,quantity=excluded.quantity,status=excluded.status,origin=excluded.origin,destination=excluded.destination,order_price=excluded.order_price,updated_at=CURRENT_TIMESTAMP",params![id,day,sku,offer,name,quantity,status,number,origin,destination,price]).map_err(|e|e.to_string())?;
            written += 1
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(written)
}

fn sync_fbo_orders_blocking(range: DateRange, state: &AppState) -> Result<i64, String> {
    let mut c = db(state)?;
    let mut offset = 0_i64;
    let mut postings = Vec::new();
    loop {
        let response = seller_post(
            &c,
            "/v2/posting/fbo/list",
            &serde_json::json!({
                "dir":"ASC",
                "filter":{
                    "since":format!("{}T00:00:00Z",range.from),
                    "to":format!("{}T23:59:59Z",range.to),
                    "status":""
                },
                "limit":1000,
                "offset":offset,
                "translit":true,
                "with":{"analytics_data":true,"financial_data":true}
            }),
        )?;
        let batch = response
            .get("result")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let count = batch.len();
        postings.extend(batch);
        if count < 1000 {
            break;
        }
        offset += count as i64;
        if offset > 50_000 {
            return Err("FBO 订单超过 50000 条，请缩小日期范围".into());
        }
    }
    let tx = c.transaction().map_err(|e| e.to_string())?;
    let mut written = 0_i64;
    for posting in postings {
        let number = json_text(posting.get("posting_number"));
        let status = json_text(posting.get("status"));
        let created = json_text(posting.get("created_at"))
            .or_else_empty(|| json_text(posting.get("in_process_at")));
        let day = created.get(0..10).unwrap_or("").to_string();
        let origin = json_text(posting.pointer("/analytics_data/warehouse_name"));
        let destination = json_text(posting.pointer("/analytics_data/region"))
            .or_else_empty(|| json_text(posting.pointer("/analytics_data/city")));
        for (index, item) in posting
            .get("products")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .enumerate()
        {
            let sku = item
                .get("sku")
                .map(|v| {
                    v.as_i64()
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| json_text(Some(v)))
                })
                .unwrap_or_default();
            let offer = json_text(item.get("offer_id"));
            let quantity = item.get("quantity").and_then(|v| v.as_i64()).unwrap_or(0);
            let price = json_text(item.get("price")).parse::<f64>().unwrap_or(0.0);
            let id = format!("seller_fbo:{number}:{sku}:{index}");
            tx.execute("INSERT INTO posting_routes(event_id,day,sku,offer_id,product_name,quantity,scheme,status,posting_number,origin,destination,order_price,source,updated_at)VALUES(?1,?2,?3,?4,?5,?6,'FBO',?7,?8,?9,?10,?11,'seller_fbo',CURRENT_TIMESTAMP)ON CONFLICT(event_id)DO UPDATE SET day=excluded.day,offer_id=excluded.offer_id,product_name=excluded.product_name,quantity=excluded.quantity,status=excluded.status,origin=excluded.origin,destination=excluded.destination,order_price=excluded.order_price,updated_at=CURRENT_TIMESTAMP",params![id,day,sku,offer,json_text(item.get("name")),quantity,status,number,origin,destination,price]).map_err(|e|e.to_string())?;
            written += 1;
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(written)
}

#[tauri::command]
async fn sync_fbs_orders(range: DateRange, state: State<'_, AppState>) -> Result<i64, String> {
    let owned = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || sync_fbs_orders_blocking(range, &owned))
        .await
        .map_err(|e| e.to_string())?
}

trait EmptyFallback {
    fn or_else_empty<F: FnOnce() -> String>(self, fallback: F) -> String;
}
impl EmptyFallback for String {
    fn or_else_empty<F: FnOnce() -> String>(self, fallback: F) -> String {
        if self.is_empty() {
            fallback()
        } else {
            self
        }
    }
}
#[tauri::command]
fn fbs_orders(query: String, state: State<AppState>) -> Result<Vec<FbsOrderRow>, String> {
    let c = db(&state)?;
    let hours = setting(&c, "fbs_shipping_hours")
        .parse::<i64>()
        .unwrap_or(24);
    let warning = setting(&c, "fbs_warning_hours").parse::<i64>().unwrap_or(4);
    let needle = format!("%{}%", query);
    let mut stmt=c.prepare("SELECT posting_number,offer_id,sku,product_name,day,status,origin,destination,order_price FROM posting_routes WHERE UPPER(scheme) LIKE '%FBS%' AND (?1='%%' OR posting_number LIKE ?1 OR offer_id LIKE ?1 OR sku LIKE ?1 OR product_name LIKE ?1) ORDER BY day DESC LIMIT 3000").map_err(|e|e.to_string())?;
    let raw = stmt
        .query_map([needle], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, f64>(8)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let now = chrono::Local::now().naive_local();
    Ok(raw
        .into_iter()
        .map(
            |(posting, offer, sku, name, day, status, origin, destination, price)| {
                let ordered = chrono::NaiveDate::parse_from_str(&day, "%Y-%m-%d")
                    .ok()
                    .and_then(|d| d.and_hms_opt(0, 0, 0));
                let deadline = ordered.map(|d| d + chrono::Duration::hours(hours));
                let waiting = status.contains("awaiting");
                let alert = match deadline {
                    Some(d) if waiting && now > d => "overdue",
                    Some(d) if waiting && now + chrono::Duration::hours(warning) >= d => "warning",
                    _ if waiting => "pending",
                    _ => "shipped",
                };
                let (fee, basis) = estimate_delivery(&c, &sku, &origin, &destination, price);
                FbsOrderRow {
                    posting_number: posting,
                    offer_id: offer,
                    sku,
                    product_name: name,
                    ordered_at: day,
                    status,
                    origin,
                    destination,
                    deadline: deadline
                        .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                        .unwrap_or_default(),
                    alert_level: alert.into(),
                    estimated_delivery: fee,
                    estimate_basis: basis,
                }
            },
        )
        .collect())
}

fn first_capture(text: &str, patterns: &[&str]) -> String {
    for p in patterns {
        if let Ok(re) = regex::Regex::new(p) {
            if let Some(c) = re.captures(text).and_then(|m| m.get(1)) {
                return c.as_str().replace("\\u0026", "&").replace("&amp;", "&");
            }
        }
    }
    String::new()
}
fn canonical_ozon_product_url(value: &str) -> Result<(String, String), String> {
    let code = first_capture(value, &[r#"(?:-|/)(\d{6,})(?:/|\?|$)"#, r#"^(\d{6,})$"#]);
    if code.is_empty() {
        return Err("无法从链接中识别 Ozon 商品编号；也可以直接粘贴纯数字 Артикул".into());
    }
    Ok((format!("https://www.ozon.ru/product/{code}/"), code))
}
struct ParsedCompetitorPage {
    name: String,
    image: String,
    gallery_images: Vec<String>,
    gallery_mode: String,
    collector_source: String,
    price: Option<f64>,
    sales: Option<i64>,
    price_raw: String,
    sales_raw: String,
}
fn competitor_page_needs_browser(parsed: &ParsedCompetitorPage) -> bool {
    parsed.image.is_empty()
}
fn capture_json_string_array(html: &str, key: &str) -> Vec<String> {
    let pattern = format!(r#"\"{}\"\s*:\s*(\[[^\]]*\])"#, regex::escape(key));
    regex::Regex::new(&pattern)
        .ok()
        .and_then(|re| re.captures(html))
        .and_then(|captures| captures.get(1))
        .and_then(|value| serde_json::from_str::<Vec<String>>(value.as_str()).ok())
        .unwrap_or_default()
}
fn parse_competitor_html(html: &str) -> Result<ParsedCompetitorPage, String> {
    let lower = html.to_lowercase();
    // Do not scan bundled JavaScript for generic words such as `captcha` or
    // `проверка`: the normal Ozon storefront contains those strings too.  Only
    // reject an explicit challenge title/body marker.
    if [
        "<title>antibot challenge page",
        "<title>captcha",
        "verify you are human</h",
        "access denied</h",
        "robot check</h",
        "проверка безопасности</h",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
    {
        return Err("blocked: Ozon 返回验证或访问限制页面".into());
    }
    let name = first_capture(
        html,
        &[
            r#"<meta[^>]+property=["']og:title["'][^>]+content=["']([^"']+)"#,
            r#"<meta[^>]+content=["']([^"']+)["'][^>]+property=["']og:title["']"#,
            r#"\"name\"\s*:\s*\"([^\"]+)\""#,
            r#"<title>([^<]+)</title>"#,
        ],
    );
    let image = first_capture(
        html,
        &[
            r#"\"codexMainImage\"\s*:\s*\"([^\"]+)\""#,
            r#"<meta[^>]+property=["']og:image["'][^>]+content=["']([^"']+)"#,
            r#"<meta[^>]+content=["']([^"']+)["'][^>]+property=["']og:image["']"#,
            r#"\"image\"\s*:\s*\"([^\"]+)\""#,
        ],
    );
    let price_raw = first_capture(
        html,
        &[
            r#"\"codexVisiblePrice\"\s*:\s*\"([^\"]+)\""#,
            r#"\"price\"\s*:\s*\"?([0-9]+(?:[.,][0-9]+)?)"#,
            r#"\"cardPrice\"\s*:\s*\"?([0-9]+(?:[.,][0-9]+)?)"#,
            r#"([0-9][0-9 \u{00a0}\u{202f}]{1,})\s*₽"#,
        ],
    );
    let sales_raw = first_capture(
        html,
        &[
            r#"\"soldQuantity\"\s*:\s*([0-9]+)"#,
            r#"\"ordersCount\"\s*:\s*([0-9]+)"#,
            r#"\"soldAmount\"\s*:\s*([0-9]+)"#,
        ],
    );
    let price = price_raw
        .replace([' ', '\u{00a0}', '\u{202f}', '₽'], "")
        .replace(',', ".")
        .parse()
        .ok();
    let sales = sales_raw.parse().ok();
    if name.is_empty() && image.is_empty() && price.is_none() {
        return Err("changed_layout: 页面可访问，但未返回可识别的商品结构".into());
    }
    let mut gallery_images = capture_json_string_array(html, "codexGalleryImages");
    if gallery_images.is_empty() && !image.is_empty() {
        gallery_images.push(image.clone());
    }
    let gallery_mode = first_capture(html, &[r#"\"codexGalleryMode\"\s*:\s*\"([^\"]*)"#]);
    let collector_source = first_capture(html, &[r#"\"codexCollectorSource\"\s*:\s*\"([^\"]*)"#]);
    Ok(ParsedCompetitorPage {
        name,
        image,
        gallery_images,
        gallery_mode,
        collector_source,
        price,
        sales,
        price_raw,
        sales_raw,
    })
}
fn save_competitor_html(
    c: &Connection,
    id: i64,
    html: &str,
    source: &str,
) -> Result<ParsedCompetitorPage, String> {
    let url = c
        .query_row(
            "SELECT product_url FROM competitor_products WHERE id=?1",
            [id],
            |r| r.get::<_, String>(0),
        )
        .map_err(|e| e.to_string())?;
    let (_, code) = canonical_ozon_product_url(&url)?;
    let parsed = parse_competitor_html(html)?;
    c.execute("UPDATE competitor_products SET product_code=CASE WHEN ?2='' THEN product_code ELSE ?2 END,name=CASE WHEN ?3='' THEN name ELSE ?3 END,image_url=CASE WHEN ?4='' THEN image_url ELSE ?4 END,updated_at=CURRENT_TIMESTAMP WHERE id=?1",params![id,code,parsed.name,parsed.image]).map_err(|e|e.to_string())?;
    let snapshot_source = if parsed.collector_source.is_empty() {
        source
    } else {
        parsed.collector_source.as_str()
    };
    c.execute("INSERT INTO competitor_snapshots(competitor_id,captured_at,price,sales_total,source) VALUES(?1,CURRENT_TIMESTAMP,?2,?3,?4)",params![id,parsed.price,parsed.sales,snapshot_source]).map_err(|e|e.to_string())?;
    Ok(parsed)
}
fn validate_competitor_identity(html: &str, expected_code: &str) -> Result<(), String> {
    let observed_url = first_capture(
        html,
        &[
            r#"<meta[^>]+property=["']og:url["'][^>]+content=["']([^"']+)"#,
            r#"<link[^>]+rel=["']canonical["'][^>]+href=["']([^"']+)"#,
        ],
    );
    validate_competitor_url_identity(&observed_url, expected_code)?;
    Ok(())
}
fn validate_competitor_url_identity(observed_url: &str, expected_code: &str) -> Result<(), String> {
    if let Ok((_, observed_code)) = canonical_ozon_product_url(observed_url) {
        if observed_code != expected_code {
            return Err(format!(
                "ambiguous_match: 目标商品 {expected_code}，页面商品 {observed_code}"
            ));
        }
    }
    Ok(())
}
fn installed_competitor_browsers() -> Vec<std::path::PathBuf> {
    [
        // Normal Chrome has been observed to reach Ozon when an isolated Edge
        // session receives the synthetic "no connection" page. Keep browsers
        // in separate profiles and try Chrome first.
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
    ]
    .into_iter()
    .map(std::path::PathBuf::from)
    .filter(|candidate| candidate.is_file())
    .collect()
}

// Kept as a migration reference only. The release path below is Rust-only;
// this legacy helper and its Python/extension callers are excluded from the
// compiled application so a stale sidecar cannot be selected accidentally.
#[cfg(any())]
fn installed_competitor_browser() -> Result<std::path::PathBuf, String> {
    installed_competitor_browsers()
        .into_iter()
        .next()
        .ok_or_else(|| "未找到 Chrome 或 Edge 可执行文件".into())
}

#[cfg(any())]
fn collect_competitor_python_html(
    url: &str,
    expected_code: &str,
    data_dir: &Path,
    competitor_id: i64,
) -> Result<String, String> {
    use std::io::Read;
    let current = std::env::current_exe().map_err(|e| e.to_string())?;
    let helper = current
        .parent()
        .unwrap_or(Path::new("."))
        .join("competitor-collector.exe");
    let helper = if helper.is_file() {
        helper
    } else {
        PathBuf::from("competitor-collector.exe")
    };
    if !helper.is_file() {
        return Err(
            "inaccessible: 缺少 competitor-collector.exe；请使用完整发布包或重新构建 Python 采集器"
                .into(),
        );
    }
    let profile = data_dir.join("competitor_legacy_python_profile");
    fs::create_dir_all(&profile).map_err(|e| e.to_string())?;
    let mut child = std::process::Command::new(&helper)
        .arg("--url")
        .arg(url)
        .arg("--expected")
        .arg(expected_code)
        .arg("--profile")
        .arg(&profile)
        .arg("--timeout")
        .arg("180")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("无法启动旧版 Python/Playwright 采集器：{e}"))?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(210);
    loop {
        if competitor_task_stop_requested(competitor_id) {
            let _ = child.kill();
            return Err("cancelled: 用户已停止竞品采集".into());
        }
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            break;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            return Err("blocked: Python/Playwright 采集超过 210 秒".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    let mut raw = Vec::new();
    if let Some(mut stdout) = child.stdout.take() {
        stdout.read_to_end(&mut raw).map_err(|e| e.to_string())?;
    }
    // New helpers emit ASCII-only JSON. Lossy decoding also lets upgraded ERP
    // builds explain output from an older helper instead of failing at the
    // process boundary with “stream did not contain valid UTF-8”.
    let decoded = String::from_utf8_lossy(&raw);
    let payload = decoded
        .find('{')
        .zip(decoded.rfind('}'))
        .filter(|(start, end)| start <= end)
        .map(|(start, end)| &decoded[start..=end])
        .ok_or_else(|| "Python 采集器未返回 JSON 数据".to_string())?;
    let value: serde_json::Value =
        serde_json::from_str(payload).map_err(|e| format!("Python 采集器返回格式无效：{e}"))?;
    if !value.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
        let status = match value.get("status").and_then(|v| v.as_str()) {
            Some("incomplete") => "incomplete",
            Some("inaccessible") => "inaccessible",
            Some("ambiguous_match") => "ambiguous_match",
            Some("changed_layout") => "changed_layout",
            _ => "blocked",
        };
        return Err(format!(
            "{status}: {}",
            value
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("Python 采集失败")
        ));
    }
    let final_url = value.get("url").and_then(|v| v.as_str()).unwrap_or(url);
    let name = value.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let price = value
        .get("price")
        .and_then(|v| v.as_f64())
        .map(|v| v.to_string())
        .unwrap_or_default();
    let image = value.get("image").and_then(|v| v.as_str()).unwrap_or("");
    let images = value
        .get("images")
        .cloned()
        .unwrap_or_else(|| serde_json::json!([]));
    let evidence = serde_json::json!({
        "codexVisiblePrice":price,
        "codexMainImage":image,
        "codexGalleryImages":images,
        "codexGalleryMode":value.get("gallery_mode").cloned().unwrap_or(serde_json::Value::Null),
        "codexCollectorSource":value.get("source").cloned().unwrap_or(serde_json::Value::Null)
    });
    let escape_attr = |input: &str| {
        input
            .replace('&', "&amp;")
            .replace('"', "&quot;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    let html=format!("<meta property=\"og:url\" content=\"{}\"><meta property=\"og:title\" content=\"{}\"><meta property=\"og:image\" content=\"{}\"><script type=\"application/json\">{}</script>",escape_attr(final_url),escape_attr(name),escape_attr(image),evidence);
    validate_competitor_identity(&html, expected_code)?;
    Ok(html)
}

#[cfg(any())]
fn collect_competitor_public_extension_html(
    url: &str,
    expected_code: &str,
    data_dir: &Path,
    competitor_id: i64,
) -> Result<String, String> {
    use std::io::{Read, Write};
    let browser_path = installed_competitor_browser()?;
    let browser_name = browser_path
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("");
    let profile = data_dir.join(if browser_name.eq_ignore_ascii_case("chrome.exe") {
        "competitor_public_chrome_profile_v3"
    } else {
        "competitor_public_edge_profile_v3"
    });
    let extension = data_dir.join("competitor_public_reader_extension");
    fs::create_dir_all(&profile).map_err(|e| e.to_string())?;
    fs::create_dir_all(&extension).map_err(|e| e.to_string())?;
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("无法启动竞品本地回传端口：{e}"))?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let debug_probe = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let debug_port = debug_probe.local_addr().map_err(|e| e.to_string())?.port();
    drop(debug_probe);
    fs::write(extension.join("manifest.json"), r#"{"manifest_version":3,"name":"Ozon ERP Public Product Reader","version":"1.0.1","description":"Reads public product fields for the local Ozon ERP.","host_permissions":["http://127.0.0.1/*"],"background":{"service_worker":"background.js"},"content_scripts":[{"matches":["https://www.ozon.ru/product/*","https://ozon.ru/product/*"],"js":["reader.js"],"run_at":"document_idle"}]}"#).map_err(|e| e.to_string())?;
    fs::write(extension.join("background.js"), format!(r#"chrome.runtime.onMessage.addListener((data)=>{{fetch('http://127.0.0.1:{port}/result',{{method:'POST',headers:{{'Content-Type':'application/json'}},body:JSON.stringify(data)}}).catch(()=>{{}});}});"#)).map_err(|e| e.to_string())?;
    let reader = format!(
        r#"(()=>{{
const receiverPort={port};
const visible=el=>{{if(!el)return false;const r=el.getBoundingClientRect(),s=getComputedStyle(el);return r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden'}};
const read=()=>{{
 const roots=[...document.querySelectorAll('[data-widget*="Price"],[data-widget*="price"],[class*="price"],[class*="Price"]')].filter(visible);
 const text=(roots.map(x=>x.innerText||'').join('\n')||document.body?.innerText||'');
 const match=text.match(/(?:^|\s)(\d[\d\s\u00a0\u202f]{{0,12}})\s*₽/);
 const imgs=[...document.querySelectorAll('[data-widget*="Gallery"] img,[data-widget*="gallery"] img,img')].filter(visible);
 const main=imgs.find(x=>{{const r=x.getBoundingClientRect(),u=x.currentSrc||x.src||'';return /^https?:/i.test(u)&&r.width>=250&&r.height>=250}})||imgs.find(x=>/^https?:/i.test(x.currentSrc||x.src||''));
 const data={{url:location.href,title:document.title||'',visibleText:(document.body?.innerText||'').slice(0,20000),html:document.documentElement?.outerHTML||'',price:match?match[1]:'',image:main?(main.currentSrc||main.src||''):''}};
 chrome.runtime.sendMessage(data).catch(()=>{{}});
}};setInterval(read,1000);read();
}})();"#
    );
    fs::write(extension.join("reader.js"), reader).map_err(|e| e.to_string())?;
    let mut child = std::process::Command::new(&browser_path)
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg(format!(
            "--disable-extensions-except={}",
            extension.display()
        ))
        .arg(format!("--load-extension={}", extension.display()))
        .arg(format!("--remote-debugging-port={debug_port}"))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--new-window")
        .arg("about:blank")
        .spawn()
        .map_err(|e| format!("无法启动竞品公开页面浏览器：{e}"))?;
    std::thread::sleep(std::time::Duration::from_secs(3));
    let _ = std::process::Command::new(&browser_path)
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg(url)
        .spawn()
        .map_err(|e| format!("浏览器已启动，但无法打开竞品商品页：{e}"))?;
    let mut devtools_browser = None;
    for _ in 0..30 {
        if let Ok(response) = ureq::get(&format!("http://127.0.0.1:{debug_port}/json/version"))
            .timeout(std::time::Duration::from_secs(1))
            .call()
        {
            if let Ok(raw) = response.into_string() {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) {
                    if let Some(ws) = value.get("webSocketDebuggerUrl").and_then(|v| v.as_str()) {
                        if let Ok(browser) = headless_chrome::Browser::connect(ws.to_string()) {
                            devtools_browser = Some(browser);
                            break;
                        }
                    }
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    let visible_script = r#"(()=>{
      const visible=(el)=>{if(!el)return false;const r=el.getBoundingClientRect(),s=getComputedStyle(el);return r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden'};
      const roots=[...document.querySelectorAll('[data-widget*="Price"],[data-widget*="price"],[class*="price"],[class*="Price"]')].filter(visible);
      const text=(roots.map(x=>x.innerText||'').join('\n')||document.body?.innerText||'');
      const match=text.match(/(?:^|\s)(\d[\d\s\u00a0\u202f]{0,12})\s*₽/);
      const imgs=[...document.querySelectorAll('[data-widget*="Gallery"] img,[data-widget*="gallery"] img,img')].filter(visible);
      const main=imgs.find(x=>{const r=x.getBoundingClientRect(),u=x.currentSrc||x.src||'';return /^https?:/i.test(u)&&r.width>=250&&r.height>=250})||imgs.find(x=>/^https?:/i.test(x.currentSrc||x.src||''));
      return JSON.stringify({url:location.href,html:document.documentElement?.outerHTML||'',price:match?match[1]:'',image:main?(main.currentSrc||main.src||''):''});
    })()"#;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(240);
    let mut best = String::new();
    let mut last_signature = String::new();
    let mut stable = 0;
    while std::time::Instant::now() < deadline {
        if competitor_task_stop_requested(competitor_id) {
            let _ = child.kill();
            return Err("cancelled: 用户已停止竞品采集".into());
        }
        if let Some(browser) = devtools_browser.as_ref() {
            let tabs = browser
                .get_tabs()
                .lock()
                .map_err(|e| format!("无法读取竞品浏览器标签页：{e}"))?;
            for tab in tabs.iter() {
                if !tab.get_url().contains("ozon.ru/product/") {
                    continue;
                }
                if let Ok(result) = tab.evaluate(visible_script, false) {
                    if let Some(raw) = result.value.and_then(|v| v.as_str().map(str::to_string)) {
                        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) {
                            let current_url =
                                value.get("url").and_then(|v| v.as_str()).unwrap_or("");
                            let html = value.get("html").and_then(|v| v.as_str()).unwrap_or("");
                            let harvested = serde_json::json!({"codexVisiblePrice":value.get("price").and_then(|v|v.as_str()).unwrap_or(""),"codexMainImage":value.get("image").and_then(|v|v.as_str()).unwrap_or("")});
                            if canonical_ozon_product_url(current_url)
                                .map(|(_, c)| c == expected_code)
                                .unwrap_or(false)
                                && !html.is_empty()
                            {
                                let enriched = format!(
                                    "<script type=\"application/json\">{harvested}</script>{html}"
                                );
                                if let Ok(parsed) = parse_competitor_html(&enriched) {
                                    if parsed.price.is_some() && !parsed.image.is_empty() {
                                        best = enriched;
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if !best.is_empty() {
                break;
            }
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(800)));
                let mut request = Vec::new();
                let mut part = [0u8; 65536];
                loop {
                    match stream.read(&mut part) {
                        Ok(0) => break,
                        Ok(n) => {
                            request.extend_from_slice(&part[..n]);
                            if let Some(header_end) =
                                request.windows(4).position(|w| w == b"\r\n\r\n")
                            {
                                let headers = String::from_utf8_lossy(&request[..header_end]);
                                let content_length = headers
                                    .lines()
                                    .find_map(|line| {
                                        let (name, value) = line.split_once(':')?;
                                        name.eq_ignore_ascii_case("content-length")
                                            .then(|| value.trim().parse::<usize>().ok())
                                            .flatten()
                                    })
                                    .unwrap_or(0);
                                if request.len() >= header_end + 4 + content_length {
                                    break;
                                }
                            }
                        }
                        Err(_) => break,
                    }
                }
                let _ = stream.write_all(b"HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Headers: Content-Type\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                let body = request
                    .windows(4)
                    .position(|w| w == b"\r\n\r\n")
                    .map(|p| &request[p + 4..])
                    .unwrap_or(&[]);
                if let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) {
                    let current_url = value.get("url").and_then(|v| v.as_str()).unwrap_or("");
                    let html = value.get("html").and_then(|v| v.as_str()).unwrap_or("");
                    let identity = canonical_ozon_product_url(current_url)
                        .map(|(_, c)| c == expected_code)
                        .unwrap_or(false);
                    let harvested = serde_json::json!({
                        "codexVisiblePrice": value.get("price").and_then(|v| v.as_str()).unwrap_or(""),
                        "codexMainImage": value.get("image").and_then(|v| v.as_str()).unwrap_or("")
                    });
                    if identity && !html.is_empty() {
                        let enriched =
                            format!("<script type=\"application/json\">{harvested}</script>{html}");
                        if let Ok(parsed) = parse_competitor_html(&enriched) {
                            if parsed.price.is_some() && !parsed.image.is_empty() {
                                let signature = format!(
                                    "{}|{}|{}",
                                    parsed.price_raw, parsed.image, parsed.name
                                );
                                stable = if signature == last_signature {
                                    stable + 1
                                } else {
                                    0
                                };
                                last_signature = signature;
                                best = enriched;
                                if stable >= 1 {
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(_) => {}
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    let _ = child.kill();
    if best.is_empty() {
        return Err("blocked: 普通浏览器公开页面在 240 秒内未回传完整售价与主图；请检查页面是否显示 Ozon 网络/验证限制".into());
    }
    validate_competitor_identity(&best, expected_code)?;
    Ok(best)
}

fn collect_competitor_browser_channel(
    url: &str,
    expected_code: &str,
    data_dir: &Path,
    competitor_id: i64,
    browser_path: &Path,
) -> Result<String, String> {
    use headless_chrome::Browser;

    struct BrowserChild(std::process::Child);
    impl Drop for BrowserChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    let channel = browser_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("browser")
        .to_lowercase();
    let profile = data_dir.join(format!("competitor_rust_{channel}_profile_v2"));
    fs::create_dir_all(&profile).map_err(|e| e.to_string())?;

    // Browser::new chooses a port from a fixed 8000-9000 range and creates an
    // about:blank tab before navigation. On machines with stale automation
    // processes this can exhaust the range or leave the user on a blank tab.
    // Ask Windows for a genuinely free port and pass the product URL directly
    // to Chrome/Edge, then attach CDP only for observation and identity checks.
    let probe = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("无法分配 {channel} 调试端口：{e}"))?;
    let debug_port = probe.local_addr().map_err(|e| e.to_string())?.port();
    drop(probe);
    let child = std::process::Command::new(browser_path)
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg(format!("--remote-debugging-port={debug_port}"))
        .arg("--remote-allow-origins=*")
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        // Some Windows machines cannot initialize Chromium's GPU process in
        // the isolated collection profile.  The browser then exits before
        // CDP can inspect the product page, although the same URL works in a
        // normal browser window.  Software rendering is sufficient here.
        .arg("--disable-gpu")
        .arg("--lang=ru-RU")
        .arg("--new-window")
        .arg(url)
        .spawn()
        .map_err(|e| format!("无法启动 {channel} 商品浏览器：{e}"))?;
    let _child = BrowserChild(child);
    let mut browser = None;
    for _ in 0..50 {
        if let Ok(response) = ureq::get(&format!("http://127.0.0.1:{debug_port}/json/version"))
            .timeout(std::time::Duration::from_secs(1))
            .call()
        {
            if let Ok(raw) = response.into_string() {
                if let Some(websocket) = serde_json::from_str::<serde_json::Value>(&raw)
                    .ok()
                    .and_then(|value| {
                        value
                            .get("webSocketDebuggerUrl")?
                            .as_str()
                            .map(str::to_string)
                    })
                {
                    if let Ok(connected) = Browser::connect(websocket) {
                        browser = Some(connected);
                        break;
                    }
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    let browser = browser.ok_or_else(|| format!("无法连接 {channel} 商品浏览器调试端口"))?;
    let mut product_tab = None;
    for _ in 0..30 {
        {
            let tabs = browser
                .get_tabs()
                .lock()
                .map_err(|e| format!("无法读取 {channel} 商品标签页：{e}"))?;
            product_tab = tabs
                .iter()
                .find(|tab| tab.get_url().contains("ozon.ru/product/"))
                .cloned();
        }
        if product_tab.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    let tab = match product_tab {
        Some(tab) => tab,
        None => {
            let tab = browser
                .new_tab()
                .map_err(|e| format!("无法创建 {channel} 商品标签页：{e}"))?;
            tab.navigate_to(url)
                .map_err(|e| format!("{channel} 商品页导航失败：{e}"))?;
            tab
        }
    };
    let _ = tab.bring_to_front();

    let script = r###"(()=>{
      const visible=(el)=>{if(!el)return false;const r=el.getBoundingClientRect();const s=getComputedStyle(el);return r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden'};
      const meta=(key)=>document.querySelector(`meta[property="${key}"],meta[name="${key}"]`)?.content||'';
      const source=(image)=>{
        const set=String(image.getAttribute('srcset')||'').split(',').map(x=>x.trim().split(/\s+/)[0]).filter(Boolean);
        return set.at(-1)||image.currentSrc||image.src||image.getAttribute('data-src')||'';
      };
      const normalize=(raw)=>{
        let value=String(raw||'').trim();
        if(value.startsWith('//')) value=`https:${value}`;
        if(!/^https:\/\//i.test(value)||!/ozone\.ru/i.test(value)||/captcha|abt-challenge|\/icons?\//i.test(value)) return '';
        try { const u=new URL(value); u.pathname=u.pathname.replace(/\/wc\d+\//i,'/wc2000/'); return u.toString(); } catch (_) { return value; }
      };
      let product={};
      for(const script of document.querySelectorAll('script[type="application/ld+json"]')){
        try{
          const raw=JSON.parse(script.textContent||'{}'), queue=Array.isArray(raw)?raw:[raw];
          for(const item of queue){
            if(item&&String(item['@type']||'').toLowerCase()==='product') product=item;
            if(item&&Array.isArray(item['@graph'])) queue.push(...item['@graph']);
          }
        }catch (_) {}
      }
      const elements=new Set();
      for(const selector of ['[data-widget="webGallery"] img','[data-widget*="Gallery"] img','[data-widget*="gallery"] img'])
        for(const image of document.querySelectorAll(selector)) elements.add(image);
      const records=[...elements].map((image,order)=>{const r=image.getBoundingClientRect();return {value:normalize(source(image)),order,x:r.x,y:r.y,w:r.width,h:r.height};}).filter(x=>x.value);
      const shown=records.filter(x=>x.w>0&&x.h>0);
      const preview=shown.reduce((a,x)=>!a||x.w*x.h>a.w*a.h?x:a,null);
      const small=shown.filter(x=>Math.max(x.w,x.h)<=180);
      const rail=preview?small.filter(x=>x.x+x.w<=preview.x+Math.max(24,preview.w*.12)):[];
      const slots=(rail.length?rail:(small.length?small:records)).sort((a,b)=>Math.abs(a.y-b.y)>2?a.y-b.y:(a.x-b.x||a.order-b.order));
      let images=slots.map(x=>x.value);
      if(!images.length){
        const raw=product.image||[];
        images=(Array.isArray(raw)?raw:[raw]).map(normalize).filter(Boolean);
        const og=normalize(meta('og:image')); if(og) images.push(og);
      }
      const heading=document.querySelector('h1')?.innerText?.trim()||'';
      if(images.length<2) for(const image of document.querySelectorAll('img[src],img[srcset],img[data-src]')){
        const r=image.getBoundingClientRect(), alt=String(image.alt||'').trim(), value=normalize(source(image));
        if(r.width>0&&r.height>0&&value&&alt&&(alt===heading||heading.includes(alt)||alt.includes(heading))) images.push(value);
      }
      const unique=[],seen=new Set();
      for(const value of images){
        let key=value;
        try{const u=new URL(value),m=u.pathname.match(/\/s3\/multimedia[^/]*\/wc\d+\/([^/]+)$/i);key=m?m[1]:u.origin+u.pathname;}catch (_) {}
        if(!seen.has(key)){seen.add(key);unique.push(value);}
      }
      const priceRoots=[...document.querySelectorAll('[data-widget*="Price"],[data-widget*="price"],[class*="price"],[class*="Price"]')].filter(visible);
      const priceText=priceRoots.map(x=>x.innerText||'').join('\n')||document.body?.innerText||'';
      const visiblePrice=priceText.match(/(?:^|\s)(\d[\d\s\u00a0\u202f]{0,12})\s*₽/);
      const structuredPrice=product.offers?.price||product.offers?.lowPrice||'';
      const main=unique[0]||'';
      return JSON.stringify({
        url:location.href,
        title:String(product.name||meta('og:title')||heading||'').trim(),
        pageTitle:document.title||'',
        visibleText:String(document.body?.innerText||'').slice(0,8000),
        price:String(visiblePrice?visiblePrice[1].replace(/[\s\u00a0\u202f]/g,''):structuredPrice).replace(',','.'),
        image:main,
        images:unique.slice(0,20),
        galleryMode:rail.length?'fixed-left-thumbnail-rail':(small.length?'thumbnail-size-fallback':'gallery-dom-fallback'),
        html:document.documentElement?.outerHTML||''
      });
    })()"###;

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
    let mut best = String::new();
    let mut stable = 0;
    let mut last_signature = String::new();
    let mut last_blocked_reason = String::new();
    let mut hydration_started = false;
    while std::time::Instant::now() < deadline {
        if competitor_task_stop_requested(competitor_id) {
            return Err("cancelled: 用户已停止竞品采集".into());
        }
        let remote = match tab.evaluate(script, false) {
            Ok(value) => value,
            Err(error) => {
                let detail = error.to_string();
                let lower = detail.to_lowercase();
                if lower.contains("closed") || lower.contains("target") {
                    return Err(format!(
                        "blocked: {channel} 浏览器页面在采集期间关闭：{detail}"
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(800));
                continue;
            }
        };
        let Some(raw) = remote
            .value
            .and_then(|value| value.as_str().map(str::to_string))
        else {
            std::thread::sleep(std::time::Duration::from_millis(800));
            continue;
        };
        let value: serde_json::Value = match serde_json::from_str(&raw) {
            Ok(value) => value,
            Err(_) => {
                std::thread::sleep(std::time::Duration::from_millis(800));
                continue;
            }
        };
        let current_url = value
            .get("url")
            .and_then(|item| item.as_str())
            .unwrap_or("");
        let title = value
            .get("title")
            .and_then(|item| item.as_str())
            .unwrap_or("")
            .trim();
        let page_title = value
            .get("pageTitle")
            .and_then(|item| item.as_str())
            .unwrap_or("")
            .to_lowercase();
        let visible_text = value
            .get("visibleText")
            .and_then(|item| item.as_str())
            .unwrap_or("")
            .to_lowercase();
        let html = value
            .get("html")
            .and_then(|item| item.as_str())
            .unwrap_or("");
        let blocked_reason = [
            "похоже, нет соединения",
            "нет соединения",
            "antibot captcha",
            "captcha",
            "access denied",
            "verify you are human",
            "доступ ограничен",
            "проверка безопасности",
        ]
        .iter()
        .find(|marker| page_title.contains(**marker) || visible_text.contains(**marker))
        .copied()
        .unwrap_or("");
        if !blocked_reason.is_empty() {
            last_blocked_reason = blocked_reason.to_string();
            // A synthetic offline page is not actionable in the window. Return
            // so the caller can try the other browser; captcha/security pages
            // stay open until timeout for the user to complete verification.
            if blocked_reason == "похоже, нет соединения" || blocked_reason == "нет соединения"
            {
                return Err(format!("blocked: {channel} Ozon 返回‘似乎没有连接’受阻页"));
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
            continue;
        }
        last_blocked_reason.clear();
        let identity = canonical_ozon_product_url(current_url)
            .map(|(_, code)| code == expected_code)
            .unwrap_or(false);
        if identity && !html.is_empty() && !title.is_empty() {
            if !hydration_started {
                let _ = tab.evaluate(
                    "(()=>{const d=document.querySelector('[data-widget=\\\"webDescription\\\"]'); if(d)d.scrollIntoView({block:'center'}); else window.scrollTo(0, Math.min(document.body.scrollHeight*.55,3500));})()",
                    false,
                );
                hydration_started = true;
                std::thread::sleep(std::time::Duration::from_millis(1200));
                continue;
            }
            let harvested = serde_json::json!({
                "codexVisiblePrice": value.get("price").and_then(|item| item.as_str()).unwrap_or(""),
                "codexMainImage": value.get("image").and_then(|item| item.as_str()).unwrap_or(""),
                "codexGalleryImages": value.get("images").cloned().unwrap_or_else(|| serde_json::json!([])),
                "codexGalleryMode": value.get("galleryMode").cloned().unwrap_or(serde_json::Value::Null),
                "codexCollectorSource": format!("rust_headless_chrome_{channel}"),
            });
            let enriched = format!(
                "<meta property=\"og:url\" content=\"{}\"><meta property=\"og:title\" content=\"{}\"><meta property=\"og:image\" content=\"{}\"><script type=\"application/json\">{harvested}</script>{html}",
                escape_html_attribute(current_url),
                escape_html_attribute(title),
                escape_html_attribute(value.get("image").and_then(|item| item.as_str()).unwrap_or("")),
            );
            if let Ok(parsed) = parse_competitor_html(&enriched) {
                let signature = format!(
                    "{}|{}|{}|{}|{}",
                    parsed.price_raw,
                    parsed.image,
                    parsed.name,
                    parsed.gallery_images.len(),
                    parsed.gallery_mode
                );
                stable = if signature == last_signature {
                    stable + 1
                } else {
                    0
                };
                last_signature = signature;
                if parsed.price.is_some() && !parsed.image.is_empty() {
                    best = enriched;
                    if stable >= 2 {
                        break;
                    }
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    if best.is_empty() {
        let detail = if last_blocked_reason.is_empty() {
            format!("{channel} 未在限定时间内得到可校验的商品页")
        } else {
            format!("{channel} 页面持续显示 {last_blocked_reason}")
        };
        return Err(format!("blocked: {detail}；请完成 Ozon 验证后重试"));
    }
    validate_competitor_identity(&best, expected_code)?;
    Ok(best)
}

fn escape_html_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub(crate) fn collect_competitor_browser_html(
    url: &str,
    expected_code: &str,
    data_dir: &Path,
    competitor_id: i64,
) -> Result<String, String> {
    let browsers = installed_competitor_browsers();
    if browsers.is_empty() {
        return Err("inaccessible: 未找到 Chrome 或 Edge 可执行文件".into());
    }
    let mut diagnostics = Vec::new();
    for browser_path in browsers {
        let channel = browser_path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("browser")
            .to_lowercase();
        match collect_competitor_browser_channel(
            url,
            expected_code,
            data_dir,
            competitor_id,
            &browser_path,
        ) {
            Ok(html) => return Ok(html),
            Err(error) if error.starts_with("cancelled:") => return Err(error),
            Err(error) => diagnostics.push(format!("{channel}: {error}")),
        }
    }
    Err(format!(
        "blocked: Rust Chrome/Edge 采集均未完成；{}",
        diagnostics.join("；")
    ))
}
fn begin_competitor_run(c: &Connection, requested: i64, scope: &str) -> Result<String, String> {
    let now = chrono::Local::now();
    let run_id = format!("competitor-{}", now.timestamp_millis());
    c.execute("INSERT INTO competitor_collection_runs(run_id,started_at,search_context,requested_scope,requested,status)VALUES(?1,?2,'language=ru-RU; device=desktop; login=anonymous; source=public_product_page',?3,?4,'running')",params![run_id,now.to_rfc3339(),scope,requested]).map_err(|e|e.to_string())?;
    Ok(run_id)
}
fn observation_status(error: &str) -> &'static str {
    let lower = error.to_lowercase();
    if lower.starts_with("blocked:") || lower.contains("403") || lower.contains("captcha") {
        "blocked"
    } else if lower.starts_with("changed_layout:") {
        "changed_layout"
    } else if lower.starts_with("ambiguous_match:") {
        "ambiguous_match"
    } else if lower.starts_with("inaccessible:") || lower.contains("401") || lower.contains("404") {
        "inaccessible"
    } else {
        "incomplete"
    }
}

#[cfg(test)]
mod competitor_monitoring_tests {
    use super::{
        competitor_page_needs_browser, observation_status, parse_competitor_html,
        validate_competitor_identity,
    };

    #[test]
    fn missing_public_sales_stays_null_instead_of_zero() {
        let parsed = parse_competitor_html(
            r#"<meta property="og:title" content="SKU-1"><script>{"price":"1299.50"}</script>"#,
        )
        .expect("page should remain a usable partial observation");
        assert_eq!(parsed.price, Some(1299.5));
        assert_eq!(parsed.sales, None);
    }

    #[test]
    fn parses_visible_ozon_price_and_harvested_main_image() {
        let parsed = parse_competitor_html(
            r#"<script>{"codexVisiblePrice":"1 686","codexMainImage":"https://cdn1.ozone.ru/s3/main.jpg"}</script><title>商品</title>"#,
        )
        .expect("visible Ozon fields");
        assert_eq!(parsed.price, Some(1686.0));
        assert_eq!(parsed.image, "https://cdn1.ozone.ru/s3/main.jpg");
        assert_eq!(parsed.sales, None);
    }

    #[test]
    fn access_challenge_has_controlled_blocked_status() {
        let error = parse_competitor_html("<title>Captcha</title>verify you are human")
            .err()
            .expect("challenge page must not be parsed as a product");
        assert_eq!(observation_status(&error), "blocked");
    }

    #[test]
    fn imported_page_must_match_stable_product_code() {
        let error = validate_competitor_identity(
            r#"<meta property="og:url" content="https://www.ozon.ru/product/demo-987654321/">"#,
            "123456789",
        )
        .expect_err("wrong product HTML must be rejected");
        assert_eq!(observation_status(&error), "ambiguous_match");
    }

    #[test]
    fn direct_page_main_image_is_complete_without_gallery() {
        let parsed = parse_competitor_html(
            r#"<meta property="og:title" content="商品"><meta property="og:image" content="https://cdn1.ozone.ru/item.jpg"><script>{"codexVisiblePrice":"1 686"}</script>"#,
        )
        .expect("partial direct page");
        assert!(!competitor_page_needs_browser(&parsed));
    }

    #[test]
    fn browser_gallery_mode_accepts_single_image_products() {
        let parsed = parse_competitor_html(
            r#"<title>商品</title><script>{"codexVisiblePrice":"1 686","codexMainImage":"https://cdn1.ozone.ru/item.jpg","codexGalleryImages":["https://cdn1.ozone.ru/item.jpg"],"codexGalleryMode":"gallery-dom-fallback"}</script>"#,
        )
        .expect("browser page");
        assert!(!competitor_page_needs_browser(&parsed));
    }

    #[test]
    fn browser_channel_is_preserved_in_evidence_fields() {
        let parsed = parse_competitor_html(
            r#"<title>商品</title><script>{"codexVisiblePrice":"1686","codexMainImage":"https://cdn1.ozone.ru/item.jpg","codexGalleryImages":["https://cdn1.ozone.ru/item.jpg"],"codexGalleryMode":"gallery-dom-fallback","codexCollectorSource":"rust_headless_chrome_chrome"}</script>"#,
        )
        .expect("browser capture");
        assert_eq!(parsed.collector_source, "rust_headless_chrome_chrome");
    }

    #[test]
    fn inaccessible_prefix_is_not_reported_as_incomplete() {
        assert_eq!(
            observation_status("inaccessible: browser missing"),
            "inaccessible"
        );
    }
}
fn record_competitor_observation(
    c: &Connection,
    run_id: &str,
    id: i64,
    status: &str,
    retries: i64,
    source_url: &str,
    source: &str,
    parsed: Option<&ParsedCompetitorPage>,
    notes: &str,
) -> Result<(), String> {
    let evidence = parsed
        .map(|p| {
            serde_json::json!({
                "title": p.name,
                "priceRaw": p.price_raw,
                "salesRaw": p.sales_raw,
                "galleryImages": p.gallery_images,
                "galleryMode": p.gallery_mode,
                "collectorSource": if p.collector_source.is_empty() {
                    source
                } else {
                    p.collector_source.as_str()
                },
            })
            .to_string()
        })
        .unwrap_or_default();
    c.execute("INSERT INTO competitor_observations(run_id,competitor_id,observed_at,status,retry_count,source_url,final_url,search_context,price_raw,sales_raw,evidence,notes)VALUES(?1,?2,?3,?4,?5,?6,?6,?7,?8,?9,?10,?11) ON CONFLICT(run_id,competitor_id) DO UPDATE SET observed_at=excluded.observed_at,status=excluded.status,retry_count=excluded.retry_count,price_raw=excluded.price_raw,sales_raw=excluded.sales_raw,evidence=excluded.evidence,notes=excluded.notes",params![run_id,id,chrono::Local::now().to_rfc3339(),status,retries,source_url,format!("language=ru-RU; device=desktop; login=anonymous; source={source}"),parsed.map(|p|p.price_raw.as_str()).unwrap_or(""),parsed.map(|p|p.sales_raw.as_str()).unwrap_or(""),evidence,notes]).map_err(|e|e.to_string())?;
    Ok(())
}
fn finish_competitor_run(c: &Connection, run_id: &str) -> Result<(), String> {
    c.execute("UPDATE competitor_collection_runs SET finished_at=?2,completed=(SELECT COUNT(*) FROM competitor_observations WHERE run_id=?1),ok=(SELECT COUNT(*) FROM competitor_observations WHERE run_id=?1 AND status='ok'),blocked=(SELECT COUNT(*) FROM competitor_observations WHERE run_id=?1 AND status='blocked'),changed_layout=(SELECT COUNT(*) FROM competitor_observations WHERE run_id=?1 AND status='changed_layout'),inaccessible=(SELECT COUNT(*) FROM competitor_observations WHERE run_id=?1 AND status='inaccessible'),ambiguous_match=(SELECT COUNT(*) FROM competitor_observations WHERE run_id=?1 AND status='ambiguous_match'),incomplete=(SELECT COUNT(*) FROM competitor_observations WHERE run_id=?1 AND status='incomplete'),completed_scope=printf('%d/%d',(SELECT COUNT(*) FROM competitor_observations WHERE run_id=?1),requested),status=CASE WHEN EXISTS(SELECT 1 FROM competitor_observations WHERE run_id=?1 AND status<>'ok') THEN 'incomplete' ELSE 'ok' END WHERE run_id=?1",params![run_id,chrono::Local::now().to_rfc3339()]).map_err(|e|e.to_string())?;
    Ok(())
}
fn refresh_competitor_for_run(
    c: &Connection,
    id: i64,
    run_id: &str,
    data_dir: &Path,
) -> Result<(), String> {
    let url = c
        .query_row(
            "SELECT product_url FROM competitor_products WHERE id=?1",
            [id],
            |r| r.get::<_, String>(0),
        )
        .map_err(|e| e.to_string())?;
    let (canonical_url, canonical_code) = canonical_ozon_product_url(&url)?;
    let mut last_error = String::new();
    for attempt in 0..2 {
        if competitor_task_stop_requested(id) {
            return Err("cancelled: 用户已停止此竞品任务".into());
        }
        let stage = if attempt == 0 { "direct" } else { "browser" };
        let task_message = if attempt == 0 {
            "正在后台读取公开商品页"
        } else {
            "浏览器已直达 Ozon 商品页，等待页面稳定或人工验证"
        };
        update_competitor_task(id, |task| {
            task.status = "running".into();
            task.stage = stage.into();
            task.message = task_message.into();
            task.retry_count = attempt;
            if task.started_at.is_empty() {
                task.started_at = chrono::Local::now().to_rfc3339();
            }
        });
        if let Ok(mut progress) = COMPETITOR_COLLECTION_PROGRESS.lock() {
            if progress.running {
                progress.stage = if attempt == 0 { "direct" } else { "browser" }.into();
                progress.message = if attempt == 0 {
                    format!("正在后台直连读取竞品 {}", canonical_code)
                } else {
                    format!(
                        "正在专用浏览器打开竞品 {}；如出现 Ozon 验证，完成后会自动继续",
                        canonical_code
                    )
                };
            }
        }
        let result = (|| -> Result<ParsedCompetitorPage, String> {
            let (html, source) = if attempt == 0 {
                let response = ureq::get(&canonical_url).set("User-Agent","Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/131 Safari/537.36").set("Accept-Language", "ru-RU,ru;q=0.9").timeout(std::time::Duration::from_secs(35)).call().map_err(|e|format!("竞品页面直连读取失败：{e}"))?;
                let final_url = response.get_url().to_string();
                validate_competitor_url_identity(&final_url, &canonical_code)?;
                (
                    response.into_string().map_err(|e| e.to_string())?,
                    "ozon_public_page",
                )
            } else {
                (
                    collect_competitor_browser_html(&canonical_url, &canonical_code, data_dir, id)?,
                    "dedicated_browser_rust",
                )
            };
            validate_competitor_identity(&html, &canonical_code)?;
            let parsed = parse_competitor_html(&html)?;
            if attempt == 0 && competitor_page_needs_browser(&parsed) {
                return Err("直连页面缺少可校验主图，转入专用浏览器读取可见页面".into());
            }
            save_competitor_html(c, id, &html, source)
        })();
        match result {
            Ok(parsed) => {
                if let Ok(mut progress) = COMPETITOR_COLLECTION_PROGRESS.lock() {
                    if progress.running {
                        progress.stage = "saving".into();
                        progress.message = format!("竞品 {} 已校验，正在写入快照", canonical_code);
                    }
                }
                // The competitor board only requires a verified main image.
                // Price and public sales remain optional evidence when present.
                let status = if !parsed.image.is_empty() {
                    "ok"
                } else {
                    "incomplete"
                };
                let notes = if status == "ok" {
                    ""
                } else {
                    "页面主图不完整；售价和销量仅在公开可核验时附带保存"
                };
                record_competitor_observation(
                    c,
                    run_id,
                    id,
                    status,
                    attempt,
                    &canonical_url,
                    if attempt == 0 {
                        "ozon_public_page"
                    } else {
                        "dedicated_browser_rust"
                    },
                    Some(&parsed),
                    notes,
                )?;
                update_competitor_task(id, |task| {
                    task.status = if status == "ok" {
                        "success"
                    } else {
                        "incomplete"
                    }
                    .into();
                    task.stage = "completed".into();
                    task.message = if status == "ok" {
                        "采集、校验并缓存成功"
                    } else {
                        "页面售价或主图不完整；销量可手工填写"
                    }
                    .into();
                    task.finished_at = chrono::Local::now().to_rfc3339();
                });
                return Ok(());
            }
            Err(error) => {
                last_error = error;
                let status = observation_status(&last_error);
                if attempt == 0 {
                    continue;
                }
                record_competitor_observation(
                    c,
                    run_id,
                    id,
                    status,
                    attempt,
                    &canonical_url,
                    "dedicated_browser_rust",
                    None,
                    &last_error,
                )?;
                update_competitor_task(id, |task| {
                    task.status = "failed".into();
                    task.stage = status.into();
                    task.message = last_error.clone();
                    task.finished_at = chrono::Local::now().to_rfc3339();
                });
                return Err(format!(
                    "{last_error}。已记录状态 {status}；可重新点击自动采集继续"
                ));
            }
        }
    }
    Err(last_error)
}
fn refresh_competitor_inner(c: &Connection, id: i64, data_dir: &Path) -> Result<(), String> {
    let run_id = begin_competitor_run(c, 1, "single_competitor")?;
    let result = refresh_competitor_for_run(c, id, &run_id, data_dir);
    finish_competitor_run(c, &run_id)?;
    result
}

#[tauri::command]
fn open_competitor_browser(id: i64, state: State<AppState>) -> Result<(), String> {
    let c = db(&state)?;
    let url = c
        .query_row(
            "SELECT product_url FROM competitor_products WHERE id=?1 AND active=1",
            [id],
            |r| r.get::<_, String>(0),
        )
        .map_err(|e| e.to_string())?;
    open::that(&url).map_err(|e| format!("无法打开系统浏览器：{e}"))
}

#[tauri::command]
fn set_competitor_manual_sales(
    id: i64,
    sales_total: Option<i64>,
    state: State<AppState>,
) -> Result<(), String> {
    if sales_total.is_some_and(|value| value < 0) {
        return Err("累计销量不能小于 0".into());
    }
    let c = db(&state)?;
    c.execute(
        "INSERT INTO competitor_snapshots(competitor_id,captured_at,price,sales_total,source) VALUES(?1,CURRENT_TIMESTAMP,(SELECT price FROM competitor_snapshots WHERE competitor_id=?1 ORDER BY captured_at DESC,id DESC LIMIT 1),?2,'manual_sales')",
        params![id, sales_total],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn import_competitor_html(id: i64, path: String, state: State<AppState>) -> Result<(), String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("请选择或粘贴验证后保存的 HTML 文件路径".into());
    }
    let html = fs::read_to_string(path).map_err(|e| format!("无法读取竞品 HTML：{e}"))?;
    let c = db(&state)?;
    let run_id = begin_competitor_run(&c, 1, "verified_browser_html")?;
    let result = (|| {
        let url = c
            .query_row(
                "SELECT product_url FROM competitor_products WHERE id=?1",
                [id],
                |r| r.get::<_, String>(0),
            )
            .map_err(|e| e.to_string())?;
        let (_, expected_code) = canonical_ozon_product_url(&url)?;
        validate_competitor_identity(&html, &expected_code)?;
        let parsed = save_competitor_html(&c, id, &html, "verified_browser_html")?;
        let status = if parsed.price.is_some() && parsed.sales.is_some() {
            "ok"
        } else {
            "incomplete"
        };
        record_competitor_observation(
            &c,
            &run_id,
            id,
            status,
            0,
            &url,
            "verified_browser_html",
            Some(&parsed),
            if status == "ok" {
                ""
            } else {
                "验证后页面仍缺少部分公开指标"
            },
        )
    })();
    if let Err(error) = &result {
        let url = c
            .query_row(
                "SELECT product_url FROM competitor_products WHERE id=?1",
                [id],
                |r| r.get::<_, String>(0),
            )
            .unwrap_or_default();
        let _ = record_competitor_observation(
            &c,
            &run_id,
            id,
            observation_status(error),
            0,
            &url,
            "verified_browser_html",
            None,
            error,
        );
    }
    finish_competitor_run(&c, &run_id)?;
    result
}
fn sales_delta(snapshots: &[CompetitorSnapshot], days: i64) -> Option<i64> {
    let latest = snapshots.last()?.sales_total?;
    let cutoff = chrono::Local::now().naive_local() - chrono::Duration::days(days);
    let old = snapshots
        .iter()
        .rev()
        .find(|s| {
            chrono::NaiveDateTime::parse_from_str(&s.captured_at, "%Y-%m-%d %H:%M:%S")
                .map(|d| d <= cutoff)
                .unwrap_or(false)
        })?
        .sales_total?;
    Some((latest - old).max(0))
}
#[tauri::command]
fn competitors(state: State<AppState>) -> Result<Vec<CompetitorRow>, String> {
    let c = db(&state)?;
    let alert_settings = competitor_alert_settings_from(&c);
    let mut stmt=c.prepare("SELECT p.id,p.product_url,p.product_code,p.name,p.image_url,COALESCE((SELECT status FROM competitor_observations o WHERE o.competitor_id=p.id ORDER BY o.id DESC LIMIT 1),''),COALESCE((SELECT observed_at FROM competitor_observations o WHERE o.competitor_id=p.id ORDER BY o.id DESC LIMIT 1),''),COALESCE((SELECT retry_count FROM competitor_observations o WHERE o.competitor_id=p.id ORDER BY o.id DESC LIMIT 1),0),COALESCE((SELECT notes FROM competitor_observations o WHERE o.competitor_id=p.id ORDER BY o.id DESC LIMIT 1),'') FROM competitor_products p WHERE p.active=1 ORDER BY p.id").map_err(|e|e.to_string())?;
    let base = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, i64>(7)?,
                r.get::<_, String>(8)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut out = vec![];
    for (
        id,
        url,
        code,
        name,
        image,
        latest_status,
        latest_observed_at,
        latest_retry_count,
        latest_notes,
    ) in base
    {
        let mut s=c.prepare("SELECT captured_at,price,sales_total FROM competitor_snapshots WHERE competitor_id=?1 ORDER BY captured_at").map_err(|e|e.to_string())?;
        let snaps = s
            .query_map([id], |r| {
                Ok(CompetitorSnapshot {
                    captured_at: r.get(0)?,
                    price: r.get(1)?,
                    sales_total: r.get(2)?,
                })
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        let latest = snaps.last().and_then(|x| x.price);
        let previous = latest.and_then(|current| {
            snaps
                .iter()
                .rev()
                .filter_map(|snapshot| snapshot.price)
                .find(|price| (*price - current).abs() > f64::EPSILON)
        });
        let price_change = latest.zip(previous).map(|(current, old)| current - old);
        let price_change_percent = latest
            .zip(previous)
            .and_then(|(current, old)| (old > 0.0).then_some((current - old) / old * 100.0));
        let cutoff = chrono::Local::now().naive_local() - chrono::Duration::days(30);
        let prices_30d = snaps
            .iter()
            .filter(|snapshot| {
                chrono::NaiveDateTime::parse_from_str(&snapshot.captured_at, "%Y-%m-%d %H:%M:%S")
                    .map(|captured| captured >= cutoff)
                    .unwrap_or(false)
            })
            .filter_map(|snapshot| snapshot.price)
            .collect::<Vec<_>>();
        let price_min_30d = prices_30d.iter().copied().reduce(f64::min);
        let price_max_30d = prices_30d.iter().copied().reduce(f64::max);
        let price_avg_30d = (!prices_30d.is_empty())
            .then(|| prices_30d.iter().sum::<f64>() / prices_30d.len() as f64);
        let price_changes_30d = prices_30d
            .windows(2)
            .filter(|pair| (pair[1] - pair[0]).abs() > f64::EPSILON)
            .count() as i64;
        let promotion_suspected = latest
            .zip(price_avg_30d)
            .is_some_and(|(current, average)| average > 0.0 && current <= average * 0.95);
        let (price_alert_level, price_alert_text) = match price_change_percent {
            Some(change) if change <= -alert_settings.critical_drop_percent => (
                "critical".to_string(),
                format!("竞品大幅降价 {change:.1}%，请检查利润底线与跟价风险"),
            ),
            Some(change) if change <= -alert_settings.warning_drop_percent => (
                "warning".to_string(),
                format!("竞品降价 {change:.1}%，建议关注转化和价格差"),
            ),
            Some(change) if change >= alert_settings.opportunity_rise_percent => (
                "opportunity".to_string(),
                format!("竞品涨价 {change:.1}%，可能出现价格空间"),
            ),
            Some(change) => ("stable".to_string(), format!("最近价格变化 {change:+.1}%")),
            None => ("pending".to_string(), "等待更多价格快照".to_string()),
        };
        let manual = c
            .query_row(
                "SELECT daily_sales,weekly_sales,monthly_sales FROM competitor_manual_metrics WHERE competitor_id=?1",
                [id],
                |r| Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Option<i64>>(1)?, r.get::<_, Option<i64>>(2)?)),
            )
            .ok();
        out.push(CompetitorRow {
            id,
            is_demo: url.starts_with("demo://") || c.query_row("SELECT EXISTS(SELECT 1 FROM competitor_snapshots WHERE competitor_id=?1 AND source='demo_seed')", [id], |r| r.get::<_, i64>(0)).unwrap_or(0) == 1,
            product_url: url,
            product_code: code,
            name,
            image_url: image,
            latest_price: latest,
            previous_price: previous,
            price_change,
            price_change_percent,
            price_min_30d,
            price_max_30d,
            price_avg_30d,
            price_alert_level,
            price_alert_text,
            price_changes_30d,
            promotion_suspected,
            daily_sales: manual.and_then(|x| x.0).or_else(|| sales_delta(&snaps, 1)),
            weekly_sales: manual.and_then(|x| x.1).or_else(|| sales_delta(&snaps, 7)),
            monthly_sales: manual.and_then(|x| x.2).or_else(|| sales_delta(&snaps, 30)),
            latest_status,
            latest_observed_at,
            latest_retry_count,
            latest_notes,
            snapshots: snaps,
        })
    }
    Ok(out)
}
fn add_competitor_blocking(product_url: String, state: &AppState) -> Result<i64, String> {
    let (product_url, _) = canonical_ozon_product_url(product_url.trim())?;
    let c = db(&state)?;
    c.execute("INSERT INTO competitor_products(product_url) VALUES(?1) ON CONFLICT(product_url) DO UPDATE SET active=1,updated_at=CURRENT_TIMESTAMP",[product_url.clone()]).map_err(|e|e.to_string())?;
    let id = c
        .query_row(
            "SELECT id FROM competitor_products WHERE product_url=?1",
            [product_url],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    Ok(id)
}
#[tauri::command]
async fn add_competitor(product_url: String, state: State<'_, AppState>) -> Result<i64, String> {
    let data_dir = state.data_dir.clone();
    let active_shop_id = state
        .active_shop_id
        .lock()
        .map_err(|_| "店铺状态锁异常")?
        .clone();
    tauri::async_runtime::spawn_blocking(move || {
        add_competitor_blocking(
            product_url,
            &AppState {
                data_dir,
                active_shop_id: Mutex::new(active_shop_id),
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}
fn refresh_competitor_blocking(id: i64, state: &AppState) -> Result<(), String> {
    let c = db(&state)?;
    refresh_competitor_inner(&c, id, &state.data_dir)
}
#[tauri::command]
async fn refresh_competitor(id: i64, state: State<'_, AppState>) -> Result<(), String> {
    let data_dir = state.data_dir.clone();
    let active_shop_id = state
        .active_shop_id
        .lock()
        .map_err(|_| "店铺状态锁异常")?
        .clone();
    tauri::async_runtime::spawn_blocking(move || {
        refresh_competitor_blocking(
            id,
            &AppState {
                data_dir,
                active_shop_id: Mutex::new(active_shop_id),
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}
fn competitor_tasks(c: &Connection, ids: &[i64]) -> Vec<CompetitorCollectionTask> {
    ids.iter()
        .filter_map(|id| {
            c.query_row(
                "SELECT product_code,product_url FROM competitor_products WHERE id=?1",
                [id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .ok()
            .map(|(product_code, product_url)| CompetitorCollectionTask {
                id: *id,
                product_code,
                product_url,
                status: "queued".into(),
                stage: "queued".into(),
                message: "等待采集".into(),
                ..Default::default()
            })
        })
        .collect()
}
fn refresh_competitors_due_blocking(state: &AppState) -> Result<i64, String> {
    {
        let progress = COMPETITOR_COLLECTION_PROGRESS
            .lock()
            .map_err(|_| "竞品采集进度锁异常")?;
        if progress.running {
            return Err("竞品采集任务已在运行".into());
        }
    }
    let c = db(&state)?;
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let mut stmt=c.prepare("SELECT p.id FROM competitor_products p WHERE p.active=1 AND NOT EXISTS(SELECT 1 FROM competitor_observations o WHERE o.competitor_id=p.id AND o.status='ok' AND substr(o.observed_at,1,10)=?1) ORDER BY p.id").map_err(|e|e.to_string())?;
    let ids = stmt
        .query_map([today], |r| r.get::<_, i64>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    if ids.is_empty() {
        return Ok(0);
    }
    COMPETITOR_COLLECTION_STOP.store(false, Ordering::SeqCst);
    if let Ok(mut stops) = COMPETITOR_TASK_STOPS.lock() {
        stops.clear();
    }
    let tasks = competitor_tasks(&c, &ids);
    if let Ok(mut progress) = COMPETITOR_COLLECTION_PROGRESS.lock() {
        *progress = CompetitorCollectionProgress {
            running: true,
            total: ids.len() as i64,
            stage: "preparing".into(),
            message: format!("今日待采集竞品 {} 个", ids.len()),
            tasks,
            ..Default::default()
        };
    }
    let mut count = 0;
    let mut first_error = None;
    let requested_scope = format!(
        "daily_due_competitors|ids={}",
        ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",")
    );
    let run_id = begin_competitor_run(&c, ids.len() as i64, &requested_scope)?;
    if let Ok(mut progress) = COMPETITOR_COLLECTION_PROGRESS.lock() {
        progress.run_id = run_id.clone();
    }
    for id in ids {
        if COMPETITOR_COLLECTION_STOP.load(Ordering::SeqCst) {
            break;
        }
        let code = c
            .query_row(
                "SELECT product_code FROM competitor_products WHERE id=?1",
                [id],
                |r| r.get::<_, String>(0),
            )
            .unwrap_or_default();
        if let Ok(mut progress) = COMPETITOR_COLLECTION_PROGRESS.lock() {
            progress.current_id = Some(id);
            progress.current_code = code;
        }
        if competitor_task_stop_requested(id) {
            update_competitor_task(id, |task| {
                task.status = "stopped".into();
                task.stage = "stopped".into();
                task.message = "已停止，未执行采集".into();
                task.stop_requested = true;
                task.finished_at = chrono::Local::now().to_rfc3339();
            });
            if let Ok(mut progress) = COMPETITOR_COLLECTION_PROGRESS.lock() {
                progress.completed += 1;
            }
            continue;
        }
        match refresh_competitor_for_run(&c, id, &run_id, &state.data_dir) {
            Ok(()) => {
                count += 1;
                if let Ok(mut progress) = COMPETITOR_COLLECTION_PROGRESS.lock() {
                    progress.succeeded += 1;
                }
            }
            Err(error) => {
                if error.starts_with("cancelled:") {
                    update_competitor_task(id, |task| {
                        task.status = "stopped".into();
                        task.stage = "stopped".into();
                        task.message = "任务已安全停止".into();
                        task.stop_requested = true;
                        task.finished_at = chrono::Local::now().to_rfc3339();
                    });
                }
                if !error.starts_with("cancelled:") {
                    if let Ok(mut progress) = COMPETITOR_COLLECTION_PROGRESS.lock() {
                        progress.failed += 1;
                    }
                }
                if first_error.is_none() && !error.starts_with("cancelled:") {
                    first_error = Some(error)
                }
            }
        }
        if let Ok(mut progress) = COMPETITOR_COLLECTION_PROGRESS.lock() {
            progress.completed += 1;
        }
    }
    finish_competitor_run(&c, &run_id)?;
    let stopped = COMPETITOR_COLLECTION_STOP.load(Ordering::SeqCst);
    if stopped {
        let _ = c.execute(
            "UPDATE competitor_collection_runs SET status='stopped',notes='用户停止采集' WHERE run_id=?1",
            [&run_id],
        );
    }
    if let Ok(mut progress) = COMPETITOR_COLLECTION_PROGRESS.lock() {
        progress.running = false;
        progress.current_id = None;
        progress.current_code.clear();
        progress.stop_requested = stopped;
        progress.stage = if stopped { "stopped" } else { "completed" }.into();
        progress.message = if stopped {
            format!(
                "已停止：完成 {}/{} 个任务",
                progress.completed, progress.total
            )
        } else {
            format!(
                "今日待采集任务完成：成功 {}，失败 {}",
                progress.succeeded, progress.failed
            )
        };
    }
    if count == 0 {
        if let Some(error) = first_error {
            return Err(error);
        }
    }
    Ok(count)
}
fn refresh_competitor_ids_blocking(
    state: &AppState,
    ids: Vec<i64>,
    workflow: &str,
) -> Result<i64, String> {
    let c = db(state)?;
    if ids.is_empty() {
        return Err("没有可重新运行的竞品任务".into());
    }
    if let Ok(mut stops) = COMPETITOR_TASK_STOPS.lock() {
        stops.clear();
    }
    let tasks = competitor_tasks(&c, &ids);
    {
        let mut progress = COMPETITOR_COLLECTION_PROGRESS
            .lock()
            .map_err(|_| "竞品采集进度锁异常")?;
        *progress = CompetitorCollectionProgress {
            running: true,
            total: ids.len() as i64,
            stage: "preparing".into(),
            message: format!("已建立 {} 个竞品采集任务", ids.len()),
            tasks,
            ..Default::default()
        };
    }
    COMPETITOR_COLLECTION_STOP.store(false, Ordering::SeqCst);
    let requested_scope = format!(
        "{workflow}|ids={}",
        ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",")
    );
    let run_id = begin_competitor_run(&c, ids.len() as i64, &requested_scope)?;
    if let Ok(mut progress) = COMPETITOR_COLLECTION_PROGRESS.lock() {
        progress.run_id = run_id.clone();
    }
    let mut ok = 0;
    let mut errors = Vec::new();
    for id in ids {
        if COMPETITOR_COLLECTION_STOP.load(Ordering::SeqCst) {
            break;
        }
        let code = c
            .query_row(
                "SELECT product_code FROM competitor_products WHERE id=?1",
                [id],
                |r| r.get::<_, String>(0),
            )
            .unwrap_or_default();
        if let Ok(mut progress) = COMPETITOR_COLLECTION_PROGRESS.lock() {
            progress.current_id = Some(id);
            progress.current_code = code.clone();
            progress.stage = "collecting".into();
            progress.message = format!(
                "正在采集 {}",
                if code.is_empty() {
                    id.to_string()
                } else {
                    code
                }
            );
        }
        if competitor_task_stop_requested(id) {
            update_competitor_task(id, |task| {
                task.status = "stopped".into();
                task.stage = "stopped".into();
                task.message = "已停止，未执行采集".into();
                task.stop_requested = true;
                task.finished_at = chrono::Local::now().to_rfc3339();
            });
            if let Ok(mut progress) = COMPETITOR_COLLECTION_PROGRESS.lock() {
                progress.completed += 1;
            }
            continue;
        }
        match refresh_competitor_for_run(&c, id, &run_id, &state.data_dir) {
            Ok(()) => {
                ok += 1;
                if let Ok(mut progress) = COMPETITOR_COLLECTION_PROGRESS.lock() {
                    progress.succeeded += 1;
                }
            }
            Err(e) => {
                if e.starts_with("cancelled:") {
                    update_competitor_task(id, |task| {
                        task.status = "stopped".into();
                        task.stage = "stopped".into();
                        task.message = "任务已安全停止".into();
                        task.stop_requested = true;
                        task.finished_at = chrono::Local::now().to_rfc3339();
                    });
                }
                if !e.starts_with("cancelled:") {
                    errors.push(e);
                    if let Ok(mut progress) = COMPETITOR_COLLECTION_PROGRESS.lock() {
                        progress.failed += 1;
                    }
                }
            }
        }
        if let Ok(mut progress) = COMPETITOR_COLLECTION_PROGRESS.lock() {
            progress.completed += 1;
        }
    }
    finish_competitor_run(&c, &run_id)?;
    let stopped = COMPETITOR_COLLECTION_STOP.load(Ordering::SeqCst);
    if stopped {
        let _ = c.execute(
            "UPDATE competitor_collection_runs SET status='stopped',notes=CASE WHEN notes='' THEN '用户停止采集' ELSE notes||'；用户停止采集' END WHERE run_id=?1",
            [&run_id],
        );
    }
    if !errors.is_empty() {
        let _ = c.execute(
            "UPDATE competitor_collection_runs SET notes=?2 WHERE run_id=?1",
            params![
                run_id,
                errors.into_iter().take(5).collect::<Vec<_>>().join("；")
            ],
        );
    }
    if let Ok(mut progress) = COMPETITOR_COLLECTION_PROGRESS.lock() {
        progress.running = false;
        progress.current_id = None;
        progress.current_code.clear();
        progress.stop_requested = stopped;
        progress.stage = if stopped { "stopped" } else { "completed" }.into();
        progress.message = if stopped {
            format!(
                "已停止：完成 {}/{} 个任务",
                progress.completed, progress.total
            )
        } else {
            format!(
                "采集完成：成功 {}，失败 {}",
                progress.succeeded, progress.failed
            )
        };
    }
    Ok(ok)
}

fn rebuild_cancellation_events(range: &DateRange, state: &AppState) -> Result<i64, String> {
    let mut c = db(state)?;
    let tx = c.transaction().map_err(|e| e.to_string())?;
    tx.execute(
        "DELETE FROM cancellation_events WHERE day BETWEEN ?1 AND ?2",
        params![range.from, range.to],
    )
    .map_err(|e| e.to_string())?;
    let written = tx.execute("INSERT INTO cancellation_events(event_id,day,sku,offer_id,product_name,quantity,scheme,source,updated_at) SELECT 'posting:'||event_id,day,sku,offer_id,product_name,quantity,scheme,'posting_status',CURRENT_TIMESTAMP FROM posting_routes WHERE day BETWEEN ?1 AND ?2 AND lower(status) LIKE '%cancel%'", params![range.from, range.to]).map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(written as i64)
}

fn sync_customer_returns_blocking(range: &DateRange, state: &AppState) -> Result<i64, String> {
    let mut c = db(state)?;
    let mut last_id = serde_json::Value::Number(0.into());
    let mut rows: Vec<(String, String, String, String, String, i64)> = Vec::new();
    loop {
        let response = seller_post(
            &c,
            "/v1/returns/list",
            &serde_json::json!({
                "filter":{"logistic_return_date":{"time_from":format!("{}T00:00:00Z",range.from),"time_to":format!("{}T23:59:59Z",range.to)}},
                "limit":500,"last_id":last_id
            }),
        )?;
        let items = response
            .get("returns")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        for item in &items {
            let return_type =
                json_text(item.get("type")).or_else_empty(|| json_text(item.get("return_type")));
            if !return_type.eq_ignore_ascii_case("ClientReturn") {
                continue;
            }
            let product = item
                .get("product")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            let logistic = item
                .get("logistic")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            let visual = item
                .get("visual")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            let raw_day = json_text(logistic.get("return_date"))
                .or_else_empty(|| json_text(logistic.get("final_moment")))
                .or_else_empty(|| json_text(visual.get("change_moment")));
            let day = raw_day.get(0..10).unwrap_or(&range.to).to_string();
            let sku = product
                .get("sku")
                .map(|v| {
                    v.as_i64()
                        .map(|x| x.to_string())
                        .unwrap_or_else(|| json_text(Some(v)))
                })
                .unwrap_or_default();
            let quantity = product
                .get("quantity")
                .and_then(|v| v.as_i64())
                .unwrap_or(1)
                .max(1);
            let return_id = item
                .get("id")
                .or_else(|| item.get("return_id"))
                .map(|v| {
                    v.as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| v.to_string())
                })
                .unwrap_or_else(|| {
                    format!("{}:{}:{}", json_text(item.get("posting_number")), sku, day)
                });
            rows.push((
                return_id,
                day,
                sku,
                json_text(product.get("offer_id")),
                json_text(product.get("name")),
                quantity,
            ));
        }
        if !response
            .get("has_next")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
            || items.is_empty()
        {
            break;
        }
        let next = items
            .last()
            .and_then(|v| v.get("id").or_else(|| v.get("return_id")))
            .cloned()
            .unwrap_or_else(|| last_id.clone());
        if next == last_id {
            break;
        }
        last_id = next;
    }
    let tx = c.transaction().map_err(|e| e.to_string())?;
    tx.execute(
        "DELETE FROM return_events WHERE day BETWEEN ?1 AND ?2",
        params![range.from, range.to],
    )
    .map_err(|e| e.to_string())?;
    for row in &rows {
        tx.execute("INSERT INTO return_events(return_id,day,sku,offer_id,product_name,quantity,source,updated_at)VALUES(?1,?2,?3,?4,?5,?6,'returns_api',CURRENT_TIMESTAMP)ON CONFLICT(return_id)DO UPDATE SET day=excluded.day,sku=excluded.sku,offer_id=excluded.offer_id,product_name=excluded.product_name,quantity=excluded.quantity,source='returns_api',updated_at=CURRENT_TIMESTAMP",params![row.0,row.1,row.2,row.3,row.4,row.5]).map_err(|e|e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(rows.len() as i64)
}

#[tauri::command]
fn seed_competitor_demo_data(state: State<AppState>) -> Result<i64, String> {
    let mut c = db(&state)?;
    let tx = c.transaction().map_err(|e| e.to_string())?;
    let demos = [
        ("DEMO-TOOLS-01", "防水工具收纳包 34cm", 649.0, 5_i64),
        ("DEMO-TOOLS-02", "加厚多功能维修工具袋", 719.0, 7_i64),
        ("DEMO-TOOLS-03", "便携式电工工具收纳箱", 899.0, 4_i64),
        ("DEMO-TOOLS-04", "大容量牛津布工具包", 579.0, 9_i64),
        ("DEMO-TOOLS-05", "专业级硬底工具提包", 1099.0, 3_i64),
        ("DEMO-TOOLS-06", "折叠式家用工具整理袋", 499.0, 11_i64),
    ];
    let today = chrono::Local::now().date_naive();
    for (index, (code, name, base_price, base_sales)) in demos.iter().enumerate() {
        let url = format!("demo://competitor/{code}");
        tx.execute(
            "INSERT INTO competitor_products(product_url,product_code,name,active,updated_at) VALUES(?1,?2,?3,1,CURRENT_TIMESTAMP) ON CONFLICT(product_url) DO UPDATE SET product_code=excluded.product_code,name=excluded.name,active=1,updated_at=CURRENT_TIMESTAMP",
            params![url, code, name],
        ).map_err(|e| e.to_string())?;
        let id: i64 = tx
            .query_row(
                "SELECT id FROM competitor_products WHERE product_url=?1",
                [url],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        tx.execute(
            "DELETE FROM competitor_snapshots WHERE competitor_id=?1 AND source='demo_seed'",
            [id],
        )
        .map_err(|e| e.to_string())?;
        let mut cumulative = 120 + index as i64 * 35;
        let mut daily_values = Vec::new();
        for day_offset in (0_i64..35).rev() {
            let day = today - chrono::Duration::days(day_offset);
            let wave = ((34 - day_offset + index as i64 * 2) % 7) - 3;
            let daily = (*base_sales + wave / 2 + if day_offset % 9 == 0 { 3 } else { 0 }).max(1);
            daily_values.push(daily);
            cumulative += daily;
            let promo = day_offset <= 5 && index % 2 == 1;
            let trend = (34 - day_offset) as f64 * (index as f64 - 2.0) * 0.7;
            let price = if promo {
                base_price * 0.88
            } else {
                base_price + trend + ((day_offset % 6) as f64 - 3.0) * 2.0
            };
            tx.execute(
                "INSERT INTO competitor_snapshots(competitor_id,captured_at,price,sales_total,source) VALUES(?1,?2,?3,?4,'demo_seed')",
                params![id, format!("{} 12:00:00", day.format("%Y-%m-%d")), price.round(), cumulative],
            ).map_err(|e| e.to_string())?;
        }
        let daily = *daily_values.last().unwrap_or(base_sales);
        let weekly: i64 = daily_values.iter().rev().take(7).sum();
        let monthly: i64 = daily_values.iter().rev().take(30).sum();
        tx.execute(
            "INSERT INTO competitor_manual_metrics(competitor_id,daily_sales,weekly_sales,monthly_sales,updated_at) VALUES(?1,?2,?3,?4,CURRENT_TIMESTAMP) ON CONFLICT(competitor_id) DO UPDATE SET daily_sales=excluded.daily_sales,weekly_sales=excluded.weekly_sales,monthly_sales=excluded.monthly_sales,updated_at=CURRENT_TIMESTAMP",
            params![id, daily, weekly, monthly],
        ).map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(demos.len() as i64)
}

#[tauri::command]
fn delete_competitor_demo_data(state: State<AppState>) -> Result<i64, String> {
    let mut c = db(&state)?;
    let tx = c.transaction().map_err(|e| e.to_string())?;
    let ids = {
        let mut stmt = tx.prepare("SELECT DISTINCT p.id FROM competitor_products p JOIN competitor_snapshots s ON s.competitor_id=p.id WHERE p.product_url LIKE 'demo://%' OR s.source='demo_seed'").map_err(|e| e.to_string())?;
        let values = stmt
            .query_map([], |r| r.get::<_, i64>(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        values
    };
    for id in &ids {
        let has_real: i64 = tx.query_row("SELECT EXISTS(SELECT 1 FROM competitor_snapshots WHERE competitor_id=?1 AND source<>'demo_seed')", [id], |r| r.get(0)).unwrap_or(0);
        tx.execute(
            "DELETE FROM competitor_snapshots WHERE competitor_id=?1 AND source='demo_seed'",
            [id],
        )
        .map_err(|e| e.to_string())?;
        if has_real == 0 {
            tx.execute(
                "DELETE FROM competitor_manual_metrics WHERE competitor_id=?1",
                [id],
            )
            .map_err(|e| e.to_string())?;
            tx.execute(
                "DELETE FROM competitor_observations WHERE competitor_id=?1",
                [id],
            )
            .map_err(|e| e.to_string())?;
            tx.execute("DELETE FROM competitor_products WHERE id=?1", [id])
                .map_err(|e| e.to_string())?;
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(ids.len() as i64)
}

fn active_competitor_ids(state: &AppState) -> Result<Vec<i64>, String> {
    let c = db(state)?;
    let mut stmt = c
        .prepare("SELECT id FROM competitor_products WHERE active=1 ORDER BY id")
        .map_err(|e| e.to_string())?;
    let ids = stmt
        .query_map([], |r| r.get::<_, i64>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(ids)
}

fn refresh_competitors_all_blocking(state: &AppState) -> Result<i64, String> {
    let ids = active_competitor_ids(state)?;
    refresh_competitor_ids_blocking(state, ids, "manual_all_competitors")
}
#[tauri::command]
async fn refresh_competitors_all(state: State<'_, AppState>) -> Result<i64, String> {
    let owned = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || refresh_competitors_all_blocking(&owned))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn start_competitors_collection(state: State<AppState>) -> Result<(), String> {
    let owned = background_state(&state)?;
    let ids = active_competitor_ids(&owned)?;
    start_competitor_ids_collection(
        owned,
        ids,
        "manual_all_competitors",
        "竞品采集任务已加入队列",
    )
}

#[tauri::command]
fn start_competitor_collection_task(id: i64, state: State<AppState>) -> Result<(), String> {
    let owned = background_state(&state)?;
    let exists = db(&owned)?
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM competitor_products WHERE id=?1 AND active=1)",
            [id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|e| e.to_string())?;
    if exists == 0 {
        return Err("竞品任务不存在或已经删除".into());
    }
    start_competitor_ids_collection(
        owned,
        vec![id],
        "manual_single_competitor",
        "单个竞品采集任务已加入队列",
    )
}

fn start_competitor_ids_collection(
    owned: AppState,
    ids: Vec<i64>,
    workflow: &'static str,
    queued_message: &str,
) -> Result<(), String> {
    {
        let progress = COMPETITOR_COLLECTION_PROGRESS
            .lock()
            .map_err(|_| "竞品采集进度锁异常")?;
        if progress.running {
            return Err("竞品采集任务已在运行".into());
        }
    }
    if ids.is_empty() {
        return Err("没有可运行的竞品任务".into());
    }
    COMPETITOR_COLLECTION_STOP.store(false, Ordering::SeqCst);
    if let Ok(mut progress) = COMPETITOR_COLLECTION_PROGRESS.lock() {
        *progress = CompetitorCollectionProgress {
            running: true,
            stage: "queued".into(),
            message: queued_message.into(),
            ..Default::default()
        };
    }
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(error) = refresh_competitor_ids_blocking(&owned, ids, workflow) {
            if let Ok(mut progress) = COMPETITOR_COLLECTION_PROGRESS.lock() {
                progress.running = false;
                progress.stage = "failed".into();
                progress.message = error;
            }
        }
    });
    Ok(())
}

fn competitor_alert_settings_from(c: &Connection) -> CompetitorAlertSettings {
    let number = |key: &str, fallback: f64| {
        setting(c, key)
            .parse::<f64>()
            .ok()
            .filter(|value| *value > 0.0 && *value <= 100.0)
            .unwrap_or(fallback)
    };
    CompetitorAlertSettings {
        warning_drop_percent: number("competitor_warning_drop_percent", 5.0),
        critical_drop_percent: number("competitor_critical_drop_percent", 10.0),
        opportunity_rise_percent: number("competitor_opportunity_rise_percent", 5.0),
    }
}

#[tauri::command]
fn competitor_alert_settings(state: State<AppState>) -> Result<CompetitorAlertSettings, String> {
    Ok(competitor_alert_settings_from(&db(&state)?))
}

#[tauri::command]
fn save_competitor_alert_settings(
    input: CompetitorAlertSettings,
    state: State<AppState>,
) -> Result<(), String> {
    if input.warning_drop_percent <= 0.0
        || input.critical_drop_percent <= input.warning_drop_percent
        || input.opportunity_rise_percent <= 0.0
        || input.critical_drop_percent > 100.0
        || input.opportunity_rise_percent > 100.0
    {
        return Err("预警阈值无效：严重降价必须大于普通降价，所有阈值需在 0–100% 之间".into());
    }
    let mut c = db(&state)?;
    let tx = c.transaction().map_err(|e| e.to_string())?;
    save_setting(
        &tx,
        "competitor_warning_drop_percent",
        &input.warning_drop_percent.to_string(),
    )?;
    save_setting(
        &tx,
        "competitor_critical_drop_percent",
        &input.critical_drop_percent.to_string(),
    )?;
    save_setting(
        &tx,
        "competitor_opportunity_rise_percent",
        &input.opportunity_rise_percent.to_string(),
    )?;
    tx.commit().map_err(|e| e.to_string())
}

#[tauri::command]
fn set_competitor_manual_metrics(
    id: i64,
    daily_sales: Option<i64>,
    weekly_sales: Option<i64>,
    monthly_sales: Option<i64>,
    state: State<AppState>,
) -> Result<(), String> {
    if [daily_sales, weekly_sales, monthly_sales]
        .into_iter()
        .flatten()
        .any(|value| value < 0)
    {
        return Err("日、周、月销量不能小于 0".into());
    }
    let c = db(&state)?;
    c.execute(
        "INSERT INTO competitor_manual_metrics(competitor_id,daily_sales,weekly_sales,monthly_sales,updated_at)VALUES(?1,?2,?3,?4,CURRENT_TIMESTAMP)ON CONFLICT(competitor_id)DO UPDATE SET daily_sales=excluded.daily_sales,weekly_sales=excluded.weekly_sales,monthly_sales=excluded.monthly_sales,updated_at=CURRENT_TIMESTAMP",
        params![id, daily_sales, weekly_sales, monthly_sales],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn latest_competitor_batch_ids(state: &AppState, failed_only: bool) -> Result<Vec<i64>, String> {
    let c = db(state)?;
    let (run_id, requested_scope): (String, String) = c
        .query_row(
            "SELECT run_id,requested_scope FROM competitor_collection_runs ORDER BY started_at DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => "尚无可重新运行的竞品批次".into(),
            _ => e.to_string(),
        })?;
    if !failed_only {
        if let Some(raw_ids) = requested_scope.split("|ids=").nth(1) {
            let ids = raw_ids
                .split(',')
                .filter_map(|value| value.parse::<i64>().ok())
                .collect::<Vec<_>>();
            if !ids.is_empty() {
                return Ok(ids);
            }
        }
    }
    let sql = if failed_only {
        "SELECT competitor_id FROM competitor_observations WHERE run_id=?1 AND status<>'ok' ORDER BY id"
    } else {
        "SELECT competitor_id FROM competitor_observations WHERE run_id=?1 ORDER BY id"
    };
    let mut stmt = c.prepare(sql).map_err(|e| e.to_string())?;
    let ids = stmt
        .query_map([run_id], |row| row.get::<_, i64>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(ids)
}

#[tauri::command]
fn rerun_competitors_collection(state: State<AppState>) -> Result<(), String> {
    let owned = background_state(&state)?;
    let ids = latest_competitor_batch_ids(&owned, false)?;
    start_competitor_ids_collection(
        owned,
        ids,
        "rerun_competitor_batch",
        "正在重新运行最近一批竞品任务",
    )
}

#[tauri::command]
fn retry_failed_competitors_collection(state: State<AppState>) -> Result<(), String> {
    let owned = background_state(&state)?;
    let ids = latest_competitor_batch_ids(&owned, true)?;
    start_competitor_ids_collection(
        owned,
        ids,
        "retry_failed_competitors",
        "正在重试失败或数据不完整的竞品任务",
    )
}

#[tauri::command]
fn competitor_collection_progress() -> Result<CompetitorCollectionProgress, String> {
    COMPETITOR_COLLECTION_PROGRESS
        .lock()
        .map(|progress| progress.clone())
        .map_err(|_| "竞品采集进度锁异常".into())
}

#[tauri::command]
fn stop_competitors_collection() -> Result<(), String> {
    COMPETITOR_COLLECTION_STOP.store(true, Ordering::SeqCst);
    let mut progress = COMPETITOR_COLLECTION_PROGRESS
        .lock()
        .map_err(|_| "竞品采集进度锁异常")?;
    if progress.running {
        progress.stop_requested = true;
        progress.stage = "stopping".into();
        progress.message = "已收到停止请求，正在安全结束当前步骤".into();
        for task in &mut progress.tasks {
            if matches!(task.status.as_str(), "queued" | "running") {
                task.stop_requested = true;
                task.status = if task.status == "running" {
                    "stopping".into()
                } else {
                    "stopped".into()
                };
                task.stage = task.status.clone();
                task.message = if task.status == "stopping" {
                    "正在安全结束当前步骤"
                } else {
                    "批次已停止，任务未执行"
                }
                .into();
            }
        }
    }
    Ok(())
}

#[tauri::command]
fn stop_competitor_collection_task(id: i64) -> Result<(), String> {
    COMPETITOR_TASK_STOPS
        .lock()
        .map_err(|_| "竞品任务停止状态锁异常")?
        .insert(id);
    let mut progress = COMPETITOR_COLLECTION_PROGRESS
        .lock()
        .map_err(|_| "竞品采集进度锁异常")?;
    let task = progress
        .tasks
        .iter_mut()
        .find(|task| task.id == id)
        .ok_or_else(|| "未找到对应的竞品采集任务".to_string())?;
    if matches!(
        task.status.as_str(),
        "success" | "failed" | "incomplete" | "stopped"
    ) {
        return Ok(());
    }
    task.stop_requested = true;
    if task.status == "queued" {
        task.status = "stopped".into();
        task.stage = "stopped".into();
        task.message = "已停止，轮到该任务时会直接跳过".into();
    } else {
        task.status = "stopping".into();
        task.stage = "stopping".into();
        task.message = "已收到停止请求，正在安全结束当前步骤".into();
    }
    Ok(())
}

#[tauri::command]
fn competitor_latest_run(state: State<AppState>) -> Result<Option<CompetitorRunSummary>, String> {
    let c = db(&state)?;
    let result=c.query_row("SELECT run_id,started_at,finished_at,requested,completed,ok,blocked,changed_layout,inaccessible,ambiguous_match,incomplete,status,notes FROM competitor_collection_runs ORDER BY started_at DESC LIMIT 1",[],|r|Ok(CompetitorRunSummary{run_id:r.get(0)?,started_at:r.get(1)?,finished_at:r.get(2)?,requested:r.get(3)?,completed:r.get(4)?,ok:r.get(5)?,blocked:r.get(6)?,changed_layout:r.get(7)?,inaccessible:r.get(8)?,ambiguous_match:r.get(9)?,incomplete:r.get(10)?,status:r.get(11)?,notes:r.get(12)?}));
    match result {
        Ok(row) => Ok(Some(row)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}
#[tauri::command]
async fn refresh_competitors_due(state: State<'_, AppState>) -> Result<i64, String> {
    let data_dir = state.data_dir.clone();
    let active_shop_id = state
        .active_shop_id
        .lock()
        .map_err(|_| "店铺状态锁异常")?
        .clone();
    tauri::async_runtime::spawn_blocking(move || {
        refresh_competitors_due_blocking(&AppState {
            data_dir,
            active_shop_id: Mutex::new(active_shop_id),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
fn remove_competitor(id: i64, state: State<AppState>) -> Result<(), String> {
    let c = db(&state)?;
    c.execute(
        "UPDATE competitor_products SET active=0,updated_at=CURRENT_TIMESTAMP WHERE id=?1",
        [id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn finance_service_category(name: &str) -> (&'static str, &'static str) {
    let n = name.to_lowercase();
    if [
        "promotion",
        "costperclick",
        "externalpromotion",
        "review",
        "premium",
        "subscription",
        "cashback",
        "advert",
        "реклам",
        "продвиж",
    ]
    .iter()
    .any(|x| n.contains(x))
    {
        ("advertising", "广告推广")
    } else if ["returnflow", "returnspvz", "reverse", "возврат"]
        .iter()
        .any(|x| n.contains(x))
    {
        ("return_logistics", "退货物流")
    } else if ["lastmile", "last_mile", "handoverplace", "courier"]
        .iter()
        .any(|x| n.contains(x))
    {
        // The monthly card has always defined 配送费用 as direct logistics
        // plus last-mile/handover services. Keep one category key so its
        // drill-down rows reconcile exactly to the card total.
        ("delivery", "配送物流")
    } else if ["acquiring", "payment", "эквайр"]
        .iter()
        .any(|x| n.contains(x))
    {
        ("acquiring", "收单支付")
    } else if [
        "storage",
        "package",
        "supply",
        "disposal",
        "warehouse",
        "fbo",
        "crossdock",
        "replenishment",
    ]
    .iter()
    .any(|x| n.contains(x))
    {
        ("storage", "仓储包装")
    } else if ["logistic", "delivery", "достав"]
        .iter()
        .any(|x| n.contains(x))
    {
        ("delivery", "配送物流")
    } else if [
        "penalt",
        "compensation",
        "shortage",
        "surplus",
        "insurance",
        "штраф",
        "компенсац",
    ]
    .iter()
    .any(|x| n.contains(x))
    {
        ("penalties", "罚款与调整")
    } else {
        ("other", "其他 Finance 项目")
    }
}

fn value_number(value: Option<&serde_json::Value>) -> f64 {
    value
        .and_then(|v| v.as_f64())
        .or_else(|| value.and_then(|v| v.as_str()).and_then(|s| s.parse().ok()))
        .unwrap_or(0.0)
}

#[tauri::command]
fn finance_breakdown(
    range: DateRange,
    state: State<AppState>,
) -> Result<Vec<FinanceBreakdownRow>, String> {
    let c = db(&state)?;
    let mut stmt=c.prepare("SELECT operation_id,raw_json FROM finance_transactions WHERE substr(operation_date,1,10) BETWEEN ?1 AND ?2 ORDER BY operation_date,operation_id").map_err(|e|e.to_string())?;
    let raw = stmt
        .query_map(params![range.from, range.to], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut grouped = std::collections::HashMap::<String, FinanceBreakdownRow>::new();
    let mut add = |category: &str, label: &str, name: String, api_name: String, amount: f64| {
        if amount.abs() < 0.005 {
            return;
        }
        let key = format!("{category}|{name}|{api_name}");
        let row = grouped.entry(key).or_insert(FinanceBreakdownRow {
            category: category.into(),
            category_label: label.into(),
            name,
            api_name,
            rows_count: 0,
            amount: 0.0,
        });
        row.rows_count += 1;
        row.amount += amount;
    };
    for (_, text) in raw {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let amount = value_number(v.get("amount"));
        let accrual = value_number(v.get("accruals_for_sale"));
        let commission = value_number(v.get("sale_commission"));
        let op = json_text(v.get("operation_type"));
        let op_name = {
            let x = json_text(v.get("operation_type_name"));
            if x.is_empty() {
                op.clone()
            } else {
                x
            }
        };
        add(
            if accrual >= 0.0 { "sales" } else { "returns" },
            if accrual >= 0.0 {
                "销售应计"
            } else {
                "销售退货"
            },
            if accrual >= 0.0 {
                "商品销售应计".into()
            } else {
                "商品退货/退款".into()
            },
            "accruals_for_sale".into(),
            accrual,
        );
        add(
            "commission",
            "平台佣金",
            "Ozon 销售佣金".into(),
            "sale_commission".into(),
            commission,
        );
        let mut service_total = 0.0;
        for service in v
            .get("services")
            .and_then(|x| x.as_array())
            .into_iter()
            .flatten()
        {
            let name = json_text(service.get("name"));
            let price = value_number(service.get("price"));
            service_total += price;
            let (category, label) = finance_service_category(&name);
            add(category, label, name.clone(), name, price);
        }
        let residual = amount - accrual - commission - service_total;
        let (category, label) = finance_service_category(&format!("{op} {op_name}"));
        add(category, label, op_name, op, residual);
    }
    let mut result = grouped.into_values().collect::<Vec<_>>();
    result.sort_by(|a, b| b.amount.abs().total_cmp(&a.amount.abs()));
    Ok(result)
}

#[tauri::command]
fn data_coverage(state: State<AppState>) -> Result<Vec<DataCoverageRow>, String> {
    let c = db(&state)?;
    let specs = [
        ("Seller 销量", "sales_daily", "day", "Seller Analytics"),
        ("Performance 广告", "ad_daily", "day", "Performance Ads"),
        (
            "Finance 结算",
            "finance_transactions",
            "substr(operation_date,1,10)",
            "Seller Finance",
        ),
    ];
    let mut out = Vec::new();
    for (source, table, date_col, log_source) in specs {
        let sql=format!("SELECT COUNT(*),COALESCE(MIN({date_col}),''),COALESCE(MAX({date_col}),'') FROM {table}");
        let (rows_count, date_from, date_to): (i64, String, String) = c
            .query_row(&sql, [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(|e| e.to_string())?;
        let last_success=c.query_row("SELECT COALESCE(MAX(finished_at),'')FROM sync_logs WHERE source=?1 AND status='success'",[log_source],|r|r.get(0)).unwrap_or_default();
        out.push(DataCoverageRow {
            source: source.into(),
            rows_count,
            date_from,
            date_to,
            last_success,
        });
    }
    Ok(out)
}

#[tauri::command]
fn prune_cache(before: String, state: State<AppState>) -> Result<i64, String> {
    chrono::NaiveDate::parse_from_str(&before, "%Y-%m-%d")
        .map_err(|_| "清理截止日期无效".to_string())?;
    let mut c = db(&state)?;
    let tx = c.transaction().map_err(|e| e.to_string())?;
    let mut deleted = 0_i64;
    deleted += tx
        .execute("DELETE FROM sales_daily WHERE day<?1", [&before])
        .map_err(|e| e.to_string())? as i64;
    deleted += tx
        .execute("DELETE FROM ad_daily WHERE day<?1", [&before])
        .map_err(|e| e.to_string())? as i64;
    deleted += tx
        .execute(
            "DELETE FROM finance_transactions WHERE substr(operation_date,1,10)<?1",
            [&before],
        )
        .map_err(|e| e.to_string())? as i64;
    tx.execute("DELETE FROM business_report_cache", []).ok();
    tx.execute("DELETE FROM analytics_detail_cache", []).ok();
    tx.commit().map_err(|e| e.to_string())?;
    Ok(deleted)
}

#[tauri::command]
fn missing_cost_rows(
    range: DateRange,
    state: State<AppState>,
) -> Result<Vec<MissingCostRow>, String> {
    let c = db(&state)?;
    // Finance is the canonical settled-unit source for the Finance P&L. A
    // posting event is only a fallback when that SKU has no attributable
    // Finance delivery, because migrated posting history can be partial.
    let mut stmt = c.prepare("WITH sales AS(SELECT sku,MAX(product_name) product_name FROM sales_daily WHERE day BETWEEN ?1 AND ?2 GROUP BY sku),delivered AS(SELECT sku,SUM(quantity) units FROM delivery_events WHERE day BETWEEN ?1 AND ?2 GROUP BY sku),finance_delivered AS(SELECT sku,COUNT(DISTINCT operation_id) units FROM finance_transactions WHERE substr(operation_date,1,10) BETWEEN ?1 AND ?2 AND sku<>'' AND lower(operation_type) LIKE '%deliveredtocustomer%' GROUP BY sku),base AS(SELECT s.sku,s.product_name,COALESCE(fd.units,d.units,0) units FROM sales s LEFT JOIN delivered d ON d.sku=s.sku LEFT JOIN finance_delivered fd ON fd.sku=s.sku) SELECT b.sku,COALESCE(MAX(p.offer_id),''),COALESCE(MAX(NULLIF(p.name,'')),MAX(b.product_name),''),MAX(b.units),MAX(COALESCE(pc.unit_cost_cny,pc.unit_cost)),MAX(COALESCE(pc.first_mile_cost,pc.first_mile_cost_cny)),MAX(pc.weight_kg),MAX(pc.length_cm),MAX(pc.width_cm),MAX(pc.height_cm),COALESCE(MAX(pc.note),'') FROM base b LEFT JOIN products p ON p.sku=b.sku LEFT JOIN product_costs pc ON pc.sku=b.sku WHERE b.units>0 GROUP BY b.sku HAVING MAX(COALESCE(pc.unit_cost_cny,pc.unit_cost)) IS NULL OR MAX(COALESCE(pc.first_mile_cost,pc.first_mile_cost_cny)) IS NULL ORDER BY MAX(b.units) DESC,b.sku").map_err(|e|e.to_string())?;
    let rows = stmt
        .query_map(params![range.from, range.to], |r| {
            let length: Option<f64> = r.get(7)?;
            let width: Option<f64> = r.get(8)?;
            let height: Option<f64> = r.get(9)?;
            let unit_cost: Option<f64> = r.get(4)?;
            let first_mile_cost: Option<f64> = r.get(5)?;
            let weight_kg: Option<f64> = r.get(6)?;
            Ok(MissingCostRow {
                sku: r.get(0)?,
                offer_id: r.get(1)?,
                product_name: r.get(2)?,
                units: r.get(3)?,
                missing_purchase: unit_cost.is_none(),
                missing_first_mile: first_mile_cost.is_none(),
                missing_weight: weight_kg.is_none(),
                missing_dimensions: length.is_none() || width.is_none() || height.is_none(),
                unit_cost,
                first_mile_cost,
                weight_kg,
                length_cm: length,
                width_cm: width,
                height_cm: height,
                note: r.get(10)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

#[tauri::command]
fn business_report(range: DateRange, state: State<AppState>) -> Result<BusinessReport, String> {
    let c = db(&state)?;
    let rate = rub_per_cny_for(&state, &c)?;
    c.execute_batch("CREATE TABLE IF NOT EXISTS business_report_cache(range_key TEXT PRIMARY KEY,fingerprint TEXT NOT NULL,payload TEXT NOT NULL,updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);").map_err(|e|e.to_string())?;
    let fingerprint:String=c.query_row("SELECT 'finance-v6-finance-delivered-first|'||printf('%d|%s|%d|%d|%d|%d',COALESCE((SELECT MAX(id)FROM sync_logs WHERE status='success' AND source IN('Seller Analytics','Seller Finance','Performance Ads')),0),COALESCE((SELECT MAX(updated_at)FROM product_costs),''),(SELECT COUNT(*)FROM sales_daily),(SELECT COUNT(*)FROM delivery_events),(SELECT COUNT(*)FROM finance_transactions),(SELECT COUNT(*)FROM ad_daily))",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    let cache_key = format!("{}|{}", range.from, range.to);
    if let Ok(payload) = c.query_row(
        "SELECT payload FROM business_report_cache WHERE range_key=?1 AND fingerprint=?2",
        params![cache_key, fingerprint],
        |r| r.get::<_, String>(0),
    ) {
        if let Ok(report) = serde_json::from_str(&payload) {
            return Ok(report);
        }
    }
    // Match the Finance-settled P&L: Finance delivered operations are the
    // canonical per-SKU quantity. Posting delivery events are a fallback only
    // when Finance has no attributable delivery row for that SKU.
    let(revenue,orders,purchase,first_mile,missing,costed_units,missing_cost_skus):(f64,i64,f64,f64,i64,i64,i64)=c.query_row("WITH sales AS(SELECT sku,SUM(revenue) revenue,SUM(ordered_units) ordered FROM sales_daily WHERE day BETWEEN ?1 AND ?2 GROUP BY sku),delivered AS(SELECT sku,SUM(quantity) delivered FROM delivery_events WHERE day BETWEEN ?1 AND ?2 GROUP BY sku),finance_delivered AS(SELECT sku,COUNT(DISTINCT operation_id) delivered FROM finance_transactions WHERE substr(operation_date,1,10) BETWEEN ?1 AND ?2 AND sku<>'' AND lower(operation_type) LIKE '%deliveredtocustomer%' GROUP BY sku),cost_base AS(SELECT s.sku,s.revenue,s.ordered,COALESCE(fd.delivered,d.delivered,0) cost_units,pc.unit_cost_cny,pc.unit_cost,pc.first_mile_cost,pc.first_mile_cost_cny FROM sales s LEFT JOIN delivered d ON d.sku=s.sku LEFT JOIN finance_delivered fd ON fd.sku=s.sku LEFT JOIN product_costs pc ON pc.sku=s.sku) SELECT COALESCE(SUM(revenue),0),COALESCE(SUM(ordered),0),COALESCE(SUM(cost_units*COALESCE(unit_cost_cny*?3,unit_cost,0)),0),COALESCE(SUM(cost_units*COALESCE(first_mile_cost,first_mile_cost_cny*?3,0)),0),COALESCE(SUM(CASE WHEN(unit_cost_cny IS NULL AND unit_cost IS NULL)OR(first_mile_cost IS NULL AND first_mile_cost_cny IS NULL)THEN cost_units ELSE 0 END),0),COALESCE(SUM(CASE WHEN(unit_cost_cny IS NOT NULL OR unit_cost IS NOT NULL)AND(first_mile_cost IS NOT NULL OR first_mile_cost_cny IS NOT NULL)THEN cost_units ELSE 0 END),0),COALESCE(COUNT(DISTINCT CASE WHEN cost_units>0 AND ((unit_cost_cny IS NULL AND unit_cost IS NULL)OR(first_mile_cost IS NULL AND first_mile_cost_cny IS NULL)) THEN sku END),0) FROM cost_base",params![range.from,range.to,rate],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).map_err(|e|e.to_string())?;
    let ad_spend = c
        .query_row(
            "SELECT COALESCE(SUM(spend),0) FROM ad_daily WHERE day BETWEEN ?1 AND ?2 AND sku=''",
            params![range.from, range.to],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let(finance_net,commission,finance_operations):(f64,f64,i64)=c.query_row("SELECT COALESCE(SUM(amount),0),COALESCE(SUM(sale_commission),0),COUNT(*) FROM finance_transactions WHERE substr(operation_date,1,10) BETWEEN ?1 AND ?2",params![range.from,range.to],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e|e.to_string())?;
    let mut delivery = 0.0;
    let mut returns = 0.0;
    let mut acquiring = 0.0;
    let mut storage = 0.0;
    let mut penalties = 0.0;
    let mut other = 0.0;
    let mut finance_advertising = 0.0;
    let mut sales_returns = 0.0;
    let mut exact_sku_operations = 0;
    let mut unallocated_operations = 0;
    let mut unallocated = 0.0;
    {
        let mut services=c.prepare("SELECT raw_json FROM finance_transactions WHERE substr(operation_date,1,10) BETWEEN ?1 AND ?2").map_err(|e|e.to_string())?;
        let raw = services
            .query_map(params![range.from, range.to], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        for text in raw {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                let operation_amount = value_number(value.get("amount"));
                let operation_accrual = value_number(value.get("accruals_for_sale"));
                let operation_commission = value_number(value.get("sale_commission"));
                sales_returns += operation_accrual;
                let item_skus = value
                    .get("items")
                    .and_then(|v| v.as_array())
                    .into_iter()
                    .flatten()
                    .filter_map(|item| {
                        let sku = json_text(item.get("sku"));
                        (!sku.is_empty()).then_some(sku)
                    })
                    .collect::<std::collections::BTreeSet<_>>();
                if item_skus.len() == 1 {
                    exact_sku_operations += 1;
                } else {
                    unallocated_operations += 1;
                    unallocated += operation_amount;
                }
                let mut service_total = 0.0;
                for service in value
                    .get("services")
                    .and_then(|v| v.as_array())
                    .into_iter()
                    .flatten()
                {
                    let name = json_text(service.get("name")).to_lowercase();
                    let price = service
                        .get("price")
                        .and_then(|v| v.as_f64())
                        .or_else(|| json_text(service.get("price")).parse().ok())
                        .unwrap_or(0.0);
                    service_total += price;
                    match finance_service_category(&name).0 {
                        "return_logistics" => returns += price,
                        "delivery" | "last_mile" => delivery += price,
                        "acquiring" => acquiring += price,
                        "storage" => storage += price,
                        "penalties" => penalties += price,
                        "other" => other += price,
                        "advertising" => finance_advertising += price,
                        _ => {}
                    }
                }
                // Many Ozon Finance fees (CPC/CPO promotion, early payout,
                // Premium and adjustments) are operation-level amounts and do
                // not appear in `services[]`.  Classify the exact residual so
                // the category cards reconcile to the API identity:
                // amount = accrual + commission + services + residual.
                let residual =
                    operation_amount - operation_accrual - operation_commission - service_total;
                let operation_name = format!(
                    "{} {}",
                    json_text(value.get("operation_type")),
                    json_text(value.get("operation_type_name"))
                );
                match finance_service_category(&operation_name).0 {
                    "return_logistics" => returns += residual,
                    "delivery" | "last_mile" => delivery += residual,
                    "acquiring" => acquiring += residual,
                    "storage" => storage += residual,
                    "penalties" => penalties += residual,
                    "advertising" => finance_advertising += residual,
                    _ => other += residual,
                }
            }
        }
    }
    let mut other_adjustments = 0.0;
    {
        let mut stmt=c.prepare("SELECT raw_json FROM finance_cash_flow_details WHERE period_to>=?1 AND period_from<=?2").map_err(|e|e.to_string())?;
        let rows = stmt
            .query_map(params![range.from, range.to], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        for text in rows {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
            let Some(others) = value.get("others").and_then(|v| v.as_object()) else {
                continue;
            };
            if let Some(total) = others.get("total") {
                other_adjustments += value_number(Some(total));
            } else if let Some(items) = others.get("items").and_then(|v| v.as_array()) {
                other_adjustments += items
                    .iter()
                    .map(|item| value_number(item.get("price")))
                    .sum::<f64>();
            }
        }
    }
    let mut cash_flow_reported_total = 0.0;
    let mut cash_flow_rows = 0;
    {
        let mut stmt = c
            .prepare(
                "SELECT raw_json FROM finance_cash_flows WHERE period_to>=?1 AND period_from<=?2",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![range.from, range.to], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        for text in rows {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
            cash_flow_rows += 1;
            cash_flow_reported_total += [
                "orders_amount",
                "returns_amount",
                "commission_amount",
                "item_delivery_and_return_amount",
                "services_amount",
            ]
            .iter()
            .map(|key| value_number(value.get(*key)))
            .sum::<f64>();
        }
    }
    let accrual_fees = finance_net - sales_returns - other_adjustments;
    let mut stmt=c.prepare("SELECT s.day,SUM(s.revenue),SUM(s.ordered_units),COALESCE((SELECT SUM(a.spend) FROM ad_daily a WHERE a.day=s.day AND a.sku=''),0) FROM sales_daily s WHERE s.day BETWEEN ?1 AND ?2 GROUP BY s.day ORDER BY s.day").map_err(|e|e.to_string())?;
    let daily = stmt
        .query_map(params![range.from, range.to], |r| {
            Ok(ReportDay {
                day: r.get(0)?,
                revenue: r.get(1)?,
                orders: r.get(2)?,
                ad_spend: r.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let profit = revenue - ad_spend - purchase - first_mile
        + commission
        + delivery
        + returns
        + acquiring
        + storage
        + penalties
        + other;
    let tax_rate = setting(&c, "local_tax_rate")
        .parse::<f64>()
        .unwrap_or(3.0)
        .max(0.0);
    let payout_fee_rate = setting(&c, "local_payout_fee_rate")
        .parse::<f64>()
        .unwrap_or(10.0)
        .max(0.0);
    let tax_amount = -revenue.max(0.0) * tax_rate / 100.0;
    let payout_fee = -finance_net.max(0.0) * payout_fee_rate / 100.0;
    let settled_profit = if finance_operations > 0 && missing == 0 {
        Some(finance_net - purchase - first_mile)
    } else {
        None
    };
    let after_tax_profit = settled_profit.map(|v| v + tax_amount + payout_fee);
    let report = BusinessReport {
        revenue,
        orders,
        ad_spend,
        finance_net,
        sales_returns,
        accrual_fees,
        other_adjustments,
        commission,
        finance_advertising,
        delivery_fees: delivery,
        return_fees: returns,
        purchase_cost: purchase,
        first_mile_cost: first_mile,
        estimated_profit: profit,
        settled_profit,
        tax_rate,
        tax_amount,
        payout_fee_rate,
        payout_fee,
        after_tax_profit,
        acquiring,
        storage_packaging: storage,
        penalties_adjustments: penalties,
        other_finance_fees: other,
        unallocated_finance_amount: unallocated,
        finance_operations,
        exact_sku_operations,
        unallocated_operations,
        cash_flow_reported_total,
        reconciliation_difference: (cash_flow_rows > 0)
            .then_some(cash_flow_reported_total - finance_net),
        missing_cost_skus,
        costed_units,
        missing_cost_units: missing,
        daily,
    };
    let payload = serde_json::to_string(&report).map_err(|e| e.to_string())?;
    c.execute("INSERT INTO business_report_cache(range_key,fingerprint,payload,updated_at)VALUES(?1,?2,?3,CURRENT_TIMESTAMP)ON CONFLICT(range_key)DO UPDATE SET fingerprint=excluded.fingerprint,payload=excluded.payload,updated_at=CURRENT_TIMESTAMP",params![cache_key,fingerprint,payload]).map_err(|e|e.to_string())?;
    Ok(report)
}

fn cross_border_shipping(price: f64, weight: f64) -> Option<f64> {
    if price < 0.0 || weight < 0.0 {
        None
    } else if price < 135.0 && weight < 0.5 {
        Some(3.37 + weight * 28.17)
    } else if price >= 135.0 && price < 635.0 && weight < 2.0 {
        Some(17.97 + weight * 28.17)
    } else if price >= 635.0 && price < 22525.0 && weight < 5.0 {
        Some(24.17 + weight * 28.17)
    } else if price < 135.0 && weight >= 0.5 && weight < 30.0 {
        Some(25.83 + weight * 19.17)
    } else if price >= 135.0 && price < 635.0 && weight >= 2.0 && weight < 30.0 {
        Some(40.44 + weight * 28.17)
    } else {
        None
    }
}

fn cross_border_report_blocking(
    range: DateRange,
    state: &AppState,
) -> Result<CrossBorderReport, String> {
    use std::collections::{HashMap, HashSet};
    let c = db(state)?;
    let rate = rub_per_cny_for(state, &c)?;
    if rate <= 0.0 {
        return Err("人民币兑卢布汇率必须大于 0".into());
    }
    let (min_day,max_day):(String,String)=c.query_row("SELECT COALESCE(MIN(substr(operation_date,1,10)),''),COALESCE(MAX(substr(operation_date,1,10)),'')FROM finance_transactions",[],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|e|e.to_string())?;
    let cutoff = if !min_day.is_empty()
        && !max_day.is_empty()
        && chrono::NaiveDate::parse_from_str(&max_day, "%Y-%m-%d")
            .ok()
            .zip(chrono::NaiveDate::parse_from_str(&min_day, "%Y-%m-%d").ok())
            .map(|(a, b)| (a - b).num_days() >= 14)
            .unwrap_or(false)
    {
        chrono::NaiveDate::parse_from_str(&max_day, "%Y-%m-%d")
            .unwrap()
            .pred_opt()
            .unwrap()
            .checked_sub_days(chrono::Days::new(6))
            .unwrap()
            .format("%Y-%m-%d")
            .to_string()
    } else {
        max_day.clone()
    };
    let mut sales_shop = 0.0;
    let mut commission_shop = 0.0;
    let mut acquiring_shop = 0.0;
    let mut sku_sales: HashMap<String, f64> = HashMap::new();
    let mut sku_commission: HashMap<String, f64> = HashMap::new();
    let mut fulfillment: HashMap<String, (i64, i64, i64, i64)> = HashMap::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut stmt=c.prepare("SELECT operation_date,posting_number,sku,accruals_for_sale,sale_commission,raw_json FROM finance_transactions").map_err(|e|e.to_string())?;
    let finance_rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, f64>(3)?,
                r.get::<_, f64>(4)?,
                r.get::<_, String>(5)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    for (day, posting, stored_sku, accrual, commission, text) in finance_rows {
        let v = serde_json::from_str::<serde_json::Value>(&text).unwrap_or_default();
        if day.get(..10).unwrap_or("") <= cutoff.as_str() {
            if accrual > 0.0 {
                sales_shop += accrual;
                if !stored_sku.is_empty() {
                    *sku_sales.entry(stored_sku.clone()).or_default() += accrual
                }
            }
            if commission < 0.0 {
                commission_shop += commission.abs();
                if !stored_sku.is_empty() {
                    *sku_commission.entry(stored_sku.clone()).or_default() += commission.abs()
                }
            }
            for service in v
                .get("services")
                .and_then(|x| x.as_array())
                .into_iter()
                .flatten()
            {
                let name = json_text(service.get("name"));
                let price = value_number(service.get("price"));
                if finance_service_category(&name).0 == "acquiring" && price < 0.0 {
                    acquiring_shop += price.abs()
                }
            }
        }
        let order_day = json_text(v.pointer("/posting/order_date"))
            .chars()
            .take(10)
            .collect::<String>();
        if order_day < range.from || order_day > range.to {
            continue;
        }
        let posting_no = {
            let x = json_text(v.pointer("/posting/posting_number"));
            if x.is_empty() {
                posting.clone()
            } else {
                x
            }
        };
        let raw_schema = json_text(v.pointer("/posting/delivery_schema")).to_uppercase();
        let schema = if raw_schema.contains("FBP") || raw_schema.contains("FBO") {
            "FBP"
        } else if raw_schema.contains("RFBS") || raw_schema.contains("FBS") {
            "RFBS"
        } else if raw_schema.contains("WHD") {
            "WHD"
        } else {
            "OTHER"
        };
        let mut skus = v
            .get("items")
            .and_then(|x| x.as_array())
            .into_iter()
            .flatten()
            .map(|x| json_text(x.get("sku")))
            .filter(|x| !x.is_empty())
            .collect::<Vec<_>>();
        if skus.is_empty() && !stored_sku.is_empty() {
            skus.push(stored_sku)
        }
        for sku in skus {
            if !seen.insert((posting_no.clone(), sku.clone())) {
                continue;
            }
            let e = fulfillment.entry(sku).or_default();
            e.0 += 1;
            match schema {
                "FBP" => e.1 += 1,
                "RFBS" => e.2 += 1,
                "WHD" => e.3 += 1,
                _ => {}
            }
        }
    }
    let shop_commission_rate = (sales_shop > 0.0 && commission_shop > 0.0)
        .then_some(commission_shop / sales_shop)
        .filter(|x| *x <= 0.60);
    let acquiring_rate = (sales_shop > 0.0 && acquiring_shop > 0.0)
        .then_some(acquiring_shop / sales_shop)
        .filter(|x| *x <= 0.10);
    let mut q=c.prepare("SELECT s.sku,COALESCE(MAX(p.offer_id),''),COALESCE(MAX(NULLIF(p.name,'')),MAX(s.product_name),''),SUM(s.ordered_units),SUM(s.revenue),pc.unit_cost_cny,pc.weight_kg,COALESCE((SELECT SUM(f.amount)FROM finance_transactions f WHERE f.sku=s.sku AND substr(f.operation_date,1,10)BETWEEN ?1 AND ?2),0),COALESCE((SELECT COUNT(*)FROM finance_transactions f WHERE f.sku=s.sku AND substr(f.operation_date,1,10)BETWEEN ?1 AND ?2),0)FROM sales_daily s LEFT JOIN products p ON p.sku=s.sku LEFT JOIN product_costs pc ON pc.sku=s.sku WHERE s.day BETWEEN ?1 AND ?2 GROUP BY s.sku HAVING SUM(s.ordered_units)>0 ORDER BY SUM(s.revenue)DESC").map_err(|e|e.to_string())?;
    let base = q
        .query_map(params![range.from, range.to], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, f64>(4)?,
                r.get::<_, Option<f64>>(5)?,
                r.get::<_, Option<f64>>(6)?,
                r.get::<_, f64>(7)?,
                r.get::<_, i64>(8)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut rows = Vec::new();
    let mut total_fees = 0.0;
    let mut purchase_freight = 0.0;
    let mut missing = 0;
    let mut fbp = 0;
    let mut rfbs = 0;
    let mut whd = 0;
    for (
        sku,
        offer_id,
        product_name,
        units,
        revenue,
        unit_cost,
        weight,
        finance_net,
        finance_count,
    ) in base
    {
        let selling_rub = revenue / units as f64;
        let selling_cny = selling_rub / rate;
        let freight_unit = weight.and_then(|w| cross_border_shipping(selling_cny, w));
        let purchase_total = unit_cost.map(|x| x * units as f64);
        let freight_total = freight_unit.map(|x| x * units as f64);
        let commission_rate = sku_sales
            .get(&sku)
            .copied()
            .filter(|x| *x > 0.0)
            .and_then(|sales| sku_commission.get(&sku).copied().map(|fee| fee / sales))
            .filter(|x| *x <= 0.60)
            .or(shop_commission_rate);
        let fees = commission_rate
            .zip(acquiring_rate)
            .map(|(a, b)| -revenue * (a + b) / rate);
        let complete = unit_cost.is_some() && freight_unit.is_some() && fees.is_some();
        let contribution = complete.then(|| {
            revenue / rate + fees.unwrap() - purchase_total.unwrap() - freight_total.unwrap()
        });
        if !complete {
            missing += 1
        }
        if let Some(x) = fees {
            total_fees += x
        }
        if let (Some(a), Some(b)) = (purchase_total, freight_total) {
            purchase_freight += a + b
        }
        let fulf = fulfillment.get(&sku).copied().unwrap_or_default();
        fbp += fulf.1;
        rfbs += fulf.2;
        whd += fulf.3;
        rows.push(CrossBorderProfitRow {
            sku,
            offer_id,
            product_name,
            units,
            fulfillment_orders: fulf.0,
            fbp_orders: fulf.1,
            rfbs_orders: fulf.2,
            whd_orders: fulf.3,
            revenue_cny: revenue / rate,
            selling_price_cny: selling_cny,
            purchase_cost_cny: unit_cost,
            weight_kg: weight,
            freight_unit_cny: freight_unit,
            purchase_total_cny: purchase_total,
            freight_total_cny: freight_total,
            estimated_platform_fees_cny: fees,
            contribution_cny: contribution,
            finance_settled_cny: (finance_count > 0).then_some(finance_net / rate),
            commission_rate,
            acquiring_rate,
            cost_complete: complete,
        })
    }
    let revenue_cny = rows.iter().map(|x| x.revenue_cny).sum::<f64>();
    let units = rows.iter().map(|x| x.units).sum();
    let ad_rub: f64 = c
        .query_row(
            "SELECT COALESCE(SUM(spend),0)FROM ad_daily WHERE day BETWEEN ?1 AND ?2 AND sku=''",
            params![range.from, range.to],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let settled:f64=c.query_row("SELECT COALESCE(SUM(amount),0)FROM finance_transactions WHERE substr(operation_date,1,10)BETWEEN ?1 AND ?2",params![range.from,range.to],|r|r.get(0)).map_err(|e|e.to_string())?;
    let finance_available:i64=c.query_row("SELECT COUNT(*)FROM finance_transactions WHERE substr(operation_date,1,10)BETWEEN ?1 AND ?2",params![range.from,range.to],|r|r.get(0)).map_err(|e|e.to_string())?;
    let profit = (missing == 0 && !rows.is_empty())
        .then(|| revenue_cny + total_fees - ad_rub / rate - purchase_freight);
    Ok(CrossBorderReport {
        date_from: range.from.clone(),
        date_to: range.to.clone(),
        rub_per_cny: rate,
        revenue_cny,
        units,
        ad_spend_cny: ad_rub / rate,
        estimated_platform_fees_cny: total_fees,
        purchase_and_freight_cny: purchase_freight,
        profit_cny: profit,
        settled_finance_net_cny: settled / rate,
        finance_available: finance_available > 0,
        commission_rate: shop_commission_rate,
        acquiring_rate,
        missing_cost_skus: missing,
        fbp_orders: fbp,
        rfbs_orders: rfbs,
        whd_orders: whd,
        daily: {
            let mut stmt=c.prepare("SELECT s.day,SUM(s.ordered_units),SUM(s.revenue),pc.unit_cost_cny,pc.weight_kg FROM sales_daily s LEFT JOIN product_costs pc ON pc.sku=s.sku WHERE s.day BETWEEN ?1 AND ?2 GROUP BY s.day,s.sku HAVING SUM(s.ordered_units)>0 ORDER BY s.day").map_err(|e|e.to_string())?;
            let raw = stmt
                .query_map(params![range.from, range.to], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, f64>(2)?,
                        r.get::<_, Option<f64>>(3)?,
                        r.get::<_, Option<f64>>(4)?,
                    ))
                })
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            let mut days = std::collections::BTreeMap::<String, (i64, f64, f64, bool)>::new();
            for (day, qty, revenue, cost, weight) in raw {
                let price = revenue / qty as f64 / rate;
                let freight = weight.and_then(|w| cross_border_shipping(price, w));
                let complete = cost.is_some()
                    && freight.is_some()
                    && shop_commission_rate.is_some()
                    && acquiring_rate.is_some();
                let expense = cost
                    .zip(freight)
                    .map(|(a, b)| (a + b) * qty as f64)
                    .unwrap_or(0.0);
                let e = days.entry(day).or_insert((0, 0.0, 0.0, true));
                e.0 += qty;
                e.1 += revenue / rate;
                e.2 += expense;
                e.3 &= complete;
            }
            days.into_iter()
                .map(|(day, (units, revenue, expense, complete))| {
                    let ad: f64 = c
                        .query_row(
                            "SELECT COALESCE(SUM(spend),0)FROM ad_daily WHERE day=?1 AND sku=''",
                            [&day],
                            |r| r.get(0),
                        )
                        .unwrap_or(0.0)
                        / rate;
                    let fees = shop_commission_rate
                        .zip(acquiring_rate)
                        .map(|(a, b)| revenue * (a + b))
                        .unwrap_or(0.0);
                    CrossBorderDayRow {
                        day,
                        units,
                        revenue_cny: revenue,
                        ad_spend_cny: ad,
                        purchase_and_freight_cny: expense,
                        profit_cny: complete.then_some(revenue - fees - ad - expense),
                    }
                })
                .collect()
        },
        rows,
    })
}
#[tauri::command]
async fn cross_border_report(
    range: DateRange,
    state: State<'_, AppState>,
) -> Result<CrossBorderReport, String> {
    let owned = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || cross_border_report_blocking(range, &owned))
        .await
        .map_err(|e| e.to_string())?
}

fn analytics_detail_blocking(
    range: DateRange,
    state: &AppState,
) -> Result<AnalyticsDetail, String> {
    let c = db(&state)?;
    let rate = rub_per_cny_for(state, &c)?;
    c.execute_batch("CREATE TABLE IF NOT EXISTS analytics_detail_cache(range_key TEXT PRIMARY KEY,fingerprint TEXT NOT NULL,payload TEXT NOT NULL,updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);").map_err(|e|e.to_string())?;
    let fingerprint:String=c.query_row("SELECT 'analytics-v5-ad-level-dedup|'||printf('%d|%s|%d|%d|%d',COALESCE((SELECT MAX(id)FROM sync_logs WHERE status='success' AND source IN('Seller Analytics','Seller Finance','Performance Ads')),0),COALESCE((SELECT MAX(updated_at)FROM product_costs),''),(SELECT COUNT(*)FROM sales_daily),(SELECT COUNT(*)FROM finance_transactions),(SELECT COUNT(*)FROM ad_daily))",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    let cache_key = format!("{}|{}", range.from, range.to);
    if let Ok(payload) = c.query_row(
        "SELECT payload FROM analytics_detail_cache WHERE range_key=?1 AND fingerprint=?2",
        params![cache_key, fingerprint],
        |r| r.get::<_, String>(0),
    ) {
        if let Ok(detail) = serde_json::from_str(&payload) {
            return Ok(detail);
        }
    }
    let mut statement=c.prepare("WITH fp AS(SELECT sku,SUM(CASE WHEN accruals_for_sale>0 THEN accruals_for_sale ELSE 0 END) sales,SUM(ABS(sale_commission)) commission,SUM(ABS(delivery_charge)+ABS(return_delivery_charge)) logistics,COUNT(DISTINCT CASE WHEN posting_number<>'' THEN posting_number END) postings FROM finance_transactions WHERE sku<>'' AND substr(operation_date,1,10)<=?2 GROUP BY sku) SELECT s.sku,COALESCE(MAX(p.offer_id),''),COALESCE(MAX(p.name),MAX(s.product_name),''),SUM(s.ordered_units),SUM(s.revenue),COALESCE((SELECT SUM(a.spend) FROM ad_daily a WHERE a.sku=s.sku AND a.day BETWEEN ?1 AND ?2),0),pc.unit_cost_cny,COALESCE(pc.first_mile_cost,pc.first_mile_cost_cny*?3),pc.weight_kg,CASE WHEN fp.sales>0 AND fp.commission/fp.sales<=0.60 THEN SUM(s.revenue)*(fp.commission/fp.sales) ELSE 0 END+CASE WHEN fp.postings>0 THEN SUM(s.ordered_units)*(fp.logistics/fp.postings) ELSE 0 END FROM sales_daily s LEFT JOIN products p ON p.sku=s.sku LEFT JOIN product_costs pc ON pc.sku=s.sku LEFT JOIN fp ON fp.sku=s.sku WHERE s.day BETWEEN ?1 AND ?2 GROUP BY s.sku ORDER BY SUM(s.revenue) DESC").map_err(|e|e.to_string())?;
    let products = statement
        .query_map(params![range.from, range.to, rate], |r| {
            let units: i64 = r.get(3)?;
            let revenue: f64 = r.get(4)?;
            let ad: f64 = r.get(5)?;
            let unit: Option<f64> = r.get(6)?;
            let first_unit: Option<f64> = r.get(7)?;
            let weight: Option<f64> = r.get(8)?;
            let fees: f64 = r.get(9)?;
            let purchase = unit.map(|v| v * rate * units as f64);
            let first = first_unit.map(|v| v * units as f64);
            let freight = weight.and_then(|w| {
                if units > 0 {
                    cross_border_shipping(revenue / units as f64 / rate, w)
                        .map(|v| v * rate * units as f64)
                } else {
                    None
                }
            });
            let complete = purchase.is_some() && first.is_some();
            let profit = if complete {
                Some(revenue - ad - purchase.unwrap_or(0.0) - first.unwrap_or(0.0) - fees)
            } else {
                None
            };
            Ok(ProductProfitRow {
                sku: r.get(0)?,
                offer_id: r.get(1)?,
                product_name: r.get(2)?,
                units,
                revenue,
                ad_spend: ad,
                purchase_cost: purchase,
                first_mile_cost: first,
                platform_fees: fees,
                estimated_profit: profit,
                profit_rate: profit.and_then(|v| {
                    if revenue != 0.0 {
                        Some(v / revenue * 100.0)
                    } else {
                        None
                    }
                }),
                cross_border_freight: freight,
                cost_complete: complete,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let series_expr="CASE WHEN instr(COALESCE(NULLIF(p.offer_id,''),s.sku),'-')>0 THEN substr(COALESCE(NULLIF(p.offer_id,''),s.sku),1,instr(COALESCE(NULLIF(p.offer_id,''),s.sku),'-')-1) ELSE COALESCE(NULLIF(p.offer_id,''),s.sku) END";
    let series_sql=format!("SELECT '日',s.day,{0},COUNT(DISTINCT s.sku),SUM(s.ordered_units),SUM(s.revenue) FROM sales_daily s LEFT JOIN products p ON p.sku=s.sku WHERE s.day BETWEEN ?1 AND ?2 GROUP BY 2,3 UNION ALL SELECT '周',strftime('%Y-W%W',s.day),{0},COUNT(DISTINCT s.sku),SUM(s.ordered_units),SUM(s.revenue) FROM sales_daily s LEFT JOIN products p ON p.sku=s.sku WHERE s.day BETWEEN ?1 AND ?2 GROUP BY 2,3 UNION ALL SELECT '月',strftime('%Y-%m',s.day),{0},COUNT(DISTINCT s.sku),SUM(s.ordered_units),SUM(s.revenue) FROM sales_daily s LEFT JOIN products p ON p.sku=s.sku WHERE s.day BETWEEN ?1 AND ?2 GROUP BY 2,3 ORDER BY 1,2 DESC,6 DESC",series_expr);
    let mut series_stmt = c.prepare(&series_sql).map_err(|e| e.to_string())?;
    let series = series_stmt
        .query_map(params![range.from, range.to], |r| {
            Ok(SeriesAnalysisRow {
                period_type: r.get(0)?,
                period: r.get(1)?,
                series: r.get(2)?,
                sku_count: r.get(3)?,
                units: r.get(4)?,
                revenue: r.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut daily_stmt=c.prepare("WITH fp AS(SELECT sku,SUM(CASE WHEN accruals_for_sale>0 THEN accruals_for_sale ELSE 0 END) sales,SUM(ABS(sale_commission)) commission,SUM(ABS(delivery_charge)+ABS(return_delivery_charge)) logistics,COUNT(DISTINCT CASE WHEN posting_number<>'' THEN posting_number END) postings FROM finance_transactions WHERE sku<>'' AND substr(operation_date,1,10)<=?2 GROUP BY sku) SELECT s.day,s.sku,COALESCE(p.offer_id,''),COALESCE(NULLIF(p.name,''),s.product_name),s.ordered_units,s.revenue,CASE WHEN EXISTS(SELECT 1 FROM return_events WHERE day BETWEEN ?1 AND ?2)THEN COALESCE((SELECT SUM(quantity)FROM return_events r WHERE r.day=s.day AND r.sku=s.sku),0)ELSE s.returns END,CASE WHEN EXISTS(SELECT 1 FROM cancellation_events WHERE day BETWEEN ?1 AND ?2)THEN COALESCE((SELECT SUM(quantity)FROM cancellation_events x WHERE x.day=s.day AND x.sku=s.sku),0)ELSE s.cancellations END,s.views,COALESCE((SELECT SUM(ABS(a.spend))FROM ad_daily a WHERE a.day=s.day AND a.sku=s.sku),0),COALESCE((SELECT SUM(a.orders)FROM ad_daily a WHERE a.day=s.day AND a.sku=s.sku),0),COALESCE(pc.unit_cost_cny*?3,pc.unit_cost),COALESCE(pc.first_mile_cost,pc.first_mile_cost_cny*?3),CASE WHEN fp.sales>0 AND fp.commission/fp.sales<=0.60 THEN (s.revenue/MAX(1,s.ordered_units))*(fp.commission/fp.sales) ELSE 0 END+CASE WHEN fp.postings>0 THEN fp.logistics/fp.postings ELSE 0 END FROM sales_daily s LEFT JOIN products p ON p.sku=s.sku LEFT JOIN product_costs pc ON pc.sku=s.sku LEFT JOIN fp ON fp.sku=s.sku WHERE s.day BETWEEN ?1 AND ?2 AND (s.ordered_units<>0 OR s.revenue<>0) ORDER BY s.day DESC,s.revenue DESC LIMIT 10000").map_err(|e|e.to_string())?;
    let daily_products = daily_stmt
        .query_map(params![range.from, range.to, rate], |r| {
            let units: i64 = r.get(4)?;
            let revenue: f64 = r.get(5)?;
            let ad_spend: f64 = r.get(9)?;
            let ad_orders: i64 = r.get(10)?;
            let unit_cost: Option<f64> = r.get(11)?;
            let first_mile: Option<f64> = r.get(12)?;
            let platform_fee_unit: f64 = r.get(13)?;
            let complete = unit_cost.is_some() && first_mile.is_some();
            let estimated_profit = complete.then(|| {
                revenue
                    - ad_spend
                    - units as f64
                        * (unit_cost.unwrap_or(0.0) + first_mile.unwrap_or(0.0) + platform_fee_unit)
            });
            Ok(DailyProductRow {
                day: r.get(0)?,
                sku: r.get(1)?,
                offer_id: r.get(2)?,
                product_name: r.get(3)?,
                units,
                revenue,
                returns: r.get(6)?,
                cancellations: r.get(7)?,
                views: r.get(8)?,
                ad_spend,
                ad_orders,
                tacos: (revenue != 0.0).then_some(ad_spend / revenue * 100.0),
                ad_cost_per_order: (units != 0).then_some(ad_spend / units as f64),
                estimated_profit,
                cost_complete: complete,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    // The legacy report is two fixed seven-day periods ending on the selected Sunday.
    // Do not use SQLite %W here: it changes the business boundary and used to omit SKU ad rows.
    let mut weekly_stmt=c.prepare("WITH periods(label,from_day,to_day,sort_no) AS (VALUES('本周',date(?2,'-6 day'),?2,0),('上周',date(?2,'-13 day'),date(?2,'-7 day'),1)),ad_by_day AS(SELECT a.day,SUM(CASE WHEN EXISTS(SELECT 1 FROM ad_daily z WHERE z.day=a.day AND z.sku='') THEN CASE WHEN a.sku='' THEN ABS(a.spend) ELSE 0 END ELSE CASE WHEN a.sku<>'' THEN ABS(a.spend) ELSE 0 END END) spend,SUM(CASE WHEN EXISTS(SELECT 1 FROM ad_daily z WHERE z.day=a.day AND z.sku='') THEN CASE WHEN a.sku='' THEN a.orders ELSE 0 END ELSE CASE WHEN a.sku<>'' THEN a.orders ELSE 0 END END) orders FROM ad_daily a GROUP BY a.day) SELECT p.label||' ('||substr(p.from_day,6)||'~'||substr(p.to_day,6)||')',COALESCE(SUM(s.revenue),0),COALESCE(SUM(s.ordered_units),0),COALESCE((SELECT SUM(a.spend) FROM ad_by_day a WHERE a.day BETWEEN p.from_day AND p.to_day),0),COALESCE(SUM(s.ordered_units*COALESCE(pc.unit_cost_cny*?3,pc.unit_cost,0)),0),COALESCE(SUM(s.ordered_units*COALESCE(pc.first_mile_cost,pc.first_mile_cost_cny*?3,0)),0),COALESCE((SELECT SUM(a.orders) FROM ad_by_day a WHERE a.day BETWEEN p.from_day AND p.to_day),0),COALESCE(SUM(s.returns),0),COALESCE(SUM(s.cancellations),0) FROM periods p LEFT JOIN sales_daily s ON s.day BETWEEN p.from_day AND p.to_day LEFT JOIN product_costs pc ON pc.sku=s.sku GROUP BY p.label,p.from_day,p.to_day,p.sort_no ORDER BY p.sort_no").map_err(|e|e.to_string())?;
    let weekly = weekly_stmt
        .query_map(params![range.from, range.to, rate], |r| {
            let revenue: f64 = r.get(1)?;
            let ad: f64 = r.get(3)?;
            let purchase: f64 = r.get(4)?;
            let first: f64 = r.get(5)?;
            let units: i64 = r.get(2)?;
            let ad_orders: i64 = r.get(6)?;
            Ok(WeeklyAnalysisRow {
                period: r.get(0)?,
                revenue,
                units,
                ad_spend: ad,
                ad_orders,
                ad_order_share: if units != 0 {
                    ad_orders as f64 / units as f64 * 100.0
                } else {
                    0.0
                },
                acots: if revenue != 0.0 {
                    ad / revenue * 100.0
                } else {
                    0.0
                },
                returns: r.get(7)?,
                cancellations: r.get(8)?,
                estimated_profit: revenue - ad - purchase - first,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut weekly_daily_stmt=c.prepare("SELECT s.day,SUM(s.revenue),SUM(s.ordered_units),COALESCE((SELECT SUM(CASE WHEN EXISTS(SELECT 1 FROM ad_daily z WHERE z.day=s.day AND z.sku='') THEN CASE WHEN a.sku='' THEN ABS(a.spend) ELSE 0 END ELSE CASE WHEN a.sku<>'' THEN ABS(a.spend) ELSE 0 END END)FROM ad_daily a WHERE a.day=s.day),0),COALESCE((SELECT SUM(CASE WHEN EXISTS(SELECT 1 FROM ad_daily z WHERE z.day=s.day AND z.sku='') THEN CASE WHEN a.sku='' THEN a.orders ELSE 0 END ELSE CASE WHEN a.sku<>'' THEN a.orders ELSE 0 END END)FROM ad_daily a WHERE a.day=s.day),0),SUM(s.returns),SUM(s.cancellations) FROM sales_daily s WHERE s.day BETWEEN date(?1,'-6 day') AND ?1 GROUP BY s.day ORDER BY s.day").map_err(|e|e.to_string())?;
    let weekly_daily = weekly_daily_stmt
        .query_map([range.to.clone()], |r| {
            let revenue: f64 = r.get(1)?;
            let units: i64 = r.get(2)?;
            let ad_spend: f64 = r.get(3)?;
            let ad_orders: i64 = r.get(4)?;
            Ok(WeeklyDailyRow {
                day: r.get(0)?,
                revenue,
                units,
                ad_spend,
                ad_orders,
                ad_order_share: if units != 0 {
                    ad_orders as f64 / units as f64 * 100.0
                } else {
                    0.0
                },
                acots: if revenue != 0.0 {
                    ad_spend / revenue * 100.0
                } else {
                    0.0
                },
                returns: r.get(5)?,
                cancellations: r.get(6)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let detail = AnalyticsDetail {
        products,
        daily_products,
        series,
        weekly,
        weekly_daily,
    };
    let payload = serde_json::to_string(&detail).map_err(|e| e.to_string())?;
    c.execute("INSERT INTO analytics_detail_cache(range_key,fingerprint,payload,updated_at)VALUES(?1,?2,?3,CURRENT_TIMESTAMP)ON CONFLICT(range_key)DO UPDATE SET fingerprint=excluded.fingerprint,payload=excluded.payload,updated_at=CURRENT_TIMESTAMP",params![cache_key,fingerprint,payload]).map_err(|e|e.to_string())?;
    Ok(detail)
}

#[tauri::command]
async fn analytics_detail(
    range: DateRange,
    state: State<'_, AppState>,
) -> Result<AnalyticsDetail, String> {
    let data_dir = state.data_dir.clone();
    let active_shop_id = state
        .active_shop_id
        .lock()
        .map_err(|_| "店铺状态锁异常")?
        .clone();
    tauri::async_runtime::spawn_blocking(move || {
        let owned = AppState {
            data_dir,
            active_shop_id: Mutex::new(active_shop_id),
        };
        analytics_detail_blocking(range, &owned)
    })
    .await
    .map_err(|e| format!("报表后台任务失败：{e}"))?
}

fn inventory_blocking(
    query: String,
    target_days: i64,
    lead_time_days: i64,
    safety_days: i64,
    state: &AppState,
) -> Result<Vec<InventoryRow>, String> {
    let c = db(state)?;
    let needle = format!("%{}%", query.trim());
    let mut stmt = c.prepare("WITH inv AS(SELECT sku,MAX(offer_id)offer_id,MAX(product_name)product_name,SUM(available_stock)available,SUM(transit_stock)transit,SUM(requested_stock)requested,COUNT(DISTINCT warehouse_id)warehouses,MAX(updated_at)updated FROM inventory_stock GROUP BY sku),feishu AS(SELECT a.sku,SUM(CASE WHEN s.cargo_status='未送仓' THEN a.quantity ELSE 0 END)production,0 domestic_stock,SUM(CASE WHEN s.cargo_status='在途' THEN a.quantity ELSE 0 END)overseas_transit,SUM(CASE WHEN s.cargo_status='到达海外仓' OR (s.cargo_status IN('已送仓','已申请') AND a.settled=0) THEN a.quantity ELSE 0 END)overseas_arrived FROM shipment_sku_allocations a JOIN shipment_tracking s ON s.tracking_id=a.tracking_id GROUP BY a.sku),sales AS(SELECT sku,SUM(CASE WHEN day>=date('now','-29 day') THEN ordered_units ELSE 0 END)/30.0 daily30,SUM(CASE WHEN day>=date('now','-6 day') THEN ordered_units ELSE 0 END)/7.0 daily7,SUM(CASE WHEN day>=date('now','-29 day') THEN ordered_units ELSE 0 END) ordered30 FROM sales_daily WHERE day>=date('now','-29 day') GROUP BY sku),return_count AS(SELECT sku,SUM(quantity) returns30 FROM return_events WHERE day>=date('now','-29 day') GROUP BY sku),return_cost AS(SELECT sku,SUM(ABS(amount)) cost FROM finance_transactions WHERE substr(operation_date,1,10)>=date('now','-29 day') AND sku<>'' AND (lower(operation_type)LIKE'%return%' OR lower(operation_type)LIKE'%возврат%') GROUP BY sku),plans AS(SELECT sku,SUM(planned_qty)planned FROM replenishment_plan GROUP BY sku) SELECT i.sku,i.offer_id,i.product_name,i.available,t.present_stock,t.reserved_stock,i.transit,i.requested,COALESCE(f.production,0),COALESCE(f.domestic_stock,0),COALESCE(f.overseas_transit,0),COALESCE(f.overseas_arrived,0),i.warehouses,COALESCE(s.daily30,0),COALESCE(s.daily7,0),COALESCE(s.ordered30,0),COALESCE(rr.returns30,0),COALESCE(rc.cost,0),COALESCE(p.planned,0),i.updated FROM inv i LEFT JOIN inventory_totals t ON t.sku=i.sku LEFT JOIN feishu f ON f.sku=i.sku LEFT JOIN sales s ON s.sku=i.sku LEFT JOIN return_count rr ON rr.sku=i.sku LEFT JOIN return_cost rc ON rc.sku=i.sku LEFT JOIN plans p ON p.sku=i.sku WHERE ?1='%%' OR i.sku LIKE ?1 OR i.offer_id LIKE ?1 OR i.product_name LIKE ?1 ORDER BY i.available,i.offer_id LIMIT 2000").map_err(|e|e.to_string())?;
    let raw = stmt
        .query_map([needle], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, Option<i64>>(4)?,
                r.get::<_, Option<i64>>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, i64>(7)?,
                r.get::<_, i64>(8)?,
                r.get::<_, i64>(9)?,
                r.get::<_, i64>(10)?,
                r.get::<_, i64>(11)?,
                r.get::<_, i64>(12)?,
                r.get::<_, f64>(13)?,
                r.get::<_, f64>(14)?,
                r.get::<_, i64>(15)?,
                r.get::<_, i64>(16)?,
                r.get::<_, f64>(17)?,
                r.get::<_, i64>(18)?,
                r.get::<_, String>(19)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(raw
        .into_iter()
        .map(
            |(
                sku,
                offer_id,
                product_name,
                available,
                portal,
                reserved,
                transit,
                requested,
                production,
                domestic_stock,
                overseas_transit,
                overseas_arrived,
                warehouses,
                daily,
                daily7,
                ordered30,
                returns30,
                return_cost,
                planned,
                updated,
            )| {
                let forecast_daily = daily.max(daily7);
                let estimated = if forecast_daily > 0.0 {
                    Some(available as f64 / forecast_daily)
                } else {
                    None
                };
                let required_days = target_days.max(lead_time_days + safety_days);
                let suggested = ((forecast_daily * required_days as f64).ceil() as i64
                    - available
                    - transit
                    - requested
                    - planned
                    - production
                    - domestic_stock
                    - overseas_transit
                    - overseas_arrived)
                    .max(0);
                let (health_status, health_text) = match estimated {
                    Some(_) if available == 0 => ("stockout", "已断货"),
                    Some(days) if days <= 7.0 => ("critical", "7 天内断货"),
                    Some(days) if days <= 14.0 => ("warning", "库存偏低"),
                    Some(days) if days > 90.0 => ("overstock", "库存超过 90 天"),
                    Some(days) if days > 60.0 => ("overstock", "库存超过 60 天"),
                    Some(_) => ("healthy", "库存健康"),
                    None if available > 0 => ("slow", "暂无销量/可能滞销"),
                    None => ("empty", "无库存且暂无销量"),
                };
                InventoryRow {
                    sku,
                    offer_id,
                    product_name,
                    available_stock: available,
                    portal_stock: portal,
                    reserved_stock: reserved,
                    transit_stock: transit,
                    requested_stock: requested,
                    domestic_production_stock: production,
                    domestic_warehouse_stock: domestic_stock,
                    overseas_transit_stock: overseas_transit,
                    overseas_arrived_stock: overseas_arrived,
                    warehouse_count: warehouses,
                    daily_sales: daily,
                    daily_sales_7d: daily7,
                    demand_trend_percent: (daily > 0.0).then_some((daily7 - daily) / daily * 100.0),
                    estimated_days: estimated,
                    health_status: health_status.into(),
                    health_text: health_text.into(),
                    suggested_qty: suggested,
                    planned_qty: planned,
                    return_units_30d: returns30,
                    return_rate_30d: (ordered30 > 0)
                        .then_some(returns30 as f64 / ordered30 as f64 * 100.0),
                    return_logistics_cost_30d: return_cost,
                    updated_at: updated,
                }
            },
        )
        .collect())
}

#[tauri::command]
async fn inventory(
    query: String,
    target_days: i64,
    lead_time_days: i64,
    safety_days: i64,
    state: State<'_, AppState>,
) -> Result<Vec<InventoryRow>, String> {
    let owned = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || {
        inventory_blocking(query, target_days, lead_time_days, safety_days, &owned)
    })
    .await
    .map_err(|e| format!("库存读取后台任务失败：{e}"))?
}

fn sync_inventory_blocking(state: &AppState) -> Result<i64, String> {
    let _sync_guard = INVENTORY_SYNC_LOCK
        .try_lock()
        .map_err(|_| "已有数据同步任务正在后台运行，请等待完成后再同步库存。".to_string())?;
    let mut c = db(&state)?;
    c.execute("INSERT INTO sync_logs(started_at,source,status) VALUES(CURRENT_TIMESTAMP,'Seller Inventory','running')", []).map_err(|e| e.to_string())?;
    let log_id = c.last_insert_rowid();
    let result = (|| -> Result<i64, String> {
        let mut stmt = c.prepare("SELECT DISTINCT sku FROM (SELECT sku FROM sales_daily UNION SELECT sku FROM product_costs UNION SELECT sku FROM posting_routes) WHERE sku<>'' ORDER BY sku").map_err(|e| e.to_string())?;
        let skus = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        drop(stmt);
        let numeric = skus
            .into_iter()
            .filter_map(|sku| sku.parse::<i64>().ok())
            .filter(|sku| *sku > 0)
            .collect::<Vec<_>>();
        if numeric.is_empty() {
            return Err("没有可同步的数字 Ozon SKU，请先同步 Seller 销量或订单数据。".into());
        }
        let mut items = Vec::new();
        for batch in numeric.chunks(100) {
            let payload = seller_post(
                &c,
                "/v1/analytics/stocks",
                &serde_json::json!({"skus": batch}),
            )?;
            items.extend(
                payload
                    .get("items")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default(),
            );
        }
        let mut offers = items
            .iter()
            .filter_map(|v| v.as_object())
            .map(|v| object_text(v, &["offer_id", "offerId"]))
            .filter(|v| !v.is_empty())
            .collect::<Vec<_>>();
        offers.sort();
        offers.dedup();
        let mut totals = Vec::<(String, String, i64, i64)>::new();
        for batch in offers.chunks(100) {
            let payload = seller_post(
                &c,
                "/v4/product/info/stocks",
                &serde_json::json!({"filter":{"offer_id":batch,"product_id":[],"visibility":"ALL"},"limit":1000,"cursor":""}),
            )?;
            for value in payload
                .get("items")
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten()
            {
                let Some(item) = value.as_object() else {
                    continue;
                };
                let sku = object_text(item, &["product_id", "sku"]);
                let offer = object_text(item, &["offer_id", "offerId"]);
                let mut present = 0_i64;
                let mut reserved = 0_i64;
                for stock in item
                    .get("stocks")
                    .and_then(|v| v.as_array())
                    .into_iter()
                    .flatten()
                {
                    let kind = stock
                        .get("type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    if kind == "fbo" || kind.is_empty() {
                        present += stock.get("present").and_then(|v| v.as_i64()).unwrap_or(0);
                        reserved += stock.get("reserved").and_then(|v| v.as_i64()).unwrap_or(0);
                    }
                }
                if !sku.is_empty() {
                    totals.push((sku, offer, present, reserved));
                }
            }
        }
        let tx = c.transaction().map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM inventory_stock", [])
            .map_err(|e| e.to_string())?;
        for (sku, offer, present, reserved) in totals {
            tx.execute("INSERT INTO inventory_totals(sku,offer_id,present_stock,reserved_stock,updated_at)VALUES(?1,?2,?3,?4,CURRENT_TIMESTAMP)ON CONFLICT(sku)DO UPDATE SET offer_id=excluded.offer_id,present_stock=excluded.present_stock,reserved_stock=excluded.reserved_stock,updated_at=CURRENT_TIMESTAMP",params![sku,offer,present,reserved]).map_err(|e|e.to_string())?;
        }
        let mut count = 0_i64;
        for (index, value) in items.iter().enumerate() {
            let Some(item) = value.as_object() else {
                continue;
            };
            let sku = object_text(item, &["sku"]);
            if sku.is_empty() {
                continue;
            }
            let mut warehouse_id = object_text(item, &["warehouse_id", "warehouseId"]);
            let warehouse_name = object_text(item, &["warehouse_name", "warehouseName"]);
            let cluster_id = object_text(item, &["cluster_id", "clusterId"]);
            let cluster_name = object_text(item, &["cluster_name", "clusterName"]);
            let macro_id = object_text(item, &["macrolocal_cluster_id", "macrolocalClusterId"]);
            if warehouse_id.is_empty() {
                warehouse_id = format!("fallback-{macro_id}-{cluster_id}-{warehouse_name}-{index}");
            }
            tx.execute("INSERT INTO inventory_stock(sku,offer_id,product_name,warehouse_id,warehouse_name,cluster_id,cluster_name,macrolocal_cluster_id,ads,ads_cluster,available_stock,valid_stock,requested_stock,transit_stock,days_without_sales,idc_cluster,turnover_grade_cluster,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,CURRENT_TIMESTAMP) ON CONFLICT(sku,warehouse_id) DO UPDATE SET offer_id=excluded.offer_id,product_name=excluded.product_name,warehouse_name=excluded.warehouse_name,cluster_id=excluded.cluster_id,cluster_name=excluded.cluster_name,macrolocal_cluster_id=excluded.macrolocal_cluster_id,ads=excluded.ads,ads_cluster=excluded.ads_cluster,available_stock=excluded.available_stock,valid_stock=excluded.valid_stock,requested_stock=excluded.requested_stock,transit_stock=excluded.transit_stock,days_without_sales=excluded.days_without_sales,idc_cluster=excluded.idc_cluster,turnover_grade_cluster=excluded.turnover_grade_cluster,updated_at=CURRENT_TIMESTAMP", params![sku,object_text(item,&["offer_id","offerId"]),object_text(item,&["name","product_name"]),warehouse_id,warehouse_name,cluster_id,cluster_name,macro_id,object_number(item,&["ads"]),object_number(item,&["ads_cluster","adsCluster"]),object_number(item,&["available_stock_count","availableStockCount"]) as i64,object_number(item,&["valid_stock_count","validStockCount"]) as i64,object_number(item,&["requested_stock_count","requestedStockCount"]) as i64,object_number(item,&["transit_stock_count","transitStockCount"]) as i64,object_number(item,&["days_without_sales","daysWithoutSales"]) as i64,object_number(item,&["idc_cluster","idcCluster"]) as i64,object_text(item,&["turnover_grade_cluster","turnoverGradeCluster"])]).map_err(|e| e.to_string())?;
            count += 1;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(count)
    })();
    match result {
        Ok(count) => {
            c.execute("UPDATE sync_logs SET finished_at=CURRENT_TIMESTAMP,status='success',rows_count=?1,message=?2 WHERE id=?3", params![count, format!("库存同步完成：{count} 行"), log_id]).map_err(|e| e.to_string())?;
            Ok(count)
        }
        Err(error) => {
            let _ = c.execute("UPDATE sync_logs SET finished_at=CURRENT_TIMESTAMP,status='failed',message=?1 WHERE id=?2", params![error, log_id]);
            Err(error)
        }
    }
}

#[tauri::command]
async fn sync_inventory(state: State<'_, AppState>) -> Result<i64, String> {
    let owned = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || sync_inventory_blocking(&owned))
        .await
        .map_err(|e| format!("库存后台任务失败：{e}"))?
}

fn json_text(value: Option<&serde_json::Value>) -> String {
    value
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

fn json_f64(value: Option<&serde_json::Value>) -> Option<f64> {
    value.and_then(|v| match v {
        serde_json::Value::Number(number) => number.as_f64(),
        serde_json::Value::String(text) => text.trim().replace(',', ".").parse().ok(),
        serde_json::Value::Object(map) => json_f64(
            map.get("value")
                .or_else(|| map.get("amount"))
                .or_else(|| map.get("budget")),
        ),
        _ => None,
    })
}

fn performance_budget_rub(value: Option<&serde_json::Value>) -> Option<f64> {
    json_f64(value).map(|micro_rubles| micro_rubles / 1_000_000.0)
}

#[tauri::command]
fn sync_logs(state: State<AppState>) -> Result<Vec<SyncLogRow>, String> {
    let c = db(&state)?;
    let mut stmt=c.prepare("SELECT id,started_at,COALESCE(finished_at,''),source,status,rows_count,message FROM sync_logs ORDER BY id DESC LIMIT 100").map_err(|e|e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok(SyncLogRow {
                id: r.get(0)?,
                started_at: r.get(1)?,
                finished_at: r.get(2)?,
                source: r.get(3)?,
                status: r.get(4)?,
                rows_count: r.get(5)?,
                message: r.get(6)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

fn metric_number(values: &[serde_json::Value], index: usize) -> f64 {
    values
        .get(index)
        .and_then(|v| v.as_f64())
        .or_else(|| {
            values
                .get(index)
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse().ok())
        })
        .unwrap_or(0.0)
}
fn sales_dimensions(values: &[serde_json::Value], fallback_day: &str) -> (String, String, String) {
    let mut sku = String::new();
    let mut name = String::new();
    let mut day = String::new();
    for d in values {
        let key = json_text(d.get("key")).to_lowercase();
        let id = d
            .get("id")
            .map(|v| {
                if let Some(s) = v.as_str() {
                    s.to_string()
                } else {
                    v.to_string()
                }
            })
            .unwrap_or_default();
        let value = if id.is_empty() {
            json_text(d.get("value"))
        } else {
            id
        };
        if key.contains("day") || value.len() >= 10 && value.as_bytes().get(4) == Some(&b'-') {
            day = value.chars().take(10).collect()
        } else if key.contains("sku") || sku.is_empty() && value.chars().all(|c| c.is_ascii_digit())
        {
            sku = value;
            name = json_text(d.get("name"));
        }
    }
    if sku.is_empty() {
        if let Some(d) = values.first() {
            sku = d
                .get("id")
                .map(|v| {
                    v.as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| v.to_string())
                })
                .unwrap_or_default();
            name = json_text(d.get("name"));
        }
    }
    if day.is_empty() {
        day = fallback_day.to_string()
    }
    (sku, name, day)
}

fn sync_seller_sales_blocking(
    range: DateRange,
    force: bool,
    state: &AppState,
) -> Result<i64, String> {
    let _sync_guard = SELLER_SYNC_LOCK
        .try_lock()
        .map_err(|_| "已有数据同步任务正在后台运行，请等待完成。".to_string())?;
    let mut c = db(state)?;
    c.execute("INSERT INTO sync_logs(started_at,source,status) VALUES(CURRENT_TIMESTAMP,'Seller Analytics','running')",[]).map_err(|e|e.to_string())?;
    let log_id = c.last_insert_rowid();
    c.execute_batch("CREATE TABLE IF NOT EXISTS sync_progress(source TEXT NOT NULL,range_from TEXT NOT NULL,range_to TEXT NOT NULL,api_from TEXT NOT NULL,next_offset INTEGER NOT NULL DEFAULT 0,rows_count INTEGER NOT NULL DEFAULT 0,updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,PRIMARY KEY(source,range_from,range_to));").map_err(|e|e.to_string())?;
    let mut persisted = 0_i64;
    let result = (|| -> Result<i64, String> {
        let metrics = [
            "revenue",
            "ordered_units",
            "delivered_units",
            "returns",
            "cancellations",
            "hits_view",
            "hits_tocart",
        ];
        type SalesSyncRow = (String, String, String, f64, i64, i64, i64, i64, i64, i64);
        if force {
            c.execute("DELETE FROM sync_progress WHERE source='Seller Analytics' AND range_from=?1 AND range_to=?2",params![range.from,range.to]).map_err(|e|e.to_string())?;
        }
        let checkpoint = if force {
            None
        } else {
            c.query_row("SELECT api_from,next_offset,rows_count FROM sync_progress WHERE source='Seller Analytics' AND range_from=?1 AND range_to=?2",params![range.from,range.to],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?))).ok()
        };
        let cached:(i64,String,String)=c.query_row("SELECT COUNT(*),COALESCE(MIN(day),''),COALESCE(MAX(day),'') FROM sales_daily WHERE source='api' AND day BETWEEN ?1 AND ?2",params![range.from,range.to],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e|e.to_string())?;
        let today = chrono::Local::now()
            .date_naive()
            .format("%Y-%m-%d")
            .to_string();
        if !force
            && checkpoint.is_none()
            && cached.0 > 0
            && cached.1 <= range.from
            && cached.2 >= range.to
            && range.to < today
        {
            return Ok(cached.0);
        }
        let (api_from, mut offset, checkpoint_rows) = checkpoint.unwrap_or_else(|| {
            let incremental_from =
                if !force && cached.0 > 0 && cached.1 <= range.from && cached.2 >= range.from {
                    cached.2.clone()
                } else {
                    range.from.clone()
                };
            (incremental_from, 0, 0)
        });
        persisted = checkpoint_rows;
        let mut rows: Vec<SalesSyncRow> = Vec::new();
        loop {
            if offset > 0 {
                std::thread::sleep(std::time::Duration::from_secs(65));
            }
            let payload = seller_post(
                &c,
                "/v1/analytics/data",
                &serde_json::json!({"date_from":api_from,"date_to":range.to,"metrics":metrics,"dimension":["sku","day"],"filters":[],"sort":[{"key":"revenue","order":"DESC"}],"limit":1000,"offset":offset}),
            )?;
            let data = payload
                .pointer("/result/data")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let count = data.len();
            for item in data {
                let dims = item
                    .get("dimensions")
                    .and_then(|v| v.as_array())
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                let vals = item
                    .get("metrics")
                    .and_then(|v| v.as_array())
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                let (sku, name, day) = sales_dimensions(dims, &range.to);
                if !sku.is_empty() {
                    rows.push((
                        day,
                        sku,
                        name,
                        metric_number(vals, 0),
                        metric_number(vals, 1) as i64,
                        metric_number(vals, 2) as i64,
                        metric_number(vals, 3) as i64,
                        metric_number(vals, 4) as i64,
                        metric_number(vals, 5) as i64,
                        metric_number(vals, 6) as i64,
                    ));
                }
            }
            let page_rows = rows.len() as i64;
            let next_offset = offset + 1000;
            let tx = c.transaction().map_err(|e| e.to_string())?;
            for row in &rows {
                tx.execute("INSERT INTO sales_daily(day,sku,product_name,revenue,ordered_units,delivered_units,returns,cancellations,views,cart_adds,source)VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'api') ON CONFLICT(day,sku) DO UPDATE SET product_name=excluded.product_name,revenue=excluded.revenue,ordered_units=excluded.ordered_units,delivered_units=excluded.delivered_units,returns=excluded.returns,cancellations=excluded.cancellations,views=excluded.views,cart_adds=excluded.cart_adds,source='api',updated_at=CURRENT_TIMESTAMP",params![row.0,row.1,row.2,row.3,row.4,row.5,row.6,row.7,row.8,row.9]).map_err(|e|e.to_string())?;
                tx.execute("INSERT INTO products(sku,name,source)VALUES(?1,?2,'seller_analytics')ON CONFLICT(sku)DO UPDATE SET name=CASE WHEN products.name='' AND excluded.name<>''THEN excluded.name ELSE products.name END,updated_at=CURRENT_TIMESTAMP",params![row.1,row.2]).map_err(|e|e.to_string())?;
            }
            persisted += page_rows;
            if count < 1000 {
                tx.execute("DELETE FROM sync_progress WHERE source='Seller Analytics' AND range_from=?1 AND range_to=?2",params![range.from,range.to]).map_err(|e|e.to_string())?;
            } else {
                tx.execute("INSERT INTO sync_progress(source,range_from,range_to,api_from,next_offset,rows_count,updated_at)VALUES('Seller Analytics',?1,?2,?3,?4,?5,CURRENT_TIMESTAMP)ON CONFLICT(source,range_from,range_to)DO UPDATE SET api_from=excluded.api_from,next_offset=excluded.next_offset,rows_count=excluded.rows_count,updated_at=CURRENT_TIMESTAMP",params![range.from,range.to,api_from,next_offset,persisted]).map_err(|e|e.to_string())?;
            }
            tx.commit().map_err(|e| e.to_string())?;
            rows.clear();
            if count < 1000 {
                break;
            }
            offset += 1000;
        }
        // Product images are catalog metadata, not order data. Refresh them as
        // a best-effort companion step; a catalog permission error must not
        // discard an otherwise successful sales sync.
        let mut metadata_offer_ids = Vec::new();
        if let Ok(catalog) = seller_post(
            &c,
            "/v3/product/list",
            &serde_json::json!({"filter":{"visibility":"ALL"},"last_id":"","limit":1000}),
        ) {
            let items = catalog
                .pointer("/result/items")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let tx = c.transaction().map_err(|e| e.to_string())?;
            for item in items {
                let offer = json_text(item.get("offer_id"));
                if !offer.is_empty() {
                    metadata_offer_ids.push(offer.clone());
                }
                let sku = json_text(item.get("sku"));
                if sku.is_empty() {
                    continue;
                }
                let images = item.get("images").and_then(|v| v.as_array());
                let first = images
                    .and_then(|v| v.first())
                    .map(|v| {
                        if v.is_string() {
                            json_text(Some(v))
                        } else {
                            json_text(v.get("url"))
                        }
                    })
                    .unwrap_or_default();
                let image = {
                    let primary = json_text(item.get("primary_image"));
                    if primary.is_empty() {
                        first
                    } else {
                        primary
                    }
                };
                tx.execute("INSERT INTO products(sku,offer_id,product_id,name,image_url,source)VALUES(?1,?2,?3,?4,?5,'api')ON CONFLICT(sku)DO UPDATE SET offer_id=excluded.offer_id,product_id=excluded.product_id,name=CASE WHEN excluded.name<>''THEN excluded.name ELSE products.name END,image_url=CASE WHEN excluded.image_url<>''THEN excluded.image_url ELSE products.image_url END,source='api',updated_at=CURRENT_TIMESTAMP",params![sku,offer,json_text(item.get("product_id")),json_text(item.get("name")),image]).map_err(|e|e.to_string())?;
            }
            tx.commit().map_err(|e| e.to_string())?;
        }
        // `/v3/product/list` supplies identifiers but does not reliably return
        // image metadata. Resolve the cached products through the product-info
        // endpoint in chunks and preserve any image already stored locally.
        let offer_ids = {
            let mut stmt = c
                .prepare(
                    "SELECT DISTINCT offer_id FROM products WHERE offer_id<>'' ORDER BY offer_id",
                )
                .map_err(|e| e.to_string())?;
            let values = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            metadata_offer_ids.extend(values);
            metadata_offer_ids.sort();
            metadata_offer_ids.dedup();
            metadata_offer_ids
        };
        for chunk in offer_ids.chunks(500) {
            let details = match seller_post(
                &c,
                "/v3/product/info/list",
                &serde_json::json!({"offer_id":chunk,"product_id":[],"sku":[]}),
            ) {
                Ok(value) => value,
                Err(_) => break,
            };
            let items = details
                .get("items")
                .or_else(|| details.pointer("/result/items"))
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let tx = c.transaction().map_err(|e| e.to_string())?;
            for item in items {
                let offer = json_text(item.get("offer_id"));
                if offer.is_empty() {
                    continue;
                }
                let primary = json_text(item.get("primary_image"));
                let image = if !primary.is_empty() {
                    primary
                } else {
                    item.get("images")
                        .and_then(|v| v.as_array())
                        .and_then(|v| v.first())
                        .map(|v| {
                            if v.is_string() {
                                json_text(Some(v))
                            } else {
                                json_text(v.get("url"))
                            }
                        })
                        .unwrap_or_default()
                };
                let sku = item
                    .get("sku")
                    .map(|v| {
                        v.as_i64()
                            .map(|n| n.to_string())
                            .unwrap_or_else(|| json_text(Some(v)))
                    })
                    .unwrap_or_default();
                if !sku.is_empty() {
                    tx.execute("INSERT INTO products(sku,offer_id,product_id,name,image_url,source)VALUES(?1,?2,?3,?4,?5,'api')ON CONFLICT(sku)DO UPDATE SET offer_id=excluded.offer_id,product_id=excluded.product_id,name=CASE WHEN excluded.name<>''THEN excluded.name ELSE products.name END,image_url=CASE WHEN excluded.image_url<>''THEN excluded.image_url ELSE products.image_url END,source='api',updated_at=CURRENT_TIMESTAMP",params![sku,offer,json_text(item.get("id")).or_else_empty(||json_text(item.get("product_id"))),json_text(item.get("name")),image]).map_err(|e|e.to_string())?;
                } else if !image.is_empty() {
                    tx.execute("UPDATE products SET image_url=?1,updated_at=CURRENT_TIMESTAMP WHERE offer_id=?2", params![image, offer]).map_err(|e| e.to_string())?;
                }
            }
            tx.commit().map_err(|e| e.to_string())?;
        }
        Ok(persisted)
    })();
    match result {
        Ok(count) => {
            c.execute("UPDATE sync_logs SET finished_at=CURRENT_TIMESTAMP,status='success',rows_count=?1,message=?2 WHERE id=?3",params![count,format!("Seller 销量同步完成：{count} 行"),log_id]).map_err(|e|e.to_string())?;
            Ok(count)
        }
        Err(error) => {
            let message = if persisted > 0 {
                format!(
                    "{error}；本次已分批缓存 {persisted} 行，下次同步同一日期范围将从断点继续。"
                )
            } else {
                error.clone()
            };
            let _=c.execute("UPDATE sync_logs SET finished_at=CURRENT_TIMESTAMP,status='failed',rows_count=?1,message=?2 WHERE id=?3",params![persisted,message,log_id]);
            Err(error)
        }
    }
}

#[tauri::command]
async fn sync_seller_sales(
    range: DateRange,
    force: bool,
    state: State<'_, AppState>,
) -> Result<i64, String> {
    let owned = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || {
        let count = sync_seller_sales_blocking(range.clone(), force, &owned)?;
        // Orders are a separate Seller endpoint. Cache them after analytics so
        // the order center is populated by the same user-visible sync action.
        let _ = sync_fbs_orders_blocking(range.clone(), &owned);
        let _ = sync_fbo_orders_blocking(range.clone(), &owned);
        let _ = rebuild_cancellation_events(&range, &owned);
        let _ = sync_customer_returns_blocking(&range, &owned);
        Ok(count)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn smart_sync_range(
    c: &Connection,
    table: &str,
    date_expression: &str,
    range: &DateRange,
    refresh_days: i64,
    force: bool,
) -> Result<Option<DateRange>, String> {
    if force {
        return Ok(Some(range.clone()));
    }
    let sql = format!("SELECT COUNT(*),COALESCE(MIN({date_expression}),''),COALESCE(MAX({date_expression}),'') FROM {table} WHERE {date_expression} BETWEEN ?1 AND ?2");
    let (count, min_day, max_day): (i64, String, String) = c
        .query_row(&sql, params![range.from, range.to], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .map_err(|e| e.to_string())?;
    let today = chrono::Local::now().date_naive();
    let mutable_from = today
        .checked_sub_signed(chrono::Duration::days(refresh_days.saturating_sub(1)))
        .unwrap_or(today)
        .format("%Y-%m-%d")
        .to_string();
    if count > 0 && min_day <= range.from && max_day >= range.to && range.to < mutable_from {
        return Ok(None);
    }
    let from = if count > 0 && min_day <= range.from && max_day >= range.from {
        std::cmp::max(range.from.clone(), std::cmp::min(max_day, mutable_from))
    } else {
        range.from.clone()
    };
    Ok(Some(DateRange {
        from,
        to: range.to.clone(),
    }))
}

#[cfg(test)]
mod smart_sync_tests {
    use super::{smart_sync_range, DateRange};
    use rusqlite::Connection;

    #[test]
    fn completed_historical_range_uses_local_cache() {
        let c = Connection::open_in_memory().unwrap();
        c.execute("CREATE TABLE x(day TEXT NOT NULL)", []).unwrap();
        c.execute("INSERT INTO x VALUES('2025-01-01'),('2025-01-31')", [])
            .unwrap();
        let range = DateRange {
            from: "2025-01-01".into(),
            to: "2025-01-31".into(),
        };
        assert!(smart_sync_range(&c, "x", "day", &range, 3, false)
            .unwrap()
            .is_none());
        assert!(smart_sync_range(&c, "x", "day", &range, 3, true)
            .unwrap()
            .is_some());
    }

    #[test]
    fn incomplete_range_resumes_from_cached_maximum() {
        let c = Connection::open_in_memory().unwrap();
        c.execute("CREATE TABLE x(day TEXT NOT NULL)", []).unwrap();
        c.execute("INSERT INTO x VALUES('2025-01-01'),('2025-01-15')", [])
            .unwrap();
        let range = DateRange {
            from: "2025-01-01".into(),
            to: "2099-01-31".into(),
        };
        let effective = smart_sync_range(&c, "x", "day", &range, 3, false)
            .unwrap()
            .unwrap();
        assert_eq!(effective.from, "2025-01-15");
        assert_eq!(effective.to, "2099-01-31");
    }
}

fn sync_performance_ads_blocking(
    range: DateRange,
    force: bool,
    state: &AppState,
) -> Result<i64, String> {
    let _sync_guard = PERFORMANCE_SYNC_LOCK
        .try_lock()
        .map_err(|_| "已有数据同步任务正在后台运行，请等待完成。".to_string())?;
    let mut c = db(state)?;
    c.execute("INSERT INTO sync_logs(started_at,source,status) VALUES(CURRENT_TIMESTAMP,'Performance Ads','running')",[]).map_err(|e|e.to_string())?;
    let log_id = c.last_insert_rowid();
    let result = (|| -> Result<i64, String> {
        let Some(range) = smart_sync_range(&c, "ad_daily", "day", &range, 3, force)? else {
            return Ok(0);
        };
        let token = performance_token(&c)?;
        let campaigns_payload = performance_get("/api/client/campaign", &token)?;
        let source = campaigns_payload
            .get("list")
            .and_then(|v| v.as_array())
            .or_else(|| campaigns_payload.as_array())
            .cloned()
            .unwrap_or_default();
        let mut names = std::collections::HashMap::new();
        {
            let tx = c.transaction().map_err(|e| e.to_string())?;
            for campaign in &source {
                let id = campaign
                    .get("id")
                    .map(|v| {
                        v.as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| v.to_string())
                    })
                    .unwrap_or_default();
                if id.is_empty() {
                    continue;
                }
                let name = json_text(campaign.get("title"));
                names.insert(id.clone(), name.clone());
                let budget = performance_budget_rub(
                    campaign
                        .get("weeklyBudget")
                        .or_else(|| campaign.get("budget")),
                );
                tx.execute("INSERT INTO campaigns(campaign_id,name,state,payment_type,budget,budget_known,budget_updated_at,budget_scale_version,source)VALUES(?1,?2,?3,?4,COALESCE(?5,0),?6,CASE WHEN ?6=1 THEN CURRENT_TIMESTAMP ELSE '' END,1,'api') ON CONFLICT(campaign_id) DO UPDATE SET name=excluded.name,state=excluded.state,payment_type=excluded.payment_type,budget=CASE WHEN excluded.budget_known=1 THEN excluded.budget ELSE campaigns.budget END,budget_known=MAX(campaigns.budget_known,excluded.budget_known),budget_updated_at=CASE WHEN excluded.budget_known=1 THEN CURRENT_TIMESTAMP ELSE campaigns.budget_updated_at END,budget_scale_version=1,source='api',updated_at=CURRENT_TIMESTAMP",params![id,name,json_text(campaign.get("state")),json_text(campaign.get("paymentType")),budget,budget.is_some()]).map_err(|e|e.to_string())?;
            }
            tx.commit().map_err(|e| e.to_string())?;
        }
        let payload = performance_get(
            &format!(
                "/api/client/statistics/daily/json?dateFrom={}&dateTo={}",
                range.from, range.to
            ),
            &token,
        )?;
        let mut objects = Vec::new();
        collect_objects(&payload, &mut objects);
        let tx = c.transaction().map_err(|e| e.to_string())?;
        let mut count = 0;
        for o in objects {
            let id = object_text(&o, &["campaignId", "campaign_id", "id"]);
            let day = object_text(&o, &["date", "day"])
                .chars()
                .take(10)
                .collect::<String>();
            if id.is_empty() || day.is_empty() {
                continue;
            }
            let name = {
                let direct = object_text(&o, &["title", "campaignName", "campaign_name", "name"]);
                if direct.is_empty() {
                    names.get(&id).cloned().unwrap_or_default()
                } else {
                    direct
                }
            };
            tx.execute("INSERT INTO ad_daily(day,campaign_id,campaign_name,sku,impressions,clicks,cart_adds,orders,revenue,spend,source)VALUES(?1,?2,?3,'',?4,?5,?6,?7,?8,?9,'api') ON CONFLICT(day,campaign_id,sku) DO UPDATE SET campaign_name=excluded.campaign_name,impressions=excluded.impressions,clicks=excluded.clicks,cart_adds=excluded.cart_adds,orders=excluded.orders,revenue=excluded.revenue,spend=excluded.spend,source='api',updated_at=CURRENT_TIMESTAMP",params![day,id,name,object_number(&o,&["views","impressions","shows"] )as i64,object_number(&o,&["clicks"])as i64,object_number(&o,&["toCart","cartAdds","cart_adds","addToCart"])as i64,object_number(&o,&["orders","modelOrders"])as i64,object_number(&o,&["revenue","sales","modelSales","ordersMoney","orderMoney","modelRevenue"]),object_number(&o,&["expense","spend","moneySpent","cost"])]).map_err(|e|e.to_string())?;
            count += 1;
        }
        tx.commit().map_err(|e| e.to_string())?;
        // 精确 SKU 接口仅允许今天和昨天；逐日增量写入且不删除历史缓存。
        let today = chrono::Local::now().date_naive();
        for product_day in [today.pred_opt().unwrap_or(today), today] {
            let day = product_day.format("%Y-%m-%d").to_string();
            if day < range.from || day > range.to {
                continue;
            }
            let ids = names.keys().cloned().collect::<Vec<_>>();
            for batch in ids.chunks(100) {
                let detail = performance_post(
                    "/api/client/statistics/products/sku",
                    &token,
                    &serde_json::json!({"campaignIds":batch,"dateFrom":day,"dateTo":day}),
                )?;
                let mut detail_objects = Vec::new();
                collect_objects(&detail, &mut detail_objects);
                let detail_tx = c.transaction().map_err(|e| e.to_string())?;
                for o in detail_objects {
                    let campaign_id = object_text(&o, &["campaignId", "campaign_id"]);
                    let sku = object_text(&o, &["sku"]);
                    if campaign_id.is_empty() || sku.is_empty() {
                        continue;
                    }
                    let returned_day = object_text(&o, &["date", "day"]);
                    let row_day = if returned_day.is_empty() {
                        day.clone()
                    } else {
                        returned_day.chars().take(10).collect()
                    };
                    detail_tx.execute("INSERT INTO ad_daily(day,campaign_id,campaign_name,sku,impressions,clicks,cart_adds,orders,revenue,spend,source)VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'api_product_sku') ON CONFLICT(day,campaign_id,sku) DO UPDATE SET campaign_name=excluded.campaign_name,impressions=excluded.impressions,clicks=excluded.clicks,cart_adds=excluded.cart_adds,orders=excluded.orders,revenue=excluded.revenue,spend=excluded.spend,source=excluded.source,updated_at=CURRENT_TIMESTAMP",params![row_day,campaign_id,names.get(&campaign_id).cloned().unwrap_or_default(),sku,object_number(&o,&["views","impressions"])as i64,object_number(&o,&["clicks"])as i64,object_number(&o,&["toCart","cartAdds"])as i64,object_number(&o,&["modelOrders","orders"])as i64,object_number(&o,&["modelSales","sales","revenue"]),object_number(&o,&["expense","spend"])]).map_err(|e|e.to_string())?;
                    count += 1;
                }
                detail_tx.commit().map_err(|e| e.to_string())?;
            }
        }
        Ok(count)
    })();
    match result {
        Ok(count) => {
            c.execute("UPDATE sync_logs SET finished_at=CURRENT_TIMESTAMP,status='success',rows_count=?1,message=?2 WHERE id=?3",params![count,format!("Performance 广告同步完成：{count} 行"),log_id]).map_err(|e|e.to_string())?;
            Ok(count)
        }
        Err(error) => {
            let _=c.execute("UPDATE sync_logs SET finished_at=CURRENT_TIMESTAMP,status='failed',message=?1 WHERE id=?2",params![error,log_id]);
            Err(error)
        }
    }
}

#[tauri::command]
async fn sync_performance_ads(
    range: DateRange,
    force: bool,
    state: State<'_, AppState>,
) -> Result<i64, String> {
    let owned = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || {
        sync_performance_ads_blocking(range, force, &owned)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn finance_period(
    value: &serde_json::Value,
    fallback_from: &str,
    fallback_to: &str,
) -> (String, String) {
    let from = json_text(value.pointer("/period/begin"));
    let from = if from.is_empty() {
        json_text(value.pointer("/period/from"))
    } else {
        from
    };
    let to = json_text(value.pointer("/period/end"));
    let to = if to.is_empty() {
        json_text(value.pointer("/period/to"))
    } else {
        to
    };
    (
        if from.is_empty() {
            fallback_from.into()
        } else {
            from.chars().take(10).collect()
        },
        if to.is_empty() {
            fallback_to.into()
        } else {
            to.chars().take(10).collect()
        },
    )
}

fn sync_finance_blocking(range: DateRange, force: bool, state: &AppState) -> Result<i64, String> {
    let _sync_guard = FINANCE_SYNC_LOCK
        .try_lock()
        .map_err(|_| "已有数据同步任务正在后台运行，请等待完成。".to_string())?;
    let mut c = db(state)?;
    c.execute("INSERT INTO sync_logs(started_at,source,status) VALUES(CURRENT_TIMESTAMP,'Seller Finance','running')",[]).map_err(|e|e.to_string())?;
    let log_id = c.last_insert_rowid();
    let result = (|| -> Result<i64, String> {
        let Some(range) = smart_sync_range(
            &c,
            "finance_transactions",
            "substr(operation_date,1,10)",
            &range,
            45,
            force,
        )?
        else {
            return Ok(0);
        };
        let mut operations = Vec::new();
        let ranges = {
            let mut stmt=c.prepare("WITH RECURSIVE m(v) AS(SELECT date(?1,'start of month') UNION ALL SELECT date(v,'+1 month') FROM m WHERE v<date(?2,'start of month'))SELECT max(v,?1),min(date(v,'+1 month','-1 day'),?2)FROM m").map_err(|e|e.to_string())?;
            let values = stmt
                .query_map(params![range.from, range.to], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            values
        };
        for (from, to) in &ranges {
            let mut page = 1;
            loop {
                let payload = seller_post(
                    &c,
                    "/v3/finance/transaction/list",
                    &serde_json::json!({"filter":{"date":{"from":format!("{from}T00:00:00Z"),"to":format!("{to}T23:59:59Z")},"operation_type":[],"posting_number":"","transaction_type":"all"},"page":page,"page_size":1000}),
                )?;
                let result = payload.get("result").unwrap_or(&payload);
                let batch = result
                    .get("operations")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                let page_count = result
                    .get("page_count")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(1);
                operations.extend(batch.iter().cloned());
                if batch.is_empty() || page >= page_count {
                    break;
                }
                page += 1;
            }
        }
        let statement = seller_post(
            &c,
            "/v1/finance/cash-flow-statement/list",
            &serde_json::json!({"date":{"from":format!("{}T00:00:00Z",range.from),"to":format!("{}T23:59:59Z",range.to)},"with_details":true,"page":1,"page_size":100}),
        )?;
        let result = statement.get("result").unwrap_or(&statement);
        let flows = result
            .get("cash_flows")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let details = result
            .get("details")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let tx = c.transaction().map_err(|e| e.to_string())?;
        // `/v3/finance/transaction/list` does not always return a stable
        // operation_id.  The previous fallback included the response index,
        // so a later sync with a different ordering inserted the same finance
        // operation again and inflated both Finance net and profit.  Fetch all
        // pages first, then atomically replace only the successfully fetched
        // date range.  A failed request therefore preserves the prior cache,
        // while repeated successful syncs are idempotent.
        tx.execute(
            "DELETE FROM finance_transactions WHERE substr(operation_date,1,10) BETWEEN ?1 AND ?2",
            params![range.from, range.to],
        )
        .map_err(|e| e.to_string())?;
        for (index, op) in operations.iter().enumerate() {
            let id = {
                let v = json_text(op.get("operation_id"));
                if v.is_empty() {
                    format!("generated-{index}-{}", json_text(op.get("operation_date")))
                } else {
                    v
                }
            };
            let mut skus = std::collections::BTreeSet::new();
            for item in op
                .get("items")
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten()
            {
                let sku = item
                    .get("sku")
                    .map(|v| {
                        v.as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| v.to_string())
                    })
                    .unwrap_or_default();
                if !sku.is_empty() {
                    skus.insert(sku);
                }
            }
            // Only a single, explicit item SKU is safe to attribute. Multi-SKU
            // and shop-level operations remain unallocated, matching Python's
            // `_normalize_finance_operation` audit rule.
            let sku = if skus.len() == 1 {
                skus.into_iter().next().unwrap_or_default()
            } else if skus.is_empty() {
                json_text(op.get("sku"))
            } else {
                String::new()
            };
            tx.execute("INSERT INTO finance_transactions(operation_id,operation_date,operation_type,posting_number,sku,amount,delivery_charge,return_delivery_charge,accruals_for_sale,sale_commission,raw_json)VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11) ON CONFLICT(operation_id) DO UPDATE SET operation_date=excluded.operation_date,operation_type=excluded.operation_type,posting_number=excluded.posting_number,sku=excluded.sku,amount=excluded.amount,delivery_charge=excluded.delivery_charge,return_delivery_charge=excluded.return_delivery_charge,accruals_for_sale=excluded.accruals_for_sale,sale_commission=excluded.sale_commission,raw_json=excluded.raw_json",params![id,json_text(op.get("operation_date")),json_text(op.get("operation_type")),json_text(op.pointer("/posting/posting_number")),sku,op.get("amount").and_then(|v|v.as_f64()).unwrap_or(0.0),op.get("delivery_charge").and_then(|v|v.as_f64()).unwrap_or(0.0),op.get("return_delivery_charge").and_then(|v|v.as_f64()).unwrap_or(0.0),op.get("accruals_for_sale").and_then(|v|v.as_f64()).unwrap_or(0.0),op.get("sale_commission").and_then(|v|v.as_f64()).unwrap_or(0.0),op.to_string()]).map_err(|e|e.to_string())?;
        }
        tx.execute(
            "DELETE FROM finance_cash_flows WHERE period_to>=?1 AND period_from<=?2",
            params![range.from, range.to],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "DELETE FROM finance_cash_flow_details WHERE period_to>=?1 AND period_from<=?2",
            params![range.from, range.to],
        )
        .map_err(|e| e.to_string())?;
        for (index, row) in flows.iter().enumerate() {
            let (from, to) = finance_period(row, &range.from, &range.to);
            let currency = json_text(row.get("currency_code"));
            tx.execute("INSERT INTO finance_cash_flows(row_id,period_from,period_to,currency_code,raw_json)VALUES(?1,?2,?3,?4,?5)",params![format!("{from}|{to}|{currency}|{index}"),from,to,currency,row.to_string()]).map_err(|e|e.to_string())?;
        }
        for (index, row) in details.iter().enumerate() {
            let (from, to) = finance_period(row, &range.from, &range.to);
            tx.execute("INSERT INTO finance_cash_flow_details(row_id,period_from,period_to,raw_json)VALUES(?1,?2,?3,?4)",params![format!("{from}|{to}|{index}"),from,to,row.to_string()]).map_err(|e|e.to_string())?;
        }
        tx.execute("DELETE FROM business_report_cache", [])
            .map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM analytics_detail_cache", [])
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(operations.len() as i64)
    })();
    match result {
        Ok(count) => {
            c.execute("UPDATE sync_logs SET finished_at=CURRENT_TIMESTAMP,status='success',rows_count=?1,message=?2 WHERE id=?3",params![count,format!("Finance 结算同步完成：{count} 笔逐笔应计"),log_id]).map_err(|e|e.to_string())?;
            Ok(count)
        }
        Err(error) => {
            let _=c.execute("UPDATE sync_logs SET finished_at=CURRENT_TIMESTAMP,status='failed',message=?1 WHERE id=?2",params![error,log_id]);
            Err(error)
        }
    }
}

#[tauri::command]
async fn sync_finance(
    range: DateRange,
    force: bool,
    state: State<'_, AppState>,
) -> Result<i64, String> {
    let owned = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || sync_finance_blocking(range, force, &owned))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn sync_all_data(
    range: DateRange,
    force: bool,
    state: State<'_, AppState>,
) -> Result<SyncAllResult, String> {
    let seller_state = background_state(&state)?;
    let performance_state = background_state(&state)?;
    let finance_state = background_state(&state)?;
    let seller_range = range.clone();
    let performance_range = range.clone();
    let seller = tauri::async_runtime::spawn_blocking(move || {
        let count = sync_seller_sales_blocking(seller_range.clone(), force, &seller_state)?;
        let _ = sync_fbs_orders_blocking(seller_range.clone(), &seller_state);
        let _ = sync_fbo_orders_blocking(seller_range.clone(), &seller_state);
        let _ = rebuild_cancellation_events(&seller_range, &seller_state);
        let _ = sync_customer_returns_blocking(&seller_range, &seller_state);
        Ok::<i64, String>(count)
    });
    let performance = tauri::async_runtime::spawn_blocking(move || {
        sync_performance_ads_blocking(performance_range, force, &performance_state)
    });
    let finance = tauri::async_runtime::spawn_blocking(move || {
        sync_finance_blocking(range, force, &finance_state)
    });
    let seller = seller.await.map_err(|e| e.to_string())?;
    let performance = performance.await.map_err(|e| e.to_string())?;
    let finance = finance.await.map_err(|e| e.to_string())?;
    Ok(SyncAllResult {
        seller_rows: seller.as_ref().ok().copied(),
        performance_rows: performance.as_ref().ok().copied(),
        finance_rows: finance.as_ref().ok().copied(),
        seller_error: seller.err().unwrap_or_default(),
        performance_error: performance.err().unwrap_or_default(),
        finance_error: finance.err().unwrap_or_default(),
    })
}

#[tauri::command]
fn test_feishu(state: State<AppState>) -> Result<String, String> {
    let c = db(&state)?;
    let token = feishu_token(&c)?;
    let app = setting(&c, "feishu_app_token");
    let table = setting(&c, "feishu_product_table_id");
    if app.is_empty() || table.is_empty() {
        return Ok("飞书应用认证成功；填写 App Token 和商品表 Table ID 后可测试多维表格。".into());
    }
    let path = feishu_table_path(&c)?;
    let payload = feishu_raw(
        "GET",
        &format!("{path}/fields?page_size=100"),
        Some(&token),
        None,
    )?;
    let count = payload
        .pointer("/data/items")
        .and_then(|v| v.as_array())
        .map(Vec::len)
        .unwrap_or(0);
    Ok(format!(
        "飞书认证及商品多维表格连接成功，共读取 {count} 个字段。"
    ))
}

fn feishu_records(token: &str, path: &str) -> Result<Vec<serde_json::Value>, String> {
    let mut rows = Vec::new();
    let mut page_token = String::new();
    loop {
        let suffix = if page_token.is_empty() {
            "?page_size=500&automatic_fields=true".into()
        } else {
            format!("?page_size=500&automatic_fields=true&page_token={page_token}")
        };
        let payload = feishu_raw("GET", &format!("{path}/records{suffix}"), Some(token), None)?;
        rows.extend(
            payload
                .pointer("/data/items")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default(),
        );
        let more = payload
            .pointer("/data/has_more")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        page_token = json_text(payload.pointer("/data/page_token"));
        if !more || page_token.is_empty() {
            break;
        }
    }
    Ok(rows)
}

fn feishu_product_record_belongs_to_shop(record: &serde_json::Value, active_shop_id: &str) -> bool {
    feishu_field_text(record.pointer("/fields/店铺ID")) == active_shop_id
}

fn sync_feishu_products_blocking(direction: String, state: &AppState) -> Result<String, String> {
    if !matches!(direction.as_str(), "pull" | "push" | "both") {
        return Err("飞书商品同步方向无效".into());
    }
    let mut c = db(&state)?;
    let (active_shop_id, active_shop_name) = active_shop_identity(state)?;
    let token = feishu_token(&c)?;
    let path = feishu_table_path(&c)?;
    c.execute("INSERT INTO sync_logs(started_at,source,status) VALUES(CURRENT_TIMESTAMP,'Feishu Products','running')",[]).map_err(|e|e.to_string())?;
    let log_id = c.last_insert_rowid();
    let result = (|| -> Result<(i64, i64, i64), String> {
        let definitions = [
            ("店铺ID", 1),
            ("店铺名称", 1),
            ("SKU", 1),
            ("货号", 1),
            ("商品名称", 1),
            ("采购成本 CNY", 2),
            ("头程 RUB", 2),
            ("单件成本", 2),
            ("单件头程", 2),
            ("备注", 1),
            ("本地更新时间", 1),
        ];
        let fields_payload = feishu_raw(
            "GET",
            &format!("{path}/fields?page_size=100"),
            Some(&token),
            None,
        )?;
        let fields = fields_payload
            .pointer("/data/items")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let existing_names: std::collections::BTreeSet<String> = fields
            .iter()
            .map(|v| json_text(v.get("field_name")))
            .collect();
        for (name, kind) in definitions {
            if !existing_names.contains(name) {
                feishu_raw(
                    "POST",
                    &format!("{path}/fields"),
                    Some(&token),
                    Some(&serde_json::json!({"field_name":name,"type":kind})),
                )?;
            }
        }
        let records = feishu_records(&token, &path)?;
        let mut remote = std::collections::HashMap::new();
        for record in records {
            let sku = feishu_field_text(record.pointer("/fields/SKU"));
            // Product costs are private to one shop. Legacy records without a
            // shop id are deliberately ignored instead of being guessed from
            // SKU, because the same SKU may have different purchase terms.
            if feishu_product_record_belongs_to_shop(&record, &active_shop_id)
                && !sku.is_empty()
                && !remote.contains_key(&sku)
            {
                remote.insert(sku, record);
            }
        }
        let mut pulled = 0;
        if matches!(direction.as_str(), "pull" | "both") {
            let tx = c.transaction().map_err(|e| e.to_string())?;
            for (sku, record) in &remote {
                let fields = record.get("fields");
                let unit_cny = feishu_field_number(fields.and_then(|v| v.get("采购成本 CNY")));
                let first_rub = feishu_field_number(fields.and_then(|v| v.get("头程 RUB")));
                let legacy_unit = feishu_field_number(fields.and_then(|v| v.get("单件成本")));
                let legacy_first = feishu_field_number(fields.and_then(|v| v.get("单件头程")));
                if unit_cny.is_none()
                    && first_rub.is_none()
                    && legacy_unit.is_none()
                    && legacy_first.is_none()
                {
                    continue;
                }
                tx.execute("INSERT INTO product_costs(sku,unit_cost,first_mile_cost,unit_cost_cny,note)VALUES(?1,?2,?3,?4,?5) ON CONFLICT(sku) DO UPDATE SET unit_cost=COALESCE(excluded.unit_cost,product_costs.unit_cost),first_mile_cost=COALESCE(excluded.first_mile_cost,product_costs.first_mile_cost),unit_cost_cny=COALESCE(excluded.unit_cost_cny,product_costs.unit_cost_cny),note=CASE WHEN excluded.note='' THEN product_costs.note ELSE excluded.note END,updated_at=CURRENT_TIMESTAMP",params![sku,legacy_unit.or(unit_cny),first_rub.or(legacy_first),unit_cny,feishu_field_text(fields.and_then(|v|v.get("备注")))]).map_err(|e|e.to_string())?;
                pulled += 1;
            }
            tx.commit().map_err(|e| e.to_string())?;
        }
        let mut creates = Vec::new();
        let mut updates = Vec::new();
        if matches!(direction.as_str(), "push" | "both") {
            let local = {
                let mut stmt=c.prepare("WITH known AS(SELECT sku FROM products UNION SELECT sku FROM sales_daily UNION SELECT sku FROM product_costs)SELECT k.sku,COALESCE(p.offer_id,''),COALESCE(p.name,MAX(s.product_name),''),pc.unit_cost,pc.first_mile_cost,pc.unit_cost_cny,COALESCE(pc.note,''),COALESCE(pc.updated_at,'')FROM known k LEFT JOIN products p ON p.sku=k.sku LEFT JOIN sales_daily s ON s.sku=k.sku LEFT JOIN product_costs pc ON pc.sku=k.sku GROUP BY k.sku").map_err(|e|e.to_string())?;
                let rows = stmt
                    .query_map([], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, Option<f64>>(3)?,
                            r.get::<_, Option<f64>>(4)?,
                            r.get::<_, Option<f64>>(5)?,
                            r.get::<_, String>(6)?,
                            r.get::<_, String>(7)?,
                        ))
                    })
                    .map_err(|e| e.to_string())?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| e.to_string())?;
                rows
            };
            for row in local {
                let mut fields = serde_json::Map::new();
                fields.insert("店铺ID".into(), active_shop_id.clone().into());
                fields.insert("店铺名称".into(), active_shop_name.clone().into());
                fields.insert("SKU".into(), row.0.clone().into());
                fields.insert("货号".into(), row.1.into());
                fields.insert("商品名称".into(), row.2.into());
                fields.insert("备注".into(), row.6.into());
                fields.insert("本地更新时间".into(), row.7.into());
                if let Some(v) = row.3 {
                    fields.insert("单件成本".into(), v.into());
                }
                if let Some(v) = row.4 {
                    fields.insert("单件头程".into(), v.into());
                    fields.insert("头程 RUB".into(), v.into());
                }
                if let Some(v) = row.5 {
                    fields.insert("采购成本 CNY".into(), v.into());
                }
                if let Some(record) = remote.get(&row.0) {
                    updates.push(serde_json::json!({"record_id":json_text(record.get("record_id")),"fields":fields}));
                } else {
                    creates.push(serde_json::json!({"fields":fields}));
                }
            }
            for chunk in creates.chunks(500) {
                feishu_raw(
                    "POST",
                    &format!("{path}/records/batch_create"),
                    Some(&token),
                    Some(&serde_json::json!({"records":chunk})),
                )?;
            }
            for chunk in updates.chunks(500) {
                feishu_raw(
                    "POST",
                    &format!("{path}/records/batch_update"),
                    Some(&token),
                    Some(&serde_json::json!({"records":chunk})),
                )?;
            }
        }
        Ok((pulled, creates.len() as i64, updates.len() as i64))
    })();
    match result {
        Ok((pulled, created, updated)) => {
            let total = pulled + created + updated;
            let message =
                format!("飞书商品同步完成：读取 {pulled}，新增 {created}，更新 {updated}");
            c.execute("UPDATE sync_logs SET finished_at=CURRENT_TIMESTAMP,status='success',rows_count=?1,message=?2 WHERE id=?3",params![total,message,log_id]).map_err(|e|e.to_string())?;
            Ok(message)
        }
        Err(error) => {
            let _=c.execute("UPDATE sync_logs SET finished_at=CURRENT_TIMESTAMP,status='failed',message=?1 WHERE id=?2",params![error,log_id]);
            Err(error)
        }
    }
}

#[tauri::command]
async fn sync_feishu_products(
    direction: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let owned = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || sync_feishu_products_blocking(direction, &owned))
        .await
        .map_err(|e| format!("飞书商品后台同步失败：{e}"))?
}

#[tauri::command]
fn send_feishu_weekly(range: DateRange, state: State<AppState>) -> Result<String, String> {
    let c = db(&state)?;
    let token = feishu_token(&c)?;
    let chat = setting(&c, "feishu_chat_id");
    if chat.is_empty() {
        return Err("请先配置飞书群 Chat ID".into());
    }
    let(revenue,units,delivered,returns,cancellations):(f64,i64,i64,i64,i64)=c.query_row("SELECT COALESCE(SUM(revenue),0),COALESCE(SUM(ordered_units),0),COALESCE(SUM(delivered_units),0),COALESCE(SUM(returns),0),COALESCE(SUM(cancellations),0)FROM sales_daily WHERE day BETWEEN ?1 AND ?2",params![range.from,range.to],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).map_err(|e|e.to_string())?;
    let(spend,ad_revenue,ad_orders):(f64,f64,i64)=c.query_row("SELECT COALESCE(SUM(spend),0),COALESCE(SUM(revenue),0),COALESCE(SUM(orders),0)FROM ad_daily WHERE day BETWEEN ?1 AND ?2 AND sku=''",params![range.from,range.to],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e|e.to_string())?;
    let acos = if ad_revenue > 0.0 {
        format!("{:.2}%", spend / ad_revenue * 100.0)
    } else {
        "—".into()
    };
    let tacos = if revenue > 0.0 {
        format!("{:.2}%", spend / revenue * 100.0)
    } else {
        "—".into()
    };
    let content=format!("**期间：** {} 至 {}\n**销售额：** {:.2}\n**销量：** {} 件　**妥投：** {} 件\n**退货：** {} 件　**取消：** {} 件\n**广告花费：** {:.2}\n**广告销售：** {:.2}　**广告订单：** {}\n**ACOS：** {}　**TACOS：** {}",range.from,range.to,revenue,units,delivered,returns,cancellations,spend,ad_revenue,ad_orders,acos,tacos);
    let card = serde_json::json!({"config":{"wide_screen_mode":true,"enable_forward":true},"header":{"template":"blue","title":{"tag":"plain_text","content":"Ozon 店铺经营周报"},"subtitle":{"tag":"plain_text","content":format!("{} 至 {}",range.from,range.to)}},"elements":[{"tag":"div","text":{"tag":"lark_md","content":content}},{"tag":"note","elements":[{"tag":"plain_text","content":"数据来自当前店铺本地 SQLite 缓存，由用户手动发送"}]}]});
    let payload = feishu_raw(
        "POST",
        &format!("{}/im/v1/messages?receive_id_type=chat_id", feishu_base(&c)),
        Some(&token),
        Some(
            &serde_json::json!({"receive_id":chat,"msg_type":"interactive","content":card.to_string()}),
        ),
    )?;
    let id = json_text(payload.pointer("/data/message_id"));
    Ok(if id.is_empty() {
        "飞书周报已发送".into()
    } else {
        format!("飞书周报已发送，消息 ID：{id}")
    })
}

fn first_feishu_field(fields: Option<&serde_json::Value>, names: &[&str]) -> String {
    for name in names {
        let value = feishu_field_text(fields.and_then(|v| v.get(*name)));
        if !value.is_empty() {
            return value;
        }
    }
    String::new()
}
fn feishu_date(c: &Connection, value: Option<&serde_json::Value>) -> String {
    if let Some(number) = value.and_then(|v| v.as_f64()) {
        let seconds = if number > 100_000_000_000.0 {
            number / 1000.0
        } else {
            number
        };
        return c
            .query_row("SELECT date(?1,'unixepoch','localtime')", [seconds], |r| {
                r.get(0)
            })
            .unwrap_or_default();
    }
    let text = feishu_field_text(value)
        .replace(['年', '月'], "-")
        .replace('日', "")
        .replace(['/', '.'], "-");
    text.split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string()
}

#[tauri::command]
fn shipment_tracking(state: State<AppState>) -> Result<Vec<ShipmentRow>, String> {
    let c = db(&state)?;
    let mut allocations = std::collections::HashMap::<String, Vec<ShipmentSkuAllocation>>::new();
    {
        let mut allocation_stmt=c.prepare("SELECT tracking_id,sku,quantity FROM shipment_sku_allocations ORDER BY tracking_id,sku").map_err(|e|e.to_string())?;
        let allocation_rows = allocation_stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    ShipmentSkuAllocation {
                        sku: r.get(1)?,
                        quantity: r.get(2)?,
                    },
                ))
            })
            .map_err(|e| e.to_string())?;
        for row in allocation_rows {
            let (tracking_id, item) = row.map_err(|e| e.to_string())?;
            allocations.entry(tracking_id).or_default().push(item);
        }
    }
    let mut stmt=c.prepare("SELECT tracking_id,product_name,batch_no,shop_name,quantity,cargo_status,channel,domestic_arrival,foreign_arrival,notified_foreign_arrival,source,updated_at FROM shipment_tracking ORDER BY CASE WHEN foreign_arrival<>'' AND foreign_arrival<>notified_foreign_arrival THEN 0 ELSE 1 END,updated_at DESC").map_err(|e|e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            let foreign: String = r.get(8)?;
            let notified: String = r.get(9)?;
            let tracking_id: String = r.get(0)?;
            let settlement_completed = c.query_row("SELECT COUNT(*)>0 AND MIN(settled)=1 FROM shipment_sku_allocations WHERE tracking_id=?1",[&tracking_id],|row|row.get(0)).unwrap_or(false);
            Ok(ShipmentRow {
                sku_allocations: allocations.get(&tracking_id).cloned().unwrap_or_default(),
                tracking_id,
                product_name: r.get(1)?,
                batch_no: r.get(2)?,
                shop_name: r.get(3)?,
                quantity: r.get(4)?,
                cargo_status: r.get(5)?,
                channel: r.get(6)?,
                domestic_arrival: r.get(7)?,
                foreign_arrival: foreign.clone(),
                notified_foreign_arrival: notified.clone(),
                source: r.get(10)?,
                updated_at: r.get(11)?,
                needs_notification: !foreign.is_empty() && foreign != notified,
                settlement_completed,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

fn sync_feishu_shipments_blocking(state: &AppState) -> Result<i64, String> {
    let mut c = db(state)?;
    let source = resolve_feishu_supply_source(state)?;
    let token = feishu_token_for_source(&source)?;
    let path = format!(
        "{}/bitable/v1/apps/{}/tables/{}",
        source.base_url, source.app_token, source.tracking_table_id
    );
    let records = feishu_records(&token, &path).map_err(|error| {
        if error.contains("1254041") || error.contains("TableIdNotFound") {
            format!("发货跟踪表不存在或不属于当前 App Token。请在飞书打开正确的发货跟踪多维表格，复制链接中 /base/ 后的 App Token，以及 table= 后以 tbl 开头的 Table ID；不要填写 view= 后的视图 ID。配置来源店铺：{}。原始错误：{error}", source.shop_name)
        } else {
            error
        }
    })?;
    let mut sku_candidates = std::collections::HashMap::<String, Option<String>>::new();
    {
        let mut stmt = c
            .prepare("SELECT sku,offer_id,name FROM products WHERE sku<>''")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        for row in rows {
            let (sku, offer, name) = row.map_err(|e| e.to_string())?;
            add_unique_sku_candidate(&mut sku_candidates, &sku, &sku);
            add_unique_sku_candidate(&mut sku_candidates, &offer, &sku);
            add_unique_sku_candidate(&mut sku_candidates, &name, &sku);
        }
        let mut stmt = c
            .prepare(
                "SELECT product_name,sku FROM feishu_supply_chain_product_mappings WHERE sku<>''",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(|e| e.to_string())?;
        for row in rows {
            let (name, sku) = row.map_err(|e| e.to_string())?;
            add_unique_sku_candidate(&mut sku_candidates, &name, &sku);
        }
    }
    if !source.product_table_id.is_empty() {
        let product_path = format!(
            "{}/bitable/v1/apps/{}/tables/{}",
            source.base_url, source.app_token, source.product_table_id
        );
        if let Ok(product_records) = feishu_records(&token, &product_path) {
            for record in product_records {
                let fields = record.get("fields");
                let sku = first_feishu_field(fields, &["SKU", "Ozon SKU", "商品SKU"]);
                let name = first_feishu_field(fields, &["品名", "产品名称", "商品名称"]);
                add_unique_sku_candidate(&mut sku_candidates, &name, &sku);
            }
        }
    }
    let tx = c.transaction().map_err(|e| e.to_string())?;
    let mut count = 0;
    let mut matched = 0;
    let mut status_totals = [0i64; 4];
    for record in records {
        let fields = record.get("fields");
        let tracking = first_feishu_field(fields, &["采购单号", "跟踪单号", "物流单号", "单号"]);
        if tracking.is_empty() {
            continue;
        }
        let quantity = first_feishu_field(fields, &["数量", "发货数量"])
            .replace(' ', "")
            .parse::<f64>()
            .unwrap_or(0.0) as i64;
        let domestic = feishu_date(
            &tx,
            fields
                .and_then(|v| v.get("国内到库"))
                .or_else(|| fields.and_then(|v| v.get("国内到仓"))),
        );
        let foreign = feishu_date(
            &tx,
            fields
                .and_then(|v| v.get("国外到库"))
                .or_else(|| fields.and_then(|v| v.get("国外到仓"))),
        );
        let foreign_appointment = feishu_date(&tx, fields.and_then(|v| v.get("国外约仓")));
        let product_name = first_feishu_field(fields, &["品名", "产品名称", "商品名称"]);
        let explicit_sku = first_feishu_field(fields, &["SKU", "Ozon SKU", "商品SKU"]);
        let sku = if !explicit_sku.is_empty() {
            explicit_sku
        } else {
            sku_candidates
                .get(&normalized_product_key(&product_name))
                .and_then(|value| value.clone())
                .unwrap_or_default()
        };
        if !sku.is_empty() {
            matched += 1;
        }
        let cargo_status = first_feishu_field(fields, &["货物状态", "状态"]);
        let (production, domestic_stock, overseas_transit, overseas_arrived) =
            supply_quantities_for_status(&cargo_status, quantity);
        match cargo_status.trim() {
            "未送仓" => status_totals[0] += quantity,
            "在途" => status_totals[1] += quantity,
            "到达海外仓" => status_totals[2] += quantity,
            "已送仓" => status_totals[3] += quantity,
            _ => {}
        }
        tx.execute("INSERT INTO shipment_tracking(tracking_id,product_name,batch_no,shop_name,quantity,cargo_status,channel,domestic_arrival,foreign_arrival,foreign_appointment,source,remote_record_id,sku,production_qty,domestic_stock_qty,overseas_transit_qty,overseas_arrived_qty,supply_source_shop_id,status_formula_version)VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'feishu',?11,?12,?13,?14,?15,?16,?17,?18) ON CONFLICT(tracking_id) DO UPDATE SET product_name=excluded.product_name,batch_no=excluded.batch_no,shop_name=excluded.shop_name,quantity=excluded.quantity,cargo_status=excluded.cargo_status,channel=excluded.channel,domestic_arrival=excluded.domestic_arrival,foreign_arrival=excluded.foreign_arrival,foreign_appointment=excluded.foreign_appointment,source='feishu',remote_record_id=excluded.remote_record_id,sku=excluded.sku,production_qty=excluded.production_qty,domestic_stock_qty=excluded.domestic_stock_qty,overseas_transit_qty=excluded.overseas_transit_qty,overseas_arrived_qty=excluded.overseas_arrived_qty,supply_source_shop_id=excluded.supply_source_shop_id,status_formula_version=excluded.status_formula_version,updated_at=CURRENT_TIMESTAMP",params![tracking,product_name,first_feishu_field(fields,&["批次号","批次"]),first_feishu_field(fields,&["店铺","店铺名称"]),quantity,cargo_status,first_feishu_field(fields,&["渠道","物流渠道"]),domestic,foreign,foreign_appointment,json_text(record.get("record_id")),sku,production,domestic_stock,overseas_transit,overseas_arrived,source.shop_id,FEISHU_CARGO_STATUS_FORMULA_VERSION]).map_err(|e|e.to_string())?;
        count += 1;
    }
    tx.execute("INSERT INTO feishu_supply_chain_sync_runs(source_shop_id,source_shop_name,records_count,matched_count,unmatched_count,unshipped_qty,transit_qty,overseas_arrived_qty,delivered_qty,status_formula_version)VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",params![source.shop_id,source.shop_name,count,matched,count-matched,status_totals[0],status_totals[1],status_totals[2],status_totals[3],FEISHU_CARGO_STATUS_FORMULA_VERSION]).map_err(|e|e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(count)
}

#[tauri::command]
async fn sync_feishu_shipments(state: State<'_, AppState>) -> Result<i64, String> {
    let owned = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || sync_feishu_shipments_blocking(&owned))
        .await
        .map_err(|e| format!("飞书发货后台同步失败：{e}"))?
}

#[tauri::command]
fn notify_feishu_shipment(tracking_id: String, state: State<AppState>) -> Result<String, String> {
    let c = db(&state)?;
    let row=c.query_row("SELECT product_name,batch_no,shop_name,quantity,cargo_status,channel,domestic_arrival,foreign_arrival FROM shipment_tracking WHERE tracking_id=?1",[&tracking_id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,i64>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,r.get::<_,String>(6)?,r.get::<_,String>(7)?))).map_err(|_|"未找到该发货跟踪记录".to_string())?;
    if row.7.is_empty() {
        return Err("该记录尚无国外到库日期，不能发送通知".into());
    }
    let chat = setting(&c, "feishu_chat_id");
    if chat.is_empty() {
        return Err("请先配置飞书群 Chat ID".into());
    }
    let token = feishu_token(&c)?;
    let content=format!("**采购/跟踪单号：** {}\n**品名：** {}\n**批次号：** {}\n**店铺：** {}\n**数量：** {}\n**渠道：** {}\n**货物状态：** {}\n**国内到库：** {}\n**国外到库：** {}",tracking_id,row.0,row.1,row.2,row.3,row.5,row.4,row.6,row.7);
    let card = serde_json::json!({"config":{"wide_screen_mode":true,"enable_forward":true},"header":{"template":"green","title":{"tag":"plain_text","content":format!("{}｜货物已到国外仓",if row.2.is_empty(){"Ozon 店铺"}else{&row.2})},"subtitle":{"tag":"plain_text","content":format!("到库日期：{}",row.7)}},"elements":[{"tag":"div","text":{"tag":"lark_md","content":content}},{"tag":"note","elements":[{"tag":"plain_text","content":"由 Ozon Analytics 发货跟踪手动发送"}]}]});
    let payload = feishu_raw(
        "POST",
        &format!("{}/im/v1/messages?receive_id_type=chat_id", feishu_base(&c)),
        Some(&token),
        Some(
            &serde_json::json!({"receive_id":chat,"msg_type":"interactive","content":card.to_string()}),
        ),
    )?;
    c.execute("UPDATE shipment_tracking SET notified_foreign_arrival=foreign_arrival,updated_at=CURRENT_TIMESTAMP WHERE tracking_id=?1",[&tracking_id]).map_err(|e|e.to_string())?;
    let id = json_text(payload.pointer("/data/message_id"));
    Ok(if id.is_empty() {
        "到库通知已发送".into()
    } else {
        format!("到库通知已发送，消息 ID：{id}")
    })
}

#[tauri::command]
fn supply_cluster_plans(
    target_days: i64,
    query: String,
    state: State<AppState>,
) -> Result<Vec<SupplyClusterPlanRow>, String> {
    let target_days = target_days.clamp(1, 365);
    let pattern = format!("%{}%", query.trim());
    let c = db(&state)?;
    let mut stmt = c.prepare("WITH stock AS(SELECT sku,MAX(offer_id) offer_id,MAX(product_name) product_name,macrolocal_cluster_id,MAX(cluster_name) cluster_name,SUM(available_stock) available_stock,SUM(transit_stock) transit_stock,SUM(requested_stock) requested_stock,MAX(ads_cluster) daily_sales FROM inventory_stock WHERE macrolocal_cluster_id<>'' GROUP BY sku,macrolocal_cluster_id) SELECT s.sku,s.offer_id,s.product_name,s.macrolocal_cluster_id,COALESCE(NULLIF(s.cluster_name,''),'未命名集群'),s.available_stock,s.transit_stock,s.requested_stock,COALESCE(s.daily_sales,0),rp.planned_qty FROM stock s LEFT JOIN replenishment_plan rp ON rp.sku=s.sku AND rp.macrolocal_cluster_id=s.macrolocal_cluster_id WHERE ?1='%%' OR s.sku LIKE ?1 OR s.offer_id LIKE ?1 OR s.product_name LIKE ?1 OR s.cluster_name LIKE ?1 ORDER BY s.offer_id,s.daily_sales DESC,s.cluster_name LIMIT 1000").map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([pattern], |row| {
            let available = row.get::<_, i64>(5)?;
            let transit = row.get::<_, i64>(6)?;
            let requested = row.get::<_, i64>(7)?;
            let daily = row.get::<_, f64>(8)?;
            let saved = row.get::<_, Option<i64>>(9)?;
            let recommended =
                ((daily * target_days as f64).ceil() as i64 - available - transit - requested)
                    .max(0);
            Ok(SupplyClusterPlanRow {
                sku: row.get(0)?,
                offer_id: row.get(1)?,
                product_name: row.get(2)?,
                macrolocal_cluster_id: row.get(3)?,
                cluster_name: row.get(4)?,
                available_stock: available,
                transit_stock: transit,
                requested_stock: requested,
                daily_sales: daily,
                recommended_qty: recommended,
                planned_qty: saved.unwrap_or(recommended),
                target_days,
                plan_saved: saved.is_some(),
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

#[tauri::command]
fn shipment_settlement(
    tracking_id: String,
    state: State<AppState>,
) -> Result<Vec<ShipmentSettlementItem>, String> {
    let c = db(&state)?;
    let mut stmt=c.prepare("SELECT a.sku,a.quantity,COALESCE((SELECT SUM(requested_stock) FROM inventory_stock i WHERE i.sku=a.sku),0),a.fbo_quantity,a.fbs_quantity,a.overseas_remaining_quantity,a.loss_quantity,a.other_quantity,a.settlement_note FROM shipment_sku_allocations a WHERE a.tracking_id=?1 ORDER BY a.sku").map_err(|e|e.to_string())?;
    let rows = stmt
        .query_map([tracking_id], |r| {
            Ok(ShipmentSettlementItem {
                sku: r.get(0)?,
                batch_quantity: r.get(1)?,
                requested_stock: r.get(2)?,
                fbo_quantity: r.get(3)?,
                fbs_quantity: r.get(4)?,
                overseas_remaining_quantity: r.get(5)?,
                loss_quantity: r.get(6)?,
                other_quantity: r.get(7)?,
                note: r.get(8)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    if rows.is_empty() {
        Err("该批次尚未配置 SKU 明细，不能结算".into())
    } else {
        Ok(rows)
    }
}

#[tauri::command]
fn settle_shipment(
    tracking_id: String,
    items: Vec<ShipmentSettlementItem>,
    state: State<AppState>,
) -> Result<(), String> {
    let mut c = db(&state)?;
    let status: String = c
        .query_row(
            "SELECT cargo_status FROM shipment_tracking WHERE tracking_id=?1",
            [tracking_id.trim()],
            |r| r.get(0),
        )
        .map_err(|_| "未找到该批次".to_string())?;
    if !matches!(status.trim(), "已送仓" | "已申请") {
        return Err("只有状态为“已申请/已送仓”的批次才能审核完结".into());
    }
    let tx = c.transaction().map_err(|e| e.to_string())?;
    let expected: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM shipment_sku_allocations WHERE tracking_id=?1",
            [tracking_id.trim()],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if expected != items.len() as i64 {
        return Err("必须审核该批次的全部 SKU".into());
    }
    for item in items {
        let values = [
            item.fbo_quantity,
            item.fbs_quantity,
            item.overseas_remaining_quantity,
            item.loss_quantity,
            item.other_quantity,
        ];
        if values.iter().any(|v| *v < 0) {
            return Err(format!("{} 的处置数量不能为负数", item.sku));
        }
        if values.iter().sum::<i64>() != item.batch_quantity {
            return Err(format!(
                "{} 的处置合计必须等于批次数量 {}",
                item.sku, item.batch_quantity
            ));
        }
        tx.execute("UPDATE shipment_sku_allocations SET fbo_quantity=?3,fbs_quantity=?4,overseas_remaining_quantity=?5,loss_quantity=?6,other_quantity=?7,settlement_note=?8,settled=1,settled_at=CURRENT_TIMESTAMP,updated_at=CURRENT_TIMESTAMP WHERE tracking_id=?1 AND sku=?2 AND quantity=?9",params![tracking_id.trim(),item.sku,item.fbo_quantity,item.fbs_quantity,item.overseas_remaining_quantity,item.loss_quantity,item.other_quantity,item.note,item.batch_quantity]).map_err(|e|e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())
}

#[tauri::command]
fn shipment_sku_options(
    query: String,
    state: State<AppState>,
) -> Result<Vec<ShipmentSkuOption>, String> {
    let c = db(&state)?;
    let pattern = format!("%{}%", query.trim());
    let mut stmt=c.prepare("SELECT sku,offer_id,name FROM products WHERE sku<>'' AND (?1='%%' OR sku LIKE ?1 OR offer_id LIKE ?1 OR name LIKE ?1) ORDER BY offer_id,sku LIMIT 300").map_err(|e|e.to_string())?;
    let rows = stmt
        .query_map([pattern], |r| {
            Ok(ShipmentSkuOption {
                sku: r.get(0)?,
                offer_id: r.get(1)?,
                name: r.get(2)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

#[tauri::command]
fn save_shipment_sku_allocations(
    tracking_id: String,
    allocations: Vec<ShipmentSkuAllocation>,
    state: State<AppState>,
) -> Result<(), String> {
    let tracking_id = tracking_id.trim();
    if tracking_id.is_empty() {
        return Err("跟踪单号不能为空".into());
    }
    let mut c = db(&state)?;
    let shipment_qty: i64 = c
        .query_row(
            "SELECT quantity FROM shipment_tracking WHERE tracking_id=?1",
            [tracking_id],
            |r| r.get(0),
        )
        .map_err(|_| "未找到该发货批次".to_string())?;
    let mut merged = std::collections::HashMap::<String, i64>::new();
    for item in allocations {
        let sku = item.sku.trim();
        if sku.is_empty() || item.quantity <= 0 {
            return Err("每条明细必须选择 SKU，补货数量必须大于 0".into());
        }
        if c.query_row(
            "SELECT EXISTS(SELECT 1 FROM products WHERE sku=?1)",
            [sku],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0)
            == 0
        {
            return Err(format!("SKU {sku} 不存在于当前店铺商品资料"));
        }
        *merged.entry(sku.to_string()).or_default() += item.quantity;
    }
    let total: i64 = merged.values().sum();
    if total > shipment_qty {
        return Err(format!(
            "SKU 明细合计 {total} 件，不能超过该批次总数量 {shipment_qty} 件"
        ));
    }
    let tx = c.transaction().map_err(|e| e.to_string())?;
    tx.execute(
        "DELETE FROM shipment_sku_allocations WHERE tracking_id=?1",
        [tracking_id],
    )
    .map_err(|e| e.to_string())?;
    for (sku, quantity) in merged {
        tx.execute(
            "INSERT INTO shipment_sku_allocations(tracking_id,sku,quantity)VALUES(?1,?2,?3)",
            params![tracking_id, sku, quantity],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())
}

const FEISHU_CARGO_STATUS_FORMULA_VERSION: &str = "cargo-status-v1:fldcysFTRb";

#[derive(Clone)]
struct FeishuSupplySource {
    shop_id: String,
    shop_name: String,
    base_url: String,
    app_id: String,
    app_secret: String,
    app_token: String,
    tracking_table_id: String,
    product_table_id: String,
}

fn resolve_feishu_supply_source(state: &AppState) -> Result<FeishuSupplySource, String> {
    let registry = read_registry(&state.data_dir)?;
    let active_id = state
        .active_shop_id
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    let mut shops = registry.shops.iter().collect::<Vec<_>>();
    shops.sort_by_key(|shop| if shop.id == active_id { 0 } else { 1 });
    for shop in shops {
        let path = state.data_dir.join(&shop.database_file);
        let Ok(conn) = Connection::open(&path) else {
            continue;
        };
        let app_id = setting(&conn, "feishu_app_id");
        let app_token = setting(&conn, "feishu_app_token");
        let tracking_table_id = setting(&conn, "feishu_tracking_table_id");
        if app_id.is_empty() || app_token.is_empty() || tracking_table_id.is_empty() {
            continue;
        }
        let Ok(app_secret) = secret_setting(&conn, "feishu_app_secret") else {
            continue;
        };
        if app_secret.is_empty() {
            continue;
        }
        return Ok(FeishuSupplySource {
            shop_id: shop.id.clone(),
            shop_name: shop.name.clone(),
            base_url: feishu_base(&conn),
            app_id,
            app_secret,
            app_token,
            tracking_table_id,
            product_table_id: setting(&conn, "feishu_product_table_id"),
        });
    }
    Err("当前店铺及其他店铺均未找到完整的飞书供应链配置（App ID、App Secret、App Token、发货跟踪 Table ID）".into())
}

fn feishu_token_for_source(source: &FeishuSupplySource) -> Result<String, String> {
    let payload = feishu_raw(
        "POST",
        &format!("{}/auth/v3/tenant_access_token/internal/", source.base_url),
        None,
        Some(&serde_json::json!({"app_id":source.app_id,"app_secret":source.app_secret})),
    )?;
    let token = json_text(payload.get("tenant_access_token"));
    if token.is_empty() {
        Err("飞书认证响应缺少 tenant_access_token".into())
    } else {
        Ok(token)
    }
}

fn normalized_product_key(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

fn add_unique_sku_candidate(
    candidates: &mut std::collections::HashMap<String, Option<String>>,
    product_name: &str,
    sku: &str,
) {
    let key = normalized_product_key(product_name);
    let sku = sku.trim();
    if key.is_empty() || sku.is_empty() {
        return;
    }
    candidates
        .entry(key)
        .and_modify(|current| {
            if current.as_deref() != Some(sku) {
                *current = None;
            }
        })
        .or_insert_with(|| Some(sku.to_string()));
}

fn supply_quantities_for_status(status: &str, quantity: i64) -> (i64, i64, i64, i64) {
    match status.trim() {
        "未送仓" => (quantity, 0, 0, 0),
        "在途" => (0, 0, quantity, 0),
        "到达海外仓" => (0, 0, 0, quantity),
        "已送仓" => (0, 0, 0, 0),
        _ => (0, 0, 0, 0),
    }
}

#[tauri::command]
fn save_supply_cluster_plan(
    sku: String,
    macrolocal_cluster_id: String,
    planned_qty: i64,
    target_days: i64,
    state: State<AppState>,
) -> Result<(), String> {
    if sku.trim().is_empty() || macrolocal_cluster_id.trim().is_empty() {
        return Err("SKU 和集群 ID 不能为空".into());
    }
    if planned_qty < 0 || !(1..=365).contains(&target_days) {
        return Err("计划数量不能小于 0，目标天数必须在 1–365 之间".into());
    }
    db(&state)?.execute("INSERT INTO replenishment_plan(sku,macrolocal_cluster_id,planned_qty,target_days,updated_at) VALUES(?1,?2,?3,?4,CURRENT_TIMESTAMP) ON CONFLICT(sku,macrolocal_cluster_id) DO UPDATE SET planned_qty=excluded.planned_qty,target_days=excluded.target_days,updated_at=CURRENT_TIMESTAMP", params![sku.trim(),macrolocal_cluster_id.trim(),planned_qty,target_days]).map_err(|e| e.to_string())?;
    Ok(())
}

fn supply_orders_blocking(state: &AppState) -> Result<Vec<SupplyOrderRow>, String> {
    let c = db(state)?;
    let mut order_ids = Vec::new();
    let mut last_id = String::new();
    for _ in 0..20 {
        let payload = seller_post(
            &c,
            "/v3/supply-order/list",
            &serde_json::json!({
                "filter":{"states":[1,2,3,4,5,6,7]},"last_id":last_id,"limit":100,"sort_by":1,"sort_order":1
            }),
        )?;
        let page: Vec<i64> = payload
            .get("order_ids")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_i64())
            .collect();
        let next = json_text(payload.get("last_id"));
        order_ids.extend(page.iter().copied());
        if page.is_empty() || next.is_empty() || next == last_id {
            break;
        }
        last_id = next;
    }
    let mut raw_orders = Vec::new();
    for batch in order_ids.chunks(50) {
        let payload = seller_post(
            &c,
            "/v3/supply-order/get",
            &serde_json::json!({"order_ids":batch}),
        )?;
        raw_orders.extend(
            payload
                .get("orders")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default(),
        );
    }
    let mut rows = Vec::new();
    for order in raw_orders {
        let supplies = order
            .get("supplies")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut clusters = std::collections::BTreeSet::new();
        let mut warehouses = std::collections::BTreeSet::new();
        let mut states = std::collections::BTreeSet::new();
        let mut crossdock = false;
        for supply in &supplies {
            if let Some(v) = supply.get("macrolocal_cluster_id").and_then(|v| v.as_i64()) {
                clusters.insert(v.to_string());
            }
            let warehouse = json_text(supply.pointer("/storage_warehouse/name"));
            if !warehouse.is_empty() {
                warehouses.insert(warehouse);
            }
            let state = json_text(supply.get("state"));
            if !state.is_empty() {
                states.insert(state);
            }
            crossdock |= supply
                .get("is_crossdock")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
        }
        let slot = order.pointer("/timeslot/timeslot");
        rows.push(SupplyOrderRow {
            order_id: order.get("order_id").and_then(|v| v.as_i64()).unwrap_or(0),
            order_number: json_text(order.get("order_number")),
            state: json_text(order.get("state")),
            created_date: json_text(order.get("created_date")),
            data_filling_deadline: json_text(order.get("data_filling_deadline")),
            dropoff_name: json_text(order.pointer("/drop_off_warehouse/name")),
            dropoff_address: json_text(order.pointer("/drop_off_warehouse/address")),
            timeslot_from: json_text(slot.and_then(|v| v.get("from"))),
            timeslot_to: json_text(slot.and_then(|v| v.get("to"))),
            timezone_name: json_text(order.pointer("/timeslot/timezone_info/iana_name")),
            supply_type: if crossdock {
                "越库".into()
            } else {
                "直送".into()
            },
            clusters: clusters.into_iter().collect::<Vec<_>>().join("、"),
            storage_warehouses: warehouses.into_iter().collect::<Vec<_>>().join("、"),
            supply_states: states.into_iter().collect::<Vec<_>>().join("、"),
            supplies_count: supplies.len(),
        });
    }
    rows.sort_by(|a, b| b.created_date.cmp(&a.created_date));
    Ok(rows)
}

#[tauri::command]
async fn supply_orders(state: State<'_, AppState>) -> Result<Vec<SupplyOrderRow>, String> {
    let owned = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || supply_orders_blocking(&owned))
        .await
        .map_err(|e| format!("读取供应单后台任务失败：{e}"))?
}

fn supply_timeslots_blocking(
    order_id: i64,
    date_from: String,
    date_to: String,
    state: &AppState,
) -> Result<Vec<SupplyTimeslot>, String> {
    if date_from > date_to {
        return Err("时间窗开始日期不能晚于结束日期".into());
    }
    let c = db(state)?;
    let payload = seller_post(
        &c,
        "/v2/supply-order/timeslot/list",
        &serde_json::json!({"order_id":order_id,"date_from":date_from,"date_to":date_to}),
    )?;
    Ok(payload
        .pointer("/timeslots_info/timeslots")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|v| {
            let from = json_text(v.get("from"));
            let to = json_text(v.get("to"));
            if from.is_empty() || to.is_empty() {
                None
            } else {
                Some(SupplyTimeslot { from, to })
            }
        })
        .collect())
}

#[tauri::command]
async fn supply_timeslots(
    order_id: i64,
    date_from: String,
    date_to: String,
    state: State<'_, AppState>,
) -> Result<Vec<SupplyTimeslot>, String> {
    let owned = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || {
        supply_timeslots_blocking(order_id, date_from, date_to, &owned)
    })
    .await
    .map_err(|e| format!("查询预约时段后台任务失败：{e}"))?
}

fn book_supply_timeslot_blocking(
    supply_order_id: i64,
    timeslot_from: String,
    timeslot_to: String,
    confirmation: String,
    state: &AppState,
) -> Result<String, String> {
    if confirmation != "确认预约" {
        return Err("预约未确认；必须输入“确认预约”".into());
    }
    let c = db(state)?;
    let payload = seller_post(
        &c,
        "/v1/supply-order/timeslot/update",
        &serde_json::json!({"supply_order_id":supply_order_id,"timeslot":{"from":timeslot_from,"to":timeslot_to}}),
    )?;
    let operation = json_text(payload.get("operation_id")).trim().to_string();
    if operation.is_empty() {
        Ok("Ozon 已接受预约请求。请刷新供应单确认最终时段。".into())
    } else {
        Ok(format!("Ozon 已接受预约请求，操作 ID：{operation}"))
    }
}

#[tauri::command]
async fn book_supply_timeslot(
    supply_order_id: i64,
    timeslot_from: String,
    timeslot_to: String,
    confirmation: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let owned = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || {
        book_supply_timeslot_blocking(
            supply_order_id,
            timeslot_from,
            timeslot_to,
            confirmation,
            &owned,
        )
    })
    .await
    .map_err(|e| format!("提交预约后台任务失败：{e}"))?
}

fn chat_endpoint(base: &str) -> String {
    let b = base.trim().trim_end_matches('/');
    if b.ends_with("/chat/completions") {
        b.into()
    } else if b.ends_with("/v1") {
        format!("{b}/chat/completions")
    } else {
        format!("{b}/v1/chat/completions")
    }
}
#[tauri::command]
fn ai_analysis(
    range: DateRange,
    question: String,
    state: State<AppState>,
) -> Result<String, String> {
    let c = db(&state)?;
    let base = setting(&c, "ai_base_url");
    let model = setting(&c, "ai_model");
    let key = secret_setting(&c, "ai_api_key")?;
    if base.is_empty() || model.is_empty() || key.is_empty() {
        return Err("请先在连接设置中配置 AI Base URL、模型和 API Key".into());
    }
    let (revenue,orders):(f64,i64)=c.query_row("SELECT COALESCE(SUM(revenue),0),COALESCE(SUM(ordered_units),0) FROM sales_daily WHERE day BETWEEN ?1 AND ?2",params![range.from,range.to],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|e|e.to_string())?;
    let (ad_spend,ad_revenue):(f64,f64)=c.query_row("SELECT COALESCE(SUM(spend),0),COALESCE(SUM(revenue),0) FROM ad_daily WHERE day BETWEEN ?1 AND ?2 AND sku=''",params![range.from,range.to],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|e|e.to_string())?;
    let mut stmt=c.prepare("SELECT s.sku,COALESCE(MAX(p.offer_id),''),COALESCE(MAX(p.name),''),SUM(s.revenue),SUM(s.ordered_units) FROM sales_daily s LEFT JOIN products p ON p.sku=s.sku WHERE s.day BETWEEN ?1 AND ?2 GROUP BY s.sku ORDER BY SUM(s.revenue) DESC LIMIT 20").map_err(|e|e.to_string())?;
    let top=stmt.query_map(params![range.from,range.to],|r|Ok(serde_json::json!({"sku":r.get::<_,String>(0)?,"offer_id":r.get::<_,String>(1)?,"name":r.get::<_,String>(2)?,"revenue":r.get::<_,f64>(3)?,"units":r.get::<_,i64>(4)?}))).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
    let context = serde_json::json!({"date_from":range.from,"date_to":range.to,"summary":{"revenue":revenue,"orders":orders,"ad_spend":ad_spend,"ad_revenue":ad_revenue},"top_products":top});
    let prompt = format!(
        "分析期间 {} 至 {}。用户问题：{}\n本地经营数据：{}",
        range.from,
        range.to,
        if question.trim().is_empty() {
            "请给出经营诊断、风险和下一步行动"
        } else {
            question.trim()
        },
        context
    );
    let body = serde_json::json!({"model":model,"messages":[{"role":"system","content":"你是 Ozon 店铺经营分析助手。只能根据提供的本地汇总分析，明确区分事实、推断和缺失数据；不要声称访问了 Ozon 后台；输出中文和可执行建议。"},{"role":"user","content":prompt}]});
    let response = ureq::post(&chat_endpoint(&base))
        .set("Authorization", &format!("Bearer {key}"))
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| format!("AI 请求失败：{e}"))?;
    let raw = response.into_string().map_err(|e| e.to_string())?;
    let payload: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("AI 返回不是有效 JSON：{e}"))?;
    payload
        .pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| {
            format!(
                "AI 返回缺少正文：{}",
                raw.chars().take(300).collect::<String>()
            )
        })
}

#[tauri::command]
fn connection_status(state: State<AppState>) -> Result<ConnectionStatus, String> {
    let c = db(&state)?;
    let seller_id = setting(&c, "seller_client_id");
    let seller_key = setting(&c, "seller_api_key");
    let performance_id = setting(&c, "performance_client_id");
    let performance_secret = setting(&c, "performance_client_secret");
    let ai_base_url = setting(&c, "ai_base_url");
    let ai_model = setting(&c, "ai_model");
    let ai_key = setting(&c, "ai_api_key");
    let feishu_id = setting(&c, "feishu_app_id");
    let feishu_secret = setting(&c, "feishu_app_secret");
    let last = c
        .query_row(
            "SELECT finished_at FROM sync_logs WHERE status='success' ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .ok();
    Ok(ConnectionStatus {
        seller_client_id: seller_id.clone(),
        seller_api_configured: !seller_id.is_empty() && !seller_key.is_empty(),
        performance_client_id: performance_id.clone(),
        performance_api_configured: !performance_id.is_empty() && !performance_secret.is_empty(),
        ai_base_url,
        ai_model,
        ai_configured: !ai_key.is_empty(),
        feishu_configured: !feishu_id.is_empty() && !feishu_secret.is_empty(),
        last_successful_sync: last,
    })
}

#[tauri::command]
fn load_credentials_form(state: State<AppState>) -> Result<CredentialsForm, String> {
    let c = db(&state)?;
    Ok(CredentialsForm {
        seller_client_id: setting(&c, "seller_client_id"),
        seller_api_key: String::new(),
        performance_client_id: setting(&c, "performance_client_id"),
        performance_client_secret: String::new(),
        ai_base_url: {
            let v = setting(&c, "ai_base_url");
            if v.is_empty() {
                "https://api.gpt.ge/v1".into()
            } else {
                v
            }
        },
        ai_api_key: String::new(),
        ai_model: {
            let v = setting(&c, "ai_model");
            if v.is_empty() {
                "gpt-5.6-terra".into()
            } else {
                v
            }
        },
        feishu_base_url: {
            let v = setting(&c, "feishu_base_url");
            if v.is_empty() {
                "https://open.feishu.cn/open-apis".into()
            } else {
                v
            }
        },
        feishu_app_id: setting(&c, "feishu_app_id"),
        feishu_app_secret: String::new(),
        feishu_app_token: setting(&c, "feishu_app_token"),
        feishu_product_table_id: setting(&c, "feishu_product_table_id"),
        feishu_weekly_table_id: setting(&c, "feishu_weekly_table_id"),
        feishu_tracking_table_id: setting(&c, "feishu_tracking_table_id"),
        feishu_series_table_id: setting(&c, "feishu_series_table_id"),
        feishu_chat_id: setting(&c, "feishu_chat_id"),
        local_tax_rate: {
            let value = setting(&c, "local_tax_rate");
            if value.is_empty() {
                "3".into()
            } else {
                value
            }
        },
        local_payout_fee_rate: {
            let value = setting(&c, "local_payout_fee_rate");
            if value.is_empty() {
                "10".into()
            } else {
                value
            }
        },
        local_rub_per_cny: {
            let value = setting(&c, "local_rub_per_cny");
            if value.is_empty() {
                setting(&c, "rub_per_cny")
                    .parse::<f64>()
                    .ok()
                    .filter(|v| *v > 0.0)
                    .unwrap_or(11.5)
                    .to_string()
            } else {
                value
            }
        },
        cross_border_rub_per_cny: {
            let value = setting(&c, "cross_border_rub_per_cny");
            if value.is_empty() {
                setting(&c, "rub_per_cny")
                    .parse::<f64>()
                    .ok()
                    .filter(|v| *v > 0.0)
                    .unwrap_or(14.0)
                    .to_string()
            } else {
                value
            }
        },
    })
}
#[tauri::command]
fn save_credentials_form(form: CredentialsForm, state: State<AppState>) -> Result<(), String> {
    for (label, value) in [
        ("税率", &form.local_tax_rate),
        ("回款手续费", &form.local_payout_fee_rate),
    ] {
        let parsed = value
            .trim()
            .parse::<f64>()
            .map_err(|_| format!("{label}必须是有效数字"))?;
        if !(0.0..=100.0).contains(&parsed) {
            return Err(format!("{label}必须在 0% 至 100% 之间"));
        }
    }
    for (label, value) in [
        ("本土店人民币兑卢布汇率", &form.local_rub_per_cny),
        ("跨境店人民币兑卢布汇率", &form.cross_border_rub_per_cny),
    ] {
        let parsed = value
            .trim()
            .parse::<f64>()
            .map_err(|_| format!("{label}必须是有效数字"))?;
        if parsed <= 0.0 {
            return Err(format!("{label}必须大于 0"));
        }
    }
    let mut c = db(&state)?;
    let tx = c.transaction().map_err(|e| e.to_string())?;
    for (key, value) in [
        ("seller_client_id", form.seller_client_id),
        ("performance_client_id", form.performance_client_id),
        ("ai_base_url", form.ai_base_url),
        ("ai_model", form.ai_model),
        ("feishu_base_url", form.feishu_base_url),
        ("feishu_app_id", form.feishu_app_id),
        ("feishu_app_token", form.feishu_app_token),
        ("feishu_product_table_id", form.feishu_product_table_id),
        ("feishu_weekly_table_id", form.feishu_weekly_table_id),
        ("feishu_tracking_table_id", form.feishu_tracking_table_id),
        ("feishu_series_table_id", form.feishu_series_table_id),
        ("feishu_chat_id", form.feishu_chat_id),
        ("local_tax_rate", form.local_tax_rate.trim().to_string()),
        (
            "local_payout_fee_rate",
            form.local_payout_fee_rate.trim().to_string(),
        ),
        (
            "local_rub_per_cny",
            form.local_rub_per_cny.trim().to_string(),
        ),
        (
            "cross_border_rub_per_cny",
            form.cross_border_rub_per_cny.trim().to_string(),
        ),
    ] {
        save_setting(&tx, key, &value)?
    }
    for (key, value) in [
        ("seller_api_key", form.seller_api_key),
        ("performance_client_secret", form.performance_client_secret),
        ("ai_api_key", form.ai_api_key),
        ("feishu_app_secret", form.feishu_app_secret),
    ] {
        if !value.is_empty() {
            save_setting(&tx, key, &secrets::protect(&value)?)?
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    c.execute("DELETE FROM business_report_cache", [])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn export_api_bundle(state: State<AppState>) -> Result<String, String> {
    let c = db(&state)?;
    let credentials = serde_json::json!({"seller_client_id":setting(&c,"seller_client_id"),"seller_api_key":secret_setting(&c,"seller_api_key")?,"performance_client_id":setting(&c,"performance_client_id"),"performance_client_secret":secret_setting(&c,"performance_client_secret")?,"ai_base_url":setting(&c,"ai_base_url"),"ai_api_key":secret_setting(&c,"ai_api_key")?,"ai_model":setting(&c,"ai_model"),"feishu_base_url":setting(&c,"feishu_base_url"),"feishu_app_id":setting(&c,"feishu_app_id"),"feishu_app_secret":secret_setting(&c,"feishu_app_secret")?,"feishu_app_token":setting(&c,"feishu_app_token"),"feishu_product_table_id":setting(&c,"feishu_product_table_id"),"feishu_weekly_table_id":setting(&c,"feishu_weekly_table_id"),"feishu_tracking_table_id":setting(&c,"feishu_tracking_table_id"),"feishu_series_table_id":setting(&c,"feishu_series_table_id"),"feishu_chat_id":setting(&c,"feishu_chat_id")});
    let shop_id = state
        .active_shop_id
        .lock()
        .map_err(|_| "店铺状态锁异常")?
        .clone();
    let registry = read_registry(&state.data_dir)?;
    let shop = registry
        .shops
        .iter()
        .find(|x| x.id == shop_id)
        .ok_or("当前店铺不存在")?;
    let bundle = serde_json::json!({"type":"ozon-seller-analytics-api-bundle","version":1,"exported_at":chrono::Utc::now().to_rfc3339(),"warning":"此文件包含明文 API 密钥，请妥善保管并在导入后删除。","profiles":[{"api_name":shop.api_name,"shop_name":shop.name,"shop_kind":shop.kind,"credentials":credentials}]});
    let folder = state
        .data_dir
        .parent()
        .unwrap_or(&state.data_dir)
        .join("exports");
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let path = folder.join(format!(
        "ozon-api-configs-{}.ozon-api.json",
        chrono::Local::now().format("%Y-%m-%d-%H%M%S")
    ));
    fs::write(
        &path,
        serde_json::to_vec_pretty(&bundle).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
fn import_api_bundle(path: String, state: State<AppState>) -> Result<i64, String> {
    let raw: serde_json::Value = serde_json::from_slice(
        &fs::read(path.trim()).map_err(|e| format!("无法读取 API 配置包：{e}"))?,
    )
    .map_err(|e| format!("API 配置包 JSON 无效：{e}"))?;
    if raw.get("type").and_then(|v| v.as_str()) != Some("ozon-seller-analytics-api-bundle")
        || raw.get("version").and_then(|v| v.as_i64()) != Some(1)
    {
        return Err("文件不是受支持的 Python/React 通用 API 配置包".into());
    }
    let profiles = raw
        .get("profiles")
        .and_then(|v| v.as_array())
        .ok_or("配置包没有 profiles")?;
    let profile = profiles.first().ok_or("配置包中没有店铺 API")?;
    let values = profile
        .get("credentials")
        .and_then(|v| v.as_object())
        .ok_or("配置包缺少 credentials")?;
    let mut c = db(&state)?;
    let tx = c.transaction().map_err(|e| e.to_string())?;
    for key in [
        "seller_client_id",
        "performance_client_id",
        "ai_base_url",
        "ai_model",
        "feishu_base_url",
        "feishu_app_id",
        "feishu_app_token",
        "feishu_product_table_id",
        "feishu_weekly_table_id",
        "feishu_tracking_table_id",
        "feishu_series_table_id",
        "feishu_chat_id",
    ] {
        if let Some(value) = values.get(key).and_then(|v| v.as_str()) {
            save_setting(&tx, key, value)?
        }
    }
    for key in [
        "seller_api_key",
        "performance_client_secret",
        "ai_api_key",
        "feishu_app_secret",
    ] {
        if let Some(value) = values.get(key).and_then(|v| v.as_str()) {
            if !value.is_empty() {
                save_setting(&tx, key, &secrets::protect(value)?)?
            }
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(1)
}

#[cfg(test)]
mod cross_border_tests {
    use super::cross_border_shipping;

    #[test]
    fn freight_formula_matches_legacy_boundaries() {
        assert_eq!(cross_border_shipping(100.0, 0.4), Some(3.37 + 0.4 * 28.17));
        assert_eq!(cross_border_shipping(200.0, 1.0), Some(17.97 + 28.17));
        assert_eq!(cross_border_shipping(700.0, 3.0), Some(24.17 + 3.0 * 28.17));
        assert_eq!(cross_border_shipping(100.0, 1.0), Some(25.83 + 19.17));
        assert_eq!(cross_border_shipping(100.0, 3.0), Some(25.83 + 3.0 * 19.17));
        assert_eq!(cross_border_shipping(30000.0, 1.0), None);
    }
}

#[cfg(test)]
mod finance_category_tests {
    use super::finance_service_category;

    #[test]
    fn delivery_card_and_breakdown_share_one_category() {
        for service in [
            "MarketplaceServiceItemDirectFlowLogistic",
            "MarketplaceServiceItemRedistributionLastMileCourier",
            "MarketplaceServiceItemDeliveryToHandoverPlaceOzon",
        ] {
            assert_eq!(finance_service_category(service).0, "delivery");
        }
    }

    #[test]
    fn audited_finance_categories_remain_distinct() {
        assert_eq!(
            finance_service_category("MarketplaceServiceItemReturnFlowLogistic").0,
            "return_logistics"
        );
        assert_eq!(
            finance_service_category("MarketplaceServiceItemDirectFlowLogistic").0,
            "delivery"
        );
        assert_eq!(
            finance_service_category("MarketplaceServiceItemAcquiring").0,
            "acquiring"
        );
        assert_eq!(
            finance_service_category("MarketplaceServiceStorageServiceAtTheWarehouseFbo").0,
            "storage"
        );
        assert_eq!(
            finance_service_category("OperationMarketplaceServiceCostPerClick").0,
            "advertising"
        );
    }
}

#[cfg(test)]
mod shop_cost_isolation_tests {
    use super::{
        active_shop_database_path, feishu_product_record_belongs_to_shop, upsert_cost, AppState,
        ProductCostInput,
    };
    use std::{
        fs,
        sync::Mutex,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn cost_input(value: f64) -> ProductCostInput {
        ProductCostInput {
            sku: "SAME-SKU".into(),
            unit_cost: Some(value),
            first_mile_cost: Some(value / 10.0),
            length_cm: None,
            width_cm: None,
            height_cm: None,
            weight_kg: None,
            note: String::new(),
        }
    }

    #[test]
    fn same_sku_keeps_independent_costs_in_each_shop_database() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ozon-shop-cost-isolation-{unique}"));
        fs::create_dir_all(root.join("shops")).unwrap();
        fs::write(
            root.join("shops.json"),
            r#"{"active_shop_id":"shop-a","shops":[{"id":"shop-a","name":"A","kind":"local","database_file":"shops/a.db","api_name":"A"},{"id":"shop-b","name":"B","kind":"local","database_file":"shops/b.db","api_name":"B"}]}"#,
        )
        .unwrap();
        for file in ["a.db", "b.db"] {
            let c = rusqlite::Connection::open(root.join("shops").join(file)).unwrap();
            c.execute_batch("CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT NOT NULL DEFAULT '');CREATE TABLE product_costs(sku TEXT PRIMARY KEY,unit_cost REAL,first_mile_cost REAL,unit_cost_cny REAL,first_mile_cost_cny REAL,length_cm REAL,width_cm REAL,height_cm REAL,weight_kg REAL,note TEXT NOT NULL DEFAULT '',updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);").unwrap();
        }
        let state = AppState {
            data_dir: root.clone(),
            active_shop_id: Mutex::new("shop-a".into()),
        };
        let shop_a_path = active_shop_database_path(&state).unwrap();
        upsert_cost(
            &rusqlite::Connection::open(&shop_a_path).unwrap(),
            &cost_input(11.0),
        )
        .unwrap();
        *state.active_shop_id.lock().unwrap() = "shop-b".into();
        let shop_b_path = active_shop_database_path(&state).unwrap();
        assert_ne!(shop_a_path, shop_b_path);
        let shop_b = rusqlite::Connection::open(&shop_b_path).unwrap();
        assert_eq!(
            shop_b
                .query_row(
                    "SELECT COUNT(*) FROM product_costs WHERE sku='SAME-SKU'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        upsert_cost(&shop_b, &cost_input(29.0)).unwrap();
        *state.active_shop_id.lock().unwrap() = "shop-a".into();
        assert_eq!(
            rusqlite::Connection::open(active_shop_database_path(&state).unwrap())
                .unwrap()
                .query_row(
                    "SELECT unit_cost_cny FROM product_costs WHERE sku='SAME-SKU'",
                    [],
                    |r| r.get::<_, f64>(0)
                )
                .unwrap(),
            11.0
        );
        *state.active_shop_id.lock().unwrap() = "shop-b".into();
        assert_eq!(
            rusqlite::Connection::open(active_shop_database_path(&state).unwrap())
                .unwrap()
                .query_row(
                    "SELECT unit_cost_cny FROM product_costs WHERE sku='SAME-SKU'",
                    [],
                    |r| r.get::<_, f64>(0)
                )
                .unwrap(),
            29.0
        );
        drop(shop_b);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn feishu_cost_records_require_exact_shop_id() {
        let a = serde_json::json!({"fields":{"店铺ID":"shop-a","SKU":"SAME-SKU"}});
        let legacy = serde_json::json!({"fields":{"SKU":"SAME-SKU"}});
        assert!(feishu_product_record_belongs_to_shop(&a, "shop-a"));
        assert!(!feishu_product_record_belongs_to_shop(&a, "shop-b"));
        assert!(!feishu_product_record_belongs_to_shop(&legacy, "shop-a"));
    }
}

#[cfg(test)]
mod feishu_supply_chain_tests {
    use super::*;

    #[test]
    fn cargo_status_formula_maps_without_double_counting_delivered_stock() {
        assert_eq!(supply_quantities_for_status("未送仓", 12), (12, 0, 0, 0));
        assert_eq!(supply_quantities_for_status("在途", 12), (0, 0, 12, 0));
        assert_eq!(
            supply_quantities_for_status("到达海外仓", 12),
            (0, 0, 0, 12)
        );
        assert_eq!(supply_quantities_for_status("已送仓", 12), (0, 0, 0, 0));
        assert_eq!(supply_quantities_for_status("未知", 12), (0, 0, 0, 0));
    }

    #[test]
    fn product_name_mapping_rejects_ambiguous_skus() {
        let mut candidates = std::collections::HashMap::new();
        add_unique_sku_candidate(&mut candidates, "测试 商品", "SKU-1");
        assert_eq!(
            candidates.get("测试商品").and_then(|v| v.as_deref()),
            Some("SKU-1")
        );
        add_unique_sku_candidate(&mut candidates, "测试商品", "SKU-2");
        assert_eq!(candidates.get("测试商品"), Some(&None));
    }
}

#[cfg(test)]
mod monthly_profit_settled_units_tests {
    use rusqlite::{params, Connection};

    #[test]
    fn finance_delivery_wins_over_partial_posting_history_per_sku() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE sales_daily(sku TEXT, day TEXT);
             CREATE TABLE delivery_events(sku TEXT, day TEXT, quantity INTEGER);
             CREATE TABLE finance_transactions(sku TEXT, operation_date TEXT, operation_id TEXT, operation_type TEXT);",
        )
        .unwrap();
        c.execute("INSERT INTO sales_daily VALUES('A','2026-07-01')", [])
            .unwrap();
        c.execute("INSERT INTO sales_daily VALUES('B','2026-07-01')", [])
            .unwrap();
        c.execute("INSERT INTO delivery_events VALUES('A','2026-07-01',2)", [])
            .unwrap();
        c.execute("INSERT INTO delivery_events VALUES('B','2026-07-01',1)", [])
            .unwrap();
        for id in 1..=5 {
            c.execute(
                "INSERT INTO finance_transactions VALUES('A','2026-07-01',?1,'OperationAgentDeliveredToCustomer')",
                params![format!("op-{id}")],
            )
            .unwrap();
        }
        let units: i64 = c
            .query_row(
                "WITH sales AS(SELECT sku FROM sales_daily WHERE day BETWEEN ?1 AND ?2 GROUP BY sku),delivered AS(SELECT sku,SUM(quantity) units FROM delivery_events WHERE day BETWEEN ?1 AND ?2 GROUP BY sku),finance_delivered AS(SELECT sku,COUNT(DISTINCT operation_id) units FROM finance_transactions WHERE substr(operation_date,1,10) BETWEEN ?1 AND ?2 AND sku<>'' AND lower(operation_type) LIKE '%deliveredtocustomer%' GROUP BY sku),base AS(SELECT s.sku,COALESCE(fd.units,d.units,0) units FROM sales s LEFT JOIN delivered d ON d.sku=s.sku LEFT JOIN finance_delivered fd ON fd.sku=s.sku) SELECT SUM(units) FROM base",
                params!["2026-07-01", "2026-07-31"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            units, 6,
            "five Finance deliveries plus one posting fallback"
        );
    }
}

#[cfg(test)]
mod cross_border_display_tests {
    use super::display_amount;

    #[test]
    fn rub_amounts_are_converted_before_cny_display() {
        assert!((display_amount(6_075.57, true, 14.0) - 433.969_285_714).abs() < 1e-9);
        assert_eq!(display_amount(6_075.57, false, 14.0), 6_075.57);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let exe = std::env::current_exe()?;
            let local_app_dir = app.path().app_local_data_dir()?;
            let resource_dir = app.path().resource_dir()?;
            let data_dir = locate_data_dir(&exe, &local_app_dir, &resource_dir)
                .map_err(std::io::Error::other)?;
            let registry = read_registry(&data_dir).map_err(std::io::Error::other)?;
            app.manage(AppState {
                data_dir,
                active_shop_id: Mutex::new(registry.active_shop_id),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_shops,
            select_shop,
            create_shop,
            update_shop,
            delete_shop,
            dashboard,
            orders,
            advertising,
            campaign_monitor,
            campaign_control,
            campaign_ai_analysis,
            products,
            inventory,
            sync_inventory,
            connection_status,
            load_credentials_form,
            save_credentials_form,
            export_api_bundle,
            import_api_bundle,
            ai_analysis,
            save_product_cost,
            match_product_costs,
            export_product_costs,
            export_dataset,
            import_product_costs_csv,
            warehouse_mappings,
            save_warehouse_mapping,
            save_fbs_threshold,
            sync_fbs_orders,
            fbs_orders,
            competitors,
            competitor_alert_settings,
            save_competitor_alert_settings,
            seed_competitor_demo_data,
            delete_competitor_demo_data,
            add_competitor,
            refresh_competitor,
            refresh_competitors_due,
            refresh_competitors_all,
            start_competitors_collection,
            start_competitor_collection_task,
            rerun_competitors_collection,
            retry_failed_competitors_collection,
            competitor_collection_progress,
            stop_competitors_collection,
            stop_competitor_collection_task,
            competitor_latest_run,
            set_competitor_manual_sales,
            set_competitor_manual_metrics,
            open_competitor_browser,
            import_competitor_html,
            remove_competitor,
            business_report,
            analytics_detail,
            cross_border_report,
            finance_breakdown,
            data_coverage,
            prune_cache,
            missing_cost_rows,
            supply_orders,
            supply_cluster_plans,
            save_supply_cluster_plan,
            supply_timeslots,
            book_supply_timeslot,
            sync_logs,
            sync_seller_sales,
            sync_performance_ads,
            sync_finance,
            sync_all_data,
            test_feishu,
            sync_feishu_products,
            send_feishu_weekly,
            shipment_tracking,
            shipment_sku_options,
            save_shipment_sku_allocations,
            shipment_settlement,
            settle_shipment,
            sync_feishu_shipments,
            notify_feishu_shipment,
            wb::wb_settings,
            wb::save_wb_settings,
            wb::export_wb_api_bundle,
            wb::import_wb_api_bundle,
            wb::wb_costs,
            wb::save_wb_cost,
            wb::wb_daily,
            wb::wb_orders,
            wb::wb_ads,
            wb::wb_warehouses,
            wb::wb_stocks,
            wb::sync_wb,
            wb::test_wb_feishu,
            wb::send_wb_weekly,
            listing::listing_settings,
            listing::save_listing_settings,
            listing::listing_rows,
            listing::sync_listing_costs,
            listing::open_listing_supplier_url,
            listing::calculate_listing_price,
            listing::create_listing_draft,
            listing::listing_jobs,
            listing::save_listing_draft,
            listing::retry_listing_job,
            listing::collect_listing_reference,
            listing::open_listing_browser,
            listing::import_listing_html,
            listing::refresh_listing_categories,
            listing::search_listing_categories,
            listing::listing_attribute_definitions,
            listing::listing_dictionary_values,
            listing::map_listing_reference_attributes,
            listing::set_listing_attribute_value,
            listing::clear_listing_attribute_value,
            listing::ai_fill_listing_required_attributes,
            listing::validate_listing_job,
            listing::launch_listing_tool,
            insights::product_insights,
            insights::product_analysis,
            insights::series_insights,
            insights::save_product_series,
            insights::delete_product_series,
            insights::product_detail,
            insights::refresh_product_price,
            insights::update_product_price,
            insights::save_product_cluster_weights
        ])
        .run(tauri::generate_context!())
        .expect("failed to run app")
}
