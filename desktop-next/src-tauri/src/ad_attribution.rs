use super::{background_state, db, AppState};
use calamine::{open_workbook_auto, Reader};
use rusqlite::params;
use serde_json::{json, Value};
use std::{collections::HashMap, path::Path};
use tauri::State;

pub(super) fn ensure(c: &rusqlite::Connection) -> Result<(), String> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS variant_report_stats(period_from TEXT NOT NULL,period_to TEXT NOT NULL,entry_sku TEXT NOT NULL,campaign_id TEXT NOT NULL,spend REAL,impressions INTEGER,clicks INTEGER,ad_orders INTEGER,ad_revenue REAL,source_file TEXT NOT NULL,imported_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,PRIMARY KEY(period_from,period_to,entry_sku,campaign_id));CREATE TABLE IF NOT EXISTS variant_report_flows(period_from TEXT NOT NULL,period_to TEXT NOT NULL,entry_sku TEXT NOT NULL,purchased_sku TEXT NOT NULL,campaign_id TEXT NOT NULL,revenue REAL,units INTEGER,source_file TEXT NOT NULL,imported_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,PRIMARY KEY(period_from,period_to,entry_sku,purchased_sku,campaign_id));").map_err(|e|e.to_string())
}
fn text(row: &[calamine::Data], i: usize) -> String {
    row.get(i)
        .map(|x| x.to_string())
        .unwrap_or_default()
        .trim()
        .to_string()
}
fn num(row: &[calamine::Data], i: usize) -> f64 {
    text(row, i)
        .replace(' ', "")
        .replace(',', ".")
        .parse()
        .unwrap_or(0.0)
}
fn headers(row: &[calamine::Data]) -> HashMap<String, usize> {
    row.iter()
        .enumerate()
        .map(|(i, x)| (x.to_string().trim().to_string(), i))
        .collect()
}
fn col(h: &HashMap<String, usize>, names: &[&str]) -> Result<usize, String> {
    names
        .iter()
        .find_map(|n| h.get(*n).copied())
        .ok_or_else(|| format!("缺少列：{}", names.join(" / ")))
}
fn period(v: &str) -> Result<(String, String), String> {
    let ds = v
        .split(|c: char| !c.is_ascii_digit() && c != '.')
        .filter(|x| x.matches('.').count() == 2)
        .collect::<Vec<_>>();
    if ds.len() < 2 {
        return Err("无法识别报告时期".into());
    }
    let cv = |x: &str| {
        let p = x.split('.').collect::<Vec<_>>();
        format!("{}-{}-{}", p[2], p[1], p[0])
    };
    Ok((cv(ds[0]), cv(ds[1])))
}
fn sample_confidence(
    clicks: f64,
    orders: f64,
    complete_days: i64,
    quality: &str,
    spend: f64,
) -> (&'static str, bool) {
    let insufficient = clicks < 50.0 || orders < 1.0 || spend < 100.0 || quality == "missing";
    let confidence =
        if clicks >= 300.0 && orders >= 10.0 && complete_days >= 3 && quality == "complete" {
            "HIGH"
        } else if clicks >= 100.0 && orders >= 3.0 && complete_days >= 2 && quality == "complete" {
            "MEDIUM"
        } else {
            "LOW"
        };
    (confidence, insufficient)
}
fn next_action(score: f64, confidence: &str, insufficient: bool, role: &str) -> &'static str {
    if insufficient {
        "INSUFFICIENT_DATA"
    } else if role == "cannibalizing_variant" || score < 35.0 {
        "REDUCE_15"
    } else if score >= 80.0 && confidence != "LOW" {
        "INCREASE_10"
    } else if score >= 65.0 {
        "INCREASE_5"
    } else if role == "profit_variant" {
        "PRICE_UP_3"
    } else {
        "HOLD_3_DAYS"
    }
}

