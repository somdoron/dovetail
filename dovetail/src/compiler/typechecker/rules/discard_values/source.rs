use std::collections::BTreeMap;

use crate::common::diagnostics::Diagnostics;
use crate::common::span::{FilePath, Span};
use crate::parser::ast::{ClassMember, Declaration, Expr, FunctionDecl, PropertyDecl, SourceFile};

use super::super::visitor::{self, ExprVisitor};

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct SourceLocation(FilePath, u32, u32, u32, u32);

impl From<&Span> for SourceLocation {
    fn from(span: &Span) -> Self {
        Self(
            span.file.clone(),
            span.line,
            span.column,
            span.end_line,
            span.end_column,
        )
    }
}

pub(super) type DiscardSites = BTreeMap<SourceLocation, bool>;

pub(super) fn collect(files: &[&SourceFile]) -> DiscardSites {
    let mut collector = SourceDiscards {
        sites: BTreeMap::new(),
        is_async: false,
    };
    let mut diagnostics = Diagnostics::new();
    for file in files {
        for declaration in &file.declarations {
            collector.declaration(declaration, &mut diagnostics);
        }
    }
    collector.sites
}

struct SourceDiscards {
    sites: DiscardSites,
    is_async: bool,
}

impl SourceDiscards {
    fn function(&mut self, function: &FunctionDecl, diagnostics: &mut Diagnostics) {
        self.body(&function.body, function.is_async, diagnostics);
    }

    fn properties(&mut self, properties: &[PropertyDecl], diagnostics: &mut Diagnostics) {
        for property in properties {
            if let Some(body) = &property.body {
                self.body(body, false, diagnostics);
            }
        }
    }

    fn body(&mut self, body: &Expr, is_async: bool, diagnostics: &mut Diagnostics) {
        let previous = self.is_async;
        self.is_async = is_async;
        self.visit_expr(body, diagnostics);
        self.is_async = previous;
    }

    fn declaration(&mut self, declaration: &Declaration, diagnostics: &mut Diagnostics) {
        match declaration {
            Declaration::Function(function) => self.function(function, diagnostics),
            Declaration::GlobalVar(global) => self.visit_expr(&global.value, diagnostics),
            Declaration::Test(test) => self.visit_expr(&test.body, diagnostics),
            Declaration::Module(module) => {
                for function in &module.functions {
                    self.function(function, diagnostics);
                }
                self.properties(&module.properties, diagnostics);
                for global in &module.globals {
                    self.visit_expr(&global.value, diagnostics);
                }
                for test in &module.tests {
                    self.visit_expr(&test.body, diagnostics);
                }
            }
            Declaration::Implement(block) => {
                for method in &block.methods {
                    self.function(method, diagnostics);
                }
                self.properties(&block.properties, diagnostics);
            }
            Declaration::Extension(block) => {
                for method in &block.methods {
                    self.function(method, diagnostics);
                }
                self.properties(&block.properties, diagnostics);
            }
            Declaration::Trait(trait_decl) => {
                for method in &trait_decl.methods {
                    if let Some(body) = &method.body {
                        self.body(body, false, diagnostics);
                    }
                }
                self.properties(&trait_decl.properties, diagnostics);
            }
            Declaration::Class(class) => {
                if let Some(extends) = &class.extends {
                    for argument in &extends.super_args {
                        self.visit_expr(argument, diagnostics);
                    }
                }
                for parameter in &class.params {
                    if let Some(value) = &parameter.default_value {
                        self.visit_expr(value, diagnostics);
                    }
                }
                for member in &class.body {
                    match member {
                        ClassMember::Method(function) => self.function(function, diagnostics),
                        ClassMember::Property(property) => {
                            self.properties(std::slice::from_ref(property), diagnostics);
                        }
                        ClassMember::LetBinding(binding) => {
                            self.visit_expr(&binding.value, diagnostics)
                        }
                        ClassMember::Expression(expr) => self.discard(expr, diagnostics),
                    }
                }
            }
            Declaration::Record(_)
            | Declaration::Enum(_)
            | Declaration::Newtype(_)
            | Declaration::TypeAlias(_) => {}
        }
    }

    /// Propagate discard only through expressions that forward their result.
    /// Everything else consumes its operands, which may contain independent
    /// block statements that still need checking.
    fn discard(&mut self, expr: &Expr, diagnostics: &mut Diagnostics) {
        match expr {
            Expr::Block(block) => {
                for expression in &block.expressions {
                    self.discard(expression, diagnostics);
                }
            }
            Expr::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                self.visit_expr(condition, diagnostics);
                self.discard(then_branch, diagnostics);
                if let Some(branch) = else_branch {
                    self.discard(branch, diagnostics);
                }
            }
            Expr::Match { subject, arms, .. } => {
                self.visit_expr(subject, diagnostics);
                for arm in arms {
                    if let Some(guard) = &arm.guard {
                        self.visit_expr(guard, diagnostics);
                    }
                    self.discard(&arm.body, diagnostics);
                }
            }
            Expr::Let { .. }
            | Expr::LetDestructure { .. }
            | Expr::Assignment { .. }
            | Expr::Use { .. } => self.visit_expr(expr, diagnostics),
            _ => {
                let span = expr.span();
                // Macro expansion sources have no user-editable discard site.
                if !span.file.starts_with("<derive:") {
                    self.sites
                        .insert(SourceLocation::from(&span), self.is_async);
                }
                self.visit_expr(expr, diagnostics);
            }
        }
    }
}

impl ExprVisitor for SourceDiscards {
    fn visit_expr(&mut self, expr: &Expr, diagnostics: &mut Diagnostics) {
        match expr {
            Expr::AsyncDo { body, .. } => self.body(body, true, diagnostics),
            Expr::Closure { body, is_async, .. } => self.body(body, *is_async, diagnostics),
            _ => visitor::walk_untyped_expr(self, expr, diagnostics),
        }
    }

    fn visit_block(&mut self, expr: &Expr, diagnostics: &mut Diagnostics) {
        if let Expr::Block(block) = expr
            && let Some((last, intermediate)) = block.expressions.split_last()
        {
            for expression in intermediate {
                self.discard(expression, diagnostics);
            }
            self.visit_expr(last, diagnostics);
        }
    }
}
