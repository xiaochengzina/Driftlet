# wp-defview-test.ps1 — validates "child of SHELLDLL_DefView, below the
# SysListView32 icons list" as the wallpaper-layer host on STOCK desktop
# (run with third-party live-wallpaper tools OFF).
#
# Creates a MAGENTA WinForms window owned by this script's thread (so
# SetParent is legal), parents it into DefView below the icons list,
# then OBJECTIVELY checks visibility by screenshotting its rect and
# counting magenta pixels, plus a WindowFromPoint mouse-immunity check.
# Restores everything afterwards. Watch the desktop too!
#
#   powershell -ExecutionPolicy Bypass -File wp-defview-test.ps1 [-Seconds 10]
param([int]$Seconds = 10)

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;

public class DT {
    [DllImport("user32.dll")] public static extern IntPtr GetShellWindow();
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string t);
    [DllImport("user32.dll")] public static extern IntPtr GetWindowLongPtrW(IntPtr h, int i);
    [DllImport("user32.dll")] public static extern IntPtr SetWindowLongPtrW(IntPtr h, int i, IntPtr v);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr SetParent(IntPtr h, IntPtr p);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr ins, int x, int y, int cx, int cy, uint f);
    [DllImport("user32.dll")] public static extern bool ScreenToClient(IntPtr h, ref POINT p);
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }

    public static IntPtr FindDefView(out IntPtr listview) {
        listview = IntPtr.Zero;
        IntPtr dv = FindWindowExW(GetShellWindow(), IntPtr.Zero, "SHELLDLL_DefView", null);
        if (dv == IntPtr.Zero) {
            IntPtr w = IntPtr.Zero;
            while (true) {
                w = FindWindowExW(IntPtr.Zero, w, "WorkerW", null);
                if (w == IntPtr.Zero) break;
                dv = FindWindowExW(w, IntPtr.Zero, "SHELLDLL_DefView", null);
                if (dv != IntPtr.Zero) break;
            }
        }
        if (dv != IntPtr.Zero)
            listview = FindWindowExW(dv, IntPtr.Zero, "SysListView32", null);
        return dv;
    }

    public static string Desc(IntPtr h) {
        if (h == IntPtr.Zero) return "0x0";
        var c = new StringBuilder(64); GetClassNameW(h, c, 64);
        uint pid; GetWindowThreadProcessId(h, out pid);
        return string.Format("0x{0:X} pid={1} {2} vis={3}", h.ToInt64(), pid, c.ToString(), IsWindowVisible(h));
    }
}
"@

function Count-Magenta([int]$x, [int]$y, [int]$w, [int]$h) {
  $bmp = New-Object System.Drawing.Bitmap $w, $h
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($x, $y, 0, 0, $bmp.Size)
  $cnt = 0
  for ($py = 0; $py -lt $h; $py += 4) {
    for ($px = 0; $px -lt $w; $px += 4) {
      $c = $bmp.GetPixel($px, $py)
      if ($c.R -gt 200 -and $c.G -lt 80 -and $c.B -gt 200) { $cnt++ }
    }
  }
  $g.Dispose(); $bmp.Dispose()
  $total = [math]::Ceiling($h / 4) * [math]::Ceiling($w / 4)
  return [math]::Round(100.0 * $cnt / $total, 1)
}

$lv = [IntPtr]::Zero
$dv = [DT]::FindDefView([ref]$lv)
if ($dv -eq [IntPtr]::Zero) { "!! no SHELLDLL_DefView found"; exit 1 }
"DefView: " + [DT]::Desc($dv)
"ListView: " + [DT]::Desc($lv)
if ($lv -eq [IntPtr]::Zero) { "!! no SysListView32 - cannot test below-icons z-order"; exit 1 }

$X = 1050; $Y = 550; $W = 500; $H = 350
$form = New-Object System.Windows.Forms.Form
$form.FormBorderStyle = 'None'
$form.StartPosition = 'Manual'
$form.Location = New-Object System.Drawing.Point($X, $Y)
$form.Size = New-Object System.Drawing.Size($W, $H)
$form.BackColor = [System.Drawing.Color]::Magenta
$form.ShowInTaskbar = $false
$form.Show()
[System.Windows.Forms.Application]::DoEvents()
Start-Sleep -Milliseconds 500
$h = $form.Handle

"--- control: normal top-level window ---"
"top-level visible magenta: " + (Count-Magenta $X $Y $W $H) + "%  (expect ~100)"

"--- SetParent into DefView, below SysListView32 ---"
$oldStyle = [DT]::GetWindowLongPtrW($h, -16).ToInt64()
[void][DT]::SetWindowLongPtrW($h, -16, [IntPtr](($oldStyle -band -bnot 0x80000000L) -bor 0x40000000L))
$prev = [DT]::SetParent($h, $dv)
"previous parent: " + [DT]::Desc($prev)
"new parent: " + [DT]::Desc([DT]::GetParent($h))
$pt = New-Object DT+POINT; $pt.X = $X; $pt.Y = $Y
[void][DT]::ScreenToClient($dv, [ref]$pt)
# 0x0010 NOACTIVATE | 0x0040 SHOWWINDOW ; insert directly below the icons list
[void][DT]::SetWindowPos($h, $lv, $pt.X, $pt.Y, $W, $H, 0x0010 -bor 0x0040)
# switch to CYAN for the wallpaper-layer phase so the two phases are
# visually distinguishable (magenta = top-level, cyan = inside DefView)
$form.BackColor = [System.Drawing.Color]::Cyan
$form.Refresh()
Start-Sleep -Milliseconds 700

$center = New-Object DT+POINT; $center.X = $X + [int]($W/2); $center.Y = $Y + [int]($H/2)
"WindowFromPoint(center): " + [DT]::Desc([DT]::WindowFromPoint($center)) + "  (expect SysListView32/SHELLDLL_DefView = mouse immune)"

"watch the desktop now for $Seconds s (magenta box at $X,$Y)..."
$shot = 0
for ($i = 0; $i -lt $Seconds; $i++) {
  Start-Sleep -Seconds 1
  [System.Windows.Forms.Application]::DoEvents()
  if ($i -eq 1) { $script:shot = Count-Magenta $X $Y $W $H }
}
"wallpaper-layer visible magenta: $shot%  (near 100 = OPTION WORKS; near 0 = dead)"

"--- restore ---"
[void][DT]::SetParent($h, [IntPtr]::Zero)
[void][DT]::SetWindowLongPtrW($h, -16, [IntPtr]$oldStyle)
[void][DT]::SetWindowPos($h, [IntPtr]::Zero, $X, $Y, $W, $H, 0x0010 -bor 0x0040)
Start-Sleep -Milliseconds 300
$form.Close()
"restored."
