//! Versioned, date-dense series exports. No missing row is silently treated as zero.
use chrono::NaiveDate;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::collections::BTreeMap;

const KEYS: [&str; 7] = [
    "totalUnits",
    "totalRevenue",
    "impressions",
    "clicks",
    "adOrders",
    "spend",
    "adRevenue",
];
type Values = [Option<f64>; 7];

fn sum(rows: &[Values]) -> Values {
    std::array::from_fn(|i| {
        let known: Vec<f64> = rows.iter().filter_map(|row| row[i]).collect();
        if known.is_empty() {
            None
        } else {
            Some(known.iter().sum())
        }
    })
}

fn metrics(values: Values) -> Value {
    let mut row = serde_json::Map::new();
    for (i, key) in KEYS.iter().enumerate() {
        row.insert(
            (*key).into(),
            json!(values[i].map(|v| (v * 100.0).round() / 100.0)),
        );
    }
    for (key, a, b, multiplier) in [
        ("ctr", 3, 2, 100.0),
        ("conversionRate", 4, 3, 100.0),
        ("cpc", 5, 3, 1.0),
        ("cpa", 5, 4, 1.0),
        ("acos", 5, 6, 100.0),
        ("tacos", 5, 1, 100.0),
        ("roas", 6, 5, 1.0),
    ] {
        let ratio = values[a]
            .zip(values[b])
            .and_then(|(a, b)| (b > 0.0).then(|| (a / b * multiplier * 10000.0).round() / 10000.0));
        row.insert(key.into(), json!(ratio));
    }
    // The legacy sync has no per-SKU completion ledger. Existing rows are observed,
    // not proof that all campaigns or sales pages were successfully synchronized.
    row.insert(
        "salesStatus".into(),
        json!(if values[..2].iter().all(Option::is_none) {
            "missing"
        } else {
            "partial"
        }),
    );
    row.insert(
        "advertisingStatus".into(),
        json!(if values[2..].iter().all(Option::is_none) {
            "missing"
        } else {
            "partial"
        }),
    );
    Value::Object(row)
}

fn aggregate(rows: &[Values]) -> Value {
    let mut result = metrics(sum(rows));
    for (key, start, end) in [("salesStatus", 0, 2), ("advertisingStatus", 2, 7)] {
        result[key] = json!(if rows
            .iter()
            .any(|row| row[start..end].iter().any(Option::is_some))
        {
            "partial"
        } else {
            "missing"
        });
    }
    result
}

