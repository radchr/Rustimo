use std::sync::RwLock;

use serde::{Serialize, de::DeserializeOwned};

use crate::{RuntimeError, View};

pub struct Ui<T> {
    name: String,
    label: String,
    value: RwLock<T>,
    min: T,
    max: T,
    step: Option<T>,
}

impl<T> Ui<T>
where
    T: Clone + PartialOrd + Serialize + DeserializeOwned + Send + Sync + 'static,
{
    pub fn slider(name: impl Into<String>, min: T, max: T, default: T) -> Self {
        Self {
            name: name.into(),
            label: String::new(),
            value: RwLock::new(default),
            step: None,
            min,
            max,
        }
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    pub fn step(mut self, step: T) -> Self {
        self.step = Some(step);
        self
    }

    pub fn value(&self) -> T {
        self.value.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn set_json(&self, raw: serde_json::Value) -> Result<(), RuntimeError> {
        let value: T = serde_json::from_value(raw).map_err(|error| {
            RuntimeError::new("invalid_signal", error.to_string(), Some(&self.name))
        })?;
        if value < self.min || value > self.max {
            return Err(RuntimeError::new(
                "invalid_signal",
                format!("value for '{}' is outside the slider range", self.name),
                Some(&self.name),
            ));
        }
        *self.value.write().unwrap_or_else(|e| e.into_inner()) = value;
        Ok(())
    }

    pub fn to_view(&self) -> View {
        View::Widget(serde_json::json!({
            "kind": "slider",
            "name": self.name,
            "label": self.label,
            "min": self.min,
            "max": self.max,
            "step": self.step,
            "value": self.value(),
        }))
    }
}
