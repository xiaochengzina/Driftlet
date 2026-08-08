# wp-topology-check.ps1 — minimal desktop topology dump (READ-ONLY).
# Answers the wallpaper-layer key questions on any machine:
#   1. Who hosts SHELLDLL_DefView (Progman or a top-level WorkerW)?
#   2. Is there a SEPARATE full-screen wallpaper WorkerW (excluding the host)?
#   3. OS version line.
# Usage: powershell -ExecutionPolicy Bypass -File wp-topology-check.ps1

Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;

public class Topo {
    [DllImport("user32.dll")] public static extern IntPtr GetShellWindow();
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string t);
    [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h, uint cmd);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);

    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }

    public static string Cls(IntPtr h) {
        var c = new StringBuilder(64); GetClassNameW(h, c, 64); return c.ToString();
    }

    public static string Desc(IntPtr h) {
        if (h == IntPtr.Zero) return "0x0";
        RECT r; GetWindowRect(h, out r);
        return string.Format("0x{0:X} {1} vis={2} rect=({3},{4})-({5},{6})",
            h.ToInt64(), Cls(h), IsWindowVisible(h), r.L, r.T, r.R, r.B);
    }

    public static bool Full(IntPtr h) {
        if (!IsWindowVisible(h)) return false;
        RECT r; if (!GetWindowRect(h, out r)) return false;
        return (r.R - r.L) >= GetSystemMetrics(0) && (r.B - r.T) >= GetSystemMetrics(1);
    }

    public static void Run() {
        IntPtr shell = GetShellWindow();
        Console.WriteLine("shell(Progman): " + Desc(shell));

        // 1. DefView host: Progman first, then top-level WorkerW scan
        IntPtr dv = FindWindowExW(shell, IntPtr.Zero, "SHELLDLL_DefView", null);
        IntPtr host = IntPtr.Zero;
        if (dv != IntPtr.Zero) {
            host = shell;
        } else {
            IntPtr w = IntPtr.Zero;
            while (true) {
                w = FindWindowExW(IntPtr.Zero, w, "WorkerW", null);
                if (w == IntPtr.Zero) break;
                IntPtr d = FindWindowExW(w, IntPtr.Zero, "SHELLDLL_DefView", null);
                if (d != IntPtr.Zero) { host = w; dv = d; break; }
            }
        }
        if (host == IntPtr.Zero) {
            Console.WriteLine("!! SHELLDLL_DefView not found (desktop icons hidden?)");
        } else {
            Console.WriteLine("DefView host:  " + Desc(host) + (host == shell ? "  == Progman" : "  == top-level WorkerW (classic form)"));
            Console.WriteLine("DefView:       " + Desc(dv));
        }

        // 2. every top-level Progman/WorkerW + fullscreen verdict
        Console.WriteLine("--- top-level Progman/WorkerW ---");
        IntPtr t = IntPtr.Zero;
        bool separateWallpaper = false;
        while (true) {
            t = FindWindowExW(IntPtr.Zero, t, null, null);
            if (t == IntPtr.Zero) break;
            string c = Cls(t);
            if (c != "Progman" && c != "WorkerW") continue;
            bool full = Full(t);
            Console.WriteLine("  " + Desc(t) + (full ? "  FULLSCREEN" : "") + (t == host ? "  <== DefView host" : ""));
            if (c == "WorkerW" && full && t != host) separateWallpaper = true;
        }

        // 3. Progman children named WorkerW (24H2 child form)
        Console.WriteLine("--- Progman children (WorkerW only) ---");
        bool any = false;
        for (IntPtr k = GetWindow(shell, 5); k != IntPtr.Zero; k = GetWindow(k, 2)) {
            if (Cls(k) != "WorkerW") continue;
            any = true;
            bool full = Full(k);
            Console.WriteLine("  " + Desc(k) + (full ? "  FULLSCREEN" : ""));
            if (full) separateWallpaper = true;
        }
        if (!any) Console.WriteLine("  (none)");

        Console.WriteLine("");
        Console.WriteLine("separate wallpaper WorkerW exists (host excluded): " + separateWallpaper);
        Console.WriteLine("Driftlet current code would send 0x052C: " + (host == shell));
        Console.WriteLine("Driftlet current existence check (host NOT excluded) sees wallpaper window: " +
            (separateWallpaper || (host != IntPtr.Zero && host != shell && Full(host))));
    }
}
"@
[Topo]::Run()
$cv = Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion"
"windows: $($cv.ProductName) $($cv.DisplayVersion) build $($cv.CurrentBuild).$($cv.UBR)"
