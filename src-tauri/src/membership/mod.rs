//! 私有会员组件的宿主接入点。
//!
//! 公开构建里本模块只有空实现：界面拿到「不可用」，不访问任何服务。
//! 私有构建在打包前替换 [`hook`] 的文件内容，由它返回真正的会员运行时
//! （Rust 会员客户端 + 凭据存储）。契约见
//! `docs/architecture/private-component-boundary.md`。

mod hook;
mod runtime;

/// 当前构建可用的会员运行时；公开构建恒为空实现。
///
/// 只再导出宿主实际用到的类型：`membership` 是私有模块，未被引用的再导出会被
/// rustc 判为 unused import。私有实现需要其余类型时，走 `super::runtime::{...}`
/// （见 `hook.rs`）。
pub use runtime::{
    EmailCodeDelivery, MembershipAvailability, MembershipCatalog, MembershipError,
    MembershipPaymentHandoff, MembershipPurchase, MembershipRuntime, MembershipSnapshot,
};

pub fn install() -> std::sync::Arc<dyn MembershipRuntime> {
    hook::install().unwrap_or_else(|| std::sync::Arc::new(runtime::UnavailableMembershipRuntime))
}
