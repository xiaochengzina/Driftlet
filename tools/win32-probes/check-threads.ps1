Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class CheckThreads {
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowW(string c, string t);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string t);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();

    public static void Dump(IntPtr hwnd, int depth) {
        uint pid; uint tid = GetWindowThreadProcessId(hwnd, out pid);
        var cls = new StringBuilder(64); GetClassNameW(hwnd, cls, 64);
        string indent = new string(' ', depth * 2);
        Console.WriteLine("{0}0x{1:X} {2} pid={3} tid={4} current_tid={5}",
            indent, hwnd.ToInt64(), cls.ToString(), pid, tid, GetCurrentThreadId());
        IntPtr c = IntPtr.Zero;
        while (true) {
            c = FindWindowExW(hwnd, c, null, null);
            if (c == IntPtr.Zero) break;
            Dump(c, depth + 1);
        }
    }
}
"@
$skin = [CheckThreads]::FindWindowW($null, "Example Clock")
[CheckThreads]::Dump($skin, 0)
