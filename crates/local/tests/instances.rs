//! `up`, `facts` and `forget` on a machine whose state is a temporary directory.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use fundamental_core::Error as _;
use fundamental_local::{
    CertificateProof, Checkout, InstanceLabel, InstanceRole, LocalMachine, LocalProject,
    LocalState, Platform, ProjectRoot,
};
use time::OffsetDateTime;

fn project(root: &Path, apexes: &str) -> LocalProject {
    fs::create_dir_all(root).expect("a checkout");
    fs::write(
        root.join("fun.toml"),
        format!("[project]\nname = \"acme\"\n\n[local]\napexes = [{apexes}]\n"),
    )
    .expect("a manifest");
    LocalProject::load(root).expect("a project")
}

fn lane(root: &Path, label: &str) -> Checkout {
    Checkout::new(
        root.canonicalize().expect("a real path"),
        InstanceRole::Lane(InstanceLabel::parse(label).expect("a label")),
    )
}

fn main_of(root: &Path) -> Checkout {
    Checkout::new(
        root.canonicalize().expect("a real path"),
        InstanceRole::Main,
    )
}

fn leaf_der(path: &Path) -> rustls_pki_types::CertificateDer<'static> {
    use rustls_pki_types::pem::PemObject;
    rustls_pki_types::CertificateDer::from_pem_slice(&fs::read(path).expect("tls.pem"))
        .expect("a certificate")
}

#[test]
#[expect(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    reason = "readability baseline"
)]
fn up_claims_once_and_issues_a_leaf_for_the_instances_own_names() {
    let dir = tempfile::tempdir().expect("a directory");
    let machine =
        LocalMachine::unmanaged(LocalState::at(dir.path().join("state")), Platform::Linux);
    let main_root = dir.path().join("main");
    let main_project = project(&main_root, "\"acme.test\", \"acme-sandbox.test\"");
    let now = OffsetDateTime::now_utc();
    let first = machine
        .up(&main_project, &main_of(&main_root), now)
        .expect("up");
    assert!(first.minted && first.issued);
    let again = machine
        .up(&main_project, &main_of(&main_root), now)
        .expect("up again");
    assert_eq!(again.facts.address, first.facts.address);
    assert!(
        !again.minted && !again.issued,
        "nothing changes on a second up"
    );

    let lane_root = dir.path().join("lane");
    let lane_project = project(&lane_root, "\"acme.test\", \"acme-sandbox.test\"");
    let lane_up = machine
        .up(&lane_project, &lane(&lane_root, "fix-login"), now)
        .expect("a lane");
    assert_ne!(lane_up.facts.address, first.facts.address);
    assert!(!lane_up.minted, "a lane shares the project's root");

    let shell = lane_up.facts.shell();
    assert!(
        shell.contains("FUN_LOCAL_SUFFIX_ACME='fix-login.acme-lane.test'"),
        "{shell}"
    );
    assert!(
        shell.contains("FUN_LOCAL_SUFFIX_ACME_SANDBOX='fix-login.acme-sandbox-lane.test'"),
        "{shell}"
    );
    assert!(shell.contains("FUN_LOCAL_ROLE='lane'"), "{shell}");

    let root = fs::read_to_string(&first.facts.root).expect("root.pem");
    let root = {
        use rustls_pki_types::pem::PemObject;
        rustls_pki_types::CertificateDer::from_pem_slice(root.as_bytes()).expect("a root")
    };
    let proof = CertificateProof::at(now);
    let main_leaf = leaf_der(&first.facts.tls);
    let lane_leaf = leaf_der(&lane_up.facts.tls);
    assert!(proof.verifies(&root, &main_leaf, "www.acme.test"));
    assert!(proof.verifies(&root, &main_leaf, "www.acme-sandbox.test"));
    assert!(!proof.verifies(&root, &main_leaf, "www.fix-login.acme-lane.test"));
    assert!(proof.verifies(&root, &lane_leaf, "www.fix-login.acme-sandbox-lane.test"));
    assert!(!proof.verifies(&root, &lane_leaf, "www.acme.test"));

    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(&first.facts.tls)
        .expect("tls.pem")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600, "the leaf's key is the developer's alone");
    let corefile =
        fs::read_to_string(dir.path().join("state/resolver/Corefile")).expect("rendered");
    assert!(
        corefile.contains(&format!("IN A {}", lane_up.facts.address)),
        "{corefile}"
    );
}

