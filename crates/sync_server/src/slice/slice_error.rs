use fundamental_core::AnyError;
use fundamental_core::error::PayloadRejection;
use fundamental_macros::Error;

/// Errors resolving a client's slice subscription into a scoped query.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SliceError {
    /// A wrapped error that does not map onto a more specific variant.
    Other(AnyError),

    /// The client named a slice that is not registered.
    ///
    /// Names nothing: the name is the client's string.
    #[error(code = "SLICE_001", message = "Unknown slice", status = 404)]
    Unknown,

    /// The slice's parameters were not well-formed for its declared type.
    ///
    /// Carries the class and position only: serde's message quotes the parameters.
    #[error(
        code = "SLICE_002",
        message = "Invalid slice parameters: {0}",
        status = 400
    )]
    Params(PayloadRejection),

    /// The slice's parameters were well-formed but failed validation.
    ///
    /// Carries the validation codes, never a message, which may quote the value.
    #[error(
        code = "SLICE_003",
        message = "Slice parameters failed validation: {0}",
        status = 422
    )]
    Invalid(String),

    /// The authenticated principal is not authorized for this slice or these parameters.
    #[error(
        code = "SLICE_004",
        message = "Not authorized for this slice",
        status = 403
    )]
    Forbidden,
}
