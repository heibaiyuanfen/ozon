use rusqlite::{params, Connection};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NetDay {
    pub day: String,
    pub cancellation_complete: bool,
    pub cancellation_synced_at: Option<String>,
    pub revenue: f64,
    pub units: i64,
    pub cancellation_units: i64,
    pub cancellation_amount: f64,
    pub net_revenue: f64,
    pub net_units: i64,
    pub ad_spend: f64,
    pub ad_revenue: f64,
    pub estimated_net_ad_revenue: f64,
    pub estimated_acos: Option<f64>,
    pub tacos: Option<f64>,
    pub estimated_products: i64,
    pub unmatched_units: i64,
    pub products: Vec<NetProduct>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NetProduct {
    sku: String,
    offer_id: String,
    name: String,
    revenue: f64,
    units: i64,
    cancellation_units: i64,
    cancellation_amount: f64,
    net_revenue: f64,
    net_units: i64,
    amount_basis: String,
}

pub(crate) fn read(c: &Connection, from: &str, to: &str, divisor: f64) -> Result<Vec<NetDay>, String> {
    crate::posting_sync::ensure_schema(c)?;
    // Events and analytics cancellations describe the same orders. Never add both.
    let sql = "WITH s AS (SELECT day,sku,SUM(revenue) revenue,SUM(ordered_units) units,SUM(cancellations) cancels,MAX(product_name) name FROM sales_daily WHERE day BETWEEN ?1 AND ?2 GROUP BY day,sku),
      e AS (SELECT e.day,e.sku,SUM(e.quantity) cancels,SUM(CASE WHEN r.order_price>0 AND r.quantity=e.quantity THEN e.quantity ELSE 0 END) priced_units,SUM(CASE WHEN r.order_price>0 AND r.quantity=e.quantity THEN e.quantity*r.order_price ELSE 0 END) amount FROM cancellation_events e LEFT JOIN posting_routes r ON e.event_id='posting:'||r.event_id WHERE e.day BETWEEN ?1 AND ?2 GROUP BY e.day,e.sku),
      keys AS (SELECT day,sku FROM s UNION SELECT day,sku FROM e)
      SELECT k.day,k.sku,COALESCE(p.offer_id,''),COALESCE(NULLIF(p.name,''),s.name,''),COALESCE(s.revenue,0),COALESCE(s.units,0),MAX(COALESCE(s.cancels,0),COALESCE(e.cancels,0)),COALESCE(e.priced_units,0),COALESCE(e.amount,0)
      FROM keys k LEFT JOIN s ON s.day=k.day AND s.sku=k.sku LEFT JOIN e ON e.day=k.day AND e.sku=k.sku LEFT JOIN products p ON p.sku=k.sku ORDER BY k.day,k.sku";
    let mut stmt = c.prepare(sql).map_err(|e| e.to_string())?;
    let rows = stmt.query_map(params![from,to], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,f64>(4)?,r.get::<_,i64>(5)?,r.get::<_,i64>(6)?,r.get::<_,i64>(7)?,r.get::<_,f64>(8)?))).map_err(|e|e.to_string())?;
    let mut days = BTreeMap::<String,NetDay>::new();
    for row in rows {
        let (day,sku,offer_id,name,revenue,units,cancels,priced_units,amount) = row.map_err(|e|e.to_string())?;
        let revenue = revenue.max(0.0);
        let units = units.max(0);
        let cancels = cancels.max(0);
        let deducted_units = cancels.min(units);
        let exact = cancels == priced_units && cancels <= units && amount <= revenue + 0.01;
        let deduction = if cancels == 0 { 0.0 } else if exact { amount } else if units > 0 { revenue * deducted_units as f64 / units as f64 } else { 0.0 };
        let item = days.entry(day.clone()).or_insert_with(|| NetDay { day, ..Default::default() });
        item.revenue += revenue / divisor;
        item.units += units;
        item.cancellation_units += cancels;
        item.cancellation_amount += deduction / divisor;
        item.net_revenue += (revenue - deduction).max(0.0) / divisor;
        item.net_units += units - deducted_units;
        item.estimated_products += i64::from(cancels > 0 && !exact);
        item.unmatched_units += cancels - deducted_units;
        item.products.push(NetProduct { sku,offer_id,name,revenue:revenue/divisor,units,cancellation_units:cancels,cancellation_amount:deduction/divisor,net_revenue:(revenue-deduction).max(0.0)/divisor,net_units:units-deducted_units,amount_basis:if cancels==0 {"无取消"} else if exact {"取消订单金额"} else if units>0 {"当天商品均价估算"} else {"缺少当天销售，无法扣减"}.into() });
    }
    // Select advertising level separately for each day, avoiding store/SKU duplication.
    let mut ads = c.prepare("SELECT day,SUM(ABS(spend)),SUM(revenue) FROM ad_daily a WHERE day BETWEEN ?1 AND ?2 AND (sku='' OR NOT EXISTS(SELECT 1 FROM ad_daily b WHERE b.day=a.day AND b.sku='')) GROUP BY day").map_err(|e|e.to_string())?;
    let rows = ads.query_map(params![from,to], |r| Ok((r.get::<_,String>(0)?,r.get::<_,f64>(1)?,r.get::<_,f64>(2)?))).map_err(|e|e.to_string())?;
    for row in rows {
        let (day,spend,revenue) = row.map_err(|e|e.to_string())?;
        let item = days.entry(day.clone()).or_insert_with(||NetDay {day,..Default::default()});
        item.ad_spend = spend / divisor;
        item.ad_revenue = revenue / divisor;
    }
    let mut coverage=c.prepare("SELECT day,COUNT(DISTINCT scheme),MIN(synced_at) FROM posting_sync_coverage WHERE day BETWEEN ?1 AND ?2 AND status='success' AND julianday(synced_at)>=julianday('now','-1 day') AND scheme IN('FBS','FBO') GROUP BY day").map_err(|e|e.to_string())?;
    let coverage_rows=coverage.query_map(params![from,to],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,String>(2)?))).map_err(|e|e.to_string())?;
    for row in coverage_rows {let (day,schemes,at)=row.map_err(|e|e.to_string())?;if let Some(item)=days.get_mut(&day){item.cancellation_complete=schemes==2;item.cancellation_synced_at=Some(at);}}
    for item in days.values_mut() {
        // No order-level advertising link exists. Proportional adjustment is explicitly an estimate.
        let retained = if item.revenue>0.0 {item.net_revenue/item.revenue} else {1.0};
        item.estimated_net_ad_revenue = item.ad_revenue * retained;
        item.estimated_acos = (item.estimated_net_ad_revenue>0.0).then(||item.ad_spend/item.estimated_net_ad_revenue*100.0);
        item.tacos = (item.net_revenue>0.0).then(||item.ad_spend/item.net_revenue*100.0);
    }
    Ok(days.into_values().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_prices_fallback_and_daily_ads_do_not_double_count() {
        let c=Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE sales_daily(day TEXT,sku TEXT,revenue REAL,ordered_units INTEGER,cancellations INTEGER,product_name TEXT);CREATE TABLE cancellation_events(event_id TEXT,day TEXT,sku TEXT,quantity INTEGER);CREATE TABLE posting_routes(event_id TEXT,order_price REAL,quantity INTEGER);CREATE TABLE products(sku TEXT,offer_id TEXT,name TEXT);CREATE TABLE ad_daily(day TEXT,sku TEXT,spend REAL,revenue REAL);
        INSERT INTO sales_daily VALUES('2026-10-14','A',1000,10,2,'A'),('2026-10-14','B',600,3,1,'B'),('2026-10-13','A',200,2,0,'A');
        INSERT INTO cancellation_events VALUES('posting:a','2026-10-14','A',2),('posting:missing','2026-10-14','C',4);
        INSERT INTO posting_routes VALUES('a',80,2);
        INSERT INTO ad_daily VALUES('2026-10-14','',100,500),('2026-10-14','A',100,500),('2026-10-13','A',20,100);").unwrap();
        let result=read(&c,"2026-10-13","2026-10-14",1.0).unwrap();
        assert_eq!(result[0].ad_spend,20.0);
        let d=&result[1];
        assert_eq!(d.cancellation_units,7);
        assert_eq!(d.net_units,10);
        assert_eq!(d.cancellation_amount,360.0);
        assert_eq!(d.net_revenue,1240.0);
        assert_eq!(d.ad_spend,100.0);
        assert_eq!(d.unmatched_units,4);
        assert_eq!(d.estimated_products,2);
        assert!((d.tacos.unwrap()-100.0/1240.0*100.0).abs()<1e-9);
        assert!((d.estimated_acos.unwrap()-100.0/387.5*100.0).abs()<1e-9);
        let converted=read(&c,"2026-10-14","2026-10-14",10.0).unwrap();
        assert_eq!(converted[0].net_revenue,124.0);
        assert_eq!(converted[0].tacos,d.tacos);
    }
    #[test]
    fn all_cancelled_zero_denominators_and_excess_units_are_safe() {
        let c=Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE sales_daily(day TEXT,sku TEXT,revenue REAL,ordered_units INTEGER,cancellations INTEGER,product_name TEXT);CREATE TABLE cancellation_events(event_id TEXT,day TEXT,sku TEXT,quantity INTEGER);CREATE TABLE posting_routes(event_id TEXT,order_price REAL,quantity INTEGER);CREATE TABLE products(sku TEXT,offer_id TEXT,name TEXT);CREATE TABLE ad_daily(day TEXT,sku TEXT,spend REAL,revenue REAL);INSERT INTO sales_daily VALUES('2026-10-14','A',100,1,3,'A');INSERT INTO ad_daily VALUES('2026-10-14','',10,80);").unwrap();
        let d=read(&c,"2026-10-14","2026-10-14",1.0).unwrap().remove(0);
        assert_eq!(d.net_units,0);assert_eq!(d.net_revenue,0.0);assert_eq!(d.unmatched_units,2);assert!(d.tacos.is_none());assert!(d.estimated_acos.is_none());
    }
}
