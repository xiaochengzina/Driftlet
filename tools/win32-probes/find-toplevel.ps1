Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class FindTop {
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowW(string c, string t);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string t);
    [DllImport("user32.dll")] public static extern IntPtr GetWindowLongPtrW(IntPtr h, int i);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint ga);
    [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
}
"@
$handles = @(0x1C064A, 0x3604FA, 0xB040C, 0x1B0578, 0x706B4, 0x90868, 0x120666)
foreach ($h in $handles) {
    $ptr = [IntPtr]$h
    $cls = New-Object System.Text.StringBuilder(64)
    [void][FindTop]::GetClassNameW($ptr, $cls, 64)
    $ex = [FindTop]::GetWindowLongPtrW($ptr, -20).ToInt64()
    $parent = [FindTop]::GetParent($ptr)
    $ancestor = [FindTop]::GetAncestor($ptr, 2) # GA_ROOT
    "0x{0:X} {1} parent=0x{2:X} root=0x{3:X} ex=0x{4:X8}" -f $h, $cls.ToString(), $parent.ToInt64(), $ancestor.ToInt64(), $ex
}
