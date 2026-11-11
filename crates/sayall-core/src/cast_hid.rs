//! Chromecast 遥控器按键的跨平台纯逻辑（ADR 0005 L0 层）。
//!
//! 键码表有三个事实来源，全部已对齐验证（2026-11-10 HCI 抓包 + vRemoter Mac
//! 实现 + Windows 厂商服务通知）：
//!
//! 1. **legacy report-1 位索引**：HID report ID 0x01 的首字节，与厂商键服务
//!    transmit 通知首字节共用同一键码空间。
//! 2. **消费页 usage**：HID 报告 2 的 `[0x02, usage_lo, usage_hi]` 三字节载荷。
//! 3. 单键翻转边沿跟踪器：Chromecast 遥控器每份 report 只携带一个按下的键，
//!    靠 usage=0 表示全部释放——与标准键盘报告的位掩码语义不同。
//!
//! Windows 与 macOS 后端共用本模块；不要在任何平台 crate 里复制键码表。

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// 语义按键（平台无关）。Windows 的 Raw Input / GATT / Helper 与 macOS 的
/// IOHIDManager / CoreBluetooth 全部映射到这一份枚举。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteButton {
    Back,
    Ok,
    Tv,
    Home,
    Right,
    Left,
    Down,
    Up,
    Menu,
    Power,
    VolumeMute,
    VolumeUp,
    VolumeDown,
    Youtube,
    Netflix,
    Input,
}

/// 语义按键的稳定顺序：映射位掩码、UI 画布布局都依赖该表。
pub const ALL_BUTTONS: [RemoteButton; 16] = [
    RemoteButton::Back,
    RemoteButton::Ok,
    RemoteButton::Tv,
    RemoteButton::Home,
    RemoteButton::Right,
    RemoteButton::Left,
    RemoteButton::Down,
    RemoteButton::Up,
    RemoteButton::Menu,
    RemoteButton::Power,
    RemoteButton::VolumeMute,
    RemoteButton::VolumeUp,
    RemoteButton::VolumeDown,
    RemoteButton::Youtube,
    RemoteButton::Netflix,
    RemoteButton::Input,
];

impl RemoteButton {
    pub fn is_chromecast_mappable(self) -> bool {
        // Chromecast has Back, Home, a direction pad, media and input buttons.
        // Tv and Menu belong to Xiaomi; the voice key uses ATVV exclusively.
        !matches!(self, Self::Tv | Self::Menu)
    }

    /// 在 [`ALL_BUTTONS`] 中的序号（0..12），用于门控位掩码。
    pub fn ordinal(self) -> usize {
        ALL_BUTTONS
            .iter()
            .position(|button| *button == self)
            .expect("ALL_BUTTONS covers every RemoteButton variant")
    }

    /// 按住连续触发间隔：返回 50ms，其余按键 100ms（2026-10-23 用户要求：
    /// 不再限制按键，只要该槽位的动作可连续执行，就允许配置）。
    /// 是否真正触发由显式开关（`ButtonActions.hold_repeat`）与该槽位动作是否
    /// 可连续执行决定，见 button_gestures.rs。
    pub fn repeat_interval(self) -> Option<Duration> {
        Some(if self == Self::Back {
            Duration::from_millis(50)
        } else {
            Duration::from_millis(100)
        })
    }

    /// 使用统计里的稳定控件键（与 Mac `button.<rawValue>` 同源）。
    ///
    /// 只用于本机统计文件，不进入日志；键名变化会让旧统计的排行榜失效，因此
    /// 必须保持稳定。
    pub fn storage_key(self) -> &'static str {
        match self {
            Self::Back => "button.back",
            Self::Ok => "button.ok",
            Self::Tv => "button.tv",
            Self::Home => "button.home",
            Self::Right => "button.right",
            Self::Left => "button.left",
            Self::Down => "button.down",
            Self::Up => "button.up",
            Self::Menu => "button.menu",
            Self::Power => "button.power",
            Self::VolumeMute => "button.volume_mute",
            Self::VolumeUp => "button.volume_up",
            Self::VolumeDown => "button.volume_down",
            Self::Youtube => "button.youtube",
            Self::Netflix => "button.netflix",
            Self::Input => "button.input",
        }
    }

    /// 历史自动连发键（Mac 原版 HIDRemoteScheduler 口径）：返回、方向、音量±。
    /// 旧配置迁移只对这些键补显式 `holdRepeat`，不因新增的可配置范围改变老行为。
    pub fn legacy_auto_repeat(self) -> bool {
        matches!(
            self,
            Self::Back
                | Self::Up
                | Self::Down
                | Self::Left
                | Self::Right
                | Self::VolumeUp
                | Self::VolumeDown
        )
    }
}

