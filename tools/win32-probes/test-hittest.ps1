Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class HitTest {
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowW(string c, string t);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string t);
    [DllImport("user32.dll")] public static extern IntPtr GetWindowLongPtrW(IntPtr h, int i);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern IntPtr SendMessageW(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }

    static void TestHwnd(IntPtr hwnd, int depth, int x, int y, StringBuilder sb) {
        IntPtr pt = (IntPtr)((y << 16) | (x & 0xFFFF));
        IntPtr hit = SendMessageW(hwnd, 0x0084, IntPtr.Zero, pt);
        var cls = new StringBuilder(64); GetClassNameW(hwnd, cls, 64);
        string indent = new string(' ', depth * 2);
        sb.AppendLine(string.Format("{0}0x{1:X} {2} WM_NCHITTEST => 0x{3:X} ({4})",
            indent, hwnd.ToInt64(), cls.ToString(), hit.ToInt64(),
            hit.ToInt64() == -1 ? "HTTRANSPARENT" : (hit.ToInt64() == 1 ? "HTCLIENT" : "other")));
        IntPtr c = IntPtr.Zero;
        while (true) {
            c = FindWindowExW(hwnd, c, null, null);
            if (c == IntPtr.Zero) break;
            TestHwnd(c, depth + 1, x, y, sb);
        }
    }

    public static string Test() {
        var sb = new StringBuilder();
        IntPtr skin = FindWindowW(null, "Example Clock");
        if (skin == IntPtr.Zero) return "skin not found";
        RECT r; GetWindowRect(skin, out r);
        int x = (r.L + r.R) / 2;
        int y = (r.T + r.B) / 2;
        TestHwnd(skin, 0, x, y, sb);
        return sb.ToString();
    }
}
"@
[HitTest]::Test()
