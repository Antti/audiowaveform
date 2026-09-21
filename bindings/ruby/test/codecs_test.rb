# frozen_string_literal: true

require_relative "test_helper"

class CodecsTest < Minitest::Test
  def test_aac_mp4_uses_playback_duration_and_preserves_silence
    directory = File.expand_path("../../../tests/fixtures/aac", __dir__)
    manifest = JSON.parse(File.read(File.join(directory, "manifest.json")))
    manifest.fetch("cases").reject { |entry| entry["fragmented"] }.each do |entry|
      frames = entry.fetch("playback_frames", entry.fetch("frames"))
      [{points: 110}, {samples_per_pixel: 23}].each do |options|
        [false, true].each do |split|
          w = AudioWaveform.generate(File.join(directory, entry.fetch("file")), split_channels: split, **options)
          assert_equal frames.fdiv(entry.fetch("rate")), w.duration, entry.fetch("file")
          assert_equal(options[:points] || (frames.fdiv(23)).ceil, w.length)
          assert_equal(split ? entry.fetch("channels") : 1, w.channels)
          assert_equal w.data.map { |value| (value.fdiv(256)).truncate }, w.data(bits: 8)
          assert w.data.all?(&:zero?) if entry.fetch("signal") == "silence"
          assert_equal [0, 0], w.point(0) if entry.fetch("leading_frames", 0) > 0
          if entry.fetch("signal") == "edges"
            assert_equal [0, 0], w.point(0)
            assert_equal [0, 0], w.point(w.length - 1)
          end
        end
      end
    end
  end

  def test_gapless_audio_has_no_leading_priming_buckets
    directory = File.expand_path("../../../tests/fixtures/codecs", __dir__)
    %w[audio.mp3 vorbis.ogg live.webm].each do |file|
      [{points: 110}, {samples_per_pixel: 256}].each do |options|
        w = AudioWaveform.generate(File.join(directory, file), **options)
        file == "audio.mp3" ? assert_equal(0.25, w.duration) : assert_includes(12000..12288, (w.duration * 48000).round)
        min, max = w.point(0)
        assert_operator min, :<, -8000
        assert_operator max, :>, 8000
      end
    end
  end

  def test_generated_codec_matrix
    directory = File.expand_path("../../../tests/fixtures/codecs", __dir__)
    manifest = JSON.parse(File.read(File.join(directory, "manifest.json")))
    manifest.fetch("cases").each do |entry|
      [false, true].each do |split|
        waveform = AudioWaveform.generate(File.join(directory, entry.fetch("file")), points: 110, split_channels: split)
        assert_equal 110, waveform.length, entry.fetch("file")
        assert_equal 48_000, waveform.sample_rate
        assert_operator waveform.duration, :>, 0
        assert_equal waveform.length * waveform.channels * 2, waveform.data.length
        assert waveform.data.each_slice(2).all? { |min, max| min <= max }
        if entry["lossless"]
          assert_equal 0.25, waveform.duration
          assert_equal manifest.fetch("expected16").fetch(split ? "split" : "mono"), waveform.data
        end
        assert waveform.data.all?(&:zero?) if entry["silent"]
      end
    end
  end
end
