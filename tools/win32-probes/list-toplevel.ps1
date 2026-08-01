Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class ListTop {
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string t);
    [DllImport("user32.dll")] public static extern IntPtr GetWindowLongPtrW(IntPtr h, int i);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
}
"@
$w = [IntPtr]::Zero
while ($true) {
    $w = [ListTop]::FindWindowExW([IntPtr]::Zero, $w, $null, $null)
    if ($w -eq [IntPtr]::Zero) { break }
    [ref]$wpid = 0
    [void][ListTop]::GetWindowThreadProcessId($w, $wpid)
    $clsB = New-Object System.Text.StringBuilder(64)
    $ttlB = New-Object System.Text.StringBuilder(128)
    [void][ListTop]::GetClassNameW($w, $clsB, 64)
    [void][ListTop]::GetWindowTextW($w, $ttlB, 128)
    $ex = [ListTop]::GetWindowLongPtrW($w, -20).ToInt64()
    if ([ListTop]::IsWindowVisible($w) -and ($ttlB.ToString() -ne "" -or $clsB.ToString() -like "*Chrome*")) {
        "0x{0:X} pid={1} cls=[{2}] title=[{3}] ex=0x{4:X8} transparent={5}" -f $w.ToInt64(), $wpid.Value, $clsB.ToString(), $ttlB.ToString(), $ex, (($ex -band 0x20) -ne 0)
    }
}
