# Frame watcher: polls the skin window's Win32 styles + DWM attributes and
# captures screenshots of the window region. Logs changes with timestamps.
param(
  [string]$Title = "Example Clock",
  [int]$DurationSec = 45,
  [string]$OutDir = ".debug-watch/out"
)

$ErrorActionPreference = "Stop"
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @"
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Drawing;
using System.Drawing.Imaging;

public class WinSpy {
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumWindowsProc cb, IntPtr lp);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetWindowLongPtrW(IntPtr h, int i);
    [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr h, uint a, IntPtr v, uint sz);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll")] static extern IntPtr FindWindowW(string cls, string title);
    [DllImport("user32.dll")] static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern IntPtr SendMessageTimeoutW(IntPtr h, uint msg, UIntPtr wp, string lp, uint flags, uint timeout, out UIntPtr result);

    public static void BroadcastSettingChange() {
        UIntPtr res;
        SendMessageTimeoutW(new IntPtr(0xffff), 0x001A, UIntPtr.Zero, "ImmersiveColorSet", 0x2, 1000, out res);
    }

    public static void Drag(IntPtr h, int dx, int dy) {
        RECT r; GetWindowRect(h, out r);
        int cx = (r.L + r.R) / 2, cy = (r.T + r.B) / 2;
        SetCursorPos(cx, cy);
        mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero); // LEFTDOWN
        for (int i = 1; i <= 10; i++) {
            SetCursorPos(cx + dx * i / 10, cy + dy * i / 10);
            System.Threading.Thread.Sleep(20);
        }
        mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero); // LEFTUP
    }

    public delegate bool EnumWindowsProc(IntPtr h, IntPtr lp);

    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }

    public static IntPtr FindByTitle(string title) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((h, lp) => {
            var sb = new StringBuilder(256);
            GetWindowTextW(h, sb, 256);
            if (sb.ToString() == title && IsWindowVisible(h)) { found = h; return false; }
            return true;
        }, IntPtr.Zero);
        return found;
    }

    public static string Snapshot(IntPtr h) {
        long style = GetWindowLongPtrW(h, -16).ToInt64();
        long ex = GetWindowLongPtrW(h, -20).ToInt64();
        IntPtr parent = GetParent(h);
        int ncr = -1, cloak = -1;
        RECT efb = new RECT(), wr = new RECT();
        IntPtr buf = Marshal.AllocHGlobal(16);
        try {
            if (DwmGetWindowAttribute(h, 2, buf, 4) == 0) ncr = Marshal.ReadInt32(buf);       // NCRENDERING_ENABLED
            if (DwmGetWindowAttribute(h, 14, buf, 4) == 0) cloak = Marshal.ReadInt32(buf);    // CLOAKED
            if (DwmGetWindowAttribute(h, 9, buf, 16) == 0) efb = Marshal.PtrToStructure<RECT>(buf); // EXTENDED_FRAME_BOUNDS
        } finally { Marshal.FreeHGlobal(buf); }
        GetWindowRect(h, out wr);
        long parentStyle = 0;
        if (parent != IntPtr.Zero) parentStyle = GetWindowLongPtrW(parent, -16).ToInt64();
        return string.Format("style=0x{0:X8} ex=0x{1:X8} parent=0x{2:X} pstyle=0x{3:X8} ncr_enabled={4} cloaked={5} wr=[{6},{7},{8},{9}] efb=[{10},{11},{12},{13}]",
            style, ex, parent.ToInt64(), parentStyle, ncr, cloak, wr.L, wr.T, wr.R, wr.B, efb.L, efb.T, efb.R, efb.B);
    }

    public static void Shot(IntPtr h, string path, int margin) {
        RECT r; GetWindowRect(h, out r);
        int x = Math.Max(0, r.L - margin), y = Math.Max(0, r.T - margin);
        int w = (r.R - r.L) + margin * 2, hh = (r.B - r.T) + margin * 2;
        using (var bmp = new Bitmap(w, hh, PixelFormat.Format32bppArgb))
        using (var g = Graphics.FromImage(bmp)) {
            g.CopyFromScreen(x, y, 0, 0, new Size(w, hh), CopyPixelOperation.SourceCopy);
            bmp.Save(path, ImageFormat.Png);
        }
    }

    public static void WinD() {
        keybd_event(0x5B, 0, 0, UIntPtr.Zero);              // LWIN down
        keybd_event(0x44, 0, 0, UIntPtr.Zero);              // D down
        keybd_event(0x44, 0, 2, UIntPtr.Zero);              // D up
        keybd_event(0x5B, 0, 2, UIntPtr.Zero);              // LWIN up
    }
    public static void FocusDesktop() {
        IntPtr progman = FindWindowW("Progman", null);
        if (progman != IntPtr.Zero) SetForegroundWindow(progman);
    }
}
"@

