//! Body-free source declarations captured before macro expansion.
use super::types::*;
use crate::common::{span::Span, types::Visibility};
use crate::parser::ast::*;
use crate::typechecker::{registry::Registry, types::TypedModule};

const INFERRED: &str = "/* inferred type unavailable */";

#[derive(Debug, Clone)]
pub struct DeclarationView {
    pub name: String,
    pub kind: &'static str,
    pub header: String,
    pub docs: Option<String>,
    pub span: Span,
    pub visibility: Visibility,
    pub children: Vec<DeclarationView>,
    pub block: bool,
    pub target: Option<String>,
    pub target_spelling: Option<String>,
    pub generated: bool,
    pub package: String,
    pub imports: Vec<String>,
}

impl DeclarationView {
    fn new(
        name: &str,
        kind: &'static str,
        header: String,
        span: &Span,
        visibility: Visibility,
        docs: &Option<String>,
    ) -> Self {
        Self {
            name: name.to_owned(),
            kind,
            header,
            docs: docs.clone(),
            span: span.clone(),
            visibility,
            children: Vec::new(),
            block: false,
            target: None,
            target_spelling: None,
            generated: false,
            package: String::new(),
            imports: Vec::new(),
        }
    }

    pub(super) fn visible(&self, all: bool) -> bool {
        all || matches!(self.visibility, Visibility::Public | Visibility::Protected)
    }

    pub(super) fn render(&self, all: bool, indent: usize, output: &mut String) {
        let prefix = "    ".repeat(indent);
        if let Some(docs) = &self.docs {
            for line in docs.lines() {
                output.push_str(&format!("{prefix}/// {line}\n"));
            }
        }
        if self.generated {
            output.push_str(&format!("{prefix}// Generated declaration\n"));
        }
        for line in self.header.lines() {
            output.push_str(&format!("{prefix}{line}\n"));
        }
        for child in self.children.iter().filter(|c| c.visible(all)) {
            child.render(all, indent + 1, output);
        }
        if self.kind == "case" && !self.children.is_empty() {
            output.push_str(&format!("{prefix}}}\n"));
        }
    }

    fn context(&mut self, package: &str, imports: &[String]) {
        self.package = package.to_owned();
        self.imports = imports.to_vec();
        for child in &mut self.children {
            child.context(package, imports);
        }
    }
}

fn visibility(v: Visibility) -> &'static str {
    match v {
        Visibility::Public => "public ",
        Visibility::Private => "private ",
        Visibility::Protected => "protected ",
        Visibility::Internal => "",
    }
}

fn modifiers(override_: bool, final_: bool, abstract_: bool) -> String {
    format!(
        "{}{}{}",
        if override_ { "override " } else { "" },
        if final_ { "final " } else { "" },
        if abstract_ { "abstract " } else { "" }
    )
}

fn attributes(derives: &[DeriveAttribute], string_literal: bool) -> String {
    let mut text = String::new();
    if string_literal {
        text.push_str("@stringLiteral\n");
    }
    for attr in derives {
        text.push_str(&format!(
            "@derive({})\n",
            attr.macro_name
                .iter()
                .map(|n| n.value.as_str())
                .collect::<Vec<_>>()
                .join(".")
        ));
    }
    text
}

fn function(f: &FunctionDecl) -> DeclarationView {
    DeclarationView::new(
        &f.name.value,
        "function",
        format!(
            "{}{}{}function {}{}({}): {}{}",
            visibility(f.visibility),
            modifiers(f.is_override, f.is_final, f.is_abstract),
            if f.is_async { "async " } else { "" },
            f.name.value,
            parameters(&f.type_params),
            params(&f.params),
            f.return_type
                .as_ref()
                .map(source_type)
                .unwrap_or_else(|| "Unit".to_owned()),
            bounds(&f.where_clause)
        ),
        &f.span,
        f.visibility,
        &f.doc_comment,
    )
}

fn property(p: &PropertyDecl) -> DeclarationView {
    DeclarationView::new(
        &p.name.value,
        "property",
        format!(
            "{}{}property {}{}{}: {}",
            visibility(p.visibility),
            modifiers(p.is_override, p.is_final, p.is_abstract),
            p.name.value,
            parameters(&p.type_params),
            if p.params.is_empty() {
                String::new()
            } else {
                format!("({})", params(&p.params))
            },
            source_type(&p.return_type)
        ),
        &p.span,
        p.visibility,
        &p.doc_comment,
    )
}

