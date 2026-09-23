use crate::{active_shop_kind, cross_border_shipping, db, insights, rub_per_cny_for, seller_post, AppState};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PriceProfitRow {
    sku: String,
    product_id: String,
    offer_id: String,
    product_name: String,
    currency_code: String,
    frontend_price: Option<f64>,
    promotion_price: Option<f64>,
    original_price: Option<f64>,
    final_pricing: Option<f64>,
    subsidy_amount: Option<f64>,
    subsidy_rate: Option<f64>,
    purchase_cost_cny: Option<f64>,
    first_mile_cny: Option<f64>,
    cross_border_freight_cny: Option<f64>,
    freight_cny: Option<f64>,
    volume_l: Option<f64>,
    weight_kg: Option<f64>,
    final_pricing_cny: Option<f64>,
    commission_rate: Option<f64>,
    advertising_cny: Option<f64>,
    damage_cny: Option<f64>,
    commission_cny: Option<f64>,
    logistics_commission_cny: Option<f64>,
    label_fee_cny: Option<f64>,
    profit_cny: Option<f64>,
    profit_margin: Option<f64>,
    warning: bool,
    missing_fields: Vec<String>,
    synced_at: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceIntelligenceData {
    rows: Vec<PriceProfitRow>,
    is_cross_border: bool,
    rub_per_cny: f64,
    warning_margin: f64,
    advertising_rate: f64,
    damage_rate: f64,
    label_fee_cny: f64,
    low_commission_rate: f64,
    high_commission_rate: f64,
    logistics_commission_rate: f64,
    commission_threshold_cny: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfitMonitorSettings {
    warning_margin: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshResult {
    requested: usize,
    refreshed: usize,
    failed: usize,
    errors: Vec<String>,
    data: PriceIntelligenceData,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepriceSuggestion {
    sku: String,
    offer_id: String,
    product_id: String,
    current_price_cny: Option<f64>,
    suggested_price_cny: Option<f64>,
    projected_margin: Option<f64>,
    reason: String,
}

const AD_RATE: f64 = 0.10;
const DAMAGE_RATE: f64 = 0.03;
const LABEL_FEE: f64 = 2.0;
const LOW_COMMISSION: f64 = 0.13;
const HIGH_COMMISSION: f64 = 0.15;
const LOGISTICS_COMMISSION: f64 = 0.02;
const COMMISSION_THRESHOLD: f64 = 134.0;

fn ensure(c: &Connection) -> Result<(), String> {
    insights::ensure(c)?;
    c.execute_batch("CREATE TABLE IF NOT EXISTS price_profit_monitor_cache(sku TEXT PRIMARY KEY,payload TEXT NOT NULL DEFAULT '{}',synced_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);")
        .map_err(|e| e.to_string())
}

fn positive(values: &[Option<f64>]) -> Option<f64> {
    values.iter().flatten().copied().find(|value| *value > 0.0)
}

fn minimum_positive(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a.filter(|x| *x > 0.0), b.filter(|x| *x > 0.0)) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (Some(x), None) | (None, Some(x)) => Some(x),
        _ => None,
    }
}

fn cross_border_freight(price_cny: Option<f64>, weight_kg: Option<f64>) -> Option<f64> {
    price_cny
        .zip(weight_kg.filter(|w| *w > 0.0))
        .and_then(|(price, weight)| cross_border_shipping(price, weight))
}

fn projected_margin(price: f64, cost: f64, weight: f64) -> Option<f64> {
    let freight = cross_border_shipping(price, weight)?;
    let commission = if price > COMMISSION_THRESHOLD { HIGH_COMMISSION } else { LOW_COMMISSION };
    let damage = (cost + LABEL_FEE + freight) * DAMAGE_RATE;
    Some((price - cost - price * AD_RATE - damage - freight - price * commission
        - price * LOGISTICS_COMMISSION - LABEL_FEE) / price)
}

fn minimum_price_for_margin(cost: f64, weight: f64, current: f64, warning_percent: f64) -> Option<(f64, f64)> {
    if !(cost > 0.0 && weight > 0.0 && current > 0.0 && warning_percent.is_finite()) { return None; }
    let target = warning_percent / 100.0;
    let start = (current * 100.0).ceil().max(1.0) as i64;
    // Shipping and commission are constant-form within these price bands.
    for (band_start, band_end) in [(1, 13_400), (13_401, 13_499), (13_500, 63_499), (63_500, 2_252_499)] {
        let mut low = start.max(band_start);
        if low > band_end { continue; }
        let mut high = band_end;
        if !projected_margin(high as f64 / 100.0, cost, weight).is_some_and(|m| m * 100.0 > warning_percent) { continue; }
        while low < high {
            let mid = low + (high - low) / 2;
            if projected_margin(mid as f64 / 100.0, cost, weight).is_some_and(|m| m > target) { high = mid; }
            else { low = mid + 1; }
        }
        let price = low as f64 / 100.0;
        if let Some(margin) = projected_margin(price, cost, weight) { return Some((price, margin)); }
    }
    None
}

#[cfg(test)]
mod reprice_tests {
    use super::{catalog_id, minimum_price_for_margin, projected_margin};
    use rusqlite::Connection;

    #[test]
    fn suggested_price_exceeds_warning_and_uses_full_cross_border_freight() {
        let (price, margin) = minimum_price_for_margin(22.5, 0.6, 95.0, 15.0).unwrap();
        assert!(margin > 0.15);
        assert!(projected_margin(price - 0.01, 22.5, 0.6).is_none_or(|previous| previous <= 0.15));
    }

    #[test]
    fn unsupported_freight_weight_has_no_suggestion() {
        assert!(minimum_price_for_margin(20.0, 30.0, 95.0, 15.0).is_none());
    }

    #[test]
    fn blank_catalog_id_does_not_hide_cached_id() {
        let c = Connection::open_in_memory().unwrap();
        let value: String = c.query_row("SELECT COALESCE(NULLIF('',''),NULLIF('987654',''),'')", [], |r| r.get(0)).unwrap();
        assert_eq!(value, "987654");
    }

    #[test]
    fn catalog_item_accepts_numeric_id_only() {
        assert_eq!(catalog_id(&serde_json::json!({"id": 1234})).as_deref(), Some("1234"));
        assert_eq!(catalog_id(&serde_json::json!({"id": "1234"})).as_deref(), Some("1234"));
        assert_eq!(catalog_id(&serde_json::json!({"id": "unknown"})), None);
    }
}

#[tauri::command]
pub async fn price_reprice_suggestions(skus: Vec<String>, state: State<'_, AppState>) -> Result<Vec<RepriceSuggestion>, String> {
    let snapshot = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || price_reprice_suggestions_blocking(skus, &snapshot))
        .await.map_err(|e| format!("利润试算任务异常：{e}"))?
}

fn price_reprice_suggestions_blocking(skus: Vec<String>, state: &AppState) -> Result<Vec<RepriceSuggestion>, String> {
    let c = db(state)?;
    let data = build_data(&c, state)?;
    if !data.is_cross_border { return Err("当前利润试算仅支持跨境模型；本土店不能套用跨境运费公式".into()); }
    let selected: std::collections::HashSet<&str> = skus.iter().map(String::as_str).collect();
    Ok(data.rows.into_iter().filter(|row| selected.contains(row.sku.as_str())).map(|row| {
        let result = row.purchase_cost_cny.zip(row.weight_kg).zip(row.final_pricing_cny)
            .and_then(|((cost, weight), current)| minimum_price_for_margin(cost, weight, current, data.warning_margin));
        let reason = if !row.warning { "当前利润率未低于预警线" }
            else if row.purchase_cost_cny.filter(|v| *v > 0.0).is_none() { "缺少采购成本" }
            else if row.weight_kg.filter(|v| *v > 0.0).is_none() { "缺少重量" }
            else if row.final_pricing_cny.is_none() { "缺少最终定价" }
            else if result.is_none() { "运费公式范围内无法达到预警利润率" }
            else { "可试算；Ozon 活动限制和补贴需另行核验" };
        RepriceSuggestion {
            sku: row.sku, offer_id: row.offer_id, product_id: row.product_id,
            current_price_cny: row.final_pricing_cny,
            suggested_price_cny: result.map(|v| v.0), projected_margin: result.map(|v| v.1),
            reason: reason.to_string(),
        }
    }).collect())
}

fn catalog_id(item: &serde_json::Value) -> Option<String> {
    item.get("id").or_else(|| item.get("product_id"))
        .and_then(|value| value.as_i64().map(|n| n.to_string()).or_else(|| value.as_str().map(str::to_string)))
        .filter(|value| value.parse::<i64>().is_ok_and(|id| id > 0))
}

#[tauri::command]
pub async fn resolve_reprice_product_ids(skus: Vec<String>, state: State<'_, AppState>) -> Result<std::collections::HashMap<String, String>, String> {
    let snapshot = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || resolve_reprice_product_ids_blocking(skus, &snapshot))
        .await.map_err(|e| format!("商品 ID 查询任务异常：{e}"))?
}

