pub mod ast;

use crate::common::diagnostics::Diagnostic;
use crate::common::span::{Span, Spanned};
use crate::common::types::{Variance, Visibility};
use crate::lexer::token::{Token, TokenKind};
use ast::*;

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    diagnostics: Vec<Diagnostic>,
}

/// How a `property` declaration treats its body at this parse site.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PropertyBodyMode {
    /// Body required (`= expr`): implement/extension/module/class concrete.
    Required,
    /// Body optional: trait properties (a body is a default implementation).
    Optional,
    /// No body parsed: abstract class properties.
    Forbidden,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            diagnostics: Vec::new(),
        }
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Parse a complete source file.
    pub fn parse_source_file(&mut self) -> SourceFile {
        // Detect file-level module form: `module pkg.ModuleName`
        if self.at(TokenKind::Module) {
            return self.parse_file_level_module();
        }

        let package = self.parse_package_decl();

        let mut imports = Vec::new();
        while self.at(TokenKind::Import) {
            imports.push(self.parse_import_decl());
        }

        let mut declarations = Vec::new();
        while !self.at(TokenKind::Eof) {
            // Handle misplaced imports after declarations
            if self.at(TokenKind::Import) {
                self.error_at_current("imports must appear before declarations");
                self.parse_import_decl(); // consume and discard the import
                continue;
            }
            if let Some(decl) = self.parse_declaration() {
                declarations.push(decl);
            } else {
                // Error recovery: skip to next declaration-level keyword
                self.skip_to_declaration();
            }
        }

        SourceFile {
            package,
            imports,
            declarations,
        }
    }

    /// Parse a file-level module: `module pkg.ModuleName [<T>]`
    /// The module name is the last segment; preceding segments form the package path.
    fn parse_file_level_module(&mut self) -> SourceFile {
        let doc_comment = self.peek().doc_comment.clone();
        let start = self.peek().span.clone();
        self.advance(); // consume 'module'

        // Parse dotted path: `pkg.sub.ModuleName`
        let mut path = Vec::new();
        if let Some(ident) = self.expect_ident("expected module path") {
            path.push(ident);
        }
        while self.at(TokenKind::Dot) {
            self.advance();
            if let Some(ident) = self.expect_ident("expected name after '.'") {
                path.push(ident);
            }
        }

        if path.len() < 2 {
            self.error_at_current(
                "file-level module requires at least package.ModuleName (e.g. 'module myapp.Math')",
            );
        }

        // Last segment = module name, preceding = package path
        let module_name = path
            .last()
            .cloned()
            .unwrap_or_else(|| Spanned::new("<error>".to_string(), start.clone()));
        let package_segments: Vec<Spanned<String>> = if path.len() >= 2 {
            path[..path.len() - 1].to_vec()
        } else {
            vec![]
        };

        let pkg_span = if let Some(last_pkg) = package_segments.last() {
            start.merge(&last_pkg.span)
        } else {
            start.clone()
        };

        // Optional type parameters: <T, U>
        let type_params = if self.at(TokenKind::Lt) {
            self.parse_type_param_list()
        } else {
            vec![]
        };

        // Construct a TypeExpr representing the module's type (for bare `self` support)
        let for_type = TypeExpr::Named(NamedType {
            name: module_name.clone(),
            type_args: type_params
                .iter()
                .map(|tp| {
                    TypeExpr::Named(NamedType {
                        name: tp.clone(),
                        type_args: vec![],
                        span: tp.span.clone(),
                    })
                })
                .collect(),
            span: module_name.span.clone(),
        });

        // Parse imports
        let mut imports = Vec::new();
        while self.at(TokenKind::Import) {
            imports.push(self.parse_import_decl());
        }

        // Parse remaining declarations as module body
        let mut functions = Vec::new();
        let mut properties = Vec::new();
        let mut globals = Vec::new();
        let mut tests = Vec::new();

        while !self.at(TokenKind::Eof) {
            if self.at(TokenKind::Import) {
                self.error_at_current("imports must appear before declarations");
                self.parse_import_decl();
                continue;
            }
            let member_doc = self.peek().doc_comment.clone();

            // Check for test declarations before visibility parsing
            if self.at(TokenKind::At) {
                let attributes = self.parse_test_attributes();
                if self.peek().kind == TokenKind::Ident && self.peek().text == "test" && self.peek_at(1).kind == TokenKind::StringLiteral {
                    tests.push(self.parse_test_decl(attributes));
                } else {
                    self.error_at_current("expected 'test' declaration after attributes");
                }
                continue;
            }
            if self.peek().kind == TokenKind::Ident && self.peek().text == "test" && self.peek_at(1).kind == TokenKind::StringLiteral {
                tests.push(self.parse_test_decl(vec![]));
                continue;
            }

            // Parse visibility
            let visibility = match self.peek().kind {
                TokenKind::Public => {
                    self.advance();
                    Visibility::Public
                }
                TokenKind::Internal => {
                    self.advance();
                    Visibility::Internal
                }
                TokenKind::Private => {
                    self.advance();
                    Visibility::Private
                }
                _ => Visibility::Internal,
            };

            let is_async = if self.at(TokenKind::Async) && self.peek_at(1).kind == TokenKind::Function {
                self.advance(); // consume 'async'
                true
            } else {
                false
            };

            if self.at(TokenKind::Function) {
                let mut func = self.parse_extension_method_with_visibility(&for_type, visibility, member_doc);
                func.is_async = is_async;
                functions.push(func);
            } else if self.at(TokenKind::Property) {
                properties.push(self.parse_property_decl(&for_type, visibility, PropertyBodyMode::Required, member_doc));
            } else if self.at(TokenKind::Let) {
                if self.peek_at(1).kind == TokenKind::Property {
                    properties.push(self.parse_module_property(visibility, member_doc));
                } else {
                    globals.push(self.parse_global_var_decl(visibility, member_doc));
                }
            } else {
                self.error_at_current(
                    "expected 'function', 'property', 'let property', 'let', or 'test' in module body",
                );
                self.skip_to_declaration();
            }
        }

        let end_span = functions
            .last()
            .map(|f| f.span.clone())
            .or_else(|| properties.last().map(|p| p.span.clone()))
            .or_else(|| globals.last().map(|g| g.span.clone()))
            .or_else(|| tests.last().map(|t| t.span.clone()))
            .unwrap_or_else(|| start.clone());

        let module_decl = ModuleDecl {
            name: module_name,
            type_params,
            functions,
            properties,
            globals,
            tests,
            doc_comment,
            span: start.merge(&end_span),
        };

        SourceFile {
            package: PackageDecl {
                path: package_segments,
                span: pkg_span,
            },
            imports,
            declarations: vec![Declaration::Module(module_decl)],
        }
    }

    // ── Package declaration ──────────────────────────────────────────

    fn parse_package_decl(&mut self) -> PackageDecl {
        let start = self.peek().span.clone();

        if !self.at(TokenKind::Package) {
            self.error_at_current("expected 'package' declaration");
            return PackageDecl {
                path: vec![],
                span: start,
            };
        }
        self.advance();

        let mut path = Vec::new();

        // First segment
        if let Some(ident) = self.expect_ident("expected package name") {
            path.push(ident);
        }

        // Additional dotted segments
        while self.at(TokenKind::Dot) {
            self.advance();
            if let Some(ident) = self.expect_ident("expected package name after '.'") {
                path.push(ident);
            }
        }

        let span = if let Some(last) = path.last() {
            start.merge(&last.span)
        } else {
            start
        };

        PackageDecl { path, span }
    }

    // ── Import declarations ────────────────────────────────────────────

    fn parse_import_decl(&mut self) -> ImportDecl {
        let start = self.peek().span.clone();
        self.advance(); // consume 'import'

        let mut path = Vec::new();

        // First segment
        if let Some(ident) = self.expect_ident("expected import path") {
            path.push(ident);
        }

        // Additional dotted segments
        while self.at(TokenKind::Dot) {
            self.advance();
            if let Some(ident) = self.expect_ident("expected name after '.'") {
                path.push(ident);
            }
        }

        // Optional alias: `as name`
        let alias = if self.at(TokenKind::As) {
            self.advance();
            self.expect_ident("expected alias name after 'as'")
        } else {
            None
        };

        let end = alias
            .as_ref()
            .map(|a| &a.span)
            .or_else(|| path.last().map(|p| &p.span))
            .cloned()
            .unwrap_or_else(|| start.clone());
        let span = start.merge(&end);

        ImportDecl { path, alias, span }
    }

    // ── Declarations ─────────────────────────────────────────────────

    fn parse_declaration(&mut self) -> Option<Declaration> {
        // Grab doc comment from the first token of this declaration
        let doc_comment = self.peek().doc_comment.clone();

        // `@derive(...)` attributes may precede the visibility keyword on a
        // record or enum declaration. Attribute spans are stored on the decl
        // for diagnostics; the macro phase consumes them before Collect.
        let derive_attributes = self.parse_derive_attributes();
        let has_derives = !derive_attributes.is_empty();
        // `@stringLiteral` marks a name as the prefix of a prefixed string
        // literal. It takes no argument: the prefix *is* the declared name, so
        // there is nothing to keep in sync, and `import pkg.sql` is what brings
        // `sql"..."` into scope.
        let string_literal = self.parse_string_literal_attribute();

        let visibility = match self.peek().kind {
            TokenKind::Public => {
                self.advance();
                Visibility::Public
            }
            TokenKind::Internal => {
                self.advance();
                Visibility::Internal
            }
            TokenKind::Private => {
                self.advance();
                Visibility::Private
            }
            _ => Visibility::Internal,
        };

        // Helper: emit an error if an attribute precedes a declaration that
        // cannot carry it.
        let has_string_literal = string_literal.is_some();
        let reject_derive = |this: &mut Self| {
            if has_derives {
                this.error_at_current(
                    "@derive(...) can only appear on a record or enum declaration",
                );
            }
            if has_string_literal {
                this.error_at_current(
                    "@stringLiteral can only appear on a type alias, record, or class declaration",
                );
            }
        };
        // Classes accept `@stringLiteral` but not `@derive`.
        let reject_derive_only = |this: &mut Self| {
            if has_derives {
                this.error_at_current(
                    "@derive(...) can only appear on a record or enum declaration",
                );
            }
        };

        match self.peek().kind {
            TokenKind::Async if self.peek_at(1).kind == TokenKind::Function => {
                reject_derive(self);
                self.advance(); // consume 'async'
                Some(Declaration::Function(self.parse_function_decl(visibility, true, doc_comment)))
            }
            TokenKind::Function => {
                reject_derive(self);
                Some(Declaration::Function(self.parse_function_decl(visibility, false, doc_comment)))
            }
            TokenKind::Let => {
                reject_derive(self);
                Some(Declaration::GlobalVar(
                    self.parse_global_var_decl(visibility, doc_comment),
                ))
            }
            TokenKind::Record => Some(Declaration::Record(self.parse_record_decl(
                visibility,
                doc_comment,
                derive_attributes,
                string_literal,
            ))),
            TokenKind::Enum => Some(Declaration::Enum(self.parse_enum_decl(
                visibility,
                doc_comment,
                derive_attributes,
            ))),
            TokenKind::Trait => {
                reject_derive(self);
                Some(Declaration::Trait(self.parse_trait_decl(visibility, doc_comment, false)))
            }
            TokenKind::Interface => {
                reject_derive(self);
                Some(Declaration::Trait(self.parse_trait_decl(visibility, doc_comment, true)))
            }
            TokenKind::Extension => {
                reject_derive(self);
                Some(Declaration::Extension(self.parse_extension_decl(doc_comment)))
            }
            TokenKind::Implement => {
                reject_derive(self);
                Some(Declaration::Implement(self.parse_implement_decl(doc_comment)))
            }
            TokenKind::Module => {
                reject_derive(self);
                Some(Declaration::Module(self.parse_module_decl(doc_comment)))
            }
            TokenKind::Newtype => Some(Declaration::Newtype(self.parse_newtype_decl(
                visibility,
                doc_comment,
                derive_attributes,
            ))),
            TokenKind::Type => {
                if has_derives {
                    self.error_at_current(
                        "@derive(...) can only appear on a record or enum declaration",
                    );
                }
                Some(self.parse_type_alias_decl(
                    visibility,
                    doc_comment,
                    string_literal,
                ))
            }
            TokenKind::Class => {
                reject_derive_only(self);
                Some(Declaration::Class(self.parse_class_decl(visibility, false, false, false, doc_comment, string_literal)))
            }
            TokenKind::Final if self.peek_at(1).kind == TokenKind::Class => {
                reject_derive_only(self);
                self.advance(); // consume 'final'
                Some(Declaration::Class(self.parse_class_decl(visibility, true, false, false, doc_comment, string_literal)))
            }
            TokenKind::Abstract if self.peek_at(1).kind == TokenKind::Class => {
                reject_derive_only(self);
                self.advance(); // consume 'abstract'
                Some(Declaration::Class(self.parse_class_decl(visibility, false, true, false, doc_comment, string_literal)))
            }
            TokenKind::Abstract if self.peek_at(1).kind == TokenKind::Final && self.peek_at(2).kind == TokenKind::Class => {
                reject_derive_only(self);
                self.advance(); // consume 'abstract'
                self.advance(); // consume 'final'
                Some(Declaration::Class(self.parse_class_decl(visibility, true, true, false, doc_comment, string_literal)))
            }
            TokenKind::Sealed if self.peek_at(1).kind == TokenKind::Abstract && self.peek_at(2).kind == TokenKind::Class => {
                reject_derive_only(self);
                self.advance(); // consume 'sealed'
                self.advance(); // consume 'abstract'
                Some(Declaration::Class(self.parse_class_decl(visibility, false, true, true, doc_comment, string_literal)))
            }
            TokenKind::At => {
                reject_derive(self);
                if visibility != Visibility::Internal {
                    self.error_at_current("test declarations cannot have visibility modifiers");
                }
                let attributes = self.parse_test_attributes();
                if self.peek().kind == TokenKind::Ident && self.peek().text == "test" && self.peek_at(1).kind == TokenKind::StringLiteral {
                    Some(Declaration::Test(self.parse_test_decl(attributes)))
                } else {
                    self.error_at_current("expected 'test' declaration after attributes");
                    None
                }
            }
            TokenKind::Ident if self.peek().text == "test" && self.peek_at(1).kind == TokenKind::StringLiteral => {
                reject_derive(self);
                if visibility != Visibility::Internal {
                    self.error_at_current("test declarations cannot have visibility modifiers");
                }
                Some(Declaration::Test(self.parse_test_decl(vec![])))
            }
            _ => {
                reject_derive(self);
                None
            }
        }
    }

    /// Parse zero or more `@derive(MacroName)` or `@derive(a.b.MacroName)`
    /// attributes. Stops at the first `@` that isn't followed by `derive`
    /// (e.g. `@skip`, `@panics`, `@timeout`) so test-attribute parsing can
    /// pick them up later.
    /// `@stringLiteral` — marks the declared name as the prefix of a prefixed
    /// string literal. It takes no argument: the prefix is the name itself, so
    /// there is nothing that can drift out of sync, and the literal is in scope
    /// exactly where the name is (`import standard.sqlite.sql` enables
    /// `sql"..."`; `import com.pg.sql as pg` renames it to `pg"..."`).
    fn parse_string_literal_attribute(&mut self) -> Option<Span> {
        if !(self.at(TokenKind::At)
            && self.peek_at(1).kind == TokenKind::Ident
            && self.peek_at(1).text == "stringLiteral")
        {
            return None;
        }
        let at_span = self.peek().span.clone();
        self.advance(); // consume '@'
        let name_span = self.peek().span.clone();
        self.advance(); // consume 'stringLiteral'
        if self.at(TokenKind::LParen) {
            self.error_at_current(
                "@stringLiteral takes no arguments — the prefix is the declared name",
            );
        }
        Some(at_span.merge(&name_span))
    }

    fn parse_derive_attributes(&mut self) -> Vec<DeriveAttribute> {
        let mut attrs = Vec::new();
        while self.at(TokenKind::At)
            && self.peek_at(1).kind == TokenKind::Ident
            && self.peek_at(1).text == "derive"
        {
            let at_span = self.peek().span.clone();
            self.advance(); // consume '@'
            let derive_span = self.peek().span.clone();
            self.advance(); // consume 'derive'

            if !self.at(TokenKind::LParen) {
                self.error_at_current("expected '(' after @derive");
                attrs.push(DeriveAttribute {
                    macro_name: vec![],
                    span: at_span.merge(&derive_span),
                });
                continue;
            }
            self.advance(); // consume '('

            let mut path: Vec<Spanned<String>> = Vec::new();
            if let Some(name) = self.expect_ident("expected macro name in @derive(...)") {
                path.push(name);
                while self.at(TokenKind::Dot) {
                    self.advance();
                    if let Some(seg) = self.expect_ident("expected name after '.'") {
                        path.push(seg);
                    }
                }
            }

            let close_span = self.peek().span.clone();
            self.expect(TokenKind::RParen, "expected ')' after @derive macro name");

            attrs.push(DeriveAttribute {
                macro_name: path,
                span: at_span.merge(&close_span),
            });
        }
        attrs
    }

    fn parse_test_attributes(&mut self) -> Vec<TestAttribute> {
        let mut attributes = Vec::new();
        while self.at(TokenKind::At) {
            let at_span = self.peek().span.clone();
            self.advance(); // consume '@'

            let ident = if self.at(TokenKind::Ident) {
                let tok = self.peek().clone();
                self.advance();
                tok
            } else {
                self.error_at_current("expected attribute name after '@'");
                continue;
            };

            match ident.text.as_str() {
                "skip" => {
                    let reason = if self.at(TokenKind::LParen) {
                        self.advance(); // consume '('
                        let r = if self.at(TokenKind::StringLiteral) {
                            let tok = self.peek().clone();
                            self.advance();
                            Some(Spanned::new(tok.text, tok.span))
                        } else {
                            self.error_at_current("expected string literal for @skip reason");
                            None
                        };
                        self.expect(TokenKind::RParen, "expected ')' after @skip reason");
                        r
                    } else {
                        None
                    };
                    let span = at_span.merge(&ident.span);
                    attributes.push(TestAttribute::Skip { reason, span });
                }
                "panics" => {
                    let message = if self.at(TokenKind::LParen) {
                        self.advance(); // consume '('
                        let m = if self.at(TokenKind::StringLiteral) {
                            let tok = self.peek().clone();
                            self.advance();
                            Some(Spanned::new(tok.text, tok.span))
                        } else {
                            self.error_at_current("expected string literal for @panics message");
                            None
                        };
                        self.expect(TokenKind::RParen, "expected ')' after @panics message");
                        m
                    } else {
                        None
                    };
                    let span = at_span.merge(&ident.span);
                    attributes.push(TestAttribute::Panics { message, span });
                }
                "timeout" => {
                    self.expect(TokenKind::LParen, "expected '(' after @timeout");
                    let millis = if self.at(TokenKind::IntLiteral) {
                        let tok = self.peek().clone();
                        self.advance();
                        Spanned::new(tok.text, tok.span)
                    } else {
                        self.error_at_current("expected integer literal for @timeout milliseconds");
                        Spanned::new("0".to_string(), ident.span.clone())
                    };
                    self.expect(TokenKind::RParen, "expected ')' after @timeout value");
                    let span = at_span.merge(&ident.span);
                    attributes.push(TestAttribute::Timeout { millis, span });
                }
                name => {
                    self.error_at_current(&format!("unknown test attribute '@{name}'"));
                }
            }
        }
        attributes
    }

    fn parse_test_decl(&mut self, attributes: Vec<TestAttribute>) -> TestDecl {
        let start = self.peek().span.clone();
        self.advance(); // consume 'test' (Ident)

        // Next token should be a StringLiteral (already checked in parse_declaration)
        let name = if self.at(TokenKind::StringLiteral) {
            let token = self.peek().clone();
            self.advance();
            Spanned::new(token.text, token.span)
        } else {
            self.error_at_current("expected string literal for test name");
            Spanned::new("<error>".to_string(), start.clone())
        };

        self.expect(TokenKind::Equals, "expected '=' before test body");

        let body = self.parse_block_expr();
        let span = start.merge(&body.span());

        TestDecl { attributes, name, body, span }
    }

    fn parse_function_decl(&mut self, visibility: Visibility, is_async: bool, doc_comment: Option<String>) -> FunctionDecl {
        let start = self.peek().span.clone();
        self.advance(); // consume 'function'

        let name = self
            .expect_ident("expected function name")
            .unwrap_or_else(|| Spanned::new("<error>".to_string(), start.clone()));

        // Optional type parameters: <T, U>
        let type_params = if self.at(TokenKind::Lt) {
            self.parse_type_param_list()
        } else {
            vec![]
        };

        self.expect(TokenKind::LParen, "expected '(' after function name");
        let params = self.parse_param_list();
        self.expect(TokenKind::RParen, "expected ')' after parameters");

        let return_type = if self.at(TokenKind::Colon) {
            self.advance();
            Some(self.parse_type_expr())
        } else {
            None
        };

        let where_clause = self.parse_where_clause();

        self.expect(TokenKind::Equals, "expected '=' before function body");

        let body = if self.at(TokenKind::Intrinsic) {
            let intrinsic_span = self.peek().span.clone();
            self.advance();
            Expr::Intrinsic(intrinsic_span)
        } else if self.at(TokenKind::Begin) && self.peek_at(1).kind == TokenKind::Intrinsic {
            self.advance(); // consume Begin
            let intrinsic_span = self.peek().span.clone();
            self.advance(); // consume Intrinsic
            self.expect(TokenKind::End, "expected end of intrinsic body");
            Expr::Intrinsic(intrinsic_span)
        } else {
            self.parse_block_expr()
        };

        let span = start.merge(&body.span());

        FunctionDecl {
            visibility,
            is_async,
            is_override: false,
            is_final: false,
            is_abstract: false,
            name,
            type_params,
            params,
            return_type,
            where_clause,
            body,
            doc_comment,
            span,
        }
    }

    /// Parse an abstract method declaration: `abstract function name(params): ReturnType` (no body).
    fn parse_abstract_method_decl(&mut self, self_type_expr: &TypeExpr, visibility: Visibility, doc_comment: Option<String>) -> FunctionDecl {
        let start = self.peek().span.clone();
        self.advance(); // consume 'function'

        let name = self
            .expect_ident("expected function name")
            .unwrap_or_else(|| Spanned::new("<error>".to_string(), start.clone()));

        // Optional type parameters: <T, U>
        let type_params = if self.at(TokenKind::Lt) {
            self.parse_type_param_list()
        } else {
            vec![]
        };

        self.expect(TokenKind::LParen, "expected '(' after function name");
        let params = self.parse_extension_param_list(self_type_expr);
        self.expect(TokenKind::RParen, "expected ')' after parameters");

        let return_type = if self.at(TokenKind::Colon) {
            self.advance();
            Some(self.parse_type_expr())
        } else {
            None
        };

        let where_clause = self.parse_where_clause();

        let span = if let Some(ref rt) = return_type {
            start.merge(&rt.span())
        } else {
            start.merge(&name.span)
        };

        // No body for abstract methods — use UnitLiteral as dummy
        FunctionDecl {
            visibility,
            is_async: false,
            is_override: false,
            is_final: false,
            is_abstract: true,
            name,
            type_params,
            params,
            return_type,
            where_clause,
            body: Expr::UnitLiteral(span.clone()),
            doc_comment,
            span,
        }
    }

    fn parse_global_var_decl(&mut self, visibility: Visibility, doc_comment: Option<String>) -> GlobalVarDecl {
        let start = self.peek().span.clone();
        self.advance(); // consume 'let'

        let mutable = if self.at(TokenKind::Mutable) {
            self.advance();
            true
        } else {
            false
        };

        let name = self
            .expect_ident("expected variable name")
            .unwrap_or_else(|| Spanned::new("<error>".to_string(), start.clone()));

        let type_annotation = if self.at(TokenKind::Colon) {
            self.advance();
            Some(self.parse_type_expr())
        } else {
            None
        };

        self.expect(TokenKind::Equals, "expected '=' after variable name");

        let value = self.parse_block_expr();
        let span = start.merge(&value.span());

        GlobalVarDecl {
            visibility,
            name,
            mutable,
            type_annotation,
            value,
            doc_comment,
            span,
        }
    }

    fn parse_newtype_decl(
        &mut self,
        visibility: Visibility,
        doc_comment: Option<String>,
        attributes: Vec<DeriveAttribute>,
    ) -> NewtypeDecl {
        let start = self.peek().span.clone();
        self.advance(); // consume 'newtype'

        let name = self
            .expect_ident("expected newtype name")
            .unwrap_or_else(|| Spanned::new("<error>".to_string(), start.clone()));

        // Optional type parameters with variance: <out T, in U>
        let type_params = if self.at(TokenKind::Lt) {
            self.parse_variant_type_param_list()
        } else {
            vec![]
        };

        let inner_private = if self.at(TokenKind::Private) {
            self.advance();
            true
        } else {
            false
        };

        // Optional where clause: `where T: Trait`
        let where_clause = self.parse_where_clause();

        self.expect(TokenKind::Equals, "expected '=' after newtype name");

        if self.at(TokenKind::Private) {
            self.error_at_current(
                "place 'private' after the type name and type parameters, before any 'where' clause and '='",
            );
            self.advance();
        }

        let inner_type = self.parse_type_expr();
        let span = start.merge(&inner_type.span());

        NewtypeDecl {
            intrinsic: false,
            visibility,
            name,
            type_params,
            where_clause,
            inner_private,
            inner_type,
            doc_comment,
            attributes,
            span,
        }
    }

    fn parse_type_alias_decl(
        &mut self,
        visibility: Visibility,
        doc_comment: Option<String>,
        string_literal: Option<Span>,
    ) -> Declaration {
        let start = self.peek().span.clone();
        self.advance(); // consume 'type'

        let name = self
            .expect_ident("expected type alias name")
            .unwrap_or_else(|| Spanned::new("<error>".to_string(), start.clone()));

        // Optional type parameters: <T, U>
        let type_params = if self.at(TokenKind::Lt) {
            self.parse_variant_type_param_list()
        } else {
            vec![]
        };

        // Optional where clause: where T: Trait
        let where_clause = self.parse_where_clause();

        self.expect(TokenKind::Equals, "expected '=' after type alias name");

        if self.at(TokenKind::Intrinsic) {
            let end = self.peek().span.clone();
            self.advance();
            return Declaration::Newtype(NewtypeDecl {
                intrinsic: true,
                visibility,
                name,
                type_params,
                where_clause,
                inner_private: true,
                // The collector supplies the compiler-owned representation.
                inner_type: TypeExpr::Tuple(vec![], end.clone()),
                doc_comment,
                attributes: vec![],
                span: start.merge(&end),
            });
        }
        for param in &type_params {
            if param.variance != crate::common::types::Variance::Invariant {
                self.error_at_current("variance annotations require a nominal or intrinsic type");
            }
        }
        let type_params = type_params.into_iter().map(|param| param.name).collect();
        let type_expr = self.parse_type_expr();
        let span = start.merge(&type_expr.span());

        Declaration::TypeAlias(TypeAliasDecl {
            visibility,
            string_literal,
            name,
            type_params,
            where_clause,
            type_expr,
            doc_comment,
            span,
        })
    }

    fn parse_record_decl(
        &mut self,
        visibility: Visibility,
        doc_comment: Option<String>,
        attributes: Vec<DeriveAttribute>,
        string_literal: Option<Span>,
    ) -> RecordDecl {
        let start = self.peek().span.clone();
        self.advance(); // consume 'record'

        let name = self
            .expect_ident("expected record name")
            .unwrap_or_else(|| Spanned::new("<error>".to_string(), start.clone()));

        // Optional type parameters with variance: <out T, in U>
        let type_params = if self.at(TokenKind::Lt) {
            self.parse_variant_type_param_list()
        } else {
            vec![]
        };

        // Empty records have no terminator, and top-level layout removes newlines.
        // A later declaration's leading `private` must not become our modifier.
        let private_continues_header = self.peek().span.line == self.tokens[self.pos - 1].span.end_line
            || matches!(self.peek_at(1).kind, TokenKind::Equals | TokenKind::Where | TokenKind::Eof);
        let construction_private = if self.at(TokenKind::Private) && private_continues_header {
            self.advance();
            true
        } else {
            false
        };

        // Optional where clause: `where T: Trait`
        let where_clause = self.parse_where_clause();

        let header_span = start.merge(&self.tokens[self.pos - 1].span);

        // Empty records (no `=`): `record Foo`
        let fields = if self.at(TokenKind::Equals) {
            self.advance(); // consume '='
            self.parse_record_fields()
        } else {
            vec![]
        };

        let span = if let Some(last) = fields.last() {
            start.merge(&last.span)
        } else {
            header_span
        };

        RecordDecl {
            visibility,
            construction_private,
            string_literal,
            name,
            type_params,
            fields,
            where_clause,
            doc_comment,
            attributes,
            span,
        }
    }

    fn parse_trait_decl(
        &mut self,
        visibility: Visibility,
        doc_comment: Option<String>,
        is_interface: bool,
    ) -> TraitDecl {
        let start = self.peek().span.clone();
        self.advance(); // consume 'trait' or 'interface'

        let name = self
            .expect_ident(if is_interface {
                "expected interface name"
            } else {
                "expected trait name"
            })
            .unwrap_or_else(|| Spanned::new("<error>".to_string(), start.clone()));

        // Optional type parameters: <T, U>
        let type_params = if self.at(TokenKind::Lt) {
            self.parse_type_param_list()
        } else {
            vec![]
        };

        // Optional `extends` clause: `trait B extends A and C<T>` — supers are
        // named types, `and`-separated (the same shape as class `implements`).
        let mut supers: Vec<NamedType> = Vec::new();
        if self.at(TokenKind::Extends) {
            self.advance(); // consume 'extends'
            loop {
                supers.push(self.parse_named_type());
                if self.at(TokenKind::And) {
                    self.advance();
                } else {
                    break;
                }
            }
        }

        // Optional `=` — if absent, this is an empty (marker) trait
        let (methods, properties, associated_types) = if self.at(TokenKind::Equals) {
            self.advance(); // consume '='
            self.parse_trait_body(&name)
        } else {
            (vec![], vec![], vec![])
        };

        let span = if let Some(last) = associated_types.last() {
            start.merge(&last.span)
        } else if let Some(last) = properties.last() {
            start.merge(&last.span)
        } else if let Some(last) = methods.last() {
            start.merge(&last.span)
        } else {
            start.merge(&name.span)
        };

        TraitDecl {
            visibility,
            name,
            type_params,
            supers,
            methods,
            properties,
            associated_types,
            is_interface,
            doc_comment,
            span,
        }
    }

    fn parse_trait_body(
        &mut self,
        trait_name: &Spanned<String>,
    ) -> (
        Vec<TraitMethodSignature>,
        Vec<PropertyDecl>,
        Vec<AssociatedTypeDecl>,
    ) {
        let mut methods = Vec::new();
        let mut properties = Vec::new();
        let mut associated_types = Vec::new();

        // Build a Self type expr for property params
        let self_type_expr = TypeExpr::Named(NamedType {
            name: Spanned::new("Self".to_string(), trait_name.span.clone()),
            type_args: vec![],
            span: trait_name.span.clone(),
        });

        if self.at(TokenKind::Begin) {
            self.advance(); // consume Begin

            self.parse_trait_member(
                &self_type_expr,
                &mut methods,
                &mut properties,
                &mut associated_types,
            );

            while self.at(TokenKind::Sep) {
                self.advance(); // consume Sep
                self.parse_trait_member(
                    &self_type_expr,
                    &mut methods,
                    &mut properties,
                    &mut associated_types,
                );
            }

            self.expect(TokenKind::End, "expected end of trait body");
        } else {
            self.parse_trait_member(
                &self_type_expr,
                &mut methods,
                &mut properties,
                &mut associated_types,
            );
        }

        (methods, properties, associated_types)
    }

    fn parse_trait_member(
        &mut self,
        self_type_expr: &TypeExpr,
        methods: &mut Vec<TraitMethodSignature>,
        properties: &mut Vec<PropertyDecl>,
        associated_types: &mut Vec<AssociatedTypeDecl>,
    ) {
        let doc_comment = self.peek().doc_comment.clone();
        if self.at(TokenKind::Function) {
            methods.push(self.parse_trait_method_signature(self_type_expr, doc_comment));
        } else if self.at(TokenKind::Property) {
            properties.push(self.parse_property_decl(self_type_expr, Visibility::Internal, PropertyBodyMode::Optional, doc_comment));
        } else if self.at(TokenKind::Type) {
            associated_types.push(self.parse_associated_type_decl(doc_comment));
        } else {
            self.error_at_current("expected 'function', 'property', or 'type' in trait body");
        }
    }

    fn parse_associated_type_decl(&mut self, doc_comment: Option<String>) -> AssociatedTypeDecl {
        let start = self.peek().span.clone();
        self.advance(); // consume 'type'

        let name = self
            .expect_ident("expected associated type name")
            .unwrap_or_else(|| Spanned::new("<error>".to_string(), start.clone()));

        // Optional type parameters: <T, U>
        let type_params = if self.at(TokenKind::Lt) {
            self.parse_type_param_list()
        } else {
            vec![]
        };

        let span = if let Some(last_tp) = type_params.last() {
            start.merge(&last_tp.span)
        } else {
            start.merge(&name.span)
        };

        AssociatedTypeDecl {
            name,
            type_params,
            doc_comment,
            span,
        }
    }

    fn parse_trait_method_signature(&mut self, self_type_expr: &TypeExpr, doc_comment: Option<String>) -> TraitMethodSignature {
        let start = self.peek().span.clone();
        self.expect(TokenKind::Function, "expected 'function' in trait body");

        let name = self
            .expect_ident("expected method name")
            .unwrap_or_else(|| Spanned::new("<error>".to_string(), start.clone()));

        // Optional type parameters: <T, U>
        let type_params = if self.at(TokenKind::Lt) {
            self.parse_type_param_list()
        } else {
            vec![]
        };

        self.expect(TokenKind::LParen, "expected '(' after method name");
        let params = self.parse_extension_param_list(self_type_expr);
        self.expect(TokenKind::RParen, "expected ')' after parameters");

        let return_type = if self.at(TokenKind::Colon) {
            self.advance();
            Some(self.parse_type_expr())
        } else {
            None
        };

        let where_clause = self.parse_where_clause();

        // Optional default body: `function bar(self): Int32 = 42`
        // (trait-design-appendix §4). Intrinsic-aware like property bodies.
        let body = if self.at(TokenKind::Equals) {
            self.advance(); // consume '='
            let expr = if self.at(TokenKind::Intrinsic) {
                let intrinsic_span = self.peek().span.clone();
                self.advance();
                Expr::Intrinsic(intrinsic_span)
            } else if self.at(TokenKind::Begin) && self.peek_at(1).kind == TokenKind::Intrinsic {
                self.advance(); // consume Begin
                let intrinsic_span = self.peek().span.clone();
                self.advance(); // consume Intrinsic
                self.expect(TokenKind::End, "expected end of intrinsic body");
                Expr::Intrinsic(intrinsic_span)
            } else {
                self.parse_block_expr()
            };
            Some(expr)
        } else {
            None
        };

        let span = if let Some(ref b) = body {
            start.merge(&b.span())
        } else if !where_clause.is_empty() {
            start.merge(&where_clause.last().unwrap().span)
        } else if let Some(ref rt) = return_type {
            start.merge(&rt.span())
        } else {
            start.merge(&name.span)
        };

        TraitMethodSignature {
            name,
            type_params,
            params,
            return_type,
            where_clause,
            body,
            doc_comment,
            span,
        }
    }

    /// Parse a where clause: `where T: Trait1 + Trait2, U: Trait3`
    /// Returns an empty vec if no `where` keyword is present.
    fn parse_where_clause(&mut self) -> Vec<TraitConstraint> {
        if !self.at(TokenKind::Where) {
            return vec![];
        }
        self.advance(); // consume 'where'

        let mut constraints = Vec::new();
        constraints.push(self.parse_trait_constraint());

        while self.at(TokenKind::Comma) {
            self.advance(); // consume ','
            constraints.push(self.parse_trait_constraint());
        }

        constraints
    }

    /// Parse a single trait constraint: `T: Trait1 + Trait2` or `T: From<Int32>`
    fn parse_trait_constraint(&mut self) -> TraitConstraint {
        let start = self.peek().span.clone();
        let type_param = self
            .expect_ident("expected type parameter name in where clause")
            .unwrap_or_else(|| Spanned::new("<error>".to_string(), start.clone()));

        self.expect(
            TokenKind::Colon,
            "expected ':' after type parameter in where clause",
        );

        let mut trait_bounds = Vec::new();
        trait_bounds.push(self.parse_type_bound());

        while self.at(TokenKind::Plus) || self.at(TokenKind::And) {
            if self.at(TokenKind::And) {
                // `and` combines interface-object types, not bounds — the
                // natural mistake gets a targeted hint, then we recover by
                // treating it as '+'.
                self.error_at_current(
                    "bounds combine with '+' (e.g. `T: Alpha + Beta`); 'and' forms an intersection type",
                );
            }
            self.advance(); // consume '+' or 'and'
            trait_bounds.push(self.parse_type_bound());
        }

        let span = start.merge(trait_bounds.last().unwrap().span());

        TraitConstraint {
            type_param,
            trait_bounds,
            span,
        }
    }

    fn parse_type_bound(&mut self) -> TypeBound {
        if self.at(TokenKind::Class) {
            TypeBound::Class(self.advance().span.clone())
        } else {
            TypeBound::Named(self.parse_named_trait_bound())
        }
    }

    fn parse_named_trait_bound(&mut self) -> NamedTraitBound {
        let name = self
            .expect_ident("expected trait name")
            .unwrap_or_else(|| Spanned::new("<error>".to_string(), self.peek().span.clone()));
        let mut type_args = Vec::new();
        let mut associated_types = Vec::new();
        if self.at(TokenKind::Lt) {
            self.advance();
            loop {
                if self.at(TokenKind::Ident) && self.peek_at(1).kind == TokenKind::Equals {
                    let binding = self.expect_ident("expected associated type name").unwrap();
                    self.advance();
                    associated_types.push((binding, self.parse_type_expr()));
                } else {
                    if !associated_types.is_empty() {
                        self.error_at_current(
                            "positional type arguments must precede associated type bindings",
                        );
                    }
                    type_args.push(self.parse_type_expr());
                }
                if !self.at(TokenKind::Comma) {
                    break;
                }
                self.advance();
            }
            self.expect_gt("expected '>' after trait bound arguments");
        }
        let span = name.span.merge(&self.peek().span);
        NamedTraitBound {
            name,
            type_args,
            associated_types,
            span,
        }
    }

    fn parse_extension_decl(&mut self, doc_comment: Option<String>) -> ExtensionDecl {
        let start = self.peek().span.clone();
        self.advance(); // consume 'extension'

        let name = self
            .expect_ident("expected extension name")
            .unwrap_or_else(|| Spanned::new("<error>".to_string(), start.clone()));

        // Parse type parameters if present: `<T>` or `<T, U>`
        let type_params = if self.at(TokenKind::Lt) {
            self.parse_type_param_list()
        } else {
            vec![]
        };

        self.expect(TokenKind::For, "expected 'for' after 'extension'");
        let for_type = self.parse_type_expr();

        let where_clause = self.parse_where_clause();

        self.expect(TokenKind::Equals, "expected '=' before extension body");

        let (methods, properties) = self.parse_extension_body(&for_type);

        let end_span = methods
            .last()
            .map(|m| m.span.clone())
            .or_else(|| properties.last().map(|p| p.span.clone()))
            .unwrap_or_else(|| start.clone());
        let span = start.merge(&end_span);

        ExtensionDecl {
            name,
            type_params,
            for_type,
            where_clause,
            methods,
            properties,
            doc_comment,
            span,
        }
    }

    fn parse_implement_decl(&mut self, doc_comment: Option<String>) -> ImplementDecl {
        let start = self.peek().span.clone();
        self.advance(); // consume 'implement'

        // Optional block-level type parameters (Phase 6): implement <T> TraitName ...
        let type_params = if self.at(TokenKind::Lt) {
            self.parse_type_param_list()
        } else {
            vec![]
        };

        let trait_name = self
            .expect_ident("expected trait name after 'implement'")
            .unwrap_or_else(|| Spanned::new("<error>".to_string(), start.clone()));

        // Optional trait type arguments: implement From<Int32> for ...
        let trait_type_args = if self.at(TokenKind::Lt) {
            self.parse_type_arg_list()
        } else {
            vec![]
        };

        self.expect(TokenKind::For, "expected 'for' after trait name");
        let for_type = self.parse_type_expr();

        let where_clause = self.parse_where_clause();

        // Optional `=` — if absent, this is an empty implement block
        let (methods, properties, associated_types) = if self.at(TokenKind::Equals) {
            self.advance(); // consume '='
            self.parse_implement_body(&for_type)
        } else {
            (vec![], vec![], vec![])
        };

        let end_span = methods
            .last()
            .map(|m| m.span.clone())
            .or_else(|| properties.last().map(|p| p.span.clone()))
            .or_else(|| associated_types.last().map(|a| a.span.clone()))
            .unwrap_or_else(|| start.clone());
        let span = start.merge(&end_span);

        ImplementDecl {
            trait_name,
            type_params,
            trait_type_args,
            for_type,
            where_clause,
            methods,
            properties,
            associated_types,
            doc_comment,
            span,
        }
    }

    fn parse_implement_body(
        &mut self,
        for_type: &TypeExpr,
    ) -> (Vec<FunctionDecl>, Vec<PropertyDecl>, Vec<AssociatedTypeDef>) {
        let mut methods = Vec::new();
        let mut properties = Vec::new();
        let mut associated_types = Vec::new();

        if self.at(TokenKind::Begin) {
            self.advance(); // consume Begin

            self.parse_implement_member(
                for_type,
                &mut methods,
                &mut properties,
                &mut associated_types,
            );

            while self.at(TokenKind::Sep) {
                self.advance(); // consume Sep
                self.parse_implement_member(
                    for_type,
                    &mut methods,
                    &mut properties,
                    &mut associated_types,
                );
            }

            self.expect(TokenKind::End, "expected end of implement body");
        } else {
            self.parse_implement_member(
                for_type,
                &mut methods,
                &mut properties,
                &mut associated_types,
            );
        }

        (methods, properties, associated_types)
    }

    fn parse_implement_member(
        &mut self,
        for_type: &TypeExpr,
        methods: &mut Vec<FunctionDecl>,
        properties: &mut Vec<PropertyDecl>,
        associated_types: &mut Vec<AssociatedTypeDef>,
    ) {
        let doc_comment = self.peek().doc_comment.clone();

        let visibility = match self.peek().kind {
            TokenKind::Public => {
                self.advance();
                Visibility::Public
            }
            TokenKind::Internal => {
                self.advance();
                Visibility::Internal
            }
            TokenKind::Private => {
                self.advance();
                Visibility::Private
            }
            _ => Visibility::Internal,
        };

        let is_async = if self.at(TokenKind::Async) && self.peek_at(1).kind == TokenKind::Function {
            self.advance(); // consume 'async'
            true
        } else {
            false
        };

        if self.at(TokenKind::Function) {
            let mut func = self.parse_extension_method_with_visibility(for_type, visibility, doc_comment);
            func.is_async = is_async;
            methods.push(func);
        } else if self.at(TokenKind::Property) {
            properties.push(self.parse_property_decl(for_type, visibility, PropertyBodyMode::Required, doc_comment));
        } else if self.at(TokenKind::Type) {
            associated_types.push(self.parse_associated_type_def());
        } else {
            self.error_at_current("expected 'function', 'property', or 'type' in implement body");
        }
    }

    fn parse_associated_type_def(&mut self) -> AssociatedTypeDef {
        let start = self.peek().span.clone();
        self.advance(); // consume 'type'

        let name = self
            .expect_ident("expected associated type name")
            .unwrap_or_else(|| Spanned::new("<error>".to_string(), start.clone()));

        // Optional type parameters: <T, U>
        let type_params = if self.at(TokenKind::Lt) {
            self.parse_type_param_list()
        } else {
            vec![]
        };

        self.expect(TokenKind::Equals, "expected '=' after associated type name");

        let type_expr = self.parse_type_expr();

        let span = start.merge(&type_expr.span());

        AssociatedTypeDef {
            name,
            type_params,
            type_expr,
            span,
        }
    }

    fn parse_module_decl(&mut self, doc_comment: Option<String>) -> ModuleDecl {
        let start = self.peek().span.clone();
        self.advance(); // consume 'module'

        let name = self
            .expect_ident("expected module name")
            .unwrap_or_else(|| Spanned::new("<error>".to_string(), start.clone()));

        // Optional type parameters: <T, U>
        let type_params = if self.at(TokenKind::Lt) {
            self.parse_type_param_list()
        } else {
            vec![]
        };

        // Construct a TypeExpr representing the module's type (for bare `self` support)
        let for_type = TypeExpr::Named(NamedType {
            name: name.clone(),
            type_args: type_params
                .iter()
                .map(|tp| {
                    TypeExpr::Named(NamedType {
                        name: tp.clone(),
                        type_args: vec![],
                        span: tp.span.clone(),
                    })
                })
                .collect(),
            span: name.span.clone(),
        });

        self.expect(TokenKind::Equals, "expected '=' before module body");

        let (functions, properties, globals, tests) = self.parse_module_body(&for_type);

        let end_span = functions
            .last()
            .map(|f| f.span.clone())
            .or_else(|| properties.last().map(|p| p.span.clone()))
            .or_else(|| globals.last().map(|g| g.span.clone()))
            .or_else(|| tests.last().map(|t| t.span.clone()))
            .unwrap_or_else(|| start.clone());
        let span = start.merge(&end_span);

        ModuleDecl {
            name,
            type_params,
            functions,
            properties,
            globals,
            tests,
            doc_comment,
            span,
        }
    }

    fn parse_module_body(
        &mut self,
        for_type: &TypeExpr,
    ) -> (Vec<FunctionDecl>, Vec<PropertyDecl>, Vec<GlobalVarDecl>, Vec<TestDecl>) {
        let mut functions = Vec::new();
        let mut properties = Vec::new();
        let mut globals = Vec::new();
        let mut tests = Vec::new();

        if self.at(TokenKind::Begin) {
            self.advance(); // consume Begin

            self.parse_module_member(for_type, &mut functions, &mut properties, &mut globals, &mut tests);

            while self.at(TokenKind::Sep) {
                self.advance(); // consume Sep
                self.parse_module_member(for_type, &mut functions, &mut properties, &mut globals, &mut tests);
            }

            self.expect(TokenKind::End, "expected end of module body");
        } else {
            self.parse_module_member(for_type, &mut functions, &mut properties, &mut globals, &mut tests);
        }

        (functions, properties, globals, tests)
    }

    fn parse_module_member(
        &mut self,
        for_type: &TypeExpr,
        functions: &mut Vec<FunctionDecl>,
        properties: &mut Vec<PropertyDecl>,
        globals: &mut Vec<GlobalVarDecl>,
        tests: &mut Vec<TestDecl>,
    ) {
        // Check for test declarations before visibility parsing
        if self.at(TokenKind::At) {
            let attributes = self.parse_test_attributes();
            // An attribute sits on its own line, so layout puts a member
            // separator between it and the `test` it decorates. Members of a
            // module body are separated by `Sep`, unlike top-level and class
            // declarations, so skip them here or every attributed test in a
            // module body is a parse error.
            while self.at(TokenKind::Sep) {
                self.advance();
            }
            if self.peek().kind == TokenKind::Ident && self.peek().text == "test" && self.peek_at(1).kind == TokenKind::StringLiteral {
                tests.push(self.parse_test_decl(attributes));
            } else {
                self.error_at_current("expected 'test' declaration after attributes");
            }
            return;
        }
        if self.peek().kind == TokenKind::Ident && self.peek().text == "test" && self.peek_at(1).kind == TokenKind::StringLiteral {
            tests.push(self.parse_test_decl(vec![]));
            return;
        }

        let doc_comment = self.peek().doc_comment.clone();

        let visibility = match self.peek().kind {
            TokenKind::Public => {
                self.advance();
                Visibility::Public
            }
            TokenKind::Internal => {
                self.advance();
                Visibility::Internal
            }
            TokenKind::Private => {
                self.advance();
                Visibility::Private
            }
            _ => Visibility::Internal,
        };

        let is_async = if self.at(TokenKind::Async) && self.peek_at(1).kind == TokenKind::Function {
            self.advance(); // consume 'async'
            true
        } else {
            false
        };

        if self.at(TokenKind::Function) {
            let mut func = self.parse_extension_method_with_visibility(for_type, visibility, doc_comment);
            func.is_async = is_async;
            functions.push(func);
        } else if self.at(TokenKind::Property) {
            properties.push(self.parse_property_decl(for_type, visibility, PropertyBodyMode::Required, doc_comment));
        } else if self.at(TokenKind::Let) {
            // Check if this is `let property ...` (contextual keyword) or a regular `let`
            if self.peek_at(1).kind == TokenKind::Property {
                properties.push(self.parse_module_property(visibility, doc_comment));
            } else {
                globals.push(self.parse_global_var_decl(visibility, doc_comment));
            }
        } else {
            self.error_at_current(
                "expected 'function', 'property', 'let property', 'let', or 'test' in module body",
            );
        }
    }

    fn parse_module_property(&mut self, visibility: Visibility, doc_comment: Option<String>) -> PropertyDecl {
        let start = self.peek().span.clone();
        self.advance(); // consume 'let'

        // Consume 'property' keyword
        if self.at(TokenKind::Property) {
            self.advance(); // consume 'property'
        }

        let name = self
            .expect_ident("expected property name")
            .unwrap_or_else(|| Spanned::new("<error>".to_string(), start.clone()));

        self.expect(TokenKind::Colon, "expected ':' after property name");
        let return_type = self.parse_type_expr();

        self.expect(TokenKind::Equals, "expected '=' before property body");

        let body = self.parse_block_expr();
        let span = start.merge(&body.span());

        PropertyDecl {
            visibility,
            is_override: false,
            is_final: false,
            is_abstract: false,
            name,
            type_params: vec![],
            params: vec![],
            return_type,
            body: Some(body),
            doc_comment,
            span,
        }
    }

    // ── Class declarations ─────────────────────────────────────────

    fn parse_class_decl(
        &mut self,
        visibility: Visibility,
        is_final: bool,
        is_abstract: bool,
        is_sealed: bool,
        doc_comment: Option<String>,
        string_literal: Option<Span>,
    ) -> ClassDecl {
        let start = self.peek().span.clone();
        self.advance(); // consume 'class'

        let name = self
            .expect_ident("expected class name")
            .unwrap_or_else(|| Spanned::new("<error>".to_string(), start.clone()));

        // Optional type parameters: `class Box<out T>(...)`
        let type_params = if self.at(TokenKind::Lt) {
            self.parse_variant_type_param_list()
        } else {
            vec![]
        };

        // Optional constructor visibility before `(`
        let constructor_visibility = if self.at(TokenKind::LParen) {
            Visibility::Public
        } else {
            self.parse_class_member_visibility_or_default(Visibility::Public)
        };

        // Optional constructor params `(...)`
        let params = if self.at(TokenKind::LParen) {
            self.advance(); // consume '('
            let params = self.parse_constructor_param_list();
            self.expect(
                TokenKind::RParen,
                "expected ')' after constructor parameters",
            );
            params
        } else {
            vec![]
        };

        // Optional extends clause: `extends ParentType(args...)`
        let extends = if self.at(TokenKind::Extends) {
            let ext_start = self.peek().span.clone();
            self.advance(); // consume 'extends'
            let parent_type = self.parse_type_expr();
            self.expect(TokenKind::LParen, "expected '(' after parent type in extends clause");
            let super_args = self.parse_arg_list();
            let rparen_span = self.peek().span.clone();
            self.expect(TokenKind::RParen, "expected ')' after super constructor args");
            let ext_span = ext_start.merge(&rparen_span);
            Some(ClassExtends {
                parent_type,
                super_args,
                span: ext_span,
            })
        } else {
            None
        };

        // Optional implements clause: `implements Trait1 and Trait2`
        // Parse each trait as a NamedType (not full type_expr, to avoid `and` being
        // consumed as an intersection type).
        let implements = if self.at(TokenKind::Implements) {
            self.advance(); // consume 'implements'
            let first = self.parse_named_type();
            let mut traits = vec![TypeExpr::Named(first)];
            while self.at(TokenKind::And) {
                self.advance(); // consume 'and'
                let next = self.parse_named_type();
                traits.push(TypeExpr::Named(next));
            }
            traits
        } else {
            vec![]
        };

        // Optional where clause: `where T: Display`
        let where_clause = self.parse_where_clause();

        // Check for misplaced `implements` after `where` — common ordering mistake
        if self.at(TokenKind::Implements) {
            self.diagnostics.push(crate::common::diagnostics::Diagnostic {
                severity: crate::common::diagnostics::Severity::Error,
                span: self.peek().span.clone(),
                message: "'implements' clause must appear before 'where' clause".to_string(),
                tag: None,
            });
        }

        // Optional body after `=`
        let body = if self.at(TokenKind::Equals) {
            self.advance(); // consume '='
            self.parse_class_body(&name, &type_params)
        } else {
            vec![]
        };

        let span = if let Some(last) = body.last() {
            let last_span = match last {
                ClassMember::LetBinding(lb) => &lb.span,
                ClassMember::Method(f) => &f.span,
                ClassMember::Property(p) => &p.span,
                ClassMember::Expression(e) => &e.span(),
            };
            start.merge(last_span)
        } else if let Some(last) = params.last() {
            start.merge(&last.span)
        } else {
            start.merge(&name.span)
        };

        ClassDecl {
            visibility,
            string_literal,
            is_final,
            is_abstract,
            is_sealed,
            name,
            type_params,
            constructor_visibility,
            params,
            extends,
            implements,
            where_clause,
            body,
            doc_comment,
            span,
        }
    }

    fn parse_constructor_param_list(&mut self) -> Vec<ConstructorParam> {
        let mut params = Vec::new();
        if self.at(TokenKind::RParen) {
            return params;
        }
        params.push(self.parse_constructor_param());
        while self.at(TokenKind::Comma) {
            self.advance(); // consume ','
            params.push(self.parse_constructor_param());
        }
        params
    }

    fn parse_constructor_param(&mut self) -> ConstructorParam {
        let doc_comment = self.peek().doc_comment.clone();
        let start = self.peek().span.clone();

        let visibility = self.parse_class_member_visibility_or_default(Visibility::Private);

        let mutable = if self.at(TokenKind::Mutable) {
            self.advance();
            true
        } else {
            false
        };

        let name = self
            .expect_ident("expected parameter name")
            .unwrap_or_else(|| Spanned::new("<error>".to_string(), start.clone()));

        self.expect(TokenKind::Colon, "expected ':' after parameter name");
        let type_annotation = self.parse_type_expr();

        let default_value = if self.at(TokenKind::Equals) {
            self.advance(); // consume '='
            Some(self.parse_value_expr())
            // parse_value_expr for same-line simple expression
        } else {
            None
        };

        let end = default_value
            .as_ref()
            .map(|e| e.span())
            .unwrap_or_else(|| type_annotation.span());
        let span = start.merge(&end);

        ConstructorParam {
            visibility,
            mutable,
            name,
            type_annotation,
            default_value,
            doc_comment,
            span,
        }
    }

    fn parse_class_body(&mut self, class_name: &Spanned<String>, type_params: &[VariantTypeParam]) -> Vec<ClassMember> {
        let mut members = Vec::new();

        // Build a self type expr for property parsing (includes type params for generic classes)
        let type_args: Vec<TypeExpr> = type_params
            .iter()
            .map(|tp| {
                TypeExpr::Named(NamedType {
                    name: tp.name.clone(),
                    type_args: vec![],
                    span: tp.name.span.clone(),
                })
            })
            .collect();
        let self_type_expr = TypeExpr::Named(NamedType {
            name: class_name.clone(),
            type_args,
            span: class_name.span.clone(),
        });

        if self.at(TokenKind::Begin) {
            self.advance(); // consume Begin

            members.push(self.parse_class_member(&self_type_expr));

            while self.at(TokenKind::Sep) {
                self.advance(); // consume Sep
                members.push(self.parse_class_member(&self_type_expr));
            }

            self.expect(TokenKind::End, "expected end of class body");
        } else {
            members.push(self.parse_class_member(&self_type_expr));
        }

        members
    }

    fn parse_class_member(&mut self, self_type_expr: &TypeExpr) -> ClassMember {
        let doc_comment = self.peek().doc_comment.clone();

        // Check for visibility prefix
        let visibility = self.parse_class_member_visibility_or_default(Visibility::Private);

        // Check for async/abstract/override/final modifiers (in any order) before
        // function/property. They're distinct keywords, so consuming them in a loop
        // lets combinations like `override async function` or `abstract async function`
        // parse regardless of the order they're written.
        let mut is_async = false;
        let mut is_abstract = false;
        let mut is_override = false;
        let mut is_final_method = false;
        loop {
            if self.at(TokenKind::Async) {
                self.advance();
                is_async = true;
            } else if self.at(TokenKind::Abstract) {
                self.advance();
                is_abstract = true;
            } else if self.at(TokenKind::Override) {
                self.advance();
                is_override = true;
            } else if self.at(TokenKind::Final) {
                self.advance();
                is_final_method = true;
            } else {
                break;
            }
        }

        // function ... (all methods use parse_function_decl; instance methods declare self: ClassName explicitly)
        if self.at(TokenKind::Function) {
            if is_abstract {
                let mut func = self.parse_abstract_method_decl(self_type_expr, visibility, doc_comment);
                func.is_async = is_async;
                func.is_override = is_override;
                func.is_final = is_final_method;
                return ClassMember::Method(func);
            }
            let mut func = self.parse_extension_method_with_visibility(self_type_expr, visibility, doc_comment);
            func.is_async = is_async;
            func.is_override = is_override;
            func.is_final = is_final_method;
            return ClassMember::Method(func);
        }

        // property ...
        if self.at(TokenKind::Property) {
            let body_mode = if is_abstract {
                PropertyBodyMode::Forbidden
            } else {
                PropertyBodyMode::Required
            };
            let mut prop = self.parse_property_decl(self_type_expr, visibility, body_mode, doc_comment);
            prop.is_override = is_override;
            prop.is_final = is_final_method;
            prop.is_abstract = is_abstract;
            return ClassMember::Property(prop);
        }

        // let binding
        if self.at(TokenKind::Let) {
            return ClassMember::LetBinding(self.parse_class_let_binding(visibility, doc_comment));
        }

        // Expression (constructor body)
        ClassMember::Expression(self.parse_expression())
    }

    fn parse_class_let_binding(&mut self, visibility: Visibility, doc_comment: Option<String>) -> ClassLetBinding {
        let start = self.peek().span.clone();
        self.advance(); // consume 'let'

        let is_static = if self.at(TokenKind::Static) {
            self.advance();
            true
        } else {
            false
        };

        let mutable = if self.at(TokenKind::Mutable) {
            self.advance();
            true
        } else {
            false
        };

        let name = self
            .expect_ident("expected variable name")
            .unwrap_or_else(|| Spanned::new("<error>".to_string(), start.clone()));

        let type_annotation = if self.at(TokenKind::Colon) {
            self.advance();
            Some(self.parse_type_expr())
        } else {
            None
        };

        self.expect(TokenKind::Equals, "expected '=' after variable name");

        let value = self.parse_block_expr();
        let span = start.merge(&value.span());

        ClassLetBinding {
            visibility,
            is_static,
            mutable,
            name,
            type_annotation,
            value,
            doc_comment,
            span,
        }
    }

    /// Parse an optional class member visibility keyword.
    /// Returns the given default if no visibility keyword is found.
    fn parse_class_member_visibility_or_default(&mut self, default: Visibility) -> Visibility {
        match self.peek().kind {
            TokenKind::Public => {
                self.advance();
                Visibility::Public
            }
            TokenKind::Internal => {
                self.advance();
                Visibility::Internal
            }
            TokenKind::Private => {
                self.advance();
                Visibility::Private
            }
            TokenKind::Protected => {
                self.advance();
                Visibility::Protected
            }
            _ => default,
        }
    }

    fn parse_extension_body(
        &mut self,
        for_type: &TypeExpr,
    ) -> (Vec<FunctionDecl>, Vec<PropertyDecl>) {
        let mut methods = Vec::new();
        let mut properties = Vec::new();

        if self.at(TokenKind::Begin) {
            self.advance(); // consume Begin

            self.parse_extension_member(for_type, &mut methods, &mut properties);

            while self.at(TokenKind::Sep) {
                self.advance(); // consume Sep
                self.parse_extension_member(for_type, &mut methods, &mut properties);
            }

            self.expect(TokenKind::End, "expected end of extension body");
        } else {
            self.parse_extension_member(for_type, &mut methods, &mut properties);
        }

        (methods, properties)
    }

    fn parse_extension_member(
        &mut self,
        for_type: &TypeExpr,
        methods: &mut Vec<FunctionDecl>,
        properties: &mut Vec<PropertyDecl>,
    ) {
        let doc_comment = self.peek().doc_comment.clone();

        // Peek past optional visibility to determine if method or property
        let visibility = match self.peek().kind {
            TokenKind::Public => {
                self.advance();
                Visibility::Public
            }
            TokenKind::Internal => {
                self.advance();
                Visibility::Internal
            }
            TokenKind::Private => {
                self.advance();
                Visibility::Private
            }
            _ => Visibility::Internal,
        };

        let is_async = if self.at(TokenKind::Async) && self.peek_at(1).kind == TokenKind::Function {
            self.advance(); // consume 'async'
            true
        } else {
            false
        };

        if self.at(TokenKind::Function) {
            let mut func = self.parse_extension_method_with_visibility(for_type, visibility, doc_comment);
            func.is_async = is_async;
            methods.push(func);
        } else if self.at(TokenKind::Property) {
            properties.push(self.parse_property_decl(for_type, visibility, PropertyBodyMode::Required, doc_comment));
        } else {
            self.error_at_current("expected 'function' or 'property' in extension body");
        }
    }

    fn parse_property_decl(
        &mut self,
        for_type: &TypeExpr,
        visibility: Visibility,
        body_mode: PropertyBodyMode,
        doc_comment: Option<String>,
    ) -> PropertyDecl {
        let start = self.peek().span.clone();
        self.advance(); // consume 'property'

        let name = self
            .expect_ident("expected property name")
            .unwrap_or_else(|| Spanned::new("<error>".to_string(), start.clone()));

        // Optional type params: `property empty<T>(...)`
        let type_params = if self.at(TokenKind::Lt) && self.peek_at(1).kind == TokenKind::Ident {
            self.parse_type_param_list()
        } else {
            vec![]
        };

        // Optional param list: `(self)` for instance property, none for static
        let params = if self.at(TokenKind::LParen) {
            self.advance(); // consume '('
            let params = self.parse_extension_param_list(for_type);
            self.expect(TokenKind::RParen, "expected ')' after property parameters");
            if params.len() > 1 {
                self.error_at_current("property can have at most one parameter (self)");
            }
            params
        } else {
            vec![]
        };

        self.expect(TokenKind::Colon, "expected ':' after property name");
        let return_type = self.parse_type_expr();

        let parse_body = match body_mode {
            PropertyBodyMode::Required => {
                self.expect(TokenKind::Equals, "expected '=' before property body");
                true
            }
            PropertyBodyMode::Optional => {
                if self.at(TokenKind::Equals) {
                    self.advance(); // consume '='
                    true
                } else {
                    false
                }
            }
            PropertyBodyMode::Forbidden => false,
        };
        let body = if parse_body {
            let expr = if self.at(TokenKind::Intrinsic) {
                let intrinsic_span = self.peek().span.clone();
                self.advance();
                Expr::Intrinsic(intrinsic_span)
            } else if self.at(TokenKind::Begin) && self.peek_at(1).kind == TokenKind::Intrinsic {
                self.advance(); // consume Begin
                let intrinsic_span = self.peek().span.clone();
                self.advance(); // consume Intrinsic
                self.expect(TokenKind::End, "expected end of intrinsic body");
                Expr::Intrinsic(intrinsic_span)
            } else {
                self.parse_block_expr()
            };
            Some(expr)
        } else {
            None
        };

        let end_span = body
            .as_ref()
            .map(|b| b.span())
            .unwrap_or_else(|| return_type.span());
        let span = start.merge(&end_span);

        PropertyDecl {
            visibility,
            is_override: false,
            is_final: false,
            is_abstract: false,
            name,
            type_params,
            params,
            return_type,
            body,
            doc_comment,
            span,
        }
    }

    fn parse_extension_method_with_visibility(
        &mut self,
        for_type: &TypeExpr,
        visibility: Visibility,
        doc_comment: Option<String>,
    ) -> FunctionDecl {
        let start = self.peek().span.clone();
        self.expect(TokenKind::Function, "expected 'function' in extension body");

        let name = self
            .expect_ident("expected method name")
            .unwrap_or_else(|| Spanned::new("<error>".to_string(), start.clone()));

        // Optional type parameters: <T, U>
        let type_params = if self.at(TokenKind::Lt) {
            self.parse_type_param_list()
        } else {
            vec![]
        };

        self.expect(TokenKind::LParen, "expected '(' after method name");
        let params = self.parse_extension_param_list(for_type);
        self.expect(TokenKind::RParen, "expected ')' after parameters");

        let return_type = if self.at(TokenKind::Colon) {
            self.advance();
            Some(self.parse_type_expr())
        } else {
            None
        };

        // Optional where clause for generic methods: `where U: Display`
        let where_clause = self.parse_where_clause();

        self.expect(TokenKind::Equals, "expected '=' before method body");

        let body = if self.at(TokenKind::Intrinsic) {
            // Same-line: `= intrinsic`
            let intrinsic_span = self.peek().span.clone();
            self.advance(); // consume Intrinsic
            Expr::Intrinsic(intrinsic_span)
        } else if self.at(TokenKind::Begin) && self.peek_at(1).kind == TokenKind::Intrinsic {
            // Multi-line: `=\n    intrinsic`
            self.advance(); // consume Begin
            let intrinsic_span = self.peek().span.clone();
            self.advance(); // consume Intrinsic
            self.expect(TokenKind::End, "expected end of intrinsic body");
            Expr::Intrinsic(intrinsic_span)
        } else {
            self.parse_block_expr()
        };
        let span = start.merge(&body.span());

        FunctionDecl {
            visibility,
            is_async: false,
            is_override: false,
            is_final: false,
            is_abstract: false,
            name,
            type_params,
            params,
            return_type,
            where_clause,
            body,
            doc_comment,
            span,
        }
    }

    fn parse_record_fields(&mut self) -> Vec<RecordField> {
        let mut fields = Vec::new();

        if self.at(TokenKind::Begin) {
            self.advance(); // consume Begin
            fields.push(self.parse_record_field());
            while self.at(TokenKind::Sep) {
                self.advance(); // consume Sep
                fields.push(self.parse_record_field());
            }
            self.expect(TokenKind::End, "expected end of record fields");
        } else {
            // Same-line: fields separated by Sep (from layout) or Semicolon
            fields.push(self.parse_record_field());
            while self.at(TokenKind::Sep) || self.at(TokenKind::Semicolon) {
                self.advance();
                fields.push(self.parse_record_field());
            }
        }

        fields
    }

    fn parse_record_field(&mut self) -> RecordField {
        let doc_comment = self.peek().doc_comment.clone();
        let start = self.peek().span.clone();
        let name = self
            .expect_ident("expected field name")
            .unwrap_or_else(|| Spanned::new("<error>".to_string(), start.clone()));

        self.expect(TokenKind::Colon, "expected ':' after field name");
        let type_annotation = self.parse_type_expr();
        let span = start.merge(&type_annotation.span());

        RecordField {
            name,
            type_annotation,
            doc_comment,
            span,
        }
    }

    fn parse_enum_decl(
        &mut self,
        visibility: Visibility,
        doc_comment: Option<String>,
        attributes: Vec<DeriveAttribute>,
    ) -> EnumDecl {
        let start = self.peek().span.clone();
        self.advance(); // consume 'enum'

        let name = self
            .expect_ident("expected enum name")
            .unwrap_or_else(|| Spanned::new("<error>".to_string(), start.clone()));

        // Optional type parameters with variance: <out T, in U>
        let type_params = if self.at(TokenKind::Lt) {
            self.parse_variant_type_param_list()
        } else {
            vec![]
        };

        let construction_private = if self.at(TokenKind::Private) {
            self.advance();
            true
        } else {
            false
        };

        // Optional where clause: `where T: Trait`
        let where_clause = self.parse_where_clause();

        self.expect(TokenKind::Equals, "expected '=' after enum name");

        let variants = self.parse_enum_variants();

        let span = if let Some(last) = variants.last() {
            start.merge(&last.span)
        } else {
            start.merge(&name.span)
        };

        EnumDecl {
            visibility,
            construction_private,
            name,
            type_params,
            variants,
            where_clause,
            doc_comment,
            attributes,
            span,
        }
    }

    fn parse_enum_variants(&mut self) -> Vec<EnumVariant> {
        let mut variants = Vec::new();

        if self.at(TokenKind::Begin) {
            self.advance(); // consume Begin
            variants.push(self.parse_enum_variant());
            while self.at(TokenKind::Sep) {
                self.advance(); // consume Sep
                variants.push(self.parse_enum_variant());
            }
            self.expect(TokenKind::End, "expected end of enum variants");
        } else {
            // Same-line: variants separated by Sep or Semicolon
            variants.push(self.parse_enum_variant());
            while self.at(TokenKind::Sep) || self.at(TokenKind::Semicolon) {
                self.advance();
                variants.push(self.parse_enum_variant());
            }
        }

        variants
    }

    fn parse_enum_variant(&mut self) -> EnumVariant {
        let doc_comment = self.peek().doc_comment.clone();
        let start = self.peek().span.clone();
        let name = self
            .expect_ident("expected variant name")
            .unwrap_or_else(|| Spanned::new("<error>".to_string(), start.clone()));

        let end_span;
        let payload;

        if self.at(TokenKind::LParen) {
            self.advance(); // consume '('
            let mut payload_types = Vec::new();
            if !self.at(TokenKind::RParen) {
                payload_types.push(self.parse_type_expr());
                while self.at(TokenKind::Comma) {
                    self.advance(); // consume ','
                    payload_types.push(self.parse_type_expr());
                }
            }
            end_span = self.peek().span.clone();
            self.expect(
                TokenKind::RParen,
                "expected ')' after variant payload types",
            );
            payload = EnumVariantPayload::Tuple(payload_types);
        } else if self.at(TokenKind::LBrace) {
            self.advance(); // consume '{'
            let mut fields = Vec::new();
            if !self.at(TokenKind::RBrace) {
                fields.push(self.parse_record_field());
                while self.at(TokenKind::Comma)
                    || self.at(TokenKind::Sep)
                    || self.at(TokenKind::Semicolon)
                {
                    self.advance();
                    if self.at(TokenKind::RBrace) {
                        break;
                    }
                    fields.push(self.parse_record_field());
                }
            }
            end_span = self.peek().span.clone();
            self.expect(
                TokenKind::RBrace,
                "expected '}' after variant record fields",
            );
            payload = EnumVariantPayload::Record(fields);
        } else {
            end_span = name.span.clone();
            payload = EnumVariantPayload::None;
        }

        let span = start.merge(&end_span);

        EnumVariant {
            name,
            payload,
            doc_comment,
            span,
        }
    }

    fn parse_record_create(&mut self, type_token: Token, type_args: Vec<TypeExpr>) -> Expr {
        self.advance(); // consume '{'

        let mut fields = Vec::new();

        // Empty record: `Foo {}`
        if self.at(TokenKind::RBrace) {
            let end = self.peek().span.clone();
            self.advance(); // consume '}'
            let span = type_token.span.merge(&end);
            return Expr::RecordCreate {
                type_name: Spanned::new(type_token.text.clone(), type_token.span),
                type_args,
                fields,
                span,
            };
        }

        // Parse field init list, separated by Sep or Semicolon
        fields.push(self.parse_field_init());
        while self.at(TokenKind::Sep) || self.at(TokenKind::Semicolon) {
            self.advance(); // consume Sep or ;
            fields.push(self.parse_field_init());
        }

        let end = self.peek().span.clone();
        self.expect(TokenKind::RBrace, "expected '}' after record fields");
        let span = type_token.span.merge(&end);

        Expr::RecordCreate {
            type_name: Spanned::new(type_token.text.clone(), type_token.span),
            type_args,
            fields,
            span,
        }
    }

    fn parse_enum_variant_record_create(
        &mut self,
        type_name: Spanned<String>,
        variant_name: Spanned<String>,
    ) -> Expr {
        let start = type_name.span.clone();
        self.advance(); // consume '{'

        let mut fields = Vec::new();

        if !self.at(TokenKind::RBrace) {
            fields.push(self.parse_field_init());
            while self.at(TokenKind::Sep) || self.at(TokenKind::Semicolon) {
                self.advance();
                if self.at(TokenKind::RBrace) {
                    break;
                }
                fields.push(self.parse_field_init());
            }
        }

        let end = self.peek().span.clone();
        self.expect(
            TokenKind::RBrace,
            "expected '}' after variant record fields",
        );
        let span = start.merge(&end);

        Expr::EnumVariantRecordCreate {
            type_name,
            variant_name,
            fields,
            span,
        }
    }

    fn parse_record_with(&mut self, object: Expr) -> Expr {
        let start = object.span();
        self.advance(); // consume 'with'

        let mut fields = Vec::new();

        if self.at(TokenKind::Begin) {
            self.advance(); // consume Begin
            fields.push(self.parse_field_init());
            while self.at(TokenKind::Sep) {
                self.advance(); // consume Sep
                fields.push(self.parse_field_init());
            }
            let end = self.peek().span.clone();
            self.expect(TokenKind::End, "expected end of with block");
            let span = start.merge(&end);
            Expr::RecordWith {
                object: Box::new(object),
                fields,
                span,
            }
        } else {
            // Same-line: only `;` separates fields (Sep belongs to outer context)
            fields.push(self.parse_field_init());
            while self.at(TokenKind::Semicolon) {
                self.advance();
                fields.push(self.parse_field_init());
            }
            let end = fields
                .last()
                .map(|f| f.span.clone())
                .unwrap_or_else(|| start.clone());
            let span = start.merge(&end);
            Expr::RecordWith {
                object: Box::new(object),
                fields,
                span,
            }
        }
    }

    fn parse_field_init(&mut self) -> FieldInit {
        let start = self.peek().span.clone();
        let name = self
            .expect_ident("expected field name")
            .unwrap_or_else(|| Spanned::new("<error>".to_string(), start.clone()));

        self.expect(TokenKind::Equals, "expected '=' after field name");
        let value = self.parse_block_expr();
        let span = start.merge(&value.span());

        FieldInit {
            name,
            value: Box::new(value),
            span,
        }
    }

    fn parse_extension_param_list(&mut self, for_type: &TypeExpr) -> Vec<Param> {
        let mut params = Vec::new();

        if self.at(TokenKind::RParen) {
            return params;
        }

        // First param: check for bare `self` (no colon after)
        if self.at(TokenKind::Ident)
            && self.peek().text == "self"
            && self.peek_at(1).kind != TokenKind::Colon
        {
            let token = self.advance().clone();
            let span = token.span.clone();
            params.push(Param {
                name: Spanned::new("self".to_string(), token.span),
                type_annotation: for_type.clone(),
                span,
            });
        } else {
            params.push(self.parse_param());
        }

        while self.at(TokenKind::Comma) {
            self.advance(); // consume ','
            params.push(self.parse_param());
        }

        params
    }

    fn parse_param_list(&mut self) -> Vec<Param> {
        let mut params = Vec::new();

        if self.at(TokenKind::RParen) {
            return params;
        }

        params.push(self.parse_param());

        while self.at(TokenKind::Comma) {
            self.advance(); // consume ','
            params.push(self.parse_param());
        }

        params
    }

    fn parse_param(&mut self) -> Param {
        let start = self.peek().span.clone();
        let name = self
            .expect_ident("expected parameter name")
            .unwrap_or_else(|| Spanned::new("<error>".to_string(), start.clone()));

        self.expect(TokenKind::Colon, "expected ':' after parameter name");
        let type_annotation = self.parse_type_expr();
        let span = start.merge(&type_annotation.span());

        Param {
            name,
            type_annotation,
            span,
        }
    }

    /// Parse an expression that may include a trailing `with` (record update).
    /// Used anywhere a value expression is expected (arguments, conditions, etc.),
    /// except inside `match expr with` where `with` has different meaning.
    fn parse_value_expr(&mut self) -> Expr {
        let expr = self.parse_expr_bp(0);
        if self.at(TokenKind::With) {
            return self.parse_record_with(expr);
        }
        expr
    }

    fn parse_arg_list(&mut self) -> Vec<Expr> {
        let mut args = Vec::new();

        if self.at(TokenKind::RParen) {
            return args;
        }

        args.push(self.parse_value_expr());

        while self.at(TokenKind::Comma) {
            self.advance(); // consume ','
            args.push(self.parse_value_expr());
        }

        args
    }

    // ── Types ────────────────────────────────────────────────────────

    fn parse_type_expr(&mut self) -> TypeExpr {
        self.parse_type_with_arrow(true)
    }

    /// Match annotations leave the unparenthesized arrow for the match arm.
    fn parse_pattern_annotated_type_expr(&mut self) -> TypeExpr {
        self.parse_type_with_arrow(false)
    }

    fn parse_type_with_arrow(&mut self, consume_arrow: bool) -> TypeExpr {
        let (mut left, mut parameters) = self.parse_type_operand(consume_arrow, false);
        while self.at(TokenKind::Tilde) {
            self.advance();
            let (right, _) = self.parse_type_operand(false, true);
            let span = left.span().merge(&right.span());
            left = TypeExpr::TupleExtend(Box::new(left), Box::new(right), span);
            parameters = None;
        }
        if consume_arrow && self.at(TokenKind::FatArrow) {
            self.advance();
            let result = self.parse_type_expr();
            let span = left.span().merge(&result.span());
            return TypeExpr::Function(parameters.unwrap_or_else(|| vec![left]), Box::new(result), span);
        }
        left
    }

    /// Keep an outer parenthesized parameter list distinct from a grouped tuple.
    fn parse_type_operand(
        &mut self,
        allow_function_parameters: bool,
        allow_scalar_group: bool,
    ) -> (TypeExpr, Option<Vec<TypeExpr>>) {
        if self.at(TokenKind::LParen) {
            let start = self.advance().span.clone();
            let mut elements = Vec::new();
            if !self.at(TokenKind::RParen) {
                elements.push(self.parse_type_expr());
                while self.at(TokenKind::Comma) {
                    self.advance();
                    elements.push(self.parse_type_expr());
                }
            }
            let end = self.peek().span.clone();
            self.expect(TokenKind::RParen, "expected ')' after type");
            let span = start.merge(&end);
            let is_parameter_list = allow_function_parameters && self.at(TokenKind::FatArrow);
            if elements.is_empty() && !is_parameter_list {
                self.diagnostics.push(Diagnostic {
                    severity: crate::common::diagnostics::Severity::Error,
                    span: span.clone(),
                    message: "empty parentheses in type position; did you mean `() => T`?".into(),
                    tag: None,
                });
            }
            let ty = if elements.len() == 1 {
                let first = elements[0].clone();
                if !is_parameter_list && !allow_scalar_group && !self.at(TokenKind::Tilde)
                    && !matches!(first, TypeExpr::Function(..) | TypeExpr::Tuple(..) | TypeExpr::TupleExtend(..)) {
                    self.diagnostics.push(Diagnostic { severity: crate::common::diagnostics::Severity::Error, span: start, message: "single-element tuples are not allowed; use the type directly".into(), tag: None });
                }
                first
            } else {
                TypeExpr::Tuple(elements.clone(), span)
            };
            return (ty, Some(elements));
        }
        let first = self.parse_named_type();
        if !self.at(TokenKind::And) {
            return (TypeExpr::Named(first), None);
        }
        let mut elements = vec![first];
        while self.at(TokenKind::And) {
            self.advance();
            elements.push(self.parse_named_type());
        }
        (TypeExpr::Intersection(elements), None)
    }

    fn parse_named_type(&mut self) -> NamedType {
        let mut ident = self
            .expect_ident("expected type name")
            .unwrap_or_else(|| Spanned::new("<error>".to_string(), self.peek().span.clone()));

        while self.at(TokenKind::Dot) {
            self.advance();
            let Some(member) = self.expect_ident("expected associated type name after '.'") else { break };
            ident.value.push('.');
            ident.value.push_str(&member.value);
            ident.span = ident.span.merge(&member.span);
        }

        // Optional type arguments: <Type1, Type2>
        let (type_args, end_span) = if self.at(TokenKind::Lt) {
            let args = self.parse_type_arg_list();
            let end = self.peek().span.clone();
            (args, end)
        } else {
            (vec![], ident.span.clone())
        };

        let span = ident.span.merge(&end_span);
        NamedType {
            name: ident,
            type_args,
            span,
        }
    }

    // ── Expressions ──────────────────────────────────────────────────

    fn parse_block_expr(&mut self) -> Expr {
        if self.at(TokenKind::Begin) {
            let start = self.peek().span.clone();
            self.advance(); // consume Begin

            let mut expressions = Vec::new();
            expressions.push(self.parse_expression());

            while self.at(TokenKind::Sep) {
                self.advance(); // consume Sep
                expressions.push(self.parse_expression());
            }

            let end_span = self.peek().span.clone();
            if !self.at(TokenKind::End) {
                let found = &self.peek().text;
                let found_kind = &self.peek().kind;
                if found.is_empty() {
                    self.error_at_current(&format!("expected end of block, found {:?}", found_kind));
                } else {
                    self.error_at_current(&format!("expected end of block, found '{}'", found));
                }
            } else {
                self.advance();
            }

            let span = start.merge(&end_span);
            Expr::Block(BlockExpr { expressions, span })
        } else {
            self.parse_expression()
        }
    }

    /// Turn a lexer `PrefixedStringLiteral` token into an AST node, sub-parsing
    /// each interpolation's token vector into an expression.
    fn parse_prefixed_literal(&mut self, token: &Token) -> Expr {
        let Some(data) = token.literal.clone() else {
            // Only the prefixed-literal scanner produces this token kind, and
            // it always attaches a payload.
            self.error_at(
                token.span.clone(),
                "internal error: prefixed string literal without parts".to_string(),
            );
            return Expr::StringLiteral(String::new(), token.span.clone());
        };

        let parts = data
            .parts
            .iter()
            .map(|part| match part {
                crate::lexer::token::LiteralPart::Text(text) => {
                    LiteralPart::Text(text.clone(), token.span.clone())
                }
                crate::lexer::token::LiteralPart::Value { tokens, span } => {
                    LiteralPart::Value(self.subparse_expr(tokens.clone(), span), span.clone())
                }
                crate::lexer::token::LiteralPart::Spread { tokens, span } => {
                    LiteralPart::Spread(self.subparse_expr(tokens.clone(), span), span.clone())
                }
            })
            .collect();

        Expr::PrefixedLiteral {
            prefix: Spanned {
                value: data.prefix.clone(),
                span: data.prefix_span.clone(),
            },
            parts,
            span: token.span.clone(),
        }
    }

    /// Parse one interpolation's tokens as a standalone expression.
    ///
    /// The token stream goes through the layout filter first so that
    /// layout-sensitive forms — `${ if a then b else c }` — see the `Begin`/
    /// `End` markers the parser expects. For the overwhelmingly common cases
    /// (`$x`, `${a.b}`, `${f(x)}`) the filter is a pass-through.
    fn subparse_expr(&mut self, mut tokens: Vec<Token>, span: &Span) -> Expr {
        tokens.push(Token::new(TokenKind::Eof, span.clone(), ""));
        let tokens = crate::compiler::layout::LayoutFilter::new(tokens).filter();

        let mut sub = Parser::new(tokens);
        let expr = sub.parse_expression();
        if !sub.at(TokenKind::Eof) {
            let extra = sub.peek().span.clone();
            sub.error_at(
                extra,
                "unexpected tokens after the interpolated expression".to_string(),
            );
        }
        self.diagnostics.extend(sub.diagnostics.iter().cloned());
        expr
    }

    fn parse_expression(&mut self) -> Expr {
        match self.peek().kind {
            TokenKind::Let => self.parse_let_expr(),
            TokenKind::Panic => self.parse_panic_expr(),
            TokenKind::Assert => self.parse_assert_expr(),
            TokenKind::If => self.parse_if_expr(),
            TokenKind::Match => self.parse_match_expr(),
            TokenKind::While => self.parse_while_expr(),
            TokenKind::For => self.parse_for_expr(),
            TokenKind::Try => {
                let start = self.peek().span.clone();
                self.advance(); // consume 'try'
                let operand = self.parse_value_expr();
                let span = start.merge(&operand.span());
                Expr::Try {
                    operand: Box::new(operand),
                    span,
                }
            }
            TokenKind::Await => {
                let start = self.peek().span.clone();
                self.advance(); // consume 'await'
                let operand = self.parse_value_expr();
                let span = start.merge(&operand.span());
                Expr::Await {
                    operand: Box::new(operand),
                    span,
                }
            }
            TokenKind::Use => {
                let start = self.peek().span.clone();
                self.advance(); // consume 'use'
                let operand = self.parse_value_expr();
                let span = start.merge(&operand.span());
                Expr::Use {
                    operand: Box::new(operand),
                    span,
                }
            }
            TokenKind::Break => {
                let span = self.peek().span.clone();
                self.advance();
                Expr::Break(span)
            }
            TokenKind::Continue => {
                let span = self.peek().span.clone();
                self.advance();
                Expr::Continue(span)
            }
            _ => {
                let expr = self.parse_value_expr();
                if self.at(TokenKind::Equals) {
                    return self.parse_assignment(expr);
                }
                // Detect two identifiers in a row on the same line
                // (e.g. `var x = 0` instead of `let mutable x = 0`)
                if self.at(TokenKind::Ident) && self.peek().span.line == expr.span().line {
                    if let Expr::Identifier(ref name, _) = expr {
                        if name == "var" {
                            self.error_at_current(
                                "unexpected identifier after 'var'; to declare a mutable variable use 'let mutable'",
                            );
                        } else {
                            self.error_at_current(&format!(
                                "unexpected identifier after '{}'",
                                name
                            ));
                        }
                    } else {
                        self.error_at_current("unexpected identifier after expression");
                    }
                    // Recovery: skip tokens until Sep or End to avoid cascading errors
                    while !self.at(TokenKind::Sep) && !self.at(TokenKind::End) && !self.at(TokenKind::Eof) {
                        self.advance();
                    }
                }
                expr
            }
        }
    }

    fn parse_panic_expr(&mut self) -> Expr {
        let start = self.peek().span.clone();
        self.advance(); // consume 'panic'
        let message = self.parse_expr_bp(0);
        let span = start.merge(&message.span());
        Expr::Panic {
            message: Box::new(message),
            span,
        }
    }

    fn parse_assert_expr(&mut self) -> Expr {
        let start = self.peek().span.clone();
        self.advance(); // consume 'assert'
        let condition = self.parse_expr_bp(0);
        let (message, end_span) = if self.at(TokenKind::Comma) {
            self.advance(); // consume ','
            let msg = self.parse_expr_bp(0);
            let end = msg.span();
            (Some(Box::new(msg)), end)
        } else {
            (None, condition.span())
        };
        let span = start.merge(&end_span);
        Expr::Assert {
            condition: Box::new(condition),
            message,
            span,
        }
    }

    fn parse_if_expr(&mut self) -> Expr {
        let start = self.peek().span.clone();
        self.advance(); // consume 'if'
        let condition = self.parse_expr_bp(0);
        self.expect(TokenKind::Then, "expected 'then' after if condition");
        let then_branch = self.parse_block_expr();
        let (else_branch, end_span) = if self.at(TokenKind::Else) {
            self.advance(); // consume 'else'
            let else_expr = self.parse_block_expr();
            let end = else_expr.span();
            (Some(Box::new(else_expr)), end)
        } else {
            (None, then_branch.span())
        };
        let span = start.merge(&end_span);
        Expr::If {
            condition: Box::new(condition),
            then_branch: Box::new(then_branch),
            else_branch,
            span,
        }
    }

    fn parse_while_expr(&mut self) -> Expr {
        let start = self.peek().span.clone();
        self.advance(); // consume 'while'
        let condition = self.parse_expr_bp(0);
        self.expect(TokenKind::Do, "expected 'do' after while condition");
        let body = self.parse_block_expr();
        let span = start.merge(&body.span());
        Expr::While {
            condition: Box::new(condition),
            body: Box::new(body),
            span,
        }
    }

    fn parse_for_expr(&mut self) -> Expr {
        let start = self.peek().span.clone();
        self.advance(); // consume 'for'
        let pattern = self.parse_pattern();
        self.expect(TokenKind::In, "expected 'in' after for pattern");
        let iterable = self.parse_expr_bp(0);
        self.expect(TokenKind::Do, "expected 'do' after for iterable");
        let body = self.parse_block_expr();
        let span = start.merge(&body.span());
        Expr::For {
            pattern,
            iterable: Box::new(iterable),
            body: Box::new(body),
            span,
        }
    }

    fn parse_match_expr(&mut self) -> Expr {
        let start = self.peek().span.clone();
        self.advance(); // consume 'match'

        let subject = self.parse_expr_bp(0);
        self.expect(TokenKind::With, "expected 'with' after match subject");

        let mut arms = Vec::new();

        // Parse match body: BEGIN { match_arm SEP } match_arm [SEP] END
        if self.at(TokenKind::Begin) {
            self.advance(); // consume Begin

            arms.push(self.parse_match_arm());

            while self.at(TokenKind::Sep) {
                self.advance(); // consume Sep
                arms.push(self.parse_match_arm());
            }

            let end_span = self.peek().span.clone();
            self.expect(TokenKind::End, "expected end of match block");

            let span = start.merge(&end_span);
            Expr::Match {
                subject: Box::new(subject),
                arms,
                span,
            }
        } else {
            // Single-line: expect at least one arm
            arms.push(self.parse_match_arm());
            let span = start.merge(&arms.last().unwrap().span);
            Expr::Match {
                subject: Box::new(subject),
                arms,
                span,
            }
        }
    }

    fn parse_match_arm(&mut self) -> MatchArm {
        let start = self.peek().span.clone();
        self.expect(TokenKind::Case, "expected 'case' in match arm");

        let pattern = self.parse_pattern();

        let guard = if self.at(TokenKind::If) {
            self.advance(); // consume 'if'
            Some(Box::new(self.parse_expr_bp(0)))
        } else {
            None
        };

        self.expect(TokenKind::FatArrow, "expected '=>' after pattern");

        let body = self.parse_block_expr();
        let span = start.merge(&body.span());

        MatchArm {
            pattern,
            guard,
            body: Box::new(body),
            span,
        }
    }

    /// Parses a pattern, including the one infix form: `h :: t`.
    ///
    /// `::` is right-associative, so `a :: b :: rest` nests to the right. Both
    /// this and `[a, b]` desugar here into plain `List` variant patterns, which
    /// keeps every later phase — inference, exhaustiveness, codegen — unaware
    /// that list patterns exist.
    fn parse_pattern(&mut self) -> Pattern {
        let lhs = self.parse_pattern_atom();
        if self.at(TokenKind::ColonColon) {
            let op_span = self.advance().span.clone();
            let rhs = self.parse_pattern(); // right-associative
            let span = lhs.span().merge(&rhs.span());
            return Self::cons_pattern(lhs, rhs, op_span, span);
        }
        lhs
    }

    /// `Cons(head, tail)`. The empty `type_name` is the bare-variant form: it
    /// resolves against the scrutinee's type, so a user-defined `List` in the
    /// same package cannot shadow the prelude's.
    fn cons_pattern(head: Pattern, tail: Pattern, op_span: Span, span: Span) -> Pattern {
        Pattern::EnumVariantTuple {
            type_name: Spanned::new(String::new(), op_span.clone()),
            variant_name: Spanned::new("Cons".to_string(), op_span),
            payload_patterns: vec![head, tail],
            span,
        }
    }

    /// `Nil`, spanned to whatever source stood for the empty list.
    fn nil_pattern(span: Span) -> Pattern {
        Pattern::EnumVariant {
            type_name: Spanned::new(String::new(), span.clone()),
            variant_name: Spanned::new("Nil".to_string(), span.clone()),
            span,
        }
    }

    /// List pattern: `[]`, `[a]`, `[a, b]`. Desugars to a `Cons` chain ending
    /// in `Nil`, each level spanned to the region it covers.
    fn parse_list_pattern(&mut self) -> Pattern {
        let start = self.peek().span.clone();
        self.advance(); // consume '['

        let mut elements = Vec::new();
        if !self.at(TokenKind::RBracket) {
            elements.push(self.parse_pattern());
            while self.at(TokenKind::Comma) {
                self.advance(); // consume ','
                if self.at(TokenKind::RBracket) {
                    break; // trailing comma
                }
                elements.push(self.parse_pattern());
            }
        }
        let end = self.peek().span.clone();
        self.expect(TokenKind::RBracket, "expected ']' after list patterns");
        let span = start.merge(&end);

        let mut acc = Self::nil_pattern(end);
        for element in elements.into_iter().rev() {
            let cell_span = element.span().merge(&acc.span());
            acc = Self::cons_pattern(element, acc, span.clone(), cell_span);
        }
        // A whole-literal span for `[]`, which has no elements to inherit one.
        match acc {
            Pattern::EnumVariant { type_name, variant_name, .. } => Pattern::EnumVariant {
                type_name,
                variant_name,
                span,
            },
            other => other,
        }
    }

    fn parse_pattern_atom(&mut self) -> Pattern {
        // Tuple pattern: `(x, y)`, `(a, _, c)`, `((a, b), c)`
        if self.at(TokenKind::LParen) {
            return self.parse_tuple_pattern();
        }

        // List pattern: `[]`, `[a, b]`
        if self.at(TokenKind::LBracket) {
            return self.parse_list_pattern();
        }

        // Bool literal patterns
        if self.at(TokenKind::True) {
            let span = self.peek().span.clone();
            self.advance();
            return Pattern::Literal(Box::new(Expr::BoolLiteral(true, span.clone())), span);
        }
        if self.at(TokenKind::False) {
            let span = self.peek().span.clone();
            self.advance();
            return Pattern::Literal(Box::new(Expr::BoolLiteral(false, span.clone())), span);
        }

        // String literal pattern
        if self.at(TokenKind::StringLiteral) {
            let token = self.advance().clone();
            let span = token.span.clone();
            return Pattern::Literal(
                Box::new(Expr::StringLiteral(token.text.clone(), token.span)),
                span,
            );
        }

        // Char literal pattern
        if self.at(TokenKind::CharLiteral) {
            let token = self.advance().clone();
            let c = token.text.chars().next().unwrap_or('\0');
            let span = token.span.clone();
            return Pattern::Literal(Box::new(Expr::CharLiteral(c, token.span)), span);
        }

        // Negative number literal pattern
        if self.at(TokenKind::Minus) {
            let neg_span = self.peek().span.clone();
            self.advance(); // consume '-'
            if self.at(TokenKind::IntLiteral) || self.at(TokenKind::ExactNumberLiteral) {
                let token = self.advance().clone();
                let span = neg_span.merge(&token.span);
                let expr = self.parse_integer_token(&token, span.clone(), true);
                return Pattern::Literal(Box::new(expr), span);
            } else if self.at(TokenKind::FloatLiteral) {
                let token = self.advance().clone();
                let span = neg_span.merge(&token.span);
                let expr = self.parse_float_literal(&token.text, span.clone(), true);
                return Pattern::Literal(Box::new(expr), span);
            } else {
                self.error_at_current("expected number literal after '-' in pattern");
                return Pattern::Wildcard(neg_span);
            }
        }

        // Int literal pattern
        if self.at(TokenKind::IntLiteral) || self.at(TokenKind::ExactNumberLiteral) {
            let token = self.advance().clone();
            let span = token.span.clone();
            let expr = self.parse_integer_token(&token, token.span.clone(), false);
            return Pattern::Literal(Box::new(expr), span);
        }

        // Float literal pattern
        if self.at(TokenKind::FloatLiteral) {
            let token = self.advance().clone();
            let span = token.span.clone();
            let expr = self.parse_float_literal(&token.text, token.span, false);
            return Pattern::Literal(Box::new(expr), span);
        }

        // Identifier: wildcard `_`, type-annotated, record pattern, or variable binding
        if self.at(TokenKind::Ident) {
            let token = self.advance().clone();
            if token.text == "_" {
                return Pattern::Wildcard(token.span);
            }
            // Enum variant pattern: `Color.Red` or `Shape.Circle(r, ...)`
            if self.at(TokenKind::Dot) {
                self.advance(); // consume '.'
                let variant_ident = match self.expect_ident("expected variant name after '.'") {
                    Some(v) => v,
                    None => return Pattern::Wildcard(token.span),
                };
                let variant_name =
                    Spanned::new(variant_ident.value.clone(), variant_ident.span.clone());
                let type_name = Spanned::new(token.text.clone(), token.span.clone());
                if self.at(TokenKind::LParen) {
                    let mut payload_patterns = Vec::new();
                    self.advance(); // consume '('
                    if !self.at(TokenKind::RParen) {
                        payload_patterns.push(self.parse_pattern());
                        while self.at(TokenKind::Comma) {
                            self.advance(); // consume ','
                            if self.at(TokenKind::RParen) {
                                break; // trailing comma
                            }
                            payload_patterns.push(self.parse_pattern());
                        }
                    }
                    let end_span = self.peek().span.clone();
                    self.expect(
                        TokenKind::RParen,
                        "expected ')' after variant payload patterns",
                    );
                    let span = type_name.span.merge(&end_span);
                    return Pattern::EnumVariantTuple {
                        type_name,
                        variant_name,
                        payload_patterns,
                        span,
                    };
                } else if self.at(TokenKind::LBrace) {
                    return self.parse_enum_variant_record_pattern(type_name, variant_name);
                } else {
                    let span = type_name.span.merge(&variant_ident.span);
                    return Pattern::EnumVariant {
                        type_name,
                        variant_name,
                        span,
                    };
                }
            }
            // Bare variant pattern with payload: `Some(x)`, `Ok(v)`, `Error(e)`
            if self.at(TokenKind::LParen) && token.text.starts_with(|c: char| c.is_uppercase()) {
                self.advance(); // consume '('
                let mut payload_patterns = Vec::new();
                if !self.at(TokenKind::RParen) {
                    payload_patterns.push(self.parse_pattern());
                    while self.at(TokenKind::Comma) {
                        self.advance(); // consume ','
                        if self.at(TokenKind::RParen) {
                            break; // trailing comma
                        }
                        payload_patterns.push(self.parse_pattern());
                    }
                }
                let end_span = self.peek().span.clone();
                self.expect(
                    TokenKind::RParen,
                    "expected ')' after variant payload patterns",
                );
                let span = token.span.merge(&end_span);
                return Pattern::EnumVariantTuple {
                    type_name: Spanned::new(String::new(), token.span.clone()),
                    variant_name: Spanned::new(token.text.clone(), token.span.clone()),
                    payload_patterns,
                    span,
                };
            }
            // Type-annotated pattern: `b: Box<Int32>`
            if self.at(TokenKind::Colon) {
                self.advance(); // consume ':'
                let type_expr = self.parse_pattern_annotated_type_expr();
                let span = token.span.merge(&type_expr.span());
                return Pattern::TypeAnnotated {
                    binding: token.text.clone(),
                    type_expr,
                    span,
                };
            }
            // Generic record pattern: `Box<Int32> { ... }`
            if let Some(type_args) = self.try_parse_type_args() {
                if self.at(TokenKind::LBrace) {
                    let type_name = Spanned::new(token.text.clone(), token.span.clone());
                    return self.parse_record_pattern(type_name, type_args);
                } else {
                    self.error_at_current("expected '{' after type arguments in record pattern");
                    return Pattern::Wildcard(token.span);
                }
            }
            // Record pattern: Ident { ... }
            if self.at(TokenKind::LBrace) {
                let type_name = Spanned::new(token.text.clone(), token.span.clone());
                return self.parse_record_pattern(type_name, vec![]);
            }
            return Pattern::Variable(token.text.clone(), token.span);
        }

        // Fallback: error
        let span = self.peek().span.clone();
        self.error_at_current("expected pattern");
        self.advance();
        Pattern::Wildcard(span)
    }

    fn parse_record_pattern(
        &mut self,
        type_name: Spanned<String>,
        type_args: Vec<TypeExpr>,
    ) -> Pattern {
        let start = type_name.span.clone();
        self.advance(); // consume '{'

        let mut fields = Vec::new();

        // Empty record pattern: `Foo {}`
        if !self.at(TokenKind::RBrace) {
            fields.push(self.parse_field_pattern());
            while self.at(TokenKind::Comma) {
                self.advance(); // consume ','
                // Allow trailing comma
                if self.at(TokenKind::RBrace) {
                    break;
                }
                fields.push(self.parse_field_pattern());
            }
        }

        let end = self.peek().span.clone();
        self.expect(
            TokenKind::RBrace,
            "expected '}' after record pattern fields",
        );
        let span = start.merge(&end);

        Pattern::Record {
            type_name,
            type_params: type_args,
            fields,
            span,
        }
    }

    fn parse_enum_variant_record_pattern(
        &mut self,
        type_name: Spanned<String>,
        variant_name: Spanned<String>,
    ) -> Pattern {
        let start = type_name.span.clone();
        self.advance(); // consume '{'

        let mut fields = Vec::new();

        if !self.at(TokenKind::RBrace) {
            fields.push(self.parse_field_pattern());
            while self.at(TokenKind::Comma) {
                self.advance(); // consume ','
                if self.at(TokenKind::RBrace) {
                    break;
                }
                fields.push(self.parse_field_pattern());
            }
        }

        let end = self.peek().span.clone();
        self.expect(
            TokenKind::RBrace,
            "expected '}' after variant record pattern fields",
        );
        let span = start.merge(&end);

        Pattern::EnumVariantRecord {
            type_name,
            variant_name,
            fields,
            span,
        }
    }

    fn parse_field_pattern(&mut self) -> FieldPattern {
        let start = self.peek().span.clone();
        let name = self
            .expect_ident("expected field name in record pattern")
            .unwrap_or_else(|| Spanned::new("<error>".to_string(), start.clone()));

        if self.at(TokenKind::Equals) {
            self.advance(); // consume '='
            let pattern = self.parse_pattern();
            let span = start.merge(&pattern.span());
            FieldPattern {
                name,
                pattern: Some(pattern),
                span,
            }
        } else {
            // Bare identifier shorthand: `x` means `x = x`
            let span = name.span.clone();
            FieldPattern {
                name,
                pattern: None,
                span,
            }
        }
    }

    fn parse_let_expr(&mut self) -> Expr {
        let start = self.peek().span.clone();
        self.advance(); // consume 'let'

        let mutable = if self.at(TokenKind::Mutable) {
            self.advance();
            true
        } else {
            false
        };

        // Tuple destructuring: `let (x, y) = expr`
        if !mutable && self.at(TokenKind::LParen) {
            return self.parse_let_destructure(start);
        }

        let name = self
            .expect_ident("expected variable name")
            .unwrap_or_else(|| Spanned::new("<error>".to_string(), start.clone()));

        let type_annotation = if self.at(TokenKind::Colon) {
            self.advance();
            Some(self.parse_type_expr())
        } else {
            None
        };

        self.expect(TokenKind::Equals, "expected '=' after variable name");

        let value = self.parse_block_expr();
        let span = start.merge(&value.span());

        Expr::Let {
            name,
            mutable,
            type_annotation,
            value: Box::new(value),
            span,
        }
    }

    fn parse_let_destructure(&mut self, start: Span) -> Expr {
        let pattern = self.parse_tuple_pattern();

        let type_annotation = if self.at(TokenKind::Colon) {
            self.advance();
            Some(self.parse_type_expr())
        } else {
            None
        };

        self.expect(
            TokenKind::Equals,
            "expected '=' after destructuring pattern",
        );

        let value = self.parse_block_expr();
        let span = start.merge(&value.span());

        Expr::LetDestructure {
            pattern,
            type_annotation,
            value: Box::new(value),
            span,
        }
    }

    fn parse_tuple_pattern(&mut self) -> Pattern {
        let start = self.peek().span.clone();
        self.advance(); // consume '('

        let mut elements = Vec::new();
        if !self.at(TokenKind::RParen) {
            elements.push(self.parse_pattern());
            while self.at(TokenKind::Comma) {
                self.advance(); // consume ','
                if self.at(TokenKind::RParen) {
                    break; // trailing comma
                }
                elements.push(self.parse_pattern());
            }
        }

        let end = self.peek().span.clone();
        self.expect(TokenKind::RParen, "expected ')' after tuple pattern");
        let span = start.merge(&end);

        Pattern::Tuple(elements, span)
    }

    fn parse_assignment(&mut self, target: Expr) -> Expr {
        self.advance(); // consume '='
        let value = self.parse_block_expr();
        let span = target.span().merge(&value.span());
        Expr::Assignment {
            target: Box::new(target),
            value: Box::new(value),
            span,
        }
    }

    // ── Pratt parser ──────────────────────────────────────────────────

    /// Binding powers for infix (binary) operators.
    /// Returns (left_bp, right_bp). Left-associative: right_bp = left_bp + 1.
    fn infix_binding_power(kind: &TokenKind) -> Option<(u8, u8)> {
        match kind {
            TokenKind::PipePipe => Some((2, 3)), // logical or
            TokenKind::AmpAmp => Some((4, 5)),   // logical and
            TokenKind::Tilde => Some((5, 6)),    // tuple extension
            TokenKind::EqEq | TokenKind::BangEq => Some((6, 7)),
            TokenKind::Lt | TokenKind::Gt | TokenKind::LtEq | TokenKind::GtEq => Some((8, 9)),
            // `::` (cons) is right-associative, hence `(l, l - 1)`. It binds
            // looser than `+`/`++` so `x :: xs ++ ys` is `x :: (xs ++ ys)`, and
            // tighter than comparison so `x :: xs == ys` is `(x :: xs) == ys`.
            TokenKind::ColonColon => Some((11, 10)),
            TokenKind::Pipe => Some((12, 13)),      // bitwise or
            TokenKind::Caret => Some((14, 15)),     // bitwise xor
            TokenKind::Ampersand => Some((16, 17)), // bitwise and
            TokenKind::LtLt | TokenKind::GtGt => Some((18, 19)), // shift
            TokenKind::Plus | TokenKind::Minus | TokenKind::PlusPlus => Some((20, 21)),
            TokenKind::Star | TokenKind::Slash | TokenKind::Percent => Some((22, 23)),
            _ => None,
        }
    }

    /// Binding power for prefix (unary) operators.
    fn prefix_binding_power(kind: &TokenKind) -> Option<u8> {
        match kind {
            TokenKind::Minus | TokenKind::Bang | TokenKind::Tilde => Some(30),
            _ => None,
        }
    }

    fn token_to_binop(kind: &TokenKind) -> BinOp {
        match kind {
            TokenKind::Plus => BinOp::Add,
            TokenKind::PlusPlus => BinOp::Concat,
            TokenKind::Tilde => BinOp::TupleExtend,
            TokenKind::Minus => BinOp::Sub,
            TokenKind::Star => BinOp::Mul,
            TokenKind::Slash => BinOp::Div,
            TokenKind::Percent => BinOp::Rem,
            TokenKind::EqEq => BinOp::Eq,
            TokenKind::BangEq => BinOp::Ne,
            TokenKind::Lt => BinOp::Lt,
            TokenKind::Gt => BinOp::Gt,
            TokenKind::LtEq => BinOp::Le,
            TokenKind::GtEq => BinOp::Ge,
            TokenKind::Ampersand => BinOp::BitAnd,
            TokenKind::Pipe => BinOp::BitOr,
            TokenKind::AmpAmp => BinOp::LogicalAnd,
            TokenKind::PipePipe => BinOp::LogicalOr,
            TokenKind::Caret => BinOp::BitXor,
            TokenKind::LtLt => BinOp::Shl,
            TokenKind::GtGt => BinOp::Shr,
            _ => unreachable!("not a binary operator"),
        }
    }

    fn token_to_unaryop(kind: &TokenKind) -> UnaryOp {
        match kind {
            TokenKind::Minus => UnaryOp::Neg,
            TokenKind::Bang => UnaryOp::Not,
            TokenKind::Tilde => UnaryOp::BitNot,
            _ => unreachable!("not a unary operator"),
        }
    }

    /// Pratt (precedence-climbing) expression parser.
    fn parse_expr_bp(&mut self, min_bp: u8) -> Expr {
        // Prefix / atom
        let mut lhs = if let Some(r_bp) = Self::prefix_binding_power(&self.peek().kind) {
            let op_token = self.advance().clone();
            let op = Self::token_to_unaryop(&op_token.kind);

            // Negative literal folding: fuse `-` with following int/float literal
            if op == UnaryOp::Neg
                && (self.at(TokenKind::IntLiteral)
                    || self.at(TokenKind::ExactNumberLiteral)
                    || self.at(TokenKind::FloatLiteral))
            {
                self.parse_negative_number_literal(op_token.span)
            } else {
                let operand = self.parse_expr_bp(r_bp);
                let span = op_token.span.merge(&operand.span());
                Expr::UnaryOp {
                    op,
                    operand: Box::new(operand),
                    span,
                }
            }
        } else {
            self.parse_postfix_expr()
        };

        // Infix loop
        loop {
            let op_kind = self.peek().kind;
            if let Some((l_bp, r_bp)) = Self::infix_binding_power(&op_kind) {
                if l_bp < min_bp {
                    break;
                }
                let op_token = self.advance().clone();
                let rhs = self.parse_expr_bp(r_bp);
                let span = lhs.span().merge(&rhs.span());

                // `::` builds a list literal rather than a BinaryOp: there is no
                // `BinOp::Cons`. Because it is right-associative the rhs is
                // already complete, so when it is itself a list literal we
                // prepend into it — making `a :: b :: []` produce exactly the
                // node `[a, b]` does, and share its element-type join.
                if op_token.kind == TokenKind::ColonColon {
                    lhs = match rhs {
                        Expr::ListLiteral {
                            mut elements, tail, ..
                        } => {
                            elements.insert(0, lhs);
                            Expr::ListLiteral {
                                elements,
                                tail,
                                span,
                            }
                        }
                        other => Expr::ListLiteral {
                            elements: vec![lhs],
                            tail: Some(Box::new(other)),
                            span,
                        },
                    };
                    continue;
                }

                let op = Self::token_to_binop(&op_token.kind);
                lhs = Expr::BinaryOp {
                    op,
                    left: Box::new(lhs),
                    right: Box::new(rhs),
                    span,
                };
            } else {
                break;
            }
        }

        lhs
    }

    fn parse_postfix_expr(&mut self) -> Expr {
        let mut expr = self.parse_primary_expr();
        loop {
            // Soft keywords (`use`, `await`, `try`) are valid member names after `.`.
            let dot_followed_by_member_name = self.at(TokenKind::Dot)
                && matches!(
                    self.peek_at(1).kind,
                    TokenKind::Ident | TokenKind::Use | TokenKind::Await | TokenKind::Try
                );
            if dot_followed_by_member_name {
                // Check for .orReturn before general member handling
                if self.peek_at(1).text == "orReturn" {
                    self.advance(); // consume '.'
                    let end_token = self.advance().clone(); // consume 'orReturn'
                    let span = expr.span().merge(&end_token.span);
                    expr = Expr::OrReturn {
                        operand: Box::new(expr),
                        span,
                    };
                    continue;
                }
                self.advance(); // consume '.'
                let member = self.advance().clone();
                let member_spanned = Spanned::new(member.text.clone(), member.span.clone());
                if let Some(type_args) = self.try_parse_type_args() {
                    // Generic method call: expr.method<Type>(args) or expr.method<Type>
                    if self.at(TokenKind::LParen) {
                        self.advance(); // consume '('
                        let args = self.parse_arg_list();
                        let end = self.peek().span.clone();
                        self.expect(TokenKind::RParen, "expected ')' after arguments");
                        let span = expr.span().merge(&end);
                        expr = Expr::MethodCall {
                            receiver: Box::new(expr),
                            method: member_spanned,
                            receiver_type_args: vec![],
                            type_args,
                            args,
                            span,
                        };
                    } else {
                        // Property access with type args: obj.prop<Int32>
                        let span = expr.span().merge(&member.span);
                        expr = Expr::FieldAccess {
                            object: Box::new(expr),
                            object_type_params: vec![],
                            field: member_spanned,
                            field_type_params: type_args,
                            span,
                        };
                    }
                } else if self.at(TokenKind::LParen) {
                    // Method call: expr.method(args)
                    self.advance(); // consume '('
                    let args = self.parse_arg_list();
                    let end = self.peek().span.clone();
                    self.expect(TokenKind::RParen, "expected ')' after arguments");
                    let span = expr.span().merge(&end);
                    expr = Expr::MethodCall {
                        receiver: Box::new(expr),
                        method: member_spanned,
                        receiver_type_args: vec![],
                        type_args: vec![],
                        args,
                        span,
                    };
                } else if self.at(TokenKind::LBrace) {
                    // Enum variant record create: Enum.Variant { field = value }
                    if let Expr::Identifier(ref type_name, ref type_span) = expr {
                        let type_name_spanned = Spanned::new(type_name.clone(), type_span.clone());
                        expr = self
                            .parse_enum_variant_record_create(type_name_spanned, member_spanned);
                    } else {
                        // Not an identifier receiver, fall back to field access
                        let span = expr.span().merge(&member.span);
                        expr = Expr::FieldAccess {
                            object: Box::new(expr),
                            object_type_params: vec![],
                            field: member_spanned,
                            field_type_params: vec![],
                            span,
                        };
                    }
                } else {
                    // Field access: expr.field
                    let span = expr.span().merge(&member.span);
                    expr = Expr::FieldAccess {
                        object: Box::new(expr),
                        object_type_params: vec![],
                        field: member_spanned,
                        field_type_params: vec![],
                        span,
                    };
                }
            } else if self.at(TokenKind::LBracketPipe) {
                let span_start = expr.span();
                self.advance();
                let start = if self.at(TokenKind::DotDot) || self.at(TokenKind::DotDotEq) {
                    None
                } else {
                    Some(Box::new(self.parse_value_expr()))
                };
                let inclusive = self.at(TokenKind::DotDotEq);
                self.expect(
                    if inclusive { TokenKind::DotDotEq } else { TokenKind::DotDot },
                    "expected '..' or '..=' in slice expression",
                );
                let end = if self.at(TokenKind::PipeRBracket) && !inclusive {
                    None
                } else {
                    Some(Box::new(self.parse_value_expr()))
                };
                let span_end = self.peek().span.clone();
                self.expect(TokenKind::PipeRBracket, "expected '|]' after slice expression");
                expr = Expr::SliceIndex {
                    object: Box::new(expr), start, end, inclusive,
                    span: span_start.merge(&span_end),
                };
            } else if self.at(TokenKind::LBracket) {
                let start = expr.span();
                self.advance(); // consume '['
                let index = self.parse_value_expr();
                let end = self.peek().span.clone();
                self.expect(TokenKind::RBracket, "expected ']' after index expression");
                let span = start.merge(&end);
                expr = Expr::Index {
                    object: Box::new(expr),
                    index: Box::new(index),
                    span,
                };
            } else if self.at(TokenKind::Is) {
                let start = expr.span();
                self.advance(); // consume 'is'
                let target = self.parse_type_expr();
                let span = start.merge(&target.span());
                expr = Expr::TypeTest {
                    expr: Box::new(expr),
                    target,
                    span,
                };
            } else if self.at(TokenKind::As) {
                let start = expr.span();
                self.advance(); // consume 'as'
                let target = self.parse_type_expr();
                let span = start.merge(&target.span());
                expr = Expr::TypeCast {
                    expr: Box::new(expr),
                    target,
                    span,
                };
            } else {
                break;
            }
        }
        expr
    }

    fn parse_primary_expr(&mut self) -> Expr {
        if self.at(TokenKind::Async) && self.peek_at(1).kind == TokenKind::Do {
            let start = self.advance().span.clone();
            self.advance(); // do already opens a layout block
            let body = self.parse_block_expr();
            let span = start.merge(&body.span());
            return Expr::AsyncDo { body: Box::new(body), span };
        }
        // Async closure: `async x => body` or `async (x: Int32) => body`
        if self.at(TokenKind::Async) && self.peek_at(1).kind != TokenKind::Function {
            let start = self.peek().span.clone();
            self.advance(); // consume 'async'
            let inner = self.parse_primary_expr();
            match inner {
                Expr::Closure { params, body, span, .. } => {
                    let span = start.merge(&span);
                    return Expr::Closure {
                        is_async: true,
                        params,
                        body,
                        span,
                    };
                }
                _ => {
                    self.diagnostics.push(crate::common::diagnostics::Diagnostic {
                        severity: crate::common::diagnostics::Severity::Error,
                        span: start,
                        message: "expected closure after 'async' keyword".to_string(),
                        tag: None,
                    });
                    return inner;
                }
            }
        }

        // Unit literal: `()`
        if self.at(TokenKind::LParen) && self.peek_at(1).kind == TokenKind::RParen {
            let start = self.peek().span.clone();
            self.advance(); // (
            let end = self.peek().span.clone();
            self.advance(); // )

            // Zero-param closure: () => body
            if self.at(TokenKind::FatArrow) {
                self.advance(); // consume =>
                let body = self.parse_block_expr();
                let span = start.merge(&body.span());
                return Expr::Closure {
                    is_async: false,
                    params: vec![],
                    body: Box::new(body),
                    span,
                };
            }

            return Expr::UnitLiteral(start.merge(&end));
        }

        // Parenthesized expression, tuple literal, or closure: `( expr )`, `( expr, ... )`, `(x: T) => body`
        if self.at(TokenKind::LParen) {
            let start = self.peek().span.clone();

            // Early detection of annotated closure params: `(ident: Type, ...)`
            if self.peek_at(1).kind == TokenKind::Ident && self.peek_at(2).kind == TokenKind::Colon {
                self.advance(); // consume (
                let params = self.parse_closure_param_list();
                self.expect(TokenKind::RParen, "expected ')' after closure parameters");
                self.expect(TokenKind::FatArrow, "expected '=>' after closure parameters");
                let body = self.parse_block_expr();
                let span = start.merge(&body.span());
                return Expr::Closure {
                    is_async: false,
                    params,
                    body: Box::new(body),
                    span,
                };
            }

            self.advance(); // consume (
            let first = self.parse_value_expr();
            if self.at(TokenKind::Comma) {
                // Tuple literal or multi-param closure
                let mut elements = vec![first];
                while self.at(TokenKind::Comma) {
                    self.advance(); // consume ','
                    elements.push(self.parse_value_expr());
                }
                let end = self.peek().span.clone();
                self.expect(TokenKind::RParen, "expected ')' after tuple elements");

                // Check for `=>` — multi-param closure: (x, y) => body
                if self.at(TokenKind::FatArrow) {
                    let params = self.exprs_to_closure_params(&elements);
                    self.advance(); // consume =>
                    let body = self.parse_block_expr();
                    let span = start.merge(&body.span());
                    return Expr::Closure {
                        is_async: false,
                        params,
                        body: Box::new(body),
                        span,
                    };
                }

                let span = start.merge(&end);
                return Expr::TupleLiteral { elements, span };
            }
            self.expect(TokenKind::RParen, "expected ')' after expression");

            // Check for `=>` — single-param closure: (x) => body
            if self.at(TokenKind::FatArrow) {
                let params = self.exprs_to_closure_params(&[first]);
                self.advance(); // consume =>
                let body = self.parse_block_expr();
                let span = start.merge(&body.span());
                return Expr::Closure {
                    is_async: false,
                    params,
                    body: Box::new(body),
                    span,
                };
            }

            return first;
        }

        // Bool literals
        if self.at(TokenKind::True) {
            let span = self.peek().span.clone();
            self.advance();
            return Expr::BoolLiteral(true, span);
        }
        if self.at(TokenKind::False) {
            let span = self.peek().span.clone();
            self.advance();
            return Expr::BoolLiteral(false, span);
        }

        // String literal
        if self.at(TokenKind::StringLiteral) {
            let token = self.advance().clone();
            return Expr::StringLiteral(token.text.clone(), token.span);
        }

        // Prefixed string literal: `sql"..."`. Reached from parse_postfix_expr,
        // so `sql"...".fetchOne(conn)` chains like any other primary.
        if self.at(TokenKind::PrefixedStringLiteral) {
            let token = self.advance().clone();
            return self.parse_prefixed_literal(&token);
        }

        // Char literal
        if self.at(TokenKind::CharLiteral) {
            let token = self.advance().clone();
            let c = token.text.chars().next().unwrap_or('\0');
            return Expr::CharLiteral(c, token.span);
        }

        // Integer literal
        if self.at(TokenKind::IntLiteral) || self.at(TokenKind::ExactNumberLiteral) {
            let token = self.advance().clone();
            return self.parse_integer_token(&token, token.span.clone(), false);
        }

        // Float literal
        if self.at(TokenKind::FloatLiteral) {
            let token = self.advance().clone();
            return self.parse_float_literal(&token.text, token.span, false);
        }

        // Try expression (also usable inside sub-expressions)
        if self.at(TokenKind::Try) {
            let start = self.peek().span.clone();
            self.advance(); // consume 'try'
            let operand = self.parse_value_expr();
            let span = start.merge(&operand.span());
            return Expr::Try {
                operand: Box::new(operand),
                span,
            };
        }

        // Await expression (also usable inside sub-expressions)
        if self.at(TokenKind::Await) {
            let start = self.peek().span.clone();
            self.advance(); // consume 'await'
            let operand = self.parse_value_expr();
            let span = start.merge(&operand.span());
            return Expr::Await {
                operand: Box::new(operand),
                span,
            };
        }

        // Use expression (also usable inside sub-expressions)
        if self.at(TokenKind::Use) {
            let start = self.peek().span.clone();
            self.advance(); // consume 'use'
            let operand = self.parse_value_expr();
            let span = start.merge(&operand.span());
            return Expr::Use {
                operand: Box::new(operand),
                span,
            };
        }

        // If expression (also usable inside sub-expressions)
        if self.at(TokenKind::If) {
            return self.parse_if_expr();
        }

        // Match expression (also usable inside sub-expressions)
        if self.at(TokenKind::Match) {
            return self.parse_match_expr();
        }

        // While expression (also usable inside sub-expressions)
        if self.at(TokenKind::While) {
            return self.parse_while_expr();
        }

        // Array literal: [| expr, ... |]
        if self.at(TokenKind::LBracketPipe) {
            return self.parse_array_literal();
        }

        // List literal: [expr, ...]
        if self.at(TokenKind::LBracket) {
            return self.parse_list_literal();
        }

        // Super keyword (for super.method() calls)
        if self.at(TokenKind::Super) {
            let token = self.advance().clone();
            return Expr::Identifier("super".to_string(), token.span);
        }

        // Identifier, function call, or record construction
        if self.at(TokenKind::Ident) {
            let token = self.advance().clone();

            // Bare identifier closure: `x => body`
            if self.at(TokenKind::FatArrow) {
                let param = ClosureParam {
                    kind: ClosureParamKind::Name(Spanned::new(token.text.clone(), token.span.clone())),
                    type_annotation: None,
                    span: token.span.clone(),
                };
                self.advance(); // consume =>
                let body = self.parse_block_expr();
                let span = token.span.merge(&body.span());
                return Expr::Closure {
                    is_async: false,
                    params: vec![param],
                    body: Box::new(body),
                    span,
                };
            }

            // Generic call or generic record: ident<Type>(args) or ident<Type> { ... }
            if let Some(type_args) = self.try_parse_type_args() {
                if self.at(TokenKind::LParen) {
                    // Generic function call: ident<Type>(args)
                    self.advance(); // consume '('
                    let args = self.parse_arg_list();
                    let end = self.peek().span.clone();
                    self.expect(TokenKind::RParen, "expected ')' after arguments");
                    let span = token.span.merge(&end);
                    return Expr::FunctionCall {
                        name: Spanned::new(token.text.clone(), token.span),
                        type_args,
                        args,
                        span,
                    };
                } else if self.at(TokenKind::LBrace) {
                    // Generic record construction: Ident<Type> { ... }
                    return self.parse_record_create(token, type_args);
                } else if self.at(TokenKind::Dot) && self.peek_at(1).kind == TokenKind::Ident {
                    // Type-qualified method call: Ident<TypeArgs>.method(args)
                    self.advance(); // consume '.'
                    let member = self.advance().clone();
                    let member_spanned = Spanned::new(member.text.clone(), member.span.clone());
                    let method_type_args = self.try_parse_type_args().unwrap_or_default();
                    if self.at(TokenKind::LParen) {
                        self.advance(); // consume '('
                        let args = self.parse_arg_list();
                        let end = self.peek().span.clone();
                        self.expect(TokenKind::RParen, "expected ')' after arguments");
                        let span = token.span.merge(&end);
                        return Expr::MethodCall {
                            receiver: Box::new(Expr::Identifier(token.text.clone(), token.span)),
                            method: member_spanned,
                            receiver_type_args: type_args,
                            type_args: method_type_args,
                            args,
                            span,
                        };
                    } else {
                        // Type-qualified field access: Ident<TypeArgs>.member
                        let span = token.span.merge(&member.span);
                        return Expr::FieldAccess {
                            object: Box::new(Expr::Identifier(token.text.clone(), token.span)),
                            object_type_params: type_args,
                            field: member_spanned,
                            field_type_params: method_type_args,
                            span,
                        };
                    }
                } else {
                    self.error_at_current("expected '(' or '{' after type arguments");
                    return Expr::Identifier(token.text.clone(), token.span);
                }
            }

            // Function call: ident(args)
            if self.at(TokenKind::LParen) {
                self.advance(); // consume '('
                let args = self.parse_arg_list();
                let end = self.peek().span.clone();
                self.expect(TokenKind::RParen, "expected ')' after arguments");
                let span = token.span.merge(&end);
                return Expr::FunctionCall {
                    name: Spanned::new(token.text.clone(), token.span),
                    type_args: vec![],
                    args,
                    span,
                };
            }

            // Record construction: Ident { field = value; ... }
            if self.at(TokenKind::LBrace) {
                return self.parse_record_create(token, vec![]);
            }

            return Expr::Identifier(token.text.clone(), token.span);
        }

        // Fallback: error
        let span = self.peek().span.clone();
        self.error_at_current("expected expression");
        self.advance();
        Expr::UnitLiteral(span) // placeholder
    }

    // ── Closure helper methods ────────────────────────────────────────

    /// Parse a comma-separated list of closure parameters: `ident [: type], ...`
    fn parse_closure_param_list(&mut self) -> Vec<ClosureParam> {
        let mut params = Vec::new();
        params.push(self.parse_closure_param());
        while self.at(TokenKind::Comma) {
            self.advance(); // consume ','
            params.push(self.parse_closure_param());
        }
        params
    }

    /// Parse a single closure parameter: `ident`, `ident: type`, or `(pattern)`
    fn parse_closure_param(&mut self) -> ClosureParam {
        // Tuple pattern param: ((a, b), c)
        if self.at(TokenKind::LParen) {
            let pattern = self.parse_tuple_pattern();
            let span = pattern.span();
            return ClosureParam {
                kind: ClosureParamKind::TuplePattern(pattern),
                type_annotation: None,
                span,
            };
        }

        let token = if self.at(TokenKind::Ident) {
            self.advance().clone()
        } else {
            let span = self.peek().span.clone();
            self.error_at_current("expected parameter name or tuple pattern");
            Token::new(TokenKind::Ident, span, "_")
        };
        let name = Spanned::new(token.text.clone(), token.span.clone());
        let start = token.span.clone();
        let type_annotation = if self.at(TokenKind::Colon) {
            self.advance(); // consume ':'
            Some(self.parse_type_expr())
        } else {
            None
        };
        let end = type_annotation
            .as_ref()
            .map(|t| t.span())
            .unwrap_or(start.clone());
        ClosureParam {
            kind: ClosureParamKind::Name(name),
            type_annotation,
            span: start.merge(&end),
        }
    }

    /// Convert parsed expressions to closure params.
    /// Each expression must be an `Expr::Identifier` or `Expr::TupleLiteral`; otherwise emit an error.
    fn exprs_to_closure_params(&mut self, exprs: &[Expr]) -> Vec<ClosureParam> {
        exprs
            .iter()
            .map(|e| match e {
                Expr::Identifier(name, span) => ClosureParam {
                    kind: ClosureParamKind::Name(Spanned::new(name.clone(), span.clone())),
                    type_annotation: None,
                    span: span.clone(),
                },
                Expr::TupleLiteral { elements, span } => {
                    let pattern = self.exprs_to_tuple_pattern(elements, span);
                    ClosureParam {
                        kind: ClosureParamKind::TuplePattern(pattern),
                        type_annotation: None,
                        span: span.clone(),
                    }
                }
                _ => {
                    self.diagnostics.push(Diagnostic {
                        severity: crate::common::diagnostics::Severity::Error,
                        span: e.span(),
                        message: "expected parameter name or tuple pattern in closure".to_string(),
                        tag: None,
                    });
                    ClosureParam {
                        kind: ClosureParamKind::Name(Spanned::new("_".to_string(), e.span())),
                        type_annotation: None,
                        span: e.span(),
                    }
                }
            })
            .collect()
    }

    /// Convert a list of expressions to a tuple pattern (for closure param destructuring).
    fn exprs_to_tuple_pattern(&mut self, elements: &[Expr], span: &Span) -> Pattern {
        let sub_patterns: Vec<Pattern> = elements.iter().map(|e| self.expr_to_pattern(e)).collect();
        Pattern::Tuple(sub_patterns, span.clone())
    }

    /// Convert an expression to a pattern (for closure param destructuring).
    fn expr_to_pattern(&mut self, expr: &Expr) -> Pattern {
        match expr {
            Expr::Identifier(name, span) if name == "_" => Pattern::Wildcard(span.clone()),
            Expr::Identifier(name, span) => Pattern::Variable(name.clone(), span.clone()),
            Expr::TupleLiteral { elements, span } => self.exprs_to_tuple_pattern(elements, span),
            _ => {
                self.diagnostics.push(Diagnostic {
                    severity: crate::common::diagnostics::Severity::Error,
                    span: expr.span(),
                    message: "expected identifier or tuple pattern in closure destructuring"
                        .to_string(),
                    tag: None,
                });
                Pattern::Wildcard(expr.span())
            }
        }
    }

    // ── Array literal parsing ─────────────────────────────────────────

    /// Parses an array literal: `[| a, b, c |]`, or `[||]` when empty.
    fn parse_array_literal(&mut self) -> Expr {
        let start = self.peek().span.clone();
        self.advance(); // consume '[|'

        let mut elements = Vec::new();

        // Empty array: `[||]`
        if self.at(TokenKind::PipeRBracket) {
            let end = self.peek().span.clone();
            self.advance();
            let span = start.merge(&end);
            return Expr::ArrayLiteral { elements, span };
        }

        // First element
        elements.push(self.parse_value_expr());

        // Remaining elements separated by commas
        while self.at(TokenKind::Comma) {
            self.advance(); // consume ','
            // Allow trailing comma
            if self.at(TokenKind::PipeRBracket) {
                break;
            }
            elements.push(self.parse_value_expr());
        }

        let end = self.peek().span.clone();
        self.expect(
            TokenKind::PipeRBracket,
            "expected '|]' after array elements",
        );
        let span = start.merge(&end);

        Expr::ArrayLiteral { elements, span }
    }

    /// Parses a list literal: `[a, b, c]`, or `[]` when empty.
    ///
    /// `::` produces the same node (see the infix loop), so `a :: b :: []` and
    /// `[a, b]` are indistinguishable after parsing and share one inference
    /// path — which is what lets both do the same element-type join.
    fn parse_list_literal(&mut self) -> Expr {
        let start = self.peek().span.clone();
        self.advance(); // consume '['

        let mut elements = Vec::new();

        // Empty list: `[]`
        if self.at(TokenKind::RBracket) {
            let end = self.peek().span.clone();
            self.advance();
            let span = start.merge(&end);
            return Expr::ListLiteral {
                elements,
                tail: None,
                span,
            };
        }

        elements.push(self.parse_value_expr());

        while self.at(TokenKind::Comma) {
            self.advance(); // consume ','
            // Allow trailing comma
            if self.at(TokenKind::RBracket) {
                break;
            }
            elements.push(self.parse_value_expr());
        }

        let end = self.peek().span.clone();
        self.expect(TokenKind::RBracket, "expected ']' after list elements");
        let span = start.merge(&end);

        Expr::ListLiteral {
            elements,
            tail: None,
            span,
        }
    }

    // ── Number literal parsing ────────────────────────────────────────

    /// Parse a negative number literal (fused from `-` prefix + literal token).
    fn parse_negative_number_literal(&mut self, neg_span: Span) -> Expr {
        if self.at(TokenKind::IntLiteral) || self.at(TokenKind::ExactNumberLiteral) {
            let token = self.advance().clone();
            let span = neg_span.merge(&token.span);
            self.parse_integer_token(&token, span, true)
        } else {
            let token = self.advance().clone();
            let span = neg_span.merge(&token.span);
            self.parse_float_literal(&token.text, span, true)
        }
    }

    fn parse_integer_token(&mut self, token: &Token, span: Span, negate: bool) -> Expr {
        if token.kind == TokenKind::ExactNumberLiteral {
            let text = if negate {
                format!("-{}", token.text)
            } else {
                token.text.clone()
            };
            Expr::ExactNumberLiteral(text, span)
        } else {
            self.parse_int_literal(&token.text, span, negate)
        }
    }

    /// Parse an integer literal from raw text. `negate` is true when preceded by `-`.
    fn parse_int_literal(&mut self, text: &str, span: Span, negate: bool) -> Expr {
        let (digits, radix, suffix) = Self::split_int_literal(text);

        // Uint128 is parsed separately — its value can exceed u64, so it needs a u128 parse.
        if suffix == "u128" {
            if negate {
                self.error_negate_unsigned(&span, text);
                return Expr::Uint128Literal(0, span);
            }
            let raw = match u128::from_str_radix(digits, radix) {
                Ok(v) => v,
                Err(_) => {
                    self.diagnostics.push(Diagnostic {
                        severity: crate::common::diagnostics::Severity::Error,
                        span: span.clone(),
                        message: format!("invalid integer literal: '{}'", text),
                        tag: None,
                    });
                    return Expr::Uint128Literal(0, span);
                }
            };
            return Expr::Uint128Literal(raw, span);
        }

        // Parse the unsigned value
        let raw = match u64::from_str_radix(digits, radix) {
            Ok(v) => v,
            Err(_) => {
                self.diagnostics.push(Diagnostic {
                    severity: crate::common::diagnostics::Severity::Error,
                    span: span.clone(),
                    message: format!("invalid integer literal: '{}'", text),
                    tag: None,
                });
                return Expr::Int32Literal(0, span);
            }
        };

        let signed_val = if negate { -(raw as i128) } else { raw as i128 };

        match suffix {
            "i8" => {
                self.check_signed_range(
                    signed_val,
                    i8::MIN as i128,
                    i8::MAX as i128,
                    "Int8",
                    &span,
                    text,
                );
                Expr::Int8Literal(signed_val as i8, span)
            }
            "i16" => {
                self.check_signed_range(
                    signed_val,
                    i16::MIN as i128,
                    i16::MAX as i128,
                    "Int16",
                    &span,
                    text,
                );
                Expr::Int16Literal(signed_val as i16, span)
            }
            "" | "i32" => {
                self.check_signed_range(
                    signed_val,
                    i32::MIN as i128,
                    i32::MAX as i128,
                    "Int32",
                    &span,
                    text,
                );
                Expr::Int32Literal(signed_val as i32, span)
            }
            "i64" => {
                self.check_signed_range(
                    signed_val,
                    i64::MIN as i128,
                    i64::MAX as i128,
                    "Int64",
                    &span,
                    text,
                );
                Expr::Int64Literal(signed_val as i64, span)
            }
            "u8" => {
                if negate {
                    self.error_negate_unsigned(&span, text);
                    return Expr::Uint8Literal(0, span);
                }
                self.check_unsigned_range(raw, u8::MAX as u64, "Uint8", &span, text);
                Expr::Uint8Literal(raw as u8, span)
            }
            "u16" => {
                if negate {
                    self.error_negate_unsigned(&span, text);
                    return Expr::Uint16Literal(0, span);
                }
                self.check_unsigned_range(raw, u16::MAX as u64, "Uint16", &span, text);
                Expr::Uint16Literal(raw as u16, span)
            }
            "u32" => {
                if negate {
                    self.error_negate_unsigned(&span, text);
                    return Expr::Uint32Literal(0, span);
                }
                self.check_unsigned_range(raw, u32::MAX as u64, "Uint32", &span, text);
                Expr::Uint32Literal(raw as u32, span)
            }
            "u64" => {
                if negate {
                    self.error_negate_unsigned(&span, text);
                    return Expr::Uint64Literal(0, span);
                }
                // raw is already u64, so no range check needed
                Expr::Uint64Literal(raw, span)
            }
            _ => {
                self.diagnostics.push(Diagnostic {
                    severity: crate::common::diagnostics::Severity::Error,
                    span: span.clone(),
                    message: format!("unknown integer suffix: '{}'", suffix),
                    tag: None,
                });
                Expr::Int32Literal(0, span)
            }
        }
    }

    fn check_signed_range(
        &mut self,
        val: i128,
        min: i128,
        max: i128,
        ty: &str,
        span: &Span,
        text: &str,
    ) {
        if val < min || val > max {
            self.diagnostics.push(Diagnostic {
                severity: crate::common::diagnostics::Severity::Error,
                span: span.clone(),
                message: format!("integer literal '{}' out of range for {}", text, ty),
                tag: None,
            });
        }
    }

    fn check_unsigned_range(&mut self, val: u64, max: u64, ty: &str, span: &Span, text: &str) {
        if val > max {
            self.diagnostics.push(Diagnostic {
                severity: crate::common::diagnostics::Severity::Error,
                span: span.clone(),
                message: format!("integer literal '{}' out of range for {}", text, ty),
                tag: None,
            });
        }
    }

    fn error_negate_unsigned(&mut self, span: &Span, text: &str) {
        self.diagnostics.push(Diagnostic {
            severity: crate::common::diagnostics::Severity::Error,
            span: span.clone(),
            message: format!("cannot negate unsigned literal '{}'", text),
            tag: None,
        });
    }

    /// Split an integer literal text into (digits, radix, suffix).
    fn split_int_literal(text: &str) -> (&str, u32, &str) {
        let (body, radix) = if let Some(hex) = text.strip_prefix("0x").or(text.strip_prefix("0X")) {
            (hex, 16)
        } else if let Some(bin) = text.strip_prefix("0b").or(text.strip_prefix("0B")) {
            (bin, 2)
        } else if let Some(oct) = text.strip_prefix("0o").or(text.strip_prefix("0O")) {
            (oct, 8)
        } else {
            (text, 10)
        };

        // Find suffix start — check longer suffixes first to avoid i1 matching i16
        let suffixes = ["u128", "i64", "i32", "i16", "i8", "u64", "u32", "u16", "u8"];
        for s in &suffixes {
            if let Some(digits) = body.strip_suffix(s) {
                return (digits, radix, s);
            }
        }
        (body, radix, "")
    }

    /// Parse a float literal from raw text. `negate` is true when preceded by `-`.
    fn parse_float_literal(&mut self, text: &str, span: Span, negate: bool) -> Expr {
        let (body, suffix) = if let Some(b) = text.strip_suffix("f32") {
            (b, "f32")
        } else if let Some(b) = text.strip_suffix("f64") {
            (b, "f64")
        } else {
            (text, "")
        };

        let value: f64 = match body.parse() {
            Ok(v) => v,
            Err(_) => {
                self.diagnostics.push(Diagnostic {
                    severity: crate::common::diagnostics::Severity::Error,
                    span: span.clone(),
                    message: format!("invalid float literal: '{}'", text),
                    tag: None,
                });
                return Expr::Float64Literal(0.0, span);
            }
        };

        let value = if negate { -value } else { value };

        match suffix {
            "f32" => Expr::Float32Literal(value as f32, span),
            "" | "f64" => Expr::Float64Literal(value, span),
            _ => unreachable!(),
        }
    }

    // ── Type parameters and type arguments ────────────────────────────

    /// Parse a type parameter list: `<T, U>`. Assumes current token is `<`.
    fn parse_type_param_list(&mut self) -> Vec<Spanned<String>> {
        self.advance(); // consume '<'
        let mut params = Vec::new();

        if let Some(ident) = self.expect_ident("expected type parameter name") {
            params.push(ident);
        }

        while self.at(TokenKind::Comma) {
            self.advance(); // consume ','
            if let Some(ident) = self.expect_ident("expected type parameter name") {
                params.push(ident);
            }
        }

        self.expect_gt("expected '>' after type parameters");
        params
    }

    /// Parse a type parameter list with optional variance annotations: `<out T, in U, V>`.
    /// Used only for record and enum declarations.
    fn parse_variant_type_param_list(&mut self) -> Vec<VariantTypeParam> {
        self.advance(); // consume '<'
        let mut params = Vec::new();

        if let Some(tp) = self.parse_single_variant_type_param() {
            params.push(tp);
        }

        while self.at(TokenKind::Comma) {
            self.advance(); // consume ','
            if let Some(tp) = self.parse_single_variant_type_param() {
                params.push(tp);
            }
        }

        self.expect_gt("expected '>' after type parameters");
        params
    }

    /// Parse a single type parameter with optional variance prefix: `out T`, `in T`, or `T`.
    fn parse_single_variant_type_param(&mut self) -> Option<VariantTypeParam> {
        let start = self.peek().span.clone();

        let variance = if self.at(TokenKind::Out) {
            self.advance();
            Variance::Covariant
        } else if self.at(TokenKind::In) {
            self.advance();
            Variance::Contravariant
        } else {
            Variance::Invariant
        };

        let name = self.expect_ident("expected type parameter name")?;
        let span = start.merge(&name.span);

        Some(VariantTypeParam {
            variance,
            name,
            span,
        })
    }

    /// Parse a type argument list: `<Type1, Type2>`. Assumes current token is `<`.
    fn parse_type_arg_list(&mut self) -> Vec<TypeExpr> {
        self.advance(); // consume '<'
        let mut args = Vec::new();

        args.push(self.parse_type_expr());

        while self.at(TokenKind::Comma) {
            self.advance(); // consume ','
            args.push(self.parse_type_expr());
        }

        self.expect_gt("expected '>' after type arguments");
        args
    }

    /// Expect a `>` closing delimiter. Handles `>>` (GtGt) by splitting it:
    /// consumes one `>` and replaces the current token with a single `>`.
    fn expect_gt(&mut self, msg: &str) {
        if self.at(TokenKind::Gt) {
            self.advance();
        } else if self.at(TokenKind::GtGt) {
            // Split ">>" into ">" + ">": consume ">>" and replace with ">"
            let tok = self.peek().clone();
            self.tokens[self.pos] = Token::new(TokenKind::Gt, tok.span, ">");
            // Don't advance — the replacement ">" stays for the outer parse_type_arg_list
        } else {
            self.diagnostics.push(Diagnostic {
                severity: crate::common::diagnostics::Severity::Error,
                span: self.peek().span.clone(),
                message: msg.to_string(),
                tag: None,
            });
        }
    }

    /// Try to parse `<Type, ...>` as a type argument list using backtracking.
    /// Returns `Some(type_args)` if successful, `None` if `<` is not a type arg opener.
    /// On failure, parser position is restored (no diagnostics emitted).
    fn try_parse_type_args(&mut self) -> Option<Vec<TypeExpr>> {
        if !self.at(TokenKind::Lt) {
            return None;
        }
        let saved_pos = self.pos;
        let saved_diagnostics_len = self.diagnostics.len();
        // Also save any token mutations (expect_gt may mutate GtGt -> Gt)
        let saved_token = self.tokens.get(self.pos).cloned();

        self.advance(); // consume '<'
        let mut args = Vec::new();
        args.push(self.parse_type_expr());

        while self.at(TokenKind::Comma) {
            self.advance();
            args.push(self.parse_type_expr());
        }

        // Check for closing '>' (or '>>' which gets split)
        if self.at(TokenKind::Gt) || self.at(TokenKind::GtGt) {
            self.expect_gt("expected '>' after type arguments");
            Some(args)
        } else {
            // Backtrack: restore position, diagnostics, and any mutated tokens
            self.diagnostics.truncate(saved_diagnostics_len);
            if let Some(tok) = saved_token {
                self.tokens[saved_pos] = tok;
            }
            self.pos = saved_pos;
            None
        }
    }

    // ── Helpers ──────────────────────────────────────────────────────

    fn peek(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn peek_at(&self, offset: usize) -> &Token {
        let idx = (self.pos + offset).min(self.tokens.len() - 1);
        &self.tokens[idx]
    }

    fn advance(&mut self) -> &Token {
        let token = &self.tokens[self.pos.min(self.tokens.len() - 1)];
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        token
    }

    fn at(&self, kind: TokenKind) -> bool {
        self.peek().kind == kind
    }

    fn expect(&mut self, kind: TokenKind, msg: &str) {
        if self.at(kind) {
            self.advance();
        } else {
            self.error_at_current(msg);
        }
    }

    fn expect_ident(&mut self, msg: &str) -> Option<Spanned<String>> {
        // Soft keywords: keywords that can also serve as identifiers in declaration
        // positions (method names, field names, parameter names). `use`, `await`,
        // and `try` are prefix operators in expression position but otherwise behave
        // like identifiers — letting trait/impl authors name a method `use` etc.
        // without escape syntax.
        if self.at(TokenKind::Ident)
            || self.at(TokenKind::Use)
            || self.at(TokenKind::Await)
            || self.at(TokenKind::Try)
        {
            let token = self.advance().clone();
            Some(Spanned::new(token.text.clone(), token.span))
        } else {
            self.error_at_current(msg);
            None
        }
    }

    fn error_at(&mut self, span: Span, msg: String) {
        self.diagnostics.push(Diagnostic {
            severity: crate::common::diagnostics::Severity::Error,
            span,
            message: msg,
            tag: None,
        });
    }

    fn error_at_current(&mut self, msg: &str) {
        let span = self.peek().span.clone();
        self.diagnostics.push(Diagnostic {
            severity: crate::common::diagnostics::Severity::Error,
            span,
            message: msg.to_string(),
            tag: None,
        });
    }

    fn skip_to_declaration(&mut self) {
        loop {
            match self.peek().kind {
                TokenKind::Function
                | TokenKind::Let
                | TokenKind::Record
                | TokenKind::Enum
                | TokenKind::Trait
                | TokenKind::Interface
                | TokenKind::Extension
                | TokenKind::Implement
                | TokenKind::Module
                | TokenKind::Newtype
                | TokenKind::Public
                | TokenKind::Internal
                | TokenKind::Private
                | TokenKind::Eof => {
                    break;
                }
                _ => {
                    self.advance();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::span::FilePath;
    use crate::layout::LayoutFilter;
    use crate::lexer::Lexer;
    use crate::lexer::attach_doc_comments;

    fn parse(source: &str) -> (SourceFile, Vec<Diagnostic>) {
        let mut lexer = Lexer::new(source, FilePath::from("test.dove"));
        let tokens = lexer.tokenize();
        let tokens = attach_doc_comments(tokens);
        let mut filter = LayoutFilter::new(tokens);
        let filtered = filter.filter();
        let mut parser = Parser::new(filtered);
        let source_file = parser.parse_source_file();
        let diagnostics = parser.diagnostics().to_vec();
        (source_file, diagnostics)
    }

    fn unwrap_function(decl: &Declaration) -> &FunctionDecl {
        match decl {
            Declaration::Function(f) => f,
            _ => panic!("expected function declaration"),
        }
    }

    #[test]
    fn tuple_extension_accepts_grouped_scalar_operands() {
        for ty in ["(Int32) ~ Bool", "Int32 ~ (Bool)", "(Int32) ~ (Bool)"] {
            let (source, diagnostics) = parse(&format!("package a\ntype Example = {ty}"));
            assert!(diagnostics.is_empty(), "{ty}: {diagnostics:?}");
            let Declaration::TypeAlias(alias) = &source.declarations[0] else {
                panic!("expected type alias");
            };
            let TypeExpr::TupleExtend(left, right, _) = &alias.type_expr else {
                panic!("expected extension type");
            };
            assert!(matches!(**left, TypeExpr::Named(_)));
            assert!(matches!(**right, TypeExpr::Named(_)));
        }
        let (_, diagnostics) = parse("package a\ntype Example = (Int32)");
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.message.contains("single-element tuples are not allowed")
        }));
    }

    #[test]
    fn empty_type_parentheses_require_a_function_arrow() {
        for source in [
            "package a\ntype Example = () ~ Int32",
            "package a\ntype Example = Int32 ~ () => Bool",
            "package a\nfunction f(x: Any): Unit =\n    match x with\n        case value: () => ()\n        case _ => ()",
        ] {
            let (_, diagnostics) = parse(source);
            assert!(diagnostics.iter().any(|diagnostic| {
                diagnostic.message.contains("empty parentheses in type position")
            }), "{source}: {diagnostics:?}");
        }
        for ty in ["() => Bool", "Int32 ~ (() => Bool)", "(() => Bool) ~ Int32"] {
            let (_, diagnostics) = parse(&format!("package a\ntype Example = {ty}"));
            assert!(diagnostics.is_empty(), "{ty}: {diagnostics:?}");
        }
    }

    #[test]
    fn test_parse_minimal_program() {
        let (source_file, diagnostics) = parse("package a\n\nfunction main(): Unit = ()");
        assert!(
            diagnostics.is_empty(),
            "unexpected errors: {:?}",
            diagnostics
        );

        // Package
        assert_eq!(source_file.package.path.len(), 1);
        assert_eq!(source_file.package.path[0].value, "a");

        // One function declaration
        assert_eq!(source_file.declarations.len(), 1);
        let f = unwrap_function(&source_file.declarations[0]);
        assert_eq!(f.name.value, "main");
        assert!(f.params.is_empty());
        assert!(f.return_type.is_some());
        match &f.return_type {
            Some(TypeExpr::Named(n)) => assert_eq!(n.name.value, "Unit"),
            _ => panic!("expected named type"),
        }
        match &f.body {
            Expr::UnitLiteral(_) => {}
            _ => panic!("expected unit literal"),
        }
    }

    #[test]
    fn test_parse_dotted_package() {
        let (source_file, diagnostics) =
            parse("package com.example.myapp\n\nfunction main(): Unit = ()");
        assert!(
            diagnostics.is_empty(),
            "unexpected errors: {:?}",
            diagnostics
        );
        assert_eq!(source_file.package.path.len(), 3);
        assert_eq!(source_file.package.path[0].value, "com");
        assert_eq!(source_file.package.path[1].value, "example");
        assert_eq!(source_file.package.path[2].value, "myapp");
    }

    #[test]
    fn test_parse_bool_literal_true() {
        let (source_file, diagnostics) = parse("package a\n\nfunction main(): Unit = true");
        assert!(
            diagnostics.is_empty(),
            "unexpected errors: {:?}",
            diagnostics
        );
        {
            let f = unwrap_function(&source_file.declarations[0]);
            match &f.body {
                Expr::BoolLiteral(val, _) => assert!(*val),
                other => panic!("expected bool literal, got {:?}", other),
            }
        }
    }

    #[test]
    fn test_parse_bool_literal_false() {
        let (source_file, diagnostics) = parse("package a\n\nfunction main(): Unit = false");
        assert!(
            diagnostics.is_empty(),
            "unexpected errors: {:?}",
            diagnostics
        );
        {
            let f = unwrap_function(&source_file.declarations[0]);
            match &f.body {
                Expr::BoolLiteral(val, _) => assert!(!*val),
                other => panic!("expected bool literal, got {:?}", other),
            }
        }
    }

    #[test]
    fn test_parse_panic() {
        let (source_file, diagnostics) =
            parse("package a\n\nfunction main(): Unit = panic \"oops\"");
        assert!(
            diagnostics.is_empty(),
            "unexpected errors: {:?}",
            diagnostics
        );
        {
            let f = unwrap_function(&source_file.declarations[0]);
            match &f.body {
                Expr::Panic { message, .. } => match message.as_ref() {
                    Expr::StringLiteral(s, _) => assert_eq!(s, "oops"),
                    other => panic!("expected string literal, got {:?}", other),
                },
                other => panic!("expected panic expr, got {:?}", other),
            }
        }
    }

    #[test]
    fn test_parse_assert_without_message() {
        let (source_file, diagnostics) = parse("package a\n\nfunction main(): Unit = assert true");
        assert!(
            diagnostics.is_empty(),
            "unexpected errors: {:?}",
            diagnostics
        );
        {
            let f = unwrap_function(&source_file.declarations[0]);
            match &f.body {
                Expr::Assert {
                    condition, message, ..
                } => {
                    assert!(matches!(condition.as_ref(), Expr::BoolLiteral(true, _)));
                    assert!(message.is_none());
                }
                other => panic!("expected assert expr, got {:?}", other),
            }
        }
    }

    #[test]
    fn test_parse_assert_with_message() {
        let (source_file, diagnostics) =
            parse("package a\n\nfunction main(): Unit = assert true, \"ok\"");
        assert!(
            diagnostics.is_empty(),
            "unexpected errors: {:?}",
            diagnostics
        );
        {
            let f = unwrap_function(&source_file.declarations[0]);
            match &f.body {
                Expr::Assert {
                    condition, message, ..
                } => {
                    assert!(matches!(condition.as_ref(), Expr::BoolLiteral(true, _)));
                    match message.as_deref() {
                        Some(Expr::StringLiteral(s, _)) => assert_eq!(s, "ok"),
                        other => panic!("expected string message, got {:?}", other),
                    }
                }
                other => panic!("expected assert expr, got {:?}", other),
            }
        }
    }

    #[test]
    fn test_parse_let() {
        let (source_file, diagnostics) =
            parse("package a\n\nfunction main(): Unit =\n    let x = 5\n    ()");
        assert!(
            diagnostics.is_empty(),
            "unexpected errors: {:?}",
            diagnostics
        );
        {
            let f = unwrap_function(&source_file.declarations[0]);
            match &f.body {
                Expr::Block(block) => {
                    assert_eq!(block.expressions.len(), 2);
                    match &block.expressions[0] {
                        Expr::Let {
                            name,
                            mutable,
                            type_annotation,
                            value,
                            ..
                        } => {
                            assert_eq!(name.value, "x");
                            assert!(!mutable);
                            assert!(type_annotation.is_none());
                            assert!(matches!(value.as_ref(), Expr::Int32Literal(5, _)));
                        }
                        other => panic!("expected let expr, got {:?}", other),
                    }
                }
                other => panic!("expected block, got {:?}", other),
            }
        }
    }

    #[test]
    fn test_parse_let_mutable() {
        let (source_file, diagnostics) =
            parse("package a\n\nfunction main(): Unit =\n    let mutable x = 5\n    ()");
        assert!(
            diagnostics.is_empty(),
            "unexpected errors: {:?}",
            diagnostics
        );
        {
            let f = unwrap_function(&source_file.declarations[0]);
            match &f.body {
                Expr::Block(block) => match &block.expressions[0] {
                    Expr::Let { name, mutable, .. } => {
                        assert_eq!(name.value, "x");
                        assert!(*mutable);
                    }
                    other => panic!("expected let expr, got {:?}", other),
                },
                other => panic!("expected block, got {:?}", other),
            }
        }
    }

    #[test]
    fn test_parse_let_with_type() {
        let (source_file, diagnostics) =
            parse("package a\n\nfunction main(): Unit =\n    let x: Int32 = 5\n    ()");
        assert!(
            diagnostics.is_empty(),
            "unexpected errors: {:?}",
            diagnostics
        );
        {
            let f = unwrap_function(&source_file.declarations[0]);
            match &f.body {
                Expr::Block(block) => match &block.expressions[0] {
                    Expr::Let {
                        name,
                        type_annotation,
                        ..
                    } => {
                        assert_eq!(name.value, "x");
                        match type_annotation.as_ref().unwrap() {
                            TypeExpr::Named(n) => assert_eq!(n.name.value, "Int32"),
                            other => panic!("expected named type, got {:?}", other),
                        }
                    }
                    other => panic!("expected let expr, got {:?}", other),
                },
                other => panic!("expected block, got {:?}", other),
            }
        }
    }

    #[test]
    fn test_parse_identifier() {
        let (source_file, diagnostics) =
            parse("package a\n\nfunction main(): Unit =\n    let x = 5\n    x");
        assert!(
            diagnostics.is_empty(),
            "unexpected errors: {:?}",
            diagnostics
        );
        {
            let f = unwrap_function(&source_file.declarations[0]);
            match &f.body {
                Expr::Block(block) => {
                    assert_eq!(block.expressions.len(), 2);
                    match &block.expressions[1] {
                        Expr::Identifier(name, _) => assert_eq!(name, "x"),
                        other => panic!("expected identifier, got {:?}", other),
                    }
                }
                other => panic!("expected block, got {:?}", other),
            }
        }
    }

    #[test]
    fn test_parse_assignment() {
        let (source_file, diagnostics) = parse(
            "package a\n\nfunction main(): Unit =\n    let mutable x = 5\n    x = 10\n    ()",
        );
        assert!(
            diagnostics.is_empty(),
            "unexpected errors: {:?}",
            diagnostics
        );
        {
            let f = unwrap_function(&source_file.declarations[0]);
            match &f.body {
                Expr::Block(block) => {
                    assert_eq!(block.expressions.len(), 3);
                    match &block.expressions[1] {
                        Expr::Assignment { target, value, .. } => {
                            match target.as_ref() {
                                Expr::Identifier(name, _) => assert_eq!(name, "x"),
                                other => panic!("expected identifier target, got {:?}", other),
                            }
                            assert!(matches!(value.as_ref(), Expr::Int32Literal(10, _)));
                        }
                        other => panic!("expected assignment, got {:?}", other),
                    }
                }
                other => panic!("expected block, got {:?}", other),
            }
        }
    }

    #[test]
    fn test_parse_multi_line_function() {
        let (source_file, diagnostics) = parse("package a\n\nfunction main(): Unit =\n    ()");
        assert!(
            diagnostics.is_empty(),
            "unexpected errors: {:?}",
            diagnostics
        );
        assert_eq!(source_file.declarations.len(), 1);
        let f = unwrap_function(&source_file.declarations[0]);
        // Body should be a block containing a unit literal
        match &f.body {
            Expr::Block(block) => {
                assert_eq!(block.expressions.len(), 1);
                match &block.expressions[0] {
                    Expr::UnitLiteral(_) => {}
                    _ => panic!("expected unit literal in block"),
                }
            }
            _ => panic!("expected block expression for multi-line body"),
        }
    }

    #[test]
    fn test_parse_if_else() {
        let (source_file, diagnostics) =
            parse("package a\n\nfunction main(): Unit = if true then 1 else 2");
        assert!(
            diagnostics.is_empty(),
            "unexpected errors: {:?}",
            diagnostics
        );
        {
            let f = unwrap_function(&source_file.declarations[0]);
            match &f.body {
                Expr::If {
                    condition,
                    then_branch,
                    else_branch,
                    ..
                } => {
                    assert!(matches!(condition.as_ref(), Expr::BoolLiteral(true, _)));
                    assert!(matches!(then_branch.as_ref(), Expr::Int32Literal(1, _)));
                    assert!(else_branch.is_some());
                    assert!(matches!(
                        else_branch.as_deref().unwrap(),
                        Expr::Int32Literal(2, _)
                    ));
                }
                other => panic!("expected if expr, got {:?}", other),
            }
        }
    }

    #[test]
    fn test_parse_if_without_else() {
        let (source_file, diagnostics) =
            parse("package a\n\nfunction main(): Unit = if true then ()");
        assert!(
            diagnostics.is_empty(),
            "unexpected errors: {:?}",
            diagnostics
        );
        {
            let f = unwrap_function(&source_file.declarations[0]);
            match &f.body {
                Expr::If {
                    condition,
                    else_branch,
                    ..
                } => {
                    assert!(matches!(condition.as_ref(), Expr::BoolLiteral(true, _)));
                    assert!(else_branch.is_none());
                }
                other => panic!("expected if expr, got {:?}", other),
            }
        }
    }

    #[test]
    fn test_parse_else_if_chain() {
        let (source_file, diagnostics) = parse(
            "package a\n\nfunction main(): Unit = if true then 1 else if false then 2 else 3",
        );
        assert!(
            diagnostics.is_empty(),
            "unexpected errors: {:?}",
            diagnostics
        );
        {
            let f = unwrap_function(&source_file.declarations[0]);
            match &f.body {
                Expr::If {
                    else_branch: Some(else_br),
                    ..
                } => {
                    // The else branch should be another If expression
                    match else_br.as_ref() {
                        Expr::If {
                            condition,
                            else_branch: Some(inner_else),
                            ..
                        } => {
                            assert!(matches!(condition.as_ref(), Expr::BoolLiteral(false, _)));
                            assert!(matches!(inner_else.as_ref(), Expr::Int32Literal(3, _)));
                        }
                        other => panic!("expected nested if expr, got {:?}", other),
                    }
                }
                other => panic!("expected if expr with else, got {:?}", other),
            }
        }
    }

    // ── Import parsing tests ──────────────────────────────────────────

    #[test]
    fn test_parse_import_simple() {
        let (source_file, diagnostics) =
            parse("package a\n\nimport a.utils.func\n\nfunction main(): Unit = ()");
        assert!(diagnostics.is_empty(), "errors: {:?}", diagnostics);
        assert_eq!(source_file.imports.len(), 1);
        let imp = &source_file.imports[0];
        assert_eq!(imp.path.len(), 3);
        assert_eq!(imp.path[0].value, "a");
        assert_eq!(imp.path[1].value, "utils");
        assert_eq!(imp.path[2].value, "func");
        assert!(imp.alias.is_none());
    }

    #[test]
    fn test_parse_import_with_alias() {
        let (source_file, diagnostics) =
            parse("package a\n\nimport a.utils.func as f\n\nfunction main(): Unit = ()");
        assert!(diagnostics.is_empty(), "errors: {:?}", diagnostics);
        assert_eq!(source_file.imports.len(), 1);
        let imp = &source_file.imports[0];
        assert_eq!(imp.path.len(), 3);
        assert_eq!(imp.alias.as_ref().unwrap().value, "f");
    }

    #[test]
    fn test_parse_multiple_imports() {
        let (source_file, diagnostics) = parse(
            "package a\n\nimport a.utils.func\nimport a.other.thing as t\n\nfunction main(): Unit = ()",
        );
        assert!(diagnostics.is_empty(), "errors: {:?}", diagnostics);
        assert_eq!(source_file.imports.len(), 2);
        assert_eq!(source_file.imports[0].path[2].value, "func");
        assert_eq!(source_file.imports[1].alias.as_ref().unwrap().value, "t");
    }

    // ── Postfix expression parsing tests ──────────────────────────────

    #[test]
    fn test_parse_field_access() {
        let (source_file, diagnostics) = parse("package a\n\nfunction main(): Unit = x.field");
        assert!(diagnostics.is_empty(), "errors: {:?}", diagnostics);
        {
            let f = unwrap_function(&source_file.declarations[0]);
            match &f.body {
                Expr::FieldAccess { object, field, .. } => {
                    assert!(matches!(object.as_ref(), Expr::Identifier(n, _) if n == "x"));
                    assert_eq!(field.value, "field");
                }
                other => panic!("expected field access, got {:?}", other),
            }
        }
    }

    #[test]
    fn test_parse_method_call() {
        let (source_file, diagnostics) =
            parse("package a\n\nfunction main(): Unit = utils.func(1, 2)");
        assert!(diagnostics.is_empty(), "errors: {:?}", diagnostics);
        {
            let f = unwrap_function(&source_file.declarations[0]);
            match &f.body {
                Expr::MethodCall {
                    receiver,
                    method,
                    args,
                    ..
                } => {
                    assert!(matches!(receiver.as_ref(), Expr::Identifier(n, _) if n == "utils"));
                    assert_eq!(method.value, "func");
                    assert_eq!(args.len(), 2);
                }
                other => panic!("expected method call, got {:?}", other),
            }
        }
    }

    #[test]
    fn test_parse_chained_postfix() {
        let (source_file, diagnostics) =
            parse("package a\n\nfunction main(): Unit = a.utils.func(1)");
        assert!(diagnostics.is_empty(), "errors: {:?}", diagnostics);
        {
            let f = unwrap_function(&source_file.declarations[0]);
            match &f.body {
                Expr::MethodCall {
                    receiver,
                    method,
                    args,
                    ..
                } => {
                    assert_eq!(method.value, "func");
                    assert_eq!(args.len(), 1);
                    // Receiver should be FieldAccess(Identifier("a"), "utils")
                    match receiver.as_ref() {
                        Expr::FieldAccess { object, field, .. } => {
                            assert!(matches!(object.as_ref(), Expr::Identifier(n, _) if n == "a"));
                            assert_eq!(field.value, "utils");
                        }
                        other => panic!("expected field access as receiver, got {:?}", other),
                    }
                }
                other => panic!("expected method call, got {:?}", other),
            }
        }
    }

    // ── Try / orReturn parsing tests ────────────────────────────────────

    #[test]
    fn test_parse_try_prefix() {
        let (source_file, diagnostics) = parse("package a\n\nfunction main(): Unit = try foo()");
        assert!(diagnostics.is_empty(), "errors: {:?}", diagnostics);
        let f = unwrap_function(&source_file.declarations[0]);
        match &f.body {
            Expr::Try { operand, .. } => {
                assert!(
                    matches!(operand.as_ref(), Expr::FunctionCall { name, .. } if name.value == "foo")
                );
            }
            other => panic!("expected Try, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_or_return_postfix() {
        let (source_file, diagnostics) =
            parse("package a\n\nfunction main(): Unit = foo().orReturn");
        assert!(diagnostics.is_empty(), "errors: {:?}", diagnostics);
        let f = unwrap_function(&source_file.declarations[0]);
        match &f.body {
            Expr::OrReturn { operand, .. } => {
                assert!(
                    matches!(operand.as_ref(), Expr::FunctionCall { name, .. } if name.value == "foo")
                );
            }
            other => panic!("expected OrReturn, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_try_chained() {
        // `try foo().bar()` should parse as `try (foo().bar())`
        let (source_file, diagnostics) =
            parse("package a\n\nfunction main(): Unit = try foo().bar()");
        assert!(diagnostics.is_empty(), "errors: {:?}", diagnostics);
        let f = unwrap_function(&source_file.declarations[0]);
        match &f.body {
            Expr::Try { operand, .. } => {
                assert!(
                    matches!(operand.as_ref(), Expr::MethodCall { method, .. } if method.value == "bar")
                );
            }
            other => panic!("expected Try wrapping MethodCall, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_or_return_chained() {
        // `foo().bar().orReturn` should parse as `OrReturn { operand: MethodCall { ... } }`
        let (source_file, diagnostics) =
            parse("package a\n\nfunction main(): Unit = foo().bar().orReturn");
        assert!(diagnostics.is_empty(), "errors: {:?}", diagnostics);
        let f = unwrap_function(&source_file.declarations[0]);
        match &f.body {
            Expr::OrReturn { operand, .. } => {
                assert!(
                    matches!(operand.as_ref(), Expr::MethodCall { method, .. } if method.value == "bar")
                );
            }
            other => panic!("expected OrReturn wrapping MethodCall, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_two_identifiers_in_a_row_error() {
        // `var x = 0` should produce a clear error about unexpected identifier
        let (_, diagnostics) =
            parse("package a\n\nfunction main(): Unit =\n    var x = 0\n    ()");
        assert!(
            diagnostics.iter().any(|d| d.message.contains("unexpected identifier after 'var'")),
            "expected 'unexpected identifier' error, got: {:?}",
            diagnostics
        );
    }

    #[test]
    fn test_parse_if_then_same_line_else_next_line() {
        // `if cond then expr \n else \n block` should parse without errors
        let source = r#"
package a

function main(): Unit =
    let n = 0
    if n == 0 then "a"
    else
        let x = "b"
        x
"#;
        let (_, diagnostics) = parse(source);
        assert!(
            diagnostics.is_empty(),
            "unexpected errors: {:?}",
            diagnostics
        );
    }

    #[test]
    fn private_type_declarations() {
        let (file, diagnostics) = parse(
            r#"
package a

public newtype Amount<T> private where T: Equatable = T

public record Box<T> private where T: Equatable =
    item: T

public enum Choice<T> private where T: Equatable =
    Item(T)

record Token private
"#,
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let Declaration::Newtype(amount) = &file.declarations[0] else {
            panic!("expected newtype")
        };
        assert!(amount.inner_private);
        assert_eq!(amount.visibility, Visibility::Public);
        assert_eq!(amount.where_clause.len(), 1);
        let record = unwrap_record(&file.declarations[1]);
        assert!(record.construction_private);
        assert_eq!(record.visibility, Visibility::Public);
        assert_eq!(record.where_clause.len(), 1);
        let enumeration = unwrap_enum(&file.declarations[2]);
        assert!(enumeration.construction_private);
        assert_eq!(enumeration.visibility, Visibility::Public);
        assert_eq!(enumeration.where_clause.len(), 1);
        let token = unwrap_record(&file.declarations[3]);
        assert!(token.construction_private);
        assert!(token.fields.is_empty());
    }

    #[test]
    fn empty_records_preserve_following_private_declarations() {
        for header in ["record Token", "record Token<T>"] {
            for separator in [" ", "\n"] {
                let (file, diagnostics) = parse(&format!(r#"
package a
{header}
private{separator}function secret(): Int32 = 1
"#));
                assert!(diagnostics.is_empty(), "{diagnostics:?}");
                assert!(!unwrap_record(&file.declarations[0]).construction_private);
                assert_eq!(unwrap_function(&file.declarations[1]).visibility, Visibility::Private);
            }
        }
        let (file, diagnostics) = parse(r#"
package a
record Token
private record Hidden
record Permit private
private function secret(): Int32 = 1
"#);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert!(!unwrap_record(&file.declarations[0]).construction_private);
        assert_eq!(unwrap_record(&file.declarations[1]).visibility, Visibility::Private);
        assert!(unwrap_record(&file.declarations[2]).construction_private);
        assert_eq!(unwrap_function(&file.declarations[3]).visibility, Visibility::Private);
    }

    #[test]
    fn record_private_modifiers_can_continue_a_header() {
        let (file, diagnostics) = parse(r#"
package a
record Account
    private =
        balance: Int32

record Box<T>
    private where T: Equatable =
        item: T
"#);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert!(unwrap_record(&file.declarations[0]).construction_private);
        assert!(unwrap_record(&file.declarations[1]).construction_private);
    }

    #[test]
    fn legacy_newtype_privacy_position_is_rejected() {
        let (_, diagnostics) = parse(r#"
package a

newtype Amount = private Int32
"#);
        assert!(
            diagnostics.iter().any(|d| d.message.contains("place 'private' after the type name")),
            "{diagnostics:?}",
        );
    }

    fn unwrap_record(decl: &Declaration) -> &RecordDecl {
        match decl {
            Declaration::Record(r) => r,
            _ => panic!("expected record declaration"),
        }
    }

    fn unwrap_enum(decl: &Declaration) -> &EnumDecl {
        match decl {
            Declaration::Enum(e) => e,
            _ => panic!("expected enum declaration"),
        }
    }

    #[test]
    fn test_parse_derive_on_record() {
        let (source_file, diagnostics) = parse(
            "package a\n\n@derive(Equatable)\npublic record Point =\n    x: Int32\n    y: Int32\n",
        );
        assert!(diagnostics.is_empty(), "unexpected errors: {:?}", diagnostics);
        let r = unwrap_record(&source_file.declarations[0]);
        assert_eq!(r.attributes.len(), 1);
        assert_eq!(r.attributes[0].macro_name.len(), 1);
        assert_eq!(r.attributes[0].macro_name[0].value, "Equatable");
    }

    #[test]
    fn test_parse_derive_on_enum() {
        let (source_file, diagnostics) = parse(
            "package a\n\n@derive(Equatable)\npublic enum Shape =\n    Circle\n    Square\n",
        );
        assert!(diagnostics.is_empty(), "unexpected errors: {:?}", diagnostics);
        let e = unwrap_enum(&source_file.declarations[0]);
        assert_eq!(e.attributes.len(), 1);
        assert_eq!(e.attributes[0].macro_name[0].value, "Equatable");
    }

    #[test]
    fn test_parse_multiple_derive() {
        let (source_file, diagnostics) = parse(
            "package a\n\n@derive(Equatable)\n@derive(Hashable)\nrecord Foo = x: Int32\n",
        );
        assert!(diagnostics.is_empty(), "unexpected errors: {:?}", diagnostics);
        let r = unwrap_record(&source_file.declarations[0]);
        assert_eq!(r.attributes.len(), 2);
        assert_eq!(r.attributes[0].macro_name[0].value, "Equatable");
        assert_eq!(r.attributes[1].macro_name[0].value, "Hashable");
    }

    #[test]
    fn test_parse_derive_qualified_name() {
        let (source_file, diagnostics) = parse(
            "package a\n\n@derive(json.JsonCodec)\nrecord Foo = x: Int32\n",
        );
        assert!(diagnostics.is_empty(), "unexpected errors: {:?}", diagnostics);
        let r = unwrap_record(&source_file.declarations[0]);
        assert_eq!(r.attributes.len(), 1);
        let path: Vec<_> = r.attributes[0]
            .macro_name
            .iter()
            .map(|s| s.value.as_str())
            .collect();
        assert_eq!(path, vec!["json", "JsonCodec"]);
    }

    #[test]
    fn test_parse_derive_on_non_record_errors() {
        let (_, diagnostics) = parse("package a\n\n@derive(Equatable)\nfunction foo(): Unit = ()\n");
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("@derive(...) can only appear on a record or enum")),
            "expected @derive misuse error, got {:?}",
            diagnostics
        );
    }
    #[test]
    fn async_do_layout_and_nesting() {
        let (source, diagnostics) = parse(r#"
package a

function program() =
    let first = async do
        let value = await fetch()
        value + 1
    consume(async do 42)
    async do
        let nested = async do await fetch()
        await nested

function after(): Unit = ()
"#);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(source.declarations.len(), 2);
        let function = unwrap_function(&source.declarations[0]);
        let Expr::Block(block) = &function.body else { panic!("expected block") };
        assert_eq!(block.expressions.len(), 3);
        assert!(matches!(block.expressions.last(), Some(Expr::AsyncDo { .. })));
    }

    #[test]
    fn async_do_requires_a_body() {
        let (_, diagnostics) = parse(r#"
package a

function program() = async do
"#);
        assert!(!diagnostics.is_empty());
    }

    #[test]
    fn async_without_do_still_requires_a_closure() {
        let (_, diagnostics) = parse(r#"
package a

function program() = async 42
"#);
        assert!(diagnostics.iter().any(|error| error.message.contains("expected closure")));
    }

}
