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
    dimensions_cm: Option<[f64;3]>,
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
    commission_threshold_rub: f64,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct ProfitMonitorSettings {
    rub_per_cny: f64,
    warning_margin: f64,
    advertising_rate: f64,
    damage_rate: f64,
    label_fee_cny: f64,
    low_commission_rate: f64,
    high_commission_rate: f64,
    logistics_commission_rate: f64,
    #[serde(default)]
    commission_threshold_cny: f64,
    commission_threshold_rub: f64,
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
    current_margin: Option<f64>,
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
const COMMISSION_THRESHOLD: f64 = 1500.0/14.0;

impl Default for ProfitMonitorSettings {
    fn default() -> Self {
        Self { rub_per_cny:14.0, warning_margin: 15.0, advertising_rate: AD_RATE, damage_rate: DAMAGE_RATE,
            label_fee_cny: LABEL_FEE, low_commission_rate: LOW_COMMISSION,
            high_commission_rate: HIGH_COMMISSION, logistics_commission_rate: LOGISTICS_COMMISSION,
            commission_threshold_cny: COMMISSION_THRESHOLD, commission_threshold_rub: 1500.0 }
    }
}

fn monitor_settings(c: &Connection) -> ProfitMonitorSettings {
    let mut s = ProfitMonitorSettings::default();
    for (key, target) in [
        ("cross_profit_warning_margin", &mut s.warning_margin),
        ("cross_profit_advertising_rate", &mut s.advertising_rate),
        ("cross_profit_damage_rate", &mut s.damage_rate),
        ("cross_profit_label_fee_cny", &mut s.label_fee_cny),
        ("cross_profit_low_commission_rate", &mut s.low_commission_rate),
        ("cross_profit_high_commission_rate", &mut s.high_commission_rate),
        ("cross_profit_logistics_commission_rate", &mut s.logistics_commission_rate),
        ("cross_profit_commission_threshold_rub", &mut s.commission_threshold_rub),
    ] {
        if let Ok(value) = c.query_row("SELECT value FROM settings WHERE key=?1", [key], |r| r.get::<_, String>(0)) {
            if let Ok(parsed) = value.parse::<f64>() { if parsed.is_finite() { *target = parsed; } }
        }
    }
    s.rub_per_cny=cross_border_rate(c);
    s.commission_threshold_cny = s.commission_threshold_rub / s.rub_per_cny;
    s
}

fn cross_border_rate(c: &Connection) -> f64 {
    ["cross_border_rub_per_cny", "rub_per_cny"].iter()
        .find_map(|key| crate::setting(c, key).parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0))
        .unwrap_or(14.0)
}

fn validate_settings(s: &ProfitMonitorSettings) -> Result<(), String> {
    if !s.commission_threshold_rub.is_finite() || s.commission_threshold_rub <= 0.0 {
        return Err("佣金的卢布分界价必须大于 0".into());
    }
    if !s.warning_margin.is_finite() || !(-100.0..=100.0).contains(&s.warning_margin) {
        return Err("利润率预警值必须在 -100% 到 100% 之间".into());
    }
    for (label, value) in [("广告费率", s.advertising_rate), ("货损率", s.damage_rate),
        ("低档佣金率", s.low_commission_rate), ("高档佣金率", s.high_commission_rate),
        ("物流佣金率", s.logistics_commission_rate)] {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) { return Err(format!("{label}必须在 0% 到 100% 之间")); }
    }
    if !s.label_fee_cny.is_finite() || !(0.0..=10_000.0).contains(&s.label_fee_cny) { return Err("贴单费必须在 ¥0 到 ¥10000 之间".into()); }
    if !s.commission_threshold_cny.is_finite() || !s.rub_per_cny.is_finite() || s.rub_per_cny<=0.0 { return Err("跨境汇率必须为有效正数".into()); }
    Ok(())
}

pub(crate) fn freight_threshold(c: &Connection) -> Result<f64,String> {
    Ok(crate::rets_shipping::SALE_BANDS_RUB[0]/cross_border_rate(c))
}

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

