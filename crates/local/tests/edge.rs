//! Two instances of one app served at once by their own pinned-Caddy edges, over HTTPS with
//! the project's root as the only trust anchor, all on [`LoopbackPool::TESTS`].

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpListener};
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use fundamental_local::{
    Checkout, InstanceFacts, InstanceKey, InstanceLabel, InstanceRole, LocalEdge, LocalMachine,
    LocalProject, LocalState, LoopbackPool, Platform,
};

/// The edges of the tests' pool, the only ones these tests start or remove.
const EDGE: LocalEdge = LocalEdge::in_pool(LoopbackPool::TESTS);
use time::OffsetDateTime;

/// Serialises the tests' edges on one address pool, by a lock file since each test may be its own process.
fn one_at_a_time() -> fs::File {
    let path = std::env::temp_dir().join("fundamental-local-edge-tests.lock");
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .expect("the lock file opens");
    file.lock().expect("the lock is granted");
    file
}

/// Removes the edges a test started, whatever the test's outcome.
struct Edges(Vec<InstanceKey>);

impl Drop for Edges {
    fn drop(&mut self) {
        for key in &self.0 {
            if let Err(error) = EDGE.remove(key) {
                eprintln!("the edge of {key} was not removed: {error}");
            }
        }
    }
}

fn project(root: &Path, port: u16) -> LocalProject {
    fs::create_dir_all(root).expect("a checkout");
    fs::write(
        root.join("fun.toml"),
        format!(
            "[project]\nname = \"edgeproof\"\n\n[apps.www.web]\nport = {port}\n\n[local]\napexes = [\"edgeproof.test\"]\n\n[local.origins]\nWWW_ORIGIN = {{ app = \"www\", host = \"www\", apex = \"edgeproof.test\" }}\n"
        ),
    )
    .expect("a manifest");
    LocalProject::load(root).expect("a project")
}

/// `up` for `checkout`, which a machine with edges serves on the tests' pool.
fn up_in_the_tests_pool(
    machine: &LocalMachine,
    project: &LocalProject,
    checkout: &Checkout,
) -> InstanceFacts {
    let facts = machine
        .up(project, checkout, OffsetDateTime::now_utc())
        .expect("up")
        .facts;
    assert!(
        LoopbackPool::TESTS.holds(facts.address),
        "a test's instance is on {}, not the tests' pool",
        facts.address
    );
    facts
}

/// Creates a container named `name`, labelled as the edge on `address`, never started.
fn create_on(name: &str, address: impl std::fmt::Display) {
    let created = Command::new("docker")
        .args(["create", "--name", name, "--label"])
        .arg(format!("fundamental.local.address={address}"))
        .arg(LocalEdge::IMAGE)
        .output()
        .expect("docker runs");
    assert!(created.status.success(), "{created:?}");
}

/// Every edge container of the machine's own instances, with when it started.
fn the_machines_edges() -> Vec<String> {
    let out = Command::new("docker")
        .args([
            "ps",
            "-a",
            "--filter",
            "name=^fundamental-local-edge_",
            "--format",
            "{{.ID}} {{.Names}} {{.State}} {{.CreatedAt}}",
        ])
        .output()
        .expect("docker runs");
    assert!(out.status.success(), "{out:?}");
    let mut edges: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_owned)
        .collect();
    edges.sort();
    edges
}

/// An app: answers every request with the listener it reached and the request head it saw.
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
fn app(address: Ipv4Addr, port: u16) -> TcpListener {
    let listener = TcpListener::bind((address, port)).expect("the app listens");
    let serving = listener.try_clone().expect("a second handle");
    thread::spawn(move || {
        for stream in serving.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().expect("a reader"));
            let mut head = String::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                head.push_str(&line);
            }
            let body = format!("served by {address}\n{head}");
            let mut stream = stream;
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    listener
}

