// Nanobuf C# runtime. Wire semantics mirror crates/nanobuf/src/{view,write,proto}.rs;
// see docs/wire-format.md for the normative format.
#nullable enable

using System;
using System.Buffers.Binary;
using System.Collections.Generic;
using System.Runtime.CompilerServices;
using System.Text;

namespace Nanobuf {
    /// Failure while reading a nanobuf or proto3 buffer.
    public sealed class DecodeException : Exception {
        public DecodeException(string message) : base(message) { }
    }

    /// Reader helpers shared by generated view types. Scalar reads past the
    /// writer's fixed section yield defaults; heap dereferences are bounds
    /// checked and throw <see cref="DecodeException"/> on corruption.
    public static class Wire {
        private static readonly UTF8Encoding StrictUtf8 = new(false, true);

        /// Validates the message header and returns the fixed section length.
        public static int ParseHeader(ReadOnlySpan<byte> data) {
            if (data.Length < 2) {
                throw new DecodeException("invalid message header");
            }
            int fixedLen = BinaryPrimitives.ReadUInt16LittleEndian(data);
            if (fixedLen < 2) {
                throw new DecodeException("invalid message header");
            }
            if (fixedLen > data.Length) {
                throw new DecodeException("buffer is truncated");
            }
            return fixedLen;
        }

        /// Reads a little-endian value of size 1, 2, 4, or 8 bytes. Works for
        /// primitives and for generated enum structs wrapping one of them.
        public static T ReadLe<T>(ReadOnlySpan<byte> source) where T : unmanaged {
            if (typeof(T) == typeof(bool)) {
                bool flag = source[0] != 0;
                return Unsafe.As<bool, T>(ref flag);
            }
            switch (Unsafe.SizeOf<T>()) {
                case 1: {
                    byte v = source[0];
                    return Unsafe.As<byte, T>(ref v);
                }
                case 2: {
                    ushort v = BinaryPrimitives.ReadUInt16LittleEndian(source);
                    return Unsafe.As<ushort, T>(ref v);
                }
                case 4: {
                    uint v = BinaryPrimitives.ReadUInt32LittleEndian(source);
                    return Unsafe.As<uint, T>(ref v);
                }
                default: {
                    ulong v = BinaryPrimitives.ReadUInt64LittleEndian(source);
                    return Unsafe.As<ulong, T>(ref v);
                }
            }
        }

        /// Stores a little-endian value of size 1, 2, 4, or 8 bytes.
        public static void WriteLe<T>(Span<byte> destination, T value) where T : unmanaged {
            if (typeof(T) == typeof(bool)) {
                destination[0] = Unsafe.As<T, bool>(ref value) ? (byte)1 : (byte)0;
                return;
            }
            switch (Unsafe.SizeOf<T>()) {
                case 1:
                    destination[0] = Unsafe.As<T, byte>(ref value);
                    break;
                case 2:
                    BinaryPrimitives.WriteUInt16LittleEndian(destination, Unsafe.As<T, ushort>(ref value));
                    break;
                case 4:
                    BinaryPrimitives.WriteUInt32LittleEndian(destination, Unsafe.As<T, uint>(ref value));
                    break;
                default:
                    BinaryPrimitives.WriteUInt64LittleEndian(destination, Unsafe.As<T, ulong>(ref value));
                    break;
            }
        }

        /// Reads a fixed-section scalar, or the default when the writer's
        /// schema was older and the slot does not exist.
        public static T ScalarAt<T>(ReadOnlySpan<byte> data, int fixedLen, int offset) where T : unmanaged {
            int size = Unsafe.SizeOf<T>();
            if (offset + size > fixedLen || offset + size > data.Length) {
                return default;
            }
            return ReadLe<T>(data.Slice(offset));
        }

        public static bool BitAt(ReadOnlySpan<byte> data, int fixedLen, int byteAddr, int bit) {
            return byteAddr < fixedLen && ((data[byteAddr] >> bit) & 1) != 0;
        }

        /// Resolves an offset slot to a message-relative heap offset.
        /// Zero means absent (also when the slot is past the fixed section).
        private static uint OffsetAt(ReadOnlySpan<byte> data, int fixedLen, int slot) {
            if (slot + 4 > fixedLen) {
                return 0;
            }
            return BinaryPrimitives.ReadUInt32LittleEndian(data.Slice(slot));
        }

