use super::*;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PaymentRate {
    from: String, to: String, rub: f64, cny: Option<f64>, rate: Option<f64>, fetched_at: String,
}
fn connection(state: &AppState) -> Result<Connection, String> {
    if active_shop_kind(state)? != "cross_border" { return Err("结算汇率仅供跨境店使用".into()); }
    db(state)
}
fn read(c: &Connection) -> Result<Vec<PaymentRate>, String> {
    let text = setting(c, "payment_rate_records_v1");
    if text.is_empty() { Ok(vec![]) } else { serde_json::from_str(&text).map_err(|e|format!("付款记录读取失败：{e}")) }
}
fn write(c: &Connection, rows: &[PaymentRate]) -> Result<(), String> {
    save_setting(c, "payment_rate_records_v1", &serde_json::to_string(rows).map_err(|e|e.to_string())?)
}
fn rate(rub: f64, cny: f64) -> Result<f64, String> {
    if !rub.is_finite() || rub <= 0.0 || !cny.is_finite() || cny <= 0.0 {return Err("付款卢布与人民币金额必须大于0".into());}
    let value = rub/cny;
    if !value.is_finite() || value <= 0.0 {return Err("汇率超出有效范围".into());}
    Ok(value)
}
fn parse_payment(payload: &serde_json::Value) -> Result<f64, String> {
    let payments = payload.pointer("/total/payments").and_then(|v|v.as_array()).ok_or("API缺少付款金额，不能按0处理")?;
    let mut total = 0.0;
    for item in payments {
        if item.get("currency_code").and_then(|v|v.as_str()) != Some("RUB") {return Err("付款包含非RUB币种，不能混币计算".into());}
        let value = item.get("value").and_then(|v|v.as_f64()).filter(|v|v.is_finite()).ok_or("API付款金额无效")?;
        total -= value;
    }
    if total < 0.0 {return Err("本期付款净额为冲回金额，不能计算结算汇率".into());}
    Ok(total)
}
#[tauri::command]
pub(crate) fn payment_rate_list(state: State<'_, AppState>) -> Result<Vec<PaymentRate>, String> {read(&connection(&state)?)}

#[tauri::command]
pub(crate) async fn payment_rate_fetch(from: String, to: String, state: State<'_, AppState>) -> Result<Vec<PaymentRate>, String> {
    let owned = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || {
        let start = chrono::NaiveDate::parse_from_str(&from,"%Y-%m-%d").map_err(|_|"开始日期无效")?;
        let end = chrono::NaiveDate::parse_from_str(&to,"%Y-%m-%d").map_err(|_|"结束日期无效")?;
        if start > end || (end-start).num_days()>365 {return Err("请选择不超过366天的有效日期范围".into());}
        let c = connection(&owned)?;
        let payload = seller_post(&c,"/v1/finance/balance", &serde_json::json!({"date_from":from,"date_to":to}))?;
        let rub = parse_payment(&payload)?;
        let mut rows = read(&c)?;
        let old = rows.iter().find(|r|r.from==from && r.to==to);
        // Revised platform amounts require the user to confirm the matching CNY again.
        let cny = old.filter(|r|(r.rub-rub).abs()<0.005).and_then(|r|r.cny);
        let entry = PaymentRate {from:from.clone(),to:to.clone(),rub,cny,rate:cny.and_then(|v|rate(rub,v).ok()),fetched_at:chrono::Utc::now().to_rfc3339()};
        rows.retain(|r|r.from!=from || r.to!=to);
        rows.push(entry); rows.sort_by(|a,b|b.from.cmp(&a.from)); write(&c,&rows)?; Ok(rows)
    }).await.map_err(|e|e.to_string())?
}
#[tauri::command]
pub(crate) fn payment_rate_save(from: String, to: String, cny: f64, state: State<'_, AppState>) -> Result<Vec<PaymentRate>, String> {
    let c = connection(&state)?; let mut rows=read(&c)?;
    let row=rows.iter_mut().find(|r|r.from==from && r.to==to).ok_or("请先获取本期付款卢布")?;
    row.rate=Some(rate(row.rub,cny)?); row.cny=Some(cny); write(&c,&rows)?; Ok(rows)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn payment_sign_currency_and_missing_fields() {
        assert_eq!(parse_payment(&serde_json::json!({"total":{"payments":[{"value":-154269,"currency_code":"RUB"}]}})).unwrap(),154269.0);
        assert!(parse_payment(&serde_json::json!({})).is_err());
        assert!(parse_payment(&serde_json::json!({"total":{"payments":[{"value":-5,"currency_code":"CNY"}]}})).is_err());
        assert!(parse_payment(&serde_json::json!({"total":{"payments":[{"value":5,"currency_code":"RUB"}]}})).is_err());
    }
    #[test] fn settlement_rate_validates_and_calculates() {
        assert!((rate(154269.0,12916.15).unwrap()-11.94388420698).abs()<1e-9);
        for n in [0.0,-1.0,f64::NAN,f64::INFINITY] {assert!(rate(154269.0,n).is_err());}
    }
}