fn resolve_reprice_product_ids_blocking(skus: Vec<String>, state: &AppState) -> Result<std::collections::HashMap<String, String>, String> {
    let c = db(state)?;
    ensure(&c)?;
    let mut found = std::collections::HashMap::new();
    let mut missing = Vec::new();
    for sku in skus {
        let identity: (String, String) = c.query_row(
            "SELECT COALESCE(NULLIF(p.offer_id,''),pp.offer_id,''),COALESCE(NULLIF(p.product_id,''),NULLIF(pp.product_id,''),'') FROM (SELECT ?1 AS sku) k LEFT JOIN products p ON p.sku=k.sku LEFT JOIN product_price_cache pp ON pp.sku=k.sku",
            [&sku], |row| Ok((row.get(0)?, row.get(1)?)),
        ).map_err(|e| e.to_string())?;
        if identity.1.parse::<i64>().is_ok_and(|id| id > 0) { found.insert(sku, identity.1); }
        else { missing.push((sku, identity.0)); }
    }
    for batch in missing.chunks(100) {
        let offers: Vec<&str> = batch.iter().filter(|(_, offer)| !offer.is_empty()).map(|(_, offer)| offer.as_str()).collect();
        let numeric_skus: Vec<i64> = batch.iter().filter_map(|(sku, _)| sku.parse().ok()).collect();
        if offers.is_empty() && numeric_skus.is_empty() { continue; }
        let response = seller_post(&c, "/v3/product/info/list", &serde_json::json!({"offer_id":offers,"product_id":[],"sku":numeric_skus}))?;
        let items = response.get("items").or_else(|| response.pointer("/result/items"))
            .and_then(|value| value.as_array()).ok_or("Ozon 商品信息接口未返回商品列表")?;
        for (sku, offer) in batch {
            let matches: Vec<&serde_json::Value> = items.iter().filter(|item| {
                let item_sku = item.get("sku").and_then(|value| value.as_i64().map(|n| n.to_string()).or_else(|| value.as_str().map(str::to_string)));
                let item_offer = item.get("offer_id").and_then(|value| value.as_str());
                if let Some(item_sku) = item_sku { item_sku == *sku }
                else { !offer.is_empty() && item_offer == Some(offer.as_str()) }
            }).collect();
            if matches.len() != 1 { continue; }
            let Some(id) = catalog_id(matches[0]) else { continue; };
            c.execute("UPDATE products SET product_id=?1,updated_at=CURRENT_TIMESTAMP WHERE sku=?2 AND CAST(product_id AS INTEGER)<=0", params![id,sku]).map_err(|e|e.to_string())?;
            c.execute("UPDATE product_price_cache SET product_id=?1 WHERE sku=?2 AND CAST(product_id AS INTEGER)<=0", params![id,sku]).map_err(|e|e.to_string())?;
            found.insert(sku.clone(), id);
        }
    }
    Ok(found)
}

