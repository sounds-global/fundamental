//! [`ClaimPins`] — the registered claims a verifier holds every token to.

use jsonwebtoken::Validation;

use fundamental_config::settings::AuthSettings;

/// The `iss` and `aud` every verifier holds a token to, read once from [`AuthSettings`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ClaimPins {
    pub(crate) issuer: Option<String>,
    pub(crate) audience: Option<String>,
}

impl ClaimPins {
    pub(crate) fn from_settings(settings: &AuthSettings) -> Self {
        Self {
            issuer: settings.issuer.clone(),
            audience: settings.audience.clone(),
        }
    }

    /// Pins `validation` to the configured issuer and audience, making `aud` required when set.
    pub(crate) fn apply(&self, validation: &mut Validation) {
        if let Some(issuer) = &self.issuer {
            validation.set_issuer(&[issuer]);
        }
        if let Some(audience) = &self.audience {
            validation.set_audience(&[audience]);
            let mut required = validation.required_spec_claims.clone();
            required.insert("aud".to_owned());
            validation.required_spec_claims = required;
        }
    }
}
