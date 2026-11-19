"""Win32 process accounting and window checks for the native comparison."""

import ctypes as c
from ctypes import wintypes as w
import subprocess
from threading import Event, Thread
from time import perf_counter_ns


class StartupInfo(c.Structure):
    _fields_ = [
        ("cb", w.DWORD), ("reserved", w.LPWSTR), ("desktop", w.LPWSTR), ("title", w.LPWSTR),
        *[(name, w.DWORD) for name in ("x", "y", "width", "height", "cols", "rows", "fill", "flags")],
        ("show", w.WORD), ("reserved_size", w.WORD), ("reserved_data", c.c_void_p),
        ("stdin", w.HANDLE), ("stdout", w.HANDLE), ("stderr", w.HANDLE),
    ]


class ProcessInfo(c.Structure):
    _fields_ = [("process", w.HANDLE), ("thread", w.HANDLE), ("pid", w.DWORD), ("tid", w.DWORD)]


class Accounting(c.Structure):
    _fields_ = [
        ("user", c.c_int64), ("kernel", c.c_int64), ("period_user", c.c_int64),
        ("period_kernel", c.c_int64), ("faults", w.DWORD), ("total", w.DWORD),
        ("active", w.DWORD), ("terminated", w.DWORD),
    ]


class Memory(c.Structure):
    _fields_ = [
        ("size", w.DWORD), ("faults", w.DWORD), ("peak_ws", c.c_size_t), ("ws", c.c_size_t),
        *[(name, c.c_size_t) for name in ("peak_pool", "pool", "peak_nonpaged", "nonpaged",
                                          "pagefile", "peak_pagefile", "private")],
    ]


class PowerStatus(c.Structure):
    _fields_ = [
        ("ac", w.BYTE), ("flags", w.BYTE), ("charge", w.BYTE), ("saver", w.BYTE),
        ("remaining", w.DWORD), ("full", w.DWORD),
    ]


def checked(result):
    if not result:
        raise c.WinError(c.get_last_error())
    return result


