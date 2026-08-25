use crate::{db, AppState};
use rusqlite::{params, Connection};
use serde::Serialize;
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

fn ensure(c: &Connection) -> Result<(), String> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS product_series(id INTEGER PRIMARY KEY AUTOINCREMENT,name TEXT NOT NULL UNIQUE,created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);CREATE TABLE IF NOT EXISTS product_series_members(series_id INTEGER NOT NULL,sku TEXT NOT NULL,PRIMARY KEY(series_id,sku),FOREIGN KEY(series_id)REFERENCES product_series(id)ON DELETE CASCADE);CREATE TABLE IF NOT EXISTS product_cluster_weights(sku TEXT NOT NULL,cluster_name TEXT NOT NULL,weight REAL NOT NULL DEFAULT 0,updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,PRIMARY KEY(sku,cluster_name));CREATE INDEX IF NOT EXISTS idx_series_members_sku ON product_series_members(sku);").map_err(|e|e.to_string())
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
    Ok(ProductDetail {
        sku,
        offer_id,
        name,
        trend,
        clusters,
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
