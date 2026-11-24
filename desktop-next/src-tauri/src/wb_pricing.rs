use crate::{wb_shop_center, AppState};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::{sync::Mutex, time::Duration};
use tauri::State;

const PRICES_API: &str = "https://discounts-prices-api.wildberries.cn/api/v2";

static PRICE_IO: Mutex<()> = Mutex::new(());

fn schema(c: &Connection) -> Result<(), String> {
    c.execute_batch("
        CREATE TABLE IF NOT EXISTS wb_pricing_state(shop_id TEXT NOT NULL, kind TEXT NOT NULL, value TEXT NOT NULL, PRIMARY KEY(shop_id,kind));
        CREATE TABLE IF NOT EXISTS wb_price_observed(shop_id TEXT NOT NULL,nm_id INTEGER NOT NULL,size_id INTEGER NOT NULL,currency TEXT NOT NULL,prices TEXT NOT NULL,PRIMARY KEY(shop_id,nm_id,size_id));
        CREATE TABLE IF NOT EXISTS wb_price_submissions(shop_id TEXT NOT NULL,task INTEGER NOT NULL,nm_id INTEGER NOT NULL,size_id INTEGER,field TEXT NOT NULL,value INTEGER NOT NULL,submitted_at INTEGER NOT NULL,PRIMARY KEY(shop_id,task));
        CREATE TABLE IF NOT EXISTS wb_price_history(id INTEGER PRIMARY KEY,shop_id TEXT NOT NULL,day TEXT NOT NULL,observed_at TEXT NOT NULL,nm_id INTEGER NOT NULL,size_id INTEGER NOT NULL,vendor_code TEXT NOT NULL,size_name TEXT NOT NULL,field TEXT NOT NULL,before TEXT NOT NULL,after TEXT NOT NULL,task INTEGER);
        CREATE INDEX IF NOT EXISTS wb_price_history_by_shop ON wb_price_history(shop_id,day,id);
        CREATE TABLE IF NOT EXISTS wb_price_monitor_runs(shop_id TEXT NOT NULL,day TEXT NOT NULL,error TEXT NOT NULL,PRIMARY KEY(shop_id,day));
    ").map_err(|e| e.to_string())
}

fn monitor_day() -> String {
    chrono::Utc::now()
        .with_timezone(&chrono::FixedOffset::east_opt(8 * 3600).unwrap())
        .format("%Y-%m-%d")
        .to_string()
}

fn observe(c: &Connection, shop: &str, goods: &[Value]) -> Result<(), String> {
    let observed = chrono::Utc::now();
    let stamp = observed.to_rfc3339();
    let day = monitor_day();
    for g in goods {
        let Some(nm) = g["nmID"].as_i64() else {
            continue;
        };
        let Some(currency) = g["currencyIsoCode4217"].as_str() else {
            continue;
        };
        let Some(sizes) = g["sizes"].as_array() else {
            continue;
        };
        for size in sizes {
            let Some(size_id) = size["sizeID"].as_i64() else {
                continue;
            };
            let mut fields = serde_json::Map::new();
            for (key, value) in [
                ("price", &size["price"]),
                ("discountedPrice", &size["discountedPrice"]),
                ("discount", &g["discount"]),
                ("clubDiscount", &g["clubDiscount"]),
                ("clubDiscountedPrice", &size["clubDiscountedPrice"]),
            ] {
                if value.is_number() {
                    fields.insert(key.into(), value.clone());
                }
            }
            if fields.is_empty() {
                continue;
            }
            let prior: Option<(String, String)> = c.query_row(
                "SELECT currency,prices FROM wb_price_observed WHERE shop_id=?1 AND nm_id=?2 AND size_id=?3",
                params![shop,nm,size_id], |r| Ok((r.get(0)?,r.get(1)?)),
            ).optional().map_err(|e| e.to_string())?;
            if let Some((previous_currency, previous)) = prior {
                let old: Value = serde_json::from_str(&previous).map_err(|e| e.to_string())?;
                for (field, current) in &fields {
                    let Some(before) = old.get(field) else {
                        continue;
                    };
                    let monetary = !matches!(field.as_str(), "discount" | "clubDiscount");
                    if before.as_f64() == current.as_f64()
                        && (!monetary || currency == previous_currency)
                    {
                        continue;
                    }
                    let task: Option<i64> = if matches!(
                        field.as_str(),
                        "price" | "discount" | "clubDiscount"
                    ) && currency == previous_currency
                    {
                        c.query_row(
                            "SELECT task FROM wb_price_submissions WHERE shop_id=?1 AND nm_id=?2
                             AND (size_id IS NULL OR size_id=?3) AND field=?4 AND value=?5 AND submitted_at>=?6
                             ORDER BY submitted_at DESC, task DESC LIMIT 1",
                            params![shop,nm,size_id,field,current.as_f64(),observed.timestamp()-172800],
                            |r| r.get(0),
                        ).optional().map_err(|e| e.to_string())?
                    } else {
                        None
                    };
                    let display = |v: &Value, unit: &str| {
                        if monetary {
                            format!("{v} {unit}")
                        } else {
                            format!("{v}%")
                        }
                    };
                    c.execute("INSERT INTO wb_price_history(shop_id,day,observed_at,nm_id,size_id,vendor_code,size_name,field,before,after,task)
                               VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                        params![shop,day,stamp,nm,size_id,g["vendorCode"].as_str().unwrap_or(""),
                            size["techSizeName"].as_str().unwrap_or(""),field,display(before,&previous_currency),display(current,currency),task],
                    ).map_err(|e| e.to_string())?;
                }
                // Some responses omit optional fields. Keep the last observed value
                // for those fields while their currency still matches.
                if currency == previous_currency {
                    if let Some(previous_fields) = old.as_object() {
                        for (key, value) in previous_fields {
                            fields.entry(key.clone()).or_insert_with(|| value.clone());
                        }
                    }
                }
            }
            c.execute("INSERT INTO wb_price_observed VALUES(?1,?2,?3,?4,?5)
                ON CONFLICT(shop_id,nm_id,size_id) DO UPDATE SET currency=excluded.currency,prices=excluded.prices",
                params![shop,nm,size_id,currency,Value::Object(fields).to_string()],
            ).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn history(c: &Connection, shop: &str, payload: &Value) -> Result<Value, String> {
    let day = payload["day"].as_str().unwrap_or("");
    let search = format!("%{}%", payload["query"].as_str().unwrap_or("").trim());
    let page = payload["page"].as_i64().unwrap_or(0).clamp(0, 100000);
    let mut query = c
        .prepare(
            "
        SELECT id,observed_at,nm_id,size_id,vendor_code,size_name,field,before,after,task
        FROM wb_price_history WHERE shop_id=?1 AND (?2='' OR day=?2)
        AND (CAST(nm_id AS TEXT) LIKE ?3 OR vendor_code LIKE ?3)
        ORDER BY id DESC LIMIT 51 OFFSET ?4",
        )
        .map_err(|e| e.to_string())?;
    let mut rows = query
        .query_map(params![shop, day, search, page * 50], |r| {
            Ok(json!({
                "id":r.get::<_,i64>(0)?, "observedAt":r.get::<_,String>(1)?,
                "nmID":r.get::<_,i64>(2)?, "sizeID":r.get::<_,i64>(3)?,
                "vendorCode":r.get::<_,String>(4)?, "sizeName":r.get::<_,String>(5)?,
                "field":r.get::<_,String>(6)?, "before":r.get::<_,String>(7)?,
                "after":r.get::<_,String>(8)?, "task":r.get::<_,Option<i64>>(9)?
            }))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let more = rows.len() > 50;
    rows.truncate(50);
    let today = monitor_day();
    let status: Option<String> = c
        .query_row(
            "SELECT error FROM wb_price_monitor_runs WHERE shop_id=?1 AND day=?2",
            params![shop, today],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(json!({"rows":rows,"more":more,"todayStatus":status}))
}

pub(super) fn start_daily_monitor(state: AppState) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(10));
        loop {
            let _ = monitor_connected_shops(&state);
            std::thread::sleep(Duration::from_secs(1800));
        }
    });
}

fn monitor_connected_shops(state: &AppState) -> Result<(), String> {
    let c = wb_shop_center::db(state)?;
    schema(&c)?;
    let mut query = c
        .prepare("SELECT DISTINCT shop_id FROM wb_api_credentials WHERE status='active'")
        .map_err(|e| e.to_string())?;
    let shops = query
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    drop(query);
    for shop in shops {
        let _guard = PRICE_IO.lock().map_err(|e| e.to_string())?;
        let day = monitor_day();
        let completed: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM wb_price_monitor_runs WHERE shop_id=?1 AND day=?2 AND error='')",
            params![shop,day], |r| r.get(0)).map_err(|e| e.to_string())?;
        if !completed {
            let _ = run_sync(&c, &shop);
        }
    }
    Ok(())
}

fn price_change(payload: &Value) -> Result<(&'static str, Value), String> {
    let nm = payload["nmID"]
        .as_i64()
        .filter(|n| *n > 0)
        .ok_or("商品编号无效")?;
    let mut item = json!({"nmID": nm});
    let endpoint = match payload["field"].as_str() {
        Some("price") => {
            item["price"] = json!(payload["value"]
                .as_i64()
                .filter(|n| (1..=100000000).contains(n))
                .ok_or("售价必须为正整数")?);
            if payload.get("sizeID").is_some() {
                item["sizeID"] = json!(payload["sizeID"]
                    .as_i64()
                    .filter(|n| *n > 0)
                    .ok_or("规格编号无效")?);
                "upload/task/size"
            } else {
                "upload/task"
            }
        }
        Some("discount") => {
            item["discount"] = json!(payload["value"]
                .as_i64()
                .filter(|n| (0..=99).contains(n))
                .ok_or("折扣必须为 0 至 99 的整数")?);
            "upload/task"
        }
        Some("clubDiscount") => {
            item["clubDiscount"] = json!(payload["value"]
                .as_i64()
                .filter(|n| (0..=99).contains(n))
                .ok_or("会员折扣必须为 0 至 99 的整数")?);
            "upload/task/club-discount"
        }
        _ => return Err("请选择要修改的价格字段".into()),
    };
    Ok((endpoint, json!({"data": [item]})))
}

fn sync_catalog(c: &Connection, shop: &str) -> Result<Value, String> {
    let (_, token) = wb_shop_center::active_token(&c, shop)?;
    let mut goods = Vec::new();
    for offset in (0..100000).step_by(1000) {
        let url=format!("https://discounts-prices-api.wildberries.cn/api/v2/list/goods/filter?limit=1000&offset={offset}");
        let response_text = ureq::get(&url)
            .set("Authorization", &token)
            .timeout(std::time::Duration::from_secs(40))
            .call()
            .map_err(|e| format!("读取 WB 价格失败：{e}"))?
            .into_string()
            .map_err(|e| e.to_string())?;
        let response: Value = serde_json::from_str(&response_text).map_err(|e| e.to_string())?;
        if response["error"].as_bool() == Some(true) {
            return Err(response["errorText"].to_string());
        }
        let page = response["data"]["listGoods"]
            .as_array()
            .ok_or("WB 商品响应格式异常")?;
        goods.extend(page.iter().cloned());
        if page.len() < 1000 {
            let tx = c.unchecked_transaction().map_err(|e| e.to_string())?;
            observe(&tx, shop, &goods)?;
            let result = json!({"goods":goods,"updatedAt":chrono::Utc::now().to_rfc3339()});
            tx.execute("INSERT INTO wb_pricing_state VALUES(?1,'catalog',?2) ON CONFLICT(shop_id,kind) DO UPDATE SET value=excluded.value",params![shop,result.to_string()]).map_err(|e|e.to_string())?;
            tx.execute(
                "INSERT INTO wb_price_monitor_runs VALUES(?1,?2,'')
                        ON CONFLICT(shop_id,day) DO UPDATE SET error=''",
                params![shop, monitor_day()],
            )
            .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
            return Ok(result);
        }
        std::thread::sleep(std::time::Duration::from_millis(650));
    }
    Err("商品超过单次同步上限，原有缓存已保留".into())
}

fn run_sync(c: &Connection, shop: &str) -> Result<Value, String> {
    let result = sync_catalog(c, shop);
    if let Err(error) = &result {
        c.execute("INSERT INTO wb_price_monitor_runs VALUES(?1,?2,?3)
            ON CONFLICT(shop_id,day) DO UPDATE SET error=excluded.error WHERE wb_price_monitor_runs.error<>''",
            params![shop,monitor_day(),error],
        ).map_err(|e| e.to_string())?;
    }
    result
}

#[tauri::command]
pub async fn wb_pricing(
    action: String,
    payload: Value,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let state = crate::background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || run(&state, &action, payload))
        .await
        .map_err(|e| e.to_string())?
}