/// `curl` trusting `root` alone, `name` pinned to `address`; the exit status and the body.
fn fetch(root: &Path, name: &str, address: Ipv4Addr, headers: &[&str]) -> (i32, String) {
    let mut command = Command::new("curl");
    command.args([
        "-sS",
        "--max-time",
        "5",
        "--cacert",
        &root.display().to_string(),
        "--capath",
        "/nonexistent",
        "--resolve",
        &format!("{name}:443:{address}"),
    ]);
    for header in headers {
        command.args(["-H", header]);
    }
    let out = command
        .arg(format!("https://{name}/"))
        .output()
        .expect("curl runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

/// Retries `fetch` until the edge answers, which takes a moment after its container starts.
fn eventually(root: &Path, name: &str, address: Ipv4Addr) -> String {
    let started = Instant::now();
    loop {
        let (status, body) = fetch(root, name, address, &[]);
        if status == 0 || started.elapsed() > Duration::from_secs(20) {
            assert_eq!(status, 0, "{name} at {address} never answered: {body}");
            return body;
        }
        thread::sleep(Duration::from_millis(250));
    }
}

#[test]
#[expect(clippy::too_many_lines, reason = "readability baseline")]
fn two_instances_serve_one_app_at_their_own_names_over_https() {
    let _serial = one_at_a_time();
    let dir = tempfile::tempdir().expect("a directory");
    let state = LocalState::at(dir.path().join("state"));
    let machine = LocalMachine::with_edges(state.clone(), Platform::Linux);

    let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .and_then(|listener| listener.local_addr())
        .expect("a free port")
        .port();
    let mut facts: Vec<InstanceFacts> = Vec::new();
    let mut keys = Vec::new();
    for (at, role) in [
        InstanceRole::Main,
        InstanceRole::Lane(InstanceLabel::parse("fix-search").expect("a label")),
    ]
    .into_iter()
    .enumerate()
    {
        let root = dir.path().join(format!("checkout-{at}"));
        let project = project(&root, port);
        let checkout = Checkout::new(root.canonicalize().expect("real"), role);
        let up_facts = up_in_the_tests_pool(&machine, &project, &checkout);
        keys.push(up_facts.key.clone());
        facts.push(up_facts);
    }
    let _edges = Edges(keys);
    let _apps: Vec<TcpListener> = facts.iter().map(|facts| app(facts.ip(), port)).collect();
    let root = facts[0].root.clone();
    let (main, lane) = (&facts[0], &facts[1]);
    assert_ne!(main.address, lane.address);

    let main_name = "www.edgeproof.test";
    let lane_name = "www.fix-search.edgeproof-lane.test";
    let main_body = eventually(&root, main_name, main.ip());
    let lane_body = eventually(&root, lane_name, lane.ip());
    assert!(
        main_body.starts_with(&format!("served by {}", main.address)),
        "{main_body}"
    );
    assert!(
        main_body.contains(&format!("Host: {main_name}")),
        "{main_body}"
    );
    assert!(
        lane_body.starts_with(&format!("served by {}", lane.address)),
        "{lane_body}"
    );
    assert!(
        lane_body.contains(&format!("Host: {lane_name}")),
        "{lane_body}"
    );

    let (code_status, code) = {
        let out = Command::new("curl")
            .args([
                "-s",
                "-o",
                "/dev/null",
                "-w",
                "%{http_code}",
                "--cacert",
                &root.display().to_string(),
                "--resolve",
                &format!("other.edgeproof.test:443:{}", main.ip()),
                "https://other.edgeproof.test/",
            ])
            .output()
            .expect("curl runs");
        (
            out.status.code(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
        )
    };
    assert_eq!(
        (code_status, code.as_str()),
        (Some(0), "421"),
        "an undeclared host"
    );

    let (status, body) = fetch(&root, lane_name, main.ip(), &[]);
    assert_ne!(
        status, 0,
        "main's edge must not serve a lane's name: {body}"
    );
}

#[test]
fn forwarded_headers_are_not_trusted_locally() {
    let _serial = one_at_a_time();
    let dir = tempfile::tempdir().expect("a directory");
    let state = LocalState::at(dir.path().join("state"));
    let machine = LocalMachine::with_edges(state.clone(), Platform::Linux);
    let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .and_then(|listener| listener.local_addr())
        .expect("a free port")
        .port();
    let root = dir.path().join("checkout");
    let project = project(&root, port);
    let checkout = Checkout::new(
        root.canonicalize().expect("real"),
        InstanceRole::Lane(InstanceLabel::parse("forged").expect("a label")),
    );
    let facts = up_in_the_tests_pool(&machine, &project, &checkout);
    let _edges = Edges(vec![facts.key.clone()]);
    let _app = app(facts.ip(), port);
    let name = "www.forged.edgeproof-lane.test";
    let clean = eventually(&facts.root, name, facts.ip());
    let (status, forged) = fetch(
        &facts.root,
        name,
        facts.ip(),
        &[
            "X-Forwarded-For: 203.0.113.9",
            "X-Forwarded-Host: bank.example",
            "X-Forwarded-Proto: http",
            "X-Real-IP: 203.0.113.9",
            "Forwarded: for=203.0.113.9;host=bank.example;proto=http",
        ],
    );
    assert_eq!(status, 0, "{forged}");
    assert_eq!(forged, clean, "the app sees the same request either way");
    for header in ["X-Forwarded", "Forwarded", "X-Real-Ip", "203.0.113.9"] {
        assert!(
            !clean.contains(header),
            "{header} reached the app:\n{clean}"
        );
    }
}

#[test]
fn forget_releases_every_claim() {
    let _serial = one_at_a_time();
    let dir = tempfile::tempdir().expect("a directory");
    let state = LocalState::at(dir.path().join("state"));
    let machine = LocalMachine::with_edges(state.clone(), Platform::Linux);
    let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .and_then(|listener| listener.local_addr())
        .expect("a free port")
        .port();
    let root = dir.path().join("checkout");
    let project = project(&root, port);
    let checkout = Checkout::new(
        root.canonicalize().expect("real"),
        InstanceRole::Lane(InstanceLabel::parse("forgotten").expect("a label")),
    );
    let facts = up_in_the_tests_pool(&machine, &project, &checkout);
    let _edges = Edges(vec![facts.key.clone()]);
    assert!(
        EDGE.present(&facts.key).expect("docker answers"),
        "up runs the edge"
    );
    assert!(facts.tls.exists());

    let given = machine
        .forget(&project, &checkout)
        .expect("nothing is left");
    assert_eq!(given, Some(facts.address));
    assert!(
        !EDGE.present(&facts.key).expect("docker answers"),
        "the edge is gone"
    );
    assert!(
        !state.instance_dir(&facts.key).exists(),
        "the leaf and the edge's file are gone"
    );
    assert!(
        machine
            .store()
            .read()
            .expect("the registry")
            .instance(&facts.key)
            .is_none(),
        "the address is given back"
    );
    let corefile = fs::read_to_string(state.resolver_dir().join("Corefile")).expect("rendered");
    assert!(!corefile.contains("forgotten"), "{corefile}");
}

/// What `docker inspect` says about `container`, through the Go template `format`.
fn inspect(container: &str, format: &str) -> String {
    let out = Command::new("docker")
        .args(["inspect", "-f", format, container])
        .output()
        .expect("docker runs");
    assert!(out.status.success(), "{container} is not a container");
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// The SHA-256 fingerprint of the certificate the edge at `address` presents for `name`.
fn served_fingerprint(name: &str, address: Ipv4Addr) -> String {
    let script = format!(
        "openssl s_client -connect {address}:443 -servername {name} </dev/null 2>/dev/null | openssl x509 -noout -fingerprint -sha256"
    );
    let out = Command::new("sh")
        .args(["-c", &script])
        .output()
        .expect("openssl runs");
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// The SHA-256 fingerprint of the first certificate in the PEM file at `path`.
fn file_fingerprint(path: &Path) -> String {
    let out = Command::new("openssl")
        .args(["x509", "-noout", "-fingerprint", "-sha256", "-in"])
        .arg(path)
        .output()
        .expect("openssl runs");
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

#[test]
fn the_edge_holds_only_its_port_and_its_own_files() {
    let _serial = one_at_a_time();
    let dir = tempfile::tempdir().expect("a directory");
    let state = LocalState::at(dir.path().join("state"));
    let machine = LocalMachine::with_edges(state.clone(), Platform::Linux);
    let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .and_then(|listener| listener.local_addr())
        .expect("a free port")
        .port();
    let root = dir.path().join("checkout");
    let project = project(&root, port);
    let checkout = Checkout::new(
        root.canonicalize().expect("real"),
        InstanceRole::Lane(InstanceLabel::parse("confined").expect("a label")),
    );
    let facts = up_in_the_tests_pool(&machine, &project, &checkout);
    let _edges = Edges(vec![facts.key.clone()]);
    let _app = app(facts.ip(), port);
    let container = EDGE.container(&facts.key);

    let added = inspect(&container, "{{json .HostConfig.CapAdd}}");
    let added: Vec<String> = added
        .trim_matches(|c| c == '[' || c == ']')
        .split(',')
        .map(|cap| cap.trim_matches('"').trim_start_matches("CAP_").to_owned())
        .collect();
    assert_eq!(
        added,
        ["NET_BIND_SERVICE"],
        "every other capability is dropped"
    );
    assert_eq!(
        inspect(&container, "{{range .Mounts}}{{.Source}} {{.RW}};{{end}}"),
        format!("{} false;", state.edge_dir(&facts.key).display()),
        "the edge sees its own directory, read-only, and nothing else"
    );
    assert_eq!(
        inspect(&container, "{{json .HostConfig.SecurityOpt}}"),
        "[\"no-new-privileges\"]"
    );
    assert_eq!(
        inspect(
            &container,
            "{{index .Config.Labels \"fundamental.local.slot\"}}"
        ),
        "lane-confined"
    );
    let name = "www.confined.edgeproof-lane.test";
    eventually(&facts.root, name, facts.ip());
    assert_eq!(
        served_fingerprint(name, facts.ip()),
        file_fingerprint(&facts.tls)
    );
}

#[test]
fn an_interrupted_renewal_is_finished_by_the_next_up() {
    let _serial = one_at_a_time();
    let dir = tempfile::tempdir().expect("a directory");
    let state = LocalState::at(dir.path().join("state"));
    let machine = LocalMachine::with_edges(state.clone(), Platform::Linux);
    let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .and_then(|listener| listener.local_addr())
        .expect("a free port")
        .port();
    let root = dir.path().join("checkout");
    let project = project(&root, port);
    let checkout = Checkout::new(
        root.canonicalize().expect("real"),
        InstanceRole::Lane(InstanceLabel::parse("renewed").expect("a label")),
    );
    let facts = up_in_the_tests_pool(&machine, &project, &checkout);
    let _edges = Edges(vec![facts.key.clone()]);
    let _app = app(facts.ip(), port);
    let name = "www.renewed.edgeproof-lane.test";
    eventually(&facts.root, name, facts.ip());
    let first = served_fingerprint(name, facts.ip());
    assert_eq!(first, file_fingerprint(&facts.tls));

    // An `up` interrupted after writing the new leaf, before recreating the edge.
    fs::remove_file(&facts.tls).expect("the leaf is gone");
    LocalMachine::unmanaged(state.clone(), Platform::Linux)
        .up(&project, &checkout, OffsetDateTime::now_utc())
        .expect("a new leaf, and no edge recreated");
    let renewed = file_fingerprint(&facts.tls);
    assert_ne!(renewed, first, "a new leaf was issued");
    assert_eq!(
        served_fingerprint(name, facts.ip()),
        first,
        "the old edge still serves"
    );

    machine
        .up(&project, &checkout, OffsetDateTime::now_utc())
        .expect("up again, issuing nothing");
    let started = Instant::now();
    while served_fingerprint(name, facts.ip()) != renewed {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "the edge never served the new leaf"
        );
        thread::sleep(Duration::from_millis(250));
    }
}

#[test]
fn an_instance_given_up_takes_its_edge_and_nothing_else_holds_its_address() {
    let _serial = one_at_a_time();
    let dir = tempfile::tempdir().expect("a directory");
    let state = LocalState::at(dir.path().join("state"));
    let machine = LocalMachine::with_edges(state.clone(), Platform::Linux);
    let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .and_then(|listener| listener.local_addr())
        .expect("a free port")
        .port();
    let root = dir.path().join("checkout");
    let project = project(&root, port);
    let real = root.canonicalize().expect("real");
    let first = Checkout::new(
        real.clone(),
        InstanceRole::Lane(InstanceLabel::parse("first").expect("a label")),
    );
    let facts = up_in_the_tests_pool(&machine, &project, &first);
    let ghost = format!(
        "fundamental-local-edge-77_ghost_{}",
        facts.address.ip().octets()[3]
    );

    let mut edges = Edges(vec![facts.key.clone()]);
    assert!(EDGE.present(&facts.key).expect("docker answers"));

    // The checkout switches branch: the first lane is released and its edge goes with it.
    let second = Checkout::new(
        real,
        InstanceRole::Lane(InstanceLabel::parse("second").expect("a label")),
    );
    let moved = machine
        .up(&project, &second, OffsetDateTime::now_utc())
        .expect("the checkout claims another lane")
        .facts;
    edges.0.push(moved.key.clone());
    assert!(
        !EDGE.present(&facts.key).expect("docker answers"),
        "a released instance's edge is removed"
    );
    assert!(!state.instance_dir(&facts.key).exists());

    // An edge nothing gave back, labelled with this address, is evicted first.
    create_on(&ghost, moved.address);
    EDGE.remove(&moved.key).expect("the edge is removed");
    machine
        .up(&project, &second, OffsetDateTime::now_utc())
        .expect("up recreates the edge");
    let left = Command::new("docker")
        .args(["ps", "-aq", "--filter"])
        .arg(format!("name=^{ghost}$"))
        .output()
        .expect("docker runs");
    assert!(
        String::from_utf8_lossy(&left.stdout).trim().is_empty(),
        "the other edge on this address was evicted"
    );
    eventually(&moved.root, "www.second.edgeproof-lane.test", moved.ip());
}

#[test]
fn the_machines_own_edges_are_left_as_they_were() {
    let _serial = one_at_a_time();
    let before = the_machines_edges();
    let dir = tempfile::tempdir().expect("a directory");
    let state = LocalState::at(dir.path().join("state"));
    let machine = LocalMachine::with_edges(state.clone(), Platform::Linux);
    let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .and_then(|listener| listener.local_addr())
        .expect("a free port")
        .port();
    let mut keys = Vec::new();
    for label in ["beside-a", "beside-b"] {
        let root = dir.path().join(label);
        let project = project(&root, port);
        let checkout = Checkout::new(
            root.canonicalize().expect("real"),
            InstanceRole::Lane(InstanceLabel::parse(label).expect("a label")),
        );
        let facts = up_in_the_tests_pool(&machine, &project, &checkout);
        assert!(
            EDGE.present(&facts.key).expect("docker answers"),
            "the test's edge runs"
        );
        keys.push(facts.key);
    }
    drop(Edges(keys));
    assert_eq!(
        the_machines_edges(),
        before,
        "a test's edges started, evicted or recreated an edge of the machine's"
    );
}

#[test]
fn an_edge_of_the_tests_pool_never_evicts_a_container_named_as_the_machines() {
    let _serial = one_at_a_time();
    let dir = tempfile::tempdir().expect("a directory");
    let machine =
        LocalMachine::with_edges(LocalState::at(dir.path().join("state")), Platform::Linux);
    let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .and_then(|listener| listener.local_addr())
        .expect("a free port")
        .port();
    let root = dir.path().join("checkout");
    let project = project(&root, port);
    let checkout = Checkout::new(root.canonicalize().expect("real"), InstanceRole::Main);
    let facts = up_in_the_tests_pool(&machine, &project, &checkout);
    let _edges = Edges(vec![facts.key.clone()]);
    let machines = format!(
        "fundamental-local-edge_edgeproof-kept_{}",
        facts.address.ip().octets()[3]
    );
    create_on(&machines, facts.address);
    EDGE.remove(&facts.key).expect("the edge is removed");
    machine
        .up(&project, &checkout, OffsetDateTime::now_utc())
        .expect("up recreates the edge");
    let kept = Command::new("docker")
        .args(["rm", "-f", &machines])
        .output()
        .expect("docker runs");
    assert!(
        String::from_utf8_lossy(&kept.stdout).trim() == machines,
        "a test's edge evicted a container named as the machine's edges are: {kept:?}"
    );
}
