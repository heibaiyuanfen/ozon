use crate::{active_shop_kind, business_report_blocking, db, AppState, DateRange};
use rusqlite::{params, Connection};
use serde::Serialize;
use tauri::State;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalWeeklyReport {
    from: String,
    to: String,
    revenue: f64,
    ad_revenue: f64,
    finance_sales: f64,
    units: i64,
    ad_units: i64,
    acos: Option<f64>,
    tacos: Option<f64>,
    profit: Option<f64>,
    profit_note: String,
    sales_days: i64,
    ad_days: i64,
    daily: Vec<WeeklyDailyRow>,
    products: Vec<WeeklyProductRow>,
    order_postings: Option<Vec<(String, String)>>,
    previous_products: Vec<WeeklyProductRow>,
    previous_sales_days: i64,
    profit_breakdown: Vec<ProfitBreakdownRow>,
    profit_log: Vec<ProfitLogEntry>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WeeklyDailyRow { day: String, revenue:f64, ad_revenue:f64, units:i64, ad_units:i64, ad_spend:f64, acos:Option<f64>, tacos:Option<f64> }
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WeeklyProductRow { sku:String, offer_id:String, name:String, revenue:f64, units:i64, ad_revenue:Option<f64>, ad_units:Option<i64>, ad_spend:Option<f64>, acos:Option<f64>, tacos:Option<f64> }
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProfitBreakdownRow { label:String, amount:f64, note:String }

#[derive(Clone, Debug, PartialEq, serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProfitSnapshot {
    profit: Option<i64>,
    items: std::collections::BTreeMap<String, i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProfitLogEntry {
    id: i64,
    recorded_at: String,
    snapshot: ProfitSnapshot,
}

fn profit_cents(amount: f64) -> i64 {
    (amount * 100.0).round() as i64
}

fn save_profit_log(c: &mut Connection, week_end: &str, current: ProfitSnapshot) -> Result<Vec<ProfitLogEntry>, String> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS weekly_profit_log (
        id INTEGER PRIMARY KEY, week_end TEXT NOT NULL, recorded_at TEXT NOT NULL, snapshot TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_weekly_profit_log_week ON weekly_profit_log(week_end,id);")
        .map_err(|e| e.to_string())?;
    let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(|e| e.to_string())?;
    let mut history = {
        let mut stmt = tx.prepare("SELECT id,recorded_at,snapshot FROM weekly_profit_log WHERE week_end=?1 ORDER BY id DESC")
            .map_err(|e| e.to_string())?;
        let rows = stmt.query_map([week_end], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        }).map_err(|e| e.to_string())?;
        rows.map(|row| {
            let (id, recorded_at, raw) = row.map_err(|e| e.to_string())?;
            let snapshot = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
            Ok(ProfitLogEntry { id, recorded_at, snapshot })
        }).collect::<Result<Vec<_>, String>>()?
    };
    if history.first().map(|entry| &entry.snapshot) != Some(&current) {
        let recorded_at = chrono::Utc::now().to_rfc3339();
        let raw = serde_json::to_string(&current).map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO weekly_profit_log(week_end,recorded_at,snapshot) VALUES(?1,?2,?3)",
            params![week_end, recorded_at, raw]).map_err(|e| e.to_string())?;
        history.insert(0, ProfitLogEntry { id: tx.last_insert_rowid(), recorded_at, snapshot: current });
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(history)
}

