//! Content-addressed static assets: a URL that names the exact bytes it serves.

mod asset_addresses;
mod asset_url;
mod asset_version;

pub use asset_addresses::AssetAddresses;
pub use asset_url::AssetUrl;
pub use asset_version::AssetVersion;
