use std::fmt;

/// Failure while reading a nanobuf or proto3 buffer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DecodeError {
    /// Buffer too small for a message header, or `fixed_len < 2`.
    Header,
    /// A length or offset points past the end of the buffer.
    Truncated,
    /// A string field holds invalid UTF-8.
    Utf8,
    /// A heap offset is malformed (zero inside an offset list).
    Offset,
    /// A proto3 varint runs past 10 bytes or the buffer end.
    Varint,
    /// A proto3 tag carries an unsupported wire type.
    WireType,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            DecodeError::Header => "invalid message header",
            DecodeError::Truncated => "buffer is truncated",
            DecodeError::Utf8 => "string field is not valid UTF-8",
            DecodeError::Offset => "malformed heap offset",
            DecodeError::Varint => "malformed varint",
            DecodeError::WireType => "unsupported wire type",
        };
        f.write_str(text)
    }
}

impl std::error::Error for DecodeError {}
