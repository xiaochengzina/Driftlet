# Dump-only variant: prints the interesting slice of the top-level z-order.
Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;
using System.Collections.Generic;

public class ZOnly {
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetWindowLongPtrW(IntPtr h, int i);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr lp);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string t);
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
            var t = new StringBuilder(48); GetWindowTextW(h, t, 48);
            string extra = "";
            if (cls == "WorkerW") {
                bool hasDv = FindWindowExW(h, IntPtr.Zero, "SHELLDLL_DefView", null) != IntPtr.Zero;
                extra = hasDv ? " [DefView=>ICONS-HOST]" : "";
            }
            sb.AppendLine(string.Format("  {0:d3}{1} 0x{2:X8} {3} [{4}] vis={5}{6}",
                i, topmost ? "*" : " ", h.ToInt64(), cls, t.ToString(), IsWindowVisible(h), extra));
            i++;
        }
        return sb.ToString();
    }
}
"@
"--- zdump $(Get-Date -Format 'HH:mm:ss.fff') ---"
[ZOnly]::Dump()
