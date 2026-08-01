Add-Type -TypeDefinition @'
using System; using System.Text; using System.Runtime.InteropServices;
public class X {
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowW(string c, string t);
  [DllImport("user32.dll")] public static extern IntPtr GetWindowLongPtrW(IntPtr h, int i);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string t);
}
'@
$h = [X]::FindWindowW($null, "Example Clock")
Write-Host "skin=$h" -ForegroundColor Green
$cls = New-Object System.Text.StringBuilder(64)
$ttl = New-Object System.Text.StringBuilder(128)
[X]::GetClassNameW($h, $cls, 64)
[X]::GetWindowTextW($h, $ttl, 128)
$ex = [X]::GetWindowLongPtrW($h, -20).ToInt64()
Write-Host "cls=$($cls.ToString()) title=$($ttl.ToString()) ex=0x$($ex.ToString('X'))" -ForegroundColor Green

Write-Host "--- children ---" -ForegroundColor Cyan
$c = [IntPtr]::Zero
while ($true) {
    $c = [X]::FindWindowExW($h, $c, $null, $null)
    if ($c -eq [IntPtr]::Zero) { break }
    $cls2 = New-Object System.Text.StringBuilder(64)
    $ttl2 = New-Object System.Text.StringBuilder(128)
    [X]::GetClassNameW($c, $cls2, 64)
    [X]::GetWindowTextW($c, $ttl2, 128)
    $ex2 = [X]::GetWindowLongPtrW($c, -20).ToInt64()
    Write-Host ("child 0x{0:X} cls={1} title={2} ex=0x{3:X8} transparent={4}" -f $c.ToInt64(), $cls2.ToString(), $ttl2.ToString(), $ex2, (($ex2 -band 0x20) -ne 0))
}
