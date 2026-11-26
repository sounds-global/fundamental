//! [`escape`] — HTML escaping for a value going into element content or an attribute.

#[cfg(not(feature = "std"))]
use alloc::string::String;

/// Escapes a value for both element content and a double-quoted attribute, HTML or XML.
///
/// `&` first, or it would re-escape the ampersands the later replacements introduce.
pub fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::escape;

    #[test]
    fn the_ampersand_is_escaped_once() {
        // `&` first, or `<` becomes `&amp;lt;`: a visible defect rather than a safe one.
        assert_eq!(escape("a & b < c"), "a &amp; b &lt; c");
    }

    #[test]
    fn a_value_cannot_close_the_element_it_sits_in() {
        assert_eq!(
            escape("</script><script>alert(1)</script>"),
            "&lt;/script&gt;&lt;script&gt;alert(1)&lt;/script&gt;"
        );
    }

    #[test]
    fn a_value_cannot_open_an_attribute_of_its_own() {
        assert_eq!(escape("a\" onload=\"x"), "a&quot; onload=&quot;x");
        assert_eq!(escape("a' onload='x"), "a&#39; onload=&#39;x");
    }
}
