//! The single seam for everything the `fun` CLI prints to a terminal, through [`console`].

use std::fmt::Display;

use console::{Style, StyledObject, measure_text_width, style};
use dialoguer::theme::ColorfulTheme;
use fundamental::Error;

use crate::emit::emit;
use crate::prompt_theme::PromptTheme;

/// The brand accent (`#87C3FF`), the only colour the CLI leans on.
const ACCENT: (u8, u8, u8) = (135, 195, 255);
const BAR: &str = "▌";
const ARROW: &str = "→";
const DOT: &str = "·";

/// Renders all human-facing `fun` output in one style; stdout writers honour `--quiet`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Ui {
    quiet: bool,
}

impl Ui {
    /// Create a renderer; `quiet` suppresses every stdout writer, never errors.
    pub(crate) fn new(quiet: bool) -> Self {
        Self { quiet }
    }

    /// Through [`emit`], since `println!` panics when the reader closes the pipe (`| head -1`).
    fn say(&self, line: impl Display) {
        if !self.quiet {
            emit(&line.to_string());
        }
    }

    pub(crate) fn blank(&self) {
        self.say("");
    }

    /// Print the `▌ fundamental` wordmark that opens an interactive flow.
    pub(crate) fn wordmark(&self) {
        self.say(render_wordmark());
    }

    /// Print a section header: an accent bar and a bold title.
    pub(crate) fn section(&self, title: impl Display) {
        self.say(render_section(&title.to_string()));
    }

    /// Print an indented, dimmed detail line under a section.
    pub(crate) fn detail(&self, text: impl Display) {
        self.say(render_detail(&text.to_string()));
    }

    /// Print one index row: an unstyled, copyable key padded to `width`, then a dim description.
    pub(crate) fn entry(&self, key: &str, width: usize, description: Option<&str>) {
        self.say(render_entry(key, width, description));
    }

    /// Print a runnable shell command: an accent `→` and the command itself.
    pub(crate) fn command(&self, cmd: impl Display) {
        self.say(render_command(&cmd.to_string()));
    }

    /// Print a line of source code to paste, dimmed and indented.
    pub(crate) fn code(&self, line: impl Display) {
        self.say(render_code(&line.to_string()));
    }

    /// Print a prose note: a dim `·` and dim text, never starting with `#`.
    pub(crate) fn note(&self, text: impl Display) {
        self.say(render_note(&text.to_string()));
    }

    /// Print a single top-level status line (e.g. "nothing to do").
    pub(crate) fn status(&self, text: impl Display) {
        self.say(render_status(&text.to_string()));
    }

    /// Style a value as a positive status token (green).
    pub(crate) fn ok(&self, text: &str) -> String {
        style(text).green().to_string()
    }

    /// Style a value as an attention status token (yellow).
    pub(crate) fn warn(&self, text: &str) -> String {
        style(text).yellow().to_string()
    }

    /// Print an aligned table with a bold header row; widths ignore ANSI styling.
    pub(crate) fn table(&self, headers: &[&str], rows: &[Vec<String>]) {
        if self.quiet {
            return;
        }
        let widths = column_widths(headers, rows);
        let header_cells: Vec<String> = headers
            .iter()
            .map(|h| style(*h).bold().to_string())
            .collect();
        emit(&render_row(&header_cells, &widths));
        for row in rows {
            emit(&render_row(row, &widths));
        }
    }

    /// Render the error a command failed with to stderr, regardless of `--quiet`.
    pub(crate) fn failure(&self, err: &impl Error) {
        eprintln!("{}", render_failure(err));
    }

    /// The [`dialoguer`] theme speaking the same `▌` / `→` / `·` language as the rest of the output.
    pub(crate) fn prompt_theme() -> PromptTheme {
        let theme = ColorfulTheme {
            prompt_prefix: accent(style(BAR.to_string())),
            prompt_suffix: style("›".to_string()).dim(),
            success_prefix: accent(style(BAR.to_string())),
            success_suffix: style("›".to_string()).dim(),
            prompt_style: Style::new().bold(),
            values_style: accent_style(),
            hint_style: Style::new().dim(),
            active_item_prefix: accent(style(format!("{ARROW} "))),
            inactive_item_prefix: style("  ".to_string()).dim(),
            active_item_style: accent_style().bold(),
            checked_item_prefix: accent(style(format!("{ARROW} "))),
            unchecked_item_prefix: style(format!("{DOT} ")).dim(),
            ..ColorfulTheme::default()
        };
        PromptTheme::new(theme)
    }
}

fn accent<D>(obj: StyledObject<D>) -> StyledObject<D> {
    let (r, g, b) = ACCENT;
    obj.true_color(r, g, b)
}

fn accent_style() -> Style {
    let (r, g, b) = ACCENT;
    Style::new().true_color(r, g, b)
}

fn render_wordmark() -> String {
    format!(
        "  {} {}",
        accent(style(BAR)),
        accent(style("fundamental")).bold(),
    )
}

fn render_section(title: &str) -> String {
    format!("  {} {}", accent(style(BAR)), style(title).bold())
}

fn render_detail(text: &str) -> String {
    format!("    {}", style(text).dim())
}

