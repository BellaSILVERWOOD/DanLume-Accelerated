# DanLume 2.0 · macOS 适配分支（保留 Vue 3）

本目录基于用户提供的 DanLume-v2.zip 修改，不是原作者发布的 macOS 正式版。

## 技术取舍

- **不换 React**：保留原有 Vue 3 + Vite + Tauri 2，最小化界面改动。
- **图标**：参考 [Lucide Vue](https://github.com/lucide-icons/lucide)，只修改 `IconGlyph.vue`/`icons.ts`，保留原调用参数；增加 `lucide-vue-next` 依赖。相比重新绘制几十个 SVG，更便于长期维护，但图标线条风格会稍有不同。
- **凭据**：参考 [keyring-rs](https://github.com/open-source-cooperative/keyring-rs)，macOS 用 Keychain，SQLite 只保存 `keychain:<随机ID>` 引用；增加仅 macOS 依赖 `keyring = 3.6.3`。Windows 仍使用 DPAPI；Linux 存储行为本阶段未改。
- **文件与数据**：macOS 文件交给 `open`，数据库放 `~/Library/Application Support/DanLume/data/`。不更换 SQLite，不重写页面。
- **构建**：保留 Windows 的 NSIS 默认配置，Mac 由额外配置输出 `.app`。

## 第一次在 macOS 上运行

1. 安装 Xcode Command Line Tools（`xcode-select --install`）、Node.js 与 Rust（`rustup`）。
2. 在本目录运行 `npm install`（**不是** `npm ci`：离线环境未能更新 package-lock.json，首次安装需重新生成锁文件）。
3. 运行 `npm run mac:icons`，生成 `src-tauri/icons/icon.icns`。
4. 建立测试数据目录并启动：

```bash
export LUMINA_DATA_DIR="$HOME/Library/Application Support/DanLume-macos-test/data"
mkdir -p "$LUMINA_DATA_DIR"
npm run build
npm run mac:dev
```

5. 另开终端运行 Rust 单测（需要在首次安装依赖后联网拉取新的 Cargo crates）：

```bash
cd src-tauri
cargo test
```

6. 开发版能运行后再打包：

```bash
npm run mac:build
```

Tauri 产物通常位于 `src-tauri/target/release/bundle/macos/DanLume.app`。

## 已知限制 / 安全提示

- 当前修改未在 macOS 上编译或实测，务必先运行上面的构建、基本 UI 与 Keychain 测试。
- **不要用真实密码做首次测试**。旧 Python/macOS 版可能把密码存成 `b64:`，原 Rust/Linux 版可能存成 `plain:`；这些历史格式仅能读取，**不会自动迁移到 Keychain**。建议清除旧版测试数据中的敏感字段后重新填写。
- 如果导入 Windows `.dlpk`，DPAPI 密钥不可在 Mac 解密，需要重新填写。
- macOS Keychain 存储失败时会直接报错，不会退化成明文；相关 Keychain 权限与多线程行为还需实机验证。
- 当前 Keychain 以随机凭据 ID 存储，修改凭据会留下旧 Keychain 项，后续可增加安全回收策略。
- 原版窗口无边框，自绘标题栏未改为 Mac 原生交通灯；不影响先验证主体功能。
- `window_list` 相关 Windows API 等仍需 Mac 专属实现；本次未扩展此能力。
- Tauri .app 在 Mac 上构建；未经开发者签名、公证的应用只能用于个人开发测试，不能视为正式发行。

## 修改范围与原版兼容

前端所有调用 `IconGlyph` 的地方保留，Windows `secrets.rs` 的 DPAPI 分支未修改，`tauri.conf.json` 的 Windows NSIS 配置也没有被覆盖。

### 源码变更文件

- `package.json`
- `src-tauri/Cargo.toml`
- `src-tauri/src/account.rs`
- `src-tauri/src/backup.rs`
- `src-tauri/src/docs.rs`
- `src-tauri/src/paths.rs`
- `src-tauri/src/secrets.rs`
- `src-tauri/src/uis.rs`
- `src/components/IconGlyph.vue`
- `src/icons.ts`

### 新增文件

- `MACOS_PORT.md`
- `src-tauri/tauri.macos.conf.json`
