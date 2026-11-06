use crate::{wb_shop_center, AppState};
use rusqlite::params;
use serde_json::{json, Value};
use tauri::State;

#[tauri::command]
pub fn wb_pricing(action: String, payload: Value, state: State<AppState>) -> Result<Value, String> {
    let shop = payload["shopId"].as_str().filter(|s| !s.is_empty()).ok_or("请选择 WB 店铺")?;
    let c = wb_shop_center::db(&state)?;
    c.execute_batch("CREATE TABLE IF NOT EXISTS wb_pricing_state(shop_id TEXT NOT NULL, kind TEXT NOT NULL, value TEXT NOT NULL, PRIMARY KEY(shop_id,kind));").map_err(|e|e.to_string())?;
    match action.as_str() {
        "load" => {
            let mut q = c.prepare("SELECT kind,value FROM wb_pricing_state WHERE shop_id=?1").map_err(|e|e.to_string())?;
            let rows = q.query_map([shop], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).map_err(|e|e.to_string())?;
            let mut result = json!({});
            for row in rows { let (key,value)=row.map_err(|e|e.to_string())?; result[key]=serde_json::from_str(&value).map_err(|e|e.to_string())?; }
            Ok(result)
        }
        "save" => {
            c.execute("INSERT INTO wb_pricing_state VALUES(?1,'costs',?2) ON CONFLICT(shop_id,kind) DO UPDATE SET value=excluded.value",params![shop,payload["costs"].to_string()]).map_err(|e|e.to_string())?;
            Ok(json!({}))
        }
        "sync" => {
            let (_,token)=wb_shop_center::active_token(&c,shop)?;
            let mut goods=Vec::new();
            for offset in (0..100000).step_by(1000) {
                let url=format!("https://discounts-prices-api.wildberries.cn/api/v2/list/goods/filter?limit=1000&offset={offset}");
                let response_text=ureq::get(&url).set("Authorization",&token).timeout(std::time::Duration::from_secs(40)).call().map_err(|e|format!("读取 WB 价格失败：{e}"))?.into_string().map_err(|e|e.to_string())?;
            let response: Value = serde_json::from_str(&response_text).map_err(|e|e.to_string())?;
                if response["error"].as_bool()==Some(true) { return Err(response["errorText"].to_string()); }
                let page=response["data"]["listGoods"].as_array().ok_or("WB 商品响应格式异常")?;
                goods.extend(page.iter().cloned());
                if page.len()<1000 {
                    let result=json!({"goods":goods,"updatedAt":chrono::Utc::now().to_rfc3339()});
                    c.execute("INSERT INTO wb_pricing_state VALUES(?1,'catalog',?2) ON CONFLICT(shop_id,kind) DO UPDATE SET value=excluded.value",params![shop,result.to_string()]).map_err(|e|e.to_string())?;
                    return Ok(result);
                }
                std::thread::sleep(std::time::Duration::from_millis(650));
            }
            Err("商品超过单次同步上限，原有缓存已保留".into())
        }
        "submit" => {
            let nm = payload["nmID"].as_i64().filter(|n| *n>0).ok_or("商品编号无效")?;
            let price = payload["price"].as_i64().filter(|n| *n>0 && *n<=100000000).ok_or("售价必须为正整数")?;
            let discount=payload["discount"].as_i64().filter(|n| (0..=99).contains(n)).ok_or("折扣必须为 0 至 99 的整数")?;
            let (_,token)=wb_shop_center::active_token(&c,shop)?;
            let response_text=ureq::post("https://discounts-prices-api.wildberries.cn/api/v2/upload/task").set("Authorization",&token).timeout(std::time::Duration::from_secs(40)).set("Content-Type","application/json").send_string(&json!({"data":[{"nmID":nm,"price":price,"discount":discount}]}).to_string()).map_err(|e|format!("改价响应异常，请先在 WB 后台核对，避免重复提交：{e}"))?.into_string().map_err(|e|e.to_string())?;
            let response: Value = serde_json::from_str(&response_text).map_err(|e|e.to_string())?;
            c.execute("INSERT INTO wb_pricing_state VALUES(?1,'lastTask',?2) ON CONFLICT(shop_id,kind) DO UPDATE SET value=excluded.value",params![shop,response.to_string()]).map_err(|e|e.to_string())?;
            if response["error"].as_bool()==Some(true){return Err(response["errorText"].to_string())}
            Ok(response)
        }
        "status" => {
            let id=payload["uploadID"].as_i64().filter(|n|*n>0).ok_or("任务编号无效")?;
            let (_,token)=wb_shop_center::active_token(&c,shop)?;
            let response_text=ureq::get(&format!("https://discounts-prices-api.wildberries.cn/api/v2/history/goods/task?uploadID={id}&limit=1000&offset=0")).set("Authorization",&token).timeout(std::time::Duration::from_secs(40)).call().map_err(|e|format!("查询任务失败，任务可能仍在处理中：{e}"))?.into_string().map_err(|e|e.to_string())?;
            let response: Value = serde_json::from_str(&response_text).map_err(|e|e.to_string())?;
            Ok(response)
        }
        _ => Err("不支持的 WB 核价操作".into())
    }
}
