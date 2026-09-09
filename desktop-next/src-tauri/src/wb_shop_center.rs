use crate::{secrets, AppState};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
};
use tauri::State;

static SEQUENCE: AtomicU64 = AtomicU64::new(1);
const ORG_ID: &str = "local";
const CAPABILITIES: [&str; 8] = [
    "content",
    "prices",
    "marketplace",
    "fbw",
    "promotion",
    "finance",
    "analytics",
    "documents",
];
const RESOURCES: [&str; 7] = [
    "products",
    "prices",
    "orders",
    "inventory",
    "fbw_supplies",
    "advertising",
    "finance",
];

fn id(prefix: &str) -> String {
    format!(
        "{prefix}-{}-{}",
        Utc::now().timestamp_millis(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}
fn now() -> String {
    Utc::now().to_rfc3339()
}
pub(crate) fn db(state: &AppState) -> Result<Connection, String> {
    let dir = state.data_dir.join("wb-v2");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let c = Connection::open(dir.join("shop_api_center.db")).map_err(|e| e.to_string())?;
    c.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| e.to_string())?;
    c.execute_batch(r#"
      PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
      CREATE TABLE IF NOT EXISTS wb_organizations(id TEXT PRIMARY KEY,name TEXT NOT NULL,created_at TEXT NOT NULL);
      INSERT OR IGNORE INTO wb_organizations(id,name,created_at)VALUES('local','本地组织',CURRENT_TIMESTAMP);
      CREATE TABLE IF NOT EXISTS wb_shops(id TEXT PRIMARY KEY,organization_id TEXT NOT NULL,platform TEXT NOT NULL DEFAULT 'wildberries',name TEXT NOT NULL,seller_type TEXT NOT NULL DEFAULT 'unknown',legal_entity_name TEXT,currency TEXT NOT NULL DEFAULT 'RUB',timezone TEXT,country TEXT,status TEXT NOT NULL DEFAULT 'connection_error',owner_user_id TEXT,created_at TEXT NOT NULL,updated_at TEXT NOT NULL,UNIQUE(organization_id,platform,name));
      CREATE INDEX IF NOT EXISTS idx_wb_shops_org ON wb_shops(organization_id); CREATE INDEX IF NOT EXISTS idx_wb_shops_status ON wb_shops(status);
      CREATE TABLE IF NOT EXISTS wb_api_credentials(id TEXT PRIMARY KEY,shop_id TEXT NOT NULL,provider TEXT NOT NULL DEFAULT 'wildberries',credential_type TEXT NOT NULL DEFAULT 'token',encrypted_secret TEXT NOT NULL,masked_hint TEXT NOT NULL,status TEXT NOT NULL,last_validated_at TEXT,last_validation_status TEXT,created_at TEXT NOT NULL,updated_at TEXT NOT NULL,FOREIGN KEY(shop_id)REFERENCES wb_shops(id));
      CREATE UNIQUE INDEX IF NOT EXISTS idx_wb_one_active_credential ON wb_api_credentials(shop_id,provider,credential_type) WHERE status='active';
      CREATE TABLE IF NOT EXISTS wb_api_capabilities(id TEXT PRIMARY KEY,shop_id TEXT NOT NULL,credential_id TEXT,capability TEXT NOT NULL,status TEXT NOT NULL,last_checked_at TEXT,error_code TEXT,error_message TEXT,UNIQUE(shop_id,capability),FOREIGN KEY(shop_id)REFERENCES wb_shops(id));
      CREATE TABLE IF NOT EXISTS wb_api_health(shop_id TEXT NOT NULL,capability TEXT NOT NULL,status TEXT NOT NULL,last_successful_request_at TEXT,last_failure_at TEXT,last_error_code TEXT,last_error_message TEXT,latency_ms INTEGER,rate_limit_state TEXT,PRIMARY KEY(shop_id,capability));
      CREATE TABLE IF NOT EXISTS wb_sync_jobs(id TEXT PRIMARY KEY,shop_id TEXT NOT NULL,resource_type TEXT NOT NULL,mode TEXT NOT NULL,status TEXT NOT NULL,created_at TEXT NOT NULL,scheduled_at TEXT);
      CREATE UNIQUE INDEX IF NOT EXISTS idx_wb_active_sync ON wb_sync_jobs(shop_id,resource_type) WHERE status IN('pending','running');
      CREATE TABLE IF NOT EXISTS wb_sync_job_runs(id TEXT PRIMARY KEY,sync_job_id TEXT NOT NULL,shop_id TEXT NOT NULL,resource_type TEXT NOT NULL,status TEXT NOT NULL,started_at TEXT,finished_at TEXT,last_cursor TEXT,last_synced_at TEXT,last_successful_sync_at TEXT,records_processed INTEGER NOT NULL DEFAULT 0,records_succeeded INTEGER NOT NULL DEFAULT 0,records_failed INTEGER NOT NULL DEFAULT 0,error_code TEXT,error_message TEXT,retry_count INTEGER NOT NULL DEFAULT 0);
      CREATE TABLE IF NOT EXISTS wb_sync_errors(id INTEGER PRIMARY KEY AUTOINCREMENT,sync_job_run_id TEXT NOT NULL,shop_id TEXT NOT NULL,resource_type TEXT NOT NULL,external_id TEXT,error_code TEXT,error_message TEXT NOT NULL,retryable INTEGER NOT NULL DEFAULT 0,resolved_at TEXT,created_at TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS wb_audit_logs(id INTEGER PRIMARY KEY AUTOINCREMENT,organization_id TEXT NOT NULL,shop_id TEXT,actor TEXT NOT NULL,action TEXT NOT NULL,metadata_json TEXT NOT NULL DEFAULT '{}',created_at TEXT NOT NULL);
      CREATE INDEX IF NOT EXISTS idx_wb_audit_shop ON wb_audit_logs(organization_id,shop_id,created_at);
    "#).map_err(|e| e.to_string())?;
    Ok(c)
}

fn require_role(payload: &Value, allowed: &[&str]) -> Result<String, String> {
    // A frontend cannot select its own identity or elevate its role.
    if payload.get("actorRole").is_some() || payload.get("actor").is_some() {
        return Err("客户端不能指定操作者或角色".into());
    }
    let role = std::env::var("WBERP_LOCAL_ROLE").unwrap_or_else(|_| "admin".into());
    if !allowed.contains(&role.as_str()) {
        return Err("Permission denied".into());
    }
    Ok("local-desktop".to_string())
}
fn field<'a>(p: &'a Value, key: &str) -> Result<&'a str, String> {
    p.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .ok_or_else(|| format!("缺少 {key}"))
}
fn audit(
    c: &Connection,
    shop: Option<&str>,
    actor: &str,
    action: &str,
    metadata: Value,
) -> Result<(), String> {
    c.execute("INSERT INTO wb_audit_logs(organization_id,shop_id,actor,action,metadata_json,created_at)VALUES(?1,?2,?3,?4,?5,?6)",params![ORG_ID,shop,actor,action,metadata.to_string(),now()]).map_err(|e|e.to_string())?;
    Ok(())
}
fn mask(token: &str) -> String {
    let tail: String = token
        .chars()
        .rev()
        .take(4)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("wb_****************{tail}")
}
pub(crate) fn active_token(c: &Connection, shop_id: &str) -> Result<(String, String), String> {
    let row: Option<(String,String)> = c.query_row("SELECT id,encrypted_secret FROM wb_api_credentials WHERE shop_id=?1 AND status='active' ORDER BY created_at DESC LIMIT 1",[shop_id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|e|e.to_string())?;
    let (id, cipher) = row.ok_or("API credential is not configured.")?;
    Ok((id, secrets::unprotect(&cipher)?))
}
fn classify(error: &ureq::Error) -> (&'static str, &'static str, bool) {
    match error {
        ureq::Error::Status(401, _) | ureq::Error::Status(403, _) => {
            ("unauthorized", "UNAUTHORIZED", false)
        }
        ureq::Error::Status(429, _) => ("rate_limited", "RATE_LIMITED", true),
        ureq::Error::Status(code, _) if *code >= 500 => ("degraded", "UPSTREAM_5XX", true),
        _ => ("error", "NETWORK_ERROR", true),
    }
}
fn probe(token: &str, capability: &str) -> (String, Option<String>, Option<String>, Option<i64>) {
    let started = std::time::Instant::now();
    let result = match capability {
        "content" => ureq::post("https://content-api.wildberries.ru/content/v2/get/cards/list")
            .set("Authorization", token)
            .set("Content-Type", "application/json")
            .send_string(
                &json!({"settings":{"cursor":{"limit":1},"filter":{"withPhoto":-1}}}).to_string(),
            ),
        "marketplace" => ureq::get("https://marketplace-api.wildberries.ru/api/v3/offices")
            .set("Authorization", token)
            .call(),
        "promotion" => ureq::get("https://advert-api.wildberries.ru/adv/v1/promotion/count")
            .set("Authorization", token)
            .call(),
        _ => {
            return (
                "unknown".into(),
                None,
                Some("当前客户端尚未配置该能力的安全探测请求".into()),
                None,
            )
        }
    };
    let latency = Some(started.elapsed().as_millis().min(i64::MAX as u128) as i64);
    match result {
        Ok(_) => ("healthy".into(), None, None, latency),
        Err(e) => {
            let (s, code, _) = classify(&e);
            (s.into(), Some(code.into()), Some(format!("{}", e)), latency)
        }
    }
}
fn check_capabilities(c: &Connection, shop_id: &str) -> Result<(), String> {
    let (credential_id, token) = active_token(c, shop_id)?;
    for capability in CAPABILITIES {
        let (health, code, message, latency) = probe(&token, capability);
        let status = match health.as_str() {
            "healthy" => "available",
            "unknown" => "unknown",
            "unauthorized" => "unavailable",
            _ => "error",
        };
        c.execute("INSERT INTO wb_api_capabilities(id,shop_id,credential_id,capability,status,last_checked_at,error_code,error_message)VALUES(?1,?2,?3,?4,?5,?6,?7,?8)ON CONFLICT(shop_id,capability)DO UPDATE SET credential_id=excluded.credential_id,status=excluded.status,last_checked_at=excluded.last_checked_at,error_code=excluded.error_code,error_message=excluded.error_message",params![id("cap"),shop_id,credential_id,capability,status,now(),code,message]).map_err(|e|e.to_string())?;
        c.execute("INSERT INTO wb_api_health(shop_id,capability,status,last_successful_request_at,last_failure_at,last_error_code,last_error_message,latency_ms,rate_limit_state)VALUES(?1,?2,?3,CASE WHEN ?3='healthy' THEN ?4 END,CASE WHEN ?3 NOT IN('healthy','unknown') THEN ?4 END,?5,?6,?7,CASE WHEN ?3='rate_limited' THEN 'backoff_required' END)ON CONFLICT(shop_id,capability)DO UPDATE SET status=excluded.status,last_successful_request_at=COALESCE(excluded.last_successful_request_at,wb_api_health.last_successful_request_at),last_failure_at=COALESCE(excluded.last_failure_at,wb_api_health.last_failure_at),last_error_code=excluded.last_error_code,last_error_message=excluded.last_error_message,latency_ms=excluded.latency_ms,rate_limit_state=excluded.rate_limit_state",params![shop_id,capability,health,now(),code,message,latency]).map_err(|e|e.to_string())?;
    }
    let available: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM wb_api_capabilities WHERE shop_id=?1 AND status='available'",
            [shop_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    c.execute(
        "UPDATE wb_shops SET status=?2,updated_at=?3 WHERE id=?1",
        params![
            shop_id,
            if available > 0 {
                "active"
            } else {
                "connection_error"
            },
            now()
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
fn dashboard(c: &Connection) -> Result<Value, String> {
    let mut stmt=c.prepare("SELECT id,name,seller_type,legal_entity_name,currency,timezone,country,status,created_at,updated_at FROM wb_shops WHERE organization_id=?1 ORDER BY created_at DESC").map_err(|e|e.to_string())?;
    let shops=stmt.query_map([ORG_ID],|r|Ok(json!({"id":r.get::<_,String>(0)?,"name":r.get::<_,String>(1)?,"sellerType":r.get::<_,String>(2)?,"legalEntityName":r.get::<_,Option<String>>(3)?,"currency":r.get::<_,String>(4)?,"timezone":r.get::<_,Option<String>>(5)?,"country":r.get::<_,Option<String>>(6)?,"status":r.get::<_,String>(7)?,"createdAt":r.get::<_,String>(8)?,"updatedAt":r.get::<_,String>(9)?}))).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
    let credentials=query_json(c,"SELECT id,shop_id,masked_hint,status,last_validated_at,last_validation_status,created_at FROM wb_api_credentials ORDER BY created_at DESC",|r|Ok(json!({"id":r.get::<_,String>(0)?,"shopId":r.get::<_,String>(1)?,"maskedHint":r.get::<_,String>(2)?,"status":r.get::<_,String>(3)?,"lastValidatedAt":r.get::<_,Option<String>>(4)?,"lastValidationStatus":r.get::<_,Option<String>>(5)?,"createdAt":r.get::<_,String>(6)?})))?;
    let capabilities=query_json(c,"SELECT shop_id,capability,status,last_checked_at,error_code,error_message FROM wb_api_capabilities ORDER BY capability",|r|Ok(json!({"shopId":r.get::<_,String>(0)?,"capability":r.get::<_,String>(1)?,"status":r.get::<_,String>(2)?,"lastCheckedAt":r.get::<_,Option<String>>(3)?,"errorCode":r.get::<_,Option<String>>(4)?,"errorMessage":r.get::<_,Option<String>>(5)?})))?;
    let health=query_json(c,"SELECT shop_id,capability,status,last_successful_request_at,last_failure_at,last_error_code,last_error_message,latency_ms,rate_limit_state FROM wb_api_health ORDER BY capability",|r|Ok(json!({"shopId":r.get::<_,String>(0)?,"capability":r.get::<_,String>(1)?,"status":r.get::<_,String>(2)?,"lastSuccessfulRequestAt":r.get::<_,Option<String>>(3)?,"lastFailureAt":r.get::<_,Option<String>>(4)?,"lastErrorCode":r.get::<_,Option<String>>(5)?,"lastErrorMessage":r.get::<_,Option<String>>(6)?,"latencyMs":r.get::<_,Option<i64>>(7)?,"rateLimitState":r.get::<_,Option<String>>(8)?})))?;
    let runs=query_json(c,"SELECT r.id,r.sync_job_id,r.shop_id,r.resource_type,r.status,r.started_at,r.finished_at,r.records_processed,r.records_succeeded,r.records_failed,r.error_code,r.error_message,r.retry_count FROM wb_sync_job_runs r ORDER BY COALESCE(r.started_at,'') DESC LIMIT 100",|r|Ok(json!({"id":r.get::<_,String>(0)?,"syncJobId":r.get::<_,String>(1)?,"shopId":r.get::<_,String>(2)?,"resourceType":r.get::<_,String>(3)?,"status":r.get::<_,String>(4)?,"startedAt":r.get::<_,Option<String>>(5)?,"finishedAt":r.get::<_,Option<String>>(6)?,"recordsProcessed":r.get::<_,i64>(7)?,"recordsSucceeded":r.get::<_,i64>(8)?,"recordsFailed":r.get::<_,i64>(9)?,"errorCode":r.get::<_,Option<String>>(10)?,"errorMessage":r.get::<_,Option<String>>(11)?,"retryCount":r.get::<_,i64>(12)?})))?;
    let audits=query_json(c,"SELECT id,shop_id,actor,action,metadata_json,created_at FROM wb_audit_logs ORDER BY id DESC LIMIT 100",|r|Ok(json!({"id":r.get::<_,i64>(0)?,"shopId":r.get::<_,Option<String>>(1)?,"actor":r.get::<_,String>(2)?,"action":r.get::<_,String>(3)?,"metadata":serde_json::from_str::<Value>(&r.get::<_,String>(4)?).unwrap_or(json!({})),"createdAt":r.get::<_,String>(5)?})))?;
    Ok(
        json!({"shops":shops,"credentials":credentials,"capabilities":capabilities,"health":health,"runs":runs,"audits":audits,"resources":RESOURCES,"capabilityNames":CAPABILITIES}),
    )
}
fn query_json<F>(c: &Connection, sql: &str, mut f: F) -> Result<Vec<Value>, String>
where
    F: FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<Value>,
{
    let mut s = c.prepare(sql).map_err(|e| e.to_string())?;
    let rows = s
        .query_map([], |r| f(r))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

#[tauri::command]
pub fn wb_shop_center(
    action: String,
    payload: Option<Value>,
    state: State<AppState>,
) -> Result<Value, String> {
    let p = payload.unwrap_or_else(|| json!({}));
    let c = db(&state)?;
    match action.as_str() {
        "dashboard" => dashboard(&c),
        "create_shop" => {
            let actor = require_role(&p, &["admin"])?;
            let shop = id("wb-shop");
            let t = now();
            let name = field(&p, "name")?;
            c.execute("INSERT INTO wb_shops(id,organization_id,name,seller_type,legal_entity_name,currency,timezone,country,status,created_at,updated_at)VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'connection_error',?9,?9)",params![shop,ORG_ID,name,p.get("sellerType").and_then(Value::as_str).unwrap_or("unknown"),p.get("legalEntityName").and_then(Value::as_str),p.get("currency").and_then(Value::as_str).unwrap_or("RUB"),p.get("timezone").and_then(Value::as_str),p.get("country").and_then(Value::as_str),t]).map_err(|e|e.to_string())?;
            audit(&c, Some(&shop), &actor, "ADD_SHOP", json!({"name":name}))?;
            if let Some(token) = p
                .get("token")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|x| !x.is_empty())
            {
                add_token(&c, &shop, token, &actor, false)?;
            }
            Ok(json!({"shopId":shop}))
        }
        "add_token" | "rotate_token" => {
            let actor = require_role(&p, &["admin"])?;
            let shop = field(&p, "shopId")?;
            let token = field(&p, "token")?;
            add_token(&c, shop, token, &actor, action == "rotate_token")?;
            Ok(json!({"ok":true}))
        }
        "validate" | "check_capabilities" => {
            let actor = require_role(&p, &["admin", "operator"])?;
            let shop = field(&p, "shopId")?;
            check_capabilities(&c, shop)?;
            c.execute("UPDATE wb_api_credentials SET last_validated_at=?2,last_validation_status=CASE WHEN EXISTS(SELECT 1 FROM wb_api_capabilities WHERE shop_id=?1 AND status='available') THEN 'valid' ELSE 'invalid' END,updated_at=?2 WHERE shop_id=?1 AND status='active'",params![shop,now()]).map_err(|e|e.to_string())?;
            audit(&c, Some(shop), &actor, "VALIDATE_TOKEN", json!({}))?;
            dashboard(&c)
        }
        "disable_token" => {
            let actor = require_role(&p, &["admin"])?;
            let cid = field(&p, "credentialId")?;
            let shop: String = c
                .query_row(
                    "SELECT shop_id FROM wb_api_credentials WHERE id=?1",
                    [cid],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            c.execute(
                "UPDATE wb_api_credentials SET status='disabled',updated_at=?2 WHERE id=?1",
                params![cid, now()],
            )
            .map_err(|e| e.to_string())?;
            audit(
                &c,
                Some(&shop),
                &actor,
                "DISABLE_TOKEN",
                json!({"credentialId":cid}),
            )?;
            Ok(json!({"ok":true}))
        }
        "sync" => {
            let actor = require_role(&p, &["admin", "operator"])?;
            let shop = field(&p, "shopId")?;
            active_token(&c, shop)?;
            let resource = field(&p, "resourceType")?;
            let list: Vec<&str> = if resource == "all" {
                RESOURCES.to_vec()
            } else if RESOURCES.contains(&resource) {
                vec![resource]
            } else {
                return Err("未知同步资源".into());
            };
            let mode = p["mode"].as_str().unwrap_or("incremental");
            if !["full", "incremental"].contains(&mode) {
                return Err("同步模式无效".into());
            }
            let tx = c.unchecked_transaction().map_err(|e| e.to_string())?;
            for r in list {
                if c.query_row("SELECT COUNT(*) FROM wb_sync_jobs WHERE shop_id=?1 AND resource_type=?2 AND status IN('pending','running')",params![shop,r],|x|x.get::<_,i64>(0)).map_err(|e|e.to_string())?>0{continue}
                let job = id("job");
                let run = id("run");
                let t = now();
                tx.execute("INSERT INTO wb_sync_jobs(id,shop_id,resource_type,mode,status,created_at)VALUES(?1,?2,?3,?5,'pending',?4)",params![job,shop,r,t,mode]).map_err(|e|e.to_string())?;
                c.execute("INSERT INTO wb_sync_job_runs(id,sync_job_id,shop_id,resource_type,status,records_processed,records_succeeded,records_failed,retry_count)VALUES(?1,?2,?3,?4,'pending',0,0,0,0)",params![run,job,shop,r]).map_err(|e|e.to_string())?;
            }
            tx.commit().map_err(|e| e.to_string())?;
            audit(
                &c,
                Some(shop),
                &actor,
                "START_SYNC",
                json!({"resource":resource}),
            )?;
            dashboard(&c)
        }
        "cancel" => {
            let actor = require_role(&p, &["admin", "operator"])?;
            let run = field(&p, "runId")?;
            let (job, shop): (String, String) = c
                .query_row(
                    "SELECT sync_job_id,shop_id FROM wb_sync_job_runs WHERE id=?1",
                    [run],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(|e| e.to_string())?;
            let t = now();
            c.execute("UPDATE wb_sync_job_runs SET status='cancelled',finished_at=?2 WHERE id=?1 AND status IN('pending','running')",params![run,t]).map_err(|e|e.to_string())?;
            c.execute(
                "UPDATE wb_sync_jobs SET status='cancelled' WHERE id=?1 AND status IN('pending','running')",
                [job],
            )
            .map_err(|e| e.to_string())?;
            audit(&c, Some(&shop), &actor, "CANCEL_SYNC", json!({"runId":run}))?;
            dashboard(&c)
        }
        "retry" => {
            let actor = require_role(&p, &["admin", "operator"])?;
            let old = field(&p, "runId")?;
            let (shop,res,retries):(String,String,i64)=c.query_row("SELECT shop_id,resource_type,retry_count FROM wb_sync_job_runs WHERE id=?1 AND status IN('failed','partial','cancelled')",[old],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|_|"只有失败、部分成功或已取消的任务可以重试".to_string())?;
            let job = id("job");
            let run = id("run");
            c.execute("INSERT INTO wb_sync_jobs(id,shop_id,resource_type,mode,status,created_at)VALUES(?1,?2,?3,'manual','pending',?4)",params![job,shop,res,now()]).map_err(|e|e.to_string())?;
            c.execute("INSERT INTO wb_sync_job_runs(id,sync_job_id,shop_id,resource_type,status,records_processed,records_succeeded,records_failed,retry_count)VALUES(?1,?2,?3,?4,'pending',0,0,0,?5)",params![run,job,shop,res,retries+1]).map_err(|e|e.to_string())?;
            audit(
                &c,
                Some(&shop),
                &actor,
                "RETRY_SYNC",
                json!({"previousRunId":old,"runId":run}),
            )?;
            dashboard(&c)
        }
        _ => Err(format!("未知 WB Shop API Center 操作：{action}")),
    }
}
fn add_token(
    c: &Connection,
    shop: &str,
    token: &str,
    actor: &str,
    rotate: bool,
) -> Result<(), String> {
    if token.len() < 12 {
        return Err("Token 格式无效：长度过短".into());
    }
    if rotate
        && ["content", "marketplace", "promotion"]
            .iter()
            .all(|capability| probe(token, capability).0 != "healthy")
    {
        return Err("新 Token 验证失败，旧 Token 仍保持启用".into());
    }
    let t = now();
    let cid = id("credential");
    let tx = c.unchecked_transaction().map_err(|e| e.to_string())?;
    tx.execute("UPDATE wb_api_credentials SET status='disabled',updated_at=?2 WHERE shop_id=?1 AND status='active'",params![shop,t]).map_err(|e|e.to_string())?;
    tx.execute("INSERT INTO wb_api_credentials(id,shop_id,encrypted_secret,masked_hint,status,created_at,updated_at,last_validation_status)VALUES(?1,?2,?3,?4,'active',?5,?5,'unknown')",params![cid,shop,secrets::protect(token)?,mask(token),t]).map_err(|e|e.to_string())?;
    audit(
        &tx,
        Some(shop),
        actor,
        if rotate { "ROTATE_TOKEN" } else { "ADD_TOKEN" },
        json!({"credentialId":cid,"maskedHint":mask(token)}),
    )?;
    tx.commit().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn token_mask_never_contains_full_secret() {
        let token = "very-secret-token-7X9A";
        let m = mask(token);
        assert!(!m.contains(token));
        assert!(m.ends_with("7X9A"));
    }
    #[test]
    fn permission_is_enforced() {
        assert!(require_role(&json!({"actorRole":"viewer"}), &["admin"]).is_err());
    }
}
