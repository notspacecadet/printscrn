use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use windows_sys::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, WPARAM,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT, VK_SNAPSHOT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, HHOOK, KBDLLHOOKSTRUCT, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL,
};

static HOOK: std::sync::Mutex<Option<HhookHolder>> = std::sync::Mutex::new(None);
static PRESSED: AtomicBool = AtomicBool::new(false);
static WANT_CTRL: AtomicBool = AtomicBool::new(false);
static WANT_ALT: AtomicBool = AtomicBool::new(false);
static WANT_SHIFT: AtomicBool = AtomicBool::new(false);
static WANT_META: AtomicBool = AtomicBool::new(false);
static ARMED: AtomicU8 = AtomicU8::new(0);

struct HhookHolder(HHOOK);

unsafe impl Send for HhookHolder {}

const LLKHF_UP: u32 = 0x80;

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
            return true;
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return false;
        }
        // Leak the handle so the mutex stays owned until the process exits.
        std::mem::forget(handle);
        true
    }
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
        let is_up = info.flags & LLKHF_UP != 0;
        if info.vkCode == VK_SNAPSHOT as u32 && !is_up && modifiers_match() {
            PRESSED.store(true, Ordering::SeqCst);
            // Swallow the key so Windows does not also open Snipping Tool.
            return 1;
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

fn modifiers_match() -> bool {
    fn down(vk: u16) -> bool {
        unsafe { GetAsyncKeyState(vk as i32) as u16 & 0x8000 != 0 }
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
