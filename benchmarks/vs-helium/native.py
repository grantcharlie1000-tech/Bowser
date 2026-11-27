"""Windows job isolation, input, screen sampling and process counters."""

import ctypes as c
from ctypes import wintypes as w
from contextlib import contextmanager
import os
from pathlib import Path
import re
import subprocess
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "helium"))
from win_job import Job, ProcessInfo, StartupInfo, Windows, checked


class Unsafe(RuntimeError):
    """The entire benchmark must stop after an ownership or cleanup failure."""


class FocusUnavailable(RuntimeError):
    """The owned window could not be reactivated without sending input."""


def bind(dll, name, result, *args):
    fn = getattr(dll, name)
    fn.restype = result
    fn.argtypes = args
    return fn


kernel = c.WinDLL("kernel32", use_last_error=True)
user = c.WinDLL("user32", use_last_error=True)
gdi = c.WinDLL("gdi32", use_last_error=True)
ntdll = c.WinDLL("ntdll")
pdh = c.WinDLL("pdh")
ptr = c.c_void_p


def image_path(api, pid):
    handle = api.OpenProcess(0x1000, False, pid)
    if not handle:
        return None
    try:
        buffer = c.create_unicode_buffer(32768)
        length = w.DWORD(len(buffer))
        if api.QueryFullProcessImageNameW(handle, 0, buffer, c.byref(length)):
            return buffer.value
        return None
    finally:
        api.CloseHandle(handle)


def interactive_desktop():
    get_thread_id = bind(kernel, "GetCurrentThreadId", w.DWORD)
    get_desktop = bind(user, "GetThreadDesktop", w.HANDLE, w.DWORD)
    get_info = bind(user, "GetUserObjectInformationW", w.BOOL, w.HANDLE, c.c_int, ptr,
                    w.DWORD, ptr)
    open_input = bind(user, "OpenInputDesktop", w.HANDLE, w.DWORD, w.BOOL, w.DWORD)
    close_input = bind(user, "CloseDesktop", w.BOOL, w.HANDLE)

    def name(handle):
        value = c.create_unicode_buffer(256)
        needed = w.DWORD()
        checked(get_info(handle, 2, value, c.sizeof(value), c.byref(needed)))
        return value.value

    current = name(checked(get_desktop(get_thread_id())))
    input_desktop = open_input(0, False, 1)  # DESKTOP_READOBJECTS
    if not input_desktop:
        return dict(thread=current, input=None, error=c.get_last_error(), usable=False)
    try:
        active = name(input_desktop)
        return dict(thread=current, input=active, usable=current == active)
    finally:
        close_input(input_desktop)


def file_version(exe):
    version = c.WinDLL("version", use_last_error=True)
    dummy = w.DWORD()
    size = bind(version, "GetFileVersionInfoSizeW", w.DWORD, w.LPCWSTR, ptr)(
        str(exe), c.byref(dummy))
    if not size:
        return None
    buffer = c.create_string_buffer(size)
    checked(bind(version, "GetFileVersionInfoW", w.BOOL, w.LPCWSTR, w.DWORD, w.DWORD, ptr)(
        str(exe), 0, size, buffer))
    address, length = ptr(), w.UINT()
    checked(bind(version, "VerQueryValueW", w.BOOL, ptr, w.LPCWSTR, ptr, ptr)(
        buffer, "\\", c.byref(address), c.byref(length)))
    fixed = (w.DWORD * 13).from_address(address.value)
    return ".".join(str(component) for value in (fixed[2], fixed[3])
                    for component in (value >> 16, value & 0xFFFF))


class StartupEx(c.Structure):
    _fields_ = [("startup", StartupInfo), ("attributes", ptr)]


class BasicLimits(c.Structure):
    _fields_ = [
        ("process_time", c.c_int64), ("job_time", c.c_int64), ("flags", w.DWORD),
        ("min_ws", c.c_size_t), ("max_ws", c.c_size_t), ("active", w.DWORD),
        ("affinity", c.c_size_t), ("priority", w.DWORD), ("scheduling", w.DWORD),
    ]


class ExtendedLimits(c.Structure):
    _fields_ = [
        ("basic", BasicLimits), ("io", c.c_uint64 * 6),
        ("memory_limits_and_peaks", c.c_size_t * 4),
    ]


