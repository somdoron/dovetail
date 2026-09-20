pub mod ai;
pub mod backtrace;
pub mod common;
pub mod compiler;
pub mod discovery;
pub mod formatter;
pub mod lsp;
pub mod manifest;
pub mod p3;
pub mod query;
pub mod runner;
pub mod test_runner;

// Re-export compiler sub-modules and pipeline at crate root for convenience
pub use compiler::{
    BuildMode, CompileResult, ProjectResult, TestExportInfo, WorkspaceResult, build_project,
    build_workspace, check, compile, compile_for_test, compile_for_test_with_derives,
    prelude_sources,
};
pub use compiler::{
    capture, codegen, coerce, coerce_byname, desugar, layout, lexer, macros, monomorphize, parser,
    typechecker,
};

pub mod image;
