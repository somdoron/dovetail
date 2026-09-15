use crate::common::diagnostics::Diagnostics;
use crate::common::span::{Span, Spanned};
use crate::parser::ast::{Declaration, Expr, FunctionDecl, Pattern, SourceFile};

use super::visitor::{ExprVisitor, walk_untyped_let, walk_untyped_match};

/// All type-parameter-related rules on the untyped AST:
/// 1. `Self` cannot be used as a type parameter name.
/// 2. Method type parameters must not shadow enclosing type parameters.
/// 3. Variable bindings in generic function bodies must not shadow type parameters.
pub(super) fn check_type_param_rules(source_files: &[&SourceFile], diagnostics: &mut Diagnostics) {
    for file in source_files {
        for decl in &file.declarations {
            check_decl(decl, diagnostics);
        }
    }
}

fn check_decl(decl: &Declaration, diagnostics: &mut Diagnostics) {
    match decl {
        Declaration::Function(func) => {
            check_self_in_type_params(&func.type_params, diagnostics);
            check_body_shadows(func, &[], diagnostics);
        }
        Declaration::Trait(trait_decl) => {
            check_self_in_type_params(&trait_decl.type_params, diagnostics);
            for method in &trait_decl.methods {
                check_self_in_type_params(&method.type_params, diagnostics);
                check_method_shadows_enclosing(
                    &method.type_params,
                    &trait_decl.type_params,
                    &method.span,
                    &method.name.value,
                    "trait",
                    &trait_decl.name.value,
                    diagnostics,
                );
                if let Some(body) = &method.body {
                    check_expression_shadows(
                        body,
                        &method.type_params,
                        &trait_decl.type_params,
                        diagnostics,
                    );
                }
            }
            for property in &trait_decl.properties {
                if let Some(body) = &property.body {
                    check_expression_shadows(body, &[], &trait_decl.type_params, diagnostics);
                }
            }
        }
        Declaration::Extension(ext) => {
            check_self_in_type_params(&ext.type_params, diagnostics);
            for method in &ext.methods {
                check_self_in_type_params(&method.type_params, diagnostics);
                check_method_shadows_enclosing(
                    &method.type_params,
                    &ext.type_params,
                    &method.span,
                    &method.name.value,
                    "extension",
                    &ext.name.value,
                    diagnostics,
                );
                check_body_shadows(method, &ext.type_params, diagnostics);
            }
        }
        Declaration::Module(module) => {
            check_self_in_type_params(&module.type_params, diagnostics);
            for func in &module.functions {
                check_self_in_type_params(&func.type_params, diagnostics);
                check_method_shadows_enclosing(
                    &func.type_params,
                    &module.type_params,
                    &func.span,
                    &func.name.value,
                    "module",
                    &module.name.value,
                    diagnostics,
                );
                check_body_shadows(func, &module.type_params, diagnostics);
            }
        }
        Declaration::Implement(impl_decl) => {
            check_self_in_type_params(&impl_decl.type_params, diagnostics);
            for method in &impl_decl.methods {
                check_self_in_type_params(&method.type_params, diagnostics);
                check_method_shadows_enclosing(
                    &method.type_params,
                    &impl_decl.type_params,
                    &method.span,
                    &method.name.value,
                    "implement",
                    &impl_decl.trait_name.value,
                    diagnostics,
                );
                check_body_shadows(method, &impl_decl.type_params, diagnostics);
            }
        }
        Declaration::Record(_)
        | Declaration::Enum(_)
        | Declaration::GlobalVar(_)
        | Declaration::Newtype(_)
        | Declaration::TypeAlias(_)
        | Declaration::Class(_)
        | Declaration::Test(_) => {}
    }
}

/// `Self` cannot be used as a type parameter name.
fn check_self_in_type_params(type_params: &[Spanned<String>], diagnostics: &mut Diagnostics) {
    for tp in type_params {
        if tp.value == "Self" {
            diagnostics.error(
                tp.span.clone(),
                "'Self' cannot be used as a type parameter name".to_string(),
            );
        }
    }
}

/// Method type parameters must not shadow enclosing type parameters.
fn check_method_shadows_enclosing(
    method_type_params: &[Spanned<String>],
    enclosing_type_params: &[Spanned<String>],
    method_span: &Span,
    method_name: &str,
    enclosing_kind: &str,
    enclosing_name: &str,
    diagnostics: &mut Diagnostics,
) {
    for method_tp in method_type_params {
        if enclosing_type_params
            .iter()
            .any(|tp| tp.value == method_tp.value)
        {
            diagnostics.error(
                method_span.clone(),
                format!(
                    "type parameter '{}' on method '{}' shadows type parameter '{}' on enclosing {} '{}'",
                    method_tp.value, method_name, method_tp.value, enclosing_kind, enclosing_name
                ),
            );
        }
    }
}

