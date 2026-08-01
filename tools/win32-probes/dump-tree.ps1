Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class DumpTreeX {
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowW(string c, string t);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string t);
    [DllImport("user32.dll")] public static extern IntPtr GetWindowLongPtrW(IntPtr h, int i);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);

    public static string Run() {
        var sb = new StringBuilder();
        IntPtr skin = FindWindowW(null, "Example Clock");
        if (skin == IntPtr.Zero) return "skin not found";
        Dump(skin, 0, sb);
        return sb.ToString();
    }

    static void Dump(IntPtr hwnd, int depth, StringBuilder sb) {
        long ex = GetWindowLongPtrW(hwnd, -20).ToInt64();
        var cls = new StringBuilder(64); GetClassNameW(hwnd, cls, 64);
        var ttl = new StringBuilder(128); GetWindowTextW(hwnd, ttl, 128);
        string indent = new string(' ', depth * 2);
        sb.AppendLine(string.Format("{0}0x{1:X} {2} [{3}] ex=0x{4:X8} transparent={5}",
            indent, hwnd.ToInt64(), cls.ToString(), ttl.ToString(), ex, (ex & 0x20) != 0));
        IntPtr c = IntPtr.Zero;
        while (true) {
            c = FindWindowExW(hwnd, c, null, null);
            if (c == IntPtr.Zero) break;
            Dump(c, depth + 1, sb);
        }
    }
}
"@
[DumpTreeX]::Run()
