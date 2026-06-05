// Conformance test for the C# backend. Mirrors conformance/src/lib.rs; the
// sample values here must stay byte-identical with the Rust fixtures.

using System;
using System.Collections.Generic;
using System.IO;
using Common;
using Demo.Telemetry;

namespace Nanobuf.ConformanceTest {
    internal static class Program {
        private static int _checks;

        private static int Main() {
            try {
                RunAll();
            } catch (Exception e) {
                Console.Error.WriteLine(e.Message);
                return 1;
            }
            Console.WriteLine($"ALL TESTS PASSED ({_checks} checks)");
            return 0;
        }

        private static void RunAll() {
            string fixtures = FindFixtures();
            byte[] everythingBin = File.ReadAllBytes(Path.Combine(fixtures, "everything.nb.bin"));
            byte[] treeBin = File.ReadAllBytes(Path.Combine(fixtures, "tree.nb.bin"));
            byte[] everythingPb3 = File.ReadAllBytes(Path.Combine(fixtures, "everything.pb3.bin"));

            EncodeMatchesFixtures(everythingBin, treeBin);
            DecodeMatchesSample(everythingBin, treeBin);
            ViewMatchesSample(everythingBin, treeBin);
            Proto3RoundTrips(everythingPb3);
            CorruptBuffersThrow(everythingBin);
            OldDataReadsAsDefaults();
        }

        private static string FindFixtures() {
            var dir = new DirectoryInfo(Directory.GetCurrentDirectory());
            while (dir is not null) {
                string candidate = Path.Combine(dir.FullName, "conformance", "fixtures");
                if (File.Exists(Path.Combine(candidate, "everything.nb.bin"))) {
                    return candidate;
                }
                dir = dir.Parent;
            }
            throw new Exception("cannot locate conformance/fixtures; run from inside the repo");
        }

        private static Everything SampleEverything() => new() {
            Flag = true,
            Tiny = 7,
            Small = -1234,
            Medium = 0xdeadbeef,
            Big = ulong.MaxValue / 3,
            Sneg = -5_000_000_000,
            Ratio = 1.25f,
            Precise = -2.5e10,
            Status = Status.Ok,
            Wide = Wide.Neg,
            Name = "telemetry probe",
            Blob = new byte[] { 0, 1, 2, 254, 255 },
            MaybeFlag = false,
            MaybeSmall = -7,
            MaybePrecise = null,
            MaybeStatus = Status.Degraded,
            MaybeName = "",
            MaybeBlob = null,
            Numbers = new List<ushort> { 1, 2, 3, 65535 },
            Floats = new List<double> { 0.5, -0.5 },
            Statuses = new List<Status> { Status.Ok, Status.Unknown, new(200) },
            Tags = new List<string> { "a", "", "long tag value" },
            Blobs = new List<byte[]> { new byte[] { 1 }, Array.Empty<byte>(), new byte[] { 2, 3 } },
            Headers = new List<Header> {
                new() { Source = 1, Region = "eu" },
                new() { Source = 2, Region = "" },
            },
            Header = new Header { Source = 42, Region = "us-east" },
            FixedInt = -123456,
            RawInt = -42,
            Flags = new List<bool> { true, false, true },
        };

        private static Tree SampleTree() => new() {
            Value = 1,
            Left = new Tree {
                Value = 2,
                Right = new Tree { Value = 3 },
            },
            Right = null,
        };

        private static void EncodeMatchesFixtures(byte[] everythingBin, byte[] treeBin) {
            CheckBytes(SampleEverything().Encode(), everythingBin, "Encode() == everything.nb.bin");
            CheckBytes(SampleTree().Encode(), treeBin, "Encode() == tree.nb.bin");
        }

        private static void DecodeMatchesSample(byte[] everythingBin, byte[] treeBin) {
            Check(Everything.Decode(everythingBin).Equals(SampleEverything()),
                "Decode(everything.nb.bin) equals sample");
            Check(Tree.Decode(treeBin).Equals(SampleTree()), "Decode(tree.nb.bin) equals sample");
        }

