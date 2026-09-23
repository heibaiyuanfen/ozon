//! One-shot, loopback-only receiver for a user-installed Seerfar companion extension.
//! The extension can read only data rendered into the Ozon page DOM; it cannot read
//! another extension's private storage or a cross-origin/closed iframe.
use crate::{db, AppState};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    thread,
    time::{Duration, Instant},
};
use tauri::State;
use uuid::Uuid;

pub(crate) fn ensure(c: &Connection) -> Result<(), String> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS competitor_shop_seerfar_capture(
            product_id INTEGER PRIMARY KEY,
            sku TEXT NOT NULL,
            seller_price_rub REAL,
            weight_g REAL,
            dimensions_mm TEXT NOT NULL DEFAULT '',
            category TEXT NOT NULL DEFAULT '',
            stock INTEGER,
            seller TEXT NOT NULL DEFAULT '',
            listing_date TEXT NOT NULL DEFAULT '',
            image_url TEXT NOT NULL DEFAULT '',
            captured_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );",
    )
    .map_err(|e| e.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CaptureRow {
    product_id: i64,
    sku: String,
    seller_price_rub: Option<f64>,
    weight_g: Option<f64>,
    dimensions_mm: String,
    category: String,
    stock: Option<i64>,
    seller: String,
    listing_date: String,
    image_url: String,
    captured_at: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CapturePayload {
    token: String,
    sku: String,
    url: String,
    seller_price_rub: Option<f64>,
    weight_g: Option<f64>,
    dimensions_mm: Option<String>,
    category: Option<String>,
    stock: Option<i64>,
    seller: Option<String>,
    listing_date: Option<String>,
    image_url: Option<String>,
}

fn ozon_product_id(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("https://www.ozon.ru/product/")
        || lower.starts_with("https://ozon.ru/product/"))
    {
        return None;
    }
    let path = lower.split(['?', '#']).next()?;
    let slug = path.trim_end_matches('/').rsplit('/').next()?;
    let id = slug.rsplit('-').next()?;
    (id.len() >= 7 && id.chars().all(|c| c.is_ascii_digit())).then(|| id.to_owned())
}

fn valid_payload(data: &CapturePayload, token: &str, sku: &str, product_url: &str) -> bool {
    data.token == token
        && data.sku == sku
        && ozon_product_id(&data.url) == ozon_product_id(product_url)
        && ozon_product_id(product_url).as_deref() == Some(sku)
        && data.seller_price_rub.is_some_and(|n| n.is_finite() && n >= 0.0)
        && data.weight_g.is_none_or(|n| n.is_finite() && n >= 0.0)
}

fn limit(value: Option<String>, max: usize) -> String {
    value.unwrap_or_default().trim().chars().take(max).collect()
}

fn receive_request(stream: &mut TcpStream) -> Result<Option<Vec<u8>>, String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::with_capacity(4096);
    let mut chunk = [0u8; 2048];
    let header_end = loop {
        let n = stream.read(&mut chunk).map_err(|e| e.to_string())?;
        if n == 0 || bytes.len() + n > 32_768 {
            return Err("请求体为空或过大".into());
        }
        bytes.extend_from_slice(&chunk[..n]);
        if let Some(pos) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
    };
    let header = std::str::from_utf8(&bytes[..header_end]).map_err(|e| e.to_string())?;
    if header.starts_with("OPTIONS /capture ") {
        return Ok(None);
    }
    if !header.starts_with("POST /capture ") {
        return Err("仅接受 POST /capture".into());
    }
    let length = header
        .lines()
        .find_map(|line| {
            line.split_once(':').and_then(|(key, value)| {
                key.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
        })
        .ok_or("缺少 Content-Length")?;
    if length == 0 || length > 16_384 {
        return Err("采集数据大小不合法".into());
    }
    while bytes.len() - header_end < length {
        let n = stream.read(&mut chunk).map_err(|e| e.to_string())?;
        if n == 0 || bytes.len() + n > 32_768 {
            return Err("采集数据未完整送达".into());
        }
        bytes.extend_from_slice(&chunk[..n]);
    }
    Ok(Some(bytes[header_end..header_end + length].to_vec()))
}

fn reply(stream: &mut TcpStream, status: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

fn save_capture(c: &Connection, product_id: i64, sku: &str, data: CapturePayload) -> Result<(), String> {
    ensure(c)?;
    c.execute(
        "INSERT INTO competitor_shop_seerfar_capture(product_id,sku,seller_price_rub,weight_g,dimensions_mm,category,stock,seller,listing_date,image_url)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
         ON CONFLICT(product_id) DO UPDATE SET sku=excluded.sku,seller_price_rub=excluded.seller_price_rub,weight_g=excluded.weight_g,dimensions_mm=excluded.dimensions_mm,category=excluded.category,stock=excluded.stock,seller=excluded.seller,listing_date=excluded.listing_date,image_url=excluded.image_url,captured_at=CURRENT_TIMESTAMP",
        params![product_id, sku, data.seller_price_rub, data.weight_g, limit(data.dimensions_mm, 100), limit(data.category, 200), data.stock, limit(data.seller, 200), limit(data.listing_date, 40), limit(data.image_url, 1000)],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

fn serve_once(listener: TcpListener, state: AppState, product_id: i64, sku: String, url: String, token: String) {
    if listener.set_nonblocking(true).is_err() {
        return;
    }
    let expires = Instant::now() + Duration::from_secs(90);
    while Instant::now() < expires {
        match listener.accept() {
            Ok((mut stream, _)) => {
                match receive_request(&mut stream) {
                    Ok(None) => reply(&mut stream, "204 No Content", ""),
                    Ok(Some(body)) => {
                        let parsed = serde_json::from_slice::<CapturePayload>(&body);
                        let result = parsed.map_err(|e| e.to_string()).and_then(|data| {
                            if !valid_payload(&data, &token, &sku, &url) {
                                return Err("SKU、商品链接或一次性令牌不匹配".into());
                            }
                            let c = db(&state)?;
                            save_capture(&c, product_id, &sku, data)
                        });
                        match result {
                            Ok(()) => { reply(&mut stream, "200 OK", "saved"); return; }
                            Err(message) => reply(&mut stream, "400 Bad Request", &message),
                        }
                    }
                    Err(message) => reply(&mut stream, "400 Bad Request", &message),
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(100)),
            Err(_) => break,
        }
    }
}

#[tauri::command]
pub(crate) fn start_competitor_seerfar_capture(
    product_id: i64,
    state: State<AppState>,
) -> Result<String, String> {
    let c = db(&state)?;
    ensure(&c)?;
    let (sku, url): (String, String) = c
        .query_row(
            "SELECT sku,listing_url FROM competitor_shop_products WHERE id=?1",
            [product_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| "找不到该竞品商品".to_string())?;
    if ozon_product_id(&url).as_deref() != Some(&sku) {
        return Err("商品链接与 SKU 不对应，无法安全回填；请核对 Excel 链接".into());
    }
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("本机采集端口无法启动：{e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let token = Uuid::new_v4().simple().to_string();
    let capture_url = format!("{}#ozon-erp-capture={port}.{token}", url.split('#').next().unwrap_or(&url));
    let owned = crate::background_state(&state)?;
    thread::spawn(move || serve_once(listener, owned, product_id, sku, url, token));
    open::that(&capture_url).map_err(|e| format!("无法打开浏览器：{e}"))?;
    Ok("已打开 Ozon 页面；等待已安装的辅助扩展读取可见的 Seerfar 面板（最多 90 秒）。".into())
}

#[tauri::command]
pub(crate) fn competitor_seerfar_captures(
    shop_name: String,
    state: State<AppState>,
) -> Result<Vec<CaptureRow>, String> {
    let c = db(&state)?;
    ensure(&c)?;
    let mut stmt = c.prepare(
        "SELECT x.product_id,x.sku,x.seller_price_rub,x.weight_g,x.dimensions_mm,x.category,x.stock,x.seller,x.listing_date,x.image_url,x.captured_at
         FROM competitor_shop_seerfar_capture x JOIN competitor_shop_products p ON p.id=x.product_id WHERE p.shop_name=?1",
    ).map_err(|e| e.to_string())?;
    let captures = stmt.query_map([shop_name], |r| Ok(CaptureRow {
        product_id: r.get(0)?, sku: r.get(1)?, seller_price_rub: r.get(2)?, weight_g: r.get(3)?,
        dimensions_mm: r.get(4)?, category: r.get(5)?, stock: r.get(6)?, seller: r.get(7)?,
        listing_date: r.get(8)?, image_url: r.get(9)?, captured_at: r.get(10)?,
    }))
    .map_err(|e| e.to_string())?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())?;
    Ok(captures)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_matching_ozon_product() {
        let data = CapturePayload {
            token: "secret".into(), sku: "4983882608".into(),
            url: "https://www.ozon.ru/product/4983882608/?x=1".into(),
            seller_price_rub: Some(209.0), weight_g: Some(65.0), dimensions_mm: None,
            category: None, stock: None, seller: None, listing_date: None, image_url: None,
        };
        assert!(valid_payload(&data, "secret", "4983882608", "https://www.ozon.ru/product/4983882608"));
        assert!(!valid_payload(&data, "wrong", "4983882608", "https://www.ozon.ru/product/4983882608"));
        assert!(!valid_payload(&data, "secret", "4983882608", "https://www.ozon.ru/product/1111111111"));
        assert_eq!(ozon_product_id("https://evil.com/product/4983882608"), None);
    }

    #[test]
    fn saves_visible_fields_without_changing_excel_product() {
        let c = Connection::open_in_memory().unwrap();
        let data = CapturePayload {
            token: "secret".into(), sku: "4983882608".into(),
            url: "https://www.ozon.ru/product/4983882608".into(),
            seller_price_rub: Some(248.0), weight_g: Some(65.0),
            dimensions_mm: Some("150×150×20mm".into()),
            category: Some("淋浴喷头".into()), stock: Some(749),
            seller: Some("529GHKJ".into()), listing_date: Some("2026-07-07".into()),
            image_url: None,
        };
        save_capture(&c, 1, "4983882608", data).unwrap();
        let captured: (f64, f64, String, i64) = c.query_row(
            "SELECT seller_price_rub,weight_g,dimensions_mm,stock FROM competitor_shop_seerfar_capture WHERE product_id=1",
            [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)),
        ).unwrap();
        assert_eq!(captured, (248.0, 65.0, "150×150×20mm".into(), 749));
    }
}
