use std::collections::{BTreeMap, BTreeSet};

use crate::{CellDescriptor, RuntimeError};

pub struct ReactiveGraph {
    order: Vec<String>,
    children: BTreeMap<String, BTreeSet<String>>,
}

impl ReactiveGraph {
    pub fn new(cells: &[CellDescriptor]) -> Result<Self, RuntimeError> {
        let mut producers = BTreeMap::new();
        for cell in cells {
            if producers.insert(cell.name, cell.result_type).is_some() {
                return Err(RuntimeError::new(
                    "duplicate_definition",
                    format!("'{}' is defined by more than one cell", cell.name),
                    Some(cell.name),
                ));
            }
        }

        let mut children: BTreeMap<String, BTreeSet<String>> = producers
            .keys()
            .map(|name| ((*name).to_owned(), BTreeSet::new()))
            .collect();
        let mut indegree: BTreeMap<String, usize> = producers
            .keys()
            .map(|name| ((*name).to_owned(), 0))
            .collect();

        for cell in cells {
            for reference in &cell.refs {
                let actual = producers.get(reference.name).ok_or_else(|| {
                    RuntimeError::new(
                        "unknown_reference",
                        format!("'{}' needs undefined cell '{}'", cell.name, reference.name),
                        Some(cell.name),
                    )
                })?;
                if *actual != reference.expected_type {
                    return Err(RuntimeError::new(
                        "type_mismatch",
                        format!(
                            "'{}' expects '{}' as {}, but it produces {}",
                            cell.name, reference.name, reference.expected_type, actual
                        ),
                        Some(cell.name),
                    ));
                }
                if children
                    .get_mut(reference.name)
                    .expect("producer exists")
                    .insert(cell.name.to_owned())
                {
                    *indegree.get_mut(cell.name).expect("consumer exists") += 1;
                }
            }
        }

        let mut ready: BTreeSet<String> = indegree
            .iter()
            .filter(|(_, degree)| **degree == 0)
            .map(|(name, _)| name.clone())
            .collect();
        let mut order = Vec::with_capacity(cells.len());
        while let Some(name) = ready.pop_first() {
            for child in &children[&name] {
                let degree = indegree.get_mut(child).expect("consumer exists");
                *degree -= 1;
                if *degree == 0 {
                    ready.insert(child.clone());
                }
            }
            order.push(name);
        }
        if order.len() != cells.len() {
            let involved: Vec<_> = indegree
                .iter()
                .filter(|(_, degree)| **degree > 0)
                .map(|(name, _)| name.clone())
                .collect();
            return Err(RuntimeError::new(
                "cycle",
                format!("cell dependency cycle involves: {}", involved.join(", ")),
                involved.first().map(String::as_str),
            ));
        }
        Ok(Self { order, children })
    }

    pub fn order(&self) -> &[String] {
        &self.order
    }

    pub fn descendants(&self, name: &str) -> Vec<String> {
        let Some(_) = self.children.get(name) else {
            return Vec::new();
        };
        let mut dirty = BTreeSet::new();
        let mut pending = vec![name.to_owned()];
        while let Some(current) = pending.pop() {
            for child in &self.children[&current] {
                if dirty.insert(child.clone()) {
                    pending.push(child.clone());
                }
            }
        }
        self.order
            .iter()
            .filter(|cell| dirty.contains(*cell))
            .cloned()
            .collect()
    }
}
