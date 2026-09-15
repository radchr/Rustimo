use rustimo::{Ui, View, cell, display, notebook, serve};

#[cell]
fn introduction() -> &'static str {
    display(View::markdown(
        "# Аналіз даних у Rustimo\n\nКлітинки нижче утворюють граф залежностей. Змініть вхідні значення або код окремої клітинки.",
    ));
    "introduction"
}

#[cell]
fn data() -> Vec<u32> {
    let values = vec![10, 20, 30, 40];
    display(View::text(format!("Вхідні дані: {values:?}")));
    values
}

#[cell]
fn limit() -> Ui<u32> {
    Ui::slider("limit", 0, 40, 15)
        .step(5)
        .label("Мінімальне значення")
}

#[allow(clippy::ptr_arg)] // The reference must match the producer's concrete stored type.
#[cell]
fn filtered(data: &Vec<u32>, limit: &Ui<u32>) -> Vec<u32> {
    let values: Vec<u32> = data
        .iter()
        .copied()
        .filter(|value| *value >= limit.value())
        .collect();
    display(View::text(format!("Відфільтровані значення: {values:?}")));
    values
}

#[allow(clippy::ptr_arg)]
#[cell]
fn count(filtered: &Vec<u32>) -> usize {
    let count = filtered.len();
    display(View::text(format!("Кількість: {count}")));
    count
}

#[cell]
fn topic() -> Ui<String> {
    Ui::text("topic", "Числовий аналіз").label("Назва звіту")
}

#[cell]
fn details() -> Ui<bool> {
    Ui::checkbox("details", true).label("Показати підсумок")
}

#[cell]
fn report(count: &usize, topic: &Ui<String>, details: &Ui<bool>) -> String {
    let title = topic.value();
    let markdown = if details.value() {
        format!("## {title}\n\nПісля фільтрації залишилося **{count}** значень.")
    } else {
        format!("## {title}\n\nПідсумок приховано віджетом.")
    };
    display(View::markdown(markdown.clone()));
    markdown
}

#[cell]
fn independent() -> &'static str {
    display(View::text("Ця клітинка не залежить від слайдера."));
    "independent"
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut app = notebook!(
        introduction,
        data,
        limit,
        filtered,
        count,
        topic,
        details,
        report,
        independent
    )?;
    app.run_all()?;
    let addr = std::env::var("RUSTIMO_ADDR").unwrap_or_else(|_| "127.0.0.1:3001".to_owned());
    serve(app, &addr)?;
    Ok(())
}
