//! Display order for page thumbnails, separate from PDF source page indices.

use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageOrder {
    sources: Vec<u32>,
    slots: Vec<u32>,
}

impl PageOrder {
    pub fn reset(&mut self, page_count: u32) {
        self.sources = (0..page_count).collect();
        self.slots = (0..page_count).collect();
    }

    pub fn source_at(&self, slot: u32) -> u32 {
        self.sources.get(slot as usize).copied().unwrap_or(slot)
    }

    pub fn slot_of(&self, source: u32) -> Option<u32> {
        self.slots
            .get(source as usize)
            .filter(|slot| **slot < self.sources.len() as u32)
            .copied()
    }

    pub fn move_source(&mut self, source: u32, to_slot: u32) -> bool {
        let Some(from) = self.slot_of(source) else {
            return false;
        };
        self.move_slot(from, to_slot)
    }

    pub fn move_slot(&mut self, from: u32, to: u32) -> bool {
        if self.sources.is_empty() || from == to || from >= self.sources.len() as u32 {
            return false;
        }

        let to = to.min(self.sources.len() as u32 - 1);
        let source = self.sources.remove(from as usize);
        self.sources.insert(to as usize, source);

        for (slot, source) in self.sources.iter().copied().enumerate() {
            self.slots[source as usize] = slot as u32;
        }
        true
    }

    pub fn insert_sources(&mut self, first: u32, count: u32, at: u32) {
        let at = (at as usize).min(self.sources.len());
        self.slots
            .resize(self.slots.len().max((first + count) as usize), u32::MAX);
        self.sources.splice(at..at, first..first + count);
        self.slots.fill(u32::MAX);
        for (slot, source) in self.sources.iter().copied().enumerate() {
            self.slots[source as usize] = slot as u32;
        }
    }

    pub fn sources_in(&self, from: u32, to: u32) -> Vec<u32> {
        let first = from.min(to) as usize;
        let last = (to.max(from).saturating_add(1) as usize).min(self.sources.len());
        self.sources[first..last].to_vec()
    }

    pub fn retain(&mut self, mut keep: impl FnMut(u32) -> bool) {
        self.sources.retain(|source| keep(*source));
        self.slots.fill(u32::MAX);

        for (slot, source) in self.sources.iter().copied().enumerate() {
            self.slots[source as usize] = slot as u32;
        }
    }

    pub fn as_sources(&self) -> &[u32] {
        &self.sources
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditState {
    pub order: PageOrder,
    rotations: BTreeMap<u32, u8>,
}

impl EditState {
    pub fn identity(page_count: u32) -> Self {
        let mut order = PageOrder::default();
        order.reset(page_count);
        Self {
            order,
            rotations: BTreeMap::new(),
        }
    }

    pub fn rotate_slots(&mut self, slots: &[u32], quarter_turns: u8) {
        for slot in slots {
            let source = self.order.source_at(*slot);
            let rotation = self.rotations.entry(source).or_default();
            *rotation = (*rotation + quarter_turns) % 4;
            if *rotation == 0 {
                self.rotations.remove(&source);
            }
        }
    }

    pub fn delete_slots(&mut self, slots: &[u32]) {
        let removed: Vec<_> = slots
            .iter()
            .map(|slot| self.order.source_at(*slot))
            .collect();
        self.order.retain(|source| !removed.contains(&source));
        self.rotations.retain(|source, _| !removed.contains(source));
    }

    pub fn rotations(&self) -> impl Iterator<Item = (u32, u8)> + '_ {
        self.rotations
            .iter()
            .map(|(source, turns)| (*source, *turns))
    }
}

#[derive(Debug, Clone)]
pub struct EditHistory {
    past: Vec<EditState>,
    present: EditState,
    future: Vec<EditState>,
    saved_pages: u32,
}

impl Default for EditHistory {
    fn default() -> Self {
        Self {
            past: Vec::new(),
            present: EditState::identity(0),
            future: Vec::new(),
            saved_pages: 0,
        }
    }
}

impl EditHistory {
    const LIMIT: usize = 64;

