use fundamental::authz::middleware::Authorized;
use fundamental::web::{LoadCx, PageRefusal};

use crate::team::Team;
use crate::team::team_relation::Member;
use crate::{Overview, PageState, TeamFrame};

pub async fn load(
    _cx: LoadCx<PageState>,
    team: Authorized<Team, Member>,
) -> Result<Overview, PageRefusal> {
    Ok(Overview {
        name: team.id().to_owned(),
    })
}

pub async fn frame(
    _cx: LoadCx<PageState>,
    team: Authorized<Team, Member>,
) -> Result<TeamFrame, PageRefusal> {
    Ok(TeamFrame {
        name: team.id().to_owned(),
    })
}