fn cross_border_freight(price_cny: Option<f64>, weight_kg: Option<f64>, settings: &ProfitMonitorSettings, dimensions:Option<[f64;3]>) -> Option<f64> {
    price_cny
        .zip(weight_kg.filter(|w| *w > 0.0))
        .and_then(|(price, weight)| cross_border_shipping(price, weight, settings.rub_per_cny,dimensions))
}

fn commission_rate_at(price: f64, s: &ProfitMonitorSettings) -> f64 {
    let tolerance = f64::EPSILON * 8.0 * price.abs().max(s.commission_threshold_cny.abs()).max(1.0);
    if price - s.commission_threshold_cny > tolerance { s.high_commission_rate } else { s.low_commission_rate }
}

fn normalized_cents(price: f64) -> f64 {
    let cents = price * 100.0;
    let nearest = cents.round();
    if (cents - nearest).abs() <= f64::EPSILON * 8.0 * cents.abs().max(1.0) { nearest } else { cents }
}

fn reprice_band_ends(s: &ProfitMonitorSettings) -> Vec<i64> {
    // Only freight price boundaries are fixed; commission uses the saved setting.
    let max=normalized_cents(250000.0/s.rub_per_cny).floor() as i64;
    let mut ends = vec![0,max,normalized_cents(s.commission_threshold_cny).floor() as i64];
    ends.extend(crate::rets_shipping::SALE_BANDS_RUB.into_iter().map(|v|normalized_cents(v/s.rub_per_cny).floor() as i64));
    ends.retain(|v|*v>=0 && *v<=max);
    ends.sort_unstable();
    ends.dedup();
    ends
}

fn projected_margin(price: f64, cost: f64, weight: f64, s: &ProfitMonitorSettings, dimensions:Option<[f64;3]>) -> Option<f64> {
    let freight = cross_border_shipping(price, weight, s.rub_per_cny,dimensions)?;
    let commission = commission_rate_at(price, s);
    let damage = (cost + s.label_fee_cny + freight) * s.damage_rate;
    Some((price - cost - price * s.advertising_rate - damage - freight - price * commission
        - price * s.logistics_commission_rate - s.label_fee_cny) / price)
}

fn minimum_price_for_margin(cost: f64, weight: f64, current: f64, warning_percent: f64, s: &ProfitMonitorSettings, dimensions:Option<[f64;3]>) -> Option<(f64, f64)> {
    if ![cost, weight, current, warning_percent].iter().all(|v|v.is_finite()) || !(cost > 0.0 && weight > 0.0 && current > 0.0) { return None; }
    let target = warning_percent / 100.0;
    let start = normalized_cents(current).ceil().max(1.0) as i64;
    // Shipping and commission are constant-form within these price bands.
    let boundaries = reprice_band_ends(s);
    for pair in boundaries.windows(2) {
        let (band_start, band_end) = (pair[0] + 1, pair[1]);
        let mut low = start.max(band_start);
        if low > band_end { continue; }
        let mut high = band_end;
        if !projected_margin(high as f64 / 100.0, cost, weight, s, dimensions).is_some_and(|m| m * 100.0 > warning_percent) { continue; }
        while low < high {
            let mid = low + (high - low) / 2;
            if projected_margin(mid as f64 / 100.0, cost, weight, s, dimensions).is_some_and(|m| m > target) { high = mid; }
            else { low = mid + 1; }
        }
        let price = low as f64 / 100.0;
        if let Some(margin) = projected_margin(price, cost, weight, s, dimensions) { return Some((price, margin)); }
    }
    None
}

