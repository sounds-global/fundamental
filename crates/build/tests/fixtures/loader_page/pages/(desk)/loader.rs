use fundamental::web::{LoadCx, PageRefusal};

use crate::{PageState, TeamFrame};

pub async fn frame(cx: LoadCx<PageState>) -> Result<TeamFrame, PageRefusal> {
    let reader = cx.signed_in()?;
    Ok(TeamFrame {
        name: reader.user_id.to_string(),
    })
}
