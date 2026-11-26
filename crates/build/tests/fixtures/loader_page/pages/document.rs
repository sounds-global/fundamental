use fundamental::web::PageContext;

pub fn document(_ctx: &PageContext, body: &str) -> String {
    format!("<!DOCTYPE html><html><body>{body}</body></html>")
}
