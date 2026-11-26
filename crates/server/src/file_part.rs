use bytes::Bytes;
use multer::{Field, Multipart};

use crate::ServerError;
use crate::streamed_form::{MAX_TEXT_FIELDS, read_text, refused};

/// The one file of a [`StreamedForm`](crate::StreamedForm), read a chunk at a time.
pub struct FilePart {
    field: Field<'static>,
    rest: Multipart<'static>,
    field_name: Option<String>,
    file_name: Option<String>,
    declared_type: Option<String>,
    failed: bool,
}

impl FilePart {
    pub(crate) fn new(field: Field<'static>, rest: Multipart<'static>) -> Self {
        Self {
            field_name: field.name().map(str::to_owned),
            file_name: field.file_name().map(str::to_owned),
            declared_type: field.content_type().map(ToString::to_string),
            field,
            rest,
            failed: false,
        }
    }

    /// The form field's name.
    #[must_use]
    pub fn field_name(&self) -> Option<&str> {
        self.field_name.as_deref()
    }

    /// The file name the browser declared.
    #[must_use]
    pub fn file_name(&self) -> Option<&str> {
        self.file_name.as_deref()
    }

    /// The media type the browser declared — a claim, never a fact.
    #[must_use]
    pub fn declared_type(&self) -> Option<&str> {
        self.declared_type.as_deref()
    }

    /// The next chunk of the file, or `None` once it has ended or failed.
    pub async fn next_chunk(&mut self) -> Option<Result<Bytes, ServerError>> {
        if self.failed {
            return None;
        }
        let next = self.field.chunk().await.map_err(refused).transpose();
        if matches!(next, Some(Err(_))) {
            self.failed = true;
        }
        next
    }

    /// Reads the text fields after the file, and refuses a second file.
    ///
    /// # Errors
    /// [`ServerError::MalformedForm`] for another file part, more than 32 fields after the
    /// file, or a malformed remainder.
    pub async fn finish(self) -> Result<Vec<(String, String)>, ServerError> {
        // The parser hands out the next field only once the previous one is gone.
        let Self {
            field, mut rest, ..
        } = self;
        drop(field);
        let mut fields = Vec::new();
        while let Some(mut field) = rest.next_field().await.map_err(refused)? {
            if field.file_name().is_some() {
                return Err(ServerError::MalformedForm {
                    reason: "a form carries one file",
                });
            }
            if fields.len() == MAX_TEXT_FIELDS {
                return Err(ServerError::MalformedForm {
                    reason: "a form carries at most 32 text fields after its file",
                });
            }
            let name = field.name().unwrap_or_default().to_owned();
            fields.push((name, read_text(&mut field).await?));
        }
        Ok(fields)
    }
}