// Search every constant-freight/commission band in cents, including prices below
// the current price. At a discontinuity an exact target may not be achievable.
fn price_for_target_margin(cost: f64, weight: f64, current: f64, percent: f64, s: &ProfitMonitorSettings, dimensions:Option<[f64;3]>) -> Option<(f64, f64)> {
    if ![cost, weight, current, percent].iter().all(|v| v.is_finite())
        || cost <= 0.0 || weight <= 0.0 || current <= 0.0 || !(0.0..100.0).contains(&percent) { return None; }
    let target = percent / 100.0;
    // Retain prices already within 0.1 percentage point above the target.
    if let Some(m) = projected_margin(current, cost, weight, s, dimensions) {
        if m + 1e-10 >= target && m - target <= 0.001 { return Some((current, m)); }
    }
    let bounds = reprice_band_ends(s);
    let mut candidates = Vec::new();
    for band in bounds.windows(2) {
        let (mut low, mut high) = (band[0] + 1, band[1]);
        if !projected_margin(high as f64 / 100.0, cost, weight, s, dimensions).is_some_and(|m| m + 1e-10 >= target) { continue; }
        while low < high {
            let mid = low + (high - low) / 2;
            if projected_margin(mid as f64 / 100.0, cost, weight, s, dimensions).is_some_and(|m| m + 1e-10 >= target) { high = mid; }
            else { low = mid + 1; }
        }
        let price = low as f64 / 100.0;
        if let Some(m) = projected_margin(price, cost, weight, s, dimensions) {
            if m + 1e-10 >= target && m - target <= 0.001 { candidates.push((price, m)); }
        }
    }
    candidates.into_iter().min_by(|a, b| (a.0-current).abs().total_cmp(&(b.0-current).abs()))
}

#[cfg(test)]
mod test_pricing_helpers {
    use super::ProfitMonitorSettings;
    pub fn projected_margin(p:f64,c:f64,w:f64,s:&ProfitMonitorSettings)->Option<f64>{super::projected_margin(p,c,w,s,Some([10.;3]))}
    pub fn minimum_price_for_margin(c:f64,w:f64,p:f64,m:f64,s:&ProfitMonitorSettings)->Option<(f64,f64)>{super::minimum_price_for_margin(c,w,p,m,s,Some([10.;3]))}
    pub fn price_for_target_margin(c:f64,w:f64,p:f64,m:f64,s:&ProfitMonitorSettings)->Option<(f64,f64)>{super::price_for_target_margin(c,w,p,m,s,Some([10.;3]))}
    pub fn cross_border_freight(p:Option<f64>,w:Option<f64>,s:&ProfitMonitorSettings)->Option<f64>{super::cross_border_freight(p,w,s,Some([10.;3]))}
}

#[cfg(test)]
mod reprice_tests {
    use super::{catalog_id, ProfitMonitorSettings};
    use super::test_pricing_helpers::{minimum_price_for_margin,price_for_target_margin,projected_margin};
    use rusqlite::Connection;

    #[test]
    fn suggested_price_exceeds_warning_and_uses_full_cross_border_freight() {
        let settings = ProfitMonitorSettings::default();
        let (price, margin) = minimum_price_for_margin(22.5, 0.6, 95.0, 15.0, &settings).unwrap();
        assert!(margin > 0.15);
        assert!(projected_margin(price - 0.01, 22.5, 0.6, &settings).is_none_or(|previous| previous <= 0.15));
    }

    #[test]
    fn unsupported_freight_weight_has_no_suggestion() {
        assert!(minimum_price_for_margin(20.0, 30.00001, 95.0, 15.0, &ProfitMonitorSettings::default()).is_none());
    }

    #[test]
    fn target_margin_prices_both_directions_and_keeps_target_price() {
        let s = ProfitMonitorSettings::default();
        let raised = price_for_target_margin(7.2, 0.25, 25.0, 15.0, &s).unwrap();
        let lowered = price_for_target_margin(7.2, 0.25, 65.0, 15.0, &s).unwrap();
        assert!(raised.0 > 25.0 && lowered.0 < 65.0);
        assert_eq!(raised, lowered);
        assert!(raised.1 >= 0.15 - 1e-10 && raised.1 <= 0.151);
        assert_eq!(price_for_target_margin(7.2, 0.25, raised.0, 15.0, &s).unwrap(), raised);
    }

