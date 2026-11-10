//! 管理员密钥与信封（对应旧版 `adminsign.py`）。
//!
//! 两种东西必须是**跨语言一致**的，否则 Python 版发出的申请 Rust 这边解不开：
//!
//! 1. 注册申请（申请人 → 管理员）
//!    ```text
//!    LUMINA-SEAL1. + base64url(json{ n: nonce12, c: AES-GCM 密文, k: RSA 包起来的 key })
//!    ```
//!    * AES-GCM-256，key 32 字节随机，nonce 12 字节，无 AAD；
//!    * 外面用 RSA-2048 **OAEP + MGF1-SHA1**（注意是 SHA-1，不是 SHA-256）；
//!    * 明文是 `serde_json::to_string`（键按字典序，与 Python `sort_keys=True` 等价）。
//! 2. 批准回执（管理员 → 申请人）
//!    ```text
//!    LUMINA-RECEIPT1. + base64url(json payload) + "." + base64url(RSA-PSS 签名)
//!    ```
//!    * 签名算法 RSA-PSS + MGF1-SHA256，盐长度取上限；
//!    * 签的是**那段 base64url 文本本身**，不是原始 JSON。
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::{STANDARD, URL_SAFE};
use base64::Engine;
use rsa::pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey};
use rsa::traits::PublicKeyParts;
use rsa::{Oaep, RsaPrivateKey, RsaPublicKey};
use serde_json::{json, Value};

use crate::store::Store;

pub const SEAL_PREFIX: &str = "LUMINA-SEAL1.";
pub const RECEIPT_PREFIX: &str = "LUMINA-RECEIPT1.";
/// 私钥（DPAPI 密文）在 settings 里的键名，与旧版一致
pub const SEAL_KEY: &str = "admin_private_key";
pub const KEY_BITS: usize = 2048;

pub type SignResult<T> = Result<T, String>;

fn err(message: impl Into<String>) -> String {
    message.into()
}

// ------------------------------------------------------------------ 密钥
pub fn has_private_key(store: &Store) -> bool {
    private_pem(store).map(|one| !one.is_empty()).unwrap_or(false)
}

/// 读私钥 PEM（先读 DPAPI 密文，再回落到数据目录里的 pem 文件）
pub fn private_pem(store: &Store) -> SignResult<String> {
    if let Some(encrypted) = store.setting(SEAL_KEY) {
        if !encrypted.trim().is_empty() {
            if let Ok(pem) = crate::secrets::unprotect(&encrypted) {
                if !pem.trim().is_empty() {
                    return Ok(pem);
                }
            }
        }
    }
    let path = crate::paths::data_dir().join("admin_private.pem");
    if let Ok(text) = std::fs::read_to_string(&path) {
        let text = text.trim().to_string();
        if !text.is_empty() {
            // 顺手塞回 DPAPI，下次直接读
            if let Ok(protected) = crate::secrets::protect(&text) {
                let _ = store.set_setting(SEAL_KEY, &protected);
            }
            return Ok(text);
        }
    }
    Err(err("这台机器上还没有管理员密钥（先到「账户审核」里生成）"))
}

fn private_key(store: &Store) -> SignResult<RsaPrivateKey> {
    let pem = private_pem(store)?;
    RsaPrivateKey::from_pkcs8_pem(&pem)
        .map_err(|exc| format!("管理员密钥读不出来：{exc}"))
}

/// 公钥 PEM：有私钥就用私钥推导，否则读 admin_pub.pem
pub fn public_pem(store: &Store) -> String {
    if let Ok(key) = private_key(store) {
        if let Ok(pem) = RsaPublicKey::from(&key).to_public_key_pem(Default::default()) {
            return pem.trim().to_string();
        }
    }
    for path in [
        crate::paths::data_dir().join("admin_pub.pem"),
        crate::paths::data_dir().join("../admin_pub.pem"),
    ] {
        if let Ok(text) = std::fs::read_to_string(&path) {
            let text = text.trim().to_string();
            if !text.is_empty() {
                return text;
            }
        }
    }
    String::new()
}

fn public_key(store: &Store) -> SignResult<RsaPublicKey> {
    let pem = public_pem(store);
    if pem.is_empty() {
        return Err(err("还没有管理员公钥"));
    }
    RsaPublicKey::from_public_key_pem(&pem).map_err(|exc| format!("公钥读不出来：{exc}"))
}

/// 公钥指纹（SHA-256 前 16 位，人眼核对用）
pub fn fingerprint(store: &Store) -> String {
    use sha2::{Digest, Sha256};
    let pem = public_pem(store);
    if pem.is_empty() {
        return String::new();
    }
    let digest = Sha256::digest(pem.as_bytes());
    digest.iter().take(8).map(|one| format!("{one:02x}")).collect()
}

