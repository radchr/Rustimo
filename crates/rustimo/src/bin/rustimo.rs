use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let command = args.next();
    let source = args.next();
    if command.as_deref() != Some(std::ffi::OsStr::new("edit"))
        || source.is_none()
        || args.next().is_some()
    {
        eprintln!("Usage: rustimo edit crates/rustimo/examples/basic.rs");
        std::process::exit(2);
    }
    rustimo::serve_edit(
        PathBuf::from(source.expect("checked above")),
        "127.0.0.1:3000",
    )?;
    Ok(())
}
