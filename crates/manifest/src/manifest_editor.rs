//! Changing a project's `fun.toml`, keeping its comments and layout.

use std::path::{Path, PathBuf};

use fundamental_core::fs::replace_file;
use toml_edit::{DocumentMut, Item, Table};
use tracing::{info, instrument};

use crate::edit_lock::EditLock;
use crate::files::{self, DEVELOPMENT, FILE};
use crate::manifest::resolve;
use crate::source_file::SourceFile;
use crate::{KeyPath, Manifest, ManifestError};

/// An editor of a project's `fun.toml` that keeps its comments and layout; only `fun.toml`,
/// since override files are written by people.
#[derive(Debug)]
pub struct ManifestEditor {
    root: PathBuf,
    source_dir: PathBuf,
    original: String,
    document: DocumentMut,
}

impl ManifestEditor {
    /// Opens the manifest of the project at `root` for editing.
    ///
    /// # Errors
    ///
    /// As [`Manifest::load`]: a manifest that does not load cannot be edited.
    #[instrument(skip_all, fields(root = %root.as_ref().display()), err(Display))]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    pub fn open(root: impl AsRef<Path>) -> Result<Self, ManifestError> {
        let (root, source_dir) = resolve(root.as_ref())?;
        let original = files::read_current(&source_dir)?.unwrap_or_default();
        let document = original
            .parse::<DocumentMut>()
            .map_err(|error| ManifestError::BadKey {
                key: FILE.to_owned(),
                cause: error.to_string(),
            })?;
        let editor = Self {
            root,
            source_dir,
            original,
            document,
        };
        editor.manifest()?;
        Ok(editor)
    }

    /// `fun.toml` as it would read with the changes made so far.
    ///
    /// # Errors
    ///
    /// [`ManifestError::Refused`] when the changes leave invalid settings.
    pub fn manifest(&self) -> Result<Manifest, ManifestError> {
        let source = SourceFile {
            file: PathBuf::from(FILE),
            text: self.document.to_string(),
        };
        Manifest::build(
            &self.root,
            &self.source_dir,
            DEVELOPMENT,
            &[source],
            Vec::new(),
        )
    }

    /// The table at `key`, created (with implicit parents) when it is not there yet.
    ///
    /// # Errors
    ///
    /// [`ManifestError::BadKey`] when something other than a table already stands at `key`
    /// or above it.
    pub fn table_mut(&mut self, key: &KeyPath) -> Result<&mut Table, ManifestError> {
        let mut table = self.document.as_table_mut();
        let last = key.segments().len().saturating_sub(1);
        for (index, segment) in key.segments().iter().enumerate() {
            let item = table.entry(segment).or_insert_with(|| {
                let mut created = Table::new();
                created.set_implicit(index < last);
                Item::Table(created)
            });
            table = item.as_table_mut().ok_or_else(|| ManifestError::BadKey {
                key: key.to_string(),
                cause: format!("`{segment}` is a value, not a table"),
            })?;
        }
        Ok(table)
    }

    /// Writes `fun.toml` when it changed, and answers whether it did.
    ///
    /// # Errors
    ///
    /// [`ManifestError::Refused`] when the changes leave invalid settings (nothing is
    /// written), [`ManifestError::ChangedSinceRead`] when `fun.toml` changed since
    /// [`open`](Self::open), and [`ManifestError::Io`] when the write fails.
    #[instrument(skip_all, fields(source_dir = %self.source_dir.display()))]
    pub fn save(&mut self) -> Result<bool, ManifestError> {
        self.manifest()?;
        let text = self.document.to_string();
        if text == self.original {
            return Ok(false);
        }
        let _lock = EditLock::acquire(&self.source_dir)?;
        self.unchanged_on_disk()?;
        self.write(text)?;
        Ok(true)
    }

    /// Replaces `fun.toml` with `text`, which is then what the editor read.
    fn write(&mut self, text: String) -> Result<(), ManifestError> {
        let target = self.source_dir.join(FILE);
        replace_file(&target, text.as_bytes())
            .map_err(|error| ManifestError::io(&target, &error))?;
        self.original = text;
        info!(file = FILE, "settings saved");
        Ok(())
    }

    /// Refuses when `fun.toml` on disk is no longer what [`open`](Self::open) read.
    fn unchanged_on_disk(&self) -> Result<(), ManifestError> {
        if files::read_current(&self.source_dir)?.unwrap_or_default() == self.original {
            Ok(())
        } else {
            Err(ManifestError::ChangedSinceRead {
                file: FILE.to_owned(),
            })
        }
    }
}
