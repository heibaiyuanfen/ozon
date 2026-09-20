use rusqlite::{params, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{fs, io::Write};
use tauri::State;

use super::{background_state, db, AppState};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContractInput {
    #[serde(default)]
    contract_no: String,
    seller_name: String,
    #[serde(default)]
    seller_tax_no: String,
    #[serde(default)]
    seller_bank: String,
    #[serde(default)]
    seller_account: String,
    #[serde(default)]
    seller_address: String,
    #[serde(default)]
    seller_phone: String,
    #[serde(default = "default_tax_rate")]
    tax_rate: f64,
    contract_date: String,
    #[serde(default)]
    delivery_terms: String,
    #[serde(default)]
    payment_terms: String,
    #[serde(default)]
    breach_terms: String,
    #[serde(default)]
    other_terms: String,
    items: Vec<ContractItemInput>,
    #[serde(default)]
    amount_uppercase: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContractItemInput {
    product_name: String,
    unit: String,
    quantity: f64,
    unit_price_ex_tax: f64,
}
fn default_tax_rate() -> f64 {
    1.0
}

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

fn validate(v: &ContractInput) -> Result<(), String> {
    if v.seller_name.trim().is_empty() {
        return Err("乙方名称不能为空".into());
    }
    if chrono::NaiveDate::parse_from_str(&v.contract_date, "%Y-%m-%d").is_err() {
        return Err("合同日期无效".into());
    }
    if !(0.0..=100.0).contains(&v.tax_rate) {
        return Err("税率必须在 0 至 100 之间".into());
    }
    if v.items.is_empty() {
        return Err("至少需要一项产品".into());
    }
    if v.items.iter().any(|x| {
        x.product_name.trim().is_empty()
            || x.unit.trim().is_empty()
            || x.quantity <= 0.0
            || x.unit_price_ex_tax < 0.0
    }) {
        return Err("请完整填写产品名称、单位、数量和单价".into());
    }
    Ok(())
}

fn summary(v: &ContractInput) -> Value {
    let subtotal = (v
        .items
        .iter()
        .map(|x| x.quantity * x.unit_price_ex_tax)
        .sum::<f64>()
        * 100.0)
        .round()
        / 100.0;
    let tax = (subtotal * v.tax_rate / 100.0 * 100.0).round() / 100.0;
    json!({"subtotal":subtotal,"tax":tax,"total":subtotal+tax})
}

fn xml(v: &str) -> String {
    v.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
fn wp_style(
    text: &str,
    bold: bool,
    align: &str,
    half_points: i32,
    after: i32,
    first_line_indent: bool,
    keep_next: bool,
) -> String {
    format!(
        "<w:p><w:pPr><w:jc w:val=\"{}\"/><w:spacing w:after=\"{}\" w:line=\"300\" w:lineRule=\"auto\"/>{}{}</w:pPr><w:r><w:rPr><w:rFonts w:ascii=\"Times New Roman\" w:hAnsi=\"Times New Roman\" w:eastAsia=\"宋体\"/>{}<w:sz w:val=\"{}\"/><w:szCs w:val=\"{}\"/></w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>",
        align,
        after,
        if first_line_indent { "<w:ind w:firstLineChars=\"200\"/>" } else { "" },
        if keep_next { "<w:keepNext/>" } else { "" },
        if bold { "<w:b/>" } else { "" },
        half_points,
        half_points,
        xml(text)
    )
}
fn wc(text: &str, width: i32, bold: bool, align: &str, shade: bool) -> String {
    format!(
        "<w:tc><w:tcPr><w:tcW w:w=\"{}\" w:type=\"dxa\"/>{}<w:tcMar><w:top w:w=\"90\" w:type=\"dxa\"/><w:left w:w=\"100\" w:type=\"dxa\"/><w:bottom w:w=\"90\" w:type=\"dxa\"/><w:right w:w=\"100\" w:type=\"dxa\"/></w:tcMar><w:vAlign w:val=\"center\"/></w:tcPr>{}</w:tc>",
        width,
        if shade { "<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"EDEDED\"/>" } else { "" },
        wp_style(text, bold, align, 20, 0, false, false)
    )
}
fn signature_cell(lines: &[String], width: i32) -> String {
    let content = lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            wp_style(
                line,
                index == 0,
                "left",
                20,
                if index == 0 { 80 } else { 25 },
                false,
                index == 0,
            )
        })
        .collect::<String>();
    format!("<w:tc><w:tcPr><w:tcW w:w=\"{}\" w:type=\"dxa\"/><w:vAlign w:val=\"top\"/><w:tcMar><w:left w:w=\"80\" w:type=\"dxa\"/><w:right w:w=\"160\" w:type=\"dxa\"/></w:tcMar></w:tcPr>{}</w:tc>", width, content)
}
fn word_document(v: &ContractInput) -> String {
    let totals = summary(v);
    let subtotal = totals["subtotal"].as_f64().unwrap_or(0.0);
    let tax = totals["tax"].as_f64().unwrap_or(0.0);
    let total = totals["total"].as_f64().unwrap_or(0.0);
    let mut b = wp_style("采购合同", true, "center", 36, 160, false, false);
    b.push_str(&wp_style(
        &format!("合同编号：{}", v.contract_no),
        false,
        "right",
        18,
        80,
        false,
        false,
    ));
    b.push_str(&wp_style(
        "购买方：厦门非凡智汇电子商务有限公司（以下简称：甲方）",
        true,
        "left",
        21,
        35,
        false,
        false,
    ));
    b.push_str(&wp_style(
        &format!("销售方：{}（以下简称：乙方）", v.seller_name),
        true,
        "left",
        21,
        60,
        false,
        false,
    ));
    b.push_str(&wp_style("甲、乙双方根据有关法律规定，在平等、自愿的基础上，经充分协商，就甲方向乙方购买产品达成以下购销合同条款。", false, "both", 21, 70, true, false));
    b.push_str(&wp_style("一、产品名称", true, "left", 22, 45, false, true));
    b.push_str("<w:tbl><w:tblPr><w:tblW w:w=\"10100\" w:type=\"dxa\"/><w:tblLayout w:type=\"fixed\"/><w:tblBorders><w:top w:val=\"single\" w:sz=\"6\" w:color=\"666666\"/><w:left w:val=\"single\" w:sz=\"6\" w:color=\"666666\"/><w:bottom w:val=\"single\" w:sz=\"6\" w:color=\"666666\"/><w:right w:val=\"single\" w:sz=\"6\" w:color=\"666666\"/><w:insideH w:val=\"single\" w:sz=\"4\" w:color=\"999999\"/><w:insideV w:val=\"single\" w:sz=\"4\" w:color=\"999999\"/></w:tblBorders></w:tblPr><w:tblGrid><w:gridCol w:w=\"3900\"/><w:gridCol w:w=\"850\"/><w:gridCol w:w=\"1100\"/><w:gridCol w:w=\"1900\"/><w:gridCol w:w=\"2350\"/></w:tblGrid><w:tr><w:trPr><w:tblHeader/></w:trPr>");
    for (h, w) in [
        ("货物名称", 3900),
        ("单位", 850),
        ("数量", 1100),
        ("不含税单价", 1900),
        ("金额", 2350),
    ] {
        b.push_str(&wc(h, w, true, "center", true))
    }
    b.push_str("</w:tr>");
    for x in &v.items {
        b.push_str("<w:tr>");
        for (index, (t, w)) in [
            (x.product_name.clone(), 3900),
            (x.unit.clone(), 850),
            (x.quantity.to_string(), 1100),
            (format!("¥ {:.2}", x.unit_price_ex_tax), 1900),
            (format!("¥ {:.2}", x.quantity * x.unit_price_ex_tax), 2350),
        ]
        .into_iter()
        .enumerate()
        {
            b.push_str(&wc(
                &t,
                w,
                false,
                if index == 0 { "left" } else { "center" },
                false,
            ))
        }
        b.push_str("</w:tr>")
    }
    b.push_str("</w:tbl>");
    b.push_str(&wp_style(&format!("合计：以上为不含税价，税率 {}%，税费 ¥ {:.2}，价税合计：{}（¥ {:.2}）；未税金额 ¥ {:.2}。",v.tax_rate,tax,v.amount_uppercase,total,subtotal),false,"both",20,65,false,false));
    for (h, t) in [
        ("二、产品交付", &v.delivery_terms),
        ("三、价款结算", &v.payment_terms),
        ("四、违约责任", &v.breach_terms),
        ("五、其他约定", &v.other_terms),
    ] {
        b.push_str(&wp_style(h, true, "left", 22, 20, false, true));
        b.push_str(&wp_style(t, false, "both", 21, 55, true, false))
    }
    b.push_str(&wp_style(
        "注：本合同书一式两份，双方各执一份。",
        true,
        "left",
        20,
        90,
        false,
        true,
    ));
    let seller = vec![
        format!("乙方（公章）：{}", v.seller_name),
        format!("名称：{}", v.seller_name),
        format!("税号：{}", v.seller_tax_no),
        format!("地址：{}", v.seller_address),
        format!("电话：{}", v.seller_phone),
        format!("开户行：{}", v.seller_bank),
        format!("账号：{}", v.seller_account),
    ];
    let buyer = vec![
        "甲方（公章）：".into(),
        "名称：厦门非凡智汇电子商务有限公司".into(),
        "税号：91350203MAEB4G4K7T".into(),
        "地址：厦门市思明区体育路78号801室".into(),
        "开户行：招商银行股份有限公司厦门体育中心支行".into(),
        "账号：592910153310001".into(),
        format!("日期：{}", v.contract_date),
    ];
    b.push_str("<w:tbl><w:tblPr><w:tblW w:w=\"10100\" w:type=\"dxa\"/><w:tblLayout w:type=\"fixed\"/><w:tblBorders><w:top w:val=\"nil\"/><w:left w:val=\"nil\"/><w:bottom w:val=\"nil\"/><w:right w:val=\"nil\"/><w:insideH w:val=\"nil\"/><w:insideV w:val=\"nil\"/></w:tblBorders></w:tblPr><w:tblGrid><w:gridCol w:w=\"5050\"/><w:gridCol w:w=\"5050\"/></w:tblGrid><w:tr><w:trPr><w:cantSplit/></w:trPr>");
    b.push_str(&signature_cell(&seller, 5050));
    b.push_str(&signature_cell(&buyer, 5050));
    b.push_str("</w:tr></w:tbl>");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{}<w:sectPr><w:pgSz w:w="11906" w:h="16838" w:orient="portrait"/><w:pgMar w:top="850" w:right="900" w:bottom="850" w:left="900" w:header="425" w:footer="425"/><w:cols w:space="425"/><w:docGrid w:linePitch="312"/></w:sectPr></w:body></w:document>"#,
        b
    )
}
fn export_word(v: &ContractInput, state: &AppState) -> Result<String, String> {
    validate(v)?;
    let folder = state
        .data_dir
        .parent()
        .unwrap_or(&state.data_dir)
        .join("exports")
        .join("contracts");
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let safe: String = v
        .contract_no
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let path = folder.join(format!(
        "采购合同_{}.docx",
        if safe.is_empty() { "未编号" } else { &safe }
    ));
    write_word_package(&path, v)?;
    let _ = open::that(&path);
    Ok(path.to_string_lossy().into_owned())
}

