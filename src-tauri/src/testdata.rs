//! 测试辅助（只在 `cargo test` 里编译）。
//!
//! 有几个用例是拿开发机上的一份**真实数据**来验证的（真实日程、真实 AI 配置、
//! 真实签名私钥、真实课件索引）。历史上的写法是把
//! `E:\...\Work\znw\Agent\data` 直接写死在测试里，于是：
//!
//! * 换一台机器、或者这个目录不存在，用例必然失败（2026-11-11 就这样失败了 11 个）；
//! * 更要命的是 `Store::open()` 是以读写方式打开库的（还要写 WAL、建表），
//!   等于拿用户的真实库当测试床 —— 万一用例里有写操作，就动了真实数据。
//!
//! 现在统一改成：先把真实库**复制**到临时目录，再打开副本；源目录只读。
//! 这些用例都标了 `#[ignore]`（默认不跑），需要时手动执行：
//!
//! ```text
//! cargo test -- --ignored --test-threads=1
//! ```

use std::path::PathBuf;

/// 开发机上的真实数据目录（只读来源）
pub const REAL_DATA_DIR: &str = r"E:\FIles\Lena‘s Files\Work\znw\Agent\data";

/// 把真实数据目录里的 `lumina.db`（连同 `-wal` / `-shm`、`memory.md`）复制到临时目录。
///
/// 源目录不存在或缺库时返回 `None`。临时目录带上进程 id，避免并行测试互相踩。
pub fn real_data_copy(tag: &str) -> Option<PathBuf> {
    let src = PathBuf::from(REAL_DATA_DIR);
    let db = src.join("lumina.db");
    if !db.is_file() {
        return None;
    }
    let dir = std::env::temp_dir().join(format!(
        "danlume-realdata-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).ok()?;
    let mut any = false;
    for name in ["lumina.db", "lumina.db-wal", "lumina.db-shm", "memory.md"] {
        let from = src.join(name);
        if from.is_file() {
            if std::fs::copy(&from, dir.join(name)).is_err() {
                return None;
            }
            any = true;
        }
    }
    if any {
        Some(dir)
    } else {
        None
    }
}

/// 给「真数据」用例用的 `Store`：数据来自副本，绝不碰用户的真实库。
pub fn real_store(tag: &str) -> crate::store::Store {
    let dir = real_data_copy(tag).unwrap_or_else(|| {
        panic!(
            "没有开发用的真实数据目录（{REAL_DATA_DIR}）。\n\
             这些用例标了 #[ignore]，默认不跑；在开发机上手动跑：\n\
             cargo test -- --ignored --test-threads=1"
        )
    });
    std::env::set_var("LUMINA_DATA_DIR", &dir);
    crate::store::Store::open().expect("真实数据副本打不开")
}

/// 打印一行「跳过」提示（真数据用例在没数据的机器上直接返回）。
pub fn skip(tag: &str) {
    println!("跳过 {tag}：这台机器上没有开发用的真实数据目录");
}
