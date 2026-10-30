// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

//! In-process single-flight for schema index builds.
//!
//! Startup auto-build and the first tool call used to each take
//! `IndexStore`'s exclusive file lock; the loser failed with "another process
//! is building the index". This gate lets every caller in the same process
//! await the build that is already running. The file lock underneath is
//! unchanged, so a second OS process still gets `IndexError::Building`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use tokio::sync::Notify;

/// Shared by the MCP server task and every tool call in the process.
pub struct IndexBuildGate {
    slots: Mutex<HashMap<String, Arc<Slot>>>,
}

struct Slot {
    notify: Notify,
    state: Mutex<SlotState>,
}

#[derive(Clone)]
enum SlotState {
    Running,
    Finished(Result<(), String>),
}

/// `enter` either makes this caller responsible for the build or waits for
/// the caller that already is.
pub enum GateRole {
    Leader(BuildLease),
    Joined(Result<(), String>),
}

/// Held by the leader until [`BuildLease::finish`]. Dropping it without
/// `finish` wakes joiners with an error so a cancelled build cannot stall them.
pub struct BuildLease {
    key: String,
    slot: Arc<Slot>,
    gate: Weak<IndexBuildGate>,
    finished: bool,
}

impl IndexBuildGate {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            slots: Mutex::new(HashMap::new()),
        })
    }

    /// `key` is `{connection_id}\\0{database}`, matching the index directory.
    pub async fn enter(self: &Arc<Self>, key: &str) -> GateRole {
        // The mutex guard must not live in this future: `MutexGuard` is not
        // `Send`, and tool tasks are spawned.
        match self.reserve(key) {
            Reserved::Wait(slot) => GateRole::Joined(wait_for_slot(&slot).await),
            Reserved::Lead(lease) => GateRole::Leader(lease),
        }
    }

    fn reserve(self: &Arc<Self>, key: &str) -> Reserved {
        let mut slots = self.slots.lock().unwrap_or_else(|err| err.into_inner());
        if let Some(slot) = slots.get(key).cloned() {
            return Reserved::Wait(slot);
        }
        let slot = Arc::new(Slot {
            notify: Notify::new(),
            state: Mutex::new(SlotState::Running),
        });
        slots.insert(key.to_owned(), Arc::clone(&slot));
        Reserved::Lead(BuildLease {
            key: key.to_owned(),
            slot,
            gate: Arc::downgrade(self),
            finished: false,
        })
    }
}

enum Reserved {
    Wait(Arc<Slot>),
    Lead(BuildLease),
}

impl Default for IndexBuildGate {
    fn default() -> Self {
        Self {
            slots: Mutex::new(HashMap::new()),
        }
    }
}

async fn wait_for_slot(slot: &Slot) -> Result<(), String> {
    loop {
        // Register interest before reading state so a finish that lands in
        // between cannot be lost.
        let notified = slot.notify.notified();
        tokio::pin!(notified);
        if let SlotState::Finished(result) = slot
            .state
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
        {
            return result;
        }
        notified.await;
    }
}

impl BuildLease {
    pub fn finish(mut self, result: Result<(), String>) {
        self.publish(result);
    }

    fn publish(&mut self, result: Result<(), String>) {
        if self.finished {
            return;
        }
        {
            let mut state = self
                .slot
                .state
                .lock()
                .unwrap_or_else(|err| err.into_inner());
            if !matches!(*state, SlotState::Finished(_)) {
                *state = SlotState::Finished(result);
            }
        }
        self.slot.notify.notify_waiters();
        self.finished = true;
        let Some(gate) = self.gate.upgrade() else {
            return;
        };
        let mut slots = gate.slots.lock().unwrap_or_else(|err| err.into_inner());
        if slots
            .get(&self.key)
            .is_some_and(|current| Arc::ptr_eq(current, &self.slot))
        {
            slots.remove(&self.key);
        }
    }
}

impl Drop for BuildLease {
    fn drop(&mut self) {
        if !self.finished {
            self.publish(Err("index build was cancelled before it finished".into()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn joiners_await_the_leader_instead_of_starting_a_second_build() {
        let gate = IndexBuildGate::new();
        let GateRole::Leader(lease) = gate.enter("conn\0db").await else {
            panic!("first enter must lead");
        };
        let gate_for_joiner = Arc::clone(&gate);
        let joined = tokio::spawn(async move {
            match gate_for_joiner.enter("conn\0db").await {
                GateRole::Joined(result) => result,
                GateRole::Leader(_) => panic!("second enter must join"),
            }
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        lease.finish(Ok(()));
        assert!(joined.await.unwrap().is_ok());

        // A finished build is removed so a later rebuild can lead again.
        assert!(matches!(gate.enter("conn\0db").await, GateRole::Leader(_)));
    }

    #[tokio::test]
    async fn dropped_lease_unblocks_joiners_with_an_error() {
        let gate = IndexBuildGate::new();
        let GateRole::Leader(lease) = gate.enter("conn\0db").await else {
            panic!("first enter must lead");
        };
        let gate_for_joiner = Arc::clone(&gate);
        let joined = tokio::spawn(async move {
            match gate_for_joiner.enter("conn\0db").await {
                GateRole::Joined(result) => result,
                GateRole::Leader(_) => panic!("second enter must join"),
            }
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        drop(lease);
        let message = joined.await.unwrap().unwrap_err();
        assert!(message.contains("cancelled"), "{message}");
    }
}