init_attrs = bind(kernel, "InitializeProcThreadAttributeList", w.BOOL,
                  ptr, w.DWORD, w.DWORD, c.POINTER(c.c_size_t))
update_attr = bind(kernel, "UpdateProcThreadAttribute", w.BOOL,
                   ptr, w.DWORD, c.c_size_t, ptr, c.c_size_t, ptr, ptr)
delete_attrs = bind(kernel, "DeleteProcThreadAttributeList", None, ptr)
set_job = bind(kernel, "SetInformationJobObject", w.BOOL, w.HANDLE, c.c_int, ptr, w.DWORD)
terminate_job = bind(kernel, "TerminateJobObject", w.BOOL, w.HANDLE, w.UINT)
get_process_times = bind(kernel, "GetProcessTimes", w.BOOL, w.HANDLE,
                         *([c.POINTER(c.c_uint64)] * 4))
query_process_cycles = bind(kernel, "QueryProcessCycleTime", w.BOOL, w.HANDLE,
                            c.POINTER(c.c_uint64))
query_thread_cycles = bind(kernel, "QueryThreadCycleTime", w.BOOL, w.HANDLE,
                           c.POINTER(c.c_uint64))
current_thread_handle = bind(kernel, "GetCurrentThread", w.HANDLE)
set_execution_state = bind(kernel, "SetThreadExecutionState", w.DWORD, w.DWORD)


@contextmanager
def keep_display_awake():
    previous = checked(set_execution_state(0x80000003))
    try:
        yield
    finally:
        checked(set_execution_state(previous | 0x80000000))


def cycle_calibration():
    """Calibrate a short, CPU-bound current-thread sample against QPC."""
    thread = current_thread_handle()
    samples = []
    for _ in range(5):
        first, last = c.c_uint64(), c.c_uint64()
        checked(query_thread_cycles(thread, c.byref(first)))
        start = time.perf_counter_ns()
        now = start
        while now - start < 3_000_000:
            now = time.perf_counter_ns()
        checked(query_thread_cycles(thread, c.byref(last)))
        samples.append(dict(cycles=last.value - first.value, qpc_ns=now - start))
    return samples


class OwnedJob(Job):
    def __init__(self, api):
        super().__init__(api)
        limits = ExtendedLimits()
        limits.basic.flags = 0x2000  # JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        try:
            checked(set_job(self.handle, 9, c.byref(limits), c.sizeof(limits)))
        except OSError:
            self.close()
            raise

    def launch(self, exe, args, env, rect):
        # Atomic job assignment at creation. A failed assignment cannot leave a suspended
        # process outside the job or allow browser code to run before ownership is established.
        size = c.c_size_t()
        init_attrs(None, 1, 0, c.byref(size))
        attributes = c.create_string_buffer(size.value)
        checked(init_attrs(attributes, 1, 0, c.byref(size)))
        try:
            job_handle = w.HANDLE(self.handle)
            checked(update_attr(attributes, 0, 0x2000D, c.byref(job_handle),
                                c.sizeof(job_handle), None, None))
            x, y, width, height = rect
            info = StartupEx(StartupInfo(cb=c.sizeof(StartupEx), x=x, y=y, width=width,
                                         height=height, flags=6), c.addressof(attributes))
            proc = ProcessInfo()
            command = c.create_unicode_buffer(subprocess.list2cmdline([str(exe), *args]))
            environment = c.create_unicode_buffer(
                "\0".join(f"{k}={v}" for k, v in sorted(env.items(), key=lambda pair: pair[0].upper()))
                + "\0\0"
            )
            start = time.perf_counter()
            checked(self.api.CreateProcessW(str(exe), command, None, None, False, 0x80404,
                                           environment, str(exe.parent), c.byref(info.startup),
                                           c.byref(proc)))
            try:
                if self.api.ResumeThread(proc.thread) == 0xFFFFFFFF:
                    raise c.WinError(c.get_last_error())
                return start, proc.pid
            finally:
                self.api.CloseHandle(proc.thread)
                self.api.CloseHandle(proc.process)
        finally:
            delete_attrs(attributes)

    def process_cycles(self, pid, creation):
        process = self.api.OpenProcess(0x1000, False, pid)
        if not process:
            return None
        try:
            inside = w.BOOL()
            if not self.api.IsProcessInJob(process, self.handle, c.byref(inside)) or not inside.value:
                return None
            created, ended, system, user_time, cycles = (c.c_uint64() for _ in range(5))
            if (not get_process_times(process, c.byref(created), c.byref(ended),
                                      c.byref(system), c.byref(user_time))
                    or created.value != creation
                    or not query_process_cycles(process, c.byref(cycles))):
                return None
            return cycles.value
        finally:
            self.api.CloseHandle(process)

    def drain(self, grace=45):
        try:
            deadline = time.perf_counter() + grace
            while self.accounting()["active"] and time.perf_counter() < deadline:
                self.request_close()
                time.sleep(0.25)
            survivors = self.pids()
            if survivors:
                images = {pid: image_path(self.api, pid) for pid in survivors if self.contains(pid)}
                checked(terminate_job(self.handle, 1))
                deadline = time.perf_counter() + 5
                while self.accounting()["active"] and time.perf_counter() < deadline:
                    time.sleep(0.05)
                raise Unsafe(f"WM_CLOSE drain failed; job-only termination requested for {images}; "
                             f"remaining {self.pids()}. Profile retained.")
            drained = self.accounting()
            if drained["active"]:
                raise Unsafe("job accounting did not confirm a complete drain; profile retained")
            return drained
        finally:
            self.close()


