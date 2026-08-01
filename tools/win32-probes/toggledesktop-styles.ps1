# Empirical probe: which window style/ownership combos survive Win+D
# (Shell.Application.ToggleDesktop) on this machine, WITHOUT being minimized?
#
# Creates 5 small labelled windows + 1 hidden host window, toggles the
# desktop twice via COM (same code path as Win+D), and prints
# IsIconic/IsWindowVisible after each toggle.
#
# NOTE: this will minimize and restore all open windows once — save your work.

Add-Type -ReferencedAssemblies System.Windows.Forms -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
using System.Windows.Forms;

public class WinDProbe
{
    [DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    public static extern IntPtr CreateWindowExW(int exStyle, string cls, string name,
        int style, int x, int y, int w, int h, IntPtr parent, IntPtr menu, IntPtr inst, IntPtr param);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("kernel32.dll")] public static extern IntPtr GetModuleHandleW(string name);

    const int WS_POPUP    = unchecked((int)0x80000000);
    const int WS_VISIBLE  = 0x10000000;
    const int WS_DISABLED = 0x08000000;
    const int WS_OVERLAPPEDWINDOW = 0x00CF0000;
    const int WS_EX_TOOLWINDOW = 0x00000080;

    public static void ToggleDesktopCom()
    {
        object shell = Activator.CreateInstance(Type.GetTypeFromProgID("Shell.Application"));
        shell.GetType().InvokeMember("ToggleDesktop",
            System.Reflection.BindingFlags.InvokeMethod, null, shell, null);
    }

    public static string State(IntPtr h)
    {
        return "iconic=" + IsIconic(h).ToString().PadRight(5) +
               " visible=" + IsWindowVisible(h);
    }

    public static void Dump(string title, string[] names, IntPtr[] hwnds, StringBuilder sb)
    {
        sb.AppendLine(title);
        for (int i = 0; i < names.Length; i++)
            sb.AppendLine("  " + names[i].PadRight(38) + State(hwnds[i]));
    }

    public static int Main()
    {
        IntPtr inst = GetModuleHandleW(null);

        // Hidden owner host (Rainmeter-style): TOOLWINDOW | POPUP | DISABLED, never shown
        IntPtr host = CreateWindowExW(WS_EX_TOOLWINDOW, "STATIC", "ProbeHost",
            WS_POPUP | WS_DISABLED, 0, 0, 0, 0, IntPtr.Zero, IntPtr.Zero, inst, IntPtr.Zero);

        IntPtr a = CreateWindowExW(0, "STATIC", "A popup unowned",
            WS_POPUP | WS_VISIBLE, 50, 60, 230, 90, IntPtr.Zero, IntPtr.Zero, inst, IntPtr.Zero);
        IntPtr b = CreateWindowExW(WS_EX_TOOLWINDOW, "STATIC", "B popup+tool unowned",
            WS_POPUP | WS_VISIBLE, 310, 60, 230, 90, IntPtr.Zero, IntPtr.Zero, inst, IntPtr.Zero);
        IntPtr c = CreateWindowExW(0, "STATIC", "C popup owned-by-host",
            WS_POPUP | WS_VISIBLE, 570, 60, 230, 90, host, IntPtr.Zero, inst, IntPtr.Zero);
        IntPtr d = CreateWindowExW(WS_EX_TOOLWINDOW, "STATIC", "D popup+tool owned-by-host",
            WS_POPUP | WS_VISIBLE, 830, 60, 230, 90, host, IntPtr.Zero, inst, IntPtr.Zero);
        IntPtr e = CreateWindowExW(0, "STATIC", "E overlapped control",
            WS_OVERLAPPEDWINDOW | WS_VISIBLE, 1090, 60, 230, 90, IntPtr.Zero, IntPtr.Zero, inst, IntPtr.Zero);

        string[] names = new string[] {
            "A popup unowned",
            "B popup+tool unowned (=current skin)",
            "C popup owned-by-hidden-host",
            "D popup+tool owned-by-hidden-host",
            "E overlapped (control, must minimize)"
        };
        IntPtr[] hwnds = new IntPtr[] { a, b, c, d, e };

        StringBuilder sb = new StringBuilder();
        sb.AppendLine("=== Win+D survival probe ===");

        Timer t1 = new Timer(); t1.Interval = 1500;
        Timer t2 = new Timer(); t2.Interval = 1500;
        Timer t3 = new Timer(); t3.Interval = 1500;

        t1.Tick += delegate(object s, EventArgs ev)
        {
            t1.Stop();
            sb.AppendLine(">> ToggleDesktop #1 (show desktop)");
            ToggleDesktopCom();
            t2.Start();
        };
        t2.Tick += delegate(object s, EventArgs ev)
        {
            t2.Stop();
            Dump("-- state AFTER show desktop:", names, hwnds, sb);
            sb.AppendLine(">> ToggleDesktop #2 (restore)");
            ToggleDesktopCom();
            t3.Start();
        };
        t3.Tick += delegate(object s, EventArgs ev)
        {
            t3.Stop();
            Dump("-- state AFTER restore:", names, hwnds, sb);
            Console.WriteLine(sb.ToString());
            Application.Exit();
        };

        t1.Start();
        Application.Run();
        return 0;
    }
}
'@

[WinDProbe]::Main() | Out-Null
