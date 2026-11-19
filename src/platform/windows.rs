//! Windows: priority class, power throttling (Efficiency mode), memory priority,
//! working-set trim, and process suspend/resume.

use super::Saved;
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::io;
use std::sync::Mutex;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM};
use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
use windows_sys::Win32::System::ProcessStatus::EmptyWorkingSet;
use windows_sys::Win32::System::Threading::{
    BELOW_NORMAL_PRIORITY_CLASS, GetPriorityClass, GetProcessInformation, IDLE_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS,
    OpenProcess, PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
    PROCESS_POWER_THROTTLING_STATE, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION, PROCESS_SET_QUOTA,
    PROCESS_SUSPEND_RESUME, ProcessMemoryPriority, ProcessPowerThrottling, SetPriorityClass, SetProcessInformation,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{EnumChildWindows, GetForegroundWindow, GetWindowThreadProcessId};
use windows_sys::core::BOOL;

pub const EASE_TEXT: &str = "lower priority; under pressure also Efficiency mode and low memory priority";
pub const CAN_TRIM: bool = true;

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtSuspendProcess(process: HANDLE) -> i32;
    fn NtResumeProcess(process: HANDLE) -> i32;
}

#[repr(C)]
struct MemoryPriority {
    value: u32,
}
const MEMORY_PRIORITY_LOW: u32 = 2;

struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

fn open(pid: u32, access: u32) -> io::Result<Handle> {
    let handle = unsafe { OpenProcess(access, 0, pid) };
    if handle.is_null() { Err(io::Error::last_os_error()) } else { Ok(Handle(handle)) }
}

fn check(ok: BOOL) -> io::Result<()> {
    if ok == 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

/// Lets `spry status` and friends print to the terminal they were started from.
pub fn attach_console() {
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}

unsafe extern "system" fn first_other_pid(hwnd: HWND, out: LPARAM) -> BOOL {
    let out = out as *mut [u32; 2];
    let mut pid = 0u32;
    unsafe {
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid != 0 && pid != (*out)[0] {
            (*out)[1] = pid;
            return 0;
        }
    }
    1
}

/// The process behind the window in front. Store apps sit inside a shared frame window,
/// so the real app is looked up among that frame's child windows.
pub fn foreground_pid() -> Option<u32> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return None;
        }
        let mut found = [pid, 0u32];
        EnumChildWindows(hwnd, Some(first_other_pid), &mut found as *mut _ as LPARAM);
        Some(if found[1] != 0 && is_frame_host(pid) { found[1] } else { pid })
    }
}

fn is_frame_host(pid: u32) -> bool {
    use windows_sys::Win32::System::Threading::QueryFullProcessImageNameW;
    let Ok(handle) = open(pid, PROCESS_QUERY_LIMITED_INFORMATION) else { return false };
    let mut buf = [0u16; 512];
    let mut len = buf.len() as u32;
    if unsafe { QueryFullProcessImageNameW(handle.0, 0, buf.as_mut_ptr(), &mut len) } == 0 {
        return false;
    }
    String::from_utf16_lossy(&buf[..len as usize]).to_lowercase().ends_with("applicationframehost.exe")
}

fn rank(class: u32) -> u8 {
    match class {
        IDLE_PRIORITY_CLASS => 0,
        BELOW_NORMAL_PRIORITY_CLASS => 1,
        NORMAL_PRIORITY_CLASS => 2,
        _ => 3,
    }
}

/// The priority class Spry last gave each process (0 = hands off). Some apps move their own
/// processes up and down (browsers do, for their tabs); Spry must never undo such a change.
static OURS: Mutex<BTreeMap<u32, u32>> = Mutex::new(BTreeMap::new());

// How `Saved::b` is packed: memory priority + 1 in the low byte (0 = unknown), then three
// bits for the Efficiency-mode state: known, controlled by the app, switched on.
const THROTTLE_KNOWN: i64 = 1 << 8;
const THROTTLE_CONTROLLED: i64 = 1 << 9;
const THROTTLE_ON: i64 = 1 << 10;

