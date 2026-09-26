//! rc003-helper —— RC003 增强捕获轨（路线 A）的提权助手（产品化 spike）。
//!
//! 它做什么
//! --------
//! 1. 从注册表定位承载 RC003 的 `WUDFHost.exe`（`Device Parameters\WUDFDiagnosticInfo\HostPid`），
//!    并核对它是「独占 RC003」宿主；
//! 2. 把**固定版本 + 已校验 SHA-256** 的 Frida Gadget 与我们自持的 agent 脚本准备到运行时目录；
//! 3. 用 `VirtualAllocEx` / `WriteProcessMemory` / `CreateRemoteThread(LoadLibraryW)` 把 Gadget
//!    加载进该宿主；
//! 4. 在 loopback 上起一个 TCP 服务，接收 agent 的 hello / hb / edge，并每 500ms 发一次续约；
//! 5. 收尾时发 `disarm`，让 agent 立即停止清键（清键许可本身也是租约制，见 agent 头部注释）。
//!
//! 收尾有三条路径，**都必须真的到得了发 `disarm` 那一步**：
//! `--duration` 到期、Ctrl+C / 关窗、agent 主动断开。
//! 2026-09-23 真机实测发现前两条此前都是**假的**，已修（详见各自注释）：
//! - `--duration`：会话读取原先用阻塞的 `BufReader::lines()`，主循环被同步阻塞在
//!   `serve_connection` 里 → 会话存活期间上限永不生效（`--duration 300` 跑到 445 s）。
//!   现改为读超时轮询 + 把绝对到期时刻传进会话循环。
//! - Ctrl+C / 关窗：原先**没有**安装 `SetConsoleCtrlHandler`，系统直接硬终止进程，
//!   `main()` 末尾那段收尾代码永远执行不到。现安装处理器：只置位，由主线程收尾。
//! 唯一不依赖以上任何一条的兜底是 **agent 侧租约**——这是设计上有意为之。
//!
//! 为什么需要提权
//! --------------
//! 宿主位于 session 0，普通用户令牌 `OpenProcess(PROCESS_ALL_ACCESS)` 返回 err=5。
//! 因此本程序必须由用户本人以管理员身份启动——自动化工具不应代用户提权（本仓库实测的安全边界）。
//!
//! 代价与边界（与 ADR 0002 修订记录、路线选型文档一致）
//! --------------------------------------------------
//! 不装内核驱动、不改 Secure Boot / 测试签名 / 驱动签名策略、不写注册表过滤项、
//! 不装计划任务、不需要重启。改动面只有：运行时目录里三个文件 + 目标宿主的进程内存。
//!
//! 本 spike **不并入产品工作区**（见 Cargo.toml 注释），也不改动 `src-tauri` / 前端。
//! 它只回答一个问题：把已验证的探针变成"自己的助手 + 自己的 agent + 自己的传输"之后，
//! 整链还能不能跑通。

#[cfg(not(windows))]
fn main() {
    eprintln!("sayall-helper 仅支持 Windows。");
    std::process::exit(2);
}

#[cfg(windows)]
fn main() {
    imp::run();
}

#[cfg(windows)]
mod imp {
    use std::collections::BTreeSet;
    use std::ffi::c_void;
    use std::fs;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{Ipv4Addr, Shutdown, SocketAddrV4, TcpListener, TcpStream};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{mpsc, Arc, Mutex};
    use std::thread::JoinHandle;
    use std::time::{Duration, Instant};

    // ============================================================ 常量

    /// RC003 的硬件 token（VID 0x2717 / PID 0x32B8 / REV 00a4），与仓库既有取证一致。
    const RC003_HARDWARE_TOKEN: &str = "vid&012717_pid&32b8_rev&00a4";
    /// BLE HID-over-GATT 服务的 UUID 前缀（设备实例名以它开头）。
    const HID_SERVICE_PREFIX: &str = "{00001812-0000-1000-8000-00805f9b34fb}";
    const ENUM_ROOT: &str = "SYSTEM\\CurrentControlSet\\Enum";
    const DIAGNOSTIC_SUFFIX: &str = "Device Parameters\\WUDFDiagnosticInfo";

    const DEFAULT_PORT: u16 = 47831;
    const RENEW_MS: u64 = 500;
    /// agent 侧租约（agent 在执行 `LEASE_MS` 后停止清键）。这里必须与 agent 的默认值一致。
    const LEASE_MS: u64 = 2000;
    /// 连上之后超过这么久没有心跳就告警（不是致命错误，只提示）。
    const HB_STALE_MS: u64 = 5000;
    /// 会话读取的轮询周期。**必须是有限的**：收尾条件（`--duration` / Ctrl+C）
    /// 只能在这个循环里被看到，而 agent 可能一条消息都不发（设备已断开）。
    /// 2026-09-23 真机实测的缺陷正是这里——原先用 `BufReader::lines()` 阻塞读，
    /// 于是 `--duration` 在会话存活期间**永不生效**（300 s 的跑到了 445 s 仍在运行）。
    const READ_POLL_MS: u64 = 250;
    /// 注入/接管后等待**已鉴权** hello 的默认上限（秒）。
    /// 为什么设默认：2026-09-23 之前，"注入返回非 0 但 agent 从未连上"这种现象
    /// 只会表现为日志一片安静、程序一直等下去——把"失败"伪装成"正在工作"。
    /// 30 s 足够覆盖 agent 侧的 CONNECT_TIMEOUT(3 s) + 重连周期(1 s)。
    const DEFAULT_AWAIT_HELLO_S: u64 = 30;

    // Win32 控制台事件码（`SetConsoleCtrlHandler` 的 ctrl_type）
    const CTRL_C_EVENT: u32 = 0;
    const CTRL_BREAK_EVENT: u32 = 1;
    const CTRL_CLOSE_EVENT: u32 = 2;
    const CTRL_LOGOFF_EVENT: u32 = 5;
    const CTRL_SHUTDOWN_EVENT: u32 = 6;

    /// 控制台事件（Ctrl+C / 关窗 / 注销 / 关机）置位；由 `SetConsoleCtrlHandler`
    /// 的处理器写入。必须是 `static`：处理器签名固定，拿不到闭包捕获。
    static CTRL_STOP: AtomicBool = AtomicBool::new(false);

    /// 被拒的 hello 计数（令牌不匹配）。只用于把日志折叠成"前几次 + 之后每 30 次一条"。
    static REJECTED_HELLOS: AtomicU64 = AtomicU64::new(0);

    // ============================================================ Win32 FFI

