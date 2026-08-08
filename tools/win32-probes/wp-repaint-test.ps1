# wp-repaint-test.ps1 — force the wallpaper WorkerW to repaint (strong variant).
# InvalidateRect + UpdateWindow (SendMessage WM_PAINT), then SetWindowPos nudge.
# Prints each API result. If ghosts still persist afterwards, the wallpaper is
# not WM_PAINT-driven (DComp-rendered) and GDI repaints can never fix them.
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public class WRepaint {
    [DllImport("user32.dll")] public static extern IntPtr GetShellWindow();
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string t);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
    [DllImport("user32.dll")] public static extern bool InvalidateRect(IntPtr h, IntPtr r, bool erase);
    [DllImport("user32.dll")] public static extern bool UpdateWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool RedrawWindow(IntPtr h, IntPtr r, IntPtr rgn, uint flags);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr ins, int x, int y, int cx, int cy, uint flags);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }

    public static IntPtr FindWallpaper() {
        IntPtr shell = GetShellWindow();
        IntPtr host = IntPtr.Zero;
        if (FindWindowExW(shell, IntPtr.Zero, "SHELLDLL_DefView", null) != IntPtr.Zero) host = shell;
        else {
            IntPtr w = IntPtr.Zero;
            while (true) {
                w = FindWindowExW(IntPtr.Zero, w, "WorkerW", null);
                if (w == IntPtr.Zero) break;
                if (FindWindowExW(w, IntPtr.Zero, "SHELLDLL_DefView", null) != IntPtr.Zero) { host = w; break; }
            }
        }
        IntPtr t = IntPtr.Zero;
        while (true) {
            t = FindWindowExW(IntPtr.Zero, t, "WorkerW", null);
            if (t == IntPtr.Zero) return IntPtr.Zero;
            if (t == host || !IsWindowVisible(t)) continue;
            RECT r; if (!GetWindowRect(t, out r)) continue;
            if ((r.R - r.L) >= GetSystemMetrics(0) && (r.B - r.T) >= GetSystemMetrics(1)) return t;
        }
    }
}
"@
$ww = [WRepaint]::FindWallpaper()
if ($ww -eq [IntPtr]::Zero) { "no wallpaper WorkerW"; exit 1 }
"wallpaper WorkerW: 0x{0:X}" -f $ww.ToInt64()
"InvalidateRect: " + [WRepaint]::InvalidateRect($ww, [IntPtr]::Zero, $true)
"UpdateWindow:   " + [WRepaint]::UpdateWindow($ww)
# RDW_INVALIDATE|RDW_ERASE|RDW_UPDATENOW|RDW_ALLCHILDREN = 0x1|0x4|0x100|0x80
"RedrawWindow:   " + [WRepaint]::RedrawWindow($ww, [IntPtr]::Zero, [IntPtr]::Zero, 0x185)
"done — check the desktop for remaining black ghosts"
