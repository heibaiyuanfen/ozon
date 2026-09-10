use chrono::{Local, NaiveDate, NaiveTime};
use rusqlite::params;
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::State;

use super::{background_state, db, AppState};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskInput {
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default = "normal")]
    priority: String,
    #[serde(default = "default_due")]
    due_time: String,
    #[serde(default = "daily")]
    repeat_mode: String,
}

fn normal() -> String {
    "normal".into()
}
fn daily() -> String {
    "daily".into()
}
fn default_due() -> String {
    "18:00".into()
}

pub(super) fn ensure(c: &rusqlite::Connection) -> Result<(), String> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS daily_tasks(
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            title TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            priority TEXT NOT NULL DEFAULT 'normal',
            due_time TEXT NOT NULL DEFAULT '18:00',
            repeat_mode TEXT NOT NULL DEFAULT 'daily',
            active INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
        CREATE TABLE IF NOT EXISTS daily_task_logs(
            task_id INTEGER NOT NULL,
            day TEXT NOT NULL,
            progress INTEGER NOT NULL DEFAULT 0,
            status TEXT NOT NULL DEFAULT 'pending',
            note TEXT NOT NULL DEFAULT '',
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            completed_at TEXT,
            PRIMARY KEY(task_id,day),
            FOREIGN KEY(task_id) REFERENCES daily_tasks(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_daily_task_logs_day ON daily_task_logs(day,status);",
    )
    .map_err(|e| e.to_string())
}

fn valid_day(value: &str) -> Result<(), String> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map(|_| ())
        .map_err(|_| "日期格式必须为 YYYY-MM-DD".into())
}

fn validate(input: &TaskInput) -> Result<(), String> {
    if input.title.trim().is_empty() {
        return Err("任务名称不能为空".into());
    }
    if input.title.chars().count() > 100 {
        return Err("任务名称不能超过 100 个字符".into());
    }
    if !matches!(
        input.priority.as_str(),
        "low" | "normal" | "high" | "urgent"
    ) {
        return Err("优先级无效".into());
    }
    if !matches!(input.repeat_mode.as_str(), "none" | "daily" | "weekdays") {
        return Err("重复方式无效".into());
    }
    NaiveTime::parse_from_str(&input.due_time, "%H:%M").map_err(|_| "截止时间无效".to_string())?;
    Ok(())
}

fn list(c: &rusqlite::Connection, day: &str) -> Result<Value, String> {
    valid_day(day)?;
    let date = NaiveDate::parse_from_str(day, "%Y-%m-%d").unwrap();
    let weekday = date.format("%u").to_string().parse::<u8>().unwrap_or(7);
    let mut stmt=c.prepare(
        "SELECT t.id,t.title,t.description,t.priority,t.due_time,t.repeat_mode,t.active,t.created_at,
                COALESCE(l.progress,0),COALESCE(l.status,'pending'),COALESCE(l.note,''),l.updated_at,l.completed_at
         FROM daily_tasks t LEFT JOIN daily_task_logs l ON l.task_id=t.id AND l.day=?1
         WHERE t.active=1 AND date(t.created_at)<=?1
           AND (t.repeat_mode='daily' OR (t.repeat_mode='weekdays' AND ?2<=5) OR l.task_id IS NOT NULL OR (t.repeat_mode='none' AND date(t.created_at)=?1))
         ORDER BY CASE t.priority WHEN 'urgent' THEN 0 WHEN 'high' THEN 1 WHEN 'normal' THEN 2 ELSE 3 END,t.due_time,t.id"
    ).map_err(|e|e.to_string())?;
    let rows=stmt.query_map(params![day,weekday],|r|Ok(json!({
        "id":r.get::<_,i64>(0)?,"title":r.get::<_,String>(1)?,"description":r.get::<_,String>(2)?,
        "priority":r.get::<_,String>(3)?,"dueTime":r.get::<_,String>(4)?,"repeatMode":r.get::<_,String>(5)?,
        "active":r.get::<_,i64>(6)?!=0,"createdAt":r.get::<_,String>(7)?,"progress":r.get::<_,i64>(8)?,
        "status":r.get::<_,String>(9)?,"note":r.get::<_,String>(10)?,"updatedAt":r.get::<_,Option<String>>(11)?,
        "completedAt":r.get::<_,Option<String>>(12)?
    }))).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
    let now = Local::now();
    let today = now.date_naive().to_string();
    let current_time = now.format("%H:%M").to_string();
    let enriched: Vec<_> = rows
        .into_iter()
        .map(|mut row| {
            let status = row["status"].as_str().unwrap_or("pending");
            let due = row["dueTime"].as_str().unwrap_or("18:00");
            let overdue = status != "completed"
                && (day < today.as_str() || (day == today && due < current_time.as_str()));
            let due_soon = status != "completed" && day == today && !overdue && {
                let due_time = NaiveTime::parse_from_str(due, "%H:%M").ok();
                due_time.is_some_and(|t| (t - now.time()).num_minutes() <= 60)
            };
            row["overdue"] = json!(overdue);
            row["dueSoon"] = json!(due_soon);
            row
        })
        .collect();
    let completed = enriched
        .iter()
        .filter(|x| x["status"] == "completed")
        .count();
    let overdue = enriched.iter().filter(|x| x["overdue"] == true).count();
    let progress = if enriched.is_empty() {
        0
    } else {
        enriched
            .iter()
            .map(|x| x["progress"].as_i64().unwrap_or(0))
            .sum::<i64>()
            / enriched.len() as i64
    };
    Ok(
        json!({"day":day,"tasks":enriched,"summary":{"total":enriched.len(),"completed":completed,"overdue":overdue,"progress":progress}}),
    )
}

