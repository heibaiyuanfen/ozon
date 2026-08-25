use crate::{secrets, AppState, DateRange};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fs};
use tauri::State;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WbSettings {
    pub store_name: String,
    pub token: String,
    pub rub_per_cny: f64,
    pub commission_percent: f64,
    pub feishu_app_id: String,
    pub feishu_app_secret: String,
    pub feishu_chat_id: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WbCost {
    pub nm_id: i64,
    pub article: String,
    pub purchase_cost_cny: Option<f64>,
    pub length_cm: Option<f64>,
    pub width_cm: Option<f64>,
    pub height_cm: Option<f64>,
    pub weight_kg: Option<f64>,
    pub warehouse_mode: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WbDaily {
    pub day: String,
    pub nm_id: i64,
    pub article: String,
    pub warehouse_name: String,
    pub quantity: i64,
    pub revenue_cny: f64,
    pub ad_spend_cny: f64,
    pub commission_cny: f64,
    pub purchase_total_cny: Option<f64>,
    pub logistics_total_cny: Option<f64>,
    pub profit_cny: Option<f64>,
    pub complete: bool,
}

fn db(state: &AppState) -> Result<Connection, String> {
    let folder = state.data_dir.join("wb");
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let c = Connection::open(folder.join("wb_analytics.db")).map_err(|e| e.to_string())?;
    c.execute_batch("PRAGMA journal_mode=WAL;CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY,value TEXT NOT NULL DEFAULT '');CREATE TABLE IF NOT EXISTS orders(srid TEXT PRIMARY KEY,day TEXT NOT NULL,changed_at TEXT,nm_id INTEGER NOT NULL DEFAULT 0,article TEXT,warehouse_name TEXT,quantity INTEGER NOT NULL DEFAULT 1,revenue_rub REAL NOT NULL DEFAULT 0,is_cancelled INTEGER NOT NULL DEFAULT 0,raw_json TEXT NOT NULL DEFAULT '{}');CREATE TABLE IF NOT EXISTS ad_daily(day TEXT NOT NULL,nm_id INTEGER NOT NULL,campaign_id INTEGER NOT NULL,spend_rub REAL NOT NULL DEFAULT 0,ad_orders INTEGER NOT NULL DEFAULT 0,ad_sales_rub REAL NOT NULL DEFAULT 0,views INTEGER NOT NULL DEFAULT 0,clicks INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(day,nm_id,campaign_id));CREATE TABLE IF NOT EXISTS product_costs(nm_id INTEGER PRIMARY KEY,article TEXT NOT NULL DEFAULT '',purchase_cost_cny REAL,length_cm REAL,width_cm REAL,height_cm REAL,weight_kg REAL,warehouse_mode TEXT NOT NULL DEFAULT 'auto');CREATE TABLE IF NOT EXISTS warehouses(warehouse_key TEXT PRIMARY KEY,name TEXT NOT NULL DEFAULT '',address TEXT NOT NULL DEFAULT '',city TEXT NOT NULL DEFAULT '',country TEXT NOT NULL DEFAULT '',mode TEXT NOT NULL DEFAULT 'unknown',raw_json TEXT NOT NULL DEFAULT '{}');").map_err(|e|e.to_string())?;
    Ok(c)
}
fn setting(c: &Connection, key: &str, default: &str) -> String {
    c.query_row("SELECT value FROM settings WHERE key=?1", [key], |r| {
        r.get(0)
    })
    .unwrap_or_else(|_| default.into())
}
fn number(v: Option<&serde_json::Value>) -> f64 {
    v.and_then(|x| x.as_f64())
        .or_else(|| v.and_then(|x| x.as_str()).and_then(|s| s.parse().ok()))
        .unwrap_or(0.0)
}
fn wb_get(token: &str, url: &str) -> Result<serde_json::Value, String> {
    if token.is_empty() {
        return Err("请先填写 WB API Token".into());
    }
    let response = ureq::get(url)
        .set("Authorization", token)
        .set("Accept", "application/json")
        .call()
        .map_err(|e| format!("WB API 请求失败：{e}"))?;
    let raw = response.into_string().map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| format!("WB API 返回无法解析：{e}"))
}
fn mode(name: &str) -> &'static str {
    let lower = name.to_lowercase();
    if ["东莞", "dongguan", "dpg", "guangdong", "广东"]
        .iter()
        .any(|x| lower.contains(x))
    {
        "dongguan"
    } else if ["russia", "russian", "россия", "москва", "moscow"]
        .iter()
        .any(|x| lower.contains(x))
        || name
            .chars()
            .any(|c| ('а'..='я').contains(&c) || ('А'..='Я').contains(&c))
    {
        "overseas"
    } else {
        "unknown"
    }
}
fn logistics(
    kind: &str,
    weight: Option<f64>,
    l: Option<f64>,
    w: Option<f64>,
    h: Option<f64>,
) -> Option<f64> {
    match kind {
        "dongguan" => weight
            .filter(|x| *x > 0.0)
            .map(|x| x * if x <= 0.3 { 58.0 } else { 43.0 } + if x <= 0.3 { 2.0 } else { 8.0 }),
        "overseas" => Some(8.0 + (l? * w? * h? / 1000.0 - 1.0).max(0.0) * 2.0),
        _ => None,
    }
}

