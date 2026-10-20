//! 本次开机的引导类型与「快速启动」开关状态：**只读取证，不做任何系统动作**。
//!
//! 背景（2026-10-15 用户现场反馈，2026-10-20 落地取证）：
//!
//! - 现象：Windows 10 开启「快速启动」时，已配对的蓝牙设备（用户那台是小米蓝牙
//!   语音遥控器）每隔几十秒掉线一次、几秒后自己连回来；重启电脑能好一阵子，
//!   关掉快速启动则彻底消失。
//! - 机制：快速启动（hiberboot）关机时把内核与驱动状态写进 `hiberfil.sys`，
//!   开机时直接恢复镜像。部分蓝牙适配器（Realtek 8852 系列为已知案例）恢复后
//!   控制器固件带着快照里的旧状态工作，链路每次建立后都会再次被踢掉。
//!   故障发生在 OS/驱动层，应用侧重连无法自愈。
//! - 因此本模块**不尝试修复**：只把「这次开机是不是 hiberboot」「快速启动是否
//!   开启」写进诊断日志。判据只有在**两种开机的对照数据**下才成立——只记
//!   hiberboot 的情况，无法区分"循环发生在 hiberboot"与"循环本来就常见"。
//!
//! 读取来源：
//! - 引导类型：`System` 日志中 `Microsoft-Windows-Kernel-Boot` 事件 27 的
//!   `BootType` 数据（0=完整启动、1=hiberboot、2=从休眠恢复）。这是 Windows
//!   自己对本次开机的判定，不靠推断。
//! - 快速启动开关：`HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Power`
//!   的 `HiberbootEnabled`。它与"本次开机是什么类型"互相独立：开关开着但用户
//!   这次点的是「重启」时，引导类型仍是完整启动——两者都记才能解释现场。
//!
//! 两项都只读、都不需要管理员权限。读不到时如实落 `unknown`，不猜测原因。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::OnceLock;

/// 本次开机的引导类型（`Microsoft-Windows-Kernel-Boot` 事件 27 的口径）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootType {
    /// 完整启动（`BootType=0`）：驱动全部重新初始化。
    Full,
    /// 快速启动 / hiberboot（`BootType=1`）：内核镜像从 `hiberfil.sys` 恢复。
    Hiberboot,
    /// 从休眠恢复（`BootType=2`）：同样走镜像恢复路径。
    HibernateResume,
    /// 读不到事件或事件里没有可解析的值。
    Unknown,
}

impl BootType {
    /// 诊断日志里的稳定 token（不随语言变化）。
    pub fn label(self) -> &'static str {
        match self {
            BootType::Full => "full",
            BootType::Hiberboot => "hiberboot",
            BootType::HibernateResume => "hibernate_resume",
            BootType::Unknown => "unknown",
        }
    }
}

/// 事件 27 查询失败时的 Win32 错误码（0 = 没失败）。只用于日志，不参与判定。
static BOOT_TYPE_QUERY_ERROR: AtomicU32 = AtomicU32::new(0);

static BOOT_TYPE: OnceLock<BootType> = OnceLock::new();
static FAST_STARTUP: OnceLock<Option<bool>> = OnceLock::new();

/// 从 Kernel-Boot 事件 27 的 XML 里解析 `BootType`（纯函数，单测覆盖）。
///
/// 事件数据形如 `<Data Name='BootType'>0x1</Data>`（`EvtRender` 输出的属性用
/// **单引号**，手工构造的样本常用双引号——两种都要认，2026-10-20 真机实测：
/// 只认双引号时解析恒为 `unknown`）。按 `Name` 定位而不是按位置取，避免事件
/// schema 增删字段后错位。
pub fn parse_boot_type_xml(xml: &str) -> BootType {
    let Some(marker) = xml
        .find("Name=\"BootType\"")
        .or_else(|| xml.find("Name='BootType'"))
    else {
        return BootType::Unknown;
    };
    let rest = &xml[marker..];
    let Some(open_end) = rest.find('>') else {
        return BootType::Unknown;
    };
    let value_start = &rest[open_end + 1..];
    let Some(value_end) = value_start.find('<') else {
        return BootType::Unknown;
    };
    let raw = value_start[..value_end].trim();
    let digits = raw
        .strip_prefix("0x")
        .or_else(|| raw.strip_prefix("0X"))
        .unwrap_or(raw);
    match u32::from_str_radix(digits, 16) {
        Ok(0) => BootType::Full,
        Ok(1) => BootType::Hiberboot,
        Ok(2) => BootType::HibernateResume,
        _ => BootType::Unknown,
    }
}

/// 本次开机的引导类型。
///
/// 成功值进程内缓存（开机类型在进程生命周期内不会变）；**`Unknown` 不缓存**：
/// 应用可能恰好在事件日志服务就绪的边沿启动，一次失败不该让这个进程余生都写
/// `unknown`——这条日志的全部价值就是能区分两种开机，读不到时宁可再查一次
/// （查询在毫秒级，一个进程最多几次）。
pub fn boot_type() -> BootType {
    if let Some(cached) = BOOT_TYPE.get().copied() {
        if cached != BootType::Unknown {
            return cached;
        }
    }
    let fresh = query_boot_type_with_fallback();
    if fresh != BootType::Unknown {
        let _ = BOOT_TYPE.set(fresh);
    }
    fresh
}