#[tauri::command]
pub async fn import_variant_report(
    state: State<'_, AppState>,
    path: String,
) -> Result<Value, String> {
    let state = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move||{if !Path::new(path.trim()).is_file(){return Err("找不到 Excel 文件".into())}let mut book=open_workbook_auto(path.trim()).map_err(|e|format!("无法读取 Excel：{e}"))?;let stats=book.worksheet_range("Statistics").map_err(|e|format!("缺少 Statistics 工作表：{e}"))?;let union=book.worksheet_range("Union").map_err(|e|format!("缺少 Union 工作表：{e}"))?;let (from,to)=period(&text(stats.rows().next().ok_or("Statistics 为空")?,0))?;let sh=headers(stats.rows().nth(1).ok_or("Statistics 缺少表头")?);let uh=headers(union.rows().nth(1).ok_or("Union 缺少表头")?);let ss=col(&sh,&["SKU"])?;let sc=col(&sh,&["广告活动 ID"])?;let spend=col(&sh,&["费用，₽"])?;let imp=col(&sh,&["展现量"])?;let clicks=col(&sh,&["点击次数"])?;let orders=col(&sh,&["已售商品数量，件"])?;let revenue=col(&sh,&["促销销售，{货币}"])?;let ue=col(&uh,&["促销中的 SKU"])?;let up=col(&uh,&["合并卡中的 SKU"])?;let uc=col(&uh,&["广告活动 ID"])?;let ur=col(&uh,&["促销销售，{货币}"])?;let uu=col(&uh,&["已售商品数量，件"])?;let mut c=db(&state)?;ensure(&c)?;let tx=c.transaction().map_err(|e|e.to_string())?;let mut ns=0;for r in stats.rows().skip(2){let sku=text(r,ss);if sku.is_empty()||sku=="0"{continue}tx.execute("INSERT INTO variant_report_stats VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,CURRENT_TIMESTAMP)ON CONFLICT(period_from,period_to,entry_sku,campaign_id)DO UPDATE SET spend=excluded.spend,impressions=excluded.impressions,clicks=excluded.clicks,ad_orders=excluded.ad_orders,ad_revenue=excluded.ad_revenue,source_file=excluded.source_file,imported_at=CURRENT_TIMESTAMP",params![from,to,sku,text(r,sc),num(r,spend),num(r,imp)as i64,num(r,clicks)as i64,num(r,orders)as i64,num(r,revenue),path]).map_err(|e|e.to_string())?;ns+=1}let mut nf=0;for r in union.rows().skip(2){let entry=text(r,ue);let purchased=text(r,up);if entry.is_empty()||purchased.is_empty(){continue}tx.execute("INSERT INTO variant_report_flows VALUES(?1,?2,?3,?4,?5,?6,?7,?8,CURRENT_TIMESTAMP)ON CONFLICT(period_from,period_to,entry_sku,purchased_sku,campaign_id)DO UPDATE SET revenue=excluded.revenue,units=excluded.units,source_file=excluded.source_file,imported_at=CURRENT_TIMESTAMP",params![from,to,entry,purchased,text(r,uc),num(r,ur),num(r,uu)as i64,path]).map_err(|e|e.to_string())?;nf+=1}tx.commit().map_err(|e|e.to_string())?;Ok(json!({"periodFrom":from,"periodTo":to,"statisticsRows":ns,"flowRows":nf,"mode":"direct"}))}).await.map_err(|e|e.to_string())?
}