fn global(g: &GlobalVarDecl) -> DeclarationView {
    DeclarationView::new(
        &g.name.value,
        "global",
        format!(
            "{}let {}{}: {}",
            visibility(g.visibility),
            if g.mutable { "mutable " } else { "" },
            g.name.value,
            g.type_annotation
                .as_ref()
                .map(source_type)
                .unwrap_or_else(|| INFERRED.to_owned())
        ),
        &g.span,
        g.visibility,
        &g.doc_comment,
    )
}

fn field(f: &RecordField) -> DeclarationView {
    DeclarationView::new(
        &f.name.value,
        "field",
        format!("{}: {}", f.name.value, source_type(&f.type_annotation)),
        &f.span,
        Visibility::Public,
        &f.doc_comment,
    )
}

fn members(methods: &[FunctionDecl], properties: &[PropertyDecl]) -> Vec<DeclarationView> {
    methods
        .iter()
        .map(function)
        .chain(properties.iter().map(property))
        .collect()
}

fn record(r: &RecordDecl) -> DeclarationView {
    let mut view = DeclarationView::new(
        &r.name.value,
        "record",
        format!(
            "{}{}record {}{}{}{} =",
            attributes(&r.attributes, r.string_literal.is_some()),
            visibility(r.visibility),
            r.name.value,
            variant_parameters(&r.type_params),
            if r.construction_private {
                " private"
            } else {
                ""
            },
            bounds(&r.where_clause)
        ),
        &r.span,
        r.visibility,
        &r.doc_comment,
    );
    view.block = true;
    view.children = r.fields.iter().map(field).collect();
    view
}

