Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class V {
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowW(string c, string t);
    [DllImport("user32.dll")] public static extern IntPtr GetWindowLongPtrW(IntPtr h, int i);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
    [DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int x, y; }

    static bool IsDescendant(IntPtr anc, IntPtr h) {
        while (h != IntPtr.Zero) { if (h == anc) return true; h = GetParent(h); }
        return false;
    }
    public static void WinD() {
        keybd_event(0x5B, 0, 0, UIntPtr.Zero);
        keybd_event(0x44, 0, 0, UIntPtr.Zero);
        keybd_event(0x44, 0, 2, UIntPtr.Zero);
        keybd_event(0x5B, 0, 2, UIntPtr.Zero);
    }
    public static string Probe(string tag) {
        var sb = new StringBuilder();
        IntPtr skin = FindWindowW(null, "Example Clock");
        if (skin == IntPtr.Zero) return tag + ": skin NOT FOUND";
        RECT r; GetWindowRect(skin, out r);
        long style = GetWindowLongPtrW(skin, -16).ToInt64();
        long ex = GetWindowLongPtrW(skin, -20).ToInt64();
        long owner = GetWindowLongPtrW(skin, -8).ToInt64(); // GWLP_HWNDPARENT
        POINT p; p.x = (r.L + r.R) / 2; p.y = (r.T + r.B) / 2;
        IntPtr hit = WindowFromPoint(p);
        var cls = new StringBuilder(64); GetClassNameW(hit, cls, 64);
        var ttl = new StringBuilder(128); GetWindowTextW(hit, ttl, 128);
        bool hitSkin = hit == skin || IsDescendant(skin, hit);
        sb.AppendLine(string.Format("{0}: skin=0x{1:X} vis={2} owner=0x{3:X} style=0x{4:X8} ex=0x{5:X8} [TOOLWINDOW={6} APPWINDOW={7} TRANSPARENT={8}] wr=[{9},{10},{11},{12}]",
            tag, skin.ToInt64(), IsWindowVisible(skin), owner, style, ex,
            (ex & 0x80) != 0, (ex & 0x40000) != 0, (ex & 0x20) != 0, r.L, r.T, r.R, r.B));
        sb.AppendLine(string.Format("  probe({0},{1}) hit=0x{2:X} cls={3} title=[{4}] => {5}",
            p.x, p.y, hit.ToInt64(), cls.ToString(), ttl.ToString(),
            hitSkin ? "HIT skin (clickable)" : "missed skin (not clickable)"));
        return sb.ToString();
    }
}
"@
"=== t0 ==="
[V]::Probe("t0")
[V]::WinD(); Start-Sleep -Seconds 3
"=== during Win+D ==="
[V]::Probe("win+d")
[V]::WinD(); Start-Sleep -Seconds 3
"=== restored ==="
[V]::Probe("restore")