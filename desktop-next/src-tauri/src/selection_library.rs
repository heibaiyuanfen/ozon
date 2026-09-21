use crate::{db, AppState};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tauri::State;

pub(crate) fn ensure(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS selection_categories(
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE,
            note TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
        CREATE TABLE IF NOT EXISTS selection_items(
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            category_id INTEGER,
            product_name TEXT NOT NULL,
            image_url TEXT NOT NULL DEFAULT '',
            target_market TEXT NOT NULL DEFAULT '',
            competitor_url TEXT NOT NULL DEFAULT '',
            purchase_url TEXT NOT NULL DEFAULT '',
            competitor_price REAL,
            purchase_price REAL,
            target_price REAL,
            estimated_monthly_sales INTEGER,
            weight_kg REAL,
            length_cm REAL,
            width_cm REAL,
            height_cm REAL,
            status TEXT NOT NULL DEFAULT '待调研',
            priority TEXT NOT NULL DEFAULT '中',
            tags TEXT NOT NULL DEFAULT '',
            advantages TEXT NOT NULL DEFAULT '',
            risks TEXT NOT NULL DEFAULT '',
            notes TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY(category_id) REFERENCES selection_categories(id) ON DELETE SET NULL
        );
        CREATE INDEX IF NOT EXISTS idx_selection_items_category ON selection_items(category_id);
        CREATE INDEX IF NOT EXISTS idx_selection_items_status ON selection_items(status);",
    )
    .map_err(|error| error.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SelectionCategory {
    id: i64,
    name: String,
    note: String,
    item_count: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SelectionItem {
    id: i64,
    category_id: Option<i64>,
    category_name: String,
    product_name: String,
    image_url: String,
    target_market: String,
    competitor_url: String,
    purchase_url: String,
    competitor_price: Option<f64>,
    purchase_price: Option<f64>,
    target_price: Option<f64>,
    estimated_monthly_sales: Option<i64>,
    weight_kg: Option<f64>,
    length_cm: Option<f64>,
    width_cm: Option<f64>,
    height_cm: Option<f64>,
    status: String,
    priority: String,
    tags: String,
    advantages: String,
    risks: String,
    notes: String,
    created_at: String,
    updated_at: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SelectionCategoryInput {
    id: Option<i64>,
    name: String,
    note: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SelectionItemInput {
    id: Option<i64>,
    category_id: Option<i64>,
    product_name: String,
    image_url: String,
    target_market: String,
    competitor_url: String,
    purchase_url: String,
    competitor_price: Option<f64>,
    purchase_price: Option<f64>,
    target_price: Option<f64>,
    estimated_monthly_sales: Option<i64>,
    weight_kg: Option<f64>,
    length_cm: Option<f64>,
    width_cm: Option<f64>,
    height_cm: Option<f64>,
    status: String,
    priority: String,
    tags: String,
    advantages: String,
    risks: String,
    notes: String,
}

fn valid_optional_number(value: Option<f64>) -> bool {
    value.is_none_or(|number| number.is_finite() && number >= 0.0)
}

#[tauri::command]
pub(crate) fn selection_categories(
    state: State<AppState>,
) -> Result<Vec<SelectionCategory>, String> {
    let conn = db(&state)?;
    let mut statement = conn
        .prepare("SELECT c.id,c.name,c.note,COUNT(i.id) FROM selection_categories c LEFT JOIN selection_items i ON i.category_id=c.id GROUP BY c.id ORDER BY c.name")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(SelectionCategory {
                id: row.get(0)?,
                name: row.get(1)?,
                note: row.get(2)?,
                item_count: row.get(3)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(rows)
}

#[tauri::command]
pub(crate) fn save_selection_category(
    input: SelectionCategoryInput,
    state: State<AppState>,
) -> Result<i64, String> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err("类目名称不能为空".into());
    }
    let conn = db(&state)?;
    if let Some(id) = input.id {
        conn.execute(
            "UPDATE selection_categories SET name=?1,note=?2,updated_at=CURRENT_TIMESTAMP WHERE id=?3",
            params![name, input.note.trim(), id],
        )
        .map_err(|error| format!("保存类目失败：{error}"))?;
        Ok(id)
    } else {
        conn.execute(
            "INSERT INTO selection_categories(name,note) VALUES(?1,?2)",
            params![name, input.note.trim()],
        )
        .map_err(|error| format!("保存类目失败：{error}"))?;
        Ok(conn.last_insert_rowid())
    }
}

#[tauri::command]
pub(crate) fn delete_selection_category(id: i64, state: State<AppState>) -> Result<(), String> {
    let conn = db(&state)?;
    conn.execute(
        "UPDATE selection_items SET category_id=NULL WHERE category_id=?1",
        [id],
    )
    .map_err(|error| error.to_string())?;
    conn.execute("DELETE FROM selection_categories WHERE id=?1", [id])
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub(crate) fn selection_items(
    query: String,
    category_id: Option<i64>,
    status: String,
    state: State<AppState>,
) -> Result<Vec<SelectionItem>, String> {
    let conn = db(&state)?;
    let pattern = format!("%{}%", query.trim());
    let mut statement = conn.prepare("SELECT i.id,i.category_id,COALESCE(c.name,''),i.product_name,i.image_url,i.target_market,i.competitor_url,i.purchase_url,i.competitor_price,i.purchase_price,i.target_price,i.estimated_monthly_sales,i.weight_kg,i.length_cm,i.width_cm,i.height_cm,i.status,i.priority,i.tags,i.advantages,i.risks,i.notes,i.created_at,i.updated_at FROM selection_items i LEFT JOIN selection_categories c ON c.id=i.category_id WHERE (?1='%%' OR i.product_name LIKE ?1 OR i.tags LIKE ?1 OR i.target_market LIKE ?1 OR i.notes LIKE ?1) AND (?2 IS NULL OR i.category_id=?2) AND (?3='' OR i.status=?3) ORDER BY CASE i.priority WHEN '高' THEN 0 WHEN '中' THEN 1 ELSE 2 END,i.updated_at DESC,i.id DESC").map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![pattern, category_id, status], |row| {
            Ok(SelectionItem {
                id: row.get(0)?,
                category_id: row.get(1)?,
                category_name: row.get(2)?,
                product_name: row.get(3)?,
                image_url: row.get(4)?,
                target_market: row.get(5)?,
                competitor_url: row.get(6)?,
                purchase_url: row.get(7)?,
                competitor_price: row.get(8)?,
                purchase_price: row.get(9)?,
                target_price: row.get(10)?,
                estimated_monthly_sales: row.get(11)?,
                weight_kg: row.get(12)?,
                length_cm: row.get(13)?,
                width_cm: row.get(14)?,
                height_cm: row.get(15)?,
                status: row.get(16)?,
                priority: row.get(17)?,
                tags: row.get(18)?,
                advantages: row.get(19)?,
                risks: row.get(20)?,
                notes: row.get(21)?,
                created_at: row.get(22)?,
                updated_at: row.get(23)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(rows)
}

#[tauri::command]
pub(crate) fn save_selection_item(
    input: SelectionItemInput,
    state: State<AppState>,
) -> Result<i64, String> {
    if input.product_name.trim().is_empty() {
        return Err("产品名称不能为空".into());
    }
    if !valid_optional_number(input.competitor_price)
        || !valid_optional_number(input.purchase_price)
        || !valid_optional_number(input.target_price)
        || !valid_optional_number(input.weight_kg)
        || !valid_optional_number(input.length_cm)
        || !valid_optional_number(input.width_cm)
        || !valid_optional_number(input.height_cm)
    {
        return Err("价格、重量和尺寸不能为负数".into());
    }
    if input.estimated_monthly_sales.is_some_and(|value| value < 0) {
        return Err("预计月销量不能为负数".into());
    }
    for (label, url) in [
        ("竞品链接", input.competitor_url.trim()),
        ("采购链接", input.purchase_url.trim()),
        ("图片链接", input.image_url.trim()),
    ] {
        if !url.is_empty() && !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(format!("{label}必须以 http:// 或 https:// 开头"));
        }
    }
    let conn = db(&state)?;
    let values = params![
        input.category_id,
        input.product_name.trim(),
        input.image_url.trim(),
        input.target_market.trim(),
        input.competitor_url.trim(),
        input.purchase_url.trim(),
        input.competitor_price,
        input.purchase_price,
        input.target_price,
        input.estimated_monthly_sales,
        input.weight_kg,
        input.length_cm,
        input.width_cm,
        input.height_cm,
        input.status.trim(),
        input.priority.trim(),
        input.tags.trim(),
        input.advantages.trim(),
        input.risks.trim(),
        input.notes.trim()
    ];
    if let Some(id) = input.id {
        conn.execute("UPDATE selection_items SET category_id=?1,product_name=?2,image_url=?3,target_market=?4,competitor_url=?5,purchase_url=?6,competitor_price=?7,purchase_price=?8,target_price=?9,estimated_monthly_sales=?10,weight_kg=?11,length_cm=?12,width_cm=?13,height_cm=?14,status=?15,priority=?16,tags=?17,advantages=?18,risks=?19,notes=?20,updated_at=CURRENT_TIMESTAMP WHERE id=?21", params![input.category_id,input.product_name.trim(),input.image_url.trim(),input.target_market.trim(),input.competitor_url.trim(),input.purchase_url.trim(),input.competitor_price,input.purchase_price,input.target_price,input.estimated_monthly_sales,input.weight_kg,input.length_cm,input.width_cm,input.height_cm,input.status.trim(),input.priority.trim(),input.tags.trim(),input.advantages.trim(),input.risks.trim(),input.notes.trim(),id]).map_err(|error|error.to_string())?;
        Ok(id)
    } else {
        conn.execute("INSERT INTO selection_items(category_id,product_name,image_url,target_market,competitor_url,purchase_url,competitor_price,purchase_price,target_price,estimated_monthly_sales,weight_kg,length_cm,width_cm,height_cm,status,priority,tags,advantages,risks,notes) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)",values).map_err(|error|error.to_string())?;
        Ok(conn.last_insert_rowid())
    }
}

#[tauri::command]
pub(crate) fn delete_selection_item(id: i64, state: State<AppState>) -> Result<(), String> {
    db(&state)?
        .execute("DELETE FROM selection_items WHERE id=?1", [id])
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::ensure;
    #[test]
    fn creates_selection_library_tables() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        ensure(&conn).unwrap();
        let count:i64=conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN('selection_categories','selection_items')",[],|row|row.get(0)).unwrap();
        assert_eq!(count, 2);
    }
}
