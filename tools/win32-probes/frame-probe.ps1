# frame-probe.ps1 — Win10 DWM frame rendering, round 3: frame-without-caption recipes
#   A  = control (plain WS_OVERLAPPEDWINDOW)
#   I  = WS_THICKFRAME|WS_BORDER|WS_SYSMENU|WS_MIN|WS_MAX (no WS_CAPTION), default NCCALCSIZE
#   J  = I but WS_DLGFRAME instead of WS_BORDER
#   K  = WS_THICKFRAME only (no BORDER/DLGFRAME), default NCCALCSIZE
# Prints DWMWA_EXTENDED_FRAME_BOUNDS to frame-probe.txt, screenshot to frame-probe.png (TEMP).
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

$cs = @"
using System;
using System.Runtime.InteropServices;
using System.Windows.Forms;
using System.Drawing;

public class W32 {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int attr, ref RECT r, int size);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int l, t, r, b;
    public override string ToString() { return "(" + l + "," + t + ")-(" + r + "," + b + ")"; } }
  public const int DWMWA_EXTENDED_FRAME_BOUNDS = 9;
}

public class ProbeForm : Form {
  public int StyleOverride;
  public ProbeForm(string name, int x, int y, int styleOverride) {
    Text = name; Name = name; StyleOverride = styleOverride;
    StartPosition = FormStartPosition.Manual;
    Location = new Point(x, y);
    Size = new Size(340, 220);
    TopMost = true;
    BackColor = Color.White;
  }
  protected override CreateParams CreateParams {
    get {
      var cp = base.CreateParams;
      if (StyleOverride != 0) cp.Style = StyleOverride | 0x10000000; // | WS_VISIBLE
      return cp;
    }
  }
  protected override void OnPaint(PaintEventArgs e) {
    base.OnPaint(e);
    TextRenderer.DrawText(e.Graphics, Name, Font, new Point(10, 10), Color.Black);
  }
}
"@
Add-Type -TypeDefinition $cs -ReferencedAssemblies System.Windows.Forms, System.Drawing

$BORDER    = 0x00800000
$DLGFRAME  = 0x00400000
$THICK     = 0x00040000
$SYSMENU   = 0x00080000
$MINBOX    = 0x00020000
$MAXBOX    = 0x00010000
$CLIPSIB   = 0x04000000

$script:wins = @()
foreach ($def in @(
  @('A-control',    60,  80,  0),
  @('I-thick-border', 440, 80,  ($THICK -bor $BORDER -bor $SYSMENU -bor $MINBOX -bor $MAXBOX -bor $CLIPSIB)),
  @('J-thick-dlg',    820, 80,  ($THICK -bor $DLGFRAME -bor $SYSMENU -bor $MINBOX -bor $MAXBOX -bor $CLIPSIB)),
  @('K-thick-only',   60, 420,  ($THICK -bor $SYSMENU -bor $MINBOX -bor $MAXBOX -bor $CLIPSIB)))) {
  $f = New-Object ProbeForm($def[0], $def[1], $def[2], $def[3])
  $f.Show()
  $script:wins += $f
}

$timer = New-Object System.Windows.Forms.Timer
$timer.Interval = 2500
$timer.add_Tick({
  $timer.Stop()
  $lines = @()
  foreach ($w in $script:wins) {
    $wr = New-Object W32+RECT; $ex = New-Object W32+RECT
    [W32]::GetWindowRect($w.Handle, [ref]$wr) | Out-Null
    [W32]::DwmGetWindowAttribute($w.Handle, [W32]::DWMWA_EXTENDED_FRAME_BOUNDS, [ref]$ex, 16) | Out-Null
    $delta = "dL={0} dT={1} dR={2} dB={3}" -f ($wr.l - $ex.l), ($wr.t - $ex.t), ($ex.r - $wr.r), ($ex.b - $wr.b)
    $lines += ("{0,-16} rect={1}  ext={2}  {3}" -f $w.Name, $wr, $ex, $delta)
  }
  [System.IO.File]::WriteAllLines((Join-Path $env:TEMP 'frame-probe.txt'), $lines)
  $bmp = New-Object System.Drawing.Bitmap 1220, 660
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen(20, 40, 0, 0, $bmp.Size)
  $bmp.Save((Join-Path $env:TEMP 'frame-probe.png'), [System.Drawing.Imaging.ImageFormat]::Png)
  $g.Dispose(); $bmp.Dispose()
  [System.Windows.Forms.Application]::Exit()
})
$timer.Start()
[System.Windows.Forms.Application]::Run()
