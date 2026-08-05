# Rewriting the UI in Rust

Assessed 2026-08-05.

## Correct the premise first

**There is no Electron here.** `ui/package.json` depends on `react`,
`react-dom`, Vite, TypeScript and Vitest — nothing else. No Electron, no Tauri,
no bundled Chromium. The shipped artifact is already one Rust binary:
`cortex-server` bakes `ui/dist` in through `include_dir` and serves it to
whatever browser the operator already has.

So the goals usually meant by "get rid of Electron" are already met:

| goal | status today |
| --- | --- |
| single self-contained binary | met — `cargo build --release` after the UI build |
| no bundled browser runtime | met — the system browser renders it |
| no runtime Node.js | met — Node is a *build-time* tool only |
| small footprint | met — no ~150 MB Chromium payload |

What is **not** met is "no JavaScript anywhere". That is the real question, and
it is a much smaller prize than removing Electron would have been.

## What a rewrite would actually cost

The front end is roughly **3 000 lines** of TypeScript, TSX and CSS
(`ui/src`, excluding tests): an SVG graph canvas with drag and hit testing, a
node/edge inspector with typed forms, a run workbench, import/export dialogs, a
telemetry panel, and now Help and Docs panels.

A Rust replacement has to reproduce all of it, plus the parts the browser
currently provides for free: text input with IME, selection and clipboard,
focus management, scrolling, accessibility, theming, and font shaping.

Realistic estimate: **4 000–6 000 lines of Rust and three to six focused
weeks**, against a UI that works today.

## The options

### 1. Keep React, served by Rust — status quo

Already a single binary. Node is a build dependency, which does mean the four
release gates include `npm.cmd --prefix ui run build`, and it means an npm
supply chain in the build (currently 6 direct dependencies, react and
react-dom at runtime).

### 2. Leptos or Yew compiled to WebAssembly

Rust source, no npm, still served by axum, still rendered by the browser.

- **Keeps**: CSS theming, DOM accessibility, browser text input, remote access
  through a loopback URL, devtools, the existing HTTP API unchanged.
- **Removes**: npm and the JavaScript toolchain, replaced by `trunk` /
  `wasm-bindgen` and the `wasm32-unknown-unknown` target.
- **Costs**: a WASM payload larger than the current JS bundle for a UI this
  size; slower first paint; SVG manipulation through `web-sys` is more verbose
  than JSX.
- This is the **lowest-risk path to "100 % Rust"**, because nothing about the
  deployment model changes.

### 3. egui / eframe — native window, no web technology at all

The only option that is genuinely browser-free. `egui_snarl` and `egui_graphs`
exist specifically for node editors, so the canvas is not from scratch, and
`egui_commonmark` covers the Docs panel.

- **Gains**: no browser, no npm, no HTTP round trip for local state, low memory,
  a real desktop window.
- **Loses**:
  - Accessibility drops to AccessKit's coverage, well behind the DOM.
  - Text-heavy forms — the node inspector is mostly forms — are egui's weakest
    area.
  - Loopback access from another machine disappears; the UI becomes local-only.
  - **Cross-compilation regresses.** The roadmap records that the pure-Rust
    crates cross-check cleanly for `aarch64-unknown-linux-gnu` and that a
    Raspberry Pi build needs only a C cross-compiler for SQLite. An egui front
    end adds windowing and GPU stacks to that list. Today the UI is static
    files that need no target support at all.
  - The HTTP API stops being the only client contract, so it can quietly rot.

### 4. Dioxus desktop

Rust source with an RSX syntax close to JSX, rendering through the platform
WebView (`wry`/`tao`) rather than a bundled Chromium. Lighter than Electron,
but it is still a web renderer, so "no web technology" is not achieved — while
most of egui's costs (accessibility, platform-specific WebView differences) are
partly inherited anyway.

### 5. Slint

Good native rendering and tooling. **Check licensing before any prototype**:
the terms are royalty-free-with-conditions, GPL, or paid depending on use, and
this repository is private with four crates prepared for MIT OR Apache-2.0
release. That is a decision to make before writing code, not after.

## Recommendation

**Do not rewrite now.** Ranked:

1. **Keep the status quo.** The stated objective — one binary, no bundled
   browser — is already achieved. A rewrite spends three to six weeks to change
   the implementation language of a working component while the actual open
   items are the ones in [benchmark.md](benchmark.md): symbol evidence fails
   closed at the recommended budget, and Weavatrix evidence carries 10 of 24
   required facts. Neither improves by changing UI framework.
2. **If "no JavaScript" becomes a hard requirement** — a licence audit, a
   supply-chain policy, a customer constraint — port to **Leptos**. It is the
   only option that removes npm without giving up accessibility, remote access,
   or the aarch64 story.
3. **If a native desktop window becomes a hard requirement**, add an optional
   `cortex-desktop` egui crate **alongside** the HTTP API rather than replacing
   it. The API is already the client contract; a second client proves that and
   costs nothing that exists today.

## If the rewrite happens anyway

The order that minimises risk:

1. Freeze the HTTP API and add contract tests, so any client can be checked
   against it independently of which one ships.
2. Port the read-only surfaces first — telemetry, docs, help, the run event
   list. They are pure rendering and prove the stack without risking data.
3. Port the graph canvas next; it is the piece with the most novel interaction
   and the least code reuse.
4. Port the inspector forms last. They are where a Rust GUI will feel worst,
   and where the browser is hardest to beat.
5. Keep the React client building until the Rust client passes the same
   contract tests. Delete one only when the other is proven, never before.
