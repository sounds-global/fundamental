//! An exact amount of money and the unit it is counted in.

mod denomination;
mod division;
mod granularity;
// One file per type: the type is `Money`, so its file is `money.rs`.
#[allow(clippy::module_inception)]
#[expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]
mod money;
mod money_parse_error;
mod rounding;
mod typed_amount;

pub use denomination::Denomination;
pub use division::Division;
pub use granularity::Granularity;
pub use money::Money;
pub use money_parse_error::MoneyParseError;
pub use rounding::Rounding;
