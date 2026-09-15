//! Only source-level discarded values are candidates. Run on final inferred
//! bodies so speculative inference and later lowering cannot duplicate warnings.
mod source;

use std::collections::BTreeSet;

use crate::common::diagnostics::Diagnostics;
use crate::common::types::Fqn;
use crate::parser::ast::SourceFile;
use crate::typechecker::registry::Registry;
use crate::typechecker::types::{Type, TypeDef, TypedExpr, TypedModule};

use super::visitor::{self, TypedExprVisitor};
use source::{DiscardSites, SourceLocation};

pub fn check(
    module: &TypedModule,
    files: &[&SourceFile],
    registry: &Registry,
    diagnostics: &mut Diagnostics,
) {
    let mut rule = DiscardValues {
        sites: source::collect(files),
        registry,
        result: Fqn::from_dotted("standard.prelude.Result").unwrap(),
        asynchronous: Fqn::from_dotted("standard.io.Async").unwrap(),
        resource: Fqn::from_dotted("standard.io.Resource").unwrap(),
    };
    visitor::visit_all_functions(&mut rule, module, diagnostics);
    for function in module.function_templates.values() {
        rule.visit_expr(&function.body, diagnostics);
    }
    for block in &module.extension_blocks {
        for method in block.methods.iter().chain(&block.properties) {
            rule.visit_expr(&method.body, diagnostics);
        }
    }
    for definition in module.types.values() {
        if let TypeDef::Class(class) = definition {
            for expression in class
                .initializer
                .iter()
                .chain(class.extends_args.iter().flatten())
            {
                rule.visit_expr(expression, diagnostics);
            }
        }
    }
    for global in module.globals.values() {
        rule.visit_expr(&global.initializer, diagnostics);
    }
    for test in &module.tests {
        rule.visit_expr(&test.body, diagnostics);
    }
}

struct DiscardValues<'a> {
    registry: &'a Registry,
    sites: DiscardSites,
    result: Fqn,
    asynchronous: Fqn,
    resource: Fqn,
}

impl DiscardValues<'_> {
    fn is_async_type(&self, fqn: &Fqn) -> bool {
        let mut current = fqn;
        let mut visited = BTreeSet::new();
        while visited.insert(current) {
            if current == &self.asynchronous {
                return true;
            }
            let Some(parent) = self
                .registry
                .get_class_type(current)
                .and_then(|class| class.parent_class.as_ref())
            else {
                return false;
            };
            current = parent;
        }
        false
    }

    fn message(&self, ty: &Type, is_async: bool) -> Option<String> {
        // Do not unwrap newtypes or inspect contained types: only the identity
        // of the discarded value itself matters. Aliases are already expanded.
        let fqn = match ty {
            Type::Enum(fqn, _)
            | Type::Record(fqn, _)
            | Type::Class(fqn, _)
            | Type::Newtype(fqn, _)
            | Type::GenericEnum { fqn, .. }
            | Type::GenericRecord { fqn, .. }
            | Type::GenericClass { fqn, .. }
            | Type::GenericNewtype { fqn, .. } => fqn,
            Type::TypeVariable(_, bounds) | Type::GenericParam(_, bounds, _)
                if bounds
                    .iter()
                    .filter_map(|bound| bound.named())
                    .any(|bound| {
                        bound.is_class_bound() && self.is_async_type(&bound.trait_fqn)
                    }) =>
            {
                &self.asynchronous
            }
            _ => return None,
        };
        if fqn == &self.result {
            Some("discarded Result value may silently ignore a failure; handle or return the Result, or use 'let _ = ...' to intentionally discard it".into())
        } else if self.is_async_type(fqn) {
            let handling = if is_async {
                "await it when compatible with this async context, or return or compose it"
            } else {
                "return or compose it"
            };
            Some(format!(
                "discarded Async value does not execute the deferred computation; {handling}; 'let _ = ...' acknowledges discard without executing it"
            ))
        } else if fqn == &self.resource {
            let handling = if is_async {
                "acquire it with 'use' when compatible with this async context, or return or compose it"
            } else {
                "return or compose it for acquisition with 'use'"
            };
            Some(format!(
                "discarded Resource value does not acquire or use the resource; {handling}; 'let _ = ...' acknowledges discard without acquiring it"
            ))
        } else {
            None
        }
    }
}

impl TypedExprVisitor for DiscardValues<'_> {
    fn visit_expr(&mut self, expr: &TypedExpr, diagnostics: &mut Diagnostics) {
        // The outermost typed expression at a source site owns its resulting
        // value. Lowered operands may reuse its span but are not new discards.
        // Removing the site also deduplicates copied method/default bodies.
        if let Some(is_async) = self.sites.remove(&SourceLocation::from(&expr.span))
            && let Some(message) = self.message(&expr.ty, is_async)
        {
            diagnostics.warning(expr.span.clone(), message);
        }
        visitor::walk_expr(self, expr, diagnostics);
    }
}
