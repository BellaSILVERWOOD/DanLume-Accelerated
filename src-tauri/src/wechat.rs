//! 直接读微信聊天（对应旧版 `wechat_read.py` / `wechat.py`）：找窗口 → 截图 → 系统 OCR → 清洗。
//!
//! 为什么不用 UI Automation：微信 4.x（`Weixin.exe`）是自绘界面，UIA 里基本读不到文字，
//! 旧版试过之后改成「截图 + Windows 自带 OCR」。
//!
//! 为什么截图也交给 PowerShell：C# 的 `System.Drawing` 在那边现成可用，Rust 侧就不用为了
//! 一次截图去引 GDI 依赖；脚本用 `-EncodedCommand`（UTF-16LE + base64）传，路径走环境变量，
//! 这样中文和引号都不会被命令行切坏（旧版就是这么做的）。
use std::path::PathBuf;
use std::process::Command;

/// Windows 的 CREATE_NO_WINDOW：跑 PowerShell 不弹黑框
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 微信窗口的进程名（4.x 是 Weixin.exe，老版是 WeChat.exe）
const PROCESS_NAMES: [&str; 2] = ["Weixin", "WeChat"];

/// 截图 + OCR 的整段脚本（做完就把识别到的文字写进 `$env:DANLUME_WECHAT_TXT`）
const GRAB_SCRIPT: &str = r##"
$ErrorActionPreference = "Stop"
$png = $env:DANLUME_WECHAT_PNG
$txt = $env:DANLUME_WECHAT_TXT

Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public class DanLumeGrab {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
  [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
}
"@

$proc = $null
foreach ($name in @("Weixin", "WeChat")) {
  $found = Get-Process -Name $name -ErrorAction SilentlyContinue |
    Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
  if ($found -ne $null) { $proc = $found; break }
}
if ($proc -eq $null) { Write-Output "NO_WINDOW"; exit 3 }

$hwnd = $proc.MainWindowHandle
$rect = New-Object DanLumeGrab+RECT
[DanLumeGrab]::GetWindowRect($hwnd, [ref]$rect) | Out-Null
$w = $rect.Right - $rect.Left
$h = $rect.Bottom - $rect.Top
# 最小化时窗口矩形会跑到 -32000 那种地方，尺寸也不对
if ($w -lt 300 -or $h -lt 300 -or $rect.Left -lt -10000 -or $rect.Top -lt -10000) {
  Write-Output "BAD_SIZE"; exit 4
}

$scale = 1.0
try {
  $dpi = [DanLumeGrab]::GetDpiForWindow($hwnd)
  if ($dpi -gt 0) { $scale = $dpi / 96.0 }
} catch { }

$bmp = New-Object System.Drawing.Bitmap $w, $h
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $g.GetHdc()
[DanLumeGrab]::PrintWindow($hwnd, $hdc, 2) | Out-Null
$g.ReleaseHdc($hdc)
$g.Dispose()

# 左边是会话列表、上面是标题、下面是输入框，只要中间的消息区
$x0 = [int](340 * $scale) + 6
if ($x0 -gt ($w - 120)) { $x0 = [Math]::Max(0, $w - 120) }
$y0 = [int](52 * $scale)
if ($y0 -gt ($h - 160)) { $y0 = [Math]::Max(0, $h - 160) }
$y1 = [Math]::Max($y0 + 80, $h - [int](62 * $scale))
$cw = $w - $x0
$ch = $y1 - $y0
if ($cw -lt 80 -or $ch -lt 60) { Write-Output "BAD_CROP"; exit 5 }

$crop = New-Object System.Drawing.Bitmap $cw, $ch
$cg = [System.Drawing.Graphics]::FromImage($crop)
$cg.DrawImage($bmp, (New-Object System.Drawing.Rectangle 0, 0, $cw, $ch),
  (New-Object System.Drawing.Rectangle $x0, $y0, $cw, $ch), [System.Drawing.GraphicsUnit]::Pixel)
$cg.Dispose()
$bmp.Dispose()

# 放大 2 倍：小字 OCR 更稳
$big = New-Object System.Drawing.Bitmap ($cw * 2), ($ch * 2)
$bg = [System.Drawing.Graphics]::FromImage($big)
$bg.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
$bg.DrawImage($crop, 0, 0, ($cw * 2), ($ch * 2))
$bg.Dispose()
$big.Save($png, [System.Drawing.Imaging.ImageFormat]::Png)
$big.Dispose()
$crop.Dispose()

# ---- Windows OCR（和中日韩语言包一起装的，不用第三方库）
Add-Type -AssemblyName System.Runtime.WindowsRuntime | Out-Null
$null = [Windows.Media.Ocr.OcrEngine, Windows.Foundation, ContentType = WindowsRuntime]
$null = [Windows.Storage.StorageFile, Windows.Storage, ContentType = WindowsRuntime]
$null = [Windows.Graphics.Imaging.BitmapDecoder, Windows.Graphics.Imaging, ContentType = WindowsRuntime]
$null = [Windows.Globalization.Language, Windows.Foundation, ContentType = WindowsRuntime]
$asTaskGeneric = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
  $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and
  $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' })[0]
function Await($op, $resultType) {
  $asTask = $asTaskGeneric.MakeGenericMethod($resultType)
  $task = $asTask.Invoke($null, @($op))
  $task.Wait(-1) | Out-Null
  $task.Result
}
$engine = $null
foreach ($tag in @("zh-Hans-CN", "zh-Hans", "en-US")) {
  try {
    $lang = New-Object Windows.Globalization.Language $tag
    $engine = [Windows.Media.Ocr.OcrEngine]::TryCreateFromLanguage($lang)
  } catch { $engine = $null }
  if ($engine -ne $null) { break }
}
if ($engine -eq $null) {
  $engine = [Windows.Media.Ocr.OcrEngine]::TryCreateFromUserProfileLanguages()
}
if ($engine -eq $null) { Write-Output "NO_ENGINE"; exit 2 }

$file = Await ([Windows.Storage.StorageFile]::GetFileFromPathAsync($png)) ([Windows.Storage.StorageFile])
$stream = Await ($file.OpenAsync([Windows.Storage.FileAccessMode]::Read)) ([Windows.Storage.Streams.IRandomAccessStream])
$decoder = Await ([Windows.Graphics.Imaging.BitmapDecoder]::CreateAsync($stream)) ([Windows.Graphics.Imaging.BitmapDecoder])
$bitmap = Await ($decoder.GetSoftwareBitmapAsync()) ([Windows.Graphics.Imaging.SoftwareBitmap])
$result = Await ($engine.RecognizeAsync($bitmap)) ([Windows.Media.Ocr.OcrResult])
$lines = New-Object System.Collections.Generic.List[string]
foreach ($line in $result.Lines) { $lines.Add($line.Text) }
[System.IO.File]::WriteAllText($txt, ($lines -join "`n"), (New-Object System.Text.UTF8Encoding $false))
$stream.Dispose()
Write-Output ("OK " + $lines.Count)
"##;

/// PowerShell 可执行文件（找不到就算了）
fn powershell_path() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(root) = std::env::var_os("SystemRoot") {
        let root = PathBuf::from(root);
        candidates.push(root.join("System32").join("WindowsPowerShell").join("v1.0").join("powershell.exe"));
        candidates.push(root.join("SysWOW64").join("WindowsPowerShell").join("v1.0").join("powershell.exe"));
    }
    candidates.push(PathBuf::from("powershell.exe"));
    candidates.into_iter().find(|path| {
        if path.components().count() == 1 {
            true // 交给 PATH 去找
        } else {
            path.exists()
        }
    })
}

