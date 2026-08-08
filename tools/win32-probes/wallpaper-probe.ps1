# wallpaper-probe.ps1 — wallpaper-layer mechanism probe (ASCII-only).
#
# Validates the "behind the desktop icons" trick on a LIVE skin window:
#   1. Find the skin window (same logic as ctprobe).
#   2. Send Progman 0x052C -> explorer spawns a WorkerW behind the icons.
#   3. SetParent(skin, that WorkerW), keep the same screen position.
#   4. Hit grid: WindowFromPoint over the skin rect should now hit the
#      desktop icons host (explorer) instead of the skin = mouse-immune.
#   5. Hold -Seconds for visual confirmation, then restore (SetParent NULL).
#
# Prereq: skin loaded, preferably in always-on-top mode (keeps the desktop
# pinner's z-glue from interfering), at a spot not covered by other windows.
# Usage: powershell -File wallpaper-probe.ps1 [-Title s] [-Seconds 8]
param(
  [string]$ProcessName = "driftlet",
  [string]$Title = "",
  [int]$Seconds = 8
)

Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;

public class WpProbe {
    [DllImport("user32.dll")] public static extern IntPtr GetDesktopWindow();
    [DllImport("user32.dll")] public static extern IntPtr GetShellWindow();
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string t);
    [DllImport("user32.dll")] public static extern IntPtr SendMessageTimeoutW(IntPtr h, uint msg, IntPtr w, IntPtr l, uint flags, uint timeout, out IntPtr result);
    [DllImport("user32.dll")] public static extern IntPtr GetWindowLongPtrW(IntPtr h, int i);
    [DllImport("user32.dll")] public static extern IntPtr SetWindowLongPtrW(IntPtr h, int i, IntPtr v);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
    [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint flags);
    [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr SetParent(IntPtr h, IntPtr p);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr ins, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] public static extern bool ScreenToClient(IntPtr h, ref POINT p);
    [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr p, EnumProc cb, IntPtr l);

    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }

    const int GWL_STYLE = -16;

    public static string Desc(IntPtr h) {
        if (h == IntPtr.Zero) return "0x0";
        var c = new StringBuilder(64); GetClassNameW(h, c, 64);
        var t = new StringBuilder(60); GetWindowTextW(h, t, 60);
        uint pid; GetWindowThreadProcessId(h, out pid);
        return string.Format("0x{0:X} pid={1} {2} [{3}] vis={4}", h.ToInt64(), pid, c.ToString(), t.ToString(), IsWindowVisible(h));
    }

    // Find the WorkerW sitting behind the desktop icons: classic recipe —
    // after Progman gets 0x052C, a WorkerW appears as the next top-level
    // sibling of the window that contains SHELLDLL_DefView.
    public static IntPtr FindWallpaperWorkerW(out string log) {
        var sb = new StringBuilder();
        IntPtr progman = GetShellWindow(); // Progman
        IntPtr res;
        // two known param variants (Win11 RE / older recipes)
        SendMessageTimeoutW(progman, 0x052C, new IntPtr(0xD), new IntPtr(0x1), 0, 2000, out res);
        System.Threading.Thread.Sleep(300);
        SendMessageTimeoutW(progman, 0x052C, IntPtr.Zero, IntPtr.Zero, 0, 2000, out res);
        sb.AppendLine("  0x052C sent to " + Desc(progman) + " (both variants)");
        // poll up to 3s for a WorkerW to appear
        IntPtr workerw = IntPtr.Zero;
        for (int i = 0; i < 10 && workerw == IntPtr.Zero; i++) {
            IntPtr h = IntPtr.Zero;
            while (true) {
                h = FindWindowExW(IntPtr.Zero, h, null, null);
                if (h == IntPtr.Zero) break;
                IntPtr dv = FindWindowExW(h, IntPtr.Zero, "SHELLDLL_DefView", null);
                if (dv != IntPtr.Zero) {
                    workerw = FindWindowExW(IntPtr.Zero, h, "WorkerW", null);
                    if (i == 0) sb.AppendLine("  icons host: " + Desc(h));
                    break;
                }
            }
            if (workerw == IntPtr.Zero) System.Threading.Thread.Sleep(300);
        }
        // dump all top-level Progman/WorkerW for diagnosis
        sb.AppendLine("  top-level Progman/WorkerW windows:");
        IntPtr t = IntPtr.Zero;
        while (true) {
            t = FindWindowExW(IntPtr.Zero, t, null, null);
            if (t == IntPtr.Zero) break;
            var c = new StringBuilder(64); GetClassNameW(t, c, 64);
            if (c.ToString() == "Progman" || c.ToString() == "WorkerW") {
                RECT r; GetWindowRect(t, out r);
                sb.AppendLine(string.Format("    {0} rect=({1},{2})-({3},{4})", Desc(t), r.L, r.T, r.R, r.B));
            }
        }
        log = sb.ToString();
        return workerw;
    }

    // Find the skin window: full-tree scan, class Tauri Window, title skips
    // 'Driftlet', visible, optionally filtered by needle.
    public static IntPtr SkinFound = IntPtr.Zero;
    public static string FindSkin(uint targetPid, string titleNeedle) {
        SkinFound = IntPtr.Zero;
        EnumProc cb = (h, l) => {
            uint pid; GetWindowThreadProcessId(h, out pid);
            if (pid != targetPid) return true;
            if (SkinFound == IntPtr.Zero) {
                var c = new StringBuilder(64); GetClassNameW(h, c, 64);
                var t = new StringBuilder(80); GetWindowTextW(h, t, 80);
                bool skip = t.ToString().Contains("Driftlet");
                bool needle = titleNeedle == "" || t.ToString().Contains(titleNeedle);
                if (c.ToString() == "Tauri Window" && !skip && needle && IsWindowVisible(h))
                    SkinFound = h;
            }
            return true;
        };
        EnumChildWindows(GetDesktopWindow(), cb, IntPtr.Zero);
        GC.KeepAlive(cb);
        return SkinFound == IntPtr.Zero ? "" : Desc(SkinFound);
    }

    public static string MapHits(RECT r, IntPtr skin) {
        var sb = new StringBuilder();
        var roots = new List<long>();
        for (int gy = 0; gy < 3; gy++) {
            var line = new StringBuilder("  ");
            for (int gx = 0; gx < 5; gx++) {
                int px = r.L + (r.R - r.L) * (1 + 2 * gx) / 10;
                int py = r.T + (r.B - r.T) * (1 + 2 * gy) / 6;
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
        foreach (var key in roots)
            sb.AppendLine("    [" + roots.IndexOf(key) + "] " + Desc(new IntPtr(key)));
        return sb.ToString();
    }
}
"@

$procs = Get-Process -Name $ProcessName -ErrorAction SilentlyContinue
if (-not $procs) { "no $ProcessName process"; exit 1 }
$targetPid = $procs[0].Id

# --- locate the skin window ---
$desc = [WpProbe]::FindSkin([uint32]$targetPid, $Title)
$skin = [WpProbe]::SkinFound
if ($skin -eq [IntPtr]::Zero) { "skin window not found - load a skin first"; exit 1 }
"skin: $desc"

$r = New-Object WpProbe+RECT
[void][WpProbe]::GetWindowRect($skin, [ref]$r)
"rect = ($($r.L),$($r.T))-($($r.R),$($r.B))"

"--- hit grid BEFORE ---"
[WpProbe]::MapHits($r, $skin)

"--- spawn/find wallpaper WorkerW ---"
$log = ""
$workerw = [WpProbe]::FindWallpaperWorkerW([ref]$log)
$log
if ($workerw -eq [IntPtr]::Zero) {
    "WorkerW not spawned - fallback: parent into Progman itself, sink below SHELLDLL_DefView"
    $host_ = [WpProbe]::GetShellWindow()
    $sinkBelow = $true
} else {
    "workerw: " + [WpProbe]::Desc($workerw)
    $host_ = $workerw
    $sinkBelow = $false
}

"--- SetParent(skin, host) ---"
$oldStyle = [WpProbe]::GetWindowLongPtrW($skin, -16)
# convert popup to child so it lives properly inside the host
[WpProbe]::SetWindowLongPtrW($skin, -16, [IntPtr](($oldStyle.ToInt64() -band -bnot 0x80000000L) -bor 0x40000000L)) | Out-Null
$prev = [WpProbe]::SetParent($skin, $host_)
"previous parent: " + [WpProbe]::Desc($prev)
# keep the same screen position (child coords are relative to the host client)
$pt = New-Object WpProbe+POINT
$pt.X = $r.L; $pt.Y = $r.T
[void][WpProbe]::ScreenToClient($host_, [ref]$pt)
[void][WpProbe]::SetWindowPos($skin, [IntPtr]::Zero, $pt.X, $pt.Y, ($r.R - $r.L), ($r.B - $r.T), 0x0010 -bor 0x0040) # NOACTIVATE|SHOWWINDOW
if ($sinkBelow) {
    # HWND_BOTTOM = 1: sink under all siblings (below the icons DefView)
    [void][WpProbe]::SetWindowPos($skin, [IntPtr]1, 0, 0, 0, 0, 0x0002 -bor 0x0001 -bor 0x0010)
}
"new parent: " + [WpProbe]::Desc([WpProbe]::GetParent($skin))

"--- hit grid AFTER (wallpaper layer) ---"
[WpProbe]::MapHits($r, $skin)

"holding $Seconds s for visual confirmation (skin should be visible BEHIND desktop icons)..."
Start-Sleep -Seconds $Seconds

"--- restore ---"
[WpProbe]::SetParent($skin, [IntPtr]::Zero) | Out-Null
[WpProbe]::SetWindowLongPtrW($skin, -16, $oldStyle) | Out-Null
[void][WpProbe]::SetWindowPos($skin, [IntPtr]::Zero, $r.L, $r.T, ($r.R - $r.L), ($r.B - $r.T), 0x0010 -bor 0x0040)
"restored. parent now: " + [WpProbe]::Desc([WpProbe]::GetParent($skin))
