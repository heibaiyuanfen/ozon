//! Cross-shop listing migration. Drafts and remote task IDs survive restarts.
use crate::{read_registry, seller_post, seller_post_once, setting, AppState};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::State;

fn store(state: &AppState) -> Result<Connection, String> {
    let c =
        Connection::open(state.data_dir.join("product-transfer.db")).map_err(|e| e.to_string())?;
    c.busy_timeout(std::time::Duration::from_secs(10))
        .map_err(|e| e.to_string())?;
    c.execute_batch("CREATE TABLE IF NOT EXISTS transfers(id TEXT PRIMARY KEY, source TEXT NOT NULL, target TEXT NOT NULL, offer TEXT NOT NULL, data TEXT NOT NULL, UNIQUE(source,target,offer));").map_err(|e| e.to_string())?;
    Ok(c)
}
fn shop(state: &AppState, id: &str) -> Result<Connection, String> {
    let registry = read_registry(&state.data_dir)?;
    let s = registry
        .shops
        .iter()
        .find(|s| s.id == id)
        .ok_or("店铺不存在")?;
    if s.kind != "cross_border" {
        return Err("第一版仅支持跨境店铺之间迁移".into());
    }
    let c = Connection::open_with_flags(
        state.data_dir.join(&s.database_file),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| e.to_string())?;
    Ok(c)
}
fn pair(state: &AppState, source: &str, target: &str) -> Result<(Connection, Connection), String> {
    if source == target {
        return Err("源店和目标店不能相同".into());
    }
    let s = shop(state, source)?;
    let t = shop(state, target)?;
    let a = setting(&s, "seller_client_id");
    let b = setting(&t, "seller_client_id");
    if a.is_empty() || b.is_empty() {
        return Err("两个店铺都需要配置 Seller API".into());
    }
    if a == b {
        return Err("两个店铺使用相同 Seller Client ID，不能迁移到自身".into());
    }
    Ok((s, t))
}
fn array(v: &Value) -> Vec<Value> {
    v.get("items")
        .or_else(|| v.pointer("/result/items"))
        .or_else(|| v.get("result").filter(|r| r.is_array()))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}
