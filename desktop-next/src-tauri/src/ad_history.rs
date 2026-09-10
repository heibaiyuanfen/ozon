use chrono::NaiveDate;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug)]
struct Row {
    day: String,
    campaign: String,
    sku: String,
    values: [Option<f64>; 6],
}

fn parse(payload: &Value, batch: &[String], from: &str, to: &str) -> Result<Vec<Row>, String> {
    fn walk(
        v: &Value,
        campaign: &str,
        batch: &[String],
        from: &str,
        to: &str,
        out: &mut Vec<Row>,
    ) -> Result<(), String> {
        match v {
            Value::Array(a) => {
                for item in a {
                    walk(item, campaign, batch, from, to, out)?;
                }
            }
            Value::Object(o) => {
                let explicit = super::object_text(o, &["campaignId", "campaign_id"]);
                let id = if explicit.is_empty() {
                    campaign
                } else {
                    &explicit
                };
                let sku = super::object_text(o, &["sku"]);
                if !sku.is_empty() {
                    let raw = super::object_text(o, &["date", "day"]);
                    let date = match NaiveDate::parse_from_str(&raw, "%Y-%m-%d")
                        .or_else(|_| NaiveDate::parse_from_str(&raw, "%d.%m.%Y"))
                    {
                        Ok(date) => date.to_string(),
                        Err(_) => return Ok(()),
                    };
                    if !batch.iter().any(|x| x == id) || date.as_str() < from || date.as_str() > to
                    {
                        return Ok(());
                    }
                    let mut values = [None; 6];
                    for (i, keys) in [
                        vec!["views", "impressions"],
                        vec!["clicks"],
                        vec![
                            "toCart",
                            "cartAdds",
                            "cart_adds",
                            "addToCart",
                            "add_to_cart",
                        ],
                        vec!["orders", "orderCount", "ordersCount", "order_count"],
                        vec!["sales", "revenue", "ordersMoney", "orderMoney"],
                        vec!["expense", "spend", "moneySpent", "cost"],
                    ]
                    .iter()
                    .enumerate()
                    {
                        values[i] = keys.iter().filter_map(|key| o.get(*key)).find_map(|value| {
                            value
                                .as_f64()
                                .or_else(|| {
                                    value.as_str().and_then(|s| {
                                        s.replace([' ', '\u{a0}'], "")
                                            .replace(',', ".")
                                            .parse::<f64>()
                                            .ok()
                                    })
                                })
                                .filter(|n| {
                                    n.is_finite() && *n >= 0.0 && (i >= 4 || n.fract() == 0.0)
                                })
                        });
                    }
                    out.push(Row {
                        day: date,
                        campaign: id.into(),
                        sku,
                        values,
                    });
                } else {
                    for (key, item) in o {
                        if matches!(key.as_str(), "totals" | "total") {
                            continue;
                        }
                        let context = if batch.contains(key) {
                            key.as_str()
                        } else {
                            id
                        };
                        walk(item, context, batch, from, to, out)?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
    let mut rows = Vec::new();
    walk(
        payload,
        if batch.len() == 1 { &batch[0] } else { "" },
        batch,
        from,
        to,
        &mut rows,
    )?;
    Ok(rows)
}

fn store(
    c: &mut Connection,
    rows: Vec<Row>,
    names: &HashMap<String, String>,
) -> Result<i64, String> {
    ensure(c)?;
    let mut merged: BTreeMap<(String, String, String), ([Option<f64>; 6], i64)> = BTreeMap::new();
    for row in rows {
        use std::collections::btree_map::Entry;
        match merged.entry((row.day, row.campaign, row.sku)) {
            Entry::Vacant(e) => {
                let mask = row
                    .values
                    .iter()
                    .enumerate()
                    .fold(0i64, |m, (i, v)| m | if v.is_none() { 1 << i } else { 0 });
                e.insert((row.values, mask));
            }
            Entry::Occupied(mut e) => {
                for i in 0..6 {
                    if row.values[i].is_none() {
                        e.get_mut().1 |= 1 << i;
                    }
                    e.get_mut().0[i] = match (e.get().0[i], row.values[i]) {
                        (Some(a), Some(b)) => Some(a + b),
                        (a, b) => a.or(b),
                    };
                }
            }
        }
    }
    let tx = c.transaction().map_err(|e| e.to_string())?;
    for ((day, campaign, sku), (v, mask)) in &merged {
        if *mask == 0 {
            tx.execute("INSERT INTO ad_daily(day,campaign_id,campaign_name,sku,impressions,clicks,cart_adds,orders,revenue,spend,source) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'api_history_sku') ON CONFLICT(day,campaign_id,sku) DO UPDATE SET impressions=excluded.impressions,clicks=excluded.clicks,cart_adds=excluded.cart_adds,orders=excluded.orders,revenue=excluded.revenue,spend=excluded.spend,source=excluded.source,updated_at=CURRENT_TIMESTAMP",params![day,campaign,names.get(campaign).cloned().unwrap_or_default(),sku,v[0],v[1],v[2],v[3],v[4],v[5]]).map_err(|e|e.to_string())?;
            tx.execute(
                "DELETE FROM ad_partial_daily WHERE day=?1 AND campaign_id=?2 AND sku=?3",
                params![day, campaign, sku],
            )
            .map_err(|e| e.to_string())?;
        } else {
            // Legacy ad_daily is NOT NULL and consumed by existing dashboards. Keep
            // partial observations separate rather than inventing numeric zeroes.
            tx.execute("INSERT INTO ad_partial_daily(day,campaign_id,sku,impressions,clicks,cart_adds,orders,revenue,spend,missing_mask) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) ON CONFLICT(day,campaign_id,sku) DO UPDATE SET impressions=excluded.impressions,clicks=excluded.clicks,cart_adds=excluded.cart_adds,orders=excluded.orders,revenue=excluded.revenue,spend=excluded.spend,missing_mask=excluded.missing_mask,updated_at=CURRENT_TIMESTAMP",params![day,campaign,sku,v[0],v[1],v[2],v[3],v[4],v[5],mask]).map_err(|e|e.to_string())?;
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(merged.len() as i64)
}

pub(super) fn ensure(c: &Connection) -> Result<(), String> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS ad_partial_daily(day TEXT NOT NULL,campaign_id TEXT NOT NULL,sku TEXT NOT NULL,impressions REAL,clicks REAL,cart_adds REAL,orders REAL,revenue REAL,spend REAL,updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,PRIMARY KEY(day,campaign_id,sku));").map_err(|e|e.to_string())?;
    let has:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('ad_partial_daily') WHERE name='missing_mask')",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if !has {
        c.execute_batch(
            "ALTER TABLE ad_partial_daily ADD COLUMN missing_mask INTEGER NOT NULL DEFAULT 0",
        )
        .map_err(|e| e.to_string())?;
    }
    c.execute_batch("CREATE TABLE IF NOT EXISTS ad_history_coverage(campaign_id TEXT NOT NULL,date_from TEXT NOT NULL,date_to TEXT NOT NULL,downloaded_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,PRIMARY KEY(campaign_id,date_from,date_to))").map_err(|e|e.to_string())?;
    Ok(())
}

fn record_coverage(c: &Connection, batch: &[String], from: &str, to: &str) -> Result<(), String> {
    for campaign in batch {
        c.execute("INSERT INTO ad_history_coverage(campaign_id,date_from,date_to) VALUES(?1,?2,?3) ON CONFLICT(campaign_id,date_from,date_to) DO UPDATE SET downloaded_at=CURRENT_TIMESTAMP",params![campaign,from,to]).map_err(|e|e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn sync(
    c: &mut Connection,
    token: &str,
    from: &str,
    to: &str,
    names: &HashMap<String, String>,
    log_id: i64,
) -> Result<i64, String> {
    sync_with_refresh(c, token, from, to, names, log_id, false)
}

pub(super) fn sync_with_refresh(
    c: &mut Connection,
    token: &str,
    from: &str,
    to: &str,
    names: &HashMap<String, String>,
    log_id: i64,
    force: bool,
) -> Result<i64, String> {
    let mut first = NaiveDate::parse_from_str(from, "%Y-%m-%d").map_err(|_| "开始日期无效")?;
    let end = NaiveDate::parse_from_str(to, "%Y-%m-%d").map_err(|_| "结束日期无效")?;
    if first > end {
        return Ok(0);
    }
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS ad_history_jobs(job_key TEXT PRIMARY KEY,uuid TEXT NOT NULL); CREATE TABLE IF NOT EXISTS ad_history_report_cache(job_key TEXT PRIMARY KEY,payload TEXT NOT NULL,saved_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP)",
    )
    .map_err(|e| e.to_string())?;
    c.execute(
        "DELETE FROM ad_history_report_cache WHERE saved_at<=datetime('now','-1 day')",
        [],
    )
    .map_err(|e| e.to_string())?;
    let mut count = 0;
    while first <= end {
        let last = (first + chrono::Duration::days(30)).min(end);
        let a = first.to_string();
        let b = last.to_string();
        let campaigns = {
            let mut stmt=c.prepare("SELECT DISTINCT a.campaign_id FROM ad_daily a LEFT JOIN campaigns c ON c.campaign_id=a.campaign_id WHERE a.day BETWEEN ?1 AND ?2 AND a.sku='' AND (a.spend<>0 OR a.impressions<>0 OR a.clicks<>0 OR a.orders<>0) AND (lower(COALESCE(c.payment_type,'')) IN ('cpc','') OR lower(c.payment_type) LIKE '%click%') ORDER BY a.campaign_id").map_err(|e|e.to_string())?;
            let rows = stmt
                .query_map(params![a, b], |r| r.get::<_, String>(0))
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?
        };
        for (batch_index, batch) in campaigns.chunks(10).enumerate() {
            let key = format!("{a}|{b}|{}", batch.join(","));
            use rusqlite::OptionalExtension;
            let label = format!(
                "历史广告 {a} 至 {b}，第 {}/{} 批",
                batch_index + 1,
                campaigns.len().div_ceil(10)
            );
            let payload_cache: Option<String> = c.query_row("SELECT payload FROM ad_history_report_cache WHERE job_key=?1 AND saved_at>datetime('now','-15 minutes') AND ?2=0", params![key,force], |r| r.get(0)).optional().map_err(|e|e.to_string())?;
            if let Some(payload) = payload_cache {
                let payload: Value = serde_json::from_str(&payload).map_err(|e| e.to_string())?;
                let rows = parse(&payload, batch, &a, &b)?;
                let saved = store(c, rows, names)?;
                record_coverage(c, batch, &a, &b)?;
                count += saved;
                c.execute(
                    "UPDATE sync_logs SET rows_count=rows_count+?1,message=?2 WHERE id=?3",
                    params![
                        saved,
                        format!("{label}：复用 15 分钟内报告，已处理 {saved} 条"),
                        log_id
                    ],
                )
                .map_err(|e| e.to_string())?;
                continue;
            }
            let cached: Option<String> = c
                .query_row(
                    "SELECT uuid FROM ad_history_jobs WHERE job_key=?1",
                    [&key],
                    |r| r.get(0),
                )
                .optional()
                .map_err(|e| e.to_string())?;
            let uuid = if let Some(uuid) = cached {
                uuid
            } else {
                let report = super::performance_post(
                    "/api/client/statistics/json",
                    token,
                    &json!({"campaigns":batch,"dateFrom":a,"dateTo":b,"groupBy":"DATE"}),
                )?;
                let uuid = report
                    .get("UUID")
                    .or_else(|| report.get("uuid"))
                    .and_then(Value::as_str)
                    .filter(|s| {
                        !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                    })
                    .ok_or("历史广告报告未返回有效 UUID")?
                    .to_owned();
                c.execute(
                    "INSERT OR REPLACE INTO ad_history_jobs VALUES(?1,?2)",
                    params![key, uuid],
                )
                .map_err(|e| e.to_string())?;
                uuid
            };
            c.execute("INSERT INTO sync_log_events(log_id,event_at,level,stage,message) VALUES(?1,CURRENT_TIMESTAMP,'INFO','历史商品广告',?2)",params![log_id,format!("等待历史商品广告报告：{a} 至 {b}，{} 个计划",batch.len())]).map_err(|e|e.to_string())?;
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
            let started = std::time::Instant::now();
            loop {
                c.execute(
                    "UPDATE sync_logs SET message=?1 WHERE id=?2",
                    params![
                        format!(
                            "{label}：等待 Ozon 生成报告，已等待 {} 秒",
                            started.elapsed().as_secs()
                        ),
                        log_id
                    ],
                )
                .map_err(|e| e.to_string())?;
                let status =
                    super::performance_get(&format!("/api/client/statistics/{uuid}"), token)?;
                let state = status
                    .get("state")
                    .or_else(|| status.get("status"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_uppercase();
                if ["ERROR", "FAILED", "CANCELLED"].contains(&state.as_str()) {
                    c.execute("DELETE FROM ad_history_jobs WHERE job_key=?1", [&key])
                        .map_err(|e| e.to_string())?;
                    return Err(format!("历史商品广告报告生成失败（{a} 至 {b}），请重试"));
                }
                if state == "OK" || state == "SUCCESS" {
                    let payload = super::performance_get(
                        &format!("/api/client/statistics/report?UUID={uuid}"),
                        token,
                    )?;
                    let rows = parse(&payload, batch, &a, &b)?;
                    if rows.is_empty() {
                        return Err(format!(
                            "{a} 至 {b} 的历史报告没有可识别的 SKU 明细；未将缺失数据填零"
                        ));
                    }
                    let partial = rows
                        .iter()
                        .filter(|r| r.values.iter().any(Option::is_none))
                        .count();
                    let saved = store(c, rows, names)?;
                    c.execute("INSERT INTO ad_history_report_cache(job_key,payload) VALUES(?1,?2) ON CONFLICT(job_key) DO UPDATE SET payload=excluded.payload,saved_at=CURRENT_TIMESTAMP",params![key,payload.to_string()]).map_err(|e|e.to_string())?;
                    record_coverage(c, batch, &a, &b)?;
                    count += saved;
                    c.execute(
                        "UPDATE sync_logs SET rows_count=rows_count+?1 WHERE id=?2",
                        params![saved, log_id],
                    )
                    .map_err(|e| e.to_string())?;
                    c.execute("INSERT INTO sync_log_events(log_id,event_at,level,stage,message) VALUES(?1,CURRENT_TIMESTAMP,?2,'历史商品广告',?3)",params![log_id,if partial>0 {"WARNING"}else{"INFO"},format!("已保存 {saved} 条商品记录；{partial} 条含缺失指标，已保留有效值并标记未知；无效日期或计划行不入库")]).map_err(|e|e.to_string())?;
                    c.execute("DELETE FROM ad_history_jobs WHERE job_key=?1", [&key])
                        .map_err(|e| e.to_string())?;
                    break;
                }
                if std::time::Instant::now() >= deadline {
                    return Err("历史广告报告仍在生成，请稍后再次同步，将继续下载同一报告".into());
                }
                std::thread::sleep(std::time::Duration::from_secs(3));
            }
        }
        first = last + chrono::Duration::days(1);
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn related_product_metrics_are_not_substitutes_for_direct_metrics() {
        let rows=parse(&json!({"42":{"rows":[{"date":"2026-09-01","sku":"A","modelOrders":5,"modelSales":100}]}}), &["42".into()],"2026-09-01","2026-09-01").unwrap();
        assert!(rows[0].values[3].is_none());
        assert!(rows[0].values[4].is_none());
    }
    #[test]
    #[ignore = "Live CSV evidence download for explicitly selected database"]
    fn download_csv_evidence() {
        let c = Connection::open(std::env::var("OZON_HISTORY_TEST_DB").unwrap()).unwrap();
        let token = super::super::performance_token(&c).unwrap();
        let response=super::super::performance_post("/api/client/statistics",&token,&json!({"campaigns":["37641191"],"dateFrom":"2026-08-31","dateTo":"2026-09-06","groupBy":"DATE"})).unwrap();
        let uuid = response
            .get("UUID")
            .or_else(|| response.get("uuid"))
            .and_then(Value::as_str)
            .unwrap();
        assert!(uuid.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
        let start = std::time::Instant::now();
        loop {
            let status =
                super::super::performance_get(&format!("/api/client/statistics/{uuid}"), &token)
                    .unwrap();
            let state = status
                .get("state")
                .or_else(|| status.get("status"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if state == "OK" || state == "SUCCESS" {
                let response = ureq::get(&format!(
                    "https://api-performance.ozon.ru/api/client/statistics/report?UUID={uuid}"
                ))
                .set("Authorization", &format!("Bearer {token}"))
                .call()
                .unwrap();
                let mut bytes = Vec::new();
                std::io::Read::read_to_end(&mut response.into_reader(), &mut bytes).unwrap();
                std::fs::write(std::env::var("OZON_CSV_EVIDENCE_OUTPUT").unwrap(), &bytes).unwrap();
                println!("Downloaded {} bytes of official CSV report", bytes.len());
                break;
            }
            assert!(
                !["ERROR", "FAILED", "CANCELLED"].contains(&state),
                "report failed"
            );
            assert!(start.elapsed().as_secs() < 180, "report timed out");
            std::thread::sleep(std::time::Duration::from_secs(3));
        }
    }
    #[test]
    #[ignore = "Explicit local database replay and export; no network"]
    fn replay_local_reports_and_export() {
        let path = std::env::var("OZON_HISTORY_TEST_DB").unwrap();
        let output = std::env::var("OZON_SERIES_TEST_OUTPUT").unwrap();
        let mut c = Connection::open(path).unwrap();
        ensure(&c).unwrap();
        let reports = c
            .prepare("SELECT job_key,payload FROM ad_history_report_cache")
            .unwrap()
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let mut count = 0;
        for (key, payload) in reports {
            let parts: Vec<_> = key.split('|').collect();
            assert_eq!(parts.len(), 3);
            let batch: Vec<String> = parts[2].split(',').map(str::to_owned).collect();
            let rows = parse(
                &serde_json::from_str(&payload).unwrap(),
                &batch,
                parts[0],
                parts[1],
            )
            .unwrap();
            if rows.is_empty() {
                continue;
            }
            count += store(&mut c, rows, &HashMap::new()).unwrap();
            record_coverage(&c, &batch, parts[0], parts[1]).unwrap();
        }
        let skus = [
            "3691349791",
            "3691342273",
            "5123796001",
            "3691376921",
            "3691416375",
            "2914261306",
            "3691307941",
            "5123791269",
        ]
        .map(str::to_owned)
        .to_vec();
        let data =
            super::super::ad_series::build(&c, "2026-08-31", "2026-09-06", skus, "BKZY001003", 1.0)
                .unwrap();
        assert_eq!(data["products"].as_array().unwrap().len(), 8);
        assert!(data["seriesSummary"]["referenceMetrics"]["acos"].is_number());
        std::fs::write(output, serde_json::to_string_pretty(&data).unwrap()).unwrap();
        println!("Replayed {count} local report rows; exported 8 products x 7 days");
    }
    #[test]
    fn recent_report_reuse_restores_rows_without_network() {
        let mut c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE ad_daily(day TEXT,campaign_id TEXT,campaign_name TEXT,sku TEXT,impressions INTEGER,clicks INTEGER,cart_adds INTEGER,orders INTEGER,revenue REAL,spend REAL,source TEXT,updated_at TEXT DEFAULT CURRENT_TIMESTAMP,UNIQUE(day,campaign_id,sku)); CREATE TABLE campaigns(campaign_id TEXT,payment_type TEXT); CREATE TABLE sync_logs(id INTEGER,rows_count INTEGER,message TEXT); INSERT INTO sync_logs VALUES(1,0,''); INSERT INTO ad_daily(day,campaign_id,sku,spend) VALUES('2026-09-01','42','',20); CREATE TABLE ad_history_report_cache(job_key TEXT PRIMARY KEY,payload TEXT NOT NULL,saved_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);").unwrap();
        let payload = json!({"42":{"report":{"rows":[{"sku":"A","date":"2026-09-01","views":100,"clicks":10,"toCart":0,"orders":2,"moneySpent":20,"ordersMoney":100}]}}});
        c.execute(
            "INSERT INTO ad_history_report_cache(job_key,payload) VALUES(?1,?2)",
            params!["2026-09-01|2026-09-01|42", payload.to_string()],
        )
        .unwrap();
        for _ in 0..2 {
            assert_eq!(
                sync(
                    &mut c,
                    "invalid-token-never-used",
                    "2026-09-01",
                    "2026-09-01",
                    &HashMap::new(),
                    1
                )
                .unwrap(),
                1
            );
        }
        assert_eq!(
            c.query_row("SELECT COUNT(*) FROM ad_daily WHERE sku='A'", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        c.execute(
            "UPDATE ad_history_report_cache SET saved_at=datetime('now','-16 minutes')",
            [],
        )
        .unwrap();
        assert_eq!(c.query_row("SELECT COUNT(*) FROM ad_history_report_cache WHERE saved_at>datetime('now','-15 minutes')",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    }
    #[test]
    fn split_rows_keep_known_subtotals_and_flag_unknown_fragments() {
        let mut c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE ad_daily(day TEXT,campaign_id TEXT,campaign_name TEXT,sku TEXT,impressions INTEGER,clicks INTEGER,cart_adds INTEGER,orders INTEGER,revenue REAL,spend REAL,source TEXT,updated_at TEXT DEFAULT CURRENT_TIMESTAMP,UNIQUE(day,campaign_id,sku));CREATE TABLE products(sku TEXT,offer_id TEXT,name TEXT);CREATE TABLE sales_daily(day TEXT,sku TEXT,product_name TEXT,ordered_units INTEGER,revenue REAL);").unwrap();
        let payload = json!({"42":{"rows":[{"sku":"A","date":"2026-09-04","orders":2,"moneySpent":10},{"sku":"A","date":"2026-09-04","orders":null,"moneySpent":20}]}});
        for _ in 0..2 {
            store(
                &mut c,
                parse(&payload, &["42".into()], "2026-09-04", "2026-09-04").unwrap(),
                &HashMap::new(),
            )
            .unwrap();
        }
        let r = super::super::ad_series::build(
            &c,
            "2026-09-04",
            "2026-09-04",
            vec!["A".into()],
            "A",
            1.0,
        )
        .unwrap();
        assert_eq!(r["seriesSummary"]["spend"], 30.0);
        assert_eq!(r["seriesSummary"]["adOrders"], 2.0);
        assert!(r["seriesSummary"]["missingMetrics"]
            .as_array()
            .unwrap()
            .contains(&json!("adOrders")));
        assert!(!r["seriesSummary"]["missingMetrics"]
            .as_array()
            .unwrap()
            .contains(&json!("spend")));
        assert!(r["seriesSummary"]["cpa"].is_null());
    }
    #[test]
    fn partial_records_preserve_known_metrics_and_do_not_poison_batch() {
        let mut c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE ad_daily(day TEXT,campaign_id TEXT,campaign_name TEXT,sku TEXT,impressions INTEGER NOT NULL,clicks INTEGER NOT NULL,cart_adds INTEGER NOT NULL,orders INTEGER NOT NULL,revenue REAL NOT NULL,spend REAL NOT NULL,source TEXT,updated_at TEXT DEFAULT CURRENT_TIMESTAMP,UNIQUE(day,campaign_id,sku));CREATE TABLE products(sku TEXT,offer_id TEXT,name TEXT);CREATE TABLE sales_daily(day TEXT,sku TEXT,product_name TEXT,ordered_units INTEGER,revenue REAL);INSERT INTO sales_daily VALUES('2026-09-04','A','A',5,500),('2026-09-04','B','B',4,400);").unwrap();
        let payload = json!({"42":{"report":{"rows":[
            {"sku":"A","date":"2026-09-04","views":"100","clicks":"10","toCart":0,"moneySpent":"20","ordersMoney":"100"},
            {"sku":"B","date":"2026-09-04","views":"200","clicks":"20","toCart":0,"orders":2,"moneySpent":"40","ordersMoney":"200"},
            {"sku":"C","date":"wrong"}
        ]}}});
        for _ in 0..2 {
            let rows = parse(&payload, &["42".into()], "2026-09-04", "2026-09-04").unwrap();
            assert_eq!(store(&mut c, rows, &HashMap::new()).unwrap(), 2);
        }
        let r = super::super::ad_series::build(
            &c,
            "2026-09-04",
            "2026-09-04",
            vec!["A".into(), "B".into()],
            "AB",
            1.0,
        )
        .unwrap();
        assert_eq!(r["seriesSummary"]["spend"], 60.0);
        assert_eq!(r["seriesSummary"]["adOrders"], 2.0);
        assert_eq!(r["seriesSummary"]["acos"], 20.0);
        assert!(r["seriesSummary"]["cpa"].is_null());
        assert!(r["products"][0]["daily"][0]["adOrders"].is_null());
        assert_eq!(
            r["dataQuality"]["missingData"][0]["metrics"],
            json!(["adOrders"])
        );
        assert_eq!(
            c.query_row("SELECT COUNT(*) FROM ad_partial_daily", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
    #[test]
    #[ignore = "Requires an explicitly selected local database and live Performance credentials"]
    fn live_history_backfill() {
        let path = std::env::var("OZON_HISTORY_TEST_DB").expect("explicit database path required");
        let from = std::env::var("OZON_HISTORY_TEST_FROM").unwrap();
        let to = std::env::var("OZON_HISTORY_TEST_TO").unwrap();
        let mut c = Connection::open(path).unwrap();
        let token = super::super::performance_token(&c).expect("Performance authentication failed");
        let names = {
            let mut stmt = c.prepare("SELECT campaign_id,name FROM campaigns").unwrap();
            let rows = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                .unwrap();
            rows.collect::<Result<HashMap<_, _>, _>>().unwrap()
        };
        c.execute("INSERT INTO sync_logs(started_at,source,status) VALUES(CURRENT_TIMESTAMP,'Performance history repair','running')",[]).unwrap();
        let id = c.last_insert_rowid();
        match sync(&mut c, &token, &from, &to, &names, id) {
            Ok(n) => {
                c.execute("UPDATE sync_logs SET status='success',finished_at=CURRENT_TIMESTAMP,rows_count=?1 WHERE id=?2",params![n,id]).unwrap();
                println!("Historical SKU rows saved: {n}");
                assert!(n > 0);
            }
            Err(e) => {
                c.execute("UPDATE sync_logs SET status='failed',finished_at=CURRENT_TIMESTAMP,message=?1 WHERE id=?2",params![e,id]).unwrap();
                panic!("{e}");
            }
        }
    }
    #[test]
    fn historical_nested_rows_keep_campaign_and_date() {
        let v = json!({"42":{"report":{"rows":[{"date":"04.09.2026","sku":"A","views":"100","clicks":"10","toCart":"1","orders":"2","sales":"1 234,50","expense":"50,25"}],"totals":{"sku":"TOTAL"}}}});
        let rows = parse(&v, &["42".into(), "43".into()], "2026-09-01", "2026-09-05").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].day, "2026-09-04");
        assert_eq!(rows[0].campaign, "42");
        assert_eq!(rows[0].values[4], Some(1234.5));
        assert!(parse(&v, &["42".into()], "2026-09-05", "2026-09-06")
            .unwrap()
            .is_empty());
    }
    #[test]
    fn missing_metric_or_date_is_not_zero() {
        let r=parse(&json!({"rows":[{"sku":"A","date":"2026-09-04","moneySpent":"15","orders":null},{"sku":"B","date":"invalid"}]}),&["42".into()],"2026-09-04","2026-09-04").unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].values[3], None);
        assert_eq!(r[0].values[5], Some(15.0));
    }
    #[test]
    fn report_aliases_and_repeat_sync_are_idempotent() {
        let mut c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE ad_daily(day TEXT,campaign_id TEXT,campaign_name TEXT,sku TEXT,impressions INTEGER,clicks INTEGER,cart_adds INTEGER,orders INTEGER,revenue REAL,spend REAL,source TEXT,updated_at TEXT,UNIQUE(day,campaign_id,sku));").unwrap();
        let row = json!({"date":"2026-09-04","sku":"A","views":"100","clicks":"10","toCart":"1","orders":"2","ordersMoney":"1234,50","moneySpent":"50,25"});
        let payload = json!({"42":{"report":{"rows":[row.clone(),row]}}});
        for _ in 0..2 {
            let rows = parse(&payload, &["42".into()], "2026-09-04", "2026-09-05").unwrap();
            assert_eq!(store(&mut c, rows, &HashMap::new()).unwrap(), 1);
        }
        let totals: (i64, f64, f64) = c
            .query_row(
                "SELECT count(*),sum(spend),sum(revenue) FROM ad_daily",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(totals, (1, 100.5, 2469.0));
    }
}
