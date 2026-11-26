use leptos::prelude::*;

use crate::TeamFrame;

pub fn layout(frame: TeamFrame, children: impl IntoView + 'static) -> impl IntoView {
    view! {
        <aside>{frame.name}</aside>
        {children}
    }
}