/// Chromecast 遥控器 legacy 键码表：HID report-1 位索引（vRemoter, MIT）
/// 与厂商服务 transmit 通知首字节共用同一键码空间（2026-11-10 HCI 抓包
/// 实测 19 键全对齐：1=Power 3=Up 4=Down 5=Left 6=Right 7=OK 8=Mute
/// 10=Home 11=Back 12=Vol+ 13=Vol- 14=YouTube 15=Netflix 17=Input）。
pub fn chromecast_button_for_legacy_index(index: u8) -> Option<RemoteButton> {
    Some(match index {
        1 => RemoteButton::Power,
        3 => RemoteButton::Up,
        4 => RemoteButton::Down,
        5 => RemoteButton::Left,
        6 => RemoteButton::Right,
        7 => RemoteButton::Ok,
        8 => RemoteButton::VolumeMute,
        10 => RemoteButton::Home,
        11 => RemoteButton::Back,
        12 => RemoteButton::VolumeUp,
        13 => RemoteButton::VolumeDown,
        14 => RemoteButton::Youtube,
        15 => RemoteButton::Netflix,
        17 => RemoteButton::Input,
        _ => return None,
    })
}

/// Chromecast 遥控器消费页 usage 键码表：HID 报告 2（三字节
/// `[0x02, usage_lo, usage_hi]`，小端 usage）与 RC003 Helper castReport
/// 报文类共用（CAST-13）。
pub fn chromecast_button_for_consumer_usage(usage: u16) -> Option<RemoteButton> {
    Some(match usage {
        0x019E => RemoteButton::Power,
        0x0089 | 0x0189 => RemoteButton::Input,
        0x0042 => RemoteButton::Up,
        0x0043 => RemoteButton::Down,
        0x0044 => RemoteButton::Left,
        0x0045 => RemoteButton::Right,
        0x0041 => RemoteButton::Ok,
        0x0224 => RemoteButton::Back,
        0x0223 => RemoteButton::Home,
        0x00E9 => RemoteButton::VolumeUp,
        0x00EA => RemoteButton::VolumeDown,
        0x00E2 => RemoteButton::VolumeMute,
        0x0077 => RemoteButton::Youtube,
        0x0078 => RemoteButton::Netflix,
        _ => return None,
    })
}

/// [`chromecast_button_for_consumer_usage`] 的反查：语义键 → 消费页 usage。
/// 一对多键（Input 有 0x0089/0x0189 两个编码）取首见编码，保证目标集下发
/// 的确定性。报告层 Helper（CAST-13）用它把 Chromecast 映射掩码换算成
/// castReport/castClear 的 usage 集。
pub fn consumer_usage_for_chromecast_button(button: RemoteButton) -> Option<u16> {
    Some(match button {
        RemoteButton::Power => 0x019E,
        RemoteButton::Input => 0x0089,
        RemoteButton::Up => 0x0042,
        RemoteButton::Down => 0x0043,
        RemoteButton::Left => 0x0044,
        RemoteButton::Right => 0x0045,
        RemoteButton::Ok => 0x0041,
        RemoteButton::Back => 0x0224,
        RemoteButton::Home => 0x0223,
        RemoteButton::VolumeUp => 0x00E9,
        RemoteButton::VolumeDown => 0x00EA,
        RemoteButton::VolumeMute => 0x00E2,
        RemoteButton::Youtube => 0x0077,
        RemoteButton::Netflix => 0x0078,
        RemoteButton::Tv | RemoteButton::Menu => return None,
    })
}

/// Chromecast 遥控器单键翻转边沿（平台无关状态机）。
///
/// 遥控器每份 HID report-1 / 厂商键通知只携带**一个**按下键；usage=0 表示
/// 释放。跟标准键盘报告不同，收到新键码意味着旧键已松开。本跟踪器把
/// 字节流翻成（按键, 按下/释放）边沿序列，Windows 的厂商服务通道与
/// macOS 的 IOHIDManager 通道共用。
///
/// 行为对齐 vRemoter `ChromecastRemoteHIDBridge.handleReport`（MIT）。
#[derive(Debug, Default)]
pub struct CastKeyEdgeTracker {
    last: Option<RemoteButton>,
}

/// 一次按键边沿。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CastKeyEdge {
    pub button: RemoteButton,
    pub is_pressed: bool,
}