$log = Join-Path $OutDir "watch.log"
"=== watch start $(Get-Date -Format o) ===" | Out-File $log
$start = Get-Date
$last = ""
$shotIdx = 0
$triggers = @{
  8  = { param($h) "TRIGGER: focus skin" | Out-File $log -Append; if ($h -ne [IntPtr]::Zero) { [WinSpy]::SetForegroundWindow($h) | Out-Null } }
  12 = { param($h) "TRIGGER: focus desktop" | Out-File $log -Append; [WinSpy]::FocusDesktop() }
  18 = { param($h) "TRIGGER: Win+D (show desktop)" | Out-File $log -Append; [WinSpy]::WinD() }
  24 = { param($h) "TRIGGER: Win+D (restore)" | Out-File $log -Append; [WinSpy]::WinD() }
  32 = { param($h) "TRIGGER: focus skin again" | Out-File $log -Append; if ($h -ne [IntPtr]::Zero) { [WinSpy]::SetForegroundWindow($h) | Out-Null } }
  36 = { param($h) "TRIGGER: focus desktop" | Out-File $log -Append; [WinSpy]::FocusDesktop() }
  40 = { param($h) "TRIGGER: drag skin +150,+100" | Out-File $log -Append; if ($h -ne [IntPtr]::Zero) { [WinSpy]::Drag($h, 150, 100) } }
  46 = { param($h) "TRIGGER: drag skin back -150,-100" | Out-File $log -Append; if ($h -ne [IntPtr]::Zero) { [WinSpy]::Drag($h, -150, -100) } }
  52 = { param($h) "TRIGGER: Win+D (show desktop)" | Out-File $log -Append; [WinSpy]::WinD() }
  56 = { param($h) "TRIGGER: Win+D (restore)" | Out-File $log -Append; [WinSpy]::WinD() }
  60 = { param($h) "TRIGGER: theme toggle dark->light" | Out-File $log -Append
         Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize" -Name AppsUseLightTheme -Value 1
         Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize" -Name SystemUsesLightTheme -Value 1
         [WinSpy]::BroadcastSettingChange() }
  66 = { param($h) "TRIGGER: theme toggle light->dark" | Out-File $log -Append
         Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize" -Name AppsUseLightTheme -Value 0
         Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize" -Name SystemUsesLightTheme -Value 0
         [WinSpy]::BroadcastSettingChange() }
}
$fired = @{}

while (((Get-Date) - $start).TotalSeconds -lt $DurationSec) {
  $elapsed = [int]((Get-Date) - $start).TotalSeconds
  $h = [WinSpy]::FindByTitle($Title)
  foreach ($t in @($triggers.Keys)) {
    if ($elapsed -ge $t -and -not $fired[$t]) { $fired[$t] = $true; & $triggers[$t] $h }
  }
  if ($h -eq [IntPtr]::Zero) {
    if ($last -ne "NOTFOUND") { "[$elapsed s] window NOT FOUND" | Out-File $log -Append; $last = "NOTFOUND" }
  } else {
    $snap = [WinSpy]::Snapshot($h)
    if ($snap -ne $last) {
      "[$elapsed s] $snap" | Out-File $log -Append
      $p = Join-Path $OutDir ("shot_{0:d3}.png" -f $shotIdx++)
      [WinSpy]::Shot($h, $p, 40)
      "    -> $p" | Out-File $log -Append
      $last = $snap
    }
  }
  Start-Sleep -Milliseconds 150
}
# final screenshot
$h = [WinSpy]::FindByTitle($Title)
if ($h -ne [IntPtr]::Zero) { [WinSpy]::Shot($h, (Join-Path $OutDir "final.png"), 40) }
"=== watch end $(Get-Date -Format o) ===" | Out-File $log -Append
Write-Output "done"
