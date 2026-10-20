//! Finance posting-date ledger. Net amounts already contain platform fees.
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SettlementReport {
    pub net_cny: f64,
    pub sale_cny: f64,
    pub refunds_cny: f64,
    pub commission_cny: f64,
    pub delivery_cny: f64,
    pub return_delivery_cny: f64,
    pub advertising_cny: f64,
    pub other_cny: f64,
    pub known_purchase_cny: f64,
    pub purchase_cny: Option<f64>,
    pub profit_before_return_recovery_cny: Option<f64>,
    pub profit_cny: Option<f64>,
    pub operation_count: usize,
    pub missing_cost_orders: usize,
    pub return_review_orders: usize,
    pub rows: Vec<SettlementOrder>,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SettlementOrder {
    pub key: String,
    pub posting_number: String,
    pub first_day: String,
    pub last_day: String,
    pub net_cny: f64,
    pub sale_cny: f64,
    pub refunds_cny: f64,
    pub commission_cny: f64,
    pub delivery_cny: f64,
    pub return_delivery_cny: f64,
    pub advertising_cny: f64,
    pub other_cny: f64,
    pub known_purchase_cny: f64,
    pub purchase_cny: Option<f64>,
    pub profit_before_return_recovery_cny: Option<f64>,
    pub profit_cny: Option<f64>,
    pub return_review: bool,
    pub issues: Vec<String>,
    pub products: Vec<SettlementProduct>,
    pub related_products: Vec<SettlementRelatedProduct>,
    pub entries: Vec<SettlementEntry>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SettlementRelatedProduct {
    pub sku: String,
    pub offer_id: String,
    pub product_name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SettlementProduct {
    pub sku: String,
    pub offer_id: String,
    pub quantity: Option<i64>,
    pub unit_cost_cny: Option<f64>,
    pub purchase_cny: Option<f64>,
    pub quantity_source: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SettlementEntry {
    pub operation_id: String,
    pub day: String,
    pub operation_type: String,
    pub net_cny: f64,
    pub sale_cny: f64,
    pub commission_cny: f64,
    pub delivery_cny: f64,
    pub return_delivery_cny: f64,
    pub advertising_cny: f64,
    pub other_cny: f64,
}

fn item_quantities(v: &Value) -> BTreeMap<String, Option<i64>> {
    let mut result = BTreeMap::new();
    let items = v.pointer("/accrual_raw/posting/products").and_then(Value::as_array)
        .or_else(|| v.get("items").and_then(Value::as_array));
    for item in items.into_iter().flatten() {
        let sku = super::finance_identifier(item.get("sku"));
        if sku.is_empty() { continue; }
        let quantity = item.get("quantity").and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok())).filter(|q| *q > 0);
        result.entry(sku).and_modify(|old: &mut Option<i64>| *old = old.zip(quantity).and_then(|(a,b)| a.checked_add(b))).or_insert(quantity);
    }
    result
}

pub(crate) fn read(c: &Connection, from: &str, to: &str, rate: f64) -> Result<SettlementReport, String> {
    if !rate.is_finite() || rate <= 0.0 { return Err("Finance 核算汇率必须为正数".into()); }
    let mut stmt = c.prepare("SELECT operation_id,substr(operation_date,1,10),operation_type,posting_number,sku,amount,accruals_for_sale,sale_commission,raw_json FROM finance_transactions WHERE substr(operation_date,1,10)<=?1 ORDER BY operation_date,operation_id").map_err(|e|e.to_string())?;
    let raw = stmt.query_map([to], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,f64>(5)?,r.get::<_,f64>(6)?,r.get::<_,f64>(7)?,r.get::<_,String>(8)?)))
        .map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
    let mut orders = BTreeMap::<String,SettlementOrder>::new();
    let mut costed = BTreeSet::<(String,String)>::new();
    for (id,day,op,posting,stored_sku,amount,accrual,commission,text) in raw {
        let parsed = serde_json::from_str::<Value>(&text);
        let v = parsed.as_ref().cloned().unwrap_or_default();
        if v.pointer("/accrual_raw/total_amount/currency").and_then(Value::as_str).is_some_and(|s| s != "RUB") {
            return Err(format!("Finance {id} 不是 RUB，需先核对币种，不能混币汇总"));
        }
        let delivered = op.to_lowercase().contains("deliveredtocustomer") && accrual > 0.0;
        let key = if posting.is_empty() {format!("operation:{id}")} else {format!("posting:{posting}")};
        let mut products = Vec::new();
        let mut issues = Vec::new();
        if delivered {
            let mut quantities = item_quantities(&v);
            if quantities.is_empty() && !stored_sku.is_empty() { quantities.insert(stored_sku.clone(),None); }
            if quantities.is_empty() { issues.push("销售入账缺少 SKU，无法关联采购成本".into()); }
            if posting.is_empty() { issues.push("销售入账缺少订单号，无法排除重复确认成本".into()); }
            for (sku, explicit_qty) in quantities {
                // Multiple fees/adjustments for a posting must never deduct its goods twice.
                if !costed.insert((key.clone(),sku.clone())) {
                    if day.as_str() >= from { issues.push(format!("订单 SKU {sku} 存在多笔正销售入账，未重复扣成本；需核对是否补差或分批确认")); }
                    continue;
                }
                if day.as_str() < from { continue; }
                let route_qty = if explicit_qty.is_none() && !posting.is_empty() {
                    c.query_row("SELECT CASE WHEN COUNT(DISTINCT quantity)=1 THEN MAX(quantity) END FROM posting_routes WHERE posting_number=?1 AND sku=?2",params![posting,sku],|r| r.get::<_,Option<i64>>(0)).map_err(|e|e.to_string())?.filter(|q|*q>0)
                } else { None };
                let qty = explicit_qty.or(route_qty);
                let cost = c.query_row("SELECT unit_cost_cny FROM product_costs WHERE sku=?1",[&sku],|r| r.get::<_,Option<f64>>(0)).ok().flatten().filter(|x| x.is_finite() && *x>=0.0);
                let offer = c.query_row("SELECT offer_id FROM products WHERE sku=?1",[&sku],|r|r.get::<_,String>(0)).unwrap_or_default();
                if qty.is_none() { issues.push(format!("SKU {sku} 缺少明确数量，未按一条流水当一件")); }
                if cost.is_none() { issues.push(format!("SKU {sku} 缺少有效采购成本")); }
                products.push(SettlementProduct {sku,offer_id:offer,quantity:qty,unit_cost_cny:cost,purchase_cny:qty.zip(cost).map(|(q,p)|q as f64*p),quantity_source:if explicit_qty.is_some(){"Finance 商品数量"}else if route_qty.is_some(){"订单 SKU 数量"}else{"数量待核对"}.into()});
            }
        }
        if day.as_str() < from { continue; }
        let row = orders.entry(key.clone()).or_insert_with(||SettlementOrder {key,posting_number:posting,first_day:day.clone(),..Default::default()});
        row.last_day = day.clone();
        // Identity is independent of cost recognition: fee/refund-only groups
        // still need to identify their products without deducting goods again.
        let mut related = item_quantities(&v).into_keys().collect::<BTreeSet<_>>();
        if !stored_sku.is_empty() { related.insert(stored_sku.clone()); }
        if !row.posting_number.is_empty() {
            let mut routes = c.prepare("SELECT DISTINCT sku FROM posting_routes WHERE posting_number=?1 AND sku<>''").map_err(|e|e.to_string())?;
            for sku in routes.query_map([&row.posting_number],|r|r.get::<_,String>(0)).map_err(|e|e.to_string())? {
                related.insert(sku.map_err(|e|e.to_string())?);
            }
        }
        for sku in related {
            if row.related_products.iter().any(|p|p.sku==sku) {continue;}
            let (offer_id,product_name) = c.query_row("SELECT offer_id,name FROM products WHERE sku=?1",[&sku],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).unwrap_or_default();
            row.related_products.push(SettlementRelatedProduct{sku,offer_id,product_name});
        }
        row.issues.extend(issues);
        row.products.extend(products);
        if parsed.is_err() { row.issues.push(format!("流水 {id} 原始数据无法解析，费用分类待核对")); }
        if accrual > 0.0 && !delivered { row.issues.push(format!("流水 {id} 有正销售应计但不是确认收货类型，成本待核对")); }
        row.return_review |= accrual < 0.0;
        let mut delivery = 0.0;
        let mut returns = 0.0;
        let mut advertising = 0.0;
        let mut classified = 0.0;
        let mut other = 0.0;
        for service in v.get("services").and_then(Value::as_array).into_iter().flatten() {
            let price = super::value_number(service.get("price"));
            classified += price;
            match category(&super::json_text(service.get("name"))) {
                "delivery" => delivery += price,
                "return_logistics" => returns += price,
                "advertising" => advertising += price,
                _ => other += price,
            }
        }
        let residual = amount - accrual - commission - classified;
        match category(&op) {
            "delivery" => delivery += residual,
            "return_logistics" => returns += residual,
            "advertising" => advertising += residual,
            _ => other += residual,
        }
        let e = SettlementEntry {operation_id:id,day,operation_type:op,net_cny:amount/rate,sale_cny:accrual/rate,commission_cny:commission/rate,delivery_cny:delivery/rate,return_delivery_cny:returns/rate,advertising_cny:advertising/rate,other_cny:other/rate};
        row.net_cny += e.net_cny;
        row.sale_cny += e.sale_cny.max(0.0);
        row.refunds_cny += e.sale_cny.min(0.0);
        row.commission_cny += e.commission_cny;
        row.delivery_cny += e.delivery_cny;
        row.return_delivery_cny += e.return_delivery_cny;
        row.advertising_cny += e.advertising_cny;
        row.other_cny += e.other_cny;
        row.entries.push(e);
    }
    let mut report = SettlementReport::default();
    for mut row in orders.into_values() {
        row.issues.sort(); row.issues.dedup();
        row.known_purchase_cny = row.products.iter().filter_map(|p|p.purchase_cny).sum();
        row.purchase_cny = row.issues.is_empty().then_some(row.known_purchase_cny);
        row.profit_before_return_recovery_cny = row.purchase_cny.map(|p|row.net_cny-p);
        row.profit_cny = if row.return_review { None } else { row.profit_before_return_recovery_cny };
        report.net_cny += row.net_cny;
        report.sale_cny += row.sale_cny;
        report.refunds_cny += row.refunds_cny;
        report.commission_cny += row.commission_cny;
        report.delivery_cny += row.delivery_cny;
        report.return_delivery_cny += row.return_delivery_cny;
        report.advertising_cny += row.advertising_cny;
        report.other_cny += row.other_cny;
        report.known_purchase_cny += row.known_purchase_cny;
        report.operation_count += row.entries.len();
        report.missing_cost_orders += usize::from(row.purchase_cny.is_none());
        report.return_review_orders += usize::from(row.return_review);
        report.rows.push(row);
    }
    report.rows.sort_by(|a,b| b.last_day.cmp(&a.last_day).then(a.key.cmp(&b.key)));
    if report.operation_count>0 && report.missing_cost_orders==0 {
        report.purchase_cny=Some(report.known_purchase_cny);
        report.profit_before_return_recovery_cny=Some(report.net_cny-report.known_purchase_cny);
        if report.return_review_orders==0 { report.profit_cny=report.profit_before_return_recovery_cny; }
    }
    Ok(report)
}

