use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use rustimo::{Notebook, Ui, View, cell, display, notebook};

static DATA_RUNS: AtomicUsize = AtomicUsize::new(0);
static LIMIT_RUNS: AtomicUsize = AtomicUsize::new(0);
static FILTER_RUNS: AtomicUsize = AtomicUsize::new(0);
static OTHER_RUNS: AtomicUsize = AtomicUsize::new(0);
static SUMMARY_RUNS: AtomicUsize = AtomicUsize::new(0);
static TEST_LOCK: Mutex<()> = Mutex::new(());

#[cell]
fn data() -> Vec<u32> {
    DATA_RUNS.fetch_add(1, Ordering::SeqCst);
    vec![10, 20, 30]
}

#[cell]
fn limit() -> Ui<u32> {
    LIMIT_RUNS.fetch_add(1, Ordering::SeqCst);
    Ui::slider("limit", 0, 30, 15).step(5).label("Minimum")
}

#[allow(clippy::ptr_arg)]
#[cell]
fn filtered(data: &Vec<u32>, limit: &Ui<u32>) -> Vec<u32> {
    FILTER_RUNS.fetch_add(1, Ordering::SeqCst);
    let result: Vec<_> = data
        .iter()
        .copied()
        .filter(|value| *value >= limit.value())
        .collect();
    display(View::text(format!("{} values", result.len())));
    result
}

#[cell]
fn other() -> u32 {
    OTHER_RUNS.fetch_add(1, Ordering::SeqCst);
    42
}

#[allow(clippy::ptr_arg)]
#[cell]
fn wrong(data: &String) -> usize {
    data.len()
}

#[cell]
fn a(b: &u32) -> u32 {
    *b
}

#[cell]
fn b(a: &u32) -> u32 {
    *a
}

#[cell]
fn panics() -> u32 {
    panic!("deliberate panic")
}

#[cell]
fn fails_on_signal(limit: &Ui<u32>) -> u32 {
    assert!(limit.value() < 25, "signal caused a panic");
    limit.value()
}

#[cell]
fn child(fails_on_signal: &u32) -> u32 {
    *fails_on_signal + 1
}

#[cell]
fn mismatched_name() -> Ui<u32> {
    Ui::slider("other_name", 0, 30, 15)
}

#[cell]
fn title() -> Ui<String> {
    Ui::text("title", "Report").label("Title")
}

#[cell]
fn details() -> Ui<bool> {
    Ui::checkbox("details", false).label("Details")
}

#[cell]
fn summary(title: &Ui<String>, details: &Ui<bool>) -> String {
    SUMMARY_RUNS.fetch_add(1, Ordering::SeqCst);
    format!("{}: {}", title.value(), details.value())
}

#[test]
fn slider_runs_only_descendants_without_compiling_or_recreating_it() {
    let _lock = TEST_LOCK.lock().unwrap();
    DATA_RUNS.store(0, Ordering::SeqCst);
    LIMIT_RUNS.store(0, Ordering::SeqCst);
    FILTER_RUNS.store(0, Ordering::SeqCst);
    OTHER_RUNS.store(0, Ordering::SeqCst);

    let mut notebook = notebook!(data, limit, filtered, other).unwrap();
    notebook.run_all().unwrap();
    let state = notebook.snapshot();
    assert_eq!(
        state
            .cells
            .iter()
            .map(|cell| cell.name.as_str())
            .collect::<Vec<_>>(),
        vec!["data", "limit", "filtered", "other"]
    );
    assert!(
        state.topo_order.iter().position(|cell| cell == "data")
            < state.topo_order.iter().position(|cell| cell == "filtered")
    );
    assert_eq!(&*notebook.get::<Vec<u32>>("filtered").unwrap(), &[20, 30]);

    let executed = notebook.set_signal("limit", serde_json::json!(25)).unwrap();
    assert_eq!(executed, vec!["filtered"]);
    assert_eq!(&*notebook.get::<Vec<u32>>("filtered").unwrap(), &[30]);
    assert_eq!(DATA_RUNS.load(Ordering::SeqCst), 1);
    assert_eq!(LIMIT_RUNS.load(Ordering::SeqCst), 1);
    assert_eq!(FILTER_RUNS.load(Ordering::SeqCst), 2);
    assert_eq!(OTHER_RUNS.load(Ordering::SeqCst), 1);
    assert_eq!(
        notebook
            .snapshot()
            .cells
            .iter()
            .find(|cell| cell.name == "limit")
            .unwrap()
            .run_count,
        1
    );
}

