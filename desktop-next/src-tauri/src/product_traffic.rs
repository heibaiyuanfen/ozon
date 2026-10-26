use crate::{db, AppState, DateRange};
use rusqlite::{params, Connection};
use serde::Serialize;
use tauri::State;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrafficRow {
    sku: String,
    offer_id: String,
    name: String,
    image_url: String,
    views: Option<i64>,
    cart_adds: Option<i64>,
    ordered_units: Option<i64>,
    first_day: Option<String>,
    last_day: Option<String>,
    cached_days: i64,
    sources: String,
    updated_at: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrafficReport {
    rows: Vec<TrafficRow>,
    total_views: i64,
    first_day: Option<String>,
    last_day: Option<String>,
    updated_at: Option<String>,
}

fn validate(range: &DateRange) -> Result<(), String> {
    let from =
        chrono::NaiveDate::parse_from_str(&range.from, "%Y-%m-%d").map_err(|_| "开始日期无效")?;
    let to =
        chrono::NaiveDate::parse_from_str(&range.to, "%Y-%m-%d").map_err(|_| "结束日期无效")?;
    if from > to {
        return Err("开始日期不能晚于结束日期".into());
    }
    Ok(())
}
fn report(c: &Connection, range: &DateRange) -> Result<TrafficReport, String> {
    validate(range)?;
    // Aggregate before joining the catalogue: no order/revenue predicate may discard views.
    let mut stmt=c.prepare("WITH traffic AS (SELECT sku,SUM(views) views,SUM(cart_adds) carts,SUM(ordered_units) units,MAX(product_name) name,MIN(day) first_day,MAX(day) last_day,COUNT(DISTINCT day) days,GROUP_CONCAT(DISTINCT source) sources,MAX(updated_at) updated_at FROM sales_daily WHERE day BETWEEN ?1 AND ?2 GROUP BY sku), keys AS (SELECT sku FROM products UNION SELECT sku FROM traffic) SELECT k.sku,COALESCE(p.offer_id,''),COALESCE(NULLIF(p.name,''),t.name,''),COALESCE(p.image_url,''),t.views,t.carts,t.units,t.first_day,t.last_day,COALESCE(t.days,0),COALESCE(t.sources,''),t.updated_at FROM keys k LEFT JOIN products p ON p.sku=k.sku LEFT JOIN traffic t ON t.sku=k.sku ORDER BY t.views DESC,k.sku").map_err(|e|e.to_string())?;
    let rows = stmt
        .query_map(params![range.from, range.to], |r| {
            Ok(TrafficRow {
                sku: r.get(0)?,
                offer_id: r.get(1)?,
                name: r.get(2)?,
                image_url: r.get(3)?,
                views: r.get(4)?,
                cart_adds: r.get(5)?,
                ordered_units: r.get(6)?,
                first_day: r.get(7)?,
                last_day: r.get(8)?,
                cached_days: r.get(9)?,
                sources: r.get(10)?,
                updated_at: r.get(11)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let (total_views,first_day,last_day,updated_at)=c.query_row("SELECT COALESCE(SUM(views),0),MIN(day),MAX(day),MAX(updated_at) FROM sales_daily WHERE day BETWEEN ?1 AND ?2",params![range.from,range.to],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(|e|e.to_string())?;
    Ok(TrafficReport {
        rows,
        total_views,
        first_day,
        last_day,
        updated_at,
    })
}
#[tauri::command]
pub async fn product_traffic(
    range: DateRange,
    state: State<'_, AppState>,
) -> Result<TrafficReport, String> {
    let owned = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || report(&db(&owned)?, &range))
        .await
        .map_err(|e| e.to_string())?
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrafficDay {
    day: String,
    views: i64,
    cart_adds: i64,
    ordered_units: i64,
    source: String,
}
#[tauri::command]
pub async fn product_traffic_days(
    range: DateRange,
    sku: String,
    state: State<'_, AppState>,
) -> Result<Vec<TrafficDay>, String> {
    validate(&range)?;
    let c = db(&state)?;
    let mut stmt=c.prepare("SELECT day,views,cart_adds,ordered_units,source FROM sales_daily WHERE day BETWEEN ?1 AND ?2 AND sku=?3 ORDER BY day DESC").map_err(|e|e.to_string())?;
    let rows = stmt
        .query_map(params![range.from, range.to, sku], |r| {
            Ok(TrafficDay {
                day: r.get(0)?,
                views: r.get(1)?,
                cart_adds: r.get(2)?,
                ordered_units: r.get(3)?,
                source: r.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn traffic_includes_zero_sales_unknown_catalogue_and_missing_cache() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE products(sku TEXT PRIMARY KEY,offer_id TEXT,name TEXT,image_url TEXT);CREATE TABLE sales_daily(day TEXT,sku TEXT,views INTEGER,cart_adds INTEGER,ordered_units INTEGER,product_name TEXT,source TEXT,updated_at TEXT);INSERT INTO products VALUES('a','offer-a','A',''),('b','offer-b','B',''),('missing','','Missing','');INSERT INTO sales_daily VALUES('2026-10-20','a',10,2,0,'A','api','2026-10-21'),('2026-10-21','a',20,3,1,'A','api','2026-10-22'),('2026-10-20','b',0,0,0,'B','api','2026-10-21'),('2026-10-20','orphan',8,1,0,'Unknown','api','2026-10-21'),('2026-09-01','a',999,0,0,'A','api','2026-09-02');").unwrap();
        let range = DateRange {
            from: "2026-10-20".into(),
            to: "2026-10-21".into(),
        };
        let r = report(&c, &range).unwrap();
        assert_eq!(r.total_views, 38);
        assert_eq!(r.rows.iter().filter_map(|x| x.views).sum::<i64>(), 38);
        assert_eq!(r.rows[0].sku, "a");
        assert_eq!(r.rows[0].cached_days, 2);
        assert_eq!(r.rows[0].ordered_units, Some(1));
        assert_eq!(
            r.rows.iter().find(|x| x.sku == "missing").unwrap().views,
            None
        );
        assert_eq!(r.rows.iter().find(|x| x.sku == "b").unwrap().views, Some(0));
        let empty = report(
            &c,
            &DateRange {
                from: "2025-01-01".into(),
                to: "2025-01-02".into(),
            },
        )
        .unwrap();
        assert_eq!(empty.total_views, 0);
        assert!(empty.first_day.is_none());
        assert!(validate(&DateRange {
            from: range.to,
            to: range.from
        })
        .is_err());
    }
}
