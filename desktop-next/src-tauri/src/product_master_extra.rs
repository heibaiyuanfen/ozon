use super::*;

pub(super) fn schema(c: &Connection) -> Result<()> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS pm_metadata(entity_id TEXT PRIMARY KEY, payload TEXT NOT NULL DEFAULT '{}');
    CREATE TABLE IF NOT EXISTS pm_imports(id TEXT PRIMARY KEY,kind TEXT NOT NULL,rows_json TEXT NOT NULL,status TEXT NOT NULL DEFAULT 'preview',created_at TEXT DEFAULT CURRENT_TIMESTAMP);
    CREATE TABLE IF NOT EXISTS pm_external_references(id TEXT PRIMARY KEY,shop_id TEXT NOT NULL REFERENCES wb_shops(id),source TEXT NOT NULL,external_key TEXT NOT NULL,payload TEXT NOT NULL,result TEXT NOT NULL,updated_at TEXT DEFAULT CURRENT_TIMESTAMP,UNIQUE(shop_id,source,external_key));
    CREATE INDEX IF NOT EXISTS pm_sku_name ON pm_skus(name);
    INSERT OR IGNORE INTO pm_migrations(version)VALUES(3);").map_err(err)?;
    let columns = c
        .prepare("PRAGMA table_info(pm_listings)")
        .map_err(err)?
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(err)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(err)?;
    for (name, definition) in [
        ("listing_status", "TEXT NOT NULL DEFAULT 'unknown'"),
        ("missing_fields", "TEXT NOT NULL DEFAULT '[]'"),
        ("created_at", "TEXT"),
        ("updated_at", "TEXT"),
    ] {
        if !columns.iter().any(|c| c == name) {
            c.execute(
                &format!("ALTER TABLE pm_listings ADD COLUMN {name} {definition}"),
                [],
            )
            .map_err(err)?;
        }
    }
    Ok(())
}
fn row(c: &Connection, table: &str, id: &str) -> Result<Value> {
    let scope = if table == "pm_listings" {
        "shop_id IN(SELECT id FROM wb_shops WHERE organization_id='local')"
    } else {
        "organization_id='local'"
    };
    let mut stmt = c
        .prepare(&format!("SELECT * FROM {table} WHERE id=?1 AND {scope}"))
        .map_err(err)?;
    let names = stmt
        .column_names()
        .iter()
        .map(|x| x.to_string())
        .collect::<Vec<_>>();
    stmt.query_row([id], |r| {
        let mut out = serde_json::Map::new();
        for (i, n) in names.iter().enumerate() {
            let v = match r.get_ref(i)? {
                rusqlite::types::ValueRef::Text(v) => json!(String::from_utf8_lossy(v)),
                rusqlite::types::ValueRef::Integer(v) => json!(v),
                _ => Value::Null,
            };
            out.insert(n.clone(), v);
        }
        Ok(Value::Object(out))
    })
    .map_err(err)
}
pub(super) fn validate(p: &Value) -> Result<()> {
    for k in ["widthCm", "lengthCm", "heightCm", "weightKg", "packSize"] {
        let s = txt(p, k);
        if !s.is_empty()
            && s.parse::<f64>()
                .map(|x| !x.is_finite() || x <= 0.0)
                .unwrap_or(true)
        {
            return Err(format!("{k} 必须为正数，未知请留空"));
        }
    }
    let cost = txt(p, "purchaseCost");
    if !cost.is_empty()
        && !regex::Regex::new(r"^\d{1,12}(\.\d{1,4})?$")
            .unwrap()
            .is_match(&cost)
    {
        return Err("采购成本格式无效".into());
    }
    if let Some(a) = p.get("attributes") {
        if !a.is_object() {
            return Err("属性必须为对象".into());
        }
    }
    if let Some(a) = p.get("media") {
        if !a.is_array() {
            return Err("媒体必须为数组".into());
        }
        for m in a.as_array().unwrap() {
            let u = txt(m, "url");
            if !u.starts_with("https://") && !u.starts_with("http://") {
                return Err("图片地址必须使用 http 或 https".into());
            }
        }
    }
    Ok(())
}
pub(super) fn metadata(c: &Connection, id: &str, p: &Value) -> Result<()> {
    validate(p)?;
    c.execute("INSERT INTO pm_metadata(entity_id,payload)VALUES(?1,?2)ON CONFLICT(entity_id)DO UPDATE SET payload=excluded.payload",params![id,p.to_string()]).map_err(err)?;
    Ok(())
}
pub(super) fn allowed(role: &str, action: &str) -> bool {
    if [
        "list",
        "list_v2",
        "detail",
        "detail_v2",
        "resolve",
        "preview_bind",
        "summary",
        "duplicates",
        "suggest",
    ]
    .contains(&action)
    {
        return ["admin", "operator", "viewer"].contains(&role);
    }
    role == "admin"
        || role == "operator"
            && ["bind", "update_sku", "remember_reference", "reprocess"].contains(&action)
}
pub(super) fn conflicts(c: &Connection, listing: &str, sku: &str) -> Result<Vec<Value>> {
    records(c,"SELECT json_object('id',l.id,'skuId',l.sku_id,'nmId',l.nm_id,'chrtId',l.chrt_id,'vendorCode',l.vendor_code,'reason',CASE WHEN l.chrt_id=o.chrt_id THEN 'CHRTID_DUPLICATE' ELSE 'BARCODE_CONFLICT' END) FROM pm_listings o JOIN pm_listings l ON l.shop_id=o.shop_id AND l.id!=o.id WHERE o.id=?1 AND l.mapping_status!='IGNORED' AND (l.chrt_id=o.chrt_id OR EXISTS(SELECT 1 FROM pm_barcodes a JOIN pm_barcodes b ON a.barcode=b.barcode WHERE a.listing_id=o.id AND b.listing_id=l.id)) AND (l.sku_id IS NULL OR l.sku_id!=?2)",params![listing,sku])
}
pub(super) fn reconcile(c: &Connection, shop_id: &str, seed: &str) -> Result<()> {
    let ids=records(c,"SELECT json_quote(l.id) FROM pm_listings l WHERE l.shop_id=?1 AND l.mapping_status!='IGNORED' AND (l.id=?2 OR l.mapping_status='CONFLICT' OR l.chrt_id=(SELECT chrt_id FROM pm_listings WHERE id=?2) OR EXISTS(SELECT 1 FROM pm_barcodes b JOIN pm_barcodes own ON own.barcode=b.barcode WHERE own.listing_id=?2 AND b.listing_id=l.id))",params![shop_id,seed])?;
    for id in ids {
        let id = id.as_str().unwrap_or("");
        let old = row(c, "pm_listings", id)?;
        let sku = old["sku_id"].as_str().unwrap_or("");
        let conflict = !conflicts(c, id, sku)?.is_empty();
        let status = if conflict {
            "CONFLICT"
        } else if !sku.is_empty() {
            "MAPPED"
        } else {
            "UNMAPPED"
        };
        if old["mapping_status"] != status {
            c.execute("UPDATE pm_listings SET mapping_status=?2,confidence=CASE WHEN ?2='MAPPED' THEN confidence ELSE 'low' END,updated_at=CURRENT_TIMESTAMP WHERE id=?1",params![id,status]).map_err(err)?;
            event(
                c,
                id,
                if conflict {
                    "CONFLICT_DETECTED"
                } else if old["mapping_status"] == "CONFLICT" {
                    "CONFLICT_RESOLVED"
                } else {
                    "MAPPING_CHANGED"
                },
                old["mapping_status"].clone(),
                json!(status),
                "重新核验同店铺标识",
            )?;
        }
    }
    Ok(())
}
pub(super) fn list(c: &Connection, p: &Value) -> Result<Value> {
    let kind = txt(p, "kind");
    let q = format!("%{}%", txt(p, "query"));
    let page = p["page"].as_i64().unwrap_or(0).clamp(0, 100000);
    let mut clauses = vec![];
    let mut args: Vec<String> = vec![];
    let (from, select, search, sort) = if kind == "spus" {
        ("pm_spus s","json_object('id',s.id,'code',s.code,'name',s.name,'status',s.status,'variants',(SELECT count(*) FROM pm_skus k WHERE k.spu_id=s.id))","s.code LIKE ? OR s.name LIKE ?","s.code")
    } else if kind == "listings" {
        ("pm_listings s LEFT JOIN pm_skus k ON k.id=s.sku_id","json_object('id',s.id,'code',k.code,'name',s.title,'nmId',s.nm_id,'chrtId',s.chrt_id,'vendorCode',s.vendor_code,'skuId',s.sku_id,'shopId',s.shop_id,'shop',(SELECT name FROM wb_shops WHERE id=s.shop_id),'barcodes',(SELECT group_concat(barcode,', ') FROM pm_barcodes WHERE listing_id=s.id),'mappingStatus',s.mapping_status,'dataStatus',CASE WHEN julianday('now')-julianday(s.last_synced_at)>7 THEN 'stale' ELSE s.data_status END,'lastSync',s.last_synced_at)","s.title LIKE ? OR s.nm_id LIKE ? OR s.chrt_id LIKE ? OR s.vendor_code LIKE ? OR k.code LIKE ? OR EXISTS(SELECT 1 FROM pm_barcodes b WHERE b.listing_id=s.id AND b.barcode LIKE ?)","s.last_synced_at DESC")
    } else {
        ("pm_skus s LEFT JOIN pm_spus z ON z.id=s.spu_id","json_object('id',s.id,'code',s.code,'name',s.name,'spuId',s.spu_id,'spu',z.code,'brand',s.brand,'color',s.color,'size',s.size,'status',s.status,'category',s.category,'supplier',s.supplier,'listings',(SELECT count(*) FROM pm_listings l WHERE l.sku_id=s.id),'purchaseCost',s.purchase_cost)","s.code LIKE ? OR s.name LIKE ? OR z.code LIKE ? OR z.name LIKE ? OR EXISTS(SELECT 1 FROM pm_listings l LEFT JOIN pm_barcodes b ON b.listing_id=l.id WHERE l.sku_id=s.id AND (l.nm_id LIKE ? OR l.chrt_id LIKE ? OR l.vendor_code LIKE ? OR b.barcode LIKE ?))","s.code")
    };
    clauses.push(if kind == "listings" {
        "s.shop_id IN(SELECT id FROM wb_shops WHERE organization_id='local')".into()
    } else {
        "s.organization_id='local'".into()
    });
    clauses.push(format!("({search})"));
    for _ in 0..search.matches('?').count() {
        args.push(q.clone())
    }
    for (key, col) in if kind == "listings" {
        vec![
            ("shopId", "s.shop_id"),
            ("status", "s.mapping_status"),
            ("brand", "s.brand"),
            ("category", "s.category"),
            ("spuId", "k.spu_id"),
            ("supplier", "k.supplier"),
            ("quality", "CASE WHEN julianday('now')-julianday(s.last_synced_at)>7 THEN 'stale' ELSE s.data_status END"),
        ]
    } else if kind == "spus" {
        vec![("status", "s.status")]
    } else {
        vec![
            ("spuId", "s.spu_id"),
            ("status", "s.status"),
            ("brand", "s.brand"),
            ("category", "s.category"),
            ("supplier", "s.supplier"),
        ]
    } {
        let v = txt(p, key);
        if !v.is_empty() {
            clauses.push(format!("{col}=?"));
            args.push(v)
        }
    }
    if kind == "skus" && !txt(p, "shopId").is_empty() {
        clauses
            .push("EXISTS(SELECT 1 FROM pm_listings l WHERE l.sku_id=s.id AND l.shop_id=?)".into());
        args.push(txt(p, "shopId"))
    }
    let order = match (kind.as_str(), txt(p, "sort").as_str()) {
        ("listings", "created") => "s.created_at DESC",
        ("listings", "updated") => "s.updated_at DESC",
        ("listings", "mapping") => "s.mapping_status",
        ("listings", _) => sort,
        (_, "created") => "s.created_at DESC",
        (_, "updated") => "s.updated_at DESC",
        _ => sort,
    };
    let filter = clauses.join(" AND ");
    let total: i64 = c
        .query_row(
            &format!("SELECT count(*) FROM {from} WHERE {filter}"),
            rusqlite::params_from_iter(&args),
            |r| r.get(0),
        )
        .map_err(err)?;
    let rows = records(
        c,
        &format!(
            "SELECT {select} FROM {from} WHERE {filter} ORDER BY {order},s.id LIMIT 50 OFFSET {}",
            page * 50
        ),
        rusqlite::params_from_iter(&args),
    )?;
    Ok(
        json!({"rows":rows,"total":total,"page":page,"spus":records(c,"SELECT json_object('id',id,'code',code,'name',name) FROM pm_spus WHERE organization_id='local' AND status!='archived' ORDER BY code LIMIT 500",[])?}),
    )
}
pub(super) fn detail(c: &Connection, id: &str, kind: &str) -> Result<Value> {
    let table = match kind {
        "spus" => "pm_spus",
        "listings" => "pm_listings",
        _ => "pm_skus",
    };
    let mut item = row(c, table, id)?;
    item["metadata"] = c
        .query_row(
            "SELECT payload FROM pm_metadata WHERE entity_id=?1",
            [id],
            |r| r.get::<_, String>(0),
        )
        .optional()
        .map_err(err)?
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .unwrap_or(json!({}));
    let listings=records(c,"SELECT json_object('id',l.id,'shop',s.name,'nmId',l.nm_id,'chrtId',l.chrt_id,'vendorCode',l.vendor_code,'mappingStatus',l.mapping_status,'confidence',l.confidence,'dataStatus',l.data_status,'barcodes',(SELECT group_concat(barcode,', ') FROM pm_barcodes WHERE listing_id=l.id),'lastSync',l.last_synced_at,'snapshot',json(l.snapshot_json)) FROM pm_listings l JOIN wb_shops s ON s.id=l.shop_id WHERE l.id=?1 OR l.sku_id=?1 OR l.sku_id IN(SELECT id FROM pm_skus WHERE spu_id=?1)",[id])?;
    let variants=records(c,"SELECT json_object('id',id,'code',code,'name',name,'color',color,'size',size,'status',status,'listings',(SELECT count(*) FROM pm_listings WHERE sku_id=pm_skus.id)) FROM pm_skus WHERE spu_id=?1 ORDER BY code LIMIT 500",[id])?;
    let history=records(c,"SELECT json_object('event',event_type,'before',before_json,'after',after_json,'reason',reason,'actor',actor,'time',created_at) FROM pm_history WHERE entity_id=?1 OR entity_id IN(SELECT id FROM pm_listings WHERE sku_id=?1) ORDER BY id DESC LIMIT 200",[id])?;
    Ok(json!({"item":item,"listings":listings,"variants":variants,"history":history}))
}
fn update(c: &Connection, p: &Value, is_spu: bool, role: &str) -> Result<Value> {
    validate(p)?;
    let id = txt(p, "id");
    let table = if is_spu { "pm_spus" } else { "pm_skus" };
    let before = row(c, table, &id)?;
    let status = txt(p, "status");
    if !["active", "draft", "inactive", "archived"].contains(&status.as_str()) {
        return Err("商品状态无效".into());
    }
    if txt(p, "code").is_empty() {
        return Err("内部编码不能为空".into());
    }
    if role == "operator" {
        for (key, col) in [
            ("code", "code"),
            ("status", "status"),
            ("purchaseCost", "purchase_cost"),
            ("supplier", "supplier"),
        ] {
            if txt(p, key) != before[col].as_str().unwrap_or("") {
                return Err("运营角色不能修改编码、状态、成本或供应商".into());
            }
        }
    }
    if is_spu {
        c.execute(
            "UPDATE pm_spus SET code=?2,name=?3,status=?4,updated_at=CURRENT_TIMESTAMP WHERE id=?1",
            params![id, txt(p, "code"), txt(p, "name"), status],
        )
        .map_err(err)?;
    } else {
        let spu = txt(p, "spuId");
        c.execute("UPDATE pm_skus SET code=?2,name=?3,status=?4,spu_id=?5,brand=?6,color=?7,size=?8,category=?9,supplier=?10,purchase_cost=?11,attributes_json=?12,media_json=?13,updated_at=CURRENT_TIMESTAMP WHERE id=?1",params![id,txt(p,"code"),txt(p,"name"),status,if spu.is_empty(){None}else{Some(spu)},txt(p,"brand"),txt(p,"color"),txt(p,"size"),txt(p,"category"),txt(p,"supplier"),if txt(p,"purchaseCost").is_empty(){None}else{Some(txt(p,"purchaseCost"))},p.get("attributes").unwrap_or(&json!({})).to_string(),p.get("media").unwrap_or(&json!([])).to_string()]).map_err(err)?;
    }
    metadata(c, &id, p)?;
    event(
        c,
        &id,
        if status == "archived" {
            if is_spu {
                "ARCHIVE_SPU"
            } else {
                "ARCHIVE_SKU"
            }
        } else if is_spu {
            "UPDATE_SPU"
        } else {
            "UPDATE_SKU"
        },
        before,
        p.clone(),
        &txt(p, "reason"),
    )?;
    Ok(json!({"id":id}))
}
pub(super) fn handle(c: &Connection, action: &str, p: &Value, role: &str) -> Result<Value> {
    match action {
        "suggest"=>{
            let listing=row(c,"pm_listings",&txt(p,"listingId"))?;
            let title=listing["title"].as_str().unwrap_or("").to_lowercase();
            let grams=|s:&str|{let chars=s.chars().filter(|c|!c.is_whitespace()).collect::<Vec<_>>();chars.windows(2).map(|p|p.iter().collect::<String>()).collect::<BTreeSet<_>>()};
            let target=grams(&title);let mut candidates=records(c,"SELECT json_object('id',id,'code',code,'name',name,'color',color,'size',size) FROM pm_skus WHERE organization_id='local' AND status!='archived' ORDER BY code",[])?;
            for item in &mut candidates{let name=item["name"].as_str().unwrap_or("").to_lowercase();let g=grams(&name);let overlap=target.intersection(&g).count() as f64;let total=target.union(&g).count().max(1) as f64;item["suggestionScore"]=json!(if item["code"]==listing["vendor_code"]{1.0}else{overlap/total});item["suggestion"]=json!("仅供人工核对，未自动绑定");}
            candidates.retain(|r|r["suggestionScore"].as_f64().unwrap_or(0.0)>=0.25);candidates.sort_by(|a,b|b["suggestionScore"].as_f64().unwrap_or(0.0).total_cmp(&a["suggestionScore"].as_f64().unwrap_or(0.0)));candidates.truncate(10);Ok(json!({"rows":candidates}))
        },
        "list_v2"=>list(c,p),"detail_v2"=>detail(c,&txt(p,"id"),&txt(p,"kind")),
        "update_sku"|"update_spu"=>{let tx=c.unchecked_transaction().map_err(err)?;let result=update(&tx,p,action=="update_spu",role)?;tx.commit().map_err(err)?;Ok(result)},
        "preview_bind"=>{let listing=row(c,"pm_listings",&txt(p,"listingId"))?;let sku=row(c,"pm_skus",&txt(p,"skuId"))?;Ok(json!({"listing":listing,"sku":sku,"conflicts":conflicts(c,&txt(p,"listingId"),&txt(p,"skuId"))?}))},
        "unbind"=>{let tx=c.unchecked_transaction().map_err(err)?;let id=txt(p,"listingId");let before=row(&tx,"pm_listings",&id)?;if txt(p,"reason").is_empty(){return Err("解除绑定必须填写原因".into())}tx.execute("UPDATE pm_listings SET sku_id=NULL,mapping_status='UNMAPPED',confidence='low' WHERE id=?1",[&id]).map_err(err)?;event(&tx,&id,"UNBIND_LISTING",before,Value::Null,&txt(p,"reason"))?;tx.commit().map_err(err)?;Ok(json!({"ok":true}))},
        "bulk_update"=>{let tx=c.unchecked_transaction().map_err(err)?;let ids=p["ids"].as_array().ok_or("请选择 SKU")?;if ids.len()>500{return Err("每批最多 500 条".into())}let column=match txt(p,"field").as_str(){"spuId"=>"spu_id","category"=>"category","supplier"=>"supplier","status"=>"status",_=>return Err("批量字段不允许".into())};let value=txt(p,"value");if column=="status"&&!["active","draft","inactive","archived"].contains(&value.as_str()){return Err("状态无效".into())}for id in ids {let id=id.as_str().ok_or("SKU ID 无效")?;let before=row(&tx,"pm_skus",id)?;tx.execute(&format!("UPDATE pm_skus SET {column}=?2,updated_at=CURRENT_TIMESTAMP WHERE id=?1"),params![id,if value.is_empty(){None}else{Some(&value)}]).map_err(err)?;event(&tx,id,"UPDATE_SKU",before,p.clone(),"批量修改")?;}tx.commit().map_err(err)?;Ok(json!({"updated":ids.len()}))},
        "summary"=>Ok(json!({"role":role,"counts":records(c,"SELECT json_object('status',mapping_status,'count',count(*)) FROM pm_listings WHERE (?1='' OR shop_id=?1) GROUP BY mapping_status",[txt(p,"shopId")])?,"lastRun":records(c,"SELECT json_object('status',status,'processed',records_processed,'succeeded',records_succeeded,'failed',records_failed,'started',started_at,'finished',finished_at,'error',error_message) FROM wb_sync_job_runs WHERE resource_type='products' AND (?1='' OR shop_id=?1) ORDER BY rowid DESC LIMIT 1",[txt(p,"shopId")])?})),
        "duplicates"=>records(c,"SELECT json_object('type','BARCODE_CONFLICT','value',b.barcode,'shopId',l.shop_id,'count',count(DISTINCT l.id)) FROM pm_barcodes b JOIN pm_listings l ON l.id=b.listing_id GROUP BY l.shop_id,b.barcode HAVING count(DISTINCT l.id)>1 LIMIT 500",[]).map(|x|json!({"rows":x})),
        "remember_reference"=>{let source=txt(p,"source");let key=txt(p,"externalKey");if source.is_empty()||key.is_empty(){return Err("必须提供来源与外部记录键".into())}let result=resolve(c,&txt(p,"shopId"),&p["identifiers"])?;c.execute("INSERT INTO pm_external_references(id,shop_id,source,external_key,payload,result)VALUES(?1,?2,?3,?4,?5,?6)ON CONFLICT(shop_id,source,external_key)DO UPDATE SET payload=excluded.payload,result=excluded.result,updated_at=CURRENT_TIMESTAMP",params![uid(c)?,txt(p,"shopId"),source,key,p["identifiers"].to_string(),result.to_string()]).map_err(err)?;Ok(result)},
        "reprocess"=>{let tx=c.unchecked_transaction().map_err(err)?;let refs=records(&tx,"SELECT json_object('id',id,'shop',shop_id,'payload',json(payload)) FROM pm_external_references WHERE (?1='' OR shop_id=?1)",[txt(p,"shopId")])?;let mut resolved=0;for r in &refs {let result=resolve(&tx,r["shop"].as_str().unwrap_or(""),&r["payload"])?;if result["status"]=="resolved"{resolved+=1}tx.execute("UPDATE pm_external_references SET result=?2,updated_at=CURRENT_TIMESTAMP WHERE id=?1",params![r["id"].as_str(),result.to_string()]).map_err(err)?;}event(&tx,"references","REPROCESS",Value::Null,json!({"total":refs.len(),"resolved":resolved}),"")?;tx.commit().map_err(err)?;Ok(json!({"total":refs.len(),"resolved":resolved}))},
        _=>Err("未知商品操作".into())
    }
}