        private static void ViewMatchesSample(byte[] everythingBin, byte[] treeBin) {
            var view = EverythingView.Parse(everythingBin);
            Check(view.Flag, "view.Flag");
            Check(view.Tiny == 7, "view.Tiny");
            Check(view.Small == -1234, "view.Small");
            Check(view.Medium == 0xdeadbeef, "view.Medium");
            Check(view.Big == ulong.MaxValue / 3, "view.Big");
            Check(view.Sneg == -5_000_000_000, "view.Sneg");
            Check(view.Ratio == 1.25f, "view.Ratio");
            Check(view.Precise == -2.5e10, "view.Precise");
            Check(view.Status == Status.Ok, "view.Status");
            Check(view.Status.Name() == "ok", "view.Status.Name()");
            Check(view.Wide == Wide.Neg, "view.Wide");
            Check(view.Name == "telemetry probe", "view.Name");
            Check(view.Blob.SequenceEqual(new byte[] { 0, 1, 2, 254, 255 }), "view.Blob");
            Check(view.MaybeFlag == false, "view.MaybeFlag");
            Check(view.MaybeSmall == -7, "view.MaybeSmall");
            Check(view.MaybePrecise == null, "view.MaybePrecise");
            Check(view.MaybeStatus == Status.Degraded, "view.MaybeStatus");
            Check(view.MaybeName == "", "view.MaybeName");
            Check(!view.TryGetMaybeBlob(out _), "view.MaybeBlob absent");

            var numbers = view.Numbers;
            Check(numbers.Count == 4 && numbers[0] == 1 && numbers[1] == 2 && numbers[2] == 3
                && numbers[3] == 65535, "view.Numbers");
            var floats = view.Floats;
            Check(floats.Count == 2 && floats[0] == 0.5 && floats[1] == -0.5, "view.Floats");
            var statuses = view.Statuses;
            Check(statuses.Count == 3 && statuses[0] == Status.Ok && statuses[1] == Status.Unknown
                && statuses[2] == new Status(200), "view.Statuses");
            Check(statuses[2].Name() == null, "unknown enum value has no name");
            var tags = view.Tags;
            Check(tags.Count == 3 && tags[0] == "a" && tags[1] == "" && tags[2] == "long tag value",
                "view.Tags");
            var blobs = view.Blobs;
            Check(blobs.Count == 3 && blobs[0].SequenceEqual(new byte[] { 1 })
                && blobs[1].Length == 0 && blobs[2].SequenceEqual(new byte[] { 2, 3 }), "view.Blobs");
            Check(view.Headers.Count == 2, "view.Headers.Count");
            Check(view.HeadersAt(0).Source == 1 && view.HeadersAt(0).Region == "eu", "view.HeadersAt(0)");
            Check(view.HeadersAt(1).Source == 2 && view.HeadersAt(1).Region == "", "view.HeadersAt(1)");
            Check(view.TryGetHeader(out var header), "view.Header present");
            Check(header.Source == 42 && header.Region == "us-east", "view.Header fields");
            Check(view.FixedInt == -123456, "view.FixedInt");
            Check(view.RawInt == -42, "view.RawInt");
            var flags = view.Flags;
            Check(flags.Count == 3 && flags[0] && !flags[1] && flags[2], "view.Flags");
            Check(view.ToOwned().Equals(SampleEverything()), "view.ToOwned() equals sample");
            Check(view.Data.SequenceEqual(everythingBin), "view.Data round trips");

            var tree = TreeView.Parse(treeBin);
            Check(tree.Value == 1, "tree.Value");
            Check(tree.TryGetLeft(out var left), "tree.Left present");
            Check(left.Value == 2, "tree.Left.Value");
            Check(!left.TryGetLeft(out _), "tree.Left.Left absent");
            Check(left.TryGetRight(out var leftRight), "tree.Left.Right present");
            Check(leftRight.Value == 3, "tree.Left.Right.Value");
            Check(!tree.TryGetRight(out _), "tree.Right absent");
        }

        private static void Proto3RoundTrips(byte[] everythingPb3) {
            var sample = SampleEverything();
            CheckBytes(sample.EncodeProto3(), everythingPb3, "EncodeProto3() == everything.pb3.bin");
            Check(Everything.DecodeProto3(everythingPb3).Equals(sample),
                "DecodeProto3(everything.pb3.bin) equals sample");
            var tree = SampleTree();
            Check(Tree.DecodeProto3(tree.EncodeProto3()).Equals(tree), "Tree proto3 round trip");
        }

