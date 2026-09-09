use super::*;
use base64::Engine;
use calamine::{Reader, Xlsx};
use std::io::Cursor;

fn csv(text: &str) -> Result<Vec<Vec<String>>> {
    let mut rows = vec![];
    let mut row = vec![];
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = text.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                if quoted && chars.peek() == Some(&'"') {
                    cell.push('"');
                    chars.next();
                } else {
                    quoted = !quoted
                }
            }
            ',' if !quoted => {
                row.push(std::mem::take(&mut cell));
            }
            '\n' if !quoted => {
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
            }
            '\r' if !quoted => {}
            _ => cell.push(ch),
        }
    }
    if quoted {
        return Err("CSV 引号未闭合".into());
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row)
    }
    Ok(rows)
}
fn parse(p: &Value) -> Result<Vec<Value>> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(txt(p, "base64"))
        .map_err(err)?;
    if bytes.len() > 10 * 1024 * 1024 {
        return Err("导入文件不能超过 10 MB".into());
    }
    let cells = if txt(p, "filename").to_lowercase().ends_with(".xlsx") {
        let mut archive = zip::ZipArchive::new(Cursor::new(&bytes)).map_err(err)?;
        let mut expanded = 0u64;
        for i in 0..archive.len() {
            expanded = expanded.saturating_add(archive.by_index(i).map_err(err)?.size());
            if expanded > 64 * 1024 * 1024 {
                return Err("工作簿解压后超过 64 MB，请拆分文件".into());
            }
        }
        let mut book: Xlsx<_> = Xlsx::new(Cursor::new(bytes)).map_err(err)?;
        let sheet = book
            .worksheet_range_at(0)
            .ok_or("工作簿没有工作表")?
            .map_err(err)?;
        if sheet.height() > 5001 || sheet.width() > 100 {
            return Err("工作表最多 5000 条数据、100 列".into());
        }
        sheet
            .rows()
            .map(|r| r.iter().map(ToString::to_string).collect::<Vec<_>>())
            .collect::<Vec<_>>()
    } else if txt(p, "filename").to_lowercase().ends_with(".csv") {
        csv(&String::from_utf8(bytes).map_err(|_| "CSV 请保存为 UTF-8")?)?
    } else {
        return Err("仅支持 CSV / XLSX".into());
    };
    if cells.len() > 5001 {
        return Err("每次最多导入 5000 行".into());
    }
    let header = cells.first().ok_or("导入文件为空")?;
    let mut unique = BTreeSet::new();
    for h in header {
        if h.trim().is_empty() || !unique.insert(h.trim()) {
            return Err("表头为空或重复".into());
        }
    }
    let mut result = vec![];
    for (i, r) in cells.iter().enumerate().skip(1) {
        if r.iter().all(|s| s.trim().is_empty()) {
            continue;
        }
        if r.len() > header.len() {
            return Err(format!("第 {} 行列数超过表头", i + 1));
        }
        let mut item = serde_json::Map::new();
        for (j, h) in header.iter().enumerate() {
            item.insert(
                h.trim().into(),
                json!(r.get(j).map(|x| x.trim()).unwrap_or("")),
            );
        }
        item.insert("_row".into(), json!(i + 1));
        result.push(Value::Object(item));
    }
    Ok(result)
}
fn product(c: &Connection, r: &Value) -> Result<Value> {
    let mut p = json!({});
    for (from, to) in [
        ("internal_sku_code", "code"),
        ("name_cn", "name"),
        ("name_ru", "nameRu"),
        ("brand", "brand"),
        ("category", "category"),
        ("color", "color"),
        ("size", "size"),
        ("width_cm", "widthCm"),
        ("length_cm", "lengthCm"),
        ("height_cm", "heightCm"),
        ("weight_kg", "weightKg"),
        ("default_purchase_cost", "purchaseCost"),
        ("purchase_currency", "purchaseCurrency"),
        ("supplier_code", "supplier"),
    ] {
        p[to] = json!(txt(r, from));
    }
    let spu = txt(r, "spu_code");
    if !spu.is_empty() {
        let id: Option<String> = c
            .query_row(
                "SELECT id FROM pm_spus WHERE code=?1 AND organization_id='local'",
                [spu],
                |r| r.get(0),
            )
            .optional()
            .map_err(err)?;
        p["spuId"] = json!(id.ok_or("SPU 不存在，请先创建")?)
    }
    Ok(p)
}
fn mapping(c: &Connection, r: &Value) -> Result<(String, String)> {
    let shop = txt(r, "shop");
    let shops = records(
        c,
        "SELECT json_quote(id) FROM wb_shops WHERE organization_id='local' AND (id=?1 OR name=?1)",
        [shop],
    )?;
    if shops.len() != 1 {
        return Err("Not Found: 店铺不存在或不唯一".into());
    }
    let shop = shops[0].as_str().unwrap_or("");
    let nm = txt(r, "nmID");
    let ch = txt(r, "chrtID");
    let vendor = txt(r, "vendorCode");
    let bar = txt(r, "barcode");
    if nm.is_empty() && ch.is_empty() && vendor.is_empty() && bar.is_empty() {
        return Err("Invalid: 至少提供一个外部标识".into());
    }
    let listings=records(c,"SELECT json_quote(l.id) FROM pm_listings l WHERE l.shop_id=?1 AND (?2='' OR l.nm_id=?2) AND (?3='' OR l.chrt_id=?3) AND (?4='' OR l.vendor_code=?4) AND (?5='' OR EXISTS(SELECT 1 FROM pm_barcodes b WHERE b.listing_id=l.id AND b.barcode=?5))",params![shop,nm,ch,vendor,bar])?;
    if listings.is_empty() {
        return Err("Not Found: 找不到 Listing".into());
    }
    if listings.len() != 1 {
        return Err("Conflict: 标识对应多个尺寸，请填写 chrtID".into());
    }
    let sku:Option<String>=c.query_row("SELECT id FROM pm_skus WHERE code=?1 AND organization_id='local' AND status!='archived'",[txt(r,"internalSkuCode")],|r|r.get(0)).optional().map_err(err)?;
    Ok((
        listings[0].as_str().unwrap_or("").into(),
        sku.ok_or("Not Found: 内部 SKU 不存在")?,
    ))
}
fn apply_row(c: &Connection, kind: &str, r: &Value) -> Result<()> {
    if kind == "mapping" {
        let (l, s) = mapping(c, r)?;
        if !super::extra::conflicts(c, &l, &s)?.is_empty() {
            return Err("Conflict: 标识冲突需人工解决".into());
        }
        bind(c, &l, &s, "")?;
    } else {
        create_sku(c, &product(c, r)?)?;
    }
    Ok(())
}
pub(super) fn handle(c: &Connection, action: &str, p: &Value) -> Result<Value> {
    if action == "import_preview" {
        let kind = txt(p, "kind");
        if !["products", "mapping"].contains(&kind.as_str()) {
            return Err("导入类型无效".into());
        }
        let rows = parse(p)?;
        if rows.is_empty() {
            return Err("没有数据行".into());
        }
        let tx = c.unchecked_transaction().map_err(err)?;
        let mut report = vec![];
        for r in &rows {
            tx.execute_batch("SAVEPOINT import_row").map_err(err)?;
            let result = apply_row(&tx, &kind, r);
            let (status, message) = match result {
                Ok(()) => ("Matched", "".to_string()),
                Err(e) => {
                    tx.execute_batch("ROLLBACK TO import_row").map_err(err)?;
                    (
                        if e.starts_with("Conflict") {
                            "Conflict"
                        } else if e.starts_with("Not Found") {
                            "Not Found"
                        } else {
                            "Invalid"
                        },
                        e,
                    )
                }
            };
            tx.execute_batch("RELEASE import_row").map_err(err)?;
            report.push(json!({"row":r["_row"],"status":status,"message":message,"data":r}));
        }
        tx.rollback().map_err(err)?;
        let id = uid(c)?;
        c.execute(
            "INSERT INTO pm_imports(id,kind,rows_json)VALUES(?1,?2,?3)",
            params![id, kind, serde_json::to_string(&rows).map_err(err)?],
        )
        .map_err(err)?;
        Ok(
            json!({"id":id,"rows":report,"valid":report.iter().filter(|r|r["status"]=="Matched").count(),"total":report.len()}),
        )
    } else {
        let tx = c.unchecked_transaction().map_err(err)?;
        let id = txt(p, "id");
        let (kind, rows, status): (String, String, String) = tx
            .query_row(
                "SELECT kind,rows_json,status FROM pm_imports WHERE id=?1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(err)?;
        if status != "preview" {
            return Err("该导入已经提交，请勿重复提交".into());
        }
        let rows: Vec<Value> = serde_json::from_str(&rows).map_err(err)?;
        for r in &rows {
            apply_row(&tx, &kind, r)
                .map_err(|e| format!("第 {} 行：{e}；本批未写入", r["_row"]))?;
        }
        tx.execute(
            "UPDATE pm_imports SET status='committed' WHERE id=?1",
            [&id],
        )
        .map_err(err)?;
        event(
            &tx,
            &id,
            if kind == "mapping" {
                "IMPORT_MAPPING"
            } else {
                "IMPORT_PRODUCTS"
            },
            Value::Null,
            json!({"count":rows.len()}),
            "用户确认预览后导入",
        )?;
        tx.commit().map_err(err)?;
        Ok(json!({"count":rows.len()}))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn csv_quotes_and_newlines() {
        assert_eq!(
            csv("a,b\n\"x,y\",\"z\"\"q\"\n").unwrap()[1],
            vec!["x,y", "z\"q"]
        );
        assert!(csv("a\n\"oops").is_err());
    }
    #[test]
    fn xlsx_uses_first_sheet_and_preserves_text_identifiers() {
        use std::io::Write;
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let files = [
            (
                "[Content_Types].xml",
                r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#,
            ),
            (
                "xl/workbook.xml",
                r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Products" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
            ),
            (
                "xl/_rels/workbook.xml.rels",
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
            ),
            (
                "xl/worksheets/sheet1.xml",
                r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>internal_sku_code</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>0001234567890123456789</t></is></c></row></sheetData></worksheet>"#,
            ),
        ];
        for (name, text) in files {
            archive
                .start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            archive.write_all(text.as_bytes()).unwrap();
        }
        let bytes = archive.finish().unwrap().into_inner();
        let rows=parse(&json!({"filename":"test.xlsx","base64":base64::engine::general_purpose::STANDARD.encode(bytes)})).unwrap();
        assert_eq!(rows[0]["internal_sku_code"], "0001234567890123456789");
    }
}
