//! The root and the leaves `fun local` issues verify only for the project's own `.test` names.

use std::net::{IpAddr, Ipv4Addr};

use fundamental_core::Error as _;
use fundamental_core::address::TestApex;
use fundamental_local::{
    ApexSet, CertificateProof, InstanceLeaf, LocalState, Probe, ProjectName, ProjectRoot,
    RootProfile,
};
use rcgen::{BasicConstraints, CertificateParams, IsCa, KeyPair};
use time::OffsetDateTime;

fn apexes(texts: &[&str]) -> ApexSet {
    ApexSet::new(
        texts
            .iter()
            .map(|text| TestApex::parse(text).expect("an apex")),
    )
    .expect("one")
}

fn root(project: &str, texts: &[&str]) -> ProjectRoot {
    ProjectRoot::mint(
        &ProjectName::parse(project).expect("a name"),
        &apexes(texts),
        OffsetDateTime::now_utc(),
    )
    .expect("a root")
}

fn names(suffix: &str) -> Vec<String> {
    vec![suffix.to_owned(), format!("*.{suffix}")]
}

fn outside() -> Vec<Probe> {
    vec![
        Probe::Name("bank.nl".to_owned()),
        Probe::Name("probe.shop.test".to_owned()),
        Probe::Address(IpAddr::V4(Ipv4Addr::LOCALHOST)),
    ]
}

#[test]
fn the_root_is_a_pathlen_zero_server_auth_ca() {
    let root = root("acme", &["acme.test", "acme-sandbox.test"]);
    let profile = RootProfile::exact(root.der()).expect("the minted profile");
    assert_eq!(
        profile.permitted(),
        [
            "acme-lane.test",
            "acme-sandbox-lane.test",
            "acme-sandbox.test",
            "acme.test"
        ],
        "each apex and its lanes twin, a site of its own"
    );
}

#[test]
fn trust_refuses_a_root_whose_profile_is_wrong() {
    let key = KeyPair::generate().expect("a key");
    let mut params = CertificateParams::new(Vec::<String>::new()).expect("params");
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let unconstrained = params.self_signed(&key).expect("a certificate");
    let refused = RootProfile::exact(unconstrained.der()).expect_err("no name constraint");
    assert_eq!(refused.error_code(), "LOCAL_006");
    let detail = refused.to_string();
    assert!(detail.contains("pathlen:0"), "{detail}");
    assert!(detail.contains("name constraint"), "{detail}");
}

#[test]
fn a_real_domain_does_not_verify() {
    let root = root("acme", &["acme.test"]);
    let proof = CertificateProof::at(OffsetDateTime::now_utc());
    proof
        .root(&root, "probe.acme.test", &outside())
        .expect("the root refuses a real domain, another project's name and an address");
}

#[test]
fn a_leaf_for_another_projects_name_does_not_verify() {
    let ours = root("acme", &["acme.test"]);
    let theirs = root("shop", &["shop.test"]);
    let now = OffsetDateTime::now_utc();
    let leaf = InstanceLeaf::issue(&theirs, &names("shop.test"), now).expect("a leaf");
    let proof = CertificateProof::at(now);
    assert!(!proof.verifies(ours.der(), leaf.der(), "www.shop.test"));
    assert!(proof.verifies(theirs.der(), leaf.der(), "www.shop.test"));
}

#[test]
fn a_leaf_with_a_preview_wildcard_covers_one_level_under_its_label_for_main_and_a_lane() {
    let root = root("shop", &["shop.test"]);
    let now = OffsetDateTime::now_utc();
    let proof = CertificateProof::at(now);
    for suffix in ["shop.test", "fix-cart.shop-lane.test"] {
        let mut names = names(suffix);
        names.push(format!("*.preview.{suffix}"));
        let leaf =
            InstanceLeaf::issue(&root, &names, now).expect("a leaf the root's constraint admits");

        proof
            .leaf(
                root.der(),
                leaf.der(),
                &[
                    format!("mijn.{suffix}"),
                    format!("k7q2m4x9z3p5r6t8.preview.{suffix}"),
                ],
                &[
                    format!("a.b.preview.{suffix}"),
                    format!("a.other.{suffix}"),
                    "k7q2m4x9z3p5r6t8.preview.shop.nl".to_owned(),
                ],
            )
            .expect("one level under preview, and nothing deeper or beside it");
    }
}

