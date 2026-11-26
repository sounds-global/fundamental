/// Whether an `If-None-Match` value names `etag` by weak comparison (RFC 9110 §13.1.2).
pub(crate) fn if_none_match(header: &str, etag: &str) -> bool {
    let ours = opaque(etag);
    header
        .split(',')
        .map(str::trim)
        .any(|tag| tag == "*" || opaque(tag) == ours)
}

fn opaque(tag: &str) -> &str {
    tag.strip_prefix("W/").unwrap_or(tag)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ETAG: &str = "\"q83vEjRWeJCrze8SNFZ4kKvN7xI0VniQq83vEjRWeJA\"";

    #[test]
    fn the_same_tag_a_star_and_a_list_holding_it_match() {
        assert!(if_none_match(ETAG, ETAG));
        assert!(if_none_match("*", ETAG));
        assert!(if_none_match(&format!("\"old\", {ETAG}"), ETAG));
    }

    #[test]
    fn a_weak_tag_compares_weakly() {
        assert!(if_none_match(&format!("W/{ETAG}"), ETAG));
    }

    #[test]
    fn another_tag_an_unquoted_one_and_an_empty_header_do_not_match() {
        assert!(!if_none_match("\"old\"", ETAG));
        assert!(!if_none_match(&ETAG[1..ETAG.len() - 1], ETAG));
        assert!(!if_none_match("", ETAG));
    }
}