#[test]
fn a_changed_apex_set_or_a_lost_root_is_refused_until_trust() {
    let dir = tempfile::tempdir().expect("a directory");
    let state = LocalState::at(dir.path().join("state"));
    let machine = LocalMachine::unmanaged(state.clone(), Platform::Linux);
    let root = dir.path().join("main");
    let now = OffsetDateTime::now_utc();
    let first = machine
        .up(&project(&root, "\"acme.test\""), &main_of(&root), now)
        .expect("up");
    let changed = machine
        .up(
            &project(&root, "\"acme.test\", \"acme-sandbox.test\""),
            &main_of(&root),
            now,
        )
        .map(|_| ())
        .map_err(|error| (error.error_code(), error.to_string()));
    let (code, message) = changed.expect_err("the apexes changed");
    assert_eq!(code, "LOCAL_015");
    assert!(message.contains("fun local trust"), "{message}");

    fs::remove_dir_all(state.root_dir(&first.facts.fingerprint)).expect("the root is lost");
    let lost = machine
        .up(&project(&root, "\"acme.test\""), &main_of(&root), now)
        .map(|_| ())
        .map_err(|error| error.error_code());
    assert_eq!(lost, Err("LOCAL_016"), "up never re-mints");
    let _ = ProjectRoot::certificate_path(&state, &first.facts.fingerprint);
}

#[test]
fn forget_releases_the_instance_and_its_files() {
    let dir = tempfile::tempdir().expect("a directory");
    let state = LocalState::at(dir.path().join("state"));
    let machine = LocalMachine::unmanaged(state, Platform::Linux);
    let root = dir.path().join("lane");
    let project = project(&root, "\"acme.test\"");
    let checkout = lane(&root, "fix-login");
    let up = machine
        .up(&project, &checkout, OffsetDateTime::now_utc())
        .expect("up");
    assert!(up.facts.tls.exists());
    let given = machine.forget(&project, &checkout).expect("forgotten");
    assert_eq!(given, Some(up.facts.address));
    assert!(!up.facts.tls.exists());
    assert_eq!(
        machine
            .facts(&project, &checkout)
            .map(|_| ())
            .map_err(|error| error.error_code()),
        Err("LOCAL_021")
    );
    let corefile =
        fs::read_to_string(dir.path().join("state/resolver/Corefile")).expect("rendered");
    assert!(!corefile.contains("fix-login"), "{corefile}");
    assert_eq!(
        machine.forget(&project, &checkout).ok(),
        Some(None),
        "forgetting twice gives nothing back and refuses nothing"
    );
}

#[test]
fn forget_from_another_checkout_leaves_the_holders_instance() {
    let dir = tempfile::tempdir().expect("a directory");
    let machine =
        LocalMachine::unmanaged(LocalState::at(dir.path().join("state")), Platform::Linux);
    let (holder, other) = (dir.path().join("a"), dir.path().join("b"));
    let held = project(&holder, "\"acme.test\"");
    let up = machine
        .up(&held, &main_of(&holder), OffsetDateTime::now_utc())
        .expect("the holder's instance");
    let elsewhere = project(&other, "\"acme.test\"");
    let refused = machine
        .forget(&elsewhere, &main_of(&other))
        .map_err(|error| (error.error_code(), error.to_string()));
    let (code, message) = refused.expect_err("another checkout holds it");
    assert_eq!(code, "LOCAL_009");
    assert!(message.contains(&holder.display().to_string()), "{message}");
    assert!(up.facts.tls.exists(), "the holder's leaf and key are left");
    assert_eq!(
        machine
            .facts(&held, &main_of(&holder))
            .map(|facts| facts.address)
            .ok(),
        Some(up.facts.address)
    );

    fs::remove_file(holder.join("fun.toml")).expect("the holder's checkout is gone");
    let taken = machine
        .forget(&elsewhere, &main_of(&other))
        .expect("a gone checkout's instance is given back");
    assert_eq!(taken, Some(up.facts.address));
    assert!(!up.facts.tls.exists());
}