/// 「快速启动」功能当前是否启用；读不到返回 `None`（不是 `false`——读不到不
/// 等于没开，日志里必须能区分）。与 [`boot_type`] 同理：只缓存读到的值。
pub fn fast_startup_enabled() -> Option<bool> {
    if let Some(cached) = FAST_STARTUP.get().copied().flatten() {
        return Some(cached);
    }
    let fresh = query_fast_startup_enabled_with_fallback();
    if fresh.is_some() {
        let _ = FAST_STARTUP.set(fresh);
    }
    fresh
}

/// 一次性取齐两项，拼成诊断日志的键值片段（`boot_type=… fast_startup_enabled=…`）。
///
/// 启动行与断连循环检测共用同一份读数：两处格式一致，grep 一次即可对齐
/// "循环发生时这次开机是什么类型"。
pub fn boot_context_fields() -> String {
    let query_error = BOOT_TYPE_QUERY_ERROR.load(Ordering::Relaxed);
    let error_field = if query_error == 0 {
        String::new()
    } else {
        format!(" boot_type_query_error=0x{query_error:08x}")
    };
    format!(
        "boot_type={} fast_startup_enabled={}{error_field}",
        boot_type().label(),
        match fast_startup_enabled() {
            Some(true) => "true",
            Some(false) => "false",
            None => "unknown",
        }
    )
}

#[cfg(windows)]
fn query_boot_type_with_fallback() -> BootType {
    match query_boot_type() {
        Ok(boot_type) => {
            // 成功就清掉上一次的失败码：否则残留的错误码会挂在一份**有效**读数
            // 旁边，把"曾经失败过"读成"这次也失败了"。
            BOOT_TYPE_QUERY_ERROR.store(0, Ordering::Relaxed);
            boot_type
        }
        Err(code) => {
            BOOT_TYPE_QUERY_ERROR.store(code, Ordering::Relaxed);
            BootType::Unknown
        }
    }
}

#[cfg(not(windows))]
fn query_boot_type_with_fallback() -> BootType {
    BootType::Unknown
}

#[cfg(windows)]
fn query_fast_startup_enabled_with_fallback() -> Option<bool> {
    query_fast_startup_enabled()
}

#[cfg(not(windows))]
fn query_fast_startup_enabled_with_fallback() -> Option<bool> {
    None
}

/// 查询最新一条 Kernel-Boot 事件 27（反向查询 = 最新在前）。
///
/// 返回 `Err(Win32 错误码)` 只用于日志；`System` 日志对普通用户可读，读不到常见
/// 原因是事件不存在（极简系统 / 事件日志服务被停）而不是权限。
#[cfg(windows)]
fn query_boot_type() -> Result<BootType, u32> {
    use windows::core::{w, PCWSTR};
    use windows::Win32::System::EventLog::{
        EvtClose, EvtNext, EvtQuery, EvtQueryChannelPath, EvtQueryReverseDirection, EvtRender,
        EvtRenderEventXml,
    };

    // 只取事件 27：`EvtQuery` 的 XPath 过滤在日志服务侧完成，不需要把整本
    // System 日志读回来（后者在长开机历史的机器上要几百毫秒）。
    const QUERY: PCWSTR =
        w!("*[System[Provider[@Name='Microsoft-Windows-Kernel-Boot'] and (EventID=27)]]");

    // `EvtQueryChannelPath` 必须给：path 是通道名（"System"），不是 .evtx 文件；
    // 缺了它查询会以 ERROR_INVALID_PARAMETER 失败。反向 = 最新一条在前。
    let flags = EvtQueryChannelPath.0 | EvtQueryReverseDirection.0;
    let result_set = unsafe { EvtQuery(None, w!("System"), QUERY, flags) }
        .map_err(|error| error.code().0 as u32)?;

    let mut events = [0isize; 1];
    let mut returned = 0_u32;
    // timeout 必须给非 0（这里 200ms，只是安全上界：查询结果已由 EvtQuery 在
    // 日志服务侧算好，正常路径立即返回；没有命中时返回 ERROR_NO_MORE_ITEMS，
    // 也不会等满）。
    // 2026-10-20 实测：传 0 时 EvtNext 直接返回 0 条而**不报错**，表现为
    // `boot_type` 永远 unknown——比查询失败更难发现（没有错误码可查）。
    let next = unsafe { EvtNext(result_set, &mut events, 200, 0, &mut returned) };
    let _ = unsafe { EvtClose(result_set) };
    if let Err(error) = next {
        // ERROR_NO_MORE_ITEMS（0x80070103）= 查询成立但 0 条命中：等同"没有这条
        // 事件"，不是故障。其余错误码留在 `boot_type_query_error` 里供取证。
        let code = error.code().0 as u32;
        if code == 0x8007_0103_u32 {
            return Ok(BootType::Unknown);
        }
        return Err(code);
    }
    if returned == 0 {
        return Ok(BootType::Unknown);
    }

    let event = windows::Win32::System::EventLog::EVT_HANDLE(events[0]);
    let mut buffer = vec![0_u16; 8192];
    let mut used_bytes = 0_u32;
    let mut property_count = 0_u32;
    let render = unsafe {
        EvtRender(
            None,
            event,
            EvtRenderEventXml.0,
            (buffer.len() * std::mem::size_of::<u16>()) as u32,
            Some(buffer.as_mut_ptr() as *mut core::ffi::c_void),
            &mut used_bytes,
            &mut property_count,
        )
    };
    let _ = unsafe { EvtClose(event) };
    render.map_err(|error| error.code().0 as u32)?;

    // `bufferused` 是字节数且包含结尾 NUL。
    let chars = (used_bytes as usize / std::mem::size_of::<u16>()).min(buffer.len());
    let xml = String::from_utf16_lossy(&buffer[..chars]);
    Ok(parse_boot_type_xml(&xml))
}

