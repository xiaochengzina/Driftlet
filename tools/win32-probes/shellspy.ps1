Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class ShellSpy {
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowW(string c, string t);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string t);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr SendMessageTimeoutW(IntPtr h, uint msg, IntPtr w, IntPtr l, uint flags, uint timeout, out IntPtr result);
    [DllImport("user32.dll")] public static extern IntPtr GetWindowLongPtrW(IntPtr h, int i);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }

    static string Info(IntPtr h) {
        if (h == IntPtr.Zero) return "0x0";
        var c = new StringBuilder(64); GetClassNameW(h, c, 64);
        RECT r; GetWindowRect(h, out r);
        return string.Format("0x{0:X} cls={1} vis={2} wr=[{3},{4},{5},{6}]",
            h.ToInt64(), c.ToString(), IsWindowVisible(h), r.L, r.T, r.R, r.B);
    }

    public static string Dump() {
        var sb = new StringBuilder();
        IntPtr progman = FindWindowW("Progman", null);
        sb.AppendLine("Progman = " + Info(progman));
        sb.AppendLine("Progman child SHELLDLL_DefView = " + Info(FindWindowExW(progman, IntPtr.Zero, "SHELLDLL_DefView", null)));
        // canonical: which TOP-LEVEL window hosts SHELLDLL_DefView?
        IntPtr defview = FindWindowW("SHELLDLL_DefView", null);
        sb.AppendLine("top-level SHELLDLL_DefView = " + Info(defview));
        IntPtr host = defview;
        if (host != IntPtr.Zero) {
            // host's parent chain top
            IntPtr parent = GetParentDll(host);
            sb.AppendLine("  its parent = " + Info(parent));
        }
        // the gadget WorkerW = next top-level WorkerW AFTER the window hosting defview
        IntPtr hostTop = defview;
        if (hostTop != IntPtr.Zero) {
            IntPtr p = GetParentDll(hostTop);
            if (p != IntPtr.Zero) hostTop = p;
            IntPtr gadget = FindWindowExW(IntPtr.Zero, hostTop, "WorkerW", null);
            sb.AppendLine("gadget WorkerW (sibling after host) = " + Info(gadget));
        }
        // also list all top-level WorkerW windows
        sb.AppendLine("--- top-level WorkerW windows ---");
        IntPtr w = IntPtr.Zero;
        while (true) {
            w = FindWindowExW(IntPtr.Zero, w, "WorkerW", null);
            if (w == IntPtr.Zero) break;
            IntPtr dv = FindWindowExW(w, IntPtr.Zero, "SHELLDLL_DefView", null);
            sb.AppendLine("  " + Info(w) + "  defview=0x" + dv.ToInt64().ToString("X"));
        }
        return sb.ToString();
    }

    [DllImport("user32.dll")] static extern IntPtr GetParent(IntPtr h);
    static IntPtr GetParentDll(IntPtr h) { return GetParent(h); }

    public static void SpawnWorkerW() {
        IntPtr progman = FindWindowW("Progman", null);
        IntPtr res;
        SendMessageTimeoutW(progman, 0x052C, IntPtr.Zero, IntPtr.Zero, 0x0, 1000, out res);
    }
}
"@
"=== BEFORE spawn ==="
[ShellSpy]::Dump()
[ShellSpy]::SpawnWorkerW()
Start-Sleep -Milliseconds 500
"=== AFTER 0x052C spawn ==="
[ShellSpy]::Dump()