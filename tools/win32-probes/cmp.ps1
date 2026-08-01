Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class Cmp {
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string t);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowW(string c, string t);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int x, y; }

    public static void WinD() {
        keybd_event(0x5B, 0, 0, UIntPtr.Zero);
        keybd_event(0x44, 0, 0, UIntPtr.Zero);
        keybd_event(0x44, 0, 2, UIntPtr.Zero);
        keybd_event(0x5B, 0, 2, UIntPtr.Zero);
    }
    static string HitAt(int x, int y) {
        POINT p; p.x = x; p.y = y;
        IntPtr hit = WindowFromPoint(p);
        var c = new StringBuilder(64); GetClassNameW(hit, c, 64);
        var t = new StringBuilder(60); GetWindowTextW(hit, t, 60);
        return string.Format("({0},{1}) -> 0x{2:X} {3} [{4}]", x, y, hit.ToInt64(), c.ToString(), t.ToString());
    }
    public static string Run() {
        var sb = new StringBuilder();
        // real Rainmeter widget rect
        IntPtr rm = IntPtr.Zero;
        IntPtr w = IntPtr.Zero;
        while (true) {
            w = FindWindowExW(IntPtr.Zero, w, "RainmeterMeterWindow", null);
            if (w == IntPtr.Zero) break;
            RECT r; if (GetWindowRect(w, out r) && r.R > r.L) { rm = w;
                sb.AppendLine(string.Format("rainmeter widget 0x{0:X} rect=[{1},{2},{3},{4}]", w.ToInt64(), r.L, r.T, r.R, r.B));
                break; }
        }
        IntPtr skin = FindWindowW(null, "Example Clock");
        RECT sr; GetWindowRect(skin, out sr);
        sb.AppendLine(string.Format("skin rect=[{0},{1},{2},{3}]", sr.L, sr.T, sr.R, sr.B));
        WinD();
        System.Threading.Thread.Sleep(2500);
        sb.AppendLine("--- during Win+D ---");
        if (rm != IntPtr.Zero) {
            RECT r; GetWindowRect(rm, out r);
            sb.AppendLine("rainmeter probe: " + HitAt((r.L + r.R) / 2, (r.T + r.B) / 2));
        }
        sb.AppendLine("skin probe:      " + HitAt((sr.L + sr.R) / 2, (sr.T + sr.B) / 2));
        WinD();
        System.Threading.Thread.Sleep(2000);
        sb.AppendLine("--- after restore ---");
        if (rm != IntPtr.Zero) {
            RECT r; GetWindowRect(rm, out r);
            sb.AppendLine("rainmeter probe: " + HitAt((r.L + r.R) / 2, (r.T + r.B) / 2));
        }
        sb.AppendLine("skin probe:      " + HitAt((sr.L + sr.R) / 2, (sr.T + sr.B) / 2));
        return sb.ToString();
    }
}
"@
[Cmp]::Run()