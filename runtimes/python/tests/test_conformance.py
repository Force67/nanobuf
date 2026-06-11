"""Conformance tests for the Python backend.

Sample values mirror `conformance/src/lib.rs` byte-for-byte; the fixture
files are golden output from the Rust implementation.
"""

import math
import pathlib
import struct

import pytest

import nanobuf
import telemetry

FIXTURES = pathlib.Path(__file__).resolve().parents[3] / "conformance" / "fixtures"

EVERYTHING_NB = (FIXTURES / "everything.nb.bin").read_bytes()
EVERYTHING_PB3 = (FIXTURES / "everything.pb3.bin").read_bytes()
TREE_NB = (FIXTURES / "tree.nb.bin").read_bytes()


def sample_everything():
    return telemetry.Everything(
        flag=True,
        tiny=7,
        small=-1234,
        medium=0xDEADBEEF,
        big=(2**64 - 1) // 3,
        sneg=-5_000_000_000,
        ratio=1.25,
        precise=-2.5e10,
        status=telemetry.Status.OK,
        wide=telemetry.Wide.NEG,
        name="telemetry probe",
        blob=bytes([0, 1, 2, 254, 255]),
        maybe_flag=False,
        maybe_small=-7,
        maybe_precise=None,
        maybe_status=telemetry.Status.DEGRADED,
        maybe_name="",
        maybe_blob=None,
        numbers=[1, 2, 3, 65535],
        floats=[0.5, -0.5],
        statuses=[telemetry.Status.OK, telemetry.Status.UNKNOWN, telemetry.Status(200)],
        tags=["a", "", "long tag value"],
        blobs=[b"\x01", b"", b"\x02\x03"],
        headers=[
            telemetry.Header(source=1, region="eu"),
            telemetry.Header(source=2, region=""),
        ],
        header=telemetry.Header(source=42, region="us-east"),
        fixed_int=-123456,
        raw_int=-42,
        flags=[True, False, True],
        half_ratio=-1.5,
        maybe_half=0.25,
        halves=[0.0, 65504.0, 2.0**-24, math.inf],
    )


def sample_tree():
    return telemetry.Tree(
        value=1,
        left=telemetry.Tree(value=2, right=telemetry.Tree(value=3)),
        right=None,
    )


def test_encode_matches_fixtures():
    assert sample_everything().encode() == EVERYTHING_NB
    assert sample_tree().encode() == TREE_NB


def test_decode_fixtures():
    assert telemetry.Everything.decode(EVERYTHING_NB) == sample_everything()
    assert telemetry.Tree.decode(TREE_NB) == sample_tree()


def test_view_accessors_match_sample_values():
    v = telemetry.EverythingView(EVERYTHING_NB)
    assert v.data == EVERYTHING_NB
    assert v.flag is True
    assert v.tiny == 7
    assert v.small == -1234
    assert v.medium == 0xDEADBEEF
    assert v.big == (2**64 - 1) // 3
    assert v.sneg == -5_000_000_000
    assert v.ratio == 1.25
    assert v.precise == -2.5e10
    assert v.status == telemetry.Status.OK
    assert v.wide == telemetry.Wide.NEG
    assert v.name == "telemetry probe"
    assert v.blob == bytes([0, 1, 2, 254, 255])
    assert v.maybe_flag is False
    assert v.maybe_small == -7
    assert v.maybe_precise is None
    assert v.maybe_status == telemetry.Status.DEGRADED
    assert v.maybe_name == ""
    assert v.maybe_blob is None
    assert v.numbers == [1, 2, 3, 65535]
    assert v.floats == [0.5, -0.5]
    assert v.statuses == [1, 0, 200]
    assert v.tags == ["a", "", "long tag value"]
    assert v.blobs == [b"\x01", b"", b"\x02\x03"]
    assert len(v.headers) == 2
    assert v.headers[0].source == 1
    assert v.headers[0].region == "eu"
    assert v.headers[1].source == 2
    assert v.headers[1].region == ""
    assert v.header.source == 42
    assert v.header.region == "us-east"
    assert v.fixed_int == -123456
    assert v.raw_int == -42
    assert v.flags == [True, False, True]
    assert v.half_ratio == -1.5
    assert v.maybe_half == 0.25
    assert v.halves == [0.0, 65504.0, 2.0**-24, math.inf]


def test_half_fields_round_to_binary16():
    # The in-memory value can be any float; the wire carries binary16.
    m = telemetry.Everything(half_ratio=0.1, maybe_half=1e30)
    decoded = telemetry.Everything.decode(m.encode())
    assert decoded.half_ratio == nanobuf.half_round(0.1)
    assert decoded.half_ratio != 0.1
    assert decoded.maybe_half == math.inf


def test_tree_views_walk_nested_messages():
    v = telemetry.TreeView(TREE_NB)
    assert v.value == 1
    assert v.right is None
    assert v.left.value == 2
    assert v.left.left is None
    assert v.left.right.value == 3
    # Sub-message views forward their raw bytes.
    inner = telemetry.Tree.decode(bytes(v.left.data))
    assert inner == sample_tree().left


