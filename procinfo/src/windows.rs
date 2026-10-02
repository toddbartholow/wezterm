#![cfg(windows)]

use super::*;
use ntapi::ntpebteb::PEB;
use ntapi::ntpsapi::{
    NtGetNextProcess, NtQueryInformationProcess, ProcessBasicInformation, ProcessWow64Information,
    PROCESS_BASIC_INFORMATION,
};
use ntapi::ntrtl::RTL_USER_PROCESS_PARAMETERS;
use ntapi::ntwow64::RTL_USER_PROCESS_PARAMETERS32;
use std::ffi::OsString;
use std::mem::MaybeUninit;
use std::os::windows::ffi::OsStringExt;
use winapi::shared::minwindef::{DWORD, FILETIME, LPVOID, MAX_PATH};
use winapi::shared::ntdef::{FALSE, NT_SUCCESS};
use winapi::shared::ntstatus::STATUS_PENDING;
use winapi::um::handleapi::CloseHandle;
use winapi::um::memoryapi::ReadProcessMemory;
use winapi::um::processthreadsapi::{GetCurrentProcessId, GetProcessTimes, OpenProcess};
use winapi::um::shellapi::CommandLineToArgvW;
use winapi::um::tlhelp32::*;
use winapi::um::winbase::{LocalFree, QueryFullProcessImageNameW};
use winapi::um::winnt::{
    HANDLE, PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
};

/// A process and its parent, as reported by one of the
/// enumeration methods below
struct ProcEntry {
    pid: u32,
    ppid: u32,
    /// The image name from a toolhelp snapshot. Only used when we
    /// cannot open the process to ask for its full path.
    exe_file: PathBuf,
}

/// Manages a Toolhelp32 snapshot handle
struct Snapshot(HANDLE);

impl Snapshot {
    pub fn new() -> Option<Self> {
        let handle = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if handle.is_null() {
            None
        } else {
            Some(Self(handle))
        }
    }

    pub fn iter(&self) -> ProcIter<'_> {
        ProcIter {
            snapshot: &self,
            first: true,
        }
    }

    pub fn entries() -> Vec<ProcEntry> {
        match Self::new() {
            Some(snapshot) => snapshot
                .iter()
                .map(|info| ProcEntry {
                    pid: info.th32ProcessID,
                    ppid: info.th32ParentProcessID,
                    exe_file: wstr_to_path(&info.szExeFile),
                })
                .collect(),
            None => vec![],
        }
    }
}

impl Drop for Snapshot {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

struct ProcIter<'a> {
    snapshot: &'a Snapshot,
    first: bool,
}

impl<'a> Iterator for ProcIter<'a> {
    type Item = PROCESSENTRY32W;

    fn next(&mut self) -> Option<Self::Item> {
        let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as _;
        let res = if self.first {
            self.first = false;
            unsafe { Process32FirstW(self.snapshot.0, &mut entry) }
        } else {
            unsafe { Process32NextW(self.snapshot.0, &mut entry) }
        };
        if res == 0 {
            None
        } else {
            Some(entry)
        }
    }
}

/// Lists the processes that we are able to open, along with their parents.
///
/// A toolhelp snapshot asks the kernel for every process and every thread
/// on the system, which takes 15ms or more on a busy machine. We only need
/// the pid and parent pid of processes that we can inspect, and walking
/// process handles with NtGetNextProcess gets those in about 1ms.
/// Processes that we cannot open at all (services running as other
/// accounts, protected processes) are left out.
fn openable_process_entries() -> Vec<ProcEntry> {
    let mut entries = vec![];
    let mut prev: Option<ProcHandle> = None;
    loop {
        let mut next: HANDLE = std::ptr::null_mut();
        let status = unsafe {
            NtGetNextProcess(
                prev.as_ref().map_or(std::ptr::null_mut(), |p| p.proc),
                PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                0,
                &mut next,
            )
        };
        if !NT_SUCCESS(status) {
            break;
        }
        let proc = ProcHandle { pid: 0, proc: next };
        if let Some(info) = proc.get_basic_info() {
            // The walk also yields processes that have exited but are
            // still referenced by an open handle somewhere; a toolhelp
            // snapshot would not list them, so skip them here too.
            if info.ExitStatus == STATUS_PENDING {
                entries.push(ProcEntry {
                    pid: info.UniqueProcessId as usize as u32,
                    ppid: info.InheritedFromUniqueProcessId as usize as u32,
                    exe_file: PathBuf::new(),
                });
            }
        }
        // Dropping the previous handle closes it; the walk continues
        // from the one we just opened.
        prev = Some(proc);
    }
    entries
}

