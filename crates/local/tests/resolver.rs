//! The machine resolver answers only registered names, and only to the machine itself.

use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use fundamental_core::address::TestApex;
use fundamental_local::{
    ApexSet, Corefile, InstanceKey, InstanceLabel, InstanceRole, LocalState, MachineResolver,
    Platform, ProjectName, Registry,
};

/// Alpine with `dig`, which can choose the address a query is sent from.
const PROBE_BASE: &str =
    "alpine:3.20@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc";
const PROBE_IMAGE: &str = "fundamental-local-dns-probe:alpine-3.20";
/// An address in TEST-NET-2 on the link: a source that is neither the resolver's nor loopback.
const STRANGER: &str = "198.51.100.7";

fn docker(args: &[&str]) -> (bool, String) {
    let output = Command::new("docker")
        .args(args)
        .output()
        .expect("docker runs");
    (
        output.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
    )
}

struct Containers(Vec<String>);

impl Drop for Containers {
    fn drop(&mut self) {
        for name in &self.0 {
            let _ = Command::new("docker").args(["rm", "-f", name]).output();
        }
    }
}

fn probe_image() {
    let dockerfile = format!("FROM {PROBE_BASE}\nRUN apk add --no-cache bind-tools\n");
    let mut child = Command::new("docker")
        .args(["build", "-q", "-t", PROBE_IMAGE, "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("docker build runs");
    std::io::Write::write_all(
        &mut child.stdin.take().expect("stdin"),
        dockerfile.as_bytes(),
    )
    .expect("the Dockerfile is written");
    assert!(
        child.wait().expect("built").success(),
        "the probe image builds"
    );
}

/// `dig`'s status line for `name`, asked from `source`.
fn dig(helper: &str, source: &str, name: &str) -> String {
    let (_, out) = docker(&[
        "exec",
        helper,
        "dig",
        "+time=1",
        "+tries=1",
        "-b",
        source,
        "@192.0.2.53",
        name,
        "A",
    ]);
    if out.contains("timed out") || out.contains("no servers could be reached") {
        return "SILENT".to_owned();
    }
    let status = out
        .lines()
        .find_map(|line| line.split("status: ").nth(1))
        .map(|rest| rest.split(',').next().unwrap_or_default().to_owned())
        .unwrap_or_else(|| format!("UNPARSED {out}"));
    let answer = out
        .lines()
        .skip_while(|line| !line.starts_with(";; ANSWER SECTION"))
        .nth(1)
        .and_then(|line| line.split_whitespace().last())
        .map(|address| format!(" {address}"))
        .unwrap_or_default();
    format!("{status}{answer}")
}

fn registry(with_lane: bool) -> Registry {
    let mut registry = Registry::default();
    let project = ProjectName::parse("acme").expect("a name");
    let apexes = ApexSet::new(TestApex::parse("acme.test")).expect("one apex");
    registry.register_project(&project, &apexes).expect("free");
    registry
        .claim_instance(
            &InstanceKey::new(project.clone(), InstanceRole::Main),
            Path::new("/main"),
            |_| true,
        )
        .expect("an address");
    if with_lane {
        let lane = InstanceRole::Lane(InstanceLabel::parse("fix-login").expect("a label"));
        registry
            .claim_instance(&InstanceKey::new(project, lane), Path::new("/lane"), |_| {
                true
            })
            .expect("an address");
    }
    registry
}

fn eventually(helper: &str, name: &str, wanted: &str) -> String {
    let started = Instant::now();
    loop {
        let seen = dig(helper, "192.0.2.53", name);
        if seen == wanted || started.elapsed() > Duration::from_secs(10) {
            return seen;
        }
        thread::sleep(Duration::from_millis(250));
    }
}

#[test]
#[expect(clippy::too_many_lines, reason = "readability baseline")]
fn the_resolver_refuses_an_unregistered_name() {
    probe_image();
    let dir = tempfile::tempdir().expect("a state directory");
    let state = LocalState::at(dir.path());
    MachineResolver::write(&state, &Corefile::render(&registry(true), Platform::Linux))
        .expect("the Corefile is written");

    let tag = std::process::id();
    let helper = format!("fundamental-local-test-link-{tag}");
    let resolver = format!("fundamental-local-test-dns-{tag}");
    let _cleanup = Containers(vec![resolver.clone(), helper.clone()]);
    let (started, out) = docker(&[
        "run",
        "-d",
        "--name",
        &helper,
        "--cap-add",
        "NET_ADMIN",
        PROBE_IMAGE,
        "sleep",
        "300",
    ]);
    assert!(started, "{out}");
    let (linked, out) = docker(&[
        "exec",
        &helper,
        "sh",
        "-c",
        &format!(
            "ip link add fundamental-dns type dummy && ip addr add 192.0.2.53/32 dev fundamental-dns && ip addr add {STRANGER}/32 dev fundamental-dns && ip link set fundamental-dns up"
        ),
    ]);
    assert!(linked, "{out}");
    let mount = format!("{}:/etc/coredns:ro", state.resolver_dir().display());
    let network = format!("container:{helper}");
    let (started, out) = docker(&[
        "run",
        "-d",
        "--name",
        &resolver,
        "--network",
        &network,
        "--user",
        "0:0",
        "--read-only",
        "--cap-drop",
        "ALL",
        "--cap-add",
        "NET_BIND_SERVICE",
        "--security-opt",
        "no-new-privileges",
        "-v",
        &mount,
        MachineResolver::IMAGE,
        "-conf",
        "/etc/coredns/Corefile",
    ]);
    assert!(started, "{out}");

    assert_eq!(
        eventually(&helper, "www.acme.test", "NOERROR 127.0.0.2"),
        "NOERROR 127.0.0.2"
    );
    assert_eq!(dig(&helper, "192.0.2.53", "acme.test"), "NOERROR 127.0.0.2");
    assert_eq!(
        dig(&helper, "127.0.0.1", "www.acme.test"),
        "NOERROR 127.0.0.2"
    );
    assert_eq!(
        dig(&helper, "192.0.2.53", "www.fix-login.acme-lane.test"),
        "NOERROR 127.0.0.3"
    );
    assert_eq!(dig(&helper, "192.0.2.53", "a.b.c.acme.test"), "NXDOMAIN");
    assert_eq!(
        dig(&helper, "192.0.2.53", "www.other.acme-lane.test"),
        "NXDOMAIN"
    );
    assert_eq!(dig(&helper, "192.0.2.53", "www.shop.test"), "REFUSED");
    assert_eq!(dig(&helper, "192.0.2.53", "example.net"), "REFUSED");
    assert_eq!(dig(&helper, STRANGER, "www.acme.test"), "SILENT");

    MachineResolver::write(&state, &Corefile::render(&registry(false), Platform::Linux))
        .expect("the lane is forgotten");
    assert_eq!(
        eventually(&helper, "www.fix-login.acme-lane.test", "NXDOMAIN"),
        "NXDOMAIN",
        "a forgotten lane's name never falls back to main"
    );
    assert_eq!(
        dig(&helper, "192.0.2.53", "www.acme.test"),
        "NOERROR 127.0.0.2"
    );
}
