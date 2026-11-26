//! [`LocalDatabase`] — the machine's YugabyteDB node, holding each project's databases.

use std::time::{Duration, Instant};

use tracing::{info, instrument};

use crate::tool::{ask, run};
use crate::{DatabaseName, LocalError, LocalLogin};

/// The machine's one YugabyteDB node, on `127.0.0.1:5433`, its data in a named volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalDatabase;

/// How long a new node may take to answer before `up` gives up on it.
const READY_WITHIN: Duration = Duration::from_secs(180);

impl LocalDatabase {
    /// The engine, by its immutable build tag (a digest names one platform).
    pub const IMAGE: &'static str = "yugabytedb/yugabyte:2026.1.1.0-b91";
    /// The container's name, and its volume's.
    pub const CONTAINER: &'static str = "fundamental-local-ysql";

    /// `docker run` for the node.
    pub(crate) fn start_args() -> Vec<String> {
        [
            "run",
            "-d",
            "--name",
            Self::CONTAINER,
            // yugabyted writes its hostname into the volume; a container id would break a recreated container.
            "--hostname",
            "yugabyte",
            // Each YSQL session exchanges rows through /dev/shm; Docker's 64 MB fills early.
            "--shm-size",
            "1g",
            "--restart",
            "unless-stopped",
            "-p",
            "127.0.0.1:5433:5433",
            "-v",
            "fundamental-local-ysql:/root/var",
            Self::IMAGE,
            "bin/yugabyted",
            "start",
            "--background=false",
            "--ui=false",
            "--callhome=false",
            "--tserver_flags=yb_enable_read_committed_isolation=true,tablet_enable_ttl_file_filter=true,ysql_num_shards_per_tserver=1",
        ]
        .map(str::to_owned)
        .to_vec()
    }

    /// `docker exec` asking the node `sql`, answered tuples-only.
    pub(crate) fn ask_args(sql: &str) -> Vec<String> {
        [
            "exec",
            Self::CONTAINER,
            "bin/ysqlsh",
            "-h",
            "yugabyte",
            "-p",
            "5433",
            "-U",
            "yugabyte",
            "-d",
            "yugabyte",
            "-tAc",
            sql,
        ]
        .map(str::to_owned)
        .to_vec()
    }

    /// Starts the node unless it runs, waits until it answers, and creates each missing database,
    /// login and role.
    ///
    /// # Errors
    ///
    /// [`LocalError::ToolFailed`] (`LOCAL_012`) when Docker or a create fails;
    /// [`LocalError::DatabaseNotReady`] (`LOCAL_025`) when the node does not answer in time.
    #[instrument(skip_all, fields(databases = databases.len(), logins = logins.len()))]
    pub fn ensure(
        self,
        databases: &[DatabaseName],
        logins: &[LocalLogin],
    ) -> Result<(), LocalError> {
        Self::start_unless_running()?;
        Self::wait_until_ready()?;
        databases.iter().try_for_each(Self::create_unless_held)?;
        logins.iter().try_for_each(Self::create_login)
    }

    fn start_unless_running() -> Result<(), LocalError> {
        let filter = format!("name=^{}$", Self::CONTAINER);
        let running = run(
            "docker",
            &[
                "ps",
                "-q",
                "--filter",
                &filter,
                "--filter",
                "status=running",
            ],
            None,
        )?;
        if running.trim().is_empty() {
            // A stopped node of an earlier start; its data stays in the volume.
            let _ = ask("docker", &["rm", "-f", Self::CONTAINER]);
            run("docker", &Self::start_args(), None)?;
            info!(container = Self::CONTAINER, "local.database_started");
        }
        Ok(())
    }

    fn wait_until_ready() -> Result<(), LocalError> {
        let started = Instant::now();
        while ask("docker", &Self::ask_args("SELECT 1")).is_err() {
            if started.elapsed() > READY_WITHIN {
                return Err(LocalError::DatabaseNotReady {
                    seconds: READY_WITHIN.as_secs(),
                });
            }
            std::thread::sleep(Duration::from_secs(2));
        }
        Ok(())
    }

    /// The statements that make `login` and its roles when absent and grant them; a role
    /// migrations also make is made by whichever runs first.
    pub(crate) fn login_statements(login: &LocalLogin) -> Vec<String> {
        let once = |statement: String| {
            format!("DO $$ BEGIN {statement}; EXCEPTION WHEN duplicate_object THEN NULL; END $$;")
        };
        let name = login.name().as_str();
        login
            .roles()
            .iter()
            .map(|role| once(format!("CREATE ROLE {} NOLOGIN", role.as_str())))
            .chain([once(format!("CREATE ROLE {name} LOGIN"))])
            .chain(
                login
                    .roles()
                    .iter()
                    .map(|role| format!("GRANT {} TO {name}", role.as_str())),
            )
            .collect()
    }

    fn create_login(login: &LocalLogin) -> Result<(), LocalError> {
        for statement in Self::login_statements(login) {
            run("docker", &Self::ask_args(&statement), None)?;
        }
        info!(login = login.name().as_str(), "local.login_ensured");
        Ok(())
    }

    fn create_unless_held(name: &DatabaseName) -> Result<(), LocalError> {
        let name = name.as_str();
        let held = format!("SELECT 1 FROM pg_database WHERE datname = '{name}'");
        if !run("docker", &Self::ask_args(&held), None)?
            .trim()
            .is_empty()
        {
            return Ok(());
        }
        let create = format!("CREATE DATABASE {name}");
        run("docker", &Self::ask_args(&create), None)?;
        info!(database = name, "local.database_created");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::LocalDatabase;

    #[test]
    fn the_node_answers_on_loopback_only_and_keeps_its_data_in_a_volume() {
        let args = LocalDatabase::start_args();
        let joined = args.join(" ");
        assert!(joined.contains("-p 127.0.0.1:5433:5433"), "{joined}");
        assert!(
            joined.contains("-v fundamental-local-ysql:/root/var"),
            "{joined}"
        );
        assert!(joined.contains(LocalDatabase::IMAGE), "{joined}");
    }

    #[test]
    fn a_login_is_made_once_and_granted_its_roles_made_once() {
        let login = crate::LocalLogin::parse("www_site", &["www".to_owned()]).expect("plain names");
        assert_eq!(
            LocalDatabase::login_statements(&login),
            [
                "DO $$ BEGIN CREATE ROLE www NOLOGIN; EXCEPTION WHEN duplicate_object THEN NULL; END $$;",
                "DO $$ BEGIN CREATE ROLE www_site LOGIN; EXCEPTION WHEN duplicate_object THEN NULL; END $$;",
                "GRANT www TO www_site",
            ]
        );
    }

    #[test]
    fn a_question_is_one_argument_never_split_by_a_shell() {
        let args = LocalDatabase::ask_args("CREATE DATABASE ops");
        assert_eq!(args.last().map(String::as_str), Some("CREATE DATABASE ops"));
        assert_eq!(args[0], "exec");
    }
}
