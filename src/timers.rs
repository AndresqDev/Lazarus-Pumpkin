//! Port of the Lazarus timer subsystem (`me.qiooip.lazarus.timer`).
//!
//! The original had a `Timer` class hierarchy (`PlayerTimer`, `ScoreboardTimer`,
//! `SystemTimer`, `CooldownTimer`, custom timers, ...). Here everything is folded
//! into one serialisable [`TimerManager`] keyed by player UUID, which is far
//! simpler to persist and to drive from a single repeating task.
//!
//! Ability timers and the LunarClient cooldown packets are intentionally omitted.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::factions::Location;

/// Player-scoped timers shown on the scoreboard, ported from `timer.scoreboard.*`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TimerType {
    CombatTag,
    PvpProtection,
    Enderpearl,
    Apple,
    Gapple,
    Home,
    Stuck,
    Logout,
}

impl TimerType {
    /// Scoreboard label (legacy formatting).
    pub fn label(self) -> &'static str {
        match self {
            TimerType::CombatTag => "&4Combat Tag",
            TimerType::PvpProtection => "&aPvP Protection",
            TimerType::Enderpearl => "&3Enderpearl",
            TimerType::Apple => "&6Golden Apple",
            TimerType::Gapple => "&6Gapple",
            TimerType::Home => "&bHome",
            TimerType::Stuck => "&bStuck",
            TimerType::Logout => "&bLogout",
        }
    }
}

/// A cancellable warmup (home / stuck / logout), ported from the original
/// `TeleportTimer` / `LogoutTimer` / `StuckTimer`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingAction {
    pub timer_type: TimerType,
    pub expiry: i64,
    pub origin_world: String,
    pub origin_x: f64,
    pub origin_y: f64,
    pub origin_z: f64,
    pub target: Option<Location>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TimerManager {
    /// uuid -> (timer -> expiry millis)
    pub players: BTreeMap<String, BTreeMap<TimerType, i64>>,
    /// uuid -> cancellable warmup
    pub pending: BTreeMap<String, PendingAction>,
    /// faction id -> DTR freeze expiry millis
    pub faction_freeze: BTreeMap<String, i64>,
    /// custom timer name -> (uuid -> expiry millis)
    pub custom: BTreeMap<String, BTreeMap<String, i64>>,
}

impl TimerManager {
    pub fn is_active(&self, uuid: &str, timer_type: TimerType) -> bool {
        self.remaining(uuid, timer_type) > 0
    }

    /// Remaining time in millis (0 when inactive/expired).
    pub fn remaining(&self, uuid: &str, timer_type: TimerType) -> i64 {
        let Some(timers) = self.players.get(uuid) else {
            return 0;
        };
        let Some(expiry) = timers.get(&timer_type) else {
            return 0;
        };
        (expiry - crate::util::now_millis()).max(0)
    }

    pub fn activate(&mut self, uuid: &str, timer_type: TimerType, seconds: u64) {
        let expiry = crate::util::now_millis() + (seconds as i64 * 1000);
        self.players.entry(uuid.to_string()).or_default().insert(timer_type, expiry);
    }

    pub fn cancel(&mut self, uuid: &str, timer_type: TimerType) {
        if let Some(timers) = self.players.get_mut(uuid) {
            timers.remove(&timer_type);
        }
        if self
            .pending
            .get(uuid)
            .is_some_and(|pending| pending.timer_type == timer_type)
        {
            self.pending.remove(uuid);
        }
    }

    pub fn set_pending(&mut self, uuid: &str, action: PendingAction) {
        self.players
            .entry(uuid.to_string())
            .or_default()
            .insert(action.timer_type, action.expiry);
        self.pending.insert(uuid.to_string(), action);
    }

    pub fn pending(&self, uuid: &str) -> Option<&PendingAction> {
        self.pending.get(uuid)
    }

    pub fn clear_pending(&mut self, uuid: &str) {
        if let Some(pending) = self.pending.remove(uuid) {
            if let Some(timers) = self.players.get_mut(uuid) {
                timers.remove(&pending.timer_type);
            }
        }
    }

    pub fn freeze_faction(&mut self, faction_id: &str, minutes: u64) {
        let expiry = crate::util::now_millis() + (minutes as i64 * 60 * 1000);
        self.faction_freeze.insert(faction_id.to_string(), expiry);
    }

    pub fn unfreeze_faction(&mut self, faction_id: &str) {
        self.faction_freeze.remove(faction_id);
    }

    pub fn is_frozen(&self, faction_id: &str) -> bool {
        self.faction_freeze
            .get(faction_id)
            .is_some_and(|expiry| *expiry > crate::util::now_millis())
    }

    pub fn freeze_remaining(&self, faction_id: &str) -> i64 {
        self.faction_freeze
            .get(faction_id)
            .map(|expiry| (expiry - crate::util::now_millis()).max(0))
            .unwrap_or(0)
    }

    pub fn activate_custom(&mut self, name: &str, uuid: &str, seconds: u64) {
        let expiry = crate::util::now_millis() + (seconds as i64 * 1000);
        self.custom.entry(name.to_string()).or_default().insert(uuid.to_string(), expiry);
    }

    pub fn custom_remaining(&self, name: &str, uuid: &str) -> i64 {
        self.custom
            .get(name)
            .and_then(|map| map.get(uuid))
            .map(|expiry| (expiry - crate::util::now_millis()).max(0))
            .unwrap_or(0)
    }

    /// Removes all expired entries and returns the list of player timers that just
    /// expired, so callers can announce them.
    pub fn purge_expired(&mut self) -> Vec<(String, TimerType)> {
        let now = crate::util::now_millis();
        let mut expired = Vec::new();

        for (uuid, timers) in &mut self.players {
            let before = timers.len();
            timers.retain(|timer_type, expiry| {
                if *expiry <= now {
                    expired.push((uuid.clone(), *timer_type));
                    false
                } else {
                    true
                }
            });
            let _ = before;
        }

        self.faction_freeze.retain(|_, expiry| *expiry > now);
        expired
    }
}
