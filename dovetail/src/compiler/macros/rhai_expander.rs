//! Rhai-backed derive macros.
//!
//! A [`RhaiDeriveExpander`] holds a Rhai script source string. On
//! expansion, the script receives an `input` Rhai object map describing
//! the target record/enum and is expected to return a Dovetail source
//! string for the generated `implement` block (and any sibling
//! declarations). The host re-parses that string through the normal
//! Lex → Layout → Parse pipeline and returns the resulting `Declaration`s
//! for the macro phase to splice into the package AST.
//!
//! ## Input shape
//!
//! ```text
//! input = #{
//!     kind: "record" | "enum" | "newtype",
//!     name: "Point",
//!     type_params: ["T", "U"],
//!     // records only:
//!     fields: [ #{ name: "x", ty: "Int32" } ],
//!     // enums only:
//!     variants: [
//!         #{ name: "Circle", kind: "none",  types: [] },
//!         #{ name: "Rect",   kind: "tuple", types: ["Int32", "Int32"] },
//!         #{ name: "P",      kind: "record", fields: [#{ name: "x", ty: "Int32" }] },
//!     ],
//!     // newtypes only:
//!     inner: "Int32",   // the inner type expression as source
//! }
//! ```
//!
//! The script returns a `String`. Failures in the script or in
//! re-parsing surface as [`MacroError`].

use std::sync::Arc;

use rhai::{Array, Dynamic, Engine, Map, Scope};

use crate::common::span::FilePath;
use crate::compiler::layout::LayoutFilter;
use crate::compiler::lexer::{Lexer, attach_doc_comments};
use crate::compiler::parser::Parser;
use crate::compiler::parser::ast::{
    Declaration, EnumDecl, EnumVariantPayload, NewtypeDecl, RecordDecl, TypeExpr,
};

use super::{DeriveExpander, DeriveInput, DeriveTarget, MacroError};

/// A derive macro implemented as a Rhai script.
pub struct RhaiDeriveExpander {
    /// The macro FQN, used for diagnostic file paths on synthetic declarations.
    fqn: String,
    /// The Rhai script source.
    script: String,
}

impl RhaiDeriveExpander {
    pub fn new(fqn: impl Into<String>, script: impl Into<String>) -> Self {
        Self {
            fqn: fqn.into(),
            script: script.into(),
        }
    }
}

impl DeriveExpander for RhaiDeriveExpander {
    fn expand(&self, input: DeriveInput<'_>) -> Result<Vec<Declaration>, MacroError> {
        let mut engine = Engine::new();
        // Generated-source builders chain many `+` string concatenations and
        // template interpolations; the default 64-deep expression limit is
        // easy to hit for non-trivial enum derives. 256 is generous without
        // risking runaway scripts.
        engine.set_max_expr_depths(256, 256);
        let mut scope = Scope::new();
        scope.push_constant("input", target_to_map(&input.target));

        let source: String = engine
            .eval_with_scope(&mut scope, &self.script)
            .map_err(|e| {
                MacroError::new(format!("rhai script error: {e}"))
            })?;

        if std::env::var("DOVETAIL_DEBUG_RHAI").is_ok() {
            eprintln!(
                "==== rhai derive '{}' generated source ====\n{}\n==== end ====",
                self.fqn, source
            );
        }

        parse_decls(&source, &self.fqn)
    }
}

