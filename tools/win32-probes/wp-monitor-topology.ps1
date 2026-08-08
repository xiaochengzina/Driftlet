# wp-monitor-topology.ps1 — dump monitor rects + per-monitor DPI (ASCII-only).
Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class MONT {
    [DllImport("user32.dll")] public static extern bool EnumDisplayMonitors(IntPtr hdc, IntPtr clip, EnumProc cb, IntPtr l);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern bool GetMonitorInfoW(IntPtr m, ref MONITORINFOEX info);
    [DllImport("shcore.dll")] public static extern int GetDpiForMonitor(IntPtr m, int type, out uint x, out uint y);
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern bool EnumDisplayDevicesW(string dev, uint num, ref DISPLAY_DEVICEW dd, uint flags);
    public delegate bool EnumProc(IntPtr m, IntPtr h, IntPtr r, IntPtr l);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)]
    public struct MONITORINFOEX {
        public int cbSize; public RECT rcMonitor; public RECT rcWork; public uint dwFlags;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst=32)] public string szDevice;
    }
    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)]
    public struct DISPLAY_DEVICEW {
        public int cb;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst=32)] public string DeviceName;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst=128)] public string DeviceString;
        public uint StateFlags;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst=128)] public string DeviceID;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst=128)] public string DeviceKey;
    }
    public static void Run() {
        Console.WriteLine("virtual screen: ({0},{1}) {2}x{3}", GetSystemMetrics(76), GetSystemMetrics(77), GetSystemMetrics(78), GetSystemMetrics(79));
        EnumProc cb = (m, h, r, l) => {
            MONITORINFOEX info = new MONITORINFOEX(); info.cbSize = Marshal.SizeOf<MONITORINFOEX>();
            GetMonitorInfoW(m, ref info);
            uint dx = 0, dy = 0; GetDpiForMonitor(m, 0, out dx, out dy);
            Console.WriteLine("monitor: rect=({0},{1})-({2},{3}) primary={4} dpi={5} device={6}",
                info.rcMonitor.L, info.rcMonitor.T, info.rcMonitor.R, info.rcMonitor.B,
                info.dwFlags == 1, dx, info.szDevice);
            return true;
        };
        EnumDisplayMonitors(IntPtr.Zero, IntPtr.Zero, cb, IntPtr.Zero);
        GC.KeepAlive(cb);
        Console.WriteLine("--- display devices ---");
        DISPLAY_DEVICEW dd = new DISPLAY_DEVICEW(); dd.cb = Marshal.SizeOf<DISPLAY_DEVICEW>();
        for (uint i = 0; EnumDisplayDevicesW(null, i, ref dd, 0); i++) {
            Console.WriteLine("[{0}] {1} | {2} | flags=0x{3:X}", i, dd.DeviceName, dd.DeviceString, dd.StateFlags);
            dd = new DISPLAY_DEVICEW(); dd.cb = Marshal.SizeOf<DISPLAY_DEVICEW>();
        }
    }
}
"@
[MONT]::Run()
