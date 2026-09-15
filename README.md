# Rustimo — reactive Rust notebook library

Rustimo registers typed Rust functions as cells. Function names are definitions;
shared-reference parameter names are dependencies. A validated DAG executes cells
in dependency order. UI signals update their dependent cells in the same running
process without invoking the compiler.

Run the editable browser notebook from this directory:

```sh
cargo run -p rustimo --bin rustimo -- edit crates/rustimo/examples/basic.rs
```

Open `http://127.0.0.1:3000`. Each `#[cell]` function appears as a notebook cell
with its Rust code, output, dependencies, and a **Зберегти / виконати** action.
Click **Запустити** to run a cell and its descendants, or **Запустити все** for
the whole notebook. **Режим застосунку** hides the code and development panels.
The dependency graph links back to cells. Expand the full-file editor at the
bottom to change imports, types, or registration outside cells.

Saving changed code starts a background Cargo build and immediately marks the
previous worker's outputs stale. Widgets keep responding while the build runs;
the editor polls for completion. Diagnostics appear above the cells and beside
the full source. A successful build starts a new notebook worker; a failed build
keeps the last working notebook available and marks its outputs stale. Compatible
slider, text, and checkbox values are replayed into a successful new worker.
Source revisions and worker generations prevent an older build from replacing
a newer saved revision.
If the initial source does not compile, the editor still opens so you can fix it.

Moving the slider reruns `filtered`, `count`, and `report` without a Cargo build;
`data`, `limit`, and `independent` keep their previous run counts. Editing the
text or checkbox widget reruns `report`. The notebook source is a normal Rust
file at `crates/rustimo/examples/basic.rs` and persists across restarts.

To run the app without the editor, use `cargo run -p rustimo --example basic`
and open `http://127.0.0.1:3001`.

The public API lives in `crates/rustimo`, the compile-time cell generation in
`crates/rustimo_macros`. `#[cell]` currently accepts synchronous functions with
an explicit return type and immutable references as cross-cell inputs.
`notebook!(...)` explicitly registers cells. A cell can call
`display(View::text(...))` or `display(View::markdown(...))` for its browser output.
`Ui::slider`, `Ui::text`, and `Ui::checkbox` are reactive signals.
The architectural decisions and next acceptance test are in `DESIGN.md`.

Current scope: local single-user editor for `.rs` files in
`crates/rustimo/examples`, typed function cells, three widget types, text and
Markdown output, and Cargo diagnostics. Code edits rebuild the entire example
and execute all cells in the new worker. Signals and manual cell runs execute
only descendants. Arbitrary Rust snippets, multiple definitions per cell,
DataFrame pagination, and portable standalone `.rs` notebooks remain future work.

Run verification:

```sh
cargo test --offline
cargo clippy --offline --all-targets -- -D warnings
```
