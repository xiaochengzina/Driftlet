# top-inset-probe.ps1 — Win10 顶部白条/最大化几何修正方案探针
# 背景：「THICKFRAME+BORDER、无 CAPTION、默认 NCCALCSIZE」配方下，DefWindowProc
# 的默认 NCCALCSIZE 把可缩放边框厚度 inset 加在四边——左/右/下的 inset 被 DWM
# 放到可视区外（GetWindowRect 比 EXTENDED_FRAME_BOUNDS 大 7px），唯独顶部 inset
# 留在可视区内：DWM 只画 1px 顶边框，其余 ~7px 是死白边（管理器窗实测 8px）。
# 另两坑（同源）：无 WS_CAPTION 的 THICKFRAME 窗口最大化时系统按显示器全矩形
# 摆窗（不走 work area 收缩），默认 NCCALCSIZE 的 client 盖住任务栏区；且窗口
# 矩形本身也是显示器口径，底边非客户区会把非置顶任务栏整段盖住。
# 本探针验证完整配方：
#   NCCALCSIZE 先交 DefWindowProc 默认处理，然后——非最大化 rgrc[0].top=窗口顶+1；
#   最大化 rgrc[0]=rcWork；WM_GETMINMAXINFO 把 ptMaxPosition/ptMaxSize 改为
#   「work area + 边框膨胀」（膨胀量从默认 ptMaxSize 与显示器尺寸反推，跨 DPI）。
#   C = control：默认 NCCALCSIZE（复现顶部白条）
#   T = top1px：仅还原态回拉（对照）
#   M = 完整配方 + 启动后最大化（验证最大化摆窗、client=rcWork、任务栏不被盖）
# 输出 top-inset-probe.txt（矩形/可视框/client 与 work area 对照），
# top-inset-probe.png（全屏截图，看任务栏是否露出、M 有无标题栏），均在 TEMP。
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

$cs = @"
using System;
using System.Runtime.InteropServices;
using System.Windows.Forms;
using System.Drawing;

public class W32T {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern IntPtr DefWindowProc(IntPtr h, int msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern IntPtr MonitorFromWindow(IntPtr h, int flags);
  [DllImport("user32.dll")] public static extern bool GetMonitorInfo(IntPtr m, ref MONITORINFO mi);
  [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int attr, ref RECT r, int size);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int l, t, r, b;
    public override string ToString() { return "(" + l + "," + t + ")-(" + r + "," + b + ")"; } }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int x, y; }
  [StructLayout(LayoutKind.Sequential)] public struct NCCP { public RECT r0, r1, r2; public IntPtr pos; }
  [StructLayout(LayoutKind.Sequential)] public struct MONITORINFO { public int cb; public RECT mon, work; public int flags; }
  [StructLayout(LayoutKind.Sequential)] public struct MINMAXINFO { public POINT reserved, maxSize, maxPos, minTrack, maxTrack; }
  public const int DWMWA_EXTENDED_FRAME_BOUNDS = 9;
  public const int WM_NCCALCSIZE = 0x83;
  public const int WM_GETMINMAXINFO = 0x24;
}

