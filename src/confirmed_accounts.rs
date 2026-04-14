use {
    crate::{SlotStatus, UpdateAccountEvent},
    log::{debug, error, warn},
    solana_pubkey::Pubkey,
    std::collections::{HashMap, HashSet},
};

const STALE_UNCONFIRMED_SLOT_RETENTION: u64 = 4096;

#[derive(Debug, Default)]
struct SlotNode {
    parent: Option<u64>,
    children: HashSet<u64>,
    status: Option<SlotStatus>,
    is_confirmed: bool,
    is_dead: bool,
    emitted: bool,
}

#[derive(Debug, Default)]
pub struct ConfirmedAccounts {
    pending_updates: HashMap<u64, HashMap<Pubkey, UpdateAccountEvent>>,
    slots: HashMap<u64, SlotNode>,
    confirmed_slots: HashSet<u64>,
    dead_slots: HashSet<u64>,
    highest_confirmed_slot: Option<u64>,
    highest_observed_slot: u64,
}

#[derive(Debug, Default)]
pub struct SlotTransitionResult {
    pub newly_confirmed_slots: Vec<u64>,
    pub confirmed_updates: Vec<UpdateAccountEvent>,
    pub dead_slots_cleaned: Vec<u64>,
    pub stale_slots_evicted: Vec<u64>,
}

impl ConfirmedAccounts {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_account(&mut self, ev: UpdateAccountEvent) {
        self.update_highest_observed_slot(ev.slot);

        if self.dead_slots.contains(&ev.slot) {
            return;
        }

        let pubkey = match Pubkey::try_from(ev.pubkey.as_slice()) {
            Ok(pubkey) => pubkey,
            Err(error) => {
                error!(
                    "dropping account update for slot {} with invalid pubkey bytes: {error}",
                    ev.slot
                );
                return;
            }
        };

        let slot = ev.slot;
        let node = self.slot_node_mut(slot);
        if node.is_dead || node.emitted {
            return;
        }

        self.pending_updates
            .entry(slot)
            .or_default()
            .insert(pubkey, ev);
    }

    pub fn record_slot_status(
        &mut self,
        slot: u64,
        parent: Option<u64>,
        status: SlotStatus,
    ) -> SlotTransitionResult {
        self.update_highest_observed_slot(slot);

        if self.dead_slots.contains(&slot) && !matches!(status, SlotStatus::Dead) {
            return SlotTransitionResult::default();
        }

        {
            let node = self.slot_node_mut(slot);
            node.status = Some(status);
        }

        if let Some(parent_slot) = parent {
            let existing_parent = self.slot_node_mut(slot).parent;
            match existing_parent {
                Some(known_parent) if known_parent != parent_slot => {
                    error!(
                        "slot {} received conflicting parents: keeping {}, ignoring {}",
                        slot, known_parent, parent_slot
                    );
                }
                None => {
                    self.slot_node_mut(slot).parent = Some(parent_slot);
                }
                Some(_) => {}
            }
            self.mark_parent_child(parent_slot, slot);
        }

        let mut result = SlotTransitionResult::default();
        if matches!(status, SlotStatus::Dead) {
            result.dead_slots_cleaned = self.cleanup_dead_subtree(slot);
        } else if Self::is_confirming_status(status) {
            let mut current = Some(slot);
            while let Some(current_slot) = current {
                let Some(node) = self.slots.get(&current_slot) else {
                    break;
                };
                if node.is_dead || node.is_confirmed {
                    break;
                }
                let parent_slot = node.parent;
                self.mark_confirmed(
                    current_slot,
                    &mut result.newly_confirmed_slots,
                    &mut result.confirmed_updates,
                );
                current = parent_slot;
            }
        }

        result.stale_slots_evicted = self.evict_stale_unconfirmed_slots();
        result
    }

    #[cfg(test)]
    fn pending_count_for(&self, slot: u64) -> usize {
        self.pending_updates.get(&slot).map_or(0, HashMap::len)
    }

    #[cfg(test)]
    fn has_slot(&self, slot: u64) -> bool {
        self.slots.contains_key(&slot)
    }

