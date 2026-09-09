use crate::{product_master, wb_shop_center, AppState};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Mutex, time::Duration};

pub(crate) fn start(data_dir: PathBuf) {
    std::thread::spawn(move || {
        // A second application window must not reset a live worker's jobs.
        #[cfg(windows)]
        let _guard = {
            use std::hash::{Hash, Hasher};
            let mut hash = std::collections::hash_map::DefaultHasher::new();
            data_dir.to_string_lossy().to_lowercase().hash(&mut hash);
            let name: Vec<u16> = format!("Local\\WberpProductWorker-{:x}\0", hash.finish())
                .encode_utf16()
                .collect();
            let handle = unsafe {
                windows_sys::Win32::System::Threading::CreateMutexW(
                    std::ptr::null(),
                    0,
                    name.as_ptr(),
                )
            };
            if handle.is_null() {
                return;
            }
            if unsafe { windows_sys::Win32::Foundation::GetLastError() }
                == windows_sys::Win32::Foundation::ERROR_ALREADY_EXISTS
            {
                unsafe { windows_sys::Win32::Foundation::CloseHandle(handle) };
                return;
            }
            struct Guard(windows_sys::Win32::Foundation::HANDLE);
            impl Drop for Guard {
                fn drop(&mut self) {
                    unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
                }
            }
            Guard(handle)
        };
        let state = AppState {
            data_dir,
            active_shop_id: Mutex::new(String::new()),
        };
        if let Ok(c) = wb_shop_center::db(&state) {
            let _=c.execute("UPDATE wb_sync_job_runs SET status='failed',finished_at=CURRENT_TIMESTAMP,error_message='应用重启中断，请重试' WHERE status='running'",[]);
            let _ = c.execute(
                "UPDATE wb_sync_jobs SET status='failed' WHERE status='running'",
                [],
            );
        }
        loop {
            if let Ok(c) = wb_shop_center::db(&state) {
                if product_master::ensure(&c).is_ok() {
                    let next=c.query_row("SELECT r.id,r.sync_job_id,r.shop_id,r.resource_type FROM wb_sync_job_runs r JOIN wb_sync_jobs j ON j.id=r.sync_job_id WHERE r.status='pending' AND j.status='pending' ORDER BY j.created_at LIMIT 1",[],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?))).optional();
                    if let Ok(Some((run, job, shop, resource))) = next {
                        if c.execute("UPDATE wb_sync_job_runs SET status='running',started_at=CURRENT_TIMESTAMP WHERE id=?1 AND status='pending'",[&run]).unwrap_or(0)==1{
     let _=c.execute("UPDATE wb_sync_jobs SET status='running' WHERE id=?1",[&job]);
     let result=if resource=="products"{sync(&c,&shop,&run)}else{Err("RESOURCE_UNAVAILABLE: 尚未接入该资源执行器".into())};
     let (status,message)=match result{Ok(())=>("success",None),Err(e)=>{let done:i64=c.query_row("SELECT records_succeeded FROM wb_sync_job_runs WHERE id=?1",[&run],|r|r.get(0)).unwrap_or(0);(if done>0{"partial"}else{"failed"},Some(e))}};
     let _=c.execute("UPDATE wb_sync_job_runs SET status=?2,finished_at=CURRENT_TIMESTAMP,error_message=?3,last_successful_sync_at=CASE WHEN ?2='success' THEN CURRENT_TIMESTAMP ELSE last_successful_sync_at END WHERE id=?1 AND status!='cancelled'",params![run,status,message]);
     let _=c.execute("UPDATE wb_sync_jobs SET status=?2 WHERE id=?1 AND status!='cancelled'",params![job,status]);
     if let Some(message)=message{let _=c.execute("INSERT INTO wb_sync_errors(sync_job_run_id,shop_id,resource_type,error_code,error_message,retryable,created_at)VALUES(?1,?2,?3,'SYNC_FAILED',?4,0,CURRENT_TIMESTAMP)",params![run,shop,resource,message]);}
    }
                    }
                }
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    });
}

