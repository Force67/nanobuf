//! Resolved schema representation shared by all code generators.
//!
//! Layout (offsets, bit positions, fixed section size) is computed here,
//! once, so every target language reads and writes identical bytes.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct MessageId(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct EnumId(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScalarType {
    Bool,
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,
}

impl ScalarType {
    /// Size in bytes inside the fixed section. `Bool` is bit-packed and has
    /// no byte size.
    pub fn size(self) -> u32 {
        match self {
            ScalarType::Bool => 0,
            ScalarType::U8 | ScalarType::I8 => 1,
            ScalarType::U16 | ScalarType::I16 => 2,
            ScalarType::U32 | ScalarType::I32 | ScalarType::F32 => 4,
            ScalarType::U64 | ScalarType::I64 | ScalarType::F64 => 8,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ScalarType::Bool => "bool",
            ScalarType::U8 => "u8",
            ScalarType::U16 => "u16",
            ScalarType::U32 => "u32",
            ScalarType::U64 => "u64",
            ScalarType::I8 => "i8",
            ScalarType::I16 => "i16",
            ScalarType::I32 => "i32",
            ScalarType::I64 => "i64",
            ScalarType::F32 => "f32",
            ScalarType::F64 => "f64",
        }
    }

    pub fn is_signed(self) -> bool {
        matches!(
            self,
            ScalarType::I8 | ScalarType::I16 | ScalarType::I32 | ScalarType::I64
        )
    }

    pub fn is_float(self) -> bool {
        matches!(self, ScalarType::F32 | ScalarType::F64)
    }

    /// Inclusive value range, for enum backing checks. Floats have none.
    pub fn range(self) -> Option<(i64, i64)> {
        Some(match self {
            ScalarType::Bool => (0, 1),
            ScalarType::U8 => (0, u8::MAX as i64),
            ScalarType::U16 => (0, u16::MAX as i64),
            ScalarType::U32 => (0, u32::MAX as i64),
            ScalarType::U64 => (0, i64::MAX),
            ScalarType::I8 => (i8::MIN as i64, i8::MAX as i64),
            ScalarType::I16 => (i16::MIN as i64, i16::MAX as i64),
            ScalarType::I32 => (i32::MIN as i64, i32::MAX as i64),
            ScalarType::I64 => (i64::MIN, i64::MAX),
            ScalarType::F32 | ScalarType::F64 => return None,
        })
    }
}

/// Element type of a `list<...>`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ElemType {
    Scalar(ScalarType),
    Enum(EnumId),
    String,
    Bytes,
    Message(MessageId),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Type {
    Scalar(ScalarType),
    Enum(EnumId),
    String,
    Bytes,
    List(ElemType),
    Message(MessageId),
}

impl Type {
    /// True for types stored behind a heap offset slot.
    pub fn is_heap(self) -> bool {
        matches!(
            self,
            Type::String | Type::Bytes | Type::List(_) | Type::Message(_)
        )
    }
}

/// Proto3 wire encoding used in migration mode.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProtoEncoding {
    Varint,
    ZigZag,
    Fixed,
    LengthDelimited,
}

/// Bit position inside the fixed section. Bit 0 is the least significant.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BitAddr {
    pub byte: u16,
    pub bit: u8,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DataSlot {
    /// Scalar or enum value at a fixed offset.
    Bytes { offset: u16, size: u8 },
    /// Bit-packed bool.
    Bit(BitAddr),
    /// `u32` offset slot referencing a heap entry; `0` means absent.
    Offset { offset: u16 },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FieldLayout {
    /// Presence bit for optional scalars, enums, and bools.
    pub presence: Option<BitAddr>,
    pub data: DataSlot,
}

#[derive(Clone, Debug)]
pub struct Field {
    pub name: String,
    pub docs: Vec<String>,
    pub id: u32,
    pub ty: Type,
    pub optional: bool,
    /// For message-typed fields: the target type contains this message,
    /// so owned representations need indirection.
    pub boxed: bool,
    pub proto: ProtoEncoding,
    pub layout: FieldLayout,
}

#[derive(Debug)]
pub struct Message {
    pub package: String,
    pub name: String,
    pub docs: Vec<String>,
    /// Sorted by ascending field id.
    pub fields: Vec<Field>,
    pub reserved: Vec<u32>,
    /// Fixed section size in bytes, including the two header bytes.
    pub fixed_len: u16,
    /// Interior padding bytes left by the layout allocator.
    pub padding: u16,
}

#[derive(Debug)]
pub struct Variant {
    pub name: String,
    pub docs: Vec<String>,
    pub value: i64,
}

#[derive(Debug)]
pub struct Enum {
    pub package: String,
    pub name: String,
    pub docs: Vec<String>,
    pub backing: ScalarType,
    pub variants: Vec<Variant>,
}

#[derive(Debug, Default)]
pub struct Schema {
    pub messages: Vec<Message>,
    pub enums: Vec<Enum>,
}

impl Schema {
    pub fn message(&self, id: MessageId) -> &Message {
        &self.messages[id.0 as usize]
    }

    pub fn enumeration(&self, id: EnumId) -> &Enum {
        &self.enums[id.0 as usize]
    }

    /// Distinct package names, in first-appearance order.
    pub fn packages(&self) -> Vec<&str> {
        let mut seen = Vec::new();
        let names = self
            .messages
            .iter()
            .map(|m| m.package.as_str())
            .chain(self.enums.iter().map(|e| e.package.as_str()));
        for name in names {
            if !seen.contains(&name) {
                seen.push(name);
            }
        }
        seen
    }

    /// Message ids ordered so that non-boxed message-typed fields always
    /// point at earlier entries. Languages that require complete member
    /// types (C++) emit owned structs in this order; boxed fields break
    /// cycles via indirection and are excluded from the ordering constraint.
    pub fn message_order(&self) -> Vec<MessageId> {
        let n = self.messages.len();
        let mut state = vec![0u8; n]; // 0 unvisited, 1 visiting, 2 done
        let mut order = Vec::with_capacity(n);
        for start in 0..n {
            self.order_visit(start, &mut state, &mut order);
        }
        order
    }

    fn order_visit(&self, index: usize, state: &mut [u8], order: &mut Vec<MessageId>) {
        if state[index] != 0 {
            return;
        }
        state[index] = 1;
        for field in &self.messages[index].fields {
            // List elements live behind container indirection in every
            // target language, so only direct message fields constrain
            // the order.
            if field.boxed {
                continue;
            }
            if let Type::Message(MessageId(dep)) = field.ty {
                self.order_visit(dep as usize, state, order);
            }
        }
        state[index] = 2;
        order.push(MessageId(index as u32));
    }
}
