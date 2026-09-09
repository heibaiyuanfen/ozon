use crate::{wb_shop_center, AppState};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use tauri::State;

#[path = "product_master_extra.rs"]
mod extra;
#[path = "product_import.rs"]
mod import;

type Result<T> = std::result::Result<T, String>;
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn uid(c: &Connection) -> Result<String> {
    c.query_row("SELECT lower(hex(randomblob(16)))", [], |r| r.get(0))
        .map_err(err)
}
fn txt(p: &Value, k: &str) -> String {
    p.get(k).and_then(Value::as_str).unwrap_or("").trim().into()
}
pub(crate) fn ensure(c: &Connection) -> Result<()> {
    let tx = rusqlite::Transaction::new_unchecked(c, rusqlite::TransactionBehavior::Immediate)
        .map_err(err)?;
    tx.execute_batch(include_str!("product_master_schema.sql"))
        .map_err(err)?;
    extra::schema(&tx)?;
    tx.commit().map_err(err)
}
fn shop(c: &Connection, id: &str) -> Result<()> {
    let exists:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM wb_shops WHERE id=?1 AND organization_id='local' AND status!='disabled')",[id],|r|r.get(0)).map_err(err)?;
    if exists {
        Ok(())
    } else {
        Err("店铺不存在或已停用".into())
    }
}
fn event(
    c: &Connection,
    id: &str,
    kind: &str,
    before: Value,
    after: Value,
    reason: &str,
) -> Result<()> {
    c.execute("INSERT INTO pm_history(entity_id,event_type,before_json,after_json,actor,reason)VALUES(?1,?2,?3,?4,'local-desktop',?5)",params![id,kind,before.to_string(),after.to_string(),reason]).map_err(err)?;
    Ok(())
}
fn records(c: &Connection, sql: &str, args: impl rusqlite::Params) -> Result<Vec<Value>> {
    let mut s = c.prepare(sql).map_err(err)?;
    let rows = s
        .query_map(args, |r| {
            let text: String = r.get(0)?;
            Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
        })
        .map_err(err)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(err)?;
    Ok(rows)
}

/// External identifiers are scoped to a shop. A card id can cover several sizes.
pub(crate) fn resolve(c: &Connection, shop_id: &str, p: &Value) -> Result<Value> {
    shop(c, shop_id)?;
    let identifier = |lower: &str, upper: &str| -> Result<String> {
        let v = p.get(lower).or_else(|| p.get(upper));
        match v {
            None | Some(Value::Null) => Ok(String::new()),
            Some(Value::String(s)) if s.trim().is_empty() => Ok(String::new()),
            Some(v) => {
                let s = v
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| v.to_string());
                s.parse::<u64>()
                    .ok()
                    .filter(|n| *n > 0)
                    .map(|n| n.to_string())
                    .ok_or_else(|| format!("INVALID_EXTERNAL_ID: {upper}"))
            }
        }
    };
    let nm = identifier("nmId", "nmID")?;
    let chrt = identifier("chrtId", "chrtID")?;
    let barcode = txt(p, "barcode");
    let vendor = txt(p, "vendorCode");
    let precise = !chrt.is_empty() || !barcode.is_empty();
    let mut candidates = BTreeSet::new();
    let mut listing_ids = BTreeSet::new();
    let mut matched_by = "";
    let mut ambiguous = false;
    for (method, value, condition) in [
        ("chrtID", &chrt, "l.chrt_id=?2"),
        (
            "barcode",
            &barcode,
            "EXISTS(SELECT 1 FROM pm_barcodes b WHERE b.listing_id=l.id AND b.barcode=?2)",
        ),
        ("vendorCode", &vendor, "l.vendor_code=?2"),
        ("nmID", &nm, "l.nm_id=?2"),
    ] {
        if value.is_empty() || precise && ["vendorCode", "nmID"].contains(&method) {
            continue;
        }
        let sql=format!("SELECT json_object('id',l.id,'sku',l.sku_id,'status',l.mapping_status,'nmId',l.nm_id,'chrtId',l.chrt_id) FROM pm_listings l WHERE l.shop_id=?1 AND l.mapping_status!='IGNORED' AND {condition}");
        let rows = records(c, &sql, params![shop_id, value])?;
        if rows.len() > 1 && rows.iter().any(|r| r["sku"].is_null()) {
            ambiguous = true;
        }
        for row in rows {
            if !chrt.is_empty() && row["chrtId"] != chrt || !nm.is_empty() && row["nmId"] != nm {
                ambiguous = true;
            }
            if row["status"] == "CONFLICT" {
                ambiguous = true
            }
            if let Some(sku) = row["sku"].as_str() {
                candidates.insert(sku.to_string());
                listing_ids.insert(row["id"].as_str().unwrap_or("").to_string());
                if matched_by.is_empty() {
                    matched_by = method
                }
            }
        }
    }
    if ambiguous || candidates.len() > 1 {
        return Ok(
            json!({"status":"conflict","skuId":null,"listingId":null,"candidates":candidates,"confidence":"low"}),
        );
    }
    if let Some(sku) = candidates.first() {
        return Ok(
            json!({"status":"resolved","skuId":sku,"listingId":if listing_ids.len()==1{listing_ids.first()}else{None},"method":matched_by,"confidence":if matched_by=="vendorCode"{"high"}else{"exact"}}),
        );
    }
    Ok(json!({"status":"unresolved","skuId":null,"listingId":null,"confidence":"low"}))
}

