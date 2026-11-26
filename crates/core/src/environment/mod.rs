//! Deployment environment detection and policy.

mod build_profile;
mod parse_error;
mod parse_posture_error;
mod posture;
mod value;

pub use build_profile::BuildProfile;
pub use parse_error::ParseEnvironmentError;
pub use parse_posture_error::ParsePostureError;
pub use posture::EnvironmentPosture;
pub use value::Environment;
