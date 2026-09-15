use std::sync::RwLock;

use serde::{Serialize, de::DeserializeOwned};

use crate::{RuntimeError, View};

pub struct Ui<T> {
    name: String,
    label: String,
    value: RwLock<T>,
    kind: UiKind<T>,
}

enum UiKind<T> {
    Slider { min: T, max: T, step: Option<T> },
    Text,
    Checkbox,
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
            kind: UiKind::Slider {
                min,
                max,
                step: None,
            },
        }
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    pub fn step(mut self, step: T) -> Self {
        if let UiKind::Slider { step: current, .. } = &mut self.kind {
            *current = Some(step);
        }
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
        if let UiKind::Slider { min, max, .. } = &self.kind
            && (value < *min || value > *max)
        {
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
        let widget = match &self.kind {
            UiKind::Slider { min, max, step } => serde_json::json!({
                "kind": "slider", "name": self.name, "label": self.label,
                "min": min, "max": max, "step": step, "value": self.value(),
            }),
            UiKind::Text => serde_json::json!({
                "kind": "text", "name": self.name, "label": self.label,
                "value": self.value(),
            }),
            UiKind::Checkbox => serde_json::json!({
                "kind": "checkbox", "name": self.name, "label": self.label,
                "value": self.value(),
            }),
        };
        View::Widget(widget)
    }
}

impl Ui<String> {
    pub fn text(name: impl Into<String>, default: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            label: String::new(),
            value: RwLock::new(default.into()),
            kind: UiKind::Text,
        }
    }
}

impl Ui<bool> {
    pub fn checkbox(name: impl Into<String>, default: bool) -> Self {
        Self {
            name: name.into(),
            label: String::new(),
            value: RwLock::new(default),
            kind: UiKind::Checkbox,
        }
    }
}
