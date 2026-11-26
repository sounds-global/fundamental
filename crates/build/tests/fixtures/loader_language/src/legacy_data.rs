//! [`LegacyData`] — the data the plain registration's page exports, loaded outside its view.

use fundamental::AnyError;
use fundamental::web::{HttpContext, PageData};

use crate::{AppState, Word};

/// The word the plain page shows, loaded before it renders.
#[derive(Clone)]
pub struct LegacyData(pub Word);

impl PageData<AppState> for LegacyData {
    async fn load(_ctx: &HttpContext<AppState>) -> Result<Self, AnyError> {
        Ok(Self(crate::loaded_word().await))
    }
}