#[tauri::command]
pub async fn price_reprice_validate(sku: String, price_cny: f64, state: State<'_, AppState>) -> Result<f64, String> {
    let snapshot = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || price_reprice_validate_blocking(sku, price_cny, &snapshot))
        .await.map_err(|e| format!("利润校验任务异常：{e}"))?
}

fn price_reprice_validate_blocking(sku: String, price_cny: f64, state: &AppState) -> Result<f64, String> {
    if !price_cny.is_finite() || price_cny <= 0.0 { return Err("目标售价必须大于 0".into()); }
    let c = db(state)?;
    let data = build_data(&c, state)?;
    if !data.is_cross_border { return Err("仅支持跨境店利润模型".into()); }
    let row = data.rows.iter().find(|row| row.sku == sku).ok_or("商品未找到")?;
    let cost = row.purchase_cost_cny.filter(|v| *v > 0.0).ok_or("缺少采购成本")?;
    let weight = row.weight_kg.filter(|v| *v > 0.0).ok_or("缺少重量")?;
    let margin = projected_margin(price_cny, cost, weight).ok_or("目标售价或重量超出跨境运费公式范围")?;
    if margin * 100.0 <= data.warning_margin { return Err(format!("预计利润率 {:.2}% 未高于预警线 {:.2}%", margin * 100.0, data.warning_margin)); }
    Ok(margin)
}

