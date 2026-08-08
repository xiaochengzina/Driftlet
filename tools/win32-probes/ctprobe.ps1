# ctprobe.ps1 — click-through verification probe (ASCII-only on purpose:
# PS 5.1 misreads UTF-8-no-BOM as GBK and Chinese bytes break C# literals).
#
# 1. Lists every window of the target process (full tree from the desktop).
# 2. Grid-maps WindowFromPoint over the skin rect: S = hit root is the skin.
# 3. Experiment: set WS_EX_TRANSPARENT on the skin + ALL descendants
#    (originals saved), re-map, then restore and re-map.
# Usage: powershell -File ctprobe.ps1 [-Title substring] [-NoMutate]
param(
  [string]$ProcessName = "driftlet",
  [string]$Title = "",
  [switch]$NoMutate
)

Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;

public class CtProbe {
    [DllImport("user32.dll")] public static extern IntPtr GetDesktopWindow();
    [DllImport("user32.dll")] public static extern IntPtr GetWindowLongPtrW(IntPtr h, int i);
    [DllImport("user32.dll")] public static extern IntPtr SetWindowLongPtrW(IntPtr h, int i, IntPtr v);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
    [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint flags);
    [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
    [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr p, EnumProc cb, IntPtr l);

    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }

    const int GWL_EXSTYLE = -20;
    const long WS_EX_TRANSPARENT = 0x20;

    public static string Desc(IntPtr h) {
        var c = new StringBuilder(64); GetClassNameW(h, c, 64);
        var t = new StringBuilder(80); GetWindowTextW(h, t, 80);
        uint pid; uint tid = GetWindowThreadProcessId(h, out pid);
        long ex = GetWindowLongPtrW(h, GWL_EXSTYLE).ToInt64();
        return string.Format("0x{0:X} pid={1} tid={2} {3} [{4}] ex=0x{5:X8} transparent={6} vis={7}",
            h.ToInt64(), pid, tid, c.ToString(), t.ToString(), ex, (ex & WS_EX_TRANSPARENT) != 0, IsWindowVisible(h));
    }

    // Full-tree scan from the desktop: list every window owned by pid; pick the
    // skin = visible window of class wclass whose title skips skipTitle and
    // contains titleNeedle.
    public static IntPtr SkinFound = IntPtr.Zero;
    public static string ScanAll(uint targetPid, string wclass, string skipTitle, string titleNeedle) {
        SkinFound = IntPtr.Zero;
        var sb = new StringBuilder();
        int shown = 0;
        EnumProc cb = (h, l) => {
            uint pid; GetWindowThreadProcessId(h, out pid);
            if (pid != targetPid) return true;
            if (shown < 60) { sb.AppendLine("  " + Desc(h)); shown++; }
            if (SkinFound == IntPtr.Zero) {
                var c = new StringBuilder(64); GetClassNameW(h, c, 64);
                var t = new StringBuilder(80); GetWindowTextW(h, t, 80);
                bool classOk = wclass == "" || c.ToString() == wclass;
                bool skip = skipTitle != "" && t.ToString().Contains(skipTitle);
                bool needle = titleNeedle == "" || t.ToString().Contains(titleNeedle);
                if (classOk && !skip && needle && IsWindowVisible(h)) SkinFound = h;
            }
            return true;
        };
        EnumChildWindows(GetDesktopWindow(), cb, IntPtr.Zero);
        GC.KeepAlive(cb);
        return sb.ToString();
    }

    public static string DumpTree(IntPtr root) {
        var sb = new StringBuilder();
        EnumProc cb = (h, l) => { sb.AppendLine("  " + Desc(h)); return true; };
        EnumChildWindows(root, cb, IntPtr.Zero);
        GC.KeepAlive(cb);
        return sb.ToString();
    }

    // 5x5 grid of WindowFromPoint over the rect:
    //   S = hit root is the skin (not passing through at this point)
    //   _ = hit nothing (desktop background = passed through)
    //   N = hit some other top-level window (occluded; legend lists roots)
    public static string MapHits(RECT r, IntPtr skin) {
        var sb = new StringBuilder();
        var roots = new List<long>();
        for (int gy = 0; gy < 5; gy++) {
            var line = new StringBuilder("  ");
            for (int gx = 0; gx < 5; gx++) {
                int px = r.L + (r.R - r.L) * (1 + 2 * gx) / 10;
                int py = r.T + (r.B - r.T) * (1 + 2 * gy) / 10;
                var pt = new POINT { X = px, Y = py };
                IntPtr hit = WindowFromPoint(pt);
                if (hit == IntPtr.Zero) { line.Append("_ "); continue; }
                IntPtr root = GetAncestor(hit, 2);
                if (root == skin) { line.Append("S "); continue; }
                long key = root.ToInt64();
                if (!roots.Contains(key)) roots.Add(key);
                line.Append(roots.IndexOf(key).ToString() + " ");
            }
            sb.AppendLine(line.ToString());
        }
        sb.AppendLine("  legend: S=skin _=desktop-bg N=other root below:");
        foreach (var key in roots)
            sb.AppendLine("    [" + roots.IndexOf(key) + "] " + Desc(new IntPtr(key)));
        return sb.ToString();
    }

