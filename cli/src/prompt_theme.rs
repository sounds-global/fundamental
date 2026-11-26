//! A [`dialoguer`] theme that lists a multi-select's answers on their own lines below the question.

use std::fmt;

use dialoguer::theme::{ColorfulTheme, Theme};

/// [`ColorfulTheme`]'s look, with a multi-select's chosen values listed below the question.
pub(crate) struct PromptTheme(ColorfulTheme);

impl PromptTheme {
    pub(crate) fn new(inner: ColorfulTheme) -> Self {
        Self(inner)
    }

    fn write_question(&self, f: &mut dyn fmt::Write, prompt: &str) -> fmt::Result {
        if !prompt.is_empty() {
            write!(
                f,
                "{} {}",
                &self.0.success_prefix,
                self.0.prompt_style.apply_to(prompt),
            )?;
        }
        Ok(())
    }
}

impl Theme for PromptTheme {
    fn format_prompt(&self, f: &mut dyn fmt::Write, prompt: &str) -> fmt::Result {
        self.0.format_prompt(f, prompt)
    }

    fn format_error(&self, f: &mut dyn fmt::Write, err: &str) -> fmt::Result {
        self.0.format_error(f, err)
    }

    fn format_input_prompt(
        &self,
        f: &mut dyn fmt::Write,
        prompt: &str,
        default: Option<&str>,
    ) -> fmt::Result {
        self.0.format_input_prompt(f, prompt, default)
    }

    fn format_confirm_prompt(
        &self,
        f: &mut dyn fmt::Write,
        prompt: &str,
        default: Option<bool>,
    ) -> fmt::Result {
        self.0.format_confirm_prompt(f, prompt, default)
    }

    fn format_multi_select_prompt(&self, f: &mut dyn fmt::Write, prompt: &str) -> fmt::Result {
        self.0.format_multi_select_prompt(f, prompt)
    }

    fn format_select_prompt(&self, f: &mut dyn fmt::Write, prompt: &str) -> fmt::Result {
        self.0.format_select_prompt(f, prompt)
    }

    fn format_select_prompt_item(
        &self,
        f: &mut dyn fmt::Write,
        text: &str,
        active: bool,
    ) -> fmt::Result {
        self.0.format_select_prompt_item(f, text, active)
    }

    fn format_multi_select_prompt_item(
        &self,
        f: &mut dyn fmt::Write,
        text: &str,
        checked: bool,
        active: bool,
    ) -> fmt::Result {
        self.0
            .format_multi_select_prompt_item(f, text, checked, active)
    }

    // Only the multi-select recap, which can carry long labels, breaks onto its own lines.

    fn format_input_prompt_selection(
        &self,
        f: &mut dyn fmt::Write,
        prompt: &str,
        sel: &str,
    ) -> fmt::Result {
        self.0.format_input_prompt_selection(f, prompt, sel)
    }

    fn format_confirm_prompt_selection(
        &self,
        f: &mut dyn fmt::Write,
        prompt: &str,
        selection: Option<bool>,
    ) -> fmt::Result {
        self.0.format_confirm_prompt_selection(f, prompt, selection)
    }

    fn format_multi_select_prompt_selection(
        &self,
        f: &mut dyn fmt::Write,
        prompt: &str,
        selections: &[&str],
    ) -> fmt::Result {
        self.write_question(f, prompt)?;
        if selections.is_empty() {
            return write!(
                f,
                "\n  {} {}",
                &self.0.success_suffix,
                self.0.hint_style.apply_to("none")
            );
        }
        for sel in selections {
            write!(
                f,
                "\n  {} {}",
                &self.0.success_suffix,
                self.0.values_style.apply_to(sel),
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> PromptTheme {
        PromptTheme::new(ColorfulTheme::default())
    }

    #[test]
    fn input_recap_stays_inline() {
        let mut out = String::new();
        theme()
            .format_input_prompt_selection(&mut out, "Project name", "todo")
            .unwrap();
        assert!(
            !out.contains('\n'),
            "input recap must stay on one line: {out:?}"
        );
        assert!(
            out.contains("Project name") && out.contains("todo"),
            "{out:?}"
        );
    }

    #[test]
    fn confirm_recap_stays_inline() {
        let mut out = String::new();
        theme()
            .format_confirm_prompt_selection(&mut out, "Create this project?", Some(true))
            .unwrap();
        assert!(
            !out.contains('\n'),
            "confirm recap must stay on one line: {out:?}"
        );
        assert!(
            out.contains("Create this project?") && out.contains("yes"),
            "{out:?}"
        );
    }

    #[test]
    fn multi_select_recap_lists_each_value_on_its_own_line() {
        let mut out = String::new();
        theme()
            .format_multi_select_prompt_selection(&mut out, "Databases", &["sql", "cql"])
            .unwrap();
        assert_eq!(out.matches('\n').count(), 2, "{out:?}");
        assert!(out.contains("sql") && out.contains("cql"), "{out:?}");
    }

    #[test]
    fn multi_select_recap_shows_none_when_nothing_is_picked() {
        let mut out = String::new();
        theme()
            .format_multi_select_prompt_selection(&mut out, "Databases", &[])
            .unwrap();
        let (_, answer) = out.split_once('\n').expect("recap spans two lines");
        assert!(answer.contains("none"), "{out:?}");
    }
}
