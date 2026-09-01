use crate::{
    background_state, chat_endpoint, collect_competitor_browser_html, db, read_registry,
    save_setting, secret_setting, seller_post, setting, AppState,
};
use calamine::{open_workbook_auto, Reader};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    path::Path,
    process::Command,
    sync::{Mutex, OnceLock},
    time::UNIX_EPOCH,
};
use tauri::State;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListingSettings {
    ledger_path: String,
    ledger_shop_name: String,
    tool_executable: String,
    tool_data_dir: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListingRow {
    shop_name: String,
    platform: String,
    offer_id: String,
    product_id: String,
    product_title: String,
    supplier_url: String,
    currency_code: String,
    unit_cost_cny: Option<f64>,
    weight_kg: Option<f64>,
    length_cm: Option<f64>,
    width_cm: Option<f64>,
    height_cm: Option<f64>,
    status: String,
    listing_mode: String,
    pricing_mode: String,
    price: Option<f64>,
    profit: Option<f64>,
    roi_percent: Option<f64>,
    category: String,
    import_task_id: String,
    updated_at: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListingPriceInput {
    purchase_cost: f64,
    label_fee: f64,
    target_roi_percent: f64,
    weight_kg: f64,
    sales_commission_percent: f64,
    sales_commission_discount_percent: f64,
    advertising_percent: f64,
    cargo_loss_percent: f64,
    minimum_sale_price: f64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ListingPriceBreakdown {
    price: f64,
    shipping: f64,
    sales_commission: f64,
    logistics_commission: f64,
    advertising: f64,
    cargo_loss: f64,
    invested: f64,
    profit: f64,
    roi_percent: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListingJobRow {
    id: i64,
    source_url: String,
    article: String,
    offer_id: String,
    title: String,
    category_id: String,
    category_display: String,
    status: String,
    stage: i64,
    error: String,
    payload: Value,
    updated_at: String,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ListingCategory {
    description_category_id: i64,
    type_id: i64,
    name: String,
    display: String,
    score: f64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ListingAttributeDefinition {
    id: i64,
    name: String,
    description: String,
    attribute_complex_id: i64,
    is_required: bool,
    is_collection: bool,
    dictionary_id: i64,
    max_value_count: i64,
    group_name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListingValidation {
    valid: bool,
    issues: Vec<String>,
    missing_required: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListingAiFillResult {
    filled: i64,
    free_text_filled: i64,
    dictionary_filled: i64,
    missing_required: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListingAttributeValueInput {
    id: i64,
    attribute_id: i64,
    attribute_complex_id: i64,
    attribute_name: String,
    is_collection: bool,
    dictionary_value_id: i64,
    value: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListingDraftInput {
    id: i64,
    offer_id: String,
    title: String,
    category_id: String,
    category_display: String,
    type_id: String,
    price: String,
    weight: f64,
    depth: f64,
    width: f64,
    height: f64,
    description: String,
    images: Vec<String>,
    attributes: Value,
    complex_attributes: Value,
}

fn normalize_reference_input(value: &str) -> Result<(String, String), String> {
    let raw = value.trim();
    let labelled =
        regex::Regex::new(r"(?i)^(?:артикул|sku|商品编号)\s*[:：#]?\s*(\d{6,})$").unwrap();
    let direct = regex::Regex::new(r"^\d{6,}$").unwrap();
    let article = if direct.is_match(raw) {
        raw.to_string()
    } else if let Some(c) = labelled.captures(raw) {
        c[1].to_string()
    } else {
        let host = regex::Regex::new(r"(?i)^https?://(?:www\.)?[^/]*ozon\.ru/").unwrap();
        if !host.is_match(raw) {
            return Err("请输入 Ozon 商品链接或纯数字 Артикул".into());
        }
        regex::Regex::new(r"(?:-|/)(\d{6,})/?(?:\?[^#]*)?(?:#.*)?$")
            .unwrap()
            .captures(raw)
            .map(|c| c[1].to_string())
            .unwrap_or_default()
    };
    if article.is_empty() {
        return Err("链接中没有识别到 Ozon Артикул".into());
    }
    let url = if direct.is_match(raw) || labelled.is_match(raw) {
        format!("https://www.ozon.ru/product/{article}/")
    } else {
        raw.to_string()
    };
    Ok((url, article))
}

fn ensure_listing_jobs(c: &rusqlite::Connection) -> Result<(), String> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS listing_jobs(id INTEGER PRIMARY KEY AUTOINCREMENT,source_url TEXT NOT NULL,article TEXT NOT NULL DEFAULT '',offer_id TEXT NOT NULL DEFAULT '',title TEXT NOT NULL DEFAULT '',category_id TEXT NOT NULL DEFAULT '',category_display TEXT NOT NULL DEFAULT '',status TEXT NOT NULL DEFAULT 'draft',stage INTEGER NOT NULL DEFAULT 0,error TEXT NOT NULL DEFAULT '',payload TEXT NOT NULL DEFAULT '{}',created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);CREATE INDEX IF NOT EXISTS idx_listing_jobs_status ON listing_jobs(status,updated_at);CREATE TABLE IF NOT EXISTS listing_catalog_cache(cache_key TEXT PRIMARY KEY,payload TEXT NOT NULL,updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);").map_err(|e|e.to_string())
}

fn flatten_categories(
    value: &Value,
    path: &[String],
    inherited: i64,
    target: &mut Vec<ListingCategory>,
) {
    if let Some(items) = value.as_array() {
        for item in items {
            flatten_categories(item, path, inherited, target)
        }
        return;
    }
    let Some(o) = value.as_object() else { return };
    if o.get("disabled")
        .or_else(|| o.get("is_disabled"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return;
    }
    let category = o
        .get("description_category_id")
        .or_else(|| o.get("category_id"))
        .and_then(Value::as_i64)
        .unwrap_or(inherited);
    let category_name = o
        .get("category_name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let mut current = path.to_vec();
    if !category_name.is_empty() {
        current.push(category_name.into())
    }
    let type_id = o.get("type_id").and_then(Value::as_i64).unwrap_or(0);
    let type_name = o
        .get("type_name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if category > 0 && type_id > 0 {
        let mut p = current.clone();
        if !type_name.is_empty() && type_name != category_name {
            p.push(type_name.into())
        }
        target.push(ListingCategory {
            description_category_id: category,
            type_id,
            name: if type_name.is_empty() {
                category_name.into()
            } else {
                type_name.into()
            },
            display: p.join(" / "),
            score: 0.0,
        })
    }
    if let Some(types) = o
        .get("type")
        .or_else(|| o.get("types"))
        .and_then(Value::as_array)
    {
        for t in types {
            if t.get("disabled")
                .or_else(|| t.get("is_disabled"))
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                continue;
            }
            let id = t
                .get("id")
                .or_else(|| t.get("type_id"))
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let name = t
                .get("name")
                .or_else(|| t.get("type_name"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if category > 0 && id > 0 {
                let mut p = current.clone();
                if !name.is_empty() {
                    p.push(name.into())
                }
                target.push(ListingCategory {
                    description_category_id: category,
                    type_id: id,
                    name: name.into(),
                    display: p.join(" / "),
                    score: 0.0,
                })
            }
        }
    }
    if let Some(children) = o.get("children").and_then(Value::as_array) {
        for child in children {
            flatten_categories(child, &current, category, target)
        }
    }
}

fn category_words(text: &str) -> std::collections::BTreeSet<String> {
    let ignored = ["товар", "товары", "новый", "новая", "новое", "для", "with"];
    regex::Regex::new(r"(?i)[a-zа-яё0-9]{4,}")
        .unwrap()
        .find_iter(text)
        .map(|m| m.as_str().to_lowercase())
        .filter(|w| !ignored.contains(&w.as_str()))
        .collect()
}
fn stem(word: &str) -> String {
    let value = word.replace('ё', "е");
    for suffix in [
        "иями", "ями", "ами", "ого", "ему", "ому", "ыми", "ими", "ов", "ев", "ей", "ой", "ый",
        "ий", "ая", "яя", "ое", "ее", "ы", "и", "а", "я",
    ] {
        if value.ends_with(suffix) && value.len().saturating_sub(suffix.len()) >= 5 {
            return value[..value.len() - suffix.len()].into();
        }
    }
    value
}
fn similarity(a: &str, b: &str) -> f64 {
    let a = a.chars().collect::<Vec<_>>();
    let b = b.chars().collect::<Vec<_>>();
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let mut prev = vec![0usize; b.len() + 1];
    for ca in &a {
        let mut next = vec![0usize; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            next[j + 1] = if ca == cb {
                prev[j] + 1
            } else {
                next[j].max(prev[j + 1])
            }
        }
        prev = next
    }
    2.0 * prev[b.len()] as f64 / (a.len() + b.len()) as f64
}
fn search_categories_blocking(
    query: String,
    limit: usize,
    state: &AppState,
) -> Result<Vec<ListingCategory>, String> {
    let c = db(state)?;
    ensure_listing_jobs(&c)?;
    let raw: String = c
        .query_row(
            "SELECT payload FROM listing_catalog_cache WHERE cache_key='categories:ZH_HANS'",
            [],
            |r| r.get(0),
        )
        .map_err(|_| "尚无类目缓存，请先点击“刷新 Ozon 类目”".to_string())?;
    let mut items: Vec<ListingCategory> = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let words = category_words(&query);
    let stems = words
        .iter()
        .map(|w| stem(w))
        .collect::<std::collections::BTreeSet<_>>();
    for item in &mut items {
        let cw = category_words(&item.display);
        let cs = cw
            .iter()
            .map(|w| stem(w))
            .collect::<std::collections::BTreeSet<_>>();
        item.score = words
            .intersection(&cw)
            .map(|w| (w.len() * w.len()) as f64)
            .sum::<f64>()
            + stems
                .intersection(&cs)
                .map(|w| (w.len() * w.len()) as f64)
                .sum::<f64>();
        for source in &words {
            for category in &cw {
                let ratio = similarity(&stem(source), &stem(category));
                if ratio >= 0.72 {
                    item.score += ratio * (source.len().min(category.len())) as f64
                }
            }
        }
        if item.display.to_lowercase().contains(&query.to_lowercase()) {
            item.score += 1000.0
        }
    }
    items.retain(|v| query.trim().is_empty() || v.score > 0.0);
    items.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.display.cmp(&b.display))
    });
    items.truncate(limit.clamp(1, 100));
    Ok(items)
}
fn refresh_categories_blocking(state: &AppState) -> Result<i64, String> {
    let c = db(state)?;
    ensure_listing_jobs(&c)?;
    let payload = seller_post(
        &c,
        "/v1/description-category/tree",
        &json!({"language":"ZH_HANS"}),
    )?;
    let root = payload
        .get("result")
        .or_else(|| payload.get("items"))
        .unwrap_or(&payload);
    let mut rows = Vec::new();
    flatten_categories(root, &[], 0, &mut rows);
    let mut unique = std::collections::BTreeMap::new();
    for row in rows {
        unique.insert((row.description_category_id, row.type_id), row);
    }
    let rows = unique.into_values().collect::<Vec<_>>();
    c.execute("INSERT INTO listing_catalog_cache(cache_key,payload,updated_at)VALUES('categories:ZH_HANS',?1,CURRENT_TIMESTAMP)ON CONFLICT(cache_key)DO UPDATE SET payload=excluded.payload,updated_at=CURRENT_TIMESTAMP",[serde_json::to_string(&rows).map_err(|e|e.to_string())?]).map_err(|e|e.to_string())?;
    Ok(rows.len() as i64)
}

#[tauri::command]
pub async fn refresh_listing_categories(state: State<'_, AppState>) -> Result<i64, String> {
    let owned = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || refresh_categories_blocking(&owned))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn search_listing_categories(
    query: String,
    limit: usize,
    state: State<'_, AppState>,
) -> Result<Vec<ListingCategory>, String> {
    let owned = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || search_categories_blocking(query, limit, &owned))
        .await
        .map_err(|e| e.to_string())?
}

fn generated_offer_id(now: chrono::DateTime<chrono::Local>) -> String {
    format!(
        "AUTO-{}-{:06X}",
        now.format("%Y%m%d"),
        now.timestamp_subsec_nanos() & 0xFF_FFFF
    )
}

#[tauri::command]
pub fn create_listing_draft(
    reference: String,
    listing_mode: String,
    state: State<AppState>,
) -> Result<i64, String> {
    let mode = listing_mode.trim().to_ascii_lowercase();
    if !["follow", "local"].contains(&mode.as_str()) {
        return Err("上品模式必须是跟卖或本地新品".into());
    }
    let (url, article) = if mode == "follow" {
        normalize_reference_input(&reference)?
    } else {
        (String::new(), String::new())
    };
    let offer_id = generated_offer_id(chrono::Local::now());
    let c = db(&state)?;
    ensure_listing_jobs(&c)?;
    let payload = json!({"listing_mode":mode,"source_url":url,"article":article,"offer_id":offer_id,"currency_code":"CNY","images":[],"attributes":[],"complex_attributes":[]});
    c.execute("INSERT INTO listing_jobs(source_url,article,offer_id,status,stage,payload)VALUES(?1,?2,?3,'draft',0,?4)",params![url,article,offer_id,payload.to_string()]).map_err(|e|e.to_string())?;
    Ok(c.last_insert_rowid())
}

#[tauri::command]
pub fn listing_jobs(state: State<AppState>) -> Result<Vec<ListingJobRow>, String> {
    let c = db(&state)?;
    ensure_listing_jobs(&c)?;
    let mut s=c.prepare("SELECT id,source_url,article,offer_id,title,category_id,category_display,status,stage,error,payload,updated_at FROM listing_jobs ORDER BY id DESC LIMIT 500").map_err(|e|e.to_string())?;
    let rows = s
        .query_map([], |r| {
            let raw: String = r.get(10)?;
            Ok(ListingJobRow {
                id: r.get(0)?,
                source_url: r.get(1)?,
                article: r.get(2)?,
                offer_id: r.get(3)?,
                title: r.get(4)?,
                category_id: r.get(5)?,
                category_display: r.get(6)?,
                status: r.get(7)?,
                stage: r.get(8)?,
                error: r.get(9)?,
                payload: serde_json::from_str(&raw).unwrap_or_else(|_| json!({})),
                updated_at: r.get(11)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

fn object_array(value: &Value, label: &str) -> Result<(), String> {
    let items = value
        .as_array()
        .ok_or_else(|| format!("{label} 必须是 JSON 数组"))?;
    if items.iter().any(|v| !v.is_object()) {
        return Err(format!("{label} 的每一项必须是 JSON 对象"));
    }
    Ok(())
}

fn value_i64(value: Option<&Value>) -> i64 {
    value
        .and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()))
        .unwrap_or(0)
}

fn value_bool(value: Option<&Value>) -> bool {
    value
        .and_then(|v| v.as_bool().or_else(|| v.as_i64().map(|n| n != 0)))
        .unwrap_or(false)
}

fn attribute_definitions_blocking(
    category_id: i64,
    type_id: i64,
    state: &AppState,
) -> Result<Vec<ListingAttributeDefinition>, String> {
    if category_id <= 0 || type_id <= 0 {
        return Err("请先选择有效的 Ozon 类目和 type".into());
    }
    let c = db(state)?;
    let response = seller_post(
        &c,
        "/v1/description-category/attribute",
        &json!({"description_category_id":category_id,"type_id":type_id,"language":"ZH_HANS"}),
    )?;
    let items = response
        .get("result")
        .or_else(|| response.get("items"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(items
        .into_iter()
        .filter_map(|item| {
            let id = value_i64(item.get("id"));
            if id <= 0 {
                return None;
            }
            Some(ListingAttributeDefinition {
                id,
                name: item
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                description: item
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                attribute_complex_id: value_i64(item.get("attribute_complex_id")),
                is_required: value_bool(item.get("is_required")),
                is_collection: value_bool(item.get("is_collection")),
                dictionary_id: value_i64(item.get("dictionary_id")),
                max_value_count: value_i64(item.get("max_value_count")),
                group_name: item
                    .get("group_name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            })
        })
        .collect())
}

#[tauri::command]
pub async fn listing_attribute_definitions(
    category_id: i64,
    type_id: i64,
    state: State<'_, AppState>,
) -> Result<Vec<ListingAttributeDefinition>, String> {
    let owned = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || {
        attribute_definitions_blocking(category_id, type_id, &owned)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn listing_dictionary_values(
    category_id: i64,
    type_id: i64,
    attribute_id: i64,
    query: String,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    if category_id <= 0 || type_id <= 0 || attribute_id <= 0 {
        return Err("类目、type 和属性 ID 必须有效".into());
    }
    let owned = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || {
        let c = db(&owned)?;
        let query = query.trim();
        let (path, body) = if query.is_empty() {
            ("/v1/description-category/attribute/values", json!({"description_category_id":category_id,"type_id":type_id,"attribute_id":attribute_id,"last_value_id":0,"limit":2000,"language":"ZH_HANS"}))
        } else {
            ("/v1/description-category/attribute/values/search", json!({"description_category_id":category_id,"type_id":type_id,"attribute_id":attribute_id,"value":query,"limit":100,"language":"ZH_HANS"}))
        };
        let response = seller_post(&c, path, &body)?;
        Ok(response.get("result").cloned().unwrap_or_else(|| json!([])))
    })
    .await
    .map_err(|e| e.to_string())?
}

fn normalized_attribute_name(value: &str) -> String {
    let without_parentheses = regex::Regex::new(r"\([^)]*\)")
        .unwrap()
        .replace_all(&value.to_lowercase().replace('ё', "е"), " ")
        .to_string();
    regex::Regex::new(r"[^a-zа-я0-9\u{3400}-\u{9fff}]+")
        .unwrap()
        .replace_all(&without_parentheses, " ")
        .split_whitespace()
        .filter(|word| !["товар", "товара", "товары", "изделие", "продукт"].contains(word))
        .collect::<Vec<_>>()
        .join(" ")
}

fn reference_facts(payload: &Value) -> HashMap<String, String> {
    let mut facts = HashMap::new();
    let Some(properties) = payload.get("properties").and_then(Value::as_object) else {
        return facts;
    };
    for (name, raw) in properties {
        if name == "页面商品参数" {
            continue;
        }
        let value = raw.as_str().unwrap_or("").trim();
        let key = normalized_attribute_name(name);
        if !key.is_empty() && !value.is_empty() {
            facts.entry(key).or_insert_with(|| value.to_string());
        }
    }
    facts
}

#[tauri::command]
pub async fn map_listing_reference_attributes(
    id: i64,
    state: State<'_, AppState>,
) -> Result<i64, String> {
    let owned = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || {
        let c = db(&owned)?;
        ensure_listing_jobs(&c)?;
        let raw: String = c.query_row("SELECT payload FROM listing_jobs WHERE id=?1", [id], |r| r.get(0)).map_err(|_| "上品草稿不存在".to_string())?;
        let mut payload: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        let category_id = payload.get("category_id").and_then(Value::as_str).and_then(|v| v.parse().ok()).unwrap_or(0);
        let type_id = payload.get("type_id").and_then(Value::as_str).and_then(|v| v.parse().ok()).unwrap_or(0);
        let definitions = attribute_definitions_blocking(category_id, type_id, &owned)?;
        let facts = reference_facts(&payload);
        let mut normal = Vec::new();
        let mut grouped: std::collections::BTreeMap<i64, Vec<Value>> = std::collections::BTreeMap::new();
        for definition in definitions {
            if definition.dictionary_id > 0 {
                continue;
            }
            let key = normalized_attribute_name(&definition.name);
            let Some(value) = facts.get(&key) else { continue };
            let item = json!({"complex_id":definition.attribute_complex_id,"id":definition.id,"values":[{"value":value}],"_name":definition.name,"_source":"reference_exact"});
            if definition.attribute_complex_id > 0 {
                grouped.entry(definition.attribute_complex_id).or_default().push(item);
            } else {
                normal.push(item);
            }
        }
        let complex = grouped.into_iter().map(|(id, attributes)| json!({"attributes":attributes,"_complex_id":id})).collect::<Vec<_>>();
        let count = normal.len() + complex.iter().filter_map(|v| v.get("attributes")?.as_array()).map(Vec::len).sum::<usize>();
        payload["attributes"] = Value::Array(normal);
        payload["complex_attributes"] = Value::Array(complex);
        payload["attribute_mapping"] = json!({"mode":"reference_exact","mapped":count,"note":"仅标准化后同名自由文本属性；字典属性需人工选择"});
        let stage = if count > 0 { 3 } else { 2 };
        c.execute("UPDATE listing_jobs SET status=?1,stage=?2,error='',payload=?3,updated_at=CURRENT_TIMESTAMP WHERE id=?4", params![if stage == 3 {"ready"} else {"draft"},stage,payload.to_string(),id]).map_err(|e|e.to_string())?;
        Ok(count as i64)
    }).await.map_err(|e|e.to_string())?
}

fn attribute_has_value(item: &Value) -> bool {
    item.get("values")
        .and_then(Value::as_array)
        .map(|values| {
            values.iter().any(|v| {
                value_i64(v.get("dictionary_value_id")) > 0
                    || v.get("value")
                        .and_then(Value::as_str)
                        .map(|s| !s.trim().is_empty())
                        .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn attribute_value(input: &ListingAttributeValueInput) -> Result<Value, String> {
    let value = input.value.trim();
    if input.dictionary_value_id > 0 {
        if value.is_empty() {
            return Err("字典值缺少显示名称，请重新选择".into());
        }
        Ok(json!({"dictionary_value_id":input.dictionary_value_id,"value":value}))
    } else {
        if value.is_empty() {
            return Err("属性值不能为空".into());
        }
        if regex::Regex::new(r"[\u{3400}-\u{9fff}]")
            .unwrap()
            .is_match(value)
        {
            return Err("Ozon 自由文本属性不能直接提交中文，请填写俄文或选择官方字典值".into());
        }
        Ok(json!({"value":value}))
    }
}

fn upsert_attribute(items: &mut Vec<Value>, input: &ListingAttributeValueInput, value: Value) {
    let position = items
        .iter()
        .position(|item| value_i64(item.get("id")) == input.attribute_id);
    if let Some(index) = position {
        if !items[index]
            .get("values")
            .map(Value::is_array)
            .unwrap_or(false)
        {
            items[index]["values"] = json!([]);
        }
        if input.is_collection {
            let values = items[index]
                .get_mut("values")
                .and_then(Value::as_array_mut)
                .unwrap();
            let duplicate = values.iter().any(|existing| {
                let old_id = value_i64(existing.get("dictionary_value_id"));
                let new_id = value_i64(value.get("dictionary_value_id"));
                (new_id > 0 && old_id == new_id)
                    || (new_id == 0 && existing.get("value") == value.get("value"))
            });
            if !duplicate {
                values.push(value);
            }
        } else {
            items[index]["values"] = Value::Array(vec![value]);
        }
        items[index]["complex_id"] = json!(input.attribute_complex_id);
        items[index]["_name"] = json!(input.attribute_name.trim());
        items[index]["_source"] = json!(if input.dictionary_value_id > 0 {
            "dictionary_manual"
        } else {
            "manual"
        });
    } else {
        items.push(json!({
            "complex_id": input.attribute_complex_id,
            "id": input.attribute_id,
            "values": [value],
            "_name": input.attribute_name.trim(),
            "_source": if input.dictionary_value_id > 0 { "dictionary_manual" } else { "manual" }
        }));
    }
}

fn set_attribute_in_payload(
    payload: &mut Value,
    input: &ListingAttributeValueInput,
) -> Result<(), String> {
    if input.attribute_id <= 0 {
        return Err("属性 ID 必须有效".into());
    }
    let value = attribute_value(input)?;
    if input.attribute_complex_id > 0 {
        let groups = payload
            .get_mut("complex_attributes")
            .and_then(Value::as_array_mut)
            .ok_or("组合属性数据格式错误")?;
        let group_index = groups.iter().position(|group| {
            value_i64(group.get("_complex_id")) == input.attribute_complex_id
                || group
                    .get("attributes")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items.iter().any(|item| {
                            value_i64(item.get("complex_id")) == input.attribute_complex_id
                        })
                    })
                    .unwrap_or(false)
        });
        let index = match group_index {
            Some(index) => index,
            None => {
                groups.push(json!({"attributes":[],"_complex_id":input.attribute_complex_id}));
                groups.len() - 1
            }
        };
        let items = groups[index]
            .get_mut("attributes")
            .and_then(Value::as_array_mut)
            .ok_or("组合属性分组格式错误")?;
        upsert_attribute(items, input, value);
    } else {
        let items = payload
            .get_mut("attributes")
            .and_then(Value::as_array_mut)
            .ok_or("普通属性数据格式错误")?;
        upsert_attribute(items, input, value);
    }
    Ok(())
}

fn clear_attribute_in_payload(payload: &mut Value, attribute_id: i64) -> bool {
    let mut removed = false;
    if let Some(items) = payload.get_mut("attributes").and_then(Value::as_array_mut) {
        let before = items.len();
        items.retain(|item| value_i64(item.get("id")) != attribute_id);
        removed |= before != items.len();
    }
    if let Some(groups) = payload
        .get_mut("complex_attributes")
        .and_then(Value::as_array_mut)
    {
        for group in groups.iter_mut() {
            if let Some(items) = group.get_mut("attributes").and_then(Value::as_array_mut) {
                let before = items.len();
                items.retain(|item| value_i64(item.get("id")) != attribute_id);
                removed |= before != items.len();
            }
        }
        groups.retain(|group| {
            group
                .get("attributes")
                .and_then(Value::as_array)
                .map(|items| !items.is_empty())
                .unwrap_or(false)
        });
    }
    removed
}

fn mutate_listing_attribute(
    id: i64,
    state: &AppState,
    mutation: impl FnOnce(&mut Value) -> Result<(), String>,
) -> Result<Value, String> {
    let c = db(state)?;
    ensure_listing_jobs(&c)?;
    let raw: String = c
        .query_row("SELECT payload FROM listing_jobs WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .map_err(|_| "上品草稿不存在".to_string())?;
    let mut payload: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    if !payload
        .get("attributes")
        .map(Value::is_array)
        .unwrap_or(false)
    {
        payload["attributes"] = json!([]);
    }
    if !payload
        .get("complex_attributes")
        .map(Value::is_array)
        .unwrap_or(false)
    {
        payload["complex_attributes"] = json!([]);
    }
    mutation(&mut payload)?;
    c.execute("UPDATE listing_jobs SET status='draft',stage=MAX(stage,2),error='',payload=?1,updated_at=CURRENT_TIMESTAMP WHERE id=?2", params![payload.to_string(),id]).map_err(|e|e.to_string())?;
    Ok(payload)
}

#[tauri::command]
pub async fn set_listing_attribute_value(
    form: ListingAttributeValueInput,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let owned = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || {
        let id = form.id;
        mutate_listing_attribute(id, &owned, |payload| {
            set_attribute_in_payload(payload, &form)
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn clear_listing_attribute_value(
    id: i64,
    attribute_id: i64,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let owned = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || {
        mutate_listing_attribute(id, &owned, |payload| {
            clear_attribute_in_payload(payload, attribute_id);
            Ok(())
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

fn ai_json_content(raw: &str) -> Result<Value, String> {
    let response: Value =
        serde_json::from_str(raw).map_err(|e| format!("AI 返回不是有效响应 JSON：{e}"))?;
    let content = response
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            format!(
                "AI 返回缺少正文：{}",
                raw.chars().take(300).collect::<String>()
            )
        })?;
    let trimmed = content
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    serde_json::from_str(trimmed).map_err(|e| format!("AI 正文不是严格 JSON：{e}"))
}

fn dictionary_rows(value: &Value) -> Vec<Value> {
    if let Some(rows) = value.as_array() {
        return rows.clone();
    }
    for key in ["result", "items", "values"] {
        if let Some(child) = value.get(key) {
            let rows = dictionary_rows(child);
            if !rows.is_empty() {
                return rows;
            }
        }
    }
    Vec::new()
}

fn ai_fill_required_attributes_blocking(
    id: i64,
    state: &AppState,
) -> Result<ListingAiFillResult, String> {
    let c = db(state)?;
    ensure_listing_jobs(&c)?;
    let raw: String = c
        .query_row("SELECT payload FROM listing_jobs WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .map_err(|_| "上品草稿不存在".to_string())?;
    let mut payload: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    if !payload
        .get("attributes")
        .map(Value::is_array)
        .unwrap_or(false)
    {
        payload["attributes"] = json!([]);
    }
    if !payload
        .get("complex_attributes")
        .map(Value::is_array)
        .unwrap_or(false)
    {
        payload["complex_attributes"] = json!([]);
    }
    let category_id = payload
        .get("category_id")
        .and_then(Value::as_str)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let type_id = payload
        .get("type_id")
        .and_then(Value::as_str)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let definitions = attribute_definitions_blocking(category_id, type_id, state)?;
    let mut present = std::collections::HashSet::new();
    for item in payload
        .get("attributes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if attribute_has_value(item) {
            present.insert(value_i64(item.get("id")));
        }
    }
    for group in payload
        .get("complex_attributes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        for item in group
            .get("attributes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if attribute_has_value(item) {
                present.insert(value_i64(item.get("id")));
            }
        }
    }
    let missing = definitions
        .iter()
        .filter(|d| d.is_required && !present.contains(&d.id))
        .cloned()
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(ListingAiFillResult {
            filled: 0,
            free_text_filled: 0,
            dictionary_filled: 0,
            missing_required: vec![],
        });
    }
    let base = setting(&c, "ai_base_url");
    let model = setting(&c, "ai_model");
    let key = secret_setting(&c, "ai_api_key")?;
    if base.is_empty() || model.is_empty() || key.is_empty() {
        return Err("请先在连接设置中配置 AI Base URL、模型和 API Key".into());
    }
    let mut allowed = Vec::new();
    for definition in &missing {
        let mut item = json!({"id":definition.id,"name":definition.name,"description":definition.description,"dictionary":definition.dictionary_id>0,"collection":definition.is_collection,"max_value_count":definition.max_value_count});
        if definition.dictionary_id > 0 {
            let response = seller_post(
                &c,
                "/v1/description-category/attribute/values",
                &json!({"description_category_id":category_id,"type_id":type_id,"attribute_id":definition.id,"last_value_id":0,"limit":2000,"language":"ZH_HANS"}),
            )?;
            let options = dictionary_rows(&response)
                .into_iter()
                .filter_map(|row| {
                    let option_id = value_i64(
                        row.get("id")
                            .or_else(|| row.get("value_id"))
                            .or_else(|| row.get("dictionary_value_id")),
                    );
                    let value = row
                        .get("value")
                        .or_else(|| row.get("name"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim();
                    (option_id > 0 && !value.is_empty())
                        .then(|| json!({"id":option_id,"value":value}))
                })
                .collect::<Vec<_>>();
            item["options"] = Value::Array(options);
        }
        allowed.push(item);
    }
    let facts = json!({
        "listing_mode": payload.get("listing_mode").cloned().unwrap_or_else(|| json!("follow")),
        "title": payload.get("title").cloned().unwrap_or(Value::Null),
        "description": payload.get("description").cloned().unwrap_or(Value::Null),
        "source_properties": payload.get("properties").cloned().unwrap_or_else(|| json!({})),
        "weight_g": payload.get("weight").cloned().unwrap_or(Value::Null),
        "dimensions_mm": {"depth":payload.get("depth"),"width":payload.get("width"),"height":payload.get("height")}
    });
    let user_prompt = format!(
        "Fill only defensible missing required attributes for the already selected Ozon category. Return strict JSON {{\"attributes\":[{{\"id\":integer,\"value\":string,\"dictionary_value_id\":integer_or_0,\"evidence\":string}}]}}. Use only supplied attribute IDs. For dictionary attributes choose only an ID and exact value from that attribute's options; if no equivalent option exists, omit it. For free-text attributes return Russian only. Never invent brand, model, dimensions, weight, material/composition, country/manufacturer, certification, warranty, package quantity/contents, capacity, or compatibility. Omit anything not stated or strongly entailed. Do not use the source seller article as our offer/model ID. Product facts:\n{}\nAllowed missing required attributes and live dictionary options:\n{}",
        facts, Value::Array(allowed.clone())
    );
    let body = json!({"model":model,"temperature":0,"messages":[{"role":"system","content":"You map factual product data to supplied live Ozon attributes. You never invent IDs or unsupported product facts and return strict JSON only."},{"role":"user","content":user_prompt}]});
    let response = ureq::post(&chat_endpoint(&base))
        .set("Authorization", &format!("Bearer {key}"))
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| format!("AI 属性填写失败：{e}"))?;
    let response_raw = response.into_string().map_err(|e| e.to_string())?;
    let answer = ai_json_content(&response_raw)?;
    let requested = answer
        .get("attributes")
        .and_then(Value::as_array)
        .ok_or("AI 属性结果缺少 attributes 数组")?;
    let definitions_by_id = missing.iter().map(|d| (d.id, d)).collect::<HashMap<_, _>>();
    let allowed_options = allowed
        .iter()
        .map(|item| {
            let attr_id = value_i64(item.get("id"));
            let options = item
                .get("options")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|option| {
                    (
                        value_i64(option.get("id")),
                        option
                            .get("value")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                    )
                })
                .collect::<HashMap<_, _>>();
            (attr_id, options)
        })
        .collect::<HashMap<_, _>>();
    let mut free_text_filled = 0;
    let mut dictionary_filled = 0;
    for item in requested {
        let attribute_id = value_i64(item.get("id"));
        let Some(definition) = definitions_by_id.get(&attribute_id) else {
            continue;
        };
        let mut value = item
            .get("value")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let dictionary_value_id = value_i64(item.get("dictionary_value_id"));
        if definition.dictionary_id > 0 {
            let Some(official) = allowed_options
                .get(&attribute_id)
                .and_then(|options| options.get(&dictionary_value_id))
            else {
                continue;
            };
            if official != &value {
                value = official.clone();
            }
            dictionary_filled += 1;
        } else {
            if dictionary_value_id > 0 {
                continue;
            }
            free_text_filled += 1;
        }
        let input = ListingAttributeValueInput {
            id,
            attribute_id,
            attribute_complex_id: definition.attribute_complex_id,
            attribute_name: definition.name.clone(),
            is_collection: definition.is_collection,
            dictionary_value_id,
            value,
        };
        if set_attribute_in_payload(&mut payload, &input).is_err() {
            if definition.dictionary_id > 0 {
                dictionary_filled -= 1;
            } else {
                free_text_filled -= 1;
            }
        }
    }
    let mut now_present = std::collections::HashSet::new();
    for item in payload
        .get("attributes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if attribute_has_value(item) {
            now_present.insert(value_i64(item.get("id")));
        }
    }
    for group in payload
        .get("complex_attributes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        for item in group
            .get("attributes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if attribute_has_value(item) {
                now_present.insert(value_i64(item.get("id")));
            }
        }
    }
    let missing_required = definitions
        .iter()
        .filter(|d| d.is_required && !now_present.contains(&d.id))
        .map(|d| d.name.clone())
        .collect::<Vec<_>>();
    payload["ai_attribute_fill"] = json!({"mode":"required_only_evidence_bound","filled":free_text_filled+dictionary_filled,"missing_required":missing_required.clone(),"prompt_version":"legacy-rfbs-v1"});
    c.execute("UPDATE listing_jobs SET status='draft',stage=MAX(stage,2),error='',payload=?1,updated_at=CURRENT_TIMESTAMP WHERE id=?2", params![payload.to_string(),id]).map_err(|e|e.to_string())?;
    Ok(ListingAiFillResult {
        filled: free_text_filled + dictionary_filled,
        free_text_filled,
        dictionary_filled,
        missing_required,
    })
}

#[tauri::command]
pub async fn ai_fill_listing_required_attributes(
    id: i64,
    state: State<'_, AppState>,
) -> Result<ListingAiFillResult, String> {
    let owned = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || ai_fill_required_attributes_blocking(id, &owned))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn validate_listing_job(
    id: i64,
    state: State<'_, AppState>,
) -> Result<ListingValidation, String> {
    let owned = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || {
        let c = db(&owned)?;
        let raw: String = c
            .query_row("SELECT payload FROM listing_jobs WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .map_err(|_| "上品草稿不存在".to_string())?;
        let payload: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        let mut issues = Vec::new();
        for (key, label) in [
            ("offer_id", "货号 offer_id"),
            ("title", "俄文标题"),
            ("price", "售价"),
        ] {
            if payload
                .get(key)
                .and_then(Value::as_str)
                .map(|s| s.trim().is_empty())
                .unwrap_or(true)
            {
                issues.push(format!("缺少{label}"));
            }
        }
        let images = payload
            .get("images")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if images.is_empty()
            || images
                .iter()
                .any(|v| !v.as_str().unwrap_or("").starts_with("https://"))
        {
            issues.push("至少需要一张可公开访问的 HTTPS 商品图片".into());
        }
        for key in ["weight", "depth", "width", "height"] {
            if payload.get(key).and_then(Value::as_f64).unwrap_or(0.0) <= 0.0 {
                issues.push(format!("{key} 必须大于 0"));
            }
        }
        let category_id = payload
            .get("category_id")
            .and_then(Value::as_str)
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let type_id = payload
            .get("type_id")
            .and_then(Value::as_str)
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let definitions = attribute_definitions_blocking(category_id, type_id, &owned)?;
        let mut present = std::collections::HashSet::new();
        for item in payload
            .get("attributes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if attribute_has_value(item) {
                present.insert(value_i64(item.get("id")));
            }
        }
        for group in payload
            .get("complex_attributes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            for item in group
                .get("attributes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if attribute_has_value(item) {
                    present.insert(value_i64(item.get("id")));
                }
            }
        }
        let missing_required = definitions
            .into_iter()
            .filter(|d| d.is_required && !present.contains(&d.id))
            .map(|d| d.name)
            .collect::<Vec<_>>();
        if !missing_required.is_empty() {
            issues.push(format!("缺少 {} 个 Ozon 必填属性", missing_required.len()));
        }
        Ok(ListingValidation {
            valid: issues.is_empty(),
            issues,
            missing_required,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

fn meta_content(page: &str, key: &str) -> String {
    let escaped = regex::escape(key);
    for pattern in [
        format!(
            r#"(?is)<meta[^>]+(?:property|name)=["']{}["'][^>]+content=["']([^"']*)["']"#,
            escaped
        ),
        format!(
            r#"(?is)<meta[^>]+content=["']([^"']*)["'][^>]+(?:property|name)=["']{}["']"#,
            escaped
        ),
    ] {
        if let Ok(re) = regex::Regex::new(&pattern) {
            if let Some(c) = re.captures(page) {
                return c
                    .get(1)
                    .map(|v| v.as_str().trim().to_string())
                    .unwrap_or_default();
            }
        }
    }
    String::new()
}

fn reference_product_from_html(page: &str) -> Result<Value, String> {
    let lower = page.to_lowercase();
    if [
        "<title>antibot challenge page",
        "<title>captcha",
        "verify you are human</h",
        "access denied</h",
        "проверка безопасности</h",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
    {
        return Err("blocked: Ozon 返回验证或访问限制页面".into());
    }
    let script_re = regex::Regex::new(
        r#"(?is)<script[^>]+type=["']application/ld\+json["'][^>]*>(.*?)</script>"#,
    )
    .unwrap();
    let mut product = None;
    for capture in script_re.captures_iter(page) {
        let Some(raw) = capture.get(1) else { continue };
        let Ok(root) = serde_json::from_str::<Value>(raw.as_str().trim()) else {
            continue;
        };
        let mut queue = match root {
            Value::Array(v) => v,
            v => vec![v],
        };
        while let Some(item) = queue.pop() {
            if item
                .get("@type")
                .and_then(Value::as_str)
                .map(|v| v.eq_ignore_ascii_case("product"))
                .unwrap_or(false)
            {
                product = Some(item);
                break;
            }
            if let Some(graph) = item.get("@graph").and_then(Value::as_array) {
                queue.extend(graph.iter().cloned())
            }
        }
        if product.is_some() {
            break;
        }
    }
    let product = product.unwrap_or_else(|| json!({}));
    let title = product
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let title = if title.is_empty() {
        meta_content(page, "og:title")
    } else {
        title
    };
    let title = if title.is_empty() {
        regex::Regex::new(r#"\\\"name\\\"\s*:\s*\\\"([^\\\"]+)"#)
            .ok()
            .and_then(|re| re.captures(page))
            .and_then(|capture| capture.get(1))
            .map(|value| value.as_str().replace(r#"\""#, "\"").trim().to_string())
            .unwrap_or_default()
    } else {
        title
    };
    if title.is_empty() {
        return Err("商品页面没有返回可识别标题，可能触发了 Ozon 验证或地区跳转".into());
    }
    let description = product
        .get("description")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| meta_content(page, "og:description"));
    let mut images = Vec::<String>::new();
    match product.get("image") {
        Some(Value::Array(values)) => {
            images.extend(values.iter().filter_map(Value::as_str).map(str::to_string))
        }
        Some(Value::String(v)) => images.push(v.clone()),
        _ => {
            let v = meta_content(page, "og:image");
            if !v.is_empty() {
                images.push(v)
            }
        }
    }
    images.retain(|v| v.starts_with("http"));
    let mut seen = std::collections::HashSet::new();
    images.retain(|value| seen.insert(value.clone()));
    let mut properties = serde_json::Map::new();
    let raw = product
        .get("additionalProperty")
        .map(|v| match v {
            Value::Array(a) => a.clone(),
            v => vec![v.clone()],
        })
        .unwrap_or_default();
    for item in raw {
        let name = item
            .get("name")
            .or_else(|| item.get("propertyID"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let value = item
            .get("value")
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| v.to_string())
            })
            .unwrap_or_default();
        if !name.is_empty() && !value.is_empty() {
            properties.insert(name.into(), Value::String(value));
        }
    }
    Ok(json!({"title":title,"description":description,"images":images,"properties":properties}))
}

#[cfg(any())]
fn installed_browser() -> Result<std::path::PathBuf, String> {
    for path in [
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
    ] {
        let p = std::path::PathBuf::from(path);
        if p.is_file() {
            return Ok(p);
        }
    }
    Err("未找到 Edge 或 Chrome 可执行文件".into())
}

#[cfg(any())]
fn collect_reference_browser_legacy(
    url: &str,
    article: &str,
    state: &AppState,
) -> Result<Value, String> {
    use headless_chrome::{Browser, LaunchOptions};
    let profile = state.data_dir.join("listing_browser_profile");
    fs::create_dir_all(&profile).map_err(|e| e.to_string())?;
    let options = LaunchOptions::default_builder()
        .headless(false)
        .path(Some(installed_browser()?))
        .user_data_dir(Some(profile))
        .window_size(Some((1360, 900)))
        .idle_browser_timeout(std::time::Duration::from_secs(240))
        .build()
        .map_err(|e| format!("浏览器启动参数错误：{e}"))?;
    let browser = Browser::new(options).map_err(|e| format!("无法启动专用 Edge/Chrome：{e}"))?;
    let tab = browser
        .new_tab()
        .map_err(|e| format!("无法创建浏览器页面：{e}"))?;
    let _ = tab.navigate_to(url);
    let script = r#"JSON.stringify((()=>{let product={};for(const s of document.querySelectorAll('script[type="application/ld+json"]')){try{const raw=JSON.parse(s.textContent||'{}'),q=Array.isArray(raw)?raw:[raw];for(const x of q){if(x&&String(x['@type']||'').toLowerCase()==='product')product=x;if(x&&Array.isArray(x['@graph']))q.push(...x['@graph']);}}catch(_){}}const meta=k=>document.querySelector(`meta[property="${k}"],meta[name="${k}"]`)?.content||'';const heading=document.querySelector('h1')?.innerText?.trim()||'';const images=[];for(const img of document.querySelectorAll('[data-widget*="Gallery"] img,[data-widget*="gallery"] img')){const set=String(img.getAttribute('srcset')||'').split(',').map(x=>x.trim().split(/\s+/)[0]).filter(Boolean);let v=set.at(-1)||img.currentSrc||img.src||img.getAttribute('data-src')||'';if(v.startsWith('//'))v='https:'+v;if(/^https:\/\//.test(v)&&/ozone\.ru/i.test(v)&&!/captcha|challenge/i.test(v)){try{const u=new URL(v);u.pathname=u.pathname.replace(/\/wc\d+\//i,'/wc2000/');v=u.toString()}catch(_){}images.push(v)}}if(!images.length){for(const v of(Array.isArray(product.image)?product.image:[product.image||meta('og:image')]))if(String(v).startsWith('http'))images.push(String(v))}const unique=[...new Map(images.map(v=>{try{const u=new URL(v);return[u.pathname.replace(/\/wc\d+\//i,'/wcX/'),v]}catch(_){return[v,v]}})).values()].slice(0,20);const properties={};for(const x of(Array.isArray(product.additionalProperty)?product.additionalProperty:[product.additionalProperty||{}])){const k=String(x?.name||x?.propertyID||'').trim(),v=String(x?.value||'').trim();if(k&&v)properties[k]=v}const chars=document.querySelector('[data-widget="webCharacteristics"]')?.innerText?.trim()||'';if(chars)properties['页面商品参数']=chars;const description=document.querySelector('[data-widget="webDescription"]')?.innerText?.trim()||String(product.description||meta('og:description')||'').trim();return{title:String(product.name||meta('og:title')||heading||'').trim(),description,images:unique,properties,pageTitle:document.title||'',url:location.href}})())"#;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
    let mut best: Option<Value> = None;
    let mut last_count = 0usize;
    let mut stable = 0;
    while std::time::Instant::now() < deadline {
        if let Ok(remote) = tab.evaluate(script, false) {
            if let Some(raw) = remote.value.and_then(|v| v.as_str().map(str::to_string)) {
                if let Ok(value) = serde_json::from_str::<Value>(&raw) {
                    let title = value.get("title").and_then(Value::as_str).unwrap_or("");
                    let page_title = value
                        .get("pageTitle")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_lowercase();
                    let count = value
                        .get("images")
                        .and_then(Value::as_array)
                        .map(Vec::len)
                        .unwrap_or(0);
                    let challenge = ["captcha", "доступ ограничен", "похоже"]
                        .iter()
                        .any(|v| page_title.contains(v));
                    if !title.is_empty() && !challenge && count > 0 {
                        if count == last_count {
                            stable += 1
                        } else {
                            stable = 0;
                            last_count = count
                        }
                        best = Some(value);
                        if stable >= 2 {
                            break;
                        }
                        let _=tab.evaluate("document.querySelector('[data-widget=\"webDescription\"]')?.scrollIntoView({block:'center'})",false);
                    }
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    let value = best.ok_or_else(|| {
        "等待浏览器验证或商品图库超时；请在弹出的专用浏览器中完成验证并保持商品页打开".to_string()
    })?;
    let final_url = value
        .get("url")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| tab.get_url());
    if let Ok((_, found)) = normalize_reference_input(&final_url) {
        if !article.is_empty() && found != article {
            return Err(format!(
                "浏览器跳转到了 Артикул {found}，目标是 {article}；已停止采集"
            ));
        }
    }
    Ok(
        json!({"title":value["title"],"description":value["description"],"images":value["images"],"properties":value["properties"]}),
    )
}

fn collect_reference_browser(url: &str, article: &str, state: &AppState) -> Result<Value, String> {
    // Reuse the exact browser channel that powers competitor monitoring:
    // Chrome first, then Edge; normal sandbox; automation defaults removed;
    // target URL and final product identity both verified through CDP.
    let html = collect_competitor_browser_html(url, article, &state.data_dir, -1)?;
    let mut product = reference_product_from_html(&html)?;
    if let Some(object) = product.as_object_mut() {
        object.insert(
            "collector_source".into(),
            Value::String("competitor_browser_capability".into()),
        );
    }
    Ok(product)
}

fn collect_listing_reference_blocking(id: i64, state: &AppState) -> Result<i64, String> {
    let c = db(state)?;
    ensure_listing_jobs(&c)?;
    let (url, article, raw_payload): (String, String, String) = c
        .query_row(
            "SELECT source_url,article,payload FROM listing_jobs WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|_| "上品草稿不存在".to_string())?;
    c.execute("UPDATE listing_jobs SET status='collecting',error='',updated_at=CURRENT_TIMESTAMP WHERE id=?1",[id]).map_err(|e|e.to_string())?;
    let direct: Result<Value, String> = (|| {
        let response=ureq::get(&url).set("User-Agent","Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/126 Safari/537.36").set("Accept-Language","ru-RU,ru;q=0.9").timeout(std::time::Duration::from_secs(35)).call().map_err(|e|format!("Ozon 参考页读取失败：{e}"))?;
        let html = response
            .into_string()
            .map_err(|e| format!("参考页解码失败：{e}"))?;
        let value = reference_product_from_html(&html)?;
        if value
            .get("images")
            .and_then(Value::as_array)
            .map(Vec::is_empty)
            .unwrap_or(true)
        {
            return Err("直连页面没有完整商品图库".into());
        }
        Ok(value)
    })();
    let result = match direct {
        Ok(value) => Ok(value),
        Err(_) => {
            c.execute("UPDATE listing_jobs SET status='browser',error='直连受限，正在专用浏览器中等待验证和自动采集',updated_at=CURRENT_TIMESTAMP WHERE id=?1",[id]).map_err(|e|e.to_string())?;
            collect_reference_browser(&url, &article, state)
        }
    };
    match result {
        Ok(collected) => {
            let mut payload =
                serde_json::from_str::<Value>(&raw_payload).unwrap_or_else(|_| json!({}));
            if let (Some(target), Some(source)) = (payload.as_object_mut(), collected.as_object()) {
                for (k, v) in source {
                    target.insert(k.clone(), v.clone());
                }
            }
            let title = payload.get("title").and_then(Value::as_str).unwrap_or("");
            c.execute("UPDATE listing_jobs SET title=?1,status='draft',stage=MAX(stage,1),error='',payload=?2,updated_at=CURRENT_TIMESTAMP WHERE id=?3",params![title,payload.to_string(),id]).map_err(|e|e.to_string())?;
            Ok(1)
        }
        Err(error) => {
            c.execute("UPDATE listing_jobs SET status='failed',error=?1,updated_at=CURRENT_TIMESTAMP WHERE id=?2",params![error,id]).map_err(|e|e.to_string())?;
            Err(error)
        }
    }
}

#[tauri::command]
pub async fn collect_listing_reference(id: i64, state: State<'_, AppState>) -> Result<i64, String> {
    let owned = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || collect_listing_reference_blocking(id, &owned))
        .await
        .map_err(|e| format!("参考商品后台采集失败：{e}"))?
}

#[tauri::command]
pub fn save_listing_draft(form: ListingDraftInput, state: State<AppState>) -> Result<i64, String> {
    if form.id <= 0 {
        return Err("无效的草稿编号".into());
    }
    if form.weight < 0.0 || form.depth < 0.0 || form.width < 0.0 || form.height < 0.0 {
        return Err("重量和尺寸不能小于 0".into());
    }
    object_array(&form.attributes, "普通属性")?;
    object_array(&form.complex_attributes, "组合属性")?;
    let c = db(&state)?;
    ensure_listing_jobs(&c)?;
    let (source_url, article, previous_raw): (String, String, String) = c
        .query_row(
            "SELECT source_url,article,payload FROM listing_jobs WHERE id=?1",
            [form.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|_| "上品草稿不存在".to_string())?;
    let stage = if !form.category_id.trim().is_empty() && !form.type_id.trim().is_empty() {
        if !form.attributes.as_array().unwrap().is_empty()
            || !form.complex_attributes.as_array().unwrap().is_empty()
        {
            3
        } else {
            2
        }
    } else if !form.title.trim().is_empty() {
        1
    } else {
        0
    };
    let status = if stage >= 3 { "ready" } else { "draft" };
    let previous: Value = serde_json::from_str(&previous_raw).unwrap_or_else(|_| json!({}));
    let listing_mode = previous
        .get("listing_mode")
        .and_then(Value::as_str)
        .unwrap_or("follow");
    let payload = json!({"listing_mode":listing_mode,"source_url":source_url,"article":article,"offer_id":form.offer_id.trim(),"title":form.title.trim(),"category_id":form.category_id.trim(),"category_display":form.category_display.trim(),"type_id":form.type_id.trim(),"price":form.price.trim(),"currency_code":"CNY","weight":form.weight,"depth":form.depth,"width":form.width,"height":form.height,"description":form.description,"images":form.images,"attributes":form.attributes,"complex_attributes":form.complex_attributes});
    c.execute("UPDATE listing_jobs SET offer_id=?1,title=?2,category_id=?3,category_display=?4,status=?5,stage=?6,error='',payload=?7,updated_at=CURRENT_TIMESTAMP WHERE id=?8",params![form.offer_id.trim(),form.title.trim(),form.category_id.trim(),form.category_display.trim(),status,stage,payload.to_string(),form.id]).map_err(|e|e.to_string())?;
    Ok(stage)
}

#[tauri::command]
pub fn retry_listing_job(id: i64, state: State<AppState>) -> Result<(), String> {
    let c = db(&state)?;
    ensure_listing_jobs(&c)?;
    let changed=c.execute("UPDATE listing_jobs SET status='draft',error='',updated_at=CURRENT_TIMESTAMP WHERE id=?1",[id]).map_err(|e|e.to_string())?;
    if changed == 0 {
        return Err("上品任务不存在".into());
    }
    Ok(())
}

fn shipping_bands(weight: f64) -> Vec<(f64, f64, f64)> {
    let mut bands = Vec::new();
    let billed_weight = (weight * 10.0).ceil() / 10.0;
    let rounded = |value: f64| (value * 100.0).round() / 100.0;
    if billed_weight < 0.5 {
        bands.push((0.0, 135.0, rounded(3.37 + billed_weight * 28.10)));
    } else if billed_weight < 30.0 {
        bands.push((0.0, 135.0, rounded(24.71 + billed_weight * 28.10)));
    }
    if billed_weight < 2.0 {
        bands.push((135.0, 635.0, rounded(17.97 + billed_weight * 28.10)));
    } else if billed_weight < 30.0 {
        bands.push((135.0, 635.0, rounded(40.44 + billed_weight * 28.10)));
    }
    if billed_weight < 5.0 {
        bands.push((635.0, 22_525.0, rounded(25.83 + billed_weight * 19.10)));
    } else if billed_weight < 30.0 {
        bands.push((635.0, 22_525.0, rounded(64.00 + billed_weight * 25.80)));
    }
    bands
}

#[tauri::command]
pub fn calculate_listing_price(form: ListingPriceInput) -> Result<ListingPriceBreakdown, String> {
    if form.purchase_cost <= 0.0 || form.weight_kg <= 0.0 {
        return Err("采购成本和实际重量必须大于 0".into());
    }
    if form.label_fee < 0.0 || form.target_roi_percent < 0.0 || form.minimum_sale_price < 0.0 {
        return Err("贴单费、目标 ROI 和最低售价不能小于 0".into());
    }
    let commission_rate =
        (form.sales_commission_percent - form.sales_commission_discount_percent) / 100.0;
    let advertising_rate = form.advertising_percent / 100.0;
    let cargo_loss_rate = form.cargo_loss_percent / 100.0;
    if commission_rate <= 0.0 || commission_rate + 0.02 + advertising_rate >= 1.0 {
        return Err("减免后佣金必须大于 0，且佣金、物流佣金和广告费率合计必须小于 100%".into());
    }
    let invested = form.purchase_cost + form.label_fee;
    let target_roi = form.target_roi_percent / 100.0;
    let mut candidates = Vec::new();
    for (minimum, maximum, shipping) in shipping_bands(form.weight_kg) {
        let retained_rate = 1.0 - commission_rate - 0.02 - advertising_rate;
        let required = (target_roi * invested + (1.0 + cargo_loss_rate) * (invested + shipping))
            / retained_rate;
        let price = required
            .ceil()
            .max(1.0)
            .max(minimum)
            .max(form.minimum_sale_price);
        if price >= maximum {
            continue;
        }
        let sales_commission = price * commission_rate;
        let logistics_commission = price * 0.02;
        let advertising = price * advertising_rate;
        let cargo_loss = (invested + shipping) * cargo_loss_rate;
        let profit = price
            - sales_commission
            - logistics_commission
            - advertising
            - invested
            - shipping
            - cargo_loss;
        let roi_percent = profit / invested * 100.0;
        if roi_percent + f64::EPSILON >= form.target_roi_percent {
            candidates.push(ListingPriceBreakdown {
                price,
                shipping,
                sales_commission,
                logistics_commission,
                advertising,
                cargo_loss,
                invested,
                profit,
                roi_percent,
            });
        }
    }
    candidates
        .into_iter()
        .min_by(|a, b| a.price.total_cmp(&b.price))
        .ok_or_else(|| "当前成本、重量和目标 ROI 超出原上品工具运费公式支持的售价区间".into())
}

fn number(value: &str) -> Option<f64> {
    value.trim().replace(',', "").parse().ok()
}
fn ledger(path: &str) -> Result<Vec<HashMap<String, String>>, String> {
    if !Path::new(path).is_file() {
        return Err(format!("找不到产品台账：{path}"));
    }
    let mut book = open_workbook_auto(path).map_err(|e| format!("无法读取产品台账：{e}"))?;
    let sheet = book
        .sheet_names()
        .iter()
        .find(|x| x.as_str() == "产品台账")
        .cloned()
        .or_else(|| book.sheet_names().first().cloned())
        .ok_or("产品台账没有工作表")?;
    let range = book.worksheet_range(&sheet).map_err(|e| e.to_string())?;
    let mut iter = range.rows();
    let headers = iter
        .next()
        .ok_or("产品台账为空")?
        .iter()
        .map(|x| x.to_string().trim().to_string())
        .collect::<Vec<_>>();
    Ok(iter
        .filter_map(|row| {
            let item = headers
                .iter()
                .enumerate()
                .filter(|(_, h)| !h.is_empty())
                .map(|(i, h)| {
                    (
                        h.clone(),
                        row.get(i)
                            .map(|x| x.to_string())
                            .unwrap_or_default()
                            .trim()
                            .to_string(),
                    )
                })
                .collect::<HashMap<_, _>>();
            if item.values().any(|x| !x.is_empty()) {
                Some(item)
            } else {
                None
            }
        })
        .collect())
}

static LEDGER_CACHE: OnceLock<Mutex<HashMap<String, (u64, Vec<HashMap<String, String>>)>>> =
    OnceLock::new();

fn cached_ledger(path: &str) -> Result<Vec<HashMap<String, String>>, String> {
    let modified = fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_secs())
        .unwrap_or(0);
    let cache = LEDGER_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some((cached_modified, rows)) = cache
        .lock()
        .map_err(|_| "产品台账缓存锁异常")?
        .get(path)
        .cloned()
    {
        if cached_modified == modified {
            return Ok(rows);
        }
    }
    let rows = ledger(path)?;
    cache
        .lock()
        .map_err(|_| "产品台账缓存锁异常")?
        .insert(path.to_string(), (modified, rows.clone()));
    Ok(rows)
}

fn valid_supplier_url(value: &str) -> String {
    let value = value.trim();
    if regex::Regex::new(r"(?i)^https?://(?:[^/]+\.)?1688\.com/")
        .unwrap()
        .is_match(value)
    {
        value.to_string()
    } else {
        String::new()
    }
}

pub(crate) fn supplier_link_for_offer(c: &rusqlite::Connection, offer_id: &str) -> String {
    let path = setting(c, "listing_ledger_path");
    if path.is_empty() || offer_id.trim().is_empty() {
        return String::new();
    }
    cached_ledger(&path)
        .ok()
        .and_then(|rows| {
            rows.into_iter()
                .find(|row| field(row, "货号").eq_ignore_ascii_case(offer_id.trim()))
        })
        .map(|row| valid_supplier_url(field(&row, "1688采购链接")))
        .unwrap_or_default()
}

#[tauri::command]
pub fn open_listing_supplier_url(url: String) -> Result<(), String> {
    let url = valid_supplier_url(&url);
    if url.is_empty() {
        return Err("台账中没有有效的 1688 采购链接".into());
    }
    open::that(url).map_err(|e| format!("无法打开 1688 采购链接：{e}"))
}
fn field<'a>(row: &'a HashMap<String, String>, key: &str) -> &'a str {
    row.get(key).map(String::as_str).unwrap_or("")
}

#[tauri::command]
pub fn listing_settings(state: State<AppState>) -> Result<ListingSettings, String> {
    let c = db(&state)?;
    Ok(ListingSettings {
        ledger_path: setting(&c, "listing_ledger_path"),
        ledger_shop_name: setting(&c, "listing_ledger_shop_name"),
        tool_executable: setting(&c, "listing_tool_executable"),
        tool_data_dir: setting(&c, "listing_tool_data_dir"),
    })
}

#[tauri::command]
pub fn save_listing_settings(form: ListingSettings, state: State<AppState>) -> Result<(), String> {
    let c = db(&state)?;
    save_setting(&c, "listing_ledger_path", form.ledger_path.trim())?;
    save_setting(&c, "listing_ledger_shop_name", form.ledger_shop_name.trim())?;
    save_setting(&c, "listing_tool_executable", form.tool_executable.trim())?;
    save_setting(&c, "listing_tool_data_dir", form.tool_data_dir.trim())
}

#[tauri::command]
pub fn listing_rows(query: String, state: State<AppState>) -> Result<Vec<ListingRow>, String> {
    listing_rows_inner(&query, &state)
}

fn listing_rows_inner(query: &str, state: &AppState) -> Result<Vec<ListingRow>, String> {
    let c = db(state)?;
    let path = setting(&c, "listing_ledger_path");
    if path.is_empty() {
        return Ok(vec![]);
    }
    let selected = setting(&c, "listing_ledger_shop_name");
    let needle = query.trim().to_lowercase();
    Ok(cached_ledger(&path)?
        .into_iter()
        .filter(|r| {
            let platform = field(r, "平台").trim().to_lowercase();
            (platform.is_empty() || platform.starts_with("ozon"))
                && (selected.is_empty() || field(r, "上品店铺").eq_ignore_ascii_case(&selected))
        })
        .filter(|r| {
            needle.is_empty()
                || format!(
                    "{} {} {} {}",
                    field(r, "上品店铺"),
                    field(r, "货号"),
                    field(r, "Ozon商品ID"),
                    field(r, "商品标题")
                )
                .to_lowercase()
                .contains(&needle)
        })
        .map(|r| ListingRow {
            shop_name: field(&r, "上品店铺").into(),
            platform: field(&r, "平台").into(),
            offer_id: field(&r, "货号").into(),
            product_id: field(&r, "Ozon商品ID").into(),
            product_title: field(&r, "商品标题").into(),
            supplier_url: valid_supplier_url(field(&r, "1688采购链接")),
            currency_code: field(&r, "币种").into(),
            unit_cost_cny: number(field(&r, "采购成本")),
            weight_kg: number(field(&r, "包装毛重(g)")).map(|v| v / 1000.0),
            length_cm: number(field(&r, "包装长度(mm)")).map(|v| v / 10.0),
            width_cm: number(field(&r, "包装宽度(mm)")).map(|v| v / 10.0),
            height_cm: number(field(&r, "包装高度(mm)")).map(|v| v / 10.0),
            status: field(&r, "状态").into(),
            listing_mode: field(&r, "上品模式").into(),
            pricing_mode: field(&r, "核价方式").into(),
            price: number(field(&r, "售价")),
            profit: number(field(&r, "利润")),
            roi_percent: number(field(&r, "实际ROI")),
            category: field(&r, "Ozon类目").into(),
            import_task_id: field(&r, "Ozon导入任务ID").into(),
            updated_at: field(&r, "更新时间").into(),
        })
        .collect())
}

fn sync_listing_costs_blocking(state: &AppState) -> Result<i64, String> {
    let rows = listing_rows_inner("", state)?;
    let mut c = db(state)?;
    let tx = c.transaction().map_err(|e| e.to_string())?;
    let mut count = 0;
    for row in rows {
        if row.offer_id.is_empty() {
            continue;
        }
        count+=tx.execute("INSERT INTO product_costs(sku,unit_cost_cny,length_cm,width_cm,height_cm,weight_kg,note) SELECT sku,?2,?3,?4,?5,?6,'跨境上品台账同步' FROM products WHERE offer_id=?1 ON CONFLICT(sku) DO UPDATE SET unit_cost_cny=COALESCE(excluded.unit_cost_cny,product_costs.unit_cost_cny),length_cm=COALESCE(excluded.length_cm,product_costs.length_cm),width_cm=COALESCE(excluded.width_cm,product_costs.width_cm),height_cm=COALESCE(excluded.height_cm,product_costs.height_cm),weight_kg=COALESCE(excluded.weight_kg,product_costs.weight_kg),updated_at=CURRENT_TIMESTAMP",params![row.offer_id,row.unit_cost_cny,row.length_cm,row.width_cm,row.height_cm,row.weight_kg]).map_err(|e|e.to_string())? as i64;
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(count)
}

#[tauri::command]
pub async fn sync_listing_costs(state: State<'_, AppState>) -> Result<i64, String> {
    let owned = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || sync_listing_costs_blocking(&owned))
        .await
        .map_err(|e| format!("上品台账后台同步失败：{e}"))?
}

#[tauri::command]
pub fn launch_listing_tool(state: State<AppState>) -> Result<String, String> {
    let c = db(&state)?;
    let exe = setting(&c, "listing_tool_executable");
    if !Path::new(&exe).is_file() {
        return Err("未找到上品工具，请先填写可执行文件路径".into());
    }
    let data = setting(&c, "listing_tool_data_dir");
    let data_dir = if data.is_empty() {
        state.data_dir.join("listing_tool")
    } else {
        Path::new(&data).to_path_buf()
    };
    fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
    let registry = read_registry(&state.data_dir)?;
    let active = state
        .active_shop_id
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    let shop = registry
        .shops
        .iter()
        .find(|x| x.id == active)
        .ok_or("当前店铺不存在")?;
    let client = setting(&c, "seller_client_id");
    let key = secret_setting(&c, "seller_api_key")?;
    if client.is_empty() || key.is_empty() {
        return Err("当前店铺未配置 Seller Client ID / API Key".into());
    }
    let config = data_dir.join("config.json");
    let mut root: Value = if config.is_file() {
        serde_json::from_str(&fs::read_to_string(&config).map_err(|e| e.to_string())?)
            .unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };
    let ozon = root
        .as_object_mut()
        .ok_or("上品工具配置格式错误")?
        .entry("ozon")
        .or_insert_with(|| json!({}));
    let object = ozon.as_object_mut().ok_or("上品工具 Ozon 配置格式错误")?;
    let shops = object
        .entry("shops")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or("上品工具店铺配置格式错误")?;
    let profile =
        json!({"id":shop.id,"name":shop.name,"client_id":client,"api_key":key,"proxy_url":""});
    if let Some(existing) = shops
        .iter_mut()
        .find(|x| x.get("id").and_then(Value::as_str) == Some(&shop.id))
    {
        *existing = profile
    } else {
        shops.push(profile)
    }
    object.insert("selected_shop_id".into(), json!(shop.id));
    let temp = config.with_extension("json.tmp");
    fs::write(
        &temp,
        serde_json::to_vec_pretty(&root).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::rename(&temp, &config).map_err(|e| e.to_string())?;
    let child = Command::new(&exe)
        .current_dir(Path::new(&exe).parent().ok_or("上品工具路径无父目录")?)
        .env("OZON_RFBS_DATA_DIR", &data_dir)
        .spawn()
        .map_err(|e| format!("无法启动上品工具：{e}"))?;
    Ok(format!("上品工具已启动，PID {}", child.id()))
}

#[cfg(test)]
mod tests {
    use super::{
        ai_json_content, calculate_listing_price, clear_attribute_in_payload, flatten_categories,
        generated_offer_id, normalize_reference_input, number, reference_product_from_html,
        set_attribute_in_payload, stem, ListingAttributeValueInput, ListingPriceInput,
    };
    use crate::collect_competitor_browser_html;

    #[test]
    fn ledger_number_accepts_grouped_values() {
        assert_eq!(number("1,234.50"), Some(1234.5));
        assert_eq!(number(""), None);
    }
    #[test]
    fn reference_input_matches_rfbs_source() {
        assert_eq!(
            normalize_reference_input("Артикул: 2379505289").unwrap().0,
            "https://www.ozon.ru/product/2379505289/"
        );
        assert!(normalize_reference_input("https://example.com/123456").is_err());
    }
    #[test]
    fn reference_html_preserves_source_fields() {
        let page = r#"<script type="application/ld+json">{"@type":"Product","name":"Товар","description":"Описание","image":["https://cdn/a.jpg"],"additionalProperty":[{"name":"Цвет","value":"Черный"}]}</script>"#;
        let value = reference_product_from_html(page).unwrap();
        assert_eq!(value["title"], "Товар");
        assert_eq!(value["properties"]["Цвет"], "Черный");
        assert_eq!(value["images"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn reference_html_rejects_explicit_challenge() {
        assert!(
            reference_product_from_html("<title>Antibot Challenge Page</title>")
                .unwrap_err()
                .starts_with("blocked:")
        );
    }

    #[test]
    fn roi_pricing_matches_rfbs_source_cases() {
        let form = |purchase_cost, commission| ListingPriceInput {
            purchase_cost,
            label_fee: 2.0,
            target_roi_percent: 60.0,
            weight_kg: 0.6,
            sales_commission_percent: commission,
            sales_commission_discount_percent: 0.0,
            advertising_percent: 15.0,
            cargo_loss_percent: 10.0,
            minimum_sale_price: 0.0,
        };
        let first = calculate_listing_price(form(15.0, 12.0)).unwrap();
        assert_eq!(first.price, 106.0);
        assert!((first.shipping - 41.57).abs() < 0.000_001);
        let crossed = calculate_listing_price(form(50.0, 14.0)).unwrap();
        assert_eq!(crossed.price, 184.0);
        assert!((crossed.shipping - 34.83).abs() < 0.000_001);
    }
    #[test]
    fn category_tree_matches_legacy_leaf_and_type_shapes() {
        let tree = serde_json::json!([{"description_category_id":10,"category_name":"Дом","children":[{"category_name":"Хранение","type_id":20,"type_name":"Органайзеры"}]},{"category_id":30,"category_name":"Одежда","types":[{"id":40,"name":"Платья"},{"id":41,"name":"disabled","disabled":true}]}]);
        let mut rows = Vec::new();
        flatten_categories(&tree, &[], 0, &mut rows);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].description_category_id, 10);
        assert_eq!(rows[0].type_id, 20);
        assert_eq!(rows[0].display, "Дом / Хранение / Органайзеры");
        assert_eq!(rows[1].type_id, 40);
    }
    #[test]
    fn russian_stemming_matches_legacy_suffix_rules() {
        assert_eq!(stem("органайзерами"), "органайзер");
        assert_eq!(stem("платья"), "плать");
    }

    fn attribute_input(
        attribute_id: i64,
        complex_id: i64,
        collection: bool,
        dictionary_id: i64,
        value: &str,
    ) -> ListingAttributeValueInput {
        ListingAttributeValueInput {
            id: 1,
            attribute_id,
            attribute_complex_id: complex_id,
            attribute_name: format!("attribute {attribute_id}"),
            is_collection: collection,
            dictionary_value_id: dictionary_id,
            value: value.into(),
        }
    }

    #[test]
    fn structured_attribute_editor_updates_normal_and_dictionary_values() {
        let mut payload = serde_json::json!({"attributes":[],"complex_attributes":[]});
        set_attribute_in_payload(&mut payload, &attribute_input(10, 0, false, 0, "Красный"))
            .unwrap();
        set_attribute_in_payload(&mut payload, &attribute_input(11, 0, false, 501, "Хлопок"))
            .unwrap();
        assert_eq!(payload["attributes"][0]["values"][0]["value"], "Красный");
        assert_eq!(
            payload["attributes"][1]["values"][0]["dictionary_value_id"],
            501
        );
    }

    #[test]
    fn structured_attribute_editor_groups_collections_and_deduplicates() {
        let mut payload = serde_json::json!({"attributes":[],"complex_attributes":[]});
        let input = attribute_input(20, 99, true, 701, "Первый");
        set_attribute_in_payload(&mut payload, &input).unwrap();
        set_attribute_in_payload(&mut payload, &input).unwrap();
        set_attribute_in_payload(&mut payload, &attribute_input(20, 99, true, 702, "Второй"))
            .unwrap();
        assert_eq!(payload["complex_attributes"].as_array().unwrap().len(), 1);
        assert_eq!(
            payload["complex_attributes"][0]["attributes"][0]["values"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert!(clear_attribute_in_payload(&mut payload, 20));
        assert!(payload["complex_attributes"].as_array().unwrap().is_empty());
    }

    #[test]
    fn structured_attribute_editor_rejects_chinese_free_text() {
        let mut payload = serde_json::json!({"attributes":[],"complex_attributes":[]});
        assert!(
            set_attribute_in_payload(&mut payload, &attribute_input(30, 0, false, 0, "红色"))
                .unwrap_err()
                .contains("不能直接提交中文")
        );
    }

    #[test]
    fn generated_offer_id_matches_legacy_auto_shape() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-08-28T12:34:56.123456789+08:00")
            .unwrap()
            .with_timezone(&chrono::Local);
        let offer = generated_offer_id(now);
        assert!(regex::Regex::new(r"^AUTO-20260828-[0-9A-F]{6}$")
            .unwrap()
            .is_match(&offer));
    }

    #[test]
    fn ai_attribute_response_accepts_strict_json_code_fence() {
        let raw = serde_json::json!({"choices":[{"message":{"content":"```json\n{\"attributes\":[]}\n```"}}]}).to_string();
        assert!(ai_json_content(&raw).unwrap()["attributes"].is_array());
    }

    #[test]
    #[ignore = "requires a visible Chrome/Edge session and live Ozon access"]
    fn live_listing_browser_opens_target_product() {
        let root =
            std::env::temp_dir().join(format!("ozon-listing-browser-smoke-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let result = collect_competitor_browser_html(
            "https://www.ozon.ru/product/2846376063/",
            "2846376063",
            &root,
            -1,
        );
        let _ = std::fs::remove_dir_all(&root);
        let html = result.expect("browser must open and collect the requested Ozon product");
        assert!(html.contains("2846376063") || html.contains("og:title"));
        assert!(
            html.contains("codexMainImage") || html.contains("og:image"),
            "browser capture must contain verified main-image evidence"
        );
    }
}

#[tauri::command]
pub fn open_listing_browser(id: i64, state: State<AppState>) -> Result<(), String> {
    let c = db(&state)?;
    ensure_listing_jobs(&c)?;
    let url: String = c
        .query_row(
            "SELECT source_url FROM listing_jobs WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .map_err(|_| "上品草稿不存在".to_string())?;
    open::that(url).map_err(|e| format!("无法打开 Edge/默认浏览器：{e}"))
}

#[tauri::command]
pub fn import_listing_html(id: i64, path: String, state: State<AppState>) -> Result<i64, String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("请输入验证后保存的 HTML 文件完整路径".into());
    }
    let html = fs::read_to_string(path).map_err(|e| format!("无法读取验证页 HTML：{e}"))?;
    let collected = reference_product_from_html(&html)?;
    let c = db(&state)?;
    ensure_listing_jobs(&c)?;
    let (article, raw_payload): (String, String) = c
        .query_row(
            "SELECT article,payload FROM listing_jobs WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| "上品草稿不存在".to_string())?;
    if !article.is_empty() {
        let canonical=regex::Regex::new(r#"(?is)(?:rel=[\"']canonical[\"'][^>]*href|property=[\"']og:url[\"'][^>]*content)=[\"']([^\"']+)[\"']"#).unwrap().captures(&html).and_then(|c|c.get(1)).map(|v|v.as_str());
        if let Some(found) = canonical
            .and_then(|url| normalize_reference_input(url).ok())
            .map(|v| v.1)
        {
            if found != article {
                return Err(format!("保存的网页属于 Артикул {found}，当前任务是 {article}；已拒绝导入，避免采错变体"));
            }
        }
    }
    let mut payload = serde_json::from_str::<Value>(&raw_payload).unwrap_or_else(|_| json!({}));
    if let (Some(target), Some(source)) = (payload.as_object_mut(), collected.as_object()) {
        for (k, v) in source {
            target.insert(k.clone(), v.clone());
        }
    }
    let title = payload.get("title").and_then(Value::as_str).unwrap_or("");
    let images = payload
        .get("images")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    if images == 0 {
        return Err("验证页已读取到标题，但没有商品图片；请等待商品图库完整显示后重新保存“网页，仅 HTML”文件".into());
    }
    c.execute("UPDATE listing_jobs SET title=?1,status='draft',stage=MAX(stage,1),error='',payload=?2,updated_at=CURRENT_TIMESTAMP WHERE id=?3",params![title,payload.to_string(),id]).map_err(|e|e.to_string())?;
    Ok(images as i64)
}