#[test]
fn invalid_graphs_are_errors() {
    let duplicate = Notebook::new(vec![
        __rustimo_cell_data::descriptor(),
        __rustimo_cell_data::descriptor(),
    ]);
    assert_eq!(duplicate.err().unwrap().code, "duplicate_definition");
    assert_eq!(notebook!(a, b).err().unwrap().code, "cycle");
    assert_eq!(notebook!(data, wrong).err().unwrap().code, "type_mismatch");
}

#[test]
fn invalid_slider_value_keeps_previous_state() {
    let _lock = TEST_LOCK.lock().unwrap();
    let mut notebook = notebook!(limit).unwrap();
    notebook.run_all().unwrap();
    assert_eq!(
        notebook
            .set_signal("limit", serde_json::json!(50))
            .unwrap_err()
            .code,
        "invalid_signal"
    );
    assert_eq!(notebook.get::<Ui<u32>>("limit").unwrap().value(), 15);
}

#[test]
fn a_panic_is_reported_on_its_cell() {
    let mut notebook = notebook!(panics).unwrap();
    assert_eq!(notebook.run_all().unwrap_err().code, "panic");
    let state = notebook.snapshot();
    assert_eq!(state.cells[0].status, "error");
}

#[test]
fn an_error_invalidates_descendant_values() {
    let _lock = TEST_LOCK.lock().unwrap();
    let mut notebook = notebook!(limit, fails_on_signal, child).unwrap();
    notebook.run_all().unwrap();
    assert_eq!(notebook.get::<u32>("child").unwrap().as_ref(), &16);
    assert_eq!(
        notebook
            .set_signal("limit", serde_json::json!(25))
            .unwrap_err()
            .code,
        "panic"
    );
    let child_state = notebook
        .snapshot()
        .cells
        .into_iter()
        .find(|cell| cell.name == "child")
        .unwrap();
    assert_eq!(child_state.status, "blocked");
    assert!(notebook.get::<u32>("child").is_err());
}

#[test]
fn a_signal_must_use_its_producing_cell_name() {
    let mut notebook = notebook!(mismatched_name).unwrap();
    assert_eq!(notebook.run_all().unwrap_err().code, "signal_name_mismatch");
}

#[test]
fn text_and_checkbox_rerun_only_their_consumer() {
    let _lock = TEST_LOCK.lock().unwrap();
    SUMMARY_RUNS.store(0, Ordering::SeqCst);
    OTHER_RUNS.store(0, Ordering::SeqCst);
    let mut notebook = notebook!(title, details, summary, other).unwrap();
    notebook.run_all().unwrap();
    assert_eq!(
        notebook.get::<String>("summary").unwrap().as_str(),
        "Report: false"
    );

    assert_eq!(
        notebook
            .set_signal("title", serde_json::json!("New title"))
            .unwrap(),
        vec!["summary"]
    );
    assert_eq!(
        notebook
            .set_signal("details", serde_json::json!(true))
            .unwrap(),
        vec!["summary"]
    );
    assert_eq!(
        notebook.get::<String>("summary").unwrap().as_str(),
        "New title: true"
    );
    assert_eq!(SUMMARY_RUNS.load(Ordering::SeqCst), 3);
    assert_eq!(OTHER_RUNS.load(Ordering::SeqCst), 1);
    assert_eq!(
        notebook
            .set_signal("details", serde_json::json!("yes"))
            .unwrap_err()
            .code,
        "invalid_signal"
    );
    assert!(notebook.get::<Ui<bool>>("details").unwrap().value());
}

#[test]
fn running_a_cell_runs_its_descendants_but_not_independent_cells() {
    let _lock = TEST_LOCK.lock().unwrap();
    let mut notebook = notebook!(data, limit, filtered, other).unwrap();
    notebook.run_all().unwrap();
    let executed = notebook.run_from("data").unwrap();
    assert_eq!(executed, vec!["data", "filtered"]);
    let state = notebook.snapshot();
    let runs = |name: &str| {
        state
            .cells
            .iter()
            .find(|cell| cell.name == name)
            .unwrap()
            .run_count
    };
    assert_eq!(runs("data"), 2);
    assert_eq!(runs("filtered"), 2);
    assert_eq!(runs("limit"), 1);
    assert_eq!(runs("other"), 1);
}
