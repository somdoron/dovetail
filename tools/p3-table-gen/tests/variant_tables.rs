//! The hand-maintained error/descriptor variant tables must match the WIT.
//!
//! `wasi_marshaling.rs` keeps four case-name tables (NetworkError, DnsError,
//! FileSystemError, DescriptorType) plus the discriminant of each `other`
//! case, and codegen lifts host discriminants through them by position. The
//! import table gets its drift protection from `no_drift.rs`; these tables had
//! none — and the WIT is at an `-rc-` version that has already reordered
//! `descriptor-type` once. So, mirroring `no_drift`, this test derives the
//! expected case order from the same vendored WIT (via wit-parser) and checks
//! it against the tables as they appear in the compiler source text.

use wit_parser::{PackageId, PackageName, Resolve, TypeDefKind};

use dovetail::p3::{P3_VERSION, WIT_FILES};

const WASI_MARSHALING_SRC: &str = include_str!(
    "../../../dovetail/src/compiler/codegen/function_emitter/wasi_marshaling.rs"
);

/// WIT case names of `interface`'s `type_name` (a variant or an enum), in
/// declaration order — the order the canonical ABI numbers discriminants in.
fn wit_case_names(resolve: &Resolve, pkg: PackageId, interface: &str, type_name: &str) -> Vec<String> {
    let iface_id = resolve.packages[pkg]
        .interfaces
        .get(interface)
        .copied()
        .unwrap_or_else(|| panic!("interface `{interface}` not found"));
    let type_id = resolve.interfaces[iface_id]
        .types
        .get(type_name)
        .copied()
        .unwrap_or_else(|| panic!("type `{type_name}` not found in `{interface}`"));
    match &resolve.types[type_id].kind {
        TypeDefKind::Variant(v) => v.cases.iter().map(|c| c.name.clone()).collect(),
        TypeDefKind::Enum(e) => e.cases.iter().map(|c| c.name.clone()).collect(),
        other => panic!("`{interface}#{type_name}` is neither variant nor enum: {other:?}"),
    }
}

/// `access-denied` → `AccessDenied`: how a WIT case name is spelled as a
/// Dovetail variant name.
fn pascal_case(kebab: &str) -> String {
    kebab
        .split('-')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// The string elements of `const <name>: [&str; N] = [ ... ];` in the
/// compiler source, in order.
fn source_table(const_name: &str) -> Vec<String> {
    let src = WASI_MARSHALING_SRC;
    let decl = src
        .find(&format!("{const_name}:"))
        .unwrap_or_else(|| panic!("const `{const_name}` not found in wasi_marshaling.rs"));
    let eq = decl + src[decl..].find('=').expect("no `=` after const name");
    let open = eq + src[eq..].find('[').expect("no `[` after `=`");
    let close = open + src[open..].find(']').expect("no closing `]`");
    src[open..close]
        .split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

/// The value of `const <name>: i32 = <n>;` in the compiler source.
fn source_disc(const_name: &str) -> i32 {
    let src = WASI_MARSHALING_SRC;
    let decl = src
        .find(&format!("{const_name}: i32 ="))
        .unwrap_or_else(|| panic!("const `{const_name}` not found in wasi_marshaling.rs"));
    let eq = decl + src[decl..].find('=').unwrap() + 1;
    src[eq..]
        .split(';')
        .next()
        .unwrap()
        .trim()
        .parse()
        .unwrap_or_else(|e| panic!("could not parse `{const_name}`: {e}"))
}

/// One table: names match the WIT exactly (spelling AND order), and the
/// `Other` discriminant points at the WIT's `other` case.
fn check(wit_cases: &[String], table_const: &str, disc_const: &str) {
    let expected: Vec<String> = wit_cases.iter().map(|c| pascal_case(c)).collect();
    let actual = source_table(table_const);
    assert_eq!(
        actual, expected,
        "{table_const} in wasi_marshaling.rs does not match the vendored WIT case order"
    );
    let wit_other = wit_cases
        .iter()
        .position(|c| c == "other")
        .unwrap_or_else(|| panic!("WIT type behind {table_const} has no `other` case"));
    assert_eq!(
        source_disc(disc_const),
        wit_other as i32,
        "{disc_const} does not match the WIT position of `other`"
    );
}

#[test]
fn the_variant_tables_match_the_wit() {
    let mut resolve = Resolve::default();
    for (name, contents) in WIT_FILES {
        resolve
            .push_str(name, contents)
            .unwrap_or_else(|e| panic!("could not parse {name}: {e}"));
    }
    let pkg = |package: &str| {
        resolve
            .package_names
            .get(&PackageName {
                namespace: "wasi".to_string(),
                name: package.to_string(),
                version: Some(P3_VERSION.parse().unwrap()),
            })
            .copied()
            .unwrap_or_else(|| panic!("wasi:{package} package not found"))
    };
    let sockets = pkg("sockets");
    let filesystem = pkg("filesystem");

    check(
        &wit_case_names(&resolve, sockets, "types", "error-code"),
        "NETWORK_ERROR_VARIANT_NAMES",
        "NETWORK_ERROR_OTHER_DISC",
    );
    check(
        &wit_case_names(&resolve, sockets, "ip-name-lookup", "error-code"),
        "DNS_ERROR_VARIANT_NAMES",
        "DNS_ERROR_OTHER_DISC",
    );
    check(
        &wit_case_names(&resolve, filesystem, "types", "error-code"),
        "FS_ERROR_VARIANT_NAMES",
        "FS_ERROR_OTHER_DISC",
    );
    check(
        &wit_case_names(&resolve, filesystem, "types", "descriptor-type"),
        "DESCRIPTOR_TYPE_VARIANT_NAMES",
        "DESCRIPTOR_TYPE_OTHER_DISC",
    );
}
