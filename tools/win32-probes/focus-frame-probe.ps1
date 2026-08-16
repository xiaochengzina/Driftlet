# focus-frame-probe.ps1 — Win10 顶边描边随焦点变化的实证探针（第二轮）
# 第一轮结论：「THICKFRAME|BORDER、无 CAPTION、NCCALCSIZE 默认+顶边回拉 1px」配方下，
# DWM 只在窗口活动态画顶边 1px 描边；失焦后根本不画（不是画成近白色——顶行像素
# 直接等于背景）。标准 WS_OVERLAPPEDWINDOW 失焦后仍保留灰色边框。即失焦丢描边是
# 无 CAPTION 配方下 DWM 的系统行为，与应用层无关。
# 第二轮验证候选修法（失焦后保住活动态描边）：
#   N1-keep  = 拦 WM_NCACTIVATE 直接返回 1（不发 DefWindowProc，经典无边框手法）
#   N2-lie   = 拦 WM_NCACTIVATE 改发 DefWindowProc(wParam=TRUE, lParam=-1)（谎报活动）
#   T-nocap  = 现行配方（对照：活动有描边、失焦无）
#   C-control= 标准 WS_OVERLAPPEDWINDOW（参照：失焦灰边框）
#   F-focus  = 焦点占位小窗
# 依次激活 N1 → N2 → T → F，每态全屏截图（物理像素，进程 PerMonitorV2 DPI 感知），
# 采样各窗顶边 -2..+8 行中心列像素。输出 focus-frame-probe.txt 与 -1..-4.png（TEMP）。
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

$cs = @"
using System;
using System.Runtime.InteropServices;
using System.Windows.Forms;
using System.Drawing;

public class W32F {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr DefWindowProc(IntPtr h, int msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr ctx);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int l, t, r, b;
    public override string ToString() { return "(" + l + "," + t + ")-(" + r + "," + b + ")"; } }
  [StructLayout(LayoutKind.Sequential)] public struct NCCP { public RECT r0, r1, r2; public IntPtr pos; }
  public const int WM_NCCALCSIZE = 0x83;
  public const int WM_NCACTIVATE = 0x86;
}

public class NoCapForm : Form {
  public int NcaMode; // 0=default, 1=return 1 w/o DefWindowProc, 2=DefWindowProc(TRUE,-1)
  public NoCapForm(string name, int x, int y, int ncaMode) {
    Text = name; Name = name; NcaMode = ncaMode;
    StartPosition = FormStartPosition.Manual;
    Location = new Point(x, y);
    Size = new Size(340, 220);
    TopMost = true;
    BackColor = Color.White;
  }
  protected override CreateParams CreateParams {
    get {
      var cp = base.CreateParams;
      // THICKFRAME|BORDER|SYSMENU|MINBOX|MAXBOX|CLIPSIBLINGS|WS_VISIBLE, no WS_CAPTION
      cp.Style = 0x00040000 | 0x00800000 | 0x00080000 | 0x00020000 | 0x00010000 | 0x04000000 | 0x10000000;
      return cp;
    }
  }
  protected override void WndProc(ref Message m) {
    if (m.Msg == W32F.WM_NCACTIVATE) {
      if (NcaMode == 1) { m.Result = new IntPtr(1); return; } // keep active frame, classic trick
      if (NcaMode == 2) { // lie to the default proc: report active
        m.Result = W32F.DefWindowProc(m.HWnd, m.Msg, new IntPtr(1), new IntPtr(-1));
        return;
      }
    }
    if (m.Msg == W32F.WM_NCCALCSIZE && m.WParam != IntPtr.Zero) {
      var p = (W32F.NCCP)Marshal.PtrToStructure(m.LParam, typeof(W32F.NCCP));
      int winTop = p.r0.t;
      m.Result = W32F.DefWindowProc(m.HWnd, m.Msg, m.WParam, m.LParam);
      p = (W32F.NCCP)Marshal.PtrToStructure(m.LParam, typeof(W32F.NCCP));
      if (!W32F.IsZoomed(m.HWnd)) { p.r0.t = winTop + 1; Marshal.StructureToPtr(p, m.LParam, false); }
      return;
    }
    base.WndProc(ref m);
  }
  protected override void OnPaint(PaintEventArgs e) {
    base.OnPaint(e);
    TextRenderer.DrawText(e.Graphics, Name, Font, new Point(10, 10), Color.Black);
  }
}