/// 生成一对新密钥（旧版 `generate_keypair`），返回公钥指纹
pub fn generate_keypair(store: &Store) -> SignResult<String> {
    let mut rng = rand::thread_rng();
    let key = RsaPrivateKey::new(&mut rng, KEY_BITS)
        .map_err(|exc| format!("生成密钥失败：{exc}"))?;
    let private_pem = key
        .to_pkcs8_pem(Default::default())
        .map_err(|exc| format!("导出私钥失败：{exc}"))?
        .to_string();
    let public_pem = RsaPublicKey::from(&key)
        .to_public_key_pem(Default::default())
        .map_err(|exc| format!("导出公钥失败：{exc}"))?;
    let protected = crate::secrets::protect(&private_pem)?;
    store
        .set_setting(SEAL_KEY, &protected)
        .map_err(|exc| format!("存私钥失败：{exc}"))?;
    let _ = std::fs::write(crate::paths::data_dir().join("admin_pub.pem"), &public_pem);
    Ok(fingerprint(store))
}

// ------------------------------------------------------------------ 信封
/// 用管理员公钥加密一份申请（申请人侧的逻辑，v2 自己也用来做自测）
pub fn seal(store: &Store, payload: &Value) -> SignResult<String> {
    let public = public_key(store)?;
    let mut rng = rand::thread_rng();
    let aes_key: [u8; 32] = rand::Rng::gen(&mut rng);
    let nonce: [u8; 12] = rand::Rng::gen(&mut rng);
    let plaintext = serde_json::to_vec(payload).map_err(|exc| format!("序列化失败：{exc}"))?;
    let cipher = Aes256Gcm::new_from_slice(&aes_key)
        .map_err(|exc| format!("AES 初始化失败：{exc}"))?;
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext.as_ref())
        .map_err(|exc| format!("加密失败：{exc}"))?;
    let wrapped = public
        .encrypt(&mut rng, Oaep::new::<sha1::Sha1>(), &aes_key)
        .map_err(|exc| format!("RSA 加密失败：{exc}"))?;
    let boxed = json!({
        "n": STANDARD.encode(nonce),
        "c": STANDARD.encode(ciphertext),
        "k": STANDARD.encode(wrapped),
    });
    Ok(format!(
        "{SEAL_PREFIX}{}",
        URL_SAFE.encode(boxed.to_string())
    ))
}

/// 解密一份申请
pub fn unseal(store: &Store, blob: &str) -> SignResult<Value> {
    let text = blob.trim();
    if !text.starts_with(SEAL_PREFIX) {
        return Err(err("这不是注册申请（缺少 LUMINA-SEAL1 标记）"));
    }
    let body = &text[SEAL_PREFIX.len()..];
    let decoded = URL_SAFE
        .decode(body.as_bytes())
        .map_err(|exc| format!("密文不是合法的 base64url：{exc}"))?;
    let boxed: Value =
        serde_json::from_slice(&decoded).map_err(|exc| format!("密文结构不对：{exc}"))?;
    let get = |key: &str| -> SignResult<Vec<u8>> {
        let raw = boxed
            .get(key)
            .and_then(|one| one.as_str())
            .ok_or_else(|| format!("密文缺少字段 {key}"))?;
        STANDARD
            .decode(raw.as_bytes())
            .map_err(|exc| format!("字段 {key} 不是合法 base64：{exc}"))
    };
    let nonce = get("n")?;
    let ciphertext = get("c")?;
    let wrapped = get("k")?;
    let key = private_key(store)?;
    let aes_key = key
        .decrypt(Oaep::new::<sha1::Sha1>(), &wrapped)
        .map_err(|exc| {
            format!("解不开这份申请：{exc}（可能不是用你这台机器的公钥加密的）")
        })?;
    let cipher = Aes256Gcm::new_from_slice(&aes_key)
        .map_err(|exc| format!("AES 初始化失败：{exc}"))?;
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&nonce), ciphertext.as_ref())
        .map_err(|_| err("解密失败（密文被改过？）"))?;
    serde_json::from_slice(&plaintext).map_err(|exc| format!("内容不是 JSON：{exc}"))
}

/// 从邮件正文里把那一行密文抠出来（客户端常把长 base64 折成多行）
pub fn extract_seal(text: &str) -> String {
    let raw: String = text.chars().filter(|ch| *ch != '\r').collect();
    let squeezed: String = raw.chars().filter(|ch| !ch.is_whitespace()).collect();
    if let Some(pos) = squeezed.find(SEAL_PREFIX) {
        let tail = &squeezed[pos + SEAL_PREFIX.len()..];
        let body: String = tail
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_' || *ch == '=')
            .collect();
        return format!("{SEAL_PREFIX}{body}");
    }
    String::new()
}

pub fn looks_like_seal(text: &str) -> bool {
    text.contains(SEAL_PREFIX)
}