        /// Reads the [len: u32][payload] heap entry at `entry`.
        public static ReadOnlySpan<byte> EntryAt(ReadOnlySpan<byte> data, long entry) {
            if (entry < 0 || entry + 4 > data.Length) {
                throw new DecodeException("buffer is truncated");
            }
            uint len = BinaryPrimitives.ReadUInt32LittleEndian(data.Slice((int)entry));
            if (len > (ulong)(data.Length - entry - 4)) {
                throw new DecodeException("buffer is truncated");
            }
            return data.Slice((int)entry + 4, (int)len);
        }

        /// string/bytes/message field payload; false means absent.
        public static bool TryHeapAt(
            ReadOnlySpan<byte> data, int fixedLen, int slot, out ReadOnlySpan<byte> entry) {
            uint offset = OffsetAt(data, fixedLen, slot);
            if (offset == 0) {
                entry = default;
                return false;
            }
            entry = EntryAt(data, offset);
            return true;
        }

        /// string field; null means absent. Throws on invalid UTF-8.
        public static string? StrAt(ReadOnlySpan<byte> data, int fixedLen, int slot) {
            return TryHeapAt(data, fixedLen, slot, out var bytes) ? Utf8(bytes) : null;
        }

        public static string Utf8(ReadOnlySpan<byte> bytes) {
            try {
                return StrictUtf8.GetString(bytes);
            } catch (ArgumentException) {
                throw new DecodeException("string field is not valid UTF-8");
            }
        }

        /// list&lt;scalar&gt; / list&lt;enum&gt; field. Absent reads as empty.
        public static ScalarList<T> ScalarListAt<T>(ReadOnlySpan<byte> data, int fixedLen, int slot)
            where T : unmanaged {
            uint offset = OffsetAt(data, fixedLen, slot);
            if (offset == 0) {
                return default;
            }
            long entry = offset;
            if (entry + 4 > data.Length) {
                throw new DecodeException("buffer is truncated");
            }
            uint count = BinaryPrimitives.ReadUInt32LittleEndian(data.Slice((int)entry));
            long payloadLen = (long)count * Unsafe.SizeOf<T>();
            if (payloadLen > data.Length - entry - 4) {
                throw new DecodeException("buffer is truncated");
            }
            return new ScalarList<T>(data.Slice((int)entry + 4, (int)payloadLen), (int)count);
        }

        /// list&lt;string|bytes|message&gt; field. Absent reads as empty.
        public static OffsetList OffsetListAt(ReadOnlySpan<byte> data, int fixedLen, int slot) {
            uint offset = OffsetAt(data, fixedLen, slot);
            if (offset == 0) {
                return default;
            }
            long entry = offset;
            if (entry + 4 > data.Length) {
                throw new DecodeException("buffer is truncated");
            }
            uint count = BinaryPrimitives.ReadUInt32LittleEndian(data.Slice((int)entry));
            long offsetsLen = (long)count * 4;
            if (offsetsLen > data.Length - entry - 4) {
                throw new DecodeException("buffer is truncated");
            }
            return new OffsetList(data, data.Slice((int)entry + 4, (int)offsetsLen), (int)count);
        }
    }

    /// Zero-copy list of fixed-width little-endian elements.
    public readonly ref struct ScalarList<T> where T : unmanaged {
        private readonly ReadOnlySpan<byte> _payload;
        private readonly int _count;

        public ScalarList(ReadOnlySpan<byte> payload, int count) {
            _payload = payload;
            _count = count;
        }

        public int Count => _count;

        public T this[int index] {
            get {
                if ((uint)index >= (uint)_count) {
                    throw new ArgumentOutOfRangeException(nameof(index));
                }
                return Wire.ReadLe<T>(_payload.Slice(index * Unsafe.SizeOf<T>()));
            }
        }

        public List<T> ToList() {
            var list = new List<T>(_count);
            for (int i = 0; i < _count; i++) {
                list.Add(this[i]);
            }
            return list;
        }

        public T[] ToArray() {
            var array = new T[_count];
            for (int i = 0; i < _count; i++) {
                array[i] = this[i];
            }
            return array;
        }
    }