    #[cfg(test)]
    fn is_confirmed(&self, slot: u64) -> bool {
        self.slots.get(&slot).is_some_and(|node| node.is_confirmed)
    }

    fn slot_node_mut(&mut self, slot: u64) -> &mut SlotNode {
        self.slots.entry(slot).or_default()
    }

    fn update_highest_observed_slot(&mut self, slot: u64) {
        self.highest_observed_slot = self.highest_observed_slot.max(slot);
    }

    fn mark_parent_child(&mut self, parent: u64, child: u64) {
        self.slot_node_mut(parent).children.insert(child);
    }

    fn mark_confirmed(
        &mut self,
        slot: u64,
        newly_confirmed_slots: &mut Vec<u64>,
        confirmed_updates: &mut Vec<UpdateAccountEvent>,
    ) {
        let parent = {
            let node = self.slot_node_mut(slot);
            if node.is_dead || node.is_confirmed {
                return;
            }
            node.is_confirmed = true;
            node.emitted = true;
            node.parent
        };

        self.confirmed_slots.insert(slot);
        self.highest_confirmed_slot = Some(
            self.highest_confirmed_slot
                .map_or(slot, |current| current.max(slot)),
        );
        newly_confirmed_slots.push(slot);

        let drained_updates = self.drain_slot_updates(slot);
        if !drained_updates.is_empty() {
            debug!(
                "slot {} confirmed with {} buffered account updates",
                slot,
                drained_updates.len()
            );
        }
        confirmed_updates.extend(drained_updates);

        if let Some(parent_slot) = parent {
            debug!(
                "slot {} confirmation allows ancestor inference toward parent {}",
                slot, parent_slot
            );
        }
    }

    fn drain_slot_updates(&mut self, slot: u64) -> Vec<UpdateAccountEvent> {
        self.pending_updates
            .remove(&slot)
            .map(|updates| updates.into_values().collect())
            .unwrap_or_default()
    }

    fn cleanup_dead_subtree(&mut self, slot: u64) -> Vec<u64> {
        self.slot_node_mut(slot).is_dead = true;

        let mut stack = vec![slot];
        let mut visited = HashSet::new();
        let mut subtree = Vec::new();

        while let Some(current) = stack.pop() {
            if !visited.insert(current) {
                continue;
            }
            subtree.push(current);

            if let Some(node) = self.slots.get(&current) {
                stack.extend(node.children.iter().copied());
            }
        }

        for dead_slot in &subtree {
            self.dead_slots.insert(*dead_slot);
            self.pending_updates.remove(dead_slot);
        }

        for dead_slot in &subtree {
            self.remove_slot(*dead_slot);
        }

        if !subtree.is_empty() {
            debug!(
                "removed dead subtree rooted at slot {}: {:?}",
                slot, subtree
            );
        }

        subtree
    }

    fn evict_stale_unconfirmed_slots(&mut self) -> Vec<u64> {
        if self.highest_observed_slot <= STALE_UNCONFIRMED_SLOT_RETENTION {
            return Vec::new();
        }

        let cutoff = self.highest_observed_slot - STALE_UNCONFIRMED_SLOT_RETENTION;
        let victims: Vec<u64> = self
            .slots
            .iter()
            .filter_map(|(&slot, node)| {
                if slot >= cutoff || node.is_confirmed || node.status == Some(SlotStatus::Rooted) {
                    return None;
                }
                Some(slot)
            })
            .collect();

        for slot in &victims {
            warn!(
                "evicting stale unconfirmed slot {} (cutoff {}, highest observed {})",
                slot, cutoff, self.highest_observed_slot
            );
            self.pending_updates.remove(slot);
        }

        for slot in &victims {
            self.remove_slot(*slot);
        }

        self.dead_slots.retain(|slot| *slot >= cutoff);

        victims
    }

