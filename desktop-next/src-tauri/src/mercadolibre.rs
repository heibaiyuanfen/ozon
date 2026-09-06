use crate::{secrets, AppState, DateRange};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{fs, path::PathBuf};
use tauri::State;

fn db_path(state: &AppState) -> Result<PathBuf, String> {
    let dir = state.data_dir.join("mercadolibre");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("mercadolibre.db"))
}

fn db(state: &AppState) -> Result<Connection, String> {
    let c = Connection::open(db_path(state)?).map_err(|e| e.to_string())?;
    c.execute_batch(
        "PRAGMA journal_mode=WAL;PRAGMA foreign_keys=ON;
         CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY,value TEXT NOT NULL DEFAULT '');
         CREATE TABLE IF NOT EXISTS listing_drafts(
           id INTEGER PRIMARY KEY AUTOINCREMENT,title TEXT NOT NULL,category_id TEXT NOT NULL DEFAULT '',
           price REAL NOT NULL DEFAULT 0,currency_id TEXT NOT NULL DEFAULT 'MXN',available_quantity INTEGER NOT NULL DEFAULT 0,
           condition_code TEXT NOT NULL DEFAULT 'new',listing_type_id TEXT NOT NULL DEFAULT 'gold_special',
           pictures_json TEXT NOT NULL DEFAULT '[]',attributes_json TEXT NOT NULL DEFAULT '[]',status TEXT NOT NULL DEFAULT 'draft',
           remote_item_id TEXT NOT NULL DEFAULT '',error TEXT NOT NULL DEFAULT '',created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
           updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
         CREATE TABLE IF NOT EXISTS orders(
           order_id TEXT PRIMARY KEY,created_at TEXT NOT NULL,status TEXT NOT NULL DEFAULT '',currency_id TEXT NOT NULL DEFAULT '',
           total_amount REAL NOT NULL DEFAULT 0,units INTEGER NOT NULL DEFAULT 0,raw_json TEXT NOT NULL DEFAULT '{}',
           updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
         CREATE INDEX IF NOT EXISTS idx_ml_orders_created ON orders(created_at);",
    )
    .map_err(|e| e.to_string())?;
    Ok(c)
}

fn setting(c: &Connection, key: &str) -> Result<String, String> {
    c.query_row("SELECT value FROM settings WHERE key=?1", [key], |r| {
        r.get(0)
    })
    .optional()
    .map_err(|e| e.to_string())
    .map(|v| v.unwrap_or_default())
}

fn save_setting(c: &Connection, key: &str, value: &str) -> Result<(), String> {
    c.execute(
        "INSERT INTO settings(key,value)VALUES(?1,?2) ON CONFLICT(key)DO UPDATE SET value=excluded.value",
        params![key, value],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn token(c: &Connection) -> Result<String, String> {
    secrets::unprotect(&setting(c, "access_token")?)
}

fn api(method: &str, path: &str, access_token: &str, body: Option<Value>) -> Result<Value, String> {
    if access_token.is_empty() {
        return Err("尚未配置美客多 OAuth Access Token".into());
    }
    let url = format!("https://api.mercadolibre.com{path}");
    let request = ureq::request(method, &url)
        .set("Authorization", &format!("Bearer {access_token}"))
        .set("Accept", "application/json")
        .set("Content-Type", "application/json")
        .set("User-Agent", "OzonERP-MercadoLibre/1.0");
    let response = match body {
        Some(value) => request.send_string(&value.to_string()),
        None => request.call(),
    }
    .map_err(|error| match error {
        ureq::Error::Status(status, response) => {
            let detail = response.into_string().unwrap_or_default();
            format!("美客多 API 请求失败（HTTP {status}）：{detail}")
        }
        other => format!("美客多 API 连接失败：{other}"),
    })?;
    let text = response.into_string().map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| format!("美客多 API 返回无法解析：{e}"))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MlSettings {
    site_id: String,
    seller_id: String,
    currency_id: String,
    token_configured: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MlSettingsInput {
    site_id: String,
    seller_id: String,
    currency_id: String,
    access_token: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MlDraft {
    id: Option<i64>,
    title: String,
    category_id: String,
    price: f64,
    currency_id: String,
    available_quantity: i64,
    condition_code: String,
    listing_type_id: String,
    pictures: Vec<String>,
    attributes: Value,
    #[serde(default)]
    status: String,
    #[serde(default)]
    remote_item_id: String,
    #[serde(default)]
    error: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MlAnalyticsDay {
    day: String,
    orders: i64,
    units: i64,
    revenue: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MlAnalytics {
    date_from: String,
    date_to: String,
    orders: i64,
    units: i64,
    revenue: f64,
    average_order_value: Option<f64>,
    currency_id: String,
    daily: Vec<MlAnalyticsDay>,
}

fn validate_draft(draft: &MlDraft) -> Result<(), String> {
    if draft.title.trim().is_empty() || draft.title.chars().count() > 60 {
        return Err("标题不能为空且不能超过 60 个字符".into());
    }
    if draft.category_id.trim().is_empty() {
        return Err("必须填写美客多叶子类目 ID".into());
    }
    if !draft.price.is_finite() || draft.price <= 0.0 {
        return Err("售价必须大于 0".into());
    }
    if draft.available_quantity < 0 {
        return Err("库存不能小于 0".into());
    }
    if !matches!(
        draft.condition_code.as_str(),
        "new" | "used" | "not_specified"
    ) {
        return Err("商品成色必须为 new、used 或 not_specified".into());
    }
    if !draft.attributes.is_array() {
        return Err("属性必须是 JSON 数组".into());
    }
    Ok(())
}

#[tauri::command]
pub fn ml_settings(state: State<AppState>) -> Result<MlSettings, String> {
    let c = db(&state)?;
    Ok(MlSettings {
        site_id: setting(&c, "site_id")?.if_empty("MLM"),
        seller_id: setting(&c, "seller_id")?,
        currency_id: setting(&c, "currency_id")?.if_empty("MXN"),
        token_configured: !setting(&c, "access_token")?.is_empty(),
    })
}

trait DefaultIfEmpty {
    fn if_empty(self, fallback: &str) -> String;
}
impl DefaultIfEmpty for String {
    fn if_empty(self, fallback: &str) -> String {
        if self.is_empty() {
            fallback.into()
        } else {
            self
        }
    }
}

#[tauri::command]
pub fn save_ml_settings(input: MlSettingsInput, state: State<AppState>) -> Result<(), String> {
    let c = db(&state)?;
    save_setting(&c, "site_id", input.site_id.trim())?;
    save_setting(&c, "seller_id", input.seller_id.trim())?;
    save_setting(&c, "currency_id", input.currency_id.trim())?;
    if !input.access_token.trim().is_empty() {
        save_setting(
            &c,
            "access_token",
            &secrets::protect(input.access_token.trim())?,
        )?;
    }
    Ok(())
}

#[tauri::command]
pub fn test_ml_connection(state: State<AppState>) -> Result<String, String> {
    let c = db(&state)?;
    let seller_id = setting(&c, "seller_id")?;
    if seller_id.is_empty() {
        return Err("请先填写卖家 ID".into());
    }
    let value = api("GET", &format!("/users/{seller_id}"), &token(&c)?, None)?;
    Ok(format!(
        "连接成功：{}",
        value
            .get("nickname")
            .and_then(Value::as_str)
            .unwrap_or(&seller_id)
    ))
}

#[tauri::command]
pub fn ml_drafts(state: State<AppState>) -> Result<Vec<MlDraft>, String> {
    let c = db(&state)?;
    load_drafts(&c)
}

fn load_drafts(c: &Connection) -> Result<Vec<MlDraft>, String> {
    let mut stmt = c.prepare("SELECT id,title,category_id,price,currency_id,available_quantity,condition_code,listing_type_id,pictures_json,attributes_json,status,remote_item_id,error FROM listing_drafts ORDER BY updated_at DESC,id DESC").map_err(|e|e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok(MlDraft {
                id: r.get(0)?,
                title: r.get(1)?,
                category_id: r.get(2)?,
                price: r.get(3)?,
                currency_id: r.get(4)?,
                available_quantity: r.get(5)?,
                condition_code: r.get(6)?,
                listing_type_id: r.get(7)?,
                pictures: serde_json::from_str::<Vec<String>>(&r.get::<_, String>(8)?)
                    .unwrap_or_default(),
                attributes: serde_json::from_str(&r.get::<_, String>(9)?)
                    .unwrap_or_else(|_| json!([])),
                status: r.get(10)?,
                remote_item_id: r.get(11)?,
                error: r.get(12)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

#[tauri::command]
pub fn save_ml_draft(draft: MlDraft, state: State<AppState>) -> Result<i64, String> {
    validate_draft(&draft)?;
    let c = db(&state)?;
    let pictures = serde_json::to_string(&draft.pictures).map_err(|e| e.to_string())?;
    let attributes = serde_json::to_string(&draft.attributes).map_err(|e| e.to_string())?;
    if let Some(id) = draft.id {
        c.execute("UPDATE listing_drafts SET title=?1,category_id=?2,price=?3,currency_id=?4,available_quantity=?5,condition_code=?6,listing_type_id=?7,pictures_json=?8,attributes_json=?9,updated_at=CURRENT_TIMESTAMP WHERE id=?10",params![draft.title.trim(),draft.category_id.trim(),draft.price,draft.currency_id,draft.available_quantity,draft.condition_code,draft.listing_type_id,pictures,attributes,id]).map_err(|e|e.to_string())?;
        Ok(id)
    } else {
        c.execute("INSERT INTO listing_drafts(title,category_id,price,currency_id,available_quantity,condition_code,listing_type_id,pictures_json,attributes_json)VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![draft.title.trim(),draft.category_id.trim(),draft.price,draft.currency_id,draft.available_quantity,draft.condition_code,draft.listing_type_id,pictures,attributes]).map_err(|e|e.to_string())?;
        Ok(c.last_insert_rowid())
    }
}

#[tauri::command]
pub fn publish_ml_draft(id: i64, state: State<AppState>) -> Result<String, String> {
    let c = db(&state)?;
    let draft = load_drafts(&c)?
        .into_iter()
        .find(|x| x.id == Some(id))
        .ok_or("草稿不存在")?;
    validate_draft(&draft)?;
    let body = json!({"title":draft.title,"category_id":draft.category_id,"price":draft.price,"currency_id":draft.currency_id,"available_quantity":draft.available_quantity,"buying_mode":"buy_it_now","condition":draft.condition_code,"listing_type_id":draft.listing_type_id,"pictures":draft.pictures.into_iter().map(|source|json!({"source":source})).collect::<Vec<_>>(),"attributes":draft.attributes});
    match api("POST", "/items", &token(&c)?, Some(body)) {
        Ok(value) => {
            let remote = value
                .get("id")
                .and_then(Value::as_str)
                .ok_or("美客多返回中缺少商品 ID")?
                .to_string();
            c.execute("UPDATE listing_drafts SET status='published',remote_item_id=?1,error='',updated_at=CURRENT_TIMESTAMP WHERE id=?2",params![remote,id]).map_err(|e|e.to_string())?;
            Ok(remote)
        }
        Err(error) => {
            c.execute("UPDATE listing_drafts SET status='failed',error=?1,updated_at=CURRENT_TIMESTAMP WHERE id=?2",params![error,id]).map_err(|e|e.to_string())?;
            Err(error)
        }
    }
}

#[tauri::command]
pub fn sync_ml_orders(range: DateRange, state: State<AppState>) -> Result<usize, String> {
    let mut c = db(&state)?;
    let seller = setting(&c, "seller_id")?;
    if seller.is_empty() {
        return Err("请先配置卖家 ID".into());
    }
    let access = token(&c)?;
    let from = format!("{}T00:00:00.000-00:00", range.from);
    let to = format!("{}T23:59:59.999-00:00", range.to);
    let mut offset = 0usize;
    let mut count = 0usize;
    loop {
        let path=format!("/orders/search?seller={seller}&order.date_created.from={from}&order.date_created.to={to}&sort=date_desc&limit=50&offset={offset}");
        let value = api("GET", &path, &access, None)?;
        let results = value
            .get("results")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if results.is_empty() {
            break;
        }
        let tx = c.transaction().map_err(|e| e.to_string())?;
        for order in &results {
            let id = order
                .get("id")
                .map(|v| v.to_string())
                .unwrap_or_default()
                .trim_matches('"')
                .to_string();
            let created = order
                .get("date_created")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let status = order
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let currency = order
                .get("currency_id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let amount = order
                .get("total_amount")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let units = order
                .get("order_items")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .map(|item| item.get("quantity").and_then(Value::as_i64).unwrap_or(0))
                        .sum()
                })
                .unwrap_or(0);
            tx.execute("INSERT INTO orders(order_id,created_at,status,currency_id,total_amount,units,raw_json)VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(order_id)DO UPDATE SET created_at=excluded.created_at,status=excluded.status,currency_id=excluded.currency_id,total_amount=excluded.total_amount,units=excluded.units,raw_json=excluded.raw_json,updated_at=CURRENT_TIMESTAMP",params![id,created,status,currency,amount,units,order.to_string()]).map_err(|e|e.to_string())?;
            count += 1;
        }
        tx.commit().map_err(|e| e.to_string())?;
        offset += results.len();
        if results.len() < 50 || offset >= 10000 {
            break;
        }
    }
    Ok(count)
}

#[tauri::command]
pub fn ml_analytics(range: DateRange, state: State<AppState>) -> Result<MlAnalytics, String> {
    let c = db(&state)?;
    let (orders,units,revenue):(i64,i64,f64)=c.query_row("SELECT COUNT(*),COALESCE(SUM(units),0),COALESCE(SUM(total_amount),0) FROM orders WHERE substr(created_at,1,10) BETWEEN ?1 AND ?2 AND status NOT IN ('cancelled','invalid')",params![range.from,range.to],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e|e.to_string())?;
    let mut stmt=c.prepare("SELECT substr(created_at,1,10),COUNT(*),COALESCE(SUM(units),0),COALESCE(SUM(total_amount),0) FROM orders WHERE substr(created_at,1,10) BETWEEN ?1 AND ?2 AND status NOT IN ('cancelled','invalid') GROUP BY 1 ORDER BY 1").map_err(|e|e.to_string())?;
    let daily = stmt
        .query_map(params![range.from, range.to], |r| {
            Ok(MlAnalyticsDay {
                day: r.get(0)?,
                orders: r.get(1)?,
                units: r.get(2)?,
                revenue: r.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(MlAnalytics {
        date_from: range.from,
        date_to: range.to,
        orders,
        units,
        revenue,
        average_order_value: (orders > 0).then_some(revenue / orders as f64),
        currency_id: setting(&c, "currency_id")?.if_empty("MXN"),
        daily,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_listing_boundaries() {
        let valid = MlDraft {
            id: None,
            title: "Producto de prueba".into(),
            category_id: "MLM123".into(),
            price: 99.0,
            currency_id: "MXN".into(),
            available_quantity: 2,
            condition_code: "new".into(),
            listing_type_id: "gold_special".into(),
            pictures: vec![],
            attributes: json!([]),
            status: String::new(),
            remote_item_id: String::new(),
            error: String::new(),
        };
        assert!(validate_draft(&valid).is_ok());
        assert!(validate_draft(&MlDraft {
            price: 0.0,
            ..valid
        })
        .is_err());
    }
}
