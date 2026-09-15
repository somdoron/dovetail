//! Name projection: WIT kebab-case identifiers → Dovetail camelCase/PascalCase.

/// `column-int64` → `columnInt64`
pub fn camel(wit_name: &str) -> String {
    let mut out = String::with_capacity(wit_name.len());
    let mut upper_next = false;
    for c in wit_name.chars() {
        if c == '-' {
            upper_next = true;
        } else if upper_next {
            out.extend(c.to_uppercase());
            upper_next = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// `sqlite-raw` → `SqliteRaw`
pub fn pascal(wit_name: &str) -> String {
    let camel = camel(wit_name);
    let mut chars = camel.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => camel,
    }
}

/// Dovetail reserved words that could collide with projected WIT identifiers
/// in expression/parameter position. Colliding names get a trailing `_`
/// stripped... no — Dovetail has no trailing-underscore idiom, so we append
/// `Value` to keep the identifier legal and readable.
const RESERVED: &[&str] = &[
    "and",
    "assert",
    "async",
    "await",
    "begin",
    "case",
    "class",
    "else",
    "end",
    "enum",
    "extension",
    "false",
    "for",
    "function",
    "if",
    "implement",
    "import",
    "in",
    "internal",
    "intrinsic",
    "let",
    "match",
    "module",
    "mut",
    "newtype",
    "not",
    "or",
    "override",
    "package",
    "panic",
    "private",
    "property",
    "public",
    "record",
    "return",
    "self",
    "singleton",
    "static",
    "test",
    "then",
    "trait",
    "true",
    "type",
    "virtual",
    "when",
    "where",
    "while",
    "with",
    "do",
    "yield",
    "defer",
    "macro",
    "interface",
];

/// Project a WIT parameter/field name to a legal Dovetail identifier.
pub fn safe_camel(wit_name: &str) -> String {
    let name = camel(wit_name);
    if RESERVED.contains(&name.as_str()) {
        format!("{name}Value")
    } else {
        name
    }
}
