use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use futures_core::Stream;
use multer::{Constraints, Multipart, SizeLimit};

use crate::{FilePart, FormFile, RequestBodyStream, ServerError};

/// The most text fields a form may carry; each is held whole, so it is bounded.
pub(crate) const MAX_TEXT_FIELDS: usize = 32;
const MAX_TEXT_FIELD_BYTES: u64 = 8 * 1024;

/// A `multipart/form-data` body read as it arrives, so a form can stream one file to a [streamed route](crate::Route::streamed).
pub struct StreamedForm {
    multipart: Multipart<'static>,
}

impl StreamedForm {
    /// The form in `body`, whose request declared `content_type`.
    ///
    /// # Errors
    /// [`ServerError::MalformedForm`] if `content_type` is not `multipart/form-data` with a
    /// boundary.
    pub fn new(content_type: Option<&str>, body: RequestBodyStream) -> Result<Self, ServerError> {
        let boundary = content_type
            .and_then(|value| multer::parse_boundary(value).ok())
            .ok_or(ServerError::MalformedForm {
                reason: "not multipart/form-data with a boundary",
            })?;
        let constraints =
            Constraints::new().size_limit(SizeLimit::new().for_field("", MAX_TEXT_FIELD_BYTES));
        Ok(Self {
            multipart: Multipart::with_constraints(
                OneChunkPerPoll {
                    body,
                    yielded: false,
                },
                boundary,
                constraints,
            ),
        })
    }

    /// Reads the text fields up to the form's file, and answers them with the file part.
    ///
    /// # Errors
    /// [`ServerError::MalformedForm`] if the form carries no file, more than 32 text fields, a
    /// field over 8 KiB, or is not well formed; or the body's own error, such as
    /// [`ServerError::PayloadTooLarge`].
    pub async fn file(mut self) -> Result<(Vec<(String, String)>, FilePart), ServerError> {
        let mut fields = Vec::new();
        loop {
            let Some(mut field) = self.multipart.next_field().await.map_err(refused)? else {
                return Err(ServerError::MalformedForm {
                    reason: "the form carries no file",
                });
            };
            if field.file_name().is_some() {
                return Ok((fields, FilePart::new(field, self.multipart)));
            }
            if fields.len() == MAX_TEXT_FIELDS {
                return Err(ServerError::MalformedForm {
                    reason: "too many fields",
                });
            }
            let name = field.name().unwrap_or_default().to_owned();
            let value = read_text(&mut field).await?;
            fields.push((name, value));
        }
    }

    /// Reads every part of the form: its text fields, and each file whole while within `limit_each` bytes.
    ///
    /// # Errors
    /// [`ServerError::MalformedForm`] for more than `max_files` files, more than 32 text fields,
    /// a field over 8 KiB, or a body that is not well formed; or the body's own error, such as
    /// [`ServerError::PayloadTooLarge`] once the route's bound is reached.
    pub async fn files(
        mut self,
        max_files: usize,
        limit_each: usize,
    ) -> Result<(Vec<(String, String)>, Vec<FormFile>), ServerError> {
        let mut fields = Vec::new();
        let mut files = Vec::new();
        while let Some(mut field) = self.multipart.next_field().await.map_err(refused)? {
            if field.file_name().is_none() {
                push_text(&mut fields, &mut field).await?;
                continue;
            }
            let field_name = field.name().map(str::to_owned);
            let file_name = field.file_name().map(str::to_owned);
            let bytes = read_within(&mut field, limit_each).await?;
            if file_name.as_deref() == Some("") && bytes.as_deref().is_some_and(<[u8]>::is_empty) {
                continue;
            }
            if files.len() == max_files {
                return Err(ServerError::MalformedForm {
                    reason: "too many files",
                });
            }
            files.push(FormFile {
                field_name,
                file_name,
                bytes,
            });
        }
        Ok((fields, files))
    }
}

/// Adds `field` to `fields`, a form's text fields, at most 32 of them.
async fn push_text(
    fields: &mut Vec<(String, String)>,
    field: &mut multer::Field<'static>,
) -> Result<(), ServerError> {
    if fields.len() == MAX_TEXT_FIELDS {
        return Err(ServerError::MalformedForm {
            reason: "too many fields",
        });
    }
    let name = field.name().unwrap_or_default().to_owned();
    let value = read_text(field).await?;
    fields.push((name, value));
    Ok(())
}

/// The bytes of the file `field` while within `limit`; past it the file is read to its end and dropped.
async fn read_within(
    field: &mut multer::Field<'static>,
    limit: usize,
) -> Result<Option<Vec<u8>>, ServerError> {
    let mut held = Some(Vec::new());
    while let Some(chunk) = field.chunk().await.map_err(refused)? {
        held = held
            .filter(|bytes| bytes.len() + chunk.len() <= limit)
            .map(|mut bytes| {
                bytes.extend_from_slice(&chunk);
                bytes
            });
    }
    Ok(held)
}

