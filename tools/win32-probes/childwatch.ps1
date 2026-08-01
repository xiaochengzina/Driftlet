Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class PinSpy {
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowW(string cls, string title);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string t);
    [DllImport("user32.dll")] public static extern IntPtr GetWindowLongPtrW(IntPtr h, int i);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
    [DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }

    public static string Scan() {
        var sb = new StringBuilder();
        IntPtr progman = FindWindowW("Progman", null);
        IntPtr w = IntPtr.Zero;
        while (true) {
            w = FindWindowExW(progman, w, "WorkerW", null);
            if (w == IntPtr.Zero) break;
            IntPtr dv = FindWindowExW(w, IntPtr.Zero, "SHELLDLL_DefView", null);
            IntPtr skin = FindWindowExW(w, IntPtr.Zero, null, "Example Clock");
            sb.AppendLine(string.Format("WorkerW 0x{0:X}: defview=0x{1:X} skin=0x{2:X}", w.ToInt64(), dv.ToInt64(), skin.ToInt64()));
            if (skin != IntPtr.Zero) {
                RECT r; GetWindowRect(skin, out r);
                sb.AppendLine(string.Format("  skin vis={0} style=0x{1:X8} ex=0x{2:X8} wr=[{3},{4},{5},{6}]",
                    IsWindowVisible(skin), GetWindowLongPtrW(skin, -16).ToInt64(), GetWindowLongPtrW(skin, -20).ToInt64(), r.L, r.T, r.R, r.B));
            }
        }
        IntPtr tl = FindWindowW(null, "Example Clock");
        sb.AppendLine(string.Format("top-level Example Clock=0x{0:X} iswindow={1}", tl.ToInt64(), IsWindow(tl)));
        return sb.ToString();
    }
    public static void WinD() {
        keybd_event(0x5B, 0, 0, UIntPtr.Zero);
        keybd_event(0x44, 0, 0, UIntPtr.Zero);
        keybd_event(0x44, 0, 2, UIntPtr.Zero);
        keybd_event(0x5B, 0, 2, UIntPtr.Zero);
    }
}
"@
"--- t0 ---"
[PinSpy]::Scan()
[PinSpy]::WinD(); Start-Sleep -Seconds 4
"--- after Win+D ---"
[PinSpy]::Scan()
[PinSpy]::WinD(); Start-Sleep -Seconds 4
"--- after restore ---"
[PinSpy]::Scan()
Start-Sleep -Seconds 12
"--- t+20s ---"
[PinSpy]::Scan()