//! [`PlanResolver`]: each scaffold choice from its flag, else a prompt when attended, else a default.

use super::ScaffoldPrompts;
use super::project_name::ProjectName;
use super::project_plan::DEFAULT_APP;
use crate::engine::Engine;
use crate::{CliError, CliResult, Surface};
use fundamental::Validate;

pub(crate) struct PlanResolver<'a> {
    prompts: &'a ScaffoldPrompts,
    /// Attended, and not forced non-interactive.
    interactive: bool,
}

impl<'a> PlanResolver<'a> {
    pub(crate) fn new(prompts: &'a ScaffoldPrompts, interactive: bool) -> Self {
        Self {
            prompts,
            interactive,
        }
    }

    /// The project name from the validated flag or a prompt.
    ///
    /// # Errors
    /// [`CliError::Validation`], [`CliError::MissingProjectName`] when non-interactive, or
    /// [`CliError::PromptFailed`].
    pub(crate) fn name(&self, flag: Option<&str>) -> CliResult<String> {
        if let Some(name) = flag {
            ProjectName::new(name).validate()?;
            return Ok(name.to_string());
        }
        if self.interactive {
            return self.prompts.project_name();
        }
        Err(CliError::MissingProjectName)
    }

    /// The surfaces from the flag, a prompt, or none.
    ///
    /// # Errors
    /// [`CliError::PromptFailed`] if the prompt cannot run.
    pub(crate) fn surfaces(&self, flag: Option<&[Surface]>) -> CliResult<Vec<Surface>> {
        if let Some(surfaces) = flag {
            return Ok(surfaces.to_vec());
        }
        if self.interactive {
            return self.prompts.surfaces();
        }
        Ok(Vec::new())
    }

    /// The app name, once a surface is selected.
    ///
    /// # Errors
    /// [`CliError::Validation`] or [`CliError::PromptFailed`].
    pub(crate) fn app(&self, flag: Option<&str>, surfaces: &[Surface]) -> CliResult<String> {
        if surfaces.is_empty() {
            return Ok(DEFAULT_APP.to_string());
        }
        if let Some(app) = flag {
            ProjectName::new(app).validate()?;
            return Ok(app.to_string());
        }
        if self.interactive {
            return self.prompts.app_name();
        }
        Ok(DEFAULT_APP.to_string())
    }

    /// The socket port from the flag or `default`; unprompted, and inert without a socket.
    pub(crate) fn port(&self, flag: Option<u16>, surfaces: &[Surface], default: u16) -> u16 {
        if !surfaces.contains(&Surface::Socket) {
            return default;
        }
        flag.unwrap_or(default)
    }

    /// The database engines; a `sync` surface with none gets YSQL for its slice store.
    ///
    /// # Errors
    /// [`CliError::InvalidDatabase`] or [`CliError::PromptFailed`].
    pub(crate) fn databases(
        &self,
        flag: Option<&[String]>,
        surfaces: &[Surface],
    ) -> CliResult<Vec<Engine>> {
        if surfaces.is_empty() {
            return Ok(Vec::new());
        }
        let mut engines = if let Some(values) = flag {
            parse_dbs(values)?
        } else if self.interactive {
            self.prompts.databases()?
        } else {
            Vec::new()
        };
        if surfaces.contains(&Surface::Sync) && engines.is_empty() {
            engines.push(Engine::SqlDb);
        }
        Ok(engines)
    }

    /// The slice store's backend from the flag (`sql`/`cql`); absent leaves it to the generator.
    ///
    /// # Errors
    /// [`CliError::InvalidDatabase`] for any other value.
    pub(crate) fn sync_store(&self, flag: Option<&str>) -> CliResult<Option<Engine>> {
        match flag {
            None => Ok(None),
            Some(value) => match value.trim().to_ascii_lowercase().as_str() {
                "sql" => Ok(Some(Engine::SqlDb)),
                "cql" => Ok(Some(Engine::CqlDb)),
                _ => Err(CliError::InvalidDatabase {
                    value: value.to_string(),
                }),
            },
        }
    }

    /// Whether to wire up Tailwind, with the web surface only, from `--tailwind` or a prompt.
    ///
    /// # Errors
    /// [`CliError::PromptFailed`] if the prompt cannot run.
    pub(crate) fn tailwind(&self, flag: bool, surfaces: &[Surface]) -> CliResult<bool> {
        if !surfaces.contains(&Surface::Web) {
            return Ok(false);
        }
        if flag {
            return Ok(true);
        }
        if self.interactive {
            return self.prompts.tailwind();
        }
        Ok(false)
    }
}

