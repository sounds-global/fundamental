use fundamental_macros::Error;

/// Why `fun local` refused, or could not finish.
///
/// Each message names the wrong value and the verb that puts it right.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum LocalError {
    /// A declared apex is not one lower-case label under `.test`.
    #[error(
        code = "LOCAL_001",
        message = "`{apex}` is not a development apex: an apex is one lower-case label under `.test`, `acme.test` (RFC 6761)",
        status = 400
    )]
    ApexNotTest {
        /// The value as declared.
        apex: String,
    },

    /// Another project on this machine already holds the apex.
    #[error(
        code = "LOCAL_002",
        message = "`{apex}` is held by project `{holder}` on this machine; an apex has one project",
        status = 409
    )]
    ApexHeldElsewhere {
        /// The apex claimed twice.
        apex: String,
        /// The project that holds it.
        holder: String,
    },

    /// Every loopback address in the pool is claimed.
    #[error(
        code = "LOCAL_003",
        message = "every loopback address from 127.0.0.2 to 127.0.0.254 is claimed; `fun local forget` in a checkout gives one back (checkouts that no longer exist: {orphans})",
        status = 503,
        retryable
    )]
    NoAddressLeft {
        /// Checkouts holding an address that no longer exist, or `none`.
        orphans: String,
    },

    /// `.test` does not reach this machine's resolver.
    #[error(
        code = "LOCAL_004",
        message = "`{name}` does not resolve to this instance through the system resolver; run `fun local setup` once on this machine",
        status = 503
    )]
    ResolverNotRouted {
        /// The name that was looked up.
        name: String,
    },

    /// The project's root is absent from a store a browser reads.
    #[error(
        code = "LOCAL_005",
        message = "the root for project `{project}` is not trusted in {stores}; run `fun local trust`",
        status = 503
    )]
    RootNotTrusted {
        /// The project.
        project: String,
        /// The stores it is missing from.
        stores: String,
    },

    /// An issued leaf or the root failed its proof.
    #[error(
        code = "LOCAL_006",
        message = "certificate proof `{probe}` failed: {detail}",
        status = 500
    )]
    LeafCoversTooMuch {
        /// The probe that failed.
        probe: String,
        /// What it found.
        detail: String,
    },

    /// `[local.brands]` and `[local] apexes` are both present.
    #[error(
        code = "LOCAL_007",
        message = "the apexes are declared twice: `[local.brands]` names each brand's apex and `[local] apexes` names apexes; keep one",
        status = 400
    )]
    ApexesDeclaredTwice,

    /// Neither `[local.brands]` nor `[local] apexes` is present.
    #[error(
        code = "LOCAL_008",
        message = "no development apex is declared: add `[local] apexes = [\"<name>.test\"]` to fun.toml, or each brand's apex under `[local.brands]`",
        status = 400
    )]
    NoApexes,

    /// Another existing checkout holds this project and label.
    #[error(
        code = "LOCAL_009",
        message = "instance `{instance}` is held by the checkout at {holder}; run this one with `--label <name>`, or `fun local forget` there",
        status = 409
    )]
    InstanceHeldElsewhere {
        /// The project and label.
        instance: String,
        /// The other checkout.
        holder: String,
    },

    /// Every problem in `[local.origins]`.
    #[error(
        code = "LOCAL_010",
        message = "`[local.origins]` is not valid: {problems}",
        status = 400
    )]
    OriginsInvalid {
        /// Every problem, one per line.
        problems: String,
    },

    /// The registry is unreadable, invalid, or from a newer `fun`.
    #[error(
        code = "LOCAL_011",
        message = "the local origins registry at {path} cannot be used: {detail}",
        status = 500
    )]
    RegistryCorrupt {
        /// The registry file.
        path: String,
        /// What is wrong with it.
        detail: String,
    },

    /// `[local] databases` names a database that is not one plain name.
    #[error(
        code = "LOCAL_024",
        message = "`{name}` is not a database name: a lowercase letter, then up to 62 lowercase letters, digits and underscores",
        status = 400
    )]
    DatabaseNameInvalid {
        /// What was written.
        name: String,
    },

    /// `[local.logins]` names a login or a role that is not one plain name.
    #[error(
        code = "LOCAL_026",
        message = "`{name}` is not a login or role name: a lowercase letter, then up to 62 lowercase letters, digits and underscores",
        status = 400
    )]
    LoginInvalid {
        /// What was written.
        name: String,
    },

    /// The local database node did not answer in time.
    #[error(
        code = "LOCAL_025",
        message = "the local database did not answer within {seconds} seconds; `docker logs fundamental-local-ysql` says why",
        status = 503
    )]
    DatabaseNotReady {
        /// How long `up` waited.
        seconds: u64,
    },

    /// A tool `fun local` runs failed.
    #[error(
        code = "LOCAL_012",
        message = "`{argv}` failed ({status}): {stderr}",
        status = 500
    )]
    ToolFailed {
        /// The command line, as run.
        argv: String,
        /// How it ended.
        status: String,
        /// The tail of what it wrote to stderr.
        stderr: String,
    },

    /// Not Linux with systemd-resolved, and not macOS.
    #[error(
        code = "LOCAL_013",
        message = "local origins need Linux with systemd-resolved, or macOS: {detail}",
        status = 501
    )]
    UnsupportedPlatform {
        /// What was found instead.
        detail: String,
    },

    /// `[project] name` is absent or not a DNS label.
    #[error(
        code = "LOCAL_014",
        message = "`[project] name` in fun.toml must be one lower-case DNS label to name local origins (found {found})",
        status = 400
    )]
    ProjectUnnamed {
        /// What was found.
        found: String,
    },

    /// The declared apexes differ from those of the registered root.
    #[error(
        code = "LOCAL_015",
        message = "project `{project}` declares {declared}, but its root on this machine was minted for {registered}; run `fun local trust` to mint and trust a root for the new set",
        status = 409
    )]
    ApexSetChanged {
        /// The project.
        project: String,
        /// The apexes the project declares now.
        declared: String,
        /// The apexes the registered root permits.
        registered: String,
    },

    /// The registered root is gone.
    #[error(
        code = "LOCAL_016",
        message = "the root for project `{project}` ({fingerprint}) is missing from {path}; `fun local trust` mints and trusts a new one",
        status = 409
    )]
    RootMissing {
        /// The project.
        project: String,
        /// The registered fingerprint.
        fingerprint: String,
        /// Where it should be.
        path: String,
    },

    /// `fun local` was run as root.
    #[error(
        code = "LOCAL_017",
        message = "`fun local` runs as the developer, never as root; run it without sudo and it asks for sudo itself where a step needs it",
        status = 403
    )]
    RunAsRoot,

    /// Another user owns this machine's local origins.
    #[error(
        code = "LOCAL_018",
        message = "local origins on this machine belong to uid {owner}; one user runs them",
        status = 403
    )]
    NotMachineOwner {
        /// The owning uid.
        owner: u32,
    },

    /// The label cannot be made a DNS label.
    #[error(
        code = "LOCAL_019",
        message = "`{label}` cannot name an instance: a label is one lower-case DNS label; pass `--label <name>`",
        status = 400
    )]
    LabelInvalid {
        /// The label, or the branch it was derived from.
        label: String,
    },

    /// The registry lock was not granted in time.
    #[error(
        code = "LOCAL_020",
        message = "the local origins registry stayed locked for {waited_ms} ms ({holder})",
        status = 503,
        retryable
    )]
    LockTimeout {
        /// How long this process waited.
        waited_ms: u64,
        /// Who holds it, as the lock file records.
        holder: String,
    },

    /// A verb needs an instance this checkout has not claimed.
    #[error(
        code = "LOCAL_021",
        message = "this checkout has no local instance; run `fun local up` first",
        status = 409
    )]
    InstanceNotUp,

    /// A file `fun local` owns could not be read or written.
    #[error(
        code = "LOCAL_022",
        message = "{path} could not be {action}: {cause}",
        status = 500
    )]
    Io {
        /// The file or directory.
        path: String,
        /// `read`, `written`, `created` or `removed`.
        action: &'static str,
        /// The operating system's error.
        #[source]
        cause: std::io::Error,
    },

    /// `forget` finished and something of the instance is still there.
    #[error(
        code = "LOCAL_023",
        message = "`fun local forget` left {what} behind",
        status = 500
    )]
    LeftBehind {
        /// What is still there.
        what: String,
    },

    /// The project manifest could not be read.
    #[error(transparent)]
    Manifest(#[from] fundamental_manifest::ManifestError),
}