fn create_sku(c: &Connection, p: &Value) -> Result<String> {
    extra::validate(p)?;
    let code = txt(p, "code");
    if code.is_empty() {
        return Err("内部 SKU 编码不能为空".into());
    }
    let cost = txt(p, "purchaseCost");
    if !cost.is_empty()
        && !regex::Regex::new(r"^\d{1,12}(\.\d{1,4})?$")
            .unwrap()
            .is_match(&cost)
    {
        return Err("采购成本必须为最多四位小数的非负金额".into());
    }
    let id = uid(c)?;
    let mut spu = txt(p, "spuId");
    if !txt(p, "newSpuCode").is_empty() {
        spu = uid(c)?;
        c.execute(
            "INSERT INTO pm_spus(id,code,name)VALUES(?1,?2,?3)",
            params![spu, txt(p, "newSpuCode"), txt(p, "newSpuName")],
        )
        .map_err(err)?;
        event(
            c,
            &spu,
            "CREATE_SPU",
            Value::Null,
            json!({"code":txt(p,"newSpuCode"),"name":txt(p,"newSpuName")}),
            "创建 SKU 时建立系列",
        )?;
    }
    if !spu.is_empty() {
        let valid:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM pm_spus WHERE id=?1 AND organization_id='local' AND status!='archived')",[&spu],|r|r.get(0)).map_err(err)?;
        if !valid {
            return Err("系列不存在或已归档".into());
        }
    }

    c.execute("INSERT INTO pm_skus(id,code,spu_id,name,brand,color,size,category,supplier,attributes_json,media_json,purchase_cost)VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",params![id,code,if spu.is_empty(){None}else{Some(spu)},txt(p,"name"),txt(p,"brand"),txt(p,"color"),txt(p,"size"),txt(p,"category"),txt(p,"supplier"),p.get("attributes").unwrap_or(&json!({})).to_string(),p.get("media").unwrap_or(&json!([])).to_string(),if cost.is_empty(){None}else{Some(cost)}]).map_err(err)?;
    extra::metadata(c, &id, p)?;
    let status = txt(p, "status");
    if !status.is_empty() {
        if !["active", "draft", "inactive", "archived"].contains(&status.as_str()) {
            return Err("商品状态无效".into());
        }
        c.execute(
            "UPDATE pm_skus SET status=?2 WHERE id=?1",
            params![id, status],
        )
        .map_err(err)?;
    }
    event(c, &id, "CREATE_SKU", Value::Null, p.clone(), "")?;
    Ok(id)
}
fn bind(c: &Connection, listing: &str, sku: &str, reason: &str) -> Result<()> {
    let old: Option<String> = c
        .query_row(
            "SELECT sku_id FROM pm_listings WHERE id=?1",
            [listing],
            |r| r.get(0),
        )
        .map_err(err)?;
    if old.as_deref() != Some(sku) && old.is_some() && reason.trim().is_empty() {
        return Err("重新绑定必须填写原因".into());
    }
    let valid:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM pm_skus WHERE id=?1 AND organization_id='local' AND status!='archived')",[sku],|r|r.get(0)).map_err(err)?;
    if !extra::conflicts(c, listing, sku)?.is_empty() && reason.trim().is_empty() {
        return Err("存在标识冲突，请核对预览并填写解决原因".into());
    }
    if !valid {
        return Err("SKU 不存在或已归档".into());
    }
    c.execute(
        "UPDATE pm_listings SET sku_id=?2,mapping_status='MAPPED',confidence='exact' WHERE id=?1",
        params![listing, sku],
    )
    .map_err(err)?;
    event(
        c,
        listing,
        if old.is_some() {
            "REMAP_LISTING"
        } else {
            "BIND_LISTING"
        },
        json!(old),
        json!(sku),
        reason,
    )?;
    let shop_id: String = c
        .query_row(
            "SELECT shop_id FROM pm_listings WHERE id=?1",
            [listing],
            |r| r.get(0),
        )
        .map_err(err)?;
    extra::reconcile(c, &shop_id, listing)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("PRAGMA foreign_keys=ON;CREATE TABLE wb_shops(id TEXT PRIMARY KEY,organization_id TEXT,status TEXT,name TEXT);INSERT INTO wb_shops VALUES('a','local','active','Shop A'),('b','local','active','Shop B');").unwrap();
        ensure(&c).unwrap();
        c.execute(
            "INSERT INTO pm_raw(shop_id,run_id,payload)VALUES('a','test','{}')",
            [],
        )
        .unwrap();
        c
    }
    fn card(n: u64, ch: u64, bar: &str) -> Value {
        json!({"nmID":n,"title":"Tool bag","vendorCode":"TOOL","sizes":[{"chrtID":ch,"skus":[bar],"techSize":"M"}]})
    }
    #[test]
    fn repeated_sync_is_idempotent_and_never_creates_skus() {
        let c = fixture();
        normalize(&c, "a", 1, &card(1, 10, "B1")).unwrap();
        normalize(&c, "a", 1, &card(1, 10, "B1")).unwrap();
        assert_eq!(
            c.query_row("SELECT COUNT(*) FROM pm_listings", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            c.query_row("SELECT COUNT(*) FROM pm_skus", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn one_sku_can_map_two_shops_but_resolver_is_shop_scoped() {
        let c = fixture();
        let sku = create_sku(&c, &json!({"code":"TOOL-YELLOW","name":"Yellow"})).unwrap();
        normalize(&c, "a", 1, &card(1, 10, "B1")).unwrap();
        c.execute(
            "INSERT INTO pm_raw(shop_id,run_id,payload)VALUES('b','test','{}')",
            [],
        )
        .unwrap();
        normalize(&c, "b", 2, &card(2, 20, "B2")).unwrap();
        let listings = records(&c, "SELECT json_quote(id) FROM pm_listings", []).unwrap();
        for l in listings {
            bind(&c, l.as_str().unwrap(), &sku, "").unwrap();
        }
        assert_eq!(
            resolve(&c, "a", &json!({"chrtId":"10"})).unwrap()["skuId"],
            sku
        );
        assert_eq!(
            resolve(&c, "b", &json!({"chrtId":"10"})).unwrap()["status"],
            "unresolved"
        );
    }
    #[test]
    fn sizes_are_not_merged_by_card_or_vendor() {
        let c = fixture();
        normalize(&c, "a", 1, &card(1, 10, "B1")).unwrap();
        normalize(&c, "a", 1, &card(1, 11, "B2")).unwrap();
        let id: String = c
            .query_row("SELECT id FROM pm_listings WHERE chrt_id='10'", [], |r| {
                r.get(0)
            })
            .unwrap();
        let sku = create_sku(&c, &json!({"code":"SKU-1"})).unwrap();
        bind(&c, &id, &sku, "").unwrap();
        assert_eq!(
            resolve(
                &c,
                "a",
                &json!({"nmId":"1","chrtId":"11","vendorCode":"TOOL"})
            )
            .unwrap()["status"],
            "unresolved"
        );
    }
    #[test]
    fn duplicate_barcode_is_conflict() {
        let c = fixture();
        normalize(&c, "a", 1, &card(1, 10, "B1")).unwrap();
        normalize(&c, "a", 1, &card(2, 20, "B1")).unwrap();
        let status: String = c
            .query_row(
                "SELECT mapping_status FROM pm_listings WHERE chrt_id='20'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, "CONFLICT");
    }
    #[test]
    fn sku_code_unique_and_cost_exact() {
        let c = fixture();
        let id = create_sku(&c, &json!({"code":"SKU","purchaseCost":"12.3401"})).unwrap();
        assert!(create_sku(&c, &json!({"code":"SKU"})).is_err());
        assert_eq!(
            c.query_row("SELECT purchase_cost FROM pm_skus WHERE id=?1", [id], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "12.3401"
        );
    }
    #[test]
    fn remap_requires_reason_and_records_history() {
        let c = fixture();
        normalize(&c, "a", 1, &card(1, 10, "B1")).unwrap();
        let id: String = c
            .query_row("SELECT id FROM pm_listings", [], |r| r.get(0))
            .unwrap();
        let a = create_sku(&c, &json!({"code":"A"})).unwrap();
        let b = create_sku(&c, &json!({"code":"B"})).unwrap();
        bind(&c, &id, &a, "").unwrap();
        assert!(bind(&c, &id, &b, "").is_err());
        bind(&c, &id, &b, "纠正尺寸").unwrap();
        assert_eq!(
            c.query_row(
                "SELECT COUNT(*) FROM pm_history WHERE event_type='REMAP_LISTING'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
    }
    #[test]
    fn malformed_ids_rejected() {
        let c = fixture();
        assert!(normalize(&c, "a", 1, &json!({"nmID":null})).is_err());
    }
    #[test]
    fn raw_cannot_cross_shops() {
        let c = fixture();
        assert!(normalize(&c, "b", 1, &card(1, 10, "B")).is_err());
    }
    #[test]
    fn exact_vendor_match_and_listing_id() {
        let c = fixture();
        let id = create_sku(&c, &json!({"code":"TOOL"})).unwrap();
        normalize(&c, "a", 1, &card(1, 10, "B")).unwrap();
        let r = resolve(&c, "a", &json!({"chrtId":"10"})).unwrap();
        assert_eq!(r["skuId"], id);
        assert!(r["listingId"].is_string());
    }
    #[test]
    fn duplicate_conflict_is_symmetric_and_resolver_reports_it() {
        let c = fixture();
        normalize(&c, "a", 1, &card(1, 10, "B")).unwrap();
        normalize(&c, "a", 1, &card(2, 20, "B")).unwrap();
        let n: i64 = c
            .query_row(
                "SELECT count(*) FROM pm_listings WHERE mapping_status='CONFLICT'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 2);
        assert_eq!(
            resolve(&c, "a", &json!({"barcode":"B"})).unwrap()["status"],
            "conflict"
        );
    }
    #[test]
    fn removing_duplicate_restores_mapping_state() {
        let c = fixture();
        normalize(&c, "a", 1, &card(1, 10, "B")).unwrap();
        normalize(&c, "a", 1, &card(2, 20, "B")).unwrap();
        normalize(&c, "a", 1, &card(2, 20, "C")).unwrap();
        let n: i64 = c
            .query_row(
                "SELECT count(*) FROM pm_listings WHERE mapping_status='CONFLICT'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 0);
    }
    #[test]
    fn fields_and_missing_quality_are_preserved() {
        let c = fixture();
        let mut a = card(1, 10, "B");
        normalize(&c, "a", 1, &a).unwrap();
        a["title"] = json!("Updated");
        normalize(&c, "a", 1, &a).unwrap();
        let n: i64 = c
            .query_row(
                "SELECT count(*) FROM pm_history WHERE event_type='TITLE_CHANGED'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
        let missing: String = c
            .query_row("SELECT missing_fields FROM pm_listings", [], |r| r.get(0))
            .unwrap();
        assert!(missing.contains("weightKg"));
    }
    #[test]
    fn role_boundaries() {
        assert!(extra::allowed("viewer", "list_v2"));
        for a in ["create_sku", "import_commit", "bind", "update_sku"] {
            assert!(!extra::allowed("viewer", a));
        }
        assert!(extra::allowed("operator", "bind"));
        assert!(!extra::allowed("operator", "create_sku"));
        assert!(!extra::allowed("operator", "bulk_update"));
        assert!(!extra::allowed("invalid", "detail"));
    }
    #[test]
    fn search_external_ids_and_pagination() {
        let c = fixture();
        let id = create_sku(&c, &json!({"code":"TOOL"})).unwrap();
        normalize(&c, "a", 1, &card(123, 456, "BARCODE")).unwrap();
        for query in ["123", "456", "BARCODE", "TOOL"] {
            let r = extra::list(&c, &json!({"kind":"skus","query":query})).unwrap();
            assert_eq!(r["rows"][0]["id"], id);
        }
        for i in 0..55 {
            create_sku(&c, &json!({"code":format!("X{i}")})).unwrap();
        }
        assert_eq!(
            extra::list(&c, &json!({"kind":"skus"})).unwrap()["rows"]
                .as_array()
                .unwrap()
                .len(),
            50
        );
        assert_eq!(
            extra::list(&c, &json!({"kind":"skus","page":1})).unwrap()["rows"]
                .as_array()
                .unwrap()
                .len(),
            6
        );
    }
    #[test]
    fn reprocess_history_after_mapping() {
        let c = fixture();
        normalize(&c, "a", 1, &card(1, 10, "B")).unwrap();
        let p = json!({"shopId":"a","source":"orders","externalKey":"order-1","identifiers":{"chrtId":"10"}});
        assert_eq!(
            extra::handle(&c, "remember_reference", &p, "admin").unwrap()["status"],
            "unresolved"
        );
        let sku = create_sku(&c, &json!({"code":"SKU"})).unwrap();
        let id: String = c
            .query_row("SELECT id FROM pm_listings", [], |r| r.get(0))
            .unwrap();
        bind(&c, &id, &sku, "").unwrap();
        assert_eq!(
            extra::handle(&c, "reprocess", &json!({}), "admin").unwrap()["resolved"],
            1
        );
    }
    #[test]
    fn import_preview_atomic_commit_and_replay() {
        use base64::Engine;
        let c = fixture();
        let data = base64::engine::general_purpose::STANDARD.encode(
            "internal_sku_code,name_cn,default_purchase_cost\nA,Yellow,12.3401\nB,Blue,5.20\n",
        );
        let preview = import::handle(
            &c,
            "import_preview",
            &json!({"kind":"products","filename":"test.csv","base64":data}),
        )
        .unwrap();
        assert_eq!(preview["valid"], 2);
        assert_eq!(
            c.query_row("SELECT count(*) FROM pm_skus", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        import::handle(&c, "import_commit", &preview).unwrap();
        assert!(import::handle(&c, "import_commit", &preview).is_err());
        assert_eq!(
            c.query_row("SELECT count(*) FROM pm_skus", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
    }
    #[test]
    fn invalid_import_rolls_back_entire_batch() {
        use base64::Engine;
        let c = fixture();
        let data = base64::engine::general_purpose::STANDARD
            .encode("internal_sku_code,default_purchase_cost\nA,12.34\nB,not-money\n");
        let preview = import::handle(
            &c,
            "import_preview",
            &json!({"kind":"products","filename":"test.csv","base64":data}),
        )
        .unwrap();
        assert_eq!(preview["valid"], 1);
        assert!(import::handle(&c, "import_commit", &preview).is_err());
        assert_eq!(
            c.query_row("SELECT count(*) FROM pm_skus", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn archived_sku_keeps_listing_and_cannot_be_rebound() {
        let c = fixture();
        let id = create_sku(&c, &json!({"code":"TOOL"})).unwrap();
        normalize(&c, "a", 1, &card(1, 10, "B")).unwrap();
        extra::handle(
            &c,
            "update_sku",
            &json!({"id":id,"code":"TOOL","name":"Tool","status":"archived"}),
            "admin",
        )
        .unwrap();
        let listing: String = c
            .query_row("SELECT id FROM pm_listings", [], |r| r.get(0))
            .unwrap();
        assert!(bind(&c, &listing, &id, "test").is_err());
        assert_eq!(
            resolve(&c, "a", &json!({"chrtId":"10"})).unwrap()["skuId"],
            id
        );
    }
    #[test]
    fn spu_create_update_detail_and_archive() {
        let c = fixture();
        let r = dispatch(
            &c,
            "create_spu",
            json!({"code":"SERIES","name":"Tool bag"}),
            "admin",
        )
        .unwrap();
        dispatch(
            &c,
            "update_spu",
            json!({"id":r["id"],"code":"SERIES","name":"工具包","status":"archived"}),
            "admin",
        )
        .unwrap();
        assert_eq!(
            dispatch(
                &c,
                "detail_v2",
                json!({"kind":"spus","id":r["id"]}),
                "viewer"
            )
            .unwrap()["item"]["status"],
            "archived"
        );
    }
    #[test]
    fn create_sku_with_new_spu_is_atomic() {
        let c = fixture();
        dispatch(
            &c,
            "create_sku",
            json!({"code":"X","newSpuCode":"SERIES","newSpuName":"Series"}),
            "admin",
        )
        .unwrap();
        assert!(dispatch(
            &c,
            "create_sku",
            json!({"code":"X","newSpuCode":"SHOULD-ROLLBACK"}),
            "admin"
        )
        .is_err());
        assert_eq!(
            c.query_row("SELECT count(*) FROM pm_spus", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
    #[test]
    fn create_from_listing_rolls_back_when_listing_invalid() {
        let c = fixture();
        assert!(dispatch(
            &c,
            "create_sku",
            json!({"code":"X","listingId":"missing"}),
            "admin"
        )
        .is_err());
        assert_eq!(
            c.query_row("SELECT count(*) FROM pm_skus", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn create_from_listing_binds_and_prefilled_fields_preserved() {
        let c = fixture();
        normalize(&c, "a", 1, &card(1, 10, "B")).unwrap();
        let id: String = c
            .query_row("SELECT id FROM pm_listings", [], |r| r.get(0))
            .unwrap();
        let s = dispatch(
            &c,
            "create_sku",
            json!({"code":"X","listingId":id,"widthCm":"12.5","nameRu":"Сумка"}),
            "admin",
        )
        .unwrap();
        assert_eq!(
            resolve(&c, "a", &json!({"chrtId":"10"})).unwrap()["skuId"],
            s["id"]
        );
        let detail = extra::detail(&c, s["id"].as_str().unwrap(), "skus").unwrap();
        assert_eq!(detail["item"]["metadata"]["widthCm"], "12.5");
    }
    #[test]
    fn dispatcher_rejects_client_role_spoofing() {
        let c = fixture();
        assert!(dispatch(
            &c,
            "create_sku",
            json!({"code":"X","actorRole":"admin"}),
            "viewer"
        )
        .is_err());
        assert!(dispatch(&c, "create_sku", json!({"code":"X"}), "viewer").is_err());
        assert!(dispatch(&c, "create_sku", json!({"code":"X"}), "operator").is_err());
    }
    #[test]
    fn operator_cannot_change_cost() {
        let c = fixture();
        let id = create_sku(&c, &json!({"code":"X","purchaseCost":"12.00"})).unwrap();
        assert!(dispatch(
            &c,
            "update_sku",
            json!({"id":id,"code":"X","name":"X","status":"active","purchaseCost":"20.00"}),
            "operator"
        )
        .is_err());
    }
    #[test]
    fn ignore_survives_sync_and_is_audited() {
        let c = fixture();
        normalize(&c, "a", 1, &card(1, 10, "B")).unwrap();
        let id: String = c
            .query_row("SELECT id FROM pm_listings", [], |r| r.get(0))
            .unwrap();
        dispatch(
            &c,
            "ignore",
            json!({"listingId":id,"reason":"不纳入运营"}),
            "admin",
        )
        .unwrap();
        normalize(&c, "a", 1, &card(1, 10, "B")).unwrap();
        assert_eq!(
            extra::detail(&c, &id, "listings").unwrap()["item"]["mapping_status"],
            "IGNORED"
        );
        assert_eq!(
            c.query_row(
                "SELECT count(*) FROM pm_history WHERE event_type='IGNORE_LISTING'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
    }
    #[test]
    fn unbind_keeps_history() {
        let c = fixture();
        create_sku(&c, &json!({"code":"TOOL"})).unwrap();
        normalize(&c, "a", 1, &card(1, 10, "B")).unwrap();
        let id: String = c
            .query_row("SELECT id FROM pm_listings", [], |r| r.get(0))
            .unwrap();
        dispatch(
            &c,
            "unbind",
            json!({"listingId":id,"reason":"纠正映射"}),
            "admin",
        )
        .unwrap();
        assert_eq!(
            resolve(&c, "a", &json!({"chrtId":"10"})).unwrap()["status"],
            "unresolved"
        );
    }
    #[test]
    fn multi_size_nmid_cannot_resolve_with_only_one_mapped() {
        let c = fixture();
        normalize(&c, "a", 1, &card(1, 10, "B")).unwrap();
        normalize(&c, "a", 1, &card(1, 11, "C")).unwrap();
        let id: String = c
            .query_row("SELECT id FROM pm_listings WHERE chrt_id='10'", [], |r| {
                r.get(0)
            })
            .unwrap();
        let sku = create_sku(&c, &json!({"code":"X"})).unwrap();
        bind(&c, &id, &sku, "").unwrap();
        assert_eq!(
            resolve(&c, "a", &json!({"nmId":"1"})).unwrap()["status"],
            "conflict"
        );
    }
    #[test]
    fn contradictory_identifiers_are_conflict() {
        let c = fixture();
        create_sku(&c, &json!({"code":"TOOL"})).unwrap();
        normalize(&c, "a", 1, &card(1, 10, "B")).unwrap();
        assert_eq!(
            resolve(&c, "a", &json!({"chrtId":"10","nmId":"999"})).unwrap()["status"],
            "conflict"
        );
    }
    #[test]
    fn platform_sync_does_not_overwrite_erp_fields() {
        let c = fixture();
        let id=create_sku(&c,&json!({"code":"TOOL","name":"ERP title","brand":"ERP brand","purchaseCost":"33.3300","supplier":"Supplier"})).unwrap();
        let mut card = card(1, 10, "B");
        card["brand"] = json!("WB brand");
        normalize(&c, "a", 1, &card).unwrap();
        let d = extra::detail(&c, &id, "skus").unwrap();
        assert_eq!(d["item"]["name"], "ERP title");
        assert_eq!(d["item"]["purchase_cost"], "33.3300");
        assert_eq!(d["item"]["supplier"], "Supplier");
    }
    #[test]
    fn trash_changes_external_status_without_losing_mapping() {
        let c = fixture();
        let id = create_sku(&c, &json!({"code":"TOOL"})).unwrap();
        normalize(&c, "a", 1, &card(1, 10, "B")).unwrap();
        normalize_status(&c, "a", 1, &card(1, 10, "B"), "inactive").unwrap();
        assert_eq!(
            c.query_row("SELECT listing_status FROM pm_listings", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "inactive"
        );
        assert_eq!(
            resolve(&c, "a", &json!({"chrtId":"10"})).unwrap()["skuId"],
            id
        );
        assert_eq!(
            c.query_row(
                "SELECT count(*) FROM pm_history WHERE event_type='EXTERNAL_STATUS_CHANGED'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
    }
    #[test]
    fn bulk_update_rolls_back_invalid_ids() {
        let c = fixture();
        let id = create_sku(&c, &json!({"code":"X"})).unwrap();
        assert!(dispatch(
            &c,
            "bulk_update",
            json!({"ids":[id,"missing"],"field":"status","value":"archived"}),
            "admin"
        )
        .is_err());
        assert_eq!(
            c.query_row("SELECT status FROM pm_skus", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "active"
        );
    }
    #[test]
    fn batch_mapping_preview_commit_and_shop_isolation() {
        use base64::Engine;
        let c = fixture();
        normalize(&c, "a", 1, &card(1, 10, "B")).unwrap();
        let sku = create_sku(&c, &json!({"code":"SKU"})).unwrap();
        let data = base64::engine::general_purpose::STANDARD
            .encode("shop,nmID,chrtID,internalSkuCode\nShop A,1,10,SKU\n");
        let p = import::handle(
            &c,
            "import_preview",
            &json!({"kind":"mapping","filename":"map.csv","base64":data}),
        )
        .unwrap();
        assert_eq!(p["valid"], 1);
        assert_eq!(
            resolve(&c, "a", &json!({"chrtId":"10"})).unwrap()["status"],
            "unresolved"
        );
        import::handle(&c, "import_commit", &p).unwrap();
        assert_eq!(
            resolve(&c, "a", &json!({"chrtId":"10"})).unwrap()["skuId"],
            sku
        );
        assert_eq!(
            resolve(&c, "b", &json!({"chrtId":"10"})).unwrap()["status"],
            "unresolved"
        );
    }
    #[test]
    fn batch_mapping_ambiguous_variants_are_rejected() {
        use base64::Engine;
        let c = fixture();
        normalize(&c, "a", 1, &card(1, 10, "B")).unwrap();
        normalize(&c, "a", 1, &card(1, 11, "C")).unwrap();
        create_sku(&c, &json!({"code":"SKU"})).unwrap();
        let data = base64::engine::general_purpose::STANDARD
            .encode("shop,nmID,internalSkuCode\nShop A,1,SKU\n");
        let p = import::handle(
            &c,
            "import_preview",
            &json!({"kind":"mapping","filename":"map.csv","base64":data}),
        )
        .unwrap();
        assert_eq!(p["rows"][0]["status"], "Conflict");
        assert!(import::handle(&c, "import_commit", &p).is_err());
    }
    #[test]
    fn listing_filters_and_stale_quality() {
        let c = fixture();
        normalize(&c, "a", 1, &card(1, 10, "B")).unwrap();
        c.execute("UPDATE pm_listings SET last_synced_at='2020-01-01'", [])
            .unwrap();
        let p = extra::list(
            &c,
            &json!({"kind":"listings","shopId":"a","quality":"stale","status":"UNMAPPED"}),
        )
        .unwrap();
        assert_eq!(p["total"], 1);
        assert_eq!(p["rows"][0]["dataStatus"], "stale");
        assert_eq!(
            extra::list(&c, &json!({"kind":"listings","shopId":"b"})).unwrap()["total"],
            0
        );
    }
    #[test]
    fn create_status_and_wb_identifier_aliases() {
        let c = fixture();
        let sku = dispatch(
            &c,
            "create_sku",
            json!({"code":"TOOL","status":"draft"}),
            "admin",
        )
        .unwrap();
        assert_eq!(
            extra::detail(&c, sku["id"].as_str().unwrap(), "skus").unwrap()["item"]["status"],
            "draft"
        );
        normalize(&c, "a", 1, &card(1, 10, "B")).unwrap();
        assert_eq!(
            resolve(&c, "a", &json!({"nmID":1,"chrtID":10})).unwrap()["skuId"],
            sku["id"]
        );
        assert!(resolve(&c, "a", &json!({"nmID":"garbage"})).is_err());
    }
    #[test]
    fn ten_thousand_skus_twenty_thousand_listings_fifty_thousand_barcodes() {
        let c = fixture();
        c.execute_batch("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<10000) INSERT INTO pm_skus(id,code,name) SELECT 'sku-'||x,'SKU-'||x,'Product '||x FROM n;WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<20000) INSERT INTO pm_listings(id,shop_id,sku_id,nm_id,chrt_id,snapshot_json,mapping_status) SELECT 'listing-'||x,'a','sku-'||((x-1)%10000+1),CAST(x AS TEXT),CAST(x AS TEXT),'{}','MAPPED' FROM n;WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<50000) INSERT INTO pm_barcodes(listing_id,barcode) SELECT 'listing-'||((x-1)%20000+1),'BAR-'||x FROM n;").unwrap();
        let result = extra::list(&c, &json!({"kind":"skus","query":"BAR-49999"})).unwrap();
        assert_eq!(result["total"], 1);
        assert_eq!(result["rows"][0]["code"], "SKU-9999");
    }
}

pub(crate) fn normalize(c: &Connection, shop_id: &str, raw_id: i64, card: &Value) -> Result<usize> {
    normalize_status(c, shop_id, raw_id, card, "active")
}
pub(crate) fn normalize_status(
    c: &Connection,
    shop_id: &str,
    raw_id: i64,
    card: &Value,
    listing_status: &str,
) -> Result<usize> {
    shop(c, shop_id)?;
    let raw_valid: bool = c
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM pm_raw WHERE id=?1 AND shop_id=?2)",
            params![raw_id, shop_id],
            |r| r.get(0),
        )
        .map_err(err)?;
    if !raw_valid {
        return Err("RAW 与店铺不匹配".into());
    }
    let external = |v: &Value| {
        v.as_u64()
            .filter(|n| *n > 0)
            .map(|n| n.to_string())
            .or_else(|| {
                v.as_str()
                    .filter(|s| !s.is_empty() && s.chars().all(|x| x.is_ascii_digit()) && *s != "0")
                    .map(str::to_owned)
            })
    };
    let nm = external(&card["nmID"]).ok_or("INVALID_EXTERNAL_ID: nmID")?;
    let sizes = card["sizes"].as_array().ok_or("PARTIAL: missing sizes")?;
    if sizes.is_empty() {
        return Err("PARTIAL: empty sizes".into());
    }
    let mut count = 0;
    for size in sizes {
        let chrt = external(&size["chrtID"]).ok_or("INVALID_EXTERNAL_ID: chrtID")?;
        let before:Option<(String,Option<String>,String)>=c.query_row("SELECT id,sku_id,snapshot_json FROM pm_listings WHERE shop_id=?1 AND nm_id=?2 AND chrt_id=?3",params![shop_id,nm,chrt],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(err)?;
        let id = before.as_ref().map(|x| x.0.clone()).unwrap_or(uid(c)?);
        let previous_status: Option<String> = c
            .query_row(
                "SELECT listing_status FROM pm_listings WHERE id=?1",
                [&id],
                |r| r.get(0),
            )
            .optional()
            .map_err(err)?;
        let snapshot = json!({"card":card,"size":size});
        let vendor = card["vendorCode"].as_str();
        let title = card["title"].as_str();
        c.execute("INSERT INTO pm_listings(id,shop_id,nm_id,chrt_id,vendor_code,title,brand,category,snapshot_json,data_status,raw_id)VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)ON CONFLICT(shop_id,nm_id,chrt_id)DO UPDATE SET vendor_code=excluded.vendor_code,title=excluded.title,brand=excluded.brand,category=excluded.category,snapshot_json=excluded.snapshot_json,data_status=excluded.data_status,raw_id=excluded.raw_id,last_synced_at=CURRENT_TIMESTAMP",params![id,shop_id,nm,chrt,vendor,title,card["brand"].as_str(),card["subjectName"].as_str(),snapshot.to_string(),if vendor.is_some()&&title.is_some()&&size["skus"].as_array().is_some_and(|x|!x.is_empty()){"complete"}else{"partial"},raw_id]).map_err(err)?;
        c.execute("DELETE FROM pm_barcodes WHERE listing_id=?1", [&id])
            .map_err(err)?;
        if let Some(barcodes) = size["skus"].as_array() {
            for b in barcodes {
                if let Some(b) = b.as_str().filter(|s| !s.is_empty()) {
                    c.execute(
                        "INSERT OR IGNORE INTO pm_barcodes(listing_id,barcode)VALUES(?1,?2)",
                        params![id, b],
                    )
                    .map_err(err)?;
                }
            }
        }
        if let Some((_, _, old)) = &before {
            if serde_json::from_str::<Value>(old).ok().as_ref() != Some(&snapshot) {
                event(
                    c,
                    &id,
                    "LISTING_CHANGED",
                    serde_json::from_str(old).unwrap_or(Value::Null),
                    snapshot.clone(),
                    "WB sync",
                )?;
            }
        }
        let mut missing = Vec::new();
        for (key, value) in [
            ("title", &card["title"]),
            ("vendorCode", &card["vendorCode"]),
            ("category", &card["subjectName"]),
            ("weightKg", &card["dimensions"]["weightBrutto"]),
        ] {
            if value.is_null() || value.as_str() == Some("") {
                missing.push(key)
            }
        }
        if size["skus"].as_array().is_none_or(|a| a.is_empty()) {
            missing.push("barcode")
        }
        c.execute("UPDATE pm_listings SET listing_status=?4,missing_fields=?2,data_status=?3,created_at=coalesce(created_at,CURRENT_TIMESTAMP),updated_at=CURRENT_TIMESTAMP WHERE id=?1",params![id,json!(missing).to_string(),if missing.is_empty(){"complete"}else{"partial"},listing_status]).map_err(err)?;
        if previous_status
            .as_deref()
            .is_some_and(|s| s != listing_status)
        {
            event(
                c,
                &id,
                "EXTERNAL_STATUS_CHANGED",
                json!(previous_status),
                json!(listing_status),
                "WB sync",
            )?;
        }
        if let Some((_, _, old)) = &before {
            let previous: Value = serde_json::from_str(old).unwrap_or(Value::Null);
            for (field, kind) in [
                ("title", "TITLE_CHANGED"),
                ("brand", "BRAND_CHANGED"),
                ("subjectName", "CATEGORY_CHANGED"),
                ("characteristics", "ATTRIBUTES_CHANGED"),
                ("photos", "MEDIA_CHANGED"),
            ] {
                if previous["card"][field] != card[field] {
                    event(
                        c,
                        &id,
                        kind,
                        previous["card"][field].clone(),
                        card[field].clone(),
                        "WB sync",
                    )?
                }
            }
            let old_b = previous["size"]["skus"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let new_b = size["skus"].as_array().cloned().unwrap_or_default();
            for b in &new_b {
                if !old_b.contains(b) {
                    event(c, &id, "BARCODE_ADDED", Value::Null, b.clone(), "WB sync")?
                }
            }
            for b in &old_b {
                if !new_b.contains(b) {
                    event(c, &id, "BARCODE_REMOVED", b.clone(), Value::Null, "WB sync")?
                }
            }
        }
        let ignored: bool = c
            .query_row(
                "SELECT mapping_status='IGNORED' FROM pm_listings WHERE id=?1",
                [&id],
                |r| r.get(0),
            )
            .map_err(err)?;
        if !ignored {
            let mut matches = BTreeSet::new();
            let mut match_confidence = "exact";
            if let Some(sku) = before.as_ref().and_then(|b| b.1.as_ref()) {
                matches.insert(sku.clone());
            }
            let mut stmt=c.prepare("SELECT DISTINCT l.sku_id FROM pm_listings l JOIN pm_barcodes b ON b.listing_id=l.id JOIN pm_barcodes own ON own.barcode=b.barcode WHERE own.listing_id=?1 AND l.shop_id=?2 AND l.sku_id IS NOT NULL").map_err(err)?;
            matches.extend(
                stmt.query_map(params![id, shop_id], |r| r.get::<_, String>(0))
                    .map_err(err)?
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(err)?,
            );
            if matches.is_empty() && sizes.len() == 1 {
                if let Some(v) = vendor {
                    let sku:Option<String>=c.query_row("SELECT id FROM pm_skus WHERE organization_id='local' AND code=?1 AND status!='archived'",[v],|r|r.get(0)).optional().map_err(err)?;
                    if let Some(sku) = sku {
                        matches.insert(sku);
                        match_confidence = "high";
                    }
                }
            }
            let duplicate:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM pm_barcodes b JOIN pm_barcodes own ON b.barcode=own.barcode JOIN pm_listings l ON l.id=b.listing_id WHERE own.listing_id=?1 AND b.listing_id!=?1 AND l.shop_id=?2)",params![id,shop_id],|r|r.get(0)).map_err(err)?;
            if matches.len() > 1 || duplicate {
                c.execute(
                    "UPDATE pm_listings SET mapping_status='CONFLICT',confidence='low' WHERE id=?1 OR (shop_id=?2 AND mapping_status!='IGNORED' AND id IN(SELECT b.listing_id FROM pm_barcodes b JOIN pm_barcodes own ON own.barcode=b.barcode WHERE own.listing_id=?1))",
                    params![id,shop_id],
                )
                .map_err(err)?;
                event(
                    c,
                    &id,
                    "CONFLICT_DETECTED",
                    Value::Null,
                    json!({"candidates":matches,"duplicateBarcode":duplicate}),
                    "同步标识冲突",
                )?;
            } else if let Some(sku) = matches.first() {
                c.execute("UPDATE pm_listings SET sku_id=?2,mapping_status='MAPPED',confidence=?3 WHERE id=?1",params![id,sku,match_confidence]).map_err(err)?;
                if before.as_ref().and_then(|x| x.1.as_ref()) != Some(sku) {
                    event(
                        c,
                        &id,
                        "AUTO_MAPPED",
                        Value::Null,
                        json!({"skuId":sku,"confidence":match_confidence}),
                        "WB sync",
                    )?;
                }
            }
        }
        extra::reconcile(c, shop_id, &id)?;
        count += 1;
    }
    Ok(count)
}

#[tauri::command]
pub async fn product_master(
    action: String,
    payload: Option<Value>,
    state: State<'_, AppState>,
) -> Result<Value> {
    let state = crate::background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || {
        let c = wb_shop_center::db(&state)?;
        ensure(&c)?;
        let p = payload.unwrap_or(json!({}));
        dispatch(
            &c,
            &action,
            p,
            &std::env::var("WBERP_LOCAL_ROLE").unwrap_or_else(|_| "admin".into()),
        )
    })
    .await
    .map_err(err)?
}

fn dispatch(c: &Connection, action: &str, p: Value, role: &str) -> Result<Value> {
    if p.get("actorRole").is_some() || p.get("actor").is_some() {
        return Err("客户端不能指定操作者或角色".into());
    }
    if !extra::allowed(role, action) {
        return Err("Permission denied".into());
    }
    match action {
        "list" => extra::list(c, &p),
        "create_spu" => {
            let tx = c.unchecked_transaction().map_err(err)?;
            let id = uid(&tx)?;
            if txt(&p, "code").is_empty() {
                return Err("SPU 编码不能为空".into());
            }
            tx.execute(
                "INSERT INTO pm_spus(id,code,name)VALUES(?1,?2,?3)",
                params![id, txt(&p, "code"), txt(&p, "name")],
            )
            .map_err(err)?;
            extra::metadata(&tx, &id, &p)?;
            let status = txt(&p, "status");
            if !status.is_empty() {
                if !["active", "draft", "archived"].contains(&status.as_str()) {
                    return Err("系列状态无效".into());
                }
                tx.execute(
                    "UPDATE pm_spus SET status=?2 WHERE id=?1",
                    params![id, status],
                )
                .map_err(err)?;
            }
            event(&tx, &id, "CREATE_SPU", Value::Null, p, "")?;
            tx.commit().map_err(err)?;
            Ok(json!({"id":id}))
        }
        "create_sku" => {
            let tx = c.unchecked_transaction().map_err(err)?;
            let id = create_sku(&tx, &p)?;
            let listing = txt(&p, "listingId");
            if !listing.is_empty() {
                bind(&tx, &listing, &id, &txt(&p, "reason"))?;
            }
            tx.commit().map_err(err)?;
            Ok(json!({"id":id}))
        }
        "bind" => {
            let tx = c.unchecked_transaction().map_err(err)?;
            bind(
                &tx,
                &txt(&p, "listingId"),
                &txt(&p, "skuId"),
                &txt(&p, "reason"),
            )?;
            tx.commit().map_err(err)?;
            Ok(json!({"ok":true}))
        }
        "ignore" => {
            let tx = c.unchecked_transaction().map_err(err)?;
            let id = txt(&p, "listingId");
            if txt(&p, "reason").is_empty() {
                return Err("忽略商品必须填写原因".into());
            }
            let before = extra::detail(&tx, &id, "listings")?;
            let shop_id = before["item"]["shop_id"].as_str().ok_or("店铺无效")?;
            tx.execute("UPDATE pm_listings SET mapping_status='IGNORED',updated_at=CURRENT_TIMESTAMP WHERE id=?1",[&id]).map_err(err)?;
            event(
                &tx,
                &id,
                "IGNORE_LISTING",
                before["item"].clone(),
                json!("IGNORED"),
                &txt(&p, "reason"),
            )?;
            extra::reconcile(&tx, shop_id, &id)?;
            tx.commit().map_err(err)?;
            Ok(json!({"ok":true}))
        }
        "detail" => extra::detail(c, &txt(&p, "id"), &txt(&p, "kind")),
        "resolve" => resolve(c, &txt(&p, "shopId"), &p),
        "import_preview" | "import_commit" => import::handle(c, action, &p),
        _ => extra::handle(c, action, &p, role),
    }
}