fn weekly_details(c:&Connection,range:&DateRange)->Result<(Vec<WeeklyDailyRow>,Vec<WeeklyProductRow>),String> {
    let mut q=c.prepare("WITH RECURSIVE days(day) AS(SELECT ?1 UNION ALL SELECT date(day,'+1 day') FROM days WHERE day<?2), sales AS(SELECT day,SUM(revenue) revenue,SUM(ordered_units) units FROM sales_daily WHERE day BETWEEN ?1 AND ?2 GROUP BY day), ads AS(SELECT a.day,SUM(ABS(a.spend)) spend,SUM(a.revenue) revenue,SUM(a.orders) units FROM ad_daily a WHERE a.day BETWEEN ?1 AND ?2 AND (a.sku='' OR NOT EXISTS(SELECT 1 FROM ad_daily x WHERE x.day=a.day AND x.sku='')) GROUP BY a.day) SELECT d.day,COALESCE(s.revenue,0),COALESCE(s.units,0),COALESCE(a.revenue,0),COALESCE(a.units,0),COALESCE(a.spend,0) FROM days d LEFT JOIN sales s ON s.day=d.day LEFT JOIN ads a ON a.day=d.day ORDER BY d.day").map_err(|e|e.to_string())?;
    let daily=q.query_map(params![range.from,range.to],|r|{
        let revenue:f64=r.get(1)?; let ad_revenue:f64=r.get(3)?; let spend:f64=r.get(5)?;
        Ok(WeeklyDailyRow{day:r.get(0)?,revenue,units:r.get(2)?,ad_revenue,ad_units:r.get(4)?,ad_spend:spend,acos:(ad_revenue>0.0).then(||spend/ad_revenue*100.0),tacos:(revenue>0.0).then(||spend/revenue*100.0)})
    }).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
    let mut q=c.prepare("WITH s AS(SELECT sku,SUM(revenue) revenue,SUM(ordered_units) units FROM sales_daily WHERE day BETWEEN ?1 AND ?2 GROUP BY sku),a AS(SELECT sku,SUM(revenue) revenue,SUM(orders) units,SUM(ABS(spend)) spend FROM ad_daily WHERE day BETWEEN ?1 AND ?2 AND sku<>'' GROUP BY sku),keys AS(SELECT sku FROM s UNION SELECT sku FROM a) SELECT k.sku,COALESCE(p.offer_id,''),COALESCE(p.name,''),COALESCE(s.revenue,0),COALESCE(s.units,0),a.revenue,a.units,a.spend FROM keys k LEFT JOIN s ON s.sku=k.sku LEFT JOIN a ON a.sku=k.sku LEFT JOIN products p ON p.sku=k.sku ORDER BY COALESCE(s.revenue,0) DESC,k.sku").map_err(|e|e.to_string())?;
    let products=q.query_map(params![range.from,range.to],|r|{
        let revenue:f64=r.get(3)?; let ad_revenue:Option<f64>=r.get(5)?; let spend:Option<f64>=r.get(7)?;
        Ok(WeeklyProductRow{sku:r.get(0)?,offer_id:r.get(1)?,name:r.get(2)?,revenue,units:r.get(4)?,ad_revenue,ad_units:r.get(6)?,ad_spend:spend,acos:spend.zip(ad_revenue).filter(|(_,v)|*v>0.0).map(|(s,v)|s/v*100.0),tacos:spend.filter(|_|revenue>0.0).map(|s|s/revenue*100.0)})
    }).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
    Ok((daily,products))
}

fn week_range(week_end: &str) -> Result<DateRange, String> {
    use chrono::Datelike;
    let end = chrono::NaiveDate::parse_from_str(week_end, "%Y-%m-%d").map_err(|_| "周结束日期无效")?;
    if end.weekday() != chrono::Weekday::Sun { return Err("请选择周日作为完整周的结束日期".into()); }
    let from = end.checked_sub_signed(chrono::Duration::days(6)).ok_or("周日期超出范围")?;
    Ok(DateRange { from: from.format("%Y-%m-%d").to_string(), to: week_end.into() })
}

fn weekly_totals(c: &Connection, range: &DateRange) -> Result<(f64, i64, f64, f64, i64, i64, i64), String> {
    let (revenue, units, sales_days) = c.query_row(
        "SELECT COALESCE(SUM(revenue),0),COALESCE(SUM(ordered_units),0),COUNT(DISTINCT day) FROM sales_daily WHERE day BETWEEN ?1 AND ?2",
        params![range.from,range.to], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
    ).map_err(|e|e.to_string())?;
    // Choose one advertising level for each day, rather than summing store
    // totals with their SKU breakdown or dropping SKU-only days.
    let (spend,ad_revenue,ad_units,ad_days) = c.query_row(
        "WITH selected AS(SELECT a.* FROM ad_daily a WHERE a.day BETWEEN ?1 AND ?2 AND ((a.sku='' AND EXISTS(SELECT 1 FROM ad_daily x WHERE x.day=a.day AND x.sku='')) OR (a.sku<>'' AND NOT EXISTS(SELECT 1 FROM ad_daily x WHERE x.day=a.day AND x.sku='')))) SELECT COALESCE(SUM(ABS(spend)),0),COALESCE(SUM(revenue),0),COALESCE(SUM(orders),0),COUNT(DISTINCT day) FROM selected",
        params![range.from,range.to],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)),
    ).map_err(|e|e.to_string())?;
    Ok((revenue,units,spend,ad_revenue,ad_units,sales_days,ad_days))
}

