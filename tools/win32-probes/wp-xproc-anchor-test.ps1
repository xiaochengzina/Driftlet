# wp-xproc-anchor-test.ps1 — does SetWindowPos(A, insertAfter=<explorer host>)
# put A BELOW the host (documented) or ABOVE it (observed in the app)?
# Prints the windows directly above/below the red form before and after.
Add-Type -TypeDefinition @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class WX {
    [DllImport("user32.dll")] public static extern IntPtr GetShellWindow();
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr p, IntPtr a, string c, string t);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr ins, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h, uint cmd);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, System.Text.StringBuilder s, int n);

    public static IntPtr FindHost() {
        IntPtr shell = GetShellWindow();
        if (FindWindowExW(shell, IntPtr.Zero, "SHELLDLL_DefView", null) != IntPtr.Zero) return shell;
        IntPtr w = IntPtr.Zero;
        while (true) {
            w = FindWindowExW(IntPtr.Zero, w, "WorkerW", null);
            if (w == IntPtr.Zero) break;
            if (FindWindowExW(w, IntPtr.Zero, "SHELLDLL_DefView", null) != IntPtr.Zero) return w;
        }
        return IntPtr.Zero;
    }

    public static string D(IntPtr h) {
        if (h == IntPtr.Zero) return "0x0(null)";
        var c = new StringBuilder(64); GetClassNameW(h, c, 64);
        return string.Format("0x{0:X} {1}", h.ToInt64(), c.ToString());
    }

    public static string Around(IntPtr red) {
        IntPtr prev = GetWindow(red, 3); // GW_HWNDPREV
        IntPtr next = GetWindow(red, 2); // GW_HWNDNEXT
        return "  above red: " + D(prev) + "\n  red:       " + D(red) + "\n  below red: " + D(next);
    }
}
"@
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
$form = New-Object System.Windows.Forms.Form
$form.FormBorderStyle = 'None'
$form.BackColor = [System.Drawing.Color]::Red
$form.StartPosition = 'Manual'
$form.Location = New-Object System.Drawing.Point(1100, 300)
$form.Size = New-Object System.Drawing.Size(200, 200)
$form.ShowInTaskbar = $false
$null = $form.Handle
$form.Show()
$host_ = [WX]::FindHost()
"host: " + [WX]::D($host_)
"--- before ---"
[WX]::Around($form.Handle)
$r = [WX]::SetWindowPos($form.Handle, $host_, 0, 0, 0, 0, 0x0001 -bor 0x0002 -bor 0x0010)
"SetWindowPos(red, host) ret=$r"
"--- after SetWindowPos(red, host) ---"
[WX]::Around($form.Handle)
$form.Close()
