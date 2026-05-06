//! Lexer and parser for the nanobuf IDL (`.nb` files).
//!
//! [`parse`] turns source text into an [`ast::File`], reporting all
//! problems into a [`nanobuf_diagnostics::Diagnostics`] collection. The
//! parser recovers at line and declaration boundaries, so a single pass
//! reports every syntax error it can find.

pub mod ast;
pub mod lexer;
pub mod parser;
pub mod token;

pub use parser::parse;
