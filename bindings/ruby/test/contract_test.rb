# frozen_string_literal: true

# Tests a separately installed replacement. Never supplies old-library outputs
# as an oracle. Fixture expectations are literal values in vectors.json.
require "json"
require "digest"
require "pathname"

FIXTURES = File.expand_path("../../../tests/fixtures/generated", __dir__)
VECTOR_PATH = File.expand_path("../../../contract/vectors.json", __dir__)
CORPUS = JSON.parse(File.read(VECTOR_PATH))
manifest = JSON.parse(File.read(File.join(FIXTURES, "manifest.json")))
abort "Fixtures were built from different vectors" unless manifest.fetch("vectors_sha256") == Digest::SHA256.file(VECTOR_PATH).hexdigest
manifest.fetch("files").each do |name, digest|
  abort "Unexpected fixture filename" unless name == File.basename(name)
  abort "Fixture checksum mismatch: #{name}" unless Digest::SHA256.file(File.join(FIXTURES, name)).hexdigest == digest
end

require "audiowaveform"
require "minitest/autorun"

class ReplacementContractTest < Minitest::Test
  def fixture(name = "uneven")
    File.join(FIXTURES, "#{name}.wav")
  end

  CORPUS.fetch("cases").each do |vector|
    define_method("test_vector_#{vector.fetch('id')}") do
      options = vector.fetch("options").transform_keys(&:to_sym)
      options[:amplitude_scale] = :auto if options[:amplitude_scale] == "auto"
      waveform = AudioWaveform.generate(fixture(vector.fetch("fixture")), **options)
      expected = vector.fetch("expected")
      %w[sample_rate channels length samples_per_pixel].each do |field|
        assert_equal expected.fetch(field), waveform.public_send(field), field
      end
      assert_equal expected.fetch("length"), waveform.size
      assert_equal expected.fetch("length").zero?, waveform.empty?
      assert_equal 16, waveform.bits
      assert_equal 16, waveform.storage_bits
      duration = expected.fetch("frames").fdiv(expected.fetch("sample_rate"))
      assert_in_delta duration, waveform.duration, 1e-12
      assert_in_delta duration, waveform.duration_seconds, 1e-12
      assert_equal expected.fetch("data16"), waveform.data
      assert_equal expected.fetch("data16"), waveform.data(bits: 16)
      assert_equal expected.fetch("data8"), waveform.data(bits: 8)
      assert waveform.data.all? { |value| value.is_a?(Integer) }
      expected.fetch("length").times do |point|
        expected.fetch("channels").times do |channel|
          offset = 2 * (point * expected.fetch("channels") + channel)
          assert_equal expected.fetch("data16").slice(offset, 2), waveform.point(point, channel: channel)
        end
      end
    end
  end

  def test_arrays_are_independent_and_bits_do_not_mutate_the_waveform
    waveform = AudioWaveform.generate(fixture, points: 3)
    first = waveform.data
    original = first.dup
    first.fill(123)
    waveform.data(bits: 8).clear
    waveform.point(0).fill(456)
    assert_equal original, waveform.data
    assert_equal original.first(2), waveform.point(0)
    refute_same waveform.data, waveform.data
    assert_equal 16, waveform.bits
  end

  def test_pathlike_input_and_string_auto
    from_string = AudioWaveform.generate(fixture, points: 3, amplitude_scale: :auto)
    from_path = AudioWaveform.generate(Pathname.new(fixture), points: 3, amplitude_scale: "auto")
    assert_equal from_string.data, from_path.data
  end

  def test_resolution_validation
    bad_values = [0, -1, 2**32, 1.5, "3", true, false]
    %i[points pixels_per_second samples_per_pixel].each do |keyword|
      bad_values.each do |value|
        assert_raises(ArgumentError, "#{keyword}=#{value.inspect}") do
          AudioWaveform.generate(fixture, **{keyword => value})
        end
      end
    end
    assert_raises(ArgumentError) { AudioWaveform.generate(fixture, samples_per_pixel: 1) }
    %i[points samples_per_pixel pixels_per_second].combination(2).each do |keys|
      assert_raises(ArgumentError) { AudioWaveform.generate(fixture, **keys.to_h { |key| [key, 4] }) }
    end
    assert_raises(ArgumentError) { AudioWaveform.generate(fixture, unknown_option: true) }
  end

  def test_gain_and_channel_validation
    [-1, Float::NAN, Float::INFINITY, -Float::INFINITY, Complex(1, 1), "1.5", :unknown, true].each do |gain|
      assert_raises(ArgumentError, "gain=#{gain.inspect}") do
        AudioWaveform.generate(fixture, amplitude_scale: gain)
      end
    end
    [nil, 0, 1, "true"].each do |split|
      assert_raises(ArgumentError) { AudioWaveform.generate(fixture, split_channels: split) }
    end
  end

  def test_bits_and_point_validation
    waveform = AudioWaveform.generate(fixture, points: 3)
    [0, 7, 32, 8.0, "8", nil, true].each do |bits|
      assert_raises(ArgumentError) { waveform.data(bits: bits) }
    end
    [-1, 3, 2**64].each do |index|
      assert_raises(IndexError) { waveform.point(index) }
    end
    [-1, 1, 2**64].each do |channel|
      assert_raises(IndexError) { waveform.point(0, channel: channel) }
    end
    [0.0, "0", nil, true].each do |value|
      assert_raises(ArgumentError) { waveform.point(value) }
      assert_raises(ArgumentError) { waveform.point(0, channel: value) }
    end
    empty = AudioWaveform.generate(fixture("empty"), points: 3)
    assert_raises(IndexError) { empty.point(0) }
  end

  def test_input_errors
    assert_raises(AudioWaveform::Error) { AudioWaveform.generate(fixture("does_not_exist")) }
    %w[invalid_header nonfinite_nan nonfinite_inf].each do |name|
      assert_raises(AudioWaveform::Error, name) { AudioWaveform.generate(fixture(name)) }
    end
    [nil, 123, Object.new, "bad\0path"].each do |path|
      assert_raises(ArgumentError) { AudioWaveform.generate(path) }
    end
    # Validation must happen before attempting input access.
    assert_raises(ArgumentError) { AudioWaveform.generate(fixture("does_not_exist"), points: 0) }
  end

  def test_no_library_export_methods
    # Ignore generic Object/Kernel methods supplied by other Ruby libraries.
    ancestors = AudioWaveform::Waveform.ancestors.take_while { |ancestor| ancestor != Object }
    own_methods = ancestors.flat_map { |ancestor| ancestor.instance_methods(false) }
    %i[save to_dat to_json to_txt].each { |name| refute_includes own_methods, name }
  end
end
