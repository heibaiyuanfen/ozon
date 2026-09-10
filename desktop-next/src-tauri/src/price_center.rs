use crate::{product_master, wb_shop_center, AppState};
use chrono::{Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use tauri::State;

type Result<T> = std::result::Result<T, String>;
const ACTOR: &str = "local-desktop";

fn now() -> String {
    Utc::now().to_rfc3339()
}
fn uid(prefix: &str) -> String {
    format!(
        "{prefix}-{}",
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    )
}
fn field<'a>(p: &'a Value, key: &str) -> Result<&'a str> {
    p.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("缺少 {key}"))
}
fn money(value: &str) -> Result<i64> {
    let s = value.trim();
    if s.is_empty() {
        return Err("金额不能为空".into());
    }
    let mut it = s.split('.');
    let whole = it.next().unwrap_or("");
    let frac = it.next().unwrap_or("");
    if it.next().is_some()
        || whole.is_empty()
        || !whole.chars().all(|c| c.is_ascii_digit())
        || !frac.chars().all(|c| c.is_ascii_digit())
        || frac.len() > 2
    {
        return Err(format!("金额格式无效：{value}"));
    }
    let w = whole.parse::<i64>().map_err(|_| "金额过大")?;
    let f = match frac.len() {
        0 => 0,
        1 => frac.parse::<i64>().unwrap() * 10,
        _ => frac.parse::<i64>().unwrap(),
    };
    w.checked_mul(100)
        .and_then(|x| x.checked_add(f))
        .ok_or_else(|| "金额过大".into())
}
fn fmt(v: i64) -> String {
    format!("{}.{:02}", v / 100, v % 100)
}
fn db(state: &AppState) -> Result<Connection> {
    let c = wb_shop_center::db(state)?;
    product_master::ensure(&c)?;
    schema(&c)?;
    Ok(c)
}
fn schema(c: &Connection) -> Result<()> {
    c.execute_batch(r#"
CREATE TABLE IF NOT EXISTS price_raw_payloads(id TEXT PRIMARY KEY,shop_id TEXT NOT NULL,payload TEXT NOT NULL,fetched_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS price_snapshots(id TEXT PRIMARY KEY,shop_id TEXT NOT NULL,sku_id TEXT,listing_id TEXT NOT NULL,source TEXT NOT NULL,base_price TEXT,discount_pct TEXT,effective_price TEXT,currency TEXT NOT NULL,observed_at TEXT NOT NULL,fetched_at TEXT NOT NULL,data_status TEXT NOT NULL,raw_payload_id TEXT,change_type TEXT NOT NULL DEFAULT 'WILDBERRIES_SYNC');
CREATE INDEX IF NOT EXISTS idx_price_snapshot_listing ON price_snapshots(shop_id,listing_id,observed_at DESC);
CREATE TABLE IF NOT EXISTS price_rules(id TEXT PRIMARY KEY,organization_id TEXT NOT NULL,shop_id TEXT,sku_id TEXT,spu_id TEXT,minimum_safe_price TEXT,target_margin_pct TEXT,target_profit_per_unit TEXT,max_discount_pct TEXT,price_floor TEXT,price_ceiling TEXT,rounding_rule TEXT NOT NULL DEFAULT 'none',enabled INTEGER NOT NULL DEFAULT 1,created_at TEXT NOT NULL,updated_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS price_changes(id TEXT PRIMARY KEY,shop_id TEXT NOT NULL,mode TEXT NOT NULL,status TEXT NOT NULL,created_by TEXT NOT NULL,reason TEXT,override_reason TEXT,created_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS price_change_items(id TEXT PRIMARY KEY,price_change_id TEXT NOT NULL,sku_id TEXT,listing_id TEXT NOT NULL,nm_id TEXT NOT NULL,before_base_price TEXT,before_discount_pct TEXT,before_effective_price TEXT,requested_price TEXT NOT NULL,requested_discount_pct TEXT,expected_effective_price TEXT,guard_status TEXT NOT NULL,guard_message TEXT NOT NULL,external_status TEXT NOT NULL DEFAULT 'pending',external_error_code TEXT,external_error_message TEXT,retryable INTEGER NOT NULL DEFAULT 0,verified_price TEXT,verified_at TEXT,idempotency_key TEXT NOT NULL UNIQUE);
CREATE INDEX IF NOT EXISTS idx_price_change_item_change ON price_change_items(price_change_id);
CREATE TABLE IF NOT EXISTS price_write_jobs(id TEXT PRIMARY KEY,price_change_id TEXT NOT NULL UNIQUE,status TEXT NOT NULL,progress INTEGER NOT NULL DEFAULT 0,success_count INTEGER NOT NULL DEFAULT 0,failed_count INTEGER NOT NULL DEFAULT 0,blocked_count INTEGER NOT NULL DEFAULT 0,external_job_id TEXT,created_at TEXT NOT NULL,updated_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS promotions(id TEXT PRIMARY KEY,shop_id TEXT NOT NULL,external_promotion_id TEXT NOT NULL,name TEXT NOT NULL,starts_at TEXT,ends_at TEXT,status TEXT NOT NULL,raw_rules TEXT,data_status TEXT NOT NULL,last_synced_at TEXT,UNIQUE(shop_id,external_promotion_id));
CREATE TABLE IF NOT EXISTS promotion_eligibility(promotion_id TEXT NOT NULL,sku_id TEXT,listing_id TEXT NOT NULL,eligible INTEGER NOT NULL,required_price TEXT,required_discount TEXT,estimated_margin TEXT,estimated_profit TEXT,guard_status TEXT NOT NULL,reason TEXT,PRIMARY KEY(promotion_id,listing_id));
"#).map_err(|e|e.to_string())
}
fn audit(c: &Connection, shop: &str, action: &str, data: Value) -> Result<()> {
    c.execute("INSERT INTO wb_audit_logs(organization_id,shop_id,actor,action,metadata_json,created_at)VALUES('local',?1,?2,?3,?4,?5)",params![shop,ACTOR,action,data.to_string(),now()]).map_err(|e|e.to_string())?;
    Ok(())
}
fn role(allowed: &[&str]) -> Result<String> {
    let r = std::env::var("WBERP_LOCAL_ROLE").unwrap_or_else(|_| "admin".into());
    if allowed.contains(&r.as_str()) {
        Ok(r)
    } else {
        Err("Permission denied".into())
    }
}
fn request(token: &str, method: &str, url: &str, body: Option<Value>) -> Result<Value> {
    let req = match method {
        "POST" => ureq::post(url),
        _ => ureq::get(url),
    }
    .set("Authorization", token)
    .set("Accept", "application/json")
    .set("Content-Type", "application/json");
    let response = if let Some(v) = body {
        req.send_string(&v.to_string())
    } else {
        req.call()
    }
    .map_err(|e| format!("WB API 请求失败：{e}"))?;
    let raw = response.into_string().map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| format!("WB API 响应无法解析：{e}"))
}
fn latest(c: &Connection, listing: &str) -> Result<Value> {
    c.query_row("SELECT json_object('basePrice',base_price,'discountPct',discount_pct,'effectivePrice',effective_price,'currency',currency,'observedAt',observed_at,'dataStatus',data_status) FROM price_snapshots WHERE listing_id=?1 ORDER BY observed_at DESC,rowid DESC LIMIT 1",[listing],|r|{let s:String=r.get(0)?;Ok(serde_json::from_str(&s).unwrap_or(Value::Null))}).optional().map_err(|e|e.to_string())?.ok_or_else(||"该商品还没有价格快照，请先同步价格".into())
}
fn applied_rule(c: &Connection, shop: &str, sku: Option<&str>) -> Result<Option<Value>> {
    let spu = sku
        .and_then(|s| {
            c.query_row("SELECT spu_id FROM pm_skus WHERE id=?1", [s], |r| {
                r.get::<_, Option<String>>(0)
            })
            .ok()
        })
        .flatten();
    let mut q=c.prepare("SELECT json_object('id',id,'minimumSafePrice',minimum_safe_price,'targetMarginPct',target_margin_pct,'targetProfitPerUnit',target_profit_per_unit,'maxDiscountPct',max_discount_pct,'priceFloor',price_floor,'priceCeiling',price_ceiling,'roundingRule',rounding_rule) FROM price_rules WHERE enabled=1 AND organization_id='local' AND (shop_id IS NULL OR shop_id=?1) AND (sku_id IS NULL OR sku_id=?2) AND (spu_id IS NULL OR spu_id=?3) ORDER BY CASE WHEN sku_id=?2 THEN 4 WHEN spu_id=?3 AND spu_id IS NOT NULL THEN 3 WHEN shop_id=?1 THEN 2 ELSE 1 END DESC,updated_at DESC LIMIT 1").map_err(|e|e.to_string())?;
    q.query_row(params![shop, sku, spu], |r| {
        let s: String = r.get(0)?;
        Ok(serde_json::from_str(&s).unwrap_or(Value::Null))
    })
    .optional()
    .map_err(|e| e.to_string())
}
fn guard(c: &Connection, shop: &str, listing: &str, proposed: &str) -> Result<Value> {
    let cents = money(proposed)?;
    if cents <= 0 {
        return Err("新价格必须大于 0".into());
    }
    let (sku, status): (Option<String>, String) = c
        .query_row(
            "SELECT sku_id,mapping_status FROM pm_listings WHERE id=?1 AND shop_id=?2",
            params![listing, shop],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| "Listing 与店铺不匹配")?;
    if status != "MAPPED" {
        return Ok(
            json!({"status":"blocked","proposedPrice":fmt(cents),"minimumSafePrice":null,"estimatedMarginPct":null,"estimatedProfitPerUnit":null,"reasons":[format!("Listing 状态为 {status}，禁止写价")],"costDataStatus":"missing"}),
        );
    }
    let current = latest(c, listing)?;
    let rule = applied_rule(c, shop, sku.as_deref())?;
    let safe = rule
        .as_ref()
        .and_then(|r| r["minimumSafePrice"].as_str())
        .and_then(|x| money(x).ok());
    let cost = sku
        .as_deref()
        .and_then(|s| {
            c.query_row("SELECT purchase_cost FROM pm_skus WHERE id=?1", [s], |r| {
                r.get::<_, Option<String>>(0)
            })
            .ok()
        })
        .flatten()
        .and_then(|x| money(&x).ok());
    let mut reasons = Vec::new();
    let (state, cost_state) = if safe.is_none() {
        reasons.push("未配置最低安全价；成本结构尚不完整，不能给出精确利润保护".to_string());
        ("insufficient_data", "partial")
    } else if cents < safe.unwrap() {
        reasons.push(format!("新价格低于最低安全价 {}", fmt(safe.unwrap())));
        (
            "blocked",
            if cost.is_some() { "partial" } else { "missing" },
        )
    } else {
        ("safe", if cost.is_some() { "partial" } else { "missing" })
    };
    let profit = cost.map(|v| cents - v);
    let margin = profit.map(|v| format!("{:.2}", v as f64 / cents as f64 * 100.0));
    Ok(
        json!({"status":state,"currentPrice":current["basePrice"],"proposedPrice":fmt(cents),"minimumSafePrice":safe.map(fmt),"estimatedMarginPct":margin,"estimatedProfitPerUnit":profit.map(fmt),"reasons":reasons,"costDataStatus":cost_state,"appliedRule":rule}),
    )
}
fn dashboard(c: &Connection, shop: &str) -> Result<Value> {
    let rows=query(c,"SELECT json_object('listingId',l.id,'skuId',l.sku_id,'sku',k.code,'name',COALESCE(k.name,l.title),'nmId',l.nm_id,'mappingStatus',l.mapping_status,'basePrice',p.base_price,'discountPct',p.discount_pct,'effectivePrice',p.effective_price,'currency',COALESCE(p.currency,'RUB'),'observedAt',p.observed_at,'dataStatus',COALESCE(p.data_status,'missing'),'minimumSafePrice',(SELECT minimum_safe_price FROM price_rules r WHERE r.enabled=1 AND (r.shop_id IS NULL OR r.shop_id=l.shop_id) AND (r.sku_id IS NULL OR r.sku_id=l.sku_id) ORDER BY CASE WHEN r.sku_id=l.sku_id THEN 3 WHEN r.shop_id=l.shop_id THEN 2 ELSE 1 END DESC LIMIT 1)) FROM pm_listings l LEFT JOIN pm_skus k ON k.id=l.sku_id LEFT JOIN price_snapshots p ON p.rowid=(SELECT rowid FROM price_snapshots z WHERE z.shop_id=l.shop_id AND z.listing_id=l.id ORDER BY z.observed_at DESC,z.rowid DESC LIMIT 1) WHERE l.shop_id=?1 AND l.mapping_status!='IGNORED' ORDER BY k.code,l.nm_id",[shop])?;
    let history=query(c,"SELECT json_object('id',id,'listingId',listing_id,'basePrice',base_price,'discountPct',discount_pct,'effectivePrice',effective_price,'source',source,'changeType',change_type,'observedAt',observed_at) FROM price_snapshots WHERE shop_id=?1 ORDER BY observed_at DESC,rowid DESC LIMIT 300",[shop])?;
    let jobs=query(c,"SELECT json_object('id',j.id,'changeId',j.price_change_id,'status',j.status,'progress',j.progress,'successCount',j.success_count,'failedCount',j.failed_count,'blockedCount',j.blocked_count,'externalJobId',j.external_job_id,'createdAt',j.created_at,'reason',c.reason) FROM price_write_jobs j JOIN price_changes c ON c.id=j.price_change_id WHERE c.shop_id=?1 ORDER BY j.created_at DESC LIMIT 100",[shop])?;
    let promotions=query(c,"SELECT json_object('id',id,'externalPromotionId',external_promotion_id,'name',name,'startsAt',starts_at,'endsAt',ends_at,'status',status,'dataStatus',data_status,'lastSyncedAt',last_synced_at) FROM promotions WHERE shop_id=?1 ORDER BY starts_at DESC",[shop])?;
    Ok(json!({"rows":rows,"history":history,"jobs":jobs,"promotions":promotions}))
}
fn query<P: rusqlite::Params>(c: &Connection, sql: &str, p: P) -> Result<Vec<Value>> {
    let mut s = c.prepare(sql).map_err(|e| e.to_string())?;
    let rows = s
        .query_map(p, |r| {
            let x: String = r.get(0)?;
            Ok(serde_json::from_str(&x).unwrap_or(Value::Null))
        })
        .map_err(|e| e.to_string())?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}
fn sync_prices(c: &Connection, shop: &str) -> Result<Value> {
    role(&["admin", "operator"])?;
    let (_, token) = wb_shop_center::active_token(c, shop)?;
    let run = uid("price-sync");
    let t = now();
    c.execute("INSERT INTO wb_sync_jobs(id,shop_id,resource_type,mode,status,created_at)VALUES(?1,?2,'prices','incremental','running',?3)",params![run,shop,t]).map_err(|e|e.to_string())?;
    let mut offset = 0;
    let mut count = 0;
    loop {
        let payload=request(&token,"GET",&format!("https://discounts-prices-api.wildberries.ru/api/v2/list/goods/filter?limit=1000&offset={offset}"),None)?;
        let list = payload
            .pointer("/data/listGoods")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if list.is_empty() {
            break;
        }
        let raw = uid("price-raw");
        c.execute(
            "INSERT INTO price_raw_payloads(id,shop_id,payload,fetched_at)VALUES(?1,?2,?3,?4)",
            params![raw, shop, payload.to_string(), now()],
        )
        .map_err(|e| e.to_string())?;
        for item in &list {
            let nm = item["nmID"]
                .as_i64()
                .map(|x| x.to_string())
                .unwrap_or_default();
            let base = item
                .pointer("/sizes/0/price")
                .and_then(Value::as_f64)
                .map(|x| format!("{x:.2}"));
            let effective = item
                .pointer("/sizes/0/discountedPrice")
                .and_then(Value::as_f64)
                .map(|x| format!("{x:.2}"));
            let discount = item
                .get("discount")
                .and_then(Value::as_f64)
                .map(|x| format!("{x:.2}"));
            let listings=query(c,"SELECT json_object('id',id,'skuId',sku_id) FROM pm_listings WHERE shop_id=?1 AND nm_id=?2",params![shop,nm])?;
            for l in listings {
                let id = uid("price-snapshot");
                let prior:Option<String>=c.query_row("SELECT base_price FROM price_snapshots WHERE listing_id=?1 ORDER BY observed_at DESC,rowid DESC LIMIT 1",[l["id"].as_str()],|r|r.get(0)).optional().map_err(|e|e.to_string())?.flatten();
                let has_intent:i64=c.query_row("SELECT EXISTS(SELECT 1 FROM price_change_items WHERE listing_id=?1 AND external_status='pending_verify' AND requested_price=?2)",params![l["id"].as_str(),base.as_deref()],|r|r.get(0)).map_err(|e|e.to_string())?;
                let change = if prior.is_some() && prior != base && has_intent == 0 {
                    "UNKNOWN_EXTERNAL_CHANGE"
                } else {
                    "WILDBERRIES_SYNC"
                };
                c.execute("INSERT INTO price_snapshots(id,shop_id,sku_id,listing_id,source,base_price,discount_pct,effective_price,currency,observed_at,fetched_at,data_status,raw_payload_id,change_type)VALUES(?1,?2,?3,?4,'wildberries',?5,?6,?7,?8,?9,?9,?10,?11,?12)",params![id,shop,l["skuId"].as_str(),l["id"].as_str(),base,discount,effective,item["currencyIsoCode4217"].as_str().unwrap_or("RUB"),now(),if base.is_some(){"complete"}else{"partial"},raw,change]).map_err(|e|e.to_string())?;
                c.execute("UPDATE price_change_items SET external_status=CASE WHEN requested_price=?2 THEN 'verified' ELSE 'mismatch' END,verified_price=?2,verified_at=?3 WHERE listing_id=?1 AND external_status='pending_verify'",params![l["id"].as_str(),base.as_deref(),now()]).map_err(|e|e.to_string())?;
                count += 1;
            }
        }
        if list.len() < 1000 {
            break;
        }
        offset += 1000;
    }
    c.execute(
        "UPDATE wb_sync_jobs SET status='success' WHERE id=?1",
        [&run],
    )
    .map_err(|e| e.to_string())?;
    c.execute("UPDATE price_changes SET status=CASE WHEN EXISTS(SELECT 1 FROM price_change_items i WHERE i.price_change_id=price_changes.id AND i.external_status='mismatch') THEN 'partial' WHEN NOT EXISTS(SELECT 1 FROM price_change_items i WHERE i.price_change_id=price_changes.id AND i.external_status='pending_verify') THEN 'success' ELSE status END WHERE shop_id=?1 AND status IN('pending','partial')",[shop]).map_err(|e|e.to_string())?;
    c.execute("UPDATE price_write_jobs SET status=(SELECT status FROM price_changes WHERE id=price_change_id),progress=100,success_count=(SELECT COUNT(*) FROM price_change_items i WHERE i.price_change_id=price_write_jobs.price_change_id AND i.external_status='verified'),failed_count=(SELECT COUNT(*) FROM price_change_items i WHERE i.price_change_id=price_write_jobs.price_change_id AND i.external_status IN('failed','mismatch')),updated_at=?2 WHERE price_change_id IN(SELECT id FROM price_changes WHERE shop_id=?1)",params![shop,now()]).map_err(|e|e.to_string())?;
    audit(c, shop, "PRICE_SYNC", json!({"records":count}))?;
    Ok(json!({"records":count}))
}
fn execute(c: &Connection, p: &Value, retry_only: bool) -> Result<Value> {
    role(&["admin", "operator"])?;
    let shop = field(p, "shopId")?;
    let reason = field(p, "reason")?;
    let items = p["items"].as_array().ok_or("缺少改价明细")?;
    if items.is_empty() {
        return Err("至少选择一个 SKU".into());
    }
    let change = if retry_only {
        field(p, "changeId")?.to_string()
    } else {
        uid("price-change")
    };
    if !retry_only {
        c.execute("INSERT INTO price_changes(id,shop_id,mode,status,created_by,reason,created_at)VALUES(?1,?2,?3,'previewed',?4,?5,?6)",params![change,shop,p["mode"].as_str().unwrap_or("bulk"),ACTOR,reason,now()]).map_err(|e|e.to_string())?;
    }
    let mut send = Vec::new();
    let mut blocked = 0;
    for item in items {
        let listing = field(item, "listingId")?;
        let proposed = field(item, "proposedPrice")?;
        let g = guard(c, shop, listing, proposed)?;
        if g["status"] == "blocked" || g["status"] == "insufficient_data" {
            blocked += 1;
            continue;
        }
        let (sku, nm): (Option<String>, String) = c
            .query_row(
                "SELECT sku_id,nm_id FROM pm_listings WHERE id=?1 AND shop_id=?2",
                params![listing, shop],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        let old = latest(c, listing)?;
        let discount = old["discountPct"]
            .as_str()
            .and_then(|x| x.parse::<i64>().ok())
            .unwrap_or(0);
        let key = format!("{}:{}:{}", change, listing, proposed);
        c.execute("INSERT OR IGNORE INTO price_change_items(id,price_change_id,sku_id,listing_id,nm_id,before_base_price,before_discount_pct,before_effective_price,requested_price,requested_discount_pct,expected_effective_price,guard_status,guard_message,idempotency_key)VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?9,?11,?12,?13)",params![uid("price-item"),change,sku,listing,nm,old["basePrice"].as_str(),old["discountPct"].as_str(),old["effectivePrice"].as_str(),fmt(money(proposed)?),discount.to_string(),g["status"].as_str(),g["reasons"].to_string(),key]).map_err(|e|e.to_string())?;
        send.push(json!({"nmID":nm.parse::<i64>().map_err(|_|"nmID 无效")?,"price":money(proposed)?/100,"discount":discount}));
    }
    let job = uid("price-job");
    c.execute("INSERT OR IGNORE INTO price_write_jobs(id,price_change_id,status,blocked_count,created_at,updated_at)VALUES(?1,?2,'running',?3,?4,?4)",params![job,change,blocked,now()]).map_err(|e|e.to_string())?;
    if send.is_empty() {
        c.execute(
            "UPDATE price_changes SET status='failed' WHERE id=?1",
            [&change],
        )
        .map_err(|e| e.to_string())?;
        c.execute(
            "UPDATE price_write_jobs SET status='failed',progress=100 WHERE price_change_id=?1",
            [&change],
        )
        .map_err(|e| e.to_string())?;
        return Ok(json!({"changeId":change,"status":"failed","blocked":blocked}));
    }
    let (_, token) = wb_shop_center::active_token(c, shop)?;
    match request(
        &token,
        "POST",
        "https://discounts-prices-api.wildberries.ru/api/v2/upload/task",
        Some(json!({"data":send})),
    ) {
        Ok(v) => {
            let external = v
                .pointer("/data/id")
                .and_then(Value::as_i64)
                .map(|x| x.to_string());
            c.execute("UPDATE price_change_items SET external_status='pending_verify' WHERE price_change_id=?1 AND external_status='pending'",[&change]).map_err(|e|e.to_string())?;
            c.execute(
                "UPDATE price_changes SET status=?2 WHERE id=?1",
                params![change, if blocked > 0 { "partial" } else { "pending" }],
            )
            .map_err(|e| e.to_string())?;
            c.execute("UPDATE price_write_jobs SET status='running',progress=60,success_count=0,external_job_id=?2,updated_at=?3 WHERE price_change_id=?1",params![change,external,now()]).map_err(|e|e.to_string())?;
            audit(
                c,
                shop,
                if items.len() > 1 {
                    "PRICE_BULK_CHANGE"
                } else {
                    "PRICE_CHANGE"
                },
                json!({"changeId":change,"externalJobId":external,"count":send.len()}),
            )?;
            Ok(
                json!({"changeId":change,"status":if blocked>0{"partial"}else{"pending"},"externalJobId":external,"submitted":send.len(),"blocked":blocked}),
            )
        }
        Err(e) => {
            let retryable = e.contains("429") || e.contains("500") || e.contains("timeout");
            c.execute("UPDATE price_change_items SET external_status='failed',external_error_code=?2,external_error_message=?3,retryable=?4 WHERE price_change_id=?1 AND external_status='pending'",params![change,if e.contains("429"){"RATE_LIMITED"}else{"WRITE_FAILED"},e,retryable as i32]).map_err(|x|x.to_string())?;
            c.execute(
                "UPDATE price_changes SET status='failed' WHERE id=?1",
                [&change],
            )
            .map_err(|x| x.to_string())?;
            c.execute("UPDATE price_write_jobs SET status='failed',progress=100,failed_count=?2,updated_at=?3 WHERE price_change_id=?1",params![change,send.len() as i64,now()]).map_err(|x|x.to_string())?;
            Err(format!(
                "Price write failed. No platform change confirmed. {e}"
            ))
        }
    }
}
fn sync_promotions(c: &Connection, shop: &str) -> Result<Value> {
    role(&["admin", "operator"])?;
    let (_, token) = wb_shop_center::active_token(c, shop)?;
    let start = (Utc::now() - Duration::days(30)).format("%Y-%m-%dT00:00:00Z");
    let end = (Utc::now() + Duration::days(180)).format("%Y-%m-%dT23:59:59Z");
    let v=request(&token,"GET",&format!("https://dp-calendar-api.wildberries.ru/api/v1/calendar/promotions?startDateTime={start}&endDateTime={end}&allPromo=true&limit=1000&offset=0"),None)?;
    let list = v
        .pointer("/data/promotions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let current = now();
    for x in &list {
        let external = x["id"].as_i64().map(|n| n.to_string()).unwrap_or_default();
        let start = x["startDateTime"].as_str();
        let end = x["endDateTime"].as_str();
        let status = if end.unwrap_or("") < current.as_str() {
            "ended"
        } else if start.unwrap_or("") > current.as_str() {
            "upcoming"
        } else {
            "active"
        };
        c.execute("INSERT INTO promotions(id,shop_id,external_promotion_id,name,starts_at,ends_at,status,raw_rules,data_status,last_synced_at)VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'complete',?9) ON CONFLICT(shop_id,external_promotion_id)DO UPDATE SET name=excluded.name,starts_at=excluded.starts_at,ends_at=excluded.ends_at,status=excluded.status,raw_rules=excluded.raw_rules,data_status='complete',last_synced_at=excluded.last_synced_at",params![uid("promotion"),shop,external,x["name"].as_str().unwrap_or("未命名活动"),start,end,status,x.to_string(),now()]).map_err(|e|e.to_string())?;
    }
    audit(c, shop, "PROMOTION_SYNC", json!({"records":list.len()}))?;
    Ok(json!({"records":list.len()}))
}

#[tauri::command]
pub fn price_center(
    action: String,
    payload: Option<Value>,
    state: State<AppState>,
) -> Result<Value> {
    let p = payload.unwrap_or_else(|| json!({}));
    let c = db(&state)?;
    match action.as_str() {
        "dashboard" => dashboard(&c, field(&p, "shopId")?),
        "preview" => guard(
            &c,
            field(&p, "shopId")?,
            field(&p, "listingId")?,
            field(&p, "proposedPrice")?,
        ),
        "save_rule" => {
            role(&["admin", "finance"])?;
            let shop = p["shopId"].as_str();
            let sku = p["skuId"].as_str();
            let min = p["minimumSafePrice"]
                .as_str()
                .filter(|x| !x.trim().is_empty());
            if let Some(x) = min {
                money(x)?;
            }
            let id = uid("price-rule");
            c.execute("INSERT INTO price_rules(id,organization_id,shop_id,sku_id,minimum_safe_price,target_margin_pct,target_profit_per_unit,max_discount_pct,price_floor,price_ceiling,rounding_rule,enabled,created_at,updated_at)VALUES(?1,'local',?2,?3,?4,?5,?6,?7,?8,?9,?10,1,?11,?11)",params![id,shop,sku,min,p["targetMarginPct"].as_str(),p["targetProfitPerUnit"].as_str(),p["maxDiscountPct"].as_str(),p["priceFloor"].as_str(),p["priceCeiling"].as_str(),p["roundingRule"].as_str().unwrap_or("none"),now()]).map_err(|e|e.to_string())?;
            audit(
                &c,
                shop.unwrap_or(""),
                "PRICE_RULE_CONFIG",
                json!({"ruleId":id,"skuId":sku}),
            )?;
            Ok(json!({"id":id}))
        }
        "execute" => execute(&c, &p, false),
        "sync_prices" => sync_prices(&c, field(&p, "shopId")?),
        "sync_promotions" => sync_promotions(&c, field(&p, "shopId")?),
        _ => Err("未知价格中心操作".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decimal_money_is_exact() {
        assert_eq!(money("1290.05").unwrap(), 129005);
        assert_eq!(fmt(129005), "1290.05");
        assert!(money("1.009").is_err());
    }
    #[test]
    fn schema_and_guard_block_unmapped() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE wb_shops(id TEXT PRIMARY KEY,organization_id TEXT,status TEXT);INSERT INTO wb_shops VALUES('s','local','active');").unwrap();
        product_master::ensure(&c).unwrap();
        schema(&c).unwrap();
        c.execute("INSERT INTO pm_listings(id,shop_id,marketplace,nm_id,chrt_id,snapshot_json,mapping_status,data_status)VALUES('l','s','wildberries','1','2','{}','UNMAPPED','complete')",[]).unwrap();
        c.execute("INSERT INTO price_snapshots(id,shop_id,listing_id,source,base_price,currency,observed_at,fetched_at,data_status)VALUES('p','s','l','wildberries','100.00','RUB','2026','2026','complete')",[]).unwrap();
        assert_eq!(guard(&c, "s", "l", "90.00").unwrap()["status"], "blocked");
    }
}
