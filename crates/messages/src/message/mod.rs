//! [`Message`], [`Value`] and [`Catalogs`]: a message and the compiled catalogs it is said from.

use std::fmt;
use std::sync::OnceLock;

use mf2_catalog::Catalog;
use mf2_runtime::{Arg, FormatContext, Formatter, Registry};

/// The functions a message may call: CLDR functions, `:string` and `:brand`.
static FUNCTIONS: [(&str, &dyn mf2_runtime::Function); 6] = [
    ("string", &mf2_runtime::functions::STRING),
    ("integer", &mf2_fn_number::INTEGER),
    ("number", &mf2_fn_number::NUMBER),
    ("percent", &mf2_fn_number::PERCENT),
    ("currency", &mf2_fn_number::CURRENCY),
    ("brand", &crate::brand_terms::BRAND),
];

/// The names of the functions a message may call: the messages build refuses any other.
pub const FUNCTION_NAMES: [&str; 6] = [
    "string", "integer", "number", "percent", "currency", "brand",
];

static REGISTRY: Registry = Registry::new(&FUNCTIONS).with_numbers(&mf2_fn_number::NUMBERS);

/// Placeholders isolated with U+2066–U+2069, for a language written right to left.
static ISOLATED: FormatContext = FormatContext::new(&mf2_host_std::HOST);

/// Placeholders as they are, for a language written left to right.
static PLAIN: FormatContext = plain();

const fn plain() -> FormatContext {
    let mut context = FormatContext::new(&mf2_host_std::HOST);
    context.bidi = mf2_runtime::BidiStrategy::None;
    context
}

/// One crate's compiled catalogs: its source language and each language's catalog.
pub struct Catalogs {
    source: &'static str,
    manifest: u64,
    files: &'static [(&'static str, &'static [u8], &'static [u8])],
    chains: &'static [(&'static str, &'static [&'static str])],
    read: OnceLock<Vec<(&'static str, Catalog)>>,
}

impl Catalogs {
    /// The catalogs a crate's build compiled, `source` the language its messages are written in.
    #[must_use]
    pub const fn new(
        source: &'static str,
        manifest: u64,
        files: &'static [(&'static str, &'static [u8], &'static [u8])],
    ) -> Self {
        Self {
            source,
            manifest,
            files,
            chains: &[],
            read: OnceLock::new(),
        }
    }

    /// It with each language's fallback chain, as the project's policy names them.
    #[must_use]
    pub const fn falling_back(
        mut self,
        chains: &'static [(&'static str, &'static [&'static str])],
    ) -> Self {
        self.chains = chains;
        self
    }

    /// The catalog for `tag`, or for the first language in its chain the crate has.
    fn catalog(&self, tag: &str) -> Option<&Catalog> {
        self.first_of(self.chain(tag).into_iter().chain([self.source]))
    }

    /// The catalog in `tag`'s own language alone: `nl-BE`'s or `nl`'s for `nl-BE`.
    fn own_catalog(&self, tag: &str) -> Option<&Catalog> {
        let language = primary(tag);
        self.first_of(
            self.chain(tag)
                .into_iter()
                .chain([self.source])
                .filter(|wanted| primary(wanted) == language),
        )
    }

    fn first_of<'t>(&self, mut wanted: impl Iterator<Item = &'t str>) -> Option<&Catalog> {
        let read = self.read.get_or_init(|| {
            self.files
                .iter()
                .filter_map(|(tag, bytes, server)| {
                    let catalog = Catalog::from_static(bytes, self.manifest).ok()?;
                    let catalog = catalog.with_server_data(server).ok()?;
                    Some((*tag, catalog))
                })
                .collect()
        });
        wanted
            .find_map(|wanted| read.iter().find(|(have, _)| *have == wanted))
            .map(|(_, catalog)| catalog)
    }

    /// `tag`, then the languages its chain names.
    fn chain<'t>(&self, tag: &'t str) -> Vec<&'t str>
    where
        'static: 't,
    {
        let mut chain = vec![tag];
        if let Some((_, named)) = self.chains.iter().find(|(language, _)| *language == tag) {
            chain.extend(named.iter().copied());
        } else if let Some((language, _)) = tag.split_once(['-', '_']) {
            chain.extend(self.chain(language));
        }
        chain
    }

    /// Every language it has a catalog for, the source among them.
    pub fn languages(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.files.iter().map(|(tag, _, _)| *tag)
    }

    /// The language its messages are written in.
    #[must_use]
    pub const fn source(&self) -> &'static str {
        self.source
    }

    /// Whether it has message `id`.
    #[must_use]
    pub fn carries(&self, id: &str) -> bool {
        self.catalog(self.source)
            .is_some_and(|catalog| catalog.lookup(id).is_some())
    }

    /// Message `id` said in `tag`'s language or the nearest it has; nothing when it cannot be said.
    #[must_use]
    pub fn say(&self, id: &str, tag: &str, values: &[(&str, Value)]) -> Option<String> {
        say_from(self.catalog(tag)?, id, values)
    }

    /// Message `id` said in `tag`'s own language and no other.
    #[must_use]
    pub fn say_only_in(&self, id: &str, tag: &str, values: &[(&str, Value)]) -> Option<String> {
        say_from(self.own_catalog(tag)?, id, values)
    }
}