fn wstr_to_path(slice: &[u16]) -> PathBuf {
    match slice.iter().position(|&c| c == 0) {
        Some(nul) => OsString::from_wide(&slice[..nul]),
        None => OsString::from_wide(slice),
    }
    .into()
}

fn wstr_to_string(slice: &[u16]) -> String {
    wstr_to_path(slice).to_string_lossy().into_owned()
}

struct ProcParams {
    argv: Vec<String>,
    cwd: PathBuf,
    console: HANDLE,
}

/// A handle to an opened process
struct ProcHandle {
    pid: u32,
    proc: HANDLE,
}

impl ProcHandle {
    pub fn new(pid: u32) -> Option<Self> {
        Self::with_access(pid, PROCESS_QUERY_INFORMATION | PROCESS_VM_READ)
    }

    pub fn with_access(pid: u32, options: DWORD) -> Option<Self> {
        if pid == unsafe { GetCurrentProcessId() } {
            // Avoid the potential for deadlock if we're examining ourselves
            log::trace!("ProcHandle::new({}): skip because it is my own pid", pid);
            return None;
        }
        log::trace!("ProcHandle::new({}): OpenProcess", pid);
        let handle = unsafe { OpenProcess(options, FALSE as _, pid) };
        log::trace!("ProcHandle::new({}): OpenProcess -> {:?}", pid, handle);
        if handle.is_null() {
            return None;
        }
        Some(Self { pid, proc: handle })
    }

    /// Returns the executable image for the process
    pub fn executable(&self) -> Option<PathBuf> {
        let mut buf = [0u16; MAX_PATH + 1];
        let mut len = buf.len() as DWORD;
        let res = unsafe { QueryFullProcessImageNameW(self.proc, 0, buf.as_mut_ptr(), &mut len) };
        if res == 0 {
            None
        } else {
            Some(wstr_to_path(&buf))
        }
    }

    /// Wrapper around NtQueryInformationProcess that fetches `what` as `T`
    fn query_proc<T>(&self, what: u32) -> Option<T> {
        let mut data = MaybeUninit::<T>::uninit();
        let res = unsafe {
            NtQueryInformationProcess(
                self.proc,
                what,
                data.as_mut_ptr() as _,
                std::mem::size_of::<T>() as _,
                std::ptr::null_mut(),
            )
        };
        if !NT_SUCCESS(res) {
            return None;
        }
        let data = unsafe { data.assume_init() };
        Some(data)
    }

    /// Read a `T` from the target process at the specified address
    fn read_struct<T>(&self, addr: LPVOID) -> Option<T> {
        let mut data = MaybeUninit::<T>::uninit();
        let res = unsafe {
            ReadProcessMemory(
                self.proc,
                addr as _,
                data.as_mut_ptr() as _,
                std::mem::size_of::<T>() as _,
                std::ptr::null_mut(),
            )
        };
        if res == 0 {
            return None;
        }
        let data = unsafe { data.assume_init() };
        Some(data)
    }

    /// If the process is a 32-bit process running on Win64, return the address
    /// of its process parameters.
    /// Otherwise, return None to indicate a native win64 process.
    fn get_peb32_addr(&self) -> Option<LPVOID> {
        let peb32_addr: LPVOID = self.query_proc(ProcessWow64Information)?;
        if peb32_addr.is_null() {
            None
        } else {
            Some(peb32_addr)
        }
    }