class Mouse(c.Structure):
    _fields_ = [("x", w.LONG), ("y", w.LONG), ("data", w.DWORD), ("flags", w.DWORD),
                ("time", w.DWORD), ("extra", c.c_size_t)]


class Key(c.Structure):
    _fields_ = [("key", w.WORD), ("scan", w.WORD), ("flags", w.DWORD),
                ("time", w.DWORD), ("extra", c.c_size_t)]


class InputValue(c.Union):
    _fields_ = [("mouse", Mouse), ("key", Key)]


class Input(c.Structure):
    _fields_ = [("kind", w.DWORD), ("value", InputValue)]


send_input = bind(user, "SendInput", w.UINT, w.UINT, c.POINTER(Input), c.c_int)
window_at = bind(user, "WindowFromPoint", w.HWND, w.POINT)
ancestor = bind(user, "GetAncestor", w.HWND, w.HWND, w.UINT)
move_window = bind(user, "SetWindowPos", w.BOOL, w.HWND, w.HWND, c.c_int, c.c_int,
                   c.c_int, c.c_int, w.UINT)
show_window = bind(user, "ShowWindow", w.BOOL, w.HWND, c.c_int)
activate = bind(user, "SetForegroundWindow", w.BOOL, w.HWND)
switch_to_window = bind(user, "SwitchToThisWindow", None, w.HWND, w.BOOL)
dpi = bind(user, "GetDpiForWindow", w.UINT, w.HWND)
get_cursor = bind(user, "GetCursorPos", w.BOOL, c.POINTER(w.POINT))
get_key = bind(user, "GetAsyncKeyState", w.SHORT, c.c_int)
attach_input = bind(user, "AttachThreadInput", w.BOOL, w.DWORD, w.DWORD, w.BOOL)
current_thread = bind(kernel, "GetCurrentThreadId", w.DWORD)