/// `tag`'s primary language: `nl` for `nl-BE`.
fn primary(tag: &str) -> &str {
    tag.split(['-', '_']).next().unwrap_or(tag)
}

/// Message `id` of `catalog`, or nothing when it has none or a value is missing.
fn say_from(catalog: &Catalog, id: &str, values: &[(&str, Value)]) -> Option<String> {
    let id = catalog.lookup(id)?;
    let args: Vec<(&str, Arg<'_>)> = values
        .iter()
        .map(|(name, value)| (*name, value.arg()))
        .collect();
    let mut out = String::new();
    let mut errors: Vec<mf2_runtime::FormatError> = Vec::new();
    let context = if catalog.dir() == mf2_catalog::Dir::Rtl {
        &ISOLATED
    } else {
        &PLAIN
    };
    Formatter::new(catalog, &REGISTRY, context).write_named(id, &args, &mut out, &mut errors);
    errors.is_empty().then_some(out)
}

/// A value a message is said with.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// Text, as given.
    Text(String),
    /// A whole number.
    Integer(i64),
    /// A number.
    Number(f64),
}

impl Value {
    /// `shown` as text.
    pub fn text(shown: impl fmt::Display) -> Self {
        Self::Text(shown.to_string())
    }

    fn as_arg(&self) -> fundamental_core::i18n::ArgValue<'_> {
        use fundamental_core::i18n::ArgValue;
        match self {
            Self::Text(text) => ArgValue::Str(text),
            Self::Integer(number) => ArgValue::Int(*number),
            Self::Number(number) => ArgValue::Float(*number),
        }
    }

    fn arg(&self) -> Arg<'_> {
        match self {
            Self::Text(text) => Arg::Str(text),
            Self::Integer(number) => Arg::Int(*number),
            Self::Number(number) => Arg::Float(*number),
        }
    }
}

impl From<i64> for Value {
    fn from(number: i64) -> Self {
        Self::Integer(number)
    }
}

impl From<f64> for Value {
    fn from(number: f64) -> Self {
        Self::Number(number)
    }
}

/// A message to say, with its values, said in the reader's language when shown.
#[derive(Clone)]
pub struct Message {
    catalogs: &'static Catalogs,
    id: &'static str,
    values: Vec<(&'static str, Value)>,
}

impl Message {
    /// Message `id` of `catalogs`, said with `values`.
    #[must_use]
    pub fn new(
        catalogs: &'static Catalogs,
        id: &'static str,
        values: Vec<(&'static str, Value)>,
    ) -> Self {
        Self {
            catalogs,
            id,
            values,
        }
    }

    /// Its id: `panel.version`.
    #[must_use]
    pub const fn id(&self) -> &'static str {
        self.id
    }

    /// It said in `tag`'s own language and no other; nothing when its crate has no words there.
    #[must_use]
    pub fn format_only_in(&self, tag: &str) -> Option<String> {
        self.catalogs.say_only_in(self.id, tag, &self.values)
    }

    /// It said in `tag`'s language, or the nearest one its crate has; its id when it cannot be said.
    #[must_use]
    pub fn format_in(&self, tag: &str) -> String {
        self.catalogs
            .say(self.id, tag, &self.values)
            .unwrap_or_else(|| self.id.to_owned())
    }
}

impl fmt::Display for Message {
    /// The app's own words for it when the app wrote some, or it in the reader's language.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Some(translator) = fundamental_core::i18n::current() else {
            return f.write_str(&self.format_in(self.catalogs.source));
        };
        let args: Vec<fundamental_core::i18n::Arg<'_>> = self
            .values
            .iter()
            .map(|(name, value)| (*name, value.as_arg()))
            .collect();
        match translator.own_words(self.id, &args) {
            Some(own) => f.write_str(&own),
            None => f.write_str(&self.format_in(translator.locale_tag())),
        }
    }
}

impl fmt::Debug for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Message")
            .field("id", &self.id)
            .field("values", &self.values)
            .finish()
    }
}

/// A message in a view renders as its words in the page's language.
#[cfg(feature = "leptos")]
impl leptos::prelude::IntoRender for Message {
    type Output = String;

    fn into_render(self) -> Self::Output {
        self.to_string()
    }
}

#[cfg(feature = "leptos")]
impl leptos::prelude::IntoAttributeValue for Message {
    type Output = String;

    fn into_attribute_value(self) -> Self::Output {
        self.to_string()
    }
}

impl From<Message> for String {
    fn from(message: Message) -> Self {
        message.to_string()
    }
}
