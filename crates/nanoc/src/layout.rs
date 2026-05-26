//! Human-readable wire layout tables for `nanoc layout`.

use std::fmt::Write;

use nanobuf_schema::{DataSlot, ElemType, Field, Schema, Type};

pub fn render(schema: &Schema) -> String {
    let mut out = String::new();
    for message in &schema.messages {
        let _ = writeln!(
            out,
            "message {}.{}: fixed section {} bytes, padding {}",
            message.package, message.name, message.fixed_len, message.padding
        );
        let name_width = message
            .fields
            .iter()
            .map(|f| f.name.len())
            .max()
            .unwrap_or(0)
            .max(4);
        let type_width = message
            .fields
            .iter()
            .map(|f| type_name(schema, f).len())
            .max()
            .unwrap_or(0)
            .max(4);
        for f in &message.fields {
            let _ = writeln!(
                out,
                "  @{:<4} {:<name_width$}  {:<type_width$}  {}",
                f.id,
                f.name,
                type_name(schema, f),
                slot_text(f)
            );
        }
        if !message.reserved.is_empty() {
            let ids: Vec<String> = message.reserved.iter().map(|id| format!("@{id}")).collect();
            let _ = writeln!(out, "  reserved {}", ids.join(", "));
        }
        out.push('\n');
    }
    out
}

fn type_name(schema: &Schema, f: &Field) -> String {
    let base = match f.ty {
        Type::Scalar(s) => s.name().to_string(),
        Type::Enum(id) => schema.enumeration(id).name.clone(),
        Type::String => "string".to_string(),
        Type::Bytes => "bytes".to_string(),
        Type::Message(id) => schema.message(id).name.clone(),
        Type::List(elem) => {
            let elem = match elem {
                ElemType::Scalar(s) => s.name().to_string(),
                ElemType::Enum(id) => schema.enumeration(id).name.clone(),
                ElemType::String => "string".to_string(),
                ElemType::Bytes => "bytes".to_string(),
                ElemType::Message(id) => schema.message(id).name.clone(),
            };
            return format!("list<{elem}>");
        }
    };
    if f.optional { format!("{base}?") } else { base }
}

fn slot_text(f: &Field) -> String {
    let data = match f.layout.data {
        DataSlot::Bytes { offset, size } => {
            format!("bytes {}..{}", offset, offset as u32 + size as u32)
        }
        DataSlot::Bit(bit) => format!("bit {}.{}", bit.byte, bit.bit),
        DataSlot::Offset { offset } => format!("slot {offset}"),
    };
    match f.layout.presence {
        Some(p) => format!("presence {}.{}, {data}", p.byte, p.bit),
        None => data,
    }
}
