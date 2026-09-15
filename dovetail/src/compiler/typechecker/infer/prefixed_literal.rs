//! Lowering for prefixed string literals (`sql"..."`).
//!
//! The literal is rewritten into an ordinary builder-call chain
//! (`Builder.empty().literal("...").value(x).spread(xs).build()`) which is then
//! inferred like any other expression. Nothing about the *meaning* of the
//! prefix lives here: the builder type comes from `[[project.literal]]`, and
//! every trait bound comes from the builder's own method signatures — so
//! `T: ToSqlValue` is enforced without the compiler knowing what that is.
//!
//! The one thing this module is careful about is spans. Each synthesized
//! `value`/`spread` call carries the span of the interpolation it came from,
//! and `check_trait_bounds` reports at the call site, so a bound failure points
//! at `$id` rather than at the literal or at generated code.

use crate::common::span::{Span, Spanned};
use crate::common::types::Fqn;
use crate::parser::ast::{Expr, LiteralPart};
use crate::typechecker::types::{Type, TypedExpr, TypedExprKind};

use super::Inference;
use super::types::SymbolKind;

/// The methods a literal builder must provide, in call order.
const BUILDER_METHODS: [&str; 5] = ["empty", "literal", "value", "spread", "build"];

impl Inference<'_> {
    pub(super) fn infer_prefixed_literal(
        &mut self,
        prefix: &Spanned<String>,
        parts: &[LiteralPart],
        span: &Span,
    ) -> TypedExpr {
        let Some(builder) = self.resolve_literal_builder(prefix, span) else {
            return self.prefixed_literal_error(span);
        };

        // Build the chain as untyped AST and hand it to ordinary inference:
        // overload resolution, generic instantiation, and bound checking then
        // all behave exactly as they would for hand-written code.
        let mut chain = self.builder_call(
            Expr::ResolvedTypeRef(builder, span.clone()),
            "empty",
            vec![],
            span,
        );
        for part in parts {
            let part_span = part.span().clone();
            let (method, arg) = match part {
                LiteralPart::Text(text, _) => (
                    "literal",
                    Expr::StringLiteral(text.clone(), part_span.clone()),
                ),
                LiteralPart::Value(expr, _) => ("value", expr.clone()),
                LiteralPart::Spread(expr, _) => ("spread", expr.clone()),
            };
            chain = self.builder_call(chain, method, vec![arg], &part_span);
        }
        chain = self.builder_call(chain, "build", vec![], span);

        self.infer_expr(&chain)
    }

    fn builder_call(&self, receiver: Expr, method: &str, args: Vec<Expr>, span: &Span) -> Expr {
        Expr::MethodCall {
            receiver: Box::new(receiver),
            method: Spanned::new(method.to_string(), span.clone()),
            receiver_type_args: vec![],
            type_args: vec![],
            args,
            span: span.clone(),
        }
    }

    /// Resolve `prefix` to its builder type.
    ///
    /// The prefix is an ordinary name: it resolves through this file's import
    /// scope exactly like a type reference would, which is why two libraries
    /// can both export a `sql` without colliding, and why
    /// `import com.pg.sql as pg` renames the literal to `pg"..."` for free.
    fn resolve_literal_builder(&mut self, prefix: &Spanned<String>, span: &Span) -> Option<Fqn> {
        // Resolve the name the same way a type reference would: type params are
        // not in play, so any of the type-shaped symbol kinds will do.
        let Some(ty) = self.resolve_type_name(&prefix.value, &[], &prefix.span) else {
            self.diagnostics.error(
                prefix.span.clone(),
                format!(
                    "unknown string-literal prefix `{}`; import the builder it names, \
                     e.g. `import standard.sqlite.{}`",
                    prefix.value, prefix.value
                ),
            );
            return None;
        };

        // The name must have been declared `@stringLiteral`. Checking the name
        // the user wrote — not the type it expands to — is deliberate: an alias
        // opts in, the underlying builder does not become a prefix by itself.
        let declared_fqn = self.resolve_fqn_for_literal_prefix(&prefix.value);
        let opted_in = declared_fqn
            .as_ref()
            .is_some_and(|fqn| self.is_literal_prefix(fqn));
        if !opted_in {
            self.diagnostics.error(
                prefix.span.clone(),
                format!(
                    "`{}` is not a string-literal prefix; mark the name with `@stringLiteral` \
                     (e.g. `@stringLiteral public type {} = MyBuilder`) to use it as one",
                    prefix.value, prefix.value
                ),
            );
            return None;
        }

        let builder = ty.to_fqn();

        let Some(module) = self.registry.lookup_module(&builder) else {
            self.diagnostics.error(
                span.clone(),
                format!(
                    "builder type `{builder}` for string-literal prefix `{}` has no module, \
                     so it provides none of the builder methods",
                    prefix.value
                ),
            );
            return None;
        };

        // Every builder method must be visible from here. A private or
        // incomplete builder is the library author's mistake, not the caller's,
        // so say so plainly instead of reporting a missing method.
        for name in BUILDER_METHODS {
            let sym = crate::common::types::SymbolName(name.to_string());
            let has_method = module.functions.contains_key(&sym)
                || module.generic_members.lookup(&sym).is_some();
            if !has_method {
                self.diagnostics.error(
                    span.clone(),
                    format!(
                        "builder type `{builder}` (string-literal prefix `{}`) has no method \
                         `{name}`; a literal builder must provide `empty`, `literal`, `value`, \
                         `spread`, and `build`, all `public`",
                        prefix.value
                    ),
                );
                return None;
            }
        }

        Some(builder)
    }

    /// Whether `fqn` was declared with `@stringLiteral`.
    ///
    /// The flag lives on the declaration's own signature rather than in a
    /// side table, so it cannot drift out of sync with the declaration and
    /// needs no separate merge across packages. Visibility is already settled
    /// by the time we hold an FQN — the name resolved through the import
    /// scope — so the visibility-free accessors are the right ones here.
    fn is_literal_prefix(&self, fqn: &Fqn) -> bool {
        self.registry
            .get_type_alias(fqn)
            .is_some_and(|sig| sig.is_string_literal)
            || self
                .registry
                .get_record_type(fqn)
                .is_some_and(|sig| sig.is_string_literal)
            || self
                .registry
                .get_class_type(fqn)
                .is_some_and(|sig| sig.is_string_literal)
    }

    /// The FQN the prefix name itself resolves to — an alias, record, or class —
    /// so the `@stringLiteral` opt-in can be checked against the name that was
    /// written rather than the type it expands to.
    fn resolve_fqn_for_literal_prefix(&self, name: &str) -> Option<Fqn> {
        for kind in [
            SymbolKind::TypeAlias,
            SymbolKind::Record,
            SymbolKind::Class,
            SymbolKind::Newtype,
            SymbolKind::Enum,
        ] {
            if let Some(fqn) = self.resolve_fqn(name, kind)
                && self.is_literal_prefix(&fqn)
            {
                return Some(fqn);
            }
        }
        // Not marked under any kind — fall back to whatever it does resolve to,
        // so the caller's diagnostic can talk about the right symbol.
        self.resolve_fqn(name, SymbolKind::TypeAlias)
            .or_else(|| self.resolve_fqn(name, SymbolKind::Record))
            .or_else(|| self.resolve_fqn(name, SymbolKind::Class))
    }

    fn prefixed_literal_error(&self, span: &Span) -> TypedExpr {
        TypedExpr {
            kind: TypedExprKind::UnitLiteral,
            ty: Type::Error,
            span: span.clone(),
        }
    }
}