    type Handle = *mut c_void;

    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
        fn CloseHandle(h: Handle) -> i32;
        fn GetLastError() -> u32;
        fn VirtualAllocEx(h: Handle, addr: *mut c_void, size: usize, alloc: u32, protect: u32)
            -> *mut c_void;
        fn VirtualFreeEx(h: Handle, addr: *mut c_void, size: usize, free_type: u32) -> i32;
        fn WriteProcessMemory(
            h: Handle,
            base: *mut c_void,
            buf: *const c_void,
            size: usize,
            written: *mut usize,
        ) -> i32;
        fn CreateRemoteThread(
            h: Handle,
            attr: *mut c_void,
            stack: usize,
            start: *mut c_void,
            param: *mut c_void,
            flags: u32,
            tid: *mut u32,
        ) -> Handle;
        fn WaitForSingleObject(h: Handle, ms: u32) -> u32;
        fn GetExitCodeThread(h: Handle, code: *mut u32) -> i32;
        fn GetModuleHandleW(name: *const u16) -> Handle;
        fn GetProcAddress(module: Handle, name: *const u8) -> *mut c_void;
        fn QueryFullProcessImageNameW(
            h: Handle,
            flags: u32,
            buf: *mut u16,
            size: *mut u32,
        ) -> i32;
        fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> Handle;
        fn Process32FirstW(snapshot: Handle, entry: *mut ProcessEntry32W) -> i32;
        fn Process32NextW(snapshot: Handle, entry: *mut ProcessEntry32W) -> i32;
        /// 模块枚举。用途只有一个：**核实某个 DLL 到底有没有被加载进目标进程**。
        /// 为什么不能只信 `CreateRemoteThread(LoadLibraryW)` 的返回值：模块已存在时
        /// `LoadLibraryW` 返回的是**已加载模块的基址**（非 0），于是"注入成功"与
        /// "本来就加载过、什么都没发生"给出同样的返回值。2026-09-23 的日志里
        /// `loaded=true` 就是这么写出来的，属于**未经核实的断言**。
        fn Module32FirstW(snapshot: Handle, entry: *mut ModuleEntry32W) -> i32;
        fn Module32NextW(snapshot: Handle, entry: *mut ModuleEntry32W) -> i32;
        fn GetLocalTime(out: *mut LocalTime);
        /// 注册控制台事件处理器。第二参数非零 = 追加到链尾。
        /// 返回非零表示注册成功。
        fn SetConsoleCtrlHandler(
            handler: Option<extern "system" fn(u32) -> i32>,
            add: i32,
        ) -> i32;
    }

    #[link(name = "advapi32")]
    extern "system" {
        fn RegOpenKeyExW(
            key: Handle,
            sub: *const u16,
            opts: u32,
            sam: u32,
            out: *mut Handle,
        ) -> i32;
        fn RegEnumKeyExW(
            key: Handle,
            index: u32,
            name: *mut u16,
            name_len: *mut u32,
            reserved: *mut u32,
            class: *mut u16,
            class_len: *mut u32,
            last_write: *mut c_void,
        ) -> i32;
        fn RegQueryValueExW(
            key: Handle,
            name: *const u16,
            reserved: *mut u32,
            ty: *mut u32,
            data: *mut u8,
            len: *mut u32,
        ) -> i32;
        fn RegCloseKey(key: Handle) -> i32;
    }

    #[link(name = "shell32")]
    extern "system" {
        fn IsUserAnAdmin() -> i32;
    }

    const HKEY_LOCAL_MACHINE: Handle = 0x8000_0002u64 as usize as Handle;
    const KEY_READ: u32 = 0x2_0019;
    const ERROR_SUCCESS: i32 = 0;
    const ERROR_NO_MORE_ITEMS: i32 = 259;
    const ERROR_FILE_NOT_FOUND: i32 = 2;
    const ERROR_MORE_DATA: i32 = 234;
    /// 注册表值类型
    const REG_BINARY: u32 = 3;
    const REG_DWORD: u32 = 4;
    const REG_QWORD: u32 = 11;
    /// 单次读取注册表值的最大字节数（HostPid 实际是 8）
    const MAX_VALUE_BYTES: u32 = 64;
    const PROCESS_ALL_ACCESS: u32 = 0x001F_0FFF;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const MEM_COMMIT_RESERVE: u32 = 0x3000;
    const PAGE_READWRITE: u32 = 0x04;
    const MEM_RELEASE: u32 = 0x8000;
    const TH32CS_SNAPPROCESS: u32 = 0x2;
    const TH32CS_SNAPMODULE: u32 = 0x8;
    const TH32CS_SNAPMODULE32: u32 = 0x10;
    /// `ERROR_SHARING_VIOLATION`：文件被别的进程占用（典型：DLL 已 LoadLibrary 进某进程，
    /// 该 image section 会一直锁着文件，直到那个进程退出）。
    const ERROR_SHARING_VIOLATION: i32 = 32;
    /// `WSAEADDRINUSE`：端口已被占用。本程序用它当**单实例锁**（同一时刻只允许一次运行）。
    const WSAEADDRINUSE: i32 = 10048;
    const MAX_PATH_W: usize = 260;
    const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;

    /// 目标三键（HID 键盘页 usage）：返回 / 音量+ / 音量−。
    /// helper 侧也留一份：用于校验哨兵键不与之重叠，并在下发命令里如实汇报"上报集合"。
    /// 必须与 `agent/rc003_agent.js` 的 `TARGET_USAGES` 一致（自检会核对内嵌脚本内容）。
    const TARGET_USAGES: [u16; 3] = [0x00F1, 0x0080, 0x0081];

    #[repr(C)]
    struct ProcessEntry32W {
        dw_size: u32,
        cnt_usage: u32,
        th32_process_id: u32,
        th32_default_heap_id: usize,
        th32_module_id: u32,
        cnt_threads: u32,
        th32_parent_process_id: u32,
        pc_pri_class_base: i32,
        dw_flags: u32,
        sz_exe_file: [u16; 260],
    }

    /// `szModule` 的长度是 **MAX_MODULE_NAME32 + 1 = 256**，不是 MAX_PATH(260)。
    /// 写错会让整个结构体大 8 字节，`Module32FirstW` 直接返回
    /// `ERROR_BAD_LENGTH(24)` 且列表为空——**自检的"阳性对照"一次就抓到了**
    /// （2026-09-23；若只有"查不到 Gadget"的阴性断言，这个 bug 会静默通过，
    /// 于是 [TAP] 永远打印 0，"没有常驻 tap"的结论全是假的）。
    const MAX_MODULE_NAME32_W: usize = 256;

    #[repr(C)]
    struct ModuleEntry32W {
        dw_size: u32,
        th32_module_id: u32,
        th32_process_id: u32,
        glblcnt_usage: u32,
        proccnt_usage: u32,
        mod_base_addr: usize,
        mod_base_size: u32,
        h_module: Handle,
        sz_module: [u16; MAX_MODULE_NAME32_W],
        sz_exe_path: [u16; MAX_PATH_W],
    }

    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn from_wide(buf: &[u16]) -> String {
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..end])
    }

    fn lasts_error() -> u32 {
        unsafe { GetLastError() }
    }

    fn is_elevated() -> bool {
        unsafe { IsUserAnAdmin() != 0 }
    }

    /// 隐藏本进程的控制台窗口（`--follow-app` / `--hide-window` 时调用）。
    ///
    /// 计划任务拉起的是控制台程序，Windows 必然给它分配一个黑框——
    /// 但用户面对的界面是主程序，这个框是纯干扰（2026-09-23 验收反馈）。
    /// 启动后立刻隐藏；日志写文件不依赖窗口，排查能力不受影响。
    /// 手动运行（run-helper.cmd 等）不带 `--follow-app`，保留窗口看日志。
    fn hide_console_window() {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetConsoleWindow() -> Handle;
        }
        #[link(name = "user32")]
        extern "system" {
            fn ShowWindow(window: Handle, command: i32) -> i32;
        }
        const SW_HIDE: i32 = 0;
        unsafe {
            let window = GetConsoleWindow();
            if !window.is_null() {
                ShowWindow(window, SW_HIDE);
            }
        }
    }

    /// 显式启用 **SeDebugPrivilege**——打开 session 0 的 SYSTEM 进程（WUDFHost）必需。
    ///
    /// **为什么必须显式**：UAC 提权只是把 SeDebugPrivilege 放进令牌的特权**列表**，
    /// 它默认处于**禁用**状态；不 `AdjustTokenPrivileges` 启用，`OpenProcess`
    /// 打开 SYSTEM 进程依旧 error 5。源码里原来的假设
    /// （"以管理员身份启动就隐含获得"，见 2026-09-23 计划任务真机验收）
    /// 已经被证伪——这正是「写注释解释系统行为」必须配真机验证的原因。
    ///
    /// Err 的两种形态必须区分开（它们指向完全不同的处置）：
    /// * `not_assigned` ⇒ **令牌里根本没有这个特权** ⇒ 不是"忘了启用"，
    ///   是令牌本身不完整（计划任务 `/rl highest` 未生效的典型表现，
    ///   处置是换 SYSTEM 账户运行任务）；
    /// * 其他错误 ⇒ 正常提权令牌不该发生，原样报出。
    fn enable_se_debug() -> Result<(), String> {
        #[repr(C)]
        #[derive(Clone, Copy, Default)]
        struct Luid {
            low_part: u32,
            high_part: i32,
        }
        #[repr(C)]
        #[derive(Clone, Copy, Default)]
        struct LuidAndAttributes {
            luid: Luid,
            attributes: u32,
        }
        #[repr(C)]
        struct TokenPrivileges {
            privilege_count: u32,
            privileges: [LuidAndAttributes; 1],
        }

        #[link(name = "kernel32")]
        extern "system" {
            fn GetCurrentProcess() -> Handle;
        }
        #[link(name = "advapi32")]
        extern "system" {
            fn OpenProcessToken(process: Handle, desired: u32, token: *mut Handle) -> i32;
            fn LookupPrivilegeValueW(system: *const u16, name: *const u16, luid: *mut Luid) -> i32;
            fn AdjustTokenPrivileges(
                token: Handle,
                disable_all: i32,
                new_state: *const TokenPrivileges,
                buf_len: u32,
                prev_state: *mut TokenPrivileges,
                ret_len: *mut u32,
            ) -> i32;
        }

        const TOKEN_ADJUST_PRIVILEGES: u32 = 0x0020;
        const TOKEN_QUERY: u32 = 0x0008;
        const SE_PRIVILEGE_ENABLED: u32 = 0x0000_0002;
        /// ERROR_NOT_ALL_ASSIGNED：AdjustTokenPrivileges 返回成功，但令牌里没有该特权。
        const ERROR_NOT_ALL_ASSIGNED: u32 = 1300;

        unsafe {
            let mut token: Handle = std::ptr::null_mut();
            if OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
                &mut token,
            ) == 0
            {
                return Err(format!("OpenProcessToken 失败 error={}", GetLastError()));
            }
            let name: Vec<u16> = "SeDebugPrivilege\0".encode_utf16().collect();
            let mut luid = Luid::default();
            if LookupPrivilegeValueW(std::ptr::null(), name.as_ptr(), &mut luid) == 0 {
                return Err(format!("LookupPrivilegeValueW 失败 error={}", GetLastError()));
            }
            let new_state = TokenPrivileges {
                privilege_count: 1,
                privileges: [LuidAndAttributes {
                    luid,
                    attributes: SE_PRIVILEGE_ENABLED,
                }],
            };
            if AdjustTokenPrivileges(
                token,
                0,
                &new_state,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ) == 0
            {
                return Err(format!("AdjustTokenPrivileges 失败 error={}", GetLastError()));
            }
            // 成功 ≠ 生效：令牌缺这个特权时它照样返回"成功"，
            // 真实结果要看 GetLastError == ERROR_NOT_ALL_ASSIGNED。
            if GetLastError() == ERROR_NOT_ALL_ASSIGNED {
                return Err("not_assigned".to_string());
            }
            Ok(())
        }
    }

    /// 用 Toolhelp 快照取进程可执行名 —— 不走 OpenProcess（宿主在 session 0，普通权限会被拒）。
    fn process_name(pid: u32) -> Option<String> {
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap == INVALID_HANDLE_VALUE || snap.is_null() {
                return None;
            }
            let mut entry: ProcessEntry32W = std::mem::zeroed();
            entry.dw_size = std::mem::size_of::<ProcessEntry32W>() as u32;
            let mut ok = Process32FirstW(snap, &mut entry);
            while ok != 0 {
                if entry.th32_process_id == pid {
                    let name = from_wide(&entry.sz_exe_file);
                    CloseHandle(snap);
                    return Some(name);
                }
                ok = Process32NextW(snap, &mut entry);
            }
            CloseHandle(snap);
            None
        }
    }

    /// 目标进程的完整映像路径（需要 PROCESS_QUERY_LIMITED_INFORMATION）。
    fn process_image_path(pid: u32) -> Option<String> {
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return None;
            }
            let mut buf = [0u16; 1024];
            let mut size = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut size);
            CloseHandle(h);
            if ok == 0 {
                return None;
            }
            Some(from_wide(&buf[..size as usize]))
        }
    }

    /// 目标进程已加载的模块。**需要提权**（宿主在 session 0）。
    ///
    /// 这是"注入到底成没成 / 之前那一代还在不在"的**唯一可信判据**：
    /// `LoadLibraryW` 的返回值在"模块已存在"时同样非 0，无法区分。
    fn enum_modules(pid: u32) -> Result<Vec<ModuleInfo>, String> {
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid);
            if snap == INVALID_HANDLE_VALUE || snap.is_null() {
                return Err(format!(
                    "CreateToolhelp32Snapshot(TH32CS_SNAPMODULE, {pid}) 失败，GetLastError={}",
                    lasts_error()
                ));
            }
            let mut entry: ModuleEntry32W = std::mem::zeroed();
            entry.dw_size = std::mem::size_of::<ModuleEntry32W>() as u32;
            let mut out = Vec::new();
            let mut ok = Module32FirstW(snap, &mut entry);
            while ok != 0 {
                out.push(ModuleInfo {
                    name: from_wide(&entry.sz_module),
                    path: from_wide(&entry.sz_exe_path),
                    base: entry.mod_base_addr,
                    size: entry.mod_base_size,
                });
                ok = Module32NextW(snap, &mut entry);
            }
            CloseHandle(snap);
            if out.is_empty() {
                return Err(format!(
                    "模块枚举返回空列表（pid={pid}, GetLastError={}）",
                    lasts_error()
                ));
            }
            Ok(out)
        }
    }

    #[derive(Debug, Clone)]
    struct ModuleInfo {
        name: String,
        path: String,
        base: usize,
        size: u32,
    }

    /// 该模块是否是我们自己注入的 Gadget（按基名判断，兼容将来的分代文件名）。
    fn is_gadget_module(name: &str) -> bool {
        let lower = name.to_ascii_lowercase();
        lower.starts_with("frida-gadget") && lower.ends_with(".dll")
    }

    fn resident_taps(modules: &[ModuleInfo]) -> Vec<ModuleInfo> {
        modules
            .iter()
            .filter(|m| is_gadget_module(&m.name))
            .cloned()
            .collect()
    }

    fn summarize_module(m: &ModuleInfo) -> String {
        format!("{} base=0x{:X} size={}", m.name, m.base, m.size)
    }

    // ============================================================ 注册表定位

    #[derive(Debug)]
    struct HostEntry {
        pid: u32,
        enumerator: String,
        device: String,
        instance: String,
        is_rc003: bool,
    }

    fn reg_subkeys(key: Handle) -> Vec<String> {
        let mut out = Vec::new();
        let mut index = 0u32;
        loop {
            let mut name = [0u16; 512];
            let mut len = name.len() as u32;
            let rc = unsafe {
                RegEnumKeyExW(
                    key,
                    index,
                    name.as_mut_ptr(),
                    &mut len,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if rc == ERROR_NO_MORE_ITEMS {
                break;
            }
            if rc != ERROR_SUCCESS {
                break;
            }
            out.push(from_wide(&name[..len as usize]));
            index += 1;
        }
        out
    }

    fn reg_open(parent: Handle, sub: &str) -> Option<Handle> {
        let path = to_wide(sub);
        let mut key: Handle = std::ptr::null_mut();
        let rc = unsafe { RegOpenKeyExW(parent, path.as_ptr(), 0, KEY_READ, &mut key) };
        if rc == ERROR_SUCCESS {
            Some(key)
        } else {
            None
        }
    }

    // ---- HostPid 读取：类型/宽度感知 ----
    //
    // **实测（2026-09-23，本机）**：6 个 `WUDFDiagnosticInfo\HostPid` 全部是
    // `REG_QWORD`（8 字节），**不是** `REG_DWORD`。此前按 4 字节缓冲区读，
    // 一律得到 `ERROR_MORE_DATA(234)`，被当成"读不到"丢掉 → 实例计数 0 →
    // 又误报成"设备未连接"。真机白跑一轮。
    //
    // 这个坑为什么危险：Python 探针用的是 `winreg.QueryValueEx`，它**按真实类型
    // 自动转换**后返回 `int`，所以同款逻辑在 Python 里取得到值。
    // **探针能跑通 ≠ 4 字节读法可用** —— 参考实现从没编码过"宽度"这个约束。
    // 自写原语必须把宽度契约写进 `--selftest`，否则只能靠真机失败反查。

    /// 读取注册表值的结果。**刻意不用 `Option`**：把"没有值"与"读法不支持"分开，
    /// 否则又会退化成"什么都匹配不到但看起来一切正常"。
    #[derive(Debug)]
    enum PidRead {
        Ok(u32),
        /// 值为 0：该设备的 UMDF 宿主当前未运行
        NoHost,
        /// 键或值不存在（设备刚移除时会出现）
        Missing,
        /// 类型不是 DWORD / QWORD / BINARY
        BadType(u32),
        /// 长度与类型不符，或超出上限
        BadSize(u32),
        /// 其它返回码（原样保留，便于定位）
        Error(i32),
    }

    /// 由原始字节解出 HostPid（纯函数，可自检）。
    fn decode_host_pid(ty: u32, raw: &[u8]) -> Option<u64> {
        fn le(raw: &[u8], n: usize) -> Option<u64> {
            if raw.len() < n {
                return None;
            }
            let mut v = 0u64;
            for (i, b) in raw[..n].iter().enumerate() {
                v |= (*b as u64) << (8 * i);
            }
            Some(v)
        }
        match ty {
            REG_DWORD => le(raw, 4),
            REG_QWORD => le(raw, 8),
            // BINARY：宽度由数据自身决定，只接受 4 / 8
            REG_BINARY => match raw.len() {
                4 => le(raw, 4),
                8 => le(raw, 8),
                _ => None,
            },
            _ => None,
        }
    }

    /// 读 `HostPid`：先问类型与所需字节数，再按**实际宽度**读。
    /// 两步走是必须的——一步到位就必须先猜宽度，而宽度正是这里踩过的坑。
    fn reg_read_host_pid(key: Handle) -> PidRead {
        let name = to_wide("HostPid");
        // 第一步：类型 + 所需字节数（缓冲区传 null 时返回所需长度）
        let mut ty = 0u32;
        let mut need = 0u32;
        let rc = unsafe {
            RegQueryValueExW(
                key,
                name.as_ptr(),
                std::ptr::null_mut(),
                &mut ty,
                std::ptr::null_mut(),
                &mut need,
            )
        };
        // 缓冲为 null 时，部分类型会返回 MORE_DATA 并给出长度，同样可用
        if rc != ERROR_SUCCESS && !(rc == ERROR_MORE_DATA && need > 0) {
            return if rc == ERROR_FILE_NOT_FOUND {
                PidRead::Missing
            } else {
                PidRead::Error(rc)
            };
        }
        if need == 0 || need > MAX_VALUE_BYTES {
            return PidRead::BadSize(need);
        }
        // 第二步：按真实宽度读
        let mut buf = [0u8; MAX_VALUE_BYTES as usize];
        let mut ty2 = 0u32;
        let mut len = need;
        let rc = unsafe {
            RegQueryValueExW(
                key,
                name.as_ptr(),
                std::ptr::null_mut(),
                &mut ty2,
                buf.as_mut_ptr(),
                &mut len,
            )
        };
        if rc != ERROR_SUCCESS {
            return PidRead::Error(rc);
        }
        let len = len.min(MAX_VALUE_BYTES) as usize;
        match decode_host_pid(ty2, &buf[..len]) {
            Some(0) => PidRead::NoHost,
            Some(v) if v <= u32::MAX as u64 => PidRead::Ok(v as u32),
            Some(_) => PidRead::BadSize(len as u32),
            None => PidRead::BadType(ty2),
        }
    }

    /// 一次全量扫描的结果。**带诊断计数**——否则"0 个实例"无法归因，
    /// 上一条误导性的"设备未连接"就是这么来的。
    struct HostScan {
        entries: Vec<HostEntry>,
        /// 成功打开的 `WUDFDiagnosticInfo` 节点数（能否读出 HostPid 都计入）
        diag_keys: usize,
        /// HostPid 为 0（宿主未运行）的节点数
        no_host: usize,
        /// 读取失败的节点（脱敏描述 + 原因）；自检会断言此处为空
        failures: Vec<String>,
    }

    /// 走遍整棵 `Enum` 树，收集所有带 `WUDFDiagnosticInfo\HostPid` 的设备实例。
    /// 必须走全量（而不是只找 RC003）：独占性判据需要知道同一宿主还承载了谁。
    fn enum_hosts() -> Result<HostScan, String> {
        let root = reg_open(HKEY_LOCAL_MACHINE, ENUM_ROOT)
            .ok_or_else(|| format!("无法打开注册表 {ENUM_ROOT}"))?;
        let mut scan = HostScan {
            entries: Vec::new(),
            diag_keys: 0,
            no_host: 0,
            failures: Vec::new(),
        };
        for enumerator in reg_subkeys(root) {
            let Some(enum_key) = reg_open(root, &enumerator) else {
                continue;
            };
            for device in reg_subkeys(enum_key) {
                let Some(device_key) = reg_open(enum_key, &device) else {
                    continue;
                };
                for instance in reg_subkeys(device_key) {
                    let sub = format!("{instance}\\{DIAGNOSTIC_SUFFIX}");
                    let Some(diag) = reg_open(device_key, &sub) else {
                        continue;
                    };
                    scan.diag_keys += 1;
                    let read = reg_read_host_pid(diag);
                    unsafe { RegCloseKey(diag) };
                    let pid = match read {
                        PidRead::Ok(pid) => pid,
                        PidRead::NoHost => {
                            scan.no_host += 1;
                            continue;
                        }
                        PidRead::Missing => continue,
                        other => {
                            // 把失败原因写成可读的话——[REG-WARN] 行是第一手排错线索
                            let why = match &other {
                                PidRead::BadType(ty) => {
                                    format!("值类型 {ty} 不是 DWORD/QWORD/BINARY")
                                }
                                PidRead::BadSize(n) => format!("值长度 {n} 与类型不符"),
                                PidRead::Error(rc) => format!("注册表返回码 {rc}"),
                                _ => format!("{other:?}"),
                            };
                            scan.failures.push(format!(
                                "{enumerator} / {} :: {why}",
                                mask_token(&device)
                            ));
                            continue;
                        }
                    };
                    let folded = device.to_lowercase();
                    scan.entries.push(HostEntry {
                        pid,
                        enumerator: enumerator.clone(),
                        device: device.clone(),
                        instance,
                        is_rc003: enumerator.eq_ignore_ascii_case("bthledevice")
                            && folded.starts_with(&HID_SERVICE_PREFIX.to_lowercase())
                            && folded.contains(RC003_HARDWARE_TOKEN),
                    });
                }
                unsafe { RegCloseKey(device_key) };
            }
            unsafe { RegCloseKey(enum_key) };
        }
        unsafe { RegCloseKey(root) };
        Ok(scan)
    }

    /// 设备实例名里需要脱敏的只有蓝牙地址：紧跟在 `_` 之后的 12 位十六进制。
    fn mask_token(text: &str) -> String {
        let bytes: Vec<char> = text.chars().collect();
        let mut out = String::new();
        let mut i = 0usize;
        while i < bytes.len() {
            if bytes[i] == '_' && i + 13 <= bytes.len() {
                let candidate: String = bytes[i + 1..i + 13].iter().collect();
                let is_hex = candidate.chars().all(|c| c.is_ascii_hexdigit());
                let boundary_ok = match bytes.get(i + 13) {
                    None => true,
                    Some(c) => !c.is_ascii_hexdigit() && *c != '-',
                };
                if is_hex && boundary_ok {
                    out.push_str("_<BT-ADDR>");
                    i += 13;
                    continue;
                }
            }
            out.push(bytes[i]);
            i += 1;
        }
        out
    }

    // ============================================================ 日志

    /// 本机本地时间。只为日志可读性——**不要**用它做任何时序判据
    /// （租约、超时一律用 `Instant` / `now_ms()`）。
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct LocalTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        ms: u16,
    }

    fn local_stamp() -> String {
        let mut t = LocalTime::default();
        unsafe { GetLocalTime(&mut t) };
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
            t.year, t.month, t.day, t.hour, t.minute, t.second, t.ms
        )
    }

    struct Logger {
        path: Option<PathBuf>,
    }

    impl Logger {
        /// 打开日志（追加）。**不写任何分隔线**——分隔线只在 `open_round` 里写。
        ///
        /// 2026-09-23 实测教训：此前分隔线写在 `new` 里，而 `new` 会被**同一进程内的
        /// 第二个 Logger**（续约线程那个）再调用一次，于是每轮都在"武装"横幅前多插一条
        /// `---------- 新一轮运行 ... ----------`。后果是 `grep '新一轮运行'` 的**运行起点
        /// 计数是错的**：真实 6 轮被数成 11 条。而这条分隔线当初加进来的**唯一目的**
        /// 就是让每轮起点可辨——它自己把它破坏了。
        fn new(path: Option<PathBuf>) -> Self {
            if let Some(p) = &path {
                if let Some(dir) = p.parent() {
                    let _ = fs::create_dir_all(dir);
                }
            }
            Self { path }
        }

        /// 开启"新一轮运行"：若日志已有内容，先插一条带本地时间戳的分隔线。
        ///
        /// **只允许在一次运行的最外层调用一次**（`run()` 的入口）。日志是追加语义、
        /// 不截断：上一轮的原始记录是排错时最主要的对照物（2026-09-23 曾有一次
        /// dry-run 把前一轮真机失败的记录抹掉，此后改为追加 + 每轮分隔线）。
        fn open_round(path: Option<PathBuf>) -> Self {
            let logger = Self::new(path);
            if let Some(p) = &logger.path {
                let existed = fs::metadata(p).map(|m| m.len() > 0).unwrap_or(false);
                if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(p) {
                    if existed {
                        let _ = writeln!(
                            f,
                            "\n---------- 新一轮运行 {} ----------",
                            local_stamp()
                        );
                    }
                }
            }
            logger
        }

        fn line(&self, msg: &str) {
            println!("{msg}");
            if let Some(p) = &self.path {
                if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(p) {
                    let _ = writeln!(f, "{msg}");
                }
            }
        }

        fn kv(&self, tag: &str, kv: &[(&str, String)]) {
            let mut s = String::from(tag);
            for (k, v) in kv {
                s.push(' ');
                s.push_str(k);
                s.push('=');
                s.push_str(v);
            }
            self.line(&s);
        }
    }

    // ============================================================ 参数

    struct Args {
        target_pid: Option<u32>,
        port: u16,
        token: String,
        /// 令牌是否来自运行时目录里**此前就存在**的 `session.token`
        /// （决定宿主里那代 tap 有没有可能被我们接管）。
        token_from_file: bool,
        gadget: Option<PathBuf>,
        runtime_dir: PathBuf,
        duration: u64,
        /// 注入/接管后等待**已鉴权** hello 的上限秒数；0 = 不限。
        await_hello: u64,
        /// 哨兵键（canary usage）：除三键外**额外**清空的 usage，仅用于验收。
        ///
        /// 为什么需要它：RC003 的三键在 Windows 侧本来就零事件，所以"清掉"与"不清"
        /// 在外部**完全不可观测**——于是"拦截生效"这个判据在 RC003 上恒为真、没有分辨力。
        /// 清掉一个 Windows 本来可用的键（如主页 `0x4A`），才能让"清空动作真的作用到了
        /// 报告上"变成肉眼可见：开 canary → 按主页无光标移动；关掉/助手死亡 → 恢复。
        /// 默认空 = 不启用；只在启动器显式传 `--canary-usage` 时生效。
        canary_usages: Vec<u16>,
        /// 向主程序转发三键边沿的桥接描述文件路径（`None` = 不转发）。
        ///
        /// 默认 `%LOCALAPPDATA%\SayAll\rc003-bridge.ini`，由主程序写出（端口 + 令牌），
        /// 助手读取后回连。`--no-app-bridge` 关闭；`--app-bridge <PATH>` 指定。
        ///
        /// **为什么默认开启**：不转发时三键只是"被清掉"，映射永远不触发；
        /// 而这正是本机制存在的全部理由。关闭只用于隔离排查。
        app_bridge: Option<PathBuf>,
        /// 安装「让主程序以后能自动拉起助手」的计划任务（**需要提权**，只在首次开启时弹一次 UAC）。
        ///
        /// 与一次捕获运行无关的**管理命令**，在 `run()` 的早期分支截住处理。
        install_task: bool,
        /// 移除上面那个计划任务（需要提权）。卸载/关闭开关时用。
        remove_task: bool,
        /// 只查询计划任务是否存在与是否最高权限（**不需要提权**）。
        task_status: bool,
        /// 跟随主程序：桥接断连超过宽限期后自行退出。
        ///
        /// 配合计划任务使用——主程序需要时 `/run` 触发一次，主程序关掉后助手自己退，
        /// 于是系统里不会长期留着一个提权进程。
        follow_app: bool,
        /// 由主程序经 PowerShell RunAs 提权调用时传入：启动后立刻隐藏
        /// 自己的控制台（提权实例无人看它的输出，黑窗纯属干扰）。
        hide_window: bool,
        dry_run: bool,
        observe: bool,
        restore: bool,
        force: bool,
        /// 要求宿主独占 RC003（旧行为）：共享宿主直接停止。
        /// 默认已允许共享宿主——agent 的内容门禁保证只改写含目标 usage 的报告。
        require_exclusive_host: bool,
        selftest: bool,
        log: Option<PathBuf>,
        new_token: bool,
        attach_only: bool,
        new_generation: bool,
    }

    fn parse_args() -> Result<Args, String> {
        let mut args = Args {
            target_pid: None,
            port: DEFAULT_PORT,
            token: String::new(),
            token_from_file: false,
            gadget: None,
            runtime_dir: default_runtime_dir(),
            duration: 0,
            await_hello: DEFAULT_AWAIT_HELLO_S,
            canary_usages: Vec::new(),
            app_bridge: Some(default_app_bridge_path()),
            install_task: false,
            remove_task: false,
            task_status: false,
            follow_app: false,
            hide_window: false,
            dry_run: false,
            observe: false,
            restore: true,
            force: false,
            require_exclusive_host: false,
            selftest: false,
            log: None,
            new_token: false,
            attach_only: false,
            new_generation: false,
        };

        let mut it = std::env::args().skip(1);
        while let Some(flag) = it.next() {
            let mut value = || it.next().ok_or_else(|| format!("{flag} 缺少取值"));
            match flag.as_str() {
                "--target-pid" => {
                    args.target_pid = Some(
                        value()?
                            .parse::<u32>()
                            .map_err(|_| "--target-pid 需要十进制 PID".to_string())?,
                    )
                }
                "--port" => {
                    args.port = value()?
                        .parse::<u16>()
                        .map_err(|_| "--port 需要 1-65535".to_string())?
                }
                "--token" => args.token = value()?,
                "--gadget" => args.gadget = Some(PathBuf::from(value()?)),
                "--runtime-dir" => args.runtime_dir = PathBuf::from(value()?),
                "--duration" => {
                    args.duration = value()?
                        .parse::<u64>()
                        .map_err(|_| "--duration 需要秒数".to_string())?
                }
                "--await-hello" => {
                    args.await_hello = value()?
                        .parse::<u64>()
                        .map_err(|_| "--await-hello 需要秒数（0=不限）".to_string())?
                }
                "--log" => args.log = Some(PathBuf::from(value()?)),
                "--app-bridge" => args.app_bridge = Some(PathBuf::from(value()?)),
                "--no-app-bridge" => args.app_bridge = None,
                // ---- 任务管理（配合主程序的开关：首次授权一次，之后自动拉起）----
                "--install-task" => args.install_task = true,
                "--remove-task" => args.remove_task = true,
                "--task-status" => args.task_status = true,
                "--follow-app" => args.follow_app = true,
                "--hide-window" => args.hide_window = true,
                "--canary-usage" => {
                    let raw = value()?;
                    let parsed = parse_usage_list(&raw)
                        .map_err(|e| format!("--canary-usage 取值 {raw:?} 无效：{e}"))?;
                    args.canary_usages.extend(parsed);
                }
                "--dry-run" => args.dry_run = true,
                "--observe" => args.observe = true,
                "--no-restore" => args.restore = false,
                "--force" => args.force = true,
                "--require-exclusive-host" => args.require_exclusive_host = true,
                "--selftest" => args.selftest = true,
                "--new-token" => args.new_token = true,
                "--attach-only" => args.attach_only = true,
                "--new-generation" => args.new_generation = true,
                "--help" | "-h" => return Err("HELP".to_string()),
                other => return Err(format!("未知参数 {other}")),
            }
        }

        // 令牌**不在这里生成**：默认值来自运行时目录里的持久文件（跨运行稳定），
        // 那一步需要 Logger，放在 run() 里做。这里只保留显式 --token 的值。
        Ok(args)
    }

    fn default_runtime_dir() -> PathBuf {
        let base = std::env::var("ProgramData").unwrap_or_else(|_| "C:\\ProgramData".to_string());
        PathBuf::from(base).join("SayAll").join("rc003-helper")
    }

    /// 解析 `--canary-usage` 的取值：十六进制，支持 `0x4A` / `4a` / `74`，逗号分隔多个。
    /// 拒绝 0：报告里 0 表示"该槽为空"，不是一个可以按下的键。
    fn parse_usage_list(s: &str) -> Result<Vec<u16>, String> {
        let mut out: Vec<u16> = Vec::new();
        for tok in s.split(',') {
            let t = tok.trim();
            if t.is_empty() {
                continue;
            }
            let hex = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t);
            let v = u16::from_str_radix(hex, 16)
                .map_err(|_| format!("{t:?} 不是十六进制 usage"))?;
            if v == 0 {
                return Err("usage 0 在报告里表示空槽，不能当哨兵键".to_string());
            }
            if !out.contains(&v) {
                out.push(v);
            }
        }
        Ok(out)
    }

    /// 由哨兵键算出**实际下发清空的集合**：三键恒在最前，哨兵追加在后，去重。
    ///
    /// 两条不变量（自检覆盖）：
    ///   1. 三键一定包含在内 —— 否则"清空"根本没清到我们要的键；
    ///   2. 哨兵不得与三键重叠 —— 重叠会让日志无法分辨一条命中是三键还是哨兵。
    fn clear_usages(canary: &[u16]) -> Result<Vec<u16>, String> {
        for c in canary {
            if TARGET_USAGES.contains(c) {
                return Err(format!(
                    "哨兵键 0x{c:04X} 与目标三键重叠；哨兵必须是别的键（RC003 上建议用主页 0x4A）"
                ));
            }
        }
        let mut out: Vec<u16> = TARGET_USAGES.to_vec();
        for c in canary {
            if !out.contains(c) {
                out.push(*c);
            }
        }
        Ok(out)
    }

    /// usage 列表 → 日志用的十六进制串。
    fn usages_hex(us: &[u16]) -> String {
        us.iter()
            .map(|u| format!("0x{u:04x}"))
            .collect::<Vec<_>>()
            .join(",")
    }

    /// usage 列表 → 手写 JSON 数组（十进制，零依赖）。
    fn usages_json(us: &[u16]) -> String {
        us.iter()
            .map(|u| u.to_string())
            .collect::<Vec<_>>()
            .join(",")
    }

    /// 令牌用进程内时间 + 计数器拼出来的伪随机值。
    /// 它不是密码学随机数，但足以阻止"本地任意进程冒充助手/抢占端口"这一威胁模型。
    ///
    /// **2026-09-23 起默认跨运行稳定**（落盘在运行时目录的 `session.token`）。原因是
    /// 一个此前的设计盲点：agent 脚本里的重连循环本来就是为"助手重启但注入未重做"准备的
    /// （见 `rc003_agent.js` 的 `ensureConnected` 注释），可**每次运行都换令牌**时，
    /// 常驻的 agent 用旧令牌 hello 会被助手当冒充者拒掉，于是那条重连路径**永远走不通**——
    /// 用户每次重跑都得重新注入，而重新注入又会被上一代的 DLL 占用挡住。
    /// 想换令牌请显式加 `--new-token`（此时必须同时能把旧 tap 清掉，见 `[STALE-TAP]`）。
    const TOKEN_FILE: &str = "session.token";

    fn random_token() -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let pid = std::process::id() as u128;
        let mix = nanos
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_add(pid.wrapping_mul(0xBF58_476D_1CE4_E5B9));
        format!("{mix:032x}")
    }

    fn usage() -> &'static str {
        "sayall-helper —— RC003 增强捕获轨提权助手（spike）\n\n\
用法：sayall-helper [选项]\n\
  --selftest            内置自检：需无提权、无设备、不注入；全通过退出码 0，否则 1\n\
  --target-pid <PID>    直接指定宿主 PID（自检用，跳过注册表定位）\n\
  --port <PORT>         监听端口（默认 47831；自检时可用 0 自动分配）\n\
  --token <TOKEN>       握手令牌（默认读取/创建运行时目录里的 session.token，跨运行稳定）\n\
  --new-token           强制换一个新令牌（**会让常驻 tap 无法接管**，见下）\n\
  --gadget <PATH>       frida-gadget.dll 路径（默认与可执行文件同目录）\n\
  --runtime-dir <PATH>  运行时目录（默认 %ProgramData%\\SayAll\\rc003-helper）\n\
  --duration <SEC>      运行多少秒后自动收尾（默认 0 = 一直运行）。\n\
                        到点会打 [TIMEUP] 并发 disarm；**会话存活期间同样生效**\n\
                        （2026-09-23 起；此前该上限在有 agent 连接时是失效的）\n\
  --await-hello <SEC>   注入/接管后最多等多久拿到**已鉴权**的 agent hello（默认 30，0=不限）。\n\
                        超时打 [NO-HELLO] 并以退出码 12 结束——宁可早失败，也不要静默干等\n\
  --attach-only         只接管宿主里已有的 tap，绝不注入（诊断用）\n\
  --new-generation      旧世代 tap 无法接管时，另起一份复制体注入（实验性：\n\
                        依赖 Frida 允许同一进程内两个 Gadget 实例，未验证）\n\
  --log <PATH>          同时写日志文件（**追加**语义，每轮带时间戳分隔头）\n\
  --dry-run             只做宿主定位、独占性核对与 Gadget 校验；不准备运行目录、不注入、不监听\n\
                        （**无需提权**：全是只读检查）\n\
  --observe             只观察不拦截（agent 侧不清键）\n\
  --canary-usage <U16>  哨兵键：除三键外**额外**清空的 usage（十六进制，逗号分隔，可重复）。\n\
                        仅用于验收 —— 见下方「为什么需要哨兵键」。默认不启用\n\
  --no-restore          不在 onLeave 回写复原（默认复原）\n\
  --force               宿主非独占 RC003 时也继续（默认即是，见 --require-exclusive-host）\n\
  --require-exclusive-host  恢复旧行为：宿主非独占 RC003 时直接停止\n\
  --app-bridge <PATH>   主程序桥接描述文件（默认 %LOCALAPPDATA%\\SayAll\\rc003-bridge.ini）。\n\
                        助手读取它后回连主程序，把三键边沿转发过去 —— 见下方\n\
                        「三键为什么按下去没反应」。默认启用\n\
  --no-app-bridge       关闭转发：三键仍会被清空，但主程序侧的映射不会触发\n\
  --install-task        【需管理员】注册计划任务 SayAll RC003 Helper（最高权限、仅手动触发）。\n\
                        主程序打开三键开关时装一次，之后用 schtasks /run 拉起，不再弹 UAC\n\
  --remove-task         【需管理员】移除上述计划任务（卸载 / 用户关闭并移除授权时用）\n\
  --task-status         查询计划任务是否存在（免提权）\n\
  --follow-app          跟随主程序：桥接断连超过宽限期后自行退出（计划任务触发时自动带上）\n\
  --hide-window         启动后隐藏自己的控制台（主程序提权安装时自动带上）\n\
为什么需要哨兵键（--canary-usage）\n\
--------------------------------\n\
RC003 的返回/音量± 在 Windows 侧**本来就零事件**（kbdhid 丢弃了这三个 usage）。\n\
于是“清掉它们”与“不清”在外部**完全看不出差别**：音量不会变、也不会有任何字符。\n\
结论：对 RC003 而言，“拦截生效”这个判据**没有分辨力**——它恒为真，无论清空是否\n\
真的作用到了报告上。要证明清空真的生效，必须清一个 Windows **本来能用**的键：\n\
\n\
  --canary-usage 0x4A    # 0x4A = 主页键（RC003 上就是“主页”那颗键）\n\
\n\
开了之后：按主页键**不再有光标移动/首页跳转**（说明清空作用到了报告上）；\n\
助手退出、租约到期、或按 Ctrl+C 之后，主页键**恢复**（说明 fail-open 生效）。\n\
这一条把 E2（清空消除了原生动作）与 E4（fail-open）从“推断”变成“肉眼可见”。\n\
安全边界：只清该 usage 的 2 个字节，report_id/modifiers 一律不碰；\n\
仍受 `--duration` 上限与 agent 侧租约（2000 ms）双重兜底，默认关闭。\n\n\
三键为什么按下去没反应（主程序桥接）\n\
------------------------------------\n\
“清空”只做到“让 Windows 看不到这三个键”，**不等于**“主程序知道了”。主程序侧的\n\
Raw Input 与键盘钩子同样拿不到它们（那正是 kbdhid 丢弃的直接后果）。所以边沿必须由\n\
助手**主动**送过去：主程序在 %LOCALAPPDATA%\\SayAll\\rc003-bridge.ini 里写出“端口 +\n\
令牌”并监听，助手读取后回连、出示令牌，之后把每个边沿按**绝对状态**转发。\n\
\n\
判断这段通没通，看两处：\n\
  * 助手日志 `[APP-BRIDGE] event=connected` / `event=unavailable`；\n\
  * `[SUMMARY]` 的 `app_bridge=connects=N failed=M edges=K`。\n\
    **有 captures 而 edges=0**，就是“键能按、映射不动”的状态。\n\
主程序没运行时本通道每 2 秒重试（日志折叠成前 3 次 + 每 30 次一条），\n\
**不影响捕获与清键**——转发失败只意味着三键“被吃掉了”而没有被重新映射。\n\
\n\
为什么第二次运行不会崩（2026-09-23 修）\n\
-------------------------------------\n\
注入成功的代价是：宿主会**长期映射**运行时目录里的 frida-gadget.dll（实测宿主\n\
`WUDFHost.exe` 可存活十几小时）。此时再 `fs::copy` 覆盖它必然失败\n\
（`os error 32` = ERROR_SHARING_VIOLATION）——这正是「第二次运行直接报错」的根因。\n\
现在启动时会先用模块枚举核对宿主里是否已有 tap：\n\
  * 有 tap 且令牌可接管 → **跳过复制、跳过注入**，直接接管（agent 自己会重连）；\n\
  * 有 tap 但令牌接不上   → 打 [STALE-TAP] 并说明怎么清（**不会**再去撞那个被占用的文件）；\n\
  * 没有 tap             → 正常复制 + 注入；复制前先按 SHA-256 复用已存在的同名文件。\n\n\
结束方式：Ctrl+C 或直接关掉窗口 → 会先给 agent 发 disarm 再退出。\n\
若助手被强杀（任务管理器结束进程），来不及发 disarm —— 此时由 **agent 侧租约**\n\
（默认 2000 ms 无续约即停止清键）兜底，这是有意设计的最后一道防线。\n\n\
建议顺序：`--selftest`（随时可跑）→ `--dry-run`（无需提权，确认能定位宿主）→\n\
管理员身份 `--observe`（零回归风险，确认三键边沿送达）→ 管理员身份正式运行。\n\n\
退出码：0 成功 / 1 自检失败 / 2 参数 / 3 未提权 / 4 找不到 Gadget / 5 宿主非独占 /\n\
        6 Gadget 校验失败 / 7 运行时目录 / 8 端口占用 / 9 注入失败 /\n\
        10 令牌文件 / 11 --attach-only 但没 tap / 12 [NO-HELLO] / 13 [STALE-TAP]。\n"
    }

    // ============================================================ 运行时目录

    /// DLL 放置策略。**决策必须显式**：此前这里是无条件 `fs::copy`，于是"宿主还映射着
    /// 上一代 DLL"这一正常状态被当成了致命错误。
    #[derive(Debug, PartialEq, Eq, Clone)]
    enum DllPlan {
        /// 用运行时目录里的规范文件名（不存在则复制过去）。
        Canonical,
        /// 复制到独立的分代目录（规范文件被旧世代占用时的退路，实验性）。
        Generation(String),
        /// 不碰 DLL（本次是接管常驻 tap，宿主已经映射着它了）。
        Attach,
    }

    /// 纯决策函数（自检覆盖）。`resident_token_ok` 表示"宿主里的 tap 用的是我们手上这个令牌"，
    /// 它是**能否接管**的唯一判据；接不上就只剩"另起一代"或"人工清干净"两条路。
    fn decide_plan(
        has_resident_tap: bool,
        token_reusable: bool,
        attach_only: bool,
        new_generation: bool,
    ) -> Result<DllPlan, String> {
        if has_resident_tap {
            if token_reusable {
                return Ok(DllPlan::Attach);
            }
            if attach_only {
                return Err(
                    "宿主里已有 tap，但令牌接不上（旧世代），而 --attach-only 禁止注入：没有任何可做的事。"
                        .to_string(),
                );
            }
            if new_generation {
                return Ok(DllPlan::Generation(String::new())); // 目录名在调用方补
            }
            return Err("STALE-TAP".to_string());
        }
        if attach_only {
            return Err("--attach-only 要求宿主里已有 tap，但模块枚举没有发现。".to_string());
        }
        Ok(DllPlan::Canonical)
    }

    /// 绑定回环端口。单独抽出来有两个目的：①让失败文案**可操作**；
    /// ②可被自检直接调用（不需要设备、不需要提权）。
    ///
    /// `10048` / `WSAEADDRINUSE` 在本程序里几乎只有一个含义：**已经有另一次运行在跑**
    /// ——端口就是单实例锁。2026-09-23 真机实测撞到过一次：上一轮还在 `--duration`
    /// 窗口内，第二轮双击就得到了 `os error 10048`，而当时那条消息只说了"失败"。
    fn bind_listener(port: u16) -> Result<TcpListener, String> {
        match TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)) {
            Ok(l) => Ok(l),
            Err(e) => {
                let mut s = format!("绑定 127.0.0.1:{port} 失败：{e}");
                if e.raw_os_error() == Some(WSAEADDRINUSE) {
                    s.push_str(&format!(
                        "。这通常意味着**已经有另一次 sayall-helper 在跑**（端口 {port} 即单实例锁）：\
                         先结束上一次运行（它那个窗口里 Ctrl+C 或直接关掉），或用 --port 换端口。\
                         谁在占：netstat -ano | findstr :{port}"
                    ));
                }
                Err(s)
            }
        }
    }

    /// 读取（或创建）跨运行稳定的令牌。
    ///
    /// 返回值第二项 = "磁盘上原本就有" —— 它决定常驻 tap 是否**可能**被我们接管：
    /// 只在本次运行才写下的令牌，不可能是上一代 agent 手里那个。
    fn load_or_create_token(
        dir: &Path,
        force_new: bool,
        logger: &Logger,
    ) -> Result<(String, bool), String> {
        let path = dir.join(TOKEN_FILE);
        if !force_new {
            if let Ok(s) = fs::read_to_string(&path) {
                let t = s.trim().to_string();
                if !t.is_empty() {
                    logger.kv(
                        "[TOKEN]",
                        &[
                            ("source", "file".into()),
                            ("path", normalize_display(&path)),
                            ("value", mask_token(&t)),
                        ],
                    );
                    return Ok((t, true));
                }
            }
        }
        let token = random_token();
        fs::create_dir_all(dir).map_err(|e| format!("创建运行时目录失败 {}: {e}", dir.display()))?;
        fs::write(&path, &token).map_err(|e| format!("写入令牌文件失败 {}: {e}", path.display()))?;
        logger.kv(
            "[TOKEN]",
            &[
                ("source", if force_new { "regenerated".into() } else { "generated".into() }),
                ("path", normalize_display(&path)),
                ("value", mask_token(&token)),
            ],
        );
        Ok((token, false))
    }

    /// 分代目录名。只允许 `[0-9a-f]`（**强制小写**：`--token` 可以是任意字符串，
    /// 大写十六进制在 Windows 路径下不区分大小写，留着只会让"两份目录是不是同一个"
    /// 变成需要推理的问题），避免任何路径歧义。
    fn generation_dir_name(token: &str) -> String {
        let tail: String = token
            .chars()
            .filter(|c| c.is_ascii_hexdigit())
            .map(|c| c.to_ascii_lowercase())
            .take(8)
            .collect();
        format!("gen-{tail}")
    }

    /// 清理旧分代目录（best-effort）。被占用的那个删不掉，这是**预期**而非错误：
    /// 它正被宿主映射着，要等宿主重启才消失。
    fn reap_generations(dir: &Path, keep: Option<&Path>, logger: &Logger) {
        let Ok(rd) = fs::read_dir(dir) else { return };
        let mut removed = 0usize;
        let mut kept = 0usize;
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if !name.starts_with("gen-") || !p.is_dir() {
                continue;
            }
            if Some(p.as_path()) == keep {
                kept += 1;
                continue;
            }
            match fs::remove_dir_all(&p) {
                Ok(_) => removed += 1,
                Err(_) => kept += 1, // 仍被占用：正常，留给下次或宿主重启
            }
        }
        if removed > 0 || kept > 0 {
            logger.kv(
                "[REAP]",
                &[("removed", removed.to_string()), ("kept_in_use", kept.to_string())],
            );
        }
    }

    /// 对运行时目录里那份规范化 DLL 的动作。抽成纯函数是为了**能被自检覆盖**：
    /// 真正决定"会不会去撞那个被宿主锁住的文件"的就是这一行判断。
    #[derive(Debug, PartialEq, Eq, Clone)]
    enum DllAction {
        /// 已存在且 SHA-256 与锁定值一致 → 复用，**一个字节都不写**。
        Reuse,
        /// 不存在或摘要不符 → 覆盖复制（此时若该文件被占用，才是真正的错误）。
        Copy,
    }

    fn dll_action(exists: bool, sha256_matches: bool) -> DllAction {
        if exists && sha256_matches {
            DllAction::Reuse
        } else {
            DllAction::Copy
        }
    }

    /// `--dry-run` 专用：只读地报告"真跑起来会怎么处理运行时目录"，不写任何文件。
    /// 加它的理由很直接：2026-09-23 那次故障，用户在**没有提权**的情况下无法预先知道
    /// "第二次运行必然会失败"，只能等提权跑完看报错。
    fn inspect_runtime_readonly(args: &Args, logger: &Logger) {
        let dll = args.runtime_dir.join("frida-gadget.dll");
        let exists = dll.exists();
        let digest = if exists {
            fs::read(&dll).ok().map(|b| sha256_hex(&b))
        } else {
            None
        };
        let matches = digest.as_deref() == Some(GADGET_SHA256);
        logger.kv(
            "[PREP-SIM]",
            &[
                ("runtime_dir", normalize_display(&args.runtime_dir)),
                ("dll_exists", exists.to_string()),
                ("sha256_matches", matches.to_string()),
                (
                    "would_do",
                    match dll_action(exists, matches) {
                        DllAction::Reuse => "reuse_existing",
                        DllAction::Copy => "copy_over",
                    }
                    .to_string(),
                ),
                (
                    "note",
                    "只报告、不写入；宿主内是否已有 tap 需要枚举目标进程模块（要提权），本模式不查"
                        .to_string(),
                ),
            ],
        );
        let mut gens = Vec::new();
        if let Ok(rd) = fs::read_dir(&args.runtime_dir) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().to_string();
                if n.starts_with("gen-") && e.path().is_dir() {
                    gens.push(n);
                }
            }
        }
        gens.sort();
        logger.kv(
            "[PREP-SIM]",
            &[
                ("generations", if gens.is_empty() { "-".into() } else { gens.join(",") }),
                ("token_file", args.runtime_dir.join(TOKEN_FILE).exists().to_string()),
            ],
        );
    }

    struct PreparedRuntime {
        dll: PathBuf,
        /// DLL 是复用已有文件（未复制）还是新写下去的。接管时无意义。
        reused: bool,
        /// 复用路径必须真的核对过摘要才算数（只看"文件存在"会把被换掉的 DLL 当成校验过的那一份）。
        verified: bool,
    }

    /// `[DLL]` 汇报的字段。抽成纯函数是为了能被自检直接断言。
    ///
    /// **为什么接管轮不能照搬 reused/verified**：2026-09-23 真机接管轮打出来的是
    /// `reused_existing=false sha256_verified=false copied=false`，读起来像"没校验就用了"，
    /// 而事实是那一轮**一个文件字节都没碰**（不复制、不注入，只是连回常驻的那一代）。
    /// 汇报要描述"这一轮做了什么"，不能拿另一条路径的字段来填空。
    fn dll_report_fields(plan: &DllPlan, prepared: &PreparedRuntime) -> Vec<(&'static str, String)> {
        let mut v: Vec<(&'static str, String)> = vec![("dll", normalize_display(&prepared.dll))];
        match plan {
            DllPlan::Attach => {
                v.push(("action", "attach_existing_tap".into()));
                v.push(("touched_files", "false".into()));
                v.push((
                    "note",
                    "这一轮不复制、不注入；DLL 是上一轮落地的那份，无需重新校验".into(),
                ));
            }
            DllPlan::Canonical | DllPlan::Generation(_) => {
                v.push((
                    "action",
                    if prepared.reused { "reuse" } else { "copied" }.into(),
                ));
                v.push(("sha256_verified", prepared.verified.to_string()));
                v.push(("copied", (!prepared.reused).to_string()));
                if let DllPlan::Generation(g) = plan {
                    v.push(("generation", g.clone()));
                }
            }
        }
        v
    }

    /// 准备运行时目录。
    ///
    /// `plan` 由 `decide_plan` 给出，本函数**只执行、不再改变语义**。
    /// 关键行为：复制任何东西之前，先看目标文件是否已经存在且 SHA-256 与锁定值一致——
    /// 一致就复用。这既避免覆盖"宿主正在映射的那一份"，也避免把 23 MB 来回搬。
    fn prepare_runtime(
        args: &Args,
        gadget_src: &Path,
        logger: &Logger,
        plan: &DllPlan,
    ) -> Result<PreparedRuntime, String> {
        let dir = &args.runtime_dir;
        fs::create_dir_all(dir).map_err(|e| format!("创建运行时目录失败 {}: {e}", dir.display()))?;

        let gen_name = match plan {
            DllPlan::Generation(_) => generation_dir_name(&args.token),
            _ => String::new(),
        };
        let (target_dir, dll) = match plan {
            DllPlan::Generation(_) => {
                let gd = dir.join(&gen_name);
                (gd.clone(), gd.join("frida-gadget.dll"))
            }
            _ => (dir.clone(), dir.join("frida-gadget.dll")),
        };

        let mut reused = false;
        let mut verified = false;

        if matches!(plan, DllPlan::Attach) {
            logger.kv(
                "[PREP]",
                &[
                    ("dir", normalize_display(dir)),
                    ("dll", normalize_display(&dll)),
                    ("action", "skip_copy_attaching_existing_tap".into()),
                ],
            );
        } else {
            if matches!(plan, DllPlan::Generation(_)) {
                fs::create_dir_all(&target_dir)
                    .map_err(|e| format!("创建分代目录失败 {}: {e}", target_dir.display()))?;
            }
            // 已经存在且摘要一致 → 复用。**必须真算摘要**：运行时目录是可被替换的，
            // 只看"文件存在"就会把被换掉的 DLL 当成校验过的那一份。
            if dll.exists() {
                let read = fs::read(&dll).ok();
                let digest = read.as_ref().map(|b| sha256_hex(b));
                match dll_action(true, digest.as_deref() == Some(GADGET_SHA256)) {
                    DllAction::Reuse => {
                        reused = true;
                        verified = true;
                        logger.kv(
                            "[PREP]",
                            &[
                                ("dll", normalize_display(&dll)),
                                ("action", "reuse".into()),
                                ("reason", "sha256_matches".into()),
                                ("sha256", GADGET_SHA256.to_string()),
                                (
                                    "note",
                                    "一个字节都没写——被宿主映射着的那份不会被覆盖".to_string(),
                                ),
                            ],
                        );
                    }
                    DllAction::Copy => logger.kv(
                        "[PREP]",
                        &[
                            ("dll", normalize_display(&dll)),
                            ("action", "overwrite".into()),
                            (
                                "reason",
                                match digest {
                                    Some(d) => format!("sha256_mismatch:{d}"),
                                    None => "unreadable".to_string(),
                                },
                            ),
                        ],
                    ),
                }
            }

            if !reused {
                fs::copy(gadget_src, &dll).map_err(|e| {
                    let mut s = format!(
                        "复制 Gadget 失败 {} -> {}: {e}",
                        gadget_src.display(),
                        dll.display()
                    );
                    if e.raw_os_error() == Some(ERROR_SHARING_VIOLATION) {
                        s.push_str(
                            "。err=32 说明该文件正被某个进程映射着（典型：宿主里还有上一代 Gadget）。\
                             本程序已在启动时用模块枚举核对过宿主，走到这里说明是**非预期**的占用；\
                             请用 hardware/RC003/probes/windows-restart-manager-probe.py 查出占用者。",
                        );
                    }
                    s
                })?;
                logger.kv(
                    "[PREP]",
                    &[
                        ("dll", normalize_display(&dll)),
                        ("action", "copied".into()),
                        ("bytes", fs::metadata(&dll).map(|m| m.len()).unwrap_or(0).to_string()),
                    ],
                );
            }
            if let DllPlan::Generation(_) = plan {
                logger.kv(
                    "[PREP]",
                    &[
                        ("generation", gen_name.clone()),
                        ("dir", normalize_display(&target_dir)),
                        ("note", "实验性：同一宿主内的第二个 Gadget 实例".into()),
                    ],
                );
            }
        }

        // agent 脚本与配置总是重写（它们是**下一次**注入读的东西，与当前宿主里
        // 已加载的实例无关；内容以编译期内联的 AGENT_JS 为准）。
        let agent = target_dir.join("rc003_agent.js");
        fs::write(&agent, AGENT_JS).map_err(|e| format!("写入 agent 脚本失败: {e}"))?;

        let config = target_dir.join("frida-gadget.config");
        fs::write(&config, serde_like_config(args))
            .map_err(|e| format!("写入 Gadget 配置失败: {e}"))?;

        logger.kv(
            "[PREP]",
            &[
                ("dir", normalize_display(dir)),
                ("config", normalize_display(&config)),
                ("agent", normalize_display(&agent)),
                ("port", args.port.to_string()),
                ("mode", if args.observe { "observe".into() } else { "clear".into() }),
                ("restore", args.restore.to_string()),
                ("lease_ms", LEASE_MS.to_string()),
            ],
        );

        Ok(PreparedRuntime { dll, reused, verified })
    }

    /// Gadget 配置：`interaction.type = "script"` 会在加载时把脚本跑起来，
    /// 并且**等 `rpc.exports.init()` 的 Promise settle 之后**才放行宿主 entrypoint。
    /// agent 的 init 有 CONNECT_TIMEOUT_MS 兜底，所以助手不在场也不会挂住宿主。
    fn serde_like_config(args: &Args) -> String {
        // 手写 JSON：本 crate 刻意零依赖。
        format!(
            "{{\n  \"interaction\": {{\n    \"type\": \"script\",\n    \"path\": \"rc003_agent.js\",\n    \
             \"parameters\": {{\n      \"port\": {},\n      \"token\": \"{}\",\n      \
             \"mode\": \"{}\",\n      \"restore\": {},\n      \"leaseMs\": {}\n    }}\n  }}\n}}\n",
            args.port,
            args.token,
            if args.observe { "observe" } else { "clear" },
            args.restore,
            LEASE_MS,
        )
    }

    // ============================================================ 注入

    struct Injection {
        thread_handle: Handle,
        remote_buf: *mut c_void,
        process_handle: Handle,
    }

    impl Injection {
        fn cleanup(&mut self) {
            unsafe {
                if !self.remote_buf.is_null() {
                    VirtualFreeEx(self.process_handle, self.remote_buf, 0, MEM_RELEASE);
                    self.remote_buf = std::ptr::null_mut();
                }
                if !self.thread_handle.is_null() {
                    CloseHandle(self.thread_handle);
                    self.thread_handle = std::ptr::null_mut();
                }
                if !self.process_handle.is_null() {
                    CloseHandle(self.process_handle);
                    self.process_handle = std::ptr::null_mut();
                }
            }
        }
    }

    /// 把 Gadget 加载进目标进程。
    ///
    /// 这是参考实现路径的**自持实现**：`SeDebugPrivilege` 由"以管理员身份启动"隐含获得
    /// （本 spike 不主动调整令牌特权），随后 `VirtualAllocEx` + `WriteProcessMemory` +
    /// `CreateRemoteThread(LoadLibraryW)`。不需要 frida 客户端库。
    fn inject_gadget(pid: u32, dll: &Path, logger: &Logger) -> Result<Injection, String> {
        let dll_str = dll
            .to_str()
            .ok_or_else(|| "Gadget 路径不是合法 UTF-8".to_string())?;

        let mut inj = Injection {
            thread_handle: std::ptr::null_mut(),
            remote_buf: std::ptr::null_mut(),
            process_handle: std::ptr::null_mut(),
        };

        unsafe {
            let h = OpenProcess(PROCESS_ALL_ACCESS, 0, pid);
            if h.is_null() {
                return Err(format!(
                    "OpenProcess(PROCESS_ALL_ACCESS, {pid}) 失败，GetLastError={}。\
                     宿主位于 session 0，通常因为未提权。",
                    lasts_error()
                ));
            }
            inj.process_handle = h;

            let wide = to_wide(dll_str);
            let size = wide.len() * std::mem::size_of::<u16>();
            let remote = VirtualAllocEx(
                h,
                std::ptr::null_mut(),
                size,
                MEM_COMMIT_RESERVE,
                PAGE_READWRITE,
            );
            if remote.is_null() {
                let err = lasts_error();
                inj.cleanup();
                return Err(format!("VirtualAllocEx 失败，GetLastError={err}"));
            }
            inj.remote_buf = remote;

            let mut written = 0usize;
            if WriteProcessMemory(
                h,
                remote,
                wide.as_ptr() as *const c_void,
                size,
                &mut written,
            ) == 0
                || written != size
            {
                let err = lasts_error();
                inj.cleanup();
                return Err(format!("WriteProcessMemory 失败，GetLastError={err}"));
            }

            let kernel32 = to_wide("kernel32.dll");
            let module = GetModuleHandleW(kernel32.as_ptr());
            if module.is_null() {
                inj.cleanup();
                return Err("GetModuleHandleW(kernel32.dll) 失败".to_string());
            }
            let load_library = GetProcAddress(module, b"LoadLibraryW\0".as_ptr());
            if load_library.is_null() {
                inj.cleanup();
                return Err("GetProcAddress(LoadLibraryW) 失败".to_string());
            }

            let thread = CreateRemoteThread(
                h,
                std::ptr::null_mut(),
                0,
                load_library,
                remote,
                Default::default(),
                std::ptr::null_mut(),
            );
            if thread.is_null() {
                let err = lasts_error();
                inj.cleanup();
                return Err(format!(
                    "CreateRemoteThread 失败，GetLastError={err}（安全软件拦截注入时常见）"
                ));
            }
            inj.thread_handle = thread;

            let wait = WaitForSingleObject(thread, 15000);
            let mut exit_code: u32 = 0;
            GetExitCodeThread(thread, &mut exit_code);
            logger.kv(
                "[INJECT]",
                &[
                    ("pid", pid.to_string()),
                    ("dll", dll_str.to_string()),
                    ("wait", format!("0x{wait:08X}")),
                    ("hmodule_return", format!("0x{exit_code:08X}")),
                ],
            );
            if exit_code == 0 {
                logger.line(
                    "[WARN] LoadLibraryW 返回 0：Gadget 未加载成功。请检查路径可达性与杀软拦截。",
                );
            }

            // **核验，而不是相信返回值**：模块已存在时 LoadLibraryW 同样返回非 0
            // （返回已加载模块的基址），所以 `hmodule_return != 0` 不能作为"注入了"的证据。
            // 2026-09-23 早先的日志里 `loaded=true` 就是从这个返回值推出来的，未经核实。
            // 判据改为：模块枚举里是否真的出现了这个路径的模块。
            match enum_modules(pid) {
                Ok(modules) => {
                    let want = dll_str.to_ascii_lowercase();
                    let hit = modules.iter().find(|m| m.path.to_ascii_lowercase() == want);
                    let taps = resident_taps(&modules);
                    logger.kv(
                        "[VERIFY-MODULE]",
                        &[
                            ("module_present", hit.is_some().to_string()),
                            (
                                "base",
                                hit.map(|m| format!("0x{:X}", m.base)).unwrap_or_else(|| "-".into()),
                            ),
                            ("gadget_modules_in_host", taps.len().to_string()),
                            (
                                "all",
                                taps.iter().map(summarize_module).collect::<Vec<_>>().join("; "),
                            ),
                        ],
                    );
                    if hit.is_none() {
                        return Err(format!(
                            "注入后核验失败：宿主 {pid} 的模块列表里没有 {dll_str}。\
                             注入未生效（或路径被规范化成了别的形式）。"
                        ));
                    }
                    if taps.len() > 1 {
                        logger.line(&format!(
                            "[WARN] 宿主里现在有 {} 个 Gadget 实例（多世代并存）。\
                             旧实例的令牌接不上，会被助手拒掉；它不会清键（租约早已到期），\
                             但会一直占用内存直到宿主重启。",
                            taps.len()
                        ));
                    }
                }
                Err(e) => {
                    logger.line(&format!(
                        "[WARN] 注入后无法枚举宿主模块（{e}）；\
                         本次无法核实注入是否真的生效，请以是否收到 [HELLO] 为准。"
                    ));
                }
            }
        }
        Ok(inj)
    }

    // ============================================================ 主程序桥接（捕获链第 ② 段）

    /// 桥接协议版本。与主程序 `rc003_bridge::BRIDGE_PROTOCOL_VERSION` 必须一致：
    /// 不一致时主程序会直接拒绝连接——宁可不可用，也不要跑一个"半懂"的协议。
    const BRIDGE_PROTOCOL_VERSION: u32 = 1;
    /// 回连重试间隔。
    const BRIDGE_RETRY_MS: u64 = 2000;
    /// 心跳间隔。主程序侧静默看门狗是 3000 ms，这里留两倍余量。
    const BRIDGE_PING_MS: u64 = 1000;
    /// 主路径读不出来时，跨账户兜底搜索（枚举 `C:\Users\*`）的限频。
    const BRIDGE_FALLBACK_SEARCH_S: u64 = 30;
    /// `--follow-app` 的宽限期：桥接断连超过这么久就认为主程序已经不在了。
    ///
    /// 必须比"主程序重启一次"的耗时更长（debug 版冷启动通常数秒），
    /// 否则主程序只是重开一下，助手却先退了——那会表现为"有时好用有时不好用"。
    const FOLLOW_APP_GRACE_MS: u64 = 20_000;
    /// 单次连接与应答的超时。
    const BRIDGE_IO_TIMEOUT_MS: u64 = 2000;

    /// 主程序桥接描述文件里读出的连接参数。
    struct BridgeTarget {
        port: u16,
        token: String,
    }

    /// 桥接累计统计（供 [SUMMARY] 汇报）。
    #[derive(Default)]
    struct BridgeStats {
        connects: AtomicU64,
        failures: AtomicU64,
        edges_forwarded: AtomicU64,
        last_error: Mutex<String>,
    }

    /// 主程序桥接：把 agent 上报的三键边沿转发给主程序。
    ///
    /// **为什么必须有这一段**：RC003 的返回 / 音量± 在 Windows 侧**零事件**
    /// （`kbdhid` 在 HID→VK 映射阶段丢弃这三个 usage），主程序的 Raw Input 与
    /// 键盘钩子永远看不到它们。助手是唯一拿到边沿的地方——不转发，三键就只是
    /// "被清掉了"，映射永远不触发（表现为"能配置但按下去没反应"）。
    ///
    /// **方向**：主程序监听随机端口并写出描述文件（端口 + 令牌），助手读取后**回连**。
    /// 反向（助手监听、主程序连接）在权限上不成立：助手以管理员身份运行、运行时目录
    /// 在 `%ProgramData%\SayAll\rc003-helper`，普通权限的主程序既读不到那里的写入，
    /// 也不该去猜。而"主程序写在 `%LOCALAPPDATA%`、提权助手去读"没有权限障碍。
    ///
    /// **边沿是绝对状态**：agent 只在集合变化时上报当前按下的 usage 集合，助手原样
    /// 转发。断线期间丢失的变化无需逐条补发——重连后拿最近的绝对状态再发一次即可对齐。
    /// 这是选绝对语义而不是"逐键按下/抬起事件"的主要理由。
    struct AppBridge {
        tx: mpsc::Sender<Vec<u16>>,
        handle: Option<JoinHandle<()>>,
        stop: Arc<AtomicBool>,
        stats: Arc<BridgeStats>,
        /// 最后一次**成功连上主程序**的时刻（epoch ms）。
        ///
        /// `--follow-app` 拿它当"主程序还在不在"的信号：主程序关掉 ⇒ 桥接断掉
        /// ⇒ 这个值不再更新 ⇒ 助手自行退出。比让主程序反过来去杀进程干净得多
        /// （主程序是普通权限，本来就杀不掉一个提权进程）。
        last_connected_ms: Arc<AtomicU64>,
    }

    impl AppBridge {
        fn last_connected_ms(&self) -> u64 {
            self.last_connected_ms.load(Ordering::Relaxed)
        }
        /// 转发一份绝对状态（空 = 全部释放）。
        fn push_edges(&self, usages: Vec<u16>) {
            let _ = self.tx.send(usages);
        }

        fn snapshot(&self) -> (u64, u64, u64, String) {
            (
                self.stats.connects.load(Ordering::Relaxed),
                self.stats.failures.load(Ordering::Relaxed),
                self.stats.edges_forwarded.load(Ordering::Relaxed),
                self.stats
                    .last_error
                    .lock()
                    .map(|guard| guard.clone())
                    .unwrap_or_default(),
            )
        }

        fn shutdown(&self) {
            self.stop.store(true, Ordering::Relaxed);
        }

        fn join(&mut self) {
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    /// 默认桥接描述文件：`%LOCALAPPDATA%\SayAll\rc003-bridge.ini`。
    ///
    /// 与主程序侧 `rc003_bridge::default_bridge_dir()` 必须指向同一位置——
    /// 这条一致性由助手自检里的一项显式核对（避免"主程序写了、助手找不着"的静默失配）。
    fn default_app_bridge_path() -> PathBuf {
        let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| {
            let home = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\".to_string());
            format!("{home}\\AppData\\Local")
        });
        PathBuf::from(base).join("SayAll").join("rc003-bridge.ini")
    }

    /// 主程序请求停止的信号文件（与桥接描述文件同目录）。
    ///
    /// **为什么需要**：助手是提权进程，普通权限的主程序杀不掉它；
    /// `schtasks /end` 只能停"当前任务实例"——若实例已换（重装任务/改名后
    /// 旧进程还挂着），停用就完全落空，旧助手继续映射（2026-09-24 真机复现）。
    /// 文件信号是普通权限也能投递的唯一通道：同用户提权进程可读可删。
    fn app_stop_signal_path() -> PathBuf {
        let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| {
            let home = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\".to_string());
            format!("{home}\\AppData\\Local")
        });
        PathBuf::from(base).join("SayAll").join("rc003-capture-stop")
    }

    /// 信号存在则删除并返回 true（谁见到谁删，避免残留信号误杀下一轮）。
    fn take_stop_signal(path: &Path) -> bool {
        if path.exists() {
            let _ = fs::remove_file(path);
            return true;
        }
        false
    }

    /// 兜底：在 `C:\Users\*\AppData\Local\SayAll\rc003-bridge.ini` 里找**最近修改**的一份。
    ///
    /// **为什么需要**：助手以管理员身份运行。若提权时用的是**另一个账户**的凭据
    /// （而非当前账户的"提升"形态），助手的 `%LOCALAPPDATA%` 就指向那个账户，
    /// 于是读不到当前用户写的描述文件——功能会静默不可用。提权进程读取任意用户目录
    /// 没有权限障碍，因此按**固定相对路径**枚举即可兜住这种情况。
    fn find_app_bridge_elsewhere(logger: &Logger) -> Option<PathBuf> {
        let users_root = Path::new("C:\\Users");
        let entries = fs::read_dir(users_root).ok()?;
        let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
        for entry in entries.flatten() {
            let candidate = entry
                .path()
                .join("AppData")
                .join("Local")
                .join("SayAll")
                .join("rc003-bridge.ini");
            let Ok(metadata) = fs::metadata(&candidate) else {
                continue;
            };
            let Ok(modified) = metadata.modified() else {
                continue;
            };
            if best.as_ref().map(|(t, _)| modified > *t).unwrap_or(true) {
                best = Some((modified, candidate));
            }
        }
        if let Some((_, path)) = &best {
            logger.kv(
                "[APP-BRIDGE]",
                &[
                    ("event", "descriptor_found_elsewhere".into()),
                    ("path", path.display().to_string()),
                    ("note", "默认 %LOCALAPPDATA% 下没有找到；已按 C:\\Users\\* 枚举兜底".into()),
                ],
            );
        }
        best.map(|(_, path)| path)
    }

    /// 解析描述文件（`key=value`）。规则与主程序侧 `rc003_bridge::parse_descriptor` 一致。
    fn parse_bridge_descriptor(text: &str) -> Option<BridgeTarget> {
        let mut port = None;
        let mut token = None;
        let mut version = 0u32;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key.trim() {
                "version" => version = value.trim().parse().unwrap_or(0),
                "port" => port = value.trim().parse().ok(),
                "token" => token = Some(value.trim().to_string()),
                _ => {}
            }
        }
        if version != BRIDGE_PROTOCOL_VERSION {
            return None;
        }
        let port = port?;
        let token = token?;
        if token.is_empty() {
            return None;
        }
        Some(BridgeTarget { port, token })
    }

    /// 桥接描述文件的**只读**探测结论（不连接、不写盘）。
    ///
    /// 用于 `--dry-run`：让"助手能不能发现主程序"这个**路径 / 权限**问题在
    /// **免提权**下先暴露一次。真机排查里最费时间的从来不是协议，而是
    /// "两边都在跑却谁也没看见谁"——这条把那一类问题前移到提权之前。
    fn probe_bridge_descriptor(path: &Path) -> String {
        match fs::read_to_string(path) {
            Ok(text) => match parse_bridge_descriptor(&text) {
                Some(target) => format!(
                    "ok port={} token_len={}",
                    target.port,
                    target.token.len()
                ),
                None => "invalid(版本不符或字段缺失)".to_string(),
            },
            Err(_) => "absent(主程序未运行？)".to_string(),
        }
    }

    fn bridge_write_line(stream: &mut TcpStream, line: &str) -> std::io::Result<()> {
        stream.write_all(line.as_bytes())?;
        stream.write_all(b"\n")?;
        stream.flush()
    }

    /// 边沿 payload 编码：空集合 → `-`（主程序据此释放全部），否则 `f1,80,81`
    /// 形式的十六进制列表。与主程序侧 `parse_bridge_line` 的 `E` 分支互为逆运算。
    fn format_edge_payload(usages: &[u16]) -> String {
        if usages.is_empty() {
            "-".to_string()
        } else {
            usages
                .iter()
                .map(|usage| format!("{usage:02x}"))
                .collect::<Vec<_>>()
                .join(",")
        }
    }

    /// 发一条边沿行。空集合编码为 `-`（主程序侧据此释放全部）。
    fn bridge_send_edges(stream: &mut TcpStream, usages: &[u16]) -> std::io::Result<()> {
        bridge_write_line(
            stream,
            &format!("E {} {}", now_ms(), format_edge_payload(usages)),
        )
    }

    /// 读描述文件 → 连接 → 出示令牌 → 等 `OK`。
    ///
    /// 返回**错误原因字符串**（不是 `io::Error`）：这些原因是给人看的排查线索，
    /// 会被折叠进日志，必须能一眼区分"主程序没开"与"令牌不对"。
    fn bridge_connect(
        path: Option<&Path>,
        logger: &Logger,
    ) -> Result<(TcpStream, PathBuf), String> {
        let path = match path {
            Some(path) => path.to_path_buf(),
            None => return Err("descriptor_path_unknown".to_string()),
        };
        let text = fs::read_to_string(&path).map_err(|_| {
            // 主程序没开时这是**正常现象**，不是故障：如实给出原因即可。
            format!("descriptor_missing({})", path.display())
        })?;
        let target = parse_bridge_descriptor(&text)
            .ok_or_else(|| "descriptor_invalid_or_version_mismatch".to_string())?;
        let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, target.port);
        let mut stream = TcpStream::connect_timeout(
            &std::net::SocketAddr::V4(addr),
            Duration::from_millis(BRIDGE_IO_TIMEOUT_MS),
        )
        .map_err(|error| format!("connect_failed({error})"))?;
        stream.set_nodelay(true).ok();
        stream
            .set_read_timeout(Some(Duration::from_millis(BRIDGE_IO_TIMEOUT_MS)))
            .ok();
        stream
            .set_write_timeout(Some(Duration::from_millis(BRIDGE_IO_TIMEOUT_MS)))
            .ok();
        let hello = format!(
            "HELLO {} {} {}\n",
            BRIDGE_PROTOCOL_VERSION,
            target.token,
            std::process::id()
        );
        stream
            .write_all(hello.as_bytes())
            .map_err(|error| format!("hello_write_failed({error})"))?;
        stream.flush().ok();
        let mut reader = BufReader::new(
            stream
                .try_clone()
                .map_err(|error| format!("clone_failed({error})"))?,
        );
        let mut ack = String::new();
        reader
            .read_line(&mut ack)
            .map_err(|error| format!("ack_timeout({error})"))?;
        if !ack.starts_with("OK") {
            return Err(format!("rejected({})", ack.trim()));
        }
        let _ = logger;
        Ok((stream, path))
    }

    fn app_bridge_worker(
        path: PathBuf,
        rx: mpsc::Receiver<Vec<u16>>,
        stop: Arc<AtomicBool>,
        stats: Arc<BridgeStats>,
        logger: Logger,
        last_connected_ms: Arc<AtomicU64>,
        follow_app: bool,
    ) {
        let mut conn: Option<TcpStream> = None;
        let mut last_known: Vec<u16> = Vec::new();
        let mut last_attempt: Option<Instant> = None;
        let mut last_ping = Instant::now();
        let mut resolved: Option<PathBuf> = Some(path);
        let mut last_fallback_search: Option<Instant> = None;
        let mut failures_logged = 0u64;

        while !stop.load(Ordering::Relaxed) {
            // 0) 停用信号：主程序关开关时写此文件。检测到即整个进程退出——
            //    比起由主程序反杀（它没有权限）或靠 schtasks /end（覆盖不了
            //    "实例已换"的情况），这是唯一对所有情形都成立的停止通道。
            //    只在 --follow-app 下生效：手动调试运行不该被残留信号误杀。
            if follow_app && take_stop_signal(&app_stop_signal_path()) {
                logger.line("[STOP-SIGNAL] 主程序请求停用，助手退出");
                std::process::exit(0);
            }

            // 1) 消化待发消息。用 try_recv 而不是 recv_timeout：断线期间积压的消息
            //    只需要"最后一份绝对状态"，逐条补发没有意义（见 AppBridge 的说明）。
            while let Ok(usages) = rx.try_recv() {
                last_known = usages;
                if let Some(stream) = conn.as_mut() {
                    match bridge_send_edges(stream, &last_known) {
                        Ok(()) => {
                            stats.edges_forwarded.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(_) => conn = None,
                    }
                }
            }

            // 2) 没有连接 → 按间隔重试（每轮都重新读描述文件，端口与令牌可能已变）。
            if conn.is_none() {
                let due = last_attempt
                    .map(|at: Instant| at.elapsed().as_millis() as u64 >= BRIDGE_RETRY_MS)
                    .unwrap_or(true);
                if due {
                    last_attempt = Some(Instant::now());
                    // **不做 exists 预筛。** 早先这里是
                    // `.filter(|p| p.exists()).or_else(兜底搜索)`，后果是：文件不存在时
                    // candidate 直接变成 None，于是日志打出 `descriptor_path_unknown`
                    // —— 读起来像"路径配置错了"，而真实原因往往只是**主程序没在跑**。
                    // 2026-09-23 真机日志上就是这样误导的（reason=descriptor_path_unknown
                    // 与同一行的 note"主程序未运行属正常"自相矛盾）。现在把"文件不存在"
                    // 交给 bridge_connect 用 `descriptor_missing(<完整路径>)` 如实表达：
                    // 原因与路径一起打出来，可以直接拿去核对。
                    // 另外原写法还会每 2 秒枚举一次 C:\Users\*，这里改为限频。
                    let mut candidate = resolved.clone();
                    let primary_unreadable = candidate
                        .as_ref()
                        .map(|path| !path.exists())
                        .unwrap_or(true);
                    if primary_unreadable {
                        let due_search = last_fallback_search
                            .map(|at: Instant| at.elapsed().as_secs() >= BRIDGE_FALLBACK_SEARCH_S)
                            .unwrap_or(true);
                        if due_search {
                            last_fallback_search = Some(Instant::now());
                            if let Some(found) = find_app_bridge_elsewhere(&logger) {
                                resolved = Some(found.clone());
                                candidate = Some(found);
                            }
                        }
                    }
                    match bridge_connect(candidate.as_deref(), &logger) {
                        Ok((stream, location)) => {
                            stats.connects.fetch_add(1, Ordering::Relaxed);
                            last_connected_ms.store(now_ms_u64(), Ordering::Relaxed);
                            resolved = Some(location.clone());
                            logger.kv(
                                "[APP-BRIDGE]",
                                &[
                                    ("event", "connected".into()),
                                    ("descriptor", location.display().to_string()),
                                ],
                            );
                            conn = Some(stream);
                            // 重连后立刻对齐绝对状态：断线期间的变化无从逐条补发，
                            // 但一份绝对状态就能完全对齐（这是绝对语义的价值）。
                            if let Some(stream) = conn.as_mut() {
                                let _ = bridge_send_edges(stream, &last_known);
                            }
                        }
                        Err(reason) => {
                            stats.failures.fetch_add(1, Ordering::Relaxed);
                            if let Ok(mut guard) = stats.last_error.lock() {
                                *guard = reason.clone();
                            }
                            failures_logged += 1;
                            // 折叠成"前 3 次 + 之后每 30 次一条"：主程序没开时
                            // 这条会每 2 秒出现一次，逐条打印会把 [EDGE] 淹掉。
                            if failures_logged <= 3 || failures_logged % 30 == 0 {
                                logger.kv(
                                    "[APP-BRIDGE]",
                                    &[
                                        ("event", "unavailable".into()),
                                        ("reason", reason),
                                        ("tries", failures_logged.to_string()),
                                        (
                                            "note",
                                            "主程序未运行属正常现象；本通道只影响三键映射，不影响捕获与清键"
                                                .into(),
                                        ),
                                    ],
                                );
                            }
                        }
                    }
                }
            }

            // 3) 心跳：主程序侧以 3 s 静默为断线判据，这里 1 s 一次。
            if let Some(stream) = conn.as_mut() {
                if last_ping.elapsed().as_millis() as u64 >= BRIDGE_PING_MS {
                    last_ping = Instant::now();
                    let stamp = now_ms_u64();
                    if bridge_write_line(stream, &format!("P {stamp}")).is_err() {
                        conn = None;
                    } else {
                        // 心跳写成功 = 主程序那一头确实还在，刷新"最后连通时刻"。
                        last_connected_ms.store(stamp, Ordering::Relaxed);
                    }
                }
            }

            std::thread::sleep(Duration::from_millis(50));
        }

        // 收尾：先给释放边沿再告别。顺序不能反——先 BYE 的话，主程序会把它
        // 当成"正常收尾"而不再等待后续释放（虽然它自己有看门狗兜底）。
        if let Some(stream) = conn.as_mut() {
            let _ = bridge_send_edges(stream, &[]);
            let _ = bridge_write_line(stream, "BYE helper_shutdown");
            let _ = stream.shutdown(Shutdown::Both);
        }
    }

    fn spawn_app_bridge(path: Option<PathBuf>, logger: &Logger, follow_app: bool) -> Option<AppBridge> {
        let path = path?;
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stats = Arc::new(BridgeStats::default());
        // 初值 = 进程启动时刻（不是 0）：主程序还没开时也要等满宽限期才退出，
        // 否则一启动就满足"很久没连上"而立刻自杀。
        let last_connected_ms = Arc::new(AtomicU64::new(now_ms_u64()));
        let worker_logger = Logger::new(logger.path.clone());
        let worker_stop = Arc::clone(&stop);
        let worker_stats = Arc::clone(&stats);
        // 初值 = 进程启动时刻，而不是 0：这样"主程序还没开"也要等满宽限期才退出，
        // 否则一启动就满足"很久没连上"而立刻自杀。
        let worker_last = Arc::clone(&last_connected_ms);
        let handle = std::thread::Builder::new()
            .name("rc003-app-bridge".to_owned())
            .spawn(move || {
                app_bridge_worker(
                    path,
                    rx,
                    worker_stop,
                    worker_stats,
                    worker_logger,
                    worker_last,
                    follow_app,
                )
            })
            .ok()?;
        Some(AppBridge {
            tx,
            handle: Some(handle),
            stop,
            stats,
            last_connected_ms,
        })
    }

    // ============================================================ 计划任务（按需提权）

    /// 计划任务名。主程序靠它触发助手，所以**改名必须与主程序侧同步**
    /// （两边各有一份常量，靠自检核对字符串、靠联调核对行为）。
    const SCHEDULED_TASK_NAME: &str = "SayAll RC003 Helper";

    /// `schtasks /create` 的参数。**纯函数**——自检要对它做断言，
    /// 真正的执行在 `manage_scheduled_task`（装/删任务都要提权，不该在自检里发生）。
    ///
    /// 建的是一个**只能手动触发**的任务：`/sc once /st 00:00` 指向一个已经过去的时间，
    /// 于是调度器永远不会自动跑它，只有主程序 `schtasks /run` 时才会启动。
    /// **为什么不用 `/sc daily`**：那会让调度器每天真的拉起一次提权进程，
    /// 而我们要的是"主程序需要时才跑、跑完就退"。
    ///
    /// `/rl highest` 是关键：任务本身以最高权限运行，
    /// 于是**触发它的主程序可以保持普通权限**——这正是"只弹一次 UAC"的实现方式。
    fn task_create_args(helper_exe: &Path) -> Vec<String> {
        vec![
            "/create".to_string(),
            "/tn".to_string(),
            SCHEDULED_TASK_NAME.to_string(),
            "/tr".to_string(),
            format!("\"{}\" --follow-app", helper_exe.display()),
            "/sc".to_string(),
            "once".to_string(),
            "/st".to_string(),
            "00:00".to_string(),
            "/rl".to_string(),
            "highest".to_string(),
            "/f".to_string(),
        ]
    }

    fn task_query_args() -> Vec<String> {
        vec![
            "/query".to_string(),
            "/tn".to_string(),
            SCHEDULED_TASK_NAME.to_string(),
            "/fo".to_string(),
            "LIST".to_string(),
        ]
    }

    fn task_delete_args() -> Vec<String> {
        vec![
            "/delete".to_string(),
            "/tn".to_string(),
            SCHEDULED_TASK_NAME.to_string(),
            "/f".to_string(),
        ]
    }

    /// 三个管理命令：`--task-status`（免提权）/ `--install-task` / `--remove-task`（需提权）。
    fn manage_scheduled_task(args: &Args, logger: &Logger) -> bool {
        // 任务要指向**当前这份 exe 自己**，而不是某次构建留下的路径——
        // 否则以后重编译后，任务还在拉起一份可能已不存在的旧文件。
        let self_exe = match std::env::current_exe() {
            Ok(path) => path,
            Err(error) => {
                logger.line(&format!("[STOP] 取不到本程序路径：{error}"));
                return false;
            }
        };

        if args.task_status {
            return run_task_command(task_query_args(), "查询", logger);
        }

        if args.install_task {
            if !is_elevated() {
                logger.line(
                    "[STOP] --install-task 需要管理员权限：右键「以管理员身份运行」本程序，\
                     或由主程序在打开开关时提权拉起。",
                );
                return false;
            }
            if !run_task_command(task_create_args(&self_exe), "安装", logger) {
                return false;
            }
            logger.kv(
                "[TASK-READY]",
                &[
                    ("task", SCHEDULED_TASK_NAME.to_string()),
                    ("exe", self_exe.display().to_string()),
                    (
                        "note",
                        "已最高权限注册；此后主程序用 `schtasks /run` 触发即可，不再弹 UAC".into(),
                    ),
                ],
            );
            return true;
        }

        // remove_task
        if !is_elevated() {
            logger.line("[STOP] --remove-task 需要管理员权限。");
            return false;
        }
        run_task_command(task_delete_args(), "移除", logger)
    }

    fn run_task_command(cmd_args: Vec<String>, label: &str, logger: &Logger) -> bool {
        match std::process::Command::new("schtasks").args(&cmd_args).output() {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let ok = output.status.success();
                logger.kv(
                    "[TASK]",
                    &[
                        ("action", label.to_string()),
                        ("task", SCHEDULED_TASK_NAME.to_string()),
                        ("ok", ok.to_string()),
                        ("argv", cmd_args.join(" ")),
                        (
                            "detail",
                            if ok {
                                stdout.lines().next().unwrap_or("").trim().to_string()
                            } else {
                                format!(
                                    "{} {}",
                                    stderr.trim(),
                                    stdout.trim()
                                )
                                .trim()
                                .to_string()
                            },
                        ),
                    ],
                );
                ok
            }
            Err(error) => {
                logger.line(&format!("[STOP] 无法执行 schtasks（{label}）：{error}"));
                false
            }
        }
    }

    // ============================================================ 服务

    struct Session {
        connected_at: Instant,
        last_rx: Instant,
        hello: Option<String>,
        edges: Vec<String>,
        lines: u64,
        authenticated: bool,
        /// 鉴权通过后是否已经补发过 `arm` / `mode` / `restore`。
        ///
        /// **为什么必须要发 `arm`**：接管常驻 tap 时，agent 处于上一次运行的收尾状态——
        /// 上一轮结束时助手发过 `disarm`，而 agent 的 `disarmed` 只能由 `arm` 清除
        /// （`leaseOk()` 里 `if (disarmed ...) return false`）。不发 `arm`，
        /// 接管后心跳一切正常、`edges` 却永远不会出现——最容易被误判成"注入没成功"。
        config_sent: bool,
    }

    fn now_ms() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    }

    /// `now_ms()` 的 u64 版本：给 `AtomicU64` 用的。
    ///
    /// 为什么不把 `now_ms()` 本身改成 u64：它已有多处调用按 u128 使用，
    /// 为一个新字段去改它们的类型，牵动面比加一个窄化函数大得多。
    /// epoch ms 到 2106 年才溢出 u64，饱和到 `u64::MAX` 也只是让
    /// "最后连通时刻"看起来极远，不会触发错误分支。
    fn now_ms_u64() -> u64 {
        u64::try_from(now_ms()).unwrap_or(u64::MAX)
    }

    fn handle_line(
        line: &str,
        session: &mut Session,
        logger: &Logger,
        token: &str,
        bridge: Option<&AppBridge>,
    ) -> bool {
        let kind = extract_str(line, "type").unwrap_or_default();
        match kind.as_str() {
            "hello" => {
                let got = extract_str(line, "token").unwrap_or_default();
                session.authenticated = got == token;
                session.hello = Some(line.to_string());
                if !session.authenticated {
                    // 令牌不匹配的连接是**预期现象**，不是攻击：宿主里可能还常驻着
                    // 旧世代的 agent，它每秒重连一次。逐条打印会把 [EDGE] 淹掉，
                    // 所以折叠成计数器（前 3 次 + 之后每 30 次一条）。
                    let n = REJECTED_HELLOS.fetch_add(1, Ordering::Relaxed) + 1;
                    if n <= 3 || n % 30 == 0 {
                        logger.kv(
                            "[REJECT]",
                            &[
                                ("reason", "token_mismatch".into()),
                                ("total", n.to_string()),
                                ("agent_pid", extract_num(line, "pid").unwrap_or(0).to_string()),
                                (
                                    "note",
                                    "可能是本地其它进程冒充，也可能是宿主里旧世代 agent 在重连；两者都不予续约（未鉴权的 agent 永远不会清键）"
                                        .into(),
                                ),
                            ],
                        );
                    }
                    return false;
                }
                logger.kv(
                    "[HELLO]",
                    &[
                        ("auth", "true".into()),
                        ("pid", extract_num(line, "pid").unwrap_or(0).to_string()),
                        ("agent", extract_str(line, "agent").unwrap_or_default()),
                        ("build", extract_str(line, "build").unwrap_or_default()),
                        ("mode", extract_str(line, "mode").unwrap_or_default()),
                        ("restore", extract_bool(line, "restore").to_string()),
                        ("lease_ms", extract_num(line, "lease_ms").unwrap_or(0).to_string()),
                    ],
                );

                // 代次核对：宿主里的实例是上一代脚本时，本轮改的逻辑**不会生效**。
                // 不打这一条，现象是"日志一切正常，但行为还是旧的"——最难查的一类。
                let build = extract_str(line, "build").unwrap_or_default();
                if !build.is_empty() && build != AGENT_BUILD {
                    logger.kv(
                        "[AGENT-STALE]",
                        &[
                            ("running", build),
                            ("embedded", AGENT_BUILD.to_string()),
                            ("host_pid", extract_num(line, "pid").unwrap_or(0).to_string()),
                            (
                                "note",
                                "宿主里跑的是上一代脚本（本轮接管的是旧实例）。\
                                 要让新脚本生效必须重新注入：重启机器最稳；\
                                 或以管理员结束该 WUDFHost 进程后让设备重新枚举。\
                                 注意 --new-generation 另起一代时，助手同一时刻只服务一条连接，\
                                 另一条会因租约过期停清键，不适合做验收判据。"
                                    .into(),
                            ),
                        ],
                    );
                }
            }
            "hb" => {
                if !session.authenticated {
                    return false;
                }
                let stat = extract_substring(line, "\"stat\":").unwrap_or_default();
                logger.kv(
                    "[HB]",
                    &[
                        ("up", format!("{}s", extract_num(line, "up").unwrap_or(0))),
                        ("lease_ok", extract_bool(line, "lease_ok").to_string()),
                        ("handshake", extract_bool(line, "handshake").to_string()),
                        ("disarmed", extract_bool(line, "disarmed").to_string()),
                        ("renew_age_ms", extract_num(line, "since_renew_ms").unwrap_or(0).to_string()),
                        ("stat", stat),
                    ],
                );
            }
            "edge" => {
                if !session.authenticated {
                    return false;
                }
                let buttons = extract_str_array(line, "buttons");
                let usages = extract_num_array(line, "usages");
                let note = if usages.is_empty() {
                    format!("(释放) reason={}", extract_str(line, "reason").unwrap_or_default())
                } else {
                    buttons.join("+")
                };
                let record = format!(
                    "t={} usages={} buttons={} n={}",
                    extract_num(line, "t").unwrap_or(now_ms() as u64),
                    usages
                        .iter()
                        .map(|u| format!("0x{u:04X}"))
                        .collect::<Vec<_>>()
                        .join(","),
                    buttons.join("+"),
                    extract_num(line, "n").unwrap_or(usages.len() as u64),
                );
                logger.kv("[EDGE]", &[("buttons", note), ("raw", record.clone())]);
                session.edges.push(record);
                // 转发给主程序（捕获链第 ② 段）。
                //
                // **必须在鉴权之后**：未鉴权的连接永远不会被续约（因此也不会清键），
                // 但它报上来的边沿如果被转发，就等于任何本地进程都能往映射引擎灌按键。
                // 鉴权是这条转发的唯一门槛，不能省。
                if let Some(bridge) = bridge {
                    let usages: Vec<u16> = usages
                        .iter()
                        .filter_map(|usage| u16::try_from(*usage).ok())
                        .collect();
                    bridge.push_edges(usages);
                }
            }
            "log" => {
                logger.kv(
                    "[AGENT]",
                    &[("msg", extract_str(line, "msg").unwrap_or_default())],
                );
            }
            "bye" => {
                logger.kv("[BYE]", &[("raw", line.to_string())]);
                return false;
            }
            _ => {
                logger.kv("[RX?]", &[("raw", line.to_string())]);
            }
        }
        true
    }

    /// 极简 JSON 取值：本 crate 零依赖，协议字段都是我们自己的 ASCII 字面量，
    /// 因此用一个只认 `"key":value` 的扫描器即可，不需要通用 JSON 解析器。
    fn extract_substring(line: &str, marker: &str) -> Option<String> {
        let start = line.find(marker)? + marker.len();
        let rest = &line[start..];
        // 平衡扫描：从当前位置找到结构结束（对象/数组/字符串/标量）
        let mut depth = 0i32;
        let mut in_str = false;
        let mut escaped = false;
        for (i, c) in rest.char_indices() {
            if in_str {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    in_str = false;
                }
                continue;
            }
            match c {
                '"' => in_str = true,
                '{' | '[' => depth += 1,
                '}' | ']' => {
                    depth -= 1;
                    if depth <= 0 {
                        return Some(rest[..=i].to_string());
                    }
                }
                ',' if depth == 0 => return Some(rest[..i].to_string()),
                _ => {}
            }
        }
        Some(rest.trim().trim_end_matches('}').to_string())
    }

    fn extract_str(line: &str, key: &str) -> Option<String> {
        let marker = format!("\"{key}\":");
        let value = extract_substring(line, &marker)?;
        let value = value.trim();
        if !value.starts_with('"') {
            return None;
        }
        let inner = value.trim_start_matches('"');
        let end = inner.find('"')?;
        Some(inner[..end].to_string())
    }

    fn extract_num(line: &str, key: &str) -> Option<u64> {
        let marker = format!("\"{key}\":");
        let value = extract_substring(line, &marker)?;
        let value = value.trim();
        let digits: String = value
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '-')
            .collect();
        if digits.is_empty() || digits == "-" {
            return None;
        }
        digits.parse::<i64>().ok().map(|v| v.max(0) as u64)
    }

    fn extract_bool(line: &str, key: &str) -> bool {
        let marker = format!("\"{key}\":");
        matches!(extract_substring(line, &marker).map(|v| v.trim().to_string()), Some(v) if v.starts_with("true"))
    }

    fn extract_str_array(line: &str, key: &str) -> Vec<String> {
        let marker = format!("\"{key}\":");
        let Some(value) = extract_substring(line, &marker) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for part in value.trim().trim_start_matches('[').trim_end_matches(']').split(',') {
            let part = part.trim().trim_matches('"');
            if !part.is_empty() {
                out.push(part.to_string());
            }
        }
        out
    }

    fn extract_num_array(line: &str, key: &str) -> Vec<u64> {
        let marker = format!("\"{key}\":");
        let Some(value) = extract_substring(line, &marker) else {
            return Vec::new();
        };
        value
            .trim()
            .trim_start_matches('[')
            .trim_end_matches(']')
            .split(',')
            .filter_map(|p| p.trim().parse::<u64>().ok())
            .collect()
    }

    // ============================================================ 主流程

    pub fn run() {
        let mut args = match parse_args() {
            Ok(a) => a,
            Err(e) if e == "HELP" => {
                println!("{}", usage());
                return;
            }
            Err(e) => {
                eprintln!("参数错误: {e}\n\n{}", usage());
                std::process::exit(2);
            }
        };

        // 计划任务触发的运行**没有启动器**替它传 --log。不给默认日志的话，
        // 一旦出问题（比如连不上主程序、起来就退）就只能靠猜——这正是
        // "日志停在 write_failed 就不再增长"那类事故的形状。
        // 放运行时目录里：提权进程可写，且与令牌/Gadget 同处一个地方。
        // 只对 --follow-app 生效，不改变手动运行"必须显式 --log"的既有约定。
        if args.follow_app && args.log.is_none() {
            let _ = std::fs::create_dir_all(&args.runtime_dir);
            args.log = Some(args.runtime_dir.join("helper-task.log"));
        }
        // 同理：后台拉起不该让用户看到一个黑框。放在最前，把窗口闪现的时间压到最短。
        if args.follow_app || args.hide_window {
            hide_console_window();
        }

        let logger = Logger::open_round(args.log.clone());
        logger.line(&format!(
            "=== sayall-helper（RC003 增强捕获轨 / 产品化 spike）  {} ===",
            local_stamp()
        ));
        logger.kv(
            "[ENV]",
            &[
                ("elevated", is_elevated().to_string()),
                ("pid", std::process::id().to_string()),
                ("port", args.port.to_string()),
                ("runtime_dir", args.runtime_dir.display().to_string()),
                ("target_pid", args.target_pid.map(|p| p.to_string()).unwrap_or_else(|| "-".into())),
                ("gadget_sha256", GADGET_SHA256.to_string()),
                ("agent_sha256", agent_sha256_hex()),
            ],
        );

        if args.selftest {
            let ok = selftest(&logger);
            std::process::exit(if ok { 0 } else { 1 });
        }

        // ---- 任务管理：与一次捕获运行无关，在这里截住 ----
        // 这是"让主程序以后能自动拉起助手"的开关：首次安装弹一次 UAC 就够了，
        // 之后主程序用 `schtasks /run` 触发，不再打扰用户。
        if args.task_status || args.install_task || args.remove_task {
            let ok = manage_scheduled_task(&args, &logger);
            std::process::exit(if ok { 0 } else { 1 });
        }

        // dry-run 承诺「不需要提权」；SeDebug 启用属于真正运行的准备，
        // 且对非提权令牌必然 not_assigned —— 所以放在 dry-run 分支之外。
        if !args.dry_run {
            // ---- SeDebugPrivilege：必须在碰宿主进程之前启用 ----
            // TAP 模块枚举与 OpenProcess 都要它。它默认只是"在特权列表里"，
            // 不显式启用就打不开 session 0 的 SYSTEM 进程——2026-09-23 计划任务
            // 验收里 `scan=failed error=5` 的直接原因之一。
            // 两种失败形态区分开：`not_assigned` 指向令牌不完整（换 SYSTEM 任务），
            // 其他错误才是"启用动作本身出问题"。
            if let Err(reason) = enable_se_debug() {
                if reason == "not_assigned" {
                    logger.line(
                        "[STOP] 当前令牌没有 SeDebugPrivilege（ERROR_NOT_ALL_ASSIGNED）。\
                         这不是普通的“没提权”——UAC 提权令牌应当包含它。\
                         若本次运行来自计划任务，说明 /rl highest 未真正生效，\
                         处置：改用 SYSTEM 账户运行任务（退出码 14）。",
                    );
                } else {
                    logger.line(&format!("[STOP] 启用 SeDebugPrivilege 失败：{reason}"));
                }
                std::process::exit(14);
            }
        }

        // ---- 哨兵键：先校验，再碰任何状态 ----
        // 配置错误一律在**做任何事之前**以退出码 2（参数错误）结束，避免出现
        // "清空范围因为一条畸形参数被改坏"这种最难查的现场。
        if !args.canary_usages.is_empty() {
            match clear_usages(&args.canary_usages) {
                Ok(clear) => {
                    logger.kv(
                        "[CANARY]",
                        &[
                            ("enabled", "true".into()),
                            ("report_usages", usages_hex(&TARGET_USAGES)),
                            ("clear_usages", usages_hex(&clear)),
                            ("canary", usages_hex(&args.canary_usages)),
                        ],
                    );
                    logger.line(
                        "[CANARY] 验收专用配置：除三键外还会清掉上面的哨兵键，它的原生行为会暂时消失。\
                         判据：清空生效 ⇒ 哨兵键无反应；助手退出 / 租约到期 / Ctrl+C ⇒ 哨兵键恢复（fail-open）。",
                    );
                }
                Err(e) => {
                    logger.line(&format!("[STOP] {e}"));
                    std::process::exit(2);
                }
            }
        }

        // `--dry-run` 只做只读检查（注册表定位、独占性核对、Gadget 文件校验），
        // 没有任何一步需要提权 —— 所以不在这里拦。理由很实际：2026-09-23 那次
        // "HostPid 读不出"的 bug 本该在免 UAC 的情况下就暴露，却因为 dry-run
        // 也被提权门挡住，白耗了一轮真机运行。注入模式（observe / 正式）仍然必须提权。
        if !args.dry_run && !is_elevated() {
            logger.line(
                "[STOP] 当前进程不是管理员。宿主位于 session 0，注入必须提权；\
                 请用管理员身份运行（右键「以管理员身份运行」）。",
            );
            std::process::exit(3);
        }

        // ---- 定位宿主 ----
        let (target_pid, entries) = match resolve_target(&args, &logger) {
            Ok(v) => v,
            Err(code) => std::process::exit(code),
        };

        let name = process_name(target_pid).unwrap_or_else(|| "?".to_string());
        let image = process_image_path(target_pid).unwrap_or_else(|| "?".to_string());
        logger.kv(
            "[HOST]",
            &[("pid", target_pid.to_string()), ("exe", name.clone()), ("image", image)],
        );
        if !name.eq_ignore_ascii_case("WUDFHost.exe") {
            logger.line(&format!(
                "[WARN] 目标进程名不是 WUDFHost.exe（实际 {name}）。自检时应显式使用 --target-pid。"
            ));
        }

        // ---- 独占性核对（参考实现的简单绑定路径前提） ----
        if args.target_pid.is_none() {
            let members: Vec<&HostEntry> = entries.iter().filter(|e| e.pid == target_pid).collect();
            let rc003_members = members.iter().filter(|e| e.is_rc003).count();
            logger.kv(
                "[EXCLUSIVE]",
                &[
                    ("members", members.len().to_string()),
                    ("rc003_members", rc003_members.to_string()),
                    (
                        "verdict",
                        if members.len() == 1 && rc003_members == 1 {
                            "exclusive_rc003_host".to_string()
                        } else {
                            "shared_host".to_string()
                        },
                    ),
                ],
            );
            for m in &members {
                logger.line(&format!(
                    "    [{}] {}\\{}\\{}",
                    if m.is_rc003 { "RC003" } else { "其它" },
                    m.enumerator,
                    mask_token(&m.device),
                    mask_token(&m.instance)
                ));
            }
            // 共享宿主**不再直接拒绝**（2026-09-25 产品决策：用户需要长期同时
            // 连接另一个 BLE 键鼠设备）。原来的独占前提只是为了兜住"改写别的
            // 设备报告"的风险；而 agent 的 `targetSetIn` 已经是**内容门禁**：
            // 报告里不含目标 usage（0x00F1/0x0080/0x0081）就一个字节都不动，
            // 普通键盘的按键报告天然不匹配——来源核验由报文内容成立。
            // 残留前提（写进日志供验收核对）：同一宿主里没有**别的**设备会发出
            // 这三个 usage（0xF1 为保留/厂商定义，0x80/0x81 是键盘页音量键，
            // 通用键盘的音量走消费页 0xE9/0xEA）。
            if !(members.len() == 1 && rc003_members == 1) {
                if args.require_exclusive_host || args.force {
                    // 旧行为（--require-exclusive-host；--force 保持同义）：
                    // 共享宿主直接停止，不做任何注入。
                    logger.line(
                        "[STOP] 该宿主并非「独占 RC003」，且已显式要求独占宿主。",
                    );
                    std::process::exit(5);
                }
                logger.kv(
                    "[SHARED-HOST]",
                    &[
                        ("decision", "continue_with_usage_gate".to_string()),
                        ("members", members.len().to_string()),
                        ("rc003_members", rc003_members.to_string()),
                    ],
                );
                logger.line(
                    "[INFO] 宿主同时承载其它 HID 设备：仅改写含目标 usage（F1/80/81）\
                     的键盘报告，其它设备的报告不动；如需恢复旧行为加 --require-exclusive-host。",
                );
            }
        }

        // ---- Gadget 与 agent ----
        let gadget_src = match args.gadget.clone().or_else(default_gadget_path) {
            Some(p) => p,
            None => {
                logger.line(
                    "[STOP] 找不到 frida-gadget.dll。请用 --gadget 指定，或先运行 \
                     vendor/fetch_frida_gadget.py 获取（版本与 SHA-256 见 vendor/frida-gadget.lock.json）。",
                );
                std::process::exit(4);
            }
        };
        logger.kv(
            "[GADGET]",
            &[("src", normalize_display(&gadget_src))],
        );
        if let Err(e) = verify_gadget(&gadget_src, &logger) {
            logger.line(&format!("[STOP] {e}"));
            std::process::exit(6);
        }

        if args.dry_run {
            logger.line(&format!(
                "[DRY-RUN] 宿主定位、独占性核对与 Gadget 校验均通过；未准备运行时目录、未注入、未监听。\
                 elevated={}（本模式不需要提权；observe / 正式运行才需要）。\
                 注意：宿主内是否已有 tap 需要枚举目标进程模块，本模式不提权、**不查**。",
                is_elevated()
            ));
            inspect_runtime_readonly(&args, &logger);
            // 桥接的只读探测：dry-run 的承诺是"不监听、不注入"，这里也不连接——
            // 只回答"主程序写了描述文件没有、写对没有"。真正连通性由 E7 在提权运行下验。
            match args.app_bridge.as_ref() {
                Some(path) => logger.kv(
                    "[DRY-RUN-BRIDGE]",
                    &[
                        ("descriptor", path.display().to_string()),
                        ("probe", probe_bridge_descriptor(path)),
                        (
                            "note",
                            "只探测不连接；absent 表示主程序没在运行（正常现象，不影响捕获与清键）"
                                .into(),
                        ),
                    ],
                ),
                None => logger.kv(
                    "[DRY-RUN-BRIDGE]",
                    &[
                        ("event", "disabled".into()),
                        ("note", "--no-app-bridge：三键边沿不会转发给主程序".into()),
                    ],
                ),
            }
            if !args.canary_usages.is_empty() {
                let clear = clear_usages(&args.canary_usages).unwrap_or_else(|_| TARGET_USAGES.to_vec());
                logger.kv(
                    "[DRY-RUN-CANARY]",
                    &[
                        ("canary", usages_hex(&args.canary_usages)),
                        ("clear_usages", usages_hex(&clear)),
                        ("note", "若开始运行，这些 usage 的原生行为会暂时消失（受租约与 --duration 兜底）".into()),
                    ],
                );
            }
            return;
        }

        // ---- 令牌：默认跨运行稳定（见 TOKEN_FILE 注释） ----
        // 必须**在**探测常驻 tap 之前做：磁盘上本来就有令牌，才谈得上"接管上一代"。
        if args.token.is_empty() {
            match load_or_create_token(&args.runtime_dir, args.new_token, &logger) {
                Ok((t, from_file)) => {
                    args.token = t;
                    args.token_from_file = from_file;
                }
                Err(e) => {
                    logger.line(&format!("[STOP] {e}"));
                    std::process::exit(10);
                }
            }
        } else {
            logger.kv(
                "[TOKEN]",
                &[("source", "explicit".into()), ("value", mask_token(&args.token))],
            );
        }

        // ---- 宿主里是否已有 tap？----
        // 这是"能不能直接接管"的唯一依据，也是"上次注入的 DLL 还锁着运行时文件"的
        // 直接证据。放在注入之前，是为了**不要再去撞那个必然失败的文件覆盖**。
        let taps: Vec<ModuleInfo> = match enum_modules(target_pid) {
            Ok(modules) => {
                let found = resident_taps(&modules);
                logger.kv(
                    "[TAP]",
                    &[
                        ("scan", "ok".into()),
                        ("modules", modules.len().to_string()),
                        ("resident", found.len().to_string()),
                        (
                            "modules_detail",
                            if found.is_empty() {
                                "-".into()
                            } else {
                                found.iter().map(summarize_module).collect::<Vec<_>>().join("; ")
                            },
                        ),
                    ],
                );
                for m in &found {
                    logger.line(&format!("    [TAP] path={}", normalize_display(Path::new(&m.path))));
                }
                // 归属核对：只有"从我们的运行时目录加载起来的"那份才可能是我们注入的。
                // 名字相同但路径在别处 → 别人（或别的工具）也注入了 frida-gadget，明确说出来，
                // 免得把"接不上"归因成自己的 bug。
                let root = args.runtime_dir.to_string_lossy().to_ascii_lowercase();
                for m in &found {
                    if !m.path.to_ascii_lowercase().starts_with(&root) {
                        logger.line(&format!(
                            "[WARN] 宿主里的 Gadget 模块不在我们的运行时目录下（{}），\
                             不是本助手注入的那一份；它的令牌必然对不上。",
                            normalize_display(Path::new(&m.path))
                        ));
                    }
                }
                found
            }
            Err(e) => {
                logger.kv(
                    "[TAP]",
                    &[
                        ("scan", "failed".into()),
                        ("error", e.clone()),
                        (
                            "note",
                            "无法枚举宿主模块；若有上一代 tap，本次注入会是无操作（LoadLibrary 返回已加载基址）"
                                .into(),
                        ),
                    ],
                );
                Vec::new()
            }
        };

        let plan = match decide_plan(
            !taps.is_empty(),
            args.token_from_file,
            args.attach_only,
            args.new_generation,
        ) {
            Ok(p) => p,
            Err(e) if e == "STALE-TAP" => {
                logger.line(
                    "[STALE-TAP] 宿主里已经有我们的 Gadget，但它握着的令牌与磁盘上的 \
                     `session.token` 不一致（或磁盘上本来就没有令牌文件）——说明那是**本机制引入之前**\
                     注入的旧世代，无法接管，也无法覆盖它的 DLL（它正被宿主映射着，err=32）。",
                );
                logger.line(
                    "怎么清（任选其一，都需要提权或人工操作）：\n\
                     \x20 1) 断开并重新配对 RC003（最干净：设备节点重建会带走整个 WUDFHost 进程）；\n\
                     \x20 2) 设备管理器 → 找到 RC003 的 HID 设备 → 禁用再启用；\n\
                     \x20 3) 重启系统。\n\
                     清干净之后可以用探针确认（users=(none) 即为已释放）：\n\
                     \x20 python hardware/RC003/probes/windows-restart-manager-probe.py C:\\ProgramData\\SayAll\\rc003-helper\\frida-gadget.dll\n\
                     本次仍可尝试实验性的并行注入（不保证，依赖 Frida 允许同进程双实例）：加 --new-generation。",
                );
                logger.line(&format!("[STOP] 退出码 13。旧世代 tap: {}", taps.iter().map(summarize_module).collect::<Vec<_>>().join("; ")));
                std::process::exit(13);
            }
            Err(e) => {
                logger.line(&format!("[STOP] {e}"));
                std::process::exit(11);
            }
        };

        // ---- 主程序桥接（捕获链第 ② 段）----
        // 刻意放在端口绑定之前：它只决定"边沿能不能送到主程序"，与"能不能捕获、
        // 能不能清键"完全解耦。主程序没运行时它会安静地每 2 秒重试（日志折叠成
        // 前 3 次 + 每 30 次一条），**不得**因此影响捕获链路。
        let app_bridge = match spawn_app_bridge(args.app_bridge.clone(), &logger, args.follow_app) {
            Some(bridge) => {
                logger.kv(
                    "[APP-BRIDGE]",
                    &[
                        ("event", "enabled".into()),
                        (
                            "descriptor",
                            args.app_bridge
                                .as_ref()
                                .map(|path| path.display().to_string())
                                .unwrap_or_default(),
                        ),
                    ],
                );
                Some(bridge)
            }
            None => {
                logger.kv(
                    "[APP-BRIDGE]",
                    &[
                        ("event", "disabled".into()),
                        (
                            "note",
                            "未启用：三键仍会被清空，但边沿不会转发，主程序侧映射不会触发"
                                .into(),
                        ),
                    ],
                );
                None
            }
        };

        // ---- 顺序：绑定端口 → 准备运行时目录 → 注入 ----
        // 绑定放最前有两条理由：①它是对**外部状态**的检查（端口可能被上一轮占着），
        // 越早失败越好；②准备阶段会**改写**运行时目录里的 config/token/agent，
        // 若先准备再发现端口被占，就等于"失败的一次运行也动了磁盘状态"
        // （2026-09-23 真机实测：第二轮撞端口 10048 时已经写完了配置）。
        // 仍然保持"先监听、后注入"：让 agent 的 init() 一连就能连上，不白等 3 秒。
        let listener = match bind_listener(args.port) {
            Ok(l) => l,
            Err(e) => {
                logger.line(&format!("[STOP] {e}"));
                std::process::exit(8);
            }
        };
        logger.kv("[LISTEN]", &[("addr", format!("127.0.0.1:{}", args.port))]);

        let prepared = match prepare_runtime(&args, &gadget_src, &logger, &plan) {
            Ok(f) => f,
            Err(e) => {
                logger.line(&format!("[STOP] {e}"));
                std::process::exit(7);
            }
        };
        if matches!(plan, DllPlan::Generation(_)) {
            reap_generations(&args.runtime_dir, prepared.dll.parent(), &logger);
        }

        let mut injection: Option<Injection> = match plan {
            DllPlan::Attach => {
                logger.kv(
                    "[ATTACH]",
                    &[
                        ("action", "skip_injection".into()),
                        ("reason", "宿主里已有我们那一代 tap（令牌一致）".into()),
                        ("path", normalize_display(&prepared.dll)),
                        ("note", "agent 会在 RECONNECT_MS 内自己连回来；连上后会补发 arm/mode/restore".into()),
                    ],
                );
                None
            }
            _ => match inject_gadget(target_pid, &prepared.dll, &logger) {
                Ok(i) => Some(i),
                Err(e) => {
                    logger.line(&format!("[STOP] {e}"));
                    std::process::exit(9);
                }
            },
        };
        logger.kv(
            "[DLL]",
            &dll_report_fields(&plan, &prepared),
        );

        // ---- 续约线程：清键许可的唯一来源 ----
        // 先装控制台处理器，让 Ctrl+C / 关窗走"置位 → 主循环收尾 → 发 disarm"，
        // 而不是默认的硬终止（后者会让下面那段收尾代码永远执行不到）。
        install_ctrl_handler(&logger);
        let shared: Arc<Mutex<Option<TcpStream>>> = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let renew_count = Arc::new(AtomicU64::new(0));
        // 续约写入失败 ⇒ 当前连接已不可用。置位后由 `serve_connection` **立刻收尾**，
        // 把主循环还给 accept —— 光清 `shared` 是不够的，理由见续约线程内的说明。
        let conn_stale = Arc::new(AtomicBool::new(false));
        let renew_shared = Arc::clone(&shared);
        let renew_stop = Arc::clone(&stop);
        let renew_counter = Arc::clone(&renew_count);
        let renew_stale = Arc::clone(&conn_stale);
        let renew_token = args.token.clone();
        // 注意：这里是 `new` **不是** `open_round` —— 续约线程不是新一轮运行，
        // 它若写分隔线会让"每轮起点"的 grep 多算一次（2026-09-23 实测）。
        let renew_logger = Logger::new(args.log.clone());
        let renew_handle = std::thread::spawn(move || {
            let mut tick: u64 = 0;
            while !renew_stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(RENEW_MS));
                tick += 1;
                let mut guard = match renew_shared.lock() {
                    Ok(g) => g,
                    Err(_) => break,
                };
                if let Some(stream) = guard.as_mut() {
                    let line = format!("{{\"type\":\"renew\",\"token\":\"{renew_token}\"}}\n");
                    if stream.write_all(line.as_bytes()).is_err() {
                        renew_logger.kv("[RENEW]", &[("state", "write_failed".into())]);
                        *guard = None;
                        // **必须同时置位 `conn_stale`**，否则会进入死亡螺旋：
                        //
                        // 写失败的可能原因不只是"对端已关闭"，还包括**对端不再读**
                        // （写缓冲写满）。后一种情况下 socket 并没有断，
                        // `serve_connection` 的读仍然正常 → 它继续阻塞主循环，
                        // 而主循环是**串行**的（accept → serve 跑完 → 再 accept），
                        // 于是 agent 重连上来的新连接只能排在 backlog 里没人 accept，
                        // 续约也就永远恢复不了：租约过期 → `auth_mismatch` → 断开重连
                        // → 又排不上队 …… 2026-09-23 真机实测的"按了没反应、日志停在
                        // write_failed"就是这个形状。
                        //
                        // 置位后由 `serve_connection` 立刻收尾，把主循环还给 accept，
                        // 新连接一进来 `shared` 就被重建，续约随即恢复。
                        renew_stale.store(true, Ordering::Relaxed);
                    } else {
                        renew_counter.fetch_add(1, Ordering::Relaxed);
                        if tick % 10 == 0 {
                            renew_logger
                                .kv("[RENEW]", &[("tick", tick.to_string()), ("state", "ok".into())]);
                        }
                    }
                }
            }
        });

        logger.line("");
        logger.line("已武装。现在可以按遥控器：返回 / 音量+ / 音量- 都会以 [EDGE] 上报；");
        logger.line("确定 / 主页 / 方向应保持 Windows 原生行为（本 spike 不碰它们）。");
        logger.line(&format!(
            "每 2 秒会打印一次 [HB] 心跳；Ctrl+C 或直接关掉本窗口结束\
             （会先给 agent 发 disarm；即使来不及，agent 也会在 {LEASE_MS} ms 租约到期后自行停止清键）。\
             {}",
            if args.duration > 0 {
                format!("本次运行上限 {} 秒，到点自动收尾。", args.duration)
            } else {
                "本次运行未设上限（--duration 0），不会自动结束。".to_string()
            }
        ));
        logger.line("");

        let started = Instant::now();
        // 进程启动的 epoch 时刻：`--follow-app` 在"从未连上过主程序"时用它做基准，
        // 这样宽限期是从启动开始算的，而不是从 1970 年算（那会立刻退出）。
        let started_ms = now_ms_u64();
        // `--duration` 的绝对到期时刻。用绝对时刻而不是"每轮比较 elapsed"，
        // 是为了让 serve_connection 也能自己判断（否则会话一建立，主循环的检查
        // 就再也不会被执行——2026-09-23 实测到的缺陷）。
        let deadline = if args.duration > 0 {
            Some(started + Duration::from_secs(args.duration))
        } else {
            None
        };
        let mut session = Session {
            connected_at: Instant::now(),
            last_rx: Instant::now(),
            hello: None,
            edges: Vec::new(),
            lines: 0,
            authenticated: false,
            config_sent: false,
        };
        let mut last_hb_warn = Instant::now();
        // 是否曾经接到过**已鉴权**的 agent。注意不能用 `session.authenticated`：
        // 连接可以断掉重来，那个字段会被重置；而"这辈子到底有没有连上过"才是判据。
        let mut saw_authenticated = false;
        let hello_deadline = if args.await_hello > 0 {
            Some(started + Duration::from_secs(args.await_hello))
        } else {
            None
        };
        let mut no_hello = false;

        loop {
            if stop_requested(&stop) {
                break;
            }
            // 跟随主程序：桥接断连超过宽限期 ⇒ 主程序已经关了（或开关被关掉）
            // ⇒ 助手自行退出，不在系统里留下一个提权进程。
            //
            // 用桥接的"最后连通时刻"当信号，而不是让主程序反过来结束我们——
            // 主程序是普通权限，本来也无法结束一个提权进程；而且它若被强杀，
            // 就不会有机会通知任何人，只有这种"对端消失 ⇒ 自退"的写法兜得住。
            if args.follow_app {
                let idle_ms = now_ms_u64().saturating_sub(
                    app_bridge
                        .as_ref()
                        .map(|bridge| bridge.last_connected_ms())
                        .unwrap_or(started_ms),
                );
                if idle_ms > FOLLOW_APP_GRACE_MS {
                    logger.kv(
                        "[FOLLOW-APP]",
                        &[
                            ("event", "exit".into()),
                            ("idle_ms", idle_ms.to_string()),
                            ("grace_ms", FOLLOW_APP_GRACE_MS.to_string()),
                            (
                                "note",
                                "主程序已不在（关闭或开关已关）；助手退出，不留后台进程".into(),
                            ),
                        ],
                    );
                    break;
                }
            }
            if !saw_authenticated {
                if let Some(d) = hello_deadline {
                    if Instant::now() >= d {
                        no_hello = true;
                        break;
                    }
                }
            }
            if let Some(d) = deadline {
                if Instant::now() >= d {
                    logger.line(&format!("[TIMEUP] 已达 --duration {}s，开始收尾。", args.duration));
                    break;
                }
            }

            listener.set_nonblocking(true).ok();
            match listener.accept() {
                Ok((stream, addr)) => {
                    stream.set_nonblocking(false).ok();
                    stream.set_nodelay(true).ok();
                    logger.kv("[ACCEPT]", &[("from", addr.to_string())]);
                    session = Session {
                        connected_at: Instant::now(),
                        last_rx: Instant::now(),
                        hello: None,
                        edges: Vec::new(),
                        lines: 0,
                        authenticated: false,
                        config_sent: false,
                    };
                    if let Ok(mut guard) = shared.lock() {
                        *guard = Some(stream.try_clone().expect("clone stream"));
                    }
                    serve_connection(
                        stream,
                        &mut session,
                        &logger,
                        &args.token,
                        args.observe,
                        args.restore,
                        &args.canary_usages,
                        &stop,
                        deadline,
                        app_bridge.as_ref(),
                        &conn_stale,
                    );
                    if session.authenticated {
                        saw_authenticated = true;
                    }
                    if let Ok(mut guard) = shared.lock() {
                        *guard = None;
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(120));
                }
                Err(e) => {
                    logger.line(&format!("[WARN] accept 失败: {e}"));
                    std::thread::sleep(Duration::from_millis(200));
                }
            }

            if session.last_rx.elapsed().as_millis() as u64 > HB_STALE_MS
                && last_hb_warn.elapsed().as_millis() as u64 > HB_STALE_MS
            {
                logger.kv(
                    "[WARN]",
                    &[("note", "超过 5 秒没有 agent 心跳（设备是否已断开？）".into())],
                );
                last_hb_warn = Instant::now();
            }
        }

        // ---- 收尾 ----
        stop.store(true, Ordering::Relaxed);
        if let Ok(mut guard) = shared.lock() {
            if let Some(stream) = guard.as_mut() {
                let line = format!("{{\"type\":\"disarm\",\"token\":\"{}\"}}\n", args.token);
                let _ = stream.write_all(line.as_bytes());
                let _ = stream.flush();
            }
            *guard = None;
        }
        let _ = renew_handle.join();

        // 桥接收尾：先取统计（shutdown 之后线程会自己收尾并退出），
        // 再等它把释放边沿与 BYE 真正写出去——先 BYE 再释放会让主程序
        // 把这次收尾当成"正常告别"而忽略后续（它有看门狗兜底，但顺序仍然要对）。
        let bridge_stats = app_bridge.as_ref().map(|bridge| bridge.snapshot());
        if let Some(mut bridge) = app_bridge {
            bridge.shutdown();
            bridge.join();
        }

        let counts: BTreeSet<&str> = session
            .edges
            .iter()
            .map(|e| e.split("buttons=").nth(1).unwrap_or("?").split(' ').next().unwrap_or("?"))
            .collect();
        logger.line("");
        logger.kv(
            "[SUMMARY]",
            &[
                ("uptime_s", started.elapsed().as_secs().to_string()),
                ("agent_lines", session.lines.to_string()),
                ("renews_sent", renew_count.load(Ordering::Relaxed).to_string()),
                ("edges", session.edges.len().to_string()),
                ("distinct_buttons", counts.into_iter().collect::<Vec<_>>().join("|")),
                ("authenticated_ever", saw_authenticated.to_string()),
                (
                    "app_bridge",
                    match bridge_stats.as_ref() {
                        // connects>0 才说明第 ② 段真的通了；edges>0 才说明边沿真的过了桥。
                        // 只有 captures 而没有 edges，正是"三键能按但映射不动"的状态。
                        Some((connects, failures, edges, _)) => {
                            format!("connects={connects} failed={failures} edges={edges}")
                        }
                        None => "disabled".to_string(),
                    },
                ),
                (
                    "app_bridge_last_error",
                    bridge_stats
                        .as_ref()
                        .map(|(_, _, _, error)| error.clone())
                        .filter(|error| !error.is_empty())
                        .unwrap_or_else(|| "-".to_string()),
                ),
                (
                    "how",
                    match plan {
                        DllPlan::Attach => "attached_existing_tap",
                        DllPlan::Generation(_) => "injected_new_generation",
                        DllPlan::Canonical => "injected",
                    }
                    .to_string(),
                ),
            ],
        );
        logger.line("[NOTE] 已向 agent 发送 disarm；agent 也会在租约到期后自行停止清键。");

        if let Some(inj) = injection.as_mut() {
            inj.cleanup();
        }

        if no_hello {
            logger.line(&format!(
                "[NO-HELLO] {} 秒内没有收到**已鉴权**的 agent hello。这不是「设备坏了」，\
                 按可能性从高到低排查：\n\
                 \x20 1) 宿主里已有 tap，但它握着一个我们不知道的旧令牌 → 看上面的 [TAP] / [REJECT] 行；\n\
                 \x20 2) 注入后的模块核验失败（看 [VERIFY-MODULE] 是否 module_present=false）；\n\
                 \x20 3) Gadget 加载了但没能读到配置/agent 脚本（看宿主目录里三个文件是否齐全）；\n\
                 \x20 4) 宿主进程被安全软件拦下了 CreateRemoteThread。",
                args.await_hello
            ));
            std::process::exit(12);
        }
    }

    /// 控制台事件处理器：只置位、不做事，把收尾留给主线程（那里才能安全地
    /// 给 agent 发 `disarm`）。
    ///
    /// 为什么必须有：没有它时 Ctrl+C / 关窗会**直接终止进程**，`main()` 里那段
    /// 收尾代码（发 `disarm`）永远到不了——而日志与 `--help` 都写着会发。
    /// 2026-09-23 实测确认：全程没有安装过处理器，那句承诺是空的。
    /// 唯一真正兜住的是 agent 侧租约（租约到期自动停止清键），不是这里。
    extern "system" fn console_ctrl_handler(ctrl_type: u32) -> i32 {
        match ctrl_type {
            CTRL_C_EVENT
            | CTRL_BREAK_EVENT
            | CTRL_CLOSE_EVENT
            | CTRL_LOGOFF_EVENT
            | CTRL_SHUTDOWN_EVENT => {
                CTRL_STOP.store(true, Ordering::Relaxed);
                1 // 已处理：不让系统立刻终止，给主线程留出收尾时间
            }
            _ => 0,
        }
    }

    /// 一次性注册控制台处理器。
    fn install_ctrl_handler(logger: &Logger) {
        let ok = unsafe { SetConsoleCtrlHandler(Some(console_ctrl_handler), 1) } != 0;
        if !ok {
            // 不致命：租约仍能兜住，但收尾会退化成"硬终止"。
            logger.line(
                "[WARN] 注册控制台处理器失败；Ctrl+C / 关窗将直接终止进程，\
                 不会发 disarm（agent 仍会在租约到期后自行停止清键）。",
            );
        }
    }

    /// 收尾判据：显式 stop，或收到了控制台事件。
    #[inline]
    fn stop_requested(stop: &AtomicBool) -> bool {
        stop.load(Ordering::Relaxed) || CTRL_STOP.load(Ordering::Relaxed)
    }

    /// 鉴权通过后立刻补发会话配置。
    ///
    /// 三条都必须发，而且顺序有意义：
    /// - `arm`     清除上一轮收尾留下的 `disarm`。**接管常驻 tap 时这条是决定性的**：
    ///             上一轮助手退出前发过 `disarm`，而 agent 只会被 `arm` 解除（见 `leaseOk()`）。
    ///             漏发的话，心跳、续约、握手全都正常，`[EDGE]` 却永远不出现——
    ///             最容易被误读成"注入失败"。
    /// - `mode`    让"这一轮用 clear 还是 observe"由**助手**决定，而不是 Gadget 加载时那份旧配置。
    /// - `restore` 同上，控制 onLeave 是否回写复原。
    fn send_session_config(
        stream: &mut TcpStream,
        logger: &Logger,
        token: &str,
        observe: bool,
        restore: bool,
        canary: &[u16],
    ) {
        // 清空集合 = 上报集合 ∪ 哨兵键。哨兵键把"清空到底有没有生效"变成外部可观测的实验
        // （详见 `--canary-usage` 的说明）。配置有误时**不下发 targets**，agent 侧维持默认
        // （恒等于三键），避免一条畸形命令把清空范围改坏。
        let clear: Vec<u16> = match clear_usages(canary) {
            Ok(v) => v,
            Err(e) => {
                logger.line(&format!("[WARN] 哨兵键配置无效，本次不下发 targets，agent 侧维持默认三键：{e}"));
                TARGET_USAGES.to_vec()
            }
        };
        let lines = [
            format!("{{\"type\":\"arm\",\"token\":\"{token}\"}}\n"),
            format!(
                "{{\"type\":\"mode\",\"token\":\"{token}\",\"clear\":{}}}\n",
                !observe
            ),
            format!("{{\"type\":\"restore\",\"token\":\"{token}\",\"on\":{restore}}}\n"),
            format!(
                "{{\"type\":\"targets\",\"token\":\"{token}\",\"report\":[{}],\"clear\":[{}]}}\n",
                usages_json(&TARGET_USAGES),
                usages_json(&clear)
            ),
        ];
        // 逐条记录**实际写出去没有**。协议里没有 ack，所以这些字段只能说"已写出"，
        // 不能说"已生效"——命名如实为 `*_sent`。
        //
        // 2026-09-23 真机实测暴露的问题：此前只要整体 ok，`arm` 一律打印 `true`，
        // 于是接管轮出现过 `sent=false arm=true` 这种自相矛盾的一行；实际情况是
        // 那条连接的 arm 根本没送到（socket 已被对端重置）。
        let mut sent = [false; 4];
        let mut stopped = false;
        for (i, l) in lines.iter().enumerate() {
            if stopped || stream.write_all(l.as_bytes()).is_err() {
                stopped = true;
                continue;
            }
            sent[i] = true;
        }
        let flush_ok = stream.flush().is_ok();
        logger.kv(
            "[CONFIG]",
            &[
                ("arm_sent", sent[0].to_string()),
                ("mode_sent", sent[1].to_string()),
                ("restore_sent", sent[2].to_string()),
                ("targets_sent", sent[3].to_string()),
                ("flush", flush_ok.to_string()),
                ("mode", if observe { "observe".into() } else { "clear".into() }),
                ("restore", restore.to_string()),
                ("report_usages", usages_hex(&TARGET_USAGES)),
                ("clear_usages", usages_hex(&clear)),
                (
                    "canary",
                    if canary.is_empty() {
                        "none".into()
                    } else {
                        usages_hex(canary)
                    },
                ),
                ("ack", "n/a（本协议无 ack）".into()),
            ],
        );
    }

    /// 服务一条 agent 连接。
    ///
    /// `deadline` 是 `--duration` 的绝对到期时刻（`None` = 不限时）。到期即返回，
    /// 由主循环打印 `[TIMEUP]` 并走统一收尾——收尾逻辑只留一处。
    ///
    /// **读取为什么是轮询而不是 `BufReader::lines()`**：后者会一直阻塞到有数据，
    /// 而 agent 可能一条都不发（设备已拔出、或宿主卡住）。那样 `--duration` 与
    /// Ctrl+C 在这个循环里就永远看不到，等于安全上限失效。所以给 socket 设读超时，
    /// 超时视为"暂无数据"继续循环，每 `READ_POLL_MS` 醒一次检查收尾条件。
    /// 代价：不再用 `BufReader` 的行缓冲——好处是没有提前读走数据的问题，
    /// 字节全部留在本函数自己的 `buf` 里。
    fn serve_connection(
        mut stream: TcpStream,
        session: &mut Session,
        logger: &Logger,
        token: &str,
        observe: bool,
        restore: bool,
        canary: &[u16],
        stop: &Arc<AtomicBool>,
        deadline: Option<Instant>,
        bridge: Option<&AppBridge>,
        conn_stale: &Arc<AtomicBool>,
    ) {
        if stream
            .set_read_timeout(Some(Duration::from_millis(READ_POLL_MS)))
            .is_err()
        {
            logger.line("[WARN] 设置读超时失败；收尾检查可能不及时。");
        }

        let mut buf: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            if stop_requested(stop) {
                return;
            }
            // 续约线程报告本条连接已不可用（写失败）→ **立刻收尾**，把主循环还给
            // accept。不能只清 `shared` 就走：那样主循环仍阻塞在这里，
            // agent 重连的新连接进不了 accept，续约永远恢复不了（死亡螺旋）。
            if conn_stale.swap(false, Ordering::Relaxed) {
                logger.line(
                    "[CONN-STALE] 续约写入失败 → 本条连接已不可用，立即收尾以便接受 agent 的新连接。",
                );
                return;
            }
            if let Some(d) = deadline {
                if Instant::now() >= d {
                    return; // 主循环会打 [TIMEUP] 并收尾
                }
            }

            match stream.read(&mut chunk) {
                Ok(0) => break, // 对端关闭
                Ok(n) => {
                    buf.extend_from_slice(&chunk[..n]);
                    while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                        let raw: Vec<u8> = buf.drain(..=pos).collect();
                        let text = String::from_utf8_lossy(&raw[..raw.len().saturating_sub(1)]);
                        let line = text.trim_end_matches('\r');
                        if line.trim().is_empty() {
                            continue;
                        }
                        session.lines += 1;
                        session.last_rx = Instant::now();
                        if !handle_line(line, session, logger, token, bridge) {
                            logger.line("[INFO] 连接结束（令牌不符或 agent 主动 bye）。");
                            return;
                        }
                        if session.authenticated && !session.config_sent {
                            session.config_sent = true;
                            send_session_config(&mut stream, logger, token, observe, restore, canary);
                        }
                    }
                }
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    continue
                }
                Err(e) => {
                    logger.line(&format!("[WARN] 读取连接失败: {e}"));
                    break;
                }
            }
        }
        logger.kv(
            "[DISCONNECT]",
            &[
                ("alive_s", session.connected_at.elapsed().as_secs().to_string()),
                ("agent", extract_str(session.hello.as_deref().unwrap_or(""), "agent").unwrap_or_default()),
                ("agent_pid", extract_num(session.hello.as_deref().unwrap_or(""), "pid").unwrap_or(0).to_string()),
                ("lines", session.lines.to_string()),
                ("edges", session.edges.len().to_string()),
            ],
        );
    }

    fn resolve_target(args: &Args, logger: &Logger) -> Result<(u32, Vec<HostEntry>), i32> {
        let scan = match enum_hosts() {
            Ok(s) => s,
            Err(e) => {
                logger.line(&format!("[STOP] {e}"));
                return Err(10);
            }
        };
        let entries = scan.entries;
        logger.kv(
            "[REG]",
            &[
                ("diag_keys", scan.diag_keys.to_string()),
                ("instances_with_hostpid", entries.len().to_string()),
                ("no_host", scan.no_host.to_string()),
                ("read_failures", scan.failures.len().to_string()),
                ("rc003_instances", entries.iter().filter(|e| e.is_rc003).count().to_string()),
            ],
        );
        for f in &scan.failures {
            logger.kv("[REG-WARN]", &[("unreadable", f.clone())]);
        }

        if let Some(pid) = args.target_pid {
            logger.kv("[REG]", &[("note", "--target-pid 指定，跳过 RC003 定位".into())]);
            return Ok((pid, entries));
        }

        let rc003: Vec<&HostEntry> = entries.iter().filter(|e| e.is_rc003).collect();
        if rc003.is_empty() {
            // 分情形归因。**不要**笼统写"设备未连接"——上一版正是在这里把
            // "HostPid 读取宽度写错" 误报成 "设备未连接"，白白消耗一轮真机运行。
            // 错误信息不许断言未经检验的原因。
            if scan.diag_keys == 0 {
                logger.line(
                    "[STOP] 本机 Enum 树里没有任何 WUDF 诊断节点（无 WUDFDiagnosticInfo）。\
                     这台机器上没有 UMDF 承载的设备，或注册表枚举被拦截。",
                );
            } else if entries.is_empty() {
                logger.line(
                    "[STOP] 找到了 WUDF 诊断节点，但 HostPid 一个都没能读出——\
                     这是**本程序的问题**，不是设备问题。请跑 --selftest 并反馈 [REG-WARN] 行。",
                );
            } else {
                logger.line(
                    "[STOP] 读到 WUDF 宿主，但都不是承载 RC003 的实例。\
                     RC003 未连接/未配对时即如此——请先连接遥控器后重跑。",
                );
            }
            return Err(11);
        }
        let pids: BTreeSet<u32> = rc003.iter().map(|e| e.pid).collect();
        if pids.len() > 1 {
            logger.kv(
                "[STOP]",
                &[("note", format!("定位到多个候选宿主 {pids:?}，请用 --target-pid 指定"))],
            );
            return Err(12);
        }
        Ok((*pids.iter().next().unwrap(), entries))
    }

    /// 装置自检：不需要提权、不需要设备、不注入任何进程。
    ///
    /// 为什么必须有：本 crate 有多处"悄悄错却不会报错"的地方——自己实现的 SHA-256、
    /// 与 Python 探针同款语义的 `mask_token`、手写的极简 JSON 取值器，以及
    /// **注册表 HostPid 的读取宽度**。前三者出错的现象是"看起来正常但什么都没匹配到"；
    /// 第四者 2026-09-23 在真机上真的发生了（按 4 字节读 QWORD → 0 个实例 →
    /// 误报"设备未连接"），所以它的宽度契约必须留在这里做回归。
    fn selftest(logger: &Logger) -> bool {
        let mut all_ok = true;
        let mut check = |label: &str, ok: bool, detail: String| {
            all_ok = all_ok && ok;
            logger.line(&format!("[{}] {label}", if ok { "PASS" } else { "FAIL" }));
            logger.line(&format!("       {detail}"));
        };

        logger.line("=== sayall-helper 自检（无需提权 / 无需设备 / 不注入）===");
        logger.line("");

        // 1) SHA-256 已知向量
        let vectors: [(&str, &str); 3] = [
            (
                "",
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            ),
            (
                "abc",
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            ),
            (
                "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
                "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
            ),
        ];
        for (input, expected) in vectors {
            let got = sha256_hex(input.as_bytes());
            check(
                &format!("SHA-256 向量 {:?}", if input.is_empty() { "<empty>" } else { input }),
                got == expected,
                format!("{got}（期望 {expected}）"),
            );
        }

        // 2) mask_token：只脱敏紧跟在 `_` 之后的 12 位十六进制
        //    注意：设备实例名里的厂商段是大写（`PID&32b8`），脱敏后必须原样保留，
        //    断言须按实际大小写写，否则会误报（此前的 FAIL 就是这个测试自身的 bug）。
        let device = "{00001812-0000-1000-8000-00805f9b34fb}_Dev_VID&012717_PID&32b8_REV&00a4_A1B2C3D4E5F6";
        let masked = mask_token(device);
        check(
            "mask_token 只打掉蓝牙地址",
            masked == "{00001812-0000-1000-8000-00805f9b34fb}_Dev_VID&012717_PID&32b8_REV&00a4_<BT-ADDR>"
                && masked.ends_with("_<BT-ADDR>")
                && masked.contains("PID&32b8")
                && masked.contains("00805f9b34fb")
                && !masked.contains("A1B2C3D4E5F6"),
            masked.clone(),
        );

        // 3) 极简 JSON 取值器：协议实际会遇到的几类形态
        let hb = "{\"type\":\"hb\",\"t\":1758600000000,\"up\":12,\"lease_ok\":true,\
                  \"handshake\":true,\"disarmed\":false,\"since_renew_ms\":-1,\
                  \"stat\":{\"clears_ok\":7,\"write_fail\":0}}";
        let edge = "{\"type\":\"edge\",\"t\":1,\"usages\":[241,128],\
                     \"buttons\":[\"back\",\"volume_up\"],\"n\":2}";
        let rel = "{\"type\":\"edge\",\"t\":2,\"usages\":[],\"released_all\":true,\"reason\":\"lease_expired\"}";

        let cases: [(&str, bool, String); 7] = [
            (
                "取字符串 type",
                extract_str(hb, "type").as_deref() == Some("hb"),
                format!("{:?}", extract_str(hb, "type")),
            ),
            (
                "取数字 t（大数）",
                extract_num(hb, "t") == Some(1758600000000),
                format!("{:?}", extract_num(hb, "t")),
            ),
            (
                "取布尔 lease_ok",
                extract_bool(hb, "lease_ok"),
                format!("{}", extract_bool(hb, "lease_ok")),
            ),
            (
                "取嵌套对象 stat",
                extract_substring(hb, "\"stat\":")
                    .map(|v| v.contains("clears_ok") && v.contains("write_fail"))
                    .unwrap_or(false),
                format!("{:?}", extract_substring(hb, "\"stat\":")),
            ),
            (
                "取字符串数组 buttons",
                extract_str_array(edge, "buttons") == vec!["back".to_string(), "volume_up".to_string()],
                format!("{:?}", extract_str_array(edge, "buttons")),
            ),
            (
                "取数字数组 usages",
                extract_num_array(edge, "usages") == vec![241u64, 128u64],
                format!("{:?}", extract_num_array(edge, "usages")),
            ),
            (
                "空数组不被误判为有值",
                extract_num_array(rel, "usages").is_empty()
                    && extract_str(rel, "reason").as_deref() == Some("lease_expired"),
                format!(
                    "usages={:?} reason={:?}",
                    extract_num_array(rel, "usages"),
                    extract_str(rel, "reason")
                ),
            ),
        ];
        for (label, ok, detail) in cases {
            check(label, ok, detail);
        }

        // 4) 内嵌 agent：内容与关键常量
        let agent_hash = agent_sha256_hex();
        check(
            "内嵌 agent 含目标 IOCTL 与三键 usage",
            AGENT_JS.contains("0x80018483")
                && AGENT_JS.contains("0x00F1")
                && AGENT_JS.contains("0x0080")
                && AGENT_JS.contains("0x0081")
                && AGENT_JS.contains("LEASE_MS = 2000"),
            format!("sha256={agent_hash} lines={}", AGENT_JS.lines().count()),
        );

        // 5) 配置生成：参数确实进了 parameters
        let args = Args {
            target_pid: None,
            port: 47831,
            token: "deadbeef".to_string(),
            token_from_file: false,
            gadget: None,
            runtime_dir: PathBuf::new(),
            duration: 0,
            await_hello: DEFAULT_AWAIT_HELLO_S,
            canary_usages: Vec::new(),
            // 自检不连主程序：显式 None（自检必须不需要设备、不需要提权、不碰网络）。
            app_bridge: None,
            // 任务管理三个命令都**不执行**：自检不得改动系统状态
            //（装/删计划任务都要提权，也不该在自检里发生）。
            install_task: false,
            remove_task: false,
            task_status: false,
            follow_app: false,
            hide_window: false,
            dry_run: false,
            observe: false,
            restore: true,
            force: false,
            require_exclusive_host: false,
            selftest: false,
            log: None,
            new_token: false,
            attach_only: false,
            new_generation: false,
        };
        let cfg = serde_like_config(&args);
        check(
            "Gadget 配置：script 交互 + 端口/令牌落进 parameters",
            cfg.contains("\"type\": \"script\"")
                && cfg.contains("\"port\": 47831")
                && cfg.contains("\"token\": \"deadbeef\"")
                && cfg.contains("rc003_agent.js"),
            cfg.replace('\n', " "),
        );

        // 6) 与锁定文件的一致性（编译期内联，确定性断言；不依赖运行时 CWD）
        check(
            "Gadget 锁定文件与 GADGET_SHA256 / GADGET_VERSION 一致",
            GADGET_LOCK_JSON.contains(GADGET_SHA256)
                && GADGET_LOCK_JSON.contains(GADGET_VERSION)
                && GADGET_LOCK_JSON.contains("windows-x86_64"),
            format!(
                "常量 sha256={GADGET_SHA256} version={GADGET_VERSION}；锁定文件 {} 字节",
                GADGET_LOCK_JSON.len()
            ),
        );

        // 7) HostPid 解码的宽度契约（2026-09-23 真机踩到的根因，回归测试）
        //    本机实测 HostPid = REG_QWORD(11) / 8 字节；REG_DWORD(4) / 4 字节也必须支持。
        //    字节取值就是真机 dump 出来的原样（ac 7f 00 00 00 00 00 00 → 32684）。
        let qword_raw = [0xacu8, 0x7f, 0, 0, 0, 0, 0, 0];
        let dword_raw = [0x18u8, 0x04, 0, 0];
        let pid_cases: [(&str, Option<u64>, Option<u64>); 5] = [
            (
                "QWORD 8 字节（本机实际形态）",
                decode_host_pid(REG_QWORD, &qword_raw),
                Some(32684),
            ),
            ("DWORD 4 字节", decode_host_pid(REG_DWORD, &dword_raw), Some(1048)),
            (
                "BINARY 8 字节按同宽度解",
                decode_host_pid(REG_BINARY, &qword_raw),
                Some(32684),
            ),
            ("BINARY 2 字节应拒绝", decode_host_pid(REG_BINARY, &[1, 0]), None),
            (
                "REG_SZ(1) 应拒绝（不得把字符串当数字）",
                decode_host_pid(1, b"32684"),
                None,
            ),
        ];
        for (label, got, want) in pid_cases {
            check(
                &format!("decode_host_pid: {label}"),
                got == want,
                format!("got={got:?} 期望={want:?}"),
            );
        }

        // 8) 实机 Enum 扫描（只读、无需提权）：把"读法能否真读出值"变成可断言的事。
        //    判据刻意不依赖 RC003 是否在场——自检必须在任何机器上都能过。
        //    只断言"凡是存在的诊断节点，HostPid 必须读得出"；RC003 命中数只作报告。
        match enum_hosts() {
            Ok(scan) => {
                let rc003_n = scan.entries.iter().filter(|e| e.is_rc003).count();
                let detail = format!(
                    "diag_keys={} HostPid读出={} no_host={} 读失败={} 命中RC003={}",
                    scan.diag_keys,
                    scan.entries.len(),
                    scan.no_host,
                    scan.failures.len(),
                    rc003_n
                );
                if scan.diag_keys == 0 {
                    check(
                        "实机 Enum 扫描：HostPid 全部可读",
                        true,
                        format!("{detail}（本机无诊断节点，本项空过，属弱结论）"),
                    );
                } else if scan.failures.is_empty() {
                    check("实机 Enum 扫描：HostPid 全部可读", true, detail);
                } else {
                    check(
                        "实机 Enum 扫描：HostPid 全部可读",
                        false,
                        format!("{detail}；失败明细: {:?}", scan.failures),
                    );
                }
            }
            Err(e) => check(
                "实机 Enum 扫描：HostPid 全部可读",
                false,
                format!("枚举失败: {e}"),
            ),
        }

        // 9) 日志时间戳（同样是自实现的原语）。若格式写错，日志里会出现
        //    "看起来像时间但其实是错的"内容，比没有时间戳更坏。
        let stamp = local_stamp();
        let raw = {
            let mut t = LocalTime::default();
            unsafe { GetLocalTime(&mut t) };
            t
        };
        let shape_ok = stamp.len() == 23
            && [4, 7].iter().all(|&i| stamp.as_bytes()[i] == b'-')
            && stamp.as_bytes()[10] == b' '
            && [13, 16].iter().all(|&i| stamp.as_bytes()[i] == b':')
            && stamp.as_bytes()[19] == b'.'
            && stamp.chars().filter(char::is_ascii_digit).count() == 17;
        let range_ok = (2020..=2100).contains(&raw.year)
            && (1..=12).contains(&raw.month)
            && (1..=31).contains(&raw.day)
            && raw.hour <= 23
            && raw.minute <= 59
            && raw.second <= 60;
        check(
            "本机时间戳：形态与取值范围",
            shape_ok && range_ok,
            format!(
                "{stamp}（y={} m={} d={} h={} min={} s={}）",
                raw.year, raw.month, raw.day, raw.hour, raw.minute, raw.second
            ),
        );

        // 10) **会话循环必须遵守 --duration** —— 2026-09-23 真机缺陷的回归项。
        //
        // 缺陷原形：会话读取用阻塞的 `BufReader::lines()`，而 `--duration` 的检查
        // 只在主循环里、且主循环被同步阻塞在 `serve_connection` 之中。于是**只要
        // 会话活着，安全上限就永不生效**（实测 `--duration 300` 已跑到 445 s 仍在运行）。
        //
        // 这里用**什么都不发的**客户端占住连接（最坏情形：连心跳都不可依赖），
        // 给 `serve_connection` 一个 1.5 s 的 deadline，断言它能自己返回。
        // 跑在修复前的实现上会一直阻塞，所以用 `recv_timeout` 兜住：**超时判 FAIL，
        // 而不是把自检挂住**。超时时**不要** join 那个线程（它正阻塞在旧逻辑里）。
        {
            let listener = match TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)) {
                Ok(l) => l,
                Err(e) => {
                    check(
                        "会话循环遵守 --duration",
                        false,
                        format!("无法绑定回环端口: {e}"),
                    );
                    return all_ok;
                }
            };
            let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);

            // 静默客户端：连上后一个字节都不发，并保持连接（不 join，随进程退出即可）。
            std::thread::spawn(move || {
                if let Ok(s) = TcpStream::connect(("127.0.0.1", port)) {
                    std::thread::sleep(Duration::from_secs(10));
                    drop(s);
                }
            });

            let (tx, rx) = std::sync::mpsc::channel::<u64>();
            let spinner = std::thread::spawn(move || {
                if let Ok((stream, _)) = listener.accept() {
                    let mut sess = Session {
                        connected_at: Instant::now(),
                        last_rx: Instant::now(),
                        hello: None,
                        edges: Vec::new(),
                        lines: 0,
                        authenticated: false,
                        config_sent: false,
                    };
                    let quiet = Logger::new(None);
                    let stop = Arc::new(AtomicBool::new(false));
                    let stale = Arc::new(AtomicBool::new(false));
                    let t0 = Instant::now();
                    serve_connection(
                        stream,
                        &mut sess,
                        &quiet,
                        "selftest-token",
                        false,
                        true,
                        &[],
                        &stop,
                        Some(t0 + Duration::from_millis(1500)),
                        // 自检不连主程序：桥接在自检里必须是 None，
                        // 否则"不需要设备、不需要提权、不碰网络"这条自检承诺就破了。
                        None,
                        &stale,
                    );
                    let _ = tx.send(t0.elapsed().as_millis() as u64);
                }
            });

            match rx.recv_timeout(Duration::from_secs(6)) {
                Ok(ms) => {
                    let _ = spinner.join();
                    check(
                        "会话循环遵守 --duration（静默客户端占住连接）",
                        (1400..=5000).contains(&ms),
                        format!("deadline=1500 ms，实际 {ms} ms 返回"),
                    );
                }
                Err(_) => check(
                    "会话循环遵守 --duration（静默客户端占住连接）",
                    false,
                    "serve_connection 6 s 内未返回——收尾条件在会话期间不可见（安全上限失效）"
                        .into(),
                ),
            }
        }

        // 11) 控制台处理器能被注册（Ctrl+C / 关窗才会走"发 disarm 再退出"）。
        //     注册成功只证明"装上了"；**端到端**的 Ctrl+C 行为仍需真机确认，
        //     自检里发 CTRL_C 会把自己杀掉，因此不做。
        let ctrl_ok = unsafe { SetConsoleCtrlHandler(Some(console_ctrl_handler), 1) } != 0;
        check(
            "控制台处理器可注册（Ctrl+C 走收尾而非硬终止）",
            ctrl_ok,
            format!("SetConsoleCtrlHandler -> {ctrl_ok}；端到端 Ctrl+C 行为未在自检中覆盖"),
        );

        // 12) 模块枚举（阳性 + 阴性对照）。
        //     **必须有阳性对照**：一个"什么都查不到"的实现会让 [TAP] 永远打印 0，
        //     于是"没有常驻 tap"这个结论毫无价值——阳性对照是本仓库的硬要求。
        match enum_modules(std::process::id()) {
            Ok(modules) => {
                let self_exe = std::env::current_exe()
                    .ok()
                    .map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())
                    .unwrap_or_default();
                let saw_self = modules
                    .iter()
                    .any(|m| m.path.to_ascii_lowercase().ends_with(&self_exe.to_ascii_lowercase()));
                let taps = resident_taps(&modules);
                check(
                    "模块枚举：阳性对照（本进程自身）与阴性对照（无 Gadget）",
                    saw_self && taps.is_empty() && modules.len() >= 5,
                    format!(
                        "modules={} saw_self_exe({})={} resident_taps={}",
                        modules.len(),
                        self_exe,
                        saw_self,
                        taps.len()
                    ),
                );
            }
            Err(e) => check(
                "模块枚举：阳性对照（本进程自身）与阴性对照（无 Gadget）",
                false,
                format!("enum_modules(自身) 失败：{e}"),
            ),
        }

        // 13) Gadget 模块名判据（含阴性例：别的 frida 组件不是我们的 Gadget）
        let name_cases: [(&str, bool); 5] = [
            ("frida-gadget.dll", true),
            ("FRIDA-GADGET.DLL", true),
            ("frida-gadget.3a7f19c2.dll", true),
            ("frida-agent.dll", false),
            ("kernel32.dll", false),
        ];
        let name_ok = name_cases.iter().all(|(n, want)| is_gadget_module(n) == *want);
        check(
            "Gadget 模块名判据（含分代文件名与阴性例）",
            name_ok,
            format!(
                "{}",
                name_cases
                    .iter()
                    .map(|(n, want)| format!("{n}->{}(期望{want})", is_gadget_module(n)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );

        // 14) 放置策略决策表。这是本轮修复的**核心纯逻辑**：
        //     它决定"要不要碰那个被宿主锁住的文件"，判错就会退回"第二次运行必崩"。
        type PlanCase<'a> = (&'a str, bool, bool, bool, bool, PlanExpect);
        #[derive(Debug, PartialEq, Eq)]
        enum PlanExpect {
            Ok(PlanKind),
            Err(&'static str),
        }
        #[derive(Debug, PartialEq, Eq)]
        enum PlanKind {
            Canonical,
            Attach,
            Generation,
        }
        let plan_cases: [PlanCase; 5] = [
            ("无 tap → 规范化注入", false, false, false, false, PlanExpect::Ok(PlanKind::Canonical)),
            ("有 tap + 令牌可接管 → 接管（不碰文件）", true, true, false, false, PlanExpect::Ok(PlanKind::Attach)),
            ("有 tap + 令牌接不上 + 允许新世代 → 分代复制", true, false, false, true, PlanExpect::Ok(PlanKind::Generation)),
            ("有 tap + 令牌接不上 + 禁止分代 → STALE-TAP", true, false, false, false, PlanExpect::Err("STALE-TAP")),
            ("无 tap + --attach-only → 报错（不能装作已接管）", false, false, true, false, PlanExpect::Err("attach_only_no_tap")),
        ];
        for (label, tap, tok, att, gen, want) in plan_cases {
            let got = decide_plan(tap, tok, att, gen);
            let (ok, detail) = match (&got, &want) {
                (Ok(DllPlan::Canonical), PlanExpect::Ok(PlanKind::Canonical)) => (true, "Canonical".to_string()),
                (Ok(DllPlan::Attach), PlanExpect::Ok(PlanKind::Attach)) => (true, "Attach".to_string()),
                (Ok(DllPlan::Generation(_)), PlanExpect::Ok(PlanKind::Generation)) => (true, "Generation".to_string()),
                (Err(e), PlanExpect::Err(want_tag)) => {
                    let matched = match *want_tag {
                        "STALE-TAP" => e == "STALE-TAP",
                        "attach_only_no_tap" => e.starts_with("--attach-only"),
                        _ => false,
                    };
                    (matched, format!("Err({e})"))
                }
                (a, b) => (false, format!("{a:?}（期望 {b:?}）")),
            };
            check(&format!("放置策略：{label}"), ok, detail);
        }

        // 15) 复用/覆盖判定：决定"会不会去撞那个被宿主锁住的文件"的就是这一行
        let dll_cases: [(bool, bool, DllAction); 3] = [
            (true, true, DllAction::Reuse),
            (true, false, DllAction::Copy),
            (false, false, DllAction::Copy),
        ];
        let dll_ok = dll_cases
            .iter()
            .all(|(e, m, want)| dll_action(*e, *m) == *want);
        check(
            "DLL 放置：摘要一致即复用（不写文件）",
            dll_ok,
            dll_cases
                .iter()
                .map(|(e, m, want)| format!("exists={e} match={m} -> {:?}(期望{want:?})", dll_action(*e, *m)))
                .collect::<Vec<_>>()
                .join(", "),
        );

        // 16) 分代目录名只含 [0-9a-f]，避免任何路径歧义
        let gen_name = generation_dir_name("9F3A1C7E5B2D4F608A1B2C3D4E5F6071");
        check(
            "分代目录名安全（仅小写 [0-9a-f]）",
            gen_name == "gen-9f3a1c7e",
            format!("generation_dir_name -> {gen_name}"),
        );

        // 17) 令牌跨运行稳定 + 分代目录回收
        let tmp = std::env::temp_dir().join(format!("rc003-helper-selftest-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let quiet = Logger::new(None);
        let token_detail = |t1: &str, from1: bool, t2: &str, from2: bool, t3: Option<&str>| {
            format!(
                "1st(from_file={from1})={} 2nd(from_file={from2})={} new={} 文件已更新={}",
                mask_token(t1),
                mask_token(t2),
                t3.map(mask_token).unwrap_or_else(|| "-".into()),
                t3.map(|t| fs::read_to_string(tmp.join(TOKEN_FILE))
                    .map(|s| s.trim() == t)
                    .unwrap_or(false))
                    .unwrap_or(false)
            )
        };
        match load_or_create_token(&tmp, false, &quiet) {
            Ok((t1, from1)) => match load_or_create_token(&tmp, false, &quiet) {
                Ok((t2, from2)) => match load_or_create_token(&tmp, true, &quiet) {
                    Ok((t3, from3)) => {
                        let ok = !from1
                            && from2
                            && t1 == t2
                            && !from3
                            && t3 != t1
                            && t3.len() == 32;
                        let detail = token_detail(&t1, from1, &t2, from2, Some(&t3));
                        check("令牌：首次生成 → 二次读到同一个 → --new-token 才换", ok, detail);
                    }
                    Err(e) => check("令牌：首次生成 → 二次读到同一个 → --new-token 才换", false, e),
                },
                Err(e) => check("令牌：首次生成 → 二次读到同一个 → --new-token 才换", false, e),
            },
            Err(e) => check("令牌：首次生成 → 二次读到同一个 → --new-token 才换", false, e),
        }

        let keep_dir = tmp.join("gen-aaaaaaaa");
        let stale_dir = tmp.join("gen-bbbbbbbb");
        let _ = fs::create_dir_all(&keep_dir);
        let _ = fs::create_dir_all(&stale_dir);
        let _ = fs::write(keep_dir.join("frida-gadget.dll"), b"x");
        let _ = fs::write(stale_dir.join("frida-gadget.dll"), b"x");
        reap_generations(&tmp, Some(keep_dir.as_path()), &quiet);
        check(
            "分代目录回收：保留在用那份，删掉其余",
            keep_dir.is_dir() && !stale_dir.exists(),
            format!(
                "keep_exists={} stale_removed={}",
                keep_dir.is_dir(),
                !stale_dir.exists()
            ),
        );
        // 18) 日志"新一轮运行"分隔线：**只有 open_round 写**，同一轮内再建 Logger 不写。
        //     阴性对照（同轮内新建不得写）与阳性对照（开新一轮必须写）成对出现——
        //     只有阴性断言的话，"一条都没写"也会静默通过。
        //     2026-09-23 真机：续约线程那个 Logger 每轮都多写一条分隔线，把 6 轮数成 11 轮。
        let logp = tmp.join("round.log");
        let r_first = Logger::open_round(Some(logp.clone()));
        r_first.line("第一轮的一行");
        let r_inner = Logger::new(Some(logp.clone())); // 模拟续约线程（同一轮内的第二个 Logger）
        r_inner.line("续约线程的一行");
        let n_same_round = fs::read_to_string(&logp)
            .map(|s| s.matches("新一轮运行").count())
            .unwrap_or(usize::MAX);
        let r_next = Logger::open_round(Some(logp.clone())); // 真正开启新一轮
        r_next.line("第二轮的一行");
        let n_new_round = fs::read_to_string(&logp)
            .map(|s| s.matches("新一轮运行").count())
            .unwrap_or(usize::MAX);
        check(
            "日志分隔线：同轮内再建 Logger 不写，开新一轮才写",
            n_same_round == 0 && n_new_round == 1,
            format!("同一轮内计数={n_same_round}（应为 0）开新轮后={n_new_round}（应为 1）"),
        );

        // 19) 端口占用（10048）必须给出**可操作**文案。2026-09-23 真机撞到过，
        //     当时只说"失败"。用 0 让系统分配一个空闲端口，再绑同一端口必然失败。
        let port_case = match bind_listener(0) {
            Ok(held) => {
                let p = held.local_addr().map(|a| a.port()).unwrap_or(0);
                match bind_listener(p) {
                    Ok(_) => (false, format!("第二次绑定 {p} 竟然成功了（本机应失败）")),
                    Err(e) => (e.contains("已经有另一次"), e),
                }
            }
            Err(e) => (false, format!("首次绑定端口 0 失败：{e}")),
        };
        check(
            "端口占用（10048）：第二次绑定必须失败且提示已有另一次运行在跑",
            port_case.0,
            port_case.1,
        );

        // 20) 接管轮的 `[DLL]` 汇报不得报告 sha256_verified —— 那一轮一个文件字节都没碰，
        //     照搬复用路径的字段会被读成"没校验就用了"（2026-09-23 真机原样打出来过）。
        let fake = PreparedRuntime {
            dll: PathBuf::from("C:\\x\\frida-gadget.dll"),
            reused: false,
            verified: false,
        };
        let attach_fields = dll_report_fields(&DllPlan::Attach, &fake);
        let attach_has_verify = attach_fields.iter().any(|(k, _)| *k == "sha256_verified");
        let attach_action_ok = attach_fields
            .iter()
            .any(|(k, v)| *k == "action" && v == "attach_existing_tap");
        check(
            "接管轮的 [DLL] 汇报：如实写 attach，不得报 sha256_verified",
            attach_action_ok && !attach_has_verify,
            format!("action_ok={attach_action_ok} has_sha256_verified={attach_has_verify}"),
        );

        // 21) 上面那条的**阳性对照**：复用路径必须报告 sha256_verified=true。
        //     没有这条的话，"该字段永远不出现"会让第 20 项假通过。
        let canon_fields = dll_report_fields(
            &DllPlan::Canonical,
            &PreparedRuntime {
                dll: PathBuf::from("C:\\x\\frida-gadget.dll"),
                reused: true,
                verified: true,
            },
        );
        let canon_ok = canon_fields
            .iter()
            .any(|(k, v)| *k == "sha256_verified" && v == "true");
        check(
            "[DLL] 汇报阳性对照：复用路径必须报告 sha256_verified=true",
            canon_ok,
            format!("{canon_fields:?}"),
        );

        // 22) 哨兵键的解析与清空集合。这是"验收可观测性"的唯一支点：解析错一处，
        //     真机那一轮就白跑（而且很可能得出错误结论）。
        let cu_default = clear_usages(&[]);
        let cu_canary = clear_usages(&[0x004A]);
        let cu_overlap = clear_usages(&[0x00F1]);
        check(
            "哨兵键解析：接受 0x4A / 4a / 逗号分隔并去重；拒绝 usage 0 与非十六进制",
            matches!(parse_usage_list("0x4A"), Ok(v) if v == vec![0x004Au16])
                && matches!(parse_usage_list("4a,80"), Ok(v) if v == vec![0x004Au16, 0x0080u16])
                && matches!(parse_usage_list("0x4a,0x4A"), Ok(v) if v.len() == 1)
                && parse_usage_list("0").is_err()
                && parse_usage_list("zz").is_err(),
            format!(
                "0x4A={:?} 4a,80={:?} 0x4a,0x4A={:?} 0={:?} zz={:?}",
                parse_usage_list("0x4A"),
                parse_usage_list("4a,80"),
                parse_usage_list("0x4a,0x4A"),
                parse_usage_list("0"),
                parse_usage_list("zz")
            ),
        );
        check(
            "清空集合：默认=三键；追加哨兵键后三键仍在前；与三键重叠必须报错",
            matches!(&cu_default, Ok(v) if v.as_slice() == TARGET_USAGES.as_slice())
                && matches!(&cu_canary, Ok(v)
                    if v.len() == 4 && v[0] == 0x00F1 && v[3] == 0x004A)
                && cu_overlap.is_err(),
            format!("default={cu_default:?} canary={cu_canary:?} overlap={cu_overlap:?}"),
        );
        check(
            "usage 渲染：hex 四位补零小写，JSON 用十进制",
            usages_hex(&[0x00F1, 0x004A]) == "0x00f1,0x004a"
                && usages_json(&[0x00F1, 0x004A]) == "241,74",
            format!(
                "hex={} json={}",
                usages_hex(&[0x00F1, 0x004A]),
                usages_json(&[0x00F1, 0x004A])
            ),
        );

        // 23) 内嵌 agent 必须与 helper 对齐。`include_str!` 是编译期内联，所以"改了源码
        //     但内联的还是旧的"只可能发生在**改错文件**时——真机表现是"命令发了没人认"，
        //     极难从日志看出来（一切握手正常，只是按键永久不可见）。这条就是防它。
        check(
            "内嵌 agent：三键常量与 helper 一致，且含 targets 命令与两个集合",
            AGENT_JS.contains("var TARGET_USAGES = [0x00F1, 0x0080, 0x0081]")
                && AGENT_JS.contains("if (cmd.type === 'targets')")
                && AGENT_JS.contains("var reportUsages = TARGET_USAGES.slice();")
                && AGENT_JS.contains("var clearUsages = TARGET_USAGES.slice();")
                && AGENT_JS.contains("reportUsages.indexOf(u) >= 0")
                && AGENT_JS.contains("clearUsages.indexOf(u) < 0"),
            format!("agent_sha256={}", agent_sha256_hex()),
        );

        // 23b) 哨兵键（canary）门禁的回归项。**这是 2026-09-26 真机抓到的缺陷**：
        //      onEnter 的早退门禁用 `targetSetIn()`（上报集合=三键）判定，于是"只含哨兵键
        //      的报告"在门禁处就 return，用 clearUsages 的清空循环永远执行不到 —— 哨兵键
        //      是死代码，而它在真机上的表现（"按主页无反应"不成立）会被读成"拦截没生效"。
        //      更阴险的是：`targets:applied ... canary=1` 照样打出来，日志看着一切正常。
        //      行为级覆盖在 agent/agent_logic_test.mjs（含阳性对照）；这里钉住
        //      "门禁确实挂在清空集合上"，防止下一次重构把它悄悄改回上报集合。
        check(
            "内嵌 agent：清空门禁走 clearSetIn / shouldTouchReport（哨兵键缺陷回归）",
            AGENT_JS.contains("function clearSetIn(")
                && AGENT_JS.contains("function shouldTouchReport(")
                && AGENT_JS.contains("if (!shouldTouchReport(bytes)) return;")
                && AGENT_JS.contains("stat.canary_hits++;"),
            format!("has_clearSetIn={} has_gate={}",
                AGENT_JS.contains("function clearSetIn("),
                AGENT_JS.contains("if (!shouldTouchReport(bytes)) return;")),
        );

        // 23c) agent 代次必须与内嵌脚本一致，否则 [AGENT-STALE] 形同虚设：
        //      hello 里报的代次永远对不上，或者反过来永远对得上（两边都忘了改）。
        check(
            "内嵌 agent：AGENT_BUILD 与 JS 里的 AGENT_BUILD 逐字一致",
            AGENT_JS.contains(&format!("var AGENT_BUILD = '{AGENT_BUILD}';"))
                && AGENT_JS.contains("build: AGENT_BUILD,"),
            format!("embedded={AGENT_BUILD}"),
        );

        // 24) 桥接描述文件解析（捕获链第 ② 段）。解析错一处，现象是
        //     "主程序在跑、助手也在跑，但三键就是不动"——日志上只有 APP-BRIDGE unavailable，
        //     而那是一条折叠日志，本身不会告诉你是哪一项不匹配。逐项钉住。
        let good = "version=1\nport=53124\ntoken=deadbeefcafe\npid=999\n";
        let parsed = parse_bridge_descriptor(good);
        check(
            "桥接描述文件：正常解析",
            parsed
                .as_ref()
                .map(|t| t.port == 53124 && t.token == "deadbeefcafe")
                .unwrap_or(false),
            format!(
                "port={} token={}",
                parsed.as_ref().map(|t| t.port).unwrap_or(0),
                parsed
                    .as_ref()
                    .map(|t| t.token.as_str())
                    .unwrap_or("<none>")
            ),
        );
        // 版本不符**整份拒绝**（宁可不可用，也不要跑一个半懂的协议）。
        // 这是阳性对照：若解析器忽略 version，这条必然 FAIL。
        check(
            "桥接描述文件：版本不符必须整份拒绝",
            parse_bridge_descriptor("version=2\nport=53124\ntoken=abc\n").is_none()
                && parse_bridge_descriptor("port=53124\ntoken=abc\n").is_none(),
            "version=2 与缺 version 均应返回 None".to_string(),
        );
        check(
            "桥接描述文件：缺令牌或缺端口必须拒绝",
            parse_bridge_descriptor("version=1\nport=53124\n").is_none()
                && parse_bridge_descriptor("version=1\ntoken=abc\n").is_none()
                && parse_bridge_descriptor("version=1\nport=53124\ntoken=\n").is_none(),
            "缺 token / 缺 port / 空 token 三种都应拒绝".to_string(),
        );

        // 25) 边沿 payload 编码，与主程序侧 `parse_bridge_line` 的 E 分支互为逆运算。
        //     空集合必须编成 `-`：主程序靠它释放全部按下状态；编错的后果是
        //     "抬起事件被吃掉"，表现为映射卡在长按/连发语义上。
        check(
            "桥接边沿编码：空集合为 -，非空为小写十六进制列表",
            format_edge_payload(&[]) == "-"
                && format_edge_payload(&[0x00F1]) == "f1"
                && format_edge_payload(&[0x00F1, 0x0080, 0x0081]) == "f1,80,81",
            format!(
                "空={} 单={} 三键={}",
                format_edge_payload(&[]),
                format_edge_payload(&[0x00F1]),
                format_edge_payload(&[0x00F1, 0x0080, 0x0081])
            ),
        );

        // 26) 与主程序侧的**路径约定**必须一致。两边各自硬编码同一相对路径，
        //     一旦有人只改了一边，现象是"双方都在跑却永远连不上"，
        //     而且两边日志都不会报错（一边说 absent、一边说 listening）。
        let bridge_path = default_app_bridge_path();
        let bridge_leaf = bridge_path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        let bridge_joined = bridge_path.to_string_lossy().to_ascii_lowercase();
        check(
            "桥接描述文件路径：与主程序侧约定一致（SayAll\\rc003-bridge.ini）",
            bridge_leaf == "rc003-bridge.ini"
                && bridge_joined.contains("\\sayall\\")
                && bridge_path
                    .parent()
                    .map(|parent| parent.ends_with("SayAll"))
                    .unwrap_or(false),
            bridge_path.display().to_string(),
        );

        // 27) 桥接描述文件的**只读探测**（`--dry-run` 用的那段）。三态必须可分辨：
        //     absent（主程序没开）/ ok（可解析）/ invalid（版本不符或字段缺失）。
        //     这三态在真机上对应完全不同的处置，混在一起就只能靠猜。
        let probe_dir = tmp.join("bridge-probe");
        let _ = fs::create_dir_all(&probe_dir);
        let probe_absent = probe_bridge_descriptor(&probe_dir.join("nope.ini"));
        let probe_file = probe_dir.join("rc003-bridge.ini");
        let _ = fs::write(&probe_file, "version=1\nport=53124\ntoken=deadbeef\npid=1\n");
        let probe_ok = probe_bridge_descriptor(&probe_file);
        let _ = fs::write(&probe_file, "version=9\nport=53124\ntoken=deadbeef\n");
        let probe_invalid = probe_bridge_descriptor(&probe_file);
        check(
            "桥接描述文件只读探测：absent / ok / invalid 三态可分辨",
            probe_absent.starts_with("absent")
                && probe_ok.starts_with("ok")
                && probe_ok.contains("port=53124")
                && probe_invalid.starts_with("invalid"),
            format!("{probe_absent} | {probe_ok} | {probe_invalid}"),
        );

        // 28) 桥接连接的错误码必须**分辨原因**。2026-09-23 真机日志里出现的
        //     `reason=descriptor_path_unknown` 正是"文件不存在被折叠成路径未知"造成的：
        //     它把人引向"路径配置错了"，而真实原因只是**主程序没在跑**——
        //     同一行的 note 却写着"主程序未运行属正常现象"，两句话自相矛盾。
        //     两类原因必须能分开，且"文件不存在"必须把**路径**一起带出来。
        let quiet_logger = Logger::new(None);
        let missing_path = probe_dir.join("definitely-missing.ini");
        let err_unknown = bridge_connect(None, &quiet_logger).err().unwrap_or_default();
        let err_missing = bridge_connect(Some(&missing_path), &quiet_logger)
            .err()
            .unwrap_or_default();
        check(
            "桥接连接错误码：路径未知 与 文件不存在（带路径）可分辨",
            err_unknown == "descriptor_path_unknown"
                && err_missing.starts_with("descriptor_missing(")
                && err_missing.contains("definitely-missing.ini"),
            format!("none->{err_unknown} | missing->{err_missing}"),
        );

        // 29) 续约写失败必须能**打断**当前连接，不能让主循环卡死。
        //
        //     这是 2026-09-23 真机"死亡螺旋"的回归项：写失败只清 `shared`、
        //     不结束 serve_connection ⇒ 主循环（accept → serve 跑完 → 再 accept，
        //     串行）无法接受 agent 重连上来的新连接 ⇒ 续约永远恢复不了，
        //     日志停在 `[RENEW] state=write_failed` 就不再增长。
        //
        //     deadline 传 None（**不限时**），所以这个用例只可能因为 conn_stale 而返回，
        //     从而把"能被这一条打断"与"到时间自己退"彻底分开。
        //     阳性对照：去掉 serve_connection 里的 conn_stale 检查后，本用例收不到
        //     "已返回"信号，3 秒后 FAIL（回退脚本见 control_make_noguard.py）。
        match TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)) {
            Ok(stale_listener) => {
                let stale_port = stale_listener.local_addr().map(|a| a.port()).unwrap_or(0);
                let (stale_tx, stale_rx) = mpsc::channel::<u64>();
                let stale_flag = Arc::new(AtomicBool::new(false));
                {
                    let stale_flag = Arc::clone(&stale_flag);
                    std::thread::spawn(move || {
                        if let Ok((stream, _)) = stale_listener.accept() {
                            let mut sess = Session {
                                connected_at: Instant::now(),
                                last_rx: Instant::now(),
                                hello: None,
                                edges: Vec::new(),
                                lines: 0,
                                authenticated: false,
                                config_sent: false,
                            };
                            let quiet = Logger::new(None);
                            let stop = Arc::new(AtomicBool::new(false));
                            let t0 = Instant::now();
                            serve_connection(
                                stream,
                                &mut sess,
                                &quiet,
                                "selftest-token",
                                false,
                                true,
                                &[],
                                &stop,
                                None,
                                None,
                                &stale_flag,
                            );
                            let _ = stale_tx.send(t0.elapsed().as_millis() as u64);
                        }
                    });
                }
                // 客户端保持连接但什么都不发：模拟"对端不再读、也不再应答"。
                let _stale_client =
                    TcpStream::connect(SocketAddrV4::new(Ipv4Addr::LOCALHOST, stale_port)).ok();
                std::thread::sleep(Duration::from_millis(300));
                stale_flag.store(true, Ordering::Relaxed);
                match stale_rx.recv_timeout(Duration::from_millis(3000)) {
                    Ok(elapsed) => check(
                        "续约写失败（conn_stale）能立刻打断当前连接",
                        elapsed < 1500,
                        format!(
                            "置位后 {elapsed} ms 内返回（读轮询 {READ_POLL_MS} ms，阈值 1500 ms）"
                        ),
                    ),
                    Err(_) => check(
                        "续约写失败（conn_stale）能立刻打断当前连接",
                        false,
                        "3 秒内 serve_connection 没有返回 ⇒ 主循环会被卡死，agent 的新连接进不了 accept"
                            .to_string(),
                    ),
                }
            }
            Err(error) => check(
                "续约写失败（conn_stale）能立刻打断当前连接",
                false,
                format!("无法绑定本地端口，本项无法执行：{error}"),
            ),
        }

        // 30) 计划任务创建参数：**纯函数**，不碰系统就能钉住契约。
        //     这是"只弹一次 UAC"方案的根基，每一条都对应一种会走样的产品行为：
        //     丢 `/rl highest` ⇒ 触发后仍是普通权限（拿不到报告）；
        //     改成 `/sc daily` ⇒ 调度器每天真的拉起一个提权进程；
        //     丢 `--follow-app` ⇒ 主程序关了助手还挂在后台；
        //     `/tr` 里丢引号 ⇒ 路径带空格时任务指向错误的位置。
        //     真正的安装/删除要提权且会改系统状态，所以**只**在这里验证参数本身。
        let create = task_create_args(std::path::Path::new("C:\\fake\\rc003-helper.exe"));
        let create_text = create.join(" ");
        let exe_part = "\"C:\\fake\\rc003-helper.exe\" --follow-app";
        check(
            "计划任务创建参数：手动触发 + 最高权限 + 跟随主程序 + 路径带引号",
            create
                .windows(2)
                .any(|pair| pair[0] == "/tn" && pair[1] == SCHEDULED_TASK_NAME)
                && create_text.contains(exe_part)
                && create_text.contains("/rl highest")
                && create_text.contains("/sc once")
                && create_text.contains("/st 00:00")
                && !create_text.contains("/sc daily"),
            create_text,
        );

        // 31) 查询与删除：任务名必须一致（否则状态查询正常、删除却删错对象），
        //     删除必须带 `/f`——不带的话 schtasks 会进入交互式确认，
        //     在被主程序拉起、没有控制台输入的场景里会**挂住**而不是失败。
        let query = task_query_args();
        let delete = task_delete_args();
        check(
            "计划任务查询/删除参数：任务名一致、删除必须带 /f",
            query
                .windows(2)
                .any(|pair| pair[0] == "/tn" && pair[1] == SCHEDULED_TASK_NAME)
                && delete
                    .windows(2)
                    .any(|pair| pair[0] == "/tn" && pair[1] == SCHEDULED_TASK_NAME)
                && delete.iter().any(|arg| arg == "/f"),
            format!("query={} | delete={}", query.join(" "), delete.join(" ")),
        );

        // 停用信号：存在则取走并删除，第二次必须读不到——"谁见到谁删"
        // 语义若坏掉，残留信号会把下一次启动的助手当场误杀。
        let signal = tmp.join("rc003-capture-stop");
        fs::write(&signal, b"stop").expect("写停用信号失败");
        let first = take_stop_signal(&signal);
        let second = take_stop_signal(&signal);
        check(
            "停用信号：存在即取走并删除、二次读取为否",
            first && !second && !signal.exists(),
            format!("first={first} second={second}"),
        );

        let _ = fs::remove_dir_all(&tmp);

        logger.line("");
        logger.line(&format!(
            "结论: {}",
            if all_ok { "全部通过" } else { "存在失败项" }
        ));
        all_ok
    }

    /// 用于日志展示的规范化路径：解析 `..` 并去掉 Windows 的 `\\?\` 前缀。
    /// 只为可读性——日志是验收证据，`target/release/../../vendor/...` 这种写法
    /// 会让人怀疑"到底加载了哪一份"。
    fn normalize_display(p: &Path) -> String {
        let resolved = fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        let s = resolved.display().to_string();
        s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
    }

    /// 默认 Gadget 位置：按顺序试若干候选，**并把选中的路径交给调用方记录**。
    /// 不打印路径的话，"找不到 / 找到的是另一个" 都会变成需要反查的谜题。
    fn default_gadget_path() -> Option<PathBuf> {
        let exe = std::env::current_exe().ok()?;
        let dir = exe.parent()?;
        let mut candidates = vec![
            dir.join("frida-gadget.dll"),
            dir.join("vendor").join("frida-gadget.dll"),
            // 开发布局：target/release/rc003-helper.exe → helper/vendor/frida-gadget.dll
            dir.join("..").join("..").join("vendor").join("frida-gadget.dll"),
        ];
        // 以当前工作目录为基准（从 helper/ 或仓库根直接调用 exe 时）
        if let Ok(cwd) = std::env::current_dir() {
            candidates.push(cwd.join("vendor").join("frida-gadget.dll"));
            candidates.push(cwd.join("frida-gadget.dll"));
        }
        candidates.into_iter().find(|p| p.exists())
    }

    /// 校验 Gadget 的 SHA-256。锁定值来自 `vendor/frida-gadget.lock.json`，
    /// 这里是编译期常量——改版本必须同时改锁定文件与本常量（见 ATTRIBUTION.md 登记）。
    fn verify_gadget(path: &Path, logger: &Logger) -> Result<(), String> {
        let bytes = fs::read(path).map_err(|e| format!("读取 Gadget 失败 {}: {e}", path.display()))?;
        let digest = sha256_hex(&bytes);
        logger.kv(
            "[VERIFY]",
            &[
                ("file", normalize_display(path)),
                ("size", bytes.len().to_string()),
                ("sha256", digest.clone()),
                ("expected", GADGET_SHA256.to_string()),
            ],
        );
        if digest != GADGET_SHA256 {
            return Err(format!(
                "Gadget SHA-256 不符：期望 {}，实际 {digest}。请重新运行 vendor/fetch_frida_gadget.py。",
                GADGET_SHA256
            ));
        }
        Ok(())
    }

    fn agent_sha256_hex() -> String {
        sha256_hex(AGENT_JS.as_bytes())
    }

    /// 自带的 SHA-256（零依赖实现，FIPS 180-4）。
    /// 只用于完整性校验而非密码学用途；正确性由 `--selftest` 的已知向量覆盖。
    fn sha256_hex(data: &[u8]) -> String {
        const K: [u32; 64] = [
            0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
            0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
            0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
            0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
            0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
            0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
            0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
            0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
            0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
            0xc67178f2,
        ];
        let mut h: [u32; 8] = [
            0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
            0x5be0cd19,
        ];

        let mut msg = data.to_vec();
        let bit_len = (data.len() as u64) * 8;
        msg.push(0x80);
        while msg.len() % 64 != 56 {
            msg.push(0);
        }
        msg.extend_from_slice(&bit_len.to_be_bytes());

        for chunk in msg.chunks_exact(64) {
            let mut w = [0u32; 64];
            for i in 0..16 {
                w[i] = u32::from_be_bytes([
                    chunk[i * 4],
                    chunk[i * 4 + 1],
                    chunk[i * 4 + 2],
                    chunk[i * 4 + 3],
                ]);
            }
            for i in 16..64 {
                let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
                let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
                w[i] = w[i - 16]
                    .wrapping_add(s0)
                    .wrapping_add(w[i - 7])
                    .wrapping_add(s1);
            }
            let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
                (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
            for i in 0..64 {
                let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
                let ch = (e & f) ^ ((!e) & g);
                let t1 = hh
                    .wrapping_add(s1)
                    .wrapping_add(ch)
                    .wrapping_add(K[i])
                    .wrapping_add(w[i]);
                let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
                let maj = (a & b) ^ (a & c) ^ (b & c);
                let t2 = s0.wrapping_add(maj);
                hh = g;
                g = f;
                f = e;
                e = d.wrapping_add(t1);
                d = c;
                c = b;
                b = a;
                a = t1.wrapping_add(t2);
            }
            h[0] = h[0].wrapping_add(a);
            h[1] = h[1].wrapping_add(b);
            h[2] = h[2].wrapping_add(c);
            h[3] = h[3].wrapping_add(d);
            h[4] = h[4].wrapping_add(e);
            h[5] = h[5].wrapping_add(f);
            h[6] = h[6].wrapping_add(g);
            h[7] = h[7].wrapping_add(hh);
        }
        h.iter().map(|v| format!("{v:08x}")).collect()
    }

    /// Gadget 的固定版本与 SHA-256（来自 vendor/frida-gadget.lock.json，2026-09-23 校验）。
    const GADGET_SHA256: &str = "350beb0e801dc7dc39d21512960d1048b9f21dc72c0ee5ced5cf5d9dc8ea6687";
    /// Gadget 的固定版本号（与锁定文件 entries[0].version 必须一致）。
    const GADGET_VERSION: &str = "17.18.0";
    /// agent 脚本随二进制内嵌，保证"运行的就是校验过的那一份"，不受运行时目录被改动影响。
    /// （写入运行时目录是为了让 Gadget 能按相对路径加载它；内容以本常量为准。）
    const AGENT_JS: &str = include_str!("../agent/rc003_agent.js");
    /// 内嵌 agent 的代次。必须与 `rc003_agent.js` 里的 `AGENT_BUILD` **逐字一致**
    /// （自检第 23c 项钉住）。
    ///
    /// 为什么需要它：Gadget 一旦 LoadLibrary 进宿主，脚本就再也不会被重新读取
    /// （同一路径再 LoadLibrary 只加引用计数，构造函数不重跑）。于是"改了 agent 源码、
    /// 重新构建助手、再跑一次"**并不会**换掉宿主里那个实例——助手会走 `Attach`
    /// 接管旧 tap，握手正常、命令照发、日志漂亮，只是行为还是旧的。
    /// 2026-09-26 哨兵键那一轮就是这样白跑的：以为在验新逻辑，其实接管的是旧实例。
    /// 有了代次，接管旧实例时日志会打 `[AGENT-STALE]`，让这件事一眼可见。
    ///
    /// 注意：**不要**因此自动 disarm 旧实例——产品路径下"宿主里是上一代脚本"是常态
    /// （升级应用后宿主往往还活着），旧脚本照样能正确清三键，自动解除反而会把
    /// 升级后的捕获打断成"必须重启才恢复"。
    const AGENT_BUILD: &str = "2026-09-26.canary-gate";
    /// 锁定文件在编译期内联。三重作用：
    /// 1) **缺失即编译失败**：锁定文件被删/路径写错，构建直接报错，不会产出"看起来正常、
    ///    实际没登记完整性"的二进制（本常量写错路径时已实测触发编译错误）；
    /// 2) **内容漂移由自检确定性捕获**：`include_str!` 只保证文件存在，不保证与
    ///    `GADGET_SHA256` / `GADGET_VERSION` 一致——所以自检第 6 项用本常量做断言，
    ///    且该断言不再依赖运行时 CWD（此前用相对路径读盘，取不到即误报 FAIL）；
    /// 3) 产品运行时不携带锁定文件，自检依然能验证完整性登记。
    const GADGET_LOCK_JSON: &str = include_str!("../vendor/frida-gadget.lock.json");
}
