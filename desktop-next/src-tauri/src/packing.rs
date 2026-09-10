use serde::{Deserialize, Serialize};
use std::{fs, io::Write};
use tauri::State;
use zip::{write::SimpleFileOptions, ZipWriter};

use super::AppState;
use super::{active_shop_database_path, db, feishu_base, feishu_token, setting};

#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PackingItemInput {
    sku: String,
    product_name: String,
    barcode_label: String,
    battery: String,
    cartons: i64,
    quantity_per_carton: i64,
    weight_kg: f64,
    length_cm: f64,
    width_cm: f64,
    height_cm: f64,
    remark: String,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PackingExportInput {
    batch_code: String,
    title: String,
    platform: String,
    shop_name: String,
    shipping_date: String,
    notice: String,
    items: Vec<PackingItemInput>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackingExportResult {
    xlsx_path: String,
    pdf_path: String,
    carton_count: usize,
    total_quantity: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackingDraft {
    id: i64,
    name: String,
    payload: PackingExportInput,
    updated_at: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeishuPackingResult {
    xlsx_file_token: String,
    pdf_file_token: String,
}

fn ensure_drafts(c: &rusqlite::Connection) -> Result<(), String> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS packing_drafts(id INTEGER PRIMARY KEY AUTOINCREMENT,name TEXT NOT NULL,payload_json TEXT NOT NULL,created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);")
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn packing_drafts(state: State<'_, AppState>) -> Result<Vec<PackingDraft>, String> {
    let c = db(&state)?;
    ensure_drafts(&c)?;
    let mut stmt = c.prepare("SELECT id,name,payload_json,updated_at FROM packing_drafts ORDER BY updated_at DESC,id DESC").map_err(|e| e.to_string())?;
    let drafts = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .map(|row| {
            let (id, name, json, updated_at) = row.map_err(|e| e.to_string())?;
            let payload = serde_json::from_str(&json).map_err(|e| e.to_string())?;
            Ok(PackingDraft {
                id,
                name,
                payload,
                updated_at,
            })
        })
        .collect();
    drafts
}

#[tauri::command]
pub fn save_packing_draft(
    id: Option<i64>,
    name: String,
    payload: PackingExportInput,
    state: State<'_, AppState>,
) -> Result<i64, String> {
    let c = db(&state)?;
    ensure_drafts(&c)?;
    let name = if name.trim().is_empty() {
        payload.batch_code.trim()
    } else {
        name.trim()
    };
    let json = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
    if let Some(id) = id {
        let changed = c.execute("UPDATE packing_drafts SET name=?1,payload_json=?2,updated_at=CURRENT_TIMESTAMP WHERE id=?3", rusqlite::params![name,json,id]).map_err(|e|e.to_string())?;
        if changed == 0 {
            return Err("草稿不存在或已被删除".into());
        }
        Ok(id)
    } else {
        c.execute(
            "INSERT INTO packing_drafts(name,payload_json)VALUES(?1,?2)",
            rusqlite::params![name, json],
        )
        .map_err(|e| e.to_string())?;
        Ok(c.last_insert_rowid())
    }
}

#[tauri::command]
pub fn delete_packing_draft(id: i64, state: State<'_, AppState>) -> Result<(), String> {
    let c = db(&state)?;
    ensure_drafts(&c)?;
    c.execute("DELETE FROM packing_drafts WHERE id=?1", [id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

struct CartonRow<'a> {
    number: usize,
    item_index: usize,
    item: &'a PackingItemInput,
}

fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn cell_text(reference: &str, value: &str, style: usize) -> String {
    format!(
        r#"<c r="{reference}" t="inlineStr" s="{style}"><is><t xml:space="preserve">{}</t></is></c>"#,
        xml(value)
    )
}

fn cell_num(reference: &str, value: impl std::fmt::Display, style: usize) -> String {
    format!(r#"<c r="{reference}" s="{style}"><v>{value}</v></c>"#)
}

fn build_xlsx(
    path: &std::path::Path,
    input: &PackingExportInput,
    cartons: &[CartonRow<'_>],
) -> Result<(), String> {
    let file = fs::File::create(path).map_err(|e| e.to_string())?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let parts = [
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
        ),
        (
            "xl/workbook.xml",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="发货装箱单" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
        ),
        (
            "xl/_rels/workbook.xml.rels",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#,
        ),
        (
            "xl/styles.xml",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><numFmts count="0"/><fonts count="3"><font><sz val="11"/><name val="Microsoft YaHei"/></font><font><b/><sz val="18"/><name val="Microsoft YaHei"/></font><font><b/><sz val="10"/><name val="Microsoft YaHei"/></font></fonts><fills count="3"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill><fill><patternFill patternType="solid"><fgColor rgb="FFD9EAF7"/><bgColor indexed="64"/></patternFill></fill></fills><borders count="2"><border><left/><right/><top/><bottom/><diagonal/></border><border><left style="thin"/><right style="thin"/><top style="thin"/><bottom style="thin"/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="5"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="0" fontId="1" fillId="0" borderId="0" xfId="0" applyAlignment="1"><alignment horizontal="center" vertical="center"/></xf><xf numFmtId="0" fontId="2" fillId="2" borderId="1" xfId="0" applyAlignment="1"><alignment horizontal="center" vertical="center" wrapText="1"/></xf><xf numFmtId="0" fontId="0" fillId="0" borderId="1" xfId="0" applyAlignment="1"><alignment horizontal="center" vertical="center" wrapText="1"/></xf><xf numFmtId="0" fontId="0" fillId="0" borderId="1" xfId="0" applyAlignment="1"><alignment vertical="center" wrapText="1"/></xf></cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>"#,
        ),
    ];
    for (name, content) in parts {
        zip.start_file(name, options).map_err(|e| e.to_string())?;
        zip.write_all(content.as_bytes())
            .map_err(|e| e.to_string())?;
    }
    let headers = [
        "超光速外箱唛",
        "箱号",
        "重KG",
        "长CM",
        "宽CM",
        "高CM",
        "体积立方米",
        "条码标签",
        "是否带电",
        "产品序号",
        "单箱数量",
        "其他备注",
    ];
    let mut rows = String::new();
    rows.push_str(&format!(
        r#"<row r="1" ht="28" customHeight="1">{}</row>"#,
        cell_text("A1", &input.title, 1)
    ));
    rows.push_str(&format!(
        r#"<row r="2">{}{}{}</row>"#,
        cell_text("A2", &format!("平台：{}", input.platform), 0),
        cell_text("E2", &format!("店名：{}", input.shop_name), 0),
        cell_text("I2", &format!("出货日期：{}", input.shipping_date), 0)
    ));
    rows.push_str(&format!(
        r#"<row r="3">{}</row>"#,
        cell_text("A3", &format!("注意事项：{}", input.notice), 0)
    ));
    let mut header_cells = String::new();
    for (index, header) in headers.iter().enumerate() {
        header_cells.push_str(&cell_text(
            &format!("{}4", (b'A' + index as u8) as char),
            header,
            2,
        ));
    }
    rows.push_str(&format!(
        r#"<row r="4" ht="34" customHeight="1">{header_cells}</row>"#
    ));
    for (offset, carton) in cartons.iter().enumerate() {
        let r = offset + 5;
        let item = carton.item;
        let mark = format!("{}-{}", input.batch_code, carton.number);
        let values = [
            cell_text(&format!("A{r}"), &mark, 4),
            cell_num(&format!("B{r}"), carton.number, 3),
            cell_num(&format!("C{r}"), item.weight_kg, 3),
            cell_num(&format!("D{r}"), item.length_cm, 3),
            cell_num(&format!("E{r}"), item.width_cm, 3),
            cell_num(&format!("F{r}"), item.height_cm, 3),
            format!(r#"<c r="G{r}" s="3"><f>D{r}*E{r}*F{r}/1000000</f></c>"#),
            cell_text(&format!("H{r}"), &item.barcode_label, 3),
            cell_text(&format!("I{r}"), &item.battery, 3),
            cell_num(&format!("J{r}"), carton.item_index, 3),
            cell_num(&format!("K{r}"), item.quantity_per_carton, 3),
            cell_text(
                &format!("L{r}"),
                &format!(
                    "{}{}{}",
                    item.product_name,
                    if item.sku.is_empty() { "" } else { " · " },
                    if item.remark.is_empty() {
                        &item.sku
                    } else {
                        &item.remark
                    }
                ),
                4,
            ),
        ];
        rows.push_str(&format!(
            r#"<row r="{r}" ht="24" customHeight="1">{}</row>"#,
            values.join("")
        ));
    }
    let last = cartons.len() + 4;
    let sheet = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetPr><pageSetUpPr fitToPage="1"/></sheetPr><dimension ref="A1:L{last}"/><sheetViews><sheetView workbookViewId="0"><pane ySplit="4" topLeftCell="A5" activePane="bottomLeft" state="frozen"/><selection pane="bottomLeft" activeCell="A5" sqref="A5"/></sheetView></sheetViews><sheetFormatPr defaultRowHeight="15"/><cols><col min="1" max="1" width="25" customWidth="1"/><col min="2" max="2" width="8" customWidth="1"/><col min="3" max="7" width="11" customWidth="1"/><col min="8" max="8" width="20" customWidth="1"/><col min="9" max="11" width="12" customWidth="1"/><col min="12" max="12" width="28" customWidth="1"/></cols><sheetData>{rows}</sheetData><autoFilter ref="A4:L{last}"/><mergeCells count="4"><mergeCell ref="A1:L1"/><mergeCell ref="A2:D2"/><mergeCell ref="E2:H2"/><mergeCell ref="I2:L2"/></mergeCells><pageMargins left="0.25" right="0.25" top="0.5" bottom="0.5" header="0.2" footer="0.2"/><pageSetup orientation="landscape" fitToWidth="1" fitToHeight="0"/></worksheet>"#
    );
    zip.start_file("xl/worksheets/sheet1.xml", options)
        .map_err(|e| e.to_string())?;
    zip.write_all(sheet.as_bytes()).map_err(|e| e.to_string())?;
    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

fn pdf_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
}

fn build_pdf(path: &std::path::Path, batch_code: &str, carton_count: usize) -> Result<(), String> {
    let (top, middle) = batch_code
        .rsplit_once('-')
        .map(|(a, b)| (format!("{a}-"), b.to_string()))
        .unwrap_or_else(|| (batch_code.to_string(), String::new()));
    let pages_id = 2usize;
    let font_id = 3usize;
    let mut objects: Vec<String> = vec![String::new(), String::new(), String::new()];
    let mut kids = Vec::new();
    for number in 1..=carton_count {
        let page_id = objects.len() + 1;
        let content_id = page_id + 1;
        kids.push(format!("{page_id} 0 R"));
        let lines: Vec<String> = if middle.is_empty() {
            vec![top.clone(), format!("{number:02}")]
        } else {
            vec![top.clone(), middle.clone(), format!("{number:02}")]
        };
        let ys = if lines.len() == 3 {
            vec![235.0, 165.0, 95.0]
        } else {
            vec![200.0, 115.0]
        };
        let mut stream = String::new();
        for (line, y) in lines.iter().zip(ys) {
            let size = if line.len() > 18 { 36.0 } else { 44.0 };
            let approx = line.len() as f64 * size * 0.55;
            let x = ((504.0 - approx) / 2.0).max(18.0);
            stream.push_str(&format!(
                "BT /F1 {size} Tf {x:.1} {y:.1} Td ({}) Tj ET\n",
                pdf_escape(line)
            ));
        }
        objects.push(format!("<< /Type /Page /Parent {pages_id} 0 R /MediaBox [0 0 504 360] /Resources << /Font << /F1 {font_id} 0 R >> >> /Contents {content_id} 0 R >>"));
        objects.push(format!(
            "<< /Length {} >>\nstream\n{}endstream",
            stream.as_bytes().len(),
            stream
        ));
    }
    objects[0] = format!("<< /Type /Catalog /Pages {pages_id} 0 R >>");
    objects[1] = format!(
        "<< /Type /Pages /Kids [{}] /Count {} >>",
        kids.join(" "),
        carton_count
    );
    objects[2] = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into();
    let mut out = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = vec![0usize];
    for (index, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", index + 1, object).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets.iter().skip(1) {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    fs::write(path, out).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn export_packing_documents(
    input: PackingExportInput,
    state: State<'_, AppState>,
) -> Result<PackingExportResult, String> {
    let batch = input.batch_code.trim();
    if batch.is_empty() || !batch.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("批次号只能包含英文字母、数字和连字符".into());
    }
    if input.items.is_empty() {
        return Err("请至少添加一个补货产品".into());
    }
    let mut cartons = Vec::new();
    let mut total_quantity = 0i64;
    for (index, item) in input.items.iter().enumerate() {
        if item.cartons <= 0 || item.quantity_per_carton <= 0 {
            return Err(format!("第 {} 个产品的箱数和单箱数量必须大于 0", index + 1));
        }
        if item.weight_kg <= 0.0
            || item.length_cm <= 0.0
            || item.width_cm <= 0.0
            || item.height_cm <= 0.0
        {
            return Err(format!("第 {} 个产品的重量和尺寸必须大于 0", index + 1));
        }
        total_quantity += item.cartons * item.quantity_per_carton;
        for _ in 0..item.cartons {
            cartons.push(CartonRow {
                number: cartons.len() + 1,
                item_index: index + 1,
                item,
            });
        }
    }
    if cartons.len() > 999 {
        return Err("单次最多生成 999 个箱唛".into());
    }
    let folder = state
        .data_dir
        .parent()
        .unwrap_or(&state.data_dir)
        .join("exports")
        .join(format!(
            "装箱单_{}_{}",
            batch,
            chrono::Local::now().format("%Y%m%d_%H%M%S")
        ));
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let xlsx_path = folder.join(format!("发货装箱单_{batch}-{}.xlsx", cartons.len()));
    let pdf_path = folder.join(format!("箱唛_{batch}-{}.pdf", cartons.len()));
    build_xlsx(&xlsx_path, &input, &cartons)?;
    build_pdf(&pdf_path, batch, cartons.len())?;
    let _ = open::that(&folder);
    Ok(PackingExportResult {
        xlsx_path: xlsx_path.to_string_lossy().into_owned(),
        pdf_path: pdf_path.to_string_lossy().into_owned(),
        carton_count: cartons.len(),
        total_quantity,
    })
}

fn folder_token(value: &str) -> String {
    let trimmed = value.trim().trim_end_matches('/');
    trimmed
        .rsplit("/folder/")
        .next()
        .unwrap_or(trimmed)
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .to_string()
}

fn upload_file(
    base: &str,
    token: &str,
    folder: &str,
    path: &std::path::Path,
) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| format!("读取待上传文件失败：{e}"))?;
    let name = path
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or("上传文件名无效")?;
    let boundary = format!(
        "----OzonPacking{}",
        chrono::Local::now()
            .timestamp_nanos_opt()
            .unwrap_or_default()
    );
    let mut body = Vec::new();
    let mut field = |key: &str, value: &str| {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{key}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    };
    field("file_name", name);
    field("parent_type", "explorer");
    field("parent_node", folder);
    field("size", &bytes.len().to_string());
    // Keep the multipart header ASCII-only. The real Unicode name is supplied in
    // the file_name field; raw non-ASCII bytes in filename are rejected by some
    // Feishu gateways with an otherwise opaque HTTP 400.
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"upload.bin\"\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes());
    body.extend_from_slice(&bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let response = ureq::post(&format!("{base}/drive/v1/files/upload_all"))
        .set("Authorization", &format!("Bearer {token}"))
        .set(
            "Content-Type",
            &format!("multipart/form-data; boundary={boundary}"),
        )
        .send_bytes(&body);
    let response = match response {
        Ok(value) => value,
        Err(ureq::Error::Status(status, response)) => {
            let request_id = response
                .header("X-Tt-Logid")
                .or_else(|| response.header("X-Request-Id"))
                .unwrap_or("")
                .to_string();
            let raw = response.into_string().unwrap_or_default();
            let detail = serde_json::from_str::<serde_json::Value>(&raw)
                .ok()
                .map(|v| {
                    let code = v
                        .get("code")
                        .and_then(|x| x.as_i64())
                        .map(|x| x.to_string())
                        .unwrap_or_default();
                    let msg = v.get("msg").and_then(|x| x.as_str()).unwrap_or(&raw);
                    format!("飞书错误码 {code}：{msg}")
                })
                .unwrap_or_else(|| {
                    if raw.trim().is_empty() {
                        "响应正文为空".into()
                    } else {
                        raw
                    }
                });
            return Err(format!(
                "上传 {name} 失败（HTTP {status}）：{detail}{}",
                if request_id.is_empty() {
                    String::new()
                } else {
                    format!("；请求 ID {request_id}")
                }
            ));
        }
        Err(error) => return Err(format!("上传 {name} 到飞书失败：{error}")),
    };
    let payload: serde_json::Value = serde_json::from_reader(response.into_reader())
        .map_err(|e| format!("飞书上传响应无法解析：{e}"))?;
    let code = payload.get("code").and_then(|v| v.as_i64()).unwrap_or(0);
    if code != 0 {
        return Err(format!(
            "飞书上传错误 {code}：{}",
            payload
                .get("msg")
                .and_then(|v| v.as_str())
                .unwrap_or("未知错误")
        ));
    }
    payload
        .pointer("/data/file_token")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| "飞书上传成功响应缺少 file_token".into())
}

#[tauri::command]
pub fn upload_packing_documents_to_feishu(
    xlsx_path: String,
    pdf_path: String,
    state: State<'_, AppState>,
) -> Result<FeishuPackingResult, String> {
    let mut candidates = vec![active_shop_database_path(&state)?];
    let default_db = state.data_dir.join("ozon_next_default.db");
    if !candidates.contains(&default_db) {
        candidates.push(default_db);
    }
    if let Ok(entries) = fs::read_dir(state.data_dir.join("shops")) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|v| v.to_str()) == Some("db")
                && !candidates.contains(&path)
            {
                candidates.push(path);
            }
        }
    }
    let mut upload_config = None;
    for path in candidates {
        if !path.is_file() {
            continue;
        }
        let Ok(c) = rusqlite::Connection::open(path) else {
            continue;
        };
        let folder = folder_token(&setting(&c, "feishu_packing_folder_token"));
        if folder.is_empty() {
            continue;
        }
        let token = feishu_token(&c)?;
        upload_config = Some((feishu_base(&c), token, folder));
        break;
    }
    let Some((base, token, folder)) = upload_config else {
        return Err(
            "未找到飞书上传配置。请在连接设置中填写“发货装箱单”文件夹链接或 Token，并保存设置"
                .into(),
        );
    };
    let xlsx = std::path::Path::new(&xlsx_path);
    let pdf = std::path::Path::new(&pdf_path);
    if !xlsx.is_file() || !pdf.is_file() {
        return Err("本地装箱单或箱唛文件不存在，请重新生成".into());
    }
    let xlsx_file_token = upload_file(&base, &token, &folder, xlsx)?;
    let pdf_file_token = upload_file(&base, &token, &folder, pdf)?;
    Ok(FeishuPackingResult {
        xlsx_file_token,
        pdf_file_token,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn escapes_xml_and_pdf_text() {
        assert_eq!(xml("A&B<1"), "A&amp;B&lt;1");
        assert_eq!(pdf_escape("A(B)"), "A\\(B\\)");
    }

    #[test]
    fn extracts_feishu_folder_token() {
        assert_eq!(
            folder_token("https://example.feishu.cn/drive/folder/AbCd123?from=space"),
            "AbCd123"
        );
        assert_eq!(folder_token("AbCd123"), "AbCd123");
    }

    #[test]
    fn creates_readable_workbook_and_multipage_pdf() {
        let root = std::env::temp_dir().join(format!("ozon-packing-test-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let item = PackingItemInput {
            sku: "SKU-1".into(),
            product_name: "测试商品".into(),
            barcode_label: "BAR-1".into(),
            battery: "否".into(),
            cartons: 2,
            quantity_per_carton: 10,
            weight_kg: 5.0,
            length_cm: 40.0,
            width_cm: 30.0,
            height_cm: 20.0,
            remark: "白色".into(),
        };
        let input = PackingExportInput {
            batch_code: "CZ7046-OZON-0909".into(),
            title: "发货装箱单".into(),
            platform: "Ozon".into(),
            shop_name: "测试店铺".into(),
            shipping_date: "2026-09-10".into(),
            notice: String::new(),
            items: vec![item],
        };
        let cartons = vec![
            CartonRow {
                number: 1,
                item_index: 1,
                item: &input.items[0],
            },
            CartonRow {
                number: 2,
                item_index: 1,
                item: &input.items[0],
            },
        ];
        let xlsx = root.join("test.xlsx");
        let pdf = root.join("test.pdf");
        build_xlsx(&xlsx, &input, &cartons).unwrap();
        build_pdf(&pdf, &input.batch_code, cartons.len()).unwrap();
        let mut archive = zip::ZipArchive::new(fs::File::open(&xlsx).unwrap()).unwrap();
        assert!(archive.by_name("xl/worksheets/sheet1.xml").is_ok());
        let bytes = fs::read(&pdf).unwrap();
        assert!(bytes.starts_with(b"%PDF-1.4"));
        assert!(String::from_utf8_lossy(&bytes).contains("/Count 2"));
        if std::env::var_os("KEEP_PACKING_TEST_OUTPUT").is_none() {
            fs::remove_dir_all(root).unwrap();
        } else {
            eprintln!("packing test output: {}", root.display());
        }
    }
}
