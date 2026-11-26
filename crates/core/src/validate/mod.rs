//! Validation system for Fundamental.
//!
//! # Example
//!
//! ```rust
//! use fundamental_core::validate::{Validate, ValidationContext, ValidationError, ValidationErrors};
//!
//! struct CreateUserInput {
//!     email: String,
//!     name: String,
//!     age: u8,
//! }
//!
//! impl Validate for CreateUserInput {
//!     fn validate(&self) -> Result<(), ValidationErrors> {
//!         let mut ctx = ValidationContext::new();
//!
//!         ctx.field("email")
//!             .required(&self.email)
//!             .email(&self.email);
//!
//!         ctx.field("name")
//!             .required(&self.name)
//!             .length(&self.name, 1, 100);
//!
//!         ctx.field("age")
//!             .range(self.age, 13, 120);
//!
//!         ctx.into_result()
//!     }
//! }
//! ```

mod context;
mod error;
mod errors;
mod field;
mod result;
mod validators;

pub use context::ValidationContext;
pub use error::{ParamValue, ValidationError};
pub use errors::ValidationErrors;
pub use field::FieldValidator;
pub use result::ValidationResult;

#[cfg(not(feature = "std"))]
use alloc::{format, string::String, vec::Vec};

/// Trait for types that can be validated.
pub trait Validate {
    /// Validates this instance, collecting all errors.
    #[expect(clippy::missing_errors_doc, reason = "readability baseline")]
    fn validate(&self) -> ValidationResult<()>;

    /// Returns whether this instance is valid.
    fn is_valid(&self) -> bool {
        self.validate().is_ok()
    }
}

impl<T: Validate> Validate for Option<T> {
    fn validate(&self) -> Result<(), ValidationErrors> {
        match self {
            Some(v) => v.validate(),
            None => Ok(()),
        }
    }
}

impl<T: Validate> Validate for Vec<T> {
    fn validate(&self) -> Result<(), ValidationErrors> {
        let mut errors = ValidationErrors::new();
        for (i, item) in self.iter().enumerate() {
            if let Err(item_errors) = item.validate() {
                errors.merge_with_prefix(&format!("[{}]", i), item_errors);
            }
        }
        errors.into_result()
    }
}

impl Validate for String {
    fn validate(&self) -> Result<(), ValidationErrors> {
        Ok(())
    }
}

impl Validate for &str {
    fn validate(&self) -> Result<(), ValidationErrors> {
        Ok(())
    }
}

macro_rules! impl_validate_for_primitives {
    ($($t:ty),*) => {
        $(
            impl Validate for $t {
                fn validate(&self) -> Result<(), ValidationErrors> {
                    Ok(())
                }
            }
        )*
    };
}

impl_validate_for_primitives!(
    bool, i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize, f32, f64
);
