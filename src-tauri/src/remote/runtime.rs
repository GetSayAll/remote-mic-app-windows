//! 移动端连接运行时的类型与生命周期契约。
//!
//! 公开实现是 [`UnavailablePhoneRemoteRuntime`]；私有实现由 `hook.rs` 注入。
//! 快照只包含展示所需的派生值：二维码/小程序码是图片 data URL，不含 token、
//! IP、端口或邀请原文。

use serde::Serialize;
use std::collections::BTreeMap;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PhoneRemoteAvailability {
    Available,
    Unavailable,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PhoneRemoteMembershipState {
    Active,
    SignInRequired,
    PlusRequired,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NearbyRemoteState {
    Idle,
    Waiting,
    WaitingForApproval,
    Connected,
    Failed,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WebRemoteState {
    Idle,
    Unavailable,
    WaitingForScan,
    WaitingForApproval,
    Connected,
    Expired,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NearbyRemoteSnapshot {
    pub state: NearbyRemoteState,
    pub expires_at: Option<i64>,
    pub device_name: Option<String>,
    /// PNG data URL；页面只拿图片，不拿邀请 URL。
    pub qr_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebRemoteSnapshot {
    pub state: WebRemoteState,
    pub expires_at: Option<i64>,
    pub device_name: Option<String>,
    /// 小程序码 data URL；不含 join token。
    pub mini_program_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhoneRemoteApproval {
    /// 用于拒绝迟到审批；不是会话密钥。
    pub request_id: String,
    pub device_name: String,
    pub pairing_code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhoneRemoteSnapshot {
    pub availability: PhoneRemoteAvailability,
    pub membership: PhoneRemoteMembershipState,
    pub ios: NearbyRemoteSnapshot,
    pub web: WebRemoteSnapshot,
    pub trusted_device_count: usize,
    pub approval: Option<PhoneRemoteApproval>,
}

impl PhoneRemoteSnapshot {
    pub fn unavailable() -> Self {
        Self {
            availability: PhoneRemoteAvailability::Unavailable,
            membership: PhoneRemoteMembershipState::PlusRequired,
            ios: NearbyRemoteSnapshot {
                state: NearbyRemoteState::Idle,
                expires_at: None,
                device_name: None,
                qr_code: None,
            },
            web: WebRemoteSnapshot {
                state: WebRemoteState::Idle,
                expires_at: None,
                device_name: None,
                mini_program_code: None,
            },
            trusted_device_count: 0,
            approval: None,
        }
    }
}

pub trait PhoneRemoteRuntime: Send + Sync {
    fn snapshot(&self) -> PhoneRemoteSnapshot;

    fn start_ios(&self) -> Result<PhoneRemoteSnapshot, String>;
    fn stop_ios(&self) -> Result<PhoneRemoteSnapshot, String>;
    fn approve(&self, request_id: &str) -> Result<PhoneRemoteSnapshot, String>;
    fn deny(&self, request_id: &str) -> Result<PhoneRemoteSnapshot, String>;
    fn clear_trusted_devices(&self) -> Result<PhoneRemoteSnapshot, String>;
    fn set_button_titles(&self, _titles: BTreeMap<String, String>) {}

    fn start_web(&self) -> Result<PhoneRemoteSnapshot, String>;
    fn stop_web(&self) -> Result<PhoneRemoteSnapshot, String>;

    /// 退出前调用；必须幂等。公开实现是 no-op。
    fn shutdown(&self) {}
}

pub struct UnavailablePhoneRemoteRuntime;

fn unavailable_error() -> String {
    "phone_remote_unavailable".to_owned()
}

impl PhoneRemoteRuntime for UnavailablePhoneRemoteRuntime {
    fn snapshot(&self) -> PhoneRemoteSnapshot {
        PhoneRemoteSnapshot::unavailable()
    }

    fn start_ios(&self) -> Result<PhoneRemoteSnapshot, String> {
        Err(unavailable_error())
    }

    fn stop_ios(&self) -> Result<PhoneRemoteSnapshot, String> {
        Err(unavailable_error())
    }

    fn approve(&self, _request_id: &str) -> Result<PhoneRemoteSnapshot, String> {
        Err(unavailable_error())
    }

    fn deny(&self, _request_id: &str) -> Result<PhoneRemoteSnapshot, String> {
        Err(unavailable_error())
    }

    fn clear_trusted_devices(&self) -> Result<PhoneRemoteSnapshot, String> {
        Err(unavailable_error())
    }

    fn start_web(&self) -> Result<PhoneRemoteSnapshot, String> {
        Err(unavailable_error())
    }

    fn stop_web(&self) -> Result<PhoneRemoteSnapshot, String> {
        Err(unavailable_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_build_reports_unavailable_and_rejects_actions() {
        let runtime = UnavailablePhoneRemoteRuntime;
        let snapshot = runtime.snapshot();
        assert_eq!(snapshot.availability, PhoneRemoteAvailability::Unavailable);
        assert_eq!(snapshot.ios.state, NearbyRemoteState::Idle);
        assert_eq!(snapshot.web.state, WebRemoteState::Idle);
        assert!(snapshot.approval.is_none());
        assert_eq!(runtime.start_ios().unwrap_err(), "phone_remote_unavailable");
        assert_eq!(runtime.start_web().unwrap_err(), "phone_remote_unavailable");
    }

    #[test]
    fn public_snapshot_serializes_camel_case_fields() {
        let json = serde_json::to_value(PhoneRemoteSnapshot::unavailable()).unwrap();
        assert_eq!(json["availability"], "unavailable");
        assert_eq!(json["membership"], "plus_required");
        assert_eq!(json["ios"]["state"], "idle");
        assert_eq!(json["web"]["state"], "idle");
        assert_eq!(json["trustedDeviceCount"], 0);
        assert!(json["approval"].is_null());
    }
}
