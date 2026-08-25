use crate::{background_state, db, read_registry, save_setting, secret_setting, setting, AppState};
use calamine::{open_workbook_auto, Reader};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashMap, fs, path::Path, process::Command};
use tauri::State;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListingSettings {
    ledger_path: String,
    ledger_shop_name: String,
    tool_executable: String,
    tool_data_dir: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListingRow {
    shop_name: String,
    platform: String,
    offer_id: String,
    product_id: String,
    unit_cost_cny: Option<f64>,
    weight_kg: Option<f64>,
    length_cm: Option<f64>,
    width_cm: Option<f64>,
    height_cm: Option<f64>,
    status: String,
    listing_mode: String,
    pricing_mode: String,
    price: Option<f64>,
    profit: Option<f64>,
    roi_percent: Option<f64>,
    category: String,
    import_task_id: String,
    updated_at: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListingPriceInput {
    purchase_cost: f64,
    label_fee: f64,
    target_roi_percent: f64,
    weight_kg: f64,
    sales_commission_percent: f64,
    sales_commission_discount_percent: f64,
    advertising_percent: f64,
    cargo_loss_percent: f64,
    minimum_sale_price: f64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ListingPriceBreakdown {
    price: f64,
    shipping: f64,
    sales_commission: f64,
    logistics_commission: f64,
    advertising: f64,
    cargo_loss: f64,
    invested: f64,
    profit: f64,
    roi_percent: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListingJobRow { id:i64, source_url:String, article:String, offer_id:String, title:String, category_id:String, category_display:String, status:String, stage:i64, error:String, payload:Value, updated_at:String }

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListingDraftInput {
    id:i64, offer_id:String, title:String, category_id:String, category_display:String,
    type_id:String, price:String, weight:f64, depth:f64, width:f64, height:f64,
    description:String, images:Vec<String>, attributes:Value, complex_attributes:Value,
}

fn normalize_reference_input(value:&str)->Result<(String,String),String>{
    let raw=value.trim();
    let labelled=regex::Regex::new(r"(?i)^(?:артикул|sku|商品编号)\s*[:：#]?\s*(\d{6,})$").unwrap();
    let direct=regex::Regex::new(r"^\d{6,}$").unwrap();
    let article=if direct.is_match(raw){raw.to_string()}else if let Some(c)=labelled.captures(raw){c[1].to_string()}else{
        let host=regex::Regex::new(r"(?i)^https?://(?:www\.)?[^/]*ozon\.ru/").unwrap();
        if !host.is_match(raw){return Err("请输入 Ozon 商品链接或纯数字 Артикул".into())}
        regex::Regex::new(r"(?:-|/)(\d{6,})/?(?:\?[^#]*)?(?:#.*)?$").unwrap().captures(raw).map(|c|c[1].to_string()).unwrap_or_default()
    };
    if article.is_empty(){return Err("链接中没有识别到 Ozon Артикул".into())}
    let url=if direct.is_match(raw)||labelled.is_match(raw){format!("https://www.ozon.ru/product/{article}/")}else{raw.to_string()};
    Ok((url,article))
}

fn ensure_listing_jobs(c:&rusqlite::Connection)->Result<(),String>{
    c.execute_batch("CREATE TABLE IF NOT EXISTS listing_jobs(id INTEGER PRIMARY KEY AUTOINCREMENT,source_url TEXT NOT NULL,article TEXT NOT NULL DEFAULT '',offer_id TEXT NOT NULL DEFAULT '',title TEXT NOT NULL DEFAULT '',category_id TEXT NOT NULL DEFAULT '',category_display TEXT NOT NULL DEFAULT '',status TEXT NOT NULL DEFAULT 'draft',stage INTEGER NOT NULL DEFAULT 0,error TEXT NOT NULL DEFAULT '',payload TEXT NOT NULL DEFAULT '{}',created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);CREATE INDEX IF NOT EXISTS idx_listing_jobs_status ON listing_jobs(status,updated_at);").map_err(|e|e.to_string())
}

#[tauri::command]
pub fn create_listing_draft(reference:String,state:State<AppState>)->Result<i64,String>{
    let (url,article)=normalize_reference_input(&reference)?; let c=db(&state)?; ensure_listing_jobs(&c)?;
    c.execute("INSERT INTO listing_jobs(source_url,article,status,stage,payload)VALUES(?1,?2,'draft',0,?3)",params![url,article,json!({"source_url":url,"article":article}).to_string()]).map_err(|e|e.to_string())?;
    Ok(c.last_insert_rowid())
}

#[tauri::command]
pub fn listing_jobs(state:State<AppState>)->Result<Vec<ListingJobRow>,String>{
    let c=db(&state)?;ensure_listing_jobs(&c)?;let mut s=c.prepare("SELECT id,source_url,article,offer_id,title,category_id,category_display,status,stage,error,payload,updated_at FROM listing_jobs ORDER BY id DESC LIMIT 500").map_err(|e|e.to_string())?;
    let rows=s.query_map([],|r|{let raw:String=r.get(10)?;Ok(ListingJobRow{id:r.get(0)?,source_url:r.get(1)?,article:r.get(2)?,offer_id:r.get(3)?,title:r.get(4)?,category_id:r.get(5)?,category_display:r.get(6)?,status:r.get(7)?,stage:r.get(8)?,error:r.get(9)?,payload:serde_json::from_str(&raw).unwrap_or_else(|_|json!({})),updated_at:r.get(11)?})}).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
    Ok(rows)
}

fn object_array(value:&Value,label:&str)->Result<(),String>{
    let items=value.as_array().ok_or_else(||format!("{label} 必须是 JSON 数组"))?;
    if items.iter().any(|v|!v.is_object()){return Err(format!("{label} 的每一项必须是 JSON 对象"))}
    Ok(())
}

fn meta_content(page:&str,key:&str)->String{
    let escaped=regex::escape(key);
    for pattern in [format!(r#"(?is)<meta[^>]+(?:property|name)=["']{}["'][^>]+content=["']([^"']*)["']"#,escaped),format!(r#"(?is)<meta[^>]+content=["']([^"']*)["'][^>]+(?:property|name)=["']{}["']"#,escaped)]{
        if let Ok(re)=regex::Regex::new(&pattern){if let Some(c)=re.captures(page){return c.get(1).map(|v|v.as_str().trim().to_string()).unwrap_or_default()}}
    } String::new()
}

fn reference_product_from_html(page:&str)->Result<Value,String>{
    let script_re=regex::Regex::new(r#"(?is)<script[^>]+type=["']application/ld\+json["'][^>]*>(.*?)</script>"#).unwrap();
    let mut product=None;
    for capture in script_re.captures_iter(page){
        let Some(raw)=capture.get(1) else{continue}; let Ok(root)=serde_json::from_str::<Value>(raw.as_str().trim()) else{continue};
        let mut queue=match root{Value::Array(v)=>v,v=>vec![v]};
        while let Some(item)=queue.pop(){
            if item.get("@type").and_then(Value::as_str).map(|v|v.eq_ignore_ascii_case("product")).unwrap_or(false){product=Some(item);break}
            if let Some(graph)=item.get("@graph").and_then(Value::as_array){queue.extend(graph.iter().cloned())}
        }
        if product.is_some(){break}
    }
    let product=product.unwrap_or_else(||json!({}));
    let title=product.get("name").and_then(Value::as_str).unwrap_or("").trim().to_string();
    let title=if title.is_empty(){meta_content(page,"og:title")}else{title};
    if title.is_empty(){return Err("商品页面没有返回可识别标题，可能触发了 Ozon 验证或地区跳转".into())}
    let description=product.get("description").and_then(Value::as_str).map(str::to_string).unwrap_or_else(||meta_content(page,"og:description"));
    let mut images=Vec::<String>::new();
    match product.get("image") {Some(Value::Array(values))=>images.extend(values.iter().filter_map(Value::as_str).map(str::to_string)),Some(Value::String(v))=>images.push(v.clone()),_=>{let v=meta_content(page,"og:image");if !v.is_empty(){images.push(v)}}}
    images.retain(|v|v.starts_with("http"));images.sort();images.dedup();
    let mut properties=serde_json::Map::new();
    let raw=product.get("additionalProperty").map(|v|match v{Value::Array(a)=>a.clone(),v=>vec![v.clone()]}).unwrap_or_default();
    for item in raw{let name=item.get("name").or_else(||item.get("propertyID")).and_then(Value::as_str).unwrap_or("").trim();let value=item.get("value").map(|v|v.as_str().map(str::to_string).unwrap_or_else(||v.to_string())).unwrap_or_default();if !name.is_empty()&&!value.is_empty(){properties.insert(name.into(),Value::String(value));}}
    Ok(json!({"title":title,"description":description,"images":images,"properties":properties}))
}

fn collect_listing_reference_blocking(id:i64,state:&AppState)->Result<i64,String>{
    let c=db(state)?;ensure_listing_jobs(&c)?;
    let (url,raw_payload):(String,String)=c.query_row("SELECT source_url,payload FROM listing_jobs WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|_|"上品草稿不存在".to_string())?;
    c.execute("UPDATE listing_jobs SET status='collecting',error='',updated_at=CURRENT_TIMESTAMP WHERE id=?1",[id]).map_err(|e|e.to_string())?;
    let result=(||{let response=ureq::get(&url).set("User-Agent","Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/126 Safari/537.36").set("Accept-Language","ru-RU,ru;q=0.9").timeout(std::time::Duration::from_secs(35)).call().map_err(|e|format!("Ozon 参考页读取失败：{e}"))?;let html=response.into_string().map_err(|e|format!("参考页解码失败：{e}"))?;reference_product_from_html(&html)})();
    match result{
        Ok(collected)=>{let mut payload=serde_json::from_str::<Value>(&raw_payload).unwrap_or_else(|_|json!({}));if let (Some(target),Some(source))=(payload.as_object_mut(),collected.as_object()){for (k,v) in source{target.insert(k.clone(),v.clone());}}let title=payload.get("title").and_then(Value::as_str).unwrap_or("");c.execute("UPDATE listing_jobs SET title=?1,status='draft',stage=MAX(stage,1),error='',payload=?2,updated_at=CURRENT_TIMESTAMP WHERE id=?3",params![title,payload.to_string(),id]).map_err(|e|e.to_string())?;Ok(1)}
        Err(error)=>{c.execute("UPDATE listing_jobs SET status='failed',error=?1,updated_at=CURRENT_TIMESTAMP WHERE id=?2",params![error,id]).map_err(|e|e.to_string())?;Err(error)}
    }
}

#[tauri::command]
pub async fn collect_listing_reference(id:i64,state:State<'_,AppState>)->Result<i64,String>{
    let owned=background_state(&state)?;tauri::async_runtime::spawn_blocking(move||collect_listing_reference_blocking(id,&owned)).await.map_err(|e|format!("参考商品后台采集失败：{e}"))?
}

#[tauri::command]
pub fn save_listing_draft(form:ListingDraftInput,state:State<AppState>)->Result<i64,String>{
    if form.id<=0{return Err("无效的草稿编号".into())}
    if form.weight<0.0||form.depth<0.0||form.width<0.0||form.height<0.0{return Err("重量和尺寸不能小于 0".into())}
    object_array(&form.attributes,"普通属性")?;object_array(&form.complex_attributes,"组合属性")?;
    let c=db(&state)?;ensure_listing_jobs(&c)?;
    let (source_url,article):(String,String)=c.query_row("SELECT source_url,article FROM listing_jobs WHERE id=?1",[form.id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|_|"上品草稿不存在".to_string())?;
    let stage=if !form.category_id.trim().is_empty()&&!form.type_id.trim().is_empty(){if !form.attributes.as_array().unwrap().is_empty()||!form.complex_attributes.as_array().unwrap().is_empty(){3}else{2}}else if !form.title.trim().is_empty(){1}else{0};
    let status=if stage>=3{"ready"}else{"draft"};
    let payload=json!({"source_url":source_url,"article":article,"offer_id":form.offer_id.trim(),"title":form.title.trim(),"category_id":form.category_id.trim(),"category_display":form.category_display.trim(),"type_id":form.type_id.trim(),"price":form.price.trim(),"weight":form.weight,"depth":form.depth,"width":form.width,"height":form.height,"description":form.description,"images":form.images,"attributes":form.attributes,"complex_attributes":form.complex_attributes});
    c.execute("UPDATE listing_jobs SET offer_id=?1,title=?2,category_id=?3,category_display=?4,status=?5,stage=?6,error='',payload=?7,updated_at=CURRENT_TIMESTAMP WHERE id=?8",params![form.offer_id.trim(),form.title.trim(),form.category_id.trim(),form.category_display.trim(),status,stage,payload.to_string(),form.id]).map_err(|e|e.to_string())?;
    Ok(stage)
}

#[tauri::command]
pub fn retry_listing_job(id:i64,state:State<AppState>)->Result<(),String>{
    let c=db(&state)?;ensure_listing_jobs(&c)?;
    let changed=c.execute("UPDATE listing_jobs SET status='draft',error='',updated_at=CURRENT_TIMESTAMP WHERE id=?1",[id]).map_err(|e|e.to_string())?;
    if changed==0{return Err("上品任务不存在".into())} Ok(())
}

fn shipping_bands(weight: f64) -> Vec<(f64, f64, f64)> {
    let mut bands = Vec::new();
    if weight < 0.5 {
        bands.push((0.0, 135.0, 3.37 + weight * 28.17));
    } else if weight < 30.0 {
        bands.push((0.0, 135.0, 25.83 + weight * 19.17));
    }
    if weight < 2.0 {
        bands.push((135.0, 635.0, 17.97 + weight * 28.17));
    } else if weight < 30.0 {
        bands.push((135.0, 635.0, 40.44 + weight * 28.17));
    }
    if weight < 5.0 {
        bands.push((635.0, 22_525.0, 24.17 + weight * 28.17));
    }
    bands
}

#[tauri::command]
pub fn calculate_listing_price(form: ListingPriceInput) -> Result<ListingPriceBreakdown, String> {
    if form.purchase_cost <= 0.0 || form.weight_kg <= 0.0 {
        return Err("采购成本和实际重量必须大于 0".into());
    }
    if form.label_fee < 0.0 || form.target_roi_percent < 0.0 || form.minimum_sale_price < 0.0 {
        return Err("贴单费、目标 ROI 和最低售价不能小于 0".into());
    }
    let commission_rate =
        (form.sales_commission_percent - form.sales_commission_discount_percent) / 100.0;
    let advertising_rate = form.advertising_percent / 100.0;
    let cargo_loss_rate = form.cargo_loss_percent / 100.0;
    if commission_rate <= 0.0 || commission_rate + 0.02 + advertising_rate >= 1.0 {
        return Err("减免后佣金必须大于 0，且佣金、物流佣金和广告费率合计必须小于 100%".into());
    }
    let invested = form.purchase_cost + form.label_fee;
    let target_roi = form.target_roi_percent / 100.0;
    let mut candidates = Vec::new();
    for (minimum, maximum, shipping) in shipping_bands(form.weight_kg) {
        let retained_rate = 1.0 - commission_rate - 0.02 - advertising_rate;
        let required = (target_roi * invested + (1.0 + cargo_loss_rate) * (invested + shipping))
            / retained_rate;
        let price = required
            .ceil()
            .max(1.0)
            .max(minimum)
            .max(form.minimum_sale_price);
        if price >= maximum {
            continue;
        }
        let sales_commission = price * commission_rate;
        let logistics_commission = price * 0.02;
        let advertising = price * advertising_rate;
        let cargo_loss = (invested + shipping) * cargo_loss_rate;
        let profit = price
            - sales_commission
            - logistics_commission
            - advertising
            - invested
            - shipping
            - cargo_loss;
        let roi_percent = profit / invested * 100.0;
        if roi_percent + f64::EPSILON >= form.target_roi_percent {
            candidates.push(ListingPriceBreakdown {
                price,
                shipping,
                sales_commission,
                logistics_commission,
                advertising,
                cargo_loss,
                invested,
                profit,
                roi_percent,
            });
        }
    }
    candidates
        .into_iter()
        .min_by(|a, b| a.price.total_cmp(&b.price))
        .ok_or_else(|| "当前成本、重量和目标 ROI 超出原上品工具运费公式支持的售价区间".into())
}

fn number(value: &str) -> Option<f64> {
    value.trim().replace(',', "").parse().ok()
}
fn ledger(path: &str) -> Result<Vec<HashMap<String, String>>, String> {
    if !Path::new(path).is_file() {
        return Err(format!("找不到产品台账：{path}"));
    }
    let mut book = open_workbook_auto(path).map_err(|e| format!("无法读取产品台账：{e}"))?;
    let sheet = book
        .sheet_names()
        .iter()
        .find(|x| x.as_str() == "产品台账")
        .cloned()
        .or_else(|| book.sheet_names().first().cloned())
        .ok_or("产品台账没有工作表")?;
    let range = book.worksheet_range(&sheet).map_err(|e| e.to_string())?;
    let mut iter = range.rows();
    let headers = iter
        .next()
        .ok_or("产品台账为空")?
        .iter()
        .map(|x| x.to_string().trim().to_string())
        .collect::<Vec<_>>();
    Ok(iter
        .filter_map(|row| {
            let item = headers
                .iter()
                .enumerate()
                .filter(|(_, h)| !h.is_empty())
                .map(|(i, h)| {
                    (
                        h.clone(),
                        row.get(i)
                            .map(|x| x.to_string())
                            .unwrap_or_default()
                            .trim()
                            .to_string(),
                    )
                })
                .collect::<HashMap<_, _>>();
            if item.values().any(|x| !x.is_empty()) {
                Some(item)
            } else {
                None
            }
        })
        .collect())
}
fn field<'a>(row: &'a HashMap<String, String>, key: &str) -> &'a str {
    row.get(key).map(String::as_str).unwrap_or("")
}

#[tauri::command]
pub fn listing_settings(state: State<AppState>) -> Result<ListingSettings, String> {
    let c = db(&state)?;
    Ok(ListingSettings {
        ledger_path: setting(&c, "listing_ledger_path"),
        ledger_shop_name: setting(&c, "listing_ledger_shop_name"),
        tool_executable: setting(&c, "listing_tool_executable"),
        tool_data_dir: setting(&c, "listing_tool_data_dir"),
    })
}

#[tauri::command]
pub fn save_listing_settings(form: ListingSettings, state: State<AppState>) -> Result<(), String> {
    let c = db(&state)?;
    save_setting(&c, "listing_ledger_path", form.ledger_path.trim())?;
    save_setting(&c, "listing_ledger_shop_name", form.ledger_shop_name.trim())?;
    save_setting(&c, "listing_tool_executable", form.tool_executable.trim())?;
    save_setting(&c, "listing_tool_data_dir", form.tool_data_dir.trim())
}

#[tauri::command]
pub fn listing_rows(query: String, state: State<AppState>) -> Result<Vec<ListingRow>, String> {
    listing_rows_inner(&query, &state)
}

fn listing_rows_inner(query: &str, state: &AppState) -> Result<Vec<ListingRow>, String> {
    let c = db(state)?;
    let path = setting(&c, "listing_ledger_path");
    if path.is_empty() {
        return Ok(vec![]);
    }
    let selected = setting(&c, "listing_ledger_shop_name");
    let needle = query.trim().to_lowercase();
    Ok(ledger(&path)?
        .into_iter()
        .filter(|r| {
            field(r, "平台").to_lowercase().starts_with("ozon")
                && (selected.is_empty() || field(r, "上品店铺").eq_ignore_ascii_case(&selected))
        })
        .filter(|r| {
            needle.is_empty()
                || format!(
                    "{} {} {}",
                    field(r, "上品店铺"),
                    field(r, "货号"),
                    field(r, "Ozon商品ID")
                )
                .to_lowercase()
                .contains(&needle)
        })
        .map(|r| ListingRow {
            shop_name: field(&r, "上品店铺").into(),
            platform: field(&r, "平台").into(),
            offer_id: field(&r, "货号").into(),
            product_id: field(&r, "Ozon商品ID").into(),
            unit_cost_cny: number(field(&r, "采购成本")),
            weight_kg: number(field(&r, "包装毛重(g)")).map(|v| v / 1000.0),
            length_cm: number(field(&r, "包装长度(mm)")).map(|v| v / 10.0),
            width_cm: number(field(&r, "包装宽度(mm)")).map(|v| v / 10.0),
            height_cm: number(field(&r, "包装高度(mm)")).map(|v| v / 10.0),
            status: field(&r, "状态").into(),
            listing_mode: field(&r, "上品模式").into(),
            pricing_mode: field(&r, "核价方式").into(),
            price: number(field(&r, "售价")),
            profit: number(field(&r, "利润")),
            roi_percent: number(field(&r, "实际ROI")),
            category: field(&r, "Ozon类目").into(),
            import_task_id: field(&r, "Ozon导入任务ID").into(),
            updated_at: field(&r, "更新时间").into(),
        })
        .collect())
}

fn sync_listing_costs_blocking(state: &AppState) -> Result<i64, String> {
    let rows = listing_rows_inner("", state)?;
    let mut c = db(state)?;
    let tx = c.transaction().map_err(|e| e.to_string())?;
    let mut count = 0;
    for row in rows {
        if row.offer_id.is_empty() {
            continue;
        }
        count+=tx.execute("INSERT INTO product_costs(sku,unit_cost_cny,length_cm,width_cm,height_cm,weight_kg,note) SELECT sku,?2,?3,?4,?5,?6,'跨境上品台账同步' FROM products WHERE offer_id=?1 ON CONFLICT(sku) DO UPDATE SET unit_cost_cny=COALESCE(excluded.unit_cost_cny,product_costs.unit_cost_cny),length_cm=COALESCE(excluded.length_cm,product_costs.length_cm),width_cm=COALESCE(excluded.width_cm,product_costs.width_cm),height_cm=COALESCE(excluded.height_cm,product_costs.height_cm),weight_kg=COALESCE(excluded.weight_kg,product_costs.weight_kg),updated_at=CURRENT_TIMESTAMP",params![row.offer_id,row.unit_cost_cny,row.length_cm,row.width_cm,row.height_cm,row.weight_kg]).map_err(|e|e.to_string())? as i64;
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(count)
}

#[tauri::command]
pub async fn sync_listing_costs(state: State<'_, AppState>) -> Result<i64, String> {
    let owned = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || sync_listing_costs_blocking(&owned))
        .await.map_err(|e| format!("上品台账后台同步失败：{e}"))?
}

#[tauri::command]
pub fn launch_listing_tool(state: State<AppState>) -> Result<String, String> {
    let c = db(&state)?;
    let exe = setting(&c, "listing_tool_executable");
    if !Path::new(&exe).is_file() {
        return Err("未找到上品工具，请先填写可执行文件路径".into());
    }
    let data = setting(&c, "listing_tool_data_dir");
    let data_dir = if data.is_empty() {
        state.data_dir.join("listing_tool")
    } else {
        Path::new(&data).to_path_buf()
    };
    fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
    let registry = read_registry(&state.data_dir)?;
    let active = state
        .active_shop_id
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    let shop = registry
        .shops
        .iter()
        .find(|x| x.id == active)
        .ok_or("当前店铺不存在")?;
    let client = setting(&c, "seller_client_id");
    let key = secret_setting(&c, "seller_api_key")?;
    if client.is_empty() || key.is_empty() {
        return Err("当前店铺未配置 Seller Client ID / API Key".into());
    }
    let config = data_dir.join("config.json");
    let mut root: Value = if config.is_file() {
        serde_json::from_str(&fs::read_to_string(&config).map_err(|e| e.to_string())?)
            .unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };
    let ozon = root
        .as_object_mut()
        .ok_or("上品工具配置格式错误")?
        .entry("ozon")
        .or_insert_with(|| json!({}));
    let object = ozon.as_object_mut().ok_or("上品工具 Ozon 配置格式错误")?;
    let shops = object
        .entry("shops")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or("上品工具店铺配置格式错误")?;
    let profile =
        json!({"id":shop.id,"name":shop.name,"client_id":client,"api_key":key,"proxy_url":""});
    if let Some(existing) = shops
        .iter_mut()
        .find(|x| x.get("id").and_then(Value::as_str) == Some(&shop.id))
    {
        *existing = profile
    } else {
        shops.push(profile)
    }
    object.insert("selected_shop_id".into(), json!(shop.id));
    let temp = config.with_extension("json.tmp");
    fs::write(
        &temp,
        serde_json::to_vec_pretty(&root).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::rename(&temp, &config).map_err(|e| e.to_string())?;
    let child = Command::new(&exe)
        .current_dir(Path::new(&exe).parent().ok_or("上品工具路径无父目录")?)
        .env("OZON_RFBS_DATA_DIR", &data_dir)
        .spawn()
        .map_err(|e| format!("无法启动上品工具：{e}"))?;
    Ok(format!("上品工具已启动，PID {}", child.id()))
}

#[cfg(test)]
mod tests {
    use super::{calculate_listing_price, normalize_reference_input, number, reference_product_from_html, ListingPriceInput};

    #[test]
    fn ledger_number_accepts_grouped_values() {
        assert_eq!(number("1,234.50"), Some(1234.5));
        assert_eq!(number(""), None);
    }
    #[test] fn reference_input_matches_rfbs_source(){assert_eq!(normalize_reference_input("Артикул: 2379505289").unwrap().0,"https://www.ozon.ru/product/2379505289/");assert!(normalize_reference_input("https://example.com/123456").is_err());}
    #[test] fn reference_html_preserves_source_fields(){let page=r#"<script type="application/ld+json">{"@type":"Product","name":"Товар","description":"Описание","image":["https://cdn/a.jpg"],"additionalProperty":[{"name":"Цвет","value":"Черный"}]}</script>"#;let value=reference_product_from_html(page).unwrap();assert_eq!(value["title"],"Товар");assert_eq!(value["properties"]["Цвет"],"Черный");assert_eq!(value["images"].as_array().unwrap().len(),1);}

    #[test]
    fn roi_pricing_matches_rfbs_source_cases() {
        let form = |purchase_cost, commission| ListingPriceInput {
            purchase_cost,
            label_fee: 2.0,
            target_roi_percent: 60.0,
            weight_kg: 0.6,
            sales_commission_percent: commission,
            sales_commission_discount_percent: 0.0,
            advertising_percent: 15.0,
            cargo_loss_percent: 10.0,
            minimum_sale_price: 0.0,
        };
        let first = calculate_listing_price(form(15.0, 12.0)).unwrap();
        assert_eq!(first.price, 99.0);
        assert!((first.shipping - 37.332).abs() < 0.000_001);
        let crossed = calculate_listing_price(form(50.0, 14.0)).unwrap();
        assert_eq!(crossed.price, 184.0);
        assert!((crossed.shipping - 34.872).abs() < 0.000_001);
    }
}
