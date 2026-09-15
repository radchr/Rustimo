# Rustimo — reactive Rust notebook library

Rustimo registers typed Rust functions as cells. Function names are definitions;
shared-reference parameter names are dependencies. A validated DAG executes cells
in dependency order. UI signals update their dependent cells in the same running
process without invoking the compiler.

Run the editable browser notebook from this directory:

```sh
cargo run -p rustimo --bin rustimo -- edit crates/rustimo/examples/basic.rs
```

Open `http://127.0.0.1:3000`. Expand **Редагувати код ноутбука (.rs)**,
change a cell, and click **Зберегти й зібрати**. Cargo diagnostics appear beside
the source. A successful build starts a new notebook worker; a failed build
keeps the last working notebook available and marks its outputs stale. Slider
values are replayed into a successful new worker when the signal still exists.
If the initial source does not compile, the editor still opens so you can fix it.

Moving the slider reruns `filtered` and `count` without a Cargo build;
`data`, `limit`, and `independent` keep their previous run counts. The notebook
source is a normal Rust file at `crates/rustimo/examples/basic.rs` and stays
available after restarting the process.

To run the app without the editor, use `cargo run -p rustimo --example basic`
and open `http://127.0.0.1:3001`.

The public API lives in `crates/rustimo`, the compile-time cell generation in
`crates/rustimo_macros`. `#[cell]` currently accepts synchronous functions with
an explicit return type and immutable references as cross-cell inputs.
`notebook!(...)` explicitly registers cells. A cell can call `display(View::text(...))`
for its browser output. A cell returning `Ui<T>` becomes a reactive slider signal.
The architectural decisions and next acceptance test are in `DESIGN.md`.

Current scope: local single-user editor for `.rs` files in
`crates/rustimo/examples`, slider, text output, and Cargo diagnostics. Each
source rebuild currently runs all cells in the new worker; signals rerun only
descendants. Markdown cells, other widget types, DataFrame pagination, and
portable standalone `.rs` notebooks remain to be implemented.

Run verification:

```sh
cargo test --offline
cargo clippy --offline --all-targets -- -D warnings
```
