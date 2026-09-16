//! BLE 资源耗尽（`0x80070008` / `0x80004004`）根因诊断探针（2026-09-16）。
//!
//! 背景：2026-09-12 至 09-15 四天内复现 16 轮"系统蓝牙栈资源耗尽"，现有
//! 日志只能证明"应用拿不到资源"，无法回答**资源被谁占着**——这正是
//! AGENTS.md"功能点必须自带日志……一次日志拉取即定位"还缺的那一块。
//!
//! 本模块**只读采样，不改变任何行为**；单个字段取不到写 `unknown`，不猜测、
//! 不省略字段（与 LOGGING.md 一致）。不含设备身份、路径或用户内容。
//!
//! 判读方法（复发时对比同一条 `resource_probe` 行）：
//! - `process_handles` / `gdi` / `user` 单调上升 → 本进程资源泄漏。
//! - `private_kb` / `working_set_kb` 明显上升 → 本进程内存累积。
//! - `system_handles` 上升 → 系统级句柄泄漏（含内核对象）。
//! - `nonpaged_kb` 逼近上限或持续上升 → **内核（驱动）非分页池泄漏**。
//!   这是 `ERROR_NOT_ENOUGH_MEMORY` 在蓝牙内核路径上最典型的成因，也是
//!   "杀用户进程能缓解（退出时释放其内核对象）"与"新进程一启动即失败
//!   （池已被系统级占满）"两个现场现象的**共同**解释。
//! - 上述全部不动 → 资源被系统 BLE 栈自身状态占着（设备节点僵死），
//!   与用户态进程无关。

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::ProcessStatus::{
    GetPerformanceInfo, GetProcessMemoryInfo, PERFORMANCE_INFORMATION, PROCESS_MEMORY_COUNTERS,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetGuiResources, GetProcessHandleCount, GR_GDIOBJECTS, GR_USEROBJECTS,
};

/// 持续失败期间重打探针的间隔（失败次数）。边沿（进入/退出）必打，
/// 中间按此间隔抽样，兼顾"状态未变化不刷屏"与"能看出趋势"。
const ONGOING_PROBE_EVERY: u32 = 25;

struct OwnProcess {
    handles: Option<u32>,
    gdi: Option<u32>,
    user: Option<u32>,
    private_kb: Option<u64>,
    working_set_kb: Option<u64>,
}

struct SystemWide {
    handles: Option<u32>,
    nonpaged_kb: Option<u64>,
    paged_kb: Option<u64>,
    commit_kb: Option<u64>,
    commit_limit_kb: Option<u64>,
    physical_available_kb: Option<u64>,
    process_count: Option<u32>,
    thread_count: Option<u32>,
}

fn own_process() -> OwnProcess {
    let mut handles = None;
    let mut gdi = None;
    let mut user = None;
    let mut private_kb = None;
    let mut working_set_kb = None;
    unsafe {
        let process: HANDLE = GetCurrentProcess();
        let mut count = 0_u32;
        if GetProcessHandleCount(process, &mut count).is_ok() {
            handles = Some(count);
        }
        let gdi_count = GetGuiResources(process, GR_GDIOBJECTS);
        if gdi_count != 0 {
            gdi = Some(gdi_count);
        }
        let user_count = GetGuiResources(process, GR_USEROBJECTS);
        if user_count != 0 {
            user = Some(user_count);
        }
        let mut counters = PROCESS_MEMORY_COUNTERS {
            cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            ..Default::default()
        };
        if GetProcessMemoryInfo(process, &mut counters, counters.cb).is_ok() {
            // PagefileUsage = 私有提交字节；WorkingSetSize = 常驻工作集。
            private_kb = Some(counters.PagefileUsage as u64 / 1024);
            working_set_kb = Some(counters.WorkingSetSize as u64 / 1024);
        }
    }
    OwnProcess {
        handles,
        gdi,
        user,
        private_kb,
        working_set_kb,
    }
}

fn system_wide() -> SystemWide {
    let mut handles = None;
    let mut nonpaged_kb = None;
    let mut paged_kb = None;
    let mut commit_kb = None;
    let mut commit_limit_kb = None;
    let mut physical_available_kb = None;
    let mut process_count = None;
    let mut thread_count = None;
    unsafe {
        let mut info = PERFORMANCE_INFORMATION {
            cb: std::mem::size_of::<PERFORMANCE_INFORMATION>() as u32,
            ..Default::default()
        };
        if GetPerformanceInfo(&mut info, info.cb).is_ok() {
            // PERFORMANCE_INFORMATION 的池/内存字段单位是**页**，需乘 PageSize。
            let page = info.PageSize as u64;
            let to_kb = |pages: usize| (pages as u64).saturating_mul(page) / 1024;
            handles = Some(info.HandleCount);
            nonpaged_kb = Some(to_kb(info.KernelNonpaged));
            paged_kb = Some(to_kb(info.KernelPaged));
            commit_kb = Some(to_kb(info.CommitTotal));
            commit_limit_kb = Some(to_kb(info.CommitLimit));
            physical_available_kb = Some(to_kb(info.PhysicalAvailable));
            process_count = Some(info.ProcessCount);
            thread_count = Some(info.ThreadCount);
        }
    }
    SystemWide {
        handles,
        nonpaged_kb,
        paged_kb,
        commit_kb,
        commit_limit_kb,
        physical_available_kb,
        process_count,
        thread_count,
    }
}

fn show<T: std::fmt::Display>(value: Option<T>) -> String {
    value.map_or_else(|| "unknown".to_owned(), |value| value.to_string())
}