class Desktop:
    def __init__(self, api, recoveries=None):
        self.api = api
        self.recoveries = recoveries

    def owns(self, job, hwnd):
        pid = w.DWORD()
        self.api.GetWindowThreadProcessId(hwnd, c.byref(pid))
        return job.contains(pid.value)

    def foreground(self, job, hwnd):
        before = self.api.GetForegroundWindow()
        if before != hwnd or not self.owns(job, before) or self.api.GetForegroundWindow() != before:
            pid = w.DWORD()
            self.api.GetWindowThreadProcessId(before, c.byref(pid))
            raise Unsafe(f"foreground ownership check failed: expected HWND {hwnd}, "
                         f"observed HWND {before}, PID {pid.value}")

    def ensure_foreground(self, job, hwnd):
        observed = self.api.GetForegroundWindow()
        if observed == hwnd:
            self.foreground(job, hwnd)
            return
        started = time.perf_counter()
        expires = started + 15
        attempts = 0
        attached_once = False
        while time.perf_counter() < expires:
            attempts += 1
            if not self.owns(job, hwnd):
                raise Unsafe("foreground recovery target left the harness job")
            if self.api.IsIconic(hwnd):
                show_window(hwnd, 9)
            activate(hwnd)
            if attempts >= 2 and self.api.GetForegroundWindow() != hwnd:
                # Request the top of the normal z-order for the owned window only.
                move_window(hwnd, None, 0, 0, 0, 0, 0x0043)
                # Windows may deny activation from a background thread. Share its
                # current foreground queue briefly, with no input while attached.
                foreground = self.api.GetForegroundWindow()
                thread = self.api.GetWindowThreadProcessId(foreground, None)
                caller = current_thread()
                if thread and thread != caller and attach_input(caller, thread, True):
                    attached_once = True
                    try:
                        if self.owns(job, hwnd):
                            activate(hwnd)
                    finally:
                        if not attach_input(caller, thread, False):
                            raise Unsafe("failed to detach the temporary foreground input queue")
                if self.api.GetForegroundWindow() != hwnd and self.owns(job, hwnd):
                    # Windows 10 also exposes a window switch without an input
                    # packet. It is safe only with the same owned-HWND checks.
                    switch_to_window(hwnd, False)
            deadline = min(expires, time.perf_counter() + 0.5)
            while time.perf_counter() < deadline:
                if self.api.GetForegroundWindow() == hwnd:
                    self.foreground(job, hwnd)
                    if self.recoveries is not None:
                        self.recoveries.append(dict(from_hwnd=observed, hwnd=hwnd, attempts=attempts,
                                                    attached=attached_once, started=started,
                                                    recovered=time.perf_counter()))
                    return
                time.sleep(0.025)
        if not self.owns(job, hwnd):
            raise Unsafe("foreground recovery target left the harness job")
        pid = w.DWORD()
        current = self.api.GetForegroundWindow()
        self.api.GetWindowThreadProcessId(current, c.byref(pid))
        raise FocusUnavailable(f"could not reactivate owned HWND {hwnd} in {attempts} attempts "
                               f"over 15 s; foreground HWND {current}, PID {pid.value}")

    def send(self, job, hwnd, events, wheel=False, closing=False):
        packet = (Input * len(events))(*events)
        self.ensure_foreground(job, hwnd)
        if any(get_key(key) < 0 for key in (0x10, 0x11, 0x12, 0x5B, 0x5C)):
            raise Unsafe("modifier physically held before input")
        if wheel:
            pos = w.POINT()
            checked(get_cursor(c.byref(pos)))
            if not self.hit(hwnd, pos.x, pos.y):
                raise Unsafe("mouse-wheel target is outside the owned browser window")
        # One checked, indivisible SendInput packet per chord, character or wheel event.
        self.foreground(job, hwnd)
        start = time.perf_counter()
        sent = send_input(len(packet), packet, c.sizeof(Input))
        if sent != len(packet):
            raise Unsafe(f"SendInput delivered {sent}/{len(packet)} events")
        if not closing:
            self.ensure_foreground(job, hwnd)
        return start

    def chord(self, job, hwnd, key, ctrl=True, closing=False):
        def event(code, flags=0):
            return Input(1, InputValue(key=Key(code, 0, flags)))
        events = [event(key), event(key, 2)]
        if ctrl:
            events = [event(0x11), *events, event(0x11, 2)]
        return self.send(job, hwnd, events, closing=closing)

    def type(self, job, hwnd, text):
        utf16 = text.encode("utf-16-le")
        for offset in range(0, len(utf16), 2):
            unit = int.from_bytes(utf16[offset:offset + 2], "little")
            self.send(job, hwnd, [Input(1, InputValue(key=Key(0, unit, flag))) for flag in (4, 6)])

    def wheel_position(self, job, hwnd, x, y):
        self.ensure_foreground(job, hwnd)
        if not self.hit(hwnd, x, y):
            raise Unsafe("mouse target is covered")
        width, height = (self.api.GetSystemMetrics(axis) for axis in (0, 1))
        return self.send(job, hwnd, [Input(0, InputValue(mouse=Mouse(
            round(x * 65535 / (width - 1)), round(y * 65535 / (height - 1)), 0, 0x8001)))])

    def wheel(self, job, hwnd, delta):
        return self.send(job, hwnd, [Input(0, InputValue(mouse=Mouse(0, 0, delta, 0x800)))],
                         wheel=True)

    @staticmethod
    def root_at(x, y):
        return ancestor(window_at(w.POINT(x, y)), 2)

    @staticmethod
    def hit(hwnd, x, y):
        return Desktop.root_at(x, y) == hwnd