    /// List of heap entries (strings, bytes, or messages), resolved lazily.
    public readonly ref struct OffsetList {
        private readonly ReadOnlySpan<byte> _message;
        private readonly ReadOnlySpan<byte> _offsets;
        private readonly int _count;

        public OffsetList(ReadOnlySpan<byte> message, ReadOnlySpan<byte> offsets, int count) {
            _message = message;
            _offsets = offsets;
            _count = count;
        }

        public int Count => _count;

        /// Raw payload of element `index`.
        public ReadOnlySpan<byte> BytesAt(int index) {
            if ((uint)index >= (uint)_count) {
                throw new ArgumentOutOfRangeException(nameof(index));
            }
            uint offset = BinaryPrimitives.ReadUInt32LittleEndian(_offsets.Slice(index * 4));
            if (offset == 0) {
                throw new DecodeException("malformed heap offset");
            }
            return Wire.EntryAt(_message, offset);
        }

        public string StrAt(int index) => Wire.Utf8(BytesAt(index));
    }

    /// Typed list&lt;string&gt; reader.
    public readonly ref struct StrList {
        private readonly OffsetList _list;

        public StrList(OffsetList list) {
            _list = list;
        }

        public int Count => _list.Count;

        public string this[int index] => _list.StrAt(index);

        public List<string> ToList() {
            var list = new List<string>(Count);
            for (int i = 0; i < Count; i++) {
                list.Add(this[i]);
            }
            return list;
        }
    }

    /// Typed list&lt;bytes&gt; reader.
    public readonly ref struct BytesList {
        private readonly OffsetList _list;

        public BytesList(OffsetList list) {
            _list = list;
        }

        public int Count => _list.Count;

        public ReadOnlySpan<byte> this[int index] => _list.BytesAt(index);

        public List<byte[]> ToList() {
            var list = new List<byte[]>(Count);
            for (int i = 0; i < Count; i++) {
                list.Add(this[i].ToArray());
            }
            return list;
        }
    }

    /// Single-buffer message writer; mirrors the Rust nanobuf::Writer so
    /// every backend produces byte-identical output.
    public sealed class Writer {
        private byte[] _buf = new byte[256];
        private int _len;
        /// Start of the current message; heap offsets are relative to it.
        private int _base;

        public byte[] ToArray() {
            var copy = new byte[_len];
            Array.Copy(_buf, copy, _len);
            return copy;
        }

        private void Reserve(int extra) {
            int needed = _len + extra;
            if (needed <= _buf.Length) {
                return;
            }
            int capacity = _buf.Length * 2;
            while (capacity < needed) {
                capacity *= 2;
            }
            Array.Resize(ref _buf, capacity);
        }

        /// Starts a message at the current position: writes the header and
        /// zero-fills the rest of the fixed section.
        public void Begin(int fixedLen) {
            _base = _len;
            Reserve(fixedLen);
            Array.Clear(_buf, _len, fixedLen);
            BinaryPrimitives.WriteUInt16LittleEndian(_buf.AsSpan(_len), (ushort)fixedLen);
            _len += fixedLen;
        }

        /// Writes a scalar into the fixed section.
        public void PutScalar<T>(int offset, T value) where T : unmanaged {
            Wire.WriteLe(_buf.AsSpan(_base + offset), value);
        }

        public void PutBit(int byteAddr, int bit, bool value) {
            if (value) {
                _buf[_base + byteAddr] |= (byte)(1 << bit);
            }
        }

        /// Appends a [len][payload] heap entry and returns its
        /// message-relative offset.
        public uint HeapBytes(ReadOnlySpan<byte> payload) {
            int entry = _len;
            AppendUInt32((uint)payload.Length);
            Reserve(payload.Length);
            payload.CopyTo(_buf.AsSpan(_len));
            _len += payload.Length;
            return (uint)(entry - _base);
        }

        public uint HeapString(string value) => HeapBytes(Encoding.UTF8.GetBytes(value));

        /// Appends a nested message entry; `writeBody` serializes the child.
        /// Returns the entry's message-relative offset.
        public uint HeapMessage(Action<Writer> writeBody) {
            // Message payloads are 8-aligned relative to the enclosing message.
            PadPayloadTo(8);
            int entry = _len;
            uint offset = (uint)(entry - _base);
            AppendUInt32(0);
            int outerBase = _base;
            int bodyStart = _len;
            writeBody(this);
            BinaryPrimitives.WriteUInt32LittleEndian(_buf.AsSpan(entry), (uint)(_len - bodyStart));
            _base = outerBase;
            return offset;
        }