#[tauri::command]
pub fn wb_settings(state: State<AppState>) -> Result<WbSettings, String> {
    let c = db(&state)?;
    let cipher = setting(&c, "token", "");
    Ok(WbSettings {
        store_name: setting(&c, "store_name", "WB 跨境店"),
        token: if cipher.is_empty() {
            String::new()
        } else {
            "••••••••".into()
        },
        rub_per_cny: setting(&c, "rub_per_cny", "12").parse().unwrap_or(12.0),
        commission_percent: setting(&c, "commission_percent", "15")
            .parse()
            .unwrap_or(15.0),
        feishu_app_id: setting(&c, "feishu_app_id", ""),
        feishu_app_secret: if setting(&c, "feishu_app_secret", "").is_empty() {
            String::new()
        } else {
            "••••••••".into()
        },
        feishu_chat_id: setting(&c, "feishu_chat_id", ""),
    })
}
#[tauri::command]
pub fn save_wb_settings(form: WbSettings, state: State<AppState>) -> Result<(), String> {
    if form.rub_per_cny <= 0.0 {
        return Err("人民币兑卢布汇率必须大于 0".into());
    }
    let c = db(&state)?;
    for (k, v) in [
        ("store_name", form.store_name),
        ("rub_per_cny", form.rub_per_cny.to_string()),
        ("commission_percent", form.commission_percent.to_string()),
        ("feishu_app_id", form.feishu_app_id),
        ("feishu_chat_id", form.feishu_chat_id),
    ] {
        c.execute("INSERT INTO settings(key,value)VALUES(?1,?2)ON CONFLICT(key)DO UPDATE SET value=excluded.value",params![k,v]).map_err(|e|e.to_string())?;
    }
    if !form.token.is_empty() && form.token != "••••••••" {
        c.execute("INSERT INTO settings(key,value)VALUES('token',?1)ON CONFLICT(key)DO UPDATE SET value=excluded.value",[secrets::protect(&form.token)?]).map_err(|e|e.to_string())?;
    }
    if !form.feishu_app_secret.is_empty() && form.feishu_app_secret != "••••••••" {
        c.execute("INSERT INTO settings(key,value)VALUES('feishu_app_secret',?1)ON CONFLICT(key)DO UPDATE SET value=excluded.value",[secrets::protect(&form.feishu_app_secret)?]).map_err(|e|e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn export_wb_api_bundle(state: State<AppState>) -> Result<String, String> {
    let c = db(&state)?;
    let token_cipher = setting(&c, "token", "");
    let token = if token_cipher.is_empty() {
        String::new()
    } else {
        secrets::unprotect(&token_cipher)?
    };
    let bundle = serde_json::json!({
        "type": "wb-erp-api-bundle", "version": 1,
        "exported_at": chrono::Utc::now().to_rfc3339(),
        "warning": "此文件包含明文 WB Token，请妥善保管并在导入后删除。",
        "credentials": {
            "store_name": setting(&c,"store_name","WB 跨境店"), "token": token,
            "rub_per_cny": setting(&c,"rub_per_cny","12"),
            "commission_percent": setting(&c,"commission_percent","15")
        }
    });
    let folder = state
        .data_dir
        .parent()
        .unwrap_or(&state.data_dir)
        .join("exports");
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let path = folder.join(format!(
        "wb-api-config-{}.wb-api.json",
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
pub fn import_wb_api_bundle(path: String, state: State<AppState>) -> Result<(), String> {
    let raw: serde_json::Value = serde_json::from_slice(
        &fs::read(path.trim()).map_err(|e| format!("无法读取 WB API 配置包：{e}"))?,
    )
    .map_err(|e| format!("WB API 配置包 JSON 无效：{e}"))?;
    if raw.get("type").and_then(|v| v.as_str()) != Some("wb-erp-api-bundle")
        || raw.get("version").and_then(|v| v.as_i64()) != Some(1)
    {
        return Err("文件不是受支持的 Python/React 通用 WB API 配置包".into());
    }
    let values = raw
        .get("credentials")
        .and_then(|v| v.as_object())
        .ok_or("配置包缺少 credentials")?;
    let c = db(&state)?;
    for key in ["store_name", "rub_per_cny", "commission_percent"] {
        if let Some(value) = values.get(key).and_then(|v| v.as_str()) {
            c.execute("INSERT INTO settings(key,value)VALUES(?1,?2)ON CONFLICT(key)DO UPDATE SET value=excluded.value", params![key,value]).map_err(|e|e.to_string())?;
        }
    }
    if let Some(token) = values
        .get("token")
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
    {
        c.execute("INSERT INTO settings(key,value)VALUES('token',?1)ON CONFLICT(key)DO UPDATE SET value=excluded.value", [secrets::protect(token)?]).map_err(|e|e.to_string())?;
    }
    Ok(())
}
#[tauri::command]
pub fn wb_costs(state: State<AppState>) -> Result<Vec<WbCost>, String> {
    let c = db(&state)?;
    let mut stmt=c.prepare("WITH known AS(SELECT nm_id,MAX(article)article FROM orders GROUP BY nm_id UNION SELECT nm_id,article FROM product_costs)SELECT k.nm_id,MAX(k.article),c.purchase_cost_cny,c.length_cm,c.width_cm,c.height_cm,c.weight_kg,COALESCE(c.warehouse_mode,'auto')FROM known k LEFT JOIN product_costs c ON c.nm_id=k.nm_id GROUP BY k.nm_id ORDER BY MAX(k.article),k.nm_id").map_err(|e|e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok(WbCost {
                nm_id: r.get(0)?,
                article: r.get(1)?,
                purchase_cost_cny: r.get(2)?,
                length_cm: r.get(3)?,
                width_cm: r.get(4)?,
                height_cm: r.get(5)?,
                weight_kg: r.get(6)?,
                warehouse_mode: r.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}
#[tauri::command]
pub fn save_wb_cost(input: WbCost, state: State<AppState>) -> Result<(), String> {
    let c = db(&state)?;
    c.execute("INSERT INTO product_costs(nm_id,article,purchase_cost_cny,length_cm,width_cm,height_cm,weight_kg,warehouse_mode)VALUES(?1,?2,?3,?4,?5,?6,?7,?8)ON CONFLICT(nm_id)DO UPDATE SET article=excluded.article,purchase_cost_cny=excluded.purchase_cost_cny,length_cm=excluded.length_cm,width_cm=excluded.width_cm,height_cm=excluded.height_cm,weight_kg=excluded.weight_kg,warehouse_mode=excluded.warehouse_mode",params![input.nm_id,input.article,input.purchase_cost_cny,input.length_cm,input.width_cm,input.height_cm,input.weight_kg,input.warehouse_mode]).map_err(|e|e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn wb_daily(range: DateRange, state: State<AppState>) -> Result<Vec<WbDaily>, String> {
    let c = db(&state)?;
    let rate = setting(&c, "rub_per_cny", "12")
        .parse::<f64>()
        .unwrap_or(12.0)
        .max(0.0001);
    let commission = setting(&c, "commission_percent", "15")
        .parse::<f64>()
        .unwrap_or(15.0)
        .max(0.0);
    let sql = "WITH o AS (SELECT day,nm_id,MAX(article) article,MAX(warehouse_name) warehouse_name,SUM(CASE WHEN is_cancelled=0 THEN quantity ELSE 0 END) quantity,SUM(CASE WHEN is_cancelled=0 THEN revenue_rub ELSE 0 END) revenue_rub FROM orders WHERE day BETWEEN ?1 AND ?2 GROUP BY day,nm_id),a AS (SELECT day,nm_id,SUM(spend_rub) spend_rub FROM ad_daily WHERE day BETWEEN ?1 AND ?2 GROUP BY day,nm_id) SELECT o.day,o.nm_id,o.article,o.warehouse_name,o.quantity,o.revenue_rub,COALESCE(a.spend_rub,0),pc.purchase_cost_cny,pc.length_cm,pc.width_cm,pc.height_cm,pc.weight_kg,COALESCE(pc.warehouse_mode,'auto') FROM o LEFT JOIN a ON a.day=o.day AND a.nm_id=o.nm_id LEFT JOIN product_costs pc ON pc.nm_id=o.nm_id ORDER BY o.day DESC,o.revenue_rub DESC";
    let mut stmt = c.prepare(sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![range.from, range.to], |r| {
            let warehouse: String = r.get(3)?;
            let qty: i64 = r.get(4)?;
            let revenue_rub: f64 = r.get(5)?;
            let spend_rub: f64 = r.get(6)?;
            let purchase: Option<f64> = r.get(7)?;
            let length = r.get(8)?;
            let width = r.get(9)?;
            let height = r.get(10)?;
            let weight = r.get(11)?;
            let configured: String = r.get(12)?;
            let resolved = if configured == "auto" {
                mode(&warehouse)
            } else {
                configured.as_str()
            };
            let each = logistics(resolved, weight, length, width, height);
            let complete = purchase.is_some() && each.is_some();
            let revenue = revenue_rub / rate;
            let ad = spend_rub / rate;
            let platform = revenue * commission / 100.0;
            let purchase_total = purchase.map(|v| v * qty as f64);
            let logistics_total = each.map(|v| v * qty as f64);
            let profit = complete.then(|| {
                revenue
                    - ad
                    - platform
                    - purchase_total.unwrap_or(0.0)
                    - logistics_total.unwrap_or(0.0)
            });
            Ok(WbDaily {
                day: r.get(0)?,
                nm_id: r.get(1)?,
                article: r.get(2)?,
                warehouse_name: warehouse,
                quantity: qty,
                revenue_cny: revenue,
                ad_spend_cny: ad,
                commission_cny: platform,
                purchase_total_cny: purchase_total,
                logistics_total_cny: logistics_total,
                profit_cny: profit,
                complete,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

fn campaign_ids(value: &serde_json::Value, target: &mut BTreeSet<i64>) {
    match value {
        serde_json::Value::Array(a) => {
            for v in a {
                campaign_ids(v, target)
            }
        }
        serde_json::Value::Object(o) => {
            for key in ["advertId", "advert_id"] {
                if let Some(id) = o.get(key).and_then(|v| v.as_i64()) {
                    target.insert(id);
                }
            }
            for v in o.values() {
                campaign_ids(v, target)
            }
        }
        _ => {}
    }
}
fn sync_wb_blocking(range: DateRange, state: &AppState) -> Result<String, String> {
    let mut c = db(state)?;
    let token = secrets::unprotect(&setting(&c, "token", ""))?;
    let orders=wb_get(&token,&format!("https://statistics-api.wildberries.ru/api/v1/supplier/orders?dateFrom={}T00:00:00&flag=0",range.from))?;
    let tx = c.transaction().map_err(|e| e.to_string())?;
    let mut order_count = 0;
    for row in orders.as_array().into_iter().flatten() {
        let day = row
            .get("date")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .chars()
            .take(10)
            .collect::<String>();
        if day < range.from || day > range.to {
            continue;
        }
        let id = row
            .get("srid")
            .or_else(|| row.get("gNumber"))
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if id.is_empty() {
            continue;
        }
        tx.execute("INSERT INTO orders(srid,day,changed_at,nm_id,article,warehouse_name,quantity,revenue_rub,is_cancelled,raw_json)VALUES(?1,?2,?3,?4,?5,?6,1,?7,?8,?9)ON CONFLICT(srid)DO UPDATE SET day=excluded.day,changed_at=excluded.changed_at,nm_id=excluded.nm_id,article=excluded.article,warehouse_name=excluded.warehouse_name,revenue_rub=excluded.revenue_rub,is_cancelled=excluded.is_cancelled,raw_json=excluded.raw_json",params![id,day,row.get("lastChangeDate").and_then(|v|v.as_str()).unwrap_or_default(),row.get("nmId").and_then(|v|v.as_i64()).unwrap_or(0),row.get("supplierArticle").and_then(|v|v.as_str()).unwrap_or_default(),row.get("warehouseName").and_then(|v|v.as_str()).unwrap_or_default(),number(row.get("finishedPrice")).max(number(row.get("priceWithDisc"))),row.get("isCancel").and_then(|v|v.as_bool()).unwrap_or(false)as i64,row.to_string()]).map_err(|e|e.to_string())?;
        order_count += 1;
    }
    tx.commit().map_err(|e| e.to_string())?;
    let campaigns = wb_get(
        &token,
        "https://advert-api.wildberries.ru/adv/v1/promotion/count",
    )?;
    let mut ids = BTreeSet::new();
    campaign_ids(&campaigns, &mut ids);
    let mut ad_count = 0;
    for (chunk_index, chunk) in ids.into_iter().collect::<Vec<_>>().chunks(50).enumerate() {
        if chunk_index > 0 {
            std::thread::sleep(std::time::Duration::from_secs(20));
        }
        let list = chunk
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let payload=wb_get(&token,&format!("https://advert-api.wildberries.ru/adv/v3/fullstats?ids={list}&beginDate={}&endDate={}",range.from,range.to))?;
        for campaign in payload.as_array().into_iter().flatten() {
            let cid = campaign
                .get("advertId")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            for day in campaign
                .get("days")
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten()
            {
                let date = day
                    .get("date")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .chars()
                    .take(10)
                    .collect::<String>();
                for app in day
                    .get("apps")
                    .and_then(|v| v.as_array())
                    .into_iter()
                    .flatten()
                {
                    for product in app
                        .get("nm")
                        .and_then(|v| v.as_array())
                        .into_iter()
                        .flatten()
                    {
                        let nm = product.get("nmId").and_then(|v| v.as_i64()).unwrap_or(0);
                        if nm == 0 {
                            continue;
                        }
                        c.execute("INSERT INTO ad_daily(day,nm_id,campaign_id,spend_rub,ad_orders,ad_sales_rub,views,clicks)VALUES(?1,?2,?3,?4,?5,?6,?7,?8)ON CONFLICT(day,nm_id,campaign_id)DO UPDATE SET spend_rub=excluded.spend_rub,ad_orders=excluded.ad_orders,ad_sales_rub=excluded.ad_sales_rub,views=excluded.views,clicks=excluded.clicks",params![date,nm,cid,number(product.get("sum")),number(product.get("orders"))as i64,number(product.get("sum_price").or_else(||product.get("sumPrice"))),number(product.get("views"))as i64,number(product.get("clicks"))as i64]).map_err(|e|e.to_string())?;
                        ad_count += 1;
                    }
                }
            }
        }
    }
    let offices = wb_get(
        &token,
        "https://marketplace-api.wildberries.ru/api/v3/offices",
    )?;
    let sellers = wb_get(
        &token,
        "https://marketplace-api.wildberries.ru/api/v3/warehouses",
    )?;
    let mut warehouse_count = 0;
    for (source, payload) in [("office", offices), ("seller", sellers)] {
        for row in payload.as_array().into_iter().flatten() {
            let raw_key = row
                .get("id")
                .or_else(|| row.get("officeId"))
                .and_then(|v| v.as_i64())
                .map(|v| v.to_string())
                .unwrap_or_else(|| {
                    row.get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string()
                });
            if raw_key.is_empty() {
                continue;
            }
            let name = row.get("name").and_then(|v| v.as_str()).unwrap_or_default();
            let address = row
                .get("address")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let city = row.get("city").and_then(|v| v.as_str()).unwrap_or_default();
            let country = row
                .get("country")
                .or_else(|| row.get("countryName"))
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let classification = mode(&format!("{name} {address} {city} {country}"));
            c.execute("INSERT INTO warehouses(warehouse_key,name,address,city,country,mode,raw_json)VALUES(?1,?2,?3,?4,?5,?6,?7)ON CONFLICT(warehouse_key)DO UPDATE SET name=excluded.name,address=excluded.address,city=excluded.city,country=excluded.country,mode=excluded.mode,raw_json=excluded.raw_json",params![format!("{source}:{raw_key}"),name,address,city,country,classification,row.to_string()]).map_err(|e|e.to_string())?;
            warehouse_count += 1;
        }
    }
    Ok(format!(
        "WB 同步完成：订单 {order_count}，商品广告 {ad_count}，仓库 {warehouse_count}"
    ))
}

#[tauri::command]
pub async fn sync_wb(range: DateRange, state: State<'_, AppState>) -> Result<String, String> {
    let owned = crate::background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || sync_wb_blocking(range, &owned))
        .await
        .map_err(|e| e.to_string())?
}

fn wb_feishu_token(c: &Connection) -> Result<String, String> {
    let id = setting(c, "feishu_app_id", "");
    let secret = secrets::unprotect(&setting(c, "feishu_app_secret", ""))?;
    if id.is_empty() || secret.is_empty() {
        return Err("请先填写 WB 专用飞书 App ID 和 App Secret".into());
    }
    let body = serde_json::json!({"app_id":id,"app_secret":secret});
    let response =
        ureq::post("https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal/")
            .set("Content-Type", "application/json")
            .send_string(&body.to_string())
            .map_err(|e| format!("飞书认证失败：{e}"))?;
    let raw = response.into_string().map_err(|e| e.to_string())?;
    let payload: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let code = payload.get("code").and_then(|v| v.as_i64()).unwrap_or(0);
    if code != 0 {
        return Err(format!(
            "飞书认证失败：{}",
            payload
                .get("msg")
                .and_then(|v| v.as_str())
                .unwrap_or("未知错误")
        ));
    }
    let token = payload
        .get("tenant_access_token")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    if token.is_empty() {
        Err("飞书未返回 tenant_access_token".into())
    } else {
        Ok(token)
    }
}

#[tauri::command]
pub fn test_wb_feishu(state: State<AppState>) -> Result<String, String> {
    let c = db(&state)?;
    wb_feishu_token(&c)?;
    Ok("WB 专用飞书应用认证成功".into())
}

#[tauri::command]
pub fn send_wb_weekly(range: DateRange, state: State<AppState>) -> Result<String, String> {
    let c = db(&state)?;
    let token = wb_feishu_token(&c)?;
    let chat = setting(&c, "feishu_chat_id", "");
    if chat.is_empty() {
        return Err("请先填写 WB 专用飞书群 Chat ID".into());
    }
    let rate = setting(&c, "rub_per_cny", "12")
        .parse::<f64>()
        .unwrap_or(12.0)
        .max(0.0001);
    let commission_rate = setting(&c, "commission_percent", "15")
        .parse::<f64>()
        .unwrap_or(15.0)
        .max(0.0);
    let sql="WITH o AS(SELECT day,nm_id,MAX(warehouse_name) warehouse_name,SUM(CASE WHEN is_cancelled=0 THEN quantity ELSE 0 END) quantity,SUM(CASE WHEN is_cancelled=0 THEN revenue_rub ELSE 0 END) revenue_rub FROM orders WHERE day BETWEEN ?1 AND ?2 GROUP BY day,nm_id),a AS(SELECT day,nm_id,SUM(spend_rub) spend_rub FROM ad_daily WHERE day BETWEEN ?1 AND ?2 GROUP BY day,nm_id)SELECT o.nm_id,o.warehouse_name,o.quantity,o.revenue_rub,COALESCE(a.spend_rub,0),pc.purchase_cost_cny,pc.length_cm,pc.width_cm,pc.height_cm,pc.weight_kg,COALESCE(pc.warehouse_mode,'auto')FROM o LEFT JOIN a ON a.day=o.day AND a.nm_id=o.nm_id LEFT JOIN product_costs pc ON pc.nm_id=o.nm_id";
    let mut stmt = c.prepare(sql).map_err(|e| e.to_string())?;
    let source = stmt
        .query_map(params![range.from, range.to], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, f64>(3)?,
                r.get::<_, f64>(4)?,
                r.get::<_, Option<f64>>(5)?,
                r.get::<_, Option<f64>>(6)?,
                r.get::<_, Option<f64>>(7)?,
                r.get::<_, Option<f64>>(8)?,
                r.get::<_, Option<f64>>(9)?,
                r.get::<_, String>(10)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut quantity = 0;
    let mut revenue = 0.0;
    let mut ad = 0.0;
    let mut platform = 0.0;
    let mut purchase = 0.0;
    let mut shipping = 0.0;
    let mut profit = 0.0;
    let mut products = BTreeSet::new();
    let mut incomplete = BTreeSet::new();
    for row in source {
        products.insert(row.0);
        quantity += row.2;
        let rev = row.3 / rate;
        let ads = row.4 / rate;
        let fee = rev * commission_rate / 100.0;
        revenue += rev;
        ad += ads;
        platform += fee;
        let resolved = if row.10 == "auto" {
            mode(&row.1)
        } else {
            row.10.as_str()
        };
        let ship = logistics(resolved, row.9, row.6, row.7, row.8);
        if let (Some(cost), Some(logistic)) = (row.5, ship) {
            let cost_total = cost * row.2 as f64;
            let ship_total = logistic * row.2 as f64;
            purchase += cost_total;
            shipping += ship_total;
            profit += rev - ads - fee - cost_total - ship_total;
        } else {
            incomplete.insert(row.0);
        }
    }
    let store = setting(&c, "store_name", "WB 跨境店");
    let content=format!("**店铺：** {store}\n**周期：** {} 至 {}\n\n**销量：** {quantity} 件\n**销售额：** ¥{revenue:.2}\n**广告费：** ¥{ad:.2}\n**暂估平台费：** ¥{platform:.2}\n**采购成本：** ¥{purchase:.2}\n**暂估物流：** ¥{shipping:.2}\n**暂估利润：** ¥{profit:.2}\n\n共 {} 个出单商品；{} 个商品缺成本、尺寸或仓库归类。",range.from,range.to,products.len(),incomplete.len());
    let card = serde_json::json!({"config":{"wide_screen_mode":true},"header":{"template":"purple","title":{"tag":"plain_text","content":"WB 跨境店周利润"}},"elements":[{"tag":"markdown","content":content},{"tag":"note","elements":[{"tag":"plain_text","content":"实时经营暂估；最终利润以 WB 财务结算为准"}]}]});
    let body =
        serde_json::json!({"receive_id":chat,"msg_type":"interactive","content":card.to_string()});
    let response =
        ureq::post("https://open.feishu.cn/open-apis/im/v1/messages?receive_id_type=chat_id")
            .set("Authorization", &format!("Bearer {token}"))
            .set("Content-Type", "application/json")
            .send_string(&body.to_string())
            .map_err(|e| format!("发送飞书失败：{e}"))?;
    let raw = response.into_string().map_err(|e| e.to_string())?;
    let payload: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    if payload.get("code").and_then(|v| v.as_i64()).unwrap_or(0) != 0 {
        return Err(format!(
            "飞书发送失败：{}",
            payload
                .get("msg")
                .and_then(|v| v.as_str())
                .unwrap_or("未知错误")
        ));
    }
    Ok("WB 周利润卡片已发送到飞书群".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn provisional_logistics_matches_legacy_rules() {
        assert!((logistics("dongguan", Some(0.2), None, None, None).unwrap() - 13.6).abs() < 0.001);
        assert!((logistics("dongguan", Some(1.0), None, None, None).unwrap() - 51.0).abs() < 0.001);
        assert!(
            (logistics("overseas", None, Some(20.0), Some(10.0), Some(10.0)).unwrap() - 10.0).abs()
                < 0.001
        );
        assert!(logistics("unknown", Some(1.0), Some(1.0), Some(1.0), Some(1.0)).is_none());
    }
    #[test]
    fn warehouse_classification_matches_legacy_rules() {
        assert_eq!(mode("东莞 DPG"), "dongguan");
        assert_eq!(mode("Москва"), "overseas");
        assert_eq!(mode("Unspecified"), "unknown");
    }
}