class Windows:
    def __init__(self):
        kernel = c.WinDLL("kernel32", use_last_error=True)
        user = c.WinDLL("user32", use_last_error=True)
        gdi = c.WinDLL("gdi32", use_last_error=True)
        ptr, size_ptr = c.c_void_p, c.POINTER(w.DWORD)
        self.enum_proc = c.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM)
        self.event_proc = c.WINFUNCTYPE(None, w.HANDLE, w.DWORD, w.HWND, w.LONG, w.LONG, w.DWORD, w.DWORD)

        def bind(dll, name, returns, *args):
            func = getattr(dll, name)
            func.restype = returns
            func.argtypes = args
            setattr(self, name, func)

        bind(kernel, "CreateJobObjectW", w.HANDLE, ptr, w.LPCWSTR)
        bind(kernel, "CreateProcessW", w.BOOL, w.LPCWSTR, w.LPWSTR, ptr, ptr, w.BOOL, w.DWORD,
             ptr, w.LPCWSTR, c.POINTER(StartupInfo), c.POINTER(ProcessInfo))
        bind(kernel, "AssignProcessToJobObject", w.BOOL, w.HANDLE, w.HANDLE)
        bind(kernel, "ResumeThread", w.DWORD, w.HANDLE)
        bind(kernel, "TerminateProcess", w.BOOL, w.HANDLE, w.UINT)
        bind(kernel, "CloseHandle", w.BOOL, w.HANDLE)
        bind(kernel, "OpenProcess", w.HANDLE, w.DWORD, w.BOOL, w.DWORD)
        bind(kernel, "IsProcessInJob", w.BOOL, w.HANDLE, w.HANDLE, c.POINTER(w.BOOL))
        bind(kernel, "QueryInformationJobObject", w.BOOL, w.HANDLE, c.c_int, ptr, w.DWORD, size_ptr)
        bind(kernel, "QueryFullProcessImageNameW", w.BOOL, w.HANDLE, w.DWORD, w.LPWSTR, size_ptr)
        bind(kernel, "K32GetProcessMemoryInfo", w.BOOL, w.HANDLE, c.POINTER(Memory), w.DWORD)
        bind(kernel, "GetSystemPowerStatus", w.BOOL, c.POINTER(PowerStatus))
        bind(kernel, "GetSystemTimes", w.BOOL, *([c.POINTER(w.FILETIME)] * 3))
        bind(user, "SetProcessDpiAwarenessContext", w.BOOL, ptr)
        bind(user, "GetForegroundWindow", w.HWND)
        bind(user, "GetWindowThreadProcessId", w.DWORD, w.HWND, size_ptr)
        bind(user, "GetWindowTextW", c.c_int, w.HWND, w.LPWSTR, c.c_int)
        bind(user, "GetClassNameW", c.c_int, w.HWND, w.LPWSTR, c.c_int)
        bind(user, "GetWindowRect", w.BOOL, w.HWND, c.POINTER(w.RECT))
        bind(user, "IsIconic", w.BOOL, w.HWND)
        bind(user, "IsWindowVisible", w.BOOL, w.HWND)
        bind(user, "EnumWindows", w.BOOL, self.enum_proc, w.LPARAM)
        bind(user, "PostMessageW", w.BOOL, w.HWND, w.UINT, w.WPARAM, w.LPARAM)
        bind(user, "GetSystemMetrics", c.c_int, c.c_int)
        bind(user, "SetWinEventHook", w.HANDLE, w.DWORD, w.DWORD, w.HMODULE, self.event_proc,
             w.DWORD, w.DWORD, w.DWORD)
        bind(user, "GetMessageW", c.c_int, c.POINTER(w.MSG), w.HWND, w.UINT, w.UINT)
        bind(user, "GetDC", w.HDC, w.HWND)
        bind(user, "ReleaseDC", c.c_int, w.HWND, w.HDC)
        bind(gdi, "GetDeviceCaps", c.c_int, w.HDC, c.c_int)
        checked(self.SetProcessDpiAwarenessContext(c.c_void_p(-4)))

    def power(self):
        status = PowerStatus()
        checked(self.GetSystemPowerStatus(c.byref(status)))
        return {key: getattr(status, key) for key in ("ac", "charge", "saver")}

    def display(self):
        dc = checked(self.GetDC(None))
        try:
            return dict(width=self.GetSystemMetrics(0), height=self.GetSystemMetrics(1),
                        monitors=self.GetSystemMetrics(80), refresh_hz=self.GetDeviceCaps(dc, 116))
        finally:
            self.ReleaseDC(None, dc)

    def system_cpu(self):
        values = (w.FILETIME(), w.FILETIME(), w.FILETIME())
        checked(self.GetSystemTimes(*(c.byref(value) for value in values)))
        return dict(zip(("idle_100ns", "kernel_100ns", "user_100ns"), (
            (value.dwHighDateTime << 32) | value.dwLowDateTime for value in values
        )))

    def foreground_events(self):
        events, errors = [], []
        ready = Event()

        @self.event_proc
        def capture(_hook, _event, hwnd, _obj, _child, _tid, event_ms):
            events.append(dict(hwnd=hwnd, qpc_ns=perf_counter_ns(), win_event_ms=event_ms))

        def observe():
            try:
                # EVENT_SYSTEM_FOREGROUND and WINEVENT_OUTOFCONTEXT on a dedicated message queue.
                checked(self.SetWinEventHook(3, 3, None, capture, 0, 0, 0))
                ready.set()
                msg = w.MSG()
                while self.GetMessageW(c.byref(msg), None, 0, 0) > 0:
                    pass
            except OSError as error:
                errors.append(error)
                ready.set()

        Thread(target=observe, daemon=True).start()
        if not ready.wait(5) or errors:
            raise RuntimeError(f"failed to observe foreground events: {errors}")
        return events