    public static Dictionary<long, long> Saved = new Dictionary<long, long>();
    public static string SetTransparentTree(IntPtr root, bool on, out int ok, out int fail) {
        ok = 0; fail = 0;
        var targets = new List<IntPtr> { root };
        EnumProc cb = (h, l) => { targets.Add(h); return true; };
        EnumChildWindows(root, cb, IntPtr.Zero);
        GC.KeepAlive(cb);
        var sb = new StringBuilder();
        foreach (var h in targets) {
            long key = h.ToInt64();
            long ex = GetWindowLongPtrW(h, GWL_EXSTYLE).ToInt64();
            if (!Saved.ContainsKey(key)) Saved[key] = ex;
            long want = on ? (ex | WS_EX_TRANSPARENT) : Saved[key];
            SetWindowLongPtrW(h, GWL_EXSTYLE, new IntPtr(want));
            long got = GetWindowLongPtrW(h, GWL_EXSTYLE).ToInt64();
            bool good = (got & WS_EX_TRANSPARENT) == (want & WS_EX_TRANSPARENT);
            if (good) ok++; else fail++;
            if (!good) sb.AppendLine(string.Format("  0x{0:X} ex 0x{1:X8} -> 0x{2:X8} << SET FAILED", h.ToInt64(), ex, got));
        }
        return sb.ToString();
    }
}
"@

$procs = Get-Process -Name $ProcessName -ErrorAction SilentlyContinue
if (-not $procs) { "no $ProcessName process"; exit 1 }
$targetPid = $procs[0].Id
"pid = $targetPid"

"--- all windows of this process (full tree) ---"
[CtProbe]::ScanAll([uint32]$targetPid, "Tauri Window", "Driftlet", $Title)

$skin = [CtProbe]::SkinFound
if ($skin -eq [IntPtr]::Zero) {
    "skin window not found (class=Tauri Window, title not containing 'Driftlet', visible) - load a skin or use -Title"
    exit 1
}
"--- target skin window ---"
"  " + [CtProbe]::Desc($skin)
"  parent: " + [CtProbe]::Desc([CtProbe]::GetParent($skin))

$r = New-Object CtProbe+RECT
[void][CtProbe]::GetWindowRect($skin, [ref]$r)
"rect = ($($r.L),$($r.T))-($($r.R),$($r.B))"

"--- child tree ---"
[CtProbe]::DumpTree($skin)

"--- hit grid BEFORE (S=skin _=desktop N=occluded/other) ---"
[CtProbe]::MapHits($r, $skin)

if (-not $NoMutate) {
    "--- experiment: set WS_EX_TRANSPARENT on skin + all descendants ---"
    $ok = 0; $fail = 0
    [CtProbe]::SetTransparentTree($skin, $true, [ref]$ok, [ref]$fail)
    "set: ok=$ok fail=$fail"
    Start-Sleep -Milliseconds 300
    "--- hit grid AFTER (all transparent) ---"
    [CtProbe]::MapHits($r, $skin)
    "--- restore ---"
    [CtProbe]::SetTransparentTree($skin, $false, [ref]$ok, [ref]$fail)
    "restore: ok=$ok fail=$fail"
    Start-Sleep -Milliseconds 300
    [CtProbe]::MapHits($r, $skin)
    "--- experiment 2: top-level WS_EX_LAYERED|WS_EX_TRANSPARENT (classic combo) ---"
    $exBefore = [CtProbe]::GetWindowLongPtrW($skin, -20)
    [CtProbe]::SetWindowLongPtrW($skin, -20, [IntPtr]($exBefore.ToInt64() -bor 0x80020)) | Out-Null
    Start-Sleep -Milliseconds 300
    [CtProbe]::MapHits($r, $skin)
    "--- restore exstyle ---"
    [CtProbe]::SetWindowLongPtrW($skin, -20, $exBefore) | Out-Null
    Start-Sleep -Milliseconds 300
    [CtProbe]::MapHits($r, $skin)
}
