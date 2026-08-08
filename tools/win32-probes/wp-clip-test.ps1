# wp-clip-test.ps1 — verify the WS_CLIPCHILDREN theory on the live desktop.
# Finds the wallpaper WorkerW (full-screen top-level != icons host), prints its
# style, CLEARS WS_CLIPCHILDREN, forces a full repaint, prints the new style.
# Expected: black ghosts at the skin's previous positions get repainted away,
# transparent corners of wallpaper-layer skins show the wallpaper again.
# Usage: powershell -ExecutionPolicy Bypass -File wp-clip-test.ps1            (clear + repaint)
#        powershell -ExecutionPolicy Bypass -File wp-clip-test.ps1 -Restore   (set the bit back)
param([switch]$Restore)

Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class WClip {
    [DllImport("user32.dll")] public static extern IntPtr GetShellWindow();
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string t);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
    [DllImport("user32.dll")] public static extern IntPtr GetWindowLongPtrW(IntPtr h, int i);
    [DllImport("user32.dll")] public static extern IntPtr SetWindowLongPtrW(IntPtr h, int i, IntPtr v);
    [DllImport("user32.dll")] public static extern bool RedrawWindow(IntPtr h, IntPtr r, IntPtr rgn, uint flags);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }

    const long WS_CLIPCHILDREN = 0x02000000;

    public static IntPtr FindIconsHost() {
        IntPtr shell = GetShellWindow();
        if (FindWindowExW(shell, IntPtr.Zero, "SHELLDLL_DefView", null) != IntPtr.Zero) return shell;
        IntPtr w = IntPtr.Zero;
        while (true) {
            w = FindWindowExW(IntPtr.Zero, w, "WorkerW", null);
            if (w == IntPtr.Zero) break;
            if (FindWindowExW(w, IntPtr.Zero, "SHELLDLL_DefView", null) != IntPtr.Zero) return w;
        }
        return IntPtr.Zero;
    }

    public static bool Full(IntPtr h) {
        if (!IsWindowVisible(h)) return false;
        RECT r; if (!GetWindowRect(h, out r)) return false;
        return (r.R - r.L) >= GetSystemMetrics(0) && (r.B - r.T) >= GetSystemMetrics(1);
    }

    public static void Run(bool restore) {
        IntPtr host = FindIconsHost();
        IntPtr ww = IntPtr.Zero;
        IntPtr t = IntPtr.Zero;
        while (true) {
            t = FindWindowExW(IntPtr.Zero, t, "WorkerW", null);
            if (t == IntPtr.Zero) break;
            if (t != host && Full(t)) { ww = t; break; }
        }
        if (ww == IntPtr.Zero) { Console.WriteLine("no wallpaper WorkerW found"); return; }
        long st = GetWindowLongPtrW(ww, -16).ToInt64();
        Console.WriteLine("wallpaper WorkerW 0x{0:X}  style before: 0x{1:X8}  clipchildren={2}",
            ww.ToInt64(), st, (st & WS_CLIPCHILDREN) != 0);
        long want = restore ? (st | WS_CLIPCHILDREN) : (st & ~WS_CLIPCHILDREN);
        SetWindowLongPtrW(ww, -16, new IntPtr(want));
        // RDW_INVALIDATE | RDW_ERASE: repaint the whole surface (ghosts included)
        RedrawWindow(ww, IntPtr.Zero, IntPtr.Zero, 0x1 | 0x4);
        long st2 = GetWindowLongPtrW(ww, -16).ToInt64();
        Console.WriteLine("style after:  0x{0:X8}  clipchildren={1}  ({2})",
            st2, (st2 & WS_CLIPCHILDREN) != 0, restore ? "restored" : "cleared + repainted");
    }
}
"@
[WClip]::Run([bool]$Restore)