#[tauri::command]
pub async fn ad_attribution_command(
    state: State<'_, AppState>,
    from: String,
    to: String,
    series_id: Option<i64>,
) -> Result<Value, String> {
    let state = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move||{
  let c=db(&state)?;ensure(&c)?;let series:Vec<Value>=c.prepare("SELECT id,name FROM product_series ORDER BY name").and_then(|mut s|s.query_map([],|r|Ok(json!({"id":r.get::<_,i64>(0)?,"name":r.get::<_,String>(1)?})))?.collect()).map_err(|e|e.to_string())?;
  let Some(sid)=series_id else{return Ok(json!({"series":series,"rows":[],"summary":null,"coverage":{"own":"available","assisted":"unsupported","halo":"unsupported","marginal":"estimated"}}))};
  let start=chrono::NaiveDate::parse_from_str(&from,"%Y-%m-%d").map_err(|_|"开始日期无效")?;let end=chrono::NaiveDate::parse_from_str(&to,"%Y-%m-%d").map_err(|_|"结束日期无效")?;if end<start{return Err("结束日期不能早于开始日期".into())}let days=(end-start).num_days()+1;let prev_to=start-chrono::Duration::days(1);let prev_from=prev_to-chrono::Duration::days(days-1);
  let mut s=c.prepare("SELECT m.sku,COALESCE(NULLIF(p.offer_id,''),m.sku),COALESCE(NULLIF(p.name,''),m.sku),COALESCE(SUM(CASE WHEN a.day BETWEEN ?2 AND ?3 THEN ABS(a.spend) ELSE 0 END),0),COALESCE(SUM(CASE WHEN a.day BETWEEN ?2 AND ?3 THEN a.revenue ELSE 0 END),0),COALESCE(SUM(CASE WHEN a.day BETWEEN ?4 AND ?5 THEN ABS(a.spend) ELSE 0 END),0),COALESCE(SUM(CASE WHEN a.day BETWEEN ?4 AND ?5 THEN a.revenue ELSE 0 END),0),(SELECT COALESCE(SUM(sd.revenue),0) FROM sales_daily sd WHERE sd.sku=m.sku AND sd.day BETWEEN ?2 AND ?3),(SELECT COALESCE(SUM(sd.revenue),0) FROM sales_daily sd WHERE sd.sku=m.sku AND sd.day BETWEEN ?4 AND ?5),COUNT(DISTINCT CASE WHEN a.day BETWEEN ?2 AND ?3 THEN a.campaign_id END) FROM product_series_members m LEFT JOIN products p ON p.sku=m.sku LEFT JOIN ad_daily a ON a.sku=m.sku WHERE m.series_id=?1 GROUP BY m.sku,p.offer_id,p.name ORDER BY 8 DESC").map_err(|e|e.to_string())?;
  let mut rows=s.query_map(params![sid,from,to,prev_from.to_string(),prev_to.to_string()],|r|{let spend:f64=r.get(3)?;let attributed:f64=r.get(4)?;Ok(json!({"sku":r.get::<_,String>(0)?,"offerId":r.get::<_,String>(1)?,"name":r.get::<_,String>(2)?,"spend":spend,"attributedRevenue":attributed,"apiRoas":if spend>0.0{Some(attributed/spend)}else{None},"previousSpend":r.get::<_,f64>(5)?,"previousAttributedRevenue":r.get::<_,f64>(6)?,"totalSales":r.get::<_,f64>(7)?,"previousTotalSales":r.get::<_,f64>(8)?,"campaigns":r.get::<_,i64>(9)?,"ownRoas":Value::Null,"assistedRoas":Value::Null,"crossSizeHaloRate":Value::Null}))}).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
  let mut direct=false;for row in &mut rows{let sku=row["sku"].as_str().unwrap_or("");let stat:(f64,i64,i64,i64,f64,i64)=c.query_row("SELECT COALESCE(SUM(spend),0),COALESCE(SUM(impressions),0),COALESCE(SUM(clicks),0),COALESCE(SUM(ad_orders),0),COALESCE(SUM(ad_revenue),0),COUNT(*) FROM variant_report_stats WHERE entry_sku=?1 AND period_from>=?2 AND period_to<=?3",params![sku,from,to],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).unwrap_or_default();let outbound:(f64,i64)=c.query_row("SELECT COALESCE(SUM(revenue),0),COALESCE(SUM(units),0) FROM variant_report_flows WHERE entry_sku=?1 AND purchased_sku<>?1 AND period_from>=?2 AND period_to<=?3",params![sku,from,to],|r|Ok((r.get(0)?,r.get(1)?))).unwrap_or_default();let inbound:(f64,i64)=c.query_row("SELECT COALESCE(SUM(revenue),0),COALESCE(SUM(units),0) FROM variant_report_flows WHERE purchased_sku=?1 AND entry_sku<>?1 AND period_from>=?2 AND period_to<=?3",params![sku,from,to],|r|Ok((r.get(0)?,r.get(1)?))).unwrap_or_default();if stat.5>0{direct=true;let assisted=stat.4+outbound.0;row["spend"]=json!(stat.0);row["attributedRevenue"]=json!(stat.4);row["apiRoas"]=if stat.0>0.0{json!(stat.4/stat.0)}else{Value::Null};row["ownRoas"]=if stat.0>0.0{json!(stat.4/stat.0)}else{Value::Null};row["assistedRoas"]=if stat.0>0.0{json!(assisted/stat.0)}else{Value::Null};row["crossSizeHaloRate"]=if assisted>0.0{json!(outbound.0/assisted*100.0)}else{Value::Null};row["outboundCrossRevenue"]=json!(outbound.0);row["outboundAssistedUnits"]=json!(outbound.1);row["inboundAssistedRevenue"]=json!(inbound.0);row["inboundAssistedUnits"]=json!(inbound.1);row["netFlow"]=json!(outbound.0-inbound.0);row["directRevenue"]=json!(assisted);row["directImpressions"]=json!(stat.1);row["directClicks"]=json!(stat.2);row["directOrders"]=json!(stat.3)}}
  for row in &mut rows{let sku=row["sku"].as_str().unwrap_or("");let m:(f64,f64,f64,f64,f64,i64,i64,i64,i64)=c.query_row("SELECT (SELECT COALESCE(SUM(ordered_units),0) FROM sales_daily WHERE sku=?1 AND day BETWEEN ?2 AND ?3),(SELECT COALESCE(SUM(ordered_units),0) FROM sales_daily WHERE sku=?1 AND day BETWEEN ?4 AND ?5),(SELECT COALESCE(SUM(impressions),0) FROM ad_daily WHERE sku=?1 AND day BETWEEN ?2 AND ?3),(SELECT COALESCE(SUM(clicks),0) FROM ad_daily WHERE sku=?1 AND day BETWEEN ?2 AND ?3),COALESCE((SELECT price FROM product_price_cache WHERE sku=?1),0),(SELECT COALESCE(SUM(orders),0) FROM ad_daily WHERE sku=?1 AND day BETWEEN ?2 AND ?3),(SELECT COALESCE(SUM(available_stock),0) FROM inventory_stock WHERE sku=?1),(SELECT COUNT(*) FROM sales_daily WHERE sku=?1 AND day BETWEEN ?2 AND ?3),(SELECT COUNT(*) FROM ad_daily WHERE sku=?1 AND day BETWEEN ?2 AND ?3)",params![sku,from,to,prev_from.to_string(),prev_to.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?))).unwrap_or_default();row["units"]=json!(m.0);row["previousUnits"]=json!(m.1);row["impressions"]=row.get("directImpressions").cloned().unwrap_or(json!(m.2));row["clicks"]=row.get("directClicks").cloned().unwrap_or(json!(m.3));row["price"]=if m.4>0.0{json!(m.4)}else{Value::Null};row["adOrders"]=row.get("directOrders").cloned().unwrap_or(json!(m.5));row["stock"]=json!(m.6);row["completeDays"]=json!(m.7.min(m.8));row["dataQuality"]=json!(if m.7>=days&&m.8>=days{"complete"}else if m.7>0||m.8>0{"partial"}else{"missing"})}
  let spend=rows.iter().map(|x|x["spend"].as_f64().unwrap_or(0.0)).sum::<f64>();let prev_spend=rows.iter().map(|x|x["previousSpend"].as_f64().unwrap_or(0.0)).sum::<f64>();let sales=rows.iter().map(|x|x["totalSales"].as_f64().unwrap_or(0.0)).sum::<f64>();let prev_sales=rows.iter().map(|x|x["previousTotalSales"].as_f64().unwrap_or(0.0)).sum::<f64>();let units=rows.iter().map(|x|x["units"].as_f64().unwrap_or(0.0)).sum::<f64>();let prev_units=rows.iter().map(|x|x["previousUnits"].as_f64().unwrap_or(0.0)).sum::<f64>();let ds=spend-prev_spend;let dr=sales-prev_sales;let series_growth=if prev_units>0.0{Some((units-prev_units)/prev_units)}else{None};for row in &mut rows{let u=row["units"].as_f64().unwrap_or(0.0);let pu=row["previousUnits"].as_f64().unwrap_or(0.0);let rs=row["spend"].as_f64().unwrap_or(0.0);let ps=row["previousSpend"].as_f64().unwrap_or(0.0);let sg=if ps>0.0{Some((rs-ps)/ps)}else{None};let ug=if pu>0.0{Some((u-pu)/pu)}else{None};let own=match(ug,sg){(Some(a),Some(b))if b.abs()>0.0001=>Some(a/b),_=>None};let series_el=match(series_growth,sg){(Some(a),Some(b))if b.abs()>0.0001=>Some(a/b),_=>None};let own_increase=(u-pu).max(0.0);let series_increase=(units-prev_units).max(0.0);let cannibal=if own_increase>0.0{Some((own_increase-series_increase).max(0.0)/own_increase)}else{None};let clicks=row["clicks"].as_f64().unwrap_or(0.0);let orders=row["adOrders"].as_f64().unwrap_or(0.0);let complete=row["completeDays"].as_i64().unwrap_or(0);let quality=row["dataQuality"].as_str().unwrap_or("missing");let (confidence,insufficient)=sample_confidence(clicks,orders,complete,quality,rs);let own_rev=row["attributedRevenue"].as_f64().unwrap_or(0.0);let outbound=row["outboundCrossRevenue"].as_f64();let inbound=row["inboundAssistedRevenue"].as_f64();let total=row["totalSales"].as_f64().unwrap_or(0.0);let outbound_halo=row["crossSizeHaloRate"].as_f64();let inbound_share=if direct&&total>0.0{inbound.map(|v|v/total*100.0)}else{None};let own_dependency=if total>0.0{Some(own_rev/total*100.0)}else{None};let spend_share=if spend>0.0{Some(rs/spend*100.0)}else{None};let sales_share=if units>0.0{Some(u/units*100.0)}else{None};let gap=match(spend_share,sales_share){(Some(a),Some(b))=>Some(a-b),_=>None};let own_roas=row["ownRoas"].as_f64().or_else(||row["apiRoas"].as_f64());let cvr=if clicks>0.0{Some(orders/clicks*100.0)}else{None};let cpa=if orders>0.0{Some(rs/orders)}else{None};let role=if insufficient{"insufficient_data"}else if cannibal.unwrap_or(0.0)>=0.5&&series_growth.unwrap_or(0.0)<0.05{"cannibalizing_variant"}else if direct&&outbound.unwrap_or(0.0)>0.0&&inbound.unwrap_or(0.0)>0.0&&outbound_halo.unwrap_or(0.0)>=20.0&&inbound_share.unwrap_or(0.0)>=20.0&&own_roas.unwrap_or(0.0)>=1.0{"balanced_hub"}else if direct&&outbound_halo.unwrap_or(0.0)>=25.0&&series_el.unwrap_or(0.0)>=0.8&&series_el.unwrap_or(0.0)>own.unwrap_or(0.0)&&series_growth.unwrap_or(0.0)>0.0{"traffic_driver"}else if direct&&inbound_share.unwrap_or(0.0)>=25.0&&own_dependency.unwrap_or(100.0)<30.0&&u/units.max(1.0)>=0.15{"upgrade_receiver"}else if cvr.unwrap_or(0.0)>=2.0&&u/units.max(1.0)>=0.25&&own_roas.unwrap_or(0.0)>=2.0{"main_converter"}else if row["price"].as_f64().unwrap_or(0.0)>0.0&&own_roas.unwrap_or(0.0)>=4.0{"profit_variant"}else if own_roas.unwrap_or(0.0)<1.0&&series_el.unwrap_or(0.0)<0.3&&outbound_halo.unwrap_or(0.0)<10.0{"low_efficiency"}else{"balanced_hub"};let contribution=(35.0*series_el.unwrap_or(0.0).clamp(0.0,1.5)/1.5+25.0*(own_roas.unwrap_or(0.0)/5.0).clamp(0.0,1.0)+20.0*(u/units.max(1.0))+20.0*(1.0-cannibal.unwrap_or(0.0))).round().clamp(0.0,100.0);let assisted_score=outbound_halo.unwrap_or(0.0).clamp(0.0,100.0);let cvr_score=(cvr.unwrap_or(0.0)/5.0*100.0).clamp(0.0,100.0);let cpa_score=cpa.map(|v|(100.0-v/20.0).clamp(0.0,100.0)).unwrap_or(0.0);let conf_score=match confidence{"HIGH"=>100.0,"MEDIUM"=>65.0,_=>20.0};let mut opportunity=(30.0*series_el.unwrap_or(0.0).clamp(0.0,1.0)+0.20*contribution+0.15*assisted_score+0.10*cvr_score+0.10*cpa_score+0.10*(100.0-cannibal.unwrap_or(0.0)*100.0)+0.05*conf_score).round().clamp(0.0,100.0);if confidence=="LOW"{opportunity=opportunity.min(79.0)}let action=next_action(opportunity,confidence,insufficient,role);row["ownScaleElasticity"]=json!(own);row["seriesScaleElasticity"]=json!(series_el);row["haloElasticity"]=json!(match(own,series_el){(Some(a),Some(b))=>Some(b-a),_=>None});row["cannibalizationRate"]=json!(cannibal.map(|x|x*100.0));row["outboundHaloRate"]=json!(outbound_halo);row["inboundAssistShare"]=json!(inbound_share);row["ownAdDependency"]=json!(own_dependency);row["adSpendShare"]=json!(spend_share);row["salesShare"]=json!(sales_share);row["trafficSalesGap"]=json!(gap);row["ownCvr"]=json!(cvr);row["cpa"]=json!(cpa);row["role"]=json!(role);row["confidence"]=json!(confidence);row["doNotRank"]=json!(insufficient);row["contributionScore"]=json!(contribution);row["budgetOpportunityScore"]=json!(opportunity);row["decision"]=json!(action)}
  let flows:Vec<Value>=c.prepare("SELECT f.entry_sku,f.purchased_sku,SUM(f.revenue),SUM(f.units) FROM variant_report_flows f JOIN product_series_members a ON a.series_id=?1 AND a.sku=f.entry_sku JOIN product_series_members b ON b.series_id=?1 AND b.sku=f.purchased_sku WHERE f.period_from>=?2 AND f.period_to<=?3 GROUP BY f.entry_sku,f.purchased_sku ORDER BY 3 DESC").and_then(|mut q|q.query_map(params![sid,from,to],|r|Ok(json!({"entrySku":r.get::<_,String>(0)?,"purchasedSku":r.get::<_,String>(1)?,"revenue":r.get::<_,f64>(2)?,"units":r.get::<_,i64>(3)?})))?.collect()).unwrap_or_default();
  let attributed=rows.iter().map(|x|x["attributedRevenue"].as_f64().unwrap_or(0.0)).sum::<f64>();let tacos=if sales>0.0{Some(spend/sales*100.0)}else{None};let revenue_growth=if prev_sales>0.0{Some((sales-prev_sales)/prev_sales)}else{None};let asp=if units>0.0{Some(sales/units)}else{None};let prev_asp=if prev_units>0.0{Some(prev_sales/prev_units)}else{None};let asp_growth=match(asp,prev_asp){(Some(a),Some(b))if b>0.0=>Some((a-b)/b),_=>None};let marginal_roas=if ds>0.0{Some(dr/ds)}else{None};let health=(25.0*(1.0-tacos.unwrap_or(30.0)/30.0).clamp(0.0,1.0)+25.0*((series_growth.unwrap_or(-0.2)+0.2)/0.4).clamp(0.0,1.0)+15.0*((revenue_growth.unwrap_or(-0.2)+0.2)/0.4).clamp(0.0,1.0)+10.0*((asp_growth.unwrap_or(-0.1)+0.1)/0.2).clamp(0.0,1.0)+15.0*(marginal_roas.unwrap_or(0.0)/5.0).clamp(0.0,1.0)+10.0).round().clamp(0.0,100.0);let health_label=if health>=80.0{"Healthy Growth"}else if health>=65.0{"Stable"}else if health>=50.0{"Watch"}else{"Risk"};let mut ranked=rows.iter().filter(|x|x["confidence"].as_str()!=Some("LOW")&&x["doNotRank"].as_bool()!=Some(true)).collect::<Vec<_>>();ranked.sort_by(|a,b|b["budgetOpportunityScore"].as_f64().unwrap_or(0.0).partial_cmp(&a["budgetOpportunityScore"].as_f64().unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal));let receiver=ranked.first().copied();let donor=ranked.iter().rev().find(|x|x["decision"].as_str()==Some("REDUCE_15")||x["budgetOpportunityScore"].as_f64().unwrap_or(100.0)<50.0).copied();let recommendations=match(receiver,donor){(Some(to_row),Some(from_row))if to_row["sku"]!=from_row["sku"]=>vec![json!({"fromSku":from_row["sku"],"fromOfferId":from_row["offerId"],"toSku":to_row["sku"],"toOfferId":to_row["offerId"],"reducePercent":15,"increasePercent":10,"reservePercent":5,"observationDays":3,"reason":format!("{} 边际机会分较低，{} 系列贡献与置信度更强",from_row["offerId"].as_str().unwrap_or("来源SKU"),to_row["offerId"].as_str().unwrap_or("目标SKU")),"successConditions":["Series Units 不低于基线","Series TACOS 不高于基线","Series Revenue 不低于基线"],"rollbackConditions":["Series Units 下降超过 5%","Series TACOS 超过硬上限"]})],_=>vec![]};Ok(json!({"series":series,"rows":rows,"flows":flows,"recommendations":recommendations,"summary":{"spend":spend,"attributedRevenue":attributed,"totalSales":sales,"totalUnits":units,"previousUnits":prev_units,"seriesGrowth":series_growth,"revenueGrowth":revenue_growth,"averageSellingPrice":asp,"aspGrowth":asp_growth,"seriesHealthScore":health,"seriesHealthLabel":health_label,"growthStage":if series_growth.unwrap_or(0.0)>0.1{"EXPANDING"}else if series_growth.unwrap_or(0.0)<0.0{"DECLINING"}else if marginal_roas.unwrap_or(0.0)<0.3{"SATURATED"}else{"STABLE"},"seriesTacos":tacos,"seriesAcos":if attributed>0.0{Some(spend/attributed*100.0)}else{None},"previousSpend":prev_spend,"previousSales":prev_sales,"incrementalSpend":ds,"incrementalSeriesRevenue":dr,"marginalSeriesRoas":marginal_roas,"marginalSeriesUnits":units-prev_units,"marginalUnitsPer1000":if ds>0.0{Some((units-prev_units)/ds*1000.0)}else{None},"baselineFrom":prev_from.to_string(),"baselineTo":prev_to.to_string(),"days":days},"coverage":{"own":if direct{"available"}else{"proxy_only"},"assisted":if direct{"available"}else{"unsupported"},"halo":if direct{"available"}else{"unsupported"},"marginal":"estimated","mode":if direct{"direct"}else{"experimental_inference"}}}))
 }).await.map_err(|e|e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn confidence_rejects_tiny_spend_and_extreme_roas() {
        assert_eq!(
            sample_confidence(60.0, 2.0, 3, "complete", 13.0),
            ("LOW", true)
        );
        assert_eq!(
            next_action(100.0, "LOW", true, "traffic_driver"),
            "INSUFFICIENT_DATA"
        )
    }
    #[test]
    fn confidence_respects_complete_and_partial_days() {
        assert_eq!(
            sample_confidence(320.0, 12.0, 3, "complete", 5000.0),
            ("HIGH", false)
        );
        assert_eq!(
            sample_confidence(320.0, 12.0, 3, "partial", 5000.0),
            ("LOW", false)
        );
        assert_eq!(
            sample_confidence(0.0, 0.0, 0, "missing", 0.0),
            ("LOW", true)
        )
    }
    #[test]
    fn low_confidence_never_priority_scales() {
        assert_ne!(
            next_action(95.0, "LOW", false, "traffic_driver"),
            "INCREASE_10"
        );
        assert_eq!(
            next_action(70.0, "LOW", false, "traffic_driver"),
            "INCREASE_5"
        )
    }
    #[test]
    fn cannibalization_has_priority_over_high_score() {
        assert_eq!(
            next_action(92.0, "HIGH", false, "cannibalizing_variant"),
            "REDUCE_15"
        )
    }
    #[test]
    fn price_action_is_single_next_step() {
        assert_eq!(
            next_action(55.0, "HIGH", false, "profit_variant"),
            "PRICE_UP_3"
        )
    }
    #[test]
    fn parses_ozon_report_period() {
        assert_eq!(
            period("推广分析 02.09.2026 — 08.09.2026").unwrap(),
            ("2026-09-02".into(), "2026-09-08".into())
        )
    }
}
