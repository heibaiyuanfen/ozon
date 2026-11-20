use calamine::{Data, Reader, Xlsx};
use lopdf::{dictionary, Document, Object};
use regex::Regex;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Cursor,
    path::Path,
};

pub fn read_workbook(bytes: Vec<u8>) -> Result<(i64, BTreeMap<String, String>), String> {
    let mut book = Xlsx::new(Cursor::new(bytes)).map_err(|e| format!("无法读取 Excel：{e}"))?;
    let sheet = book
        .worksheet_range("箱唛明细")
        .map_err(|e| format!("缺少箱唛明细工作表：{e}"))?;
    let order_pattern = Regex::new(r"(\d+)\s*$").unwrap();
    let order_id = sheet
        .rows()
        .next()
        .and_then(|row| row.first())
        .and_then(|cell| {
            order_pattern
                .captures(&cell.to_string())
                .and_then(|m| m[1].parse::<i64>().ok())
        })
        .filter(|id| *id > 0)
        .ok_or("表格首行缺少供应单 ID")?;
    let mut columns = None;
    let mut labels = BTreeMap::new();
    for (index, row) in sheet.rows().enumerate() {
        if columns.is_none() {
            columns = row
                .iter()
                .position(|cell| cell.to_string().trim() == "物流箱号")
                .zip(
                    row.iter()
                        .position(|cell| cell.to_string().trim() == "箱唛"),
                );
            continue;
        }
        let (number_col, cargo_col) = columns.unwrap();
        let number = row
            .get(number_col)
            .map(ToString::to_string)
            .unwrap_or_default();
        let cargo = match row.get(cargo_col) {
            Some(Data::Float(_)) => {
                return Err(format!(
                    "第 {} 行箱唛为数字格式，请使用原始文本箱唛以免 Excel 丢失精度",
                    index + 1
                ))
            }
            Some(cell) => cell.to_string(),
            None => String::new(),
        };
        let (cargo, number) = (cargo.trim(), number.trim());
        if cargo.is_empty() && number.is_empty() {
            continue;
        }
        let suffix = number.rsplit('-').next().unwrap_or_default();
        if cargo.len() != 16
            || !cargo.bytes().all(|b| b.is_ascii_digit())
            || suffix.is_empty()
            || suffix.len() > 6
            || !suffix.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(format!("第 {} 行的箱唛或物流箱号无效", index + 1));
        }
        if labels.insert(cargo.to_owned(), suffix.to_owned()).is_some() {
            return Err(format!("Excel 中箱唛 {cargo} 重复"));
        }
    }
    if labels.is_empty() {
        return Err("Excel 中没有箱唛和物流箱号明细".into());
    }
    Ok((order_id, labels))
}

pub fn stamp_directory(
    source: &Path,
    output: &Path,
    numbers: &BTreeMap<String, String>,
) -> Result<usize, String> {
    let cargo_pattern = Regex::new(r"\b([0-9]{6})\s*([0-9]{7})\s*([0-9]{3})\b").unwrap();
    let mut seen = BTreeSet::new();
    let mut files = Vec::new();
    for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with("Ozon箱唛-供应-") || !name.ends_with(".pdf") {
            continue;
        }
        let mut doc = Document::load(&path).map_err(|e| format!("读取 {name} 失败：{e}"))?;
        let mut omit = Vec::new();
        let mut count = 0;
        for (page, id) in doc.get_pages() {
            let text = doc
                .extract_text(&[page])
                .map_err(|e| format!("提取 {name} 第 {page} 页箱唛失败：{e}"))?;
            let matches: BTreeSet<_> = cargo_pattern
                .captures_iter(&text)
                .map(|m| format!("{}{}{}", &m[1], &m[2], &m[3]))
                .filter(|cargo| numbers.contains_key(cargo))
                .collect();
            if matches.is_empty() {
                omit.push(page);
                continue;
            }
            if matches.len() != 1 {
                return Err(format!("{name} 第 {page} 页匹配到多个箱唛，无法确定箱号"));
            }
            let cargo = matches.first().unwrap();
            if !seen.insert(cargo.clone()) {
                return Err(format!("原始标签中箱唛 {cargo} 重复"));
            }
            stamp_page(&mut doc, id, &numbers[cargo])
                .map_err(|e| format!("{name} 第 {page} 页：{e}"))?;
            count += 1;
        }
        if count > 0 {
            doc.delete_pages(&omit);
            doc.prune_objects();
            files.push((name.to_owned(), doc));
        }
    }
    if seen.len() != numbers.len() {
        let missing = numbers
            .keys()
            .filter(|k| !seen.contains(*k))
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join("、");
        return Err(format!(
            "只匹配到 {}/{} 个箱唛，缺少标签：{missing}",
            seen.len(),
            numbers.len()
        ));
    }
    fs::create_dir_all(output).map_err(|e| e.to_string())?;
    for (name, mut doc) in files {
        doc.save(output.join(name))
            .map_err(|e| format!("保存标签失败：{e}"))?;
    }
    Ok(seen.len())
}

