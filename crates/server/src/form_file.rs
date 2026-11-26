/// A file of a [`StreamedForm`](crate::StreamedForm) read by [`files`](crate::StreamedForm::files).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormFile {
    /// The form field's name.
    pub field_name: Option<String>,
    /// The file name the browser declared: a base name, never a path a caller may trust.
    pub file_name: Option<String>,
    /// The file's bytes, or `None` for one over its limit.
    pub bytes: Option<Vec<u8>>,
}