pub(crate) async fn read_text(field: &mut multer::Field<'static>) -> Result<String, ServerError> {
    let mut value = Vec::new();
    while let Some(chunk) = field.chunk().await.map_err(refused)? {
        if value.len() as u64 + chunk.len() as u64 > MAX_TEXT_FIELD_BYTES {
            return Err(ServerError::MalformedForm {
                reason: "a field is too long",
            });
        }
        value.extend_from_slice(&chunk);
    }
    String::from_utf8(value).map_err(|_| ServerError::MalformedForm {
        reason: "a field is not UTF-8",
    })
}

/// A body error keeps its own meaning through the parser; every other failure is a malformed form.
pub(crate) fn refused(error: multer::Error) -> ServerError {
    match error {
        multer::Error::StreamReadFailed(source) => match source.downcast::<ServerError>() {
            Ok(server) => *server,
            Err(_) => ServerError::BodyInterrupted,
        },
        multer::Error::FieldSizeExceeded { .. } | multer::Error::StreamSizeExceeded { .. } => {
            ServerError::MalformedForm {
                reason: "a field is too long",
            }
        }
        _ => ServerError::MalformedForm {
            reason: "not a well-formed multipart body",
        },
    }
}

/// The body handed to the parser one chunk per poll, so its buffer holds at most one.
struct OneChunkPerPoll {
    body: RequestBodyStream,
    yielded: bool,
}

impl Stream for OneChunkPerPoll {
    type Item = Result<Bytes, ServerError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.yielded {
            self.yielded = false;
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        let next = Pin::new(&mut self.body).poll_next(cx);
        if matches!(next, Poll::Ready(Some(Ok(_)))) {
            self.yielded = true;
        }
        next
    }
}

#[cfg(test)]
mod tests {
    use super::StreamedForm;
    use crate::{RequestBodyStream, ServerError};
    use bytes::Bytes;

    const TYPE: &str = "multipart/form-data; boundary=XyZ";

    fn body(parts: &[&str]) -> RequestBodyStream {
        let joined: String = parts.concat();
        let chunks: Vec<Result<Bytes, ServerError>> = joined
            .into_bytes()
            .chunks(7)
            .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
            .collect();
        RequestBodyStream::new(futures_util_stream(chunks))
    }

