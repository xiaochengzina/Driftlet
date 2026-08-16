# manager-focus-frame.ps1 — 实证真实管理器窗顶边描边随焦点的变化
# 流程：先建本进程自有小窗 F（可见窗口让本进程合法取得前台权）→ 找「Driftlet」
# 管理器窗 → 激活管理器采样顶边 → 激活 F 抢焦点再采样 → 还原。每步打印
# GetForegroundWindow 标题确证前台切换真实发生（防前台锁静默失败造成假阴性）。
# 像素取管理器窗顶边中心列 -2..+8 行（GetDC 屏幕 DC + GetPixel，进程 PerMonitorV2）。
# usage: powershell -File tools/win32-probes/manager-focus-frame.ps1（需 driftlet.exe 已在运行）
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

$cs = @"
using System;
using System.Text;
using System.Runtime.InteropServices;
using System.Windows.Forms;
using System.Drawing;

public class W32M {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h, StringBuilder sb, int max);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern IntPtr GetDC(IntPtr h);
  [DllImport("user32.dll")] public static extern int ReleaseDC(IntPtr h, IntPtr dc);
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr ctx);
  [DllImport("gdi32.dll")] public static extern uint GetPixel(IntPtr dc, int x, int y);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint idAttach, uint idAttachTo, bool fAttach);
  [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int l, t, r, b;
    public override string ToString() { return "(" + l + "," + t + ")-(" + r + "," + b + ")"; } }

  // 前台锁：切走前台后本进程失去 SetForegroundWindow 权限——附到前台线程的
  // 输入队列上再切（AttachThreadInput 经典手法），切完立即脱离。
  public static void ForceForeground(IntPtr h) {
    uint pidUnused;
    uint fgThread = GetWindowThreadProcessId(GetForegroundWindow(), out pidUnused);
    uint cur = GetCurrentThreadId();
    bool attached = fgThread != cur && AttachThreadInput(cur, fgThread, true);
    SetForegroundWindow(h);
    if (attached) AttachThreadInput(cur, fgThread, false);
  }

  public static IntPtr FindByTitle(string title) {
    IntPtr hit = IntPtr.Zero;
    EnumWindows((h, l) => {
      var sb = new StringBuilder(256);
      GetWindowText(h, sb, 256);
      if (sb.ToString() == title) { hit = h; return false; }
      return true;
    }, IntPtr.Zero);
    return hit;
  }
  public static string ForegroundTitle() {
    var sb = new StringBuilder(256);
    GetWindowText(GetForegroundWindow(), sb, 256);
    return sb.ToString();
  }
  public static string SampleTop(IntPtr hwnd) {
    RECT wr; GetWindowRect(hwnd, out wr);
    int cx = (wr.l + wr.r) / 2;
    IntPtr dc = GetDC(IntPtr.Zero);
    var sb = new StringBuilder();
    for (int dy = -2; dy <= 8; dy++) {
      uint c = GetPixel(dc, cx, wr.t + dy);
      sb.Append(dy).Append(":").Append((c & 0xFF).ToString("X2"))
        .Append(((c >> 8) & 0xFF).ToString("X2")).Append(((c >> 16) & 0xFF).ToString("X2")).Append(" ");
    }
    ReleaseDC(IntPtr.Zero, dc);
    return "rect=" + wr + " cx=" + cx + "  " + sb;
  }
}

public class ProbeHolder : Form { // own visible window: grants this process foreground rights
  public ProbeHolder() {
    Text = "focus-holder"; Name = "focus-holder";
    StartPosition = FormStartPosition.Manual;
    Location = new Point(1200, 500);
    Size = new Size(240, 160);
  }
}
"@
Add-Type -TypeDefinition $cs -ReferencedAssemblies System.Windows.Forms, System.Drawing

[W32M]::SetProcessDpiAwarenessContext([IntPtr]::Zero - 4) | Out-Null  # PER_MONITOR_AWARE_V2 = -4

$mgr = [W32M]::FindByTitle('Driftlet')
if ($mgr -eq [IntPtr]::Zero) { Write-Host 'manager window not found — start driftlet.exe first'; exit 1 }

$holder = New-Object ProbeHolder
$holder.Show()
$script:step = 0
$script:lines = @()
$wasVisible = [W32M]::IsWindowVisible($mgr)

$timer = New-Object System.Windows.Forms.Timer
$timer.Interval = 900
$timer.add_Tick({
  switch ($script:step) {
    0 {
      [W32M]::ShowWindow($mgr, 5) | Out-Null  # SW_SHOW
      [W32M]::ForceForeground($mgr)
    }
    1 {
      $script:lines += ("ACTIVE   fg=[{0}]" -f [W32M]::ForegroundTitle())
      $script:lines += ("  " + [W32M]::SampleTop($mgr))
      [W32M]::ForceForeground($holder.Handle)
    }
    2 {
      $script:lines += ("INACTIVE fg=[{0}]" -f [W32M]::ForegroundTitle())
      $script:lines += ("  " + [W32M]::SampleTop($mgr))
      $timer.Stop()
      if (-not $wasVisible) { [W32M]::ShowWindow($mgr, 0) | Out-Null }  # SW_HIDE，还原显隐
      [System.IO.File]::WriteAllLines((Join-Path $env:TEMP 'manager-focus-frame.txt'), $script:lines)
      $script:lines | ForEach-Object { Write-Host $_ }
      Write-Host "report -> $env:TEMP\manager-focus-frame.txt"
      [System.Windows.Forms.Application]::Exit()
    }
  }
  $script:step++
})
$timer.Start()
[System.Windows.Forms.Application]::Run()
