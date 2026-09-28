use rusqlite::{params, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{fs, io::Write, path::Path};
use tauri::State;

use super::{background_state, db, feishu_base, feishu_raw, feishu_token, setting, AppState};

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

fn calculated_quantity_scaled(item: &PurchaseOrderItem) -> Result<i64, String> {
    let rate = decimal(&item.units_per_carton, "装箱率", false)?.unwrap();
    let cartons = decimal(&item.carton_count, "箱数", false)?.unwrap();
    if rate <= 0 || cartons <= 0 {
        return Err("装箱率和箱数必须大于 0".into());
    }
    rate.checked_mul(cartons)
        .and_then(|value| value.checked_div(10000))
        .ok_or_else(|| "自动计算的数量过大".to_string())
}

fn scaled_decimal_string(value: i64) -> String {
    let whole = value / 10000;
    let fraction = (value % 10000).abs();
    if fraction == 0 {
        whole.to_string()
    } else {
        format!("{whole}.{fraction:04}")
            .trim_end_matches('0')
            .to_string()
    }
}

fn calculated_quantity(item: &PurchaseOrderItem) -> Result<String, String> {
    calculated_quantity_scaled(item).map(scaled_decimal_string)
}

fn safe_file_component(value: &str, fallback: &str) -> String {
    let cleaned = value
        .trim()
        .chars()
        .map(|c| {
            if matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect::<String>()
        .trim_matches([' ', '.'])
        .to_string();
    if cleaned.is_empty() {
        fallback.into()
    } else {
        cleaned
    }
}

fn export_file_stem(v: &PurchaseOrderInput) -> String {
    format!(
        "采购单-{}-{}",
        safe_file_component(&v.order_no, "未编号"),
        safe_file_component(&v.title, "未命名产品")
    )
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
        if price < 0 {
            return Err(format!("第 {} 行单价不能为负数", i + 1));
        }
        calculated_quantity_scaled(x).map_err(|error| format!("第 {} 行{}", i + 1, error))?;
        for (n, s) in [
            ("包装长", &x.package_length_cm),
            ("包装宽", &x.package_width_cm),
            ("包装高", &x.package_height_cm),
            ("重量", &x.unit_weight_kg),
            ("外箱重量", &x.carton_weight_kg),
            ("外箱长", &x.carton_length_cm),
            ("外箱宽", &x.carton_width_cm),
            ("外箱高", &x.carton_height_cm),
        ] {
            decimal(s, n, true)?;
        }
    }
    Ok(())
}

fn display_number(value: &str) -> String {
    let number = value.trim().parse::<f64>().unwrap_or_default();
    if (number.fract()).abs() < f64::EPSILON {
        format!("{number:.0}")
    } else {
        format!("{number:.2}")
    }
}

fn calculated_quantity_number(item: &PurchaseOrderItem) -> f64 {
    calculated_quantity(item)
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or_default()
}

fn purchase_order_feishu_cards(v: &PurchaseOrderInput) -> Vec<Value> {
    let total_quantity = v.items.iter().map(calculated_quantity_number).sum::<f64>();
    let total_cartons = v
        .items
        .iter()
        .map(|item| item.carton_count.trim().parse::<f64>().unwrap_or_default())
        .sum::<f64>();
    let total_amount = v
        .items
        .iter()
        .map(|item| {
            item.unit_price.trim().parse::<f64>().unwrap_or_default()
                * calculated_quantity_number(item)
        })
        .sum::<f64>();
    let chunks = v.items.chunks(15).collect::<Vec<_>>();
    let total_parts = chunks.len().max(1);
    chunks
        .into_iter()
        .enumerate()
        .map(|(index, chunk)| {
            let summary = if index == 0 {
                format!(
                    "**采购单号：** {}\n**采购日期：** {}　**预计出货：** {}\n**经办人：** {}　**审批人：** {}\n**工厂地址：** {}\n\n**共 {} 项｜数量 {}｜箱数 {}｜总金额 ¥{:.2}**\n\n",
                    v.order_no.trim(),
                    v.order_date,
                    if v.expected_ship_at.trim().is_empty() { "—" } else { v.expected_ship_at.trim() },
                    if v.operator.trim().is_empty() { "—" } else { v.operator.trim() },
                    if v.approver.trim().is_empty() { "待领导审核" } else { v.approver.trim() },
                    if v.factory_address.trim().is_empty() { "—" } else { v.factory_address.trim() },
                    v.items.len(), display_number(&total_quantity.to_string()),
                    display_number(&total_cartons.to_string()), total_amount,
                )
            } else {
                String::new()
            };
            let details = chunk.iter().enumerate().map(|(offset, item)| {
                let quantity = calculated_quantity(item).unwrap_or_default();
                let amount = item.unit_price.trim().parse::<f64>().unwrap_or_default()
                    * calculated_quantity_number(item);
                format!(
                    "**{}. {}**　`{}`\n单价 ¥{}｜数量 {} {}｜箱数 {}｜装箱率 {}｜小计 ¥{:.2}{}",
                    index * 15 + offset + 1,
                    item.product_name.trim(), item.sku.trim(), display_number(&item.unit_price),
                    display_number(&quantity), item.unit.trim(), display_number(&item.carton_count),
                    display_number(&item.units_per_carton), amount,
                    if item.note.trim().is_empty() { String::new() } else { format!("｜备注 {}", item.note.trim()) },
                )
            }).collect::<Vec<_>>().join("\n\n");
            let note = if index + 1 == total_parts && !v.note.trim().is_empty() {
                format!("\n\n**采购单备注：** {}", v.note.trim())
            } else {
                String::new()
            };
            json!({
                "config": {"wide_screen_mode": true, "enable_forward": true},
                "header": {
                    "template": "orange",
                    "title": {"tag":"plain_text", "content":format!("待审核 · {}", if v.title.trim().is_empty() { "采购清单" } else { v.title.trim() })},
                    "subtitle": {"tag":"plain_text", "content":format!("第 {} / {} 页", index + 1, total_parts)}
                },
                "elements": [
                    {"tag":"div", "text":{"tag":"lark_md", "content":format!("{}{}{}", summary, details, note)}},
                    {"tag":"note", "elements":[{"tag":"plain_text", "content":"该采购单由 Ozon ERP 提交，请领导审核过目；如需调整，请联系经办人后在软件中修改并重新提交。"}]}
                ]
            })
        })
        .collect()
}

fn send_purchase_order_to_feishu(
    c: &rusqlite::Connection,
    v: &PurchaseOrderInput,
) -> Result<usize, String> {
    let token = feishu_token(c)?;
    let chat = setting(c, "feishu_chat_id");
    if chat.is_empty() {
        return Err("请先在当前店铺的飞书协作设置中配置群 Chat ID".into());
    }
    let cards = purchase_order_feishu_cards(v);
    for card in &cards {
        feishu_raw(
            "POST",
            &format!("{}/im/v1/messages?receive_id_type=chat_id", feishu_base(c)),
            Some(&token),
            Some(&json!({"receive_id":chat,"msg_type":"interactive","content":card.to_string()})),
        )?;
    }
    Ok(cards.len())
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
    // Match the supplied purchasing sheet: A:V, title, grouped headers, body,
    // totals, and operator footer. All figures remain numeric/formula cells.
    let columns = ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V"];
    let title = if v.title.trim().is_empty() { v.order_no.trim().to_string() } else { format!("{}-{}", v.order_no.trim(), v.title.trim()) };
    let (prefix, accent) = if let Some(rest) = title.strip_prefix("CG-") { ("CG-", rest) } else { ("", title.as_str()) };
    let title_cell = format!(r#"<c r="A1" t="inlineStr" s="1"><is><r><rPr><rFont val="Microsoft YaHei"/><sz val="25"/></rPr><t>{}</t></r><r><rPr><rFont val="Microsoft YaHei"/><sz val="25"/><color rgb="FFFF2D2D"/></rPr><t>{}</t></r></is></c>"#, xml(prefix), xml(accent));
    let mut rows = format!(r#"<row r="1" ht="42.26" customHeight="1">{title_cell}</row>"#);
    let headers = [("A", "中文品名"), ("B", "SKU"), ("C", "图片"), ("D", "单价"), ("E", "包装规格"), ("H", "重量kg"), ("I", "外箱重"), ("J", "装箱率"), ("K", "外箱规格"), ("N", "箱数"), ("O", "数量"), ("P", "单位"), ("Q", "总价"), ("R", "总重"), ("S", "体积"), ("T", "密度"), ("U", "工厂地址"), ("V", "备注")];
    let mut header_cells = String::new();
    for (col, label) in headers {
        let style = if col == "N" { 3 } else if matches!(col, "O" | "Q") { 4 } else { 2 };
        header_cells.push_str(&xt(&format!("{col}2"), label, style));
    }
    rows.push_str(&format!(r#"<row r="2" ht="27.38" customHeight="1">{header_cells}</row>"#));
    for (i, x) in v.items.iter().enumerate() {
        let r = i + 3;
        let mut c = xt(&format!("A{r}"), &x.product_name, 6)
            + &xt(&format!("B{r}"), &x.sku, 6)
            + &xt(&format!("C{r}"), &x.image_url, 6);
        for (col, val) in [
            ("D", &x.unit_price),
            ("E", &x.package_length_cm),
            ("F", &x.package_width_cm),
            ("G", &x.package_height_cm),
            ("H", &x.unit_weight_kg),
            ("I", &x.carton_weight_kg),
            ("J", &x.units_per_carton),
            ("K", &x.carton_length_cm),
            ("L", &x.carton_width_cm),
            ("M", &x.carton_height_cm),
            ("N", &x.carton_count),
        ] {
            c.push_str(&xn(
                &format!("{col}{r}"),
                val,
                if col == "D" { 7 } else { 5 },
            ))
        }
        c.push_str(&xf(&format!("O{r}"), &format!("J{r}*N{r}"), 8));
        c.push_str(&xt(
            &format!("P{r}"),
            if x.unit.trim().is_empty() {
                "个"
            } else {
                &x.unit
            },
            5,
        ));
        c.push_str(&xf(&format!("Q{r}"), &format!("D{r}*O{r}"), 13));
        c.push_str(&xf(
            &format!("R{r}"),
            &format!("I{r}*N{r}"),
            5,
        ));
        c.push_str(&xf(
            &format!("S{r}"),
            &format!("K{r}*L{r}*M{r}/1000000*N{r}"),
            5,
        ));
        c.push_str(&xf(&format!("T{r}"), &format!("IF(S{r}=0,\"\",R{r}/S{r})"), 5));
        c.push_str(&xt(&format!("U{r}"), &v.factory_address, 5));
        c.push_str(&xt(&format!("V{r}"), &x.note, 5));
        rows.push_str(&format!(
            r#"<row r="{r}" ht="40.48" customHeight="1">{c}</row>"#
        ));
    }
    let total = v.items.len() + 3;
    let mut c = String::new();
    for col in columns {
        let cell = format!("{col}{total}");
        c.push_str(&match col {
            "A" => xt(&cell, "合计", 11),
            "D" => xf(&cell, &format!("IF(O{total}=0,\"\",Q{total}/O{total})"), 14),
            "N" | "O" | "R" | "S" => xf(&cell, &format!("SUM({col}3:{col}{})", total - 1), 9),
            "P" => xt(&cell, "个", 9),
            "Q" => xf(&cell, &format!("SUM(Q3:Q{})", total - 1), 10),
            "T" => xf(&cell, &format!("IF(S{total}=0,\"\",R{total}/S{total})"), 9),
            _ => format!(r#"<c r="{cell}" s="5"/>"#),
        });
    }
    rows.push_str(&format!(
        r#"<row r="{total}" ht="34.52" customHeight="1">{c}</row>"#
    ));
    let footer = total + 1;
    let mut footer_cells = xt(&format!("A{footer}"), &format!("经办人: {}", v.operator.trim()), 12);
    for col in &columns[1..20] { footer_cells.push_str(&format!(r#"<c r="{col}{footer}" s="5"/>"#)); }
    footer_cells.push_str(&xt(&format!("U{footer}"), &format!("预计出货: {}", v.expected_ship_at), 6));
    footer_cells.push_str(&xt(&format!("V{footer}"), &format!("审批: {}", v.approver), 6));
    rows.push_str(&format!(r#"<row r="{footer}" ht="34.52" customHeight="1">{footer_cells}</row>"#));
    let note_row = footer + 1;
    if !v.note.trim().is_empty() {
        rows.push_str(&format!(r#"<row r="{note_row}" ht="27.38" customHeight="1">{}</row>"#, xt(&format!("A{note_row}"), &format!("采购单备注: {}", v.note.trim()), 12)));
    }
    let widths = [21.01,11.10,12.0,11.10,9.07,9.07,9.07,9.07,9.07,9.07,9.07,9.07,9.07,9.07,9.07,9.07,17.07,14.03,14.03,14.03,16.06,15.04];
    let cols = widths.iter().enumerate().map(|(i,w)| format!(r#"<col min="{n}" max="{n}" width="{w:.2}" customWidth="1"/>"#, n=i+1)).collect::<String>();
    let extra_merge = if v.note.trim().is_empty() { String::new() } else { format!(r#"<mergeCell ref="A{note_row}:V{note_row}"/>"#) };
    let merge_count = if extra_merge.is_empty() { 3 } else { 4 };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetPr><pageSetUpPr fitToPage="1"/></sheetPr><sheetViews><sheetView showGridLines="1" workbookViewId="0"/></sheetViews><sheetFormatPr defaultRowHeight="15.48"/><cols>{cols}</cols><sheetData>{rows}</sheetData><mergeCells count="{merge_count}"><mergeCell ref="A1:V1"/><mergeCell ref="E2:G2"/><mergeCell ref="K2:M2"/>{extra_merge}</mergeCells><printOptions horizontalCentered="1"/><pageMargins left="0.2" right="0.2" top="0.35" bottom="0.35" header="0.15" footer="0.15"/><pageSetup orientation="landscape" fitToWidth="1" fitToHeight="0" paperSize="9"/></worksheet>"#
    )
}
fn write_xlsx(path: &Path, v: &PurchaseOrderInput) -> Result<(), String> {
    let mut z = zip::ZipWriter::new(fs::File::create(path).map_err(|e| e.to_string())?);
    let o = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let styles = r#"<?xml version="1.0" encoding="UTF-8"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<numFmts count="1"><numFmt numFmtId="164" formatCode="&quot;¥&quot;#,##0.00"/></numFmts>
<fonts count="5"><font><sz val="14"/><name val="Microsoft YaHei"/></font><font><sz val="25"/><name val="Microsoft YaHei"/></font><font><b/><sz val="14"/><name val="Microsoft YaHei"/></font><font><sz val="10"/><name val="Calibri"/></font><font><b/><sz val="12"/><name val="Microsoft YaHei"/></font></fonts>
<fills count="5"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill><fill><patternFill patternType="solid"><fgColor rgb="FF7EDAFB"/></patternFill></fill><fill><patternFill patternType="solid"><fgColor rgb="FFFFF258"/></patternFill></fill><fill><patternFill patternType="solid"><fgColor rgb="FFDADADA"/></patternFill></fill></fills>
<borders count="2"><border/><border><left style="thin"/><right style="thin"/><top style="thin"/><bottom style="thin"/></border></borders>
<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>
<cellXfs count="15">
<xf numFmtId="0" fontId="0" fillId="0" borderId="0"/>
<xf numFmtId="0" fontId="1" fillId="0" borderId="0" applyFont="1" applyAlignment="1"><alignment horizontal="center" vertical="center"/></xf>
<xf numFmtId="0" fontId="2" fillId="0" borderId="1" applyAlignment="1"><alignment horizontal="center" vertical="center" wrapText="1"/></xf>
<xf numFmtId="0" fontId="2" fillId="2" borderId="1" applyAlignment="1"><alignment horizontal="center" vertical="center" wrapText="1"/></xf>
<xf numFmtId="0" fontId="2" fillId="3" borderId="1" applyAlignment="1"><alignment horizontal="center" vertical="center" wrapText="1"/></xf>
<xf numFmtId="0" fontId="0" fillId="0" borderId="1" applyAlignment="1"><alignment horizontal="center" vertical="center" wrapText="1"/></xf>
<xf numFmtId="0" fontId="3" fillId="0" borderId="1" applyAlignment="1"><alignment horizontal="center" vertical="center" wrapText="1"/></xf>
<xf numFmtId="164" fontId="3" fillId="3" borderId="1" applyNumberFormat="1" applyAlignment="1"><alignment horizontal="center" vertical="center"/></xf>
<xf numFmtId="0" fontId="0" fillId="3" borderId="1" applyAlignment="1"><alignment horizontal="center" vertical="center"/></xf>
<xf numFmtId="0" fontId="2" fillId="0" borderId="1" applyAlignment="1"><alignment horizontal="center" vertical="center"/></xf>
<xf numFmtId="164" fontId="2" fillId="3" borderId="1" applyNumberFormat="1" applyAlignment="1"><alignment horizontal="center" vertical="center"/></xf>
<xf numFmtId="0" fontId="4" fillId="4" borderId="1" applyAlignment="1"><alignment horizontal="center" vertical="center"/></xf>
<xf numFmtId="0" fontId="4" fillId="0" borderId="1" applyAlignment="1"><alignment horizontal="left" vertical="center"/></xf>
<xf numFmtId="164" fontId="0" fillId="3" borderId="1" applyNumberFormat="1" applyAlignment="1"><alignment horizontal="center" vertical="center"/></xf>
<xf numFmtId="164" fontId="2" fillId="0" borderId="1" applyNumberFormat="1" applyAlignment="1"><alignment horizontal="center" vertical="center"/></xf>
</cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>"#;
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
    let p = dir.join(format!("{}.xlsx", export_file_stem(v)));
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
   "save"=>{let input:PurchaseOrderInput=serde_json::from_value(payload.unwrap_or(json!({}))).map_err(|e|e.to_string())?;validate(&input)?;let tx=c.transaction().map_err(|e|e.to_string())?;let oid=if let Some(id)=id{tx.execute("UPDATE purchase_orders SET order_no=?1,title=?2,operator=?3,order_date=?4,expected_ship_at=?5,factory_address=?6,approver=?7,note=?8,status='draft',updated_at=CURRENT_TIMESTAMP WHERE id=?9",params![input.order_no.trim(),input.title.trim(),input.operator.trim(),input.order_date,input.expected_ship_at,input.factory_address.trim(),input.approver.trim(),input.note.trim(),id]).map_err(|e|e.to_string())?;tx.execute("DELETE FROM purchase_order_items WHERE order_id=?1",[id]).map_err(|e|e.to_string())?;id}else{tx.execute("INSERT INTO purchase_orders(order_no,title,operator,order_date,expected_ship_at,factory_address,approver,note)VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![input.order_no.trim(),input.title.trim(),input.operator.trim(),input.order_date,input.expected_ship_at,input.factory_address.trim(),input.approver.trim(),input.note.trim()]).map_err(|e|e.to_string())?;tx.last_insert_rowid()};for(i,x)in input.items.iter().enumerate(){let quantity=calculated_quantity(x)?;tx.execute("INSERT INTO purchase_order_items(order_id,product_name,sku,image_url,unit_price,package_length_cm,package_width_cm,package_height_cm,unit_weight_kg,units_per_carton,carton_weight_kg,carton_length_cm,carton_width_cm,carton_height_cm,carton_count,quantity,unit,note,sort_order)VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",params![oid,x.product_name.trim(),x.sku.trim(),x.image_url.trim(),x.unit_price.trim(),x.package_length_cm.trim(),x.package_width_cm.trim(),x.package_height_cm.trim(),x.unit_weight_kg.trim(),x.units_per_carton.trim(),x.carton_weight_kg.trim(),x.carton_length_cm.trim(),x.carton_width_cm.trim(),x.carton_height_cm.trim(),x.carton_count.trim(),quantity,if x.unit.trim().is_empty(){"个"}else{x.unit.trim()},x.note.trim(),i as i64]).map_err(|e|e.to_string())?;}tx.commit().map_err(|e|e.to_string())?;Ok(json!({"id":oid}))},
   "submit_feishu"=>{let oid=id.ok_or("请先保存采购单")?;let input:PurchaseOrderInput=serde_json::from_value(detail(&c,oid)?).map_err(|e|e.to_string())?;validate(&input)?;let cards=send_purchase_order_to_feishu(&c,&input)?;c.execute("UPDATE purchase_orders SET status='submitted',updated_at=CURRENT_TIMESTAMP WHERE id=?1",[oid]).map_err(|e|e.to_string())?;Ok(json!({"ok":true,"cards":cards,"message":format!("采购单已提交并发送到飞书群，共 {} 张卡片",cards)}))},
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
        assert!(s.contains("<mergeCell ref=\"A1:V1\"/>"));
        assert!(s.contains("<mergeCell ref=\"E2:G2\"/>"));
        assert!(s.contains("<mergeCell ref=\"K2:M2\"/>"));
        assert!(s.contains("J3*N3"));
        assert!(s.contains("D3*O3"));
        assert!(s.contains("SUM(Q3:Q3)"));
        assert!(s.contains("经办人: W"));
        assert!(s.find("<sheetData>").unwrap() < s.find("<mergeCells").unwrap());
        drop(z);
        let _ = fs::remove_file(p);
    }

    #[test]
    fn feishu_card_contains_review_totals_and_product_details() {
        let cards = purchase_order_feishu_cards(&sample());
        assert_eq!(cards.len(), 1);
        let content = cards[0]
            .pointer("/elements/0/text/content")
            .and_then(Value::as_str)
            .unwrap();
        assert!(content.contains("采购单号：** CG-1"));
        assert!(content.contains("共 1 项｜数量 300｜箱数 12｜总金额 ¥3645.00"));
        assert!(content.contains("沙漠数码"));
        assert!(content.contains("`1.5*6`"));
        assert!(content.contains("小计 ¥3645.00"));
    }

    #[test]
    fn quantity_and_export_name_are_derived_from_current_inputs() {
        let value = sample();
        assert_eq!(calculated_quantity(&value.items[0]).unwrap(), "300");
        assert_eq!(export_file_stem(&value), "采购单-CG-1-伪装网");
        assert_eq!(safe_file_component("伪装/网:*?", "fallback"), "伪装_网___");
    }
}
