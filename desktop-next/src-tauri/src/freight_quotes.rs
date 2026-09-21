use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    time::Duration,
};
use tauri::State;

use super::{background_state, AppState};

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FreightQuoteInput {
    inquiry_no: String,
    title: String,
    #[serde(default)]
    route: String,
    #[serde(default)]
    destination: String,
    #[serde(default)]
    contact: String,
    #[serde(default)]
    note: String,
    items: Vec<FreightQuoteItem>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FreightQuoteItem {
    #[serde(default)]
    image_url: String,
    product_name: String,
    #[serde(default)]
    sku: String,
    package_length_cm: String,
    package_width_cm: String,
    package_height_cm: String,
    unit_weight_kg: String,
    units_per_carton: String,
    carton_length_cm: String,
    carton_width_cm: String,
    carton_height_cm: String,
    carton_weight_kg: String,
    carton_count: String,
    #[serde(default)]
    note: String,
}

fn number(value: &str, name: &str, row: usize) -> Result<f64, String> {
    let parsed = value
        .trim()
        .parse::<f64>()
        .map_err(|_| format!("第 {row} 行{name}格式无效"))?;
    if !parsed.is_finite() || parsed <= 0.0 {
        return Err(format!("第 {row} 行{name}必须大于 0"));
    }
    Ok(parsed)
}

fn validate(input: &FreightQuoteInput) -> Result<(), String> {
    if input.inquiry_no.trim().is_empty() {
        return Err("询价编号不能为空".into());
    }
    if input.title.trim().is_empty() {
        return Err("配货表名称不能为空".into());
    }
    if input.items.is_empty() {
        return Err("至少需要一项产品".into());
    }
    for (index, item) in input.items.iter().enumerate() {
        let row = index + 1;
        if item.product_name.trim().is_empty() {
            return Err(format!("第 {row} 行产品名称不能为空"));
        }
        for (name, value) in [
            ("包装长", &item.package_length_cm),
            ("包装宽", &item.package_width_cm),
            ("包装高", &item.package_height_cm),
            ("单件重量", &item.unit_weight_kg),
            ("装箱率", &item.units_per_carton),
            ("箱规长", &item.carton_length_cm),
            ("箱规宽", &item.carton_width_cm),
            ("箱规高", &item.carton_height_cm),
            ("单箱重", &item.carton_weight_kg),
            ("箱数", &item.carton_count),
        ] {
            number(value, name, row)?;
        }
    }
    Ok(())
}

fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
fn text_cell(reference: &str, value: &str, style: u8) -> String {
    format!(
        r#"<c r="{reference}" t="inlineStr" s="{style}"><is><t xml:space="preserve">{}</t></is></c>"#,
        xml(value)
    )
}
fn number_cell(reference: &str, value: &str, style: u8) -> String {
    format!(
        r#"<c r="{reference}" s="{style}"><v>{}</v></c>"#,
        xml(value.trim())
    )
}
fn formula_cell(reference: &str, formula: &str, style: u8) -> String {
    format!(r#"<c r="{reference}" s="{style}"><f>{formula}</f><v>0</v></c>"#)
}
fn safe_component(value: &str, fallback: &str) -> String {
    let value = value
        .trim()
        .chars()
        .map(|c| {
            if matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect::<String>();
    if value.trim_matches([' ', '.']).is_empty() {
        fallback.into()
    } else {
        value
    }
}

struct ImageAsset {
    bytes: Vec<u8>,
    extension: &'static str,
    content_type: &'static str,
    row: usize,
}

fn image_asset(source: &str, row: usize) -> Option<ImageAsset> {
    let source = source.trim();
    if source.is_empty() {
        return None;
    }
    let bytes = if source.starts_with("http://") || source.starts_with("https://") {
        let response = ureq::get(source)
            .timeout(Duration::from_secs(15))
            .call()
            .ok()?;
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(6 * 1024 * 1024)
            .read_to_end(&mut bytes)
            .ok()?;
        bytes
    } else {
        fs::read(source).ok()?
    };
    let (extension, content_type) = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        ("png", "image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        ("jpg", "image/jpeg")
    } else {
        return None;
    };
    Some(ImageAsset {
        bytes,
        extension,
        content_type,
        row,
    })
}

fn drawing_xml(images: &[ImageAsset]) -> String {
    let anchors = images.iter().enumerate().map(|(index, image)| {
        let id = index + 1;
        let row = image.row - 1;
        format!(r#"<xdr:twoCellAnchor editAs="oneCell"><xdr:from><xdr:col>1</xdr:col><xdr:colOff>80000</xdr:colOff><xdr:row>{row}</xdr:row><xdr:rowOff>80000</xdr:rowOff></xdr:from><xdr:to><xdr:col>2</xdr:col><xdr:colOff>-80000</xdr:colOff><xdr:row>{}</xdr:row><xdr:rowOff>-80000</xdr:rowOff></xdr:to><xdr:pic><xdr:nvPicPr><xdr:cNvPr id="{id}" name="产品图片 {id}"/><xdr:cNvPicPr><a:picLocks noChangeAspect="1"/></xdr:cNvPicPr></xdr:nvPicPr><xdr:blipFill><a:blip r:embed="rId{id}"/><a:stretch><a:fillRect/></a:stretch></xdr:blipFill><xdr:spPr><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></xdr:spPr></xdr:pic><xdr:clientData/></xdr:twoCellAnchor>"#, image.row)
    }).collect::<String>();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">{anchors}</xdr:wsDr>"#
    )
}

fn drawing_rels(images: &[ImageAsset]) -> String {
    let rels = images.iter().enumerate().map(|(index, image)| format!(r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image{}.{}"/>"#, index + 1, index + 1, image.extension)).collect::<String>();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{rels}</Relationships>"#
    )
}

fn sheet_xml(input: &FreightQuoteInput, has_images: bool) -> String {
    let headers = [
        "序号",
        "产品图片",
        "产品名称",
        "SKU",
        "包装长 cm",
        "包装宽 cm",
        "包装高 cm",
        "单件重量 kg",
        "装箱率 件/箱",
        "箱规长 cm",
        "箱规宽 cm",
        "箱规高 cm",
        "单箱重 kg",
        "箱数",
        "总数量 件",
        "总重 kg",
        "总体积 m³",
        "密度 kg/m³",
        "备注",
    ];
    let columns = [
        "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R",
        "S",
    ];
    let mut rows = String::new();
    rows.push_str(&format!(
        r#"<row r="2" ht="26" customHeight="1">{}</row>"#,
        text_cell("A2", &input.title, 1)
    ));
    rows.push_str(&format!(
        r#"<row r="3" ht="22" customHeight="1">{}</row>"#,
        text_cell(
            "A3",
            &format!(
                "询价编号：{}    运输线路：{}    目的地：{}    联系人：{}",
                input.inquiry_no, input.route, input.destination, input.contact
            ),
            2
        )
    ));
    let last = input.items.len() + 6;
    let summary = "明细中的总数量、总重、总体积和密度均由公式计算；底部合计行用于整票询价核对。";
    rows.push_str(&format!(
        r#"<row r="4" ht="24" customHeight="1">{}</row>"#,
        text_cell("A4", &summary, 3)
    ));
    rows.push_str(&format!(
        r#"<row r="5" ht="21" customHeight="1">{}</row>"#,
        text_cell("A5", &format!("备注：{}", input.note), 2)
    ));
    let header = headers
        .iter()
        .enumerate()
        .map(|(index, value)| text_cell(&format!("{}6", columns[index]), value, 4))
        .collect::<String>();
    rows.push_str(&format!(
        r#"<row r="6" ht="34" customHeight="1">{header}</row>"#
    ));
    for (index, item) in input.items.iter().enumerate() {
        let row = index + 7;
        let mut cells = number_cell(&format!("A{row}"), &(index + 1).to_string(), 5);
        cells.push_str(&text_cell(&format!("B{row}"), &item.image_url, 5));
        cells.push_str(&text_cell(&format!("C{row}"), &item.product_name, 5));
        cells.push_str(&text_cell(&format!("D{row}"), &item.sku, 5));
        for (column, value) in [
            ("E", &item.package_length_cm),
            ("F", &item.package_width_cm),
            ("G", &item.package_height_cm),
            ("H", &item.unit_weight_kg),
            ("I", &item.units_per_carton),
            ("J", &item.carton_length_cm),
            ("K", &item.carton_width_cm),
            ("L", &item.carton_height_cm),
            ("M", &item.carton_weight_kg),
            ("N", &item.carton_count),
        ] {
            cells.push_str(&number_cell(&format!("{column}{row}"), value, 6));
        }
        cells.push_str(&formula_cell(
            &format!("O{row}"),
            &format!("I{row}*N{row}"),
            7,
        ));
        cells.push_str(&formula_cell(
            &format!("P{row}"),
            &format!("M{row}*N{row}"),
            8,
        ));
        cells.push_str(&formula_cell(
            &format!("Q{row}"),
            &format!("J{row}*K{row}*L{row}/1000000*N{row}"),
            9,
        ));
        cells.push_str(&formula_cell(
            &format!("R{row}"),
            &format!("IFERROR(P{row}/Q{row},0)"),
            8,
        ));
        cells.push_str(&text_cell(&format!("S{row}"), &item.note, 5));
        rows.push_str(&format!(
            r#"<row r="{row}" ht="72" customHeight="1">{cells}</row>"#
        ));
    }
    let total = last + 1;
    let mut totals = text_cell(&format!("A{total}"), "合计", 10);
    for col in ["N", "O"] {
        totals.push_str(&formula_cell(
            &format!("{col}{total}"),
            &format!("SUM({col}7:{col}{last})"),
            10,
        ));
    }
    for col in ["P", "Q"] {
        totals.push_str(&formula_cell(
            &format!("{col}{total}"),
            &format!("SUM({col}7:{col}{last})"),
            10,
        ));
    }
    totals.push_str(&formula_cell(
        &format!("R{total}"),
        &format!("IFERROR(P{total}/Q{total},0)"),
        10,
    ));
    rows.push_str(&format!(
        r#"<row r="{total}" ht="28" customHeight="1">{totals}</row>"#
    ));
    let drawing = if has_images {
        r#"<drawing r:id="rId1"/>"#
    } else {
        ""
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheetViews><sheetView showGridLines="0" workbookViewId="0"><pane xSplit="4" ySplit="6" topLeftCell="E7" activePane="bottomRight" state="frozen"/></sheetView></sheetViews><sheetFormatPr defaultRowHeight="20"/><cols><col min="1" max="1" width="7" customWidth="1"/><col min="2" max="2" width="18" customWidth="1"/><col min="3" max="3" width="22" customWidth="1"/><col min="4" max="4" width="15" customWidth="1"/><col min="5" max="19" width="13" customWidth="1"/></cols><sheetData>{rows}</sheetData><autoFilter ref="A6:S{last}"/><mergeCells count="5"><mergeCell ref="A2:S2"/><mergeCell ref="A3:S3"/><mergeCell ref="A4:S4"/><mergeCell ref="A5:S5"/><mergeCell ref="A{total}:M{total}"/></mergeCells><pageMargins left="0.2" right="0.2" top="0.35" bottom="0.35" header="0.15" footer="0.15"/><pageSetup orientation="landscape" fitToWidth="1" fitToHeight="0" paperSize="9"/>{drawing}</worksheet>"#
    )
}

fn write_xlsx(path: &Path, input: &FreightQuoteInput) -> Result<usize, String> {
    let images = input
        .items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| image_asset(&item.image_url, index + 7))
        .collect::<Vec<_>>();
    let has_images = !images.is_empty();
    let mut zip = zip::ZipWriter::new(fs::File::create(path).map_err(|e| e.to_string())?);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let image_defaults = images
        .iter()
        .map(|image| (image.extension, image.content_type))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .map(|(extension, content)| {
            format!(r#"<Default Extension="{extension}" ContentType="{content}"/>"#)
        })
        .collect::<String>();
    let drawing_override = if has_images {
        r#"<Override PartName="/xl/drawings/drawing1.xml" ContentType="application/vnd.openxmlformats-officedocument.drawing+xml"/>"#
    } else {
        ""
    };
    let content_types = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>{image_defaults}<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>{drawing_override}</Types>"#
    );
    /* Replaced below with a raw string delimiter that safely contains the #,##0 format code.
    let styles = r#"<?xml version="1.0" encoding="UTF-8"?><styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><numFmts count="3"><numFmt numFmtId="164" formatCode="0.000"/><numFmt numFmtId="165" formatCode="0.00"/><numFmt numFmtId="166" formatCode="#,##0"/></numFmts><fonts count="3"><font><sz val="10"/><name val="Microsoft YaHei"/></font><font><b/><sz val="16"/><color rgb="FF12345B"/><name val="Microsoft YaHei"/></font><font><b/><sz val="10"/><color rgb="FFFFFFFF"/><name val="Microsoft YaHei"/></font></fonts><fills count="5"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill><fill><patternFill patternType="solid"><fgColor rgb="FF17365D"/></patternFill></fill><fill><patternFill patternType="solid"><fgColor rgb="FFFFF2CC"/></patternFill></fill><fill><patternFill patternType="solid"><fgColor rgb="FFEAF2F8"/></patternFill></fill></fills><borders count="2"><border/><border><left style="thin"><color rgb="FFD9E2F3"/></left><right style="thin"><color rgb="FFD9E2F3"/></right><top style="thin"><color rgb="FFD9E2F3"/></top><bottom style="thin"><color rgb="FFD9E2F3"/></bottom></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="11"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/><xf numFmtId="0" fontId="1" fillId="0" borderId="0"/><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/><xf numFmtId="0" fontId="0" fillId="4" borderId="0"/><xf numFmtId="0" fontId="2" fillId="2" borderId="1" applyAlignment="1"><alignment horizontal="center" vertical="center" wrapText="1"/></xf><xf numFmtId="0" fontId="0" fillId="0" borderId="1" applyAlignment="1"><alignment vertical="center" wrapText="1"/></xf><xf numFmtId="165" fontId="0" fillId="3" borderId="1" applyAlignment="1"><alignment horizontal="right" vertical="center"/></xf><xf numFmtId="166" fontId="0" fillId="4" borderId="1" applyAlignment="1"><alignment horizontal="right" vertical="center"/></xf><xf numFmtId="165" fontId="0" fillId="4" borderId="1" applyAlignment="1"><alignment horizontal="right" vertical="center"/></xf><xf numFmtId="164" fontId="0" fillId="4" borderId="1" applyAlignment="1"><alignment horizontal="right" vertical="center"/></xf><xf numFmtId="165" fontId="0" fillId="4" borderId="1" applyAlignment="1"><alignment horizontal="right" vertical="center"/></xf></cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>"#;
    */
    let styles = r##"<?xml version="1.0" encoding="UTF-8"?><styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><numFmts count="3"><numFmt numFmtId="164" formatCode="0.000"/><numFmt numFmtId="165" formatCode="0.00"/><numFmt numFmtId="166" formatCode="#,##0"/></numFmts><fonts count="3"><font><sz val="10"/><name val="Microsoft YaHei"/></font><font><b/><sz val="16"/><color rgb="FF12345B"/><name val="Microsoft YaHei"/></font><font><b/><sz val="10"/><color rgb="FFFFFFFF"/><name val="Microsoft YaHei"/></font></fonts><fills count="5"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill><fill><patternFill patternType="solid"><fgColor rgb="FF17365D"/></patternFill></fill><fill><patternFill patternType="solid"><fgColor rgb="FFFFF2CC"/></patternFill></fill><fill><patternFill patternType="solid"><fgColor rgb="FFEAF2F8"/></patternFill></fill></fills><borders count="2"><border/><border><left style="thin"><color rgb="FFD9E2F3"/></left><right style="thin"><color rgb="FFD9E2F3"/></right><top style="thin"><color rgb="FFD9E2F3"/></top><bottom style="thin"><color rgb="FFD9E2F3"/></bottom></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="11"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/><xf numFmtId="0" fontId="1" fillId="0" borderId="0"/><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/><xf numFmtId="0" fontId="0" fillId="4" borderId="0"/><xf numFmtId="0" fontId="2" fillId="2" borderId="1" applyAlignment="1"><alignment horizontal="center" vertical="center" wrapText="1"/></xf><xf numFmtId="0" fontId="0" fillId="0" borderId="1" applyAlignment="1"><alignment vertical="center" wrapText="1"/></xf><xf numFmtId="165" fontId="0" fillId="3" borderId="1" applyAlignment="1"><alignment horizontal="right" vertical="center"/></xf><xf numFmtId="166" fontId="0" fillId="4" borderId="1" applyAlignment="1"><alignment horizontal="right" vertical="center"/></xf><xf numFmtId="165" fontId="0" fillId="4" borderId="1" applyAlignment="1"><alignment horizontal="right" vertical="center"/></xf><xf numFmtId="164" fontId="0" fillId="4" borderId="1" applyAlignment="1"><alignment horizontal="right" vertical="center"/></xf><xf numFmtId="165" fontId="0" fillId="4" borderId="1" applyAlignment="1"><alignment horizontal="right" vertical="center"/></xf></cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>"##;
    let mut parts: Vec<(String, String)> = vec![
        ("[Content_Types].xml".into(), content_types),
        ("_rels/.rels".into(), r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#.into()),
        ("xl/workbook.xml".into(), r#"<?xml version="1.0" encoding="UTF-8"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="物流询价配货表" sheetId="1" r:id="rId1"/></sheets><calcPr fullCalcOnLoad="1" forceFullCalc="1" calcMode="auto"/></workbook>"#.into()),
        ("xl/_rels/workbook.xml.rels".into(), r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#.into()),
        ("xl/styles.xml".into(), styles.into()),
        ("xl/worksheets/sheet1.xml".into(), sheet_xml(input, has_images)),
    ];
    if has_images {
        parts.push(("xl/worksheets/_rels/sheet1.xml.rels".into(), r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing1.xml"/></Relationships>"#.into()));
        parts.push(("xl/drawings/drawing1.xml".into(), drawing_xml(&images)));
        parts.push((
            "xl/drawings/_rels/drawing1.xml.rels".into(),
            drawing_rels(&images),
        ));
    }
    for (name, data) in parts {
        zip.start_file(name, options).map_err(|e| e.to_string())?;
        zip.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
    }
    for (index, image) in images.iter().enumerate() {
        zip.start_file(
            format!("xl/media/image{}.{}", index + 1, image.extension),
            options,
        )
        .map_err(|e| e.to_string())?;
        zip.write_all(&image.bytes).map_err(|e| e.to_string())?;
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(images.len())
}

fn export(input: &FreightQuoteInput, state: &AppState) -> Result<Value, String> {
    validate(input)?;
    let dir = state
        .data_dir
        .parent()
        .unwrap_or(&state.data_dir)
        .join("exports")
        .join("freight-quotes");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let name = format!(
        "配货询价表-{}-{}.xlsx",
        safe_component(&input.inquiry_no, "未编号"),
        safe_component(&input.title, "未命名")
    );
    let path = dir.join(name);
    let embedded_images = write_xlsx(&path, input)?;
    let _ = open::that(&path);
    Ok(
        json!({"path": path.to_string_lossy(), "embeddedImages": embedded_images, "totalImages": input.items.iter().filter(|item| !item.image_url.trim().is_empty()).count()}),
    )
}

#[tauri::command]
pub async fn freight_quote_command(
    state: State<'_, AppState>,
    command: String,
    payload: Value,
) -> Result<Value, String> {
    let state = background_state(&state)?;
    tauri::async_runtime::spawn_blocking(move || match command.as_str() {
        "export_excel" => export(
            &serde_json::from_value(payload).map_err(|e| e.to_string())?,
            &state,
        ),
        _ => Err("不支持的物流询价命令".into()),
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> FreightQuoteInput {
        FreightQuoteInput {
            inquiry_no: "XJ-20260917-001".into(),
            title: "伪装网".into(),
            route: "中国-莫斯科".into(),
            destination: "莫斯科仓".into(),
            contact: "W".into(),
            note: "询价".into(),
            items: vec![FreightQuoteItem {
                image_url: "".into(),
                product_name: "沙漠数码伪装网".into(),
                sku: "1.5*10".into(),
                package_length_cm: "26".into(),
                package_width_cm: "17".into(),
                package_height_cm: "16".into(),
                unit_weight_kg: "0.79".into(),
                units_per_carton: "20".into(),
                carton_length_cm: "60".into(),
                carton_width_cm: "50".into(),
                carton_height_cm: "40".into(),
                carton_weight_kg: "15.8".into(),
                carton_count: "10".into(),
                note: "".into(),
            }],
        }
    }
    #[test]
    fn workbook_contains_auditable_formulas() {
        let path = std::env::temp_dir().join("freight-quote-test.xlsx");
        write_xlsx(&path, &sample()).unwrap();
        let mut archive = zip::ZipArchive::new(fs::File::open(&path).unwrap()).unwrap();
        let mut xml = String::new();
        archive
            .by_name("xl/worksheets/sheet1.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        assert!(xml.contains("I7*N7"));
        assert!(xml.contains("M7*N7"));
        assert!(xml.contains("J7*K7*L7/1000000*N7"));
        assert!(xml.contains("IFERROR(P7/Q7,0)"));
        drop(archive);
        let mut workbook = calamine::open_workbook_auto(&path).unwrap();
        use calamine::Reader;
        assert!(workbook.worksheet_range("物流询价配货表").is_ok());
        let _ = fs::remove_file(path);
    }
    #[test]
    fn embeds_local_product_image() {
        use base64::Engine;
        let image_path = std::env::temp_dir().join("freight-quote-pixel.png");
        let bytes=base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=").unwrap();
        fs::write(&image_path, bytes).unwrap();
        let mut value = sample();
        value.items[0].image_url = image_path.to_string_lossy().into_owned();
        let path = std::env::temp_dir().join("freight-quote-image-test.xlsx");
        assert_eq!(write_xlsx(&path, &value).unwrap(), 1);
        let mut archive = zip::ZipArchive::new(fs::File::open(&path).unwrap()).unwrap();
        assert!(archive.by_name("xl/media/image1.png").is_ok());
        assert!(archive.by_name("xl/drawings/drawing1.xml").is_ok());
        drop(archive);
        let _ = fs::remove_file(path);
        let _ = fs::remove_file(image_path);
    }
    #[test]
    fn rejects_missing_dimensions() {
        let mut value = sample();
        value.items[0].carton_height_cm.clear();
        assert!(validate(&value).unwrap_err().contains("箱规高"));
    }
}
