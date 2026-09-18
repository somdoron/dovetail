pub mod capture;
pub mod codegen;
pub mod coerce;
pub mod coerce_byname;
pub mod desugar;
pub mod layout;
pub mod lexer;
pub mod macros;
pub mod monomorphize;
pub(crate) mod named_calls;
pub mod parser;
mod pipeline;
pub mod typechecker;
pub mod witgen;

pub use pipeline::*;