fn run(state: &AppState, action: &str, payload: Value) -> Result<Value, String> {
    let shop = payload["shopId"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("请选择 WB 店铺")?;
    let _guard = matches!(action, "sync" | "read" | "submit")
        .then(|| PRICE_IO.lock().map_err(|e| e.to_string()))
        .transpose()?;
    let c = wb_shop_center::db(state)?;
    schema(&c)?;
    match action {
        "load" => {
            let mut q = c
                .prepare("SELECT kind,value FROM wb_pricing_state WHERE shop_id=?1")
                .map_err(|e| e.to_string())?;
            let rows = q
                .query_map([shop], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })
                .map_err(|e| e.to_string())?;
            let mut result = json!({});
            for row in rows {
                let (key, value) = row.map_err(|e| e.to_string())?;
                result[key] = serde_json::from_str(&value).map_err(|e| e.to_string())?;
            }
            Ok(result)
        }
        "save" => {
            c.execute("INSERT INTO wb_pricing_state VALUES(?1,'costs',?2) ON CONFLICT(shop_id,kind) DO UPDATE SET value=excluded.value",params![shop,payload["costs"].to_string()]).map_err(|e|e.to_string())?;
            Ok(json!({}))
        }
        "sync" => run_sync(&c, shop),
        "history" => history(&c, shop, &payload),
        "read" => {
            let nm = payload["nmID"]
                .as_i64()
                .filter(|n| *n > 0)
                .ok_or("商品编号无效")?;
            let (_, token) = wb_shop_center::active_token(&c, shop)?;
            let response = ureq::get(&format!(
                "{PRICES_API}/list/goods/filter?limit=1&offset=0&filterNmID={nm}"
            ))
            .set("Authorization", &token)
            .timeout(std::time::Duration::from_secs(40))
            .call()
            .map_err(|e| format!("读取 WB 当前价格失败：{e}"))?
            .into_string()
            .map_err(|e| e.to_string())?;
            let response: Value = serde_json::from_str(&response).map_err(|e| e.to_string())?;
            let product = response["data"]["listGoods"]
                .as_array()
                .and_then(|goods| goods.iter().find(|item| item["nmID"].as_i64() == Some(nm)))
                .cloned()
                .ok_or_else(|| format!("WB 未返回商品 {nm} 的价格"))?;
            let tx = c.unchecked_transaction().map_err(|e| e.to_string())?;
            observe(&tx, shop, std::slice::from_ref(&product))?;
            tx.commit().map_err(|e| e.to_string())?;
            Ok(product)
        }
        "submit" => {
            let (endpoint, body) = price_change(&payload)?;
            let (_, token) = wb_shop_center::active_token(&c, shop)?;
            let response_text = ureq::post(&format!("{PRICES_API}/{endpoint}"))
                .set("Authorization", &token)
                .timeout(std::time::Duration::from_secs(40))
                .set("Content-Type", "application/json")
                .send_string(&body.to_string())
                .map_err(|e| format!("改价响应异常，请先在 WB 后台核对，避免重复提交：{e}"))?
                .into_string()
                .map_err(|e| e.to_string())?;
            let response: Value =
                serde_json::from_str(&response_text).map_err(|e| e.to_string())?;
            if response["error"].as_bool() == Some(true) {
                return Err(response["errorText"].to_string());
            }
            if let Some(task) = response["data"]["id"].as_i64().filter(|id| *id > 0) {
                let tx = c.unchecked_transaction().map_err(|e| e.to_string())?;
                tx.execute(
                    "INSERT OR IGNORE INTO wb_price_submissions VALUES(?1,?2,?3,?4,?5,?6,?7)",
                    params![
                        shop,
                        task,
                        payload["nmID"].as_i64(),
                        payload["sizeID"].as_i64(),
                        payload["field"].as_str(),
                        payload["value"].as_i64(),
                        chrono::Utc::now().timestamp()
                    ],
                )
                .map_err(|e| format!("WB 已接收，保存本地任务失败：{e}"))?;
                tx.execute("INSERT INTO wb_pricing_state VALUES(?1,'lastTask',?2) ON CONFLICT(shop_id,kind) DO UPDATE SET value=excluded.value",params![shop,response.to_string()]).map_err(|e|format!("WB 已接收，保存本地任务失败：{e}"))?;
                tx.commit()
                    .map_err(|e| format!("WB 已接收，保存本地任务失败：{e}"))?;
            }
            Ok(response)
        }
        "status" => {
            let id = payload["uploadID"]
                .as_i64()
                .filter(|n| *n > 0)
                .ok_or("任务编号无效")?;
            let (_, token) = wb_shop_center::active_token(&c, shop)?;
            let response_text=ureq::get(&format!("https://discounts-prices-api.wildberries.cn/api/v2/history/goods/task?uploadID={id}&limit=1000&offset=0")).set("Authorization",&token).timeout(std::time::Duration::from_secs(40)).call().map_err(|e|format!("查询任务失败，任务可能仍在处理中：{e}"))?.into_string().map_err(|e|e.to_string())?;
            let response: Value =
                serde_json::from_str(&response_text).map_err(|e| e.to_string())?;
            Ok(response)
        }
        _ => Err("不支持的 WB 核价操作".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn price_history_tracks_changes_and_matches_only_the_requested_shop_and_size() {
        let c = Connection::open_in_memory().unwrap();
        schema(&c).unwrap();
        let product = |price: i64| {
            json!({
                "nmID":42,"vendorCode":"ITEM-42","currencyIsoCode4217":"CNY","discount":10,
                "sizes":[{"sizeID":1,"price":price},{"sizeID":2,"price":price}]
            })
        };
        observe(&c, "a", &[product(100)]).unwrap();
        observe(&c, "b", &[product(100)]).unwrap();
        assert_eq!(history(&c, "a", &json!({})).unwrap()["rows"], json!([]));
        c.execute(
            "INSERT INTO wb_price_submissions VALUES('a',77,42,1,'price',120,?1)",
            [chrono::Utc::now().timestamp()],
        )
        .unwrap();
        observe(&c, "a", &[product(120)]).unwrap();
        observe(&c, "a", &[product(120)]).unwrap();
        observe(&c, "b", &[product(120)]).unwrap();
        let result = history(&c, "a", &json!({"day":monitor_day(),"query":"ITEM-42"})).unwrap();
        let rows = result["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["sizeID"], 2);
        assert!(rows[0]["task"].is_null());
        assert_eq!(rows[1]["task"], 77);
        assert_eq!(rows[1]["before"], "100 CNY");
        assert_eq!(rows[1]["after"], "120 CNY");
        let result = history(&c, "b", &json!({})).unwrap();
        assert!(result["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["task"].is_null()));
    }

    #[test]
    fn price_changes_use_the_matching_endpoint_and_only_send_the_edited_field() {
        for (field, endpoint, size) in [
            ("price", "upload/task", None),
            ("price", "upload/task/size", Some(123)),
            ("discount", "upload/task", None),
            ("clubDiscount", "upload/task/club-discount", None),
        ] {
            let mut payload = json!({"nmID": 42, "field": field, "value": 10});
            let mut item = json!({"nmID": 42});
            item[field] = json!(10);
            if let Some(size) = size {
                payload["sizeID"] = json!(size);
                item["sizeID"] = json!(size);
            }
            assert_eq!(
                price_change(&payload).unwrap(),
                (endpoint, json!({"data": [item]}))
            );
        }
        assert!(price_change(&json!({"nmID":42,"field":"price","value":0})).is_err());
        assert!(price_change(&json!({"nmID":42,"field":"discount","value":100})).is_err());
    }
}
