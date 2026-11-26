//! `#[derive(MessageSet)]` rejects every non-set shape at compile time.
//!
//! Each enum below trips one of the derive's guards — a non-enum target, an empty enum, a
//! unit (payload-less) variant, and two variants colliding on one subject. Expected to fail;
//! the paired `message_set_invalid.stderr` snapshot pins the diagnostics.

use fundamental_macros::MessageSet;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Payload {
    id: u64,
}

// A set must be an enum, not a struct.
#[derive(MessageSet)]
struct NotAnEnum {
    id: u64,
}

// An empty set would subscribe to nothing.
#[derive(MessageSet)]
enum Empty {}

// Every variant must carry exactly one payload.
#[derive(MessageSet)]
enum HasUnitVariant {
    Ok(Payload),
    Bare,
}

// Two variants cannot share a subject — dispatch would be ambiguous.
#[derive(MessageSet)]
enum DuplicateSubject {
    #[subject = "events.thing"]
    First(Payload),
    #[subject = "events.thing"]
    Second(Payload),
}

fn main() {}