fn sync(c: &rusqlite::Connection, shop: &str, run: &str) -> Result<(), String> {
    let active = sync_cards(c, shop, run, false);
    if active.as_ref().err().is_some_and(|e| e == "SYNC_CANCELLED") {
        return active;
    }
    let trash = sync_cards(c, shop, run, true);
    match (active, trash) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(a), Err(b)) => Err(format!("商品列表：{a}；回收站：{b}")),
        (Err(a), _) => Err(a),
        (_, Err(b)) => Err(format!("回收站：{b}")),
    }
}
fn sync_cards(c: &rusqlite::Connection, shop: &str, run: &str, trash: bool) -> Result<(), String> {
    let (_, token) = wb_shop_center::active_token(c, shop)?;
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(30))
        .build();
    let saved:Option<String>=c.query_row("SELECT last_cursor FROM wb_sync_job_runs WHERE shop_id=?1 AND resource_type='products' AND status='success' AND last_cursor IS NOT NULL ORDER BY finished_at DESC LIMIT 1",[shop],|r|r.get(0)).optional().map_err(|e|e.to_string())?.flatten();
    let mode:String=c.query_row("SELECT j.mode FROM wb_sync_jobs j JOIN wb_sync_job_runs r ON r.sync_job_id=j.id WHERE r.id=?1",[run],|r|r.get(0)).map_err(|e|e.to_string())?;
    let saved = if trash || mode == "full" { None } else { saved };
    let mut cursor = saved
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .filter(|v| !v["updatedAt"].is_null() && !v["nmID"].is_null())
        .unwrap_or(json!({"limit":100}));
    let started = std::time::Instant::now();
    loop {
        if started.elapsed() > Duration::from_secs(1800) {
            return Err("同步超过 30 分钟，请重试".into());
        }
        let cancelled: bool = c
            .query_row(
                "SELECT status='cancelled' FROM wb_sync_job_runs WHERE id=?1",
                [run],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if cancelled {
            return Err("SYNC_CANCELLED".into());
        }
        let mut response = None;
        for attempt in 0..4 {
            let request = if trash {
                json!({"settings":{"sort":{"ascending":true},"cursor":cursor}})
            } else {
                json!({"settings":{"sort":{"ascending":true},"filter":{"withPhoto":-1},"cursor":cursor}})
            };
            match agent
                .post(if trash {
                    "https://content-api.wildberries.ru/content/v2/get/cards/trash"
                } else {
                    "https://content-api.wildberries.ru/content/v2/get/cards/list"
                })
                .set("Authorization", &token)
                .set("Content-Type", "application/json")
                .send_string(&request.to_string())
            {
                Ok(r) => {
                    response = Some(r.into_string().map_err(|_| "读取 WB 响应失败")?);
                    break;
                }
                Err(e) => {
                    let (code, retry) = match e {
                        ureq::Error::Status(code, _) => (code, code == 429 || code >= 500),
                        ureq::Error::Transport(_) => (0, true),
                    };
                    if !retry || attempt == 3 {
                        return Err(format!("WB_CONTENT_REQUEST_FAILED: HTTP {code}"));
                    }
                    c.execute("UPDATE wb_sync_job_runs SET retry_count=retry_count+1,error_message=?2 WHERE id=?1",params![run,format!("HTTP {code}: 等待重试")]).map_err(|e|e.to_string())?;
                    std::thread::sleep(Duration::from_secs(2u64.pow(attempt + 1)));
                }
            }
        }
        let raw = response.ok_or("无 WB 响应")?;
        // Persist the response before parsing so malformed batches remain inspectable.
        c.execute(
            "INSERT INTO pm_raw(shop_id,run_id,payload)VALUES(?1,?2,?3)",
            params![shop, run, raw],
        )
        .map_err(|e| e.to_string())?;
        let raw_id = c.last_insert_rowid();
        let payload: Value = serde_json::from_str(&raw).map_err(|_| "WB 响应 JSON 无效")?;
        let cards = payload["cards"]
            .as_array()
            .ok_or("WB 响应缺少 cards 数组")?;
        for card in cards {
            let tx = c.unchecked_transaction().map_err(|e| e.to_string())?;
            match if trash {
                product_master::normalize_status(&tx, shop, raw_id, card, "inactive")
            } else {
                product_master::normalize(&tx, shop, raw_id, card)
            } {
                Ok(_) => {
                    tx.execute("UPDATE wb_sync_job_runs SET records_processed=records_processed+1,records_succeeded=records_succeeded+1 WHERE id=?1",[run]).map_err(|e|e.to_string())?;
                    tx.commit().map_err(|e| e.to_string())?;
                }
                Err(message) => {
                    drop(tx);
                    c.execute("UPDATE wb_sync_job_runs SET records_processed=records_processed+1,records_failed=records_failed+1 WHERE id=?1",[run]).map_err(|e|e.to_string())?;
                    c.execute("INSERT INTO wb_sync_errors(sync_job_run_id,shop_id,resource_type,error_code,error_message,created_at)VALUES(?1,?2,'products','NORMALIZE_FAILED',?3,CURRENT_TIMESTAMP)",params![run,shop,message]).map_err(|e|e.to_string())?;
                }
            }
        }
        let key = if trash { "trashedAt" } else { "updatedAt" };
        let mut next = json!({"limit":100,"nmID":payload["cursor"]["nmID"]});
        next[key] = payload["cursor"][key].clone();
        let checkpoint = if next[key].is_null() || next["nmID"].is_null() {
            &cursor
        } else {
            &next
        };
        if !trash {
            c.execute("UPDATE wb_sync_job_runs SET last_cursor=?2,last_synced_at=CURRENT_TIMESTAMP WHERE id=?1",params![run,checkpoint.to_string()]).map_err(|e|e.to_string())?;
        }
        if cards.len() < 100 {
            break;
        }
        if next == cursor || next[key].is_null() || next["nmID"].is_null() {
            return Err("分页游标缺失或未前进".into());
        }
        cursor = next;
        std::thread::sleep(Duration::from_millis(650));
    }
    let failed: i64 = c
        .query_row(
            "SELECT records_failed FROM wb_sync_job_runs WHERE id=?1",
            [run],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if failed > 0 {
        Err(format!("{failed} 条商品规范化失败，成功数据已保留"))
    } else {
        Ok(())
    }
}
