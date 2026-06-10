"""Encode a sensor reading, read it back through a zero-copy view, then
decode it into an owned value."""

import sensor

reading = sensor.Reading(
    probe=17,
    unix_ms=1_765_432_100_000,
    celsius=21.5,
    quality=sensor.Quality.GOOD,
    site="greenhouse-3",
    samples=[2150, 2152, 2149],
    battery=0.75,
)

data = reading.encode()
print(f"encoded {len(data)} bytes")

# Field access reads straight from `data`: no parse pass.
view = sensor.ReadingView(data)
print(f"probe {view.probe} at {view.site}: {view.celsius:.1f} C ({view.quality.label()})")
if view.battery is not None:
    print(f"battery {view.battery * 100:.0f}%")
print(f"samples: {view.samples.to_list()}")

# Owned decode for when the data must outlive the buffer.
assert sensor.Reading.decode(data) == reading
print("round trip ok")
