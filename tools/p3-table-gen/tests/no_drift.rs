//! The checked-in import table must be what the generator produces.
//!
//! `p3_imports.rs` is derived from `dovetail/wit`, but nothing stops someone
//! editing the WIT and not regenerating. The consequences are quiet: a changed
//! signature mismatches at instantiation, and a change that only shifts the
//! ordering makes the compiler call the wrong import — which surfaces as a trap
//! in code that has nothing to do with the edit. So the build checks it.

#[test]
fn the_checked_in_import_table_matches_the_wit() {
    let generated = p3_table_gen::generate();
    let checked_in = include_str!("../../../dovetail/src/compiler/codegen/p3_imports.rs");
    if generated != checked_in {
        let first_difference = generated
            .lines()
            .zip(checked_in.lines())
            .enumerate()
            .find(|(_, (a, b))| a != b)
            .map(|(line, (a, b))| format!("line {}:\n  generated:  {a}\n  checked in: {b}", line + 1))
            .unwrap_or_else(|| {
                format!(
                    "identical for the first {} lines, then one ends (generated {} lines, checked in {})",
                    generated.lines().count().min(checked_in.lines().count()),
                    generated.lines().count(),
                    checked_in.lines().count()
                )
            });
        panic!(
            "dovetail/src/compiler/codegen/p3_imports.rs is out of date with dovetail/wit.\n\
             Regenerate it:\n  \
             cargo run -p p3-table-gen > dovetail/src/compiler/codegen/p3_imports.rs\n\n\
             First difference — {first_difference}"
        );
    }
}
