use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

use crate::RuntimeError;

pub type CellValue = Arc<dyn Any + Send + Sync>;

#[derive(Default)]
pub struct StateVault {
    values: HashMap<String, CellValue>,
}

impl StateVault {
    pub fn insert(&mut self, name: &str, value: CellValue) {
        self.values.insert(name.to_owned(), value);
    }

    pub fn get<T: Any + Send + Sync>(&self, name: &str) -> Result<Arc<T>, RuntimeError> {
        let value = self.values.get(name).ok_or_else(|| {
            RuntimeError::new(
                "missing_value",
                format!("cell '{name}' has no current value"),
                Some(name),
            )
        })?;
        Arc::clone(value).downcast::<T>().map_err(|_| {
            RuntimeError::new(
                "type_mismatch",
                format!("value of '{name}' is not {}", std::any::type_name::<T>()),
                Some(name),
            )
        })
    }

    pub fn remove(&mut self, name: &str) {
        self.values.remove(name);
    }

    pub fn contains(&self, name: &str) -> bool {
        self.values.contains_key(name)
    }
}
