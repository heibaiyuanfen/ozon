use super::*;
static SWITCH_LOCK: Mutex<()> = Mutex::new(());
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Item {
    sku: String,
    offer_id: String,
    name: String,
    source: i64,
    #[serde(default)]
    source_id: i64,
    #[serde(default)]
    source_quantity: i64,
    target: i64,
    quantity: i64,
    status: String,
    #[serde(default)]
    actual_source: Option<i64>,
    #[serde(default)]
    actual_target: Option<i64>,
    #[serde(default)]
    observed_at: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Plan {
    id: String,
    source_id: i64,
    target_id: i64,
    rows: Vec<Item>,
    status: String,
    #[serde(default)]
    exclude_target_stock: bool,
}
fn has_stock(rows: &[serde_json::Value], warehouse: i64) -> bool {
    rows.iter()
        .filter(|r| id(r, "warehouse_id") == warehouse)
        .any(|r| {
            ["free_stock", "reserved", "present"]
                .iter()
                .any(|k| id(r, k) > 0)
        })
}
fn observe(row: &mut Item, raw: &[serde_json::Value], target: i64) -> Result<(), String> {
    row.actual_source = free(raw, row.source_id)?;
    row.actual_target = free(raw, target)?;
    row.observed_at = chrono::Utc::now().to_rfc3339();
    Ok(())
}
#[tauri::command]
pub(crate) fn warehouse_switch_progress(
    plan_id: String,
    expected_shop_id: String,
    state: State<'_, AppState>,
) -> Result<Plan, String> {
    if *state.active_shop_id.lock().map_err(|e| e.to_string())? != expected_shop_id {
        return Err("店铺已切换".into());
    }
    let c = connection(&state)?;
    let raw: String = c
        .query_row(
            "SELECT payload FROM warehouse_switch_plans WHERE id=?1",
            [plan_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}
#[derive(Deserialize)]
pub(crate) struct Input {
    sku: String,
    quantity: i64,
    source_id: i64,
    source_quantity: i64,
}
fn connection(s: &AppState) -> Result<Connection, String> {
    if active_shop_kind(s)? != "cross_border" {
        return Err("仅跨境店可切换仓库".into());
    }
    let c = db(s)?;
    c.execute_batch("CREATE TABLE IF NOT EXISTS warehouse_switch_plans(id TEXT PRIMARY KEY,payload TEXT NOT NULL,updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP)").map_err(|e|e.to_string())?;
    Ok(c)
}
fn save(c: &Connection, p: &Plan) -> Result<(), String> {
    c.execute("INSERT INTO warehouse_switch_plans(id,payload) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload,updated_at=CURRENT_TIMESTAMP",params![p.id,serde_json::to_string(p).map_err(|e|e.to_string())?]).map_err(|e|e.to_string())?;
    Ok(())
}
fn id(v: &serde_json::Value, k: &str) -> i64 {
    v.get(k)
        .and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()))
        .unwrap_or(0)
}
fn warehouses(c: &Connection) -> Result<Vec<serde_json::Value>, String> {
    let mut rows = vec![];
    let mut cursor = String::new();
    for _ in 0..100 {
        let v = seller_post(
            c,
            "/v2/warehouse/list",
            &serde_json::json!({"limit":100,"cursor":cursor}),
        )?;
        let a = v
            .get("warehouses")
            .and_then(|v| v.as_array())
            .ok_or("仓库列表缺失")?;
        rows.extend(a.iter().map(|w|serde_json::json!({"id":id(w,"warehouse_id"),"name":w["name"],"kind":w["warehouse_type"],"status":w["status"]})));
        if v["has_next"] != true {
            return Ok(rows);
        }
        let next = v["cursor"].as_str().ok_or("仓库游标缺失")?;
        if next == cursor {
            return Err("仓库分页无进展".into());
        }
        cursor = next.into();
    }
    Err("仓库分页超限".into())
}
fn validate_warehouses(w: &[serde_json::Value], source: i64, target: i64) -> Result<(), String> {
    if source <= 0 || target <= 0 || source == target {
        return Err("请选择不同的原仓和目标仓".into());
    }
    for wanted in [source, target] {
        if !w
            .iter()
            .any(|w| id(w, "id") == wanted && w["status"] == "created")
        {
            return Err("所选仓库已停用或不属于当前店铺".into());
        }
        let warehouse = w
            .iter()
            .find(|w| id(w, "id") == wanted)
            .ok_or("仓库不存在")?;
        if !editable(warehouse) {
            return Err("FBP及未知类型仓库不允许修改库存".into());
        }
    }
    Ok(())
}
fn editable(w: &serde_json::Value) -> bool {
    matches!(w["kind"].as_str(), Some("rfbs" | "ogl" | "fbs"))
}
fn stocks(c: &Connection, sku: &str) -> Result<Vec<serde_json::Value>, String> {
    stocks_batch(c, &[sku.to_string()])
}
fn stocks_batch(c: &Connection, skus: &[String]) -> Result<Vec<serde_json::Value>, String> {
    let ids = skus
        .iter()
        .map(|s| s.parse::<i64>().map_err(|_| "SKU无效".to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    if ids.iter().any(|id| *id <= 0) {
        return Err("SKU必须为大于0的有效商品编号，未发送平台请求".into());
    }
    let mut all = vec![];
    let mut cursor = String::new();
    for _ in 0..100 {
        let v = seller_post(
            c,
            "/v2/product/info/stocks-by-warehouse/fbs",
            &serde_json::json!({"sku":ids,"limit":1000,"cursor":cursor}),
        )?;
        let rows = v["products"].as_array().ok_or("分仓库存响应缺失")?;
        all.extend(rows.iter().filter(|r| ids.contains(&id(r, "sku"))).cloned());
        if v["has_next"] != true {
            return Ok(all);
        }
        let next = v["cursor"].as_str().ok_or("库存游标缺失")?;
        if next == cursor {
            return Err("库存分页无进展".into());
        }
        cursor = next.into();
    }
    Err("库存分页超限".into())
}
#[tauri::command]
pub(crate) async fn warehouse_switch_stocks(
    skus: Vec<String>,
    expected_shop_id: String,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let s = background_state(&state)?;
    if *s.active_shop_id.lock().map_err(|e| e.to_string())? != expected_shop_id {
        return Err("店铺已切换".into());
    }
    tauri::async_runtime::spawn_blocking(move||{if skus.is_empty()||skus.len()>100{return Err("每次读取1至100件商品".into());}let c=connection(&s)?;for sku in &skus {if !c.query_row("SELECT EXISTS(SELECT 1 FROM products WHERE sku=?1)",[sku],|r|r.get::<_,bool>(0)).map_err(|e|e.to_string())?{return Err("SKU不属于当前店铺".into());}}
 let raw=stocks_batch(&c,&skus)?;Ok(serde_json::json!({"stocks":raw.iter().map(|r|serde_json::json!({"sku":id(r,"sku").to_string(),"warehouseId":id(r,"warehouse_id"),"name":r["warehouse_name"],"free":r["free_stock"],"reserved":r["reserved"]})).collect::<Vec<_>>() }))}).await.map_err(|e|e.to_string())?
}
fn free(rows: &[serde_json::Value], warehouse: i64) -> Result<Option<i64>, String> {
    let found: Vec<_> = rows
        .iter()
        .filter(|r| id(r, "warehouse_id") == warehouse)
        .collect();
    if found.len() > 1 {
        return Err("同商品同仓返回多条库存，需人工核对".into());
    }
    found
        .first()
        .map(|v| {
            v.get("free_stock")
                .and_then(|v| v.as_i64())
                .filter(|n| *n >= 0)
                .ok_or_else(|| "可售库存字段无效".into())
        })
        .transpose()
}
fn write_stock(c: &Connection, offer: &str, warehouse: i64, quantity: i64) -> Result<(), String> {
    let v = seller_post_once(
        c,
        "/v2/products/stocks",
        &serde_json::json!({"stocks":[{"offer_id":offer,"warehouse_id":warehouse,"stock":quantity}]}),
    )?;
    let rows = v["result"]
        .as_array()
        .ok_or("库存写入缺少逐项回执，结果待核对")?;
    let row = rows
        .iter()
        .find(|r| r["offer_id"] == offer && id(r, "warehouse_id") == warehouse)
        .ok_or("库存写入未返回对应商品仓库回执")?;
    if row["updated"] != true || row["errors"].as_array().is_some_and(|a| !a.is_empty()) {
        return Err(format!("平台未确认更新：{}", row));
    }
    Ok(())
}
#[tauri::command]
pub(crate) async fn warehouse_switch_options(
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let s = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move||{let c=connection(&s)?;let w=warehouses(&c)?;let mut st=c.prepare("SELECT sku,offer_id,name FROM products WHERE sku<>'' AND offer_id<>'' ORDER BY offer_id").map_err(|e|e.to_string())?;let products=st.query_map([],|r|Ok(serde_json::json!({"sku":r.get::<_,String>(0)?,"offerId":r.get::<_,String>(1)?,"name":r.get::<_,String>(2)?}))).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;Ok(serde_json::json!({"warehouses":w,"products":products}))}).await.map_err(|e|e.to_string())?
}
#[tauri::command]
pub(crate) async fn warehouse_switch_preview(
    target_id: i64,
    items: Vec<Input>,
    exclude_target_stock: Option<bool>,
    expected_shop_id: String,
    state: State<'_, AppState>,
) -> Result<Plan, String> {
    let s = background_state(&state)?;
    if *s.active_shop_id.lock().map_err(|e| e.to_string())? != expected_shop_id {
        return Err("店铺已切换".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let c = connection(&s)?;
        if items.is_empty() || items.len() > 5000 {
            return Err("每批请选择1至5000件商品".into());
        }
        let warehouse_list = warehouses(&c)?;
        let mut inventory = Vec::new();
        for batch in items.chunks(100) {
            inventory.extend(stocks_batch(
                &c,
                &batch.iter().map(|i| i.sku.clone()).collect::<Vec<_>>(),
            )?);
        }
        let mut seen = HashSet::new();
        let mut rows = vec![];
        for i in items {
            validate_warehouses(&warehouse_list, i.source_id, target_id)?;
            if i.quantity < 0
                || i.quantity > 1_000_000
                || !(0..=1_000_000).contains(&i.source_quantity)
                || !seen.insert(i.sku.clone())
            {
                return Err("库存须为0至1000000整数，商品不能重复".into());
            }
            let (offer, name) = c
                .query_row(
                    "SELECT offer_id,name FROM products WHERE sku=?1",
                    [&i.sku],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
                )
                .map_err(|_| "商品不属于当前店铺")?;
            let raw = inventory
                .iter()
                .filter(|v| id(v, "sku").to_string() == i.sku)
                .cloned()
                .collect::<Vec<_>>();
            let source =
                free(&raw, i.source_id)?.ok_or("所选商品在原仓无明确库存记录，请核对原仓")?;
            let target = free(&raw, target_id)?.unwrap_or(0);
            rows.push(Item {
                sku: i.sku,
                offer_id: offer,
                name,
                source,
                source_id: i.source_id,
                source_quantity: i.source_quantity,
                target,
                quantity: i.quantity,
                status: if exclude_target_stock.unwrap_or(false) && has_stock(&raw, target_id) {
                    "已跳过：目标仓已有库存"
                } else {
                    "待提交"
                }
                .into(),
                actual_source: Some(source),
                actual_target: Some(target),
                observed_at: chrono::Utc::now().to_rfc3339(),
            });
        }
        let p = Plan {
            id: format!(
                "switch-{}",
                chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
            ),
            source_id: 0,
            target_id,
            rows,
            status: "preview".into(),
            exclude_target_stock: exclude_target_stock.unwrap_or(false),
        };
        save(&c, &p)?;
        Ok(p)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub(crate) async fn warehouse_switch_execute(
    plan_id: String,
    expected_shop_id: String,
    state: State<'_, AppState>,
) -> Result<Plan, String> {
    let s = background_state(&state)?;
    if *s.active_shop_id.lock().map_err(|e| e.to_string())? != expected_shop_id {
        return Err("店铺已切换，未执行".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = SWITCH_LOCK.try_lock().map_err(|_| "已有切仓任务执行中")?;
        let c = connection(&s)?;
        let raw: String = c
            .query_row(
                "SELECT payload FROM warehouse_switch_plans WHERE id=?1",
                [&plan_id],
                |r| r.get(0),
            )
            .map_err(|_| "预览不存在")?;
        let mut p: Plan = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        if p.status != "preview" {
            return Err("该预览已提交，不能重复执行；请查看历史或重新预览核对".into());
        }
        let warehouse_list = warehouses(&c)?;
        for r in &p.rows {
            validate_warehouses(&warehouse_list, r.source_id, p.target_id)?;
        }
        p.status = "running".into();
        save(&c, &p)?;
        for n in 0..p.rows.len() {
            let r = p.rows[n].clone();
            if r.status.starts_with("已跳过") {
                continue;
            }
            let mut skip = false;
            let before = stocks(&c, &r.sku).and_then(|v| {
                observe(&mut p.rows[n], &v, p.target_id)?;
                if p.exclude_target_stock && has_stock(&v, p.target_id) {
                    skip = true;
                    return Ok(());
                }
                if free(&v, r.source_id)? != Some(r.source)
                    || free(&v, p.target_id)?.unwrap_or(0) != r.target
                {
                    return Err("库存已变化，请重新预览".into());
                }
                Ok(())
            });
            if skip {
                p.rows[n].status = "已跳过：目标仓已有库存，未提交".into();
                save(&c, &p)?;
                continue;
            }
            if let Err(e) = before {
                p.rows[n].status = format!("未修改：{e}");
                save(&c, &p)?;
                continue;
            }
            p.rows[n].status = "目标仓提交中，结果待核对".into();
            save(&c, &p)?;
            let result: Result<(), String> = (|| {
                write_stock(&c, &r.offer_id, p.target_id, r.quantity)?;
                let v = stocks(&c, &r.sku)?;
                observe(&mut p.rows[n], &v, p.target_id)?;
                if free(&v, p.target_id)? != Some(r.quantity)
                    || free(&v, r.source_id)? != Some(r.source)
                {
                    return Err("目标仓回读尚未一致或原仓库存已变化，保留原仓；请核对".into());
                }
                Ok(())
            })();
            if let Err(e) = result {
                p.rows[n].status = format!("目标仓未确认，原仓未修改：{e}");
                save(&c, &p)?;
                continue;
            }
            p.rows[n].status = "目标仓已确认，原仓更新中，结果待核对".into();
            save(&c, &p)?;
            let result: Result<(), String> = (|| {
                write_stock(&c, &r.offer_id, r.source_id, r.source_quantity)?;
                let v = stocks(&c, &r.sku)?;
                observe(&mut p.rows[n], &v, p.target_id)?;
                if free(&v, r.source_id)? != Some(r.source_quantity)
                    || free(&v, p.target_id)? != Some(r.quantity)
                {
                    return Err("写入已受理，回读不一致，请核对两仓".into());
                }
                Ok(())
            })();
            p.rows[n].status = match result {
                Ok(()) => "成功：目标库存已设置，原仓已更新".into(),
                Err(e) => format!("部分完成：目标仓已设置；原仓更新待核对：{e}"),
            };
            save(&c, &p)?;
        }
        p.status = "finished".into();
        save(&c, &p)?;
        Ok(p)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub(crate) fn warehouse_switch_history(state: State<'_, AppState>) -> Result<Vec<Plan>, String> {
    let c = connection(&state)?;
    let mut st = c
        .prepare("SELECT payload FROM warehouse_switch_plans ORDER BY updated_at DESC LIMIT 20")
        .map_err(|e| e.to_string())?;
    let raw = st
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    raw.iter()
        .map(|r| serde_json::from_str(r).map_err(|e| e.to_string()))
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exclude_target_includes_reserved_but_not_other_warehouses() {
        let r = vec![
            serde_json::json!({"warehouse_id":1,"free_stock":100,"reserved":0}),
            serde_json::json!({"warehouse_id":2,"free_stock":0,"reserved":2}),
        ];
        assert!(has_stock(&r, 2));
        assert!(!has_stock(&r, 3));
        assert!(!has_stock(
            &[serde_json::json!({"warehouse_id":2,"free_stock":0,"reserved":0,"present":0})],
            2
        ));
    }
    #[test]
    fn progress_uses_observed_stock_instead_of_requested_quantities() {
        let mut row:Item=serde_json::from_value(serde_json::json!({"sku":"12","offerId":"a","name":"test","source":50,"sourceId":1,"sourceQuantity":0,"target":0,"quantity":1000,"status":"目标仓提交中"})).unwrap();
        observe(
            &mut row,
            &[
                serde_json::json!({"warehouse_id":1,"free_stock":50}),
                serde_json::json!({"warehouse_id":2,"free_stock":23}),
            ],
            2,
        )
        .unwrap();
        assert_eq!(row.actual_source, Some(50));
        assert_eq!(row.actual_target, Some(23));
        assert_eq!(row.quantity, 1000);
        assert!(!row.observed_at.is_empty());
    }
    #[test]
    fn invalid_sku_is_rejected_before_network_access() {
        let c = Connection::open_in_memory().unwrap();
        for sku in ["0", "-1", "not-a-sku"] {
            let error = stocks_batch(&c, &[sku.to_string()]).unwrap_err();
            assert!(error.contains("SKU"));
            assert!(!error.contains("API"));
        }
    }
    #[test]
    fn fbp_cannot_be_source_or_target_even_when_active() {
        let w = vec![
            serde_json::json!({"id":1,"kind":"fbp","status":"created"}),
            serde_json::json!({"id":2,"kind":"rfbs","status":"created"}),
            serde_json::json!({"id":3,"kind":"ogl","status":"created"}),
        ];
        assert!(validate_warehouses(&w, 1, 3).is_err());
        assert!(validate_warehouses(&w, 2, 1).is_err());
        assert!(validate_warehouses(&w, 2, 3).is_ok());
        assert!(validate_warehouses(&w, 2, 2).is_err());
    }
    #[test]
    fn free_stock_is_not_reserved_or_present() {
        let r =
            vec![serde_json::json!({"warehouse_id":1,"free_stock":7,"present":10,"reserved":3})];
        assert_eq!(free(&r, 1).unwrap(), Some(7));
        assert_eq!(free(&r, 2).unwrap(), None);
        assert!(free(&[r[0].clone(), r[0].clone()], 1).is_err());
        assert!(free(&[serde_json::json!({"warehouse_id":1,"present":5})], 1).is_err());
    }
}
