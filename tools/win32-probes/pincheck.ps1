Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;
using System.Drawing;
using System.Drawing.Imaging;
public class PinCheck {
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowW(string c, string t);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string t);
    [DllImport("user32.dll")] public static extern IntPtr GetWindowLongPtrW(IntPtr h, int i);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
    [DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }

    static IntPtr GadgetWorkerW() {
        IntPtr w = IntPtr.Zero;
        while (true) {
            w = FindWindowExW(IntPtr.Zero, w, "WorkerW", null);
            if (w == IntPtr.Zero) return IntPtr.Zero;
            IntPtr dv = FindWindowExW(w, IntPtr.Zero, "SHELLDLL_DefView", null);
            if (dv == IntPtr.Zero && IsWindowVisible(w)) {
                RECT r; GetWindowRect(w, out r);
                if (r.R - r.L > 500) return w; // full-screen visible WorkerW w/o icons = gadget layer
            }
        }
    }

    public static string Scan() {
        var sb = new StringBuilder();
        IntPtr gw = GadgetWorkerW();
        sb.AppendLine("gadget WorkerW = 0x" + gw.ToInt64().ToString("X"));
        IntPtr skin = IntPtr.Zero;
        if (gw != IntPtr.Zero) skin = FindWindowExW(gw, IntPtr.Zero, null, "Example Clock");
        if (skin == IntPtr.Zero) skin = FindWindowW(null, "Example Clock");
        if (skin == IntPtr.Zero) { sb.AppendLine("skin: NOT FOUND"); return sb.ToString(); }
        RECT r; GetWindowRect(skin, out r);
        sb.AppendLine(string.Format("skin=0x{0:X} parent=0x{1:X} vis={2} style=0x{3:X8} ex=0x{4:X8} wr=[{5},{6},{7},{8}]",
            skin.ToInt64(), GetParent(skin).ToInt64(), IsWindowVisible(skin),
            GetWindowLongPtrW(skin, -16).ToInt64(), GetWindowLongPtrW(skin, -20).ToInt64(), r.L, r.T, r.R, r.B));
        return sb.ToString();
    }

    public static void Shot(int x, int y, int w, int h, string path) {
        using (var bmp = new Bitmap(w, h, PixelFormat.Format32bppArgb))
        using (var g = Graphics.FromImage(bmp)) {
            g.CopyFromScreen(x, y, 0, 0, new Size(w, h), CopyPixelOperation.SourceCopy);
            bmp.Save(path, ImageFormat.Png);
        }
    }
    public static void WinD() {
        keybd_event(0x5B, 0, 0, UIntPtr.Zero);
        keybd_event(0x44, 0, 0, UIntPtr.Zero);
        keybd_event(0x44, 0, 2, UIntPtr.Zero);
        keybd_event(0x5B, 0, 2, UIntPtr.Zero);
    }
}
"@
New-Item -ItemType Directory -Force -Path .debug-watch/out | Out-Null
"--- t0 ---"
[PinCheck]::Scan()
[PinCheck]::WinD()
Start-Sleep -Seconds 3
"--- after Win+D (show desktop) ---"
[PinCheck]::Scan()
[PinCheck]::Shot(200, 200, 500, 400, ".debug-watch/out/showdesktop.png")
[PinCheck]::WinD()
Start-Sleep -Seconds 3
"--- after Win+D (restore) ---"
[PinCheck]::Scan()
Start-Sleep -Seconds 20
"--- t+26s (after repin tick) ---"
[PinCheck]::Scan()