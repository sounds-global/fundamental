//! Interactive prompts for scaffolding; [`PlanResolver`](super::PlanResolver) decides when each runs.

use dialoguer::{Confirm, Input, MultiSelect};
use fundamental::Validate;

use super::project_name::ProjectName;
use super::project_plan::DEFAULT_APP;
use crate::engine::Engine;
use crate::ui::Ui;
use crate::{CliError, CliResult, CommandContext, Surface};

pub(crate) struct ScaffoldPrompts(CommandContext);

impl ScaffoldPrompts {
    pub(crate) fn new(ctx: CommandContext) -> Self {
        Self(ctx)
    }

    pub(crate) fn ui(&self) -> &Ui {
        self.0.ui()
    }

    pub(crate) fn intro(&self) {
        let ui = self.0.ui();
        ui.blank();
        ui.wordmark();
        ui.detail("One codebase. Every surface.");
        ui.blank();
    }

    /// Prompt for the project name until it is a valid crate name.
    ///
    /// # Errors
    /// [`CliError::PromptFailed`] if the terminal prompt cannot run.
    pub(crate) fn project_name(&self) -> CliResult<String> {
        Input::with_theme(&Ui::prompt_theme())
            .with_prompt("Project name")
            .validate_with(|input: &String| -> Result<(), String> {
                ProjectName::new(input.as_str())
                    .validate()
                    .map_err(|e| e.to_string())
            })
            .interact_text()
            .map_err(prompt_failed)
    }

    /// Multi-select of the surfaces; none yields a domain-only library project.
    ///
    /// # Errors
    /// [`CliError::PromptFailed`] if the terminal prompt cannot run.
    pub(crate) fn surfaces(&self) -> CliResult<Vec<Surface>> {
        let items: &[(Surface, &str)] = &[
            (Surface::Worker, "Worker  ·  NATS consumer"),
            (Surface::Socket, "Socket  ·  WebSocket server"),
            (Surface::Api, "API     ·  HTTP API server"),
            (
                Surface::Web,
                "Web     ·  Server-rendered pages (SSR + islands)",
            ),
            (
                Surface::Sync,
                "Sync    ·  Slice-sync server (offline-first replication)",
            ),
            (
                Surface::Native,
                "Native  ·  Desktop + mobile app (Tauri, pages in-process)",
            ),
        ];
        let labels: Vec<&str> = items.iter().map(|(_, l)| *l).collect();

        let selected = MultiSelect::with_theme(&Ui::prompt_theme())
            .with_prompt("Which surfaces does your app target? (space to toggle, enter to confirm)")
            .items(&labels)
            .defaults(&vec![false; items.len()])
            .interact()
            .map_err(prompt_failed)?;

        Ok(selected.into_iter().map(|i| items[i].0).collect())
    }

    /// Prompt for the app name, defaulting to [`DEFAULT_APP`].
    ///
    /// # Errors
    /// [`CliError::PromptFailed`] if the terminal prompt cannot run.
    pub(crate) fn app_name(&self) -> CliResult<String> {
        Input::with_theme(&Ui::prompt_theme())
            .with_prompt("App name")
            .default(DEFAULT_APP.to_string())
            .validate_with(|input: &String| -> Result<(), String> {
                ProjectName::new(input.as_str())
                    .validate()
                    .map_err(|e| e.to_string())
            })
            .interact_text()
            .map_err(prompt_failed)
    }

    /// Prompt for the database engines: none, one, or both.
    ///
    /// # Errors
    /// [`CliError::PromptFailed`] if the terminal prompt cannot run.
    pub(crate) fn databases(&self) -> CliResult<Vec<Engine>> {
        let items: &[(Engine, &str)] = &[
            (Engine::SqlDb, "SqlDb  ·  PostgreSQL-compatible SQL"),
            (Engine::CqlDb, "CqlDb  ·  Cassandra CQL"),
        ];
        let labels: Vec<&str> = items.iter().map(|(_, l)| *l).collect();

        let selected = MultiSelect::with_theme(&Ui::prompt_theme())
            .with_prompt("Databases? (space to toggle, enter to confirm — none is fine)")
            .items(&labels)
            .defaults(&vec![false; items.len()])
            .interact()
            .map_err(prompt_failed)?;

        Ok(selected.into_iter().map(|i| items[i].0.clone()).collect())
    }

    /// Ask whether to set up Tailwind CSS for the web surface.
    ///
    /// # Errors
    /// [`CliError::PromptFailed`] if the terminal prompt cannot run.
    pub(crate) fn tailwind(&self) -> CliResult<bool> {
        Confirm::with_theme(&Ui::prompt_theme())
            .with_prompt("Set up Tailwind CSS for the web surface?")
            .default(false)
            .interact()
            .map_err(prompt_failed)
    }

    /// Confirm the plan before anything is written; `subject` is `"project"` or `"app"`.
    ///
    /// # Errors
    /// [`CliError::PromptFailed`] if the terminal prompt cannot run.
    pub(crate) fn confirm(&self, subject: &str) -> CliResult<bool> {
        Confirm::with_theme(&Ui::prompt_theme())
            .with_prompt(format!("Create this {subject}?"))
            .default(true)
            .interact()
            .map_err(prompt_failed)
    }
}

pub(super) fn prompt_failed(e: dialoguer::Error) -> CliError {
    CliError::PromptFailed {
        reason: format!("prompt failed: {e}"),
    }
}
