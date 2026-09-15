use std::any::Any;
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;

use crate::graph::ReactiveGraph;
use crate::state::CellValue;
use crate::{StateVault, View};

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeError {
    pub code: String,
    pub message: String,
    pub cell: Option<String>,
}

impl RuntimeError {
    pub fn new(code: &str, message: impl Into<String>, cell: Option<&str>) -> Self {
        Self {
            code: code.to_owned(),
            message: message.into(),
            cell: cell.map(str::to_owned),
        }
    }
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for RuntimeError {}

#[derive(Clone)]
pub struct RefSpec {
    pub name: &'static str,
    pub expected_type: &'static str,
}

pub type CellRunner = fn(&StateVault) -> Result<CellExecution, RuntimeError>;
pub type SignalUpdater = fn(&StateVault, serde_json::Value) -> Result<View, RuntimeError>;
pub type InitialView = fn(&StateVault) -> Result<View, RuntimeError>;

#[derive(Clone)]
pub struct CellDescriptor {
    pub name: &'static str,
    pub refs: Vec<RefSpec>,
    pub result_type: &'static str,
    pub run: CellRunner,
    pub update_signal: Option<SignalUpdater>,
    pub initial_view: Option<InitialView>,
}

pub struct CellExecution {
    pub value: CellValue,
    pub view: Option<View>,
}

impl CellExecution {
    pub fn new<T: Any + Send + Sync>(value: T, view: Option<View>) -> Self {
        Self {
            value: Arc::new(value),
            view,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CellSnapshot {
    pub name: String,
    pub status: String,
    pub view: View,
    pub run_count: u64,
    pub execution_time_us: u128,
    pub error: Option<RuntimeError>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NotebookSnapshot {
    pub cells: Vec<CellSnapshot>,
    pub topo_order: Vec<String>,
}

pub struct Notebook {
    descriptors: HashMap<String, CellDescriptor>,
    display_order: Vec<String>,
    graph: ReactiveGraph,
    vault: StateVault,
    snapshots: HashMap<String, CellSnapshot>,
}

impl Notebook {
    pub fn new(cells: Vec<CellDescriptor>) -> Result<Self, RuntimeError> {
        let graph = ReactiveGraph::new(&cells)?;
        let display_order = cells.iter().map(|cell| cell.name.to_owned()).collect();
        let snapshots = cells
            .iter()
            .map(|cell| {
                (
                    cell.name.to_owned(),
                    CellSnapshot {
                        name: cell.name.to_owned(),
                        status: "idle".to_owned(),
                        view: View::Empty,
                        run_count: 0,
                        execution_time_us: 0,
                        error: None,
                    },
                )
            })
            .collect();
        Ok(Self {
            descriptors: cells
                .into_iter()
                .map(|cell| (cell.name.to_owned(), cell))
                .collect(),
            display_order,
            graph,
            vault: StateVault::default(),
            snapshots,
        })
    }

    pub fn run_all(&mut self) -> Result<Vec<String>, RuntimeError> {
        let order = self.graph.order().to_vec();
        for name in &order {
            self.execute(name)?;
        }
        Ok(order)
    }

    pub fn run_from(&mut self, name: &str) -> Result<Vec<String>, RuntimeError> {
        if !self.descriptors.contains_key(name) {
            return Err(RuntimeError::new(
                "unknown_cell",
                format!("unknown cell '{name}'"),
                Some(name),
            ));
        }
        let mut order = vec![name.to_owned()];
        order.extend(self.graph.descendants(name));
        for cell in &order {
            self.snapshots.get_mut(cell).expect("cell exists").status = "stale".to_owned();
        }
        for cell in &order {
            self.execute(cell)?;
        }
        Ok(order)
    }

    pub fn set_signal(
        &mut self,
        name: &str,
        value: serde_json::Value,
    ) -> Result<Vec<String>, RuntimeError> {
        let descriptor = self.descriptors.get(name).ok_or_else(|| {
            RuntimeError::new("unknown_cell", format!("unknown cell '{name}'"), Some(name))
        })?;
        let update = descriptor.update_signal.ok_or_else(|| {
            RuntimeError::new(
                "not_a_signal",
                format!("'{name}' is not a UI cell"),
                Some(name),
            )
        })?;
        let view = update(&self.vault, value)?;
        self.snapshots.get_mut(name).expect("cell exists").view = view;
        let order = self.graph.descendants(name);
        for cell in &order {
            self.snapshots.get_mut(cell).expect("cell exists").status = "stale".to_owned();
        }
        for cell in &order {
            self.execute(cell)?;
        }
        Ok(order)
    }

    pub fn get<T: Any + Send + Sync>(&self, name: &str) -> Result<Arc<T>, RuntimeError> {
        self.vault.get(name)
    }

    pub fn snapshot(&self) -> NotebookSnapshot {
        NotebookSnapshot {
            cells: self
                .display_order
                .iter()
                .map(|name| self.snapshots[name].clone())
                .collect(),
            topo_order: self.graph.order().to_vec(),
        }
    }

    fn execute(&mut self, name: &str) -> Result<(), RuntimeError> {
        let descriptor = &self.descriptors[name];
        let started = Instant::now();
        let result = catch_unwind(AssertUnwindSafe(|| (descriptor.run)(&self.vault)));
        let result = match result {
            Ok(result) => result,
            Err(payload) => {
                let message = payload
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| payload.downcast_ref::<&str>().copied())
                    .unwrap_or("cell panicked");
                Err(RuntimeError::new("panic", message, Some(name)))
            }
        };
        let elapsed = started.elapsed().as_micros();
        match result {
            Ok(output) => {
                self.vault.insert(name, output.value);
                let view = if let Some(view) = output.view {
                    view
                } else if let Some(make_view) = descriptor.initial_view {
                    make_view(&self.vault)?
                } else {
                    View::Empty
                };
                let state = self.snapshots.get_mut(name).expect("cell exists");
                state.status = "success".to_owned();
                state.view = view;
                state.run_count += 1;
                state.execution_time_us = elapsed;
                state.error = None;
                Ok(())
            }
            Err(error) => {
                self.vault.remove(name);
                let state = self.snapshots.get_mut(name).expect("cell exists");
                state.status = "error".to_owned();
                state.view = View::Empty;
                state.error = Some(error.clone());
                state.execution_time_us = elapsed;
                for child in self.graph.descendants(name) {
                    self.vault.remove(&child);
                    let state = self.snapshots.get_mut(&child).expect("cell exists");
                    state.status = "blocked".to_owned();
                    state.view = View::Empty;
                    state.error = Some(RuntimeError::new(
                        "blocked_by_parent",
                        format!("'{}' cannot run while '{}' has an error", child, name),
                        Some(&child),
                    ));
                }
                Err(error)
            }
        }
    }
}
