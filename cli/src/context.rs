//! The shared [`CommandContext`] passed to every `fun` command handler.

use crate::ui::Ui;

/// Shared context for all command handlers: the configured [`Ui`] and nothing else, so `fun new` stays fast.
pub(crate) struct CommandContext {
    ui: Ui,
}

impl CommandContext {
    pub(crate) fn new(quiet: bool) -> Self {
        Self { ui: Ui::new(quiet) }
    }

    pub(crate) fn ui(&self) -> &Ui {
        &self.ui
    }
}
