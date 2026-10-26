use crate::{seller_post, DateRange};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::collections::HashSet;

pub(crate) fn ensure_schema(c: &Connection) -> Result<(), String> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS posting_sync_coverage(day TEXT NOT NULL,scheme TEXT NOT NULL,status TEXT NOT NULL,synced_at TEXT NOT NULL,error TEXT NOT NULL DEFAULT '',PRIMARY KEY(day,scheme));").map_err(|e|e.to_string())
}
fn coverage(
    c: &Connection,
    range: &DateRange,
    scheme: &str,
    status: &str,
    error: &str,
) -> Result<(), String> {
    c.execute("WITH RECURSIVE days(day) AS (SELECT ?1 UNION ALL SELECT date(day,'+1 day') FROM days WHERE day<?2) INSERT INTO posting_sync_coverage(day,scheme,status,synced_at,error) SELECT day,?3,?4,strftime('%Y-%m-%dT%H:%M:%SZ','now'),?5 FROM days WHERE true ON CONFLICT(day,scheme) DO UPDATE SET status=excluded.status,synced_at=excluded.synced_at,error=excluded.error",params![range.from,range.to,scheme,status,error]).map_err(|e|e.to_string())?;
    Ok(())
}
fn text(v: Option<&Value>) -> String {
    v.map(|v| {
        v.as_str().map(str::to_string).unwrap_or_else(|| {
            if v.is_number() {
                v.to_string()
            } else {
                String::new()
            }
        })
    })
    .unwrap_or_default()
}
fn price(v: &Value) -> f64 {
    if v.is_object()
        && v.get("currency")
            .and_then(Value::as_str)
            .is_some_and(|s| s != "RUB")
    {
        return 0.0;
    }
    let v = v.get("amount").unwrap_or(v);
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .filter(|v| v.is_finite() && *v >= 0.0)
        .unwrap_or(0.0)
}
fn page(v: &Value) -> Result<(Vec<Value>, bool, String), String> {
    let rows = v
        .get("postings")
        .and_then(Value::as_array)
        .ok_or("新版订单响应缺少 postings 数组，不能视作0单")?
        .clone();
    let has_next = v["has_next"].as_bool().ok_or("新版订单响应缺少 has_next")?;
    let cursor = text(v.get("cursor"));
    if has_next && (rows.is_empty() || cursor.is_empty()) {
        return Err("订单仍有下一页但缺少有效记录或游标".into());
    }
    Ok((rows, has_next, cursor))
}
pub(crate) fn ingest(c: &Connection, scheme: &str, postings: &[Value]) -> Result<i64, String> {
    let tx = c.unchecked_transaction().map_err(|e| e.to_string())?;
    let mut written = 0;
    for posting in postings {
        let number = text(posting.get("posting_number"));
        if number.is_empty() {
            return Err("订单缺少 posting_number".into());
        }
        let status = text(posting.get("status"));
        if status.is_empty() {
            return Err("订单缺少 status".into());
        }
        let created = if scheme == "FBO" {
            text(posting.get("created_at").or(posting.get("in_process_at")))
        } else {
            text(posting.get("in_process_at").or(posting.get("created_at")))
        };
        let day = created.get(..10).ok_or("订单缺少有效创建日期")?;
        let origin = text(
            posting
                .pointer("/analytics_data/warehouse_name")
                .or(posting.pointer("/delivery_method/warehouse")),
        );
        let destination = text(
            posting
                .pointer("/analytics_data/region")
                .or(posting.pointer("/analytics_data/city")),
        );
        let products = posting["products"]
            .as_array()
            .ok_or("订单缺少 products 数组")?;
        for (index, item) in products.iter().enumerate() {
            let sku = text(item.get("sku"));
            let offer = text(item.get("offer_id"));
            let name = text(item.get("name"));
            let quantity = item["quantity"].as_i64().ok_or("订单商品缺少数量")?;
            if sku.is_empty() || quantity <= 0 {
                return Err("订单商品SKU或数量无效".into());
            }
            let id = format!("seller_{}:{number}:{sku}:{index}", scheme.to_lowercase());
            let unit_price = price(&item["price"]);
            tx.execute("INSERT INTO posting_routes(event_id,day,sku,offer_id,product_name,quantity,scheme,status,posting_number,origin,destination,order_price,source,updated_at)VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,CURRENT_TIMESTAMP) ON CONFLICT(event_id) DO UPDATE SET day=excluded.day,offer_id=excluded.offer_id,product_name=excluded.product_name,quantity=excluded.quantity,status=excluded.status,origin=excluded.origin,destination=excluded.destination,order_price=excluded.order_price,updated_at=CURRENT_TIMESTAMP",params![id,day,sku,offer,name,quantity,scheme,status,number,origin,destination,unit_price,format!("seller_{}",scheme.to_lowercase())]).map_err(|e|e.to_string())?;
            let event = format!("posting:{id}");
            if status.to_lowercase().contains("cancel") {
                tx.execute("INSERT INTO cancellation_events(event_id,day,sku,offer_id,product_name,quantity,scheme,source,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,'posting_status',CURRENT_TIMESTAMP) ON CONFLICT(event_id) DO UPDATE SET day=excluded.day,sku=excluded.sku,offer_id=excluded.offer_id,product_name=excluded.product_name,quantity=excluded.quantity,scheme=excluded.scheme,updated_at=CURRENT_TIMESTAMP",params![event,day,sku,offer,name,quantity,scheme]).map_err(|e|e.to_string())?;
            } else {
                tx.execute("DELETE FROM cancellation_events WHERE event_id=?1", [event])
                    .map_err(|e| e.to_string())?;
            }
            written += 1;
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(written)
}
pub(crate) fn sync(c: &Connection, range: &DateRange, scheme: &str) -> Result<i64, String> {
    ensure_schema(c)?;
    coverage(c, range, scheme, "running", "")?;
    let result = (|| {
        let path = match scheme {
            "FBS" => "/v4/posting/fbs/list",
            "FBO" => "/v3/posting/fbo/list",
            _ => return Err("未知履约模式".into()),
        };
        let mut cursor = String::new();
        let mut seen = HashSet::new();
        let mut postings = Vec::new();
        loop {
            let response = seller_post(
                c,
                path,
                &json!({"dir":"ASC","filter":{"since":format!("{}T00:00:00Z",range.from),"to":format!("{}T23:59:59Z",range.to),"status":""},"limit":100,"cursor":cursor,"with":{"analytics_data":true,"financial_data":true}}),
            )?;
            let (batch, has_next, next) = page(&response)?;
            postings.extend(batch);
            if postings.len() > 50000 {
                return Err("订单超过50000条，请缩小日期范围".into());
            }
            if !has_next {
                break;
            }
            if !seen.insert(next.clone()) {
                return Err("订单分页游标重复，未将不完整数据标记为同步成功".into());
            }
            cursor = next;
        }
        ingest(c, scheme, &postings)
    })();
    let (status, count, message) = match &result {
        Ok(n) => (
            "success",
            *n,
            format!("{} 至 {}：{n} 行，当前订单状态已核对", range.from, range.to),
        ),
        Err(e) => ("failed", 0, format!("{} 至 {}：{e}", range.from, range.to)),
    };
    coverage(c, range, scheme, status, &message)?;
    c.execute("INSERT INTO sync_logs(started_at,source,status,rows_count,message,finished_at) VALUES(CURRENT_TIMESTAMP,?1,?2,?3,?4,CURRENT_TIMESTAMP)",params![format!("Seller Orders {scheme}"),status,count,message]).map_err(|e|e.to_string())?;
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    fn storage() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE posting_routes(event_id TEXT PRIMARY KEY,day TEXT,sku TEXT,offer_id TEXT,product_name TEXT,quantity INTEGER,scheme TEXT,status TEXT,posting_number TEXT,origin TEXT,destination TEXT,order_price REAL,source TEXT,updated_at TEXT);CREATE TABLE cancellation_events(event_id TEXT PRIMARY KEY,day TEXT,sku TEXT,offer_id TEXT,product_name TEXT,quantity INTEGER,scheme TEXT,source TEXT,updated_at TEXT);CREATE TABLE sales_daily(day TEXT,sku TEXT,product_name TEXT,revenue REAL,ordered_units INTEGER,cancellations INTEGER);CREATE TABLE products(sku TEXT,name TEXT,offer_id TEXT);CREATE TABLE ad_daily(day TEXT,sku TEXT,spend REAL,revenue REAL);").unwrap();
        c
    }
    #[test]
    fn cancelled_quantities_and_money_survive_resync() {
        let c = storage();
        let mut p = json!({"posting_number":"p","status":"cancelled","created_at":"2026-10-24T01:00:00Z","products":[{"sku":1,"quantity":2,"price":{"amount":"100","currency":"RUB"}}]});
        ingest(&c, "FBO", &[p.clone()]).unwrap();
        ingest(&c, "FBO", &[p.clone()]).unwrap();
        c.execute(
            "INSERT INTO sales_daily VALUES('2026-10-24','1','A',300,3,0)",
            [],
        )
        .unwrap();
        let d = crate::net_dashboard::read(&c, "2026-10-24", "2026-10-24", 1.0)
            .unwrap()
            .remove(0);
        assert_eq!(d.cancellation_units, 2);
        assert_eq!(d.cancellation_amount, 200.0);
        assert!(!d.cancellation_complete);
        let range = DateRange {
            from: "2026-10-24".into(),
            to: "2026-10-24".into(),
        };
        for scheme in ["FBS", "FBO"] {
            coverage(&c, &range, scheme, "success", "").unwrap()
        }
        assert!(
            crate::net_dashboard::read(&c, "2026-10-24", "2026-10-24", 1.0).unwrap()[0]
                .cancellation_complete
        );
        p["status"] = json!("delivered");
        ingest(&c, "FBO", &[p]).unwrap();
        assert_eq!(
            crate::net_dashboard::read(&c, "2026-10-24", "2026-10-24", 1.0).unwrap()[0]
                .cancellation_units,
            0
        );
    }
    #[test]
    #[ignore = "requires locally saved authenticated Ozon posting responses"]
    fn authenticated_postings_feed_net_dashboard() {
        let v: Value = serde_json::from_str(
            &std::fs::read_to_string(std::env::var("OZON_POSTING_FIXTURE").unwrap()).unwrap(),
        )
        .unwrap();
        let c = storage();
        let mut expected = 0;
        for scheme in ["FBS", "FBO"] {
            let (rs, _, _) = page(&v[scheme]).unwrap();
            expected += rs
                .iter()
                .filter(|r| text(r.get("status")).contains("cancel"))
                .flat_map(|r| r["products"].as_array().unwrap())
                .map(|p| p["quantity"].as_i64().unwrap())
                .sum::<i64>();
            ingest(&c, scheme, &rs).unwrap();
        }
        assert_eq!(expected, 177);
        let ds = crate::net_dashboard::read(&c, "2026-10-17", "2026-10-24", 1.0).unwrap();
        assert_eq!(
            ds.iter().map(|d| d.cancellation_units).sum::<i64>(),
            expected
        );
        assert!(ds
            .iter()
            .any(|d| d.day >= "2026-10-19".to_string() && d.cancellation_units > 0));
    }
    #[test]
    fn cursor_contract_and_money_objects() {
        let (rows, next, cursor) =
            page(&json!({"postings":[{"posting_number":"p"}],"has_next":true,"cursor":"page2"}))
                .unwrap();
        assert_eq!(rows.len(), 1);
        assert!(next);
        assert_eq!(cursor, "page2");
        assert!(page(&json!({"result":[]})).is_err());
        assert!(page(&json!({"postings":[],"has_next":true,"cursor":"x"})).is_err());
        assert_eq!(price(&json!({"amount":"123.45","currency":"RUB"})), 123.45);
        assert_eq!(price(&json!({"amount":"10","currency":"USD"})), 0.0);
    }
    #[test]
    fn failed_coverage_does_not_masquerade_as_zero() {
        let c = Connection::open_in_memory().unwrap();
        ensure_schema(&c).unwrap();
        let range = DateRange {
            from: "2026-10-19".into(),
            to: "2026-10-24".into(),
        };
        coverage(&c, &range, "FBS", "success", "").unwrap();
        coverage(&c, &range, "FBO", "failed", "obsolete").unwrap();
        assert_eq!(
            c.query_row(
                "SELECT COUNT(*) FROM posting_sync_coverage WHERE status='failed'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            6
        );
    }
}