/// 生成一行结构化探针记录（`gatt_note` 载体）。`reason` 为稳定单词，
/// `detail` 为可选的附加键值（空串则省略）。
pub fn resource_probe_note(reason: &str, detail: &str) -> String {
    let own = own_process();
    let system = system_wide();
    let mut note = format!("resource_probe reason={reason}");
    if !detail.is_empty() {
        note.push(' ');
        note.push_str(detail);
    }
    // 逐字段拼接：避免多行字符串字面量用 `\` 续行时把行首空格吃掉，
    // 造成相邻字段粘连（`a=1b=2`）而无法解析。
    let fields = [
        ("process_handles", show(own.handles)),
        ("gdi", show(own.gdi)),
        ("user", show(own.user)),
        ("private_kb", show(own.private_kb)),
        ("working_set_kb", show(own.working_set_kb)),
        ("system_handles", show(system.handles)),
        ("nonpaged_kb", show(system.nonpaged_kb)),
        ("paged_kb", show(system.paged_kb)),
        ("commit_kb", show(system.commit_kb)),
        ("commit_limit_kb", show(system.commit_limit_kb)),
        ("physical_available_kb", show(system.physical_available_kb)),
        ("process_count", show(system.process_count)),
        ("thread_count", show(system.thread_count)),
    ];
    for (key, value) in fields {
        note.push(' ');
        note.push_str(key);
        note.push('=');
        note.push_str(&value);
    }
    note
}

/// 以"重连尝试"为粒度做边沿触发 + 节流采样：进入新一轮资源耗尽、持续中
/// 抽样、恢复时收尾各打一条，避免每次尝试都刷屏。
#[derive(Default)]
pub struct ResourceProbe {
    in_episode: bool,
    failures: u32,
}

impl ResourceProbe {
    /// 一次重连尝试失败。返回需要落盘的探针行（可能为空）。
    pub fn on_failure(&mut self, error_code: &str, attempt: u32) -> Option<String> {
        self.failures = self.failures.saturating_add(1);
        if !self.in_episode {
            self.in_episode = true;
            return Some(resource_probe_note(
                "episode_start",
                &format!("error_code={error_code} attempt={attempt}"),
            ));
        }
        if self.failures % ONGOING_PROBE_EVERY == 0 {
            return Some(resource_probe_note(
                "episode_ongoing",
                &format!(
                    "error_code={error_code} failures={} attempt={attempt}",
                    self.failures
                ),
            ));
        }
        None
    }

    /// 一次重连尝试成功。若此前处于资源耗尽轮次，返回收尾探针行。
    pub fn on_success(&mut self, attempt: u32) -> Option<String> {
        if !self.in_episode {
            return None;
        }
        let failures = self.failures;
        self.in_episode = false;
        self.failures = 0;
        Some(resource_probe_note(
            "episode_end",
            &format!("result=passed attempt={attempt} failures={failures}"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 字段清单：新增/改名时必须同步，否则解析侧会静默丢字段。
    const PROBE_FIELDS: [&str; 13] = [
        "process_handles",
        "gdi",
        "user",
        "private_kb",
        "working_set_kb",
        "system_handles",
        "nonpaged_kb",
        "paged_kb",
        "commit_kb",
        "commit_limit_kb",
        "physical_available_kb",
        "process_count",
        "thread_count",
    ];

    /// 取 `<key>=<value>` 的值。要求 key 前有空格——若相邻字段粘连成
    /// `a=1b=2`，这里会找不到 `" b="` 而失败（回归守卫）。
    fn value_of<'a>(note: &'a str, key: &str) -> &'a str {
        let needle = format!(" {key}=");
        let at = note
            .find(&needle)
            .unwrap_or_else(|| panic!("missing `{key}` in {note}"));
        let rest = &note[at + needle.len()..];
        let end = rest.find(' ').unwrap_or(rest.len());
        &rest[..end]
    }

    #[test]
    fn probe_note_carries_every_field_atomically() {
        let note = resource_probe_note("unit_test", "");
        assert!(
            note.starts_with("resource_probe reason=unit_test "),
            "note={note}"
        );
        for key in PROBE_FIELDS {
            assert!(!value_of(&note, key).is_empty(), "blank `{key}` in {note}");
        }
    }

    #[test]
    fn probe_note_keeps_detail_between_reason_and_fields() {
        let note = resource_probe_note("episode_start", "error_code=x attempt=3");
        assert!(
            note.starts_with("resource_probe reason=episode_start error_code=x attempt=3 "),
            "note={note}"
        );
        for key in PROBE_FIELDS {
            assert!(!value_of(&note, key).is_empty(), "blank `{key}` in {note}");
        }
    }

    #[test]
    fn probe_is_edge_triggered_and_throttled() {
        let mut probe = ResourceProbe::default();
        // 首次失败：进入轮次，必打。
        assert!(probe.on_failure("windows_resource_exhausted", 1).is_some());
        // 同轮次内的普通失败不打。
        assert!(probe.on_failure("windows_resource_exhausted", 1).is_none());
        // 恢复时收尾必打一次。
        assert!(probe.on_success(1).is_some());
        // 恢复正常后的成功不再打。
        assert!(probe.on_success(1).is_none());
        // 新轮次再次边沿触发。
        assert!(probe.on_failure("windows_resource_exhausted", 1).is_some());
    }

    #[test]
    fn ongoing_probe_fires_every_interval() {
        let mut probe = ResourceProbe::default();
        probe.on_failure("windows_resource_exhausted", 1); // start (#1)
        let mut fired = Vec::new();
        for n in 2..=(ONGOING_PROBE_EVERY + 2) {
            if probe.on_failure("windows_resource_exhausted", 1).is_some() {
                fired.push(n);
            }
        }
        assert_eq!(fired, vec![ONGOING_PROBE_EVERY]);
    }
}
