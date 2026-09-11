use rusqlite::{params, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{fs, io::Write, path::Path};
use tauri::State;

use super::{background_state, db, AppState};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PurchaseOrderInput {
    order_no: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    operator: String,
    order_date: String,
    #[serde(default)]
    expected_ship_at: String,
    #[serde(default)]
    factory_address: String,
    #[serde(default)]
    approver: String,
    #[serde(default)]
    note: String,
    items: Vec<PurchaseOrderItem>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PurchaseOrderItem {
    #[serde(default)]
    product_name: String,
    #[serde(default)]
    sku: String,
    #[serde(default)]
    image_url: String,
    #[serde(default)]
    unit_price: String,
    #[serde(default)]
    package_length_cm: String,
    #[serde(default)]
    package_width_cm: String,
    #[serde(default)]
    package_height_cm: String,
    #[serde(default)]
    unit_weight_kg: String,
    #[serde(default)]
    units_per_carton: String,
    #[serde(default)]
    carton_weight_kg: String,
    #[serde(default)]
    carton_length_cm: String,
    #[serde(default)]
    carton_width_cm: String,
    #[serde(default)]
    carton_height_cm: String,
    #[serde(default)]
    carton_count: String,
    #[serde(default)]
    quantity: String,
    #[serde(default)]
    unit: String,
    #[serde(default)]
    note: String,
}

pub(super) fn ensure(c: &rusqlite::Connection) -> Result<(), String> {
    c.execute_batch(r#"
      CREATE TABLE IF NOT EXISTS purchase_orders(
        id INTEGER PRIMARY KEY AUTOINCREMENT, order_no TEXT NOT NULL, title TEXT NOT NULL DEFAULT '',
        operator TEXT NOT NULL DEFAULT '', order_date TEXT NOT NULL, expected_ship_at TEXT NOT NULL DEFAULT '',
        factory_address TEXT NOT NULL DEFAULT '', approver TEXT NOT NULL DEFAULT '', note TEXT NOT NULL DEFAULT '',
        status TEXT NOT NULL DEFAULT 'draft', created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
      CREATE UNIQUE INDEX IF NOT EXISTS idx_purchase_order_no ON purchase_orders(order_no) WHERE status!='archived';
      CREATE TABLE IF NOT EXISTS purchase_order_items(
        id INTEGER PRIMARY KEY AUTOINCREMENT, order_id INTEGER NOT NULL, product_name TEXT NOT NULL DEFAULT '',
        sku TEXT NOT NULL DEFAULT '', image_url TEXT NOT NULL DEFAULT '', unit_price TEXT,
        package_length_cm TEXT, package_width_cm TEXT, package_height_cm TEXT, unit_weight_kg TEXT,
        units_per_carton TEXT, carton_weight_kg TEXT, carton_length_cm TEXT, carton_width_cm TEXT,
        carton_height_cm TEXT, carton_count TEXT, quantity TEXT, unit TEXT NOT NULL DEFAULT '个', note TEXT NOT NULL DEFAULT '',
        sort_order INTEGER NOT NULL DEFAULT 0, FOREIGN KEY(order_id) REFERENCES purchase_orders(id) ON DELETE CASCADE);
      CREATE INDEX IF NOT EXISTS idx_purchase_orders_date ON purchase_orders(order_date DESC,id DESC);
    "#).map_err(|e|e.to_string())
}

fn decimal(value: &str, name: &str, allow_empty: bool) -> Result<Option<i64>, String> {
    let s = value.trim();
    if s.is_empty() && allow_empty {
        return Ok(None);
    }
    let mut parts = s.split('.');
    let whole = parts.next().unwrap_or("");
    let frac = parts.next().unwrap_or("");
    if parts.next().is_some()
        || whole.is_empty()
        || !whole.chars().all(|c| c.is_ascii_digit())
        || !frac.chars().all(|c| c.is_ascii_digit())
        || frac.len() > 4
    {
        return Err(format!("{name}格式无效"));
    }
    let scale = 10_i64.pow((4 - frac.len()) as u32);
    let w = whole.parse::<i64>().map_err(|_| format!("{name}过大"))?;
    let f = if frac.is_empty() {
        0
    } else {
        frac.parse::<i64>().unwrap() * scale
    };
    Ok(Some(w * 10000 + f))
}
fn validate(v: &PurchaseOrderInput) -> Result<(), String> {
    if v.order_no.trim().is_empty() {
        return Err("采购单号不能为空".into());
    }
    if chrono::NaiveDate::parse_from_str(&v.order_date, "%Y-%m-%d").is_err() {
        return Err("采购日期无效".into());
    }
    if v.items.is_empty() {
        return Err("至少需要一项采购产品".into());
    }
    for (i, x) in v.items.iter().enumerate() {
        if x.product_name.trim().is_empty() || x.sku.trim().is_empty() {
            return Err(format!("第 {} 行必须填写品名和 SKU", i + 1));
        }
        let price = decimal(&x.unit_price, "单价", false)?.unwrap();
        let qty = decimal(&x.quantity, "数量", false)?.unwrap();
        if price < 0 || qty <= 0 {
            return Err(format!("第 {} 行单价不能为负数，数量必须大于 0", i + 1));
        }
        for (n, s) in [
            ("包装长", &x.package_length_cm),
            ("包装宽", &x.package_width_cm),
            ("包装高", &x.package_height_cm),
            ("重量", &x.unit_weight_kg),
            ("装箱率", &x.units_per_carton),
            ("外箱重量", &x.carton_weight_kg),
            ("外箱长", &x.carton_length_cm),
            ("外箱宽", &x.carton_width_cm),
            ("外箱高", &x.carton_height_cm),
            ("箱数", &x.carton_count),
        ] {
            decimal(s, n, true)?;
        }
    }
    Ok(())
}
fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
fn xt(r: &str, v: &str, s: u8) -> String {
    format!(
        r#"<c r="{r}" t="inlineStr" s="{s}"><is><t xml:space="preserve">{}</t></is></c>"#,
        xml(v)
    )
}
fn xn(r: &str, v: &str, s: u8) -> String {
    if v.trim().is_empty() {
        format!(r#"<c r="{r}" s="{s}"/>"#)
    } else {
        format!(r#"<c r="{r}" s="{s}"><v>{}</v></c>"#, xml(v.trim()))
    }
}
fn xf(r: &str, f: &str, s: u8) -> String {
    format!(r#"<c r="{r}" s="{s}"><f>{f}</f></c>"#)
}
fn sheet_xml(v: &PurchaseOrderInput) -> String {
    let hs = [
        "中文品名",
        "SKU",
        "图片链接",
        "单价",
        "包装长 cm",
        "包装宽 cm",
        "包装高 cm",
        "重量 kg",
        "装箱率",
        "外箱重量 kg",
        "外箱长 cm",
        "外箱宽 cm",
        "外箱高 cm",
        "箱数",
        "数量",
        "单位",
        "总价",
        "密度 kg/m³",
        "总重 kg",
        "体积 m³",
        "预计出货时间",
        "工厂地址",
        "备注",
    ];
    let cs = [
        "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R",
        "S", "T", "U", "V", "W",
    ];
    let mut rows = format!(
        r#"<row r="1" ht="34" customHeight="1">{}</row><row r="2" ht="24" customHeight="1">{}</row>"#,
        xt(
            "A1",
            &format!(
                "{} - 采购清单",
                if v.title.trim().is_empty() {
                    "采购单"
                } else {
                    v.title.trim()
                }
            ),
            1
        ),
        xt(
            "A2",
            &format!(
                "采购单编号：{}    采购日期：{}    经办人：{}    领导审批：{}",
                v.order_no, v.order_date, v.operator, v.approver
            ),
            2
        )
    );
    let h = hs
        .iter()
        .enumerate()
        .map(|(i, x)| xt(&format!("{}3", cs[i]), x, 3))
        .collect::<String>();
    rows.push_str(&format!(r#"<row r="3" ht="30" customHeight="1">{h}</row>"#));
    for (i, x) in v.items.iter().enumerate() {
        let r = i + 4;
        let mut c = xt(&format!("A{r}"), &x.product_name, 4)
            + &xt(&format!("B{r}"), &x.sku, 4)
            + &xt(&format!("C{r}"), &x.image_url, 4);
        for (col, val) in [
            ("D", &x.unit_price),
            ("E", &x.package_length_cm),
            ("F", &x.package_width_cm),
            ("G", &x.package_height_cm),
            ("H", &x.unit_weight_kg),
            ("I", &x.units_per_carton),
            ("J", &x.carton_weight_kg),
            ("K", &x.carton_length_cm),
            ("L", &x.carton_width_cm),
            ("M", &x.carton_height_cm),
            ("N", &x.carton_count),
            ("O", &x.quantity),
        ] {
            c.push_str(&xn(
                &format!("{col}{r}"),
                val,
                if matches!(col, "D" | "N" | "O") { 5 } else { 4 },
            ))
        }
        c.push_str(&xt(
            &format!("P{r}"),
            if x.unit.trim().is_empty() {
                "个"
            } else {
                &x.unit
            },
            4,
        ));
        c.push_str(&xf(&format!("Q{r}"), &format!("D{r}*O{r}"), 5));
        c.push_str(&xf(
            &format!("R{r}"),
            &format!("IFERROR(J{r}/(K{r}*L{r}*M{r}/1000000),0)"),
            4,
        ));
        c.push_str(&xf(&format!("S{r}"), &format!("J{r}*N{r}"), 4));
        c.push_str(&xf(
            &format!("T{r}"),
            &format!("K{r}*L{r}*M{r}/1000000*N{r}"),
            4,
        ));
        c.push_str(&xt(&format!("U{r}"), &v.expected_ship_at, 4));
        c.push_str(&xt(&format!("V{r}"), &v.factory_address, 4));
        c.push_str(&xt(&format!("W{r}"), &x.note, 4));
        rows.push_str(&format!(
            r#"<row r="{r}" ht="42" customHeight="1">{c}</row>"#
        ));
    }
    let total = v.items.len() + 4;
    let mut c = xt(&format!("A{total}"), "合计", 6);
    for col in ["N", "O"] {
        c.push_str(&xf(
            &format!("{col}{total}"),
            &format!("SUM({col}4:{col}{})", total - 1),
            6,
        ))
    }
    c.push_str(&xt(&format!("P{total}"), "个", 6));
    for col in ["Q", "S", "T"] {
        c.push_str(&xf(
            &format!("{col}{total}"),
            &format!("SUM({col}4:{col}{})", total - 1),
            6,
        ))
    }
    rows.push_str(&format!(
        r#"<row r="{total}" ht="26" customHeight="1">{c}</row>"#
    ));
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetViews><sheetView showGridLines="0" workbookViewId="0"><pane ySplit="3" topLeftCell="A4" activePane="bottomLeft" state="frozen"/></sheetView></sheetViews><sheetFormatPr defaultRowHeight="18"/><cols><col min="1" max="1" width="16" customWidth="1"/><col min="2" max="2" width="12" customWidth="1"/><col min="3" max="3" width="22" customWidth="1"/><col min="4" max="23" width="13" customWidth="1"/></cols><sheetData>{rows}</sheetData><autoFilter ref="A3:W{}"/><mergeCells count="3"><mergeCell ref="A1:W1"/><mergeCell ref="A2:W2"/><mergeCell ref="A{total}:M{total}"/></mergeCells><pageMargins left="0.2" right="0.2" top="0.35" bottom="0.35" header="0.15" footer="0.15"/><pageSetup orientation="landscape" fitToWidth="1" fitToHeight="0" paperSize="9"/></worksheet>"#,
        total - 1
    )
}
fn write_xlsx(path: &Path, v: &PurchaseOrderInput) -> Result<(), String> {
    let mut z = zip::ZipWriter::new(fs::File::create(path).map_err(|e| e.to_string())?);
    let o = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let styles = r#"<?xml version="1.0" encoding="UTF-8"?><styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fonts count="3"><font><sz val="10"/><name val="Microsoft YaHei"/></font><font><b/><sz val="20"/><color rgb="FFFF2D2D"/><name val="Microsoft YaHei"/></font><font><b/><sz val="10"/><name val="Microsoft YaHei"/></font></fonts><fills count="4"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill><fill><patternFill patternType="solid"><fgColor rgb="FFF4F6F9"/></patternFill></fill><fill><patternFill patternType="solid"><fgColor rgb="FFFFF36B"/></patternFill></fill></fills><borders count="2"><border/><border><left style="thin"/><right style="thin"/><top style="thin"/><bottom style="thin"/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="7"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/><xf numFmtId="0" fontId="1" fillId="0" borderId="0" applyAlignment="1"><alignment horizontal="center"/></xf><xf numFmtId="0" fontId="0" fillId="0" borderId="0" applyAlignment="1"><alignment horizontal="center"/></xf><xf numFmtId="0" fontId="2" fillId="2" borderId="1" applyAlignment="1"><alignment horizontal="center" vertical="center" wrapText="1"/></xf><xf numFmtId="4" fontId="0" fillId="0" borderId="1" applyAlignment="1"><alignment horizontal="center" vertical="center" wrapText="1"/></xf><xf numFmtId="4" fontId="0" fillId="3" borderId="1" applyAlignment="1"><alignment horizontal="center" vertical="center"/></xf><xf numFmtId="4" fontId="2" fillId="2" borderId="1" applyAlignment="1"><alignment horizontal="center" vertical="center"/></xf></cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>"#;
    let parts=[("[Content_Types].xml",r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/></Types>"#.to_string()),("_rels/.rels",r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#.to_string()),("xl/workbook.xml",r#"<?xml version="1.0" encoding="UTF-8"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="采购清单" sheetId="1" r:id="rId1"/></sheets><calcPr fullCalcOnLoad="1" forceFullCalc="1"/></workbook>"#.to_string()),("xl/_rels/workbook.xml.rels",r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#.to_string()),("xl/styles.xml",styles.to_string()),("xl/worksheets/sheet1.xml",sheet_xml(v))];
    for (n, d) in parts {
        z.start_file(n, o).map_err(|e| e.to_string())?;
        z.write_all(d.as_bytes()).map_err(|e| e.to_string())?
    }
    z.finish().map_err(|e| e.to_string())?;
    Ok(())
}
fn export_excel(v: &PurchaseOrderInput, state: &AppState) -> Result<String, String> {
    validate(v)?;
    let dir = state
        .data_dir
        .parent()
        .unwrap_or(&state.data_dir)
        .join("exports")
        .join("purchase-orders");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let safe: String = v
        .order_no
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let p = dir.join(format!(
        "采购单_{}.xlsx",
        if safe.is_empty() { "未编号" } else { &safe }
    ));
    write_xlsx(&p, v)?;
    let _ = open::that(&p);
    Ok(p.to_string_lossy().into_owned())
}
fn detail(c: &rusqlite::Connection, id: i64) -> Result<Value, String> {
    let mut v:Value=c.query_row("SELECT json_object('id',id,'orderNo',order_no,'title',title,'operator',operator,'orderDate',order_date,'expectedShipAt',expected_ship_at,'factoryAddress',factory_address,'approver',approver,'note',note,'status',status,'createdAt',created_at,'updatedAt',updated_at) FROM purchase_orders WHERE id=?1",[id],|r|{let s:String=r.get(0)?;Ok(serde_json::from_str(&s).unwrap_or(Value::Null))}).optional().map_err(|e|e.to_string())?.ok_or("采购单不存在")?;
    let mut q=c.prepare("SELECT json_object('id',id,'productName',product_name,'sku',sku,'imageUrl',image_url,'unitPrice',COALESCE(unit_price,''),'packageLengthCm',COALESCE(package_length_cm,''),'packageWidthCm',COALESCE(package_width_cm,''),'packageHeightCm',COALESCE(package_height_cm,''),'unitWeightKg',COALESCE(unit_weight_kg,''),'unitsPerCarton',COALESCE(units_per_carton,''),'cartonWeightKg',COALESCE(carton_weight_kg,''),'cartonLengthCm',COALESCE(carton_length_cm,''),'cartonWidthCm',COALESCE(carton_width_cm,''),'cartonHeightCm',COALESCE(carton_height_cm,''),'cartonCount',COALESCE(carton_count,''),'quantity',COALESCE(quantity,''),'unit',unit,'note',note) FROM purchase_order_items WHERE order_id=?1 ORDER BY sort_order,id").map_err(|e|e.to_string())?;
    let items = q
        .query_map([id], |r| {
            let s: String = r.get(0)?;
            Ok(serde_json::from_str(&s).unwrap_or(Value::Null))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    v["items"] = json!(items);
    Ok(v)
}

#[tauri::command]
pub async fn purchase_order_command(
    state: State<'_, AppState>,
    command: String,
    id: Option<i64>,
    payload: Option<Value>,
) -> Result<Value, String> {
    let state = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move||{
  let mut c=db(&state)?;ensure(&c)?;match command.as_str(){
   "list"=>{let mut q=c.prepare("SELECT json_object('id',id,'orderNo',order_no,'title',title,'operator',operator,'orderDate',order_date,'expectedShipAt',expected_ship_at,'factoryAddress',factory_address,'status',status,'updatedAt',updated_at,'itemCount',(SELECT COUNT(*) FROM purchase_order_items i WHERE i.order_id=p.id)) FROM purchase_orders p WHERE status!='archived' ORDER BY order_date DESC,id DESC").map_err(|e|e.to_string())?;let rows=q.query_map([],|r|{let s:String=r.get(0)?;Ok(serde_json::from_str(&s).unwrap_or(Value::Null))}).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;Ok(json!({"orders":rows}))},
   "detail"=>detail(&c,id.ok_or("缺少采购单 ID")?),
   "save"=>{let input:PurchaseOrderInput=serde_json::from_value(payload.unwrap_or(json!({}))).map_err(|e|e.to_string())?;validate(&input)?;let tx=c.transaction().map_err(|e|e.to_string())?;let oid=if let Some(id)=id{tx.execute("UPDATE purchase_orders SET order_no=?1,title=?2,operator=?3,order_date=?4,expected_ship_at=?5,factory_address=?6,approver=?7,note=?8,updated_at=CURRENT_TIMESTAMP WHERE id=?9",params![input.order_no.trim(),input.title.trim(),input.operator.trim(),input.order_date,input.expected_ship_at,input.factory_address.trim(),input.approver.trim(),input.note.trim(),id]).map_err(|e|e.to_string())?;tx.execute("DELETE FROM purchase_order_items WHERE order_id=?1",[id]).map_err(|e|e.to_string())?;id}else{tx.execute("INSERT INTO purchase_orders(order_no,title,operator,order_date,expected_ship_at,factory_address,approver,note)VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![input.order_no.trim(),input.title.trim(),input.operator.trim(),input.order_date,input.expected_ship_at,input.factory_address.trim(),input.approver.trim(),input.note.trim()]).map_err(|e|e.to_string())?;tx.last_insert_rowid()};for(i,x)in input.items.iter().enumerate(){tx.execute("INSERT INTO purchase_order_items(order_id,product_name,sku,image_url,unit_price,package_length_cm,package_width_cm,package_height_cm,unit_weight_kg,units_per_carton,carton_weight_kg,carton_length_cm,carton_width_cm,carton_height_cm,carton_count,quantity,unit,note,sort_order)VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",params![oid,x.product_name.trim(),x.sku.trim(),x.image_url.trim(),x.unit_price.trim(),x.package_length_cm.trim(),x.package_width_cm.trim(),x.package_height_cm.trim(),x.unit_weight_kg.trim(),x.units_per_carton.trim(),x.carton_weight_kg.trim(),x.carton_length_cm.trim(),x.carton_width_cm.trim(),x.carton_height_cm.trim(),x.carton_count.trim(),x.quantity.trim(),if x.unit.trim().is_empty(){"个"}else{x.unit.trim()},x.note.trim(),i as i64]).map_err(|e|e.to_string())?;}tx.commit().map_err(|e|e.to_string())?;Ok(json!({"id":oid}))},
   "archive"=>{let id=id.ok_or("缺少采购单 ID")?;c.execute("UPDATE purchase_orders SET status='archived',updated_at=CURRENT_TIMESTAMP WHERE id=?1",[id]).map_err(|e|e.to_string())?;Ok(json!({"ok":true}))},
   "export_excel"=>{let input:PurchaseOrderInput=serde_json::from_value(payload.unwrap_or(json!({}))).map_err(|e|e.to_string())?;Ok(json!({"path":export_excel(&input,&state)?}))},
   _=>Err("不支持的采购单命令".into())}
 }).await.map_err(|e|e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn schema_is_created() {
        let c = rusqlite::Connection::open_in_memory().unwrap();
        ensure(&c).unwrap();
        assert!(c.prepare("SELECT * FROM purchase_orders").is_ok())
    }
    #[test]
    fn decimal_rejects_float_noise() {
        assert_eq!(decimal("12.15", "单价", false).unwrap(), Some(121500));
        assert!(decimal("1.00001", "单价", false).is_err())
    }
    fn sample() -> PurchaseOrderInput {
        PurchaseOrderInput {
            order_no: "CG-1".into(),
            title: "伪装网".into(),
            operator: "W".into(),
            order_date: "2026-09-10".into(),
            expected_ship_at: "2026-09-20".into(),
            factory_address: "滨州".into(),
            approver: "".into(),
            note: "".into(),
            items: vec![PurchaseOrderItem {
                product_name: "沙漠数码".into(),
                sku: "1.5*6".into(),
                image_url: "".into(),
                unit_price: "12.15".into(),
                package_length_cm: "23".into(),
                package_width_cm: "16".into(),
                package_height_cm: "10".into(),
                unit_weight_kg: "0.47".into(),
                units_per_carton: "25".into(),
                carton_weight_kg: "11.75".into(),
                carton_length_cm: "60".into(),
                carton_width_cm: "50".into(),
                carton_height_cm: "40".into(),
                carton_count: "12".into(),
                quantity: "300".into(),
                unit: "个".into(),
                note: "".into(),
            }],
        }
    }
    #[test]
    fn creates_xlsx_with_formulas() {
        use std::io::Read;
        let p = std::env::temp_dir().join("purchase-order-test.xlsx");
        write_xlsx(&p, &sample()).unwrap();
        let mut z = zip::ZipArchive::new(fs::File::open(&p).unwrap()).unwrap();
        let mut s = String::new();
        z.by_name("xl/worksheets/sheet1.xml")
            .unwrap()
            .read_to_string(&mut s)
            .unwrap();
        assert!(s.contains("D4*O4"));
        assert!(s.contains("SUM(Q4:Q4)"));
        assert!(
            s.find("<autoFilter").unwrap() < s.find("<mergeCells").unwrap(),
            "worksheet child elements must follow the OOXML schema order"
        );
        drop(z);
        let _ = fs::remove_file(p);
    }
}