/// 这台机器能不能直读微信（有没有 PowerShell）
pub fn available() -> bool {
    powershell_path().is_some()
}

fn temp_file(prefix: &str, ext: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|one| one.as_millis())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("danlume-wechat-{prefix}-{stamp}.{ext}"))
}

/// 把脚本按 UTF-16LE + base64 打包（PowerShell 的 `-EncodedCommand` 格式）
fn encode_command(script: &str) -> String {
    use base64::Engine;
    let mut bytes: Vec<u8> = Vec::with_capacity(script.len() * 2);
    for unit in script.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// 截当前微信聊天窗口并识别成文字（窗口在后台也能用，不抢焦点）
///
/// 失败返回一句人能看懂的话（找不到窗口 / 没 OCR 引擎 / 没识别出字）。
pub fn grab_current_chat() -> Result<String, String> {
    let exe = powershell_path().ok_or_else(|| "这台机器找不到 PowerShell".to_string())?;
    let png = temp_file("shot", "png");
    let txt = temp_file("text", "txt");
    let encoded = encode_command(GRAB_SCRIPT);
    let mut command = Command::new(exe);
    command.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-EncodedCommand",
        &encoded,
    ]);
    command.env("DANLUME_WECHAT_PNG", &png);
    command.env("DANLUME_WECHAT_TXT", &txt);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = command
        .output()
        .map_err(|exc| format!("叫不动 PowerShell：{exc}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let raw = std::fs::read_to_string(&txt).unwrap_or_default();
    // 临时文件用完就删（和旧版一样，不留垃圾）
    let _ = std::fs::remove_file(&png);
    let _ = std::fs::remove_file(&txt);

    if stdout.contains("NO_WINDOW") {
        return Err("没找到微信窗口（微信要先打开，并且打开一个聊天）".to_string());
    }
    if stdout.contains("NO_ENGINE") {
        return Err("系统没有中文 OCR 引擎（设置 → 时间和语言 → 语言 → 中文 → 可选功能里的「光学字符识别」）".to_string());
    }
    if stdout.contains("BAD_SIZE") || stdout.contains("BAD_CROP") {
        return Err("微信窗口被最小化了（或太小）：先把微信打开、点开一个聊天，把窗口拉到正常大小再读".to_string());
    }
    if raw.trim().is_empty() {
        let detail: String = stdout.chars().take(200).collect();
        return Err(format!(
            "截到图了但没识别出文字（聊天窗口是不是空着？）{}",
            if detail.trim().is_empty() { String::new() } else { format!("（{detail}）") }
        ));
    }
    let text = normalize(&raw);
    if text.trim().is_empty() {
        return Err("只识别到界面上的固定文字（搜索 / 通讯录之类），换个聊天试试".to_string());
    }
    Ok(text)
}

/// 读剪贴板（直读失败时的退路：用户自己右键「复制」消息）
pub fn clipboard_text() -> Result<String, String> {
    let exe = powershell_path().ok_or_else(|| "这台机器找不到 PowerShell".to_string())?;
    let mut command = Command::new(exe);
    command.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        "Get-Clipboard -Raw",
    ]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = command
        .output()
        .map_err(|exc| format!("读剪贴板失败：{exc}"))?;
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    if text.trim().is_empty() {
        return Err("剪贴板是空的".to_string());
    }
    Ok(normalize(&text))
}