// Use posting IDs because the local cache stores postings, not parent order IDs.
fn weekly_order_postings(c: &Connection, range: &DateRange) -> Result<Option<Vec<(String, String)>>, String> {
    crate::posting_sync::ensure_schema(c)?;
    let covered: i64 = c.query_row(
        "SELECT COUNT(*) FROM posting_sync_coverage WHERE day BETWEEN ?1 AND ?2
         AND status='success' AND scheme IN ('FBS','FBO')",
        params![range.from, range.to], |row| row.get(0),
    ).map_err(|e| e.to_string())?;
    if covered != 14 { return Ok(None); }
    let mut query = c.prepare(
        "SELECT DISTINCT sku, posting_number FROM posting_routes WHERE day BETWEEN ?1 AND ?2
         AND sku<>'' AND posting_number<>'' AND scheme IN ('FBS','FBO')",
    ).map_err(|e| e.to_string())?;
    let rows = query.query_map(params![range.from, range.to], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map(Some).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn local_weekly_report(week_end: String, state: State<'_, AppState>) -> Result<LocalWeeklyReport,String> {
    let snapshot=state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        if active_shop_kind(&snapshot)? != "local" { return Err("本土店周报仅支持本土店铺".into()); }
        let range=week_range(&week_end)?;
        let c=db(&snapshot)?;
        let (revenue,units,spend,ad_revenue,ad_units,sales_days,ad_days)=weekly_totals(&c,&range)?;
        let (daily,products)=weekly_details(&c,&range)?;
        let previous_end = chrono::NaiveDate::parse_from_str(&range.from, "%Y-%m-%d")
            .map_err(|e| e.to_string())?.pred_opt().ok_or("无法计算上周日期")?;
        let previous_range = week_range(&previous_end.format("%Y-%m-%d").to_string())?;
        let (_, previous_products) = weekly_details(&c, &previous_range)?;
        let (_, _, _, _, _, previous_sales_days, _) = weekly_totals(&c, &previous_range)?;
        let order_postings = weekly_order_postings(&c, &range)?;
        drop(c);
        static PROFIT_LOG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = PROFIT_LOG_LOCK.lock().map_err(|e| e.to_string())?;
        let profit_report=business_report_blocking(range.clone(),&snapshot)?;
        let current = ProfitSnapshot {
            profit: profit_report.after_tax_profit.map(profit_cents),
            items: [
                ("Finance 结算净额", profit_report.finance_net),
                ("采购成本", -profit_report.purchase_cost),
                ("头程成本", -profit_report.first_mile_cost),
                ("税费", profit_report.tax_amount),
                ("回款手续费", profit_report.payout_fee),
                ("财务销售和退货（结算组成）", profit_report.sales_returns),
                ("应计费用（结算组成）", profit_report.accrual_fees),
                ("其他收入及调整（结算组成）", profit_report.other_adjustments),
                ("平台佣金（费用明细）", profit_report.commission),
                ("Finance 广告（费用明细）", profit_report.finance_advertising),
                ("配送费用（费用明细）", profit_report.delivery_fees),
                ("退货物流（费用明细）", profit_report.return_fees),
                ("收单支付（费用明细）", profit_report.acquiring),
                ("仓储包装（费用明细）", profit_report.storage_packaging),
                ("罚款与调整（费用明细）", profit_report.penalties_adjustments),
                ("其他 Finance 项目（费用明细）", profit_report.other_finance_fees),
            ].into_iter().map(|(name, amount)| (name.to_string(), profit_cents(amount))).collect(),
        };
        let profit_log = save_profit_log(&mut db(&snapshot)?, &week_end, current)?;
        let profit_note=if profit_report.finance_operations==0 {"本周尚无 Finance 结算数据，请先同步财务".into()}
            else if profit_report.missing_cost_units>0 {format!("本周 {} 件已结算商品缺少采购或头程成本，请完善成本",profit_report.missing_cost_units)}
            else {"沿用月度盈亏：Finance 净额 − 已结算商品采购成本 − 头程成本 − 税费 − 回款手续费；Finance 已含平台广告扣费，不重复扣广告费".into()};
        Ok(LocalWeeklyReport { from:range.from,to:range.to,revenue,ad_revenue,finance_sales:profit_report.sales_returns,units,ad_units,
            acos:(ad_revenue>0.0).then(||spend/ad_revenue*100.0),
            tacos:(revenue>0.0).then(||spend/revenue*100.0),
            profit:profit_report.after_tax_profit,profit_note,sales_days,ad_days,daily,products,order_postings,previous_products,previous_sales_days,profit_log,
            profit_breakdown:vec![
                ProfitBreakdownRow{label:"Finance 结算净额".into(),amount:profit_report.finance_net,note:"含平台销售结算、广告及各项费用".into()},
                ProfitBreakdownRow{label:"采购成本".into(),amount:-profit_report.purchase_cost,note:"本周已结算商品的采购成本".into()},
                ProfitBreakdownRow{label:"头程成本".into(),amount:-profit_report.first_mile_cost,note:"本周已结算商品的头程成本".into()},
                ProfitBreakdownRow{label:"税费".into(),amount:profit_report.tax_amount,note:format!("Finance 销售和退货 × {}%",profit_report.tax_rate)},
                ProfitBreakdownRow{label:"回款手续费".into(),amount:profit_report.payout_fee,note:format!("正结算净额 × {}%",profit_report.payout_fee_rate)},
            ] })
    }).await.map_err(|e|format!("本土店周报读取失败：{e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn weekly_profit_history_keeps_baselines_and_changed_items_by_week() {
        let mut c = Connection::open_in_memory().unwrap();
        let baseline = ProfitSnapshot {
            profit: Some(10000),
            items: [("采购成本".to_string(), -4000), ("税费".to_string(), -600)].into(),
        };
        let first = save_profit_log(&mut c, "2026-11-15", baseline.clone()).unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(save_profit_log(&mut c, "2026-11-15", baseline.clone()).unwrap().len(), 1);
        let mut changed = baseline.clone();
        changed.profit = Some(9800);
        changed.items.insert("采购成本".into(), -4200);
        let history = save_profit_log(&mut c, "2026-11-15", changed.clone()).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].snapshot, changed);
        assert_eq!(history[1].snapshot, baseline);
        assert_eq!(save_profit_log(&mut c, "2026-11-08", changed).unwrap().len(), 1);
    }
    #[test]
    fn calendar_week_has_seven_days_across_month_boundary() {
        let range=week_range("2026-10-04").unwrap();
        assert_eq!(range.from,"2026-09-28"); assert_eq!(range.to,"2026-10-04");
        assert!(week_range("2026-10-05").is_err());
    }
    #[test]
    fn combines_daily_ad_levels_without_double_counting() {
        let c=Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE sales_daily(day TEXT,sku TEXT,revenue REAL,ordered_units INTEGER); CREATE TABLE ad_daily(day TEXT,sku TEXT,spend REAL,revenue REAL,orders INTEGER); INSERT INTO sales_daily VALUES('2026-10-05','A',1000,10),('2026-10-11','A',500,5),('2026-10-12','A',99999,999); INSERT INTO ad_daily VALUES('2026-10-05','',-100,400,4),('2026-10-05','A',100,400,4),('2026-10-06','A',50,200,2),('2026-10-12','',999,999,999);").unwrap();
        let (revenue,units,spend,ad_revenue,ad_units,_,_)=weekly_totals(&c,&week_range("2026-10-11").unwrap()).unwrap();
        assert_eq!((revenue,units,spend,ad_revenue,ad_units),(1500.0,15,150.0,600.0,6));
        assert_eq!(spend/ad_revenue*100.0,25.0); assert_eq!(spend/revenue*100.0,10.0);
    }
    #[test]
    fn daily_details_reconcile_and_unattributed_product_ads_stay_missing() {
        let c=Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE sales_daily(day TEXT,sku TEXT,revenue REAL,ordered_units INTEGER); CREATE TABLE ad_daily(day TEXT,sku TEXT,spend REAL,revenue REAL,orders INTEGER); CREATE TABLE products(sku TEXT,offer_id TEXT,name TEXT); INSERT INTO products VALUES('A','OFFER-A','Product A'),('B','OFFER-B','Product B'); INSERT INTO sales_daily VALUES('2026-10-05','A',1000,10),('2026-10-07','B',500,5); INSERT INTO ad_daily VALUES('2026-10-05','',100,400,4),('2026-10-05','A',100,400,4),('2026-10-06','A',50,200,2);").unwrap();
        let range=week_range("2026-10-11").unwrap();
        let totals=weekly_totals(&c,&range).unwrap();
        let (daily,products)=weekly_details(&c,&range).unwrap();
        assert_eq!(daily.len(),7);
        assert_eq!(daily.iter().map(|r|r.revenue).sum::<f64>(),totals.0);
        assert_eq!(daily.iter().map(|r|r.ad_spend).sum::<f64>(),totals.2);
        assert_eq!(daily.iter().map(|r|r.ad_revenue).sum::<f64>(),totals.3);
        assert_eq!(daily.iter().map(|r|r.ad_units).sum::<i64>(),totals.4);
        let ads_only=daily.iter().find(|r|r.day=="2026-10-06").unwrap();
        assert_eq!(ads_only.revenue,0.0);assert_eq!(ads_only.tacos,None);assert_eq!(ads_only.acos,Some(25.0));
        let unallocated=products.iter().find(|r|r.sku=="B").unwrap();
        assert_eq!(unallocated.ad_revenue,None);assert_eq!(unallocated.ad_units,None);assert_eq!(unallocated.tacos,None);
    }
}
