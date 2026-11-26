// The island bootstrap: load the wasm bundle, hydrate every island the server rendered,
// then **say so**.
//
// Mirrors `leptos-0.8.20/src/hydration/island_script.js` — the traversal below is that
// script's contract with the bundle (`mod[id](el)` per `<leptos-island>`, `$$on_hydrate`
// per `<leptos-children>`), not an invention of this crate's. Read the two files side by
// side whenever the Leptos version moves.
//
// Two deliberate differences, and both are the point. The traversal promise is **awaited**
// and published, where Leptos's line 63 discards it; and the `requestIdleCallback` gate
// Leptos wraps the whole bootstrap in is **gone**, because a request for spare time has no
// bound on a machine that has none. `hydration_script`'s rustdoc carries the reasoning.
(() => {
  const root = document.documentElement;

  /// Publish what state hydration is in, on the element and as an event.
  ///
  /// The attribute is the race-free half: a reader that arrives late still sees it. The
  /// event is for code that was already listening, and carries the same word.
  const declare = (state) => {
    root.setAttribute("data-hydration", state);
    document.dispatchEvent(
      new CustomEvent("fundamental:hydration", { detail: { state }, bubbles: true }),
    );
  };

  declare("pending");

  // Exactly one, and only on this origin. The page's Content Security Policy admits this
  // script by the hash of its text, so the text must not be steerable: an element injected
  // into the page with the same attribute, ahead of the real one, would otherwise hand it
  // an attacker's bundle URL to import. Two elements, or a bundle on another origin, is
  // refused rather than guessed between.
  const scripts = document.querySelectorAll("script[data-fundamental-hydration]");
  const script = scripts.length === 1 ? scripts[0] : null;
  const bundleUrl = script && script.getAttribute("data-bundle");
  const wasmUrl = script && script.getAttribute("data-wasm");
  const onThisOrigin = (url) => {
    try {
      return new URL(url, document.baseURI).origin === window.location.origin;
    } catch {
      return false;
    }
  };
  if (scripts.length > 1 || (bundleUrl && wasmUrl && !(onThisOrigin(bundleUrl) && onThisOrigin(wasmUrl)))) {
    declare("failed");
    console.error(
      "fundamental: the hydration bootstrap refused to load a bundle — the page carries more " +
        "than one hydration element, or names a bundle on another origin.",
    );
    return;
  }
  if (!bundleUrl || !wasmUrl) {
    declare("failed");
    console.error(
      "fundamental: the hydration bootstrap could not find the bundle URLs it was emitted with, " +
        "so no island on this page will come to life.",
    );
    return;
  }

  // The `<leptos-children>` callback for the island currently being walked into, which is
  // how Leptos orders a parent island's children against the parent itself.
  const childrenCallbacks = [];

  async function hydrateIsland(el, id, mod) {
    const islandFn = mod[id];
    if (!islandFn) {
      console.warn(`fundamental: the bundle exports no hydrate function for the island \`${id}\`.`);
      return;
    }
    const childrenCallback = childrenCallbacks[childrenCallbacks.length - 1];
    if (childrenCallback) childrenCallback();
    const hydrating = islandFn(el);
    // An island whose hydrate returns a promise — a split bundle's lazily-loaded one — is
    // awaited here, which is what makes the walk's own promise mean *every* island is live.
    if (hydrating && hydrating.then) await hydrating;
  }

  async function traverse(node, mod) {
    if (node.nodeType !== Node.ELEMENT_NODE) return;
    const tag = node.tagName.toLowerCase();
    if (tag === "leptos-island") {
      await hydrateIsland(node, node.getAttribute("data-component"), mod);
      for (const child of node.children) await traverse(child, mod);
      return;
    }
    if (tag === "leptos-children") {
      childrenCallbacks.push(node.$$on_hydrate);
      for (const child of node.children) await traverse(child, mod);
      childrenCallbacks.pop();
      return;
    }
    for (const child of node.children) await traverse(child, mod);
  }

  import(bundleUrl)
    .then(async (mod) => {
      // Leptos's published escape hatch for an island that arrives after the walk — a
      // fragment inserted by the islands router, or by an island of the page's own. Kept
      // because removing it would take a documented capability away in silence.
      window.__hydrateIsland = (el, id) => hydrateIsland(el, id, mod);
      await mod.default({ module_or_path: wasmUrl });
      // The bundle's `hydrate` export: `fundamental_web::hydrate`, which establishes the
      // root reactive owner every island hydrates beneath. It must run before the walk.
      mod.hydrate();
      // A part of the page swapped in place by script brings islands this walk never saw: it is
      // walked as the body is, parents before their children. Published once the root owner is.
      window.__hydrateWithin = (root) => traverse(root, mod);
      await traverse(document.body, mod);
      declare("complete");
    })
    .catch((error) => {
      // A bundle that never loads left no trace at all before this: the page stayed inert,
      // every island stayed dead, and nothing said why. A swallowed error is a bug.
      declare("failed");
      console.error("fundamental: island hydration failed.", error);
    });
})();