/// 读 `HiberbootEnabled`（REG_DWORD）。只读、不需要提权。
#[cfg(windows)]
fn query_fast_startup_enabled() -> Option<bool> {
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD};

    let subkey = wide("SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Power");
    let value_name = wide("HiberbootEnabled");
    let mut value = 0_u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(subkey.as_ptr()),
            PCWSTR(value_name.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut core::ffi::c_void),
            Some(&mut size),
        )
    };
    if status.0 != 0 {
        return None;
    }
    Some(value != 0)
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 四种取值 + 事件里没有该字段 + 非法文本：解析按 `Name="BootType"` 定位，
    /// 不依赖字段顺序（事件 schema 加字段后仍应正确）。
    ///
    /// 引号两种都要覆盖：`EvtRender` 的真实输出是单引号（本机 2026-10-20 事件：
    /// `<Data Name='BootType'>0</Data>`），测试样本早期只写了双引号——正是这个
    /// 差异让真机解析恒为 `unknown`，所以这里同时钉住两种写法。
    #[test]
    fn parses_boot_type_from_event_xml() {
        // 本机真机事件的真实形状（单引号、无 0x 前缀）。
        let real = r#"<Event xmlns='http://schemas.microsoft.com/win/2004/08/events/event'><System><Provider Name='Microsoft-Windows-Kernel-Boot'/><EventID>27</EventID></System><EventData><Data Name='BootType'>0</Data><Data Name='LoadOptions'> NOEXECUTE=OPTIN</Data></EventData></Event>"#;
        assert_eq!(parse_boot_type_xml(real), BootType::Full);

        let real = real.replace(">0</Data>", ">1</Data>");
        assert_eq!(parse_boot_type_xml(&real), BootType::Hiberboot);

        let xml = r#"<Event><EventData><Data Name="BootType">0x0</Data><Data Name="Other">0x1</Data></EventData></Event>"#;
        assert_eq!(parse_boot_type_xml(xml), BootType::Full);

        let xml = r#"<Event><EventData><Data Name="BootType">0x1</Data></EventData></Event>"#;
        assert_eq!(parse_boot_type_xml(xml), BootType::Hiberboot);

        let xml = r#"<Event><EventData><Data Name="BootType">0x2</Data></EventData></Event>"#;
        assert_eq!(parse_boot_type_xml(xml), BootType::HibernateResume);

        // 没有 0x 前缀也认（不同 Windows 版本的事件渲染方式不完全一致）。
        let xml = r#"<Event><EventData><Data Name="BootType">1</Data></EventData></Event>"#;
        assert_eq!(parse_boot_type_xml(xml), BootType::Hiberboot);

        // 未知取值 / 非法文本 / 字段缺失一律 unknown，不硬猜成 hiberboot。
        for xml in [
            r#"<Event><EventData><Data Name="BootType">0x7</Data></EventData></Event>"#,
            r#"<Event><EventData><Data Name="BootType">hiberboot</Data></EventData></Event>"#,
            r#"<Event><EventData><Data Name="BootType"></Data></EventData></Event>"#,
            r#"<Event><EventData><Data Name="Other">0x1</Data></EventData></Event>"#,
            "",
        ] {
            assert_eq!(parse_boot_type_xml(xml), BootType::Unknown, "xml={xml}");
        }
    }

    #[test]
    fn every_boot_type_has_a_stable_label() {
        assert_eq!(BootType::Full.label(), "full");
        assert_eq!(BootType::Hiberboot.label(), "hiberboot");
        assert_eq!(BootType::HibernateResume.label(), "hibernate_resume");
        assert_eq!(BootType::Unknown.label(), "unknown");
    }

    /// 键值片段必须同时带两项：断连循环日志靠它区分"hiberboot 病"与普通断链。
    #[test]
    fn context_fields_carry_both_values() {
        let fields = boot_context_fields();
        assert!(fields.contains("boot_type="), "{fields}");
        assert!(fields.contains("fast_startup_enabled="), "{fields}");
    }
}
