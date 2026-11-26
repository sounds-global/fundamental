use leptos::prelude::*;

use crate::TeamFrame;

pub fn layout(frame: TeamFrame, children: impl IntoView + 'static) -> impl IntoView {
    view! {
        <section>
            <h2>{frame.name}</h2>
            {children}
        </section>
    }
}