fn checked_items(v: &Value) -> Result<Vec<Value>, String> {
    if v.get("items")
        .or_else(|| v.pointer("/result/items"))
        .or_else(|| v.get("result").filter(|r| r.is_array()))
        .is_some_and(Value::is_array)
    {
        Ok(array(v))
    } else {
        Err("平台列表响应结构异常，已停止处理；不能将未知响应当作无商品".into())
    }
}
fn target_items(c: &Connection, offer: &str) -> Result<Vec<Value>, String> {
    checked_items(&seller_post(
        c,
        "/v3/product/info/list",
        &json!({"offer_id":[offer]}),
    )?)
}
fn number(v: &Value) -> f64 {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(0.0)
}
fn images(v: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for field in ["primary_image", "images"] {
        let values = v
            .get(field)
            .map(|v| {
                if let Some(a) = v.as_array() {
                    a.clone()
                } else {
                    vec![v.clone()]
                }
            })
            .unwrap_or_default();
        for image in values {
            if let Some(url) = image.as_str() {
                if (url.starts_with("https://") || url.starts_with("http://"))
                    && !out.iter().any(|s| s == url)
                {
                    out.push(url.to_string());
                }
            }
        }
    }
    out
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transfer {
    id: String,
    source: String,
    target: String,
    source_offer: String,
    offer: String,
    name: String,
    images: Vec<String>,
    price: String,
    currency: String,
    status: String,
    issues: Vec<String>,
    warnings: Vec<String>,
    task_id: Option<i64>,
    target_product_id: Option<i64>,
    updated_at: String,
    payload: Value,
}
fn save(c: &Connection, row: &Transfer) -> Result<(), String> {
    c.execute("INSERT INTO transfers(id,source,target,offer,data) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET data=excluded.data", params![row.id,row.source,row.target,row.offer,serde_json::to_string(row).map_err(|e|e.to_string())?]).map_err(|e|e.to_string())?;
    Ok(())
}
fn load(c: &Connection, id: &str) -> Result<Transfer, String> {
    let text: String = c
        .query_row("SELECT data FROM transfers WHERE id=?1", [id], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}
fn now() -> String {
    chrono::Local::now().to_rfc3339()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcePage {
    items: Vec<Value>,
    last_id: String,
    total: i64,
}
#[tauri::command]
pub async fn transfer_source_page(
    source: String,
    last_id: String,
    state: State<'_, AppState>,
) -> Result<SourcePage, String> {
    let owned = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let c = shop(&owned, &source)?;
        let list = seller_post(
            &c,
            "/v3/product/list",
            &json!({"filter":{"visibility":"ALL"},"last_id":last_id,"limit":100}),
        )?;
        let items = checked_items(&list)?;
        let ids: Vec<Value> = items
            .iter()
            .filter_map(|v| v.get("product_id").cloned())
            .collect();
        let details = if ids.is_empty() {
            vec![]
        } else {
            checked_items(&seller_post(
                &c,
                "/v3/product/info/list",
                &json!({"product_id":ids}),
            )?)?
        };
        if !items.is_empty() && details.len() != items.len() {
            return Err("商品详情返回不完整，请重试当前批次，避免漏读商品".into());
        }
        Ok(SourcePage {
            items: details,
            last_id: list
                .pointer("/result/last_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .into(),
            total: list
                .pointer("/result/total")
                .and_then(Value::as_i64)
                .unwrap_or(0),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftInput {
    source: String,
    target: String,
    offer_id: String,
    prefix: String,
    multiplier: f64,
    currency: String,
}

fn build_payload(
    info: &Value,
    attr: &Value,
    offer: &str,
    multiplier: f64,
    currency: &str,
) -> (Value, Vec<String>) {
    let mut issues = Vec::new();
    let source_currency = info
        .get("currency_code")
        .and_then(Value::as_str)
        .unwrap_or("");
    if source_currency != currency {
        issues.push(format!(
            "源商品币种 {source_currency} 与目标 {currency} 不一致；当前版本不自动换汇"
        ));
    }
    let price = number(&info["price"]) * multiplier;
    if !price.is_finite() || price <= 0.0 || price > 1e12 {
        issues.push("售价无效".into());
    }
    let mut payload = json!({"offer_id":offer,"name":info["name"],"price":format!("{price:.2}"),"currency_code":currency,"vat":"0","images":images(info),"dimension_unit":"mm","weight_unit":"g"});
    // Read attributes endpoint returns physical dimensions in mm and weight in g.
    for key in [
        "description_category_id",
        "type_id",
        "depth",
        "width",
        "height",
        "weight",
        "barcode",
    ] {
        if let Some(value) = attr.get(key).or_else(|| info.get(key)) {
            payload[key] = value.clone();
        }
    }
    payload["attributes"]=Value::Array(attr["attributes"].as_array().into_iter().flatten().map(|a|json!({"id":a["id"],"complex_id":a.get("complex_id").cloned().unwrap_or(json!(0)),"values":a["values"]})).collect());
    for key in ["complex_attributes", "images360", "color_image", "pdf_list"] {
        if let Some(v) = attr.get(key).or_else(|| info.get(key)) {
            payload[key] = v.clone();
        }
    }
    for key in [
        "description_category_id",
        "type_id",
        "depth",
        "width",
        "height",
        "weight",
    ] {
        if number(&payload[key]) <= 0.0 {
            issues.push(format!("缺少有效的 {key}"));
        }
    }
    if info["name"].as_str().unwrap_or("").trim().is_empty() {
        issues.push("缺少商品标题".into());
    }
    if images(info).is_empty() {
        issues.push("缺少可用的图片链接".into());
    }
    if offer.is_empty() || offer.chars().count() > 50 {
        issues.push("目标货号须为 1～50 个字符".into());
    }
    if payload["attributes"]
        .as_array()
        .is_none_or(|a| a.is_empty())
    {
        issues.push("未读取到商品属性".into());
    }
    (payload, issues)
}

#[tauri::command]
pub async fn transfer_prepare(
    input: DraftInput,
    state: State<'_, AppState>,
) -> Result<Transfer, String> {
    let owned = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (s,t)=pair(&owned,&input.source,&input.target)?;
        if !input.multiplier.is_finite() || input.multiplier<=0.0 || !matches!(input.currency.as_str(),"CNY"|"RUB"|"USD"|"EUR") {return Err("价格倍率或币种无效".into());}
        let offer=format!("{}{}",input.prefix,input.offer_id);
        let c=store(&owned)?;
        let existing=c.query_row("SELECT id FROM transfers WHERE source=?1 AND target=?2 AND offer=?3",params![input.source,input.target,offer],|r|r.get::<_,String>(0)).ok();
        if let Some(id)=existing { let old=load(&c,&id)?; if protected(&old.status) {return Ok(old);} }
        let info=array(&seller_post(&s,"/v3/product/info/list",&json!({"offer_id":[input.offer_id]}))?).into_iter().find(|v|v["offer_id"]==input.offer_id).ok_or("源商品不存在或无读取权限")?;
        let attr=array(&seller_post(&s,"/v4/product/info/attributes",&json!({"filter":{"offer_id":[input.offer_id],"visibility":"ALL"},"limit":100}))?).into_iter().find(|v|v["offer_id"]==input.offer_id).ok_or("未读取到源商品完整属性")?;
        let (mut payload,mut issues)=build_payload(&info,&attr,&offer,input.multiplier,&input.currency);
        let description=seller_post(&s,"/v1/product/info/description",&json!({"product_id":info["id"]}))?;
        if let Some(text)=description.pointer("/result/description").and_then(Value::as_str).filter(|s|!s.is_empty()) {
            let attrs=payload["attributes"].as_array_mut().ok_or("属性格式错误")?;
            if !attrs.iter().any(|a|a["id"]==4191) {attrs.push(json!({"id":4191,"complex_id":0,"values":[{"value":text}]}));}
        }
        let target=target_items(&t,&offer)?;
        let duplicate=target.iter().any(|v|v["offer_id"]==offer);
        if duplicate{issues.push("目标店已存在此货号，跳过以避免覆盖".into());}
        if number(&payload["description_category_id"])>0.0 && number(&payload["type_id"])>0.0 {
            let required=seller_post(&t,"/v1/description-category/attribute",&json!({"description_category_id":payload["description_category_id"],"type_id":payload["type_id"],"language":"DEFAULT"}))?;
            for a in checked_items(&required)?.iter().filter(|a|a["is_required"]==true) {
                let id=&a["id"];
                let present=payload["attributes"].as_array().into_iter().flatten().any(|v|&v["id"]==id && v["values"].as_array().is_some_and(|x|!x.is_empty())) || payload["complex_attributes"].as_array().into_iter().flatten().any(|group|group["attributes"].as_array().into_iter().flatten().any(|v|&v["id"]==id && v["values"].as_array().is_some_and(|x|!x.is_empty())));
                if !present{issues.push(format!("目标类目缺少必填属性：{} ({id})",a["name"].as_str().unwrap_or("未知")));}
            }
        }
        c.execute_batch("BEGIN IMMEDIATE").map_err(|e|e.to_string())?;
        let id=c.query_row("SELECT id FROM transfers WHERE source=?1 AND target=?2 AND offer=?3",params![input.source,input.target,offer],|r|r.get::<_,String>(0)).unwrap_or_else(|_|uuid::Uuid::new_v4().to_string());
        if let Ok(old)=load(&c,&id) {if protected(&old.status){c.execute_batch("COMMIT").map_err(|e|e.to_string())?;return Ok(old);}}
        let row=Transfer{id,source:input.source,target:input.target,source_offer:input.offer_id,offer,name:info["name"].as_str().unwrap_or("").into(),images:images(&info),price:payload["price"].as_str().unwrap_or("").into(),currency:input.currency,status:if duplicate{"skipped"}else if issues.is_empty(){"ready"}else{"blocked"}.into(),issues,warnings:vec!["图片使用源商品公开链接；平台会校验抓取结果。视频、变体关系和资质需在目标后台核验；库存不迁移，创建结果以平台审核为准。".into()],task_id:None,target_product_id:None,updated_at:now(),payload};
        save(&c,&row)?;c.execute_batch("COMMIT").map_err(|e|e.to_string())?; Ok(row)
    }).await.map_err(|e|e.to_string())?
}

fn protected(status: &str) -> bool {
    matches!(
        status,
        "submitting" | "uncertain" | "submitted" | "imported" | "moderating" | "found"
    )
}

#[tauri::command]
pub async fn transfer_history(
    source: String,
    target: String,
    state: State<'_, AppState>,
) -> Result<Vec<Transfer>, String> {
    let c = store(&state)?;
    let mut stmt = c
        .prepare("SELECT data FROM transfers WHERE source=?1 AND target=?2 ORDER BY rowid DESC")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![source, target], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    rows.map(|r| serde_json::from_str(&r.map_err(|e| e.to_string())?).map_err(|e| e.to_string()))
        .collect()
}

#[tauri::command]
pub async fn transfer_submit(id: String, state: State<'_, AppState>) -> Result<Transfer, String> {
    let owned = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let c = store(&owned)?;
        // Claim under a transaction. Concurrent clicks cannot submit the same draft twice.
        c.execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| e.to_string())?;
        let mut row = load(&c, &id)?;
        if row.status != "ready" {
            return Err("仅预检通过的草稿可提交；已提交或结果不明确的商品请刷新状态".into());
        }
        let (_, t) = pair(&owned, &row.source, &row.target)?;
        row.status = "submitting".into();
        row.updated_at = now();
        save(&c, &row)?;
        c.execute_batch("COMMIT").map_err(|e| e.to_string())?;
        let result = (|| -> Result<Value, String> {
            let found = target_items(&t, &row.offer)?;
            if found.iter().any(|v| v["offer_id"] == row.offer) {
                return Err("目标店已出现相同货号，未执行写入，请核对目标后台".into());
            }
            seller_post_once(&t, "/v3/product/import", &json!({"items":[row.payload]}))
        })();
        match result {
            Ok(v) => {
                row.task_id = v
                    .pointer("/result/task_id")
                    .map(number)
                    .filter(|n| *n > 0.0)
                    .map(|n| n as i64);
                row.status = if row.task_id.is_some() {
                    "submitted"
                } else {
                    "uncertain"
                }
                .into();
                if row.task_id.is_none() {
                    row.issues
                        .push("响应没有有效 task_id，请在目标后台核对，禁止直接重复提交".into());
                }
            }
            Err(e) => {
                row.status = "uncertain".into();
                row.issues.push(e);
            }
        }
        row.updated_at = now();
        save(&c, &row)?;
        Ok(row)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn transfer_refresh(id: String, state: State<'_, AppState>) -> Result<Transfer, String> {
    let owned = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let c = store(&owned)?;
        let mut row = load(&c, &id)?;
        let t = shop(&owned, &row.target)?;
        if let Some(task) = row.task_id {
            let result = seller_post(&t, "/v1/product/import/info", &json!({"task_id":task}))?;
            let item = array(&result)
                .into_iter()
                .find(|v| v["offer_id"] == row.offer)
                .ok_or("平台尚未返回该商品结果，请稍后刷新")?;
            let remote = item["status"].as_str().unwrap_or("pending");
            row.status = match remote {
                "imported" => "imported",
                "failed" => "failed",
                "moderating" => "moderating",
                _ => "submitted",
            }
            .into();
            row.target_product_id = item["product_id"].as_i64().filter(|id| *id > 0);
            row.issues = item["errors"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|v| v.to_string())
                .collect();
        } else if matches!(row.status.as_str(), "uncertain" | "submitting") {
            let found = target_items(&t, &row.offer)?;
            if let Some(v) = found.iter().find(|v| v["offer_id"] == row.offer) {
                row.status = "found".into();
                row.target_product_id = v["id"].as_i64();
                row.issues =
                    vec!["目标店已找到相同货号；请在后台核对是否为本次创建及审核状态".into()];
            } else {
                row.status = "uncertain".into();
                row.issues = vec![
                    "目标尚未找到此货号，平台可能仍在处理；请稍后刷新或后台核对，系统不会自动重发"
                        .into(),
                ];
            }
        }
        row.updated_at = now();
        save(&c, &row)?;
        Ok(row)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> (Value, Value) {
        (
            json!({"name":"示例","price":"120","currency_code":"CNY","primary_image":["https://x/first.png"],"images":["https://x/second.png","https://x/first.png"]}),
            json!({"description_category_id":1,"type_id":2,"depth":260,"width":170,"height":160,"weight":500,"attributes":[{"id":85,"complex_id":0,"values":[{"value":"品牌"}],"name":"只读字段"}]}),
        )
    }
    #[test]
    fn payload_preserves_images_units_and_attributes_without_readonly_fields() {
        let (i, a) = sample();
        let (p, e) = build_payload(&i, &a, "copy-1", 1.2, "CNY");
        assert!(e.is_empty());
        assert_eq!(p["price"], "144.00");
        assert_eq!(
            p["images"],
            json!(["https://x/first.png", "https://x/second.png"])
        );
        assert_eq!(p["weight_unit"], "g");
        assert!(p["attributes"][0].get("name").is_none());
        assert!(p.get("stock").is_none());
        assert!(p.get("product_id").is_none());
    }
    #[test]
    fn currency_mismatch_missing_dimensions_and_bad_price_block() {
        let (mut i, mut a) = sample();
        i["price"] = json!(0);
        a["depth"] = json!(0);
        let (_, e) = build_payload(&i, &a, "", 1.0, "RUB");
        assert!(e.len() >= 4);
    }
    #[test]
    fn response_shapes_are_understood() {
        for v in [
            json!({"result":[{"id":1}]}),
            json!({"result":{"items":[{"id":1}]}}),
            json!({"items":[{"id":1}]}),
        ] {
            assert_eq!(array(&v).len(), 1);
        }
        assert!(checked_items(&json!({"unexpected":[]})).is_err());
        assert!(checked_items(&json!({"items":null})).is_err());
        assert!(checked_items(&json!({"items":[]})).unwrap().is_empty());
    }
    #[test]
    fn unresolved_and_successful_tasks_cannot_be_reprepared() {
        for status in [
            "submitting",
            "uncertain",
            "submitted",
            "moderating",
            "imported",
            "found",
        ] {
            assert!(protected(status));
        }
        for status in ["ready", "failed", "blocked", "skipped"] {
            assert!(!protected(status));
        }
    }
    #[test]
    fn persistence_keeps_remote_task_and_shop_isolation() {
        let root =
            std::env::temp_dir().join(format!("ozon-transfer-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let state = AppState {
            data_dir: root.clone(),
            active_shop_id: std::sync::Mutex::new("unrelated".into()),
        };
        let (i, a) = sample();
        let (payload, issues) = build_payload(&i, &a, "offer", 1.0, "CNY");
        let row = Transfer {
            id: "row".into(),
            source: "source".into(),
            target: "target".into(),
            source_offer: "original".into(),
            offer: "offer".into(),
            name: "test".into(),
            images: images(&i),
            price: "120.00".into(),
            currency: "CNY".into(),
            status: "submitted".into(),
            issues,
            warnings: vec![],
            task_id: Some(12345),
            target_product_id: None,
            updated_at: now(),
            payload,
        };
        {
            let c = store(&state).unwrap();
            save(&c, &row).unwrap();
        }
        {
            let c = store(&state).unwrap();
            let loaded = load(&c, "row").unwrap();
            assert_eq!(loaded.task_id, Some(12345));
            assert_eq!(loaded.target, "target");
            assert_eq!(loaded.payload, row.payload);
            let mut collision = row.clone();
            collision.id = "another".into();
            assert!(save(&c, &collision).is_err());
            collision.target = "other-target".into();
            assert!(save(&c, &collision).is_ok());
        }
        std::fs::remove_file(root.join("product-transfer.db")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
