//! Undo and redo (`Editor::snapshot`, `undo`, `redo`, `beginCoalescedEdit`,
//! `endCoalescedEdit`).
//!
//! The stacks hold whole projects as `Project::toJson()`. A snapshot taken with a coalescing key
//! is skipped while the last snapshot had the same key, so a drag or a slider makes one undo
//! step. The key stays after the edit: two separate trims of the same clip with nothing in
//! between coalesce into one step, as in the C++. `undo` ends a coalesced edit and `redo` does
//! not.

use crate::compat::qjson::Object;

/// The most undo steps kept; the oldest goes first.
pub const UNDO_LIMIT: usize = 100;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct History {
    pub undo: Vec<Object>,
    pub redo: Vec<Object>,
    pub coalesce_key: String,
    pub coalesce_snapshot_taken: bool,
}

impl History {
    /// `Editor::snapshot(coalesceKey)` without the signal: true when a snapshot was pushed (and
    /// `historyChanged` is due).
    pub fn snapshot(&mut self, coalesce_key: &str, current: Object) -> bool {
        if !coalesce_key.is_empty() && self.coalesce_key == coalesce_key && self.coalesce_snapshot_taken {
            return false;
        }
        self.undo.push(current);
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
        if coalesce_key.is_empty() {
            self.coalesce_key.clear();
            self.coalesce_snapshot_taken = false;
        } else {
            self.coalesce_key = coalesce_key.to_owned();
            self.coalesce_snapshot_taken = true;
        }
        true
    }

    pub fn begin_coalesced_edit(&mut self, key: &str) {
        self.coalesce_key = key.to_owned();
        self.coalesce_snapshot_taken = false;
    }

    pub fn end_coalesced_edit(&mut self) {
        self.coalesce_key.clear();
        self.coalesce_snapshot_taken = false;
    }

    /// The key a coalescing edit uses: the open coalesced edit's, or `fallback`.
    pub fn key_or(&self, fallback: &str) -> String {
        if self.coalesce_key.is_empty() { fallback.to_owned() } else { self.coalesce_key.clone() }
    }
}
