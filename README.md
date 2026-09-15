# Rustimo — reactive Rust notebook library (first vertical slice)

Rustimo registers typed Rust functions as cells. Function names are definitions;
shared-reference parameter names are dependencies. A validated DAG executes cells
in dependency order. UI signals update their dependent cells in the same running
process without invoking the compiler.

Run the browser example from this directory:

```sh
cargo run --offline -p rustimo --example basic
```

Open `http://127.0.0.1:3001`. Moving the slider reruns `filtered` and `count`;
`data`, `limit`, and `independent` keep their previous run counts. The notebook
source is a normal Rust file at `crates/rustimo/examples/basic.rs` and stays
available after restarting the process.

The public API lives in `crates/rustimo`, the compile-time cell generation in
`crates/rustimo_macros`. `#[cell]` currently accepts synchronous functions with
an explicit return type and immutable references as cross-cell inputs.
`notebook!(...)` explicitly registers cells. A cell can call `display(View::text(...))`
for its browser output. A cell returning `Ui<T>` becomes a reactive slider signal.
The architectural decisions and next acceptance test are in `DESIGN.md`.

Current scope: local single-user app mode with slider and text output. Source
editing in the browser, Markdown cells, Cargo build supervision, other widget
types, DataFrame pagination, and portable standalone `.rs` notebooks remain to
be implemented. The older `rustimo_engine` and `rustimo_demo` directories are
separate prototypes and are not part of this workspace.

Run verification:

```sh
cargo test --offline
cargo clippy --offline --all-targets -- -D warnings
```
