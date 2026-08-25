use crate::{
    background_state, db, read_registry, save_setting, secret_setting, seller_post, setting,
    AppState,
};
use calamine::{open_workbook_auto, Reader};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashMap, fs, path::Path, process::Command};
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

#[tauri::command]
pub fn create_listing_draft(reference: String, state: State<AppState>) -> Result<i64, String> {
    let (url, article) = normalize_reference_input(&reference)?;
    let c = db(&state)?;
    ensure_listing_jobs(&c)?;
    c.execute("INSERT INTO listing_jobs(source_url,article,status,stage,payload)VALUES(?1,?2,'draft',0,?3)",params![url,article,json!({"source_url":url,"article":article}).to_string()]).map_err(|e|e.to_string())?;
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
    images.sort();
    images.dedup();
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

fn collect_reference_browser(url: &str, article: &str, state: &AppState) -> Result<Value, String> {
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
    let (source_url, article): (String, String) = c
        .query_row(
            "SELECT source_url,article FROM listing_jobs WHERE id=?1",
            [form.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
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
    let payload = json!({"source_url":source_url,"article":article,"offer_id":form.offer_id.trim(),"title":form.title.trim(),"category_id":form.category_id.trim(),"category_display":form.category_display.trim(),"type_id":form.type_id.trim(),"price":form.price.trim(),"weight":form.weight,"depth":form.depth,"width":form.width,"height":form.height,"description":form.description,"images":form.images,"attributes":form.attributes,"complex_attributes":form.complex_attributes});
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
    if weight < 0.5 {
        bands.push((0.0, 135.0, 3.37 + weight * 28.17));
    } else if weight < 30.0 {
        bands.push((0.0, 135.0, 25.83 + weight * 19.17));
    }
    if weight < 2.0 {
        bands.push((135.0, 635.0, 17.97 + weight * 28.17));
    } else if weight < 30.0 {
        bands.push((135.0, 635.0, 40.44 + weight * 28.17));
    }
    if weight < 5.0 {
        bands.push((635.0, 22_525.0, 24.17 + weight * 28.17));
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
    Ok(ledger(&path)?
        .into_iter()
        .filter(|r| {
            field(r, "平台").to_lowercase().starts_with("ozon")
                && (selected.is_empty() || field(r, "上品店铺").eq_ignore_ascii_case(&selected))
        })
        .filter(|r| {
            needle.is_empty()
                || format!(
                    "{} {} {}",
                    field(r, "上品店铺"),
                    field(r, "货号"),
                    field(r, "Ozon商品ID")
                )
                .to_lowercase()
                .contains(&needle)
        })
        .map(|r| ListingRow {
            shop_name: field(&r, "上品店铺").into(),
            platform: field(&r, "平台").into(),
            offer_id: field(&r, "货号").into(),
            product_id: field(&r, "Ozon商品ID").into(),
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
        calculate_listing_price, flatten_categories, normalize_reference_input, number,
        reference_product_from_html, stem, ListingPriceInput,
    };

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
        assert_eq!(first.price, 99.0);
        assert!((first.shipping - 37.332).abs() < 0.000_001);
        let crossed = calculate_listing_price(form(50.0, 14.0)).unwrap();
        assert_eq!(crossed.price, 184.0);
        assert!((crossed.shipping - 34.872).abs() < 0.000_001);
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
