use super::{background_state, AppState};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use tauri::State;

fn run(
    c: &Connection,
    command: &str,
    mut payload: Value,
    shop_id: Option<&str>,
) -> Result<Value, String> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS chains(id TEXT PRIMARY KEY, order_no TEXT NOT NULL UNIQUE, body TEXT NOT NULL, revision INTEGER NOT NULL DEFAULT 1);").map_err(|e|e.to_string())?;
    match command {
        "list" => {
            let mut q = c
                .prepare("SELECT body, revision FROM chains ORDER BY rowid DESC")
                .map_err(|e| e.to_string())?;
            let rows = q
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
                .map_err(|e| e.to_string())?;
            let mut result = Vec::new();
            for row in rows {
                let (body, rev) = row.map_err(|e| e.to_string())?;
                let mut v: Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
                v["revision"] = json!(rev);
                result.push(v);
            }
            Ok(json!(result))
        }
        "save" => {
            if let Some(lines) = payload.get_mut("inventoryItems") {
                let lines = lines.as_array_mut().ok_or("库存产品明细格式无效")?;
                for line in lines {
                    let sku = line["sku"].as_str().unwrap_or("").trim();
                    let name = line["productName"].as_str().unwrap_or("").trim();
                    let quantity = line["quantity"]
                        .as_str()
                        .unwrap_or("")
                        .trim()
                        .parse::<i32>()
                        .ok()
                        .filter(|quantity| *quantity > 0);
                    let quantity = quantity.ok_or("每项产品数量必须是正整数")?;
                    if sku.is_empty() {
                        return Err("请填写每项产品的 SKU 或货号".into());
                    }
                    *line = json!({"sku":sku,"productName":name,"quantity":quantity.to_string()});
                }
            }
            let id = payload["id"].as_str().ok_or("缺少记录 ID")?.to_owned();
            let order = payload["orderNo"].as_str().unwrap_or("").trim().to_owned();
            if id.is_empty() || order.is_empty() || order.len() > 200 {
                return Err("请填写采购单号（最多 200 字节）".into());
            }
            let rev = payload["revision"].as_i64().unwrap_or(0);
            let current: Option<(i64, Option<String>)> = c
                .query_row(
                    "SELECT revision, json_extract(body,'$.shopId') FROM chains WHERE id=?1",
                    [&id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(|e| e.to_string())?;
            if current.as_ref().map_or(0, |entry| entry.0) != rev {
                return Err("记录已在其他窗口更新，请刷新后重试".into());
            }
            let owner = current
                .as_ref()
                .and_then(|entry| entry.1.as_deref())
                .or(shop_id);
            if payload.get("inventoryItems").is_some() && owner != shop_id {
                return Err("请在此链路所属店铺编辑并保存".into());
            }
            payload["shopId"] = json!(owner);
            let body = payload.to_string();
            if body.len() > 40 * 1024 * 1024 {
                return Err("单条链路资料总量请控制在 30 MB 内".into());
            }
            let changed=if current.is_some(){c.execute("UPDATE chains SET order_no=?1,body=?2,revision=revision+1 WHERE id=?3 AND revision=?4",params![order,body,id,rev])}else{c.execute("INSERT INTO chains(id,order_no,body) VALUES(?1,?2,?3)",params![id,order,body])}.map_err(|e|format!("保存失败（采购单号不能重复）：{e}"))?;
            if changed != 1 {
                return Err("记录已变化，请刷新".into());
            }
            Ok(json!({"revision":rev+1,"shopId":owner}))
        }
        "delete" => {
            let n = c
                .execute(
                    "DELETE FROM chains WHERE id=?1 AND revision=?2",
                    params![
                        payload["id"].as_str().ok_or("缺少 ID")?,
                        payload["revision"].as_i64().ok_or("缺少版本")?
                    ],
                )
                .map_err(|e| e.to_string())?;
            if n != 1 {
                return Err("记录已变化，请刷新".into());
            }
            Ok(json!(true))
        }
        _ => Err("未知操作".into()),
    }
}

#[tauri::command]
pub async fn procurement_board_command(
    state: State<'_, AppState>,
    command: String,
    payload: Value,
) -> Result<Value, String> {
    let snapshot = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || {
        let c = open(&snapshot)?;
        let mut shop_id = None;
        if command == "save" {
            let order = payload["orderNo"].as_str().unwrap_or("").trim();
            let shop = super::db(&snapshot)?;
            super::purchase_orders::ensure(&shop)?;
            let matches: i64 = shop
                .query_row(
                    "SELECT COUNT(*) FROM purchase_orders WHERE order_no=?1",
                    [order],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;
            if matches == 1 || payload.get("inventoryItems").is_some() {
                shop_id = Some(super::active_shop_identity(&snapshot)?.0);
            }
        }
        run(&c, &command, payload, shop_id.as_deref())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn open(state: &AppState) -> Result<Connection, String> {
    let c = Connection::open(state.data_dir.join("procurement-board.sqlite"))
        .map_err(|e| e.to_string())?;
    c.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| e.to_string())?;
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS chains(
        id TEXT PRIMARY KEY, order_no TEXT NOT NULL UNIQUE,
        body TEXT NOT NULL, revision INTEGER NOT NULL DEFAULT 1
    );",
    )
    .map_err(|e| e.to_string())?;
    Ok(c)
}

const INVENTORY_STAGES: &str = "
    SELECT order_no, CASE
        WHEN json_extract(body,'$.steps[8].status')='已完成' THEN 4
        WHEN json_extract(body,'$.steps[6].status')='已完成' THEN 3
        WHEN json_extract(body,'$.steps[5].status')='已完成' THEN 2
        WHEN json_extract(body,'$.steps[4].status')='已完成'
          OR json_extract(body,'$.steps[3].status')='已完成' THEN 1
        ELSE 0 END AS stage
    , json_extract(body,'$.inventoryItems') AS items
    FROM chains WHERE json_extract(body,'$.shopId')=?1";

pub(super) fn load_inventory_stages(c: &Connection, state: &AppState) -> Result<(), String> {
    super::purchase_orders::ensure(c)?;
    c.execute_batch(
        "CREATE TEMP TABLE procurement_inventory_stages(
        order_no TEXT PRIMARY KEY, stage INTEGER NOT NULL, items TEXT
    );",
    )
    .map_err(|e| e.to_string())?;
    let source = open(state)?;
    let shop_id = super::active_shop_identity(state)?.0;
    let mut stages = source
        .prepare(INVENTORY_STAGES)
        .map_err(|e| e.to_string())?;
    let rows = stages
        .query_map([shop_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut insert = c
        .prepare("INSERT INTO procurement_inventory_stages VALUES (?1,?2,?3)")
        .map_err(|e| e.to_string())?;
    for row in rows {
        let (order, stage, items) = row.map_err(|e| e.to_string())?;
        insert
            .execute(params![order, stage, items])
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persistence_conflicts_and_deletion() {
        let c = Connection::open_in_memory().unwrap();
        let mut p =
            json!({"id":"a","orderNo":"CG-1","revision":0,"docs":[{"stage":2,"data":"abc"}]});
        assert_eq!(
            run(&c, "save", p.clone(), Some("shop")).unwrap()["revision"],
            1
        );
        assert!(run(&c, "save", p.clone(), Some("shop")).is_err());
        p["revision"] = json!(1);
        p["docs"][0]["stage"] = json!(4);
        run(&c, "save", p.clone(), Some("shop")).unwrap();
        let rows = run(&c, "list", Value::Null, Some("shop")).unwrap();
        assert_eq!(rows[0]["docs"][0]["stage"], 4);
        assert_eq!(rows[0]["docs"][0]["data"], "abc");
        assert!(run(&c, "delete", p.clone(), Some("shop")).is_err());
        p["revision"] = json!(2);
        run(&c, "delete", p, Some("shop")).unwrap();
        assert_eq!(
            run(&c, "list", Value::Null, Some("shop")).unwrap(),
            json!([])
        );
    }
    #[test]
    fn rejects_duplicate_order() {
        let c = Connection::open_in_memory().unwrap();
        run(
            &c,
            "save",
            json!({"id":"a","orderNo":"CG","revision":0}),
            Some("shop"),
        )
        .unwrap();
        assert!(run(
            &c,
            "save",
            json!({"id":"b","orderNo":"CG","revision":0}),
            Some("shop")
        )
        .is_err());
    }
}

#[cfg(test)]
mod inventory_stage_tests {
    use super::*;

    #[test]
    fn chain_inventory_items_validate_and_preserve_an_explicit_empty_list() {
        let c = Connection::open_in_memory().unwrap();
        let mut chain = json!({
            "id": "manual", "orderNo": "MANUAL-1", "revision": 0,
            "inventoryItems": [{"sku":" SKU-1 ","productName":"Sample","quantity":"0"}]
        });
        assert!(run(&c, "save", chain.clone(), Some("shop-a")).is_err());
        chain["inventoryItems"][0]["quantity"] = json!(" 12 ");
        run(&c, "save", chain.clone(), Some("shop-a")).unwrap();
        let items = || -> String {
            c.query_row(INVENTORY_STAGES, ["shop-a"], |r| r.get(2))
                .unwrap()
        };
        assert_eq!(
            serde_json::from_str::<Value>(&items()).unwrap()[0]["sku"],
            "SKU-1"
        );
        assert_eq!(
            serde_json::from_str::<Value>(&items()).unwrap()[0]["quantity"],
            "12"
        );
        chain["revision"] = json!(1);
        chain["inventoryItems"] = json!([]);
        assert!(run(&c, "save", chain.clone(), Some("shop-b")).is_err());
        run(&c, "save", chain, Some("shop-a")).unwrap();
        assert_eq!(items(), "[]");
    }

    #[test]
    fn saved_purchase_batches_follow_completed_stages_in_their_own_shop() {
        let c = Connection::open_in_memory().unwrap();
        let mut chain = json!({
            "id": "batch", "orderNo": "PO-1", "revision": 0,
            "steps": vec![json!({"status":"待处理"}); 9]
        });
        let stage = |shop: &str| -> Option<i64> {
            c.query_row(INVENTORY_STAGES, [shop], |r| r.get(1))
                .optional()
                .unwrap()
        };
        assert_eq!(
            run(&c, "save", chain.clone(), None).unwrap()["shopId"],
            Value::Null
        );
        assert_eq!(stage("shop-a"), None);
        chain["revision"] = json!(1);
        run(&c, "save", chain.clone(), Some("shop-a")).unwrap();
        assert_eq!(stage("shop-a"), Some(0));
        for (rev, (step, expected)) in [(3, 1), (4, 1), (5, 2), (6, 3), (7, 3), (8, 4)]
            .into_iter()
            .enumerate()
        {
            chain["revision"] = json!(rev + 2);
            chain["steps"][step]["status"] = json!("已完成");
            run(&c, "save", chain.clone(), Some("shop-b")).unwrap();
            assert_eq!(stage("shop-a"), Some(expected));
            assert_eq!(stage("shop-b"), None);
        }
    }
}