def test_enum_labels_and_unknown_values():
    assert telemetry.Status.OK.label() == "ok"
    assert telemetry.Status(200).label() is None
    assert telemetry.Wide.NEG == -5
    assert telemetry.Wide.NEG.label() == "neg"
    decoded = telemetry.Everything.decode(EVERYTHING_NB)
    assert decoded.statuses[2] == 200
    assert decoded.statuses[2].label() is None


def test_proto3_encode_matches_fixture():
    assert sample_everything().encode_proto3() == EVERYTHING_PB3


def test_proto3_round_trips():
    assert telemetry.Everything.decode_proto3(EVERYTHING_PB3) == sample_everything()
    tree = sample_tree()
    assert telemetry.Tree.decode_proto3(tree.encode_proto3()) == tree


def test_proto3_decoder_skips_unknown_fields():
    data = bytearray(EVERYTHING_PB3)
    nanobuf.write_varint_field(data, 999, 5)
    nanobuf.write_len_field(data, 998, b"xy")
    assert telemetry.Everything.decode_proto3(bytes(data)) == sample_everything()


def test_old_data_reads_as_defaults():
    # A writer that only knew fields 1 and 2: fixed section of 4 bytes
    # holding the flag bit and `tiny`.
    old = bytes([4, 0, 0x01, 9])
    decoded = telemetry.Everything.decode(old)
    assert decoded == telemetry.Everything(flag=True, tiny=9)
    v = telemetry.EverythingView(old)
    assert v.small == 0
    assert v.precise == 0.0
    assert v.maybe_small is None
    assert v.name == ""
    assert v.numbers == []
    assert v.header is None


def test_default_message_encodes_fixed_section_only():
    data = telemetry.Everything().encode()
    fixed_len = struct.unpack_from("<H", data)[0]
    assert len(data) == fixed_len
    assert telemetry.Everything.decode(data) == telemetry.Everything()


def test_optional_default_distinction_survives():
    present = telemetry.Everything(maybe_flag=False, maybe_small=0, maybe_name="")
    absent = telemetry.Everything()
    assert present.encode() != absent.encode()
    decoded = telemetry.Everything.decode(present.encode())
    assert decoded.maybe_flag is False
    assert decoded.maybe_small == 0
    assert decoded.maybe_name == ""
    decoded = telemetry.Everything.decode(absent.encode())
    assert decoded.maybe_flag is None
    assert decoded.maybe_small is None
    assert decoded.maybe_name is None
    # Empty non-optional strings encode identically to the default.
    assert telemetry.Everything(name="").encode() == absent.encode()


def test_corrupt_headers_raise_decode_error():
    for data in (b"", b"\x01", b"\x01\x00", b"\x00\x00", b"\xff\xff"):
        with pytest.raises(nanobuf.DecodeError):
            telemetry.EverythingView(data)


def test_corrupt_heap_offsets_raise_decode_error():
    buf = bytearray(telemetry.Header(source=1, region="x").encode())
    # Point the region slot past the end of the buffer.
    struct.pack_into("<I", buf, 8, 1000)
    with pytest.raises(nanobuf.DecodeError):
        telemetry.HeaderView(bytes(buf)).region

    buf = bytearray(EVERYTHING_NB)
    # Point the numbers slot at a count that runs off the end.
    struct.pack_into("<I", buf, 72, len(buf) - 2)
    with pytest.raises(nanobuf.DecodeError):
        telemetry.EverythingView(bytes(buf)).numbers


def test_invalid_utf8_raises_decode_error():
    buf = bytearray(telemetry.Header(source=1, region="ab").encode())
    buf[-2:] = b"\xff\xfe"
    with pytest.raises(nanobuf.DecodeError):
        telemetry.HeaderView(bytes(buf)).region


def test_truncated_buffers_never_raise_index_errors():
    for cut in (0, 1, 3, 10, len(EVERYTHING_NB) // 2):
        try:
            v = telemetry.EverythingView(EVERYTHING_NB[:cut])
        except nanobuf.DecodeError:
            continue
        for accessor in (lambda: v.name, lambda: v.headers[0], lambda: v.to_owned()):
            try:
                accessor()
            except nanobuf.DecodeError:
                pass


def test_corrupt_proto3_raises_decode_error():
    corrupt = (
        b"\x08",  # truncated varint value
        b"\x0b",  # wire type 3 (group start)
        b"\x0c",  # wire type 4 (group end)
        b"\x5a\x05hi",  # length-delimited field cut short
        bytes([0x08] + [0xFF] * 9 + [0x02]),  # varint overruns 10 bytes
    )
    for data in corrupt:
        with pytest.raises(nanobuf.DecodeError):
            telemetry.Everything.decode_proto3(data)
