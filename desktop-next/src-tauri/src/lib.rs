use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};
use tauri::{Manager, State};
mod insights;
mod listing;
mod secrets;
mod wb;
static API_SYNC_LOCK: Mutex<()> = Mutex::new(());

pub(crate) struct AppState {
    pub(crate) data_dir: PathBuf,
    active_shop_id: Mutex<String>,
}

#[derive(Serialize, Deserialize)]
struct ShopFile {
    active_shop_id: String,
    shops: Vec<RawShop>,
}
#[derive(Serialize, Deserialize)]
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
    campaigns: Vec<CampaignRow>,
    trend: Vec<AdvertisingTrendRow>,
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
    warehouse_count: i64,
    daily_sales: f64,
    estimated_days: Option<f64>,
    suggested_qty: i64,
    planned_qty: i64,
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
    product_url: String,
    product_code: String,
    name: String,
    image_url: String,
    latest_price: Option<f64>,
    daily_sales: Option<i64>,
    weekly_sales: Option<i64>,
    monthly_sales: Option<i64>,
    snapshots: Vec<CompetitorSnapshot>,
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
}

fn read_registry(data_dir: &Path) -> Result<ShopFile, String> {
    let text = fs::read_to_string(data_dir.join("shops.json")).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}
fn locate_data_dir(executable: &Path) -> Result<PathBuf, String> {
    let executable_dir = executable.parent().ok_or("无法确定程序目录")?;
    for root in executable_dir.ancestors() {
        let candidate = root.join("data");
        if candidate.join("shops.json").is_file() {
            return Ok(candidate);
        }
    }
    Err("找不到 data/shops.json；请将程序放回 Ozon Analytics 项目目录，或把 data 文件夹放在程序旁边".into())
}
fn initialize_extensions(c: &Connection) -> Result<(), String> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS warehouse_cluster_mappings(warehouse_name TEXT PRIMARY KEY,cluster_name TEXT NOT NULL DEFAULT '',updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);CREATE TABLE IF NOT EXISTS competitor_products(id INTEGER PRIMARY KEY AUTOINCREMENT,product_url TEXT NOT NULL UNIQUE,product_code TEXT NOT NULL DEFAULT '',name TEXT NOT NULL DEFAULT '',image_url TEXT NOT NULL DEFAULT '',active INTEGER NOT NULL DEFAULT 1,created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);CREATE TABLE IF NOT EXISTS competitor_snapshots(id INTEGER PRIMARY KEY AUTOINCREMENT,competitor_id INTEGER NOT NULL,captured_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,price REAL,sales_total INTEGER,source TEXT NOT NULL DEFAULT 'web',UNIQUE(competitor_id,captured_at),FOREIGN KEY(competitor_id) REFERENCES competitor_products(id));CREATE TABLE IF NOT EXISTS shipment_tracking(tracking_id TEXT PRIMARY KEY,product_name TEXT NOT NULL DEFAULT '',batch_no TEXT NOT NULL DEFAULT '',shop_name TEXT NOT NULL DEFAULT '',quantity INTEGER NOT NULL DEFAULT 0,cargo_status TEXT NOT NULL DEFAULT '',channel TEXT NOT NULL DEFAULT '',domestic_arrival TEXT NOT NULL DEFAULT '',foreign_arrival TEXT NOT NULL DEFAULT '',notified_foreign_arrival TEXT NOT NULL DEFAULT '',source TEXT NOT NULL DEFAULT 'local',remote_record_id TEXT NOT NULL DEFAULT '',updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);CREATE INDEX IF NOT EXISTS idx_posting_routes_day ON posting_routes(day);CREATE INDEX IF NOT EXISTS idx_posting_routes_sku_destination ON posting_routes(sku,destination);CREATE INDEX IF NOT EXISTS idx_sales_daily_day_sku ON sales_daily(day,sku);CREATE INDEX IF NOT EXISTS idx_ad_daily_day_sku ON ad_daily(day,sku);CREATE INDEX IF NOT EXISTS idx_finance_operation_date ON finance_transactions(operation_date);").map_err(|e|e.to_string())?;
    c.execute_batch("CREATE TABLE IF NOT EXISTS product_cluster_weights(sku TEXT NOT NULL,cluster_name TEXT NOT NULL,weight REAL NOT NULL DEFAULT 0,updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,PRIMARY KEY(sku,cluster_name));").map_err(|e|e.to_string())?;
    c.execute_batch("CREATE TABLE IF NOT EXISTS inventory_totals(sku TEXT PRIMARY KEY,offer_id TEXT NOT NULL DEFAULT '',present_stock INTEGER NOT NULL DEFAULT 0,reserved_stock INTEGER NOT NULL DEFAULT 0,updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);").map_err(|e|e.to_string())?;
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
    Ok(())
}
pub(crate) fn db(state: &AppState) -> Result<Connection, String> {
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
    let c =
        Connection::open(state.data_dir.join(&shop.database_file)).map_err(|e| e.to_string())?;
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

fn seller_post(
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
fn performance_post(path: &str, token: &str, body: &serde_json::Value) -> Result<serde_json::Value, String> {
    let url = format!("https://api-performance.ozon.ru{path}");
    let response = ureq::post(&url).set("Authorization", &format!("Bearer {token}"))
        .set("Accept", "application/json").set("Content-Type", "application/json")
        .send_string(&body.to_string()).map_err(|e| format!("Performance API 请求失败（{path}）：{e}"))?;
    let raw = response.into_string().map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| format!("Performance API 返回无法解析：{e}"))
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
    shop.name = name.trim().into();
    shop.kind = kind;
    shop.api_name = api_name.trim().into();
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
    let (revenue,orders,return_units,cancellation_units,views):(f64,i64,i64,i64,i64)=c.query_row("SELECT COALESCE(SUM(revenue),0),COALESCE(SUM(ordered_units),0),CASE WHEN EXISTS(SELECT 1 FROM return_events WHERE day BETWEEN ?1 AND ?2) THEN COALESCE((SELECT SUM(quantity) FROM return_events WHERE day BETWEEN ?1 AND ?2),0) ELSE COALESCE(SUM(returns),0) END,CASE WHEN EXISTS(SELECT 1 FROM cancellation_events WHERE day BETWEEN ?1 AND ?2) THEN COALESCE((SELECT SUM(quantity) FROM cancellation_events WHERE day BETWEEN ?1 AND ?2),0) ELSE COALESCE(SUM(cancellations),0) END,COALESCE(SUM(views),0) FROM sales_daily WHERE day BETWEEN ?1 AND ?2",params![range.from,range.to],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).map_err(|e|e.to_string())?;
    let sold_units = orders;
    let active_products: i64 = c
        .query_row("SELECT COUNT(*) FROM products", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let (ad_spend,ad_revenue,ad_orders,clicks,impressions):(f64,f64,i64,i64,i64)=c.query_row("WITH x AS(SELECT * FROM ad_daily WHERE day BETWEEN ?1 AND ?2),m AS(SELECT EXISTS(SELECT 1 FROM x WHERE sku='') store)SELECT COALESCE(SUM(spend),0),COALESCE(SUM(revenue),0),COALESCE(SUM(orders),0),COALESCE(SUM(clicks),0),COALESCE(SUM(impressions),0)FROM x,m WHERE (m.store=1 AND x.sku='')OR(m.store=0 AND x.sku<>'')",params![range.from,range.to],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).map_err(|e|e.to_string())?;
    let mut stmt=c.prepare("SELECT s.day,COALESCE(SUM(s.revenue),0),COALESCE(SUM(s.ordered_units),0),COALESCE((SELECT CASE WHEN EXISTS(SELECT 1 FROM ad_daily z WHERE z.day=s.day AND z.sku='') THEN SUM(CASE WHEN a.sku='' THEN a.spend ELSE 0 END) ELSE SUM(CASE WHEN a.sku<>'' THEN a.spend ELSE 0 END) END FROM ad_daily a WHERE a.day=s.day),0) FROM sales_daily s WHERE s.day BETWEEN ?1 AND ?2 GROUP BY s.day ORDER BY s.day").map_err(|e|e.to_string())?;
    let trend = stmt
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
    let last_sync = c
        .query_row(
            "SELECT finished_at FROM sync_logs WHERE status='success' ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .ok();
    Ok(DashboardData {
        revenue,
        orders,
        sold_units,
        active_products,
        ad_spend,
        ad_revenue,
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
           AND (?3='%%' OR r.posting_number LIKE ?3 OR r.sku LIKE ?3 OR r.offer_id LIKE ?3 OR r.product_name LIKE ?3 OR r.status LIKE ?3)
         ORDER BY r.day DESC,r.updated_at DESC LIMIT 500"
    ).map_err(|e| e.to_string())?;
    let raw = stmt
        .query_map(params![range.from, range.to, needle], |r| {
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
        })
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
                OrderRow {
                    event_id,
                    posting_number,
                    sku,
                    offer_id,
                    product_name,
                    quantity,
                    scheme,
                    status,
                    amount: if cross_border { amount / rate } else { amount },
                    created_at,
                    updated_at,
                    origin,
                    destination,
                    estimated_delivery: fee
                        .map(|value| if cross_border { value / rate } else { value }),
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
    let (impressions,clicks,cart_adds,orders,revenue,spend):(i64,i64,i64,i64,f64,f64)=c.query_row("WITH x AS(SELECT * FROM ad_daily WHERE day BETWEEN ?1 AND ?2),m AS(SELECT EXISTS(SELECT 1 FROM x WHERE sku='') store)SELECT COALESCE(SUM(impressions),0),COALESCE(SUM(clicks),0),COALESCE(SUM(cart_adds),0),COALESCE(SUM(orders),0),COALESCE(SUM(revenue),0),COALESCE(SUM(spend),0)FROM x,m WHERE (m.store=1 AND x.sku='')OR(m.store=0 AND x.sku<>'')",params![range.from,range.to],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).map_err(|e|e.to_string())?;
    let mut stmt=c.prepare("WITH x AS(SELECT * FROM ad_daily WHERE day BETWEEN ?1 AND ?2),m AS(SELECT EXISTS(SELECT 1 FROM x WHERE sku='') store)SELECT a.campaign_id,COALESCE(NULLIF(MAX(a.campaign_name),''),MAX(c.name),a.campaign_id),COALESCE(MAX(c.state),''),COALESCE(MAX(c.payment_type),''),SUM(a.impressions),SUM(a.clicks),SUM(a.orders),SUM(a.spend),SUM(a.revenue)FROM x a CROSS JOIN m LEFT JOIN campaigns c ON c.campaign_id=a.campaign_id WHERE (m.store=1 AND a.sku='')OR(m.store=0 AND a.sku<>'')GROUP BY a.campaign_id ORDER BY SUM(a.spend)DESC").map_err(|e|e.to_string())?;
    let campaigns = stmt
        .query_map(params![range.from, range.to], |r| {
            let spend: f64 = r.get(7)?;
            let revenue: f64 = r.get(8)?;
            Ok(CampaignRow {
                id: r.get(0)?,
                name: r.get(1)?,
                state: r.get(2)?,
                payment_type: r.get(3)?,
                impressions: r.get(4)?,
                clicks: r.get(5)?,
                orders: r.get(6)?,
                spend,
                revenue,
                roas: if spend > 0.0 {
                    Some(revenue / spend)
                } else {
                    None
                },
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut trend_stmt=c.prepare("WITH x AS(SELECT * FROM ad_daily WHERE day BETWEEN ?1 AND ?2),m AS(SELECT EXISTS(SELECT 1 FROM x WHERE sku='') store)SELECT a.day,SUM(a.impressions),SUM(a.clicks),SUM(a.orders),SUM(a.spend),SUM(a.revenue)FROM x a CROSS JOIN m WHERE (m.store=1 AND a.sku='')OR(m.store=0 AND a.sku<>'')GROUP BY a.day ORDER BY a.day").map_err(|e|e.to_string())?;
    let trend = trend_stmt
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
    Ok(AdvertisingData {
        impressions,
        clicks,
        cart_adds,
        orders,
        revenue,
        spend,
        ctr: if impressions > 0 {
            Some(clicks as f64 / impressions as f64 * 100.0)
        } else {
            None
        },
        cpc: if clicks > 0 {
            Some(spend / clicks as f64)
        } else {
            None
        },
        roas: if spend > 0.0 {
            Some(revenue / spend)
        } else {
            Some(0.0)
        },
        campaigns,
        trend,
    })
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
fn capture_number(text: &str, patterns: &[&str]) -> Option<f64> {
    let value = first_capture(text, patterns)
        .replace(' ', "")
        .replace("&nbsp;", "")
        .replace(',', ".");
    value.parse().ok()
}
fn canonical_ozon_product_url(value: &str) -> Result<(String, String), String> {
    let code = first_capture(value, &[r#"(?:-|/)(\d{6,})(?:/|\?|$)"#, r#"^(\d{6,})$"#]);
    if code.is_empty() {
        return Err("无法从链接中识别 Ozon 商品编号；也可以直接粘贴纯数字 Артикул".into());
    }
    Ok((format!("https://www.ozon.ru/product/{code}/"), code))
}
fn refresh_competitor_inner(c: &Connection, id: i64) -> Result<(), String> {
    let url = c
        .query_row(
            "SELECT product_url FROM competitor_products WHERE id=?1",
            [id],
            |r| r.get::<_, String>(0),
        )
        .map_err(|e| e.to_string())?;
    let (canonical_url, canonical_code) = canonical_ozon_product_url(&url)?;
    let response = ureq::get(&canonical_url)
        .set(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/131 Safari/537.36",
        )
        .set("Accept-Language", "ru-RU,ru;q=0.9")
        .call()
        .map_err(|e| format!("竞品页面直连读取失败：{e}。已使用规范链接 {canonical_url}；Ozon 若要求验证，请在“跨境上品”打开专用浏览器完成验证后重试"))?;
    let html = response.into_string().map_err(|e| e.to_string())?;
    let name = first_capture(
        &html,
        &[
            r#"<meta[^>]+property=["']og:title["'][^>]+content=["']([^"']+)"#,
            r#"<meta[^>]+content=["']([^"']+)["'][^>]+property=["']og:title["']"#,
            r#"\"name\"\s*:\s*\"([^\"]+)\""#,
            r#"<title>([^<]+)</title>"#,
        ],
    );
    let image = first_capture(
        &html,
        &[
            r#"<meta[^>]+property=["']og:image["'][^>]+content=["']([^"']+)"#,
            r#"<meta[^>]+content=["']([^"']+)["'][^>]+property=["']og:image["']"#,
            r#"\"image\"\s*:\s*\"([^\"]+)\""#,
        ],
    );
    let code = canonical_code;
    let price = capture_number(
        &html,
        &[
            r#"\"price\"\s*:\s*\"?([0-9]+(?:[.,][0-9]+)?)"#,
            r#"\"cardPrice\"\s*:\s*\"?([0-9]+(?:[.,][0-9]+)?)"#,
        ],
    );
    let sales = capture_number(
        &html,
        &[
            r#"\"soldQuantity\"\s*:\s*([0-9]+)"#,
            r#"\"ordersCount\"\s*:\s*([0-9]+)"#,
            r#"\"soldAmount\"\s*:\s*([0-9]+)"#,
        ],
    )
    .map(|v| v as i64);
    if name.is_empty() && image.is_empty() && price.is_none() {
        return Err("页面未返回可识别的商品结构，可能需要登录或页面结构已变化".into());
    }
    c.execute("UPDATE competitor_products SET product_code=CASE WHEN ?2='' THEN product_code ELSE ?2 END,name=CASE WHEN ?3='' THEN name ELSE ?3 END,image_url=CASE WHEN ?4='' THEN image_url ELSE ?4 END,updated_at=CURRENT_TIMESTAMP WHERE id=?1",params![id,code,name,image]).map_err(|e|e.to_string())?;
    c.execute("INSERT INTO competitor_snapshots(competitor_id,captured_at,price,sales_total,source) VALUES(?1,CURRENT_TIMESTAMP,?2,?3,'ozon_public_page')",params![id,price,sales]).map_err(|e|e.to_string())?;
    Ok(())
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
    let mut stmt=c.prepare("SELECT id,product_url,product_code,name,image_url FROM competitor_products WHERE active=1 ORDER BY id").map_err(|e|e.to_string())?;
    let base = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut out = vec![];
    for (id, url, code, name, image) in base {
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
        out.push(CompetitorRow {
            id,
            product_url: url,
            product_code: code,
            name,
            image_url: image,
            latest_price: latest,
            daily_sales: sales_delta(&snaps, 1),
            weekly_sales: sales_delta(&snaps, 7),
            monthly_sales: sales_delta(&snaps, 30),
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
    refresh_competitor_inner(&c, id)?;
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
    refresh_competitor_inner(&c, id)
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
fn refresh_competitors_due_blocking(state: &AppState) -> Result<i64, String> {
    let c = db(&state)?;
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let mut stmt=c.prepare("SELECT p.id FROM competitor_products p WHERE p.active=1 AND COALESCE((SELECT MAX(substr(s.captured_at,1,10)) FROM competitor_snapshots s WHERE s.competitor_id=p.id),'')<?1 ORDER BY p.id").map_err(|e|e.to_string())?;
    let ids = stmt
        .query_map([today], |r| r.get::<_, i64>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut count = 0;
    let mut first_error = None;
    for id in ids {
        match refresh_competitor_inner(&c, id) {
            Ok(()) => count += 1,
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(error)
                }
            }
        }
    }
    if count == 0 {
        if let Some(error) = first_error {
            return Err(error);
        }
    }
    Ok(count)
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
        "starsmembership",
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
        ("last_mile", "末公里配送")
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
    let mut stmt = c.prepare("SELECT s.sku,COALESCE(MAX(p.offer_id),''),COALESCE(MAX(NULLIF(p.name,'')),MAX(s.product_name),''),SUM(s.ordered_units),MAX(COALESCE(pc.unit_cost_cny,pc.unit_cost)),MAX(COALESCE(pc.first_mile_cost,pc.first_mile_cost_cny)),MAX(pc.weight_kg),MAX(pc.length_cm),MAX(pc.width_cm),MAX(pc.height_cm) FROM sales_daily s LEFT JOIN products p ON p.sku=s.sku LEFT JOIN product_costs pc ON pc.sku=s.sku WHERE s.day BETWEEN ?1 AND ?2 AND s.ordered_units<>0 GROUP BY s.sku HAVING MAX(COALESCE(pc.unit_cost_cny,pc.unit_cost)) IS NULL OR MAX(COALESCE(pc.first_mile_cost,pc.first_mile_cost_cny)) IS NULL ORDER BY SUM(s.ordered_units) DESC,s.sku").map_err(|e|e.to_string())?;
    let rows = stmt
        .query_map(params![range.from, range.to], |r| {
            let length: Option<f64> = r.get(7)?;
            let width: Option<f64> = r.get(8)?;
            let height: Option<f64> = r.get(9)?;
            Ok(MissingCostRow {
                sku: r.get(0)?,
                offer_id: r.get(1)?,
                product_name: r.get(2)?,
                units: r.get(3)?,
                missing_purchase: r.get::<_, Option<f64>>(4)?.is_none(),
                missing_first_mile: r.get::<_, Option<f64>>(5)?.is_none(),
                missing_weight: r.get::<_, Option<f64>>(6)?.is_none(),
                missing_dimensions: length.is_none() || width.is_none() || height.is_none(),
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
    let fingerprint:String=c.query_row("SELECT 'finance-v2|'||printf('%d|%s|%d|%d|%d',COALESCE((SELECT MAX(id)FROM sync_logs WHERE status='success' AND source IN('Seller Analytics','Seller Finance','Performance Ads')),0),COALESCE((SELECT MAX(updated_at)FROM product_costs),''),(SELECT COUNT(*)FROM sales_daily),(SELECT COUNT(*)FROM finance_transactions),(SELECT COUNT(*)FROM ad_daily))",[],|r|r.get(0)).map_err(|e|e.to_string())?;
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
    let(revenue,orders,purchase,first_mile,missing):(f64,i64,f64,f64,i64)=c.query_row("SELECT COALESCE(SUM(s.revenue),0),COALESCE(SUM(s.ordered_units),0),COALESCE(SUM(s.ordered_units*COALESCE(pc.unit_cost_cny*?3,pc.unit_cost,0)),0),COALESCE(SUM(s.ordered_units*COALESCE(pc.first_mile_cost,pc.first_mile_cost_cny*?3,0)),0),COALESCE(SUM(CASE WHEN pc.sku IS NULL OR (pc.unit_cost_cny IS NULL AND pc.unit_cost IS NULL) THEN s.ordered_units ELSE 0 END),0) FROM sales_daily s LEFT JOIN product_costs pc ON pc.sku=s.sku WHERE s.day BETWEEN ?1 AND ?2",params![range.from,range.to,rate],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).map_err(|e|e.to_string())?;
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
                sales_returns += value_number(value.get("accruals_for_sale"));
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
    let missing_cost_skus:i64=c.query_row("SELECT COUNT(DISTINCT s.sku)FROM sales_daily s LEFT JOIN product_costs pc ON pc.sku=s.sku WHERE s.day BETWEEN ?1 AND ?2 AND s.ordered_units<>0 AND (pc.sku IS NULL OR (pc.unit_cost_cny IS NULL AND pc.unit_cost IS NULL)OR(pc.first_mile_cost IS NULL AND pc.first_mile_cost_cny IS NULL))",params![range.from,range.to],|r|r.get(0)).map_err(|e|e.to_string())?;
    let costed_units = orders - missing;
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
    let fingerprint:String=c.query_row("SELECT 'analytics-v4|'||printf('%d|%s|%d|%d|%d',COALESCE((SELECT MAX(id)FROM sync_logs WHERE status='success' AND source IN('Seller Analytics','Seller Finance','Performance Ads')),0),COALESCE((SELECT MAX(updated_at)FROM product_costs),''),(SELECT COUNT(*)FROM sales_daily),(SELECT COUNT(*)FROM finance_transactions),(SELECT COUNT(*)FROM ad_daily))",[],|r|r.get(0)).map_err(|e|e.to_string())?;
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
    let mut weekly_stmt=c.prepare("WITH periods(label,from_day,to_day,sort_no) AS (VALUES('本周',date(?2,'-6 day'),?2,0),('上周',date(?2,'-13 day'),date(?2,'-7 day'),1)) SELECT p.label||' ('||substr(p.from_day,6)||'~'||substr(p.to_day,6)||')',COALESCE(SUM(s.revenue),0),COALESCE(SUM(s.ordered_units),0),COALESCE((SELECT SUM(ABS(a.spend)) FROM ad_daily a WHERE a.day BETWEEN p.from_day AND p.to_day),0),COALESCE(SUM(s.ordered_units*COALESCE(pc.unit_cost_cny*?3,pc.unit_cost,0)),0),COALESCE(SUM(s.ordered_units*COALESCE(pc.first_mile_cost,pc.first_mile_cost_cny*?3,0)),0),COALESCE((SELECT SUM(a.orders) FROM ad_daily a WHERE a.day BETWEEN p.from_day AND p.to_day),0),COALESCE(SUM(s.returns),0),COALESCE(SUM(s.cancellations),0) FROM periods p LEFT JOIN sales_daily s ON s.day BETWEEN p.from_day AND p.to_day LEFT JOIN product_costs pc ON pc.sku=s.sku GROUP BY p.label,p.from_day,p.to_day,p.sort_no ORDER BY p.sort_no").map_err(|e|e.to_string())?;
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
    let mut weekly_daily_stmt=c.prepare("SELECT s.day,SUM(s.revenue),SUM(s.ordered_units),COALESCE((SELECT SUM(ABS(a.spend))FROM ad_daily a WHERE a.day=s.day),0),COALESCE((SELECT SUM(a.orders)FROM ad_daily a WHERE a.day=s.day),0),SUM(s.returns),SUM(s.cancellations) FROM sales_daily s WHERE s.day BETWEEN date(?1,'-6 day') AND ?1 GROUP BY s.day ORDER BY s.day").map_err(|e|e.to_string())?;
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
    state: &AppState,
) -> Result<Vec<InventoryRow>, String> {
    let c = db(state)?;
    let needle = format!("%{}%", query.trim());
    let mut stmt = c.prepare("WITH inv AS(SELECT sku,MAX(offer_id)offer_id,MAX(product_name)product_name,SUM(available_stock)available,SUM(transit_stock)transit,SUM(requested_stock)requested,COUNT(DISTINCT warehouse_id)warehouses,MAX(updated_at)updated FROM inventory_stock GROUP BY sku),sales AS(SELECT sku,SUM(ordered_units)/30.0 daily FROM sales_daily WHERE day>=date('now','-29 day') GROUP BY sku),plans AS(SELECT sku,SUM(planned_qty)planned FROM replenishment_plan GROUP BY sku) SELECT i.sku,i.offer_id,i.product_name,i.available,t.present_stock,t.reserved_stock,i.transit,i.requested,i.warehouses,COALESCE(s.daily,0),COALESCE(p.planned,0),i.updated FROM inv i LEFT JOIN inventory_totals t ON t.sku=i.sku LEFT JOIN sales s ON s.sku=i.sku LEFT JOIN plans p ON p.sku=i.sku WHERE ?1='%%' OR i.sku LIKE ?1 OR i.offer_id LIKE ?1 OR i.product_name LIKE ?1 ORDER BY i.available,i.offer_id LIMIT 2000").map_err(|e|e.to_string())?;
    let raw = stmt
        .query_map([needle], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, Option<i64>>(4)?,
                r.get::<_, Option<i64>>(5)?,
                r.get::<_, i64>(6)?, r.get::<_, i64>(7)?, r.get::<_, i64>(8)?,
                r.get::<_, f64>(9)?, r.get::<_, i64>(10)?, r.get::<_, String>(11)?,
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
                warehouses,
                daily,
                planned,
                updated,
            )| {
                let estimated = if daily > 0.0 {
                    Some(available as f64 / daily)
                } else {
                    None
                };
                let suggested =
                    ((daily * target_days as f64).ceil() as i64 - available - transit - requested)
                        .max(0);
                InventoryRow {
                    sku,
                    offer_id,
                    product_name,
                    available_stock: available,
                    portal_stock: portal,
                    reserved_stock: reserved,
                    transit_stock: transit,
                    requested_stock: requested,
                    warehouse_count: warehouses,
                    daily_sales: daily,
                    estimated_days: estimated,
                    suggested_qty: suggested,
                    planned_qty: planned,
                    updated_at: updated,
                }
            },
        )
        .collect())
}

#[tauri::command]
async fn inventory(query:String,target_days:i64,state:State<'_,AppState>)->Result<Vec<InventoryRow>,String>{
    let owned=background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move||inventory_blocking(query,target_days,&owned))
        .await.map_err(|e|format!("库存读取后台任务失败：{e}"))?
}

fn sync_inventory_blocking(state: &AppState) -> Result<i64, String> {
    let _sync_guard = API_SYNC_LOCK
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
        let mut offers = items.iter().filter_map(|v| v.as_object())
            .map(|v| object_text(v, &["offer_id", "offerId"]))
            .filter(|v| !v.is_empty()).collect::<Vec<_>>();
        offers.sort(); offers.dedup();
        let mut totals = Vec::<(String,String,i64,i64)>::new();
        for batch in offers.chunks(100) {
            let payload = seller_post(&c, "/v4/product/info/stocks",
                &serde_json::json!({"filter":{"offer_id":batch,"product_id":[],"visibility":"ALL"},"limit":1000,"cursor":""}))?;
            for value in payload.get("items").and_then(|v|v.as_array()).into_iter().flatten() {
                let Some(item)=value.as_object() else { continue };
                let sku=object_text(item,&["product_id","sku"]);
                let offer=object_text(item,&["offer_id","offerId"]);
                let mut present=0_i64; let mut reserved=0_i64;
                for stock in item.get("stocks").and_then(|v|v.as_array()).into_iter().flatten() {
                    let kind=stock.get("type").and_then(|v|v.as_str()).unwrap_or("").to_ascii_lowercase();
                    if kind=="fbo" || kind.is_empty() {
                        present += stock.get("present").and_then(|v|v.as_i64()).unwrap_or(0);
                        reserved += stock.get("reserved").and_then(|v|v.as_i64()).unwrap_or(0);
                    }
                }
                if !sku.is_empty() { totals.push((sku,offer,present,reserved)); }
            }
        }
        let tx = c.transaction().map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM inventory_stock", [])
            .map_err(|e| e.to_string())?;
        for (sku,offer,present,reserved) in totals {
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

fn sync_seller_sales_blocking(range: DateRange, state: &AppState) -> Result<i64, String> {
    let _sync_guard = API_SYNC_LOCK
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
        let checkpoint = c.query_row("SELECT api_from,next_offset,rows_count FROM sync_progress WHERE source='Seller Analytics' AND range_from=?1 AND range_to=?2",params![range.from,range.to],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?))).ok();
        let cached:(i64,String,String)=c.query_row("SELECT COUNT(*),COALESCE(MIN(day),''),COALESCE(MAX(day),'') FROM sales_daily WHERE source='api' AND day BETWEEN ?1 AND ?2",params![range.from,range.to],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e|e.to_string())?;
        let today = chrono::Local::now()
            .date_naive()
            .format("%Y-%m-%d")
            .to_string();
        if checkpoint.is_none()
            && cached.0 > 0
            && cached.1 <= range.from
            && cached.2 >= range.to
            && range.to < today
        {
            return Ok(cached.0);
        }
        let (api_from, mut offset, checkpoint_rows) = checkpoint.unwrap_or_else(|| {
            let incremental_from =
                if cached.0 > 0 && cached.1 <= range.from && cached.2 >= range.from {
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
async fn sync_seller_sales(range: DateRange, state: State<'_, AppState>) -> Result<i64, String> {
    let owned = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || {
        let count = sync_seller_sales_blocking(range.clone(), &owned)?;
        // Orders are a separate Seller endpoint. Cache them after analytics so
        // the order center is populated by the same user-visible sync action.
        let _ = sync_fbs_orders_blocking(range.clone(), &owned);
        let _ = sync_fbo_orders_blocking(range, &owned);
        Ok(count)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn sync_performance_ads_blocking(range: DateRange, state: &AppState) -> Result<i64, String> {
    let _sync_guard = API_SYNC_LOCK
        .try_lock()
        .map_err(|_| "已有数据同步任务正在后台运行，请等待完成。".to_string())?;
    let mut c = db(state)?;
    c.execute("INSERT INTO sync_logs(started_at,source,status) VALUES(CURRENT_TIMESTAMP,'Performance Ads','running')",[]).map_err(|e|e.to_string())?;
    let log_id = c.last_insert_rowid();
    let result = (|| -> Result<i64, String> {
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
                tx.execute("INSERT INTO campaigns(campaign_id,name,state,payment_type,budget,source)VALUES(?1,?2,?3,?4,?5,'api') ON CONFLICT(campaign_id) DO UPDATE SET name=excluded.name,state=excluded.state,payment_type=excluded.payment_type,budget=excluded.budget,source='api',updated_at=CURRENT_TIMESTAMP",params![id,name,json_text(campaign.get("state")),json_text(campaign.get("paymentType")),campaign.get("budget").and_then(|v|v.as_f64()).unwrap_or(0.0)]).map_err(|e|e.to_string())?;
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
            if day < range.from || day > range.to { continue; }
            let ids = names.keys().cloned().collect::<Vec<_>>();
            for batch in ids.chunks(100) {
                let detail = performance_post("/api/client/statistics/products/sku", &token,
                    &serde_json::json!({"campaignIds":batch,"dateFrom":day,"dateTo":day}))?;
                let mut detail_objects = Vec::new();
                collect_objects(&detail, &mut detail_objects);
                let detail_tx = c.transaction().map_err(|e| e.to_string())?;
                for o in detail_objects {
                    let campaign_id = object_text(&o, &["campaignId", "campaign_id"]);
                    let sku = object_text(&o, &["sku"]);
                    if campaign_id.is_empty() || sku.is_empty() { continue; }
                    let returned_day = object_text(&o, &["date", "day"]);
                    let row_day = if returned_day.is_empty() { day.clone() } else { returned_day.chars().take(10).collect() };
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
async fn sync_performance_ads(range: DateRange, state: State<'_, AppState>) -> Result<i64, String> {
    let owned = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || sync_performance_ads_blocking(range, &owned))
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

fn sync_finance_blocking(range: DateRange, state: &AppState) -> Result<i64, String> {
    let _sync_guard = API_SYNC_LOCK
        .try_lock()
        .map_err(|_| "已有数据同步任务正在后台运行，请等待完成。".to_string())?;
    let mut c = db(state)?;
    c.execute("INSERT INTO sync_logs(started_at,source,status) VALUES(CURRENT_TIMESTAMP,'Seller Finance','running')",[]).map_err(|e|e.to_string())?;
    let log_id = c.last_insert_rowid();
    let result = (|| -> Result<i64, String> {
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
            let sku = if skus.len() == 1 {
                skus.into_iter().next().unwrap_or_default()
            } else {
                json_text(op.get("sku"))
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
async fn sync_finance(range: DateRange, state: State<'_, AppState>) -> Result<i64, String> {
    let owned = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || sync_finance_blocking(range, &owned))
        .await
        .map_err(|e| e.to_string())?
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

fn sync_feishu_products_blocking(direction: String, state: &AppState) -> Result<String, String> {
    if !matches!(direction.as_str(), "pull" | "push" | "both") {
        return Err("飞书商品同步方向无效".into());
    }
    let mut c = db(&state)?;
    let token = feishu_token(&c)?;
    let path = feishu_table_path(&c)?;
    c.execute("INSERT INTO sync_logs(started_at,source,status) VALUES(CURRENT_TIMESTAMP,'Feishu Products','running')",[]).map_err(|e|e.to_string())?;
    let log_id = c.last_insert_rowid();
    let result = (|| -> Result<(i64, i64, i64), String> {
        let definitions = [
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
            if !sku.is_empty() && !remote.contains_key(&sku) {
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
async fn sync_feishu_products(direction: String, state: State<'_, AppState>) -> Result<String, String> {
    let owned=background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move||sync_feishu_products_blocking(direction,&owned))
        .await.map_err(|e|format!("飞书商品后台同步失败：{e}"))?
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
    let mut stmt=c.prepare("SELECT tracking_id,product_name,batch_no,shop_name,quantity,cargo_status,channel,domestic_arrival,foreign_arrival,notified_foreign_arrival,source,updated_at FROM shipment_tracking ORDER BY CASE WHEN foreign_arrival<>'' AND foreign_arrival<>notified_foreign_arrival THEN 0 ELSE 1 END,updated_at DESC").map_err(|e|e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            let foreign: String = r.get(8)?;
            let notified: String = r.get(9)?;
            Ok(ShipmentRow {
                tracking_id: r.get(0)?,
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
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

fn sync_feishu_shipments_blocking(state: &AppState) -> Result<i64, String> {
    let mut c = db(state)?;
    let token = feishu_token(&c)?;
    let app = setting(&c, "feishu_app_token");
    let table = setting(&c, "feishu_tracking_table_id");
    if app.is_empty() || table.is_empty() {
        return Err("请先配置飞书 App Token 和发货跟踪 Table ID".into());
    }
    let path = format!("{}/bitable/v1/apps/{app}/tables/{table}", feishu_base(&c));
    let records = feishu_records(&token, &path)?;
    let tx = c.transaction().map_err(|e| e.to_string())?;
    let mut count = 0;
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
        tx.execute("INSERT INTO shipment_tracking(tracking_id,product_name,batch_no,shop_name,quantity,cargo_status,channel,domestic_arrival,foreign_arrival,source,remote_record_id)VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'feishu',?10) ON CONFLICT(tracking_id) DO UPDATE SET product_name=excluded.product_name,batch_no=excluded.batch_no,shop_name=excluded.shop_name,quantity=excluded.quantity,cargo_status=excluded.cargo_status,channel=excluded.channel,domestic_arrival=excluded.domestic_arrival,foreign_arrival=excluded.foreign_arrival,source='feishu',remote_record_id=excluded.remote_record_id,updated_at=CURRENT_TIMESTAMP",params![tracking,first_feishu_field(fields,&["品名","产品名称","商品名称"]),first_feishu_field(fields,&["批次号","批次"]),first_feishu_field(fields,&["店铺","店铺名称"]),quantity,first_feishu_field(fields,&["货物状态","状态"]),first_feishu_field(fields,&["渠道","物流渠道"]),domestic,foreign,json_text(record.get("record_id"))]).map_err(|e|e.to_string())?;
        count += 1;
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(count)
}

#[tauri::command]
async fn sync_feishu_shipments(state: State<'_, AppState>) -> Result<i64, String> {
    let owned=background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move||sync_feishu_shipments_blocking(&owned))
        .await.map_err(|e|format!("飞书发货后台同步失败：{e}"))?
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
fn supply_orders(state: State<AppState>) -> Result<Vec<SupplyOrderRow>, String> {
    let c = db(&state)?;
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
fn supply_timeslots(
    order_id: i64,
    date_from: String,
    date_to: String,
    state: State<AppState>,
) -> Result<Vec<SupplyTimeslot>, String> {
    if date_from > date_to {
        return Err("时间窗开始日期不能晚于结束日期".into());
    }
    let c = db(&state)?;
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
fn book_supply_timeslot(
    supply_order_id: i64,
    timeslot_from: String,
    timeslot_to: String,
    confirmation: String,
    state: State<AppState>,
) -> Result<String, String> {
    if confirmation != "确认预约" {
        return Err("预约未确认；必须输入“确认预约”".into());
    }
    let c = db(&state)?;
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let exe = std::env::current_exe()?;
            let data_dir = locate_data_dir(&exe).map_err(std::io::Error::other)?;
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
            add_competitor,
            refresh_competitor,
            refresh_competitors_due,
            remove_competitor,
            business_report,
            analytics_detail,
            cross_border_report,
            finance_breakdown,
            data_coverage,
            prune_cache,
            missing_cost_rows,
            supply_orders,
            supply_timeslots,
            book_supply_timeslot,
            sync_logs,
            sync_seller_sales,
            sync_performance_ads,
            sync_finance,
            test_feishu,
            sync_feishu_products,
            send_feishu_weekly,
            shipment_tracking,
            sync_feishu_shipments,
            notify_feishu_shipment,
            wb::wb_settings,
            wb::save_wb_settings,
            wb::export_wb_api_bundle,
            wb::import_wb_api_bundle,
            wb::wb_costs,
            wb::save_wb_cost,
            wb::wb_daily,
            wb::sync_wb,
            wb::test_wb_feishu,
            wb::send_wb_weekly,
            listing::listing_settings,
            listing::save_listing_settings,
            listing::listing_rows,
            listing::sync_listing_costs,
            listing::calculate_listing_price,
            listing::create_listing_draft,
            listing::listing_jobs,
            listing::save_listing_draft,
            listing::retry_listing_job,
            listing::collect_listing_reference,
            listing::launch_listing_tool,
            insights::product_insights,
            insights::series_insights,
            insights::save_product_series,
            insights::delete_product_series,
            insights::product_detail,
            insights::save_product_cluster_weights
        ])
        .run(tauri::generate_context!())
        .expect("failed to run app")
}
