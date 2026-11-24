use chrono::{Days, FixedOffset, NaiveDate, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashSet},
    time::Duration,
};

use crate::{wb_operations::request, wb_shop_center};

const STOCK_LIMIT: usize = 250_000;

fn load(c: &Connection, shop: &str) -> Result<Value, String> {
    let mut statement = c
        .prepare("SELECT resource, body FROM wb_inventory_snapshots WHERE shop_id=?1")
        .map_err(|e| e.to_string())?;
    let mut result = json!({});
    let rows = statement
        .query_map([shop], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?;
    for row in rows {
        let (resource, body) = row.map_err(|e| e.to_string())?;
        result[resource] = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    }
    let catalog: Option<String> = c
        .query_row(
            "SELECT value FROM wb_pricing_state WHERE shop_id=?1 AND kind='catalog'",
            [shop],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some(catalog) = catalog {
        let catalog: Value = serde_json::from_str(&catalog).map_err(|e| e.to_string())?;
        let codes: BTreeMap<i64, &str> = catalog["goods"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| Some((item["nmID"].as_i64()?, item["vendorCode"].as_str()?)))
            .collect();
        result["vendorCodes"] = json!(codes);
    }
    Ok(result)
}

fn stock_rows(token: &str, source: &str) -> Result<Value, String> {
    let path = format!("/api/analytics/v1/stocks-report/{source}-warehouses");
    let mut rows = BTreeMap::new();
    for page in 0..20 {
        if page > 0 {
            std::thread::sleep(Duration::from_secs(20));
        }
        let response = request(
            token,
            "seller-analytics-api",
            &path,
            Some(json!({
                "nmIds":[], "chrtIds":[], "limit":STOCK_LIMIT, "offset":page*STOCK_LIMIT
            })),
        )?;
        let items = response
            .pointer("/data/items")
            .and_then(Value::as_array)
            .ok_or("WB 库存响应缺少明细，已保留上次库存")?;
        for item in items {
            let number = |key: &str| {
                item[key]
                    .as_i64()
                    .ok_or_else(|| format!("WB 库存明细缺少 {key}，已保留上次库存"))
            };
            let nm = number("nmId")?;
            let size = number("chrtId")?;
            let warehouse = number("warehouseId")?;
            let quantity = number("quantity")?;
            rows.insert(
                (nm, size, warehouse),
                json!({
                    "nmId":nm,"sizeId":size,"warehouseId":warehouse,
                    "warehouse":item["warehouseName"].as_str().unwrap_or(""),
                    "quantity":quantity,"toCustomer":item["inWayToClient"].as_i64(),
                    "fromCustomer":item["inWayFromClient"].as_i64()
                }),
            );
        }
        if items.len() < STOCK_LIMIT {
            return Ok(
                json!({"rows":rows.into_values().collect::<Vec<_>>(),"updatedAt":Utc::now().to_rfc3339()}),
            );
        }
    }
    Err("库存超过本次分页上限，已保留上次库存".into())
}

fn count_sales(
    page: &[Value],
    from: NaiveDate,
    until: NaiveDate,
    seen: &mut HashSet<String>,
    totals: &mut BTreeMap<i64, [u64; 2]>,
) -> Result<(), String> {
    for row in page {
        let id = row["saleID"].as_str().ok_or("WB 销售记录缺少编号")?;
        if !id.starts_with('S') || seen.contains(id) {
            continue;
        }
        let date = row["date"]
            .as_str()
            .and_then(|value| value.get(..10))
            .and_then(|date| date.parse::<NaiveDate>().ok())
            .ok_or("WB 销售记录缺少有效日期")?;
        if date < from || date >= until {
            continue;
        }
        let nm = row["nmId"].as_i64().ok_or("WB 销售记录缺少商品编号")?;
        seen.insert(id.to_owned());
        let counts = totals.entry(nm).or_default();
        counts[1] += 1;
        if (until - date).num_days() <= 7 {
            counts[0] += 1;
        }
    }
    Ok(())
}

fn sales(token: &str) -> Result<Value, String> {
    let until = Utc::now()
        .with_timezone(&FixedOffset::east_opt(10800).unwrap())
        .date_naive();
    let from = until
        .checked_sub_days(Days::new(30))
        .ok_or("销量起始日期无效")?;
    let mut cursor = format!("{from}T00:00:00");
    let mut seen = HashSet::new();
    let mut totals = BTreeMap::new();
    for page_index in 0..30 {
        if page_index > 0 {
            std::thread::sleep(Duration::from_secs(60));
        }
        let response = request(
            token,
            "statistics-api",
            &format!("/api/v1/supplier/sales?dateFrom={cursor}&flag=0"),
            None,
        )?;
        let page = response
            .as_array()
            .ok_or("WB 销售响应缺少明细，已保留上次销量")?;
        if page.is_empty() {
            return Ok(
                json!({"totals":totals,"through":until.pred_opt(),"updatedAt":Utc::now().to_rfc3339()}),
            );
        }
        count_sales(page, from, until, &mut seen, &mut totals)?;
        let next = page
            .last()
            .and_then(|row| row["lastChangeDate"].as_str())
            .ok_or("WB 销售分页缺少游标，已保留上次销量")?;
        // WB returns the boundary record again when dateFrom equals lastChangeDate.
        if next == cursor && page.len() == 1 {
            return Ok(
                json!({"totals":totals,"through":until.pred_opt(),"updatedAt":Utc::now().to_rfc3339()}),
            );
        }
        if next <= cursor.as_str() {
            return Err("WB 销售分页游标无法前进，已保留上次销量".into());
        }
        cursor = next.to_owned();
    }
    Err("销售超过本次分页上限，已保留上次销量".into())
}

pub(super) fn run(c: &Connection, shop: &str, resource: Option<&str>) -> Result<Value, String> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS wb_inventory_snapshots(
        shop_id TEXT NOT NULL, resource TEXT NOT NULL, body TEXT NOT NULL, PRIMARY KEY(shop_id,resource));
        CREATE TABLE IF NOT EXISTS wb_pricing_state(
        shop_id TEXT NOT NULL, kind TEXT NOT NULL, value TEXT NOT NULL, PRIMARY KEY(shop_id,kind));"
    ).map_err(|e| e.to_string())?;
    if let Some(resource) = resource {
        let (_, token) = wb_shop_center::active_token(c, shop)?;
        let data = match resource {
            "wb" | "seller" => stock_rows(&token, resource)?,
            "sales" => sales(&token)?,
            _ => return Err("库存数据来源无效".into()),
        };
        c.execute(
            "INSERT INTO wb_inventory_snapshots VALUES(?1,?2,?3)
            ON CONFLICT(shop_id,resource) DO UPDATE SET body=excluded.body",
            params![shop, resource, data.to_string()],
        )
        .map_err(|e| e.to_string())?;
    }
    load(c, shop)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn counts_only_unique_sales_in_the_completed_windows() {
        let until = NaiveDate::from_ymd_opt(2026, 11, 24).unwrap();
        let from = until.checked_sub_days(Days::new(30)).unwrap();
        let mut seen = HashSet::new();
        let mut totals = BTreeMap::new();
        let page = [
            json!({"saleID":"S1","date":"2026-11-17T01:00:00","nmId":42}),
            json!({"saleID":"S2","date":"2026-11-16T01:00:00","nmId":42}),
            json!({"saleID":"S3","date":"2026-11-24T01:00:00","nmId":42}),
            json!({"saleID":"R1","date":"2026-11-20T01:00:00","nmId":42}),
        ];
        for _ in 0..2 {
            count_sales(&page, from, until, &mut seen, &mut totals).unwrap();
        }
        assert_eq!(totals[&42], [1, 2]);
    }
}
