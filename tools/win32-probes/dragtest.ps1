Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class DragTest {
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowW(string c, string t);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string t);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
    [DllImport("user32.dll")] static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int x, y; }

    public static string Run() {
        var sb = new StringBuilder();
        IntPtr skin = FindWindowW(null, "Example Clock");
        if (skin == IntPtr.Zero) return "skin not found";
        // move to a spot and show what's topmost there
        SetWindowPos(skin, IntPtr.Zero, 1100, 600, 0, 0, 0x0001 | 0x0004 | 0x0010);
        System.Threading.Thread.Sleep(400);
        RECT r; GetWindowRect(skin, out r);
        int cx = (r.L + r.R) / 2, cy = (r.T + r.B) / 2;
        POINT p; p.x = cx; p.y = cy;
        IntPtr hit = WindowFromPoint(p);
        var c = new StringBuilder(64); GetClassNameW(hit, c, 64);
        var t = new StringBuilder(60); GetWindowTextW(hit, t, 60);
        sb.AppendLine(string.Format("skin at [{0},{1}] topmost at center: 0x{2:X} {3} [{4}]", r.L, r.T, hit.ToInt64(), c.ToString(), t.ToString()));
        bool hitSkin = false;
        IntPtr h = hit;
        while (h != IntPtr.Zero) { if (h == skin) { hitSkin = true; break; } h = GetParentDll(h); }
        if (!hitSkin) return sb.ToString() + "=> skin NOT topmost here, drag skipped";
        sb.AppendLine("skin IS topmost -> dragging +90,+70 ...");
        SetCursorPos(cx, cy);
        mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero);
        for (int i = 1; i <= 10; i++) { SetCursorPos(cx + 9 * i, cy + 7 * i); System.Threading.Thread.Sleep(25); }
        mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero);
        System.Threading.Thread.Sleep(500);
        RECT r2; GetWindowRect(skin, out r2);
        sb.AppendLine(string.Format("rect after drag: [{0},{1},{2},{3}] => {4}", r2.L, r2.T, r2.R, r2.B,
            (r2.L != r.L || r2.T != r.T) ? "MOVED (drag works)" : "did NOT move"));
        return sb.ToString();
    }
    [DllImport("user32.dll")] static extern IntPtr GetParent(IntPtr h);
    static IntPtr GetParentDll(IntPtr h) { return GetParent(h); }
}
"@
[DragTest]::Run()