fn warning_margin(c: &Connection) -> f64 {
    c.query_row(
        "SELECT value FROM settings WHERE key='cross_profit_warning_margin'",
        [],
        |r| r.get::<_, String>(0),
    )
    .ok()
    .and_then(|v| v.parse::<f64>().ok())
    .unwrap_or(15.0)
}

fn build_data(c: &Connection, state: &AppState) -> Result<PriceIntelligenceData, String> {
    ensure(c)?;
    let is_cross_border = active_shop_kind(state)? == "cross_border";
    let rate = rub_per_cny_for(state, c)?;
    let limit = warning_margin(c);
    let mut stmt = c.prepare("WITH known AS(SELECT sku FROM products UNION SELECT sku FROM product_costs UNION SELECT sku FROM product_price_cache) SELECT k.sku,COALESCE(NULLIF(p.offer_id,''),pp.offer_id,''),COALESCE(p.name,''),COALESCE(pp.currency_code,'RUB'),pp.price,pp.old_price,pp.marketing_price,pp.marketing_seller_price,pp.retail_price,pc.unit_cost_cny,COALESCE(pc.first_mile_cost_cny,CASE WHEN pc.first_mile_cost IS NULL THEN NULL ELSE pc.first_mile_cost/?1 END),pc.length_cm,pc.width_cm,pc.height_cm,pc.weight_kg,COALESCE(pp.synced_at,''),COALESCE(NULLIF(p.product_id,''),NULLIF(pp.product_id,''),'') FROM known k LEFT JOIN products p ON p.sku=k.sku LEFT JOIN product_price_cache pp ON pp.sku=k.sku LEFT JOIN product_costs pc ON pc.sku=k.sku ORDER BY CASE WHEN pc.unit_cost_cny>0 AND pc.weight_kg>0 AND pc.length_cm>0 AND pc.width_cm>0 AND pc.height_cm>0 THEN 0 ELSE 1 END,COALESCE(p.offer_id,k.sku)").map_err(|e|e.to_string())?;
    let rows = stmt
        .query_map([rate], |r| {
            let sku: String = r.get(0)?;
            let offer_id: String = r.get(1)?;
            let product_name: String = r.get(2)?;
            let currency_code: String = r.get(3)?;
            let price: Option<f64> = r.get(4)?;
            let old_price: Option<f64> = r.get(5)?;
            let marketing_price: Option<f64> = r.get(6)?;
            let marketing_seller_price: Option<f64> = r.get(7)?;
            let retail_price: Option<f64> = r.get(8)?;
            let purchase: Option<f64> = r.get(9)?;
            let first_mile: Option<f64> = r.get(10)?;
            let length: Option<f64> = r.get(11)?;
            let width: Option<f64> = r.get(12)?;
            let height: Option<f64> = r.get(13)?;
            let weight: Option<f64> = r.get(14)?;
            let synced_at: String = r.get(15)?;
            let product_id: String = r.get(16)?;

            // Ozon: marketing_price is the storefront price after platform subsidies;
            // marketing_seller_price is the seller price after seller promotions.
            // Never substitute a seller-controlled price for the buyer-facing
            // storefront price. If Price Details is unavailable, keep it empty.
            let frontend = positive(&[marketing_price]);
            let promotion = positive(&[marketing_seller_price, price, retail_price]);
            let original = positive(&[old_price, retail_price, price]);
            let final_pricing = minimum_positive(promotion, original);
            let subsidy_amount = final_pricing
                .zip(frontend)
                .map(|(basis, front)| (basis - front).max(0.0));
            let subsidy_rate = subsidy_amount
                .zip(final_pricing)
                .and_then(|(amount, basis)| (basis > 0.0).then_some(amount / basis));
            let volume_l = length.zip(width).zip(height).and_then(|((l, w), h)| {
                (l > 0.0 && w > 0.0 && h > 0.0).then_some(l * w * h / 1000.0)
            });
            let final_cny = final_pricing.and_then(|x| {
                if currency_code.eq_ignore_ascii_case("CNY") {
                    Some(x)
                } else {
                    (rate > 0.0).then_some(x / rate)
                }
            });
            // The cross-border tier already includes all freight. Never add
            // first-mile cost here; local stores use a separate settlement model.
            let freight = if is_cross_border {
                cross_border_freight(final_cny, weight)
            } else { None };
            let mut missing = Vec::new();
            if final_pricing.is_none() {
                missing.push("价格未读取".to_string());
            }
            if frontend.is_none() {
                missing.push("前台价接口无权限或未返回".to_string());
            }
            if is_cross_border {
                if purchase.filter(|x| *x > 0.0).is_none() {
                    missing.push("采购成本".to_string());
                }
                if weight.filter(|x| *x > 0.0).is_none() {
                    missing.push("重量".to_string());
                }
                if final_cny.is_some() && weight.filter(|x| *x > 0.0).is_some() && freight.is_none() {
                    missing.push("运费超出公式区间".to_string());
                }
            }
            // Price-based fees are useful even when the product has no cost or
            // weight data. Damage and profit use the cross-border freight only.
            let pricing = if is_cross_border { final_cny } else { None };
            let commission_rate = pricing.map(|a| if a > COMMISSION_THRESHOLD { HIGH_COMMISSION } else { LOW_COMMISSION });
            let advertising = pricing.map(|a| a * AD_RATE);
            let commission = pricing.zip(commission_rate).map(|(a, rate)| a * rate);
            let logistics = pricing.map(|a| a * LOGISTICS_COMMISSION);
            let damage = purchase.filter(|x| *x > 0.0).zip(freight)
                .map(|(cost, shipping)| (cost + LABEL_FEE + shipping) * DAMAGE_RATE);
            let profit = pricing.zip(purchase.filter(|x| *x > 0.0)).zip(freight).zip(damage)
                .map(|(((a, cost), shipping), loss)| {
                    a - cost - a * AD_RATE - loss - shipping
                        - a * commission_rate.unwrap_or(LOW_COMMISSION)
                        - a * LOGISTICS_COMMISSION - LABEL_FEE
                });
            let margin = profit.zip(pricing).map(|(value, a)| value / a);
            Ok(PriceProfitRow {
                sku,
                product_id,
                offer_id,
                product_name,
                currency_code,
                frontend_price: frontend,
                promotion_price: promotion,
                original_price: original,
                final_pricing,
                subsidy_amount,
                subsidy_rate,
                purchase_cost_cny: purchase,
                first_mile_cny: first_mile,
                cross_border_freight_cny: freight,
                freight_cny: freight,
                volume_l,
                weight_kg: weight,
                final_pricing_cny: final_cny,
                commission_rate,
                advertising_cny: advertising,
                damage_cny: damage,
                commission_cny: commission,
                logistics_commission_cny: logistics,
                label_fee_cny: is_cross_border.then_some(LABEL_FEE),
                profit_cny: profit,
                profit_margin: margin,
                warning: margin.is_some_and(|m| m * 100.0 < limit),
                missing_fields: missing,
                synced_at,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(PriceIntelligenceData {
        rows,
        is_cross_border,
        rub_per_cny: rate,
        warning_margin: limit,
        advertising_rate: AD_RATE,
        damage_rate: DAMAGE_RATE,
        label_fee_cny: LABEL_FEE,
        low_commission_rate: LOW_COMMISSION,
        high_commission_rate: HIGH_COMMISSION,
        logistics_commission_rate: LOGISTICS_COMMISSION,
        commission_threshold_cny: COMMISSION_THRESHOLD,
    })
}

#[tauri::command]
pub async fn price_intelligence(state: State<'_, AppState>) -> Result<PriceIntelligenceData, String> {
    let snapshot = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let c = db(&snapshot)?;
        build_data(&c, &snapshot)
    }).await.map_err(|e| format!("利润缓存读取任务异常：{e}"))?
}

#[tauri::command]
pub async fn refresh_price_intelligence(
    skus: Vec<String>,
    state: State<'_, AppState>,
) -> Result<RefreshResult, String> {
    let snapshot = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || refresh_price_intelligence_blocking(skus, &snapshot))
        .await
        .map_err(|e| format!("后台价格同步任务异常：{e}"))?
}

fn refresh_price_intelligence_blocking(
    skus: Vec<String>,
    state: &AppState,
) -> Result<RefreshResult, String> {
    let c = db(state)?;
    ensure(&c)?;
    let targets = if skus.is_empty() {
        let mut stmt = c.prepare("SELECT p.sku FROM products p LEFT JOIN product_costs pc ON pc.sku=p.sku ORDER BY CASE WHEN pc.unit_cost_cny>0 AND pc.weight_kg>0 AND pc.length_cm>0 AND pc.width_cm>0 AND pc.height_cm>0 THEN 0 ELSE 1 END,p.sku").map_err(|e|e.to_string())?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        rows
    } else {
        skus.into_iter()
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect()
    };
    let (refreshed, errors) = insights::refresh_prices_batch(&c, &targets)?;
    let data = build_data(&c, state)?;
    let target_set: std::collections::HashSet<&str> = targets.iter().map(String::as_str).collect();
    for row in data.rows.iter().filter(|row| target_set.contains(row.sku.as_str())) {
        let payload = serde_json::to_string(row).map_err(|e| e.to_string())?;
        c.execute("INSERT INTO price_profit_monitor_cache(sku,payload,synced_at)VALUES(?1,?2,CURRENT_TIMESTAMP)ON CONFLICT(sku)DO UPDATE SET payload=excluded.payload,synced_at=CURRENT_TIMESTAMP",params![row.sku,payload]).map_err(|e|e.to_string())?;
    }
    Ok(RefreshResult {
        requested: targets.len(),
        refreshed,
        failed: errors.len(),
        errors,
        data,
    })
}

#[tauri::command]
pub fn save_profit_monitor_settings(
    form: ProfitMonitorSettings,
    state: State<AppState>,
) -> Result<PriceIntelligenceData, String> {
    if !form.warning_margin.is_finite()
        || form.warning_margin < -100.0
        || form.warning_margin > 100.0
    {
        return Err("利润率预警值必须在 -100% 到 100% 之间".into());
    }
    let c = db(&state)?;
    c.execute("INSERT INTO settings(key,value)VALUES('cross_profit_warning_margin',?1)ON CONFLICT(key)DO UPDATE SET value=excluded.value",[form.warning_margin.to_string()]).map_err(|e|e.to_string())?;
    build_data(&c, &state)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lower_seller_price_is_pricing_basis() {
        assert_eq!(minimum_positive(Some(120.0), Some(150.0)), Some(120.0));
    }
    #[test]
    fn subsidy_is_non_negative() {
        let x = (120.0_f64 - 100.0).max(0.0) / 120.0;
        assert!((x - 1.0 / 6.0).abs() < 1e-9);
    }
    #[test]
    fn commission_boundary_is_strict() {
        assert_eq!(
            if 134.0 > COMMISSION_THRESHOLD {
                HIGH_COMMISSION
            } else {
                LOW_COMMISSION
            },
            LOW_COMMISSION
        );
        assert_eq!(
            if 134.01 > COMMISSION_THRESHOLD {
                HIGH_COMMISSION
            } else {
                LOW_COMMISSION
            },
            HIGH_COMMISSION
        );
    }
    #[test]
    fn cross_border_freight_is_the_complete_freight() {
        assert_eq!(cross_border_freight(Some(95.0), Some(0.6)), Some(41.57));
        let freight = cross_border_freight(Some(95.0), Some(0.6)).unwrap();
        let damage = (22.5 + LABEL_FEE + freight) * DAMAGE_RATE;
        let profit = 95.0 - 22.5 - 95.0 * AD_RATE - damage - freight
            - 95.0 * LOW_COMMISSION - 95.0 * LOGISTICS_COMMISSION - LABEL_FEE;
        assert!((damage - 1.9821).abs() < 1e-8);
        assert!((profit - 3.1979).abs() < 1e-8);
    }
}
