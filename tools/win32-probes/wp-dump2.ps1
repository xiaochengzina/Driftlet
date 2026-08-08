# wp-dump2.ps1 — full wallpaper-layer diagnostics for machines where the
# wallpaper layer fails ("skin completely invisible"). READ-ONLY.
#
# Run on the FAILING machine while a skin is switched to wallpaper layer:
#   powershell -ExecutionPolicy Bypass -File wp-dump2.ps1
#   powershell -ExecutionPolicy Bypass -File wp-dump2.ps1 > wp-dump2.txt
# ASCII-only output. Paste the whole output back for analysis.
param([string]$ProcessName = "driftlet")

Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;

public class W2 {
    [DllImport("user32.dll")] public static extern IntPtr GetDesktopWindow();
    [DllImport("user32.dll")] public static extern IntPtr GetShellWindow();
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string t);
    [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h, uint cmd);
    [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint flags);
    [DllImport("user32.dll")] public static extern IntPtr GetWindowLongPtrW(IntPtr h, int i);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr p, EnumProc cb, IntPtr l);
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
    [DllImport("user32.dll")] public static extern uint GetDpiForSystem();
    [DllImport("shcore.dll")] public static extern int GetProcessDpiAwareness(IntPtr hproc, out int a);
    [DllImport("kernel32.dll")] public static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
    [DllImport("kernel32.dll")] public static extern bool CloseHandle(IntPtr h);
    [DllImport("advapi32.dll")] public static extern bool OpenProcessToken(IntPtr hproc, uint access, out IntPtr tok);
    [DllImport("advapi32.dll")] public static extern bool GetTokenInformation(IntPtr tok, int cls, out int buf, int len, out int ret);

    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }

    public static string Cls(IntPtr h) {
        var c = new StringBuilder(64); GetClassNameW(h, c, 64); return c.ToString();
    }

    public static string Desc(IntPtr h) {
        if (h == IntPtr.Zero) return "0x0";
        var t = new StringBuilder(50); GetWindowTextW(h, t, 50);
        uint pid; GetWindowThreadProcessId(h, out pid);
        long st = GetWindowLongPtrW(h, -16).ToInt64();
        long ex = GetWindowLongPtrW(h, -20).ToInt64();
        RECT r; GetWindowRect(h, out r);
        return string.Format("0x{0:X} pid={1} {2} [{3}] st=0x{4:X8} ex=0x{5:X8}{6}{7}{8} IsVis={9} rect=({10},{11})-({12},{13})",
            h.ToInt64(), pid, Cls(h), t.ToString(), st, ex,
            (st & 0x40000000L) != 0 ? " CHILD" : " TOP",
            (st & 0x10000000L) != 0 ? " +VIS" : " -VIS",
            (ex & 0x00080000L) != 0 ? " LAYERED" : "",
            IsWindowVisible(h), r.L, r.T, r.R, r.B);
    }

    public static string Tree(IntPtr root, uint markPid) {
        var sb = new StringBuilder();
        EnumProc cb = (h, l) => {
            uint pid; GetWindowThreadProcessId(h, out pid);
            int depth = 0;
            for (IntPtr p = GetParent(h); p != IntPtr.Zero && p != root; p = GetParent(p)) depth++;
            sb.AppendLine("  " + new string(' ', depth * 2) + (pid == markPid ? ">> " : "   ") + Desc(h));
            return true;
        };
        EnumChildWindows(root, cb, IntPtr.Zero);
        GC.KeepAlive(cb);
        return sb.ToString();
    }

    // direct children of parent in z-order (top first)
    public static string KidsZ(IntPtr parent, uint markPid) {
        var sb = new StringBuilder();
        int i = 0;
        for (IntPtr k = GetWindow(parent, 5); k != IntPtr.Zero; k = GetWindow(k, 2), i++) {
            uint pid; GetWindowThreadProcessId(k, out pid);
            sb.AppendLine(string.Format("  [{0}] {1}{2}", i, pid == markPid ? ">> " : "   ", Desc(k)));
            if (i > 25) { sb.AppendLine("  ..."); break; }
        }
        if (i == 0) sb.AppendLine("  (no children)");
        return sb.ToString();
    }

    public static string Elevation(uint pid) {
        IntPtr hproc = OpenProcess(0x0400, false, pid);
        if (hproc == IntPtr.Zero) return "n/a (cannot open process)";
        IntPtr tok;
        string r = "n/a";
        if (OpenProcessToken(hproc, 0x0008, out tok)) {
            int v, ret;
            if (GetTokenInformation(tok, 18, out v, 4, out ret))
                r = v == 2 ? "ELEVATED" : (v == 3 ? "not-elevated(limited)" : "not-elevated(default)");
            CloseHandle(tok);
        }
        CloseHandle(hproc);
        return r;
    }

    public static string DpiAware(uint pid) {
        IntPtr hproc = OpenProcess(0x0400, false, pid);
        if (hproc == IntPtr.Zero) return "n/a";
        int a;
        string r = GetProcessDpiAwareness(hproc, out a) == 0
            ? (a == 0 ? "unaware" : (a == 1 ? "system" : "per-monitor")) : "n/a";
        CloseHandle(hproc);
        return r;
    }

    static bool IsProgOrWorker(IntPtr h) {
        string c = Cls(h);
        return c == "Progman" || c == "WorkerW";
    }

    public static string DumpAll(uint markPid) {
        var sb = new StringBuilder();
        IntPtr shell = GetShellWindow();
        IntPtr desk = GetDesktopWindow();
        sb.AppendLine("shell window: " + Desc(shell));

        // --- top-level Progman/WorkerW list + locate DefView host ---
        sb.AppendLine("--- top-level Progman/WorkerW ---");
        var tops = new List<IntPtr>();
        EnumProc topCb = (h, l) => { tops.Add(h); return true; };
        EnumWindows(topCb, IntPtr.Zero);
        GC.KeepAlive(topCb);
        IntPtr dvHost = IntPtr.Zero, dv = IntPtr.Zero;
        foreach (var h in tops) {
            if (!IsProgOrWorker(h)) continue;
            sb.AppendLine("  " + Desc(h));
            IntPtr d = FindWindowExW(h, IntPtr.Zero, "SHELLDLL_DefView", null);
            if (d != IntPtr.Zero && dvHost == IntPtr.Zero) {
                dvHost = h; dv = d;
                sb.AppendLine("    ^ hosts SHELLDLL_DefView");
            }
        }
        if (dvHost == IntPtr.Zero) {
            sb.AppendLine("!! NO SHELLDLL_DefView under any top-level Progman/WorkerW");
            sb.AppendLine("   (desktop icons hidden shell-wide? shell replacement?)");
        } else {
            sb.AppendLine("DefView: " + Desc(dv));
            IntPtr lv = FindWindowExW(dv, IntPtr.Zero, "SysListView32", null);
            sb.AppendLine("ListView: " + Desc(lv));
            sb.AppendLine("--- DefView host direct children, z-order top-first ---");
            sb.Append(KidsZ(dvHost, markPid));
            sb.AppendLine("--- DefView host tree ---");
            sb.Append(Tree(dvHost, markPid));
        }
        // every OTHER top-level WorkerW tree too (wallpaper candidates)
        foreach (var h in tops) {
            if (!IsProgOrWorker(h) || h == dvHost || h == shell) continue;
            if (Cls(h) != "WorkerW") continue;
            sb.AppendLine("--- other top-level WorkerW " + string.Format("0x{0:X}", h.ToInt64()) + " tree ---");
            sb.Append(Tree(h, markPid));
        }

        // --- every driftlet window anywhere + parent chain + hit test ---
        sb.AppendLine("--- all driftlet windows anywhere (class Tauri Window) ---");
        var mine = new List<IntPtr>();
        EnumProc allCb = (h, l) => {
            uint pid; GetWindowThreadProcessId(h, out pid);
            if (pid == markPid && Cls(h) == "Tauri Window") mine.Add(h);
            return true;
        };
        EnumChildWindows(desk, allCb, IntPtr.Zero);
        GC.KeepAlive(allCb);
        if (mine.Count == 0) sb.AppendLine("!! no Tauri Window of this pid in the whole desktop tree");
        foreach (var h in mine) {
            sb.AppendLine("window: " + Desc(h));
            sb.AppendLine("  root ancestor: " + Desc(GetAncestor(h, 2)));
            for (IntPtr p = GetParent(h); p != IntPtr.Zero; p = GetParent(p))
                sb.AppendLine("  parent: " + Desc(p));
            RECT r; GetWindowRect(h, out r);
            if (r.R > r.L && r.B > r.T) {
                var pt = new POINT { X = (r.L + r.R) / 2, Y = (r.T + r.B) / 2 };
                IntPtr hit = WindowFromPoint(pt);
                sb.AppendLine("  WindowFromPoint(center): " + Desc(hit));
                if (hit != IntPtr.Zero)
                    sb.AppendLine("    its root: " + Desc(GetAncestor(hit, 2)));
            }
            IntPtr par = GetParent(h);
            if (par != IntPtr.Zero && par != desk) {
                sb.AppendLine("  --- siblings (parent's children, z top-first) ---");
                sb.Append(KidsZ(par, markPid));
            }
        }
        return sb.ToString();
    }
}
"@

