//! Write `dovetail/src/compiler/codegen/p3_imports.rs`. See the crate docs in
//! `lib.rs` for what the table is and why it is derived rather than written.
//!
//! ```text
//! cargo run -p p3-table-gen > dovetail/src/compiler/codegen/p3_imports.rs
//! ```
//!
//! Regenerating is a no-op unless `dovetail/wit` changed, and
//! `tests/no_drift.rs` fails the build if it is not.

fn main() {
    print!("{}", p3_table_gen::generate());
}