// ------------------------------------------------------------------ 回执
/// 给批准回执签名
pub fn sign_receipt(store: &Store, payload: &Value) -> SignResult<String> {
    let key = private_key(store)?;
    let body = URL_SAFE.encode(payload.to_string());
    // 旧版用 padding.PSS.MAX_LENGTH：emLen - hLen - 2
    let max_salt = (key.size() - 32 - 2) as usize;
    let pss = rsa::pss::Pss::new_with_salt::<sha2::Sha256>(max_salt);
    // rsa 的 PSS 接口要的是「摘要」而不是原文（它会自己套 MGF1）
    use sha2::Digest;
    let digest = sha2::Sha256::digest(body.as_bytes());
    let signature = key
        .sign_with_rng(&mut rand::thread_rng(), pss, &digest)
        .map_err(|exc| format!("签名失败：{exc}"))?;
    Ok(format!(
        "{RECEIPT_PREFIX}{body}.{}",
        URL_SAFE.encode(signature)
    ))
}

/// 验回执；通过则返回里面的 JSON
pub fn verify_receipt(store: &Store, token: &str) -> SignResult<Value> {
    let text = token.trim();
    if !text.starts_with(RECEIPT_PREFIX) {
        return Err(err("这不是批准回执（缺少 LUMINA-RECEIPT1 标记）"));
    }
    let rest = &text[RECEIPT_PREFIX.len()..];
    let Some(pos) = rest.rfind('.') else {
        return Err(err("回执格式不对（少了签名）"));
    };
    let body = &rest[..pos];
    let signature = URL_SAFE
        .decode(rest[pos + 1..].as_bytes())
        .map_err(|exc| format!("签名不是合法 base64url：{exc}"))?;
    let public = public_key(store)?;
    let max_salt = (public.size() - 32 - 2) as usize;
    let pss = rsa::pss::Pss::new_with_salt::<sha2::Sha256>(max_salt);
    use sha2::Digest;
    let digest = sha2::Sha256::digest(body.as_bytes());
    public
        .verify(pss, &digest, &signature)
        .map_err(|_| err("回执签名不对（可能被改过，或者不是这位管理员签发的）"))?;
    let decoded = URL_SAFE
        .decode(body.as_bytes())
        .map_err(|exc| format!("回执内容不是合法 base64url：{exc}"))?;
    serde_json::from_slice(&decoded).map_err(|exc| format!("回执内容读不出来：{exc}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn real_store() -> Store {
        std::env::set_var(
            "LUMINA_DATA_DIR",
            r"E:\FIles\Lena‘s Files\Work\znw\Agent\data",
        );
        Store::open().expect("打不开数据库")
    }

    #[test]
    fn seal_roundtrip_with_real_keys() {
        let store = real_store();
        let pem = public_pem(&store);
        println!("公钥长度 = {}", pem.len());
        println!("指纹 = {}", fingerprint(&store));
        assert!(!pem.is_empty(), "应该能从旧库/admin_pub.pem 读到公钥");

        let payload = json!({
            "username": "冒烟测试",
            "email": "smoke@m.fudan.edu.cn",
            "device": "abc123",
            "nonce": "n-1",
            "ts": "2026-11-07 10:00",
        });
        let sealed = seal(&store, &payload).expect("加密失败");
        println!("密文前缀 = {}", &sealed[..20]);
        assert!(sealed.starts_with(SEAL_PREFIX));
        // 邮件折行也要能抠出来
        let folded = format!(
            "这是一封注册申请\n\n{}\n{}",
            &sealed[..36],
            &sealed[36..]
        );
        let extracted = extract_seal(&folded);
        assert_eq!(extracted, sealed);

        if !has_private_key(&store) {
            println!("本机没有私钥，跳过解密那半段");
            return;
        }
        let back = unseal(&store, &extracted).expect("解密失败");
        println!("解出来 = {back}");
        assert_eq!(back["username"], "冒烟测试");
        assert_eq!(back["nonce"], "n-1");

        // ---- 跨语言：让 Python 版也能解开 Rust 封的这封信，再让 Rust 解 Python 的 ----
        let temp = std::env::temp_dir();
        let rust_file = temp.join("danlume_rust_seal.txt");
        let _ = std::fs::write(&rust_file, &sealed);
        println!("Rust 密文写到 {}", rust_file.display());
        let py_file = temp.join("danlume_py_seal.txt");
        if let Ok(py_blob) = std::fs::read_to_string(&py_file) {
            let py_blob = py_blob.trim();
            if !py_blob.is_empty() {
                let py_back = unseal(&store, py_blob).expect("Python 封的信解不开");
                println!("Python 封的信 → {py_back}");
                assert_eq!(py_back["username"], "python 封的");
            }
        } else {
            println!("（还没有 Python 封的信，跳过跨语言那半段）");
        }
    }

    #[test]
    fn receipt_roundtrip() {
        let store = real_store();
        if !has_private_key(&store) {
            println!("本机没有私钥，跳过");
            return;
        }
        let payload = json!({ "username": "冒烟测试", "code": "123456" });
        let token = sign_receipt(&store, &payload).expect("签名失败");
        println!("回执 = {}…", &token[..30]);
        let back = verify_receipt(&store, &token).expect("验签失败");
        assert_eq!(back["code"], "123456");
    }
}