fn stamp_page(doc: &mut Document, id: lopdf::ObjectId, number: &str) -> Result<(), String> {
    let page = doc.get_dictionary(id).map_err(|e| e.to_string())?;
    let bounds = page
        .get(b"MediaBox")
        .and_then(Object::as_array)
        .map_err(|e| e.to_string())?;
    let [left, bottom, right, top] = bounds.as_slice() else {
        return Err("标签页面尺寸无效".into());
    };
    let coord = |n: &Object| n.as_float().map_err(|e| e.to_string());
    let (left, bottom) = (coord(left)?, coord(bottom)?);
    let (width, height) = (coord(right)? - left, coord(top)? - bottom);
    if (width / height - 1.6).abs() > 0.03
        || page.get(b"Rotate").and_then(Object::as_i64).unwrap_or(0) != 0
    {
        return Err("标签版式与 Ozon 横版箱唛样式不符".into());
    }
    let size = (height * 0.16).min(width * 0.24 / (0.5 * number.len() as f32));
    let x = left + width * 0.61 - size * 0.25 * number.len() as f32;
    let y = bottom + height * 0.48;
    let font = doc.add_object(
        dictionary! {"Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Times-Bold"},
    );
    let resources = doc
        .get_or_create_resources(id)
        .and_then(Object::as_dict_mut)
        .map_err(|e| e.to_string())?;
    resources
        .get_mut(b"Font")
        .and_then(Object::as_dict_mut)
        .map_err(|e| e.to_string())?
        .set("ERPBoxNumber", font);
    let overlay = format!(
        "Q q BT /ERPBoxNumber {size:.3} Tf 0 g 1 0 0 1 {x:.3} {y:.3} Tm ({number}) Tj ET Q\n"
    );
    // Isolate the original label's coordinate transforms from the box number.
    doc.add_page_contents(id, overlay.into_bytes())
        .map_err(|e| e.to_string())?;
    let save_state = doc.add_object(lopdf::Stream::new(dictionary! {}, b"q\n".to_vec()));
    doc.get_object_mut(id)
        .and_then(Object::as_dict_mut)
        .and_then(|page| page.get_mut(b"Contents"))
        .and_then(Object::as_array_mut)
        .map_err(|e| e.to_string())?
        .insert(0, Object::Reference(save_state));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires an exported workbook and its source PDFs via OZON_LABEL_SAMPLE"]
    fn stamps_sample_and_preserves_page_contents() {
        let workbook = std::path::PathBuf::from(std::env::var_os("OZON_LABEL_SAMPLE").unwrap());
        let source = workbook.parent().unwrap();
        let output = std::env::var_os("OZON_LABEL_OUTPUT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::env::temp_dir().join(format!("ozon-label-test-{}", uuid::Uuid::new_v4()))
            });
        let (order_id, numbers) = read_workbook(fs::read(workbook.clone()).unwrap()).unwrap();
        assert_eq!(order_id, 140674216);
        assert_eq!(numbers["1022122240898000"], "38");
        assert_eq!(
            stamp_directory(source, &output, &numbers).unwrap(),
            numbers.len()
        );
        let file = "Ozon箱唛-供应-2000074212641.pdf";
        let original = Document::load(source.join(file)).unwrap();
        let edited = Document::load(output.join(file)).unwrap();
        let page_id = *edited
            .get_pages()
            .iter()
            .find(|(page, _)| {
                edited
                    .extract_text(&[**page])
                    .unwrap()
                    .lines()
                    .any(|line| line == "38")
            })
            .unwrap()
            .1;
        let stamped = edited.get_page_content(page_id).unwrap();
        assert!(original.get_pages().values().any(|id| stamped
            .strip_prefix(b"q\n")
            .unwrap()
            .starts_with(&original.get_page_content(*id).unwrap())));
        println!("{}", output.display());
    }
}