pub fn save(pid: u32) -> io::Result<Saved> {
    let handle = open(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
    let class = unsafe { GetPriorityClass(handle.0) };
    if class == 0 {
        return Err(io::Error::last_os_error());
    }
    if rank(class) > 2 {
        // Someone raised this process on purpose. Leave it alone.
        return Err(io::Error::other("priority was raised on purpose"));
    }
    let mut b = 0i64;
    let mut mem = MemoryPriority { value: 0 };
    if unsafe { GetProcessInformation(handle.0, ProcessMemoryPriority, &mut mem as *mut _ as *mut c_void, 4) } != 0 {
        b |= (mem.value as i64 + 1) & 0xff;
    }
    let mut state = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: 0,
        StateMask: 0,
    };
    let size = std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32;
    if unsafe { GetProcessInformation(handle.0, ProcessPowerThrottling, &mut state as *mut _ as *mut c_void, size) }
        != 0
    {
        b |= THROTTLE_KNOWN;
        if state.ControlMask & PROCESS_POWER_THROTTLING_EXECUTION_SPEED != 0 {
            b |= THROTTLE_CONTROLLED;
        }
        if state.StateMask & PROCESS_POWER_THROTTLING_EXECUTION_SPEED != 0 {
            b |= THROTTLE_ON;
        }
    }
    Ok(Saved { a: class as i64, b })
}

fn set_throttle(handle: &Handle, controlled: bool, on: bool) {
    let bit = |yes: bool| if yes { PROCESS_POWER_THROTTLING_EXECUTION_SPEED } else { 0 };
    let state = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        // Control mask 0 hands the decision back to Windows, which is the normal state.
        ControlMask: bit(controlled),
        StateMask: bit(controlled && on),
    };
    let size = std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32;
    unsafe { SetProcessInformation(handle.0, ProcessPowerThrottling, &state as *const _ as *const c_void, size) };
}

/// Efficiency mode and memory priority: lowered while `deep`, otherwise as they were found.
fn set_extras(handle: &Handle, saved: &Saved, deep: bool) {
    if deep {
        set_throttle(handle, true, true);
    } else {
        set_throttle(handle, saved.b & THROTTLE_CONTROLLED != 0, saved.b & THROTTLE_ON != 0);
    }
    let memory = (saved.b & 0xff) as u32;
    if memory > 0 {
        let value = if deep { MEMORY_PRIORITY_LOW.min(memory - 1) } else { memory - 1 };
        let mem = MemoryPriority { value };
        unsafe { SetProcessInformation(handle.0, ProcessMemoryPriority, &mem as *const _ as *const c_void, 4) };
    }
}

/// Hold a process back. `deep` is used while the whole machine is under pressure.
/// A process is only ever lowered, never raised above what it had.
pub fn apply(pid: u32, saved: &Saved, deep: bool) -> io::Result<()> {
    let handle = open(pid, PROCESS_SET_INFORMATION | PROCESS_QUERY_LIMITED_INFORMATION)?;
    let current = unsafe { GetPriorityClass(handle.0) };
    let mut ours = OURS.lock().unwrap();
    // If the class is no longer what Spry set, the app manages it itself: hands off from now on.
    let hands_off = ours.get(&pid).is_some_and(|set| *set == 0 || *set != current);
    if hands_off {
        ours.insert(pid, 0);
    } else {
        let want = if deep { IDLE_PRIORITY_CLASS } else { BELOW_NORMAL_PRIORITY_CLASS };
        let class = if rank(saved.a as u32) <= rank(want) { saved.a as u32 } else { want };
        if class != current {
            check(unsafe { SetPriorityClass(handle.0, class) })?;
        }
        ours.insert(pid, class);
    }
    set_extras(&handle, saved, deep);
    Ok(())
}

pub fn restore(pid: u32, saved: &Saved) -> io::Result<()> {
    let handle = open(pid, PROCESS_SET_INFORMATION | PROCESS_QUERY_LIMITED_INFORMATION)?;
    set_extras(&handle, saved, false);
    let current = unsafe { GetPriorityClass(handle.0) };
    let put_back = match OURS.lock().unwrap().remove(&pid) {
        Some(set) => set != 0 && set == current,
        // After a crash there is no record: only ever raise back up to what was saved.
        None => current != 0 && rank(current) < rank(saved.a as u32),
    };
    if put_back && current != saved.a as u32 {
        check(unsafe { SetPriorityClass(handle.0, saved.a as u32) })?;
    }
    Ok(())
}

