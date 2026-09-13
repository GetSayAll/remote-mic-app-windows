//! Optional, explicitly started user-mode capture. This is not driver attribution.
//! The ordinary-privilege mapping engine remains the only action executor.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::raw_input::{ButtonEdge, RemoteButton};
use crate::rc003_filter::requires_filter;
use crate::ConnectionPhase;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserHidSnapshot {
    pub available: bool,
    pub phase: UserHidPhase,
    pub reason: Option<String>,
    pub scope: Option<SourceScope>,
    pub cleanup_confirmed: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserHidPhase {
    #[default]
    Stopped,
    Starting,
    Ready,
    Stopping,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceScope {
    Rc003,
    ProxyUnverified,
}

#[derive(Default)]
struct Link {
    epoch: u64,
    peer: Option<u64>,
    online: bool,
}

static LINK: Mutex<Link> = Mutex::new(Link {
    epoch: 0,
    peer: None,
    online: false,
});

pub(crate) fn invalidate_link() {
    let mut link = LINK.lock().unwrap_or_else(|p| p.into_inner());
    link.epoch = link.epoch.wrapping_add(1);
    link.online = false;
    link.peer = None;
}

pub(crate) fn note_peer(peer: u64, is_rc003: bool) {
    let mut link = LINK.lock().unwrap_or_else(|p| p.into_inner());
    link.epoch = link.epoch.wrapping_add(1);
    link.peer = is_rc003.then_some(peer);
    link.online = false;
}

pub(crate) fn note_phase(phase: ConnectionPhase) {
    let mut link = LINK.lock().unwrap_or_else(|p| p.into_inner());
    let online = matches!(
        phase,
        ConnectionPhase::Ready | ConnectionPhase::Streaming | ConnectionPhase::Draining
    );
    if !online && link.online {
        link.epoch = link.epoch.wrapping_add(1);
    }
    link.online = online;
}

#[derive(Debug, Clone)]
pub struct UserHidPermit {
    pub(crate) id: u64,
    alive: Arc<AtomicBool>,
    epoch: Option<u64>,
}

impl UserHidPermit {
    pub(crate) fn for_link(id: u64) -> Result<(Self, u64), &'static str> {
        let link = LINK.lock().unwrap_or_else(|p| p.into_inner());
        let peer = link
            .peer
            .filter(|_| link.online)
            .ok_or("rc003_connection_not_ready")?;
        Ok((
            Self {
                id,
                alive: Arc::new(AtomicBool::new(true)),
                epoch: Some(link.epoch),
            },
            peer,
        ))
    }

    pub(crate) fn valid(&self) -> bool {
        if !self.alive.load(Ordering::Acquire) {
            return false;
        }
        let link = LINK.lock().unwrap_or_else(|p| p.into_inner());
        self.epoch
            .is_none_or(|epoch| link.online && link.epoch == epoch && link.peer.is_some())
    }

    pub(crate) fn cancel(&self) {
        self.alive.store(false, Ordering::Release);
    }

    #[cfg(test)]
    pub(crate) fn fixture(id: u64) -> Self {
        Self {
            id,
            alive: Arc::new(AtomicBool::new(true)),
            epoch: None,
        }
    }
}

#[derive(Debug)]
pub struct UserHidReport {
    pub(crate) id: u64,
    pub(crate) seq: u64,
    pub(crate) stream: u8,
    pub(crate) scope: SourceScope,
    pub(crate) active: Vec<RemoteButton>,
    pub(crate) received_at: std::time::Instant,
}

pub(crate) struct UserHidSession {
    pub(crate) permit: UserHidPermit,
    source: Option<(u8, SourceScope)>,
    seq: u64,
    held: BTreeSet<RemoteButton>,
    confirmed: BTreeSet<RemoteButton>,
}

impl UserHidSession {
    pub(crate) fn new(permit: UserHidPermit) -> Self {
        Self {
            permit,
            source: None,
            seq: 0,
            held: BTreeSet::new(),
            confirmed: BTreeSet::new(),
        }
    }

    pub(crate) fn confirmed(&self) -> Vec<RemoteButton> {
        self.confirmed.iter().copied().collect()
    }

    pub(crate) fn observe(
        &mut self,
        report: UserHidReport,
    ) -> Result<Vec<ButtonEdge>, &'static str> {
        if report.id != self.permit.id || !self.permit.valid() {
            return Err("stale_session");
        }
        let source = (report.stream, report.scope);
        let active: BTreeSet<_> = report.active.iter().copied().collect();
        if report.seq <= self.seq
            || report.received_at.elapsed() > std::time::Duration::from_secs(2)
            || !(1..=16).contains(&report.stream)
            || self.source.is_some_and(|old| old != source)
            || report.active.len() != active.len()
            || active.len() > 3
            || active.iter().any(|button| !requires_filter(*button))
        {
            self.permit.cancel();
            return Err("invalid_report_or_source");
        }
        self.source = Some(source);
        self.seq = report.seq;
        let mut edges = Vec::new();
        for button in self.held.difference(&active) {
            if self.confirmed.contains(button) {
                edges.push(ButtonEdge {
                    button: *button,
                    is_pressed: false,
                });
            } else {
                self.confirmed.insert(*button);
            }
        }
        for button in active.difference(&self.held) {
            if self.confirmed.contains(button) {
                edges.push(ButtonEdge {
                    button: *button,
                    is_pressed: true,
                });
            }
        }
        self.held = active;
        Ok(edges)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(seq: u64, active: Vec<RemoteButton>) -> UserHidReport {
        UserHidReport {
            id: 1,
            seq,
            stream: 1,
            scope: SourceScope::ProxyUnverified,
            active,
            received_at: std::time::Instant::now(),
        }
    }

    #[test]
    fn confirms_each_pair_without_executing_it_and_deduplicates_states() {
        let mut session = UserHidSession::new(UserHidPermit::fixture(1));
        assert!(session.observe(report(1, vec![])).unwrap().is_empty());
        assert!(session
            .observe(report(2, vec![RemoteButton::Back]))
            .unwrap()
            .is_empty());
        assert!(session.observe(report(3, vec![])).unwrap().is_empty());
        assert_eq!(session.confirmed(), vec![RemoteButton::Back]);
        assert_eq!(
            session
                .observe(report(4, vec![RemoteButton::Back]))
                .unwrap()
                .len(),
            1
        );
        assert!(session
            .observe(report(5, vec![RemoteButton::Back]))
            .unwrap()
            .is_empty());
        assert_eq!(
            session.observe(report(6, vec![])).unwrap(),
            vec![ButtonEdge {
                button: RemoteButton::Back,
                is_pressed: false
            }]
        );
    }

    #[test]
    fn rejects_other_keys_replays_source_changes_and_stopped_sessions() {
        for case in 0..5 {
            let mut session = UserHidSession::new(UserHidPermit::fixture(1));
            session.observe(report(1, vec![])).unwrap();
            let mut input = report(2, vec![]);
            match case {
                0 => input.active.push(RemoteButton::Ok),
                1 => input.seq = 1,
                2 => input.stream = 2,
                3 => input.scope = SourceScope::Rc003,
                _ => session.permit.cancel(),
            }
            assert!(session.observe(input).is_err());
            assert!(!session.permit.valid());
        }
    }
}