#[test]
fn a_main_leaf_does_not_cover_a_lane() {
    let root = root("acme", &["acme.test"]);
    let now = OffsetDateTime::now_utc();
    let main = InstanceLeaf::issue(&root, &names("acme.test"), now).expect("main's leaf");
    let lane =
        InstanceLeaf::issue(&root, &names("fix-login.acme-lane.test"), now).expect("a lane's");
    let proof = CertificateProof::at(now);
    proof
        .leaf(
            root.der(),
            main.der(),
            &["acme.test".to_owned(), "www.acme.test".to_owned()],
            &[
                "www.fix-login.acme-lane.test".to_owned(),
                "a.b.acme.test".to_owned(),
                "www.acme.nl".to_owned(),
            ],
        )
        .expect("main covers main and nothing else");
    proof
        .leaf(
            root.der(),
            lane.der(),
            &["www.fix-login.acme-lane.test".to_owned()],
            &[
                "www.acme.test".to_owned(),
                "www.other.acme-lane.test".to_owned(),
            ],
        )
        .expect("a lane covers its own names and nothing else");
}

#[test]
fn the_probe_would_catch_an_unconstrained_root() {
    let key = KeyPair::generate().expect("a key");
    let mut params = CertificateParams::new(Vec::<String>::new()).expect("params");
    params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    params.key_usages = vec![rcgen::KeyUsagePurpose::KeyCertSign];
    let unconstrained = params.self_signed(&key).expect("a certificate");
    let state = tempfile::tempdir().expect("a directory");
    let state = LocalState::at(state.path());
    let pem = unconstrained.pem();
    let fingerprint = fundamental_local::RootFingerprint::of_der(unconstrained.der());
    let dir = state.root_dir(&fingerprint);
    std::fs::create_dir_all(&dir).expect("a root directory");
    std::fs::write(dir.join("root.pem"), pem).expect("the certificate");
    std::fs::write(dir.join("key.pem"), key.serialize_pem()).expect("the key");
    let loaded = ProjectRoot::load(
        &state,
        &ProjectName::parse("x").expect("a name"),
        &fingerprint,
    )
    .expect("readable");
    let refused = CertificateProof::at(OffsetDateTime::now_utc())
        .root(&loaded, "probe.acme.test", &outside())
        .expect_err("an unconstrained root signs a real domain, and the probe must say so");
    assert_eq!(refused.error_code(), "LOCAL_006");
    assert!(refused.to_string().contains("bank.nl"), "{refused}");
}

#[test]
fn a_stored_root_reads_back_and_its_key_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("a directory");
    let state = LocalState::at(dir.path().join("local"));
    let project = ProjectName::parse("acme").expect("a name");
    let minted = root("acme", &["acme.test"]);
    minted.store(&state).expect("stored");
    assert!(
        minted.store(&state).is_err(),
        "a stored root is never overwritten"
    );
    let key = state.root_dir(&minted.fingerprint()).join("key.pem");
    let mode = std::fs::metadata(&key)
        .expect("the key")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
    let loaded = ProjectRoot::load(&state, &project, &minted.fingerprint()).expect("loaded");
    assert_eq!(loaded.fingerprint(), minted.fingerprint());
    let gone = ProjectRoot::load(
        &state,
        &project,
        &fundamental_local::RootFingerprint::of_der(b"other"),
    )
    .map(|_| ())
    .map_err(|error| error.error_code());
    assert_eq!(gone, Err("LOCAL_016"));
}