fn with_quality(mut result: Value, missing: &[String]) -> Value {
    result["missingMetrics"] = json!(missing);
    result["aggregationBasis"] = json!("known_values_only");
    let mut references = serde_json::Map::new();
    let mut statuses = serde_json::Map::new();
    for (ratio, a, b) in [
        ("ctr", "clicks", "impressions"),
        ("conversionRate", "adOrders", "clicks"),
        ("cpc", "spend", "clicks"),
        ("cpa", "spend", "adOrders"),
        ("acos", "spend", "adRevenue"),
        ("tacos", "spend", "totalRevenue"),
        ("roas", "adRevenue", "spend"),
    ] {
        let status = if result[a].is_null() || result[b].is_null() {
            "missing_inputs"
        } else if result[b].as_f64().unwrap_or(0.0) <= 0.0 {
            "zero_denominator"
        } else if missing.iter().any(|k| k == a || k == b) {
            references.insert(ratio.into(), result[ratio].clone());
            result[ratio] = Value::Null;
            "reference_only"
        } else {
            "observed_inputs"
        };
        statuses.insert(ratio.into(), json!(status));
    }
    result["referenceMetrics"] = Value::Object(references);
    result["ratioStatus"] = Value::Object(statuses);
    result
}
fn report_coverage(c: &Connection, days: &[String]) -> Result<BTreeMap<String, Value>, String> {
    let exists: bool = c.query_row("SELECT COUNT(*)=2 FROM sqlite_master WHERE name IN ('ad_history_coverage','campaigns')", [], |r|r.get(0)).map_err(|e|e.to_string())?;
    let mut out = BTreeMap::new();
    for day in days {
        if !exists {
            out.insert(
                day.clone(),
                json!({"allExpectedCampaignsDownloaded":false,"status":"unverified"}),
            );
            continue;
        }
        let mut stmt=c.prepare("SELECT DISTINCT a.campaign_id FROM ad_daily a LEFT JOIN campaigns p ON p.campaign_id=a.campaign_id WHERE a.day=?1 AND a.sku='' AND (a.spend<>0 OR a.impressions<>0 OR a.clicks<>0 OR a.orders<>0) AND (lower(COALESCE(p.payment_type,'')) IN ('cpc','') OR lower(p.payment_type) LIKE '%click%') ORDER BY a.campaign_id").map_err(|e|e.to_string())?;
        let expected = stmt
            .query_map([day], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        let mut pending = Vec::new();
        for campaign in &expected {
            let found:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM ad_history_coverage WHERE campaign_id=?1 AND date_from<=?2 AND date_to>=?2)",params![campaign,day],|r|r.get(0)).map_err(|e|e.to_string())?;
            if !found {
                pending.push(campaign.clone());
            }
        }
        out.insert(day.clone(),json!({"scope":"known_active_cpc_campaigns","expectedCampaignCount":expected.len(),"pendingCampaignIds":pending,"allExpectedCampaignsDownloaded":!expected.is_empty() && pending.is_empty(),"note":"已下载不等于字段完整；没有 SKU 行不能单独证明未投放；其他广告类型未由该报告核验"}));
    }
    Ok(out)
}

fn missing_keys(values: Values) -> Vec<String> {
    KEYS.iter()
        .enumerate()
        .filter(|(i, _)| values[*i].is_none())
        .map(|(_, k)| (*k).to_owned())
        .collect()
}

fn dates(from: &str, to: &str) -> Result<Vec<String>, String> {
    let parse = |s: &str| {
        NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .ok()
            .filter(|d| d.to_string() == s)
    };
    let first = parse(from).ok_or("开始日期无效")?;
    let last = parse(to).ok_or("结束日期无效")?;
    let count = (last - first).num_days() + 1;
    if !(1..=366).contains(&count) {
        return Err("请选择 1 至 366 天的日期范围".into());
    }
    Ok((0..count)
        .map(|i| (first + chrono::Duration::days(i)).to_string())
        .collect())
}

