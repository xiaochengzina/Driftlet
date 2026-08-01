Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class Dm {
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string t);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern IntPtr GetWindowLongPtrW(IntPtr h, int i);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    public const uint MOUSEEVENTF_RIGHTDOWN = 0x0008;
    public const uint MOUSEEVENTF_RIGHTUP = 0x0010;

    public static IntPtr FindSkinWindow(uint targetPid) {
        IntPtr w = IntPtr.Zero;
        while (true) {
            w = FindWindowExW(IntPtr.Zero, w, null, null);
            if (w == IntPtr.Zero) break;
            uint pid;
            GetWindowThreadProcessId(w, out pid);
            if (pid != targetPid) continue;
            var ttl = new StringBuilder(128);
            GetWindowTextW(w, ttl, 128);
            if (ttl.ToString() == "Example Clock") return w;
        }
        return IntPtr.Zero;
    }

    public static void RightClick(int x, int y) {
        SetCursorPos(x, y);
        System.Threading.Thread.Sleep(100);
        mouse_event(MOUSEEVENTF_RIGHTDOWN, 0, 0, 0, UIntPtr.Zero);
        System.Threading.Thread.Sleep(50);
        mouse_event(MOUSEEVENTF_RIGHTUP, 0, 0, 0, UIntPtr.Zero);
        System.Threading.Thread.Sleep(100);
    }
    public static string Class(IntPtr h) {
        var sb = new StringBuilder(64);
        GetClassNameW(h, sb, 64);
        return sb.ToString();
    }
    public static string Text(IntPtr h) {
        var sb = new StringBuilder(128);
        GetWindowTextW(h, sb, 128);
        return sb.ToString();
    }
}
"@

$procs = Get-Process -Name "driftlet" -ErrorAction SilentlyContinue
if (-not $procs) { "driftlet process not found"; exit }
$appPid = $procs[0].Id
"driftlet pid=$appPid"

$skin = [Dm]::FindSkinWindow([uint32]$appPid)
if ($skin -eq [IntPtr]::Zero) { "skin window not found"; exit }
$r = New-Object Dm+RECT
[void][Dm]::GetWindowRect($skin, [ref]$r)
$x = ($r.L + $r.R) / 2
$y = ($r.T + $r.B) / 2
$ex = [Dm]::GetWindowLongPtrW($skin, -20).ToInt64()
"skin=0x{0:X} center=({1},{2}) ex=0x{3:X8} transparent={4}" -f $skin.ToInt64(), $x, $y, $ex, (($ex -band 0x20) -ne 0)

"--- before click ---"
$menuBefore = [Dm]::FindWindowExW([IntPtr]::Zero, [IntPtr]::Zero, "#32768", $null)
"menu before: 0x{0:X} visible={1}" -f $menuBefore.ToInt64(), [Dm]::IsWindowVisible($menuBefore)

"--- clicking ---"
[Dm]::RightClick($x, $y)

"--- after click ---"
$menuAfter = [Dm]::FindWindowExW([IntPtr]::Zero, [IntPtr]::Zero, "#32768", $null)
$mr = New-Object Dm+RECT
[void][Dm]::GetWindowRect($menuAfter, [ref]$mr)
"menu after: 0x{0:X} visible={1} class={2} rect=[{3},{4},{5},{6}]" -f $menuAfter.ToInt64(), [Dm]::IsWindowVisible($menuAfter), [Dm]::Class($menuAfter), $mr.L, $mr.T, $mr.R, $mr.B

# Also enumerate any visible menu-like top-level window
$w = [IntPtr]::Zero
"--- visible top-level windows after click ---"
while ($true) {
    $w = [Dm]::FindWindowExW([IntPtr]::Zero, $w, $null, $null)
    if ($w -eq [IntPtr]::Zero) { break }
    if ([Dm]::IsWindowVisible($w)) {
        $cls = [Dm]::Class($w)
        if ($cls -eq "#32768" -or $cls -like "*Menu*") {
            "  0x{0:X} cls={1} text=[{2}]" -f $w.ToInt64(), $cls, [Dm]::Text($w)
        }
    }
}
