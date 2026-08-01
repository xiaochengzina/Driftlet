# End-to-end verification of the Win+D fix.
# Dumps the interesting z-order slice + a screenshot of the skin region
# at each stage: t0 -> Win+D(show) -> Win+D(restore) -> Win+D(show) again.

Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;
using System.Collections.Generic;
using System.Drawing;
using System.Drawing.Imaging;

public class V {
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetWindowLongPtrW(IntPtr h, int i);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr lp);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string t);
    [DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    delegate bool EnumProc(IntPtr h, IntPtr lp);

    public static string Dump() {
        var list = new List<IntPtr>();
        EnumWindows(delegate(IntPtr h, IntPtr lp) { list.Add(h); return true; }, IntPtr.Zero);
        var sb = new StringBuilder();
        int i = 0;
        foreach (var h in list) {
            var c = new StringBuilder(64); GetClassNameW(h, c, 64);
            string cls = c.ToString();
            bool interesting = cls == "WorkerW" || cls == "Progman" || cls == "Tauri Window"
                || cls == "DriftletDesktopHelper" || cls == "Shell_TrayWnd";
            if (!interesting) { i++; continue; }
            long ex = GetWindowLongPtrW(h, -20).ToInt64();
            bool topmost = (ex & 0x8) != 0;
            bool toolwin = (ex & 0x80) != 0;
            var t = new StringBuilder(48); GetWindowTextW(h, t, 48);
            string extra = "";
            if (cls == "WorkerW" && FindWindowExW(h, IntPtr.Zero, "SHELLDLL_DefView", null) != IntPtr.Zero)
                extra = " [ICONS-HOST]";
            string fg = (GetForegroundWindow() == h) ? " [FG]" : "";
            sb.AppendLine(string.Format("  {0:d3}{1} 0x{2:X8} {3} [{4}] vis={5} iconic={6}{7}{8}{9}",
                i, topmost ? "*" : " ", h.ToInt64(), cls, t.ToString(),
                IsWindowVisible(h), IsIconic(h), toolwin ? " tool" : "", extra, fg));
            i++;
        }
        return sb.ToString();
    }

    public static void WinD() {
        keybd_event(0x5B, 0, 0, UIntPtr.Zero);
        keybd_event(0x44, 0, 0, UIntPtr.Zero);
        keybd_event(0x44, 0, 2, UIntPtr.Zero);
        keybd_event(0x5B, 0, 2, UIntPtr.Zero);
    }

    public static void Shot(int x, int y, int w, int h, string path) {
        using (var bmp = new Bitmap(w, h, PixelFormat.Format32bppArgb))
        using (var g = Graphics.FromImage(bmp)) {
            g.CopyFromScreen(x, y, 0, 0, new Size(w, h), CopyPixelOperation.SourceCopy);
            bmp.Save(path, ImageFormat.Png);
        }
    }
}
"@

New-Item -ItemType Directory -Force -Path .debug-watch/out | Out-Null
$skinX = 1500; $skinY = 100; $skinW = 500; $skinH = 320

"=== t0 $(Get-Date -Format 'HH:mm:ss.fff') (pinned, normal) ==="
[V]::Dump()
[V]::Shot($skinX, $skinY, $skinW, $skinH, '.debug-watch/out/v0-normal.png')

"press Win+D (show) at $(Get-Date -Format 'HH:mm:ss.fff')"
[V]::WinD()
Start-Sleep -Milliseconds 1200
"=== after Win+D show $(Get-Date -Format 'HH:mm:ss.fff') ==="
[V]::Dump()
[V]::Shot($skinX, $skinY, $skinW, $skinH, '.debug-watch/out/v1-showdesktop.png')

"press Win+D (restore) at $(Get-Date -Format 'HH:mm:ss.fff')"
[V]::WinD()
Start-Sleep -Milliseconds 1200
"=== after restore $(Get-Date -Format 'HH:mm:ss.fff') ==="
[V]::Dump()
[V]::Shot($skinX, $skinY, $skinW, $skinH, '.debug-watch/out/v2-restored.png')

"press Win+D (show again) at $(Get-Date -Format 'HH:mm:ss.fff')"
[V]::WinD()
Start-Sleep -Milliseconds 1200
"=== after Win+D show #2 $(Get-Date -Format 'HH:mm:ss.fff') ==="
[V]::Dump()
[V]::Shot($skinX, $skinY, $skinW, $skinH, '.debug-watch/out/v3-showdesktop2.png')

"press Win+D (restore again) at $(Get-Date -Format 'HH:mm:ss.fff')"
[V]::WinD()
Start-Sleep -Milliseconds 1200
"=== after restore #2 $(Get-Date -Format 'HH:mm:ss.fff') ==="
[V]::Dump()
