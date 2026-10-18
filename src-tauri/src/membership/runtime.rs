//! 会员运行时的类型与生命周期契约。
//!
//! 平台无关、可被私有构建替换：公开实现是 [`UnavailableMembershipRuntime`]，
//! 私有实现由 `hook.rs` 注入。生命周期方法成对存在（进入前台 / 退出），
//! 与仓库既有的异步资源规则一致。

use serde::Serialize;

/// 会员功能的可用性。
///
/// `Available` 只由私有实现构造：公开构建永远产不出它，所以公开构建下 rustc 会
/// 报「variant 从未构造」。这不是缺陷——公开构建能表达的值本来就少一个。
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MembershipAvailability {
    /// 当前构建没有会员组件（公开构建）。
    Unavailable,
    /// 有会员组件，可以读取账号与权益。
    Available,
}

/// 界面可读的会员摘要。
///
/// 隐私红线：只承载展示所需的派生值，**不含凭据、令牌、邮箱原文、安装标识
/// 或设备标识**；这些一律不越过 Rust 边界。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MembershipSnapshot {
    pub availability: MembershipAvailability,
    /// 展示用账号名（脱敏邮箱等）。未登录为 `None`。
    pub account_display_name: Option<String>,
    /// 是否已开通 Plus。只来自已验签的权益，不采信未签名的服务端摘要。
    pub plus_active: bool,
    /// 权益可用的最晚时刻（Unix 秒）；`plus_active` 为假时为 `None`。
    pub valid_until: Option<i64>,
}

impl MembershipSnapshot {
    /// 公开构建与任何失败分支共用的「不可用」摘要。
    pub fn unavailable() -> Self {
        Self {
            availability: MembershipAvailability::Unavailable,
            account_display_name: None,
            plus_active: false,
            valid_until: None,
        }
    }
}

/// 会员组件的失败。只带稳定错误码，不带服务端文案或用户内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MembershipError {
    pub code: String,
}

impl MembershipError {
    pub fn new(code: impl Into<String>) -> Self {
        Self { code: code.into() }
    }
}

/// 登录验证码的投递结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailCodeDelivery {
    /// 服务端的投递状态；界面只据此显示「已发送」。
    pub delivery: String,
    pub expires_at: Option<String>,
}

/// 会员页可展示的一个商品行（只用于显示，不是购买依据）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MembershipProduct {
    pub id: String,
    pub tier: String,
    pub duration_days: i64,
    pub amount_minor: i64,
    pub original_amount_minor: Option<i64>,
    pub currency: String,
    pub hardware_model: Option<String>,
    pub offer_kind: Option<String>,
    pub target_major_version: Option<i64>,
}

/// 服务端确认的购买方案。
///
/// 价格、币种、版本和销售修订号原样来自服务端；界面不得自行计算。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MembershipOffer {
    pub product_id: String,
    pub tier: String,
    pub currency: String,
    pub amount_minor: i64,
    pub original_amount_minor: Option<i64>,
    pub entitlement_kind: String,
    pub major_version: i64,
    pub target_major_version: i64,
    pub source_major_version: Option<i64>,
    pub offer_kind: String,
    pub sales_revision: i64,
}

/// 会员页的目录：账号状态 + 展示商品 + 唯一可购买方案。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MembershipCatalog {
    /// 是否已登录账号；未登录时 `offer` 来自公开预览，只能展示。
    pub signed_in: bool,
    pub products: Vec<MembershipProduct>,
    pub offer: Option<MembershipOffer>,
}

/// 一次购买尝试。
///
/// `checkout_url` 是服务端返回的托管收银台地址；页面不构造也不修改它。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MembershipPurchase {
    pub order_id: String,
    pub product_id: String,
    pub status: String,
    pub amount_minor: i64,
    pub currency: String,
    pub checkout_url: Option<String>,
}

/// 宿主访问会员功能的唯一接口。
///
/// 线程约定：`snapshot()` 会被主线程按秒调用，**必须立刻返回**；其余方法都会走
/// 网络，`on_foreground()` 由窗口事件在主线程触发，因此实现**必须把阻塞工作放到
/// 别的线程**，不得在调用线程上等待 I/O。
pub trait MembershipRuntime: std::fmt::Debug + Send + Sync {
    /// 读取当前摘要。必须是非阻塞的：界面按秒轮询调用。
    fn snapshot(&self) -> MembershipSnapshot;

    /// 主动刷新（进入前台、用户点刷新）。失败返回稳定错误码。
    fn refresh(&self) -> Result<MembershipSnapshot, MembershipError>;

