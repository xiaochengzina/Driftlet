# wp-dump.ps1 — wallpaper-layer diagnostics (ASCII-only).
# Dumps: Progman's child tree (is the skin there? style/rect/visible?) and
# the skin's own children (did WebView2 follow?), plus a hit test at the
# skin's rect.
# Prereq: skin switched to wallpaper layer in the manager.
param([string]$ProcessName = "driftlet")

Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;

public class WpDump {
    [DllImport("user32.dll")] public static extern IntPtr GetDesktopWindow();
    [DllImport("user32.dll")] public static extern IntPtr GetShellWindow();
    [DllImport("user32.dll")] public static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string t);
    [DllImport("user32.dll")] public static extern IntPtr GetWindowLongPtrW(IntPtr h, int i);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
    [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint flags);
    [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
    [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr p, EnumProc cb, IntPtr l);

    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }

    public static string Desc(IntPtr h) {
        if (h == IntPtr.Zero) return "0x0";
        var c = new StringBuilder(64); GetClassNameW(h, c, 64);
        var t = new StringBuilder(50); GetWindowTextW(h, t, 50);
        uint pid; GetWindowThreadProcessId(h, out pid);
        long st = GetWindowLongPtrW(h, -16).ToInt64();
        RECT r; GetWindowRect(h, out r);
        return string.Format("0x{0:X} pid={1} {2} [{3}] st=0x{4:X8} vis={5} rect=({6},{7})-({8},{9})",
            h.ToInt64(), pid, c.ToString(), t.ToString(), st, IsWindowVisible(h), r.L, r.T, r.R, r.B);
    }

    public static string DumpChildren(IntPtr root, uint markPid) {
        var sb = new StringBuilder();
        EnumProc cb = (h, l) => {
            uint pid; GetWindowThreadProcessId(h, out pid);
            sb.AppendLine("  " + (pid == markPid ? ">> " : "   ") + Desc(h));
            return true;
        };
        EnumChildWindows(root, cb, IntPtr.Zero);
        GC.KeepAlive(cb);
        return sb.ToString();
    }

    public static string Hit(int px, int py) {
        var pt = new POINT { X = px, Y = py };
        IntPtr hit = WindowFromPoint(pt);
        if (hit == IntPtr.Zero) return "  hit = NULL (desktop background)";
        return "  hit = " + Desc(hit) + "\n  root = " + Desc(GetAncestor(hit, 2));
    }
}
"@

$procs = Get-Process -Name $ProcessName -ErrorAction SilentlyContinue
if (-not $procs) { "no $ProcessName process"; exit 1 }
$targetPid = $procs[0].Id

$progman = [WpDump]::GetShellWindow()
"progman: " + [WpDump]::Desc($progman)
"--- progman child tree (>> = driftlet) ---"
[WpDump]::DumpChildren($progman, [uint32]$targetPid)