    fn remove_slot(&mut self, slot: u64) {
        let Some(node) = self.slots.remove(&slot) else {
            return;
        };

        self.pending_updates.remove(&slot);
        self.confirmed_slots.remove(&slot);

        if let Some(parent) = node.parent
            && let Some(parent_node) = self.slots.get_mut(&parent)
        {
            parent_node.children.remove(&slot);
        }

        for child in node.children {
            if let Some(child_node) = self.slots.get_mut(&child)
                && child_node.parent == Some(slot)
            {
                child_node.parent = None;
            }
        }
    }

    fn is_confirming_status(status: SlotStatus) -> bool {
        matches!(status, SlotStatus::Confirmed | SlotStatus::Rooted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted_slots<I>(slots: I) -> Vec<u64>
    where
        I: IntoIterator<Item = u64>,
    {
        let mut slots: Vec<u64> = slots.into_iter().collect();
        slots.sort_unstable();
        slots
    }

    fn account_event(slot: u64, pubkey_byte: u8, write_version: u64) -> UpdateAccountEvent {
        UpdateAccountEvent {
            slot,
            pubkey: vec![pubkey_byte; 32],
            lamports: write_version,
            owner: vec![9; 32],
            executable: false,
            rent_epoch: 0,
            data: vec![write_version as u8],
            write_version,
            txn_signature: None,
            data_version: write_version as u32,
            is_startup: false,
            account_age: 0,
        }
    }

    #[test]
    fn buffers_latest_update_per_slot_pubkey() {
        let mut confirmed = ConfirmedAccounts::new();

        confirmed.record_account(account_event(10, 1, 1));
        confirmed.record_account(account_event(10, 1, 2));

        assert_eq!(confirmed.pending_count_for(10), 1);
        let result = confirmed.record_slot_status(10, None, SlotStatus::Confirmed);
        assert_eq!(result.newly_confirmed_slots, vec![10]);
        assert_eq!(result.confirmed_updates.len(), 1);
        assert_eq!(
            sorted_slots(result.confirmed_updates.iter().map(|event| event.slot)),
            vec![10]
        );
        assert!(result.dead_slots_cleaned.is_empty());
        assert!(result.stale_slots_evicted.is_empty());
        assert_eq!(result.confirmed_updates[0].write_version, 2);
        assert_eq!(confirmed.pending_count_for(10), 0);
        assert!(confirmed.is_confirmed(10));
    }

    #[test]
    fn confirmed_slot_drains_updates_once() {
        let mut confirmed = ConfirmedAccounts::new();
        confirmed.record_account(account_event(11, 2, 1));

        let first = confirmed.record_slot_status(11, None, SlotStatus::Confirmed);
        let second = confirmed.record_slot_status(11, None, SlotStatus::Confirmed);

        assert_eq!(first.newly_confirmed_slots, vec![11]);
        assert_eq!(first.confirmed_updates.len(), 1);
        assert_eq!(
            sorted_slots(first.confirmed_updates.iter().map(|event| event.slot)),
            vec![11]
        );
        assert!(first.dead_slots_cleaned.is_empty());
        assert!(first.stale_slots_evicted.is_empty());
        assert_eq!(confirmed.pending_count_for(11), 0);
        assert!(confirmed.is_confirmed(11));
        assert!(second.confirmed_updates.is_empty());
        assert_eq!(second.newly_confirmed_slots.len(), 0);
        assert!(second.dead_slots_cleaned.is_empty());
        assert!(second.stale_slots_evicted.is_empty());
        assert_eq!(confirmed.pending_count_for(11), 0);
    }

    #[test]
    fn rooted_slot_drains_updates_once() {
        let mut confirmed = ConfirmedAccounts::new();
        confirmed.record_account(account_event(12, 3, 1));

        let result = confirmed.record_slot_status(12, None, SlotStatus::Rooted);

        assert_eq!(result.newly_confirmed_slots, vec![12]);
        assert_eq!(result.confirmed_updates.len(), 1);
        assert_eq!(
            sorted_slots(result.confirmed_updates.iter().map(|event| event.slot)),
            vec![12]
        );
        assert!(result.dead_slots_cleaned.is_empty());
        assert!(result.stale_slots_evicted.is_empty());
        assert_eq!(confirmed.pending_count_for(12), 0);
        assert!(confirmed.is_confirmed(12));
    }

    #[test]
    fn confirmed_child_confirms_known_ancestors() {
        let mut confirmed = ConfirmedAccounts::new();
        confirmed.record_account(account_event(20, 4, 1));
        confirmed.record_account(account_event(21, 5, 1));
        confirmed.record_account(account_event(22, 6, 1));
        confirmed.record_slot_status(21, Some(20), SlotStatus::Processed);
        confirmed.record_slot_status(22, Some(21), SlotStatus::Processed);

        let result = confirmed.record_slot_status(22, None, SlotStatus::Confirmed);

        assert_eq!(result.newly_confirmed_slots, vec![22, 21, 20]);
        assert_eq!(result.confirmed_updates.len(), 3);
        assert_eq!(
            sorted_slots(result.confirmed_updates.iter().map(|event| event.slot)),
            vec![20, 21, 22]
        );
        assert!(result.dead_slots_cleaned.is_empty());
        assert!(result.stale_slots_evicted.is_empty());
        assert_eq!(confirmed.pending_count_for(20), 0);
        assert_eq!(confirmed.pending_count_for(21), 0);
        assert_eq!(confirmed.pending_count_for(22), 0);
        assert!(confirmed.is_confirmed(20));
        assert!(confirmed.is_confirmed(21));
        assert!(confirmed.is_confirmed(22));
    }

    #[test]
    fn confirmed_with_missing_parent_stops_inference() {
        let mut confirmed = ConfirmedAccounts::new();
        confirmed.record_account(account_event(30, 7, 1));

        let result = confirmed.record_slot_status(30, None, SlotStatus::Confirmed);

        assert_eq!(result.newly_confirmed_slots, vec![30]);
        assert_eq!(result.confirmed_updates.len(), 1);
        assert_eq!(
            sorted_slots(result.confirmed_updates.iter().map(|event| event.slot)),
            vec![30]
        );
        assert!(result.dead_slots_cleaned.is_empty());
        assert!(result.stale_slots_evicted.is_empty());
        assert_eq!(confirmed.pending_count_for(30), 0);
        assert!(confirmed.is_confirmed(30));
    }

    #[test]
    fn confirmed_event_with_parent_none_uses_previous_parent_link() {
        let mut confirmed = ConfirmedAccounts::new();
        confirmed.record_account(account_event(40, 8, 1));
        confirmed.record_account(account_event(41, 9, 1));
        confirmed.record_slot_status(41, Some(40), SlotStatus::Processed);

        let result = confirmed.record_slot_status(41, None, SlotStatus::Confirmed);

        assert_eq!(result.newly_confirmed_slots, vec![41, 40]);
        assert_eq!(result.confirmed_updates.len(), 2);
        assert_eq!(
            sorted_slots(result.confirmed_updates.iter().map(|event| event.slot)),
            vec![40, 41]
        );
        assert!(result.dead_slots_cleaned.is_empty());
        assert!(result.stale_slots_evicted.is_empty());
        assert_eq!(confirmed.pending_count_for(40), 0);
        assert_eq!(confirmed.pending_count_for(41), 0);
        assert!(confirmed.is_confirmed(40));
        assert!(confirmed.is_confirmed(41));
    }

    #[test]
    fn dead_slot_removes_own_updates() {
        let mut confirmed = ConfirmedAccounts::new();
        confirmed.record_account(account_event(50, 10, 1));

        let result = confirmed.record_slot_status(50, None, SlotStatus::Dead);

        assert_eq!(result.dead_slots_cleaned, vec![50]);
        assert!(result.confirmed_updates.is_empty());
        assert!(result.newly_confirmed_slots.is_empty());
        assert!(result.stale_slots_evicted.is_empty());
        assert!(!confirmed.has_slot(50));
        assert_eq!(confirmed.pending_count_for(50), 0);
        assert!(!confirmed.is_confirmed(50));
    }

    #[test]
    fn dead_slot_removes_known_descendants() {
        let mut confirmed = ConfirmedAccounts::new();
        confirmed.record_account(account_event(60, 11, 1));
        confirmed.record_account(account_event(61, 12, 1));
        confirmed.record_slot_status(61, Some(60), SlotStatus::Processed);

        let result = confirmed.record_slot_status(60, None, SlotStatus::Dead);

        assert_eq!(sorted_slots(result.dead_slots_cleaned), vec![60, 61]);
        assert!(result.confirmed_updates.is_empty());
        assert!(result.newly_confirmed_slots.is_empty());
        assert!(result.stale_slots_evicted.is_empty());
        assert!(!confirmed.has_slot(60));
        assert!(!confirmed.has_slot(61));
        assert_eq!(confirmed.pending_count_for(60), 0);
        assert_eq!(confirmed.pending_count_for(61), 0);
        assert!(!confirmed.is_confirmed(60));
        assert!(!confirmed.is_confirmed(61));
    }

    #[test]
    fn dead_slot_prevents_later_emission() {
        let mut confirmed = ConfirmedAccounts::new();
        confirmed.record_account(account_event(70, 13, 1));
        confirmed.record_slot_status(70, None, SlotStatus::Dead);

        let result = confirmed.record_slot_status(70, None, SlotStatus::Confirmed);

        assert!(result.confirmed_updates.is_empty());
        assert!(result.newly_confirmed_slots.is_empty());
        assert!(result.dead_slots_cleaned.is_empty());
        assert!(result.stale_slots_evicted.is_empty());
        assert_eq!(confirmed.pending_count_for(70), 0);
        assert!(!confirmed.has_slot(70));
        assert!(!confirmed.is_confirmed(70));
    }

    #[test]
    fn repeated_confirmed_status_is_idempotent() {
        let mut confirmed = ConfirmedAccounts::new();
        confirmed.record_account(account_event(80, 14, 1));

        let _ = confirmed.record_slot_status(80, None, SlotStatus::Confirmed);
        let result = confirmed.record_slot_status(80, None, SlotStatus::Rooted);

        assert!(result.confirmed_updates.is_empty());
        assert!(result.newly_confirmed_slots.is_empty());
        assert!(result.dead_slots_cleaned.is_empty());
        assert!(result.stale_slots_evicted.is_empty());
        assert_eq!(confirmed.pending_count_for(80), 0);
        assert!(confirmed.is_confirmed(80));
    }

    #[test]
    fn stale_fallback_evicts_old_unconfirmed_slots() {
        let mut confirmed = ConfirmedAccounts::new();
        confirmed.record_account(account_event(1, 15, 1));

        let result = confirmed.record_slot_status(
            STALE_UNCONFIRMED_SLOT_RETENTION + 10,
            None,
            SlotStatus::Processed,
        );

        assert_eq!(result.stale_slots_evicted, vec![1]);
        assert!(result.confirmed_updates.is_empty());
        assert!(result.newly_confirmed_slots.is_empty());
        assert!(result.dead_slots_cleaned.is_empty());
        assert!(!confirmed.has_slot(1));
        assert_eq!(confirmed.pending_count_for(1), 0);
        assert!(!confirmed.is_confirmed(1));
    }

    #[test]
    fn stale_fallback_does_not_evict_confirmed_slots() {
        let mut confirmed = ConfirmedAccounts::new();
        confirmed.record_account(account_event(2, 16, 1));
        let _ = confirmed.record_slot_status(2, None, SlotStatus::Confirmed);

        let result = confirmed.record_slot_status(
            STALE_UNCONFIRMED_SLOT_RETENTION + 10,
            None,
            SlotStatus::Processed,
        );

        assert!(result.stale_slots_evicted.is_empty());
        assert!(result.confirmed_updates.is_empty());
        assert!(result.newly_confirmed_slots.is_empty());
        assert!(result.dead_slots_cleaned.is_empty());
        assert!(confirmed.has_slot(2));
        assert!(confirmed.is_confirmed(2));
        assert_eq!(confirmed.pending_count_for(2), 0);
    }
}