/// Walk a generic function body checking for variable bindings that shadow type parameters.
/// `enclosing_type_params` are type params from the enclosing extension/module/implement.
fn check_body_shadows(
    func: &FunctionDecl,
    enclosing_type_params: &[Spanned<String>],
    diagnostics: &mut Diagnostics,
) {
    check_expression_shadows(
        &func.body,
        &func.type_params,
        enclosing_type_params,
        diagnostics,
    );
}

fn check_expression_shadows(
    body: &Expr,
    type_params: &[Spanned<String>],
    enclosing_type_params: &[Spanned<String>],
    diagnostics: &mut Diagnostics,
) {
    if type_params.is_empty() && enclosing_type_params.is_empty() {
        return;
    }
    let mut type_param_names: Vec<&str> = enclosing_type_params
        .iter()
        .map(|tp| tp.value.as_str())
        .collect();
    type_param_names.extend(type_params.iter().map(|tp| tp.value.as_str()));
    let mut checker = BodyShadowChecker {
        type_params: &type_param_names,
    };
    checker.visit_expr(body, diagnostics);
}

struct BodyShadowChecker<'a> {
    type_params: &'a [&'a str],
}

impl BodyShadowChecker<'_> {
    fn check_name(&self, name: &str, span: &Span, diagnostics: &mut Diagnostics) {
        for tp in self.type_params {
            if name == *tp {
                diagnostics.error(
                    span.clone(),
                    format!("variable '{}' shadows type parameter '{}'", name, tp),
                );
            }
        }
    }

    fn check_pattern(&self, pattern: &Pattern, diagnostics: &mut Diagnostics) {
        match pattern {
            Pattern::Variable(name, span) => {
                self.check_name(name, span, diagnostics);
            }
            Pattern::TypeAnnotated { binding, span, .. } => {
                self.check_name(binding, span, diagnostics);
            }
            Pattern::Record { fields, .. } => {
                for field in fields {
                    match &field.pattern {
                        Some(pat) => self.check_pattern(pat, diagnostics),
                        // Bare field shorthand `{ x }` binds variable `x`
                        None => self.check_name(&field.name.value, &field.span, diagnostics),
                    }
                }
            }
            Pattern::EnumVariant { .. } => {}
            Pattern::EnumVariantTuple {
                payload_patterns, ..
            } => {
                for sub_pat in payload_patterns {
                    self.check_pattern(sub_pat, diagnostics);
                }
            }
            Pattern::EnumVariantRecord { fields, .. } => {
                for field in fields {
                    if let Some(pat) = &field.pattern {
                        self.check_pattern(pat, diagnostics);
                    } else {
                        self.check_name(&field.name.value, &field.span, diagnostics);
                    }
                }
            }
            Pattern::Tuple(sub_pats, _) => {
                for sub_pat in sub_pats {
                    self.check_pattern(sub_pat, diagnostics);
                }
            }
            Pattern::Wildcard(_) | Pattern::Literal(_, _) => {}
        }
    }
}