        private static void CorruptBuffersThrow(byte[] everythingBin) {
            Throws("empty buffer", () => Everything.Decode(Array.Empty<byte>()));
            Throws("one-byte buffer", () => Everything.Decode(new byte[] { 5 }));
            Throws("fixed_len < 2", () => Everything.Decode(new byte[] { 1, 0 }));
            Throws("fixed_len past end", () => Everything.Decode(new byte[] { 50, 0, 0 }));

            byte[] truncated = everythingBin.AsSpan(0, 50).ToArray();
            Throws("truncated fixed section", () => Everything.Decode(truncated));

            // Point the name slot (offset 44) past the end of the buffer.
            byte[] badName = (byte[])everythingBin.Clone();
            BitConverter.GetBytes(0x00ffffffu).CopyTo(badName, 44);
            Throws("heap offset past end", () => {
                var view = EverythingView.Parse(badName);
                _ = view.Name;
            });
            Throws("heap offset past end via Decode", () => Everything.Decode(badName));

            // Blow up the count of the numbers list (slot 72).
            byte[] badList = (byte[])everythingBin.Clone();
            uint numbersEntry = BitConverter.ToUInt32(badList, 72);
            BitConverter.GetBytes(0xffffffffu).CopyTo(badList, (int)numbersEntry);
            Throws("oversized list count", () => {
                var view = EverythingView.Parse(badList);
                _ = view.Numbers;
            });

            // Invalid UTF-8 in the name payload.
            byte[] badUtf8 = (byte[])everythingBin.Clone();
            uint nameEntry = BitConverter.ToUInt32(badUtf8, 44);
            badUtf8[nameEntry + 4] = 0xff;
            Throws("invalid UTF-8", () => {
                var view = EverythingView.Parse(badUtf8);
                _ = view.Name;
            });

            Throws("proto3 wire type 3", () => Everything.DecodeProto3(new byte[] { 0x0b }));
            byte[] longVarint = new byte[11];
            longVarint[0] = 0x08;
            Array.Fill(longVarint, (byte)0x80, 1, 10);
            Throws("proto3 varint > 10 bytes", () => Everything.DecodeProto3(longVarint));
            byte[] overflowVarint = new byte[11];
            overflowVarint[0] = 0x08;
            Array.Fill(overflowVarint, (byte)0xff, 1, 9);
            overflowVarint[10] = 0x02;
            Throws("proto3 tenth varint byte > 1", () => Everything.DecodeProto3(overflowVarint));
            Throws("proto3 truncated length", () =>
                Everything.DecodeProto3(new byte[] { 0x5a, 0x05, 0x01 }));
        }

        private static void OldDataReadsAsDefaults() {
            // A writer with an older schema only knew the two header bytes.
            byte[] empty = { 2, 0 };
            Check(Everything.Decode(empty).Equals(new Everything()), "old data decodes to defaults");
            var view = EverythingView.Parse(empty);
            Check(!view.Flag, "old data: Flag default");
            Check(view.Tiny == 0, "old data: Tiny default");
            Check(view.Big == 0, "old data: Big default");
            Check(view.Status == Status.Unknown, "old data: Status default");
            Check(view.Name == "", "old data: Name default");
            Check(view.Blob.Length == 0, "old data: Blob default");
            Check(view.MaybeFlag == null, "old data: MaybeFlag absent");
            Check(view.MaybeSmall == null, "old data: MaybeSmall absent");
            Check(view.Numbers.Count == 0, "old data: Numbers empty");
            Check(view.Tags.Count == 0, "old data: Tags empty");
            Check(view.Headers.Count == 0, "old data: Headers empty");
            Check(!view.TryGetHeader(out _), "old data: Header absent");

            // A writer that knew only flag (bit 2.0) and tiny (byte 3).
            byte[] partial = { 4, 0, 0x01, 7 };
            var partialView = EverythingView.Parse(partial);
            Check(partialView.Flag, "partial data: Flag");
            Check(partialView.Tiny == 7, "partial data: Tiny");
            Check(partialView.Small == 0, "partial data: Small default");
            Check(partialView.MaybeFlag == null, "partial data: MaybeFlag absent");
        }

        private static void Check(bool condition, string what) {
            _checks++;
            if (!condition) {
                throw new Exception($"FAILED: {what}");
            }
        }

        private static void CheckBytes(byte[] actual, byte[] expected, string what) {
            _checks++;
            if (actual.AsSpan().SequenceEqual(expected)) {
                return;
            }
            int limit = Math.Min(actual.Length, expected.Length);
            int at = limit;
            for (int i = 0; i < limit; i++) {
                if (actual[i] != expected[i]) {
                    at = i;
                    break;
                }
            }
            throw new Exception(
                $"FAILED: {what}: lengths {actual.Length}/{expected.Length}, first mismatch at {at}");
        }

        private static void Throws(string what, Action action) {
            _checks++;
            try {
                action();
            } catch (Nanobuf.DecodeException) {
                return;
            } catch (Exception e) {
                throw new Exception(
                    $"FAILED: {what}: threw {e.GetType().Name} instead of DecodeException");
            }
            throw new Exception($"FAILED: {what}: no exception thrown");
        }
    }
}
