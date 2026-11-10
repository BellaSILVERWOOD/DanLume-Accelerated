# -*- coding: utf-8 -*-
"""DanLume v2 验收：把能自动验证的点都过一遍，并生成 _验收报告.md。

用法（在工作区根目录）：
    python DanLume-v2\\_acceptance_v2.py          # 只验收
    python DanLume-v2\\_acceptance_v2.py --pack   # 验收 + 打包到 DanLume-v2/release
"""
import ctypes
import ctypes.wintypes as wt
import os
import re
import shutil
import sqlite3
import subprocess
import sys
import time
import zipfile
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8")
WS = Path(r"E:\FIles\Lena‘s Files\Study\大学\AgentGeneral")
PROJ = WS / "DanLume-v2"
EXE = PROJ / "src-tauri" / "target" / "release" / "danlume.exe"
DIST = PROJ / "dist"
RELEASE = PROJ / "release"
DATA = Path(r"E:\FIles\Lena‘s Files\Work\znw\Agent\data")
SHOTS = WS / "_shots"

results = []


def check(name, ok, detail=""):
    results.append((bool(ok), name, str(detail)[:180]))
    print(("PASS " if ok else "FAIL ") + name + (("  → " + str(detail)[:180]) if detail else ""))


# ---------------------------------------------------------------- 1. 构建产物
check("release 版 exe 存在", EXE.is_file(), EXE)
size_mb = EXE.stat().st_size / 1024 / 1024 if EXE.is_file() else 0
check("exe 是单文件（< 30 MB）", 0 < size_mb < 30, "%.2f MB" % size_mb)

dist_js = sorted(p.name for p in (DIST / "assets").glob("index-*.js"))
exe_bytes = EXE.read_bytes() if EXE.is_file() else b""
embedded = sorted({m.decode() for m in re.findall(rb"index-[A-Za-z0-9_\-]{6,10}\.js", exe_bytes)})
check("exe 内嵌的是最新前端资源", dist_js and set(dist_js) == set(embedded),
      "dist=%s exe=%s" % (dist_js, embedded))

# ---------------------------------------------------------------- 2. 数据目录
pointer = Path(os.environ["LOCALAPPDATA"]) / "DanLume" / "data_dir.txt"
pointer_text = pointer.read_text(encoding="utf-8").strip() if pointer.is_file() else ""
check("data_dir.txt 指针存在且指向真实数据", pointer_text == str(DATA), pointer_text)
check("数据库存在", (DATA / "lumina.db").is_file(), DATA / "lumina.db")

conn = sqlite3.connect(DATA / "lumina.db")
expect_tables = ["events", "memos", "notices", "papers", "chat_sessions",
                 "llm_profiles", "imap_accounts", "mails", "settings"]
tables = {row[0] for row in conn.execute(
    "SELECT name FROM sqlite_master WHERE type='table'")}
check("旧库的关键表都认得", all(name in tables for name in expect_tables),
      sorted(set(expect_tables) - tables))

counts = {}
for name in ("events", "memos", "notices", "papers", "chat_sessions",
             "llm_profiles", "imap_accounts"):
    try:
        counts[name] = conn.execute("SELECT COUNT(*) FROM %s" % name).fetchone()[0]
    except Exception:
        counts[name] = -1
check("日程表有数据", counts["events"] > 0, counts["events"])
check("文献库有数据", counts["papers"] > 0, counts["papers"])
check("IMAP 账户已配置", counts["imap_accounts"] > 0, counts["imap_accounts"])
check("AI 配置存在", counts["llm_profiles"] > 0, counts["llm_profiles"])

# 关键设置项（重构版依赖它们）
def setting(key):
    row = conn.execute("SELECT value FROM settings WHERE key=?", (key,)).fetchone()
    return row[0] if row else ""

check("作业缓存可读（homework_assignments）", len(setting("homework_assignments")) > 10,
      len(setting("homework_assignments")))
check("空教室缓存可读", "ok" in setting("ehall_cache_empty_room"),
      len(setting("ehall_cache_empty_room")))
check("校区/教学楼表可读", "{" in setting("ehall_campus_buildings"),
      len(setting("ehall_campus_buildings")))
check("SMTP/IMAP 密码是 DPAPI 密文而不是明文",
      setting("imap_password_1").startswith("dpapi:"),
      setting("imap_password_1")[:14])