    /// 给邮箱发一封登录验证码。
    fn start_email_login(&self, _email: &str) -> Result<EmailCodeDelivery, MembershipError> {
        Err(MembershipError::new("membership_unavailable"))
    }

    /// 提交验证码完成登录。
    fn verify_email_login(
        &self,
        _email: &str,
        _code: &str,
    ) -> Result<MembershipSnapshot, MembershipError> {
        Err(MembershipError::new("membership_unavailable"))
    }

    /// 退出登录。只影响本机；账号与权益不因此改变。
    fn logout(&self) -> Result<MembershipSnapshot, MembershipError> {
        Err(MembershipError::new("membership_unavailable"))
    }

    /// 读取商品与购买方案。
    fn catalog(&self) -> Result<MembershipCatalog, MembershipError> {
        Err(MembershipError::new("membership_unavailable"))
    }

    /// 为当前账号创建一个订单，返回托管收银台地址。
    fn purchase(&self, _product_id: &str) -> Result<MembershipPurchase, MembershipError> {
        Err(MembershipError::new("membership_unavailable"))
    }

    /// 查询当前订单；付款确认后同时刷新签名权益。
    fn refresh_purchase(&self) -> Result<MembershipPurchase, MembershipError> {
        Err(MembershipError::new("membership_unavailable"))
    }

    /// 关闭当前订单展示（不取消服务端订单）。
    fn clear_purchase(&self) {}

    /// 当前订单的收银台地址；由宿主原样打开，页面不提供 URL。
    fn checkout_url(&self) -> Option<String> {
        None
    }

    /// 主窗口重新获得焦点。默认不做事。
    fn on_foreground(&self) {}

    /// 应用退出前释放后台任务（网络、凭据刷新、轮询）。
    fn shutdown(&self) {}
}

/// 公开构建的空实现：恒为不可用，不访问任何服务，不持有任何资源。
#[derive(Debug, Default)]
pub struct UnavailableMembershipRuntime;

impl MembershipRuntime for UnavailableMembershipRuntime {
    fn snapshot(&self) -> MembershipSnapshot {
        MembershipSnapshot::unavailable()
    }

    fn refresh(&self) -> Result<MembershipSnapshot, MembershipError> {
        Err(MembershipError::new("membership_unavailable"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_public_build_refuses_every_membership_operation() {
        let runtime = UnavailableMembershipRuntime;
        for code in [
            runtime.refresh().expect_err("refresh").code,
            runtime
                .start_email_login("user@example.com")
                .expect_err("start")
                .code,
            runtime
                .verify_email_login("user@example.com", "123456")
                .expect_err("verify")
                .code,
            runtime.logout().expect_err("logout").code,
            runtime.catalog().expect_err("catalog").code,
            runtime.purchase("plus-v2-cn").expect_err("purchase").code,
            runtime
                .refresh_purchase()
                .expect_err("refresh purchase")
                .code,
        ] {
            assert_eq!(code, "membership_unavailable");
        }
        assert_eq!(runtime.checkout_url(), None);
        runtime.clear_purchase();
    }

    #[test]
    fn the_public_build_installs_an_unavailable_runtime() {
        let runtime = super::super::install();
        assert_eq!(
            runtime.snapshot().availability,
            MembershipAvailability::Unavailable
        );
    }

    #[test]
    fn the_unavailable_runtime_reports_a_stable_code() {
        let runtime = UnavailableMembershipRuntime;
        let error = runtime.refresh().expect_err("must not succeed");
        assert_eq!(error.code, "membership_unavailable");
    }

    #[test]
    fn the_unavailable_snapshot_carries_no_account_data() {
        let snapshot = UnavailableMembershipRuntime.snapshot();
        assert!(snapshot.account_display_name.is_none());
        assert!(!snapshot.plus_active);
        assert!(snapshot.valid_until.is_none());
    }

    #[test]
    fn lifecycle_hooks_are_optional() {
        // 默认实现必须是 no-op：公开构建没有任何后台任务可释放。
        let runtime = UnavailableMembershipRuntime;
        runtime.on_foreground();
        runtime.shutdown();
    }

    #[test]
    fn the_snapshot_serializes_as_camel_case() {
        let json =
            serde_json::to_value(UnavailableMembershipRuntime.snapshot()).expect("must serialize");
        assert_eq!(json["availability"], "unavailable");
        assert_eq!(json["accountDisplayName"], serde_json::Value::Null);
        assert_eq!(json["plusActive"], false);
        assert_eq!(json["validUntil"], serde_json::Value::Null);
    }
}
