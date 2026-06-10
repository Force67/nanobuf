// Encode a sensor reading, read it back through a zero-copy view, then
// decode it into an owned value.

#include <cassert>
#include <cstdio>
#include <vector>

#include "sensor.nb.h"

using example::sensor::Quality;
using example::sensor::Reading;
using example::sensor::ReadingView;

int main() {
  Reading reading;
  reading.probe = 17;
  reading.unix_ms = 1765432100000;
  reading.celsius = 21.5;
  reading.quality = Quality::kGood;
  reading.site = "greenhouse-3";
  reading.samples = {2150, 2152, 2149};
  reading.battery = 0.75f;

  std::vector<uint8_t> bytes = reading.Encode();
  std::printf("encoded %zu bytes\n", bytes.size());

  // Field access reads straight from `bytes`: no allocation, no parse pass.
  std::optional<ReadingView> view = ReadingView::Parse(bytes.data(), bytes.size());
  if (!view) {
    std::fprintf(stderr, "corrupt buffer\n");
    return 1;
  }
  std::string_view site = view->site().value_or("?");
  const char* quality = view->quality().name();
  std::printf("probe %u at %.*s: %.1f C (%s)\n", view->probe(),
              static_cast<int>(site.size()), site.data(), view->celsius(),
              quality ? quality : "unlisted");
  if (std::optional<float> battery = view->battery()) {
    std::printf("battery %.0f%%\n", *battery * 100.0f);
  }
  nanobuf::ScalarList<uint16_t> samples = view->samples().value();
  std::printf("samples: [");
  for (size_t i = 0; i < samples.size(); ++i) {
    std::printf("%s%u", i ? ", " : "", samples[i]);
  }
  std::printf("]\n");

  // Owned decode for when the data must outlive the buffer.
  std::optional<Reading> decoded = Reading::Decode(bytes.data(), bytes.size());
  assert(decoded && *decoded == reading);
  std::printf("round trip ok\n");
  return 0;
}
