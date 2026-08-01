Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class ZSpy {
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowW(string c, string t);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string t);
    [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h, uint cmd);
    [DllImport("user32.dll")] public static extern IntPtr GetWindowLongPtrW(IntPtr h, int i);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);

    public static string DumpZ(int max) {
        var sb = new StringBuilder();
        // GW_HWNDFIRST = 0 → topmost top-level window
        IntPtr h = GetWindow(FindWindowW("Progman", null), 0);
        // find icons host
        IntPtr iconsHost = IntPtr.Zero, gadget = IntPtr.Zero;
        IntPtr w = IntPtr.Zero;
        while (true) {
            w = FindWindowExW(IntPtr.Zero, w, "WorkerW", null);
            if (w == IntPtr.Zero) break;
            if (FindWindowExW(w, IntPtr.Zero, "SHELLDLL_DefView", null) != IntPtr.Zero) iconsHost = w;
            else if (IsWindowVisible(w)) gadget = w;
        }
        sb.AppendLine(string.Format("iconsHost=0x{0:X} gadget=0x{1:X}", iconsHost.ToInt64(), gadget.ToInt64()));
        // walk from a known top window: use GetWindow(Progman, GW_HWNDFIRST)
        int i = 0;
        while (h != IntPtr.Zero && i < max) {
            var c = new StringBuilder(64); GetClassNameW(h, c, 64);
            var t = new StringBuilder(96); GetWindowTextW(h, t, 96);
            string mark = "";
            if (h == iconsHost) mark = "  <== ICONS HOST";
            if (h == gadget) mark = "  <== GADGET WorkerW";
            if (t.ToString() == "Example Clock") mark = "  <== SKIN";
            if (IsWindowVisible(h) || mark != "")
                sb.AppendLine(string.Format("{0:d2} 0x{1:X} {2} [{3}]{4}", i, h.ToInt64(), c.ToString(), t.ToString(), mark));
            h = GetWindow(h, 2); // GW_HWNDNEXT
            i++;
        }
        return sb.ToString();
    }
}
"@
[ZSpy]::DumpZ(40)