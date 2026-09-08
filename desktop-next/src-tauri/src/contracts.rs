use rusqlite::{params, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::State;
use std::{fs, io::Write};

use super::{background_state, db, AppState};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContractInput {
    #[serde(default)] contract_no: String,
    seller_name: String,
    #[serde(default)] seller_tax_no: String,
    #[serde(default)] seller_bank: String,
    #[serde(default)] seller_account: String,
    #[serde(default)] seller_address: String,
    #[serde(default)] seller_phone: String,
    #[serde(default = "default_tax_rate")] tax_rate: f64,
    contract_date: String,
    #[serde(default)] delivery_terms: String,
    #[serde(default)] payment_terms: String,
    #[serde(default)] breach_terms: String,
    #[serde(default)] other_terms: String,
    items: Vec<ContractItemInput>,
    #[serde(default)] amount_uppercase: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContractItemInput { product_name: String, unit: String, quantity: f64, unit_price_ex_tax: f64 }
fn default_tax_rate() -> f64 { 1.0 }

pub(super) fn ensure(c: &rusqlite::Connection) -> Result<(), String> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS purchase_contracts(
      id INTEGER PRIMARY KEY AUTOINCREMENT, contract_no TEXT NOT NULL, seller_name TEXT NOT NULL,
      seller_tax_no TEXT NOT NULL DEFAULT '', seller_bank TEXT NOT NULL DEFAULT '', seller_account TEXT NOT NULL DEFAULT '',
      seller_address TEXT NOT NULL DEFAULT '', seller_phone TEXT NOT NULL DEFAULT '', tax_rate REAL NOT NULL DEFAULT 1,
      contract_date TEXT NOT NULL, delivery_terms TEXT NOT NULL DEFAULT '', payment_terms TEXT NOT NULL DEFAULT '',
      breach_terms TEXT NOT NULL DEFAULT '', other_terms TEXT NOT NULL DEFAULT '', status TEXT NOT NULL DEFAULT 'draft',
      created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
      CREATE TABLE IF NOT EXISTS purchase_contract_items(
      id INTEGER PRIMARY KEY AUTOINCREMENT, contract_id INTEGER NOT NULL, product_name TEXT NOT NULL, unit TEXT NOT NULL,
      quantity REAL NOT NULL, unit_price_ex_tax REAL NOT NULL, sort_order INTEGER NOT NULL DEFAULT 0,
      FOREIGN KEY(contract_id) REFERENCES purchase_contracts(id) ON DELETE CASCADE);
      CREATE INDEX IF NOT EXISTS idx_purchase_contracts_date ON purchase_contracts(contract_date DESC,id DESC);"
    ).map_err(|e|e.to_string())
}

fn validate(v:&ContractInput)->Result<(),String>{
    if v.seller_name.trim().is_empty(){return Err("乙方名称不能为空".into())}
    if chrono::NaiveDate::parse_from_str(&v.contract_date,"%Y-%m-%d").is_err(){return Err("合同日期无效".into())}
    if !(0.0..=100.0).contains(&v.tax_rate){return Err("税率必须在 0 至 100 之间".into())}
    if v.items.is_empty(){return Err("至少需要一项产品".into())}
    if v.items.iter().any(|x|x.product_name.trim().is_empty()||x.unit.trim().is_empty()||x.quantity<=0.0||x.unit_price_ex_tax<0.0){return Err("请完整填写产品名称、单位、数量和单价".into())}
    Ok(())
}

fn summary(v:&ContractInput)->Value{
    let subtotal=(v.items.iter().map(|x|x.quantity*x.unit_price_ex_tax).sum::<f64>()*100.0).round()/100.0;
    let tax=(subtotal*v.tax_rate/100.0*100.0).round()/100.0;
    json!({"subtotal":subtotal,"tax":tax,"total":subtotal+tax})
}

fn xml(v:&str)->String{v.replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;")}
fn wp(text:&str,bold:bool)->String{format!("<w:p><w:pPr><w:spacing w:after=\"60\" w:line=\"280\" w:lineRule=\"auto\"/></w:pPr><w:r><w:rPr>{}<w:rFonts w:eastAsia=\"宋体\"/></w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>",if bold{"<w:b/>"}else{""},xml(text))}
fn wc(text:&str,width:i32)->String{format!("<w:tc><w:tcPr><w:tcW w:w=\"{}\" w:type=\"dxa\"/><w:tcBorders><w:top w:val=\"single\" w:sz=\"4\"/><w:left w:val=\"single\" w:sz=\"4\"/><w:bottom w:val=\"single\" w:sz=\"4\"/><w:right w:val=\"single\" w:sz=\"4\"/></w:tcBorders></w:tcPr>{}</w:tc>",width,wp(text,false))}
fn word_document(v:&ContractInput)->String{
 let totals=summary(v);let subtotal=totals["subtotal"].as_f64().unwrap_or(0.0);let tax=totals["tax"].as_f64().unwrap_or(0.0);let total=totals["total"].as_f64().unwrap_or(0.0);let mut b=String::from("<w:p><w:pPr><w:jc w:val=\"center\"/></w:pPr><w:r><w:rPr><w:b/><w:sz w:val=\"36\"/><w:rFonts w:eastAsia=\"黑体\"/></w:rPr><w:t>采购合同</w:t></w:r></w:p>");
 for(t,bold)in[(format!("合同编号：{}",v.contract_no),false),("购买方：厦门非凡智汇电子商务有限公司　（以下简称：甲方）".into(),true),(format!("销售方：{}　（以下简称：乙方）",v.seller_name),true),("甲、乙双方根据有关法律规定，在平等、自愿的基础上，经充分协商，就甲方向乙方购买产品达成以下购销合同条款。".into(),false),("一、产品名称".into(),true)]{b.push_str(&wp(&t,bold))}
 b.push_str("<w:tbl><w:tr>");for(h,w)in[("货物名称",3200),("单位",800),("数量",1000),("不含税单价",1500),("金额",1500)]{b.push_str(&wc(h,w))}b.push_str("</w:tr>");for x in &v.items{b.push_str("<w:tr>");for(t,w)in[(x.product_name.clone(),3200),(x.unit.clone(),800),(x.quantity.to_string(),1000),(format!("¥ {:.2}",x.unit_price_ex_tax),1500),(format!("¥ {:.2}",x.quantity*x.unit_price_ex_tax),1500)]{b.push_str(&wc(&t,w))}b.push_str("</w:tr>")}b.push_str("</w:tbl>");
 b.push_str(&wp(&format!("合计：以上为不含税价，税率 {}%，税费 ¥ {:.2}，价税合计：{}（¥ {:.2}）；未税金额 ¥ {:.2}。",v.tax_rate,tax,v.amount_uppercase,total,subtotal),false));for(h,t)in[("二、产品交付",&v.delivery_terms),("三、价款结算",&v.payment_terms),("四、违约责任",&v.breach_terms),("五、其他约定",&v.other_terms)]{b.push_str(&wp(h,true));b.push_str(&wp(t,false))}b.push_str(&wp("注：本合同书一式两份，双方各执一份。",true));
 for t in [format!("乙方（公章）：{}　　　　　　　　　甲方（公章）：厦门非凡智汇电子商务有限公司",v.seller_name),format!("乙方税号：{}　　　　　　　　　甲方税号：91350203MAEB4G4K7T",v.seller_tax_no),format!("乙方地址：{}　　　　　　　　　甲方地址：厦门市思明区体育路78号801室",v.seller_address),format!("乙方电话：{}　　　　　　　　　甲方开户行：招商银行股份有限公司厦门体育中心支行",v.seller_phone),format!("乙方开户行：{}　　　　　　　　甲方账号：592910153310001",v.seller_bank),format!("乙方账号：{}　　　　　　　　　日期：{}",v.seller_account,v.contract_date)]{b.push_str(&wp(&t,false))}
 format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{}<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="700" w:right="800" w:bottom="700" w:left="800"/></w:sectPr></w:body></w:document>"#,b)
}
fn export_word(v:&ContractInput,state:&AppState)->Result<String,String>{validate(v)?;let folder=state.data_dir.parent().unwrap_or(&state.data_dir).join("exports").join("contracts");fs::create_dir_all(&folder).map_err(|e|e.to_string())?;let safe:String=v.contract_no.chars().map(|c|if c.is_alphanumeric()||matches!(c,'-'|'_'){c}else{'_'}).collect();let path=folder.join(format!("采购合同_{}.docx",if safe.is_empty(){"未编号"}else{&safe}));let mut z=zip::ZipWriter::new(fs::File::create(&path).map_err(|e|e.to_string())?);let o=zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);for(n,d)in[("[Content_Types].xml",r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#.to_string()),("_rels/.rels",r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#.to_string()),("word/document.xml",word_document(v))]{z.start_file(n,o).map_err(|e|e.to_string())?;z.write_all(d.as_bytes()).map_err(|e|e.to_string())?}z.finish().map_err(|e|e.to_string())?;let _=open::that(&path);Ok(path.to_string_lossy().into_owned())}

fn detail(c:&rusqlite::Connection,id:i64)->Result<Value,String>{
    let mut contract:Value=c.query_row("SELECT id,contract_no,seller_name,seller_tax_no,seller_bank,seller_account,seller_address,seller_phone,tax_rate,contract_date,delivery_terms,payment_terms,breach_terms,other_terms,status,created_at,updated_at FROM purchase_contracts WHERE id=?1",[id],|r|Ok(json!({
      "id":r.get::<_,i64>(0)?,"contractNo":r.get::<_,String>(1)?,"sellerName":r.get::<_,String>(2)?,"sellerTaxNo":r.get::<_,String>(3)?,"sellerBank":r.get::<_,String>(4)?,"sellerAccount":r.get::<_,String>(5)?,"sellerAddress":r.get::<_,String>(6)?,"sellerPhone":r.get::<_,String>(7)?,"taxRate":r.get::<_,f64>(8)?,"contractDate":r.get::<_,String>(9)?,"deliveryTerms":r.get::<_,String>(10)?,"paymentTerms":r.get::<_,String>(11)?,"breachTerms":r.get::<_,String>(12)?,"otherTerms":r.get::<_,String>(13)?,"status":r.get::<_,String>(14)?,"createdAt":r.get::<_,String>(15)?,"updatedAt":r.get::<_,String>(16)?
    }))).optional().map_err(|e|e.to_string())?.ok_or("合同不存在")?;
    let mut s=c.prepare("SELECT id,product_name,unit,quantity,unit_price_ex_tax FROM purchase_contract_items WHERE contract_id=?1 ORDER BY sort_order,id").map_err(|e|e.to_string())?;
    let items=s.query_map([id],|r|Ok(json!({"id":r.get::<_,i64>(0)?,"productName":r.get::<_,String>(1)?,"unit":r.get::<_,String>(2)?,"quantity":r.get::<_,f64>(3)?,"unitPriceExTax":r.get::<_,f64>(4)?}))).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
    contract["items"]=json!(items); Ok(contract)
}

#[tauri::command]
pub async fn purchase_contract_command(state:State<'_,AppState>,command:String,id:Option<i64>,payload:Option<Value>)->Result<Value,String>{
 let state=background_state(&state)?; tauri::async_runtime::spawn_blocking(move||{
  let mut c=db(&state)?; ensure(&c)?;
  match command.as_str(){
   "list"=>{let mut s=c.prepare("SELECT id,contract_no,seller_name,contract_date,tax_rate,status,updated_at,(SELECT COALESCE(SUM(quantity*unit_price_ex_tax),0) FROM purchase_contract_items i WHERE i.contract_id=p.id) FROM purchase_contracts p WHERE status!='archived' ORDER BY contract_date DESC,id DESC").map_err(|e|e.to_string())?;let rows=s.query_map([],|r|{let subtotal:f64=r.get(7)?;let rate:f64=r.get(4)?;Ok(json!({"id":r.get::<_,i64>(0)?,"contractNo":r.get::<_,String>(1)?,"sellerName":r.get::<_,String>(2)?,"contractDate":r.get::<_,String>(3)?,"taxRate":rate,"status":r.get::<_,String>(5)?,"updatedAt":r.get::<_,String>(6)?,"total":((subtotal*(1.0+rate/100.0))*100.0).round()/100.0}))}).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;Ok(json!({"contracts":rows}))},
   "detail"=>detail(&c,id.ok_or("缺少合同 ID")?),
   "save"=>{let input:ContractInput=serde_json::from_value(payload.unwrap_or(json!({}))).map_err(|e|e.to_string())?;validate(&input)?;let totals=summary(&input);let tx=c.transaction().map_err(|e|e.to_string())?;let contract_id=if let Some(id)=id{tx.execute("UPDATE purchase_contracts SET contract_no=?1,seller_name=?2,seller_tax_no=?3,seller_bank=?4,seller_account=?5,seller_address=?6,seller_phone=?7,tax_rate=?8,contract_date=?9,delivery_terms=?10,payment_terms=?11,breach_terms=?12,other_terms=?13,updated_at=CURRENT_TIMESTAMP WHERE id=?14",params![input.contract_no,input.seller_name,input.seller_tax_no,input.seller_bank,input.seller_account,input.seller_address,input.seller_phone,input.tax_rate,input.contract_date,input.delivery_terms,input.payment_terms,input.breach_terms,input.other_terms,id]).map_err(|e|e.to_string())?;tx.execute("DELETE FROM purchase_contract_items WHERE contract_id=?1",[id]).map_err(|e|e.to_string())?;id}else{tx.execute("INSERT INTO purchase_contracts(contract_no,seller_name,seller_tax_no,seller_bank,seller_account,seller_address,seller_phone,tax_rate,contract_date,delivery_terms,payment_terms,breach_terms,other_terms)VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",params![input.contract_no,input.seller_name,input.seller_tax_no,input.seller_bank,input.seller_account,input.seller_address,input.seller_phone,input.tax_rate,input.contract_date,input.delivery_terms,input.payment_terms,input.breach_terms,input.other_terms]).map_err(|e|e.to_string())?;tx.last_insert_rowid()};for(index,item)in input.items.iter().enumerate(){tx.execute("INSERT INTO purchase_contract_items(contract_id,product_name,unit,quantity,unit_price_ex_tax,sort_order)VALUES(?1,?2,?3,?4,?5,?6)",params![contract_id,item.product_name.trim(),item.unit.trim(),item.quantity,item.unit_price_ex_tax,index as i64]).map_err(|e|e.to_string())?;}tx.commit().map_err(|e|e.to_string())?;Ok(json!({"id":contract_id,"totals":totals}))},
   "export_word"=>{let input:ContractInput=serde_json::from_value(payload.unwrap_or(json!({}))).map_err(|e|e.to_string())?;Ok(json!({"path":export_word(&input,&state)?}))},
   "archive"=>{let id=id.ok_or("缺少合同 ID")?;c.execute("UPDATE purchase_contracts SET status='archived',updated_at=CURRENT_TIMESTAMP WHERE id=?1",[id]).map_err(|e|e.to_string())?;Ok(json!({"ok":true}))},
   _=>Err("不支持的合同命令".into())
  }
 }).await.map_err(|e|e.to_string())?
}

#[cfg(test)] mod tests{use super::*;fn sample()->ContractInput{ContractInput{contract_no:"CG-1".into(),seller_name:"乙方".into(),seller_tax_no:"".into(),seller_bank:"".into(),seller_account:"".into(),seller_address:"".into(),seller_phone:"".into(),tax_rate:1.0,contract_date:"2026-09-08".into(),delivery_terms:"".into(),payment_terms:"".into(),breach_terms:"".into(),other_terms:"".into(),amount_uppercase:"人民币壹万元整".into(),items:vec![ContractItemInput{product_name:"产品".into(),unit:"个".into(),quantity:5000.0,unit_price_ex_tax:2.15}]}}#[test]fn calculates_tax(){let v=summary(&sample());assert_eq!(v["subtotal"],10750.0);assert_eq!(v["tax"],107.5);assert_eq!(v["total"],10857.5)}#[test]fn persists_contract(){let c=rusqlite::Connection::open_in_memory().unwrap();ensure(&c).unwrap();assert!(c.prepare("SELECT * FROM purchase_contracts").is_ok())}}