class Job:
    def __init__(self, api):
        self.api = api
        self.handle = checked(api.CreateJobObjectW(None, None))

    def close(self):
        checked(self.api.CloseHandle(self.handle))

    def start(self, exe, args, env):
        api = self.api
        startup = StartupInfo(cb=c.sizeof(StartupInfo))
        process = ProcessInfo()
        command = c.create_unicode_buffer(subprocess.list2cmdline([str(exe), *args]))
        environment = c.create_unicode_buffer(
            "\0".join(f"{k}={v}" for k, v in sorted(env.items(), key=lambda kv: kv[0].upper())) + "\0\0")
        # Suspend the unstarted launcher so its descendants inherit the job from their first CPU tick.
        checked(api.CreateProcessW(str(exe), command, None, None, False, 0x404, environment,
                                   str(exe.parent), c.byref(startup), c.byref(process)))
        try:
            checked(api.AssignProcessToJobObject(self.handle, process.process))
            if api.ResumeThread(process.thread) == 0xFFFFFFFF:
                raise c.WinError(c.get_last_error())
        except OSError:
            api.TerminateProcess(process.process, 1)
            raise
        finally:
            api.CloseHandle(process.thread)
            api.CloseHandle(process.process)
        return process.pid

    def accounting(self):
        counters = Accounting()
        checked(self.api.QueryInformationJobObject(self.handle, 1, c.byref(counters), c.sizeof(counters), None))
        return dict(user_100ns=counters.user, kernel_100ns=counters.kernel,
                    active=counters.active, lifetime_processes=counters.total)

    def pids(self):
        slots = 32
        while True:
            buf = c.create_string_buffer(8 + slots * c.sizeof(c.c_size_t))
            if self.api.QueryInformationJobObject(self.handle, 3, buf, c.sizeof(buf), None):
                count = w.DWORD.from_buffer(buf, 4).value
                return list((c.c_size_t * count).from_buffer(buf, 8))
            if c.get_last_error() != 234 or slots >= 4096:  # ERROR_MORE_DATA
                raise c.WinError(c.get_last_error())
            slots *= 2

    def contains(self, pid):
        process = self.api.OpenProcess(0x1000, False, pid)  # QUERY_LIMITED_INFORMATION
        if not process:
            return False
        try:
            inside = w.BOOL()
            return bool(self.api.IsProcessInJob(process, self.handle, c.byref(inside)) and inside.value)
        finally:
            self.api.CloseHandle(process)

    def window(self, hwnd, expected_class):
        api = self.api
        if not hwnd or not api.IsWindowVisible(hwnd) or api.IsIconic(hwnd):
            return None
        pid, rect = w.DWORD(), w.RECT()
        api.GetWindowThreadProcessId(hwnd, c.byref(pid))
        name = c.create_unicode_buffer(256)
        api.GetClassNameW(hwnd, name, len(name))
        if name.value != expected_class or not self.contains(pid.value):
            return None
        title = c.create_unicode_buffer(512)
        api.GetWindowTextW(hwnd, title, len(title))
        checked(api.GetWindowRect(hwnd, c.byref(rect)))
        return dict(hwnd=hwnd, pid=pid.value, title=title.value,
                    rect=[rect.left, rect.top, rect.right, rect.bottom])

    def request_close(self):
        api = self.api
        members = set(self.pids())

        @api.enum_proc
        def close_owned(hwnd, _):
            pid = w.DWORD()
            api.GetWindowThreadProcessId(hwnd, c.byref(pid))
            if pid.value in members and api.IsWindowVisible(hwnd) and self.contains(pid.value):
                api.PostMessageW(hwnd, 0x10, 0, 0)  # WM_CLOSE
            return True

        checked(api.EnumWindows(close_owned, 0))

    def memory(self):
        api = self.api
        samples = []
        for pid in self.pids():
            process = api.OpenProcess(0x0410, False, pid)
            if not process:
                samples.append(dict(pid=pid, error=c.get_last_error()))
                continue
            try:
                assigned = w.BOOL()
                checked(api.IsProcessInJob(process, self.handle, c.byref(assigned)))
                if not assigned.value:
                    samples.append(dict(pid=pid, error="pid_reused"))
                    continue
                info = Memory(size=c.sizeof(Memory))
                ok = api.K32GetProcessMemoryInfo(process, c.byref(info), info.size)
                error = c.get_last_error() if not ok else None
                path = c.create_unicode_buffer(32768)
                size = w.DWORD(len(path))
                api.QueryFullProcessImageNameW(process, 0, path, c.byref(size))
                samples.append(dict(pid=pid, image=path.value, private=info.private, working_set=info.ws)
                               if ok else dict(pid=pid, image=path.value, error=error))
            finally:
                api.CloseHandle(process)
        return samples
