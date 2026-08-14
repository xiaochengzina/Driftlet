# verify_manager_frame.py — find Driftlet manager window of a running driftlet.exe,
# force-show it, dump GWL_STYLE + DWMWA_EXTENDED_FRAME_BOUNDS.
# usage: python tools/win32-probes/verify_manager_frame.py
import ctypes
from ctypes import wintypes

user32 = ctypes.windll.user32
dwm = ctypes.windll.dwmapi
kernel32 = ctypes.windll.kernel32

WNDENUMPROC = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)


class RECT(ctypes.Structure):
    _fields_ = [("left", wintypes.LONG), ("top", wintypes.LONG),
                ("right", wintypes.LONG), ("bottom", wintypes.LONG)]


def find_manager():
    target_pid = wintypes.DWORD()
    hits = []

    def cb(hwnd, _):
        pid = wintypes.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
        buf = ctypes.create_unicode_buffer(256)
        user32.GetWindowTextW(hwnd, buf, 256)
        cls = ctypes.create_unicode_buffer(256)
        user32.GetClassNameW(hwnd, cls, 256)
        if buf.value == "Driftlet":
            hits.append((hwnd, pid.value, cls.value))
        return True

    user32.EnumWindows(WNDENUMPROC(cb), 0)
    return hits


def main():
    hits = find_manager()
    if not hits:
        print("manager window not found")
        raise SystemExit(1)
    for hwnd, pid, cls in hits:
        print(f"hwnd={hwnd} pid={pid} class={cls!r}")
    hwnd = hits[0][0]

    user32.ShowWindow(hwnd, 5)  # SW_SHOW
    user32.SetForegroundWindow(hwnd)
    import time
    time.sleep(1.2)

    style = user32.GetWindowLongPtrW(hwnd, -16)  # GWL_STYLE
    wr, ex = RECT(), RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(wr))
    dwm.DwmGetWindowAttribute(hwnd, 9, ctypes.byref(ex), ctypes.sizeof(ex))  # EXTENDED_FRAME_BOUNDS
    print(f"style=0x{style & 0xFFFFFFFF:08X} "
          f"THICKFRAME={bool(style & 0x00040000)} BORDER={bool(style & 0x00800000)} "
          f"CAPTION={style & 0x00C00000 == 0x00C00000}")  # 须 BORDER|DLGFRAME 同时置位才是真 CAPTION（单 WS_BORDER 不误报）
    print(f"rect=({wr.left},{wr.top})-({wr.right},{wr.bottom}) "
          f"ext=({ex.left},{ex.top})-({ex.right},{ex.bottom}) "
          f"dL={wr.left-ex.left} dT={wr.top-ex.top} dR={ex.right-wr.right} dB={ex.bottom-wr.bottom}")


if __name__ == "__main__":
    main()