    fn futures_util_stream(
        items: Vec<Result<Bytes, ServerError>>,
    ) -> impl futures_core::Stream<Item = Result<Bytes, ServerError>> + Send + 'static {
        struct Iter(std::vec::IntoIter<Result<Bytes, ServerError>>);
        impl futures_core::Stream for Iter {
            type Item = Result<Bytes, ServerError>;
            fn poll_next(
                mut self: std::pin::Pin<&mut Self>,
                _: &mut std::task::Context<'_>,
            ) -> std::task::Poll<Option<Self::Item>> {
                std::task::Poll::Ready(self.0.next())
            }
        }
        Iter(items.into_iter())
    }

    const FIELD: &str =
        "--XyZ\r\nContent-Disposition: form-data; name=\"upload_key\"\r\n\r\nk-1\r\n";
    const FILE: &str = "--XyZ\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.txt\"\r\nContent-Type: text/plain\r\n\r\nhello file\r\n";
    const END: &str = "--XyZ--\r\n";

    #[tokio::test]
    async fn a_form_file_part_streams_in_bounded_memory() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU64, Ordering};

        const CHUNK: usize = 64 * 1024;
        const CHUNKS: usize = 1024;
        let pulled = Arc::new(AtomicU64::new(0));
        let head = Bytes::from(format!(
            "{FIELD}--XyZ\r\nContent-Disposition: form-data; name=\"file\"; filename=\"big.bin\"\r\n\r\n"
        ));
        let tail = Bytes::from(format!("\r\n{END}"));
        let parts = std::iter::once(head)
            .chain((0..CHUNKS).map(|_| Bytes::from(vec![b'x'; CHUNK])))
            .chain(std::iter::once(tail));
        struct Counted(Box<dyn Iterator<Item = Bytes> + Send>, Arc<AtomicU64>);
        impl futures_core::Stream for Counted {
            type Item = Result<Bytes, ServerError>;
            #[expect(clippy::excessive_nesting, reason = "readability baseline")]
            fn poll_next(
                mut self: std::pin::Pin<&mut Self>,
                _: &mut std::task::Context<'_>,
            ) -> std::task::Poll<Option<Self::Item>> {
                let next = self.0.next();
                if let Some(part) = &next {
                    self.1.fetch_add(part.len() as u64, Ordering::AcqRel);
                }
                std::task::Poll::Ready(next.map(Ok))
            }
        }
        let body = RequestBodyStream::new(Counted(Box::new(parts), Arc::clone(&pulled)));
        let form = StreamedForm::new(Some(TYPE), body).expect("a form");
        let (fields, mut file) = form.file().await.expect("a file");
        assert_eq!(fields, vec![("upload_key".to_owned(), "k-1".to_owned())]);
        let first = file.next_chunk().await.expect("a chunk").expect("bytes");
        let at_first = pulled.load(Ordering::Acquire);
        assert!(
            at_first <= 4 * CHUNK as u64,
            "{at_first} bytes were read before the file's first chunk was handed on"
        );
        let mut total = first.len();
        let mut largest = first.len();
        while let Some(chunk) = file.next_chunk().await {
            let chunk = chunk.expect("a chunk");
            largest = largest.max(chunk.len());
            total += chunk.len();
        }
        assert_eq!(total, CHUNK * CHUNKS, "the whole 64 MiB arrived");
        assert!(
            largest <= 4 * CHUNK,
            "a chunk held more than the parser's lookahead: {largest}"
        );
        assert!(file.finish().await.expect("nothing after").is_empty());
    }

    #[tokio::test]
    async fn more_than_32_fields_after_the_file_are_refused() {
        let field = |index: usize| {
            format!("--XyZ\r\nContent-Disposition: form-data; name=\"f{index}\"\r\n\r\nv\r\n")
        };
        let mut parts = vec![FILE.to_owned()];
        parts.extend((0..33).map(field));
        parts.push(END.to_owned());
        let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
        let form = StreamedForm::new(Some(TYPE), body(&parts)).expect("a form");
        let (_, mut file) = form.file().await.expect("a file");
        while file.next_chunk().await.is_some() {}
        assert!(matches!(
            file.finish().await,
            Err(ServerError::MalformedForm {
                reason: "a form carries at most 32 text fields after its file"
            })
        ));
    }

    #[tokio::test]
    async fn a_second_file_part_is_refused() {
        let form = StreamedForm::new(Some(TYPE), body(&[FILE, FILE, END])).expect("a form");
        let (_, mut file) = form.file().await.expect("a file");
        while file.next_chunk().await.is_some() {}
        assert!(matches!(
            file.finish().await,
            Err(ServerError::MalformedForm {
                reason: "a form carries one file"
            })
        ));
    }

    #[tokio::test]
    async fn every_file_of_a_form_is_read_and_one_over_its_limit_is_named_without_bytes() {
        const BIG: &str = "--XyZ\r\nContent-Disposition: form-data; name=\"file\"; filename=\"b.txt\"\r\n\r\nthis file is too long\r\n";
        let form =
            StreamedForm::new(Some(TYPE), body(&[FIELD, FILE, BIG, FILE, END])).expect("a form");
        let (fields, files) = form.files(3, 12).await.expect("the files");
        assert_eq!(fields, vec![("upload_key".to_owned(), "k-1".to_owned())]);
        let read: Vec<_> = files
            .iter()
            .map(|file| (file.file_name.as_deref(), file.bytes.as_deref()))
            .collect();
        assert_eq!(
            read,
            vec![
                (Some("a.txt"), Some(&b"hello file"[..])),
                (Some("b.txt"), None),
                (Some("a.txt"), Some(&b"hello file"[..])),
            ]
        );
    }

    #[tokio::test]
    async fn a_file_input_left_empty_is_no_file() {
        const EMPTY: &str = "--XyZ\r\nContent-Disposition: form-data; name=\"more\"; filename=\"\"\r\nContent-Type: application/octet-stream\r\n\r\n\r\n";
        let form = StreamedForm::new(Some(TYPE), body(&[FILE, EMPTY, END])).expect("a form");
        let (_, files) = form.files(1, 1024).await.expect("the files");
        let read: Vec<_> = files
            .iter()
            .map(|file| (file.file_name.as_deref(), file.bytes.as_deref()))
            .collect();
        assert_eq!(read, vec![(Some("a.txt"), Some(&b"hello file"[..]))]);
    }

    #[tokio::test]
    async fn a_form_with_more_files_than_allowed_is_refused() {
        let form = StreamedForm::new(Some(TYPE), body(&[FILE, FILE, FILE, END])).expect("a form");
        assert!(matches!(
            form.files(2, 1024).await,
            Err(ServerError::MalformedForm {
                reason: "too many files"
            })
        ));
    }

    #[tokio::test]
    async fn a_form_without_a_file_or_a_boundary_is_refused() {
        assert!(matches!(
            StreamedForm::new(Some("text/plain"), body(&[END])),
            Err(ServerError::MalformedForm { .. })
        ));
        let form = StreamedForm::new(Some(TYPE), body(&[FIELD, END])).expect("a form");
        assert!(matches!(
            form.file().await,
            Err(ServerError::MalformedForm { .. })
        ));
    }

    #[tokio::test]
    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    async fn a_body_over_its_bound_stays_a_413_through_the_parser() {
        let over: RequestBodyStream = RequestBodyStream::new(futures_util_stream(vec![
            Ok(Bytes::from_static(b"--XyZ\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a\"\r\n\r\nabc")),
            Err(ServerError::PayloadTooLarge { max: 3 }),
        ]));
        let form = StreamedForm::new(Some(TYPE), over).expect("a form");
        let outcome = match form.file().await {
            Ok((_, mut file)) => {
                let mut last = None;
                while let Some(chunk) = file.next_chunk().await {
                    if let Err(error) = chunk {
                        last = Some(error);
                    }
                }
                last
            }
            Err(error) => Some(error),
        };
        assert!(
            matches!(outcome, Some(ServerError::PayloadTooLarge { max: 3 })),
            "{outcome:?}"
        );
    }
}