        /// Patches an offset slot in the fixed section.
        public void SetSlot(int slot, uint heapOffset) {
            BinaryPrimitives.WriteUInt32LittleEndian(_buf.AsSpan(_base + slot), heapOffset);
        }

        /// string/bytes field: writes the entry unless the value is empty and
        /// `force` is false (absent and empty read identically then).
        public void PutBytes(int slot, ReadOnlySpan<byte> payload, bool force) {
            if (payload.Length == 0 && !force) {
                return;
            }
            SetSlot(slot, HeapBytes(payload));
        }

        public void PutString(int slot, string value, bool force) {
            if (value.Length == 0 && !force) {
                return;
            }
            SetSlot(slot, HeapString(value));
        }

        /// Message-typed field.
        public void PutMessage(int slot, Action<Writer> writeBody) {
            SetSlot(slot, HeapMessage(writeBody));
        }

        /// list&lt;scalar&gt; / list&lt;enum&gt; field. Empty lists are absent.
        public void PutScalarList<T>(int slot, List<T> items) where T : unmanaged {
            if (items.Count == 0) {
                return;
            }
            int size = Unsafe.SizeOf<T>();
            PadPayloadTo(size);
            int entry = _len;
            AppendUInt32((uint)items.Count);
            Reserve(items.Count * size);
            foreach (T item in items) {
                Wire.WriteLe(_buf.AsSpan(_len), item);
                _len += size;
            }
            SetSlot(slot, (uint)(entry - _base));
        }

        /// list&lt;string|bytes|message&gt; field: writes the offset array, then
        /// one entry per item via `writeItem` (which returns each entry's
        /// message-relative offset, e.g. from <see cref="HeapBytes"/>).
        public void PutOffsetList<T>(int slot, List<T> items, Func<Writer, T, uint> writeItem) {
            if (items.Count == 0) {
                return;
            }
            int entry = _len;
            AppendUInt32((uint)items.Count);
            int table = _len;
            AppendZeros(items.Count * 4);
            for (int i = 0; i < items.Count; i++) {
                uint offset = writeItem(this, items[i]);
                BinaryPrimitives.WriteUInt32LittleEndian(_buf.AsSpan(table + i * 4), offset);
            }
            SetSlot(slot, (uint)(entry - _base));
        }

        private void AppendUInt32(uint value) {
            Reserve(4);
            BinaryPrimitives.WriteUInt32LittleEndian(_buf.AsSpan(_len), value);
            _len += 4;
        }

        private void AppendZeros(int count) {
            Reserve(count);
            Array.Clear(_buf, _len, count);
            _len += count;
        }

        /// Pads so the next entry's payload (after its 4-byte length/count
        /// prefix) lands `align`-aligned relative to the message start.
        private void PadPayloadTo(int align) {
            int payloadPos = _len - _base + 4;
            int target = (payloadPos + align - 1) / align * align;
            AppendZeros(target - payloadPos);
        }
    }

    /// Growable byte buffer used by the proto3 migration codecs.
    public sealed class ByteWriter {
        private byte[] _buf = new byte[64];
        private int _len;

        public int Length => _len;

        public void Push(byte value) {
            if (_len == _buf.Length) {
                Array.Resize(ref _buf, _buf.Length * 2);
            }
            _buf[_len++] = value;
        }

        public void Append(ReadOnlySpan<byte> bytes) {
            if (_len + bytes.Length > _buf.Length) {
                int capacity = _buf.Length * 2;
                while (capacity < _len + bytes.Length) {
                    capacity *= 2;
                }
                Array.Resize(ref _buf, capacity);
            }
            bytes.CopyTo(_buf.AsSpan(_len));
            _len += bytes.Length;
        }

        public byte[] ToArray() {
            var copy = new byte[_len];
            Array.Copy(_buf, copy, _len);
            return copy;
        }

        public ReadOnlySpan<byte> WrittenSpan => _buf.AsSpan(0, _len);
    }

    /// One decoded proto3 field value.
    public readonly ref struct ProtoValue {
        /// 0 = varint, 1 = fixed64, 2 = length-delimited, 5 = fixed32.
        public readonly int WireType;
        public readonly ulong Number;
        public readonly ReadOnlySpan<byte> Payload;

        public ProtoValue(int wireType, ulong number, ReadOnlySpan<byte> payload) {
            WireType = wireType;
            Number = number;
            Payload = payload;
        }
    }

