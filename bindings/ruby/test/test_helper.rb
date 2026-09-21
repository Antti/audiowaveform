# frozen_string_literal: true

require "minitest/autorun"
require "tmpdir"
require "json"
require "pathname"
require "audiowaveform"

module AudioWaveformTestSupport
  ROOT = File.expand_path("../../..", __dir__)

  def fixture(name)
    File.join(ROOT, "tests/fixtures/generated", name)
  end

  # Sparse silence avoids allocating a full PCM buffer in the test process.
  def write_silence_wav(path, frame_count:)
    bytes = frame_count * 2
    header = ["RIFF", 36 + bytes, "WAVE", "fmt ", 16, 1, 1,
      16_000, 32_000, 2, 16, "data", bytes].pack("a4Va4a4VvvVVvva4V")
    File.open(path, "wb") do |file|
      file.write(header)
      file.truncate(header.bytesize + bytes)
    end
  end
end