#[tauri::command]
pub async fn daily_task_command(
    state: State<'_, AppState>,
    command: String,
    day: Option<String>,
    id: Option<i64>,
    payload: Option<Value>,
) -> Result<Value, String> {
    let command = command.clone();
    let day = day.unwrap_or_else(|| Local::now().date_naive().to_string());
    let state = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move||{
        let c=db(&state)?; ensure(&c)?;
        match command.as_str() {
            "list"=>list(&c,&day),
            "create"=>{
                let input:TaskInput=serde_json::from_value(payload.unwrap_or(json!({}))).map_err(|e|e.to_string())?; validate(&input)?;
                c.execute("INSERT INTO daily_tasks(title,description,priority,due_time,repeat_mode)VALUES(?1,?2,?3,?4,?5)",params![input.title.trim(),input.description.trim(),input.priority,input.due_time,input.repeat_mode]).map_err(|e|e.to_string())?;
                list(&c,&day)
            },
            "update"=>{
                let id=id.ok_or("缺少任务 ID")?; let input:TaskInput=serde_json::from_value(payload.unwrap_or(json!({}))).map_err(|e|e.to_string())?; validate(&input)?;
                c.execute("UPDATE daily_tasks SET title=?1,description=?2,priority=?3,due_time=?4,repeat_mode=?5,updated_at=CURRENT_TIMESTAMP WHERE id=?6",params![input.title.trim(),input.description.trim(),input.priority,input.due_time,input.repeat_mode,id]).map_err(|e|e.to_string())?;
                list(&c,&day)
            },
            "progress"=>{
                valid_day(&day)?; let id=id.ok_or("缺少任务 ID")?; let p=payload.unwrap_or(json!({})); let progress=p["progress"].as_i64().unwrap_or(0).clamp(0,100); let note=p["note"].as_str().unwrap_or(""); let status=if progress>=100{"completed"}else if progress>0{"in_progress"}else{"pending"};
                c.execute("INSERT INTO daily_task_logs(task_id,day,progress,status,note,updated_at,completed_at)VALUES(?1,?2,?3,?4,?5,CURRENT_TIMESTAMP,CASE WHEN ?3=100 THEN CURRENT_TIMESTAMP END) ON CONFLICT(task_id,day) DO UPDATE SET progress=excluded.progress,status=excluded.status,note=excluded.note,updated_at=CURRENT_TIMESTAMP,completed_at=CASE WHEN excluded.progress=100 THEN COALESCE(daily_task_logs.completed_at,CURRENT_TIMESTAMP) ELSE NULL END",params![id,day,progress,status,note]).map_err(|e|e.to_string())?;
                list(&c,&day)
            },
            "archive"=>{let id=id.ok_or("缺少任务 ID")?;c.execute("UPDATE daily_tasks SET active=0,updated_at=CURRENT_TIMESTAMP WHERE id=?1",[id]).map_err(|e|e.to_string())?;list(&c,&day)},
            "history"=>{let id=id.ok_or("缺少任务 ID")?;let mut s=c.prepare("SELECT day,progress,status,note,updated_at,completed_at FROM daily_task_logs WHERE task_id=?1 ORDER BY day DESC LIMIT 90").map_err(|e|e.to_string())?;let rows=s.query_map([id],|r|Ok(json!({"day":r.get::<_,String>(0)?,"progress":r.get::<_,i64>(1)?,"status":r.get::<_,String>(2)?,"note":r.get::<_,String>(3)?,"updatedAt":r.get::<_,String>(4)?,"completedAt":r.get::<_,Option<String>>(5)?}))).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;Ok(json!({"history":rows}))},
            _=>Err("不支持的任务命令".into())
        }
    }).await.map_err(|e|e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_task_fields() {
        assert!(validate(&TaskInput {
            title: "复盘广告".into(),
            description: "".into(),
            priority: "high".into(),
            due_time: "18:30".into(),
            repeat_mode: "daily".into()
        })
        .is_ok());
    }
    #[test]
    fn rejects_bad_due_time() {
        assert!(validate(&TaskInput {
            title: "x".into(),
            description: "".into(),
            priority: "normal".into(),
            due_time: "25:00".into(),
            repeat_mode: "daily".into()
        })
        .is_err());
    }
    #[test]
    fn progress_summary_is_preserved() {
        let c = rusqlite::Connection::open_in_memory().unwrap();
        ensure(&c).unwrap();
        c.execute("INSERT INTO daily_tasks(title)VALUES('A')", [])
            .unwrap();
        c.execute("UPDATE daily_tasks SET created_at='2026-09-08'", [])
            .unwrap();
        c.execute("INSERT INTO daily_task_logs(task_id,day,progress,status)VALUES(1,'2026-09-08',60,'in_progress')",[]).unwrap();
        let v = list(&c, "2026-09-08").unwrap();
        assert_eq!(v["summary"]["progress"], 60);
    }
}
