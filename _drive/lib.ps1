Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type -Namespace W -Name U -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
[DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
[DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
[DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
[DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
[DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, System.Text.StringBuilder s, int n);
[DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
public struct RECT { public int Left, Top, Right, Bottom; }
'@
function Focus-DanLume {
  $p = Get-Process DanLume -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
  if (-not $p) { return $null }
  $h = [IntPtr]$p.MainWindowHandle
  [void][W.U]::ShowWindow($h, 9)
  [void][W.U]::ShowWindow($h, 5)
  Start-Sleep -Milliseconds 250
  [W.U]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero)
  [W.U]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)
  [void][W.U]::SetForegroundWindow($h)
  Start-Sleep -Milliseconds 500
  return $h
}
function Get-Rect($h) {
  $r = New-Object W.U+RECT
  [void][W.U]::GetWindowRect($h, [ref]$r)
  return $r
}
function Capture($h, $out) {
  $r = Get-Rect $h
  $w = $r.Right - $r.Left; $hh = $r.Bottom - $r.Top
  $bmp = New-Object System.Drawing.Bitmap $w, $hh
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($r.Left, $r.Top, 0, 0, $bmp.Size)
  $bmp.Save($out)
}
$h = Focus-DanLume
if (-not $h) { "no window"; exit 1 }
$sb = New-Object System.Text.StringBuilder 256
[void][W.U]::GetWindowText([W.U]::GetForegroundWindow(), $sb, 256)
$r = Get-Rect $h
"hwnd=$h fg=[" + $sb.ToString() + "] rect=$($r.Left),$($r.Top) - $($r.Right),$($r.Bottom) iconic=$([W.U]::IsIconic($h))"
Capture $h (Join-Path (Get-Location) '_drive\00.png')
"captured 00.png"
