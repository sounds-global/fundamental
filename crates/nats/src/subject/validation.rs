use crate::error::{NatsError, NatsResult};

/// Validates a subject string, optionally allowing the `*` and `>` wildcards.
pub(super) fn validate_subject(subject: &str, allow_wildcards: bool) -> NatsResult<()> {
    if subject.is_empty() {
        return Err(NatsError::invalid_subject(
            subject,
            "subject cannot be empty",
        ));
    }
    if subject.starts_with('.') || subject.ends_with('.') {
        return Err(NatsError::invalid_subject(
            subject,
            "subject cannot start or end with a dot",
        ));
    }
    if subject.contains("..") {
        return Err(NatsError::invalid_subject(
            subject,
            "subject cannot contain consecutive dots",
        ));
    }

    let tokens: Vec<&str> = subject.split('.').collect();
    for (i, token) in tokens.iter().enumerate() {
        if *token == "*" || *token == ">" {
            if !allow_wildcards {
                return Err(NatsError::invalid_subject(
                    subject,
                    "wildcards are not allowed in concrete subjects",
                ));
            }
            if *token == ">" && i != tokens.len() - 1 {
                return Err(NatsError::invalid_subject(
                    subject,
                    "the '>' wildcard must be the last token",
                ));
            }
            continue;
        }
        validate_token(token)?;
    }
    Ok(())
}

/// Validates a single subject token (no wildcards, no separators).
pub(super) fn validate_token(token: &str) -> NatsResult<()> {
    if token.is_empty() {
        return Err(NatsError::invalid_subject(token, "token cannot be empty"));
    }
    for c in token.chars() {
        if !is_valid_token_char(c) {
            return Err(NatsError::invalid_subject(
                token,
                format!("invalid character '{c}' in token"),
            ));
        }
    }
    Ok(())
}

fn is_valid_token_char(c: char) -> bool {
    c.is_alphanumeric() || c == '-' || c == '_'
}