/// The process ended: drop what was remembered about it.
pub fn forget(pid: u32) {
    OURS.lock().unwrap().remove(&pid);
}

/// A number that shows the current state, for tests: the priority class.
/// Runs the calling process at normal priority until dropped, if it was started lower.
pub struct NormalPriority(u32);

impl NormalPriority {
    pub fn hold() -> Self {
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        unsafe {
            let me = GetCurrentProcess();
            let class = GetPriorityClass(me);
            if class != 0 && rank(class) < 2 && SetPriorityClass(me, NORMAL_PRIORITY_CLASS) != 0 {
                return NormalPriority(class);
            }
        }
        NormalPriority(0)
    }
}

impl Drop for NormalPriority {
    fn drop(&mut self) {
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        if self.0 != 0 {
            unsafe { SetPriorityClass(GetCurrentProcess(), self.0) };
        }
    }
}

/// How busy the whole machine was since the last call, in percent. One cheap system call.
pub fn cpu_total() -> Option<f32> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::GetSystemTimes;
    static LAST: Mutex<Option<(u64, u64)>> = Mutex::new(None);
    let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
    let (mut idle, mut kernel, mut user) = (zero, zero, zero);
    if unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) } == 0 {
        return None;
    }
    let ticks = |t: FILETIME| ((t.dwHighDateTime as u64) << 32) | t.dwLowDateTime as u64;
    // Kernel time includes idle time.
    let now = (ticks(idle), ticks(kernel) + ticks(user));
    let before = LAST.lock().ok()?.replace(now)?;
    let (idle, all) = (now.0.saturating_sub(before.0), now.1.saturating_sub(before.1));
    (all > 0).then(|| (100.0 * (1.0 - idle as f64 / all as f64)).clamp(0.0, 100.0) as f32)
}

pub fn read_level(pid: u32) -> Option<i64> {
    let handle = open(pid, PROCESS_QUERY_LIMITED_INFORMATION).ok()?;
    let class = unsafe { GetPriorityClass(handle.0) };
    (class != 0).then_some(class as i64)
}

pub fn suspend(pid: u32) -> io::Result<()> {
    let handle = open(pid, PROCESS_SUSPEND_RESUME)?;
    if unsafe { NtSuspendProcess(handle.0) } < 0 { Err(io::Error::other("suspend refused")) } else { Ok(()) }
}

pub fn resume(pid: u32) -> io::Result<()> {
    let handle = open(pid, PROCESS_SUSPEND_RESUME)?;
    if unsafe { NtResumeProcess(handle.0) } < 0 { Err(io::Error::other("resume refused")) } else { Ok(()) }
}

/// Ask a background process to hand its idle memory back to Windows.
pub fn trim(pid: u32) -> io::Result<()> {
    let handle = open(pid, PROCESS_SET_QUOTA | PROCESS_QUERY_LIMITED_INFORMATION)?;
    check(unsafe { EmptyWorkingSet(handle.0) })
}

/// Size and item count of the Recycle Bin, all drives.
pub fn recycle_bin() -> (u64, u64) {
    use windows_sys::Win32::UI::Shell::{SHQUERYRBINFO, SHQueryRecycleBinW};
    let mut info: SHQUERYRBINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHQUERYRBINFO>() as u32;
    if unsafe { SHQueryRecycleBinW(std::ptr::null(), &mut info) } != 0 {
        return (0, 0);
    }
    (info.i64Size.max(0) as u64, info.i64NumItems.max(0) as u64)
}

pub fn empty_recycle_bin() -> bool {
    use windows_sys::Win32::UI::Shell::SHEmptyRecycleBinW;
    // 7 = no confirmation, no progress window, no sound
    unsafe { SHEmptyRecycleBinW(std::ptr::null_mut(), std::ptr::null(), 7) == 0 }
}
