# wp-spawn-workerw.ps1 — send 0x052C to Progman and observe whether a
# full-screen wallpaper WorkerW appears as a CHILD of Progman (24H2 form).
# If the wallpaper rendering moves into that WorkerW, a skin pinned below
# SHELLDLL_DefView should become visible on a STOCK desktop.
# READ-ONLY except the (documented, explorer-internal) 0x052C message.
Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;

public class SW {
    [DllImport("user32.dll")] public static extern IntPtr GetShellWindow();
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h, uint cmd);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern IntPtr SendMessageTimeoutW(IntPtr h, uint msg, IntPtr w, IntPtr l, uint flags, uint timeout, out IntPtr res);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }

    public static string Kids(IntPtr parent) {
        var sb = new StringBuilder();
        int i = 0;
        for (IntPtr k = GetWindow(parent, 5); k != IntPtr.Zero; k = GetWindow(k, 2), i++) {
            var c = new StringBuilder(64); GetClassNameW(k, c, 64);
            var t = new StringBuilder(40); GetWindowTextW(k, t, 40);
            uint pid; GetWindowThreadProcessId(k, out pid);
            RECT r; GetWindowRect(k, out r);
            sb.AppendLine(string.Format("  [{0}] 0x{1:X} pid={2} {3} [{4}] vis={5} rect=({6},{7})-({8},{9})",
                i, k.ToInt64(), pid, c.ToString(), t.ToString(), IsWindowVisible(k), r.L, r.T, r.R, r.B));
            if (i > 30) { sb.AppendLine("  ..."); break; }
        }
        if (i == 0) sb.AppendLine("  (no children)");
        return sb.ToString();
    }
}
"@

$progman = [SW]::GetShellWindow()
"progman: 0x{0:X}" -f $progman.ToInt64()
"--- Progman children BEFORE 0x052C ---"
[SW]::Kids($progman)

$res = [IntPtr]::Zero
[void][SW]::SendMessageTimeoutW($progman, 0x052C, [IntPtr]0xD, [IntPtr]0x1, 0, 2000, [ref]$res)
Start-Sleep -Milliseconds 300
[void][SW]::SendMessageTimeoutW($progman, 0x052C, [IntPtr]::Zero, [IntPtr]::Zero, 0, 2000, [ref]$res)
Start-Sleep -Milliseconds 700

"--- Progman children AFTER 0x052C ---"
[SW]::Kids($progman)
"(watch the desktop: did the skin at (100,100) appear?)"
