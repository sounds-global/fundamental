//! `trust` installs a root into an NSS database and reads the same root back.

use fundamental_core::address::TestApex;
use fundamental_local::{ApexSet, ProjectName, ProjectRoot, TrustStore};
use time::OffsetDateTime;

#[test]
fn trust_installs_into_an_nss_database_and_reads_it_back() {
    let dir = tempfile::tempdir().expect("a directory");
    let database = dir.path().join("nssdb");
    let project = ProjectName::parse("acme").expect("a name");
    let apexes = ApexSet::new(TestApex::parse("acme.test")).expect("one");
    let first = ProjectRoot::mint(&project, &apexes, OffsetDateTime::now_utc()).expect("a root");
    let store = TrustStore::Nss(database.clone());
    assert!(!store.holds(&project, first.der()));
    store
        .install(&project, first.der())
        .expect("installed and read back");
    assert!(store.holds(&project, first.der()));

    let second = ProjectRoot::mint(&project, &apexes, OffsetDateTime::now_utc()).expect("a root");
    store
        .install(&project, second.der())
        .expect("a re-minted root replaces the project's own");
    assert!(store.holds(&project, second.der()));
    assert!(!store.holds(&project, first.der()));
}