/// Translate a `DeriveTarget` (record or enum AST node) into a Rhai
/// object map. Type expressions are stringified using their source-level
/// representation — derive scripts only need to copy them into the
/// generated source, never inspect them.
fn target_to_map(target: &DeriveTarget<'_>) -> Map {
    let mut map = Map::new();
    match target {
        DeriveTarget::Record(r) => {
            map.insert("kind".into(), Dynamic::from("record"));
            map.insert("name".into(), Dynamic::from(r.name.value.clone()));
            map.insert("type_params".into(), Dynamic::from(type_params(r)));
            map.insert("fields".into(), Dynamic::from(record_fields(r)));
            map.insert("variants".into(), Dynamic::from(Array::new()));
            map.insert("inner".into(), Dynamic::from(""));
        }
        DeriveTarget::Enum(e) => {
            map.insert("kind".into(), Dynamic::from("enum"));
            map.insert("name".into(), Dynamic::from(e.name.value.clone()));
            map.insert("type_params".into(), Dynamic::from(enum_type_params(e)));
            map.insert("fields".into(), Dynamic::from(Array::new()));
            map.insert("variants".into(), Dynamic::from(enum_variants(e)));
            map.insert("inner".into(), Dynamic::from(""));
        }
        DeriveTarget::Newtype(n) => {
            map.insert("kind".into(), Dynamic::from("newtype"));
            map.insert("name".into(), Dynamic::from(n.name.value.clone()));
            map.insert("type_params".into(), Dynamic::from(newtype_type_params(n)));
            map.insert("fields".into(), Dynamic::from(Array::new()));
            map.insert("variants".into(), Dynamic::from(Array::new()));
            map.insert(
                "inner".into(),
                Dynamic::from(type_expr_to_source(&n.inner_type)),
            );
        }
    }
    map
}

fn type_params(r: &RecordDecl) -> Array {
    r.type_params
        .iter()
        .map(|tp| Dynamic::from(tp.name.value.clone()))
        .collect()
}

fn enum_type_params(e: &EnumDecl) -> Array {
    e.type_params
        .iter()
        .map(|tp| Dynamic::from(tp.name.value.clone()))
        .collect()
}

fn newtype_type_params(n: &NewtypeDecl) -> Array {
    n.type_params
        .iter()
        .map(|tp| Dynamic::from(tp.name.value.clone()))
        .collect()
}

fn record_fields(r: &RecordDecl) -> Array {
    r.fields
        .iter()
        .map(|f| {
            let mut m = Map::new();
            m.insert("name".into(), Dynamic::from(f.name.value.clone()));
            m.insert("ty".into(), Dynamic::from(type_expr_to_source(&f.type_annotation)));
            Dynamic::from(m)
        })
        .collect()
}

fn enum_variants(e: &EnumDecl) -> Array {
    e.variants
        .iter()
        .map(|v| {
            let mut m = Map::new();
            m.insert("name".into(), Dynamic::from(v.name.value.clone()));
            match &v.payload {
                EnumVariantPayload::None => {
                    m.insert("kind".into(), Dynamic::from("none"));
                    m.insert("types".into(), Dynamic::from(Array::new()));
                    m.insert("fields".into(), Dynamic::from(Array::new()));
                }
                EnumVariantPayload::Tuple(types) => {
                    let types_arr: Array = types
                        .iter()
                        .map(|t| Dynamic::from(type_expr_to_source(t)))
                        .collect();
                    m.insert("kind".into(), Dynamic::from("tuple"));
                    m.insert("types".into(), Dynamic::from(types_arr));
                    m.insert("fields".into(), Dynamic::from(Array::new()));
                }
                EnumVariantPayload::Record(fields) => {
                    let fields_arr: Array = fields
                        .iter()
                        .map(|f| {
                            let mut fm = Map::new();
                            fm.insert("name".into(), Dynamic::from(f.name.value.clone()));
                            fm.insert(
                                "ty".into(),
                                Dynamic::from(type_expr_to_source(&f.type_annotation)),
                            );
                            Dynamic::from(fm)
                        })
                        .collect();
                    m.insert("kind".into(), Dynamic::from("record"));
                    m.insert("types".into(), Dynamic::from(Array::new()));
                    m.insert("fields".into(), Dynamic::from(fields_arr));
                }
            }
            Dynamic::from(m)
        })
        .collect()
}