WindowProc = c.WINFUNCTYPE(c.c_ssize_t, w.HWND, w.UINT, w.WPARAM, w.LPARAM)


class WindowClass(c.Structure):
    _fields_ = [("style", w.UINT), ("proc", WindowProc), ("cls_extra", c.c_int),
                ("wnd_extra", c.c_int), ("instance", w.HINSTANCE), ("icon", w.HICON),
                ("cursor", w.HANDLE), ("brush", w.HBRUSH), ("menu", w.LPCWSTR),
                ("name", w.LPCWSTR)]


class Backdrop:
    _name = "BowserVsHeliumBackdrop"
    _proc = None

    def __init__(self, rect):
        create = bind(user, "CreateWindowExW", w.HWND, w.DWORD, w.LPCWSTR, w.LPCWSTR,
                      w.DWORD, c.c_int, c.c_int, c.c_int, c.c_int, w.HWND, w.HMENU, w.HINSTANCE, ptr)
        instance = bind(kernel, "GetModuleHandleW", w.HINSTANCE, w.LPCWSTR)(None)
        if Backdrop._proc is None:
            default = bind(user, "DefWindowProcW", c.c_ssize_t, w.HWND, w.UINT, w.WPARAM, w.LPARAM)
            proc = WindowProc(default)
            brush = bind(gdi, "GetStockObject", w.HBRUSH, c.c_int)(2)  # GRAY_BRUSH
            registration = WindowClass(proc=proc, instance=instance, brush=brush, name=self._name)
            checked(bind(user, "RegisterClassW", w.ATOM, c.POINTER(WindowClass))(c.byref(registration)))
            Backdrop._proc = proc
        # Nonactivating, borderless and owned by the harness process.
        self.hwnd = checked(create(0x08000088, self._name, "", 0x90000000, *rect,
                                   None, None, instance, None))
        self.rect = rect
        show_window(self.hwnd, 4)  # SW_SHOWNOACTIVATE
        checked(move_window(self.hwnd, w.HWND(-1), *rect, 0x0050))
        checked(bind(user, "UpdateWindow", w.BOOL, w.HWND)(self.hwnd))
        self.peek = bind(user, "PeekMessageW", w.BOOL, c.POINTER(w.MSG), w.HWND,
                         w.UINT, w.UINT, w.UINT)
        self.dispatch = bind(user, "DispatchMessageW", c.c_ssize_t, c.POINTER(w.MSG))

    def pump(self):
        msg = w.MSG()
        while self.peek(c.byref(msg), self.hwnd, 0, 0, 1):
            self.dispatch(c.byref(msg))

    def allow_window_above(self):
        checked(move_window(self.hwnd, w.HWND(-2), *self.rect, 0x0010))

    def close(self):
        checked(bind(user, "DestroyWindow", w.BOOL, w.HWND)(self.hwnd))


