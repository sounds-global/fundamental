//! A page rendered to HTML, as a served page renders it.

use fundamental_test_render::render_html;
use leptos::prelude::*;

/// How many rows the page's list holds.
pub const ROWS: usize = 50;

/// A page of plain Leptos parts with [`ROWS`] rows, rendered to HTML as a served page is.
#[must_use]
pub fn page() -> String {
    render_html(page_view)
}

/// The page's view, for a server that renders it itself.
pub fn page_view() -> impl IntoView {
    view! {
            <main>
                <header>
                    <h1>"Projects"</h1>
                    <nav>
                        <a href="/">"Home"</a>
                        <a href="/projects">"Projects"</a>
                        <a href="/news">"News"</a>
                    </nav>
                </header>
                <ul>
                    {(0..ROWS).map(row).collect_view()}
                </ul>
                <form method="post" action="/api/projects">
                    <label for="name">"Name"</label>
                    <input id="name" name="name" required=true />
                    <button type="submit">"Add a project"</button>
                </form>
            </main>
    }
}

fn row(index: usize) -> impl IntoView {
    let href = format!("/projects/{index}");
    let stage = if index.is_multiple_of(3) {
        "Live"
    } else {
        "In development"
    };
    view! {
        <li class="row">
            <a href=href>{format!("Project {index}")}</a>
            <span class="badge">{stage}</span>
        </li>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_holds_every_row() {
        let html = page();
        assert_eq!(html.matches("<li").count(), ROWS);
        assert!(html.contains("Project 49"));
    }
}
