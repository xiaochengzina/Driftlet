Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Diagnostics;
using System.Runtime.InteropServices;
public class DumpChildren {
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowW(string c, string t);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string t);
    [DllImport("user32.dll")] public static extern IntPtr GetWindowLongPtrW(IntPtr h, int i);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);

    public static string Dump(IntPtr parent, int depth) {
        var sb = new StringBuilder();
        IntPtr c = IntPtr.Zero;
        while (true) {
            c = FindWindowExW(parent, c, null, null);
            if (c == IntPtr.Zero) break;
            long ex = GetWindowLongPtrW(c, -20).ToInt64();
            var cls = new StringBuilder(64); GetClassNameW(c, cls, 64);
            var ttl = new StringBuilder(128); GetWindowTextW(c, ttl, 128);
            uint pid; GetWindowThreadProcessId(c, out pid);
            string indent = new string(' ', depth * 2);
            sb.AppendLine(string.Format("{0}0x{1:X} pid={2} {3} [{4}] ex=0x{5:X8} transparent={6} vis={7}",
                indent, c.ToInt64(), pid, cls.ToString(), ttl.ToString(), ex, (ex & 0x20) != 0, IsWindowVisible(c)));
            sb.Append(Dump(c, depth + 1));
        }
        return sb.ToString();
    }
}
"@
$procs = Get-Process -Name "driftlet" -ErrorAction SilentlyContinue
if (-not $procs) { "no driftlet process"; exit }
$targetPid = $procs[0].Id
"using pid $targetPid"
$skin = [IntPtr]::Zero
$w = [IntPtr]::Zero
while ($true) {
    $w = [DumpChildren]::FindWindowExW([IntPtr]::Zero, $w, $null, $null)
    if ($w -eq [IntPtr]::Zero) { break }
    [ref]$wpid = 0
    [void][DumpChildren]::GetWindowThreadProcessId($w, $wpid)
    if ($wpid.Value -eq $targetPid) {
        $skin = $w
        break
    }
}
if ($skin -eq [IntPtr]::Zero) { "skin not found"; exit }
"skin = 0x{0:X}" -f $skin.ToInt64()
[DumpChildren]::Dump($skin, 0)
