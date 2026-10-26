use crate::{wb_shop_center, AppState};
use chrono::{DateTime, FixedOffset, NaiveDate, Utc};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    sync::{Mutex, OnceLock},
    time::Duration,
};
use tauri::State;

static SYNCING: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
fn schema(c: &Connection) -> Result<(), String> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS wb_ops_rows(shop_id TEXT NOT NULL,kind TEXT NOT NULL,id TEXT NOT NULL,raw TEXT NOT NULL,local TEXT NOT NULL DEFAULT '{}',updated_at TEXT NOT NULL,PRIMARY KEY(shop_id,kind,id));
    CREATE TABLE IF NOT EXISTS wb_ops_sync(shop_id TEXT PRIMARY KEY,running INTEGER NOT NULL DEFAULT 0,message TEXT NOT NULL DEFAULT '',updated_at TEXT NOT NULL);").map_err(|e|e.to_string())
}
fn rows(c: &Connection, shop: &str, kind: &str) -> Result<Vec<Value>, String> {
    let mut q=c.prepare("SELECT id,raw,local,updated_at FROM wb_ops_rows WHERE shop_id=?1 AND kind=?2 ORDER BY id").map_err(|e|e.to_string())?;
    let result=q.query_map(params![shop,kind],|r|Ok(json!({"id":r.get::<_,String>(0)?,"raw":serde_json::from_str::<Value>(&r.get::<_,String>(1)?).unwrap_or(Value::Null),"local":serde_json::from_str::<Value>(&r.get::<_,String>(2)?).unwrap_or(json!({})),"updatedAt":r.get::<_,String>(3)?}))).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
    Ok(result)
}
fn put(c: &Connection, shop: &str, kind: &str, id: &str, raw: &Value) -> Result<(), String> {
    if id.is_empty() {
        return Err(format!("{kind} 响应缺少唯一编号，未保存"));
    }
    c.execute("INSERT INTO wb_ops_rows(shop_id,kind,id,raw,updated_at) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(shop_id,kind,id) DO UPDATE SET raw=excluded.raw,updated_at=excluded.updated_at",params![shop,kind,id,raw.to_string(),Utc::now().to_rfc3339()]).map_err(|e|e.to_string())?;
    Ok(())
}
fn text(v: &Value) -> String {
    v.as_str().map(str::to_owned).unwrap_or_else(|| {
        if v.is_number() {
            v.to_string()
        } else {
            String::new()
        }
    })
}
fn order_status(supplier: &str, wb: &str, prepared: bool) -> &'static str {
    if matches!(wb, "sold" | "delivered" | "received") || supplier == "receive" {
        "received"
    } else if matches!(wb, "canceled_by_client" | "defect") || supplier == "reject" {
        "returning"
    } else if supplier.starts_with("cancel") || wb.contains("cancel") || wb == "declined_by_client"
    {
        "cancelled"
    } else if wb == "ready_for_pickup" {
        "awaiting_receipt"
    } else if matches!(supplier, "complete" | "deliver")
        || matches!(
            wb,
            "sorted" | "accepted_by_carrier" | "sent_to_carrier" | "postponed_delivery"
        )
    {
        "delivering"
    } else if prepared {
        "prepared"
    } else if matches!(supplier, "new" | "confirm") {
        "awaiting_preparation"
    } else {
        "unknown"
    }
}
fn moscow_day(s: &str) -> String {
    DateTime::parse_from_rfc3339(s)
        .map(|d| {
            d.with_timezone(&FixedOffset::east_opt(10800).unwrap())
                .date_naive()
                .to_string()
        })
        .unwrap_or_else(|_| s.chars().take(10).collect())
}
fn return_state(raw: &Value, local: &Value) -> &'static str {
    if local["collected"].as_bool() == Some(true)
        || raw["completedDt"].as_str().is_some_and(|s| !s.is_empty())
    {
        "collected"
    } else if raw["expiredDt"].as_str().is_some_and(|s| {
        !s.is_empty()
            && s.get(..10)
                .is_some_and(|day| day < Utc::now().date_naive().to_string().as_str())
    }) {
        "expired"
    } else if local["ready"].as_bool() == Some(true)
        || raw["readyToReturnDt"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
    {
        "pickup"
    } else if raw["_source"] == "sales_return" {
        "buyer_return"
    } else if raw["_source"] == "claim" {
        "application"
    } else {
        "returning"
    }
}
fn snapshot(c: &Connection, shop: &str) -> Result<Value, String> {
    let warehouses = rows(c, shop, "warehouse")?;
    let orders = rows(c, shop, "order")?
        .into_iter()
        .map(|mut r| {
            r["status"] = json!(order_status(
                r["raw"]["supplierStatus"].as_str().unwrap_or(""),
                r["raw"]["wbStatus"].as_str().unwrap_or(""),
                r["local"]["prepared"].as_bool() == Some(true)
            ));
            r["day"] = json!(moscow_day(r["raw"]["createdAt"].as_str().unwrap_or("")));
            r
        })
        .collect::<Vec<_>>();
    let returns = rows(c, shop, "return")?
        .into_iter()
        .map(|mut r| {
            let matched = orders.iter().find(|o| {
                let srid = text(&r["raw"]["srid"]);
                let oid = text(&r["raw"]["orderId"]);
                (!srid.is_empty() && srid == text(&o["raw"]["rid"]))
                    || (!oid.is_empty() && oid == text(&o["raw"]["id"]))
            });
            r["warehouseId"] = if !text(&r["local"]["warehouseId"]).is_empty() {
                r["local"]["warehouseId"].clone()
            } else {
                matched
                    .map(|o| o["raw"]["warehouseId"].clone())
                    .unwrap_or(Value::Null)
            };
            r["status"] = json!(return_state(&r["raw"], &r["local"]));
            r["day"] = json!(moscow_day(
                r["raw"]["dt"]
                    .as_str()
                    .or(r["raw"]["date"].as_str())
                    .or(r["raw"]["orderDt"].as_str())
                    .or(r["raw"]["readyToReturnDt"].as_str())
                    .unwrap_or("")
            ));
            r
        })
        .collect::<Vec<_>>();
    let sync = c
        .query_row(
            "SELECT message,updated_at FROM wb_ops_sync WHERE shop_id=?1",
            [shop],
            |r| Ok(json!({"message":r.get::<_,String>(0)?,"updatedAt":r.get::<_,String>(1)?})),
        )
        .unwrap_or(json!({}));
    let running = SYNCING
        .get_or_init(Default::default)
        .lock()
        .map_err(|e| e.to_string())?
        .contains(shop);
    Ok(
        json!({"warehouses":warehouses,"orders":orders,"returns":returns,"sync":sync,"running":running}),
    )
}
fn request(token: &str, host: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(40))
        .build();
    let url = format!("https://{host}.wildberries.ru{path}");
    let response = if let Some(body) = body {
        agent
            .post(&url)
            .set("Authorization", token)
            .set("Content-Type", "application/json")
            .send_string(&body.to_string())
    } else {
        agent.get(&url).set("Authorization", token).call()
    };
    match response {
        Ok(r) if r.status() == 204 => Ok(json!({})),
        Ok(r) => serde_json::from_reader(r.into_reader())
            .map_err(|e| format!("{path}: 无法解析响应：{e}")),
        Err(ureq::Error::Status(code, _)) => Err(format!(
            "{path}: HTTP {code}（检查 Token 权限、接口可用性或请求频率）"
        )),
        Err(_) => Err(format!("{path}: 网络连接失败")),
    }
}
fn array(v: &Value, key: &str) -> Result<Vec<Value>, String> {
    v.get(key)
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| format!("平台响应缺少 {key} 数组，未作为空数据处理"))
}
fn sync_orders(
    c: &Connection,
    shop: &str,
    token: &str,
    model: &str,
    from: i64,
    to: i64,
) -> Result<(), String> {
    let prefix = if model == "dbs" {
        "/api/v3/dbs/orders"
    } else {
        "/api/v3/orders"
    };
    let mut fetched = Vec::new();
    let mut next = 0;
    loop {
        let v = request(
            token,
            "marketplace-api",
            &format!("{prefix}?limit=1000&next={next}&dateFrom={from}&dateTo={to}"),
            None,
        )?;
        let page = array(&v, "orders")?;
        let empty = page.is_empty();
        fetched.extend(page);
        let cursor = v["next"].as_i64().unwrap_or(0);
        if empty || cursor == 0 {
            break;
        }
        if cursor == next {
            return Err(format!("{model} 分页游标未前进"));
        }
        next = cursor;
        std::thread::sleep(Duration::from_millis(250));
    }
    std::thread::sleep(Duration::from_millis(250));
    fetched.extend(array(
        &request(token, "marketplace-api", &format!("{prefix}/new"), None)?,
        "orders",
    )?);
    let mut merged = std::collections::BTreeMap::new();
    // Keep older unfinished orders in status polling even outside the selected creation period.
    for r in rows(c, shop, "order")? {
        if r["raw"]["_model"] == model
            && !matches!(
                order_status(
                    r["raw"]["supplierStatus"].as_str().unwrap_or(""),
                    r["raw"]["wbStatus"].as_str().unwrap_or(""),
                    false
                ),
                "received" | "cancelled"
            )
        {
            merged.insert(text(&r["raw"]["id"]), r["raw"].clone());
        }
    }
    for mut r in fetched {
        let id = text(&r["id"]);
        if id.is_empty() {
            return Err("订单响应缺少 id".into());
        }
        r["_model"] = json!(model);
        merged.insert(id, r);
    }
    let ids = merged.keys().cloned().collect::<Vec<_>>();
    for chunk in ids.chunks(1000) {
        std::thread::sleep(Duration::from_millis(250));
        let payload = json!({"orders":chunk.iter().map(|s|s.parse::<i64>()).collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?});
        let path = if model == "dbs" {
            "/api/v3/dbs/orders/status/info"
        } else {
            "/api/v3/orders/status"
        };
        let v = match request(token, "marketplace-api", path, Some(payload.clone())) {
            Err(e) if model == "dbs" && e.contains("HTTP 404") => request(
                token,
                "marketplace-api",
                "/api/marketplace/v3/dbs/orders/status/info",
                Some(payload),
            )?,
            other => other?,
        };
        let statuses = v
            .get("orders")
            .or(v.get("data"))
            .and_then(Value::as_array)
            .ok_or("状态响应缺少 orders/data 数组")?;
        for s in statuses {
            let id = text(s.get("id").or(s.get("orderId")).unwrap_or(&Value::Null));
            if let Some(r) = merged.get_mut(&id) {
                r["supplierStatus"] = s["supplierStatus"].clone();
                r["wbStatus"] = s["wbStatus"].clone();
                r["_statusAt"] = json!(Utc::now().to_rfc3339());
            }
        }
    }
    let tx = c.unchecked_transaction().map_err(|e| e.to_string())?;
    for (id, r) in merged {
        put(&tx, shop, "order", &format!("{model}:{id}"), &r)?;
    }
    tx.commit().map_err(|e| e.to_string())
}
fn sync_returns(
    c: &Connection,
    shop: &str,
    token: &str,
    from: &str,
    to: &str,
) -> Result<(), String> {
    let mut offset = 0;
    loop {
        let v=request(token,"seller-analytics-api",&format!("/api/analytics/v1/item-returns?dateFrom={from}&dateTo={to}&limit=1000&offset={offset}"),None)?;
        if v == json!({}) {
            break;
        }
        let page = array(&v, "report")?;
        let count = page.len();
        ingest_item_returns(c, shop, &page)?;
        offset += count;
        if count == 0 || offset >= v["count"].as_u64().unwrap_or(offset as u64) as usize {
            break;
        }
        // Analytics item-returns has a one-minute request interval.
        std::thread::sleep(Duration::from_secs(60));
    }
    Ok(())
}
fn ingest_item_returns(c: &Connection, shop: &str, page: &[Value]) -> Result<(), String> {
    for event in page {
        let mut r = event.clone();
        r["_source"] = json!("item_return");
        if text(&r["shkId"]).is_empty() {
            return Err("退货响应缺少 shkId".into());
        }
        let id = format!("item:{}:{}", text(&r["shkId"]), text(&r["orderId"]));
        put(c, shop, "return", &id, &r)?;
    }
    Ok(())
}
fn ingest_sales_returns(c: &Connection, shop: &str, response: &Value) -> Result<usize, String> {
    let events = response
        .as_array()
        .ok_or("销售/退货接口响应不是数组，未作为空数据处理")?;
    let mut count = 0;
    for event in events {
        let id = text(&event["saleID"]);
        // S events are purchases, R events are actual buyer returns. Cancellations are not returns.
        if !id.starts_with('R') {
            continue;
        }
        let mut raw = event.clone();
        raw["_source"] = json!("sales_return");
        put(c, shop, "return", &format!("sale:{id}"), &raw)?;
        count += 1;
    }
    Ok(count)
}
fn sync_sales_returns(c: &Connection, shop: &str, token: &str, from: &str) -> Result<(), String> {
    let mut cursor = from.to_string();
    loop {
        let response = request(
            token,
            "statistics-api",
            &format!("/api/v1/supplier/sales?dateFrom={cursor}&flag=0"),
            None,
        )?;
        let page = response.as_array().ok_or("销售/退货接口响应不是数组")?;
        if page.is_empty() {
            break;
        }
        ingest_sales_returns(c, shop, &response)?;
        let next = page
            .last()
            .and_then(|r| r["lastChangeDate"].as_str())
            .ok_or("销售/退货响应缺少 lastChangeDate，已保存返回的退货但无法继续分页")?;
        if next <= cursor.as_str() {
            return Err("销售/退货分页游标未前进，已保存返回的退货记录".into());
        }
        cursor = next.to_string();
        std::thread::sleep(Duration::from_secs(60));
    }
    Ok(())
}
fn return_backfill_period(today: NaiveDate) -> (String, String) {
    (
        (today - chrono::Duration::days(89)).to_string(),
        today.to_string(),
    )
}
fn sync_claims(c: &Connection, shop: &str, token: &str) -> Result<(), String> {
    for archive in [false, true] {
        let mut offset = 0;
        loop {
            let v = request(
                token,
                "returns-api",
                &format!("/api/v1/claims?is_archive={archive}&limit=200&offset={offset}"),
                None,
            )?;
            let page = array(&v, "claims")?;
            let count = page.len();
            for mut r in page {
                let id = text(&r["id"]);
                if id.is_empty() {
                    return Err("退货申请缺少唯一编号".into());
                }
                r["_source"] = json!("claim");
                put(c, shop, "return", &format!("claim:{id}"), &r)?;
            }
            offset += count;
            std::thread::sleep(Duration::from_secs(3));
            if count == 0 || offset >= v["total"].as_u64().unwrap_or(offset as u64) as usize {
                break;
            }
        }
    }
    Ok(())
}
fn sync(c: &Connection, shop: &str, token: &str, from: &str, to: &str) -> Result<(), String> {
    let start = NaiveDate::parse_from_str(from, "%Y-%m-%d").map_err(|_| "开始日期无效")?;
    let end = NaiveDate::parse_from_str(to, "%Y-%m-%d").map_err(|_| "结束日期无效")?;
    if end < start || (end - start).num_days() > 29 {
        return Err("每次请选择 1–30 天".into());
    }
    let zone = FixedOffset::east_opt(10800).unwrap();
    let from_ts = start
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_local_timezone(zone)
        .unwrap()
        .timestamp();
    let to_ts = end
        .and_hms_opt(23, 59, 59)
        .unwrap()
        .and_local_timezone(zone)
        .unwrap()
        .timestamp();
    let (return_from, return_to) =
        return_backfill_period(Utc::now().with_timezone(&zone).date_naive());
    let mut log = vec![format!(
        "订单创建日期：{from} 至 {to}；退货独立回补：{return_from} 至 {return_to}（近90天）"
    )];
    for resource in [
        "仓库",
        "FBS 订单",
        "DBS 订单",
        "退货件",
        "买家退货",
        "退货申请",
    ] {
        c.execute("INSERT INTO wb_ops_sync(shop_id,running,message,updated_at) VALUES(?1,1,?2,?3) ON CONFLICT(shop_id) DO UPDATE SET running=1,message=excluded.message,updated_at=excluded.updated_at",params![shop,format!("{}\n正在读取{resource}…",log.join("\n")),Utc::now().to_rfc3339()]).map_err(|e|e.to_string())?;
        let result = match resource {
            "仓库" => {
                request(token, "marketplace-api", "/api/v3/warehouses", None).and_then(|v| {
                    let ws = v.as_array().ok_or("仓库响应不是数组")?;
                    for r in ws {
                        put(c, shop, "warehouse", &text(&r["id"]), r)?;
                    }
                    Ok(())
                })
            }
            "FBS 订单" => sync_orders(c, shop, token, "fbs", from_ts, to_ts),
            "DBS 订单" => sync_orders(c, shop, token, "dbs", from_ts, to_ts),
            "退货件" => sync_returns(c, shop, token, &return_from, &return_to),
            "买家退货" => sync_sales_returns(c, shop, token, &return_from),
            _ => sync_claims(c, shop, token),
        };
        log.push(match result {
            Ok(()) => {
                let (kind, source) = match resource {
                    "仓库" => ("warehouse", None),
                    "FBS 订单" => ("order", Some(("_model", "fbs"))),
                    "DBS 订单" => ("order", Some(("_model", "dbs"))),
                    "退货件" => ("return", Some(("_source", "item_return"))),
                    "买家退货" => ("return", Some(("_source", "sales_return"))),
                    _ => ("return", Some(("_source", "claim"))),
                };
                let count = rows(c, shop, kind)?
                    .iter()
                    .filter(|r| source.map(|(key, v)| r["raw"][key] == v).unwrap_or(true))
                    .count();
                format!("{resource}：已同步，缓存 {count} 条")
            }
            Err(e) => format!("{resource}：{e}"),
        });
    }
    c.execute(
        "UPDATE wb_ops_sync SET running=0,message=?2,updated_at=?3 WHERE shop_id=?1",
        params![shop, log.join("\n"), Utc::now().to_rfc3339()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
#[tauri::command]
pub async fn wb_operations(
    action: String,
    payload: Value,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let c = wb_shop_center::db(&state)?;
        schema(&c)?;
        let shop = payload["shopId"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("请先选择 WB 店铺")?;
        let exists: bool = c
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM wb_shops WHERE id=?1)",
                [shop],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if !exists {
            return Err("店铺不存在".into());
        }
        match action.as_str() {
            "list" => snapshot(&c, shop),
            "sync" => {
                let (_, token) = wb_shop_center::active_token(&c, shop)?;
                let inserted = SYNCING
                    .get_or_init(Default::default)
                    .lock()
                    .map_err(|e| e.to_string())?
                    .insert(shop.to_string());
                if !inserted {
                    return Err("该店铺正在同步，请稍候".into());
                }
                let result = sync(
                    &c,
                    shop,
                    &token,
                    payload["from"].as_str().unwrap_or(""),
                    payload["to"].as_str().unwrap_or(""),
                );
                SYNCING
                    .get_or_init(Default::default)
                    .lock()
                    .map_err(|e| e.to_string())?
                    .remove(shop);
                result?;
                snapshot(&c, shop)
            }
            "save_local" => {
                if !matches!(
                    std::env::var("WBERP_LOCAL_ROLE")
                        .unwrap_or_else(|_| "admin".into())
                        .as_str(),
                    "admin" | "operator"
                ) {
                    return Err("当前角色不能修改本地履约记录".into());
                }
                let kind = payload["kind"].as_str().ok_or("缺少类型")?;
                if !matches!(kind, "order" | "return") {
                    return Err("不允许修改该类型".into());
                }
                let id = payload["id"].as_str().ok_or("缺少编号")?;
                let local = payload["local"].as_object().ok_or("缺少操作内容")?;
                if kind == "order" && local.keys().any(|k| k != "prepared") {
                    return Err("订单只支持本地备货标记".into());
                }
                let wh = local.get("warehouseId").map(text).unwrap_or_default();
                if !wh.is_empty()
                    && !rows(&c, shop, "warehouse")?
                        .iter()
                        .any(|w| text(&w["raw"]["id"]) == wh)
                {
                    return Err("仓库不属于当前店铺".into());
                }
                if local.get("due").and_then(Value::as_str).is_some_and(|s| {
                    !s.is_empty() && NaiveDate::parse_from_str(s, "%Y-%m-%d").is_err()
                }) {
                    return Err("取件日期无效".into());
                }
                let changed = c
                    .execute(
                        "UPDATE wb_ops_rows SET local=?4 WHERE shop_id=?1 AND kind=?2 AND id=?3",
                        params![shop, kind, id, Value::Object(local.clone()).to_string()],
                    )
                    .map_err(|e| e.to_string())?;
                if changed != 1 {
                    return Err("记录不存在，未保存".into());
                }
                snapshot(&c, shop)
            }
            _ => Err("未知 WB 工作台操作".into()),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn platform_status_has_priority() {
        assert_eq!(order_status("complete", "sold", true), "received");
        assert_eq!(
            order_status("complete", "ready_for_pickup", true),
            "awaiting_receipt"
        );
        assert_eq!(order_status("confirm", "waiting", true), "prepared");
        assert_eq!(
            order_status("new", "waiting", false),
            "awaiting_preparation"
        );
        assert_eq!(order_status("deliver", "", false), "delivering");
        assert_eq!(order_status("new", "defect", false), "returning");
        assert_eq!(order_status("", "new_future_status", false), "unknown");
    }
    #[test]
    fn claims_are_not_pickup_notifications() {
        assert_eq!(
            return_state(&json!({"_source":"claim","status":2}), &json!({})),
            "application"
        );
        assert_eq!(
            return_state(&json!({"readyToReturnDt":"2026-10-24"}), &json!({})),
            "pickup"
        );
        assert_eq!(
            return_state(
                &json!({"readyToReturnDt":"2026-10-24","completedDt":"2026-10-25"}),
                &json!({})
            ),
            "collected"
        );
    }
    #[test]
    fn upsert_keeps_local_data_and_shop_isolation() {
        let c = Connection::open_in_memory().unwrap();
        schema(&c).unwrap();
        put(&c, "a", "order", "fbs:1", &json!({"id":1})).unwrap();
        c.execute(
            "UPDATE wb_ops_rows SET local=?1",
            [json!({"prepared":true}).to_string()],
        )
        .unwrap();
        put(
            &c,
            "a",
            "order",
            "fbs:1",
            &json!({"id":1,"wbStatus":"sold"}),
        )
        .unwrap();
        assert_eq!(
            rows(&c, "a", "order").unwrap()[0]["local"]["prepared"],
            true
        );
        assert!(rows(&c, "b", "order").unwrap().is_empty());
    }
    #[test]
    fn return_links_are_exact_and_preparation_is_local() {
        let c = Connection::open_in_memory().unwrap();
        schema(&c).unwrap();
        put(&c,"a","order","fbs:7",&json!({"id":7,"rid":"exact-rid","warehouseId":11,"supplierStatus":"new","wbStatus":"waiting","createdAt":"2026-10-23T22:30:00Z"})).unwrap();
        put(
            &c,
            "a",
            "return",
            "item:1",
            &json!({"srid":"exact-rid","readyToReturnDt":"2026-10-24"}),
        )
        .unwrap();
        put(
            &c,
            "a",
            "return",
            "item:2",
            &json!({"srid":"different-rid","nm_id":123}),
        )
        .unwrap();
        put(
            &c,
            "b",
            "order",
            "fbs:8",
            &json!({"id":8,"rid":"different-rid","warehouseId":99}),
        )
        .unwrap();
        let d = snapshot(&c, "a").unwrap();
        assert_eq!(d["returns"][0]["warehouseId"], 11);
        assert!(d["returns"][1]["warehouseId"].is_null());
        assert_eq!(d["orders"][0]["day"], "2026-10-24");
        assert_eq!(d["orders"][0]["status"], "awaiting_preparation");
        c.execute(
            "UPDATE wb_ops_rows SET local=?1 WHERE id='item:2'",
            [json!({"warehouseId":"22","ready":true,"collected":true}).to_string()],
        )
        .unwrap();
        let d = snapshot(&c, "a").unwrap();
        assert_eq!(d["returns"][1]["warehouseId"], "22");
        assert_eq!(d["returns"][1]["status"], "collected");
    }
    #[test]
    fn future_deadlines_are_not_already_expired() {
        assert_eq!(
            return_state(
                &json!({"readyToReturnDt":"2026-10-24","expiredDt":"2099-01-01T00:00:00"}),
                &json!({})
            ),
            "pickup"
        );
        assert_eq!(order_status("receive", "", false), "received");
        assert_eq!(order_status("reject", "", false), "returning");
        assert!(array(&json!({"error":"unauthorized"}), "orders").is_err());
    }
    #[test]
    fn buyer_returns_do_not_become_pickup_and_are_idempotent() {
        let c = Connection::open_in_memory().unwrap();
        schema(&c).unwrap();
        let data = json!([{"saleID":"S1","date":"2026-10-12T12:26:18"},{"saleID":"R2","date":"2026-10-12T12:26:18","lastChangeDate":"2026-10-12T12:41:51","srid":"buyer-order"}]);
        assert_eq!(ingest_sales_returns(&c, "a", &data).unwrap(), 1);
        ingest_sales_returns(&c, "a", &data).unwrap();
        let d = snapshot(&c, "a").unwrap();
        assert_eq!(d["returns"].as_array().unwrap().len(), 1);
        assert_eq!(d["returns"][0]["status"], "buyer_return");
        assert_eq!(d["returns"][0]["day"], "2026-10-12");
        assert_eq!(d["returns"][0]["raw"]["srid"], "buyer-order");
        assert!(ingest_sales_returns(&c, "a", &json!({"error":1})).is_err());
    }
    #[test]
    fn return_window_is_independent_of_the_order_week() {
        let (from, to) = return_backfill_period(NaiveDate::from_ymd_opt(2026, 10, 24).unwrap());
        assert_eq!(from, "2026-07-27");
        assert_eq!(to, "2026-10-24");
        assert!(from.as_str() < "2026-09-24");
        assert!(from.as_str() < "2026-10-02");
        assert!(from.as_str() < "2026-10-12");
    }
    #[test]
    #[ignore = "requires a locally saved authenticated WB response; does not call network or write production DB"]
    fn authenticated_return_fixture_contains_reported_dates() {
        let path = std::env::var("WB_LIVE_RETURNS_FIXTURE").unwrap();
        let data: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let c = Connection::open_in_memory().unwrap();
        schema(&c).unwrap();
        ingest_item_returns(
            &c,
            "fixture",
            data["resources"]["item_returns"].as_array().unwrap(),
        )
        .unwrap();
        ingest_sales_returns(&c, "fixture", &data["resources"]["sales_returns"]).unwrap();
        let d = snapshot(&c, "fixture").unwrap();
        let rs = d["returns"].as_array().unwrap();
        for day in ["2026-09-24", "2026-10-02", "2026-10-12"] {
            assert!(rs
                .iter()
                .any(|r| r["raw"]["_source"] == "sales_return" && r["day"] == day));
        }
        assert_eq!(
            rs.iter()
                .filter(|r| r["raw"]["_source"] == "sales_return")
                .count(),
            8
        );
        assert!(rs
            .iter()
            .filter(|r| r["raw"]["_source"] == "sales_return")
            .all(|r| r["status"] == "buyer_return"));
    }
    #[test]
    fn business_day_is_moscow() {
        assert_eq!(moscow_day("2026-10-23T22:30:00Z"), "2026-10-24");
    }
}
