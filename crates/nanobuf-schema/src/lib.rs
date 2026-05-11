//! Semantic analysis for nanobuf schemas.
//!
//! [`analyze`] turns parsed files into a validated [`Schema`] whose wire
//! layout (field offsets, presence bits, fixed section sizes) is fully
//! assigned. Code generators consume the IR as-is and never make layout
//! decisions of their own.

pub mod ir;
pub mod layout;
mod resolve;

pub use ir::*;
pub use resolve::{Unit, analyze};
