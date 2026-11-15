use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use tauri::State;
use super::AppState;

fn run(c: &Connection, command: &str, payload: Value) -> Result<Value, String> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS chains(id TEXT PRIMARY KEY, order_no TEXT NOT NULL UNIQUE, body TEXT NOT NULL, revision INTEGER NOT NULL DEFAULT 1);").map_err(|e|e.to_string())?;
    match command {
        "list" => {
            let mut q=c.prepare("SELECT body, revision FROM chains ORDER BY rowid DESC").map_err(|e|e.to_string())?;
            let rows=q.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?))).map_err(|e|e.to_string())?;
            let mut result=Vec::new();
            for row in rows {let (body,rev)=row.map_err(|e|e.to_string())?;let mut v:Value=serde_json::from_str(&body).map_err(|e|e.to_string())?;v["revision"]=json!(rev);result.push(v);}
            Ok(json!(result))
        },
        "save" => {
            let id=payload["id"].as_str().ok_or("缺少记录 ID")?;
            let order=payload["orderNo"].as_str().unwrap_or("").trim();
            if id.is_empty() || order.is_empty() || order.len()>200 {return Err("请填写采购单号（最多 200 字节）".into());}
            let rev=payload["revision"].as_i64().unwrap_or(0);
            let current:Option<i64>=c.query_row("SELECT revision FROM chains WHERE id=?1",[id],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
            if current.unwrap_or(0)!=rev {return Err("记录已在其他窗口更新，请刷新后重试".into());}
            let body=payload.to_string();
            if body.len()>40*1024*1024 {return Err("单条链路资料总量请控制在 30 MB 内".into());}
            let changed=if current.is_some(){c.execute("UPDATE chains SET order_no=?1,body=?2,revision=revision+1 WHERE id=?3 AND revision=?4",params![order,body,id,rev])}else{c.execute("INSERT INTO chains(id,order_no,body) VALUES(?1,?2,?3)",params![id,order,body])}.map_err(|e|format!("保存失败（采购单号不能重复）：{e}"))?;
            if changed!=1{return Err("记录已变化，请刷新".into());}
            Ok(json!({"revision":rev+1}))
        },
        "delete" => {let n=c.execute("DELETE FROM chains WHERE id=?1 AND revision=?2",params![payload["id"].as_str().ok_or("缺少 ID")?,payload["revision"].as_i64().ok_or("缺少版本")?]).map_err(|e|e.to_string())?;if n!=1{return Err("记录已变化，请刷新".into());}Ok(json!(true))},
        _=>Err("未知操作".into())
    }
}

#[tauri::command]
pub async fn procurement_board_command(state:State<'_,AppState>,command:String,payload:Value)->Result<Value,String>{
    let path=state.data_dir.join("procurement-board.sqlite");
    tauri::async_runtime::spawn_blocking(move||{let c=Connection::open(path).map_err(|e|e.to_string())?;c.busy_timeout(std::time::Duration::from_secs(5)).map_err(|e|e.to_string())?;run(&c,&command,payload)}).await.map_err(|e|e.to_string())?
}

#[cfg(test)] mod tests {
 use super::*;
 #[test] fn persistence_conflicts_and_deletion(){let c=Connection::open_in_memory().unwrap();let mut p=json!({"id":"a","orderNo":"CG-1","revision":0,"docs":[{"stage":2,"data":"abc"}]});assert_eq!(run(&c,"save",p.clone()).unwrap()["revision"],1);assert!(run(&c,"save",p.clone()).is_err());p["revision"]=json!(1);p["docs"][0]["stage"]=json!(4);run(&c,"save",p.clone()).unwrap();let rows=run(&c,"list",Value::Null).unwrap();assert_eq!(rows[0]["docs"][0]["stage"],4);assert_eq!(rows[0]["docs"][0]["data"],"abc");assert!(run(&c,"delete",p.clone()).is_err());p["revision"]=json!(2);run(&c,"delete",p).unwrap();assert_eq!(run(&c,"list",Value::Null).unwrap(),json!([]));}
 #[test] fn rejects_duplicate_order(){let c=Connection::open_in_memory().unwrap();run(&c,"save",json!({"id":"a","orderNo":"CG","revision":0})).unwrap();assert!(run(&c,"save",json!({"id":"b","orderNo":"CG","revision":0})).is_err());}
}
