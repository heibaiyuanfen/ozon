use base64::{engine::general_purpose::STANDARD, Engine};
use std::{ffi::c_void, ptr};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB},
};

pub fn protect(value: &str) -> Result<String, String> {
    if value.is_empty() {
        return Ok(String::new());
    }
    let mut bytes = value.as_bytes().to_vec();
    let source = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_mut_ptr(),
    };
    let mut target = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    };
    let description: Vec<u16> = "OzonAnalytics\0".encode_utf16().collect();
    let ok = unsafe {
        CryptProtectData(
            &source,
            description.as_ptr(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            0,
            &mut target,
        )
    };
    if ok == 0 {
        return Err(format!(
            "DPAPI 加密失败：{}",
            std::io::Error::last_os_error()
        ));
    }
    let encrypted =
        unsafe { std::slice::from_raw_parts(target.pbData, target.cbData as usize).to_vec() };
    unsafe {
        LocalFree(target.pbData as *mut c_void);
    }
    Ok(format!("dpapi:{}", STANDARD.encode(encrypted)))
}

pub fn unprotect(value: &str) -> Result<String, String> {
    if value.is_empty() {
        return Ok(String::new());
    }
    if let Some(plain) = value.strip_prefix("plain:") {
        return Ok(plain.to_string());
    }
    let encoded = match value.strip_prefix("dpapi:") {
        Some(v) => v,
        None => return Ok(value.to_string()),
    };
    let mut encrypted = STANDARD.decode(encoded).map_err(|e| e.to_string())?;
    let source = CRYPT_INTEGER_BLOB {
        cbData: encrypted.len() as u32,
        pbData: encrypted.as_mut_ptr(),
    };
    let mut target = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    };
    let ok = unsafe {
        CryptUnprotectData(
            &source,
            ptr::null_mut(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            0,
            &mut target,
        )
    };
    if ok == 0 {
        return Err(format!(
            "DPAPI 解密失败：{}",
            std::io::Error::last_os_error()
        ));
    }
    let clear =
        unsafe { std::slice::from_raw_parts(target.pbData, target.cbData as usize).to_vec() };
    unsafe {
        LocalFree(target.pbData as *mut c_void);
    }
    String::from_utf8(clear).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dpapi_roundtrip() {
        let value = "ozon-密钥-roundtrip";
        let encrypted = protect(value).unwrap();
        assert!(encrypted.starts_with("dpapi:"));
        assert_eq!(unprotect(&encrypted).unwrap(), value);
    }
}
