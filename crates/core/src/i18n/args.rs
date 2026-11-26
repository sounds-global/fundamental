//! Translation arguments and the dependency-free `{named}` interpolation engine.

#[cfg(not(feature = "std"))]
use alloc::string::String;
use core::fmt;

/// A single named translation argument: `(name, value)`.
pub type Arg<'a> = (&'a str, ArgValue<'a>);

/// A value substituted into a translated message.
///
/// The numeric variants let the MessageFormat 2 engine apply CLDR plural selection.
pub enum ArgValue<'a> {
    /// A borrowed string slice.
    Str(&'a str),
    /// An owned string, such as a field pre-rendered with its format spec.
    Owned(String),
    /// Any `Display` value, stringified on demand.
    Display(&'a dyn fmt::Display),
    /// A signed integer (enables plural-category selection).
    Int(i64),
    /// A floating-point number.
    Float(f64),
}

impl<'a> ArgValue<'a> {
    /// Borrows any [`Display`](fmt::Display) value as an argument.
    pub fn display<T: fmt::Display>(value: &'a T) -> Self {
        ArgValue::Display(value)
    }

    /// Wraps an already-rendered string as an argument.
    pub fn owned(value: String) -> Self {
        ArgValue::Owned(value)
    }
}

impl fmt::Display for ArgValue<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArgValue::Str(s) => f.write_str(s),
            ArgValue::Owned(s) => f.write_str(s),
            ArgValue::Display(d) => d.fmt(f),
            ArgValue::Int(i) => write!(f, "{i}"),
            ArgValue::Float(x) => write!(f, "{x}"),
        }
    }
}

impl<'a> From<i64> for ArgValue<'a> {
    fn from(v: i64) -> Self {
        ArgValue::Int(v)
    }
}

impl<'a> From<f64> for ArgValue<'a> {
    fn from(v: f64) -> Self {
        ArgValue::Float(v)
    }
}

impl<'a> From<&'a str> for ArgValue<'a> {
    fn from(v: &'a str) -> Self {
        ArgValue::Str(v)
    }
}

/// Substitutes `{name}` placeholders in `template` from `args`.
///
/// An unknown `{name}` is left verbatim; `{{` and `}}` are escaped braces; no `:spec` is interpreted.
#[expect(clippy::excessive_nesting, reason = "readability baseline")]
pub fn interpolate(template: &str, args: &[Arg<'_>]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                out.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                out.push('}');
            }
            '{' => {
                let mut name = String::new();
                let mut closed = false;
                for c2 in chars.by_ref() {
                    if c2 == '}' {
                        closed = true;
                        break;
                    }
                    name.push(c2);
                }
                let key = name.split(':').next().unwrap_or("").trim();
                match args.iter().find(|(n, _)| *n == key) {
                    Some((_, value)) => {
                        use fmt::Write as _;
                        let _ = write!(out, "{value}");
                    }
                    None => {
                        out.push('{');
                        out.push_str(&name);
                        if closed {
                            out.push('}');
                        }
                    }
                }
            }
            _ => out.push(ch),
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(feature = "std"))]
    use alloc::string::ToString;

    #[test]
    fn substitutes_named_args() {
        let id = 42;
        let args = [("id", ArgValue::display(&id))];
        assert_eq!(
            interpolate("User {id} not found", &args),
            "User 42 not found"
        );
    }

    #[test]
    fn handles_escaped_braces() {
        assert_eq!(interpolate("a {{literal}} b", &[]), "a {literal} b");
    }

    #[test]
    fn leaves_unknown_placeholder_verbatim() {
        assert_eq!(interpolate("hi {name}", &[]), "hi {name}");
    }

    #[test]
    fn ignores_runtime_spec() {
        let n = 7;
        let args = [("n", ArgValue::Int(7)), ("ignored", ArgValue::display(&n))];
        assert_eq!(interpolate("{n:04} items", &args), "7 items");
    }

    #[test]
    fn arg_value_display_matches() {
        assert_eq!(ArgValue::Int(3).to_string(), "3");
        assert_eq!(ArgValue::from("hi").to_string(), "hi");
    }
}