    /// Iterates (field number, value) pairs of one proto3 message.
    /// Unknown fields are skipped by ignoring the yielded value.
    public ref struct ProtoReader {
        private readonly ReadOnlySpan<byte> _buf;
        private int _pos;

        public ProtoReader(ReadOnlySpan<byte> buf) {
            _buf = buf;
            _pos = 0;
        }

        public bool Next(out uint field, out ProtoValue value) {
            if (_pos >= _buf.Length) {
                field = 0;
                value = default;
                return false;
            }
            ulong tag = Proto.ReadVarint(_buf, ref _pos);
            ulong number = tag >> 3;
            if (number == 0 || number > uint.MaxValue) {
                throw new DecodeException("unsupported wire type");
            }
            field = (uint)number;
            switch ((int)(tag & 7)) {
                case 0:
                    value = new ProtoValue(0, Proto.ReadVarint(_buf, ref _pos), default);
                    break;
                case 1:
                    value = new ProtoValue(1, ReadFixed(8), default);
                    break;
                case 2: {
                    ulong len = Proto.ReadVarint(_buf, ref _pos);
                    if (len > (ulong)(_buf.Length - _pos)) {
                        throw new DecodeException("buffer is truncated");
                    }
                    value = new ProtoValue(2, 0, _buf.Slice(_pos, (int)len));
                    _pos += (int)len;
                    break;
                }
                case 5:
                    value = new ProtoValue(5, ReadFixed(4), default);
                    break;
                default:
                    throw new DecodeException("unsupported wire type");
            }
            return true;
        }

        private ulong ReadFixed(int size) {
            if (_pos + size > _buf.Length) {
                throw new DecodeException("buffer is truncated");
            }
            ulong value = size == 4
                ? BinaryPrimitives.ReadUInt32LittleEndian(_buf.Slice(_pos))
                : BinaryPrimitives.ReadUInt64LittleEndian(_buf.Slice(_pos));
            _pos += size;
            return value;
        }
    }

    /// Proto3 wire format helpers, used only in migration mode.
    public static class Proto {
        public static ulong ReadVarint(ReadOnlySpan<byte> buf, ref int pos) {
            ulong value = 0;
            for (int i = 0; i < 10; i++) {
                if (pos >= buf.Length) {
                    throw new DecodeException("malformed varint");
                }
                byte b = buf[pos++];
                // The tenth byte may only carry the final bit of a u64.
                if (i == 9 && b > 1) {
                    throw new DecodeException("malformed varint");
                }
                value |= (ulong)(b & 0x7f) << (i * 7);
                if ((b & 0x80) == 0) {
                    return value;
                }
            }
            throw new DecodeException("malformed varint");
        }

        public static void WriteVarint(ByteWriter output, ulong value) {
            while (true) {
                byte b = (byte)(value & 0x7f);
                value >>= 7;
                if (value == 0) {
                    output.Push(b);
                    return;
                }
                output.Push((byte)(b | 0x80));
            }
        }

        public static ulong ZigzagEncode(long value) => (ulong)((value << 1) ^ (value >> 63));

        public static long ZigzagDecode(ulong value) => (long)(value >> 1) ^ -(long)(value & 1);

        public static ulong ExpectVarint(ProtoValue value) {
            if (value.WireType != 0) {
                throw new DecodeException("unsupported wire type");
            }
            return value.Number;
        }

        public static uint ExpectI32(ProtoValue value) {
            if (value.WireType != 5) {
                throw new DecodeException("unsupported wire type");
            }
            return (uint)value.Number;
        }

        public static ulong ExpectI64(ProtoValue value) {
            if (value.WireType != 1) {
                throw new DecodeException("unsupported wire type");
            }
            return value.Number;
        }

        public static ReadOnlySpan<byte> ExpectLen(ProtoValue value) {
            if (value.WireType != 2) {
                throw new DecodeException("unsupported wire type");
            }
            return value.Payload;
        }

        public static string ExpectString(ProtoValue value) => Wire.Utf8(ExpectLen(value));

