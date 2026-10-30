use super::{
    active_shop_identity, analytics_detail_blocking, business_report_blocking,
    cross_border_report_blocking, AppState, DateRange,
};
use chrono::{Datelike, NaiveDate};
use std::{fs, io::Write, path::Path};
use tauri::State;

fn esc(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
fn text_cell(cell: &str, value: &str, style: usize) -> String {
    format!(
        r#"<c r="{cell}" s="{style}" t="inlineStr"><is><t>{}</t></is></c>"#,
        esc(value)
    )
}
fn num_cell(cell: &str, value: f64, style: usize) -> String {
    format!(r#"<c r="{cell}" s="{style}"><v>{value}</v></c>"#)
}
fn formula_cell(cell: &str, formula: &str, cached: f64, style: usize) -> String {
    format!(
        r#"<c r="{cell}" s="{style}"><f>{}</f><v>{cached}</v></c>"#,
        esc(formula)
    )
}

fn sheet_xml(
    report: &super::BusinessReport,
    detail: &super::AnalyticsDetail,
    month: &str,
    store: &str,
    rate: f64,
) -> String {
    let first = 12usize;
    let last = (first + detail.products.len().saturating_sub(1)).max(first);
    let mut rows = String::new();
    rows.push_str(&format!(
        r#"<row r="1" ht="36" customHeight="1">{}</row>"#,
        text_cell("A1", &format!("Ozon 月度经营报告 · {month}"), 1)
    ));
    rows.push_str(&format!(
        r#"<row r="2">{}{}</row>"#,
        text_cell("A2", &format!("店铺：{store}"), 2),
        text_cell("E2", "币种：RUB；采购成本按 RUB/CNY 汇率换算", 2)
    ));

    let summaries = [
        (
            "A4",
            "月销量",
            "B4",
            format!("SUM(D{first}:D{last})"),
            report.orders as f64,
            5,
        ),
        (
            "D4",
            "月销售额",
            "E4",
            format!("SUM(F{first}:F{last})"),
            report.revenue,
            6,
        ),
        (
            "G4",
            "月广告（店铺）",
            "H4",
            String::new(),
            report.ad_spend,
            6,
        ),
        (
            "J4",
            "SKU 广告合计",
            "K4",
            format!("SUM(G{first}:G{last})"),
            detail.products.iter().map(|x| x.ad_spend).sum(),
            6,
        ),
        (
            "M4",
            "广告分配差异",
            "N4",
            "H4-K4".into(),
            report.ad_spend - detail.products.iter().map(|x| x.ad_spend).sum::<f64>(),
            6,
        ),
        (
            "P4",
            "月采购成本",
            "Q4",
            format!("SUM(K{first}:K{last})"),
            report.purchase_cost,
            6,
        ),
        (
            "S4",
            "月头程成本",
            "T4",
            format!("SUM(M{first}:M{last})"),
            report.first_mile_cost,
            6,
        ),
    ];
    let mut summary_row = String::new();
    for (label_cell, label, value_cell, formula, cached, style) in summaries {
        summary_row.push_str(&text_cell(label_cell, label, 3));
        if formula.is_empty() {
            summary_row.push_str(&num_cell(value_cell, cached, style));
        } else {
            summary_row.push_str(&formula_cell(value_cell, &formula, cached, style));
        }
    }
    rows.push_str(&format!(
        r#"<row r="4" ht="28" customHeight="1">{summary_row}</row>"#
    ));

    let profit = report.settled_profit.unwrap_or(0.0);
    let controls = [
        (
            "A6",
            "Finance 已结算净额",
            "B6",
            report.finance_net,
            String::new(),
        ),
        (
            "D6",
            "Finance 销售/退货",
            "E6",
            report.sales_returns,
            String::new(),
        ),
        (
            "G6",
            "Finance 应计费用",
            "H6",
            report.accrual_fees,
            String::new(),
        ),
        (
            "J6",
            "SKU 月总成本",
            "K6",
            detail
                .products
                .iter()
                .filter_map(|x| x.estimated_profit.map(|p| x.revenue - p))
                .sum(),
            format!("SUM(O12:O{last})"),
        ),
        (
            "M6",
            "SKU 预估利润",
            "N6",
            detail
                .products
                .iter()
                .filter_map(|x| x.estimated_profit)
                .sum(),
            format!("SUM(P12:P{last})"),
        ),
        ("P6", "结算口径税前利润", "Q6", profit, String::new()),
        (
            "S6",
            "缺成本 SKU",
            "T6",
            report.missing_cost_skus as f64,
            String::new(),
        ),
    ];
    let mut control_row = String::new();
    for (lc, label, vc, value, formula) in controls {
        control_row.push_str(&text_cell(lc, label, 3));
        if formula.is_empty() {
            control_row.push_str(&num_cell(vc, value, if vc == "T6" { 5 } else { 6 }));
        } else {
            control_row.push_str(&formula_cell(vc, &formula, value, 6));
        }
    }
    rows.push_str(&format!(
        r#"<row r="6" ht="28" customHeight="1">{control_row}</row>"#
    ));
    rows.push_str(&format!(r#"<row r="8">{}{}</row>"#, text_cell("A8","核算与验证说明",2), text_cell("B8","SKU 预估利润=销售额-广告费-采购成本-头程成本-预估平台履约费。结算口径税前利润来自 Finance 净额减已交付采购及头程；缺成本 SKU 不计入 SKU 利润合计。广告分配差异用于识别店铺级广告未分摊到 SKU 的金额。",2)));

    let headers = [
        "SKU",
        "货号",
        "商品名称",
        "月销量",
        "平均售价 RUB",
        "月销售额 RUB",
        "SKU 广告费 RUB",
        "单位采购成本 CNY",
        "RUB/CNY",
        "单位采购成本 RUB",
        "采购成本 RUB",
        "单位头程 RUB",
        "头程成本 RUB",
        "预估平台履约费 RUB",
        "总成本 RUB",
        "预估利润 RUB",
        "利润率",
        "利润贡献",
        "成本完整性",
        "公式校验",
    ];
    let cols = [
        "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R",
        "S", "T",
    ];
    let mut header = String::new();
    for (i, value) in headers.iter().enumerate() {
        header.push_str(&text_cell(&format!("{}11", cols[i]), value, 4));
    }
    rows.push_str(&format!(
        r#"<row r="11" ht="31" customHeight="1">{header}</row>"#
    ));

    let total_profit = detail
        .products
        .iter()
        .filter_map(|x| x.estimated_profit)
        .sum::<f64>();
    for (index, item) in detail.products.iter().enumerate() {
        let r = first + index;
        let unit_purchase = item.purchase_cost.map(|v| {
            if item.units > 0 {
                v / item.units as f64
            } else {
                0.0
            }
        });
        let unit_cny = unit_purchase.map(|v| v / rate);
        let unit_first = item.first_mile_cost.map(|v| {
            if item.units > 0 {
                v / item.units as f64
            } else {
                0.0
            }
        });
        let total_cost = match (item.purchase_cost, item.first_mile_cost) {
            (Some(p), Some(f)) => Some(item.ad_spend + p + f + item.platform_fees),
            _ => None,
        };
        let mut c = String::new();
        c.push_str(&text_cell(&format!("A{r}"), &item.sku, 7));
        c.push_str(&text_cell(&format!("B{r}"), &item.offer_id, 7));
        c.push_str(&text_cell(&format!("C{r}"), &item.product_name, 7));
        c.push_str(&num_cell(&format!("D{r}"), item.units as f64, 5));
        c.push_str(&formula_cell(
            &format!("E{r}"),
            &format!("IFERROR(F{r}/D{r},0)"),
            if item.units > 0 {
                item.revenue / item.units as f64
            } else {
                0.0
            },
            6,
        ));
        c.push_str(&num_cell(&format!("F{r}"), item.revenue, 6));
        c.push_str(&num_cell(&format!("G{r}"), item.ad_spend, 6));
        if let Some(v) = unit_cny {
            c.push_str(&num_cell(&format!("H{r}"), v, 6));
        } else {
            c.push_str(&text_cell(&format!("H{r}"), "", 8));
        }
        c.push_str(&num_cell(&format!("I{r}"), rate, 6));
        c.push_str(&formula_cell(
            &format!("J{r}"),
            &format!("IF(H{r}=\"\",\"\",H{r}*I{r})"),
            unit_purchase.unwrap_or(0.0),
            6,
        ));
        c.push_str(&formula_cell(
            &format!("K{r}"),
            &format!("IF(J{r}=\"\",\"\",J{r}*D{r})"),
            item.purchase_cost.unwrap_or(0.0),
            6,
        ));
        if let Some(v) = unit_first {
            c.push_str(&num_cell(&format!("L{r}"), v, 6));
        } else {
            c.push_str(&text_cell(&format!("L{r}"), "", 8));
        }
        c.push_str(&formula_cell(
            &format!("M{r}"),
            &format!("IF(L{r}=\"\",\"\",L{r}*D{r})"),
            item.first_mile_cost.unwrap_or(0.0),
            6,
        ));
        c.push_str(&num_cell(&format!("N{r}"), item.platform_fees, 6));
        c.push_str(&formula_cell(
            &format!("O{r}"),
            &format!("IF(OR(K{r}=\"\",M{r}=\"\"),\"\",SUM(G{r},K{r},M{r},N{r}))"),
            total_cost.unwrap_or(0.0),
            6,
        ));
        c.push_str(&formula_cell(
            &format!("P{r}"),
            &format!("IF(O{r}=\"\",\"\",F{r}-O{r})"),
            item.estimated_profit.unwrap_or(0.0),
            6,
        ));
        c.push_str(&formula_cell(
            &format!("Q{r}"),
            &format!("IFERROR(P{r}/F{r},0)"),
            item.profit_rate.unwrap_or(0.0) / 100.0,
            9,
        ));
        c.push_str(&formula_cell(
            &format!("R{r}"),
            &format!("IFERROR(P{r}/$N$6,0)"),
            item.estimated_profit
                .filter(|_| total_profit != 0.0)
                .map(|v| v / total_profit)
                .unwrap_or(0.0),
            9,
        ));
        c.push_str(&text_cell(
            &format!("S{r}"),
            if item.cost_complete {
                "完整"
            } else {
                "缺少成本"
            },
            if item.cost_complete { 10 } else { 8 },
        ));
        c.push_str(&formula_cell(&format!("T{r}"),&format!("IF(S{r}=\"缺少成本\",\"缺少成本\",IF(ABS(P{r}-(F{r}-O{r}))<0.01,\"通过\",\"异常\"))"),0.0,10));
        rows.push_str(&format!(
            r#"<row r="{r}" ht="27" customHeight="1">{c}</row>"#
        ));
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetViews><sheetView showGridLines="0" workbookViewId="0"><pane ySplit="11" topLeftCell="A12" activePane="bottomLeft" state="frozen"/></sheetView></sheetViews><sheetFormatPr defaultRowHeight="20"/><cols><col min="1" max="2" width="16" customWidth="1"/><col min="3" max="3" width="28" customWidth="1"/><col min="4" max="20" width="16" customWidth="1"/></cols><sheetData>{rows}</sheetData><autoFilter ref="A11:T{last}"/><mergeCells count="3"><mergeCell ref="A1:T1"/><mergeCell ref="A2:D2"/><mergeCell ref="B8:T8"/></mergeCells><pageMargins left="0.2" right="0.2" top="0.35" bottom="0.35" header="0.15" footer="0.15"/><pageSetup orientation="landscape" fitToWidth="1" fitToHeight="0" paperSize="9"/></worksheet>"#
    )
}

pub(crate) fn write_xlsx(path: &Path, sheet: String) -> Result<(), String> {
    write_xlsx_extra(path, sheet, Vec::new())
}

fn write_xlsx_extra(path: &Path, sheet: String, extras: Vec<(String,String)>) -> Result<(), String> {
    let styles = r#"<?xml version="1.0" encoding="UTF-8"?><styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><numFmts count="2"><numFmt numFmtId="164" formatCode="₽#,##0.00;[Red]-₽#,##0.00"/><numFmt numFmtId="165" formatCode="0.00%"/></numFmts><fonts count="3"><font><sz val="10"/><name val="Microsoft YaHei"/></font><font><b/><sz val="20"/><color rgb="FF0B1F3A"/><name val="Microsoft YaHei"/></font><font><b/><sz val="10"/><color rgb="FFFFFFFF"/><name val="Microsoft YaHei"/></font></fonts><fills count="5"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill><fill><patternFill patternType="solid"><fgColor rgb="FFEDF4FF"/></patternFill></fill><fill><patternFill patternType="solid"><fgColor rgb="FF246BFD"/></patternFill></fill><fill><patternFill patternType="solid"><fgColor rgb="FFFFF2CC"/></patternFill></fill></fills><borders count="2"><border/><border><left style="thin"><color rgb="FFDDE5F0"/></left><right style="thin"><color rgb="FFDDE5F0"/></right><top style="thin"><color rgb="FFDDE5F0"/></top><bottom style="thin"><color rgb="FFDDE5F0"/></bottom></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="11"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/><xf numFmtId="0" fontId="1" fillId="0" borderId="0"/><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/><xf numFmtId="0" fontId="0" fillId="2" borderId="1"/><xf numFmtId="0" fontId="2" fillId="3" borderId="1" applyAlignment="1"><alignment horizontal="center" vertical="center" wrapText="1"/></xf><xf numFmtId="0" fontId="0" fillId="0" borderId="1"/><xf numFmtId="164" fontId="0" fillId="0" borderId="1"/><xf numFmtId="0" fontId="0" fillId="0" borderId="1" applyAlignment="1"><alignment vertical="center" wrapText="1"/></xf><xf numFmtId="0" fontId="0" fillId="4" borderId="1"/><xf numFmtId="165" fontId="0" fillId="0" borderId="1"/><xf numFmtId="0" fontId="0" fillId="2" borderId="1"/></cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>"#;
    let parts=[("[Content_Types].xml",r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/></Types>"#.to_string()),("_rels/.rels",r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#.to_string()),("xl/workbook.xml",r#"<?xml version="1.0" encoding="UTF-8"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="月报" sheetId="1" r:id="rId1"/></sheets><calcPr calcMode="auto" fullCalcOnLoad="1" forceFullCalc="1"/></workbook>"#.to_string()),("xl/_rels/workbook.xml.rels",r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#.to_string()),("xl/styles.xml",styles.to_string()),("xl/worksheets/sheet1.xml",sheet)];
    let mut parts = parts.into_iter().map(|(n,d)|(n.to_string(),d)).collect::<Vec<_>>();
    if !extras.is_empty() { parts[2].1=parts[2].1.replace("name=\"月报\"","name=\"Finance订单核算\""); }
    for (index,(name,xml)) in extras.into_iter().enumerate() {
        let number=index+2;
        let rid=index+3;
        parts[0].1=parts[0].1.replace("</Types>",&format!("<Override PartName=\"/xl/worksheets/sheet{number}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/></Types>"));
        parts[2].1=parts[2].1.replace("</sheets>",&format!("<sheet name=\"{}\" sheetId=\"{number}\" r:id=\"rId{rid}\"/></sheets>",esc(&name)));
        parts[3].1=parts[3].1.replace("</Relationships>",&format!("<Relationship Id=\"rId{rid}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet{number}.xml\"/></Relationships>"));
        parts.push((format!("xl/worksheets/sheet{number}.xml"),xml));
    }
    let mut zip = zip::ZipWriter::new(fs::File::create(path).map_err(|e| e.to_string())?);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, data) in parts {
        zip.start_file(name, options).map_err(|e| e.to_string())?;
        zip.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

fn settlement_sheets(report: &super::CrossBorderReport, store: &str) -> Vec<(String,String)> {
    fn table(title: &str, note: &str, headers: &[&str], records: Vec<Vec<serde_json::Value>>) -> String {
        let mut rows=format!("<row r=\"1\">{}</row><row r=\"2\">{}</row>",text_cell("A1",title,1),text_cell("A2",note,2));
        rows.push_str("<row r=\"4\">");
        for (i,h) in headers.iter().enumerate() { rows.push_str(&text_cell(&format!("{}4",(b'A'+i as u8) as char),h,4)); }
        rows.push_str("</row>");
        for (index,record) in records.iter().enumerate() {
            let n=index+5;
            rows.push_str(&format!("<row r=\"{n}\">"));
            for (i,v) in record.iter().enumerate() {
                let cell=format!("{}{n}",(b'A'+i as u8) as char);
                if let Some(value)=v.as_f64() { rows.push_str(&num_cell(&cell,value,5)); }
                else { rows.push_str(&text_cell(&cell,v.as_str().unwrap_or("待核对"),7)); }
            }
            rows.push_str("</row>");
        }
        let last=(records.len()+4).max(5);
        let end=(b'A'+headers.len() as u8-1)as char;
        format!(r#"<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetViews><sheetView workbookViewId="0"><pane ySplit="4" topLeftCell="A5" state="frozen"/></sheetView></sheetViews><cols><col min="1" max="{}" width="22" customWidth="1"/></cols><sheetData>{rows}</sheetData><autoFilter ref="A4:{end}{last}"/></worksheet>"#,headers.len())
    }
    use serde_json::json;
    let s=&report.settlement;
    let note=format!("店铺 {store}；{} 至 {}，按 Finance 入账日；1 CNY={} RUB。净额已含广告/运费/退款，不重复扣费；利润=净额-对应商品采购成本。采用当前录入采购成本，退货回仓未核实则不冲回成本；银行卡到账、税费和平台外费用未核实。",report.date_from,report.date_to,report.rub_per_cny);
    let mut orders=vec![vec![json!("合计"),json!(report.date_from),json!(report.date_to),json!(s.sale_cny),json!(s.refunds_cny),json!(s.commission_cny),json!(s.delivery_cny),json!(s.return_delivery_cny),json!(s.advertising_cny),json!(s.other_cny),json!(s.net_cny),json!(s.purchase_cny),json!(s.profit_before_return_recovery_cny),json!(s.profit_cny),json!(format!("{}组缺成本/数量，{}组退款待核对",s.missing_cost_orders,s.return_review_orders))]];
    let mut entries=Vec::new();
    let mut products=Vec::new();
    for r in &s.rows {
        let status=if r.return_review {format!("{}；退货回仓成本待核对",r.issues.join("；"))}else{r.issues.join("；")};
        orders.push(vec![json!(r.posting_number),json!(r.first_day),json!(r.last_day),json!(r.sale_cny),json!(r.refunds_cny),json!(r.commission_cny),json!(r.delivery_cny),json!(r.return_delivery_cny),json!(r.advertising_cny),json!(r.other_cny),json!(r.net_cny),json!(r.purchase_cny),json!(r.profit_before_return_recovery_cny),json!(r.profit_cny),json!(status)]);
        for e in &r.entries { entries.push(vec![json!(r.posting_number),json!(e.operation_id),json!(e.day),json!(e.operation_type),json!(e.sale_cny),json!(e.commission_cny),json!(e.delivery_cny),json!(e.return_delivery_cny),json!(e.advertising_cny),json!(e.other_cny),json!(e.net_cny)]); }
        for p in &r.products { products.push(vec![json!(r.posting_number),json!(p.sku),json!(p.offer_id),json!(p.quantity),json!(p.quantity_source),json!(p.unit_cost_cny),json!(p.purchase_cny)]); }
    }
    vec![
        ("Finance订单核算".into(),table("跨境 Finance 入账利润（CNY）",&note,&["订单号","首次入账日","最后入账日","销售应计 CNY","退款 CNY","佣金 CNY","实际正向物流 CNY","实际退货物流 CNY","实际广告 CNY","收单及其他 CNY","入账净额 CNY","对应采购成本 CNY","退货成本未冲回前利润 CNY","入账利润 CNY","待核对"],orders)),
        ("Finance原始流水".into(),table("Finance 逐笔流水（CNY）",&note,&["订单号","流水 ID","入账日","平台类型","销售/退款 CNY","佣金 CNY","正向物流 CNY","退货物流 CNY","广告 CNY","收单及其他 CNY","入账净额 CNY"],entries)),
        ("入账商品成本".into(),table("本期确认的订单商品成本（CNY）",&note,&["订单号","SKU","货号","明确数量","数量来源","当前录入单位成本 CNY","本期商品成本 CNY"],products)),
    ]
}

fn cross_border_sheet(report: &super::CrossBorderReport, store: &str) -> String {
    let first = 12usize;
    let last = (first + report.rows.len().saturating_sub(1)).max(first);
    let mut rows = String::new();
    rows.push_str(&format!(
        r#"<row r="1" ht="36" customHeight="1">{}</row>"#,
        text_cell(
            "A1",
            &format!(
                "Ozon 跨境店铺利润报表 · {} 至 {}",
                report.date_from, report.date_to
            ),
            1
        )
    ));
    rows.push_str(&format!(
        r#"<row r="2">{}{}</row>"#,
        text_cell("A2", &format!("店铺：{store}"), 2),
        text_cell(
            "F2",
            &format!("币种：CNY；汇率 1 CNY = {:.4} RUB", report.rub_per_cny),
            2
        )
    ));
    let mut top = String::new();
    for (lc, label, vc, formula, cached, style) in [
        (
            "A4",
            "销量",
            "B4",
            format!("SUM(D{first}:D{last})"),
            report.units as f64,
            5,
        ),
        (
            "D4",
            "销售额",
            "E4",
            format!("SUM(J{first}:J{last})"),
            report.revenue_cny,
            6,
        ),
        ("G4", "广告费", "H4", String::new(), report.ad_spend_cny, 6),
        (
            "J4",
            "采购成本",
            "K4",
            format!("SUM(M{first}:M{last})"),
            report
                .rows
                .iter()
                .filter_map(|x| x.purchase_total_cny)
                .sum(),
            6,
        ),
        (
            "M4",
            "跨境运费",
            "N4",
            format!("SUM(P{first}:P{last})"),
            report.rows.iter().filter_map(|x| x.freight_total_cny).sum(),
            6,
        ),
        (
            "P4",
            "平台及收单调整",
            "Q4",
            format!("SUM(Q{first}:Q{last})"),
            report.estimated_platform_fees_cny,
            6,
        ),
        (
            "S4",
            "月总成本",
            "T4",
            format!("SUM(R{first}:R{last})"),
            report
                .profit_cny
                .map(|p| report.revenue_cny - p)
                .unwrap_or(0.0),
            6,
        ),
        (
            "V4",
            "月利润",
            "W4",
            format!("SUM(S{first}:S{last})"),
            report.profit_cny.unwrap_or(0.0),
            6,
        ),
        (
            "Y4",
            "缺成本 SKU",
            "Z4",
            String::new(),
            report.missing_cost_skus as f64,
            5,
        ),
    ] {
        top.push_str(&text_cell(lc, label, 3));
        if formula.is_empty() {
            top.push_str(&num_cell(vc, cached, style));
        } else {
            top.push_str(&formula_cell(vc, &formula, cached, style));
        }
    }
    rows.push_str(&format!(
        r#"<row r="4" ht="28" customHeight="1">{top}</row>"#
    ));
    let mut source = String::new();
    for (lc, label, vc, value, style) in [
        (
            "A6",
            "Performance 广告",
            "B6",
            report.performance_ad_spend_cny,
            6,
        ),
        ("D6", "Stars 会员费", "E6", report.stars_membership_cny, 6),
        (
            "G6",
            "Finance 结算净额",
            "H6",
            report.settled_finance_net_cny,
            6,
        ),
        ("J6", "FBP 订单", "K6", report.fbp_orders as f64, 5),
        ("M6", "RFBS 订单", "N6", report.rfbs_orders as f64, 5),
        ("P6", "WHD 订单", "Q6", report.whd_orders as f64, 5),
        (
            "S6",
            "佣金率",
            "T6",
            report.commission_rate.unwrap_or(0.0),
            9,
        ),
        (
            "V6",
            "收单费率",
            "W6",
            report.acquiring_rate.unwrap_or(0.0),
            9,
        ),
    ] {
        source.push_str(&text_cell(lc, label, 3));
        source.push_str(&num_cell(vc, value, style));
    }
    rows.push_str(&format!(
        r#"<row r="6" ht="28" customHeight="1">{source}</row>"#
    ));
    let basis=format!("下单预估参考（未扣退货取消，不代表 Finance 入账利润）。运费首档售价分界 ¥{}（含），采用俄通收 RETS 2026.07.24 俄罗斯段；货值档位1500/7000/250000 RUB按当前汇率换算，实重计费并校验尺寸，按逐日SKU均价汇总，单位运费为加权均值。广告按 SKU 销售额占比分摊；总成本=广告+采购+跨境运费-平台及收单调整，利润=销售额-总成本。缺少成本或费率时不计入利润合计。",report.freight_threshold_cny);
    rows.push_str(&format!(r#"<row r="8">{}{}</row>"#,text_cell("A8","核算与验证说明",2),text_cell("B8",&basis,2)));
    let headers = [
        "SKU",
        "货号",
        "商品名称",
        "销量",
        "履约订单",
        "FBP",
        "RFBS",
        "WHD",
        "平均售价 CNY",
        "销售额 CNY",
        "分摊广告 CNY",
        "单位采购成本 CNY",
        "采购成本 CNY",
        "重量 kg",
        "加权件均跨境运费 CNY",
        "跨境运费 CNY",
        "平台及收单调整 CNY",
        "总成本 CNY",
        "利润 CNY",
        "利润率",
        "利润贡献",
        "Finance 结算 CNY",
        "佣金率",
        "收单费率",
        "成本完整性",
        "公式校验",
    ];
    let cols = [
        "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R",
        "S", "T", "U", "V", "W", "X", "Y", "Z",
    ];
    let mut header = String::new();
    for (i, v) in headers.iter().enumerate() {
        header.push_str(&text_cell(&format!("{}11", cols[i]), v, 4));
    }
    rows.push_str(&format!(
        r#"<row r="11" ht="32" customHeight="1">{header}</row>"#
    ));
    for (index, item) in report.rows.iter().enumerate() {
        let r = first + index;
        let allocated_ad = if report.revenue_cny != 0.0 {
            report.ad_spend_cny * item.revenue_cny / report.revenue_cny
        } else {
            0.0
        };
        let total = match (
            item.purchase_total_cny,
            item.freight_total_cny,
            item.estimated_platform_fees_cny,
        ) {
            (Some(p), Some(f), Some(fee)) => Some(allocated_ad + p + f - fee),
            _ => None,
        };
        let profit = total.map(|v| item.revenue_cny - v);
        let mut c = String::new();
        c.push_str(&text_cell(&format!("A{r}"), &item.sku, 7));
        c.push_str(&text_cell(&format!("B{r}"), &item.offer_id, 7));
        c.push_str(&text_cell(&format!("C{r}"), &item.product_name, 7));
        c.push_str(&num_cell(&format!("D{r}"), item.units as f64, 5));
        c.push_str(&num_cell(
            &format!("E{r}"),
            item.fulfillment_orders as f64,
            5,
        ));
        c.push_str(&num_cell(&format!("F{r}"), item.fbp_orders as f64, 5));
        c.push_str(&num_cell(&format!("G{r}"), item.rfbs_orders as f64, 5));
        c.push_str(&num_cell(&format!("H{r}"), item.whd_orders as f64, 5));
        c.push_str(&formula_cell(
            &format!("I{r}"),
            &format!("IFERROR(J{r}/D{r},0)"),
            item.selling_price_cny,
            6,
        ));
        c.push_str(&num_cell(&format!("J{r}"), item.revenue_cny, 6));
        c.push_str(&formula_cell(
            &format!("K{r}"),
            &format!("IFERROR($H$4*J{r}/$E$4,0)"),
            allocated_ad,
            6,
        ));
        if let Some(v) = item.purchase_cost_cny {
            c.push_str(&num_cell(&format!("L{r}"), v, 6));
        } else {
            c.push_str(&text_cell(&format!("L{r}"), "", 8));
        }
        c.push_str(&formula_cell(
            &format!("M{r}"),
            &format!("IF(L{r}=\"\",\"\",L{r}*D{r})"),
            item.purchase_total_cny.unwrap_or(0.0),
            6,
        ));
        if let Some(v) = item.weight_kg {
            c.push_str(&num_cell(&format!("N{r}"), v, 6));
        } else {
            c.push_str(&text_cell(&format!("N{r}"), "", 8));
        }
        if let Some(v) = item.freight_unit_cny {
            c.push_str(&num_cell(&format!("O{r}"), v, 6));
        } else {
            c.push_str(&text_cell(&format!("O{r}"), "", 8));
        }
        c.push_str(&formula_cell(
            &format!("P{r}"),
            &format!("IF(O{r}=\"\",\"\",O{r}*D{r})"),
            item.freight_total_cny.unwrap_or(0.0),
            6,
        ));
        if let Some(v) = item.estimated_platform_fees_cny {
            c.push_str(&num_cell(&format!("Q{r}"), v, 6));
        } else {
            c.push_str(&text_cell(&format!("Q{r}"), "", 8));
        }
        c.push_str(&formula_cell(
            &format!("R{r}"),
            &format!("IF(OR(M{r}=\"\",P{r}=\"\",Q{r}=\"\"),\"\",K{r}+M{r}+P{r}-Q{r})"),
            total.unwrap_or(0.0),
            6,
        ));
        c.push_str(&formula_cell(
            &format!("S{r}"),
            &format!("IF(R{r}=\"\",\"\",J{r}-R{r})"),
            profit.unwrap_or(0.0),
            6,
        ));
        c.push_str(&formula_cell(
            &format!("T{r}"),
            &format!("IFERROR(S{r}/J{r},0)"),
            profit
                .filter(|_| item.revenue_cny != 0.0)
                .map(|v| v / item.revenue_cny)
                .unwrap_or(0.0),
            9,
        ));
        c.push_str(&formula_cell(
            &format!("U{r}"),
            &format!("IFERROR(S{r}/$W$4,0)"),
            profit
                .filter(|_| report.profit_cny.unwrap_or(0.0) != 0.0)
                .map(|v| v / report.profit_cny.unwrap_or(1.0))
                .unwrap_or(0.0),
            9,
        ));
        if let Some(v) = item.finance_settled_cny {
            c.push_str(&num_cell(&format!("V{r}"), v, 6));
        } else {
            c.push_str(&text_cell(&format!("V{r}"), "", 7));
        }
        if let Some(v) = item.commission_rate {
            c.push_str(&num_cell(&format!("W{r}"), v, 9));
        } else {
            c.push_str(&text_cell(&format!("W{r}"), "", 8));
        }
        if let Some(v) = item.acquiring_rate {
            c.push_str(&num_cell(&format!("X{r}"), v, 9));
        } else {
            c.push_str(&text_cell(&format!("X{r}"), "", 8));
        }
        c.push_str(&text_cell(
            &format!("Y{r}"),
            if item.cost_complete {
                "完整"
            } else {
                "缺少成本"
            },
            if item.cost_complete { 10 } else { 8 },
        ));
        c.push_str(&formula_cell(&format!("Z{r}"),&format!("IF(Y{r}=\"缺少成本\",\"缺少成本\",IF(ABS(S{r}-(J{r}-R{r}))<0.01,\"通过\",\"异常\"))"),0.0,10));
        rows.push_str(&format!(
            r#"<row r="{r}" ht="27" customHeight="1">{c}</row>"#
        ));
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetViews><sheetView showGridLines="0" workbookViewId="0"><pane ySplit="11" topLeftCell="A12" activePane="bottomLeft" state="frozen"/></sheetView></sheetViews><sheetFormatPr defaultRowHeight="20"/><cols><col min="1" max="2" width="16" customWidth="1"/><col min="3" max="3" width="28" customWidth="1"/><col min="4" max="26" width="15" customWidth="1"/></cols><sheetData>{rows}</sheetData><autoFilter ref="A11:Z{last}"/><mergeCells count="3"><mergeCell ref="A1:Z1"/><mergeCell ref="A2:E2"/><mergeCell ref="B8:Z8"/></mergeCells><pageMargins left="0.2" right="0.2" top="0.35" bottom="0.35" header="0.15" footer="0.15"/><pageSetup orientation="landscape" fitToWidth="1" fitToHeight="0" paperSize="9"/></worksheet>"#
    )
}

#[tauri::command]
pub fn export_ozon_monthly_report(month: String, state: State<AppState>) -> Result<String, String> {
    let start = NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d")
        .map_err(|_| "月份格式无效，请选择 YYYY-MM".to_string())?;
    let next = if start.month() == 12 {
        NaiveDate::from_ymd_opt(start.year() + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(start.year(), start.month() + 1, 1)
    }
    .ok_or("月份无效")?;
    let today = chrono::Local::now().date_naive();
    if start > today {
        return Err("不能导出未来月份".into());
    }
    let end = next.pred_opt().ok_or("月份无效")?.min(today);
    let range = DateRange {
        from: start.format("%Y-%m-%d").to_string(),
        to: end.format("%Y-%m-%d").to_string(),
    };
    let report = business_report_blocking(range.clone(), &state)?;
    let detail = analytics_detail_blocking(range, &state)?;
    let (_, store) = active_shop_identity(&state)?;
    let c = super::db(&state)?;
    let rate = super::rub_per_cny_for(&state, &c)?;
    let dir = state
        .data_dir
        .parent()
        .unwrap_or(&state.data_dir)
        .join("exports")
        .join("ozon-monthly");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let safe: String = store
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let path = dir.join(format!("Ozon月报_{safe}_{month}.xlsx"));
    write_xlsx(&path, sheet_xml(&report, &detail, &month, &store, rate))?;
    let _ = open::that(&path);
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn export_ozon_cross_border_report(
    range: DateRange,
    state: State<AppState>,
) -> Result<String, String> {
    if range.from.len() != 10 || range.to.len() != 10 || range.from > range.to {
        return Err("跨境利润日期范围无效".into());
    }
    let report = cross_border_report_blocking(range, &state)?;
    let (_, store) = active_shop_identity(&state)?;
    let dir = state
        .data_dir
        .parent()
        .unwrap_or(&state.data_dir)
        .join("exports")
        .join("ozon-cross-border");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let safe: String = store
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let path = dir.join(format!(
        "Ozon跨境利润_{safe}_{}_{}.xlsx",
        report.date_from, report.date_to
    ));
    let mut sheets=settlement_sheets(&report,&store);
    let (_,first)=sheets.remove(0);
    sheets.push(("下单预估参考".into(),cross_border_sheet(&report,&store)));
    write_xlsx_extra(&path,first,sheets)?;
    let _ = open::that(&path);
    Ok(path.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settlement_export_keeps_order_costs_and_all_operations_in_separate_sheets() {
        use super::super::finance_settlement::{SettlementOrder,SettlementReport,SettlementProduct,SettlementEntry};
        let entry=SettlementEntry{operation_id:"OP-1".into(),day:"2026-10-01".into(),operation_type:"OperationAgentDeliveredToCustomer".into(),net_cny:80.,sale_cny:100.,commission_cny:-10.,delivery_cny:-10.,return_delivery_cny:0.,advertising_cny:0.,other_cny:0.};
        let order=SettlementOrder{posting_number:"ORDER-1".into(),net_cny:80.,purchase_cny:Some(20.),profit_cny:Some(60.),profit_before_return_recovery_cny:Some(60.),products:vec![SettlementProduct{sku:"SKU-1".into(),offer_id:"GOODS-1".into(),quantity:Some(2),unit_cost_cny:Some(10.),purchase_cny:Some(20.),quantity_source:"Finance 商品数量".into()}],entries:vec![entry],..Default::default()};
        let report=super::super::CrossBorderReport{date_from:"2026-10-01".into(),date_to:"2026-10-20".into(),rub_per_cny:12.,settlement:SettlementReport{net_cny:80.,purchase_cny:Some(20.),profit_cny:Some(60.),rows:vec![order],..Default::default()},..Default::default()};
        let mut sheets=settlement_sheets(&report,"审计店");
        assert_eq!(sheets.len(),3);
        assert!(sheets[0].1.contains("银行卡到账"));
        assert!(sheets[1].1.contains("OP-1"));
        assert!(sheets[2].1.contains("SKU-1"));
        let (_,first)=sheets.remove(0);
        let path=std::env::temp_dir().join(format!("ozon-settlement-export-{}.xlsx",std::process::id()));
        write_xlsx_extra(&path,first,sheets).unwrap();
        let mut zip=zip::ZipArchive::new(fs::File::open(&path).unwrap()).unwrap();
        use std::io::Read;
        let mut workbook=String::new(); zip.by_name("xl/workbook.xml").unwrap().read_to_string(&mut workbook).unwrap();
        assert!(workbook.contains("Finance订单核算"));
        assert!(workbook.contains("Finance原始流水"));
        assert!(workbook.contains("入账商品成本"));
        assert!(zip.by_name("xl/worksheets/sheet3.xml").is_ok());
        drop(zip); fs::remove_file(path).unwrap();
    }
    #[test]
    fn monthly_sheet_contains_totals_and_formula_audit() {
        let report = super::super::BusinessReport {
            revenue: 1000.0,
            orders: 2,
            ad_spend: 80.0,
            finance_net: 600.0,
            sales_returns: 900.0,
            accrual_fees: -300.0,
            other_adjustments: 0.0,
            commission: -100.0,
            finance_advertising: -80.0,
            delivery_fees: -50.0,
            return_fees: 0.0,
            purchase_cost: 250.0,
            first_mile_cost: 20.0,
            estimated_profit: 500.0,
            settled_profit: Some(330.0),
            tax_rate: 3.0,
            tax_amount: -30.0,
            payout_fee_rate: 10.0,
            payout_fee: -60.0,
            after_tax_profit: Some(240.0),
            acquiring: 0.0,
            storage_packaging: 0.0,
            penalties_adjustments: 0.0,
            other_finance_fees: 0.0,
            unallocated_finance_amount: 0.0,
            finance_operations: 1,
            exact_sku_operations: 1,
            unallocated_operations: 0,
            cash_flow_reported_total: 600.0,
            reconciliation_difference: Some(0.0),
            missing_cost_skus: 0,
            costed_units: 2,
            missing_cost_units: 0,
            daily: vec![],
        };
        let detail = super::super::AnalyticsDetail {
            products: vec![super::super::ProductProfitRow {
                sku: "SKU-1".into(),
                offer_id: "OFFER-1".into(),
                product_name: "商品".into(),
                units: 2,
                revenue: 1000.0,
                ad_spend: 80.0,
                purchase_cost: Some(250.0),
                first_mile_cost: Some(20.0),
                platform_fees: 100.0,
                estimated_profit: Some(550.0),
                profit_rate: Some(55.0),
                cross_border_freight: None,
                cost_complete: true,
            }],
            daily_products: vec![],
            series: vec![],
            weekly: vec![],
            weekly_daily: vec![],
        };
        let xml = sheet_xml(&report, &detail, "2026-09", "测试店", 12.5);
        assert!(xml.contains("SUM(P12:P12)"));
        assert!(xml.contains("IF(ABS(P12-(F12-O12))&lt;0.01"));
        assert!(xml.contains("autoFilter ref=\"A11:T12\""));
        assert!(xml.contains("广告分配差异"));
    }
    #[test]
    fn cross_border_sheet_allocates_ads_and_reconciles_profit() {
        let item = super::super::CrossBorderProfitRow {
            sku: "SKU-CROSS".into(),
            offer_id: "OFFER".into(),
            product_name: "跨境商品".into(),
            units: 2,
            fulfillment_orders: 2,
            fbp_orders: 0,
            rfbs_orders: 2,
            whd_orders: 0,
            revenue_cny: 500.0,
            selling_price_cny: 250.0,
            purchase_cost_cny: Some(50.0),
            weight_kg: Some(0.4),
            freight_unit_cny: Some(20.0),
            purchase_total_cny: Some(100.0),
            freight_total_cny: Some(40.0),
            estimated_platform_fees_cny: Some(-75.0),
            contribution_cny: Some(285.0),
            finance_settled_cny: Some(300.0),
            commission_rate: Some(0.12),
            acquiring_rate: Some(0.03),
            cost_complete: true,
        };
        let report = super::super::CrossBorderReport {
            freight_threshold_cny: 120.0,
            settlement: Default::default(),
            date_from: "2026-09-01".into(),
            date_to: "2026-09-30".into(),
            rub_per_cny: 12.5,
            revenue_cny: 500.0,
            units: 2,
            ad_spend_cny: 25.0,
            performance_ad_spend_cny: 20.0,
            stars_membership_cny: 5.0,
            estimated_platform_fees_cny: -75.0,
            purchase_and_freight_cny: 140.0,
            profit_cny: Some(260.0),
            settled_finance_net_cny: 300.0,
            finance_available: true,
            commission_rate: Some(0.12),
            acquiring_rate: Some(0.03),
            missing_cost_skus: 0,
            fbp_orders: 0,
            rfbs_orders: 2,
            whd_orders: 0,
            daily: vec![],
            rows: vec![item],
        };
        let xml = cross_border_sheet(&report, "跨境店");
        assert!(xml.contains("运费首档售价分界 ¥120（含）"));
        assert!(xml.contains("加权件均跨境运费 CNY"));
        assert!(xml.contains("IFERROR($H$4*J12/$E$4,0)"));
        assert!(xml.contains("K12+M12+P12-Q12"));
        assert!(xml.contains("SUM(S12:S12)"));
        assert!(xml.contains("IF(ABS(S12-(J12-R12))&lt;0.01"));
        assert!(xml.contains("autoFilter ref=\"A11:Z12\""));
    }
}