fn write_word_package(path: &std::path::Path, v: &ContractInput) -> Result<(), String> {
    let mut z = zip::ZipWriter::new(fs::File::create(path).map_err(|e| e.to_string())?);
    let o = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for(n,d)in[("[Content_Types].xml",r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#.to_string()),("_rels/.rels",r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#.to_string()),("word/document.xml",word_document(v))]{z.start_file(n,o).map_err(|e|e.to_string())?;z.write_all(d.as_bytes()).map_err(|e|e.to_string())?}
    z.finish().map_err(|e| e.to_string())?;
    Ok(())
}

fn detail(c: &rusqlite::Connection, id: i64) -> Result<Value, String> {
    let mut contract:Value=c.query_row("SELECT id,contract_no,seller_name,seller_tax_no,seller_bank,seller_account,seller_address,seller_phone,tax_rate,contract_date,delivery_terms,payment_terms,breach_terms,other_terms,status,created_at,updated_at FROM purchase_contracts WHERE id=?1",[id],|r|Ok(json!({
      "id":r.get::<_,i64>(0)?,"contractNo":r.get::<_,String>(1)?,"sellerName":r.get::<_,String>(2)?,"sellerTaxNo":r.get::<_,String>(3)?,"sellerBank":r.get::<_,String>(4)?,"sellerAccount":r.get::<_,String>(5)?,"sellerAddress":r.get::<_,String>(6)?,"sellerPhone":r.get::<_,String>(7)?,"taxRate":r.get::<_,f64>(8)?,"contractDate":r.get::<_,String>(9)?,"deliveryTerms":r.get::<_,String>(10)?,"paymentTerms":r.get::<_,String>(11)?,"breachTerms":r.get::<_,String>(12)?,"otherTerms":r.get::<_,String>(13)?,"status":r.get::<_,String>(14)?,"createdAt":r.get::<_,String>(15)?,"updatedAt":r.get::<_,String>(16)?
    }))).optional().map_err(|e|e.to_string())?.ok_or("合同不存在")?;
    let mut s=c.prepare("SELECT id,product_name,unit,quantity,unit_price_ex_tax FROM purchase_contract_items WHERE contract_id=?1 ORDER BY sort_order,id").map_err(|e|e.to_string())?;
    let items=s.query_map([id],|r|Ok(json!({"id":r.get::<_,i64>(0)?,"productName":r.get::<_,String>(1)?,"unit":r.get::<_,String>(2)?,"quantity":r.get::<_,f64>(3)?,"unitPriceExTax":r.get::<_,f64>(4)?}))).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
    contract["items"] = json!(items);
    Ok(contract)
}

#[tauri::command]
pub async fn purchase_contract_command(
    state: State<'_, AppState>,
    command: String,
    id: Option<i64>,
    payload: Option<Value>,
) -> Result<Value, String> {
    let state = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move||{
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

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> ContractInput {
        ContractInput {
            contract_no: "CG-1".into(),
            seller_name: "乙方".into(),
            seller_tax_no: "".into(),
            seller_bank: "".into(),
            seller_account: "".into(),
            seller_address: "".into(),
            seller_phone: "".into(),
            tax_rate: 1.0,
            contract_date: "2026-09-08".into(),
            delivery_terms: "".into(),
            payment_terms: "".into(),
            breach_terms: "".into(),
            other_terms: "".into(),
            amount_uppercase: "人民币壹万元整".into(),
            items: vec![ContractItemInput {
                product_name: "产品".into(),
                unit: "个".into(),
                quantity: 5000.0,
                unit_price_ex_tax: 2.15,
            }],
        }
    }
    #[test]
    fn calculates_tax() {
        let v = summary(&sample());
        assert_eq!(v["subtotal"], 10750.0);
        assert_eq!(v["tax"], 107.5);
        assert_eq!(v["total"], 10857.5)
    }
    #[test]
    fn persists_contract() {
        let c = rusqlite::Connection::open_in_memory().unwrap();
        ensure(&c).unwrap();
        assert!(c.prepare("SELECT * FROM purchase_contracts").is_ok())
    }
    #[test]
    fn exported_layout_uses_fixed_tables_instead_of_space_alignment() {
        let document = word_document(&sample());
        assert!(document.contains("<w:tblLayout w:type=\"fixed\"/>"));
        assert!(document.contains("<w:tblHeader/>"));
        assert!(document.contains("<w:gridCol w:w=\"5050\"/>"));
        assert!(document.contains("w:orient=\"portrait\""));
        assert!(!document.contains("　　　　　　　　　"));
    }
    #[test]
    #[ignore = "Generates a local DOCX for visual layout QA"]
    fn renders_contract_layout_fixture() {
        let output = std::env::var("CONTRACT_QA_OUTPUT").expect("CONTRACT_QA_OUTPUT is required");
        let mut value = sample();
        value.contract_no = "CG-2026091802".into();
        value.seller_name = "义乌市树淮电子商务有限公司".into();
        value.seller_tax_no = "91330782MA8GCYQQ2U".into();
        value.seller_address = "浙江省义乌市测试地址 168 号".into();
        value.seller_phone = "13800000000".into();
        value.seller_bank = "中国工商银行股份有限公司义乌商贸支行".into();
        value.seller_account = "1208021419100177535".into();
        value.delivery_terms = "乙方需提供每个产品的包装袋及贴标及装箱，并为箱子提供防水措施，并且拍照上传记录。甲方负责托运，运费由甲方支付。".into();
        value.payment_terms = "甲方应在本合同书签订之日起一次性向乙方预付100%货款，乙方收到预付款后生产制作，具体生产周期由双方约定。".into();
        value.breach_terms = "本合同签订后，任何一方违约，都应承担总货款10%的违约金。".into();
        value.other_terms =
            "本合同未约定的事项，由双方另行签订补充协议。补充协议与本合同具有同等法律效力。".into();
        value.amount_uppercase = "人民币肆仟叁佰贰拾陆元整".into();
        value.items = vec![ContractItemInput {
            product_name: "花酒头".into(),
            unit: "个".into(),
            quantity: 1050.0,
            unit_price_ex_tax: 4.0,
        }];
        write_word_package(std::path::Path::new(&output), &value).unwrap();
    }
}
