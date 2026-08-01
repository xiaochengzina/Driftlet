Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class HitSpy {
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowW(string c, string t);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr SendMessageTimeoutW(IntPtr h, uint msg, IntPtr w, IntPtr l, uint flags, uint timeout, out IntPtr result);
    [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h, uint cmd);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetWindowLongPtrW(IntPtr h, int i);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }

    public static string Test(int px, int py) {
        var sb = new StringBuilder();
        IntPtr skin = FindWindowW(null, "Example Clock");
        if (skin == IntPtr.Zero) return "skin not found";
        // 1. direct WM_NCHITTEST to the skin
        IntPtr res;
        IntPtr lp = new IntPtr((py << 16) | (px & 0xFFFF));
        SendMessageTimeoutW(skin, 0x84, IntPtr.Zero, lp, 0x0, 1000, out res);
        sb.AppendLine(string.Format("WM_NCHITTEST to skin = {0} (1=HTCLIENT 2=HTCAPTION -1=HTTRANSPARENT)", res.ToInt64()));
        // 2. walk z-order, find windows containing the point
        sb.AppendLine("z-order windows containing the point (top→bottom):");
        IntPtr h = GetWindow(skin, 0); // GW_HWNDFIRST
        int i = 0;
        while (h != IntPtr.Zero && i < 300) {
            RECT r;
            if (GetWindowRect(h, out r) && px >= r.L && px < r.R && py >= r.T && py < r.B && IsWindowVisible(h)) {
                var c = new StringBuilder(64); GetClassNameW(h, c, 64);
                var t = new StringBuilder(60); GetWindowTextW(h, t, 60);
                sb.AppendLine(string.Format("  [{0}] 0x{1:X} {2} [{3}] ex=0x{4:X8}", i, h.ToInt64(), c.ToString(), t.ToString(), GetWindowLongPtrW(h, -20).ToInt64()));
            }
            h = GetWindow(h, 2); i++;
        }
        return sb.ToString();
    }
}
"@
[HitSpy]::Test(410, 345)