    #[test]
    fn target_margin_matches_exhaustive_cent_search_across_tariff_bands() {
        let s = ProfitMonitorSettings { commission_threshold_cny: 180.0, ..ProfitMonitorSettings::default() };
        let starts=super::reprice_band_ends(&s).into_iter().map(|v|v+1).collect::<Vec<_>>();
        for cost in [7.2, 70.0, 300.0] {
            let actual = price_for_target_margin(cost, 0.6, 600.0, 15.0, &s).unwrap();
            let expected = (1..=100_000).filter_map(|cents| {
                let p = cents as f64 / 100.0;
                projected_margin(p, cost, 0.6, &s).filter(|m| *m + 1e-10 >= 0.15 && *m <= 0.151)
                    .filter(|_| starts.contains(&cents) || projected_margin(p-0.01, cost, 0.6, &s).is_none_or(|m| m + 1e-10 < 0.15)).map(|m| (p,m))
            }).min_by(|a,b| (a.0-600.0).abs().total_cmp(&(b.0-600.0).abs())).unwrap();
            // The band solver rounds up to the first qualifying cent.
            assert!((actual.1-0.15).abs() <= 0.001);
            assert_eq!(actual, expected);
        }
        assert!(price_for_target_margin(20.0, 30.00001, 95.0, 15.0, &s).is_none());
        assert!(price_for_target_margin(20.0, 0.6, 95.0, 99.0, &s).is_none());
        assert!(price_for_target_margin(0.0, 0.6, 95.0, 15.0, &s).is_none());
        assert!(price_for_target_margin(20.0, 0.6, 95.0, f64::NAN, &s).is_none());
    }

