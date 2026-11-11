use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU8, Ordering};
use std::sync::Mutex;
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, WPARAM,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{CreateMutexW, GetCurrentProcessId};
use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_ESCAPE, VK_LWIN, VK_MENU, VK_RBUTTON, VK_RWIN, VK_SHIFT,
    VK_SNAPSHOT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, EnumWindows, GetWindowThreadProcessId, HHOOK, HWND_NOTOPMOST, KBDLLHOOKSTRUCT,
    PostMessageW, PostQuitMessage, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetForegroundWindow,
    SetWindowPos, SetWindowsHookExW, UnhookWindowsHookEx, WM_CLOSE, WH_KEYBOARD_LL,
};

use crate::gate::{async_key_is_down, low_level_key_is_release, rising_edge};

static HOOK: Mutex<Option<HhookHolder>> = Mutex::new(None);
static INSTANCE: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());
static PRESSED: AtomicBool = AtomicBool::new(false);
static OVERLAY: AtomicBool = AtomicBool::new(false);
static ESC_DOWN: AtomicBool = AtomicBool::new(false);
static RIGHT_DOWN: AtomicBool = AtomicBool::new(false);
static PANIC_DOWN: AtomicBool = AtomicBool::new(false);
static WANT_CTRL: AtomicBool = AtomicBool::new(false);
static WANT_ALT: AtomicBool = AtomicBool::new(false);
static WANT_SHIFT: AtomicBool = AtomicBool::new(false);
static WANT_META: AtomicBool = AtomicBool::new(false);
static ARMED: AtomicU8 = AtomicU8::new(0);

struct HhookHolder(HHOOK);

unsafe impl Send for HhookHolder {}

pub fn enable_dpi_awareness() {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

pub fn acquire_single_instance() -> bool {
    let name: Vec<u16> = "Local\\StillshotSingleton\0".encode_utf16().collect();
    unsafe {
        let handle = CreateMutexW(std::ptr::null(), 1, name.as_ptr());
        if handle.is_null() {
            crate::log::log("CreateMutexW failed; not starting a second hotkey handler");
            return false;
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            CloseHandle(handle);
            return false;
        }
        // Keep the handle alive for the process lifetime. Closing it would
        // drop the mutex and let a second Stillshot install another hook.
        INSTANCE.store(handle, Ordering::SeqCst);
        true
    }
}

pub fn set_overlay_active(active: bool) {
    OVERLAY.store(active, Ordering::SeqCst);
}

/// Edge-triggered. Esc or right-click while the overlay is up.
pub fn dismiss_pressed() -> bool {
    let escape_down = key_down(VK_ESCAPE);
    let escape = rising_edge(ESC_DOWN.swap(escape_down, Ordering::SeqCst), escape_down);
    let right_down = key_down(VK_RBUTTON);
    let right = rising_edge(RIGHT_DOWN.swap(right_down, Ordering::SeqCst), right_down);
    escape || right
}

/// Edge-triggered Ctrl+Alt+Q. This is not Ctrl+Shift+Esc and not Ctrl+Alt+Del.
pub fn panic_quit_pressed() -> bool {
    let down = key_down(VK_CONTROL) && key_down(VK_MENU) && key_down(0x51);
    rising_edge(PANIC_DOWN.swap(down, Ordering::SeqCst), down)
}

pub fn raise_app_windows() {
    unsafe {
        EnumWindows(Some(enum_raise), GetCurrentProcessId() as LPARAM);
    }
}

pub fn emergency_close_windows() {
    unsafe {
        EnumWindows(Some(enum_close), GetCurrentProcessId() as LPARAM);
        PostQuitMessage(0);
    }
}

unsafe extern "system" fn enum_raise(hwnd: HWND, pid: LPARAM) -> i32 {
    let mut owner = 0u32;
    GetWindowThreadProcessId(hwnd, &mut owner);
    if owner == pid as u32 {
        // Drop topmost so Task Manager, which is also topmost, can cover us.
        SetWindowPos(
            hwnd,
            HWND_NOTOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
        SetForegroundWindow(hwnd);
    }
    1
}

unsafe extern "system" fn enum_close(hwnd: HWND, pid: LPARAM) -> i32 {
    let mut owner = 0u32;
    GetWindowThreadProcessId(hwnd, &mut owner);
    if owner == pid as u32 {
        PostMessageW(hwnd, WM_CLOSE, 0, 0);
    }
    1
}

fn key_down(vk: u16) -> bool {
    unsafe { async_key_is_down(GetAsyncKeyState(vk as i32)) }
}

pub fn install_print_screen_hook(ctrl: bool, alt: bool, shift: bool, meta: bool) {
    WANT_CTRL.store(ctrl, Ordering::SeqCst);
    WANT_ALT.store(alt, Ordering::SeqCst);
    WANT_SHIFT.store(shift, Ordering::SeqCst);
    WANT_META.store(meta, Ordering::SeqCst);
    ARMED.store(1, Ordering::SeqCst);
    let mut slot = HOOK.lock().unwrap_or_else(|err| err.into_inner());
    if slot.is_some() {
        return;
    }
    unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), module, 0);
        if hook.is_null() {
            crate::log::log("Print Screen hook failed to install");
            return;
        }
        *slot = Some(HhookHolder(hook));
    }
    crate::log::log("Print Screen hook installed");
}

pub fn remove_print_screen_hook() {
    ARMED.store(0, Ordering::SeqCst);
    let mut slot = HOOK.lock().unwrap_or_else(|err| err.into_inner());
    if let Some(holder) = slot.take() {
        unsafe {
            UnhookWindowsHookEx(holder.0);
        }
    }
}

pub fn take_print_screen() -> bool {
    PRESSED.swap(false, Ordering::SeqCst)
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && ARMED.load(Ordering::SeqCst) == 1 {
        let info = &*(lparam as *const KBDLLHOOKSTRUCT);
        if info.vkCode == VK_SNAPSHOT as u32 && modifiers_match() {
            // Swallow press and release so Snipping Tool does not also open.
            // Repeats while the overlay is up must not queue another capture:
            // capture runs on this thread, and a second one stalls the hook.
            if !low_level_key_is_release(info.flags) && !OVERLAY.load(Ordering::SeqCst) {
                PRESSED.store(true, Ordering::SeqCst);
            }
            return 1;
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

fn modifiers_match() -> bool {
    fn down(vk: u16) -> bool {
        unsafe { async_key_is_down(GetAsyncKeyState(vk as i32)) }
    }
    down(VK_CONTROL) == WANT_CTRL.load(Ordering::SeqCst)
        && down(VK_MENU) == WANT_ALT.load(Ordering::SeqCst)
        && down(VK_SHIFT) == WANT_SHIFT.load(Ordering::SeqCst)
        && (down(VK_LWIN) || down(VK_RWIN)) == WANT_META.load(Ordering::SeqCst)
}

#[allow(dead_code)]
fn _hwnd_marker() -> HWND {
    std::ptr::null_mut()
}