#[test]
fn trust_refuses_a_root_file_holding_a_second_certificate() {
    let dir = tempfile::tempdir().expect("a directory");
    let machine =
        LocalMachine::unmanaged(LocalState::at(dir.path().join("state")), Platform::Linux);
    let root = dir.path().join("main");
    let project = project(&root, "\"acme.test\"");
    let now = OffsetDateTime::now_utc();
    let up = machine.up(&project, &main_of(&root), now).expect("up");
    let planted = ProjectRoot::mint(
        &fundamental_local::ProjectName::parse("evil").expect("a name"),
        &fundamental_local::ApexSet::new(fundamental_core::address::TestApex::parse("evil.test"))
            .expect("one"),
        now,
    )
    .expect("an attacker's root");
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(&up.facts.root)
        .expect("root.pem is the developer's");
    std::io::Write::write_all(&mut file, planted.pem().as_bytes()).expect("appended");
    let refused = machine
        .plan_trust(&project, now)
        .map(|_| ())
        .map_err(|error| (error.error_code(), error.to_string()));
    let (code, message) = refused.expect_err("two certificates in root.pem");
    assert_eq!(code, "LOCAL_006");
    assert!(message.contains("2 PEM blocks"), "{message}");
}

const CHILD: &str = "FUNDAMENTAL_LOCAL_TEST_CHILD";

#[test]
fn concurrent_up_processes_get_distinct_addresses() {
    if let Some(spec) = std::env::var_os(CHILD) {
        let spec = PathBuf::from(spec);
        let (state, checkout) = (spec.join("state"), spec.join("checkout"));
        let label = std::env::var("FUNDAMENTAL_LOCAL_TEST_LABEL").expect("a label");
        let machine = LocalMachine::unmanaged(LocalState::at(state), Platform::Linux);
        let project = LocalProject::load(&checkout).expect("a project");
        let up = machine
            .up(
                &project,
                &lane(&checkout.join(&label), &label),
                OffsetDateTime::now_utc(),
            )
            .expect("claimed");
        println!("ADDRESS={}", up.facts.address);
        return;
    }
    let dir = tempfile::tempdir().expect("a directory");
    let checkout = dir.path().join("checkout");
    project(&checkout, "\"acme.test\"");
    let children: Vec<_> = (0..8)
        .map(|at| {
            let label = format!("p{at}");
            fs::create_dir_all(checkout.join(&label)).expect("a lane directory");
            fs::copy(
                checkout.join("fun.toml"),
                checkout.join(&label).join("fun.toml"),
            )
            .expect("a manifest");
            Command::new(std::env::current_exe().expect("this test binary"))
                .args([
                    "--exact",
                    "concurrent_up_processes_get_distinct_addresses",
                    "--nocapture",
                ])
                .env(CHILD, dir.path())
                .env("FUNDAMENTAL_LOCAL_TEST_LABEL", &label)
                .output_async()
        })
        .collect();
    let addresses: BTreeSet<String> = children
        .into_iter()
        .map(|child| {
            let output = child.wait_with_output().expect("a child");
            let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
            assert!(
                output.status.success(),
                "{stdout}{}",
                String::from_utf8_lossy(&output.stderr)
            );
            stdout
                .lines()
                .find_map(|line| line.strip_prefix("ADDRESS=").map(str::to_owned))
                .expect("an address")
        })
        .collect();
    assert_eq!(addresses.len(), 8, "{addresses:?}");
}

trait OutputAsync {
    fn output_async(&mut self) -> std::process::Child;
}

impl OutputAsync for Command {
    fn output_async(&mut self) -> std::process::Child {
        self.stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("a child starts")
    }
}
