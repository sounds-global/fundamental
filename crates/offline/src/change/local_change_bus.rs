//! [`LocalChangeBus`] — fans local table writes out to per-table watchers.

use std::sync::{Mutex, MutexGuard, PoisonError};

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};

use crate::change::TableChange;

struct Watcher {
    table: String,
    sender: UnboundedSender<TableChange>,
}

/// Broadcasts [`TableChange`]s to interested watchers.
///
/// Closed watchers are pruned on the next notify; "no watcher" is success.
#[derive(Debug, Default)]
pub struct LocalChangeBus {
    watchers: Mutex<Vec<Watcher>>,
}

impl LocalChangeBus {
    /// Returns a stream of every change to `table`.
    ///
    /// The stream ends when the bus is dropped; dropping the stream unregisters it.
    #[must_use]
    pub fn watch(&self, table: impl Into<String>) -> UnboundedReceiver<TableChange> {
        let (sender, receiver) = unbounded();
        self.lock().push(Watcher {
            table: table.into(),
            sender,
        });
        receiver
    }

    /// Delivers `change` to every watcher of its table, pruning any that have gone away.
    pub fn notify(&self, change: TableChange) {
        let mut watchers = self.lock();
        watchers.retain(|watcher| {
            if watcher.sender.is_closed() {
                return false;
            }
            if watcher.table == change.table {
                watcher.sender.unbounded_send(change.clone()).is_ok()
            } else {
                true
            }
        });
    }

    /// Locks the watcher list, recovering from a poisoned lock rather than panicking.
    fn lock(&self) -> MutexGuard<'_, Vec<Watcher>> {
        self.watchers.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl std::fmt::Debug for Watcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Watcher")
            .field("table", &self.table)
            .field("closed", &self.sender.is_closed())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::change::ChangeOp;
    use crate::db::Value;

    fn change(table: &str, id: &str) -> TableChange {
        TableChange::new(table, Value::from(id.to_string()), ChangeOp::Update)
    }

    #[test]
    fn delivers_only_to_watchers_of_the_changed_table() {
        let bus = LocalChangeBus::default();
        let mut tasks = bus.watch("tasks");
        let mut lists = bus.watch("lists");

        bus.notify(change("tasks", "t1"));

        assert_eq!(tasks.try_recv().expect("open"), change("tasks", "t1"));
        assert!(lists.try_recv().is_err());
    }

    #[test]
    fn every_watcher_of_a_table_receives_the_change() {
        let bus = LocalChangeBus::default();
        let mut a = bus.watch("tasks");
        let mut b = bus.watch("tasks");

        bus.notify(change("tasks", "t1"));

        assert_eq!(a.try_recv().expect("open"), change("tasks", "t1"));
        assert_eq!(b.try_recv().expect("open"), change("tasks", "t1"));
    }

    #[test]
    fn a_dropped_watcher_is_pruned_and_does_not_break_delivery() {
        let bus = LocalChangeBus::default();
        let dropped = bus.watch("tasks");
        let mut live = bus.watch("tasks");
        drop(dropped);

        bus.notify(change("tasks", "t1"));
        assert_eq!(live.try_recv().expect("open"), change("tasks", "t1"));
        assert_eq!(bus.lock().len(), 1, "the dropped watcher must be pruned");
    }

    #[test]
    fn notify_with_no_watchers_is_a_no_op() {
        let bus = LocalChangeBus::default();
        bus.notify(change("tasks", "t1")); // must not panic
    }
}
