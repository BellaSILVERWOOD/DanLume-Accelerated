//! 密码加密：与旧版 `lumina/secure.py` **完全同一种格式**，
//! 所以旧库里已经存好的 SMTP / IMAP / API Key 能直接解开，反之亦然：
//!
//! * `dpapi:<base64>`：Windows DPAPI 密文（CryptProtectData）
//! * `b64:<base64>`：DPAPI 不可用时的降级（只是编码，不是加密）
//! * 其它：老数据里的明文，原样返回
const DPAPI_PREFIX: &str = "dpapi:";
const B64_PREFIX: &str = "b64:";

#[cfg(windows)]
pub fn protect(secret: &str) -> Result<String, String> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    if secret.is_empty() {
        return Ok(String::new());
    }
    let bytes = secret.as_bytes();
    let mut input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let ok = unsafe {
        CryptProtectData(
            &mut input,
            PCWSTR::null(),
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok.is_err() {
        // 与旧版一致：DPAPI 不可用时降级成 base64
        return Ok(format!("{B64_PREFIX}{}", base64_encode(bytes)));
    }
    let slice = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) };
    let encoded = base64_encode(slice);
    unsafe {
        let _ = LocalFree(HLOCAL(output.pbData as *mut core::ffi::c_void));
    }
    Ok(format!("{DPAPI_PREFIX}{encoded}"))
}

#[cfg(windows)]
pub fn unprotect(payload: &str) -> Result<String, String> {
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    if payload.is_empty() {
        return Ok(String::new());
    }
    // 旧格式的明文（很老的库）原样返回
    let Some(body) = payload.strip_prefix(DPAPI_PREFIX) else {
        if let Some(encoded) = payload.strip_prefix(B64_PREFIX) {
            let raw = base64_decode(encoded)?;
            return Ok(String::from_utf8_lossy(&raw).to_string());
        }
        return Ok(payload.to_string());
    };
    let mut raw = base64_decode(body)?;
    let mut input = CRYPT_INTEGER_BLOB {
        cbData: raw.len() as u32,
        pbData: raw.as_mut_ptr(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let ok = unsafe {
        CryptUnprotectData(
            &mut input,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok.is_err() {
        return Err("解密失败（不是这台机器 / 这个 Windows 用户加密的）".to_string());
    }
    let slice = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) };
    let text = String::from_utf8_lossy(slice).to_string();
    unsafe {
        let _ = LocalFree(HLOCAL(output.pbData as *mut core::ffi::c_void));
    }
    Ok(text)
}

#[cfg(not(windows))]
pub fn protect(secret: &str) -> Result<String, String> {
    Ok(format!("plain:{secret}"))
}

#[cfg(not(windows))]
pub fn unprotect(payload: &str) -> Result<String, String> {
    Ok(payload.trim_start_matches("plain:").to_string())
}

// ------------------------------------------------------------ base64（免依赖）

const ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// 通用 base64 编码（邮件内联图片也用这个转 data URL）
pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[((triple >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((triple >> 12) & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[((triple >> 6) & 0x3f) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(triple & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn base64_decode(text: &str) -> Result<Vec<u8>, String> {
    let mut bits = 0u32;
    let mut count = 0u32;
    let mut out = Vec::new();
    for ch in text.bytes() {
        if ch == b'=' || ch.is_ascii_whitespace() {
            continue;
        }
        let value = ALPHABET
            .iter()
            .position(|c| *c == ch)
            .ok_or_else(|| "密文不是合法的 base64".to_string())? as u32;
        bits = (bits << 6) | value;
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push(((bits >> count) & 0xff) as u8);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_roundtrip() {
        for sample in ["", "a", "ab", "abc", "中文密钥🔑"] {
            let encoded = base64_encode(sample.as_bytes());
            let decoded = base64_decode(&encoded).unwrap();
            assert_eq!(String::from_utf8_lossy(&decoded), sample);
        }
    }
}
