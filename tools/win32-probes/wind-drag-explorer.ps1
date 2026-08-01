# Regression: (a) skin drag still works after the fix; (b) skin re-anchors
# after an explorer.exe restart. Prints skin rect + z-slice at each step.

Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;
using System.Collections.Generic;
using System.Drawing;
using System.Drawing.Imaging;

public class D3 {
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowW(string c, string t);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string t);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr lp);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    delegate bool EnumProc(IntPtr h, IntPtr lp);

    public static string SkinRect() {
        IntPtr skin = FindWindowW(null, "Example Clock");
        if (skin == IntPtr.Zero) return "skin NOT FOUND";
        RECT r; GetWindowRect(skin, out r);
        return string.Format("skin rect = [{0},{1} {2}x{3}] iconic={4} vis={5}",
            r.L, r.T, r.R - r.L, r.B - r.T, IsIconic(skin), IsWindowVisible(skin));
    }

    public static string Dump() {
        var list = new List<IntPtr>();
        EnumWindows(delegate(IntPtr h, IntPtr lp) { list.Add(h); return true; }, IntPtr.Zero);
        var sb = new StringBuilder();
        int i = 0;
        foreach (var h in list) {
            var c = new StringBuilder(64); GetClassNameW(h, c, 64);
            string cls = c.ToString();
            bool interesting = cls == "WorkerW" || cls == "Progman" || cls == "Tauri Window";
            if (!interesting || !IsWindowVisible(h)) { i++; continue; }
            var t = new StringBuilder(48); GetWindowTextW(h, t, 48);
            string extra = "";
            if (cls == "WorkerW" && FindWindowExW(h, IntPtr.Zero, "SHELLDLL_DefView", null) != IntPtr.Zero)
                extra = " [ICONS-HOST]";
            string fg = (GetForegroundWindow() == h) ? " [FG]" : "";
            sb.AppendLine(string.Format("  {0:d3} 0x{1:X8} {2} [{3}]{4}{5}", i, h.ToInt64(), cls, t.ToString(), extra, fg));
            i++;
        }
        return sb.ToString();
    }

    public static void Drag(int x1, int y1, int x2, int y2) {
        SetCursorPos(x1, y1);
        mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero); // LEFTDOWN
        int steps = 12;
        for (int s = 1; s <= steps; s++) {
            SetCursorPos(x1 + (x2 - x1) * s / steps, y1 + (y2 - y1) * s / steps);
            System.Threading.Thread.Sleep(30);
        }
        mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero); // LEFTUP
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
"--- before drag ---"
[D3]::SkinRect()
"drag (1719,254) -> (1659,314)"
[D3]::Drag(1719, 254, 1659, 314)
Start-Sleep -Milliseconds 800
"--- after drag (expect rect moved about -60,+60) ---"
[D3]::SkinRect()
"--- z after drag (expect skin directly above ICONS-HOST) ---"
[D3]::Dump()

"=== restarting explorer.exe ==="
taskkill /f /im explorer.exe | Out-Null
Start-Sleep -Seconds 3
if (-not (Get-Process explorer -ErrorAction SilentlyContinue)) {
    "explorer did not auto-restart; starting it"
    Start-Process explorer.exe
}
Start-Sleep -Seconds 5
"--- after explorer restart (expect skin above NEW icons host) ---"
[D3]::SkinRect()
[D3]::Dump()
[D3]::Shot(1400, 150, 560, 320, '.debug-watch/out/v4-after-explorer-restart.png')
"done"
