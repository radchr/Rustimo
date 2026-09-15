use std::cell::RefCell;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum View {
    Empty,
    Text(String),
    Markdown(String),
    Widget(serde_json::Value),
}

impl View {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }

    pub fn markdown(markdown: impl Into<String>) -> Self {
        Self::Markdown(markdown.into())
    }
}

thread_local! {
    static CURRENT_VIEW: RefCell<Option<View>> = const { RefCell::new(None) };
}

pub fn display(view: View) {
    CURRENT_VIEW.with(|slot| *slot.borrow_mut() = Some(view));
}

pub fn capture_output<T>(run: impl FnOnce() -> T) -> (T, Option<View>) {
    struct Reset(Option<View>);
    impl Drop for Reset {
        fn drop(&mut self) {
            CURRENT_VIEW.with(|slot| *slot.borrow_mut() = self.0.take());
        }
    }

    let old = CURRENT_VIEW.with(|slot| slot.replace(None));
    let _reset = Reset(old);
    let value = run();
    let view = CURRENT_VIEW.with(|slot| slot.borrow_mut().take());
    (value, view)
}
