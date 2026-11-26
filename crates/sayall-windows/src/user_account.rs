//! 当前登录账号能否完成「全按键支持」的提权授权（2026-11-26）。
//!
//! 背景（`Bugs/2026-11-26-fullkey-uac-credential-input-blocked.md`）：
//! 标准用户触发提权时，Windows 要求输入**另一个管理员账户**的凭据；而助手
//! 注册的计划任务按 `InteractiveToken` 绑定到被提权的那个账户，该账户并不在
//! 当前登录会话里——`schtasks /run` 起不来。也就是说：凭据输对了也用不了。
//!
//! 所以开关入口就要把这件事说清楚，而不是把用户送进一个注定失败的 UAC。
//! 判定只看当前进程令牌，不需要管理员权限：
//!
//! - `TokenElevationTypeLimited` → 管理员（UAC 过滤令牌），提权只需"是/否"；
//! - `TokenElevationTypeFull` → 已处于提权态（管理员）；
//! - `TokenElevationTypeDefault` → 未分裂令牌：标准用户，或 UAC 关闭的管理员；
//!   用 `TokenElevation` 区分（后者为 `true`）。
//!
//! 探测失败一律 fail-open（返回 [`ElevationKind::Unknown`]）：一次 API 失败
//! 不能把功能锁死。

/// 当前进程令牌的提权形态（只读，不改任何系统状态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElevationKind {
    /// 管理员，但当前是 UAC 过滤令牌：提权会弹"是/否"。
    AdminFiltered,
    /// 管理员且已提权（例如应用本身以管理员身份启动）。
    AdminElevated,
    /// 标准用户：提权必须输入别的管理员账户的凭据。
    StandardUser,
    /// 探测失败：沿用既有行为（不因一次失败判死）。
    Unknown,
}

impl ElevationKind {
    /// 稳定 token（只进诊断日志，不含用户名/设备身份）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AdminFiltered => "admin_filtered",
            Self::AdminElevated => "admin_elevated",
            Self::StandardUser => "standard_user",
            Self::Unknown => "unknown",
        }
    }
}

/// 纯判定：这种令牌形态能否完成「提权安装助手 + 注册计划任务」的授权。
///
/// 只有标准用户为 `false`；`Unknown` 必须放行（fail-open）。
pub fn allows_capture_authorization(kind: ElevationKind) -> bool {
    !matches!(kind, ElevationKind::StandardUser)
}

/// 读取当前进程的令牌形态（只读）。
#[cfg(windows)]
pub fn elevation_kind() -> ElevationKind {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::TOKEN_QUERY;
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return ElevationKind::Unknown;
        }
        let kind = read_token_kind(token);
        let _ = CloseHandle(token);
        kind
    }
}

#[cfg(windows)]
unsafe fn read_token_kind(token: windows::Win32::Foundation::HANDLE) -> ElevationKind {
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TokenElevationType, TokenElevationTypeFull,
        TokenElevationTypeLimited, TOKEN_ELEVATION, TOKEN_ELEVATION_TYPE,
    };

    let mut elevation_type = TOKEN_ELEVATION_TYPE::default();
    let mut returned = 0u32;
    if GetTokenInformation(
        token,
        TokenElevationType,
        Some(&mut elevation_type as *mut _ as *mut _),
        std::mem::size_of::<TOKEN_ELEVATION_TYPE>() as u32,
        &mut returned,
    )
    .is_err()
    {
        return ElevationKind::Unknown;
    }
    match elevation_type {
        t if t == TokenElevationTypeLimited => ElevationKind::AdminFiltered,
        t if t == TokenElevationTypeFull => ElevationKind::AdminElevated,
        // Default = 未分裂令牌：标准用户，或 UAC 关闭的管理员。
        _ => {
            let mut elevation = TOKEN_ELEVATION::default();
            let mut returned = 0u32;
            if GetTokenInformation(
                token,
                TokenElevation,
                Some(&mut elevation as *mut _ as *mut _),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut returned,
            )
            .is_err()
            {
                return ElevationKind::Unknown;
            }
            if elevation.TokenIsElevated != 0 {
                ElevationKind::AdminElevated
            } else {
                ElevationKind::StandardUser
            }
        }
    }
}

#[cfg(not(windows))]
pub fn elevation_kind() -> ElevationKind {
    ElevationKind::Unknown
}

#[cfg(test)]
mod tests {
    use super::{allows_capture_authorization, ElevationKind};

    #[test]
    fn only_standard_users_are_blocked_and_unknown_stays_open() {
        assert!(allows_capture_authorization(ElevationKind::AdminFiltered));
        assert!(allows_capture_authorization(ElevationKind::AdminElevated));
        assert!(!allows_capture_authorization(ElevationKind::StandardUser));
        // fail-open：探测失败不锁死功能
        assert!(allows_capture_authorization(ElevationKind::Unknown));
    }

    /// 真实令牌查询必须能读出形态：`Unknown` 只允许出现在 API 失败时。
    /// 本机/CI/标准用户机器上都成立，用来拦住"API 接错 → 永远 fail-open →
    /// 门禁形同不存在"的假阴性（2026-11-26）。
    #[cfg(windows)]
    #[test]
    fn token_query_works_on_a_real_process() {
        assert_ne!(
            super::elevation_kind(),
            ElevationKind::Unknown,
            "own-process token must be readable"
        );
    }

    #[test]
    fn tokens_are_stable_strings() {
        assert_eq!(ElevationKind::AdminFiltered.as_str(), "admin_filtered");
        assert_eq!(ElevationKind::AdminElevated.as_str(), "admin_elevated");
        assert_eq!(ElevationKind::StandardUser.as_str(), "standard_user");
        assert_eq!(ElevationKind::Unknown.as_str(), "unknown");
    }
}