impl LocalError {
    /// An I/O failure on `path`, with what was being done to it.
    pub(crate) fn io(path: &std::path::Path, action: &'static str, cause: std::io::Error) -> Self {
        Self::Io {
            path: path.display().to_string(),
            action,
            cause,
        }
    }
}

#[cfg(test)]
mod tests {
    use fundamental_core::Error as _;

    use super::LocalError;

    #[test]
    fn every_refusal_has_its_code_and_only_two_are_retryable() {
        let cases = [
            (
                LocalError::ApexNotTest {
                    apex: "a.nl".into(),
                },
                "LOCAL_001",
                false,
            ),
            (
                LocalError::NoAddressLeft {
                    orphans: "none".into(),
                },
                "LOCAL_003",
                true,
            ),
            (LocalError::NoApexes, "LOCAL_008", false),
            (LocalError::RunAsRoot, "LOCAL_017", false),
            (
                LocalError::LockTimeout {
                    waited_ms: 1,
                    holder: "pid 1".into(),
                },
                "LOCAL_020",
                true,
            ),
            (LocalError::InstanceNotUp, "LOCAL_021", false),
        ];
        for (error, code, retryable) in cases {
            assert_eq!(error.error_code(), code);
            assert_eq!(error.is_retryable(), retryable, "{code}");
        }
    }

    #[test]
    fn a_message_names_the_verb_that_puts_it_right() {
        let error = LocalError::ApexSetChanged {
            project: "acme".into(),
            declared: "acme.test".into(),
            registered: "old.test".into(),
        };
        assert!(error.to_string().contains("fun local trust"), "{error}");
    }
}