pub(super) fn build(
    c: &Connection,
    from: &str,
    to: &str,
    skus: Vec<String>,
    name: &str,
    factor: f64,
) -> Result<Value, String> {
    let days = dates(from, to)?;
    let mut skus: Vec<String> = skus
        .into_iter()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect();
    skus.sort();
    skus.dedup();
    if skus.is_empty() || skus.len() > 500 {
        return Err("请选择 1 至 500 个产品".into());
    }
    if !factor.is_finite() || factor <= 0.0 {
        return Err("币种换算率无效".into());
    }
    let coverage = report_coverage(c, &days)?;
    let mut products = Vec::new();
    let mut gaps: Vec<Value> = Vec::new();
    let mut by_day: Vec<Vec<Values>> = vec![Vec::new(); days.len()];
    let mut sales = c.prepare("SELECT day,SUM(ordered_units),SUM(revenue) FROM sales_daily WHERE sku=?1 AND day BETWEEN ?2 AND ?3 GROUP BY day").map_err(|e|e.to_string())?;
    // ad_daily's unique (day,campaign_id,sku) key is the canonical sync identity.
    // Empty-SKU store totals are deliberately excluded, so they cannot double-count details.
    let has_partial: bool = c
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='ad_partial_daily')",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let ad_sql = if has_partial {
        "WITH partial AS (SELECT p.* FROM ad_partial_daily p WHERE NOT EXISTS(SELECT 1 FROM ad_daily a WHERE a.day=p.day AND a.campaign_id=p.campaign_id AND a.sku=p.sku AND a.updated_at>p.updated_at)), combined AS (SELECT a.day,a.sku,a.impressions,a.clicks,a.orders,a.spend,a.revenue,0 AS missing_mask FROM ad_daily a WHERE NOT EXISTS(SELECT 1 FROM partial p WHERE p.day=a.day AND p.campaign_id=a.campaign_id AND p.sku=a.sku) UNION ALL SELECT day,sku,impressions,clicks,orders,spend,revenue,missing_mask FROM partial) SELECT day,SUM(impressions),SUM(clicks),SUM(orders),SUM(ABS(spend)),SUM(revenue),SUM(CASE WHEN impressions IS NULL OR (missing_mask & 1)<>0 THEN 1 ELSE 0 END),SUM(CASE WHEN clicks IS NULL OR (missing_mask & 2)<>0 THEN 1 ELSE 0 END),SUM(CASE WHEN orders IS NULL OR (missing_mask & 8)<>0 THEN 1 ELSE 0 END),SUM(CASE WHEN spend IS NULL OR (missing_mask & 32)<>0 THEN 1 ELSE 0 END),SUM(CASE WHEN revenue IS NULL OR (missing_mask & 16)<>0 THEN 1 ELSE 0 END) FROM combined WHERE sku=?1 AND day BETWEEN ?2 AND ?3 GROUP BY day"
    } else {
        "SELECT day,SUM(impressions),SUM(clicks),SUM(orders),SUM(ABS(spend)),SUM(revenue),COUNT(*)-COUNT(impressions),COUNT(*)-COUNT(clicks),COUNT(*)-COUNT(orders),COUNT(*)-COUNT(spend),COUNT(*)-COUNT(revenue) FROM ad_daily WHERE sku=?1 AND day BETWEEN ?2 AND ?3 GROUP BY day"
    };
    let mut ads = c.prepare(ad_sql).map_err(|e| e.to_string())?;
    for sku in &skus {
        let mut incomplete: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut rows: BTreeMap<String, Values> =
            days.iter().map(|d| (d.clone(), [None; 7])).collect();
        let iter = sales
            .query_map(params![sku, from, to], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<f64>>(1)?,
                    r.get::<_, Option<f64>>(2)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        for r in iter {
            let (day, units, revenue) = r.map_err(|e| e.to_string())?;
            if let Some(v) = rows.get_mut(&day) {
                v[0] = units;
                v[1] = revenue.map(|n| n * factor);
            }
        }
        let iter = ads
            .query_map(params![sku, from, to], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    [
                        r.get::<_, Option<f64>>(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ],
                    [
                        r.get::<_, i64>(6)?,
                        r.get(7)?,
                        r.get(8)?,
                        r.get(9)?,
                        r.get(10)?,
                    ],
                ))
            })
            .map_err(|e| e.to_string())?;
        for r in iter {
            let (day, values, missing) = r.map_err(|e| e.to_string())?;
            incomplete.insert(
                day.clone(),
                missing
                    .iter()
                    .enumerate()
                    .filter(|(_, n)| **n > 0)
                    .map(|(i, _)| KEYS[i + 2].to_owned())
                    .collect(),
            );
            if let Some(v) = rows.get_mut(&day) {
                v[2..].copy_from_slice(&values);
                for i in [5, 6] {
                    v[i] = v[i].map(|n| n * factor);
                }
            }
        }
        let identity: (String,String) = c.query_row("SELECT COALESCE((SELECT offer_id FROM products WHERE sku=?1),''),COALESCE(NULLIF((SELECT name FROM products WHERE sku=?1),''),(SELECT MAX(product_name) FROM sales_daily WHERE sku=?1),'')", [sku], |r|Ok((r.get(0)?,r.get(1)?))).map_err(|e|e.to_string())?;
        let mut daily = Vec::new();
        let mut raw = Vec::new();
        for (i, day) in days.iter().enumerate() {
            let v = rows[day];
            raw.push(v);
            by_day[i].push(v);
            let mut missing = missing_keys(v);
            missing.extend(incomplete.get(day).cloned().unwrap_or_default());
            missing.sort();
            missing.dedup();
            if !missing.is_empty() {
                let reasons: BTreeMap<String, String> = missing
                    .iter()
                    .map(|metric| {
                        let reason = if metric == "totalUnits" || metric == "totalRevenue" {
                            "sales_record_missing"
                        } else if incomplete
                            .get(day)
                            .is_some_and(|keys| keys.contains(metric))
                        {
                            "source_field_missing_or_invalid"
                        } else if coverage
                            .get(day)
                            .is_some_and(|v| v["allExpectedCampaignsDownloaded"] == true)
                        {
                            "downloaded_reports_no_sku_row"
                        } else {
                            "report_coverage_unverified"
                        };
                        (metric.clone(), reason.to_owned())
                    })
                    .collect();
                gaps.push(json!({"date":day,"sku":sku,"offerId":identity.0,"metrics":missing,"reasons":reasons}));
            }
            let mut row = with_quality(metrics(v), &missing);
            row["date"] = json!(day);
            daily.push(row);
        }
        let mut missing: Vec<String> = gaps
            .iter()
            .filter(|g| g["sku"] == *sku)
            .flat_map(|g| {
                g["metrics"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap().to_owned())
            })
            .collect();
        missing.sort();
        missing.dedup();
        products.push(json!({"sku":sku,"offerId":identity.0,"name":identity.1,"daily":daily,"summary":with_quality(aggregate(&raw),&missing)}));
    }
    let totals: Vec<Values> = by_day.iter().map(|rows| sum(rows)).collect();
    let daily: Vec<Value> = days
        .iter()
        .zip(&by_day)
        .map(|(day, rows)| {
            let mut missing: Vec<String> = gaps
                .iter()
                .filter(|g| g["date"] == *day)
                .flat_map(|g| {
                    g["metrics"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| v.as_str().unwrap().to_owned())
                })
                .collect();
            missing.sort();
            missing.dedup();
            let mut row = with_quality(aggregate(rows), &missing);
            row["date"] = json!(day);
            row
        })
        .collect();
    let mut summary = metrics(sum(&totals));
    let all_rows: Vec<Values> = by_day.iter().flatten().copied().collect();
    let quality = aggregate(&all_rows);
    summary["salesStatus"] = quality["salesStatus"].clone();
    summary["advertisingStatus"] = quality["advertisingStatus"].clone();
    let mut missing: Vec<String> = gaps
        .iter()
        .flat_map(|g| {
            g["metrics"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_owned())
        })
        .collect();
    missing.sort();
    missing.dedup();
    summary = with_quality(summary, &missing);
    Ok(json!({
        "schema":"ozon.advertising-series.ai-dataset", "schemaVersion":"2.2", "isSample":false,
        "generatedAt":chrono::Utc::now().to_rfc3339(),
        "period":{"dateFrom":from,"dateTo":to,"dayCount":days.len(),"inclusive":true,"timezone":null,"dateBasis":"source_report_date"},
        "series":{"name":if name.trim().is_empty(){"临时系列"}else{name.trim()},"skus":skus,"skuCount":products.len()},
        "metricDefinitions":{"totalUnits":"全部下单销量（件），不是订单数或扣退款后的净销量","totalRevenue":"销售源报表的全部销售额","impressions":"广告曝光次数","clicks":"广告点击次数","adOrders":"广告归因订单数，不等于广告销量","spend":"广告花费","adRevenue":"广告归因销售额","ctr":"点击率，百分数","conversionRate":"广告订单转化率，百分数","cpc":"每次点击成本","cpa":"每个广告订单成本","acos":"广告费/广告销售额，百分数","tacos":"广告费/全部销售额，百分数","roas":"广告销售额/广告费，倍数"},
        "formulas":{"ctr":"clicks / impressions * 100","conversionRate":"adOrders / clicks * 100","cpc":"spend / clicks","cpa":"spend / adOrders","acos":"spend / adRevenue * 100","tacos":"spend / totalRevenue * 100","roas":"adRevenue / spend"},
        "dataRules":["单品周期=sum(单品每日基础指标)；系列每日=sum(所选产品当日基础指标)；系列周期=sum(系列每日基础指标)","比率使用未舍入的汇总分子/分母重新计算，不平均百分比；百分数 5 表示 5%","缺失为 null；仅源记录明确为零时输出 0；合计累加已知值，全部未知才为 null；missingMetrics 标记不完整指标；关联指标不完整时正式比率为 null，referenceMetrics 提供按已知合计计算的参考比率，分子分母可能覆盖不同产品日期，不能视为完整业绩","分母为零或输入缺失时比率为 null","金额在汇总后保留两位小数；比率保留四位小数","广告订单不等于销量，不能相减推算自然销量","广告数据按日、广告计划、SKU 的数据库唯一键汇总，不叠加空 SKU 的店铺总计"],
        "dataQuality":{"reportCoverage":coverage,"missingData":gaps,"warnings":["数值为已知数据合计，标记 * 的指标仍不完整；缺失产品/日期/字段详见清单；≈ 表示按已知合计计算的参考比率，分子分母覆盖可能不同；正式比率保留 null，不能视为完整业绩","历史同步未记录逐日逐 SKU 完整性，已有记录标记 partial（已采集值，未保证完整），没有记录标记 missing；不将无记录视为零","源报表时区未核验，保留源日期，timezone 为 null","未归属 SKU 的店铺广告费用不分摊、不计入系列；广告归因日与销售下单日可能不同"]},
        "products":products,"seriesDaily":daily,"seriesSummary":summary
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incomplete_totals_offer_explicit_references_without_changing_formal_ratios() {
        let v = [
            Some(69.0),
            Some(177919.0),
            Some(62814.0),
            Some(2742.0),
            Some(24.0),
            Some(22733.74),
            Some(57311.0),
        ];
        let r = with_quality(metrics(v), &vec!["spend".into(), "adRevenue".into()]);
        assert!(r["acos"].is_null());
        assert_eq!(r["ratioStatus"]["acos"], "reference_only");
        assert_eq!(r["referenceMetrics"]["acos"], 39.6673);
        assert_eq!(r["referenceMetrics"]["tacos"], 12.7776);
        assert_eq!(r["referenceMetrics"]["roas"], 2.521);
        assert!(r["ctr"].as_f64().unwrap() > 0.0);
    }
    #[test]
    fn no_reference_when_inputs_unknown_or_denominator_zero() {
        let r = with_quality(
            metrics([Some(1.0), Some(0.0), None, None, None, Some(0.0), None]),
            &vec!["adRevenue".into()],
        );
        assert!(r["referenceMetrics"].as_object().unwrap().is_empty());
        assert_eq!(r["ratioStatus"]["acos"], "missing_inputs");
        assert_eq!(r["ratioStatus"]["tacos"], "zero_denominator");
    }
    #[test]
    fn series_daily_totals_and_missing_values() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE products(sku TEXT,offer_id TEXT,name TEXT); CREATE TABLE sales_daily(day TEXT,sku TEXT,ordered_units INTEGER,revenue REAL,product_name TEXT); CREATE TABLE ad_daily(day TEXT,sku TEXT,impressions INTEGER,clicks INTEGER,orders INTEGER,spend REAL,revenue REAL);
          INSERT INTO sales_daily VALUES('2026-08-01','A',10,1000,'A'),('2026-08-01','B',20,2000,'B'),('2026-08-02','A',0,0,'A');
          INSERT INTO ad_daily VALUES('2026-08-01','A',100,10,2,20,200),('2026-08-01','B',900,30,3,60,300),('2026-08-01','',1000,40,5,80,500);").unwrap();
        let r = build(
            &c,
            "2026-08-01",
            "2026-08-02",
            vec!["B".into(), "A".into(), "A".into()],
            "AB",
            0.5,
        )
        .unwrap();
        assert_eq!(r["products"].as_array().unwrap().len(), 2);
        assert_eq!(r["products"][0]["daily"].as_array().unwrap().len(), 2);
        let day = &r["seriesDaily"][0];
        assert_eq!(day["totalUnits"], 30.0);
        assert_eq!(day["spend"], 40.0);
        assert_eq!(day["ctr"], 4.0);
        assert_eq!(day["acos"], 16.0);
        assert_eq!(r["seriesSummary"]["totalUnits"], 30.0);
        assert!(r["seriesSummary"]["missingMetrics"]
            .as_array()
            .unwrap()
            .contains(&json!("totalUnits")));
        assert_eq!(r["seriesSummary"]["salesStatus"], "partial");
        assert_eq!(r["products"][0]["daily"][1]["totalUnits"], 0.0);
        assert!(r["products"][0]["daily"][1]["spend"].is_null());
        assert!(r["products"][0]["daily"][1]["tacos"].is_null());
        let full = build(
            &c,
            "2026-08-01",
            "2026-08-01",
            vec!["A".into(), "B".into()],
            "AB",
            1.0,
        )
        .unwrap();
        assert_eq!(full["seriesSummary"]["totalRevenue"], 3000.0);
        assert_eq!(full["seriesSummary"]["spend"], 80.0);
    }
    #[test]
    fn validates_dates_and_calendar_lengths() {
        assert_eq!(dates("2026-08-01", "2026-08-31").unwrap().len(), 31);
        assert_eq!(dates("2024-02-01", "2024-02-29").unwrap().len(), 29);
        assert!(dates("2026-02-29", "2026-03-01").is_err());
        assert!(dates("2026-09-02", "2026-09-01").is_err());
        assert!(dates("2020-01-01", "2026-09-01").is_err());
    }
    #[test]
    fn abc_month_reconciles_all_four_levels() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE products(sku TEXT,offer_id TEXT,name TEXT);CREATE TABLE sales_daily(day TEXT,sku TEXT,ordered_units INTEGER,revenue REAL,product_name TEXT);CREATE TABLE ad_daily(day TEXT,sku TEXT,impressions INTEGER,clicks INTEGER,orders INTEGER,spend REAL,revenue REAL);").unwrap();
        for d in dates("2026-08-01", "2026-08-31").unwrap() {
            for (sku, n) in [("A", 1), ("B", 2), ("C", 3)] {
                c.execute(
                    "INSERT INTO sales_daily VALUES(?1,?2,?3,?4,?2)",
                    params![d, sku, n, 100 * n],
                )
                .unwrap();
                c.execute(
                    "INSERT INTO ad_daily VALUES(?1,?2,?3,?4,?5,?6,?7)",
                    params![d, sku, 100 * n, 10 * n, n, 5 * n, 50 * n],
                )
                .unwrap();
            }
        }
        for end in ["2026-08-07", "2026-08-31"] {
            let r = build(
                &c,
                "2026-08-01",
                end,
                vec!["A".into(), "B".into(), "C".into()],
                "ABC",
                1.0,
            )
            .unwrap();
            for k in KEYS {
                let daily: f64 = r["seriesDaily"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x[k].as_f64().unwrap())
                    .sum();
                let members: f64 = r["products"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x["summary"][k].as_f64().unwrap())
                    .sum();
                assert_eq!(daily, members);
                assert_eq!(daily, r["seriesSummary"][k].as_f64().unwrap());
            }
            assert_eq!(r["seriesSummary"]["acos"], 10.0);
        }
    }
}
