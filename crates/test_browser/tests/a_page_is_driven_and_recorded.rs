//! A page is loaded, read, clicked and typed into against the real browser.

use std::time::Duration;

use fundamental_test_browser::Browser;

const PAGE: &str = "data:text/html,<!doctype html><html lang=en><title>t</title>\
<main><h1>Hi</h1><label for=n>Name</label><input id=n>\
<button id=b onclick=\"console.log('pressed '+document.getElementById('n').value)\">Go</button></main>";

#[tokio::test(flavor = "multi_thread")]
async fn a_page_is_driven_and_recorded() {
    let browser = Browser::connect().await.expect("the test browser");
    let page = browser.page().await.expect("a page");
    page.goto(PAGE).await.expect("loads");
    page.type_into("#n", "Ada").await.expect("types");
    page.click("#b").await.expect("clicks");
    page.wait_for("document.activeElement.id === 'b'", Duration::from_secs(5))
        .await
        .expect("the press focused the button");
    let console = page.console();
    assert!(
        console.iter().any(|line| line.text == "pressed Ada"),
        "{console:?}"
    );
    let tree = page.accessibility_tree().await.expect("a tree");
    assert!(
        tree.iter()
            .any(|node| node.role == "textbox" && node.name == "Name")
    );
    assert!(
        tree.iter()
            .any(|node| node.role == "button" && node.name == "Go")
    );
    page.close().await.expect("closes");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_page_with_scripting_off_runs_none() {
    let browser = Browser::connect().await.expect("the test browser");
    let page = browser.page().await.expect("a page");
    page.scripting(false).await.expect("switches scripting off");
    page.goto("data:text/html,<p id=p>static</p><script>document.getElementById('p').textContent='ran'</script>")
        .await
        .expect("loads");
    let text: String = page
        .eval("document.getElementById('p').textContent")
        .await
        .expect("reads");
    assert_eq!(text, "static");
    page.close().await.expect("closes");
}
