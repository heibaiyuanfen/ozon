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

fn products(token: &str, search: &str, exact_id: Option<i64>) -> Result<Vec<Value>, String> {
    let search = search.to_lowercase();
    let mut products = Vec::new();
    let mut cursor = json!({"limit":100});
    loop {
        let response = request(
            token,
            "content-api",
            "/content/v2/get/cards/list",
            Some(json!({
                "settings":{"filter":{"withPhoto":-1},"cursor":cursor}
            })),
        )?;
        let cards = response["cards"].as_array().ok_or("WB 未返回商品列表")?;
        for card in cards {
            let Some(nm) = card["nmID"].as_i64() else {
                continue;
            };
            let vendor_code = card["vendorCode"].as_str().unwrap_or("");
            if exact_id.map_or_else(
                || {
                    !nm.to_string().contains(&search)
                        && !vendor_code.to_lowercase().contains(&search)
                },
                |id| nm != id,
            ) {
                continue;
            }
            for size in card["sizes"].as_array().into_iter().flatten() {
                if let Some(id) = size["chrtID"].as_i64() {
                    products.push(json!({"nmId":nm,"sizeId":id,"title":card["title"],
                        "vendorCode":vendor_code,"size":size["techSize"]}));
                }
            }
        }
        if cards.len() < 100
            || products.len() >= 100
            || (exact_id.is_some() && !products.is_empty())
        {
            break;
        }
        let next = &response["cursor"];
        if !next["updatedAt"].is_string() || !next["nmID"].is_i64() {
            return Err("WB 商品列表缺少分页游标".into());
        }
        let next_cursor = json!({"limit":100,"updatedAt":next["updatedAt"],"nmID":next["nmID"]});
        if next_cursor == cursor {
            return Err("WB 商品列表无法继续分页".into());
        }
        cursor = next_cursor;
        std::thread::sleep(Duration::from_millis(650));
    }
    Ok(products)
}

fn warehouses(token: &str) -> Result<Value, String> {
    let result = request(token, "marketplace-api", "/api/v3/warehouses", None)?;
    if !result.is_array() {
        return Err("WB 未返回卖家仓列表".into());
    }
    Ok(result)
}

pub(super) fn options(c: &Connection, shop: &str, payload: &Value) -> Result<Value, String> {
    let (_, token) = wb_shop_center::active_token(c, shop)?;
    match payload["search"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(search) => Ok(json!(products(&token, search, None)?)),
        None => warehouses(&token),
    }
}

pub(super) fn update(c: &Connection, shop: &str, payload: &Value) -> Result<Value, String> {
    let id = |key: &str| {
        payload[key]
            .as_i64()
            .filter(|id| *id > 0)
            .ok_or("仓库或规格编号无效")
    };
    let warehouse = id("warehouseId")?;
    let size = id("sizeId")?;
    let amount = payload["amount"]
        .as_u64()
        .filter(|n| *n <= 999_999)
        .ok_or("库存必须是 0 至 999999 的整数")?;
    let mut snapshot = run(c, shop, None)?;
    let matches = |row: &Value| {
        row["warehouseId"].as_i64() == Some(warehouse) && row["sizeId"].as_i64() == Some(size)
    };
    let exists = snapshot["seller"]["rows"]
        .as_array()
        .is_some_and(|rows| rows.iter().any(matches));
    let adding = payload["add"].as_bool() == Some(true);
    if exists == adding {
        return Err(if exists {
            "此规格已有库存明细，请使用修改库存"
        } else {
            "未找到对应卖家仓库存，请重新同步"
        }
        .into());
    }
    let (_, token) = wb_shop_center::active_token(c, shop)?;
    let new_row = if adding {
        let nm = id("nmId")?;
        let product = products(&token, "", Some(nm))?
            .into_iter()
            .find(|row| row["nmId"].as_i64() == Some(nm) && row["sizeId"].as_i64() == Some(size))
            .ok_or("当前店铺找不到该商品规格")?;
        let warehouse_info = warehouses(&token)?
            .as_array()
            .into_iter()
            .flatten()
            .find(|row| row["id"].as_i64() == Some(warehouse))
            .cloned()
            .ok_or("当前店铺找不到该卖家仓")?;
        if let Some(code) = product["vendorCode"].as_str() {
            snapshot["vendorCodes"][nm.to_string()] = json!(code);
        }
        Some(json!({"nmId":nm,"sizeId":size,"warehouseId":warehouse,
            "warehouse":warehouse_info["name"],"quantity":amount,"vendorCode":product["vendorCode"]}))
    } else {
        None
    };
    let path = format!("/api/v3/stocks/{warehouse}");
    if adding {
        if amount == 0 {
            return Err("加入库存的数量必须大于 0".into());
        }
        let current = request(
            &token,
            "marketplace-api",
            &path,
            Some(json!({"chrtIds":[size]})),
        )?;
        let stocks = current["stocks"]
            .as_array()
            .ok_or("WB 未返回当前仓库库存")?;
        for stock in stocks {
            if stock["chrtId"].as_i64() == Some(size) && stock["amount"].as_u64() != Some(0) {
                return Err("该规格在此仓已有可售库存，请同步卖家仓后使用修改库存".into());
            }
        }
    }
    super::wb_operations::request_with_method(
        &token,
        "marketplace-api",
        &path,
        "PUT",
        Some(json!({"stocks":[{"chrtId":size,"amount":amount}]})),
    )?;

    let result = request(
        &token,
        "marketplace-api",
        &path,
        Some(json!({"chrtIds":[size]})),
    )
    .map_err(|e| format!("WB 已接受更新，但回读失败，请查询平台库存后再操作：{e}"))?;
    let confirmed = result["stocks"]
        .as_array()
        .and_then(|stocks| {
            stocks
                .iter()
                .find(|stock| stock["chrtId"].as_i64() == Some(size))
        })
        .and_then(|stock| stock["amount"].as_u64());
    if confirmed != Some(amount) {
        return Err("WB 回读的库存与提交值不一致，请查询平台库存后再操作".into());
    }
    if !snapshot["seller"].is_object() {
        snapshot["seller"] = json!({"rows":[]});
    }
    let rows = snapshot["seller"]["rows"]
        .as_array_mut()
        .ok_or("卖家仓缓存格式不正确")?;
    if let Some(row) = new_row {
        rows.push(row);
    } else if let Some(row) = rows.iter_mut().find(|row| matches(row)) {
        row["quantity"] = json!(amount);
    }
    c.execute(
        "INSERT INTO wb_inventory_snapshots (body,shop_id,resource) VALUES(?1,?2,'seller')
         ON CONFLICT(shop_id,resource) DO UPDATE SET body=excluded.body",
        params![snapshot["seller"].to_string(), shop],
    )
    .map_err(|e| format!("WB 已接受库存更新，本地缓存保存失败，请重新同步卖家仓：{e}"))?;
    Ok(snapshot)
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