impl CastKeyEdgeTracker {
    pub fn new() -> Self {
        Self { last: None }
    }

    /// 输入一份 report 的键码字节（usage；0 = 全部释放），
    /// 输出 0..2 条边沿（新键按下前旧键补一条释放）。
    pub fn ingest_index(&mut self, index: u8) -> Vec<CastKeyEdge> {
        let mut edges = Vec::new();
        if index == 0 {
            if let Some(button) = self.last.take() {
                edges.push(CastKeyEdge {
                    button,
                    is_pressed: false,
                });
            }
            return edges;
        }
        let button = match chromecast_button_for_legacy_index(index) {
            Some(button) => button,
            None => return edges,
        };
        if self.last == Some(button) {
            return edges;
        }
        if let Some(previous) = self.last.replace(button) {
            edges.push(CastKeyEdge {
                button: previous,
                is_pressed: false,
            });
        }
        edges.push(CastKeyEdge {
            button,
            is_pressed: true,
        });
        edges
    }

    /// 断连时补齐残留按下键的释放边沿（幂等）。
    pub fn release_all(&mut self) -> Vec<CastKeyEdge> {
        self.ingest_index(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_index_table_matches_documented_keys() {
        assert_eq!(
            chromecast_button_for_legacy_index(1),
            Some(RemoteButton::Power)
        );
        assert_eq!(
            chromecast_button_for_legacy_index(7),
            Some(RemoteButton::Ok)
        );
        assert_eq!(
            chromecast_button_for_legacy_index(11),
            Some(RemoteButton::Back)
        );
        assert_eq!(
            chromecast_button_for_legacy_index(17),
            Some(RemoteButton::Input)
        );
        assert_eq!(chromecast_button_for_legacy_index(2), None);
        assert_eq!(chromecast_button_for_legacy_index(0), None);
        assert_eq!(chromecast_button_for_legacy_index(18), None);
    }

    #[test]
    fn consumer_usage_round_trip_covers_all_cast_keys() {
        for button in ALL_BUTTONS {
            let Some(usage) = consumer_usage_for_chromecast_button(button) else {
                continue;
            };
            assert_eq!(chromecast_button_for_consumer_usage(usage), Some(button));
        }
        assert!(consumer_usage_for_chromecast_button(RemoteButton::Tv).is_none());
        assert!(consumer_usage_for_chromecast_button(RemoteButton::Menu).is_none());
    }

    #[test]
    fn tracker_emits_press_then_release() {
        let mut tracker = CastKeyEdgeTracker::new();
        assert_eq!(
            tracker.ingest_index(7),
            vec![CastKeyEdge {
                button: RemoteButton::Ok,
                is_pressed: true
            }]
        );
        assert_eq!(
            tracker.ingest_index(11),
            vec![
                CastKeyEdge {
                    button: RemoteButton::Ok,
                    is_pressed: false
                },
                CastKeyEdge {
                    button: RemoteButton::Back,
                    is_pressed: true
                },
            ]
        );
        assert_eq!(
            tracker.ingest_index(0),
            vec![CastKeyEdge {
                button: RemoteButton::Back,
                is_pressed: false
            }]
        );
        assert!(tracker.ingest_index(0).is_empty());
    }

    #[test]
    fn tracker_deduplicates_repeated_index() {
        let mut tracker = CastKeyEdgeTracker::new();
        assert_eq!(
            tracker.ingest_index(3),
            vec![CastKeyEdge {
                button: RemoteButton::Up,
                is_pressed: true
            }]
        );
        assert!(tracker.ingest_index(3).is_empty());
        assert!(tracker.ingest_index(3).is_empty());
    }

    #[test]
    fn tracker_ignores_unknown_index_but_keeps_state() {
        let mut tracker = CastKeyEdgeTracker::new();
        assert_eq!(
            tracker.ingest_index(6),
            vec![CastKeyEdge {
                button: RemoteButton::Right,
                is_pressed: true
            }]
        );
        assert!(tracker.ingest_index(200).is_empty());
        assert_eq!(
            tracker.ingest_index(0),
            vec![CastKeyEdge {
                button: RemoteButton::Right,
                is_pressed: false
            }]
        );
    }

    #[test]
    fn release_all_is_idempotent() {
        let mut tracker = CastKeyEdgeTracker::new();
        tracker.ingest_index(10);
        assert_eq!(tracker.release_all().len(), 1);
        assert!(tracker.release_all().is_empty());
    }
}
