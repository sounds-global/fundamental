/// A platform hook's decision on a user-initiated exit request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use = "an ExitAction only takes effect when returned from on_exit_requested"]
pub enum ExitAction {
    /// Let the exit proceed (the default).
    Exit,
    /// Keep the app running — the classic close-to-tray pattern.
    ///
    /// Only user-initiated exits can be prevented.
    Prevent,
}

#[cfg(test)]
mod tests {
    use super::ExitAction;

    #[test]
    fn actions_are_distinct_and_copyable() {
        let action = ExitAction::Prevent;
        let copy = action;
        assert_eq!(action, copy);
        assert_ne!(ExitAction::Exit, ExitAction::Prevent);
    }
}