public class TopInsetForm : Form {
  public int NcMode; // 0=default, 1=top1px, 2=完整配方(最大化 rcWork + GETMINMAXINFO 摆窗修正)
  public TopInsetForm(string name, int x, int y, int ncMode, bool topmost) {
    Text = name; Name = name; NcMode = ncMode;
    StartPosition = FormStartPosition.Manual;
    Location = new Point(x, y);
    Size = new Size(340, 220);
    TopMost = topmost;
    BackColor = Color.White;
  }
  protected override CreateParams CreateParams {
    get {
      var cp = base.CreateParams;
      // THICKFRAME|BORDER|SYSMENU|MINBOX|MAXBOX|CLIPSIBLINGS|WS_VISIBLE —— 无 WS_CAPTION
      cp.Style = 0x00040000 | 0x00800000 | 0x00080000 | 0x00020000 | 0x00010000 | 0x04000000 | 0x10000000;
      return cp;
    }
  }
  private W32T.MONITORINFO GetMi(IntPtr h) {
    var mi = new W32T.MONITORINFO(); mi.cb = Marshal.SizeOf(typeof(W32T.MONITORINFO));
    W32T.GetMonitorInfo(W32T.MonitorFromWindow(h, 2), ref mi);
    return mi;
  }
  protected override void WndProc(ref Message m) {
    if (m.Msg == W32T.WM_GETMINMAXINFO && NcMode == 2) {
      base.WndProc(ref m); // 先默认/窗体约束，再改最大化摆放
      var mm = (W32T.MINMAXINFO)Marshal.PtrToStructure(m.LParam, typeof(W32T.MINMAXINFO));
      var mi = GetMi(m.HWnd);
      int ix = (mm.maxSize.x - (mi.mon.r - mi.mon.l)) / 2;
      int iy = (mm.maxSize.y - (mi.mon.b - mi.mon.t)) / 2;
      mm.maxPos.x = mi.work.l - ix; mm.maxPos.y = mi.work.t - iy;
      mm.maxSize.x = (mi.work.r - mi.work.l) + 2 * ix;
      mm.maxSize.y = (mi.work.b - mi.work.t) + 2 * iy;
      Marshal.StructureToPtr(mm, m.LParam, false);
      return;
    }
    if (m.Msg == W32T.WM_NCCALCSIZE && NcMode != 0 && m.WParam != IntPtr.Zero) {
      var p = (W32T.NCCP)Marshal.PtrToStructure(m.LParam, typeof(W32T.NCCP));
      int winTop = p.r0.t;
      m.Result = W32T.DefWindowProc(m.HWnd, m.Msg, m.WParam, m.LParam);
      p = (W32T.NCCP)Marshal.PtrToStructure(m.LParam, typeof(W32T.NCCP));
      if (W32T.IsZoomed(m.HWnd)) {
        if (NcMode == 2) { var mi = GetMi(m.HWnd); p.r0 = mi.work; }
      } else {
        p.r0.t = winTop + 1;
      }
      Marshal.StructureToPtr(p, m.LParam, false);
      return;
    }
    base.WndProc(ref m);
  }
  protected override void OnPaint(PaintEventArgs e) {
    base.OnPaint(e);
    TextRenderer.DrawText(e.Graphics, Name, Font, new Point(10, 10), Color.Black);
  }
}
"@
Add-Type -TypeDefinition $cs -ReferencedAssemblies System.Windows.Forms, System.Drawing

$script:wins = @()
foreach ($def in @(
  @('C-control',  60,  120, 0, $true),
  @('T-top1px',  440,  120, 1, $true),
  @('M-zoomed',  820,  120, 2, $false))) {
  $f = New-Object TopInsetForm($def[0], $def[1], $def[2], $def[3], $def[4])
  $f.Show()
  $script:wins += $f
}

$timer = New-Object System.Windows.Forms.Timer
$timer.Interval = 2500
$timer.add_Tick({
  $timer.Stop()
  foreach ($w in $script:wins) {
    if ($w.Name -eq 'M-zoomed') { [W32T]::ShowWindow($w.Handle, 3) | Out-Null }  # SW_MAXIMIZE
  }
  Start-Sleep -Milliseconds 800
  $lines = @()
  foreach ($w in $script:wins) {
    $wr = New-Object W32T+RECT; $ex = New-Object W32T+RECT; $cr = New-Object W32T+RECT; $pt = New-Object W32T+POINT
    $mi = New-Object W32T+MONITORINFO; $mi.cb = [Runtime.InteropServices.Marshal]::SizeOf([type][W32T+MONITORINFO])
    [W32T]::GetWindowRect($w.Handle, [ref]$wr) | Out-Null
    [W32T]::DwmGetWindowAttribute($w.Handle, [W32T]::DWMWA_EXTENDED_FRAME_BOUNDS, [ref]$ex, 16) | Out-Null
    [W32T]::GetClientRect($w.Handle, [ref]$cr) | Out-Null
    [W32T]::ClientToScreen($w.Handle, [ref]$pt) | Out-Null
    [W32T]::GetMonitorInfo([W32T]::MonitorFromWindow($w.Handle, 2), [ref]$mi) | Out-Null
    $lines += ("{0,-10} zoomed={1} rect={2} ext={3}" -f $w.Name, [W32T]::IsZoomed($w.Handle), $wr, $ex)
    $lines += ("{0,-10}   client org=({1},{2}) size={3}x{4} | work={5}" -f $w.Name, $pt.x, $pt.y, ($cr.r-$cr.l), ($cr.b-$cr.t), $mi.work)
  }
  [System.IO.File]::WriteAllLines((Join-Path $env:TEMP 'top-inset-probe.txt'), $lines)
  $vs = [System.Windows.Forms.SystemInformation]::VirtualScreen
  $bmp = New-Object System.Drawing.Bitmap $vs.Width, $vs.Height
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($vs.Left, $vs.Top, 0, 0, $bmp.Size)
  $bmp.Save((Join-Path $env:TEMP 'top-inset-probe.png'), [System.Drawing.Imaging.ImageFormat]::Png)
  $g.Dispose(); $bmp.Dispose()
  [System.Windows.Forms.Application]::Exit()
})
$timer.Start()
[System.Windows.Forms.Application]::Run()
