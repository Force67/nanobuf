// Conformance test for the C++ backend. Mirrors conformance/src/lib.rs:
// the sample values here must stay byte-identical to the Rust fixtures.

#include <cassert>
#include <cstdint>
#include <cstdio>
#include <fstream>
#include <memory>
#include <optional>
#include <string>
#include <utility>
#include <vector>

#include "telemetry.nb.h"

namespace {

using common::Header;
using demo::telemetry::Everything;
using demo::telemetry::EverythingView;
using demo::telemetry::Status;
using demo::telemetry::Tree;
using demo::telemetry::TreeView;
using demo::telemetry::Wide;

std::vector<uint8_t> ReadFile(const std::string& path) {
  std::ifstream in(path, std::ios::binary);
  assert(in && "fixture not found");
  return std::vector<uint8_t>(std::istreambuf_iterator<char>(in),
                              std::istreambuf_iterator<char>());
}

Everything SampleEverything() {
  Everything e;
  e.flag = true;
  e.tiny = 7;
  e.small = -1234;
  e.medium = 0xdeadbeef;
  e.big = UINT64_MAX / 3;
  e.sneg = -5000000000;
  e.ratio = 1.25f;
  e.precise = -2.5e10;
  e.status = Status::kOk;
  e.wide = Wide::kNeg;
  e.name = "telemetry probe";
  e.blob = {0, 1, 2, 254, 255};
  e.maybe_flag = false;
  e.maybe_small = -7;
  e.maybe_precise = std::nullopt;
  e.maybe_status = Status::kDegraded;
  e.maybe_name = std::string();
  e.maybe_blob = std::nullopt;
  e.numbers = {1, 2, 3, 65535};
  e.floats = {0.5, -0.5};
  e.statuses = {Status::kOk, Status::kUnknown, Status{200}};
  e.tags = {"a", "", "long tag value"};
  e.blobs = {{1}, {}, {2, 3}};
  e.headers = {Header{1, "eu"}, Header{2, ""}};
  e.header = Header{42, "us-east"};
  e.fixed_int = -123456;
  e.raw_int = -42;
  e.flags = {true, false, true};
  return e;
}

Tree SampleTree() {
  Tree right;
  right.value = 3;
  Tree left;
  left.value = 2;
  left.right = std::make_unique<Tree>(std::move(right));
  Tree root;
  root.value = 1;
  root.left = std::make_unique<Tree>(std::move(left));
  return root;
}

void TestEncodeMatchesFixtures(const std::vector<uint8_t>& everything_bin,
                               const std::vector<uint8_t>& tree_bin) {
  assert(SampleEverything().Encode() == everything_bin);
  assert(SampleTree().Encode() == tree_bin);
}

void TestDecodeFixtures(const std::vector<uint8_t>& everything_bin,
                        const std::vector<uint8_t>& tree_bin) {
  std::optional<Everything> e =
      Everything::Decode(everything_bin.data(), everything_bin.size());
  assert(e.has_value());
  assert(*e == SampleEverything());

  std::optional<Tree> t = Tree::Decode(tree_bin.data(), tree_bin.size());
  assert(t.has_value());
  assert(*t == SampleTree());

  // Re-encoding the decoded message reproduces the fixture bytes.
  assert(e->Encode() == everything_bin);
  assert(t->Encode() == tree_bin);
}

void TestViewAccessors(const std::vector<uint8_t>& everything_bin) {
  std::optional<EverythingView> parsed =
      EverythingView::Parse(everything_bin.data(), everything_bin.size());
  assert(parsed.has_value());
  const EverythingView& v = *parsed;

  assert(v.data() == everything_bin.data());
  assert(v.size() == everything_bin.size());

  assert(v.flag() == true);
  assert(v.tiny() == 7);
  assert(v.small() == -1234);
  assert(v.medium() == 0xdeadbeef);
  assert(v.big() == UINT64_MAX / 3);
  assert(v.sneg() == -5000000000);
  assert(v.ratio() == 1.25f);
  assert(v.precise() == -2.5e10);
  assert(v.status() == Status::kOk);
  assert(v.status().name() == std::string("ok"));
  assert(v.wide() == Wide::kNeg);

  assert(v.name().has_value() && *v.name() == "telemetry probe");
  assert(v.blob().has_value() &&
         v.blob()->ToVector() == std::vector<uint8_t>({0, 1, 2, 254, 255}));

  assert(v.maybe_flag().has_value() && *v.maybe_flag() == false);
  assert(v.maybe_small().has_value() && *v.maybe_small() == -7);
  assert(!v.maybe_precise().has_value());
  assert(v.maybe_status().has_value() && *v.maybe_status() == Status::kDegraded);
  assert(v.maybe_status()->name() == std::string("degraded"));
  assert(v.maybe_name().ok() && v.maybe_name().has_value() &&
         v.maybe_name().value().empty());
  assert(v.maybe_blob().ok() && !v.maybe_blob().has_value());

  std::optional<nanobuf::ScalarList<uint16_t>> numbers = v.numbers();
  assert(numbers.has_value() && numbers->size() == 4);
  assert((*numbers)[0] == 1 && (*numbers)[1] == 2 && (*numbers)[2] == 3 &&
         (*numbers)[3] == 65535);
  uint32_t sum = 0;
  for (uint16_t n : *numbers) {
    sum += n;
  }
  assert(sum == 1 + 2 + 3 + 65535);

  std::optional<nanobuf::ScalarList<double>> floats = v.floats();
  assert(floats.has_value() &&
         floats->ToVector() == std::vector<double>({0.5, -0.5}));

  std::optional<nanobuf::ScalarList<Status>> statuses = v.statuses();
  assert(statuses.has_value() && statuses->size() == 3);
  assert((*statuses)[0] == Status::kOk && (*statuses)[1] == Status::kUnknown &&
         (*statuses)[2] == Status{200});
  assert((*statuses)[2].name() == nullptr);

  std::optional<nanobuf::StrList> tags = v.tags();
  assert(tags.has_value() && tags->size() == 3);
  assert(*tags->Get(0) == "a");
  assert(*tags->Get(1) == "");
  assert(*tags->Get(2) == "long tag value");

  std::optional<nanobuf::BytesList> blobs = v.blobs();
  assert(blobs.has_value() && blobs->size() == 3);
  assert(blobs->Get(0)->ToVector() == std::vector<uint8_t>({1}));
  assert(blobs->Get(1)->ToVector() == std::vector<uint8_t>());
  assert(blobs->Get(2)->ToVector() == std::vector<uint8_t>({2, 3}));

  std::optional<nanobuf::MessageList<common::HeaderView>> headers = v.headers();
  assert(headers.has_value() && headers->size() == 2);
  std::optional<common::HeaderView> first = headers->Get(0);
  assert(first.has_value() && first->source() == 1 && *first->region() == "eu");
  std::optional<common::HeaderView> second = headers->Get(1);
  assert(second.has_value() && second->source() == 2 &&
         second->region()->empty());

  std::optional<common::HeaderView> header = v.header();
  assert(header.has_value());
  assert(header->source() == 42);
  assert(*header->region() == "us-east");

  assert(v.fixed_int() == -123456);
  assert(v.raw_int() == -42);

  std::optional<nanobuf::ScalarList<bool>> flags = v.flags();
  assert(flags.has_value() &&
         flags->ToVector() == std::vector<bool>({true, false, true}));
}

void TestTreeView(const std::vector<uint8_t>& tree_bin) {
  std::optional<TreeView> root = TreeView::Parse(tree_bin.data(), tree_bin.size());
  assert(root.has_value());
  assert(root->value() == 1);
  assert(!root->right().has_value());

  std::optional<TreeView> left = root->left();
  assert(left.has_value() && left->value() == 2);
  assert(!left->left().has_value());

  std::optional<TreeView> leaf = left->right();
  assert(leaf.has_value() && leaf->value() == 3);
  assert(!leaf->left().has_value() && !leaf->right().has_value());
}

void TestProto3(const std::vector<uint8_t>& pb3_bin) {
  Everything sample = SampleEverything();
  assert(sample.EncodeProto3() == pb3_bin);

  std::optional<Everything> decoded =
      Everything::DecodeProto3(pb3_bin.data(), pb3_bin.size());
  assert(decoded.has_value());
  assert(*decoded == sample);

  // Tree round trip through proto3.
  Tree tree = SampleTree();
  std::vector<uint8_t> tree_pb3 = tree.EncodeProto3();
  std::optional<Tree> tree_back =
      Tree::DecodeProto3(tree_pb3.data(), tree_pb3.size());
  assert(tree_back.has_value() && *tree_back == tree);

  // Unknown fields are skipped: field 99, varint 1.
  std::vector<uint8_t> extended = pb3_bin;
  nanobuf::proto::WriteVarintField(extended, 99, 1);
  std::optional<Everything> skipped =
      Everything::DecodeProto3(extended.data(), extended.size());
  assert(skipped.has_value() && *skipped == sample);

  // Unpacked scalars are accepted: field 19 (numbers) as a single varint.
  std::vector<uint8_t> unpacked;
  nanobuf::proto::WriteVarintField(unpacked, 19, 5);
  std::optional<Everything> one =
      Everything::DecodeProto3(unpacked.data(), unpacked.size());
  assert(one.has_value() && one->numbers == std::vector<uint16_t>({5}));

  // Groups (wire types 3 and 4) are rejected.
  const uint8_t group_start[] = {0x0b};
  assert(!Everything::DecodeProto3(group_start, 1).has_value());
  const uint8_t group_end[] = {0x0c};
  assert(!Everything::DecodeProto3(group_end, 1).has_value());

  // Malformed varints: more than 10 bytes, or a tenth byte above 1.
  std::vector<uint8_t> runaway = {0x08};
  runaway.insert(runaway.end(), 11, 0x80);
  assert(!Everything::DecodeProto3(runaway.data(), runaway.size()).has_value());
  std::vector<uint8_t> too_big = {0x08};
  too_big.insert(too_big.end(), 9, 0xff);
  too_big.push_back(0x02);
  assert(!Everything::DecodeProto3(too_big.data(), too_big.size()).has_value());
}

// Touches every accessor so the sanitizers can scan for out-of-bounds reads.
void Exercise(const EverythingView& v) {
  static_cast<void>(v.flag());
  static_cast<void>(v.tiny());
  static_cast<void>(v.small());
  static_cast<void>(v.medium());
  static_cast<void>(v.big());
  static_cast<void>(v.sneg());
  static_cast<void>(v.ratio());
  static_cast<void>(v.precise());
  static_cast<void>(v.status());
  static_cast<void>(v.wide());
  static_cast<void>(v.name());
  static_cast<void>(v.blob());
  static_cast<void>(v.maybe_flag());
  static_cast<void>(v.maybe_small());
  static_cast<void>(v.maybe_precise());
  static_cast<void>(v.maybe_status());
  static_cast<void>(v.maybe_name());
  static_cast<void>(v.maybe_blob());
  if (std::optional<nanobuf::ScalarList<uint16_t>> list = v.numbers()) {
    for (size_t i = 0; i < list->size(); ++i) static_cast<void>((*list)[i]);
  }
  if (std::optional<nanobuf::StrList> list = v.tags()) {
    for (size_t i = 0; i < list->size(); ++i) static_cast<void>(list->Get(i));
  }
  if (std::optional<nanobuf::BytesList> list = v.blobs()) {
    for (size_t i = 0; i < list->size(); ++i) static_cast<void>(list->Get(i));
  }
  if (std::optional<nanobuf::MessageList<common::HeaderView>> list = v.headers()) {
    for (size_t i = 0; i < list->size(); ++i) {
      if (std::optional<common::HeaderView> h = list->Get(i)) {
        static_cast<void>(h->source());
        static_cast<void>(h->region());
      }
    }
  }
  if (std::optional<common::HeaderView> h = v.header()) {
    static_cast<void>(h->source());
    static_cast<void>(h->region());
  }
  static_cast<void>(v.fixed_int());
  static_cast<void>(v.raw_int());
  static_cast<void>(v.flags());
  static_cast<void>(v.ToOwned());
}

void TestCorruptInput(const std::vector<uint8_t>& everything_bin,
                      const std::vector<uint8_t>& pb3_bin) {
  assert(!EverythingView::Parse(nullptr, 0).has_value());
  const uint8_t tiny_buf[] = {0x07};
  assert(!EverythingView::Parse(tiny_buf, 1).has_value());
  const uint8_t bad_header[] = {0x01, 0x00};  // fixed_len < 2
  assert(!EverythingView::Parse(bad_header, 2).has_value());
  const uint8_t short_fixed[] = {0x10, 0x00, 0x00};  // fixed_len > size
  assert(!EverythingView::Parse(short_fixed, 3).has_value());

  // Every truncation either fails to parse or degrades gracefully.
  for (size_t len = 0; len <= everything_bin.size(); ++len) {
    std::optional<EverythingView> view =
        EverythingView::Parse(everything_bin.data(), len);
    if (view.has_value()) {
      Exercise(*view);
    }
    static_cast<void>(Everything::Decode(everything_bin.data(), len));
  }

  // Single-byte corruptions never crash or read out of bounds.
  std::vector<uint8_t> mutated;
  for (size_t i = 0; i < everything_bin.size(); ++i) {
    for (uint8_t delta : {0x01, 0x80, 0xff}) {
      mutated = everything_bin;
      mutated[i] = static_cast<uint8_t>(mutated[i] ^ delta);
      std::optional<EverythingView> view =
          EverythingView::Parse(mutated.data(), mutated.size());
      if (view.has_value()) {
        Exercise(*view);
      }
    }
  }

  // A heap slot pointing past the end fails cleanly. Slot 44 is `name`.
  mutated = everything_bin;
  nanobuf::StoreLe<uint32_t>(mutated.data() + 44, 0x7fffffff);
  std::optional<EverythingView> view =
      EverythingView::Parse(mutated.data(), mutated.size());
  assert(view.has_value());
  assert(!view->name().has_value());
  assert(!view->ToOwned().has_value());
  assert(!Everything::Decode(mutated.data(), mutated.size()).has_value());

  // Invalid UTF-8 in a string heap entry is corruption, not data.
  mutated = everything_bin;
  mutated[0x78] = 0xff;  // first byte of "telemetry probe"
  view = EverythingView::Parse(mutated.data(), mutated.size());
  assert(view.has_value());
  assert(!view->name().has_value());
  assert(!view->ToOwned().has_value());

  // Proto3 decoding survives truncation and corruption.
  for (size_t len = 0; len <= pb3_bin.size(); ++len) {
    static_cast<void>(Everything::DecodeProto3(pb3_bin.data(), len));
  }
  for (size_t i = 0; i < pb3_bin.size(); ++i) {
    mutated = pb3_bin;
    mutated[i] = static_cast<uint8_t>(mutated[i] ^ 0xff);
    static_cast<void>(Everything::DecodeProto3(mutated.data(), mutated.size()));
  }
}

void TestOldDataReadsAsDefaults() {
  // A writer built from an older schema that only knew `flag` and `tiny`:
  // fixed_len 4, bit pool byte at 2, tiny at 3. Everything else must read
  // as zero / false / empty / absent.
  const uint8_t old_data[] = {0x04, 0x00, 0x01, 0x07};
  std::optional<EverythingView> view = EverythingView::Parse(old_data, 4);
  assert(view.has_value());
  assert(view->flag() == true);
  assert(view->tiny() == 7);
  assert(view->small() == 0);
  assert(view->medium() == 0);
  assert(view->big() == 0);
  assert(view->ratio() == 0.0f);
  assert(view->precise() == 0.0);
  assert(view->status() == Status::kUnknown);
  assert(view->wide() == Wide::kZero);
  assert(view->name().has_value() && view->name()->empty());
  assert(view->blob().has_value() && view->blob()->size == 0);
  assert(!view->maybe_flag().has_value());
  assert(!view->maybe_small().has_value());
  assert(!view->maybe_precise().has_value());
  assert(!view->maybe_status().has_value());
  assert(view->maybe_name().ok() && !view->maybe_name().has_value());
  assert(view->maybe_blob().ok() && !view->maybe_blob().has_value());
  assert(view->numbers().has_value() && view->numbers()->empty());
  assert(view->tags().has_value() && view->tags()->empty());
  assert(view->blobs().has_value() && view->blobs()->empty());
  assert(view->headers().has_value() && view->headers()->empty());
  assert(!view->header().has_value());
  assert(view->fixed_int() == 0);
  assert(view->raw_int() == 0);
  assert(view->flags().has_value() && view->flags()->empty());

  std::optional<Everything> owned = view->ToOwned();
  assert(owned.has_value());
  Everything expected;
  expected.flag = true;
  expected.tiny = 7;
  assert(*owned == expected);
}

void TestEmptyAndAbsentAreIdentical() {
  // Non-optional empty string/bytes/lists write nothing: the encoded
  // message is exactly the fixed section.
  Everything empty;
  std::vector<uint8_t> bytes = empty.Encode();
  assert(bytes.size() == Everything::kFixedLen);
  std::optional<Everything> back = Everything::Decode(bytes.data(), bytes.size());
  assert(back.has_value() && *back == empty);

  // An optional string holding "" is present, not absent.
  Everything with_empty;
  with_empty.maybe_name = std::string();
  std::vector<uint8_t> forced = with_empty.Encode();
  assert(forced.size() > Everything::kFixedLen);
  std::optional<EverythingView> view =
      EverythingView::Parse(forced.data(), forced.size());
  assert(view.has_value());
  assert(view->maybe_name().ok() && view->maybe_name().has_value());

  // Default proto3 encoding is empty: every field is at its default.
  assert(empty.EncodeProto3().empty());
}

}  // namespace

int main(int argc, char** argv) {
  std::string fixtures = argc > 1 ? argv[1] : "conformance/fixtures";
  std::vector<uint8_t> everything_bin = ReadFile(fixtures + "/everything.nb.bin");
  std::vector<uint8_t> tree_bin = ReadFile(fixtures + "/tree.nb.bin");
  std::vector<uint8_t> pb3_bin = ReadFile(fixtures + "/everything.pb3.bin");

  TestEncodeMatchesFixtures(everything_bin, tree_bin);
  TestDecodeFixtures(everything_bin, tree_bin);
  TestViewAccessors(everything_bin);
  TestTreeView(tree_bin);
  TestProto3(pb3_bin);
  TestCorruptInput(everything_bin, pb3_bin);
  TestOldDataReadsAsDefaults();
  TestEmptyAndAbsentAreIdentical();

  std::printf("conformance: all tests passed\n");
  return 0;
}