class Pixels:
    """Read a small composed-screen rectangle into an allocation reused across samples."""

    size = 16

    def __init__(self, api, rect):
        self.api = api
        self.x = rect[0] + rect[2] // 2 - self.size // 2
        self.y = rect[1] + rect[3] // 2 - self.size // 2
        self.screen = checked(api.GetDC(None))
        self.dc = checked(bind(gdi, "CreateCompatibleDC", w.HDC, w.HDC)(self.screen))
        header = (c.c_uint32 * 10)(40, self.size, self.size, (32 << 16) | 1)
        data = ptr()
        create = bind(gdi, "CreateDIBSection", w.HBITMAP, w.HDC, ptr, w.UINT,
                      c.POINTER(ptr), w.HANDLE, w.DWORD)
        self.bitmap = checked(create(self.dc, header, 0, c.byref(data), None, 0))
        self.select = bind(gdi, "SelectObject", w.HANDLE, w.HDC, w.HANDLE)
        self.old = checked(self.select(self.dc, self.bitmap))
        self.data = (c.c_uint32 * (self.size ** 2)).from_address(data.value)
        self.blit = bind(gdi, "BitBlt", w.BOOL, w.HDC, c.c_int, c.c_int, c.c_int, c.c_int,
                         w.HDC, c.c_int, c.c_int, w.DWORD)

    def read(self):
        checked(self.blit(self.dc, 0, 0, self.size, self.size, self.screen, self.x, self.y, 0x40CC0020))
        return tuple(value & 0xFFFFFF for value in self.data)

    def close(self):
        self.select(self.dc, self.old)
        bind(gdi, "DeleteObject", w.BOOL, w.HANDLE)(self.bitmap)
        bind(gdi, "DeleteDC", w.BOOL, w.HDC)(self.dc)
        self.api.ReleaseDC(None, self.screen)


class ProcessHeader(c.Structure):
    _fields_ = [
        ("next", w.ULONG), ("count", w.ULONG), ("private_ws", c.c_int64),
        ("faults", w.ULONG), ("thread_high", w.ULONG), ("cycles", c.c_uint64),
        ("created", c.c_int64), ("user", c.c_int64), ("kernel", c.c_int64),
        ("image_len", w.USHORT), ("image_max", w.USHORT), ("image", ptr),
        ("priority", w.LONG), ("pid", c.c_size_t), ("parent", c.c_size_t),
        ("handles", w.ULONG), ("session", w.ULONG), ("key", c.c_size_t),
        *[(field, c.c_size_t) for field in ("peak_virtual", "virtual")],
        ("page_faults", w.ULONG),
        *[(field, c.c_size_t) for field in ("peak_ws", "ws", "peak_paged", "paged",
                                           "peak_nonpaged", "nonpaged", "pagefile",
                                           "peak_pagefile", "private_pages")],
        ("io", c.c_int64 * 6),
    ]


class ThreadRecord(c.Structure):
    _fields_ = [
        ("kernel", c.c_int64), ("user", c.c_int64), ("created", c.c_int64),
        ("wait", w.ULONG), ("start", ptr), ("pid", c.c_size_t), ("tid", c.c_size_t),
        ("priority", w.LONG), ("base", w.LONG), ("switches", w.ULONG),
        ("state", w.ULONG), ("reason", w.ULONG),
    ]


query_system = bind(ntdll, "NtQuerySystemInformation", w.ULONG, w.ULONG, ptr,
                    w.ULONG, c.POINTER(w.ULONG))
query_process = bind(ntdll, "NtQueryInformationProcess", w.ULONG, w.HANDLE,
                     w.ULONG, ptr, w.ULONG, c.POINTER(w.ULONG))


