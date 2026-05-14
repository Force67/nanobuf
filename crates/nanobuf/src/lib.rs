//! Runtime support for nanobuf-generated Rust code.
//!
//! Wire format details live in `docs/wire-format.md`. Generated code uses
//! [`View`] for zero-copy reads, [`Writer`] for encoding, and [`proto`]
//! for the proto3 migration mode. Application code normally touches only
//! the generated types.

mod error;
pub mod proto;
mod scalar;
mod view;
mod write;

pub use error::DecodeError;
pub use scalar::Scalar;
pub use view::{BytesList, FromView, MessageList, OffsetList, ScalarList, StrList, View};
pub use write::Writer;
