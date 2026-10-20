//! 系统唤醒的进程内代次（跨层信号，issue #264，2026-10-15 现场）。
//!
//! 睡眠/唤醒通知落在 BLE 工作线程（`ble.rs` 的 `SystemResumed`），而"全按键支持"
//! 的常驻对账在应用侧（`src-tauri/src/lib.rs`）。依赖方向是 app → 本 crate，
//! 因此信号只能寄存这里，由两侧各自读写。
//!
//! 只做一件事：唤醒时自增一代，对账侧的睡眠比对代次后**立刻**醒来重查。缺了它，
//! 唤醒后要等满一个 15 秒观察窗才轮到下一轮判断——用户按下第一次语音键时，
//! 应用还在睡着（现场唤醒后 0.66 秒判出失效纯属轮询恰好落在那里，不是设计）。

use std::sync::atomic::{AtomicU64, Ordering};

static RESUME_GENERATION: AtomicU64 = AtomicU64::new(0);

/// 记录一次系统唤醒（幂等，纯计数；重复调用只会多醒一次对账）。
pub fn note_system_resume() {
    RESUME_GENERATION.fetch_add(1, Ordering::Relaxed);
}

/// 当前唤醒代次。调用方保存上次看到的值，变化即表示"期间发生过唤醒"。
pub fn resume_generation() -> u64 {
    RESUME_GENERATION.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_generation_only_moves_forward() {
        let before = resume_generation();
        note_system_resume();
        note_system_resume();
        assert!(resume_generation() >= before + 2);
    }
}