def inventory(threads=False):
    size = 512 * 1024
    while size <= 64 * 1024 * 1024:
        buffer = c.create_string_buffer(size)
        needed = w.ULONG()
        status = query_system(5, buffer, size, c.byref(needed))
        if not status:
            break
        if status != 0xC0000004:
            raise RuntimeError(f"SystemProcessInformation status 0x{status:08x}")
        size = max(size * 2, needed.value)
    else:
        raise RuntimeError("SystemProcessInformation buffer limit exceeded")
    stamp = time.perf_counter()
    offset = 0
    rows = {}
    while True:
        header = ProcessHeader.from_buffer(buffer, offset)
        row = dict(pid=header.pid, created=header.created, session=header.session,
                   image=c.wstring_at(header.image, header.image_len // 2) if header.image else "",
                   cpu_100ns=header.user + header.kernel, private_ws=header.private_ws,
                   commit=header.pagefile, thread_count=header.count)
        if threads:
            offset_threads = offset + c.sizeof(ProcessHeader)
            thread_rows = (ThreadRecord * header.count).from_buffer(buffer, offset_threads)
            if any(thread.pid != header.pid for thread in thread_rows):
                raise RuntimeError("Windows process/thread layout validation failed")
            row["threads"] = {f"{thread.tid}:{thread.created}": thread.switches for thread in thread_rows}
        rows[header.pid] = row
        if not header.next:
            return stamp, rows
        offset += header.next


def command_line(api, pid):
    handle = api.OpenProcess(0x1000, False, pid)
    if not handle:
        return ""
    try:
        buffer = c.create_string_buffer(128 * 1024)
        if query_process(handle, 60, buffer, len(buffer), None):
            return ""
        length = w.USHORT.from_buffer(buffer).value
        return c.wstring_at(c.c_void_p.from_buffer(buffer, c.sizeof(c.c_void_p)).value, length // 2)
    finally:
        api.CloseHandle(handle)


def role(image, command):
    image = image.lower()
    if image == "bowser.exe":
        return "Bowser host"
    if "crashpad" in image or "--type=crashpad-handler" in command:
        return "crashpad"
    match = re.search(r"--type=([\w-]+)", command)
    if match:
        kind = match[1]
        if kind == "utility" and "network.mojom.NetworkService" in command:
            return "utility/network"
        return {"gpu-process": "gpu", "renderer": "renderer", "utility": "utility"}.get(kind, kind)
    return "browser/main" if command else "unknown"


class RawValue(c.Structure):
    _fields_ = [("status", w.DWORD), ("time", w.FILETIME), ("value", c.c_int64),
                ("second", c.c_int64), ("count", w.DWORD)]


class RawItem(c.Structure):
    _fields_ = [("name", w.LPWSTR), ("raw", RawValue)]


class Gpu:
    def __init__(self):
        self.query = w.HANDLE()
        self.counter = w.HANDLE()
        self.collect = bind(pdh, "PdhCollectQueryData", w.ULONG, w.HANDLE)
        self.array = bind(pdh, "PdhGetRawCounterArrayW", w.ULONG, w.HANDLE, ptr, ptr, ptr)
        self.close_query = bind(pdh, "PdhCloseQuery", w.ULONG, w.HANDLE)
        self.check(bind(pdh, "PdhOpenQueryW", w.ULONG, w.LPCWSTR, c.c_size_t, ptr)(
            None, 0, c.byref(self.query)))
        try:
            self.check(bind(pdh, "PdhAddEnglishCounterW", w.ULONG, w.HANDLE, w.LPCWSTR,
                            c.c_size_t, ptr)(self.query, r"\GPU Engine(*)\Running Time",
                                             0, c.byref(self.counter)))
        except RuntimeError:
            self.close()
            raise

    @staticmethod
    def check(status):
        if status:
            raise RuntimeError(f"GPU PDH status 0x{status:08x}")

    def read(self, pids):
        self.check(self.collect(self.query))
        stamp = time.perf_counter()
        size, count = w.DWORD(), w.DWORD()
        status = self.array(self.counter, c.byref(size), c.byref(count), None)
        if status != 0x800007D2 or not size.value:
            self.check(status)
            raise RuntimeError("GPU Engine counter contains no instances")
        buffer = c.create_string_buffer(size.value)
        self.check(self.array(self.counter, c.byref(size), c.byref(count), buffer))
        rows = {}
        for item in (RawItem * count.value).from_buffer(buffer):
            match = re.match(r"pid_(\d+)_", item.name)
            if match and int(match[1]) in pids and item.raw.status in (0, 1):
                rows[item.name] = item.raw.value
        return stamp, rows

    def close(self):
        self.close_query(self.query)


def cpu_percent(before, after):
    idle = after["idle_100ns"] - before["idle_100ns"]
    total = sum(after[key] - before[key] for key in ("kernel_100ns", "user_100ns"))
    if total <= 0:
        raise RuntimeError("system CPU sample interval too short")
    return 100 * (1 - idle / total)


def owned_window(api, job, expected):
    windows = []

    @api.enum_proc
    def observe(hwnd, _):
        match = job.window(hwnd, expected)
        if match:
            windows.append(match)
        return True

    checked(api.EnumWindows(observe, 0))
    if len(windows) > 1:
        raise RuntimeError(f"multiple owned browser windows: {windows}")
    return windows[0] if windows else None