    /// Returns the cwd and args for the process
    pub fn get_params(&self) -> Option<ProcParams> {
        match self.get_peb32_addr() {
            Some(peb32) => self.get_params_32(peb32),
            None => self.get_params_64(),
        }
    }

    fn get_basic_info(&self) -> Option<PROCESS_BASIC_INFORMATION> {
        self.query_proc(ProcessBasicInformation)
    }

    fn get_peb(&self, info: &PROCESS_BASIC_INFORMATION) -> Option<PEB> {
        self.read_struct(info.PebBaseAddress as _)
    }

    fn get_proc_params(&self, peb: &PEB) -> Option<RTL_USER_PROCESS_PARAMETERS> {
        self.read_struct(peb.ProcessParameters as _)
    }

    /// Returns the cwd and args for a 64 bit process
    fn get_params_64(&self) -> Option<ProcParams> {
        let info = self.get_basic_info()?;
        let peb = self.get_peb(&info)?;
        let params = self.get_proc_params(&peb)?;

        let cmdline = self.read_process_wchar(
            params.CommandLine.Buffer as _,
            params.CommandLine.Length as _,
        )?;
        let cwd = self.read_process_wchar(
            params.CurrentDirectory.DosPath.Buffer as _,
            params.CurrentDirectory.DosPath.Length as _,
        )?;

        Some(ProcParams {
            argv: cmd_line_to_argv(&cmdline),
            cwd: wstr_to_path(&cwd),
            console: params.ConsoleHandle,
        })
    }

    fn get_proc_params_32(&self, peb32: LPVOID) -> Option<RTL_USER_PROCESS_PARAMETERS32> {
        self.read_struct(peb32)
    }

    /// Returns the cwd and args for a 32 bit process
    fn get_params_32(&self, peb32: LPVOID) -> Option<ProcParams> {
        let params = self.get_proc_params_32(peb32)?;

        let cmdline = self.read_process_wchar(
            params.CommandLine.Buffer as _,
            params.CommandLine.Length as _,
        )?;
        let cwd = self.read_process_wchar(
            params.CurrentDirectory.DosPath.Buffer as _,
            params.CurrentDirectory.DosPath.Length as _,
        )?;

        Some(ProcParams {
            argv: cmd_line_to_argv(&cmdline),
            cwd: wstr_to_path(&cwd),
            console: params.ConsoleHandle as _,
        })
    }

    /// Copies a sized WSTR from the address in the process
    fn read_process_wchar(&self, ptr: LPVOID, byte_size: usize) -> Option<Vec<u16>> {
        if byte_size > MAX_PATH * 4 {
            // Defend against implausibly large paths, just in
            // case we're reading the wrong offset into a kernel struct
            return None;
        }

        let mut buf = vec![0u16; byte_size / 2];
        let mut bytes_read = 0;

        let res = unsafe {
            ReadProcessMemory(
                self.proc,
                ptr as _,
                buf.as_mut_ptr() as _,
                byte_size,
                &mut bytes_read,
            )
        };
        if res == 0 {
            return None;
        }

        // In the unlikely event that we have a short read,
        // truncate the buffer to fit.
        let wide_chars_read = bytes_read / 2;
        buf.resize(wide_chars_read, 0);

        // Ensure that it is NUL terminated
        match buf.iter().position(|&c| c == 0) {
            Some(n) => {
                // Truncate to include existing NUL but no later chars
                buf.resize(n + 1, 0);
            }
            None => {
                // Add a NUL
                buf.push(0);
            }
        }

        Some(buf)
    }

    /// Retrieves the start time of the process
    fn start_time(&self) -> Option<u64> {
        const fn empty() -> FILETIME {
            FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            }
        }

        let mut start = empty();
        let mut exit = empty();
        let mut kernel = empty();
        let mut user = empty();

        let res =
            unsafe { GetProcessTimes(self.proc, &mut start, &mut exit, &mut kernel, &mut user) };
        if res == 0 {
            return None;
        }