        /// Reads a packed varint list payload, or a single unpacked element.
        public static void ReadPackedVarints(ProtoValue value, Action<ulong> push) {
            switch (value.WireType) {
                case 2: {
                    int pos = 0;
                    while (pos < value.Payload.Length) {
                        push(ReadVarint(value.Payload, ref pos));
                    }
                    break;
                }
                case 0:
                    push(value.Number);
                    break;
                default:
                    throw new DecodeException("unsupported wire type");
            }
        }

        /// Reads a packed fixed32 list payload, or a single unpacked element.
        public static void ReadPackedFixed32(ProtoValue value, Action<uint> push) {
            switch (value.WireType) {
                case 2: {
                    if (value.Payload.Length % 4 != 0) {
                        throw new DecodeException("buffer is truncated");
                    }
                    for (int pos = 0; pos < value.Payload.Length; pos += 4) {
                        push(BinaryPrimitives.ReadUInt32LittleEndian(value.Payload.Slice(pos)));
                    }
                    break;
                }
                case 5:
                    push((uint)value.Number);
                    break;
                default:
                    throw new DecodeException("unsupported wire type");
            }
        }

        /// Reads a packed fixed64 list payload, or a single unpacked element.
        public static void ReadPackedFixed64(ProtoValue value, Action<ulong> push) {
            switch (value.WireType) {
                case 2: {
                    if (value.Payload.Length % 8 != 0) {
                        throw new DecodeException("buffer is truncated");
                    }
                    for (int pos = 0; pos < value.Payload.Length; pos += 8) {
                        push(BinaryPrimitives.ReadUInt64LittleEndian(value.Payload.Slice(pos)));
                    }
                    break;
                }
                case 1:
                    push(value.Number);
                    break;
                default:
                    throw new DecodeException("unsupported wire type");
            }
        }

        public static void WriteTag(ByteWriter output, int field, int wireType) {
            WriteVarint(output, ((ulong)(uint)field << 3) | (uint)wireType);
        }

        public static void WriteVarintField(ByteWriter output, int field, ulong value) {
            WriteTag(output, field, 0);
            WriteVarint(output, value);
        }

        public static void WriteFixed32(ByteWriter output, uint value) {
            Span<byte> bytes = stackalloc byte[4];
            BinaryPrimitives.WriteUInt32LittleEndian(bytes, value);
            output.Append(bytes);
        }

        public static void WriteFixed64(ByteWriter output, ulong value) {
            Span<byte> bytes = stackalloc byte[8];
            BinaryPrimitives.WriteUInt64LittleEndian(bytes, value);
            output.Append(bytes);
        }

        public static void WriteI32Field(ByteWriter output, int field, uint value) {
            WriteTag(output, field, 5);
            WriteFixed32(output, value);
        }

        public static void WriteI64Field(ByteWriter output, int field, ulong value) {
            WriteTag(output, field, 1);
            WriteFixed64(output, value);
        }

        public static void WriteLenField(ByteWriter output, int field, ReadOnlySpan<byte> payload) {
            WriteTag(output, field, 2);
            WriteVarint(output, (ulong)payload.Length);
            output.Append(payload);
        }

        /// Writes a length-delimited field whose payload is produced by `body`.
        /// Used for nested messages and packed lists.
        public static void WriteLenFieldWith(ByteWriter output, int field, Action<ByteWriter> body) {
            var payload = new ByteWriter();
            body(payload);
            WriteLenField(output, field, payload.WrittenSpan);
        }
    }

    /// Deep equality helpers for generated owned types.
    public static class Eq {
        public static bool Bytes(byte[]? a, byte[]? b) {
            if (a is null || b is null) {
                return ReferenceEquals(a, b);
            }
            return a.AsSpan().SequenceEqual(b);
        }

        public static bool Lists<T>(List<T> a, List<T> b) {
            if (a.Count != b.Count) {
                return false;
            }
            for (int i = 0; i < a.Count; i++) {
                if (!EqualityComparer<T>.Default.Equals(a[i], b[i])) {
                    return false;
                }
            }
            return true;
        }

        public static bool ByteLists(List<byte[]> a, List<byte[]> b) {
            if (a.Count != b.Count) {
                return false;
            }
            for (int i = 0; i < a.Count; i++) {
                if (!Bytes(a[i], b[i])) {
                    return false;
                }
            }
            return true;
        }
    }
}
