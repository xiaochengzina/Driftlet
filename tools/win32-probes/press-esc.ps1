Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public class K {
    [DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    public static void Esc() {
        keybd_event(0x1B, 0, 0, UIntPtr.Zero);
        keybd_event(0x1B, 0, 2, UIntPtr.Zero);
    }
}
"@
[K]::Esc()
