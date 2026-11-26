//! A project whose loaders build words through the ambient translator, served by `tests/language.rs`.

mod app_state;
mod legacy_data;
mod word;

pub use app_state::AppState;
pub use legacy_data::LegacyData;
pub use word::Word;

/// The state the pipeline tree is served over.
pub type PageState = AppState;

/// The word a loader builds, read after an `.await`, so the language must hold across every poll.
pub async fn loaded_word() -> Word {
    tokio::task::yield_now().await;
    Word(
        fundamental::core::i18n::translate("fixture.word", &[], || "word".into()).into_owned(),
    )
}

/// The registration of the tree that only exports `Data`.
pub mod legacy {
    include!(concat!(env!("OUT_DIR"), "/legacy/pages.rs"));
}

/// The registration of the tree with a `loader.rs`.
pub mod pipeline {
    include!(concat!(env!("OUT_DIR"), "/pipeline/pages.rs"));
}