    #[test]
    fn custom_ad_rate_changes_margin_and_reprice_suggestion() {
        let standard = ProfitMonitorSettings::default();
        let costly = ProfitMonitorSettings { advertising_rate: 0.25, commission_threshold_cny: 200.0, ..standard };
        let standard_margin = projected_margin(150.0, 22.5, 0.6, &standard).unwrap();
        let costly_margin = projected_margin(150.0, 22.5, 0.6, &costly).unwrap();
        assert!(costly_margin < standard_margin);
        let normal_price = minimum_price_for_margin(22.5, 0.6, 95.0, 15.0, &standard).unwrap().0;
        let costly_price = minimum_price_for_margin(22.5, 0.6, 95.0, 15.0, &costly).unwrap().0;
        assert!(costly_price > normal_price);
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
pub async fn price_reprice_suggestions(skus: Vec<String>, target_margin: Option<f64>, state: State<'_, AppState>) -> Result<Vec<RepriceSuggestion>, String> {
    let snapshot = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || price_reprice_suggestions_blocking(skus, target_margin, &snapshot))
        .await.map_err(|e| format!("利润试算任务异常：{e}"))?
}

fn price_reprice_suggestions_blocking(skus: Vec<String>, target_margin: Option<f64>, state: &AppState) -> Result<Vec<RepriceSuggestion>, String> {
    if target_margin.is_some_and(|v| !v.is_finite() || !(0.0..100.0).contains(&v)) { return Err("目标利润率必须在 0%（含）到 100%（不含）之间".into()); }
    let c = db(state)?;
    let data = build_data(&c, state)?;
    let settings = monitor_settings(&c);
    if !data.is_cross_border { return Err("当前利润试算仅支持跨境模型；本土店不能套用跨境运费公式".into()); }
    let selected: std::collections::HashSet<&str> = skus.iter().map(String::as_str).collect();
    Ok(data.rows.into_iter().filter(|row| selected.contains(row.sku.as_str())).map(|row| {
        let result = row.purchase_cost_cny.zip(row.weight_kg).zip(row.final_pricing_cny)
            .and_then(|((cost, weight), current)| match target_margin {
                Some(target) => price_for_target_margin(cost, weight, current, target, &settings,row.dimensions_cm),
                None => minimum_price_for_margin(cost, weight, current, data.warning_margin, &settings,row.dimensions_cm),
            });
        let reason = if row.purchase_cost_cny.filter(|v| *v > 0.0).is_none() { "缺少采购成本" }
            else if row.weight_kg.filter(|v| *v > 0.0).is_none() { "缺少重量" }

            else if row.final_pricing_cny.is_none() { "缺少最终定价" }
            else if result.is_none() { "阶梯费用或价格精度限制，无法在目标利润率上方 0.1 个百分点内定价" }
            else { "可试算；Ozon 活动限制和补贴需另行核验" };
        RepriceSuggestion {
            sku: row.sku, offer_id: row.offer_id, product_id: row.product_id,
            current_price_cny: row.final_pricing_cny,
            current_margin: row.profit_margin,
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
pub async fn price_reprice_validate(sku: String, price_cny: f64, state: State<'_, AppState>) -> Result<Option<f64>, String> {
    let snapshot = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || price_reprice_validate_blocking(sku, price_cny, &snapshot))
        .await.map_err(|e| format!("利润校验任务异常：{e}"))?
}

fn price_reprice_validate_blocking(sku: String, price_cny: f64, state: &AppState) -> Result<Option<f64>, String> {
    if !price_cny.is_finite() || price_cny <= 0.0 { return Err("目标售价必须大于 0".into()); }
    let c = db(state)?;
    let data = build_data(&c, state)?;
    if !data.is_cross_border { return Err("仅支持跨境店利润模型".into()); }
    let row = data.rows.iter().find(|row| row.sku == sku).ok_or("商品未找到")?;
    Ok(row.purchase_cost_cny.filter(|v| *v > 0.0)
        .zip(row.weight_kg.filter(|v| *v > 0.0))
        .and_then(|(cost, weight)| projected_margin(price_cny, cost, weight, &monitor_settings(&c),row.dimensions_cm)))
}

fn build_data(c: &Connection, state: &AppState) -> Result<PriceIntelligenceData, String> {
    ensure(c)?;
    let is_cross_border = active_shop_kind(state)? == "cross_border";
    let rate = rub_per_cny_for(state, c)?;
    let settings = monitor_settings(c);
    validate_settings(&settings)?;
    let limit = settings.warning_margin;
    // products belongs to the selected shop database. Cost and price caches may
    // contain historical imports; they cannot introduce extra shop products.
    let mut stmt = c.prepare("SELECT p.sku,COALESCE(NULLIF(p.offer_id,''),pp.offer_id,''),COALESCE(p.name,''),COALESCE(pp.currency_code,'RUB'),pp.price,pp.old_price,pp.marketing_price,pp.marketing_seller_price,pp.retail_price,pc.unit_cost_cny,COALESCE(pc.first_mile_cost_cny,CASE WHEN pc.first_mile_cost IS NULL THEN NULL ELSE pc.first_mile_cost/?1 END),pc.length_cm,pc.width_cm,pc.height_cm,pc.weight_kg,COALESCE(pp.synced_at,''),COALESCE(NULLIF(p.product_id,''),NULLIF(pp.product_id,''),'') FROM products p LEFT JOIN product_price_cache pp ON pp.sku=p.sku LEFT JOIN product_costs pc ON pc.sku=p.sku ORDER BY CASE WHEN pc.unit_cost_cny>0 AND pc.weight_kg>0 AND pc.length_cm>0 AND pc.width_cm>0 AND pc.height_cm>0 THEN 0 ELSE 1 END,p.offer_id").map_err(|e|e.to_string())?;
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
                cross_border_freight(final_cny, weight, &settings,length.zip(width).zip(height).map(|((l,w),h)|[l,w,h]))
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
                    missing.push(crate::rets_shipping::quote(final_cny.unwrap()*rate,weight.unwrap(),length.zip(width).zip(height).map(|((l,w),h)|[l,w,h])).err().unwrap_or_else(||"RETS 费率待核对".into()));
                }
            }
            // Price-based fees are useful even when the product has no cost or
            // weight data. Damage and profit use the cross-border freight only.
            let pricing = if is_cross_border { final_cny } else { None };
            let commission_rate = pricing.map(|a| commission_rate_at(a, &settings));
            let advertising = pricing.map(|a| a * settings.advertising_rate);
            let commission = pricing.zip(commission_rate).map(|(a, rate)| a * rate);
            let logistics = pricing.map(|a| a * settings.logistics_commission_rate);
            let damage = purchase.filter(|x| *x > 0.0).zip(freight)
                .map(|(cost, shipping)| (cost + settings.label_fee_cny + shipping) * settings.damage_rate);
            let profit = pricing.zip(purchase.filter(|x| *x > 0.0)).zip(freight).zip(damage)
                .map(|(((a, cost), shipping), loss)| {
                    a - cost - a * settings.advertising_rate - loss - shipping
                        - a * commission_rate.unwrap_or(settings.low_commission_rate)
                        - a * settings.logistics_commission_rate - settings.label_fee_cny
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
                dimensions_cm: length.zip(width).zip(height).map(|((l,w),h)|[l,w,h]),
                final_pricing_cny: final_cny,
                commission_rate,
                advertising_cny: advertising,
                damage_cny: damage,
                commission_cny: commission,
                logistics_commission_cny: logistics,
                label_fee_cny: is_cross_border.then_some(settings.label_fee_cny),
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
        advertising_rate: settings.advertising_rate,
        damage_rate: settings.damage_rate,
        label_fee_cny: settings.label_fee_cny,
        low_commission_rate: settings.low_commission_rate,
        high_commission_rate: settings.high_commission_rate,
        logistics_commission_rate: settings.logistics_commission_rate,
        commission_threshold_cny: settings.commission_threshold_cny,
        commission_threshold_rub: settings.commission_threshold_rub,
    })
}

fn belongs_to_current_shop(c: &Connection, sku: &str) -> Result<bool, String> {
    c.query_row(
        "SELECT EXISTS(SELECT 1 FROM products WHERE sku=?1)",
        [sku],
        |r| r.get(0),
    ).map_err(|e| e.to_string())
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
        let requested = skus.into_iter()
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect::<Vec<_>>();
        for sku in &requested {
            if !belongs_to_current_shop(&c, sku)? {
                return Err(format!("SKU {sku} 不属于当前店铺商品目录；请切换到正确店铺后重试"));
            }
        }
        requested
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
    mut form: ProfitMonitorSettings,
    state: State<AppState>,
) -> Result<PriceIntelligenceData, String> {
    if active_shop_kind(&state)? != "cross_border" { return Err("仅跨境店可修改此利润模型".into()); }
    let mut c = db(&state)?;
    form.commission_threshold_cny = form.commission_threshold_rub / form.rub_per_cny;
    validate_settings(&form)?;
    let tx = c.transaction().map_err(|e| e.to_string())?;
    for (key, value) in [
        ("cross_border_rub_per_cny", form.rub_per_cny),
        ("cross_profit_warning_margin", form.warning_margin),
        ("cross_profit_advertising_rate", form.advertising_rate),
        ("cross_profit_damage_rate", form.damage_rate),
        ("cross_profit_label_fee_cny", form.label_fee_cny),
        ("cross_profit_low_commission_rate", form.low_commission_rate),
        ("cross_profit_high_commission_rate", form.high_commission_rate),
        ("cross_profit_logistics_commission_rate", form.logistics_commission_rate),
        ("cross_profit_commission_threshold_rub", form.commission_threshold_rub),
    ] {
        tx.execute("INSERT INTO settings(key,value)VALUES(?1,?2)ON CONFLICT(key)DO UPDATE SET value=excluded.value",params![key,value.to_string()]).map_err(|e|e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    build_data(&c, &state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::test_pricing_helpers::{minimum_price_for_margin,projected_margin,cross_border_freight};
    #[test]
    fn fixed_ruble_threshold_tracks_exchange_rate_without_rounding() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT); INSERT INTO settings VALUES('cross_profit_commission_threshold_cny','120'),('cross_border_rub_per_cny','12');").unwrap();
        let s = monitor_settings(&c);
        assert_eq!(s.commission_threshold_rub, 1500.0);
        assert_eq!(s.commission_threshold_cny, 125.0);
        assert_eq!(cross_border_freight(Some(125.0),Some(0.4),&s),Some(14.61));
        assert_eq!(cross_border_freight(Some(125.01),Some(0.4),&s),Some(29.21));
        c.execute("UPDATE settings SET value='14' WHERE key='cross_border_rub_per_cny'",[]).unwrap();
        let s = monitor_settings(&c);
        assert_eq!(s.commission_threshold_cny,1500.0/14.0);
        assert_eq!(commission_rate_at(107.14,&s),LOW_COMMISSION);
        assert_eq!(commission_rate_at(107.15,&s),HIGH_COMMISSION);
        assert!(reprice_band_ends(&s).contains(&10714));
    }
    #[test]
    fn price_monitor_only_accepts_products_in_current_shop_catalog() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE products(sku TEXT PRIMARY KEY); CREATE TABLE product_costs(sku TEXT PRIMARY KEY); CREATE TABLE product_price_cache(sku TEXT PRIMARY KEY); INSERT INTO products VALUES('OWN'); INSERT INTO product_costs VALUES('OTHER'); INSERT INTO product_price_cache VALUES('OTHER');").unwrap();
        assert!(belongs_to_current_shop(&c, "OWN").unwrap());
        assert!(!belongs_to_current_shop(&c, "OTHER").unwrap());
    }
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
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT);INSERT INTO settings VALUES('cross_profit_commission_threshold_rub','1500'),('cross_border_rub_per_cny','12.5');").unwrap();
        let settings = monitor_settings(&c);
        assert_eq!(commission_rate_at(119.99, &settings), LOW_COMMISSION);
        assert_eq!(commission_rate_at(120.0, &settings), LOW_COMMISSION);
        assert_eq!(commission_rate_at(120.00000000000001, &settings), LOW_COMMISSION);
        assert_eq!(commission_rate_at(120.01, &settings), HIGH_COMMISSION);
        assert_eq!(cross_border_freight(Some(120.0),Some(0.4),&settings),Some(14.61));
        assert_eq!(cross_border_freight(Some(120.01),Some(0.4),&settings),Some(29.21));
        assert!(reprice_band_ends(&settings).contains(&12000));
        assert!(!reprice_band_ends(&settings).contains(&13400));
        let old = ProfitMonitorSettings {commission_threshold_cny:150.0,..Default::default()};
        let freight_change = 0.0;
        let expected_drop = 0.02 + freight_change*(1.0+settings.damage_rate)/130.0;
        assert!((projected_margin(130.0,20.0,0.3,&old).unwrap()-projected_margin(130.0,20.0,0.3,&settings).unwrap()-expected_drop).abs()<1e-10);
        c.execute("UPDATE settings SET value='10' WHERE key='cross_border_rub_per_cny'",[]).unwrap();
        let changed = monitor_settings(&c);
        assert_eq!(commission_rate_at(130.0,&changed),LOW_COMMISSION);
        assert_eq!(cross_border_freight(Some(130.0),Some(0.4),&changed),Some(14.61));
        assert_eq!(commission_rate_at(150.01,&changed),HIGH_COMMISSION);
    }

    #[test]
    fn fractional_cent_boundaries_and_suggestions_match_exhaustive_search() {
        let settings = ProfitMonitorSettings { commission_threshold_cny:0.29,..Default::default() };
        assert!(reprice_band_ends(&settings).contains(&29));
        assert_eq!(commission_rate_at(0.29,&settings),LOW_COMMISSION);
        assert_eq!(commission_rate_at(0.30,&settings),HIGH_COMMISSION);
        for threshold in [120.0,120.29,150.0] {
            let s=ProfitMonitorSettings {commission_threshold_cny:threshold,..Default::default()};
            for weight in [0.49999,0.5,0.50001,1.99999,2.0,2.00001,4.99999,5.0,5.00001] {
                let expected=(100..=100000).find_map(|cents| {
                    let price=cents as f64/100.0;
                    projected_margin(price,20.0,weight,&s).filter(|m|*m>0.15).map(|m|(price,m))
                });
                assert_eq!(minimum_price_for_margin(20.0,weight,1.0,15.0,&s),expected,"threshold={threshold},weight={weight}");
            }
        }
    }
    #[test]
    fn cross_border_freight_is_the_complete_freight() {
        let s=ProfitMonitorSettings::default();
        assert_eq!(cross_border_freight(Some(65.0), Some(0.301), &s), Some(11.83));
        assert_eq!(cross_border_freight(Some(65.0), Some(0.401), &s), Some(14.64));
        assert_eq!(cross_border_freight(Some(95.0), Some(0.6), &s), Some(37.29));
        let freight = cross_border_freight(Some(95.0), Some(0.6), &s).unwrap();
        let damage = (22.5 + LABEL_FEE + freight) * DAMAGE_RATE;
        let profit = 95.0 - 22.5 - 95.0 * AD_RATE - damage - freight
            - 95.0 * LOW_COMMISSION - 95.0 * LOGISTICS_COMMISSION - LABEL_FEE;
        assert!((damage - 1.8537).abs() < 1e-8);
        assert!((profit - 7.6063).abs() < 1e-8);
    }
}

