//! Canonical source formatting shared by the CLI and language server.
mod document;
pub mod files;
mod printer;
mod syntax;

use crate::common::span::FilePath;
use crate::discovery::parse_source;
use crate::parser::ast::SourceFile;
use serde_json::Value;
use std::fmt;

#[derive(Debug)]
pub struct FormatError(pub String);

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for FormatError {}

/// Format a complete source document without consulting the filesystem.
pub fn format_source(source: &str, file_path: FilePath) -> Result<String, FormatError> {
    let parsed = syntax::parse(source, file_path.clone())?;
    let original = parsed.ast.as_ref().map(structure).transpose()?;
    let output = document::render(&parsed.document(source)?);
    let formatted = syntax::parse(&output, file_path.clone())?;
    let actual = formatted.ast.as_ref().map(structure).transpose()?;
    if original != actual {
        return Err(FormatError(format!(
            "formatter changed parsed program structure at {}; source was left unchanged",
            difference(
                original.as_ref().unwrap_or(&Value::Null),
                actual.as_ref().unwrap_or(&Value::Null),
                String::new()
            )
        )));
    }
    if parsed.comments(source) != formatted.comments(&output) {
        return Err(FormatError(
            "formatter changed comments; source was left unchanged".into(),
        ));
    }
    Ok(output)
}

fn structure(ast: &impl serde::Serialize) -> Result<Value, FormatError> {
    let mut value = serde_json::to_value(ast).map_err(|error| FormatError(error.to_string()))?;
    normalize(&mut value);
    Ok(value)
}

fn normalize(value: &mut Value) {
    match value {
        Value::Object(object) => {
            if object.contains_key("file") && object.contains_key("end_column") {
                // Keep Some(Span) distinct from None: @stringLiteral stores its
                // semantic presence as an optional span.
                *value = Value::String("<source-span>".into());
                return;
            }
            object.remove("span");
            // Locations on literal attributes are metadata, too.
            for child in object.values_mut() {
                normalize(child);
            }
            if let Some(block) = object.get("Block")
                && let Some(expressions) = block.get("expressions").and_then(Value::as_array)
                && expressions.len() == 1
                && !scope_sensitive(&expressions[0])
            {
                *value = expressions[0].clone();
            }
        }
        Value::Array(values) => {
            for child in values {
                normalize(child);
            }
        }
        _ => {}
    }
}

fn scope_sensitive(value: &Value) -> bool {
    value.as_object().is_some_and(|object| {
        object
            .keys()
            .any(|key| matches!(key.as_str(), "Let" | "LetDestructure" | "Use"))
    })
}

fn validate(source: &str, file_path: FilePath) -> Result<SourceFile, FormatError> {
    let (ast, diagnostics) = parse_source(source, file_path);
    if diagnostics.has_errors() {
        let messages = diagnostics
            .iter()
            .map(|diagnostic| {
                format!(
                    "{}:{}:{}: {}",
                    diagnostic.span.file,
                    diagnostic.span.line,
                    diagnostic.span.column,
                    diagnostic.message
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        return Err(FormatError(messages));
    }
    Ok(ast)
}

#[cfg(test)]
mod tests;

fn difference(left: &Value, right: &Value, path: String) -> String {
    match (left, right) {
        (Value::Object(left), Value::Object(right)) => {
            for (key, value) in left {
                if right.get(key) != Some(value) {
                    return difference(
                        value,
                        right.get(key).unwrap_or(&Value::Null),
                        format!("{path}.{key}"),
                    );
                }
            }
        }
        (Value::Array(left), Value::Array(right)) => {
            for (index, value) in left.iter().enumerate() {
                if right.get(index) != Some(value) {
                    return difference(
                        value,
                        right.get(index).unwrap_or(&Value::Null),
                        format!("{path}[{index}]"),
                    );
                }
            }
        }
        _ => {}
    }
    if path.is_empty() {
        "<root>".into()
    } else {
        path
    }
}