"=== 1. OS / runtime ==="
$cv = Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion"
"windows: $($cv.ProductName) $($cv.DisplayVersion) build $($cv.CurrentBuild).$($cv.UBR)"
foreach ($root in "HKLM:\SOFTWARE","HKLM:\SOFTWARE\WOW6432Node","HKCU:\SOFTWARE") {
  $p = "$root\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"
  $wv = Get-ItemProperty $p -ErrorAction SilentlyContinue
  if ($wv) { "webview2 runtime: $($wv.pv)  ($root)" }
}
"monitors: " + [W2]::GetSystemMetrics(80) + "  primary: " + [W2]::GetSystemMetrics(0) + "x" + [W2]::GetSystemMetrics(1) + "  system DPI: " + [W2]::GetDpiForSystem()

""
"=== 2. processes ==="
$exp = Get-Process -Name explorer -ErrorAction SilentlyContinue | Select-Object -First 1
if ($exp) { "explorer: pid=$($exp.Id) elevation=" + [W2]::Elevation([uint32]$exp.Id) }
$procs = Get-Process -Name $ProcessName -ErrorAction SilentlyContinue
if (-not $procs) { "!! no $ProcessName process - start Driftlet and switch a skin to wallpaper layer first"; exit 1 }
$target = $procs[0]
$targetPid = [uint32]$target.Id
"driftlet: pid=$($target.Id) path=$($target.Path)"
"driftlet elevation=" + [W2]::Elevation($targetPid) + " dpi-awareness=" + [W2]::DpiAware($targetPid)

""
"=== 3. shell topology + driftlet windows ==="
[W2]::DumpAll($targetPid)

""
"=== done ==="