/// `none` (or empty) yields no engine and duplicates collapse.
///
/// # Errors
/// [`CliError::InvalidDatabase`] for a value that is not `none`/`sql`/`cql`.
fn parse_dbs(values: &[String]) -> CliResult<Vec<Engine>> {
    let mut engines = Vec::new();
    for value in values {
        let engine = match value.trim().to_ascii_lowercase().as_str() {
            "none" | "" => continue,
            "sql" => Engine::SqlDb,
            "cql" => Engine::CqlDb,
            _ => {
                return Err(CliError::InvalidDatabase {
                    value: value.clone(),
                });
            }
        };
        if !engines.contains(&engine) {
            engines.push(engine);
        }
    }
    Ok(engines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CommandContext;
    use fundamental::core::Error as _;

    fn non_interactive() -> ScaffoldPrompts {
        ScaffoldPrompts::new(CommandContext::new(true))
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn name_takes_a_valid_flag() {
        let p = non_interactive();
        let r = PlanResolver::new(&p, false);
        assert_eq!(r.name(Some("acme")).unwrap(), "acme");
    }

    #[test]
    fn name_rejects_an_invalid_flag() {
        let p = non_interactive();
        let r = PlanResolver::new(&p, false);
        let err = r.name(Some("1bad")).unwrap_err();
        assert!(matches!(err, CliError::Validation(_)));
    }

    #[test]
    fn name_missing_in_a_non_interactive_run_is_an_error() {
        let p = non_interactive();
        let r = PlanResolver::new(&p, false);
        let err = r.name(None).unwrap_err();
        assert!(matches!(err, CliError::MissingProjectName));
        assert_eq!(err.error_code(), "CLI_015");
    }

    #[test]
    fn surfaces_flag_beats_the_prompt_and_the_default() {
        let p = non_interactive();
        let r = PlanResolver::new(&p, false);
        let flag = [Surface::Web, Surface::Api];
        assert_eq!(r.surfaces(Some(&flag)).unwrap(), flag.to_vec());
    }

    #[test]
    fn surfaces_default_to_none_when_non_interactive() {
        let p = non_interactive();
        let r = PlanResolver::new(&p, false);
        assert!(r.surfaces(None).unwrap().is_empty());
    }

    #[test]
    fn app_defaults_to_main_without_a_flag() {
        let p = non_interactive();
        let r = PlanResolver::new(&p, false);
        assert_eq!(r.app(None, &[Surface::Web]).unwrap(), "main");
    }

    #[test]
    fn app_is_ignored_without_a_surface() {
        let p = non_interactive();
        let r = PlanResolver::new(&p, false);
        assert_eq!(r.app(Some("billing"), &[]).unwrap(), "main");
    }

    #[test]
    fn port_prefers_the_flag_when_socket_is_selected() {
        let p = non_interactive();
        let r = PlanResolver::new(&p, false);
        assert_eq!(r.port(Some(8080), &[Surface::Socket], 9000), 8080);
        assert_eq!(r.port(None, &[Surface::Socket], 9000), 9000);
        assert_eq!(r.port(Some(8080), &[Surface::Web], 9500), 9500);
    }

    #[test]
    fn databases_parse_the_flag() {
        let p = non_interactive();
        let r = PlanResolver::new(&p, false);
        let dbs = r
            .databases(Some(&strings(&["sql", "cql"])), &[Surface::Api])
            .unwrap();
        assert_eq!(dbs, vec![Engine::SqlDb, Engine::CqlDb]);
    }

    #[test]
    fn databases_none_flag_yields_no_engine() {
        let p = non_interactive();
        let r = PlanResolver::new(&p, false);
        assert!(
            r.databases(Some(&strings(&["none"])), &[Surface::Api])
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn databases_reject_an_unknown_engine() {
        let p = non_interactive();
        let r = PlanResolver::new(&p, false);
        let err = r
            .databases(Some(&strings(&["duckdb"])), &[Surface::Api])
            .unwrap_err();
        assert!(matches!(err, CliError::InvalidDatabase { .. }));
        assert_eq!(err.error_code(), "CLI_016");
    }

    #[test]
    fn databases_default_to_none_when_non_interactive() {
        let p = non_interactive();
        let r = PlanResolver::new(&p, false);
        assert!(r.databases(None, &[Surface::Api]).unwrap().is_empty());
    }

    #[test]
    fn sync_surface_forces_sql_when_no_database_chosen() {
        let p = non_interactive();
        let r = PlanResolver::new(&p, false);
        assert_eq!(
            r.databases(None, &[Surface::Sync]).unwrap(),
            vec![Engine::SqlDb]
        );
    }

    #[test]
    fn sync_surface_keeps_cql_when_chosen() {
        let p = non_interactive();
        let r = PlanResolver::new(&p, false);
        assert_eq!(
            r.databases(Some(&strings(&["cql"])), &[Surface::Sync])
                .unwrap(),
            vec![Engine::CqlDb]
        );
    }

    #[test]
    fn sync_store_parses_the_known_backends() {
        let p = non_interactive();
        let r = PlanResolver::new(&p, false);
        assert_eq!(r.sync_store(None).unwrap(), None);
        assert_eq!(r.sync_store(Some("sql")).unwrap(), Some(Engine::SqlDb));
        assert_eq!(r.sync_store(Some("CQL")).unwrap(), Some(Engine::CqlDb));
    }

    #[test]
    fn sync_store_rejects_an_unknown_backend() {
        let p = non_interactive();
        let r = PlanResolver::new(&p, false);
        let err = r.sync_store(Some("duckdb")).unwrap_err();
        assert!(matches!(err, CliError::InvalidDatabase { .. }));
    }

    #[test]
    fn tailwind_only_applies_to_the_web_surface() {
        let p = non_interactive();
        let r = PlanResolver::new(&p, false);
        assert!(!r.tailwind(true, &[Surface::Api]).unwrap());
        assert!(r.tailwind(true, &[Surface::Web]).unwrap());
        assert!(!r.tailwind(false, &[Surface::Web]).unwrap());
    }
}
