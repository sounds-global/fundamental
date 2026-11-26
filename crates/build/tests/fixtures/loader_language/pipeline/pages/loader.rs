use fundamental::web::{LoadCx, PageRefusal};

use crate::{PageState, Word};

pub async fn load(_cx: LoadCx<PageState>) -> Result<Word, PageRefusal> {
    Ok(crate::loaded_word().await)
}
