//! A set-only app declares its surface with `mount_messages! { sets: … }`.

use fundamental_macros::{MessageSet, mount_messages};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct ForkDetected {
    origin: String,
}

#[derive(Serialize, Deserialize)]
struct AnchorsChanged {
    source: String,
}

#[derive(MessageSet)]
enum Events {
    #[subject = "acme.audit.fork"]
    ForkDetected(ForkDetected),
    #[subject = "acme.interop.anchors_changed"]
    AnchorsChanged(AnchorsChanged),
}

mount_messages! { sets: Events }

#[test]
fn describe_messages_yields_only_the_set_variants() {
    let subjects: Vec<_> = describe_messages().iter().map(|d| d.subject).collect();
    assert_eq!(
        subjects,
        ["acme.audit.fork", "acme.interop.anchors_changed"]
    );
}
