// Encode a sensor reading, read it back through a zero-copy view, then
// decode it into an owned value.

using System;
using System.Collections.Generic;
using Example.Sensor;

var reading = new Reading {
    Probe = 17,
    UnixMs = 1_765_432_100_000,
    Celsius = 21.5,
    Quality = Quality.Good,
    Site = "greenhouse-3",
    Samples = new List<ushort> { 2150, 2152, 2149 },
    Battery = 0.75f,
};

byte[] bytes = reading.Encode();
Console.WriteLine($"encoded {bytes.Length} bytes");

// Field access reads straight from `bytes`: no allocation, no parse pass.
var view = ReadingView.Parse(bytes);
Console.WriteLine($"probe {view.Probe} at {view.Site}: {view.Celsius:F1} C ({view.Quality})");
if (view.Battery is float battery) {
    Console.WriteLine($"battery {battery * 100:F0}%");
}
var samples = view.Samples;
var parts = new string[samples.Count];
for (int i = 0; i < samples.Count; i++) {
    parts[i] = samples[i].ToString();
}
Console.WriteLine($"samples: [{string.Join(", ", parts)}]");

// Owned decode for when the data must outlive the buffer.
Reading decoded = Reading.Decode(bytes);
if (!decoded.Equals(reading)) {
    throw new Exception("round trip mismatch");
}
Console.WriteLine("round trip ok");
