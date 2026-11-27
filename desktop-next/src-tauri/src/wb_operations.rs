use crate::{wb_shop_center, AppState};
use chrono::{DateTime, FixedOffset, NaiveDate, Utc};
use rusqlite::{params, Connection, OptionalExtension};
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
    CREATE TABLE IF NOT EXISTS wb_ops_ads(shop_id TEXT NOT NULL,day TEXT NOT NULL,spend REAL NOT NULL,attributed_sales REAL NOT NULL,PRIMARY KEY(shop_id,day)); CREATE TABLE IF NOT EXISTS wb_ops_sync(shop_id TEXT PRIMARY KEY,running INTEGER NOT NULL DEFAULT 0,message TEXT NOT NULL DEFAULT '',updated_at TEXT NOT NULL);").map_err(|e|e.to_string())
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
pub(super) fn request(
    token: &str,
    host: &str,
    path: &str,
    body: Option<Value>,
) -> Result<Value, String> {
    request_with_method(
        token,
        host,
        path,
        if body.is_some() { "POST" } else { "GET" },
        body,
    )
}

pub(super) fn request_with_method(
    token: &str,
    host: &str,
    path: &str,
    method: &str,
    body: Option<Value>,
) -> Result<Value, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(40))
        .build();
    let url = format!("https://{host}.wildberries.ru{path}");
    let response = if let Some(body) = body {
        agent
            .request(method, &url)
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
fn order_path(model: &str) -> &'static str {
    if model == "dbs" {
        "/api/v3/dbs/orders"
    } else {
        "/api/v3/orders"
    }
}
fn fetch_orders(token: &str, model: &str, from: i64, to: i64) -> Result<Vec<Value>, String> {
    let prefix = order_path(model);
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
    Ok(fetched)
}
fn sync_orders(
    c: &Connection,
    shop: &str,
    token: &str,
    model: &str,
    from: i64,
    to: i64,
) -> Result<(), String> {
    let prefix = order_path(model);
    let mut fetched = fetch_orders(token, model, from, to)?;
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
fn reconcile(c: &Connection, shop: &str, action: &str, input: &Value) -> Result<Value, String> {
    let date = |key: &str| {
        NaiveDate::parse_from_str(input[key].as_str().unwrap_or(""), "%Y-%m-%d")
            .map_err(|_| "请选择有效的核对日期")
    };
    let (start, end) = (date("from")?, date("to")?);
    if end < start || (end - start).num_days() > 29 {
        return Err("每次请选择 1–30 天".into());
    }
    let key = format!("{start}/{end}");
    if action == "reconciliation_list" {
        return Ok(c
            .query_row(
                "SELECT raw FROM wb_ops_rows WHERE shop_id=?1 AND kind='reconciliation' AND id=?2",
                params![shop, key],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .map_err(|e| e.to_string())?
            .map(|raw| serde_json::from_str(&raw))
            .transpose()
            .map_err(|e| e.to_string())?
            .unwrap_or(Value::Null));
    }
    let (_, token) = wb_shop_center::active_token(c, shop)?;
    let zone = FixedOffset::east_opt(10800).unwrap();
    let from = start
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_local_timezone(zone)
        .unwrap()
        .timestamp();
    let to = end
        .and_hms_opt(23, 59, 59)
        .unwrap()
        .and_local_timezone(zone)
        .unwrap()
        .timestamp();
    let orders = fetch_orders(&token, "fbs", from, to)?;
    let report = request(
        &token,
        "finance-api",
        "/api/finance/v1/sales-reports/detailed",
        Some(
            json!({"dateFrom":start.to_string(),"dateTo":end.to_string(),"limit":100000,"rrdId":0,"period":"weekly"}),
        ),
    )?;
    let empty = Vec::new();
    let no_matches = Vec::new();
    let details = if report == json!({}) {
        &empty
    } else {
        report
            .as_array()
            .or_else(|| report["data"].as_array())
            .or_else(|| report.pointer("/data/items").and_then(Value::as_array))
            .ok_or("周销售报告响应缺少明细数组")?
    };
    if details.len() >= 100000 {
        return Err("财务明细达到读取上限，请缩短日期范围".into());
    }
    let mut by_id = std::collections::HashMap::new();
    for row in details {
        if let Some(id) = row["srid"].as_str().filter(|id| !id.is_empty()) {
            by_id.entry(id).or_insert_with(Vec::new).push(row);
        }
    }
    let mut matched = 0;
    let results: Vec<_> = orders
        .iter()
        .map(|order| {
            let matches = order["rid"]
                .as_str()
                .and_then(|id| by_id.get(id))
                .unwrap_or(&no_matches);
            matched += usize::from(!matches.is_empty());
            json!({"order":order,"details":matches})
        })
        .collect();
    let result = json!({"updatedAt":Utc::now().to_rfc3339(),"reportRows":details.len(),"matched":matched,"rows":results});
    put(c, shop, "reconciliation", &key, &result)?;
    Ok(result)
}

fn ads_snapshot(c: &Connection, shop: &str) -> Result<Value, String> {
    let mut stmt = c
        .prepare(
            "SELECT a.day, a.spend, a.attributed_sales, detail.raw FROM wb_ops_ads a
             LEFT JOIN wb_ops_rows detail ON detail.shop_id=a.shop_id AND detail.kind='ad_detail' AND detail.id=a.day
             WHERE a.shop_id=?1 ORDER BY a.day",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([shop], |r| {
            Ok(json!({
                "day": r.get::<_, String>(0)?,
                "spend": r.get::<_, f64>(1)?,
                "attributedSales": r.get::<_, f64>(2)?,
                "detail": r.get::<_, Option<String>>(3)?.and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
            }))
        })
        .map_err(|e| e.to_string())?;
    Ok(json!(rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?))
}

fn ad_bid_configuration(advert: &Value) -> (Option<&str>, Value) {
    let raw = &advert["settings"]["placements"];
    if let Some(values) = raw.as_array() {
        let has = |name: &str| values.iter().any(|value| value.as_str() == Some(name));
        if has("combined") {
            return (Some("unified"), json!({"search":true,"recommendations":true}));
        }
        let search = has("search");
        let recommendations = has("recommendations") || has("recommendation");
        let strategy = (search || recommendations).then_some("manual");
        return (strategy, json!({"search":search,"recommendations":recommendations}));
    }
    let strategy = advert["bidType"].as_str().or_else(|| advert["bid_type"].as_str());
    (strategy, raw.clone())
}

fn ad_product_bids(item: &Value, strategy: Option<&str>, placements: &Value) -> serde_json::Map<String, Value> {
    let legacy = item.get("bidsKopecks").or_else(|| item.get("bids_kopecks")).unwrap_or(&Value::Null);
    let value = |key: &str| {
        if let Some(entries) = item["bids"].as_array() {
            let mut matching = entries.iter().filter(|entry| {
                let kind = entry["type"].as_str();
                kind == Some(key) || (key == "recommendations" && kind == Some("recommendation"))
            });
            let first = matching.next()?;
            return if matching.next().is_none() { first["value"].as_u64() } else { None };
        }
        legacy[key].as_u64()
    };
    let search = value("search");
    let recommendations = value("recommendations");
    let mut fields = serde_json::Map::new();
    match strategy {
        Some("manual") => {
            for (placement, bid) in [("search", search), ("recommendations", recommendations)] {
                if placements[placement] == true {
                    if let Some(bid) = bid {
                        fields.insert(placement.into(), json!(bid));
                    }
                }
            }
        }
        Some("unified" | "standard") => {
            let combined = value("combined").or_else(|| match (search, recommendations) {
                (Some(a), Some(b)) if a == b => Some(a),
                (Some(a), None | Some(0)) | (None | Some(0), Some(a)) if a > 0 => Some(a),
                _ => None,
            });
            if let Some(bid) = combined {
                fields.insert("combined".into(), json!(bid));
            }
        }
        _ => {}
    }
    fields
}

fn ad_bid_details(c: &Connection, shop: &str, payload: &Value, save: bool) -> Result<Value, String> {
    let id = payload["id"].as_u64().filter(|id| *id > 0).ok_or("活动编号无效")?;
    let (_, token) = wb_shop_center::active_token(c, shop)?;
    let result = request(&token, "advert-api", &format!("/api/advert/v3/adverts?advertIds={id}"), None)?;
    let advert = array(&result, "adverts")?.into_iter().find(|row|
        row["advertId"].as_u64().or_else(|| row["id"].as_u64()) == Some(id))
        .ok_or("当前店铺未返回该广告活动")?;
    let currency = advert["currency"].as_str().filter(|s| !s.is_empty()).ok_or("活动币种未知")?;
    let (strategy, placements) = ad_bid_configuration(&advert);
    let editable = matches!(advert["status"].as_i64(), Some(4 | 9 | 11));
    let items = advert["nms"].as_array().or_else(|| advert["nm_settings"].as_array())
        .ok_or("WB 未返回活动商品")?;
    let catalog: Option<String> = c.query_row("SELECT body FROM wb_product_catalog WHERE shop_id=?1",
        [shop], |row| row.get(0)).optional().ok().flatten();
    let catalog: Value = catalog.and_then(|raw| serde_json::from_str(&raw).ok()).unwrap_or(Value::Null);
    let mut products = Vec::new();
    for item in items {
        let Some(nm) = item["nmId"].as_u64().or_else(|| item["nm_id"].as_u64()) else { continue };
        let fields = ad_product_bids(item, strategy, &placements);
        let card = catalog["cards"].as_array().and_then(|cards| cards.iter().find(|card| card["nmID"].as_u64() == Some(nm)));
        products.push(json!({"nmId":nm,"bids":fields,"name":card.and_then(|c|c["title"].as_str()),
            "code":card.and_then(|c|c["vendorCode"].as_str()),"image":card.and_then(|c|c["photos"][0]["big"].as_str())}));
    }
    if !save {
        return Ok(json!({"currency":currency,"strategy":strategy,"placements":placements,"editable":editable,"products":products}));
    }
    if !editable { return Err("当前活动状态不支持修改竞价".into()); }
    let nm = payload["nmId"].as_u64().filter(|nm| *nm > 0).ok_or("商品编号无效")?;
    let placement = payload["placement"].as_str().filter(|p| matches!(*p,"search"|"recommendations"|"combined")).ok_or("竞价位置无效")?;
    let expected = payload["previous"].as_u64().ok_or("缺少当前竞价")?;
    let bid = payload["bid"].as_u64().filter(|bid| *bid > 0 && *bid <= i32::MAX as u64).ok_or("竞价值无效")?;
    if payload["currency"].as_str() != Some(currency) ||
        products.iter().find(|p| p["nmId"].as_u64() == Some(nm))
            .and_then(|p|p["bids"][placement].as_u64()) != Some(expected) {
        return Err("活动币种、商品或当前竞价已变化，请刷新详情后再提交".into());
    }
    let updated = request_with_method(&token,"advert-api","/api/advert/v1/bids","PATCH",
        Some(json!({"bids":[{"advert_id":id,"nm_bids":[{"nm_id":nm,"bid_kopecks":bid,"placement":placement}]}]})))?;
    let confirmed = updated["bids"].as_array().into_iter().flatten()
        .filter(|row|row["advert_id"].as_u64()==Some(id))
        .flat_map(|row|row["nm_bids"].as_array().into_iter().flatten())
        .any(|row|row["nm_id"].as_u64()==Some(nm) && row["placement"].as_str()==Some(placement)
            && row["bid_kopecks"].as_u64()==Some(bid));
    if updated["currency"].as_str() != Some(currency) || !confirmed { return Err("WB 未确认新竞价，请刷新详情并核对后再操作".into()); }
    Ok(json!({"bid":bid}))
}

fn ad_campaigns(c: &Connection, shop: &str) -> Result<Value, String> {
    let (_, token) = wb_shop_center::active_token(c, shop)?;
    let mut campaigns = std::collections::BTreeMap::new();
    let mut cursors = std::collections::HashSet::new();
    let mut path = "/api/advert/v3/adverts?limit=50".to_owned();
    loop {
        let result = request(&token, "advert-api", &path, None)?;
        for row in array(&result, "adverts")? {
            let id = row["advertId"].as_i64().filter(|id| *id > 0).ok_or("活动编号缺失")?;
            campaigns.insert(id, json!({"id":id,"name":row["settings"]["name"],
                "payment":row["settings"]["paymentType"],"status":row["status"],
                "products":row["nms"].as_array().into_iter().flatten()
                    .filter_map(|item| item["nmId"].as_i64()).collect::<Vec<_>>()}));
        }
        let next = result["next"].as_str().ok_or("WB 未返回活动分页游标")?;
        if next.is_empty() {
            return Ok(json!(campaigns.into_values().collect::<Vec<_>>()));
        }
        if !next.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.=".contains(&b))
            || !cursors.insert(next.to_owned()) {
            return Err("WB 活动分页游标无效或重复".into());
        }
        path = format!("/api/advert/v3/adverts?limit=50&next={next}");
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn ad_product_attribution(c: &Connection, shop: &str, payload: &Value) -> Result<Value, String> {
    let campaign = payload["campaignId"].as_i64().filter(|id| *id > 0).ok_or("广告活动编号无效")?;
    let date = |key: &str| {
        NaiveDate::parse_from_str(payload[key].as_str().unwrap_or(""), "%Y-%m-%d")
            .map_err(|_| "请选择有效日期")
    };
    let (from, to) = (date("from")?, date("to")?);
    if !(0..=30).contains(&(to - from).num_days()) {
        return Err("商品归因单次最多查询 31 天".into());
    }
    let (_, token) = wb_shop_center::active_token(c, shop)?;
    let result = request(&token, "advert-api", &format!(
        "/api/advert/v4/fullstats?advertIds={campaign}&beginDate={from}&endDate={to}"
    ), None)?;
    let mut rows = Vec::new();
    for row in array(&result, "rows")? {
        if row["advertId"].as_i64() != Some(campaign) {
            return Err("归因数据的活动编号与查询不匹配".into());
        }
        if row["placement"].as_str() != Some("search") {
            continue;
        }
        let id = row["nmId"].as_i64().filter(|id| *id > 0).ok_or("归因商品编号缺失")?;
        let kind = row["conversionType"].as_str().ok_or("归因类型缺失")?;
        let currency = row["currency"].as_str().filter(|s| !s.is_empty()).ok_or("归因币种缺失")?;
        let number = |key: &str| row[key].as_f64().filter(|n| *n >= 0.0).ok_or("归因金额缺失");
        rows.push(json!({"nmId":id,"kind":kind,"currency":currency,
            "orders":row["orders"].as_u64().ok_or("归因订单量缺失")?,
            "sales":number("sumPrice")?,"spend":number("sum")?}));
    }
    Ok(json!(rows))
}

fn ad_cost_daily(c: &Connection, shop: &str, payload: &Value) -> Result<Value, String> {
    let date = |key: &str| {
        NaiveDate::parse_from_str(payload[key].as_str().unwrap_or(""), "%Y-%m-%d")
            .map_err(|_| "请选择有效日期")
    };
    let (from, to) = (date("from")?, date("to")?);
    if !(0..=30).contains(&(to - from).num_days()) {
        return Err("广告日明细单次最多查询 31 天".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    for item in array(payload, "ids")? {
        ids.insert(item.as_i64().filter(|id| *id > 0).ok_or("活动编号无效")?);
    }
    if ids.is_empty() || ids.len() > 50 {
        return Err("请选择 1 至 50 个广告活动".into());
    }
    let ids_joined = ids.iter().map(ToString::to_string).collect::<Vec<_>>().join(",");
    let (_, token) = wb_shop_center::active_token(c, shop)?;
    let metadata = request(&token, "advert-api", &format!("/api/advert/v3/adverts?advertIds={ids_joined}"), None)?;
    let mut settings = std::collections::BTreeMap::new();
    for item in array(&metadata, "adverts")? {
        let Some(id) = item["advertId"].as_i64().filter(|id| ids.contains(id)) else { continue };
        let payment = item["settings"]["paymentType"].as_str()
            .filter(|value| matches!(*value, "cpc" | "cpm")).ok_or("WB 未返回广告活动的计费方式")?;
        let currency = item["currency"].as_str().filter(|s| !s.is_empty()).ok_or("WB 未返回广告活动币种")?;
        settings.insert(id, (payment.to_owned(), currency.to_owned()));
    }
    if settings.len() != ids.len() {
        return Err("部分活动无法读取计费方式，请核对活动编号和所属店铺".into());
    }
    let stats = request(&token, "advert-api", &format!(
        "/adv/v3/fullstats?ids={ids_joined}&beginDate={from}&endDate={to}"
    ), None)?;
    let mut rows = Vec::new();
    for item in stats.as_array().ok_or("WB 未返回活动统计")? {
        let (payment, currency) = item["advertId"].as_i64().and_then(|id| settings.get(&id))
            .ok_or("活动统计中的编号与查询不匹配")?;
        for row in array(item, "days")? {
            let day = row["date"].as_str().and_then(|date| date.get(..10))
                .and_then(|date| date.parse::<NaiveDate>().ok())
                .filter(|day| *day >= from && *day <= to).ok_or("WB 返回无效统计日期")?;
            let spend = row["sum"].as_f64().filter(|n| *n >= 0.0).ok_or("WB 日统计缺少广告花费")?;
            rows.push(json!({"advertId":item["advertId"],"day":day,"payment":payment,"currency":currency,"spend":spend,"orders":row["orders"],
                "views":row["views"].as_u64(),"clicks":row["clicks"].as_u64()}));
        }
    }
    Ok(json!(rows))
}

fn search_cluster_stats(c: &Connection, shop: &str, payload: &Value) -> Result<Value, String> {
    let id = |key: &str| {
        payload[key].as_i64().filter(|id| *id > 0).ok_or("请输入有效的活动编号和商品编号")
    };
    let date = |key: &str| {
        NaiveDate::parse_from_str(payload[key].as_str().unwrap_or(""), "%Y-%m-%d")
            .map_err(|_| "请选择有效日期")
    };
    let (from, to) = (date("from")?, date("to")?);
    if !(0..=29).contains(&(to - from).num_days()) {
        return Err("搜索词统计单次最多查询 30 天".into());
    }
    let (campaign, product) = (id("campaignId")?, id("nmId")?);
    let (_, token) = wb_shop_center::active_token(c, shop)?;
    let result = request(&token, "advert-api", "/adv/v1/normquery/stats", Some(json!({
        "from":from, "to":to, "items":[{"advertId":campaign,"nmId":product}]
    })))?;
    let mut rows = Vec::new();
    for item in array(&result, "items")? {
        if item["advertId"].as_i64() != Some(campaign) || item["nmId"].as_i64() != Some(product) {
            return Err("搜索词响应的活动或商品编号不匹配".into());
        }
        for row in array(&item, "dailyStats")? {
            let stat = &row["stat"];
            let query = stat["normQuery"].as_str().ok_or("搜索词组名称缺失")?;
            let day = row["date"].as_str().and_then(|day| day.parse::<NaiveDate>().ok())
                .filter(|day| *day >= from && *day <= to).ok_or("搜索词统计日期无效")?;
            let count = |key: &str| stat[key].as_u64().ok_or("搜索词统计缺少数量字段");
            let spend = stat["spend"].as_f64().filter(|amount| *amount >= 0.0)
                .ok_or("搜索词统计缺少广告费")?;
            rows.push(json!({"query":query, "day":day, "clicks":count("clicks")?,
                "orders":count("orders")?, "carts":count("atbs")?, "spend":spend,
                "views":stat["views"].as_u64()}));
        }
    }
    Ok(json!(rows))
}

fn collect_ad_products(
    day: &Value,
    products: &mut std::collections::BTreeMap<i64, (f64, f64, u64)>,
) -> Option<()> {
    for app in day["apps"].as_array()? {
        for item in app["nms"].as_array()? {
            let id = item["nmId"].as_i64().filter(|id| *id > 0)?;
            let spend = item["sum"].as_f64()?;
            let sales = item["sum_price"].as_f64()?;
            let orders = item["orders"].as_u64()?;
            let total = products.entry(id).or_default();
            total.0 += spend;
            total.1 += sales;
            total.2 = total.2.checked_add(orders)?;
        }
    }
    Some(())
}

fn sync_ads(c: &mut Connection, shop: &str, token: &str, payload: &Value) -> Result<Value, String> {
    let parse = |key: &str| {
        NaiveDate::parse_from_str(payload[key].as_str().unwrap_or(""), "%Y-%m-%d")
            .map_err(|_| "请选择有效日期".to_string())
    };
    let from = parse("from")?;
    let to = parse("to")?;
    let duration = (to - from).num_days();
    if !(0..=29).contains(&duration) {
        return Err("广告单次同步最多 30 天".into());
    }
    let campaigns = request(token, "advert-api", "/adv/v1/promotion/count", None)?;
    let groups = array(&campaigns, "adverts")?;
    let mut ids = std::collections::BTreeSet::new();
    for group in groups
        .iter()
        .filter(|g| matches!(g["status"].as_i64(), Some(7 | 9 | 11)))
    {
        for entry in array(group, "advert_list")? {
            if let Some(id) = entry["advertId"].as_i64() {
                ids.insert(id);
            }
        }
    }
    let mut daily = vec![(0.0, 0.0); duration as usize + 1];
    let mut details = vec![(Some(0_u64), Some(std::collections::BTreeMap::<i64, (f64, f64, u64)>::new())); daily.len()];
    let ids: Vec<_> = ids.iter().map(ToString::to_string).collect();
    for (index, chunk) in ids.chunks(50).enumerate() {
        if index > 0 {
            std::thread::sleep(Duration::from_secs(21));
        }
        let path = format!(
            "/adv/v3/fullstats?ids={}&beginDate={from}&endDate={to}",
            chunk.join(",")
        );
        let result = request(token, "advert-api", &path, None)?;
        for campaign in result.as_array().ok_or("广告统计响应不是活动列表")? {
            for day in array(campaign, "days")? {
                let date = day["date"]
                    .as_str()
                    .and_then(|text| text.get(..10))
                    .and_then(|text| NaiveDate::parse_from_str(text, "%Y-%m-%d").ok())
                    .ok_or("广告统计缺少有效日期")?;
                let offset = (date - from).num_days();
                if (0..=duration).contains(&offset) {
                    let spend = day["sum"].as_f64().ok_or("广告统计缺少花费")?;
                    let sales = day["sum_price"].as_f64().ok_or("广告统计缺少归因销售额")?;
                    daily[offset as usize].0 += spend;
                    daily[offset as usize].1 += sales;
                    let (orders, products) = &mut details[offset as usize];
                    *orders = orders.zip(day["orders"].as_u64()).and_then(|(total, count)| total.checked_add(count));
                    if let Some(products) = products {
                        if collect_ad_products(&day, products).is_none() {
                            details[offset as usize].1 = None;
                        }
                    }
                }
            }
        }
    }
    let tx = c.transaction().map_err(|e| e.to_string())?;
    for (offset, (spend, sales)) in daily.into_iter().enumerate() {
        let day = from + chrono::Duration::days(offset as i64);
        let (orders, products) = &details[offset];
        put(&tx, shop, "ad_detail", &day.to_string(), &json!({"orders": orders, "products": products}))?;
        tx.execute("INSERT INTO wb_ops_ads(shop_id,day,spend,attributed_sales) VALUES(?1,?2,?3,?4)
            ON CONFLICT(shop_id,day) DO UPDATE SET spend=excluded.spend, attributed_sales=excluded.attributed_sales",
            params![shop, day.to_string(), spend, sales]).map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    ads_snapshot(c, shop)
}

fn weekly_api_report(c: &Connection, shop: &str, payload: &Value) -> Result<Value, String> {
    use chrono::Datelike;
    let from = payload["from"].as_str().and_then(|value| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
        .ok_or("请选择有效的周一")?;
    let to = from.checked_add_signed(chrono::Duration::days(6)).ok_or("日期超出范围")?;
    let today = (Utc::now() + chrono::Duration::hours(3)).date_naive();
    if from.weekday() != chrono::Weekday::Mon || to >= today {
        return Err("请选择已经结束的完整周".into());
    }
    let key = from.to_string();
    if payload["refresh"] != true {
        let cached: Option<String> = c.query_row(
            "SELECT raw FROM wb_ops_rows WHERE shop_id=?1 AND kind='weekly_api_report' AND id=?2",
            params![shop, key], |row| row.get(0),
        ).optional().map_err(|e| e.to_string())?;
        if let Some(raw) = cached {
            return serde_json::from_str(&raw).map_err(|e| e.to_string());
        }
    }

    let (_, token) = wb_shop_center::active_token(c, shop)?;
    let mut cursor = 0;
    let mut records = Vec::new();
    const LIMIT: usize = 100_000;
    loop {
        let value = request(&token, "finance-api", "/api/finance/v1/sales-reports/detailed", Some(json!({
            "dateFrom":key,"dateTo":format!("{to}T23:59:59"),"period":"weekly","limit":LIMIT,"rrdId":cursor
        })))?;
        if value == json!({}) { break; }
        let batch = value.as_array().or_else(|| value.pointer("/data/items").and_then(Value::as_array))
            .ok_or("WB 财务接口没有返回报告明细")?;
        if batch.iter().any(|row| !row.is_object()) { return Err("WB 报告明细格式无效".into()); }
        records.extend(batch.iter().cloned());
        if batch.len() < LIMIT { break; }
        let next = batch.last().and_then(|row| row["rrdId"].as_u64()
            .or_else(|| row["rrdId"].as_str().and_then(|value| value.parse().ok())))
            .filter(|next| *next > cursor).ok_or("WB 没有返回有效的分页游标")?;
        cursor = next;
        std::thread::sleep(std::time::Duration::from_secs(61));
    }

    let core = ["reportId","dateFrom","dateTo","currency","rrdId","nmId","vendorCode","docTypeName",
        "sellerOperName","orderDt","saleDt","quantity","retailAmount","forPay","deliveryService","storageFee","deduction"];
    let names: std::collections::BTreeSet<_> = records.iter()
        .flat_map(|row| row.as_object().into_iter().flat_map(|row| row.keys().map(String::as_str))).collect();
    let headers: Vec<_> = core.iter().copied().filter(|field| names.contains(field))
        .chain(names.iter().copied().filter(|field| !core.contains(field))).collect();
    let mut rows = vec![headers.iter().map(|field| field.to_string()).collect::<Vec<_>>()];
    rows.extend(records.iter().map(|row| headers.iter().map(|field| match &row[*field] {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        value => value.to_string(),
    }).collect()));
    let result = json!({"name":format!("WB 官方销售报告 · {from} 至 {to}"),
        "source":"api","sheets":[{"name":"销售报告明细","firstRow":1,"firstColumn":1,"width":headers.len(),"rows":rows}]});
    if !records.is_empty() { put(c, shop, "weekly_api_report", &key, &result)?; }
    Ok(result)
}

fn weekly_workbook(c: &Connection, shop: &str, action: &str, payload: &Value) -> Result<Value, String> {
    use calamine::{Reader, Xlsx};
    if action == "weekly_report_load" {
        return c.query_row(
            "SELECT raw FROM wb_ops_rows WHERE shop_id=?1 AND kind='weekly_workbook' AND id='latest'",
            [shop], |row| row.get::<_, String>(0),
        ).optional().map_err(|e| e.to_string())?
          .map(|raw| serde_json::from_str(&raw).map_err(|e| e.to_string())).unwrap_or(Ok(Value::Null));
    }
    let name = payload["name"].as_str().filter(|name| name.to_lowercase().ends_with(".xlsx"))
        .ok_or("请选择 WB 周报 XLSX 文件")?;
    let bytes = payload["bytes"].as_array().filter(|bytes| !bytes.is_empty() && bytes.len() <= 16 * 1024 * 1024)
        .ok_or("文件为空或超过 16 MB")?.iter().map(|value| value.as_u64().and_then(|n| u8::try_from(n).ok())
        .ok_or("文件数据无效")).collect::<Result<Vec<_>, _>>()?;
    let mut workbook = Xlsx::new(std::io::Cursor::new(bytes)).map_err(|e| format!("无法打开 Excel：{e}"))?;
    let mut sheets = Vec::new();
    for sheet in workbook.sheet_names().to_vec() {
        let range = workbook.worksheet_range(&sheet).map_err(|e| format!("{sheet}：{e}"))?;
        let start = range.start().map(|(row, col)| (row + 1, col + 1)).unwrap_or((1, 1));
        let rows: Vec<Vec<String>> = range.rows().map(|row| row.iter().map(ToString::to_string).collect()).collect();
        sheets.push(json!({"name":sheet,"firstRow":start.0,"firstColumn":start.1,"width":range.width(),"rows":rows}));
    }
    if sheets.is_empty() {
        return Err("工作簿没有工作表".into());
    }
    let result = json!({"name":name,"sheets":sheets});
    put(c, shop, "weekly_workbook", "latest", &result)?;
    Ok(result)
}

#[tauri::command]
pub async fn wb_operations(
    action: String,
    payload: Value,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut c = wb_shop_center::db(&state)?;
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
            "weekly_report_summary" => {
                let from = payload["from"].as_str().and_then(|s| NaiveDate::parse_from_str(s,"%Y-%m-%d").ok()).ok_or("周报日期无效")?;
                let to = from.checked_add_signed(chrono::Duration::days(6)).ok_or("日期超出范围")?;
                let key = from.to_string();
                let cached: Option<Value> = c.query_row(
                    "SELECT raw FROM wb_ops_rows WHERE shop_id=?1 AND kind='weekly_summary' AND id=?2",
                    params![shop, key], |row| row.get::<_, String>(0),
                ).optional().map_err(|e| e.to_string())?
                  .map(|raw| serde_json::from_str(&raw)).transpose().map_err(|e| e.to_string())?;
                if payload["refresh"] != true {
                    if let Some(cached) = &cached { return Ok(cached.clone()); }
                }
                let (_, token) = wb_shop_center::active_token(&c, shop)?;
                let result = request(&token,"finance-api","/api/finance/v1/sales-reports/list",
                    Some(json!({"dateFrom":key,"dateTo":to.to_string(),"period":"weekly","limit":100,"offset":0})))?;
                let mut reports = if result == json!({}) { Vec::new() }
                    else { result.as_array().filter(|rows| rows.len() < 100)
                        .cloned().ok_or("WB 周报汇总格式异常或超过单周读取上限")? };
                let mut ids = std::collections::HashSet::new();
                if reports.iter().any(|row| {
                    let id = text(&row["reportId"]);
                    id.is_empty() || !ids.insert(id)
                }) { return Err("WB 汇总缺少报告编号或报告编号重复".into()); }
                if reports.is_empty() {
                    if let Some(cached) = &cached {
                        if cached["reports"].as_array().is_some_and(|rows| !rows.is_empty()) {
                            return Err("WB 本次未返回汇总，已保留上次成功的缓存".into());
                        }
                    }
                }
                reports.sort_by_key(|row| text(&row["reportId"]));
                let snapshot = json!({"reports":reports,
                    "previous":cached.as_ref().map(|value| &value["reports"]),
                    "updatedAt":Utc::now().to_rfc3339()});
                if !reports.is_empty() { put(&c, shop, "weekly_summary", &key, &snapshot)?; }
                Ok(snapshot)
            }
            "weekly_report_api" => weekly_api_report(&c, shop, &payload),
            "weekly_report_load" | "weekly_report_import" => weekly_workbook(&c, shop, &action, &payload),
            "list" => snapshot(&c, shop),
            "ad_balance" | "ad_budgets" | "ad_deposit" => {
                let (_, token) = wb_shop_center::active_token(&c, shop)?;
                match action.as_str() {
                    "ad_balance" => request(&token, "advert-api", "/adv/v1/balance", None),
                    "ad_budgets" => {
                        let ids = array(&payload, "ids")?;
                        if ids.is_empty() || ids.len() > 50 || !ids.iter().all(|id| id.as_u64().is_some_and(|id| id > 0)) {
                            return Err("请选择 1–50 个有效活动".into());
                        }
                        request(&token, "advert-api", "/api/advert/v2/budget", Some(json!({"advertIds": ids})))
                    }
                    _ => {
                        let id = payload["id"].as_u64().filter(|id| *id > 0).ok_or("活动编号无效")?;
                        let sum = payload["sum"].as_u64().filter(|sum| *sum > 0).ok_or("充值金额必须为正整数")?;
                        let source = payload["source"].as_u64().filter(|source| matches!(source, 0 | 1 | 3)).ok_or("充值来源无效")?;
                        request(&token, "advert-api", &format!("/adv/v1/budget/deposit?id={id}"),
                            Some(json!({"sum":sum,"type":source,"return":true})))
                    }
                }
            }
            "ad_bid_details" | "ad_bid_save" => ad_bid_details(&c, shop, &payload, action == "ad_bid_save"),
            "ad_campaigns" => ad_campaigns(&c, shop),
            "ad_cost_daily" => ad_cost_daily(&c, shop, &payload),
            "ad_product_attribution" => ad_product_attribution(&c, shop, &payload),
            "ad_keywords" => search_cluster_stats(&c, shop, &payload),
            "product_catalog" | "product_catalog_refresh" | "product_cards" | "product_detail" | "product_update" | "product_media" =>
                super::wb_product_editor::run(&c, shop, &action, &payload),
            "inventory_options" => super::wb_inventory::options(&c, shop, &payload),
            "inventory_list" => super::wb_inventory::run(&c, shop, None),
            "inventory_sync" | "inventory_update" => {
                let resource = payload["resource"].as_str();
                if action == "inventory_sync" && resource.is_none() {
                    return Err("请选择同步来源".into());
                }
                let lock = SYNCING.get_or_init(Default::default);
                if !lock
                    .lock()
                    .map_err(|e| e.to_string())?
                    .insert(shop.to_owned())
                {
                    return Err("该店铺正在同步，请稍候".into());
                }
                let result = if action == "inventory_update" {
                    super::wb_inventory::update(&c, shop, &payload)
                } else {
                    super::wb_inventory::run(&c, shop, resource)
                };
                lock.lock().map_err(|e| e.to_string())?.remove(shop);
                result
            }
            "reconciliation_list" => reconcile(&c, shop, &action, &payload),
            "reconciliation_sync" => {
                let lock = SYNCING.get_or_init(Default::default);
                if !lock
                    .lock()
                    .map_err(|e| e.to_string())?
                    .insert(shop.to_string())
                {
                    return Err("该店铺正在同步，请稍候".into());
                }
                let result = reconcile(&c, shop, &action, &payload);
                lock.lock().map_err(|e| e.to_string())?.remove(shop);
                result
            }
            "ads_list" => ads_snapshot(&c, shop),
            "ads_sync" => {
                let (_, token) = wb_shop_center::active_token(&c, shop)?;
                let lock = SYNCING.get_or_init(Default::default);
                if !lock
                    .lock()
                    .map_err(|e| e.to_string())?
                    .insert(shop.to_string())
                {
                    return Err("该店铺正在同步，请稍候".into());
                }
                let result = sync_ads(&mut c, shop, &token, &payload);
                lock.lock().map_err(|e| e.to_string())?.remove(shop);
                result
            }
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
    #[test]
    fn v3_ad_bids_match_search_recommendations_and_standard_placements() {
        let advert = json!({"settings":{"paymentType":"cpc","placements":["search","recommendations"]}});
        let item = json!({"nmId":498090473,"bids":[{"type":"search","value":60},{"type":"recommendations","value":65}]});
        let (strategy, placements) = ad_bid_configuration(&advert);
        assert_eq!(strategy, Some("manual"));
        assert_eq!(Value::Object(ad_product_bids(&item, strategy, &placements)), json!({"search":60,"recommendations":65}));

        let advert = json!({"settings":{"placements":["combined"]}});
        let item = json!({"bids":[{"type":"search","value":65000}]});
        let (strategy, placements) = ad_bid_configuration(&advert);
        assert_eq!(placements, json!({"search":true,"recommendations":true}));
        assert_eq!(Value::Object(ad_product_bids(&item, strategy, &placements)), json!({"combined":65000}));
    }

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

#[cfg(test)]
#[test]
#[ignore = "Requires a configured WB workspace and live API access"]
fn live_reconciliation() {
    let path = std::env::var("WB_RECONCILIATION_DB").expect("Set WB_RECONCILIATION_DB");
    let c = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE).unwrap();
    schema(&c).unwrap();
    let shop: String = c.query_row(
        "SELECT s.id FROM wb_shops s JOIN wb_api_credentials a ON a.shop_id=s.id WHERE s.seller_type='cross_border' AND a.status='active' ORDER BY a.created_at DESC LIMIT 1",
        [], |r| r.get(0),
    ).unwrap();
    let input = json!({"from":std::env::var("WB_RECONCILIATION_FROM").unwrap(),"to":std::env::var("WB_RECONCILIATION_TO").unwrap()});
    let result = reconcile(&c, &shop, "reconciliation_sync", &input).unwrap();
    assert_eq!(
        result,
        reconcile(&c, &shop, "reconciliation_list", &input).unwrap()
    );
    let orders = result["rows"].as_array().unwrap();
    assert!(!orders.is_empty());
    for item in orders {
        for row in item["details"].as_array().unwrap() {
            assert_eq!(item["order"]["rid"], row["srid"]);
        }
    }
    println!(
        "orders={} matched={} finance={}",
        orders.len(),
        result["matched"],
        result["reportRows"]
    );
    if let Some(item) = orders
        .iter()
        .find(|item| item["order"]["id"] == 6257835101_i64)
    {
        println!(
            "target converted={} linked={}",
            item["order"]["convertedPrice"],
            item["details"].as_array().unwrap().len()
        );
    }
}