impl ExprVisitor for BodyShadowChecker<'_> {
    fn visit_let(&mut self, expr: &Expr, diagnostics: &mut Diagnostics) {
        match expr {
            Expr::Let { name, .. } => {
                self.check_name(&name.value, &name.span, diagnostics);
            }
            Expr::LetDestructure { pattern, .. } => {
                self.check_pattern(pattern, diagnostics);
            }
            _ => {}
        }
        walk_untyped_let(self, expr, diagnostics);
    }

    fn visit_match(&mut self, expr: &Expr, diagnostics: &mut Diagnostics) {
        if let Expr::Match { arms, .. } = expr {
            for arm in arms {
                self.check_pattern(&arm.pattern, diagnostics);
            }
        }
        walk_untyped_match(self, expr, diagnostics);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::diagnostics::Diagnostics;
    use crate::layout::LayoutFilter;
    use crate::lexer::Lexer;
    use crate::lexer::attach_doc_comments;
    use crate::parser::Parser;
    use std::sync::Arc;

    fn parse_and_check(source: &str) -> Vec<String> {
        let file = Arc::from("test.dove");
        let mut lexer = Lexer::new(source, file);
        let raw_tokens = lexer.tokenize();
        let raw_tokens = attach_doc_comments(raw_tokens);
        let mut filter = LayoutFilter::new(raw_tokens);
        let tokens = filter.filter();
        let mut parser = Parser::new(tokens);
        let source_file = parser.parse_source_file();
        let mut diagnostics = Diagnostics::new();
        check_type_param_rules(&[&source_file], &mut diagnostics);
        diagnostics.iter().map(|d| d.message.clone()).collect()
    }

    fn assert_has_error(errors: &[String], needle: &str) {
        assert!(
            errors.iter().any(|e| e.contains(needle)),
            "expected error containing '{}', got: {:?}",
            needle,
            errors
        );
    }

    fn assert_no_errors(errors: &[String]) {
        assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
    }

    // -- Generic function in generic extension: body shadows extension type param --

    #[test]
    fn extension_method_body_shadows_extension_type_param() {
        let errors = parse_and_check(
            r#"
package a

extension Wrapper<T> for Int32 =
    function foo(self): Int32 =
        let T = 42
        T
"#,
        );
        assert_has_error(&errors, "variable 'T' shadows type parameter 'T'");
    }

    #[test]
    fn extension_generic_method_body_shadows_method_type_param() {
        let errors = parse_and_check(
            r#"
package a

extension Wrapper<T> for Int32 =
    function foo<U>(self): Int32 =
        let U = 42
        U
"#,
        );
        assert_has_error(&errors, "variable 'U' shadows type parameter 'U'");
    }

    #[test]
    fn extension_generic_method_body_shadows_extension_type_param() {
        let errors = parse_and_check(
            r#"
package a

extension Wrapper<T> for Int32 =
    function foo<U>(self): Int32 =
        let T = 42
        T
"#,
        );
        assert_has_error(&errors, "variable 'T' shadows type parameter 'T'");
    }

    #[test]
    fn extension_method_body_no_shadow_ok() {
        let errors = parse_and_check(
            r#"
package a

extension Wrapper<T> for Int32 =
    function foo<U>(self): Int32 =
        let x = 42
        x
"#,
        );
        assert_no_errors(&errors);
    }

    // -- Generic function in generic module: body shadows module type param --

    #[test]
    fn module_method_body_shadows_module_type_param() {
        let errors = parse_and_check(
            r#"
package a

module Box<T> =
    function unwrap(): Int32 =
        let T = 42
        T
"#,
        );
        assert_has_error(&errors, "variable 'T' shadows type parameter 'T'");
    }

    #[test]
    fn module_generic_method_body_shadows_method_type_param() {
        let errors = parse_and_check(
            r#"
package a

module Box<T> =
    function convert<U>(): Int32 =
        let U = 42
        U
"#,
        );
        assert_has_error(&errors, "variable 'U' shadows type parameter 'U'");
    }

    #[test]
    fn module_generic_method_body_shadows_module_type_param() {
        let errors = parse_and_check(
            r#"
package a

module Box<T> =
    function convert<U>(): Int32 =
        let T = 42
        T
"#,
        );
        assert_has_error(&errors, "variable 'T' shadows type parameter 'T'");
    }

    #[test]
    fn module_method_body_no_shadow_ok() {
        let errors = parse_and_check(
            r#"
package a

module Box<T> =
    function convert<U>(): Int32 =
        let x = 42
        x
"#,
        );
        assert_no_errors(&errors);
    }

    // -- Method type param shadows enclosing type param --

    #[test]
    fn extension_method_type_param_shadows_extension() {
        let errors = parse_and_check(
            r#"
package a

extension Wrapper<T> for Int32 =
    function foo<T>(self): Int32 = 42
"#,
        );
        assert_has_error(&errors, "shadows type parameter 'T' on enclosing extension");
    }

    #[test]
    fn module_method_type_param_shadows_module() {
        let errors = parse_and_check(
            r#"
package a

module Box<T> =
    function foo<T>(): Int32 = 42
"#,
        );
        assert_has_error(&errors, "shadows type parameter 'T' on enclosing module");
    }

    // -- Non-generic method in generic extension/module: body shadows enclosing --

    #[test]
    fn non_generic_extension_method_body_shadows_extension_type_param() {
        let errors = parse_and_check(
            r#"
package a

extension Wrapper<T> for Int32 =
    function bar(self): Int32 =
        let T = 10
        T
"#,
        );
        assert_has_error(&errors, "variable 'T' shadows type parameter 'T'");
    }

    #[test]
    fn non_generic_module_method_body_shadows_module_type_param() {
        let errors = parse_and_check(
            r#"
package a

module Box<T> =
    function bar(): Int32 =
        let T = 10
        T
"#,
        );
        assert_has_error(&errors, "variable 'T' shadows type parameter 'T'");
    }
}
