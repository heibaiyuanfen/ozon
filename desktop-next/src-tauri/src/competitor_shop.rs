use crate::{db, AppState};
use calamine::{open_workbook_auto, Data, Reader};
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::{collections::HashMap, path::Path};
use tauri::State;

pub(crate) fn ensure(c: &Connection) -> Result<(), String> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS competitor_shop_imports(id INTEGER PRIMARY KEY AUTOINCREMENT,shop_name TEXT NOT NULL,source_file TEXT NOT NULL,product_count INTEGER NOT NULL DEFAULT 0,imported_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
    CREATE TABLE IF NOT EXISTS competitor_shop_products(id INTEGER PRIMARY KEY AUTOINCREMENT,shop_name TEXT NOT NULL,sku TEXT NOT NULL,title TEXT NOT NULL DEFAULT '',image_url TEXT NOT NULL DEFAULT '',listing_url TEXT NOT NULL DEFAULT '',brand TEXT NOT NULL DEFAULT '',category TEXT NOT NULL DEFAULT '',sales_method TEXT NOT NULL DEFAULT '',price REAL NOT NULL DEFAULT 0,sales REAL NOT NULL DEFAULT 0,revenue REAL NOT NULL DEFAULT 0,sales_growth REAL NOT NULL DEFAULT 0,revenue_growth REAL NOT NULL DEFAULT 0,gross_margin REAL NOT NULL DEFAULT 0,total_impressions REAL NOT NULL DEFAULT 0,impressions REAL NOT NULL DEFAULT 0,product_views REAL NOT NULL DEFAULT 0,view_to_cart REAL NOT NULL DEFAULT 0,search_to_cart REAL NOT NULL DEFAULT 0,average_discount REAL NOT NULL DEFAULT 0,promo_sales_share REAL NOT NULL DEFAULT 0,impression_to_order REAL NOT NULL DEFAULT 0,order_conversion REAL NOT NULL DEFAULT 0,ad_cost_share REAL NOT NULL DEFAULT 0,estimated_ad_cost REAL NOT NULL DEFAULT 0,return_cancel_rate REAL NOT NULL DEFAULT 0,lost_revenue REAL NOT NULL DEFAULT 0,rating REAL NOT NULL DEFAULT 0,rating_count REAL NOT NULL DEFAULT 0,seller_type TEXT NOT NULL DEFAULT '',fulfillment TEXT NOT NULL DEFAULT '',weight_kg REAL,launch_age TEXT NOT NULL DEFAULT '',source_file TEXT NOT NULL DEFAULT '',updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,UNIQUE(shop_name,sku));
    CREATE INDEX IF NOT EXISTS idx_competitor_shop_products_shop ON competitor_shop_products(shop_name);CREATE INDEX IF NOT EXISTS idx_competitor_shop_products_sales ON competitor_shop_products(sales DESC);").map_err(|e|e.to_string())
}

fn text(v: Option<&Data>) -> String {
    match v {
        Some(Data::String(x)) => x.trim().to_string(),
        Some(Data::Float(x)) if x.fract() == 0.0 => format!("{x:.0}"),
        Some(x) => x.to_string().trim().to_string(),
        None => String::new(),
    }
}
fn number(v: Option<&Data>) -> f64 {
    let s = text(v).replace(',', ".");
    let cleaned: String = s
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    cleaned.parse().unwrap_or(0.0)
}
fn percent(v: Option<&Data>) -> f64 {
    number(v)
}
fn weight(v: Option<&Data>) -> Option<f64> {
    let s = text(v);
    if s.is_empty() {
        None
    } else {
        let n = number(v);
        Some(if s.to_lowercase().contains("kg") || s.contains("кг") {
            n
        } else {
            n / 1000.0
        })
    }
}
fn web_url(value: String) -> String {
    let trimmed = value.trim();
    let Some(start) = trimmed.find("http://").or_else(|| trimmed.find("https://")) else {
        return String::new();
    };
    trimmed[start..]
        .split(|c: char| c == '"' || c == '\'' || c == ')' || c.is_whitespace())
        .next()
        .unwrap_or_default()
        .trim_end_matches([',', ';'])
        .to_string()
}
fn header_key(value: &str) -> String {
    value.trim().to_lowercase().replace([' ', '_', '-'], "")
}
fn col(map: &HashMap<String, usize>, names: &[&str]) -> Option<usize> {
    names
        .iter()
        .find_map(|name| map.get(&header_key(name)).copied())
}
fn cell<'a>(row: &'a [Data], map: &HashMap<String, usize>, names: &[&str]) -> Option<&'a Data> {
    col(map, names).and_then(|i| row.get(i))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImportResult {
    shop_name: String,
    imported: i64,
    updated: i64,
    source_file: String,
}