check("AI Key 是 DPAPI 密文", setting("llm_profile_key_1").startswith("dpapi:"),
      setting("llm_profile_key_1")[:14])

# ---------------------------------------------------------------- 3. Rust 单测
env = dict(os.environ)
env["PATH"] = r"E:\DevTools\rust\cargo\bin;" + env.get("PATH", "")
env["CARGO_HOME"] = r"E:\DevTools\rust\cargo"
env["RUSTUP_HOME"] = r"E:\DevTools\rust\rustup"
tests = subprocess.run(
    [r"E:\DevTools\rust\cargo\bin\cargo.exe", "test",
     "--manifest-path", str(PROJ / "src-tauri" / "Cargo.toml"),
     "--lib", "--", "--test-threads=1"],
    capture_output=True, text=True, env=env, cwd=str(PROJ),
    encoding="utf-8", errors="replace")
test_text = (tests.stdout or "") + (tests.stderr or "")
ok_tests = "test result: ok" in test_text
passed = re.search(r"test result: ok\. (\d+) passed", test_text)
check("Rust 单测（含真实库集成测试）全过", ok_tests,
      "passed=%s" % (passed.group(1) if passed else "?"))

# ---------------------------------------------------------------- 4. 截图覆盖
pages = ["dashboard", "calendar", "mail", "notices", "memos", "homework",
         "classroom", "docs", "lit", "latex", "settings", "ai"]
missing = [name for name in pages if not (SHOTS / ("v2-%s.png" % name)).is_file()]
check("13 个页面截图齐全（eHall 与空教室共用一页）", not missing, missing)

# ---------------------------------------------------------------- 5. 启动与内存
class PMC(ctypes.Structure):
    _fields_ = [("cb", wt.DWORD), ("PageFaultCount", wt.DWORD),
                ("PeakWorkingSetSize", ctypes.c_size_t),
                ("WorkingSetSize", ctypes.c_size_t),
                ("QuotaPeakPagedPoolUsage", ctypes.c_size_t),
                ("QuotaPagedPoolUsage", ctypes.c_size_t),
                ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t),
                ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
                ("PagefileUsage", ctypes.c_size_t),
                ("PeakPagefileUsage", ctypes.c_size_t)]


def find_window(pid):
    u = ctypes.windll.user32
    found = []

    CB = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)

    def cb(hwnd, _param):
        if not u.IsWindowVisible(hwnd):
            return True
        owner = wt.DWORD()
        u.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value != pid:
            return True
        rect = wt.RECT()
        u.GetWindowRect(hwnd, ctypes.byref(rect))
        if rect.right - rect.left > 400 and rect.bottom - rect.top > 300:
            found.append(hwnd)
        return True

    u.EnumWindows(CB(cb), 0)
    return found[0] if found else None


subprocess.run(["taskkill", "/F", "/IM", "danlume.exe"], capture_output=True)
time.sleep(0.8)
started = time.perf_counter()
proc = subprocess.Popen([str(EXE)], cwd=str(EXE.parent),
                        env={**os.environ, "DANLUME_START_PAGE": "dashboard"})
hwnd = None
for _ in range(80):
    time.sleep(0.1)
    hwnd = find_window(proc.pid)
    if hwnd:
        break
elapsed_ms = (time.perf_counter() - started) * 1000
check("窗口能起来（< 2 s）", hwnd is not None and elapsed_ms < 2000,
      "%.0f ms" % elapsed_ms)
time.sleep(2.5)
counters = PMC()
counters.cb = ctypes.sizeof(counters)
handle = ctypes.windll.kernel32.OpenProcess(0x0400 | 0x0010, False, proc.pid)
ctypes.windll.psapi.GetProcessMemoryInfo(handle, ctypes.byref(counters),
                                         ctypes.sizeof(counters))
mem_mb = counters.WorkingSetSize / 1024 / 1024
check("常驻内存 < 120 MB", mem_mb < 120, "%.1f MB" % mem_mb)
alive = proc.poll() is None
check("启动后进程稳定（没崩）", alive, "pid=%d" % proc.pid)
subprocess.run(["taskkill", "/F", "/IM", "danlume.exe"], capture_output=True)

