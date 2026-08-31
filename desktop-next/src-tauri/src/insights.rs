use crate::{db, rub_per_cny_for, seller_post, AppState};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::State;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ClusterShare {
    cluster: String,
    orders: i64,
    share: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductTrend {
    day: String,
    units: i64,
    revenue: f64,
    ad_spend: f64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductWeight {
    cluster: String,
    historical_share: f64,
    configured_weight: f64,
    orders: i64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductDetail {
    sku: String,
    offer_id: String,
    name: String,
    trend: Vec<ProductTrend>,
    clusters: Vec<ProductWeight>,
    price: Option<ProductPrice>,
    price_logs: Vec<ProductPriceLog>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProductPrice {
    sku: String,
    offer_id: String,
    product_id: String,
    currency_code: String,
    price: f64,
    old_price: f64,
    min_price: f64,
    marketing_seller_price: Option<f64>,
    retail_price: Option<f64>,
    net_price: Option<f64>,
    synced_at: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductPriceLog {
    id: i64,
    before_price: f64,
    requested_price: f64,
    verified_price: Option<f64>,
    status: String,
    message: String,
    created_at: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductPriceUpdate {
    sku: String,
    price: f64,
    old_price: f64,
    min_price: f64,
    currency_code: String,
}

fn validate_price_update(form: &ProductPriceUpdate) -> Result<(), String> {
    if !form.price.is_finite() || form.price <= 0.0 || form.old_price < 0.0 || form.min_price < 0.0
    {
        return Err("售价必须大于 0，划线价和最低价不能小于 0".into());
    }
    if form.old_price > 0.0 && form.old_price <= form.price {
        return Err("划线价必须高于当前售价；不使用划线价时请填写 0".into());
    }
    if form.min_price > 0.0 && form.price < form.min_price {
        return Err("售价不能低于最低价".into());
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InsightRow {
    id: String,
    name: String,
    skus: Vec<String>,
    offer_ids: Vec<String>,
    day_units: i64,
    week_units: i64,
    month_units: i64,
    day_revenue: f64,
    week_revenue: f64,
    month_revenue: f64,
    day_ad_spend: f64,
    week_ad_spend: f64,
    month_ad_spend: f64,
    day_ad_orders: i64,
    week_ad_orders: i64,
    month_ad_orders: i64,
    clusters: Vec<ClusterShare>,
    ad_source: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductAnalysisRow {
    sku: String,
    offer_id: String,
    name: String,
    day: String,
    impressions: i64,
    clicks: i64,
    ad_orders: i64,
    ad_revenue: f64,
    ad_spend: f64,
    total_units: i64,
    total_revenue: f64,
    price: Option<f64>,
    stock: i64,
    ctr: Option<f64>,
    cpc: Option<f64>,
    cvr: Option<f64>,
    cpa: Option<f64>,
    roas: Option<f64>,
    cpa_limit: Option<f64>,
    break_even_roas: Option<f64>,
    purchase_cost: Option<f64>,
    first_mile_cost: Option<f64>,
    platform_unit_cost: Option<f64>,
    inventory_days: Option<f64>,
    incremental_roas: Option<f64>,
    clicks_3d: i64,
    units_7d: i64,
    previous_units_7d: i64,
    revenue_7d: f64,
    previous_revenue_7d: f64,
    ad_spend_7d: f64,
    previous_ad_spend_7d: f64,
    ad_orders_7d: i64,
    previous_ad_orders_7d: i64,
    ad_revenue_7d: f64,
    previous_ad_revenue_7d: f64,
    sample_ready: bool,
    overall_score: i64,
    traffic_score: i64,
    conversion_score: i64,
    profit_score: i64,
    inventory_score: i64,
    scale_score: i64,
    grade: String,
    confidence: String,
    ad_source: String,
    rule_version: String,
    diagnosis: String,
    action: String,
}

#[tauri::command]
pub fn product_analysis(
    to: String,
    query: String,
    state: State<AppState>,
) -> Result<Vec<ProductAnalysisRow>, String> {
    let c = db(&state)?;
    ensure(&c)?;
    let rub_per_cny = rub_per_cny_for(&state, &c)?;
    let needle = format!("%{}%", query.trim());
    let mut stmt = c.prepare("WITH known AS(SELECT sku FROM products UNION SELECT sku FROM sales_daily),candidate AS(SELECT DISTINCT a.campaign_id,p.sku FROM ad_daily a JOIN products p ON a.sku='' AND ((length(COALESCE(p.offer_id,''))>=4 AND instr(lower(a.campaign_name),lower(p.offer_id))>0)OR(length(p.sku)>=6 AND instr(a.campaign_name,p.sku)>0))),cmap AS(SELECT campaign_id,MIN(sku)sku FROM candidate GROUP BY campaign_id HAVING COUNT(DISTINCT sku)=1),effective_ads AS(SELECT day,sku,impressions,clicks,orders,revenue,ABS(spend) spend,0 fallback FROM ad_daily WHERE sku<>'' UNION ALL SELECT a.day,m.sku,CASE WHEN EXISTS(SELECT 1 FROM ad_daily x WHERE x.day=a.day AND x.campaign_id=a.campaign_id AND x.sku=m.sku)THEN 0 ELSE a.impressions END,CASE WHEN EXISTS(SELECT 1 FROM ad_daily x WHERE x.day=a.day AND x.campaign_id=a.campaign_id AND x.sku=m.sku)THEN 0 ELSE a.clicks END,CASE WHEN EXISTS(SELECT 1 FROM ad_daily x WHERE x.day=a.day AND x.campaign_id=a.campaign_id AND x.sku=m.sku AND (x.orders<>0 OR x.revenue<>0))THEN 0 ELSE a.orders END,CASE WHEN EXISTS(SELECT 1 FROM ad_daily x WHERE x.day=a.day AND x.campaign_id=a.campaign_id AND x.sku=m.sku AND (x.orders<>0 OR x.revenue<>0))THEN 0 ELSE a.revenue END,CASE WHEN EXISTS(SELECT 1 FROM ad_daily x WHERE x.day=a.day AND x.campaign_id=a.campaign_id AND x.sku=m.sku)THEN 0 ELSE ABS(a.spend) END,CASE WHEN EXISTS(SELECT 1 FROM ad_daily x WHERE x.day=a.day AND x.campaign_id=a.campaign_id AND x.sku=m.sku)AND NOT EXISTS(SELECT 1 FROM ad_daily x WHERE x.day=a.day AND x.campaign_id=a.campaign_id AND x.sku=m.sku AND (x.orders<>0 OR x.revenue<>0))AND (a.orders<>0 OR a.revenue<>0)THEN 1 ELSE 0 END FROM ad_daily a JOIN cmap m ON m.campaign_id=a.campaign_id WHERE a.sku=''),ad AS(SELECT sku,SUM(CASE WHEN day=?1 THEN impressions ELSE 0 END) impressions,SUM(CASE WHEN day=?1 THEN clicks ELSE 0 END) clicks,SUM(CASE WHEN day=?1 THEN orders ELSE 0 END) orders,SUM(CASE WHEN day=?1 THEN revenue ELSE 0 END) revenue,SUM(CASE WHEN day=?1 THEN spend ELSE 0 END) spend,SUM(CASE WHEN day BETWEEN date(?1,'-2 day') AND ?1 THEN clicks ELSE 0 END) clicks3,SUM(CASE WHEN day=date(?1,'-1 day') THEN revenue ELSE 0 END) prev_revenue,SUM(CASE WHEN day=date(?1,'-1 day') THEN spend ELSE 0 END) prev_spend,MAX(CASE WHEN day=?1 THEN fallback ELSE 0 END) fallback,SUM(CASE WHEN day BETWEEN date(?1,'-6 day') AND ?1 THEN spend ELSE 0 END) spend7,SUM(CASE WHEN day BETWEEN date(?1,'-13 day') AND date(?1,'-7 day') THEN spend ELSE 0 END) prev_spend7,SUM(CASE WHEN day BETWEEN date(?1,'-6 day') AND ?1 THEN orders ELSE 0 END) orders7,SUM(CASE WHEN day BETWEEN date(?1,'-13 day') AND date(?1,'-7 day') THEN orders ELSE 0 END) prev_orders7,SUM(CASE WHEN day BETWEEN date(?1,'-6 day') AND ?1 THEN revenue ELSE 0 END) revenue7,SUM(CASE WHEN day BETWEEN date(?1,'-13 day') AND date(?1,'-7 day') THEN revenue ELSE 0 END) prev_revenue7 FROM effective_ads GROUP BY sku),sales AS(SELECT sku,SUM(CASE WHEN day=?1 THEN ordered_units ELSE 0 END) units,SUM(CASE WHEN day=?1 THEN revenue ELSE 0 END) revenue,SUM(CASE WHEN day BETWEEN date(?1,'-6 day') AND ?1 THEN ordered_units ELSE 0 END)/7.0 daily7,SUM(CASE WHEN day BETWEEN date(?1,'-6 day') AND ?1 THEN ordered_units ELSE 0 END) units7,SUM(CASE WHEN day BETWEEN date(?1,'-13 day') AND date(?1,'-7 day') THEN ordered_units ELSE 0 END) prev_units7,SUM(CASE WHEN day BETWEEN date(?1,'-6 day') AND ?1 THEN revenue ELSE 0 END) revenue7,SUM(CASE WHEN day BETWEEN date(?1,'-13 day') AND date(?1,'-7 day') THEN revenue ELSE 0 END) prev_revenue7 FROM sales_daily GROUP BY sku),stock AS(SELECT sku,SUM(available_stock) qty FROM inventory_stock GROUP BY sku),fp AS(SELECT sku,SUM(CASE WHEN accruals_for_sale>0 THEN accruals_for_sale ELSE 0 END) sales,SUM(ABS(sale_commission)) commission,SUM(ABS(delivery_charge)+ABS(return_delivery_charge)) logistics,COUNT(DISTINCT CASE WHEN posting_number<>'' THEN posting_number END) postings FROM finance_transactions WHERE sku<>'' AND substr(operation_date,1,10)<=?1 GROUP BY sku) SELECT k.sku,COALESCE(p.offer_id,''),COALESCE(NULLIF(p.name,''),''),COALESCE(ad.impressions,0),COALESCE(ad.clicks,0),COALESCE(ad.orders,0),COALESCE(ad.revenue,0),COALESCE(ad.spend,0),COALESCE(sales.units,0),COALESCE(sales.revenue,0),NULLIF(COALESCE(pp.price,CASE WHEN sales.units>0 THEN sales.revenue/sales.units ELSE 0 END),0),COALESCE(stock.qty,0),COALESCE(ad.clicks3,0),COALESCE(ad.prev_revenue,0),COALESCE(ad.prev_spend,0),pc.unit_cost,pc.first_mile_cost,fp.sales,fp.commission,fp.logistics,fp.postings,COALESCE(sales.daily7,0),COALESCE(ad.fallback,0),COALESCE(sales.units7,0),COALESCE(sales.prev_units7,0),COALESCE(sales.revenue7,0),COALESCE(sales.prev_revenue7,0),COALESCE(ad.spend7,0),COALESCE(ad.prev_spend7,0),COALESCE(ad.orders7,0),COALESCE(ad.prev_orders7,0),COALESCE(ad.revenue7,0),COALESCE(ad.prev_revenue7,0) FROM known k LEFT JOIN products p ON p.sku=k.sku LEFT JOIN ad ON ad.sku=k.sku LEFT JOIN sales ON sales.sku=k.sku LEFT JOIN stock ON stock.sku=k.sku LEFT JOIN product_costs pc ON pc.sku=k.sku LEFT JOIN product_price_cache pp ON pp.sku=k.sku LEFT JOIN fp ON fp.sku=k.sku WHERE (?2='%%' OR k.sku LIKE ?2 OR p.offer_id LIKE ?2 OR p.name LIKE ?2) AND (COALESCE(sales.units,0)>0 OR COALESCE(ad.impressions,0)>0 OR COALESCE(stock.qty,0)>0) ORDER BY COALESCE(sales.revenue,0) DESC,COALESCE(ad.spend,0) DESC LIMIT 1000").map_err(|e|e.to_string())?;
    let rows = stmt
        .query_map(params![to, needle], |r| {
            let sku: String = r.get(0)?;
            let impressions: i64 = r.get(3)?;
            let clicks: i64 = r.get(4)?;
            let ad_orders: i64 = r.get(5)?;
            let ad_revenue: f64 = r.get(6)?;
            let ad_spend: f64 = r.get(7)?;
            let price: Option<f64> = r.get(10)?;
            let stock: i64 = r.get(11)?;
            let clicks_3d: i64 = r.get(12)?;
            let prev_revenue: f64 = r.get(13)?;
            let prev_spend: f64 = r.get(14)?;
            let unit_cost_native: Option<f64> = r.get(15)?;
            let first_mile_native: Option<f64> = r.get(16)?;
            let (unit_cost_cny, first_mile_cny): (Option<f64>, Option<f64>) = c
                .query_row(
                    "SELECT unit_cost_cny,first_mile_cost_cny FROM product_costs WHERE sku=?1",
                    [&sku],
                    |x| Ok((x.get(0)?, x.get(1)?)),
                )
                .unwrap_or((None, None));
            let unit_cost = unit_cost_native.or(unit_cost_cny.map(|v| v * rub_per_cny));
            let first_mile = first_mile_native.or(first_mile_cny.map(|v| v * rub_per_cny));
            let fp_sales: Option<f64> = r.get(17)?;
            let fp_commission: Option<f64> = r.get(18)?;
            let fp_logistics: Option<f64> = r.get(19)?;
            let fp_postings: Option<i64> = r.get(20)?;
            let daily7: f64 = r.get(21)?;
            let ad_fallback: i64 = r.get(22)?;
            let ratio = |n: f64, d: f64| (d > 0.0).then_some(n / d);
            let platform_unit = match (price, fp_sales, fp_commission, fp_logistics, fp_postings) {
                (Some(p), Some(s), Some(cm), Some(lg), Some(posts))
                    if s > 0.0 && posts > 0 && cm / s <= 0.6 =>
                {
                    Some(p * cm / s + lg / posts as f64)
                }
                _ => None,
            };
            let cpa_limit = match (price, unit_cost, first_mile, platform_unit) {
                (Some(p), Some(cost), Some(first), Some(platform)) => {
                    Some(p - cost - first - platform)
                }
                _ => None,
            }
            .filter(|v| *v > 0.0);
            let cpa = ratio(ad_spend, ad_orders as f64);
            let roas = ratio(ad_revenue, ad_spend);
            let ctr = ratio(clicks as f64 * 100.0, impressions as f64);
            let cvr = ratio(ad_orders as f64 * 100.0, clicks as f64);
            let incremental_roas = {
                let ds = ad_spend - prev_spend;
                (ds.abs() > 0.01).then_some((ad_revenue - prev_revenue) / ds)
            };
            let diagnosis = if impressions == 0 {
                "无曝光"
            } else if ctr.unwrap_or(0.0) < 1.0 {
                "CTR 偏低"
            } else if clicks >= 30 && cvr.unwrap_or(0.0) < 3.0 {
                "CVR 偏低"
            } else if cpa.zip(cpa_limit).is_some_and(|(a, l)| a > l) {
                "CPA 超过利润上限"
            } else {
                "漏斗暂未发现明显异常"
            }
            .to_string();
            let action = if clicks_3d < 100 {
                "样本不足：保持单变量，累计至少 3 天或 100 点击"
            } else if cpa.zip(cpa_limit).is_some_and(|(a, l)| a > l) {
                "停止放量并回退最近一次变量"
            } else if incremental_roas
                .zip(price.zip(cpa_limit))
                .is_some_and(|(ir, (p, l))| l > 0.0 && ir < p / l)
            {
                "增量 ROAS 低于保本线，回退"
            } else {
                "可继续观察；每次只改预算、图片或价格中的一个"
            }
            .to_string();
            // Five transparent dimensions total 100 points. Low-sample products receive a
            // neutral conversion/scale score and are explicitly marked low confidence.
            let traffic_score = if impressions == 0 {
                0
            } else if ctr.unwrap_or(0.0) >= 3.0 {
                20
            } else if ctr.unwrap_or(0.0) >= 2.0 {
                16
            } else if ctr.unwrap_or(0.0) >= 1.0 {
                10
            } else {
                5
            };
            let conversion_score = if clicks < 20 {
                8
            } else if cvr.unwrap_or(0.0) >= 8.0 {
                20
            } else if cvr.unwrap_or(0.0) >= 5.0 {
                16
            } else if cvr.unwrap_or(0.0) >= 3.0 {
                12
            } else if cvr.unwrap_or(0.0) >= 1.0 {
                6
            } else {
                0
            };
            let profit_score = if ad_spend <= 0.0 {
                15
            } else {
                match (cpa, cpa_limit) {
                    (Some(a), Some(l)) if a <= l * 0.7 => 25,
                    (Some(a), Some(l)) if a <= l => 20,
                    (Some(a), Some(l)) if a <= l * 1.2 => 10,
                    (Some(_), Some(_)) => 0,
                    _ => 6,
                }
            };
            let inventory_days = (daily7 > 0.0).then_some(stock as f64 / daily7);
            let inventory_score = match inventory_days {
                Some(d) if (14.0..=60.0).contains(&d) => 15,
                Some(d) if (7.0..=90.0).contains(&d) => 10,
                Some(d) if d < 3.0 || d > 120.0 => 3,
                Some(_) => 7,
                None if stock == 0 => 0,
                None => 5,
            };
            let break_even_roas = price.zip(cpa_limit).and_then(|(p, l)| ratio(p, l));
            let scale_score = if clicks_3d < 100 {
                8
            } else {
                match (incremental_roas, break_even_roas) {
                    (Some(i), Some(b)) if i >= b => 20,
                    (Some(i), _) if i > 0.0 => 12,
                    (Some(_), _) => 0,
                    (None, _) => 8,
                }
            };
            let overall_score =
                traffic_score + conversion_score + profit_score + inventory_score + scale_score;
            let grade = if overall_score >= 85 {
                "优秀"
            } else if overall_score >= 70 {
                "良好"
            } else if overall_score >= 55 {
                "观察"
            } else {
                "风险"
            }
            .to_string();
            let confidence = if clicks_3d >= 100 && cpa_limit.is_some() {
                "高"
            } else if clicks_3d >= 30 {
                "中"
            } else {
                "低"
            }
            .to_string();
            Ok(ProductAnalysisRow {
                sku,
                offer_id: r.get(1)?,
                name: r.get(2)?,
                day: to.clone(),
                impressions,
                clicks,
                ad_orders,
                ad_revenue,
                ad_spend,
                total_units: r.get(8)?,
                total_revenue: r.get(9)?,
                price,
                stock,
                ctr,
                cpc: ratio(ad_spend, clicks as f64),
                cvr,
                cpa,
                roas,
                cpa_limit,
                break_even_roas,
                purchase_cost: unit_cost,
                first_mile_cost: first_mile,
                platform_unit_cost: platform_unit,
                inventory_days,
                incremental_roas,
                clicks_3d,
                units_7d: r.get(23)?,
                previous_units_7d: r.get(24)?,
                revenue_7d: r.get(25)?,
                previous_revenue_7d: r.get(26)?,
                ad_spend_7d: r.get(27)?,
                previous_ad_spend_7d: r.get(28)?,
                ad_orders_7d: r.get(29)?,
                previous_ad_orders_7d: r.get(30)?,
                ad_revenue_7d: r.get(31)?,
                previous_ad_revenue_7d: r.get(32)?,
                sample_ready: clicks_3d >= 100,
                overall_score,
                traffic_score,
                conversion_score,
                profit_score,
                inventory_score,
                scale_score,
                grade,
                confidence,
                ad_source: if ad_fallback > 0 {
                    "SKU 明细曝光/点击/花费 + 唯一匹配活动级订单/销售额补齐".into()
                } else {
                    "SKU 明细或唯一活动名称匹配".into()
                },
                rule_version: "product-analysis-v2.1.0".into(),
                diagnosis,
                action,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

fn ensure(c: &Connection) -> Result<(), String> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS product_series(id INTEGER PRIMARY KEY AUTOINCREMENT,name TEXT NOT NULL UNIQUE,created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);CREATE TABLE IF NOT EXISTS product_series_members(series_id INTEGER NOT NULL,sku TEXT NOT NULL,PRIMARY KEY(series_id,sku),FOREIGN KEY(series_id)REFERENCES product_series(id)ON DELETE CASCADE);CREATE TABLE IF NOT EXISTS product_cluster_weights(sku TEXT NOT NULL,cluster_name TEXT NOT NULL,weight REAL NOT NULL DEFAULT 0,updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,PRIMARY KEY(sku,cluster_name));CREATE INDEX IF NOT EXISTS idx_series_members_sku ON product_series_members(sku);CREATE TABLE IF NOT EXISTS product_price_cache(sku TEXT PRIMARY KEY,offer_id TEXT NOT NULL DEFAULT '',product_id TEXT NOT NULL DEFAULT '',currency_code TEXT NOT NULL DEFAULT 'RUB',price REAL NOT NULL DEFAULT 0,old_price REAL NOT NULL DEFAULT 0,min_price REAL NOT NULL DEFAULT 0,marketing_seller_price REAL,retail_price REAL,net_price REAL,raw_json TEXT NOT NULL DEFAULT '',synced_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);CREATE TABLE IF NOT EXISTS product_price_action_logs(id INTEGER PRIMARY KEY AUTOINCREMENT,sku TEXT NOT NULL,offer_id TEXT NOT NULL DEFAULT '',before_price REAL NOT NULL DEFAULT 0,requested_price REAL NOT NULL,requested_old_price REAL NOT NULL DEFAULT 0,requested_min_price REAL NOT NULL DEFAULT 0,currency_code TEXT NOT NULL DEFAULT 'RUB',status TEXT NOT NULL DEFAULT 'pending',message TEXT NOT NULL DEFAULT '',response_json TEXT NOT NULL DEFAULT '',verified_price REAL,created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,verified_at TEXT NOT NULL DEFAULT '');CREATE INDEX IF NOT EXISTS idx_product_price_logs_sku ON product_price_action_logs(sku,created_at DESC);").map_err(|e|e.to_string())
}

fn number(value: Option<&serde_json::Value>) -> Option<f64> {
    value.and_then(|v| {
        v.as_f64()
            .or_else(|| v.as_str()?.replace(',', ".").parse().ok())
    })
}

fn cached_price(c: &Connection, sku: &str) -> Option<ProductPrice> {
    c.query_row("SELECT sku,offer_id,product_id,currency_code,price,old_price,min_price,marketing_seller_price,retail_price,net_price,synced_at FROM product_price_cache WHERE sku=?1",[sku],|r|Ok(ProductPrice{sku:r.get(0)?,offer_id:r.get(1)?,product_id:r.get(2)?,currency_code:r.get(3)?,price:r.get(4)?,old_price:r.get(5)?,min_price:r.get(6)?,marketing_seller_price:r.get(7)?,retail_price:r.get(8)?,net_price:r.get(9)?,synced_at:r.get(10)?})).ok()
}

fn price_logs(c: &Connection, sku: &str) -> Vec<ProductPriceLog> {
    c.prepare("SELECT id,before_price,requested_price,verified_price,status,message,created_at FROM product_price_action_logs WHERE sku=?1 ORDER BY id DESC LIMIT 20").and_then(|mut s|s.query_map([sku],|r|Ok(ProductPriceLog{id:r.get(0)?,before_price:r.get(1)?,requested_price:r.get(2)?,verified_price:r.get(3)?,status:r.get(4)?,message:r.get(5)?,created_at:r.get(6)?}))?.collect()).unwrap_or_default()
}

fn refresh_price(c: &Connection, sku: &str) -> Result<ProductPrice, String> {
    let (offer_id, product_id): (String, String) = c
        .query_row(
            "SELECT COALESCE(offer_id,''),COALESCE(product_id,'') FROM products WHERE sku=?1",
            [sku],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| "当前产品缺少本地商品标识，请先同步 Seller 商品资料".to_string())?;
    if offer_id.is_empty() && product_id.is_empty() {
        return Err("当前产品缺少货号和 Product ID，无法查询价格".into());
    }
    let filter = if !offer_id.is_empty() {
        serde_json::json!({"offer_id":[offer_id]})
    } else {
        serde_json::json!({"product_id":[product_id.parse::<i64>().unwrap_or_default()]})
    };
    let payload = seller_post(
        c,
        "/v5/product/info/prices",
        &serde_json::json!({"filter":filter,"cursor":"","limit":100}),
    )?;
    let item = payload
        .get("items")
        .or_else(|| payload.pointer("/result/items"))
        .and_then(|v| v.as_array())
        .and_then(|v| v.first())
        .ok_or("Ozon 未返回该产品的价格；请检查商品标识与 API 权限")?;
    let price_obj = item.get("price").unwrap_or(item);
    let current = ProductPrice {
        sku: sku.to_string(),
        offer_id: item
            .get("offer_id")
            .and_then(|v| v.as_str())
            .unwrap_or(&offer_id)
            .to_string(),
        product_id: item
            .get("product_id")
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| v.to_string())
            })
            .unwrap_or(product_id),
        currency_code: price_obj
            .get("currency_code")
            .or_else(|| item.get("currency_code"))
            .and_then(|v| v.as_str())
            .unwrap_or("RUB")
            .to_string(),
        price: number(price_obj.get("price"))
            .or_else(|| number(price_obj.get("marketing_seller_price")))
            .unwrap_or(0.0),
        old_price: number(price_obj.get("old_price")).unwrap_or(0.0),
        min_price: number(price_obj.get("min_price")).unwrap_or(0.0),
        marketing_seller_price: number(price_obj.get("marketing_seller_price")),
        retail_price: number(price_obj.get("retail_price")),
        net_price: number(price_obj.get("net_price")),
        synced_at: chrono::Local::now().to_rfc3339(),
    };
    c.execute("INSERT INTO product_price_cache(sku,offer_id,product_id,currency_code,price,old_price,min_price,marketing_seller_price,retail_price,net_price,raw_json,synced_at)VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,CURRENT_TIMESTAMP)ON CONFLICT(sku)DO UPDATE SET offer_id=excluded.offer_id,product_id=excluded.product_id,currency_code=excluded.currency_code,price=excluded.price,old_price=excluded.old_price,min_price=excluded.min_price,marketing_seller_price=excluded.marketing_seller_price,retail_price=excluded.retail_price,net_price=excluded.net_price,raw_json=excluded.raw_json,synced_at=CURRENT_TIMESTAMP",params![current.sku,current.offer_id,current.product_id,current.currency_code,current.price,current.old_price,current.min_price,current.marketing_seller_price,current.retail_price,current.net_price,item.to_string()]).map_err(|e|e.to_string())?;
    cached_price(c, sku).ok_or("价格缓存写入失败".into())
}

#[tauri::command]
pub fn refresh_product_price(sku: String, state: State<AppState>) -> Result<ProductPrice, String> {
    let c = db(&state)?;
    ensure(&c)?;
    refresh_price(&c, sku.trim())
}

#[tauri::command]
pub fn update_product_price(
    form: ProductPriceUpdate,
    state: State<AppState>,
) -> Result<ProductPrice, String> {
    validate_price_update(&form)?;
    let c = db(&state)?;
    ensure(&c)?;
    let before = refresh_price(&c, form.sku.trim()).or_else(|_| {
        cached_price(&c, form.sku.trim()).ok_or_else(|| "无法取得改价前价格".to_string())
    })?;
    let offer = if before.offer_id.is_empty() {
        c.query_row(
            "SELECT COALESCE(offer_id,'') FROM products WHERE sku=?1",
            [form.sku.trim()],
            |r| r.get::<_, String>(0),
        )
        .unwrap_or_default()
    } else {
        before.offer_id.clone()
    };
    if offer.is_empty() {
        return Err("当前产品缺少货号，无法安全修改价格".into());
    }
    c.execute("INSERT INTO product_price_action_logs(sku,offer_id,before_price,requested_price,requested_old_price,requested_min_price,currency_code,status)VALUES(?1,?2,?3,?4,?5,?6,?7,'pending')",params![form.sku,offer,before.price,form.price,form.old_price,form.min_price,form.currency_code]).map_err(|e|e.to_string())?;
    let log_id = c.last_insert_rowid();
    let payload = serde_json::json!({"prices":[{"offer_id":offer,"price":form.price.to_string(),"old_price":form.old_price.to_string(),"min_price":form.min_price.to_string(),"currency_code":if form.currency_code.is_empty(){"RUB"}else{&form.currency_code},"auto_action_enabled":"UNKNOWN"}]});
    let response = match seller_post(&c, "/v1/product/import/prices", &payload) {
        Ok(v) => v,
        Err(e) => {
            let _ = c.execute(
                "UPDATE product_price_action_logs SET status='failed',message=?1 WHERE id=?2",
                params![e, log_id],
            );
            return Err(e);
        }
    };
    let result = response
        .get("result")
        .and_then(|v| v.as_array())
        .and_then(|v| v.first());
    let updated = result
        .and_then(|v| v.get("updated"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let errors = result
        .and_then(|v| v.get("errors"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if !updated || !errors.is_empty() {
        let message = if errors.is_empty() {
            "Ozon 未确认价格更新".to_string()
        } else {
            errors
                .iter()
                .map(|e| {
                    e.get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("未知错误")
                })
                .collect::<Vec<_>>()
                .join("；")
        };
        let _=c.execute("UPDATE product_price_action_logs SET status='failed',message=?1,response_json=?2 WHERE id=?3",params![message,response.to_string(),log_id]);
        return Err(message);
    }
    std::thread::sleep(std::time::Duration::from_secs(2));
    let verified = refresh_price(&c, form.sku.trim()).ok();
    let verified_price = verified.as_ref().map(|v| v.price);
    let status = if verified_price.is_some_and(|v| (v - form.price).abs() < 0.01) {
        "verified"
    } else {
        "accepted"
    };
    let message = if status == "verified" {
        "Ozon 已接受并回读确认新价格"
    } else {
        "Ozon 已接受更新；价格缓存尚未刷新，请稍后再次读取"
    };
    c.execute("UPDATE product_price_action_logs SET status=?1,message=?2,response_json=?3,verified_price=?4,verified_at=CASE WHEN ?4 IS NULL THEN '' ELSE CURRENT_TIMESTAMP END WHERE id=?5",params![status,message,response.to_string(),verified_price,log_id]).map_err(|e|e.to_string())?;
    verified
        .or_else(|| cached_price(&c, form.sku.trim()))
        .ok_or("价格更新已提交，但无法读取价格缓存".into())
}

#[cfg(test)]
mod price_tests {
    use super::*;

    fn form(price: f64, old_price: f64, min_price: f64) -> ProductPriceUpdate {
        ProductPriceUpdate {
            sku: "SKU-1".into(),
            price,
            old_price,
            min_price,
            currency_code: "RUB".into(),
        }
    }

    #[test]
    fn accepts_safe_price_relationships() {
        assert!(validate_price_update(&form(1686.0, 1990.0, 1500.0)).is_ok());
        assert!(validate_price_update(&form(1686.0, 0.0, 0.0)).is_ok());
    }

    #[test]
    fn rejects_invalid_price_relationships() {
        assert!(validate_price_update(&form(0.0, 0.0, 0.0)).is_err());
        assert!(validate_price_update(&form(1686.0, 1600.0, 1500.0)).is_err());
        assert!(validate_price_update(&form(1400.0, 1990.0, 1500.0)).is_err());
    }
}

#[tauri::command]
pub fn product_detail(
    sku: String,
    to: String,
    state: State<AppState>,
) -> Result<ProductDetail, String> {
    let c = db(&state)?;
    ensure(&c)?;
    let (offer_id,name)=c.query_row("SELECT COALESCE(offer_id,''),COALESCE(NULLIF(name,''),(SELECT MAX(product_name) FROM sales_daily WHERE sku=?1),'') FROM products WHERE sku=?1",[&sku],|r|Ok((r.get(0)?,r.get(1)?))).unwrap_or_default();
    let mut stmt=c.prepare("WITH RECURSIVE days(day)AS(SELECT date(?2,'-29 day')UNION ALL SELECT date(day,'+1 day')FROM days WHERE day<?2)SELECT d.day,COALESCE(s.ordered_units,0),COALESCE(s.revenue,0),COALESCE((SELECT SUM(spend)FROM ad_daily a WHERE a.sku=?1 AND a.day=d.day),0)FROM days d LEFT JOIN sales_daily s ON s.sku=?1 AND s.day=d.day ORDER BY d.day").map_err(|e|e.to_string())?;
    let trend = stmt
        .query_map(params![sku, to], |r| {
            Ok(ProductTrend {
                day: r.get(0)?,
                units: r.get(1)?,
                revenue: r.get(2)?,
                ad_spend: r.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let all = clusters(&c, &to)?.remove(&sku).unwrap_or_default();
    let mut clusters = Vec::new();
    for item in all {
        let configured = c
            .query_row(
                "SELECT weight FROM product_cluster_weights WHERE sku=?1 AND cluster_name=?2",
                params![sku, item.cluster],
                |r| r.get(0),
            )
            .unwrap_or(item.share);
        clusters.push(ProductWeight {
            cluster: item.cluster,
            historical_share: item.share,
            configured_weight: configured,
            orders: item.orders,
        })
    }
    let price = cached_price(&c, &sku);
    let price_logs = price_logs(&c, &sku);
    Ok(ProductDetail {
        sku,
        offer_id,
        name,
        trend,
        clusters,
        price,
        price_logs,
    })
}

#[tauri::command]
pub fn save_product_cluster_weights(
    sku: String,
    weights: HashMap<String, f64>,
    state: State<AppState>,
) -> Result<(), String> {
    let mut c = db(&state)?;
    ensure(&c)?;
    let total: f64 = weights.values().sum();
    if total <= 0.0 {
        return Err("集群权重合计必须大于 0".into());
    }
    let tx = c.transaction().map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM product_cluster_weights WHERE sku=?1", [&sku])
        .map_err(|e| e.to_string())?;
    for (cluster, value) in weights {
        tx.execute(
            "INSERT INTO product_cluster_weights(sku,cluster_name,weight)VALUES(?1,?2,?3)",
            params![sku, cluster, value / total * 100.0],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())
}

fn clusters(c: &Connection, to: &str) -> Result<HashMap<String, Vec<ClusterShare>>, String> {
    let mut stmt=c.prepare("SELECT p.sku,COALESCE(NULLIF(m.cluster_name,''),NULLIF(p.destination,''),'未识别'),COUNT(DISTINCT p.posting_number) FROM posting_routes p LEFT JOIN warehouse_cluster_mappings m ON m.warehouse_name=p.destination WHERE p.day BETWEEN date(?1,'-29 day') AND ?1 GROUP BY p.sku,2 ORDER BY p.sku,3 DESC").map_err(|e|e.to_string())?;
    let raw = stmt
        .query_map([to], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut grouped: HashMap<String, Vec<(String, i64)>> = HashMap::new();
    for (sku, cluster, count) in raw {
        grouped.entry(sku).or_default().push((cluster, count))
    }
    Ok(grouped
        .into_iter()
        .map(|(sku, values)| {
            let total = values.iter().map(|x| x.1).sum::<i64>().max(1);
            (
                sku,
                values
                    .into_iter()
                    .map(|(cluster, orders)| ClusterShare {
                        cluster,
                        orders,
                        share: orders as f64 / total as f64 * 100.0,
                    })
                    .collect(),
            )
        })
        .collect())
}

#[tauri::command]
pub fn product_insights(
    to: String,
    query: String,
    state: State<AppState>,
) -> Result<Vec<InsightRow>, String> {
    let c = db(&state)?;
    ensure(&c)?;
    let cluster_map = clusters(&c, &to)?;
    let needle = format!("%{}%", query.trim());
    let mut stmt=c.prepare("WITH known AS(SELECT sku FROM products UNION SELECT sku FROM sales_daily),s AS(SELECT sku,SUM(CASE WHEN day=?1 THEN ordered_units ELSE 0 END)du,SUM(CASE WHEN day BETWEEN date(?1,'-6 day')AND ?1 THEN ordered_units ELSE 0 END)wu,SUM(CASE WHEN day BETWEEN date(?1,'-29 day')AND ?1 THEN ordered_units ELSE 0 END)mu,SUM(CASE WHEN day=?1 THEN revenue ELSE 0 END)dr,SUM(CASE WHEN day BETWEEN date(?1,'-6 day')AND ?1 THEN revenue ELSE 0 END)wr,SUM(CASE WHEN day BETWEEN date(?1,'-29 day')AND ?1 THEN revenue ELSE 0 END)mr FROM sales_daily GROUP BY sku),candidate AS(SELECT DISTINCT a.campaign_id,p.sku FROM ad_daily a JOIN products p ON a.sku='' AND ((length(COALESCE(p.offer_id,''))>=4 AND instr(lower(a.campaign_name),lower(p.offer_id))>0)OR(length(p.sku)>=6 AND instr(a.campaign_name,p.sku)>0))),cmap AS(SELECT campaign_id,MIN(sku)sku FROM candidate GROUP BY campaign_id HAVING COUNT(DISTINCT sku)=1),effective_ads AS(SELECT day,sku,spend,orders FROM ad_daily WHERE sku<>'' UNION ALL SELECT a.day,m.sku,a.spend,a.orders FROM ad_daily a JOIN cmap m ON m.campaign_id=a.campaign_id WHERE a.sku='' AND NOT EXISTS(SELECT 1 FROM ad_daily x WHERE x.day=a.day AND x.campaign_id=a.campaign_id AND x.sku=m.sku)),a AS(SELECT sku,SUM(CASE WHEN day=?1 THEN spend ELSE 0 END)ds,SUM(CASE WHEN day BETWEEN date(?1,'-6 day')AND ?1 THEN spend ELSE 0 END)ws,SUM(CASE WHEN day BETWEEN date(?1,'-29 day')AND ?1 THEN spend ELSE 0 END)ms,SUM(CASE WHEN day=?1 THEN orders ELSE 0 END)do,SUM(CASE WHEN day BETWEEN date(?1,'-6 day')AND ?1 THEN orders ELSE 0 END)wo,SUM(CASE WHEN day BETWEEN date(?1,'-29 day')AND ?1 THEN orders ELSE 0 END)mo FROM effective_ads GROUP BY sku)SELECT k.sku,COALESCE(p.offer_id,''),COALESCE(NULLIF(p.name,''),MAX(sd.product_name),''),COALESCE(s.du,0),COALESCE(s.wu,0),COALESCE(s.mu,0),COALESCE(s.dr,0),COALESCE(s.wr,0),COALESCE(s.mr,0),COALESCE(a.ds,0),COALESCE(a.ws,0),COALESCE(a.ms,0),COALESCE(a.do,0),COALESCE(a.wo,0),COALESCE(a.mo,0)FROM known k LEFT JOIN products p ON p.sku=k.sku LEFT JOIN sales_daily sd ON sd.sku=k.sku LEFT JOIN s ON s.sku=k.sku LEFT JOIN a ON a.sku=k.sku WHERE ?2='%%' OR k.sku LIKE ?2 OR p.offer_id LIKE ?2 OR p.name LIKE ?2 GROUP BY k.sku ORDER BY COALESCE(s.mu,0)DESC,p.offer_id LIMIT 1000").map_err(|e|e.to_string())?;
    let rows = stmt
        .query_map(params![to, needle], |r| {
            let sku: String = r.get(0)?;
            Ok(InsightRow {
                id: sku.clone(),
                name: r.get::<_, String>(2)?,
                skus: vec![sku.clone()],
                offer_ids: vec![r.get(1)?],
                day_units: r.get(3)?,
                week_units: r.get(4)?,
                month_units: r.get(5)?,
                day_revenue: r.get(6)?,
                week_revenue: r.get(7)?,
                month_revenue: r.get(8)?,
                day_ad_spend: r.get(9)?,
                week_ad_spend: r.get(10)?,
                month_ad_spend: r.get(11)?,
                day_ad_orders: r.get(12)?,
                week_ad_orders: r.get(13)?,
                month_ad_orders: r.get(14)?,
                clusters: cluster_map.get(&sku).cloned().unwrap_or_default(),
                ad_source: "SKU 精确数据；无 SKU 时仅使用唯一匹配的活动名称".into(),
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut active_stmt=c.prepare("SELECT DISTINCT sku FROM sales_daily WHERE day BETWEEN date(?1,'-59 day') AND ?1 AND ordered_units>0").map_err(|e|e.to_string())?;
    let active = active_stmt
        .query_map([&to], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<std::collections::HashSet<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .filter(|row| active.contains(&row.id))
        .collect())
}

#[tauri::command]
pub fn series_insights(to: String, state: State<AppState>) -> Result<Vec<InsightRow>, String> {
    let product_rows = product_insights(to.clone(), String::new(), state.clone())?;
    let product_map = product_rows
        .into_iter()
        .map(|row| (row.id.clone(), row))
        .collect::<HashMap<_, _>>();
    let c = db(&state)?;
    ensure(&c)?;
    let cluster_map = clusters(&c, &to)?;
    let mut stmt=c.prepare("SELECT ps.id,ps.name,group_concat(DISTINCT m.sku),group_concat(DISTINCT COALESCE(p.offer_id,'')),COALESCE(SUM(CASE WHEN s.day=?1 THEN s.ordered_units ELSE 0 END),0),COALESCE(SUM(CASE WHEN s.day BETWEEN date(?1,'-6 day')AND ?1 THEN s.ordered_units ELSE 0 END),0),COALESCE(SUM(CASE WHEN s.day BETWEEN date(?1,'-29 day')AND ?1 THEN s.ordered_units ELSE 0 END),0),COALESCE(SUM(CASE WHEN s.day=?1 THEN s.revenue ELSE 0 END),0),COALESCE(SUM(CASE WHEN s.day BETWEEN date(?1,'-6 day')AND ?1 THEN s.revenue ELSE 0 END),0),COALESCE(SUM(CASE WHEN s.day BETWEEN date(?1,'-29 day')AND ?1 THEN s.revenue ELSE 0 END),0)FROM product_series ps JOIN product_series_members m ON m.series_id=ps.id LEFT JOIN products p ON p.sku=m.sku LEFT JOIN sales_daily s ON s.sku=m.sku GROUP BY ps.id ORDER BY ps.name").map_err(|e|e.to_string())?;
    let base = stmt
        .query_map([&to], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, f64>(7)?,
                r.get::<_, f64>(8)?,
                r.get::<_, f64>(9)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut out = vec![];
    for (id, name, sku_text, offer_text, du, wu, mu, dr, wr, mr) in base {
        let skus = sku_text
            .split(',')
            .filter(|x| !x.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>();
        let ds = skus
            .iter()
            .filter_map(|sku| product_map.get(sku))
            .map(|x| x.day_ad_spend)
            .sum();
        let ws = skus
            .iter()
            .filter_map(|sku| product_map.get(sku))
            .map(|x| x.week_ad_spend)
            .sum();
        let ms = skus
            .iter()
            .filter_map(|sku| product_map.get(sku))
            .map(|x| x.month_ad_spend)
            .sum();
        let d_order = skus
            .iter()
            .filter_map(|sku| product_map.get(sku))
            .map(|x| x.day_ad_orders)
            .sum();
        let w_order = skus
            .iter()
            .filter_map(|sku| product_map.get(sku))
            .map(|x| x.week_ad_orders)
            .sum();
        let m_order = skus
            .iter()
            .filter_map(|sku| product_map.get(sku))
            .map(|x| x.month_ad_orders)
            .sum();
        let mut counts: HashMap<String, i64> = HashMap::new();
        for sku in &skus {
            for item in cluster_map.get(sku).into_iter().flatten() {
                *counts.entry(item.cluster.clone()).or_default() += item.orders
            }
        }
        let total = counts.values().sum::<i64>().max(1);
        let cluster_values = counts
            .into_iter()
            .map(|(cluster, orders)| ClusterShare {
                cluster,
                orders,
                share: orders as f64 / total as f64 * 100.0,
            })
            .collect();
        out.push(InsightRow {
            id: id.to_string(),
            name,
            skus,
            offer_ids: offer_text.split(',').map(str::to_string).collect(),
            day_units: du,
            week_units: wu,
            month_units: mu,
            day_revenue: dr,
            week_revenue: wr,
            month_revenue: mr,
            day_ad_spend: ds,
            week_ad_spend: ws,
            month_ad_spend: ms,
            day_ad_orders: d_order,
            week_ad_orders: w_order,
            month_ad_orders: m_order,
            clusters: cluster_values,
            ad_source: "系列成员精确/唯一活动广告汇总".into(),
        })
    }
    Ok(out)
}

#[tauri::command]
pub fn save_product_series(
    id: Option<i64>,
    name: String,
    skus: Vec<String>,
    state: State<AppState>,
) -> Result<i64, String> {
    if name.trim().is_empty() {
        return Err("系列名称不能为空".into());
    }
    if skus.is_empty() {
        return Err("至少选择一个产品".into());
    }
    let mut c = db(&state)?;
    ensure(&c)?;
    let tx = c.transaction().map_err(|e| e.to_string())?;
    let series_id = if let Some(id) = id {
        tx.execute(
            "UPDATE product_series SET name=?1,updated_at=CURRENT_TIMESTAMP WHERE id=?2",
            params![name.trim(), id],
        )
        .map_err(|e| e.to_string())?;
        id
    } else {
        tx.execute("INSERT INTO product_series(name)VALUES(?1)", [name.trim()])
            .map_err(|e| e.to_string())?;
        tx.last_insert_rowid()
    };
    tx.execute(
        "DELETE FROM product_series_members WHERE series_id=?1",
        [series_id],
    )
    .map_err(|e| e.to_string())?;
    for sku in skus {
        tx.execute(
            "INSERT OR IGNORE INTO product_series_members(series_id,sku)VALUES(?1,?2)",
            params![series_id, sku],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(series_id)
}

#[tauri::command]
pub fn delete_product_series(id: i64, state: State<AppState>) -> Result<(), String> {
    let c = db(&state)?;
    ensure(&c)?;
    c.execute(
        "DELETE FROM product_series_members WHERE series_id=?1",
        [id],
    )
    .map_err(|e| e.to_string())?;
    c.execute("DELETE FROM product_series WHERE id=?1", [id])
        .map_err(|e| e.to_string())?;
    Ok(())
}