#[tauri::command]
pub(crate) fn import_competitor_shop_excel(
    path: String,
    state: State<AppState>,
) -> Result<ImportResult, String> {
    if !Path::new(path.trim()).is_file() {
        return Err("找不到 Excel 文件，请检查完整路径".into());
    }
    let mut book = open_workbook_auto(path.trim()).map_err(|e| format!("无法读取 Excel：{e}"))?;
    let range = book
        .worksheet_range_at(0)
        .ok_or("Excel 中没有工作表")?
        .map_err(|e| e.to_string())?;
    let mut rows = range.rows();
    let headers = rows.next().ok_or("Excel 没有表头")?;
    let mut map = HashMap::new();
    for (i, h) in headers.iter().enumerate() {
        let key = header_key(&text(Some(h)));
        map.entry(key).or_insert(i);
    }
    for required in ["SKU", "Title", "Listing URL"] {
        if col(&map, &[required]).is_none() {
            return Err(format!("Excel 缺少必要字段：{required}"));
        }
    }
    let data: Vec<Vec<Data>> = rows
        .map(|r| r.to_vec())
        .filter(|r| !text(cell(r, &map, &["SKU"])).is_empty())
        .collect();
    if data.is_empty() {
        return Err("Excel 中没有可导入的商品".into());
    }
    let shop_name = data
        .iter()
        .find_map(|r| {
            let v = text(cell(r, &map, &["Shop"]));
            (!v.is_empty()).then_some(v)
        })
        .unwrap_or_else(|| {
            Path::new(&path)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        });
    let mut c = db(&state)?;
    ensure(&c)?;
    let tx = c.transaction().map_err(|e| e.to_string())?;
    let mut inserted = 0;
    let mut updated = 0;
    for r in &data {
        let sku = text(cell(r, &map, &["SKU"]));
        let exists:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM competitor_shop_products WHERE shop_name=?1 AND sku=?2)",params![shop_name,sku],|x|x.get(0)).unwrap_or(false);
        let revenue = number(cell(r, &map, &["Revenue"]));
        let ad_share = percent(cell(r, &map, &["Advertising cost share"]));
        tx.execute("INSERT INTO competitor_shop_products(shop_name,sku,title,image_url,listing_url,brand,category,sales_method,price,sales,revenue,sales_growth,revenue_growth,gross_margin,total_impressions,impressions,product_views,view_to_cart,search_to_cart,average_discount,promo_sales_share,impression_to_order,order_conversion,ad_cost_share,estimated_ad_cost,return_cancel_rate,lost_revenue,rating,rating_count,seller_type,fulfillment,weight_kg,launch_age,source_file) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,?29,?30,?31,?32,?33,?34) ON CONFLICT(shop_name,sku) DO UPDATE SET title=excluded.title,image_url=excluded.image_url,listing_url=excluded.listing_url,brand=excluded.brand,category=excluded.category,sales_method=excluded.sales_method,price=excluded.price,sales=excluded.sales,revenue=excluded.revenue,sales_growth=excluded.sales_growth,revenue_growth=excluded.revenue_growth,gross_margin=excluded.gross_margin,total_impressions=excluded.total_impressions,impressions=excluded.impressions,product_views=excluded.product_views,view_to_cart=excluded.view_to_cart,search_to_cart=excluded.search_to_cart,average_discount=excluded.average_discount,promo_sales_share=excluded.promo_sales_share,impression_to_order=excluded.impression_to_order,order_conversion=excluded.order_conversion,ad_cost_share=excluded.ad_cost_share,estimated_ad_cost=excluded.estimated_ad_cost,return_cancel_rate=excluded.return_cancel_rate,lost_revenue=excluded.lost_revenue,rating=excluded.rating,rating_count=excluded.rating_count,seller_type=excluded.seller_type,fulfillment=excluded.fulfillment,weight_kg=excluded.weight_kg,launch_age=excluded.launch_age,source_file=excluded.source_file,updated_at=CURRENT_TIMESTAMP",
      params![shop_name,sku,text(cell(r,&map,&["Title"])),web_url(text(cell(r,&map,&["Image"]))),web_url(text(cell(r,&map,&["Listing URL"]))),text(cell(r,&map,&["Brand"])),text(cell(r,&map,&["Categories"])),text(cell(r,&map,&["sales method"])),number(cell(r,&map,&["Price"])),number(cell(r,&map,&["Sales"])),revenue,percent(cell(r,&map,&["Sales Growth Rate"])),percent(cell(r,&map,&["Revenue Rate"])),percent(cell(r,&map,&["Gross Margin"])),number(cell(r,&map,&["Total impressions"])),number(cell(r,&map,&["Impressions"])),number(cell(r,&map,&["Product Card Views"])),percent(cell(r,&map,&["View-to-cart rate"])),percent(cell(r,&map,&["Search-to-cart rate"])),percent(cell(r,&map,&["Average discount"])),percent(cell(r,&map,&["Promo sales share"])),percent(cell(r,&map,&["Impression-to-order rate"])),percent(cell(r,&map,&["Order conversion rate"])),ad_share,revenue*ad_share/100.0,percent(cell(r,&map,&["Return cancellation rate"])),number(cell(r,&map,&["Lost revenue"])),number(cell(r,&map,&["Ratings"])),number(cell(r,&map,&["NO. Ratings"])),text(cell(r,&map,&["Seller type"])),text(cell(r,&map,&["Fulfillment"])),weight(cell(r,&map,&["Weight"])),text(cell(r,&map,&["Launch Age"])),path]).map_err(|e|format!("保存 SKU {sku} 失败：{e}"))?;
        if exists {
            updated += 1
        } else {
            inserted += 1
        }
    }
    tx.execute(
        "INSERT INTO competitor_shop_imports(shop_name,source_file,product_count) VALUES(?1,?2,?3)",
        params![shop_name, path, data.len()],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(ImportResult {
        shop_name,
        imported: inserted,
        updated,
        source_file: path,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CompetitorProduct {
    id: i64,
    shop_name: String,
    sku: String,
    title: String,
    image_url: String,
    listing_url: String,
    brand: String,
    category: String,
    sales_method: String,
    price: f64,
    sales: f64,
    revenue: f64,
    sales_growth: f64,
    revenue_growth: f64,
    gross_margin: f64,
    total_impressions: f64,
    impressions: f64,
    product_views: f64,
    view_to_cart: f64,
    search_to_cart: f64,
    average_discount: f64,
    promo_sales_share: f64,
    impression_to_order: f64,
    order_conversion: f64,
    ad_cost_share: f64,
    estimated_ad_cost: f64,
    return_cancel_rate: f64,
    lost_revenue: f64,
    rating: f64,
    rating_count: f64,
    seller_type: String,
    fulfillment: String,
    weight_kg: Option<f64>,
    launch_age: String,
    updated_at: String,
}

#[tauri::command]
pub(crate) fn competitor_shop_products(
    shop_name: String,
    query: String,
    state: State<AppState>,
) -> Result<Vec<CompetitorProduct>, String> {
    let c = db(&state)?;
    ensure(&c)?;
    let p = format!("%{}%", query.trim());
    let mut s=c.prepare("SELECT id,shop_name,sku,title,image_url,listing_url,brand,category,sales_method,price,sales,revenue,sales_growth,revenue_growth,gross_margin,total_impressions,impressions,product_views,view_to_cart,search_to_cart,average_discount,promo_sales_share,impression_to_order,order_conversion,ad_cost_share,estimated_ad_cost,return_cancel_rate,lost_revenue,rating,rating_count,seller_type,fulfillment,weight_kg,launch_age,updated_at FROM competitor_shop_products WHERE (?1='' OR shop_name=?1) AND (?2='%%' OR sku LIKE ?2 OR title LIKE ?2 OR category LIKE ?2) ORDER BY sales DESC,revenue DESC").map_err(|e|e.to_string())?;
    let result = s
        .query_map(params![shop_name, p], |r| {
            Ok(CompetitorProduct {
                id: r.get(0)?,
                shop_name: r.get(1)?,
                sku: r.get(2)?,
                title: r.get(3)?,
                image_url: web_url(r.get(4)?),
                listing_url: web_url(r.get(5)?),
                brand: r.get(6)?,
                category: r.get(7)?,
                sales_method: r.get(8)?,
                price: r.get(9)?,
                sales: r.get(10)?,
                revenue: r.get(11)?,
                sales_growth: r.get(12)?,
                revenue_growth: r.get(13)?,
                gross_margin: r.get(14)?,
                total_impressions: r.get(15)?,
                impressions: r.get(16)?,
                product_views: r.get(17)?,
                view_to_cart: r.get(18)?,
                search_to_cart: r.get(19)?,
                average_discount: r.get(20)?,
                promo_sales_share: r.get(21)?,
                impression_to_order: r.get(22)?,
                order_conversion: r.get(23)?,
                ad_cost_share: r.get(24)?,
                estimated_ad_cost: r.get(25)?,
                return_cancel_rate: r.get(26)?,
                lost_revenue: r.get(27)?,
                rating: r.get(28)?,
                rating_count: r.get(29)?,
                seller_type: r.get(30)?,
                fulfillment: r.get(31)?,
                weight_kg: r.get(32)?,
                launch_age: r.get(33)?,
                updated_at: r.get(34)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(result)
}

#[tauri::command]
pub(crate) fn competitor_shop_names(state: State<AppState>) -> Result<Vec<String>, String> {
    let c = db(&state)?;
    ensure(&c)?;
    let mut s = c
        .prepare("SELECT DISTINCT shop_name FROM competitor_shop_products ORDER BY shop_name")
        .map_err(|e| e.to_string())?;
    let result = s
        .query_map([], |r| r.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(result)
}

#[tauri::command]
pub(crate) fn add_competitor_to_selection(id: i64, state: State<AppState>) -> Result<i64, String> {
    let c = db(&state)?;
    ensure(&c)?;
    crate::selection_library::ensure(&c)?;
    let v=c.query_row("SELECT title,image_url,listing_url,price,sales,weight_kg,category,sku FROM competitor_shop_products WHERE id=?1",[id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,f64>(3)?,r.get::<_,f64>(4)?,r.get::<_,Option<f64>>(5)?,r.get::<_,String>(6)?,r.get::<_,String>(7)?))).map_err(|_|"找不到该竞品".to_string())?;
    c.execute("INSERT INTO selection_items(product_name,image_url,target_market,competitor_url,competitor_price,estimated_monthly_sales,weight_kg,status,priority,tags,notes) VALUES(?1,?2,'俄罗斯 Ozon',?3,?4,?5,?6,'待调研','中',?7,?8)",params![v.0,v.1,v.2,v.3,v.4.round() as i64,v.5,v.6,format!("来自竞品店铺拆解，SKU {}",v.7)]).map_err(|e|e.to_string())?;
    Ok(c.last_insert_rowid())
}

fn competitor_shop_rows(c: &Connection, shop_name: &str) -> Result<Vec<CompetitorProduct>, String> {
    let mut s=c.prepare("SELECT id,shop_name,sku,title,image_url,listing_url,brand,category,sales_method,price,sales,revenue,sales_growth,revenue_growth,gross_margin,total_impressions,impressions,product_views,view_to_cart,search_to_cart,average_discount,promo_sales_share,impression_to_order,order_conversion,ad_cost_share,estimated_ad_cost,return_cancel_rate,lost_revenue,rating,rating_count,seller_type,fulfillment,weight_kg,launch_age,updated_at FROM competitor_shop_products WHERE shop_name=?1 ORDER BY sales DESC").map_err(|e|e.to_string())?;
    let v = s
        .query_map([shop_name], |r| {
            Ok(CompetitorProduct {
                id: r.get(0)?,
                shop_name: r.get(1)?,
                sku: r.get(2)?,
                title: r.get(3)?,
                image_url: web_url(r.get(4)?),
                listing_url: web_url(r.get(5)?),
                brand: r.get(6)?,
                category: r.get(7)?,
                sales_method: r.get(8)?,
                price: r.get(9)?,
                sales: r.get(10)?,
                revenue: r.get(11)?,
                sales_growth: r.get(12)?,
                revenue_growth: r.get(13)?,
                gross_margin: r.get(14)?,
                total_impressions: r.get(15)?,
                impressions: r.get(16)?,
                product_views: r.get(17)?,
                view_to_cart: r.get(18)?,
                search_to_cart: r.get(19)?,
                average_discount: r.get(20)?,
                promo_sales_share: r.get(21)?,
                impression_to_order: r.get(22)?,
                order_conversion: r.get(23)?,
                ad_cost_share: r.get(24)?,
                estimated_ad_cost: r.get(25)?,
                return_cancel_rate: r.get(26)?,
                lost_revenue: r.get(27)?,
                rating: r.get(28)?,
                rating_count: r.get(29)?,
                seller_type: r.get(30)?,
                fulfillment: r.get(31)?,
                weight_kg: r.get(32)?,
                launch_age: r.get(33)?,
                updated_at: r.get(34)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(v)
}

#[tauri::command]
pub(crate) fn open_competitor_product_url(url: String) -> Result<(), String> {
    let url = web_url(url);
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("该商品没有有效的网页链接，请重新导入源 Excel".into());
    }
    open::that(&url).map_err(|e| format!("无法打开商品链接：{e}"))
}

const DEFAULT_COMPETITOR_FEISHU_URL: &str = "https://ecnmfepjh251.feishu.cn/base/IxeZbgbIIaJjPqsetkXcxDMjnXc?table=tblbO9mtOJ22kCcf&view=vewjxSkHXu";

fn competitor_feishu_path(c: &Connection, value: &str) -> Result<String, String> {
    let value = if value.trim().is_empty() {
        crate::setting(c, "competitor_feishu_table_url")
    } else {
        value.trim().to_string()
    };
    let base = regex::Regex::new(r"/base/([^/?#]+)")
        .unwrap()
        .captures(&value)
        .and_then(|x| x.get(1))
        .map(|x| x.as_str().to_string());
    let table = regex::Regex::new(r"(?:[?&])table=([^&#]+)")
        .unwrap()
        .captures(&value)
        .and_then(|x| x.get(1))
        .map(|x| x.as_str().to_string());
    match (base, table) {
        (Some(app), Some(table)) if !app.is_empty() && table.starts_with("tbl") => Ok(format!(
            "{}/bitable/v1/apps/{app}/tables/{table}",
            crate::feishu_base(c)
        )),
        _ => Err("飞书链接格式不正确：请粘贴包含 /base/…?table=tbl… 的完整多维表格链接".into()),
    }
}

#[tauri::command]
pub(crate) fn competitor_feishu_target(state: State<AppState>) -> Result<String, String> {
    let c = db(&state)?;
    let saved = crate::setting(&c, "competitor_feishu_table_url");
    Ok(if saved.is_empty() {
        DEFAULT_COMPETITOR_FEISHU_URL.into()
    } else {
        saved
    })
}

#[tauri::command]
pub(crate) fn save_competitor_feishu_target(
    url: String,
    state: State<AppState>,
) -> Result<String, String> {
    let c = db(&state)?;
    competitor_feishu_path(&c, &url)?;
    crate::save_setting(&c, "competitor_feishu_table_url", url.trim())?;
    Ok("飞书目标表已保存".into())
}

#[tauri::command]
pub(crate) fn test_competitor_feishu_target(
    url: String,
    state: State<AppState>,
) -> Result<String, String> {
    let c = db(&state)?;
    let token = crate::feishu_token(&c)?;
    let path = competitor_feishu_path(&c, &url)?;
    crate::feishu_raw(
        "GET",
        &format!("{path}/fields?page_size=1"),
        Some(&token),
        None,
    )?;
    Ok("飞书目标表权限验证成功，可以同步竞品数据".into())
}

fn sync_feishu(shop_name: String, target_url: String, state: &AppState) -> Result<String, String> {
    let c = db(state)?;
    ensure(&c)?;
    let token = crate::feishu_token(&c)?;
    let path = competitor_feishu_path(&c, &target_url)?;
    crate::save_setting(&c, "competitor_feishu_table_url", target_url.trim())?;
    let definitions = [
        ("数据类型", 1),
        ("竞品店铺", 1),
        ("SKU", 1),
        ("品名", 1),
        ("图片链接", 1),
        ("类目", 1),
        ("履约方式", 1),
        ("售价", 2),
        ("销量", 2),
        ("销售额", 2),
        ("销量增长率", 2),
        ("毛利率", 2),
        ("总曝光", 2),
        ("商品访问", 2),
        ("访问加购率", 2),
        ("下单转化率", 2),
        ("广告费占比", 2),
        ("估算广告费", 2),
        ("退货取消率", 2),
        ("损失销售额", 2),
        ("评分", 2),
        ("评价数量", 2),
        ("包装重", 2),
        ("本地更新时间", 1),
    ];
    let payload = crate::feishu_raw(
        "GET",
        &format!("{path}/fields?page_size=100"),
        Some(&token),
        None,
    )?;
    let existing: std::collections::BTreeSet<String> = payload
        .pointer("/data/items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|v| crate::json_text(v.get("field_name")))
        .collect();
    for (name, kind) in definitions {
        if !existing.contains(name) {
            crate::feishu_raw(
                "POST",
                &format!("{path}/fields"),
                Some(&token),
                Some(&json!({"field_name":name,"type":kind})),
            )?;
        }
    }
    let mut remote = HashMap::new();
    for record in crate::feishu_records(&token, &path)? {
        let fields = record.get("fields");
        let kind = crate::feishu_field_text(fields.and_then(|v| v.get("数据类型")));
        let store = crate::feishu_field_text(fields.and_then(|v| v.get("竞品店铺")));
        let sku = crate::feishu_field_text(fields.and_then(|v| v.get("SKU")));
        if kind == "竞品" && store == shop_name && !sku.is_empty() {
            remote.entry(sku).or_insert(record);
        }
    }
    let rows = competitor_shop_rows(&c, &shop_name)?;
    if rows.is_empty() {
        return Err("当前竞品店铺没有可同步的商品".into());
    }
    let mut creates = Vec::new();
    let mut updates = Vec::new();
    for x in rows {
        let mut f = Map::new();
        f.insert("数据类型".into(), "竞品".into());
        f.insert("竞品店铺".into(), x.shop_name.clone().into());
        f.insert("SKU".into(), x.sku.clone().into());
        f.insert("品名".into(), x.listing_url.into());
        f.insert("图片链接".into(), x.image_url.into());
        f.insert("类目".into(), x.category.into());
        f.insert("履约方式".into(), x.fulfillment.into());
        for (n, v) in [
            ("售价", x.price),
            ("销量", x.sales),
            ("销售额", x.revenue),
            ("销量增长率", x.sales_growth),
            ("毛利率", x.gross_margin),
            ("总曝光", x.total_impressions),
            ("商品访问", x.product_views),
            ("访问加购率", x.view_to_cart),
            ("下单转化率", x.order_conversion),
            ("广告费占比", x.ad_cost_share),
            ("估算广告费", x.estimated_ad_cost),
            ("退货取消率", x.return_cancel_rate),
            ("损失销售额", x.lost_revenue),
            ("评分", x.rating),
            ("评价数量", x.rating_count),
        ] {
            f.insert(n.into(), v.into());
        }
        if let Some(v) = x.weight_kg {
            f.insert("包装重".into(), v.into());
        }
        f.insert("本地更新时间".into(), x.updated_at.into());
        if let Some(r) = remote.get(&x.sku) {
            updates.push(json!({"record_id":crate::json_text(r.get("record_id")),"fields":f}));
        } else {
            creates.push(json!({"fields":f}));
        }
    }
    for chunk in creates.chunks(500) {
        crate::feishu_raw(
            "POST",
            &format!("{path}/records/batch_create"),
            Some(&token),
            Some(&json!({"records":chunk})),
        )?;
    }
    for chunk in updates.chunks(500) {
        crate::feishu_raw(
            "POST",
            &format!("{path}/records/batch_update"),
            Some(&token),
            Some(&json!({"records":chunk})),
        )?;
    }
    Ok(format!(
        "竞品数据已同步到飞书：新增 {} 条，更新 {} 条",
        creates.len(),
        updates.len()
    ))
}
#[tauri::command]
pub(crate) async fn sync_competitor_shop_feishu(
    shop_name: String,
    target_url: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let owned = crate::background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || sync_feishu(shop_name, target_url, &owned))
        .await
        .map_err(|e| format!("飞书后台同步失败：{e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_units() {
        assert_eq!(number(Some(&Data::String("1 171₽".into()))), 1171.0);
        assert_eq!(percent(Some(&Data::String("3.45%".into()))), 3.45);
        assert_eq!(weight(Some(&Data::String("550 g".into()))), Some(0.55));
        assert_eq!(
            web_url("=IMAGE(\"https://ir.ozone.ru/a.jpg\")".into()),
            "https://ir.ozone.ru/a.jpg"
        );
        assert_eq!(
            web_url("https://www.ozon.ru/product/4753186677".into()),
            "https://www.ozon.ru/product/4753186677"
        );
    }

    #[test]
    fn parses_competitor_feishu_link() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT)")
            .unwrap();
        let path = competitor_feishu_path(&c, DEFAULT_COMPETITOR_FEISHU_URL).unwrap();
        assert!(path.contains("/apps/IxeZbgbIIaJjPqsetkXcxDMjnXc/tables/tblbO9mtOJ22kCcf"));
        assert!(!path.contains("vewjxSkHXu"));
    }
}
