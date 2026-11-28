//! 私有移动端连接运行时的构建期注入点。
//!
//! 公开仓保持本文件返回 `None`。私有构建在打包前替换本文件，返回真实的
//! `sayall-windows-remote` 运行时；公开代码不感知私有 crate 的存在。

use std::sync::Arc;

use super::{PhoneRemoteAccess, PhoneRemoteRuntime};

pub(super) fn install(_access: PhoneRemoteAccess) -> Option<Arc<dyn PhoneRemoteRuntime>> {
    None
}