fn enumeration(e: &EnumDecl) -> DeclarationView {
    let mut view = DeclarationView::new(
        &e.name.value,
        "enum",
        format!(
            "{}{}enum {}{}{}{} =",
            attributes(&e.attributes, false),
            visibility(e.visibility),
            e.name.value,
            variant_parameters(&e.type_params),
            if e.construction_private {
                " private"
            } else {
                ""
            },
            bounds(&e.where_clause)
        ),
        &e.span,
        e.visibility,
        &e.doc_comment,
    );
    view.block = true;
    view.children = e
        .variants
        .iter()
        .map(|v| {
            let payload = match &v.payload {
                EnumVariantPayload::None => String::new(),
                EnumVariantPayload::Tuple(types) => format!(
                    "({})",
                    types.iter().map(source_type).collect::<Vec<_>>().join(", ")
                ),
                EnumVariantPayload::Record(fields) => format!(
                    " {{ {} }}",
                    fields
                        .iter()
                        .map(|f| format!("{}: {}", f.name.value, source_type(&f.type_annotation)))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            };
            let mut case = DeclarationView::new(
                &v.name.value,
                "case",
                format!("{}{payload}", v.name.value),
                &v.span,
                Visibility::Public,
                &v.doc_comment,
            );
            if let EnumVariantPayload::Record(fields) = &v.payload
                && !fields.is_empty()
            {
                case.header = format!("{} {{", v.name.value);
                case.children = fields.iter().map(field).collect();
            }
            case
        })
        .collect();
    view
}

fn trait_view(t: &TraitDecl) -> DeclarationView {
    let supers = if t.supers.is_empty() {
        String::new()
    } else {
        format!(
            " extends {}",
            t.supers
                .iter()
                .map(|s| source_type(&TypeExpr::Named(s.clone())))
                .collect::<Vec<_>>()
                .join(" and ")
        )
    };
    let kind = if t.is_interface { "interface" } else { "trait" };
    let mut view = DeclarationView::new(
        &t.name.value,
        kind,
        format!(
            "{}{kind} {}{}{supers} =",
            visibility(t.visibility),
            t.name.value,
            parameters(&t.type_params)
        ),
        &t.span,
        t.visibility,
        &t.doc_comment,
    );
    view.block = true;
    view.children.extend(t.associated_types.iter().map(|a| {
        DeclarationView::new(
            &a.name.value,
            "associated type",
            format!("type {}{}", a.name.value, parameters(&a.type_params)),
            &a.span,
            Visibility::Public,
            &a.doc_comment,
        )
    }));
    view.children.extend(t.methods.iter().map(|m| {
        DeclarationView::new(
            &m.name.value,
            "function",
            format!(
                "function {}{}({}): {}{}{}",
                m.name.value,
                parameters(&m.type_params),
                params(&m.params),
                m.return_type
                    .as_ref()
                    .map(source_type)
                    .unwrap_or_else(|| "Unit".to_owned()),
                bounds(&m.where_clause),
                if m.body.is_some() {
                    " // default implementation"
                } else {
                    ""
                }
            ),
            &m.span,
            Visibility::Public,
            &m.doc_comment,
        )
    }));
    view.children.extend(t.properties.iter().map(|p| {
        let mut v = property(p);
        v.visibility = Visibility::Public;
        if p.body.is_some() {
            v.header.push_str(" // default implementation");
        }
        v
    }));
    view
}

fn class(c: &ClassDecl) -> DeclarationView {
    let params = c
        .params
        .iter()
        .map(|p| {
            format!(
                "{}{}{}: {}",
                visibility(p.visibility),
                if p.mutable { "mutable " } else { "" },
                p.name.value,
                source_type(&p.type_annotation)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let extends = c
        .extends
        .as_ref()
        .map(|e| format!(" extends {}", source_type(&e.parent_type)))
        .unwrap_or_default();
    let implements = if c.implements.is_empty() {
        String::new()
    } else {
        format!(
            " implements {}",
            c.implements
                .iter()
                .map(source_type)
                .collect::<Vec<_>>()
                .join(" and ")
        )
    };
    let mut view = DeclarationView::new(
        &c.name.value,
        "class",
        format!(
            "{}{}{}{}class {}{} {}({params}){extends}{implements}{} =",
            attributes(&[], c.string_literal.is_some()),
            visibility(c.visibility),
            modifiers(false, c.is_final, c.is_abstract),
            if c.is_sealed { "sealed " } else { "" },
            c.name.value,
            variant_parameters(&c.type_params),
            visibility(c.constructor_visibility),
            bounds(&c.where_clause)
        ),
        &c.span,
        c.visibility,
        &c.doc_comment,
    );
    view.block = true;
    view.children = c
        .body
        .iter()
        .filter_map(|m| match m {
            ClassMember::Method(f) => Some(function(f)),
            ClassMember::Property(p) => Some(property(p)),
            ClassMember::Expression(_) => None,
            ClassMember::LetBinding(f) => Some(DeclarationView::new(
                &f.name.value,
                "field",
                format!(
                    "{}let {}{}{}: {}",
                    visibility(f.visibility),
                    if f.is_static { "static " } else { "" },
                    if f.mutable { "mutable " } else { "" },
                    f.name.value,
                    f.type_annotation
                        .as_ref()
                        .map(source_type)
                        .unwrap_or_else(|| INFERRED.to_owned())
                ),
                &f.span,
                f.visibility,
                &f.doc_comment,
            )),
        })
        .collect();
    view.children.extend(c.params.iter().map(|p| {
        DeclarationView::new(
            &p.name.value,
            "constructor field",
            format!(
                "// Constructor field: {}{}{}: {}",
                visibility(p.visibility),
                if p.mutable { "mutable " } else { "" },
                p.name.value,
                source_type(&p.type_annotation)
            ),
            &p.span,
            p.visibility,
            &p.doc_comment,
        )
    }));
    view
}

fn declaration(d: &Declaration) -> Option<DeclarationView> {
    Some(match d {
        Declaration::Function(f) => function(f),
        Declaration::GlobalVar(g) => global(g),
        Declaration::Record(r) => record(r),
        Declaration::Enum(e) => enumeration(e),
        Declaration::Trait(t) => trait_view(t),
        Declaration::Class(c) => class(c),
        Declaration::TypeAlias(a) => DeclarationView::new(
            &a.name.value,
            "alias",
            format!(
                "{}{}type {}{}{} = {}",
                attributes(&[], a.string_literal.is_some()),
                visibility(a.visibility),
                a.name.value,
                parameters(&a.type_params),
                bounds(&a.where_clause),
                source_type(&a.type_expr)
            ),
            &a.span,
            a.visibility,
            &a.doc_comment,
        ),
        Declaration::Newtype(n) => DeclarationView::new(
            &n.name.value,
            "newtype",
            format!(
                "{}{}{} {}{}{}{} = {}",
                attributes(&n.attributes, false),
                visibility(n.visibility),
                if n.intrinsic { "type" } else { "newtype" },
                n.name.value,
                variant_parameters(&n.type_params),
                if n.inner_private && !n.intrinsic {
                    " private"
                } else {
                    ""
                },
                bounds(&n.where_clause),
                if n.intrinsic {
                    "intrinsic".to_owned()
                } else {
                    source_type(&n.inner_type)
                }
            ),
            &n.span,
            n.visibility,
            &n.doc_comment,
        ),
        Declaration::Module(m) => {
            let mut v = DeclarationView::new(
                &m.name.value,
                "module",
                format!("module {}{} =", m.name.value, parameters(&m.type_params)),
                &m.span,
                Visibility::Public,
                &m.doc_comment,
            );
            v.block = true;
            v.children = members(&m.functions, &m.properties);
            v.children.extend(m.globals.iter().map(global));
            v
        }
        Declaration::Extension(e) => {
            let mut v = DeclarationView::new(
                &e.name.value,
                "extension",
                format!(
                    "extension {}{} for {}{} =",
                    e.name.value,
                    parameters(&e.type_params),
                    source_type(&e.for_type),
                    bounds(&e.where_clause)
                ),
                &e.span,
                Visibility::Public,
                &e.doc_comment,
            );
            v.block = true;
            v.children = members(&e.methods, &e.properties);
            v
        }
        Declaration::Implement(i) => {
            let name = format!(
                "implement {} for {}",
                i.trait_name.value,
                source_type(&i.for_type)
            );
            let mut v = DeclarationView::new(
                &name,
                "implementation",
                format!(
                    "implement{} {}{} for {}{} =",
                    parameters(&i.type_params),
                    i.trait_name.value,
                    arguments(i.trait_type_args.iter().map(source_type)),
                    source_type(&i.for_type),
                    bounds(&i.where_clause)
                ),
                &i.span,
                Visibility::Public,
                &i.doc_comment,
            );
            v.block = true;
            v.target_spelling = Some(source_type(&i.for_type));
            v.children = members(&i.methods, &i.properties);
            v.children.extend(i.associated_types.iter().map(|a| {
                DeclarationView::new(
                    &a.name.value,
                    "associated type",
                    format!(
                        "type {}{} = {}",
                        a.name.value,
                        parameters(&a.type_params),
                        source_type(&a.type_expr)
                    ),
                    &a.span,
                    Visibility::Public,
                    &None,
                )
            }));
            v
        }
        Declaration::Test(_) => return None,
    })
}

pub(crate) fn capture_file(file: &SourceFile) -> Vec<DeclarationView> {
    let package = file
        .package
        .path
        .iter()
        .map(|p| p.value.as_str())
        .collect::<Vec<_>>()
        .join(".");
    let imports = file
        .imports
        .iter()
        .map(|i| {
            format!(
                "import {}{}",
                i.path
                    .iter()
                    .map(|p| p.value.as_str())
                    .collect::<Vec<_>>()
                    .join("."),
                i.alias
                    .as_ref()
                    .map(|a| format!(" as {}", a.value))
                    .unwrap_or_default()
            )
        })
        .collect::<Vec<_>>();
    file.declarations
        .iter()
        .filter_map(declaration)
        .map(|mut v| {
            if v.block && v.children.is_empty() {
                v.header = v.header.trim_end_matches(" =").to_owned();
            }
            v.context(&package, &imports);
            v
        })
        .collect()
}

pub(crate) fn capture(package: &PackageAst) -> Vec<DeclarationView> {
    package.files.iter().flat_map(capture_file).collect()
}

/// Macro expansion consumes attributes. Keep pre-expansion views and append only
/// declarations introduced by expansion, retaining their source provenance.
pub(crate) fn add_generated(views: &mut Vec<DeclarationView>, package: &PackageAst) {
    for mut candidate in capture(package) {
        if !views.iter().any(|v| {
            v.kind == candidate.kind && v.name == candidate.name && v.span == candidate.span
        }) {
            candidate.generated = true;
            views.push(candidate);
        }
    }
}

pub(crate) fn enrich(
    views: &mut [DeclarationView],
    registry: &Registry,
    module: &TypedModule,
    reliable: bool,
) {
    for view in views {
        enrich_container(view, registry, module, reliable);
        enrich(view.children.as_mut_slice(), registry, module, reliable);
        if view.kind == "implementation" {
            if let Some(i) = registry
                .all_implement_blocks()
                .iter()
                .filter(|i| i.span == view.span)
                .find(|i| {
                    view.target_spelling.as_ref().is_none_or(|target| {
                        target == &i.for_type.to_string() || target == &resolved_type(&i.for_type)
                    }) || registry
                        .all_implement_blocks()
                        .iter()
                        .filter(|other| other.span == view.span)
                        .count()
                        == 1
                })
            {
                view.target = Some(i.type_fqn.to_string());
                for method in i
                    .methods
                    .iter()
                    .chain(&i.properties)
                    .filter(|m| m.is_default)
                {
                    view.header.push_str(&format!(
                        "\n    // {} supplied by {}",
                        method.name, i.trait_fqn
                    ));
                }
            }
        } else if view.kind == "extension"
            && let Some(e) = registry.all_extension_blocks().iter().find(|e| {
                e.ext_fqn.to_string() == format!("{}.{}", view.package, view.name)
                    && e.source_file == view.span.file
            })
        {
            view.target = (!e.for_type.contains_error())
                .then(|| e.for_type.try_to_fqn())
                .flatten()
                .map(|fqn| fqn.to_string());
        }
        if !reliable || !view.header.contains(INFERRED) {
            continue;
        }
        let inferred = inferred_type(view, registry, module);
        if let Some(ty) = inferred.filter(|ty| !ty.contains_error()) {
            view.header = view.header.replace(INFERRED, &resolved_type(ty));
        }
    }
}

fn inferred_type<'a>(
    view: &DeclarationView,
    registry: &'a Registry,
    module: &'a TypedModule,
) -> Option<&'a crate::typechecker::types::Type> {
    if view.kind != "global" && view.kind != "field" {
        return None;
    }
    module
        .globals
        .values()
        .find(|g| g.span == view.span)
        .map(|g| &g.ty)
        .or_else(|| {
            registry
                .all_modules()
                .flat_map(|(_, m)| m.generic_globals.iter())
                .find(|(name, g)| name.0 == view.name && contains_span(&view.span, &g.body.span()))
                .map(|(_, g)| &g.ty)
        })
}

fn enrich_container(
    view: &mut DeclarationView,
    registry: &Registry,
    module: &TypedModule,
    reliable: bool,
) {
    if view.kind == "module"
        && let Some((_, module)) = registry
            .all_modules()
            .find(|(fqn, _)| fqn.to_string() == format!("{}.{}", view.package, view.name))
    {
        let bounds = resolved_bounds(&module.trait_bounds);
        if !bounds.is_empty() {
            view.header
                .push_str(&format!("\n    // Enclosing type bounds: {bounds}"));
        }
    }
    if view.kind == "class"
        && let Some((_, class)) = registry
            .all_class_types()
            .find(|(_, c)| c.span == view.span)
    {
        if let Some(parent) = &class.parent_class {
            view.header.push_str(&format!(
                "\n    // Inherited members: dovetail query definition {parent}"
            ));
        }
        for (name, origin) in &class.default_supplied_members {
            view.header
                .push_str(&format!("\n    // {name} supplied by {origin}"));
        }
        if reliable {
            let typed_class = module.types.values().find_map(|ty| match ty {
                crate::typechecker::types::TypeDef::Class(c) if c.span == view.span => Some(c),
                _ => None,
            });
            for child in &mut view.children {
                if child.kind != "field" {
                    continue;
                }
                let ty = match typed_class {
                    Some(c) => c
                        .fields
                        .iter()
                        .find(|f| f.name == child.name && f.declared_by == c.fqn)
                        .map(|f| &f.ty),
                    None => class
                        .fields
                        .iter()
                        .find(|f| f.name == child.name)
                        .map(|f| &f.ty),
                };
                if let Some(ty) = ty.filter(|ty| !ty.contains_error()) {
                    child.header = child.header.replace(INFERRED, &resolved_type(ty));
                }
            }
        }
    }
    if matches!(view.kind, "trait" | "interface")
        && let Some((_, trait_)) = registry.all_traits().find(|(fqn, t)| {
            fqn.to_string() == format!("{}.{}", view.package, view.name)
                && t.source_file == view.span.file
        })
    {
        for parent in &trait_.supers {
            let parent = &parent.fqn;
            view.header.push_str(&format!(
                "\n    // Inherited members: dovetail query definition {parent}"
            ));
        }
    }
}

fn contains_span(outer: &Span, inner: &Span) -> bool {
    outer.file == inner.file
        && (outer.line, outer.column) <= (inner.line, inner.column)
        && (outer.end_line, outer.end_column) >= (inner.end_line, inner.end_column)
}
