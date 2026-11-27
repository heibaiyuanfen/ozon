use super::{wb_operations::request, wb_shop_center};
use rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};

fn cards(token: &str, query: &str, cursor: &Value) -> Result<Value, String> {
    let mut cursor = cursor.as_object().cloned().unwrap_or_default();
    cursor.retain(|key, _| matches!(key.as_str(), "updatedAt" | "nmID"));
    cursor.insert("limit".into(), json!(50));
    let mut filter = json!({"withPhoto": -1});
    if !query.trim().is_empty() {
        filter["textSearch"] = json!(query.trim());
    }
    send(token, "/content/v2/get/cards/list", json!({
        "settings": {"filter": filter, "cursor": cursor}
    }))
}

fn send(token: &str, path: &str, body: Value) -> Result<Value, String> {
    let result = request(token, "content-api", path, Some(body))?;
    if result["error"] == true {
        return Err(result["errorText"].as_str().filter(|s| !s.is_empty())
            .unwrap_or("WB 拒绝了操作").to_owned());
    }
    Ok(result)
}

fn exact_card(token: &str, id: u64) -> Result<Value, String> {
    let data = cards(token, &id.to_string(), &Value::Null)?;
    data["cards"].as_array().and_then(|rows| rows.iter().find(|row| row["nmID"].as_u64() == Some(id)))
        .cloned().ok_or_else(|| "当前店铺未找到此商品".into())
}

fn media_urls(card: &Value) -> Result<Vec<Value>, String> {
    card["photos"].as_array().into_iter().flatten().map(|photo| {
        photo["big"].as_str().filter(|url| !url.is_empty()).map(|url| json!(url))
            .ok_or_else(|| "已有图片缺少原图地址，请在 WB 卖家后台管理图片".into())
    }).collect()
}

pub(super) fn run(c: &Connection, shop: &str, action: &str, p: &Value) -> Result<Value, String> {
    if action == "product_catalog" || action == "product_catalog_refresh" {
        c.execute_batch("CREATE TABLE IF NOT EXISTS wb_product_catalog(shop_id TEXT PRIMARY KEY, body TEXT NOT NULL)")
            .map_err(|e| e.to_string())?;
        if action == "product_catalog" {
            let raw: Option<String> = c.query_row("SELECT body FROM wb_product_catalog WHERE shop_id=?1",
                [shop], |row| row.get(0)).optional().map_err(|e| e.to_string())?;
            if let Some(raw) = raw { return serde_json::from_str(&raw).map_err(|e| e.to_string()); }
        }
        let (_, token) = wb_shop_center::active_token(c, shop)?;
        let mut cursor = Value::Null;
        let mut catalog = Vec::new();
        let mut ids = std::collections::HashSet::new();
        loop {
            let result = cards(&token, "", &cursor)?;
            let rows = result["cards"].as_array().ok_or("WB 未返回商品列表")?;
            for card in rows {
                let id = card["nmID"].as_u64().ok_or("WB 返回的商品缺少编号")?;
                if !ids.insert(id) { return Err("WB 商品分页重复，请重试更新".into()); }
                catalog.push(card.clone());
            }
            if rows.len() < 50 { break; }
            let next = &result["cursor"];
            if !next["updatedAt"].is_string() || !next["nmID"].is_number() || *next == cursor {
                return Err("WB 未返回有效商品分页游标".into());
            }
            cursor = next.clone();
            std::thread::sleep(std::time::Duration::from_millis(650));
        }
        let data = json!({"cards":catalog,"updatedAt":chrono::Utc::now().to_rfc3339()});
        c.execute("INSERT INTO wb_product_catalog VALUES(?1,?2) ON CONFLICT(shop_id) DO UPDATE SET body=excluded.body",
            rusqlite::params![shop,data.to_string()]).map_err(|e| e.to_string())?;
        return Ok(data);
    }
    let (_, token) = wb_shop_center::active_token(c, shop)?;
    if action == "product_cards" {
        let data = cards(&token, p["query"].as_str().unwrap_or(""), &p["cursor"])?;
        if !data["cards"].is_array() {
            return Err("WB 未返回商品列表".into());
        }
        return Ok(data);
    }
    let id = p["nmId"].as_u64().filter(|id| *id > 0).ok_or("WB 商品 ID 无效")?;
    let card = exact_card(&token, id)?;
    if action == "product_detail" {
        return Ok(card);
    }
    let revision = p["updatedAt"].as_str().filter(|s| !s.is_empty()).ok_or("请先刷新商品")?;
    if card["updatedAt"].as_str() != Some(revision) {
        return Err("商品已被修改，请重新载入后再提交".into());
    }
    match action {
        "product_update" => {
            let mut update = json!({"nmID": id});
            for key in ["vendorCode", "brand", "dimensions", "characteristics", "sizes",
                "documents", "gtin", "wholesale"] {
                if let Some(value) = card.get(key) {
                    update[key] = value.clone();
                }
            }
            if !update["sizes"].is_array() || !update["characteristics"].is_array() || !update["dimensions"].is_object() {
                return Err("WB 返回的商品规格或属性不完整，请刷新后重试".into());
            }
            let title = p["title"].as_str().map(str::trim)
                .filter(|s| !s.is_empty() && s.chars().count() <= 60).ok_or("标题须为 1–60 个字符")?;
            let description = p["description"].as_str().ok_or("缺少描述字段")?;
            update["title"] = json!(title);
            update["description"] = json!(description);
            send(&token, "/content/v2/cards/update", json!([update]))
        }
        "product_media" => {
            let old = media_urls(&card)?;
            if p["originalPhotos"].as_array() != Some(&old) {
                return Err("WB 图片已更新，请重新载入后再操作".into());
            }
            let mut urls = p["photos"].as_array().filter(|urls| !urls.is_empty() && urls.len() <= 30)
                .cloned().ok_or("请保留 1–30 张图片")?;
            let mut unique = std::collections::HashSet::new();
            if !urls.iter().all(|value| value.as_str().is_some_and(|url| {
                let path = url.split(['?', '#']).next().unwrap_or("").to_lowercase();
                url.starts_with("https://") && !url.chars().any(char::is_whitespace)
                    && [".jpg", ".jpeg", ".png", ".bmp", ".gif", ".webp"].iter().any(|ext| path.ends_with(ext))
                    && unique.insert(url)
            })) {
                return Err("图片必须是互不重复的 HTTPS 图片直链".into());
            }
            if let Some(video) = card["video"].as_str().filter(|url| !url.is_empty()) {
                urls.push(json!(video));
            }
            send(&token, "/content/v3/media/save", json!({"nmId": id, "data": urls}))
        }
        _ => Err("不支持的商品操作".into()),
    }
}
