// Fundamental's development live reload: served only by a router that serves its bundle from
// disk. A stylesheet change swaps every same-origin stylesheet in place; any other change to the
// bundle reloads the page, and so does reconnecting to a server that has restarted since.
const source = new EventSource("/_fundamental/live");
let boot = null;
source.addEventListener("hello", (event) => {
  if (boot !== null && boot !== event.data) {
    location.reload();
  }
  boot = event.data;
});
source.addEventListener("css", (event) => {
  for (const link of document.querySelectorAll('link[rel="stylesheet"]')) {
    const url = new URL(link.href, location.href);
    if (url.origin === location.origin) {
      url.searchParams.set("v", event.data);
      link.href = url.href;
    }
  }
});
source.addEventListener("reload", () => location.reload());
