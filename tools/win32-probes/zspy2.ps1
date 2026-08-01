Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class ZSpy2 {
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string t);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumWindowsProc cb, IntPtr lp);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    public delegate bool EnumWindowsProc(IntPtr h, IntPtr lp);

    public static string DumpZ(int max) {
        var sb = new StringBuilder();
        IntPtr iconsHost = IntPtr.Zero, gadget = IntPtr.Zero;
        IntPtr w = IntPtr.Zero;
        while (true) {
            w = FindWindowExW(IntPtr.Zero, w, "WorkerW", null);
            if (w == IntPtr.Zero) break;
            if (FindWindowExW(w, IntPtr.Zero, "SHELLDLL_DefView", null) != IntPtr.Zero) iconsHost = w;
            else if (IsWindowVisible(w)) gadget = w;
        }
        sb.AppendLine(string.Format("iconsHost=0x{0:X} gadget=0x{1:X}", iconsHost.ToInt64(), gadget.ToInt64()));
        int i = 0;
        EnumWindows((h, lp) => {
            if (i >= max) return false;
            var c = new StringBuilder(64); GetClassNameW(h, c, 64);
            var t = new StringBuilder(80); GetWindowTextW(h, t, 80);
            string mark = "";
            if (h == iconsHost) mark = "  <== ICONS HOST";
            else if (h == gadget) mark = "  <== GADGET";
            else if (t.ToString() == "Example Clock") mark = "  <== SKIN";
            else if (c.ToString() == "RainmeterMeterWindow") mark = "  <== RealRainmeter";
            else if (c.ToString() == "Progman") mark = "  <== Progman";
            if (mark != "" || (IsWindowVisible(h) && i < 200)) {
                sb.AppendLine(string.Format("{0:d3} 0x{1:X} {2} [{3}]{4}", i, h.ToInt64(), c.ToString(), t.ToString(), mark));
            }
            i++;
            return true;
        }, IntPtr.Zero);
        return sb.ToString();
    }
}
"@
[ZSpy2]::DumpZ(60)