//! 私有会员组件的注入点。
//!
//! 公开构建保持本文件原样：始终返回 `None`，应用不使用会员服务。
//!
//! 私有构建在打包前用同名文件替换本文件内容，返回私有运行时。契约：
//!
//! 1. 只依赖本模块可用的类型：`MembershipRuntime`、`MembershipSnapshot`，
//!    以及 `super::runtime::{MembershipAvailability, MembershipError}`；
//! 2. 不修改本目录外的任何公开文件；
//! 3. 不得让公开构建因为缺少私有 crate 而无法编译。
//!
//! 见 `docs/architecture/private-component-boundary.md`。

use super::MembershipRuntime;
use std::sync::Arc;

pub fn install(_usage_statistics_path: std::path::PathBuf) -> Option<Arc<dyn MembershipRuntime>> {
    None
}