    pub fn reset(&mut self, page_count: u32) {
        self.saved_pages = page_count;
        self.past.clear();
        self.present = EditState::identity(page_count);
        self.future.clear();
    }

    pub fn current(&self) -> &EditState {
        &self.present
    }

    pub fn dirty(&self) -> bool {
        !self
            .present
            .order
            .as_sources()
            .iter()
            .copied()
            .eq(0..self.saved_pages)
            || !self.present.rotations.is_empty()
    }

    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    pub fn commit(&mut self, next: EditState) -> bool {
        if next == self.present {
            return false;
        }

        self.past.push(self.present.clone());
        if self.past.len() > Self::LIMIT {
            self.past.remove(0);
        }
        self.present = next;
        self.future.clear();
        true
    }

    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.past.pop() else {
            return false;
        };
        self.future.push(self.present.clone());
        self.present = previous;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.future.pop() else {
            return false;
        };
        self.past.push(self.present.clone());
        self.present = next;
        true
    }

    pub fn restore(&mut self, snapshot: EditHistory) {
        *self = snapshot;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reorder_keeps_source_and_slot_mappings_in_sync() {
        let mut order = PageOrder::default();
        order.reset(3);
        order.move_source(2, 0);

        assert_eq!(order.as_sources(), &[2, 0, 1]);
        assert_eq!(order.source_at(0), 2);
        assert_eq!(order.slot_of(0), Some(1));
        assert_eq!(order.slot_of(2), Some(0));
        assert_eq!(order.sources_in(0, 2), [2, 0, 1]);
    }

    #[test]
    fn reorder_clamps_the_destination() {
        let mut order = PageOrder::default();
        order.reset(3);
        order.move_source(0, 99);

        assert_eq!(order.as_sources(), &[1, 2, 0]);
    }

    #[test]
    fn deleting_pages_repairs_the_reverse_mapping() {
        let mut order = PageOrder::default();
        order.reset(3);
        order.retain(|source| source != 0);

        assert_eq!(order.as_sources(), &[1, 2]);
        assert_eq!(order.source_at(0), 1);
        assert_eq!(order.slot_of(0), None);
        assert_eq!(order.slot_of(1), Some(0));
        assert_eq!(order.slot_of(2), Some(1));
    }

    #[test]
    fn deleting_trailing_pages_keeps_edit_history_dirty() {
        let mut history = EditHistory::default();
        history.reset(3);

        let mut deleted = history.current().clone();
        deleted.delete_slots(&[2]);
        assert!(history.commit(deleted));
        assert_eq!(history.current().order.as_sources(), &[0, 1]);
        assert!(history.dirty());

        assert!(history.undo());
        assert!(!history.dirty());
    }

    #[test]
    fn edit_history_restores_order_rotation_and_deletion() {
        let mut history = EditHistory::default();
        history.reset(3);

        let mut rotated = history.current().clone();
        rotated.rotate_slots(&[2], 1);
        assert!(history.commit(rotated));

        let mut deleted = history.current().clone();
        deleted.delete_slots(&[0, 2]);
        assert!(history.commit(deleted));
        assert_eq!(history.current().order.as_sources(), &[1]);
        assert!(history.dirty());

        assert!(history.undo());
        assert_eq!(history.current().order.as_sources(), &[0, 1, 2]);
        assert_eq!(history.current().rotations().collect::<Vec<_>>(), [(2, 1)]);
        assert!(history.undo());
        assert!(!history.dirty());
        assert!(!history.can_undo());

        assert!(history.redo());
        assert!(history.redo());
        assert_eq!(history.current().order.as_sources(), &[1]);
        assert!(!history.can_redo());
    }
}
#[test]
fn appended_import_is_dirty_and_one_undo_removes_it() {
    let mut history = EditHistory::default();
    history.reset(3);
    let mut next = history.current().clone();
    next.order.insert_sources(3, 2, 3);
    history.commit(next);
    assert!(history.dirty());
    assert_eq!(history.current().order.slot_of(4), Some(4));
    assert!(history.undo());
    assert!(!history.dirty());
    assert!(history.redo());
    let mut next = history.current().clone();
    next.delete_slots(&[3, 4]);
    history.commit(next);
    assert!(!history.dirty());
}
