# wp-black-diag.ps1 — diagnose wallpaper-layer black-background scene (ASCII-only).
# Dumps the live desktop topology and the skin window's full parent chain:
#   - Progman + DefView location, all top-level WorkerW windows (style, rect, vis)
#   - every Driftlet-owned window anywhere under the desktop, and for skin
#     windows (class "Tauri Window") the complete ancestor chain with each
#     level's siblings, flagging WS_CLIPCHILDREN / WS_CLIPSIBLINGS so we can
#     see whether the skin's ACTUAL parent still has the clip bit (declip
#     failed / re-set by explorer) or the skin landed somewhere unexpected.
# Prereq: a skin switched to wallpaper layer and black background visible.
# Usage: powershell -ExecutionPolicy Bypass -File wp-black-diag.ps1
param([string]$ProcessName = "driftlet")

Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;

public class BDiag {
    [DllImport("user32.dll")] public static extern IntPtr GetDesktopWindow();
    [DllImport("user32.dll")] public static extern IntPtr GetShellWindow();
    [DllImport("user32.dll")] public static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string t);
    [DllImport("user32.dll")] public static extern IntPtr GetWindowLongPtrW(IntPtr h, int i);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr p, EnumProc cb, IntPtr l);

    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }

    public static string Desc(IntPtr h) {
        if (h == IntPtr.Zero) return "0x0";
        var c = new StringBuilder(64); GetClassNameW(h, c, 64);
        var t = new StringBuilder(30); GetWindowTextW(h, t, 30);
        uint pid; GetWindowThreadProcessId(h, out pid);
        long st = GetWindowLongPtrW(h, -16).ToInt64();
        RECT r; GetWindowRect(h, out r);
        string bits = "";
        if ((st & 0x02000000) != 0) bits += " CLIPCHILDREN";
        if ((st & 0x04000000) != 0) bits += " CLIPSIBLINGS";
        if ((st & 0x40000000) != 0) bits += " CHILD";
        if ((st & 0x10000000) != 0) bits += " VIS";
        return string.Format("0x{0:X} pid={1} {2} [{3}] st=0x{4:X8} vis={5} rect=({6},{7})-({8},{9}){10}",
            h.ToInt64(), pid, c.ToString(), t.ToString(), st, IsWindowVisible(h),
            r.L, r.T, r.R, r.B, bits);
    }

    public static string ClassOf(IntPtr h) {
        var c = new StringBuilder(64); GetClassNameW(h, c, 64);
        return c.ToString();
    }

    public static void DumpChildrenMark(IntPtr root, uint markPid, string indent) {
        EnumProc cb = (h, l) => {
            uint pid; GetWindowThreadProcessId(h, out pid);
            Console.WriteLine(indent + (pid == markPid ? ">> " : "   ") + Desc(h));
            return true;
        };
        EnumChildWindows(root, cb, IntPtr.Zero);
        GC.KeepAlive(cb);
    }

    public static void DumpChain(IntPtr h) {
        int d = 0;
        while (h != IntPtr.Zero && d < 8) {
            Console.WriteLine("  " + Desc(h));
            IntPtr p = GetParent(h);
            if (p != IntPtr.Zero) {
                IntPtr me = h;
                EnumProc cb = (s, l) => {
                    Console.WriteLine("    " + (s == me ? ">> " : "   ") + Desc(s));
                    return true;
                };
                EnumChildWindows(p, cb, IntPtr.Zero);
                GC.KeepAlive(cb);
            }
            h = p; d++;
        }
    }

    // Find every window owned by markPid anywhere under the desktop; for
    // "Tauri Window" class ones that are NOT top-level (i.e. pinned skins),
    // dump the full ancestor chain.
    public static void HuntSkins(uint markPid) {
        EnumProc cb = (h, l) => {
            uint pid; GetWindowThreadProcessId(h, out pid);
            if (pid == markPid && ClassOf(h) == "Tauri Window" && GetParent(h) != IntPtr.Zero) {
                Console.WriteLine("=== pinned skin chain ===");
                DumpChain(h);
            }
            return true;
        };
        EnumChildWindows(GetDesktopWindow(), cb, IntPtr.Zero);
        GC.KeepAlive(cb);
    }

    public static bool Full(IntPtr h) {
        if (!IsWindowVisible(h)) return false;
        RECT r; if (!GetWindowRect(h, out r)) return false;
        return (r.R - r.L) >= GetSystemMetrics(0) && (r.B - r.T) >= GetSystemMetrics(1);
    }
}
"@

$procs = Get-Process -Name $ProcessName -ErrorAction SilentlyContinue
if (-not $procs) { "no $ProcessName process running"; exit 1 }
$tpid = [uint32]$procs[0].Id
"driftlet pid: $tpid"
""

$progman = [BDiag]::GetShellWindow()
"progman: " + [BDiag]::Desc($progman)
"--- progman children ---"
[BDiag]::DumpChildrenMark($progman, $tpid, "  ")

"--- top-level WorkerW list ---"
$w = [IntPtr]::Zero
$found = $false
while ($true) {
    $w = [BDiag]::FindWindowExW([IntPtr]::Zero, $w, "WorkerW", $null)
    if ($w -eq [IntPtr]::Zero) { break }
    $found = $true
    $full = [BDiag]::Full($w)
    "workerw: " + [BDiag]::Desc($w) + $(if ($full) { "  <== FULLSCREEN" } else { "" })
    [BDiag]::DumpChildrenMark($w, $tpid, "  ")
}
if (-not $found) { "  (none)" }

"--- pinned skin parent chains ---"
[BDiag]::HuntSkins($tpid)
