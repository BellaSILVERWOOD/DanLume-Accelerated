# DanLume（旦光）

给复旦学生用的一体化桌面客户端：**日程 / 教务 / eLearning / 文献 / 公式 / 邮件 / AI 助手 / 学生工作**。

> 这份 README 面向「第一次拿到这个仓库的人」：它是什么、怎么跑起来、每个页面能干什么、
> 数据存在哪里、出问题去哪儿找文档。深入细节在 `_交接说明.md`、`_待办清单.md`、`_工作日志.md`。

---

## 1. 仓库里有什么

| 目录 | 是什么 | 技术栈 |
| --- | --- | --- |
| `DanLume-v2/` | **主力桌面端（Rust 版）** | Tauri 2 + Vue 3 + TypeScript + Rust |
| `_lumina/` | Python 桌面版（**功能基准**，历史最久） | PySide6 / Qt |
| `DanLume-server/` | 账号服务端（注册 / 登录 / 设备 / 公告 / 后台） | FastAPI + SQLite |
| `DanLume-Mobile/` | 手机端 | （见目录内 docs） |
| `_build/`、`_dist/` | Python 版打包副本与分发包 | — |

线上服务端：`https://43.108.99.38.sslip.io`（管理后台 `/admin`）。

---

## 2. 跑起来

### 2.1 桌面端（Rust）

```powershell
$env:CARGO_HOME="E:\DevTools\rust\cargo"; $env:RUSTUP_HOME="E:\DevTools\rust\rustup"
$env:Path="E:\DevTools\rust\cargo\bin;$env:Path"
cd DanLume-v2
npm install
npm run build                       # vue-tsc + vite（零错误才算过）
npm run tauri build -- --no-bundle  # 只出 exe，3–10 分钟
# 产物：src-tauri/target/release/danlume.exe
```

开发模式：`npm run tauri dev`（前端 `http://localhost:5273`，Rust 侧 devUrl 指的就是它）。

依赖：Node 20+ / Rust 1.77+ / Windows WebView2；**导出 PDF 需要本机装 MiKTeX（xelatex）**。

### 2.2 服务端

```powershell
cd DanLume-server
.\.venv\Scripts\python.exe -m pytest -q     # 54 passed
python run-dev.ps1                          # 本地起在 127.0.0.1:8000
```

⚠️ **uvicorn 只能单进程**：公告推送表与限流计数都在进程内存里，加 worker 会重复推送、限流失效。

### 2.3 Python 版

```powershell
cd _lumina
python -m compileall -q lumina
$env:LUMINA_DATA_DIR = Join-Path $env:TEMP ("smoke_" + [guid]::NewGuid().ToString("N").Substring(0,6))
python main.py --smoke-test                 # 用临时数据目录，别碰真实 data/
```

---

## 3. 功能一览

### 仪表板 / 日程
- 今天做什么一眼看完：即将到来、教务信息、最近作业、天气；eHall 卡片（校车 / 校园卡 / 体育 / 空教室）。
- **按分钟定位的日历**：左侧时间刻度、日程块按真实时长画高度，同时段多件事自动并排；重复日程只影响某一天。
- 提醒三按钮（知道了 / 稍后 10 分钟 / 标记完成），关窗后仍会补提醒。

### 教务 / eLearning / eHall / 空教室
- **教务课表**：在登录窗口里拦截 `print-data`，票和学期都由页面自己带，解析出 14 节作息的真实课表。
- **eLearning**：作业/截止日期、课件下载（带 Cookie 代理）、文件按课程自动归类到 `<下载目录>/<课程名>/`。
- **eHall / eLearning / 空教室全部是内嵌网页**，遇到登录页自动 UIS 登录；空教室页只显示结果（查询在隐藏页面里跑）。

### 邮箱
- 收件箱本地缓存（先读本地、再增量同步新 UID），正文按需拉取；退信识别；发件箱。
- **每封邮件顶端的 AI 摘要**：调用你自己配的接口，缓存进库（同一封只花一次 token）；
  顺手列出「可能要加进日程的事」，勾选后右下角「添加日程」一键建日程。

### 文献 / 公式
- 文献库：arXiv + Crossref + Semantic Scholar，分页 + 总量 + 多关键词按 AND；参考文献导入/导出（BibTeX / RIS / ENW / DOI）；EndNote 安装检测。
- 公式页：**像 Word 一样写**（文档视图 + LaTeX 源码双视图），排版预览 + 真 PDF 预览、**编辑即编译**（不弹黑窗）；
  Word 式快捷键（Ctrl+Z/Y/B/I/U/E/L/R/J/S、Alt+=）、**选中公式可剪切粘贴**、选中内容**一键转公式**（本地换算 / 让 AI 改写并给建议）。

