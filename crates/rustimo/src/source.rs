use serde::Serialize;
use syn::{FnArg, Item, Pat, Type, spanned::Spanned};

#[derive(Debug, Clone, Serialize)]
pub struct SourceCell {
    pub name: String,
    pub source: String,
    pub refs: Vec<String>,
    pub line: usize,
    #[serde(skip)]
    start: usize,
    #[serde(skip)]
    end: usize,
}

pub fn parse_cells(source: &str) -> Result<Vec<SourceCell>, syn::Error> {
    let file = syn::parse_file(source)?;
    let mut cells = Vec::new();
    for item in file.items {
        let Item::Fn(function) = item else { continue };
        if !function.attrs.iter().any(|attr| {
            let path = attr.path();
            path.is_ident("cell")
                || (path.segments.len() == 2
                    && path.segments[0].ident == "rustimo"
                    && path.segments[1].ident == "cell")
        }) {
            continue;
        }
        let start = function.attrs.first().map_or_else(
            || function.span().byte_range().start,
            |attr| attr.span().byte_range().start,
        );
        let end = function.block.brace_token.span.close().byte_range().end;
        let text = source.get(start..end).ok_or_else(|| {
            syn::Error::new(function.sig.ident.span(), "cell source span is unavailable")
        })?;
        let refs = function
            .sig
            .inputs
            .iter()
            .filter_map(|input| {
                let FnArg::Typed(argument) = input else {
                    return None;
                };
                let (Pat::Ident(pattern), Type::Reference(_)) =
                    (argument.pat.as_ref(), argument.ty.as_ref())
                else {
                    return None;
                };
                Some(pattern.ident.to_string())
            })
            .collect();
        cells.push(SourceCell {
            name: function.sig.ident.to_string(),
            source: text.to_owned(),
            refs,
            line: source[..start]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count()
                + 1,
            start,
            end,
        });
    }
    Ok(cells)
}

pub fn replace_cell(source: &str, name: &str, replacement: &str) -> Result<String, String> {
    let cells = parse_cells(source).map_err(|error| error.to_string())?;
    let cell = cells
        .iter()
        .find(|cell| cell.name == name)
        .ok_or_else(|| format!("unknown source cell '{name}'"))?;
    let mut updated = String::with_capacity(source.len() - cell.source.len() + replacement.len());
    updated.push_str(&source[..cell.start]);
    updated.push_str(replacement);
    updated.push_str(&source[cell.end..]);
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::{parse_cells, replace_cell};

    #[test]
    fn utf8_cell_replacement_preserves_other_rust_source() {
        let original = "use rustimo::cell;\n\n#[cell]\nfn data() -> String {\n    \"Привіт\".into()\n}\n\nfn main() { println!(\"data\"); }\n";
        let cells = parse_cells(original).unwrap();
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].name, "data");
        assert_eq!(cells[0].line, 3);
        assert!(cells[0].source.starts_with("#[cell]"));
        let updated = replace_cell(
            original,
            "data",
            "#[cell]\nfn data() -> String { \"Змінено\".into() }",
        )
        .unwrap();
        assert!(updated.contains("Змінено"));
        assert!(updated.ends_with("fn main() { println!(\"data\"); }\n"));
    }
}