public class PlainForm : Form { // plain WS_OVERLAPPEDWINDOW reference
  public PlainForm(string name, int x, int y, int w, int h) {
    Text = name; Name = name;
    StartPosition = FormStartPosition.Manual;
    Location = new Point(x, y);
    Size = new Size(w, h);
    TopMost = true;
    BackColor = Color.White;
  }
  protected override void OnPaint(PaintEventArgs e) {
    base.OnPaint(e);
    TextRenderer.DrawText(e.Graphics, Name, Font, new Point(10, 10), Color.Black);
  }
}
"@
Add-Type -TypeDefinition $cs -ReferencedAssemblies System.Windows.Forms, System.Drawing

[W32F]::SetProcessDpiAwarenessContext([IntPtr]::Zero - 4) | Out-Null  # PER_MONITOR_AWARE_V2 = -4

$n1 = New-Object NoCapForm('N1-keep', 40, 140, 1)
$n2 = New-Object NoCapForm('N2-lie', 420, 140, 2)
$t  = New-Object NoCapForm('T-nocap', 800, 140, 0)
$c  = New-Object PlainForm('C-control', 40, 440, 340, 220)
$f  = New-Object PlainForm('F-focus', 800, 440, 220, 160)
foreach ($w in @($n1, $n2, $t, $c, $f)) { $w.Show() }
$script:probeWins = @($n1, $n2, $t, $c)
$script:step = 0
$script:shotDir = $env:TEMP

function Save-FullShot($idx) {
  $vs = [System.Windows.Forms.SystemInformation]::VirtualScreen
  $bmp = New-Object System.Drawing.Bitmap $vs.Width, $vs.Height
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($vs.Left, $vs.Top, 0, 0, $bmp.Size)
  $bmp.Save((Join-Path $script:shotDir "focus-frame-probe-$idx.png"), [System.Drawing.Imaging.ImageFormat]::Png)
  $g.Dispose(); $bmp.Dispose()
}

$timer = New-Object System.Windows.Forms.Timer
$timer.Interval = 900
$timer.add_Tick({
  switch ($script:step) {
    0 { [W32F]::SetForegroundWindow($n1.Handle) | Out-Null }
    1 { Save-FullShot 1; [W32F]::SetForegroundWindow($n2.Handle) | Out-Null }
    2 { Save-FullShot 2; [W32F]::SetForegroundWindow($t.Handle) | Out-Null }
    3 { Save-FullShot 3; [W32F]::SetForegroundWindow($f.Handle) | Out-Null }
    4 {
      Save-FullShot 4
      $timer.Stop()
      # -- analysis: sample the top-edge center column (-2..+8 rows) of each window per state --
      # (must run BEFORE Application::Exit — Exit closes all forms and kills the handles)
      $lines = @()
      foreach ($w in $script:probeWins) {
        $wr = New-Object W32F+RECT
        [W32F]::GetWindowRect($w.Handle, [ref]$wr) | Out-Null
        $lines += ("== {0} rect={1} cx={2}" -f $w.Name, $wr, [int](($wr.l + $wr.r) / 2))
        for ($s = 1; $s -le 4; $s++) {
          $bmp = New-Object System.Drawing.Bitmap((Join-Path $env:TEMP "focus-frame-probe-$s.png"))
          $cx = [int](($wr.l + $wr.r) / 2)
          $row = "  S{0}: " -f $s
          for ($dy = -2; $dy -le 8; $dy++) {
            $y = $wr.t + $dy
            if ($y -ge 0 -and $y -lt $bmp.Height) {
              $px = $bmp.GetPixel($cx, $y)
              $row += ("{0}:{1:X2}{2:X2}{3:X2} " -f $dy, $px.R, $px.G, $px.B)
            }
          }
          $lines += $row
          $bmp.Dispose()
        }
      }
      $lines += "S1=N1 active / S2=N2 active / S3=T active / S4=F active (all others inactive)"
      [System.IO.File]::WriteAllLines((Join-Path $env:TEMP 'focus-frame-probe.txt'), $lines)
      Write-Host "probe done -> $env:TEMP\focus-frame-probe.txt"
      [System.Windows.Forms.Application]::Exit()
    }
  }
  $script:step++
})
$timer.Start()
[System.Windows.Forms.Application]::Run()
