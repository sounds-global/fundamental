use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use async_nats::jetstream::stream::{Compression, Config};

/// Every field of `actual` that is not what `declared` asks for, by name.
///
/// Both configurations are destructured in full, so a new async-nats field fails to compile
/// here. Server normalisation reads as equal, and the server-only fields are not compared.
#[expect(clippy::too_many_lines, reason = "readability baseline")]
pub(crate) fn stream_drift(declared: &Config, actual: &Config) -> Vec<&'static str> {
    let Config {
        name,
        max_bytes,
        max_messages,
        max_messages_per_subject,
        discard,
        discard_new_per_subject,
        subjects,
        retention,
        max_consumers,
        max_age,
        max_message_size,
        storage,
        num_replicas,
        no_ack,
        duplicate_window,
        template_owner,
        sealed,
        description,
        allow_rollup,
        deny_delete,
        deny_purge,
        republish,
        allow_direct: _,
        mirror_direct,
        mirror,
        sources,
        metadata,
        subject_transform,
        compression,
        consumer_limits: _,
        first_sequence: _,
        placement: _,
        persist_mode,
        pause_until,
        allow_message_ttl,
        subject_delete_marker_ttl,
        allow_atomic_publish,
        allow_message_schedules,
        allow_message_counter,
        allow_batch_publish,
    } = declared;

    let mut drift = Vec::new();
    let mut differs = |field: &'static str, same: bool| {
        if !same {
            drift.push(field);
        }
    };
    differs("name", *name == actual.name);
    differs("subjects", as_set(subjects) == as_set(&actual.subjects));
    differs("description", *description == actual.description);
    differs("max_bytes", limit(*max_bytes) == limit(actual.max_bytes));
    differs(
        "max_messages",
        limit(*max_messages) == limit(actual.max_messages),
    );
    differs(
        "max_messages_per_subject",
        limit(*max_messages_per_subject) == limit(actual.max_messages_per_subject),
    );
    differs(
        "max_consumers",
        limit(*max_consumers) == limit(actual.max_consumers),
    );
    differs(
        "max_message_size",
        limit(*max_message_size) == limit(actual.max_message_size),
    );
    differs("max_age", *max_age == actual.max_age);
    differs(
        "duplicate_window",
        window(*duplicate_window, actual.duplicate_window),
    );
    differs("discard", *discard == actual.discard);
    differs(
        "discard_new_per_subject",
        *discard_new_per_subject == actual.discard_new_per_subject,
    );
    differs("retention", *retention == actual.retention);
    differs("storage", *storage == actual.storage);
    differs(
        "num_replicas",
        (*num_replicas).max(1) == actual.num_replicas.max(1),
    );
    differs("no_ack", *no_ack == actual.no_ack);
    differs("template_owner", *template_owner == actual.template_owner);
    differs("sealed", *sealed == actual.sealed);
    differs("allow_rollup", *allow_rollup == actual.allow_rollup);
    differs("deny_delete", *deny_delete == actual.deny_delete);
    differs("deny_purge", *deny_purge == actual.deny_purge);
    differs("republish", *republish == actual.republish);
    differs("mirror_direct", *mirror_direct == actual.mirror_direct);
    differs("mirror", *mirror == actual.mirror);
    differs(
        "sources",
        sources.as_deref().unwrap_or_default() == actual.sources.as_deref().unwrap_or_default(),
    );
    differs(
        "metadata",
        declared_keys(metadata) == declared_keys(&actual.metadata),
    );
    differs(
        "subject_transform",
        *subject_transform == actual.subject_transform,
    );
    differs(
        "compression",
        compressed(compression.as_ref()) == compressed(actual.compression.as_ref()),
    );
    differs(
        "persist_mode",
        persist_mode.unwrap_or_default() == actual.persist_mode.unwrap_or_default(),
    );
    differs("pause_until", *pause_until == actual.pause_until);
    differs(
        "allow_message_ttl",
        *allow_message_ttl == actual.allow_message_ttl,
    );
    differs(
        "subject_delete_marker_ttl",
        *subject_delete_marker_ttl == actual.subject_delete_marker_ttl,
    );
    differs(
        "allow_atomic_publish",
        *allow_atomic_publish == actual.allow_atomic_publish,
    );
    differs(
        "allow_message_schedules",
        *allow_message_schedules == actual.allow_message_schedules,
    );
    differs(
        "allow_message_counter",
        *allow_message_counter == actual.allow_message_counter,
    );
    differs(
        "allow_batch_publish",
        *allow_batch_publish == actual.allow_batch_publish,
    );
    drift
}

/// A limit, with every spelling of "unlimited" (`0`, `-1`) read as one: `None`.
pub(super) fn limit<T: Copy + Into<i64>>(value: T) -> Option<T> {
    (value.into() > 0).then_some(value)
}

/// Whether a duplicate window matches: one the declaration left unset is the server's.
fn window(declared: Duration, actual: Duration) -> bool {
    declared.is_zero() || declared == actual
}

fn as_set(subjects: &[String]) -> BTreeSet<&str> {
    subjects.iter().map(String::as_str).collect()
}

/// The metadata a stream's owner wrote, without the `_nats.` keys the server stamps itself.
fn declared_keys(metadata: &std::collections::HashMap<String, String>) -> BTreeMap<&str, &str> {
    metadata
        .iter()
        .filter(|(key, _)| !key.starts_with("_nats."))
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect()
}

/// Whether a stream is compressed: unset and `none` are the same answer.
fn compressed(compression: Option<&Compression>) -> bool {
    matches!(compression, Some(Compression::S2))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use async_nats::jetstream::stream::{Config, StorageType};

    use super::stream_drift;

    fn declared() -> Config {
        Config {
            name: "ORDERS".to_owned(),
            subjects: vec!["orders.>".to_owned()],
            max_bytes: 1_000_000,
            deny_delete: true,
            ..Config::default()
        }
    }

    #[test]
    fn the_servers_spelling_of_unlimited_and_its_own_stamps_are_not_drift() {
        let mut actual = declared();
        actual.max_messages = -1;
        actual.max_consumers = -1;
        actual.max_message_size = -1;
        actual.max_messages_per_subject = -1;
        actual.duplicate_window = Duration::from_secs(120);
        actual.allow_direct = true;
        actual.first_sequence = Some(1);
        actual
            .metadata
            .insert("_nats.req.level".to_owned(), "0".to_owned());
        assert!(stream_drift(&declared(), &actual).is_empty());
    }

    #[test]
    fn every_field_that_changes_the_stream_is_named() {
        let mut actual = declared();
        actual.max_bytes = 10;
        actual.deny_delete = false;
        actual.storage = StorageType::Memory;
        actual.allow_rollup = true;
        actual.subjects.push("payments.>".to_owned());
        let mut drift = stream_drift(&declared(), &actual);
        drift.sort_unstable();
        assert_eq!(
            drift,
            [
                "allow_rollup",
                "deny_delete",
                "max_bytes",
                "storage",
                "subjects"
            ]
        );
    }

    #[test]
    fn a_declared_duplicate_window_is_held_to() {
        let mut declared = declared();
        declared.duplicate_window = Duration::from_secs(7200);
        let mut actual = declared.clone();
        actual.duplicate_window = Duration::from_secs(120);
        assert_eq!(stream_drift(&declared, &actual), ["duplicate_window"]);
    }
}
