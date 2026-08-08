# wp-plain-child-test.ps1 — decisive Win10 rendering test.
# Creates a plain solid-red WinForms window, SetParents it into the desktop
# icons host (below SHELLDLL_DefView, same as the wallpaper-layer skin),
# holds for -Seconds, then cleans up. If the red square is visible on the
# desktop, cross-process child windows render fine and the skin's problem is
# WebView2/DComp-specific; if not, Win10's DWM doesn't compose cross-process
# children at all.
param([int]$Seconds = 10, [string]$Target = "host", [int]$X = 1450, [int]$Y = 100)

Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class WPlain {
    [DllImport("user32.dll")] public static extern IntPtr GetShellWindow();
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string t);
    [DllImport("user32.dll")] public static extern IntPtr SetParent(IntPtr h, IntPtr p);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr ins, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] public static extern IntPtr GetWindowLongPtrW(IntPtr h, int i);
    [DllImport("user32.dll")] public static extern IntPtr SetWindowLongPtrW(IntPtr h, int i, IntPtr v);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }

    public static IntPtr FindHost(out IntPtr defview) {
        IntPtr shell = GetShellWindow();
        IntPtr dv = FindWindowExW(shell, IntPtr.Zero, "SHELLDLL_DefView", null);
        if (dv != IntPtr.Zero) { defview = dv; return shell; }
        IntPtr w = IntPtr.Zero;
        while (true) {
            w = FindWindowExW(IntPtr.Zero, w, "WorkerW", null);
            if (w == IntPtr.Zero) break;
            dv = FindWindowExW(w, IntPtr.Zero, "SHELLDLL_DefView", null);
            if (dv != IntPtr.Zero) { defview = dv; return w; }
        }
        defview = IntPtr.Zero;
        return IntPtr.Zero;
    }

    // wallpaper WorkerW = full-screen visible top-level WorkerW that is NOT the icons host
    public static IntPtr FindWallpaper(IntPtr host) {
        IntPtr w = IntPtr.Zero;
        while (true) {
            w = FindWindowExW(IntPtr.Zero, w, "WorkerW", null);
            if (w == IntPtr.Zero) break;
            if (w == host || !IsWindowVisible(w)) continue;
            RECT r; if (!GetWindowRect(w, out r)) continue;
            if ((r.R - r.L) >= GetSystemMetrics(0) && (r.B - r.T) >= GetSystemMetrics(1)) return w;
        }
        return IntPtr.Zero;
    }
}
"@
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

$defview = [IntPtr]::Zero
$host_ = [WPlain]::FindHost([ref]$defview)
if ($host_ -eq [IntPtr]::Zero) { "no icons host found"; exit 1 }
"host: 0x{0:X}  defview: 0x{1:X}" -f $host_.ToInt64(), $defview.ToInt64()

$parent = $host_
$insertAfter = $defview   # default: into icons host, directly below DefView
if ($Target -eq "wallpaper") {
    $parent = [WPlain]::FindWallpaper($host_)
    if ($parent -eq [IntPtr]::Zero) { "no wallpaper WorkerW found"; exit 1 }
    $insertAfter = [IntPtr]::Zero  # HWND_TOP within the wallpaper window
    "wallpaper WorkerW: 0x{0:X}" -f $parent.ToInt64()
}
if ($Target -eq "host-top") {
    $insertAfter = [IntPtr]::Zero  # HWND_TOP within the icons host (above DefView)
}
if ($Target -eq "glue") {
    $parent = [IntPtr]::Zero       # no SetParent: stay top-level
    $insertAfter = $host_          # z-glue: directly below the icons host
}

$form = New-Object System.Windows.Forms.Form
$form.FormBorderStyle = 'None'
$form.BackColor = [System.Drawing.Color]::Red
$form.StartPosition = 'Manual'
$form.Location = New-Object System.Drawing.Point(1450, 100)
$form.Size = New-Object System.Drawing.Size(300, 300)
$form.ShowInTaskbar = $false
$null = $form.Handle  # force handle creation
$hwnd = $form.Handle

# WS_POPUP -> WS_CHILD, parent into target, place per -Target
if ($parent -ne [IntPtr]::Zero) {
    $st = [WPlain]::GetWindowLongPtrW($hwnd, -16).ToInt64()
    [void][WPlain]::SetWindowLongPtrW($hwnd, -16, [IntPtr](($st -band -bnot 0x80000000L) -bor 0x40000000L))
    $prev = [WPlain]::SetParent($hwnd, $parent)
    "SetParent(prev=0x{0:X}) into 0x{1:X} (target=$Target)" -f $prev.ToInt64(), $parent.ToInt64()
} else {
    "no SetParent (target=$Target): top-level z-glue below icons host 0x{0:X}" -f $host_.ToInt64()
}
[void][WPlain]::SetWindowPos($hwnd, $insertAfter, $X, $Y, 300, 300, 0x0010 -bor 0x0040) # NOACTIVATE|SHOWWINDOW
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public class WHit {
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, System.Text.StringBuilder s, int n);
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
    public static string Hit(int x, int y) {
        var p = new POINT { X = x, Y = y };
        IntPtr h = WindowFromPoint(p);
        var c = new System.Text.StringBuilder(64); GetClassNameW(h, c, 64);
        return string.Format("({0},{1}) -> 0x{2:X} {3}", x, y, h.ToInt64(), c.ToString());
    }
}
"@
"hit test over square: " + [WHit]::Hit($X + 150, $Y + 150)
"red square planted at (1450,100) 300x300, below DefView. holding $Seconds s (pumping messages)..."
$end = (Get-Date).AddSeconds($Seconds)
while ((Get-Date) -lt $end) {
    [System.Windows.Forms.Application]::DoEvents()
    Start-Sleep -Milliseconds 50
}
$form.Close()
"cleaned up"