/// Render a `TypeExpr` as Dovetail source. The script copies this string
/// into the generated code verbatim; we don't need to round-trip it
/// perfectly, just produce something that re-parses to the same shape.
fn type_expr_to_source(t: &TypeExpr) -> String {
    match t {
        TypeExpr::Named(n) => {
            if n.type_args.is_empty() {
                n.name.value.clone()
            } else {
                let args = n
                    .type_args
                    .iter()
                    .map(type_expr_to_source)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}<{}>", n.name.value, args)
            }
        }
        TypeExpr::TupleExtend(left, right, _) => {
            let operand = |ty: &TypeExpr| {
                let text = type_expr_to_source(ty);
                if matches!(ty, TypeExpr::Function(..)) { format!("({text})") } else { text }
            };
            format!("({} ~ {})", operand(left), operand(right))
        }
        TypeExpr::Tuple(items, _) => {
            let parts = items
                .iter()
                .map(type_expr_to_source)
                .collect::<Vec<_>>()
                .join(", ");
            format!("({parts})")
        }
        TypeExpr::Intersection(types) => types
            .iter()
            .map(|nt| {
                if nt.type_args.is_empty() {
                    nt.name.value.clone()
                } else {
                    let args = nt
                        .type_args
                        .iter()
                        .map(type_expr_to_source)
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{}<{}>", nt.name.value, args)
                }
            })
            .collect::<Vec<_>>()
            .join(" and "),
        TypeExpr::Function(params, ret, _) => {
            let params_s = params
                .iter()
                .map(type_expr_to_source)
                .collect::<Vec<_>>()
                .join(", ");
            format!("({params_s}) => {}", type_expr_to_source(ret))
        }
    }
}

/// Re-parse the script's returned Dovetail source through Lex / Layout / Parse.
///
/// The script is expected to return one or more top-level declarations
/// (typically a single `implement` block). We prepend a synthetic
/// `package _derive` line so the existing parser entry point can be
/// reused unchanged; the package decl gets discarded.
fn parse_decls(source: &str, macro_fqn: &str) -> Result<Vec<Declaration>, MacroError> {
    let wrapped = format!("package _derive\n\n{source}\n");
    let file_path: FilePath = Arc::from(format!("<derive:{macro_fqn}>").as_str());

    let mut lexer = Lexer::new(&wrapped, file_path);
    let tokens = lexer.tokenize();
    if !lexer.diagnostics().is_empty() {
        return Err(MacroError::new(format!(
            "rhai derive '{macro_fqn}' produced source that failed to lex: {}",
            lexer
                .diagnostics()
                .iter()
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        )));
    }

    let tokens = attach_doc_comments(tokens);
    let mut filter = LayoutFilter::new(tokens);
    let filtered = filter.filter();

    let mut parser = Parser::new(filtered);
    let source_file = parser.parse_source_file();
    if !parser.diagnostics().is_empty() {
        return Err(MacroError::new(format!(
            "rhai derive '{macro_fqn}' produced source that failed to parse: {}",
            parser
                .diagnostics()
                .iter()
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        )));
    }

    Ok(source_file.declarations)
}

#[cfg(test)]
mod tuple_extension_tests {
    use super::*;

    #[test]
    fn extension_function_operands_survive_macro_source_roundtrip() {
        for (source, left_function, right_function) in [
            ("type Example = Int32 ~ (Bool => Bool)", false, true),
            ("type Example = (Int32 => Int32) ~ Bool", true, false),
        ] {
            let declarations = parse_decls(source, "test").unwrap();
            let Declaration::TypeAlias(alias) = &declarations[0] else { panic!("expected alias") };
            let rendered = type_expr_to_source(&alias.type_expr);
            let declarations = parse_decls(&format!("type Example = {rendered}"), "test").unwrap();
            let Declaration::TypeAlias(alias) = &declarations[0] else { panic!("expected alias") };
            let TypeExpr::TupleExtend(left, right, _) = &alias.type_expr else { panic!("extension grouping lost: {rendered}") };
            assert_eq!(matches!(**left, TypeExpr::Function(..)), left_function);
            assert_eq!(matches!(**right, TypeExpr::Function(..)), right_function);
        }
    }
}