fn render_entry(key: &str, width: usize, description: Option<&str>) -> String {
    let Some(description) = description else {
        return format!("      {key}");
    };
    let pad = " ".repeat(width.saturating_sub(measure_text_width(key)));
    format!("      {key}{pad}  {}", style(description).dim())
}

/// The command text stays unstyled so it is copyable.
fn render_command(cmd: &str) -> String {
    format!("    {} {cmd}", accent(style(ARROW)))
}

fn render_code(line: &str) -> String {
    format!("    {}", style(line).dim())
}

fn render_note(text: &str) -> String {
    format!("    {} {}", style(DOT).dim(), style(text).dim())
}

fn render_status(text: &str) -> String {
    format!("  {} {}", style(DOT).dim(), style(text).dim())
}

fn render_error(code: &str, message: &str) -> String {
    format!(
        "{} {message}",
        style(format!("error[{code}]:")).red().bold()
    )
}

fn render_failure(err: &impl Error) -> String {
    let message = err.to_string();
    let line = render_error(err.error_code(), &message);
    match err.developer_message() {
        Some(detail) if detail != message => format!("{line}\n{}", render_note(&detail)),
        _ => line,
    }
}

fn column_widths(headers: &[&str], rows: &[Vec<String>]) -> Vec<usize> {
    let mut widths: Vec<usize> = headers.iter().map(|h| measure_text_width(h)).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if let Some(w) = widths.get_mut(i) {
                *w = (*w).max(measure_text_width(cell));
            }
        }
    }
    widths
}

/// The final column is not padded.
fn render_row(cells: &[String], widths: &[usize]) -> String {
    let last = cells.len().saturating_sub(1);
    let mut out = String::from("  ");
    for (i, cell) in cells.iter().enumerate() {
        out.push_str(cell);
        if i != last {
            let pad = widths
                .get(i)
                .copied()
                .unwrap_or(0)
                .saturating_sub(measure_text_width(cell));
            out.push_str(&" ".repeat(pad + 2));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_never_looks_like_a_shell_comment() {
        let line = render_note("configure NATS under [apps.main]");
        assert!(
            !line.trim_start().starts_with('#'),
            "note started with #: {line:?}"
        );
        assert!(line.contains(DOT), "note missing the · glyph: {line:?}");
        assert!(line.contains("configure NATS under [apps.main]"));
    }

    #[test]
    fn section_and_command_carry_their_glyphs() {
        assert!(render_section("Created acme").contains(BAR));
        assert!(render_section("Created acme").contains("Created acme"));
        assert!(render_command("cargo run").contains(ARROW));
        assert!(render_command("cargo run").contains("cargo run"));
    }

    #[test]
    fn wordmark_is_the_lowercase_brand() {
        let w = render_wordmark();
        assert!(w.contains("fundamental"), "{w:?}");
        assert!(w.contains(BAR), "{w:?}");
    }

    #[test]
    fn error_line_names_the_code_and_message() {
        let line = render_error("CLI_001", "directory exists");
        assert!(line.contains("error[CLI_001]:"), "{line:?}");
        assert!(line.contains("directory exists"), "{line:?}");
    }

    #[test]
    fn a_failure_names_the_file_and_cause_its_developer_message_holds() {
        let failed = crate::error::CliError::from(fundamental_package::PackageError::Io {
            path: "share/dist/app_bg.wasm".to_owned(),
            cause: std::io::Error::other("permission denied"),
        });
        let shown = render_failure(&failed);
        assert!(shown.contains("error[PKG_007]:"), "{shown:?}");
        assert!(shown.contains("share/dist/app_bg.wasm"), "{shown:?}");
        assert!(shown.contains("permission denied"), "{shown:?}");
    }

    #[test]
    fn a_failure_without_more_to_say_is_one_line() {
        let failed = crate::error::CliError::CommandFailed { code: 3 };
        assert_eq!(render_failure(&failed).lines().count(), 1);
    }

    #[test]
    fn table_aligns_columns_ignoring_ansi() {
        let headers = ["ENGINE", "NAME", "DOWN"];
        let widths = column_widths(
            &headers,
            &[vec![
                "sql".into(),
                "0001_init".into(),
                style("yes").green().to_string(),
            ]],
        );
        // Shown on NAME, below: both engine names are shorter than "ENGINE".
        assert_eq!(widths[0], measure_text_width("ENGINE"));
        assert_eq!(widths[1], "0001_init".len());
        // The styled "yes" measures as 3 columns despite its ANSI codes.
        assert_eq!(widths[2], measure_text_width("DOWN"));

        let row = render_row(
            &[
                "sql".into(),
                "0001_init".into(),
                style("yes").green().to_string(),
            ],
            &widths,
        );
        assert!(row.contains("sql"), "{row:?}");
        assert!(row.starts_with("  "), "rows are indented: {row:?}");
    }

    #[test]
    fn quiet_suppresses_stdout_writers() {
        let ui = Ui::new(true);
        ui.section("hidden");
        ui.detail("hidden");
        ui.table(&["A"], &[vec!["b".into()]]);
    }
}