        Some((start.dwHighDateTime as u64) << 32 | start.dwLowDateTime as u64)
    }
}

/// Parse a command line string into an argv array
fn cmd_line_to_argv(buf: &[u16]) -> Vec<String> {
    let mut argc = 0;
    let argvp = unsafe { CommandLineToArgvW(buf.as_ptr(), &mut argc) };
    if argvp.is_null() {
        return vec![];
    }

    let argv = unsafe { std::slice::from_raw_parts(argvp, argc as usize) };
    let mut args = vec![];
    for &arg in argv {
        let len = unsafe { libc::wcslen(arg) };
        let arg = unsafe { std::slice::from_raw_parts(arg, len) };
        args.push(wstr_to_string(arg));
    }
    unsafe { LocalFree(argvp as _) };
    args
}

impl Drop for ProcHandle {
    fn drop(&mut self) {
        log::trace!("ProcHandle::drop(pid={} proc={:?})", self.pid, self.proc);
        unsafe { CloseHandle(self.proc) };
    }
}

impl LocalProcessInfo {
    pub fn current_working_dir(pid: u32) -> Option<PathBuf> {
        log::trace!("current_working_dir({})", pid);
        let proc = ProcHandle::new(pid)?;
        let params = proc.get_params()?;
        Some(params.cwd)
    }

    pub fn executable_path(pid: u32) -> Option<PathBuf> {
        log::trace!("executable_path({})", pid);
        let proc = ProcHandle::new(pid)?;
        proc.executable()
    }

    pub fn with_root_pid(pid: u32) -> Option<Self> {
        log::trace!(
            "LocalProcessInfo::with_root_pid({}), listing processes",
            pid
        );
        let mut procs = openable_process_entries();
        if !procs.iter().any(|info| info.pid == pid) {
            // We can't open the root, so the cheap listing can't tell
            // us about it. Fall back to a full snapshot, which lists
            // every process whether we can open it or not.
            log::trace!("{} is not openable, getting snapshot", pid);
            procs = Snapshot::entries();
        }
        log::trace!("Got {} processes", procs.len());

        fn build_proc(
            info: &ProcEntry,
            procs: &[ProcEntry],
            visited: &mut HashSet<u32>,
        ) -> LocalProcessInfo {
            let mut children = HashMap::new();

            for kid in procs {
                if kid.ppid == info.pid && !visited.contains(&kid.pid) {
                    visited.insert(kid.pid);
                    children.insert(kid.pid, build_proc(kid, procs, visited));
                }
            }

            let mut executable = None;
            let mut start_time = 0;
            let mut cwd = PathBuf::new();
            let mut argv = vec![];
            let mut console = 0;

            if let Some(proc) = ProcHandle::new(info.pid) {
                if let Some(exe) = proc.executable() {
                    executable.replace(exe);
                }
                if let Some(params) = proc.get_params() {
                    cwd = params.cwd;
                    argv = params.argv;
                    console = params.console as _;
                }
                if let Some(start) = proc.start_time() {
                    start_time = start;
                }
            } else if let Some(proc) =
                ProcHandle::with_access(info.pid, PROCESS_QUERY_LIMITED_INFORMATION)
            {
                // We can't read its memory (eg: it is elevated and we
                // are not), but we can still learn its path and age.
                executable = proc.executable();
                start_time = proc.start_time().unwrap_or(0);
            }

            let executable = executable.unwrap_or_else(|| info.exe_file.clone());
            let name = match executable.file_name() {
                Some(name) => name.to_string_lossy().into_owned(),
                None => String::new(),
            };

            LocalProcessInfo {
                pid: info.pid,
                ppid: info.ppid,
                name,
                executable,
                cwd,
                argv,
                start_time,
                status: LocalProcessStatus::Run,
                children,
                console,
            }
        }

        if let Some(info) = procs.iter().find(|info| info.pid == pid) {
            let mut visited = HashSet::new();
            visited.insert(pid);
            Some(build_proc(info, &procs, &mut visited))
        } else {
            None
        }
    }
}
