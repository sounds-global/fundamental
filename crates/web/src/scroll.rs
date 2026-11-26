use web_sys::window;

/// Scrolls the window to the top-left.
pub fn scroll_to_top() {
    if let Some(window) = window() {
        window.scroll_to_with_x_and_y(0.0, 0.0);
    }
}
