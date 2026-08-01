# Locate SHELLDLL_DefView anywhere in the window tree and print its ancestry.
Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;
using System.Collections.Generic;
public class FD {
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint f);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr lp);
    [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr p, EnumProc cb, IntPtr lp);
    delegate bool EnumProc(IntPtr h, IntPtr lp);

    static string Cls(IntPtr h) { var c = new StringBuilder(64); GetClassNameW(h, c, 64); return c.ToString(); }

    public static string Run() {
        var sb = new StringBuilder();
        var tops = new List<IntPtr>();
        EnumWindows(delegate(IntPtr h, IntPtr lp) { tops.Add(h); return true; }, IntPtr.Zero);
        int found = 0;
        foreach (var top in tops) {
            if (Cls(top) == "SHELLDLL_DefView") { Report(top, sb); found++; }
            EnumChildWindows(top, delegate(IntPtr h, IntPtr lp) {
                if (Cls(h) == "SHELLDLL_DefView") { Report(h, sb); found++; }
                return true;
            }, IntPtr.Zero);
        }
        if (found == 0) sb.AppendLine("SHELLDLL_DefView NOT FOUND anywhere");
        return sb.ToString();
    }

    static void Report(IntPtr dv, StringBuilder sb) {
        sb.AppendLine(string.Format("SHELLDLL_DefView = 0x{0:X} vis={1}", dv.ToInt64(), IsWindowVisible(dv)));
        IntPtr p = GetParent(dv);
        while (p != IntPtr.Zero) {
            var t = new StringBuilder(48); GetWindowTextW(p, t, 48);
            sb.AppendLine(string.Format("  parent: 0x{0:X} {1} [{2}] vis={3}", p.ToInt64(), Cls(p), t.ToString(), IsWindowVisible(p)));
            p = GetParent(p);
        }
    }
}
"@
[FD]::Run()
