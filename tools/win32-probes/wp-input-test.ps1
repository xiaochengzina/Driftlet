# wp-input-test.ps1 — is the pinned skin REALLY mouse-immune right now?
# Moves the cursor to: (a) plain desktop, (b) skin center, (c) skin bottom
# edge (the 6px resize hot zone), reading the HCURSOR after each move.
# Same cursor everywhere = messages land on the icons view (immune).
# Different cursor at the skin = the skin is receiving mouse input.
param(
  [int]$X = 481,   # skin center-x (edit if needed)
  [int]$Y = 322,   # skin center-y
  [int]$EdgeY = 616  # skin bottom-edge hot zone y (skin bottom - 3)
)
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public class IT {
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern bool GetCursorInfo(ref CURSORINFO ci);
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, System.Text.StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
    [StructLayout(LayoutKind.Sequential)] public struct CURSORINFO { public int cbSize; public int flags; public IntPtr hCursor; public POINT pt; }
    public static string Probe(int x, int y) {
        SetCursorPos(x, y);
        System.Threading.Thread.Sleep(120);
        var ci = new CURSORINFO(); ci.cbSize = Marshal.SizeOf(typeof(CURSORINFO));
        GetCursorInfo(ref ci);
        var pt = new POINT { X = x, Y = y };
        IntPtr hit = WindowFromPoint(pt);
        var cls = new System.Text.StringBuilder(64);
        uint pid = 0;
        if (hit != IntPtr.Zero) { GetClassNameW(hit, cls, 64); GetWindowThreadProcessId(hit, out pid); }
        return string.Format("({0},{1}) hcursor=0x{2:X} hit={3} pid={4}", x, y, ci.hCursor.ToInt64(), cls.ToString(), pid);
    }
}
"@
"plain desktop:  " + [IT]::Probe(1000, 700)
"skin center:    " + [IT]::Probe($X, $Y)
"skin edge (6px):" + [IT]::Probe($X, $EdgeY)
"plain desktop:  " + [IT]::Probe(1000, 700)
"(same hcursor everywhere = immune; different at skin = interactive)"