fn category(name: &str) -> &'static str {
    // These are real cross-border Finance deductions, including charges that
    // have no items[] and can only be matched by posting number.
    if name.to_lowercase().contains("starsmembership") { "advertising" }
    else { super::finance_service_category(name).0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn database() -> Connection {
        let c=Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE finance_transactions(operation_id TEXT PRIMARY KEY,operation_date TEXT,operation_type TEXT,posting_number TEXT,sku TEXT,amount REAL,accruals_for_sale REAL,sale_commission REAL,raw_json TEXT); CREATE TABLE posting_routes(posting_number TEXT,sku TEXT,quantity INTEGER); CREATE TABLE product_costs(sku TEXT,unit_cost_cny REAL); CREATE TABLE products(sku TEXT,offer_id TEXT); INSERT INTO product_costs VALUES('A',20),('B',5);").unwrap(); c
    }
    fn put(c: &Connection,id:&str,day:&str,op:&str,posting:&str,amount:f64,sale:f64,commission:f64,raw:Value) {
        c.execute("INSERT INTO finance_transactions VALUES(?1,?2,?3,?4,'',?5,?6,?7,?8)",params![id,day,op,posting,amount,sale,commission,raw.to_string()]).unwrap();
    }
    fn close(a:f64,b:f64) { assert!((a-b).abs()<1e-8,"{a} != {b}"); }
    #[test]
    fn actual_net_already_contains_freight_and_ads_and_quantity_is_not_operation_count() {
        let c=database();
        put(&c,"sale","2026-10-01","OperationAgentDeliveredToCustomer","P",900.,1000.,-100.,serde_json::json!({"items":[{"sku":"A","quantity":3}]}));
        put(&c,"freight","2026-10-01","RfbsGlobalDelivery","P",-200.,0.,0.,serde_json::json!({"services":[{"name":"RfbsGlobalDelivery","price":-200}]}));
        put(&c,"ad","2026-10-01","PayPerClick","",-50.,0.,0.,serde_json::json!({}));
        put(&c,"adjust","2026-10-02","Adjustment","P",5.,0.,0.,serde_json::json!({}));
        let r=read(&c,"2026-10-01","2026-10-20",10.).unwrap();
        close(r.net_cny,65.5); close(r.purchase_cny.unwrap(),60.); close(r.profit_cny.unwrap(),5.5);
        close(r.delivery_cny,-20.); close(r.advertising_cny,-5.);
        close(r.sale_cny+r.refunds_cny+r.commission_cny+r.delivery_cny+r.return_delivery_cny+r.advertising_cny+r.other_cny,r.net_cny);
        assert_eq!(r.operation_count,4);
        assert_eq!(r.rows.iter().find(|o|o.posting_number=="P").unwrap().entries.len(),3);
    }
    #[test]
    fn fee_only_group_identifies_products_without_recognizing_purchase_cost() {
        let c=database();
        c.execute_batch("ALTER TABLE products ADD COLUMN name TEXT;INSERT INTO products VALUES('A','OFFER-A','Product A');INSERT INTO posting_routes VALUES('P','A',2);").unwrap();
        put(&c,"fee","2026-10-01","RfbsGlobalDelivery","P",-10.,0.,0.,serde_json::json!({}));
        let r=read(&c,"2026-10-01","2026-10-20",10.).unwrap();
        assert_eq!(r.rows[0].related_products.len(),1);
        assert_eq!(r.rows[0].related_products[0].sku,"A");
        assert_eq!(r.rows[0].related_products[0].offer_id,"OFFER-A");
        assert_eq!(r.rows[0].related_products[0].product_name,"Product A");
        assert!(r.rows[0].products.is_empty());
        close(r.known_purchase_cny,0.);
        close(r.net_cny,-1.);
    }
    #[test]
    fn later_period_fees_and_duplicate_sale_adjustments_do_not_deduct_goods_again() {
        let c=database();
        let raw=serde_json::json!({"items":[{"sku":"A","quantity":2}]});
        put(&c,"first","2026-09-30","OperationAgentDeliveredToCustomer","P",90.,100.,-10.,raw.clone());
        put(&c,"late","2026-10-01","RfbsGlobalDelivery","P",-10.,0.,0.,serde_json::json!({}));
        put(&c,"correction","2026-10-02","OperationAgentDeliveredToCustomer","P",1.,1.,0.,raw);
        let r=read(&c,"2026-10-01","2026-10-20",10.).unwrap();
        close(r.known_purchase_cny,0.); close(r.net_cny,-0.9);
        assert!(r.purchase_cny.is_none()); assert!(r.profit_cny.is_none());
        assert!(r.rows[0].products.is_empty());
    }
    #[test]
    fn unknown_quantity_never_becomes_one_and_blocks_total_profit() {
        let c=database();
        put(&c,"sale","2026-10-01","OperationAgentDeliveredToCustomer","P",90.,100.,-10.,serde_json::json!({"items":[{"sku":"A"}]}));
        let r=read(&c,"2026-10-01","2026-10-20",10.).unwrap();
        assert!(r.profit_cny.is_none()); assert_eq!(r.missing_cost_orders,1);
        c.execute("INSERT INTO posting_routes VALUES('P','A',2)",[]).unwrap();
        let r=read(&c,"2026-10-01","2026-10-20",10.).unwrap();
        close(r.purchase_cny.unwrap(),40.);
        assert_eq!(r.rows[0].products[0].quantity_source,"订单 SKU 数量");
    }
    #[test]
    fn refunds_reduce_net_once_and_do_not_assume_a_stock_recovery() {
        let c=database();
        put(&c,"sale","2026-10-01","OperationAgentDeliveredToCustomer","P",90.,100.,-10.,serde_json::json!({"items":[{"sku":"A","quantity":1}]}));
        put(&c,"refund","2026-10-02","OperationItemReturn","P",-90.,-100.,10.,serde_json::json!({}));
        put(&c,"cancel-fee","2026-10-03","ReturnFlowLogistic","CANCELLED",-5.,0.,0.,serde_json::json!({}));
        let r=read(&c,"2026-10-01","2026-10-20",10.).unwrap();
        close(r.net_cny,-0.5); close(r.purchase_cny.unwrap(),20.);
        close(r.profit_before_return_recovery_cny.unwrap(),-20.5);
        assert!(r.profit_cny.is_none()); assert_eq!(r.return_review_orders,1);
    }
    #[test]
    fn multi_sku_order_uses_each_explicit_quantity_and_groups_shared_fees_without_allocation() {
        let c=database();
        put(&c,"sale","2026-10-01","OperationAgentDeliveredToCustomer","P",90.,100.,-10.,serde_json::json!({"accrual_raw":{"posting":{"products":[{"sku":"A","quantity":2},{"sku":"B","quantity":3}]}},"items":[{"sku":"A","quantity":99}]}));
        put(&c,"delivery","2026-10-01","RfbsGlobalDelivery","P",-10.,0.,0.,serde_json::json!({}));
        let r=read(&c,"2026-10-01","2026-10-20",10.).unwrap();
        close(r.purchase_cny.unwrap(),55.); assert_eq!(r.rows.len(),1); assert_eq!(r.rows[0].products.len(),2);
    }
    #[test]
    #[ignore = "explicit read-only audit against the selected local shop"]
    fn readonly_shop_audit() {
        let path=std::env::var("OZON_FINANCE_AUDIT_DB").unwrap();
        let c=Connection::open_with_flags(path,rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let r=read(&c,"2026-10-01","2026-10-20",12.).unwrap();
        close(r.sale_cny+r.refunds_cny+r.commission_cny+r.delivery_cny+r.return_delivery_cny+r.advertising_cny+r.other_cny,r.net_cny);
        println!("{}",serde_json::json!({"operations":r.operation_count,"netCny":r.net_cny,"knownPurchaseCny":r.known_purchase_cny,"missingCostOrders":r.missing_cost_orders,"returnReviewOrders":r.return_review_orders,"profitBeforeReturnRecoveryCny":r.profit_before_return_recovery_cny,"actualFreightCny":r.delivery_cny,"actualAdsCny":r.advertising_cny}));
    }
}