/// 界面上的固定文案 / 噪声行（OCR 会读到，但不是消息）
const NOISE: [&str; 14] = [
    "search", "hide pinned chats", "hold mouse to input by voice", "send", "todolist",
    "favorites", "file transfer", "contacts", "moments", "channels", "微信", "通讯录",
    "收藏", "搜索",
];

/// 屏幕上别的窗口压过来时 OCR 到的无关文字
const JUNK_WORDS: [&str; 12] = [
    "缓存命中", "未缓存命中", "缓存读取", "token", "input tokens", "output tokens",
    "消耗", "余额", "thinking", "小程序", "文件传输助手", "朋友圈",
];

fn is_cjk(ch: char) -> bool {
    matches!(ch as u32, 0x3400..=0x9fff | 0x3000..=0x303f | 0xff00..=0xffef)
}

/// 去中文之间的空格、丢时间戳和界面噪声（照旧版 `wechat_read.normalize`）
pub fn normalize(text: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for raw in text.lines() {
        let mut line = raw.trim().to_string();
        if line.is_empty() {
            continue;
        }
        // 中文之间被 OCR 插进去的空格
        let mut squeezed = String::with_capacity(line.len());
        let mut chars = line.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == ' ' {
                // 前后都是中文 → 丢掉这个空格
                let prev = squeezed.chars().last();
                let next = chars.clone().find(|one| *one != ' ');
                let drop_space = matches!((prev, next), (Some(a), Some(b)) if is_cjk(a) && is_cjk(b));
                if drop_space {
                    continue;
                }
                squeezed.push(ch);
                while chars.peek() == Some(&' ') {
                    chars.next();
                }
            } else {
                squeezed.push(ch);
            }
        }
        line = squeezed.trim().to_string();
        if line.is_empty() {
            continue;
        }
        let lowered = line.to_lowercase();
        if NOISE.contains(&lowered.as_str()) {
            continue;
        }
        if JUNK_WORDS.iter().any(|word| lowered.contains(&word.to_lowercase())) {
            continue;
        }
        // 纯时间 / 纯日期
        let time_only = line
            .chars()
            .all(|ch| ch.is_ascii_digit() || "：: -/.年月日时分".contains(ch));
        if time_only {
            continue;
        }
        if line.chars().count() <= 1 {
            continue;
        }
        // 汉字太少、又不像一句英文短语的，当噪声丢掉
        let cjk = line.chars().filter(|one| is_cjk(*one)).count();
        if cjk < 2 {
            let letters = line.chars().filter(|one| one.is_ascii_alphabetic()).count();
            if !(letters >= 4 && !line.contains(' ')) {
                continue;
            }
        }
        out.push(line);
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_drops_ui_noise_and_timestamps() {
        let raw = "10:24\nSearch\n张老师: 明天下午三点开会\n张三  李四\nSend\n文件传输助手\n";
        let text = normalize(raw);
        assert_eq!(text, "张老师: 明天下午三点开会\n张三李四", "实际：{text:?}");
    }

    #[test]
    fn normalize_keeps_english_phrases() {
        let raw = "Hello\nok\nmeeting moved to 3pm\n";
        let text = normalize(raw);
        // 和旧版一致：汉字少于 2 个时，只有「连着写、≥4 字母」的英文词才留下
        // （带空格的英文句子旧版也丢，这里不为了一句话改行为）
        assert_eq!(text, "Hello", "实际：{text:?}");
    }

    #[test]
    fn encodes_utf16le_for_powershell() {
        let encoded = encode_command("-Command");
        // "-Command" 的 UTF-16LE 再 base64：LQBDAG8AbQBtAGEAbgBkAA==
        assert_eq!(encoded, "LQBDAG8AbQBtAGEAbgBkAA==");
    }

    /// 真读一次当前微信聊天（默认跳过：要开微信 + 中文 OCR）
    /// `cargo test wechat::tests::reads_real_wechat -- --ignored --test-threads=1 --nocapture`
    #[test]
    #[ignore = "需要微信开着，并装好中文 OCR"]
    fn reads_real_wechat() {
        if !available() {
            println!("这台机器没有 PowerShell，跳过");
            return;
        }
        match grab_current_chat() {
            Ok(text) => println!("识别出 {} 行；前 60 字：{}", text.lines().count(), text.chars().take(60).collect::<String>()),
            Err(exc) => println!("读取没成功（可能是没打开聊天）：{exc}"),
        }
    }
}
