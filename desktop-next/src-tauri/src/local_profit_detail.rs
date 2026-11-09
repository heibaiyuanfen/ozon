use crate::*;
use serde_json::{json,Value};
#[tauri::command]
pub fn local_profit_detail(range:DateRange,state:State<AppState>)->Result<Value,String>{
 if active_shop_kind(&state)?!="local"{return Err("仅支持本土店".into())}
 let mut c=db(&state)?;apply_month_costs(&c,&range)?;repair_finance_accrual_attribution(&mut c,&range)?;insights::ensure(&c)?;
 let rate=rub_per_cny_for(&state,&c)?;
 let mut q=c.prepare("WITH s AS(SELECT sku,SUM(revenue) revenue,SUM(delivered_units) qty FROM sales_daily WHERE day BETWEEN ?1 AND ?2 GROUP BY sku),f AS(SELECT sku,SUM(amount) net,COUNT(*) n,COUNT(DISTINCT CASE WHEN lower(operation_type) LIKE '%deliveredtocustomer%' THEN operation_id END) qty FROM finance_transactions WHERE substr(operation_date,1,10) BETWEEN ?1 AND ?2 GROUP BY sku),a AS(SELECT sku,SUM(ABS(spend)) spend,SUM(revenue) revenue FROM ad_daily WHERE day BETWEEN ?1 AND ?2 AND sku<>'' GROUP BY sku),d AS(SELECT sku,SUM(quantity) qty FROM delivery_events WHERE day BETWEEN ?1 AND ?2 GROUP BY sku),k AS(SELECT sku FROM s UNION SELECT sku FROM f UNION SELECT sku FROM a) SELECT k.sku,COALESCE(p.offer_id,''),COALESCE(p.name,''),COALESCE(s.revenue,0),COALESCE(f.net,0),COALESCE(f.n,0),COALESCE(NULLIF(f.qty,0),NULLIF(d.qty,0),s.qty,0),COALESCE(pc.unit_cost_cny*?3,pc.unit_cost),COALESCE(pc.first_mile_cost,pc.first_mile_cost_cny*?3),a.spend,a.revenue,COALESCE((SELECT group_concat(ps.name,' / ') FROM product_series ps JOIN product_series_members m ON m.series_id=ps.id WHERE m.sku=k.sku),'未分组') FROM k LEFT JOIN s ON s.sku=k.sku LEFT JOIN f ON f.sku=k.sku LEFT JOIN a ON a.sku=k.sku LEFT JOIN d ON d.sku=k.sku LEFT JOIN products p ON p.sku=k.sku LEFT JOIN product_costs pc ON pc.sku=k.sku").map_err(|e|e.to_string())?;
 let mut rows=q.query_map(params![range.from,range.to,rate],|r|{let qty:i64=r.get(6)?;let cost:Option<f64>=r.get(7)?;let freight:Option<f64>=r.get(8)?;let net:f64=r.get(4)?;let n:i64=r.get(5)?;let sku:String=r.get(0)?;let purchase=if qty==0{Some(0.0)}else{cost.map(|v|v*qty as f64)};let first=if qty==0{Some(0.0)}else{freight.map(|v|v*qty as f64)};Ok(json!({"sku":sku,"offer":r.get::<_,String>(1)?,"name":r.get::<_,String>(2)?,"revenue":r.get::<_,f64>(3)?,"net":net,"purchase":purchase,"first":first,"profit":if n>0 && !sku.is_empty(){purchase.zip(first).map(|(a,b)|net-a-b)}else{None},"spend":r.get::<_,Option<f64>>(9)?,"adRevenue":r.get::<_,Option<f64>>(10)?,"series":r.get::<_,String>(11)?}))}).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
 let mut q=c.prepare("SELECT sku,operation_id,operation_date,operation_type,amount,raw_json FROM finance_transactions WHERE substr(operation_date,1,10) BETWEEN ?1 AND ?2 ORDER BY operation_date DESC").map_err(|e|e.to_string())?;
 let mut details=q.query_map(params![range.from,range.to],|r|{let raw:String=r.get(5)?;let v:Value=serde_json::from_str(&raw).unwrap_or(Value::Null);let name=format!("{} {}",r.get::<_,String>(3)?,json_text(v.get("operation_type_name")));let review=name.to_lowercase().contains("firstcustomerreview")||name.contains("收集首批评价");Ok(json!({"sku":r.get::<_,String>(0)?,"id":r.get::<_,String>(1)?,"date":r.get::<_,String>(2)?,"name":name,"amount":r.get::<_,f64>(4)?,"review":review,"services":v.get("services"),"accrual":value_number(v.get("accruals_for_sale")),"commission":value_number(v.get("sale_commission"))}))}).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
 for d in &mut details {
 let raw=d["name"].as_str().unwrap_or("").to_string();let n=raw.to_lowercase();
 let label=if d["review"].as_bool()==Some(true){"收集首批评价"}else if n.contains("deliveredtocustomer"){"销售结算（含佣金及服务费）"}else if n.contains("return") && !n.contains("logistic"){"退货及退款结算"}else{finance_service_category(&raw).1};
 d["label"]=json!(label);
 let mut parts=vec![];
 let accrual=d["accrual"].as_f64().unwrap_or(0.0);let commission=d["commission"].as_f64().unwrap_or(0.0);
 if accrual!=0.0{parts.push(json!({"label":if accrual>0.0{"销售入账"}else{"销售退款"},"amount":accrual}));}
 if commission!=0.0{parts.push(json!({"label":"平台佣金","amount":commission}));}
 let mut service_total=0.0;
 if let Some(services)=d["services"].as_array_mut(){for service in services{let name=service["name"].as_str().unwrap_or("");let label=if name.to_lowercase().contains("firstcustomerreview"){"收集首批评价"}else{finance_service_category(name).1};let amount=value_number(service.get("price"));service_total+=amount;service["label"]=json!(label);parts.push(json!({"label":label,"amount":amount}));}}
 let residual=d["amount"].as_f64().unwrap_or(0.0)-accrual-commission-service_total;
 if residual.abs()>0.0000001{parts.push(json!({"label":if label=="销售结算（含佣金及服务费）"{"其他结算调整"}else{label},"amount":residual}));}
 d["parts"]=json!(parts);
 }
 let tax_rate=setting(&c,"local_tax_rate").parse::<f64>().unwrap_or(3.0).max(0.0);
 let payout_rate=setting(&c,"local_payout_fee_rate").parse::<f64>().unwrap_or(10.0).max(0.0);
 let mut financial_sales=std::collections::HashMap::<String,f64>::new();
 for d in &details {let sku=d["sku"].as_str().unwrap_or("").to_string();*financial_sales.entry(sku).or_default()+=d["accrual"].as_f64().unwrap_or(0.0);}
 let total_sales:f64=financial_sales.values().sum();
 let positive_sales:f64=financial_sales.iter().filter(|(sku,_)|!sku.is_empty()).map(|(_,v)|v.max(0.0)).sum();
 let total_tax=total_sales.max(0.0)*tax_rate/100.0;
 let total_net: f64 = rows.iter().map(|r|r["net"].as_f64().unwrap_or(0.0)).sum();
 let positive_net: f64 = rows.iter().filter(|r|!r["sku"].as_str().unwrap_or("").is_empty()).map(|r|r["net"].as_f64().unwrap_or(0.0).max(0.0)).sum();
 let total_payout = total_net.max(0.0)*payout_rate/100.0;
 for row in &mut rows {
 let sku=row["sku"].as_str().unwrap_or("");
 // Only reverse identifiable CPC/CPO advertising; review and other promotion fees remain in Finance.
 let booked=details.iter().filter(|d|d["sku"].as_str()==Some(sku)&&!sku.is_empty()&&!d["review"].as_bool().unwrap_or(false)).filter(|d|{let n=d["name"].as_str().unwrap_or("").to_lowercase();n.contains("click")||n.contains("cpc")||n.contains("cpo")||n.contains("costperorder")}).map(|d|-d["amount"].as_f64().unwrap_or(0.0)).sum::<f64>();
 let net=row["net"].as_f64().unwrap_or(0.0);
 let tax=if !sku.is_empty() && positive_sales>0.0 {total_tax*financial_sales.get(sku).copied().unwrap_or(0.0).max(0.0)/positive_sales} else {0.0};
 let payout=if !sku.is_empty() && positive_net>0.0 {total_payout*net.max(0.0)/positive_net} else {0.0};
 let profit=row["profit"].as_f64().zip(row["spend"].as_f64()).map(|(p,a)|p+booked-a-tax-payout);
 row["bookedAds"]=json!(booked);row["tax"]=json!(tax);row["payout"]=json!(payout);row["afterTaxProfit"]=json!(profit);
 }
 Ok(json!({"rows":rows,"details":details,"taxRate":tax_rate,"payoutRate":payout_rate}))
}

#[cfg(test)]
mod payout_tests {
 #[test] fn allocated_fee_matches_store_total_with_negative_unallocated() { let nets=[1000.0_f64,500.0,-100.0]; let total:f64=nets.iter().sum::<f64>()-400.0; let fee=total.max(0.0)*0.1; let weight:f64=nets.iter().map(|n|n.max(0.0)).sum(); let allocated:f64=nets.iter().map(|n|fee*n.max(0.0)/weight).sum(); assert!((allocated-fee).abs()<1e-9); assert_eq!(fee,100.0); }
}
