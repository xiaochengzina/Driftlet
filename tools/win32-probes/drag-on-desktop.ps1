# Drag regression: press Win+D (skin stays above desktop thanks to the fix),
# drag the skin by synthetic mouse input, verify it moved, then restore.
Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class DG {
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowW(string c, string t);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
    [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
    [DllImport("user32.dll")] static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
    [DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int x, y; }

    public static void WinD() {
        keybd_event(0x5B, 0, 0, UIntPtr.Zero);
        keybd_event(0x44, 0, 0, UIntPtr.Zero);
        keybd_event(0x44, 0, 2, UIntPtr.Zero);
        keybd_event(0x5B, 0, 2, UIntPtr.Zero);
    }

    public static string Run() {
        var sb = new StringBuilder();
        IntPtr skin = FindWindowW(null, "Example Clock");
        if (skin == IntPtr.Zero) return "skin not found";
        RECT r0; GetWindowRect(skin, out r0);
        sb.AppendLine(string.Format("before: [{0},{1} {2}x{3}]", r0.L, r0.T, r0.R - r0.L, r0.B - r0.T));

        // show desktop -> skin must be topmost above the desktop
        WinD();
        System.Threading.Thread.Sleep(1200);
        RECT r; GetWindowRect(skin, out r);
        int cx = (r.L + r.R) / 2, cy = (r.T + r.B) / 2;
        POINT p; p.x = cx; p.y = cy;
        IntPtr hit = WindowFromPoint(p);
        IntPtr h = hit; bool hitSkin = false;
        while (h != IntPtr.Zero) { if (h == skin) { hitSkin = true; break; } h = GetParent(h); }
        var hc = new StringBuilder(64); GetClassNameW(hit, hc, 64);
        sb.AppendLine(string.Format("on show-desktop: WindowFromPoint({0},{1}) = {2} hitSkin={3}", cx, cy, hc.ToString(), hitSkin));
        if (!hitSkin) { WinD(); return sb.ToString() + "=> skin not clickable on desktop"; }

        // drag +90,+70
        SetCursorPos(cx, cy);
        mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero);
        for (int i = 1; i <= 12; i++) { SetCursorPos(cx + 8 * i, cy + 6 * i); System.Threading.Thread.Sleep(25); }
        mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero);
        System.Threading.Thread.Sleep(700);
        RECT r2; GetWindowRect(skin, out r2);
        sb.AppendLine(string.Format("after drag: [{0},{1} {2}x{3}] => {4}", r2.L, r2.T, r2.R - r2.L, r2.B - r2.T,
            (r2.L != r.L || r2.T != r.T) ? "MOVED (drag works)" : "did NOT move"));

        // restore desktop
        WinD();
        System.Threading.Thread.Sleep(1000);
        return sb.ToString();
    }
}
"@
[DG]::Run()