### AI 助手（右栏）
- 三标签：API 服务 / WebUI 对话 / 系统提示词；场景提示词、历史会话（可改名删除）、token 用量、思考过程折叠。
- **流式输出** + **停止生成**；附件（文件卡、拖拽、粘贴）；**45+ 个工具**（日程、教务、文献、网页、文件、沙箱…）。
- **危险工具要你点头**（发邮件 / 改设置 / 跑命令 / 写文件 / 下载），可「允许一次 / 这次会话都允许 / 拒绝」。
- **Agent 沙箱**：AI 只能读写 `data/agent_sandbox/`，跑命令默认关闭、不限时长；**通讯录读取默认关闭**（勾选状态会记住）。

### 学生工作
- **通讯录**：姓名 / 职位 / 邮箱 / 手机号 / 微信备注名 / QQ / 其它联系方式，可搜索、增改删。
- **定时通知**：给指定的人、通过指定渠道（**邮箱** / 自定义 **webhook** / **微信**）、在指定时间发指定内容；
  支持只发一次 / 每天 / 每周，也能「立刻发」。
  - 微信没有可靠接口（个人号自动化属于外挂），所以是「整理好内容 + 一键复制」，你自己粘贴发送。

---

## 4. 账号与注册（2026-11-12 起）

- **第一次使用必须先注册 DanLume 服务器账号**：为了账号管理，也为了能把更新和公告及时推给你；
  注册页只在服务器连不上 / 连续失败时才给「先离线使用」的退路（否则服务器一宕大家软件都打不开）。
- 邮箱目前只放行 **`fudan.edu.cn` / `m.fudan.edu.cn`**（管理员账号走后台特例）。
- **忘记密码**：登录页「忘记密码？」→ 注册邮箱收验证码 → 设新口令
  （服务端 `/api/v1/password/request-code` + `/password/reset`，**这两个接口还需部署到线上**）。
- 普通用户**看不到管理员入口**；封禁 / 改口令 / 看设备都在网页后台 `/admin`（口令 + 可选 TOTP）。

> 应急开关：把设置 `account_gate_skip` 写成 `1` 可以跳过注册门（服务器故障或做演示时用）。

---

## 5. 数据放在哪

- 数据目录：`%APPDATA%\DanLume\data`（开发机上可能是 `Work\znw\Agent\data`），里面 `lumina.db`（SQLite）+ `downloads/`、`elearning/`、`agent_sandbox/`。
- 密码类一律 **DPAPI 加密**保存，界面只给「改」不给「看」。
- 备份：设置页可一键备份数据库；也可以整包导出 `.dlpk`（含 `memory.md`）。

---

## 6. 开发与验证

```powershell
# Rust：必须带 --test-threads=1（测试改进程级 LUMINA_DATA_DIR，并行会互相污染）
cd DanLume-v2\src-tauri
cargo test -- --test-threads=1      # 当前：168 passed / 0 failed / 28 ignored

# 前端类型检查 + 构建
cd DanLume-v2
npm run build

# 服务端
cd DanLume-server
.\.venv\Scripts\python.exe -m pytest -q     # 54 passed
```

- 铁律：改源码只用编辑工具（不要用 PowerShell 正则批量替换）；`App.vue`、`focus.rs` 历史上被这么改坏过。
- GitHub 附件上传**不能用 multipart**（会写坏文件），走 REST API 原始二进制上传。
- 子 Webview 永远在最上层：离开页面必须 `embedClose()`；`add_child` 不能在主线程调。

---

## 7. 文档索引

| 文件 | 内容 |
| --- | --- |
| `AGENTS.md` | 给接手的人：铁律、环境、命令、用户偏好 |
| `_交接说明.md` | 环境、构建 / 打包 / 发布、服务器信息、踩过的坑 |
| `_待办清单.md` | 还没做什么、需要你拍板什么（当前状态以它为准） |
| `_工作日志.md` | 每一轮做了什么、怎么验证的（只追加） |
| `_三端进度盘点.md` | 三端功能矩阵与代码量 |
| `_ACCEPTANCE.md` | 端到端验收清单 |

---

## 8. 已知限制

- 空教室 / 教务 / eHall 都依赖复旦内网身份，需要先 UIS 登录；eHall 部分页面有反爬，失败会提示。
- 微信相关能力只做到「整理内容 + 复制」（不碰个人号自动化）。
- 服务端公告是 5 秒轮询（WebSocket 探针可用，主程序未集成）。
- 手机端只做到「编译通过 + 签名 APK」，没做真机回归。
- 悬浮球（看板娘）还没做完：窗口能开，但小窗页面没加载出来（见 `_待办清单.md` T2）。
