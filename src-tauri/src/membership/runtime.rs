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
    ///
    /// 这是**租约**到期时刻（服务端最长 7 天），只说明「该刷新了」，不是会员到期日。
    pub valid_until: Option<i64>,
    /// 会员本身到期时刻（Unix 秒）。
    ///
    /// 大版本买断会拿到服务端约定的远期哨兵值，界面应显示「当前版本长期有效」
    /// 而不是具体日期。与 Mac 的 `MembershipAccountState.expiresAt` 同义。
    pub membership_expires_at: Option<i64>,
    /// 本机是否存着账号会话。
    ///
    /// 与「有没有显示名」分开：bootstrap 失败时显示名会缺席，但账号会话仍在，
    /// 此时服务端会以 `anonymous_session_required` 拒绝邮箱登录。界面必须据此
    /// 显示「已登录但状态待同步」，而不是给一个注定失败的登录表单。
    pub account_signed_in: bool,
    /// 权益答案的新鲜度。
    ///
    /// `current`：本次实时读取并验签成功。
    /// `offline`：实时读取失败，但此前验签过的租约仍在有效期内，Plus 继续可用。
    /// `unavailable`：读取失败且没有可用租约。
    ///
    /// 与 Mac 的 `MembershipSyncState` 同义；界面据它区分「离线使用中」与
    /// 「暂时无法同步」，不能把离线用户显示成权益丢失。
    pub sync_state: String,
    /// 已验签租约覆盖的大版本。
    ///
    /// 买断覆盖一个或多个大版本；Mac 取最大值显示为「SayAll N」。`None` 表示
    /// 租约没有声明。
    pub major_versions: Option<Vec<i64>>,
}

impl MembershipSnapshot {
    /// 公开构建与任何失败分支共用的「不可用」摘要。
    pub fn unavailable() -> Self {
        Self {
            availability: MembershipAvailability::Unavailable,
            account_display_name: None,
            plus_active: false,
            valid_until: None,
            membership_expires_at: None,
            account_signed_in: false,
            sync_state: "current".to_string(),
            major_versions: None,
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
    /// 服务端授权方案全集（首个即 `offer`）；私有页按需展示切换。
    pub options: Vec<MembershipOffer>,
    pub catalog_status: Option<MembershipCatalogStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MembershipCatalogStatus {
    pub status: String,
    pub official_url: Option<String>,
    pub github_url: Option<String>,
    pub support_url: Option<String>,
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

/// 微信小程序支付交接（与 Mac 的 handoff sheet 同款）。
///
/// `code_data_url` 是宿主取回并编码后的二维码图片；页面不接触 access token
/// 或微信 code URL。`requested_product_id` 是页面选中的服务端方案 id。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MembershipPaymentHandoff {
    pub id: String,
    pub email: String,
    pub product_id: String,
    pub requested_product_id: String,
    pub amount_minor: i64,
    pub currency: String,
    pub status: String,
    pub code_data_url: Option<String>,
    pub open_scheme: Option<String>,
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

    /// 兑换一枚兑换码；成功后重新读取签名权益。
    fn redeem(&self, _code: &str) -> Result<MembershipSnapshot, MembershipError> {
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

    /// 为当前账号创建（或复用）微信小程序支付交接；返回二维码与价格。
    fn create_payment_handoff(
        &self,
        _product_id: &str,
    ) -> Result<MembershipPaymentHandoff, MembershipError> {
        Err(MembershipError::new("membership_unavailable"))
    }

    /// 查询当前支付交接状态；成功后应已刷新签名权益。
    fn refresh_payment_handoff(&self) -> Result<MembershipPaymentHandoff, MembershipError> {
        Err(MembershipError::new("membership_unavailable"))
    }

    /// 当前支付交接；用于关闭弹窗后恢复“继续支付”。
    fn payment_handoff(&self) -> Option<MembershipPaymentHandoff> {
        None
    }

    /// Whether this build talks to staging; drives the test-purchase badge.
    fn is_test_environment(&self) -> bool {
        false
    }

    /// Expire the active handoff on the server and drop it locally.
    fn expire_payment_handoff(&self) -> Result<(), MembershipError> {
        Err(MembershipError::new("membership_unavailable"))
    }

    /// 丢失当前支付交接（退出登录等）；服务端交接过期后自行清理。
    fn clear_payment_handoff(&self) {}

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
            runtime.redeem("MXII23456789").expect_err("redeem").code,
            runtime.catalog().expect_err("catalog").code,
            runtime.purchase("plus-v2-cn").expect_err("purchase").code,
            runtime
                .refresh_purchase()
                .expect_err("refresh purchase")
                .code,
            runtime
                .expire_payment_handoff()
                .expect_err("expire handoff")
                .code,
        ] {
            assert_eq!(code, "membership_unavailable");
        }
        assert_eq!(runtime.checkout_url(), None);
        assert!(!runtime.is_test_environment());
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
        assert_eq!(json["membershipExpiresAt"], serde_json::Value::Null);
        assert_eq!(json["accountSignedIn"], false);
        assert_eq!(json["syncState"], "current");
        assert_eq!(json["majorVersions"], serde_json::Value::Null);
    }
}