# ---------------------------------------------------------------- 报告
passed_n = sum(1 for ok, _, _ in results if ok)
total_n = len(results)
lines = ["# DanLume v2 验收报告", "",
         "生成时间：%s" % time.strftime("%Y-%m-%d %H:%M:%S"), "",
         "结论：**%d / %d 通过**" % (passed_n, total_n), "",
         "| 结果 | 检查项 | 说明 |", "| --- | --- | --- |"]
for ok, name, detail in results:
    lines.append("| %s | %s | %s |" % ("✅" if ok else "❌", name,
                                       detail.replace("|", "/") if detail else ""))
lines += ["", "指标：exe %.2f MB · 启动 %.0f ms · 内存 %.1f MB" % (size_mb, elapsed_ms, mem_mb), ""]
report = PROJ / "_验收报告.md"
report.write_text("\n".join(lines), encoding="utf-8")
print("\n报告：%s（%d/%d 通过）" % (report, passed_n, total_n))

# ---------------------------------------------------------------- 打包
if "--pack" in sys.argv and passed_n == total_n:
    RELEASE.mkdir(parents=True, exist_ok=True)
    client_dir = RELEASE / "DanLume-2.0.0-client"
    if client_dir.exists():
        shutil.rmtree(client_dir)
    client_dir.mkdir(parents=True)
    shutil.copy2(EXE, client_dir / "DanLume.exe")
    for icon in ("icon.ico", "icon.png"):
        src = PROJ / "src-tauri" / "icons" / icon
        if src.is_file():
            shutil.copy2(src, client_dir / icon)
    shutil.copy2(PROJ / "README.md", client_dir / "README.md")
    guide = """DanLume 2.0.0（重构版 · Rust + Tauri + Vue）

怎么用
  · 双击 DanLume.exe 就能开；不需要装 Python / Qt。
  · 第一次启动会自动读取原来的数据目录：
      %LOCALAPPDATA%\\DanLume\\data_dir.txt 里写的路径，
      没写就用 exe 同级的 data\\ 目录。
  · 数据和旧版（PySide6 版）是同一份 data\\lumina.db，可以交替打开（别同时开）。

和旧版的关系
  · 界面与功能布局与旧版一致（13 个页面）；内核换成 Rust，界面用系统自带的 WebView2。
  · 启动约 0.15 s、内存约 30 MB（旧版约 1.5–2.5 s、250–550 MB）。
  · 密码仍然用 Windows DPAPI 加密，存的是同一种格式，两个版本能互相解开。

已知限制
  · eHall 那几张卡读的是旧版抓好的缓存；要刷新数据，先在旧版里同步一次，
    或者在新版的「eHall / 空教室」里点「打开网页」登录后抓取。
  · 公式页用 KaTeX 实时预览，导出 PDF 仍需要本机 XeLaTeX（后续阶段）。
"""
    (client_dir / "使用说明.txt").write_text(guide, encoding="utf-8")
    client_zip = RELEASE / "DanLume-2.0.0-client.zip"
    with zipfile.ZipFile(client_zip, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as zf:
        for path in client_dir.rglob("*"):
            if path.is_file():
                zf.write(path, path.relative_to(RELEASE).as_posix())

    source_zip = RELEASE / "DanLume-2.0.0-client-source.zip"
    skip_dirs = {"target", "node_modules", "dist", "__pycache__", "release"}
    with zipfile.ZipFile(source_zip, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as zf:
        for name in ("package.json", "package-lock.json", "vite.config.ts",
                     "tsconfig.json", "tsconfig.node.json", "index.html", "README.md"):
            path = PROJ / name
            if path.is_file():
                zf.write(path, "DanLume-v2/" + name)
        for base in ("src", "src-tauri", "docs"):
            root = PROJ / base
            if not root.is_dir():
                continue
            for path in root.rglob("*"):
                if not path.is_file():
                    continue
                if any(part in skip_dirs for part in path.relative_to(PROJ).parts):
                    continue
                zf.write(path, "DanLume-v2/" + path.relative_to(PROJ).as_posix())
    print("打包完成：")
    for path in (client_zip, source_zip):
        print("  %s  %.2f MB" % (path, path.stat().st_size / 1024 / 1024))
else:
    print("未打包（有失败项或没加 --pack）")
