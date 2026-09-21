# frozen_string_literal: true

require_relative "test_helper"
require "stringio"
require "timeout"
require "objspace"

class PcmTest < Minitest::Test
  OPTIONS = {format: :s16le, sample_rate: 8000, channels: 1, samples_per_pixel: 2}.freeze

  class FragmentedInput
    attr_reader :buffers, :requests

    def initialize(bytes, chunk: 7)
      @bytes, @chunk, @offset = bytes, chunk, 0
      @buffers, @requests = [], []
    end

    def read(length, outbuf)
      @buffers << outbuf.object_id
      @requests << length
      return nil if @offset == @bytes.bytesize
      part = @bytes.byteslice(@offset, [length, @chunk].min)
      @offset += part.bytesize
      outbuf.replace(part)
    end
  end

  def test_all_encodings_and_short_reads_match_known_stereo_peaks
    samples = [-1.0, 0.5, 0, -0.5, 0.25, -0.25]
    encodings = {
      u8: samples.map { |v| (v * 128 + 128).to_i }.pack("C*"),
      s8: samples.map { |v| (v * 128).to_i }.pack("c*"),
      s16le: samples.map { |v| (v * 32768).to_i }.pack("s<*"),
      s16be: samples.map { |v| (v * 32768).to_i }.pack("s>*"),
      s24le: samples.map { |v| [(v * 8_388_608).to_i].pack("l<").byteslice(0, 3) }.join,
      s24be: samples.map { |v| [(v * 8_388_608).to_i].pack("l>").byteslice(1, 3) }.join,
      s32le: samples.map { |v| (v * 2_147_483_648).to_i }.pack("l<*"),
      s32be: samples.map { |v| (v * 2_147_483_648).to_i }.pack("l>*"),
      f32le: samples.pack("e*"), f32be: samples.pack("g*"),
      f64le: samples.pack("E*"), f64be: samples.pack("G*")
    }
    encodings.each do |format, bytes|
      [1, 3, 7, 32_768].each do |chunk|
        input = FragmentedInput.new(bytes, chunk: chunk)
        w = AudioWaveform.generate_pcm(input, **OPTIONS, format: format, channels: 2, split_channels: true)
        assert_equal [-32768, 0, -16384, 16384, 8192, 8192, -8192, -8192], w.data, "#{format}/#{chunk}"
        assert_equal [-128, 0, -64, 64, 32, 32, -32, -32], w.data(bits: 8)
        assert_equal 3.0 / 8000, w.duration
        assert_equal 2, w.length
        assert_equal 2, w.channels
        assert_equal 1, input.buffers.uniq.length
        assert_equal [32_768], input.requests.uniq
      end
    end
  end

  def test_stringio_is_consumed_from_current_position_and_kept_open
    input = StringIO.new("prefix".b + [-2000, 1000, -500, 3000, 7].pack("s<*"))
    input.pos = 6
    w = AudioWaveform.generate_pcm(input, **OPTIONS)
    assert_equal [-2000, 1000, -500, 3000, 7, 7], w.data
    assert_equal 5.0 / 8000, w.duration
    assert input.eof?
    refute input.closed?
  end

  def test_pipe_input_preserves_peaks_without_seeking
    reader, writer = IO.pipe
    reader.binmode
    writer.binmode
    bytes = [-2000, 1000, -500, 3000, 7].pack("s<*")
    producer = Thread.new do
      bytes.each_byte { |byte| writer.write(byte.chr); Thread.pass }
    ensure
      writer.close
    end
    waveform = Timeout.timeout(5) { AudioWaveform.generate_pcm(reader, **OPTIONS) }
    assert_equal [-2000, 1000, -500, 3000, 7, 7], waveform.data
    assert_equal bytes.bytesize / 2.0 / 8000, waveform.duration
    refute reader.closed?
    producer.value
  ensure
    reader&.close unless reader&.closed?
    producer&.join(1)
    producer&.kill if producer&.alive?
    writer&.close unless writer&.closed?
  end

  def test_mixing_gain_and_auto_happen_before_quantization
    data = [0.0000152587890625, -0.0000152587890625].pack("E*")
    w = AudioWaveform.generate_pcm(StringIO.new(data), **OPTIONS, format: "f64le", amplitude_scale: :auto)
    assert_equal [-32767, 32767], w.data
    data = [32767, -32767, -32768, 32767].pack("s<*")
    w = AudioWaveform.generate_pcm(StringIO.new(data), **OPTIONS, channels: 2, amplitude_scale: "auto")
    assert_equal [-32767, 0], w.data
    w = AudioWaveform.generate_pcm(StringIO.new([-2.0, 2.0].pack("e*")), **OPTIONS, format: :f32le, amplitude_scale: 0.5)
    assert_equal [-32768, 32767], w.data
  end

  def test_empty_input_retains_metadata
    w = AudioWaveform.generate_pcm(StringIO.new(""), **OPTIONS, channels: 6, split_channels: true)
    assert w.empty?
    assert_equal 6, w.channels
    assert_equal 8000, w.sample_rate
    assert_equal 0.0, w.duration
    assert_equal [], w.data
  end

  def test_invalid_options_fail_before_reading
    input = Object.new
    def input.read(*) = raise("must not read")
    [{format: "wav"}, {format: nil}, {sample_rate: 0}, {sample_rate: 48_000.0},
      {channels: 0}, {channels: 65536}, {channels: "1"}, {samples_per_pixel: 1},
      {points: 110}, {pixels_per_second: 20}, {split_channels: 1},
      {amplitude_scale: Float::INFINITY}, {amplitude_scale: "1"}].each do |invalid|
      assert_raises(ArgumentError, invalid.inspect) { AudioWaveform.generate_pcm(input, **OPTIONS, **invalid) }
    end
    assert_raises(ArgumentError) { AudioWaveform.generate_pcm("path", **OPTIONS) }
  end

  def test_partial_samples_and_channel_frames_are_errors
    ["\x01".b, "\x01\x00".b, "\x01\x00\x02".b].each do |data|
      assert_raises(AudioWaveform::Error) do
        AudioWaveform.generate_pcm(StringIO.new(data), **OPTIONS, channels: 2)
      end
    end
    [Float::NAN, Float::INFINITY, -Float::INFINITY].each do |value|
      assert_raises(AudioWaveform::Error) do
        AudioWaveform.generate_pcm(StringIO.new([value].pack("E")), **OPTIONS, format: :f64le)
      end
    end
  end

  def test_invalid_reader_results_are_rejected
    [123, "x" * 32_769].each do |result|
      input = Object.new
      input.define_singleton_method(:read) { |*| result }
      assert_raises(TypeError, ArgumentError) { AudioWaveform.generate_pcm(input, **OPTIONS) }
    end
  end

  def test_read_failure_preserves_exception_and_releases_accumulated_native_peaks
    expected = IOError.new("producer failed")
    input = Object.new
    reads = 0
    block = "\0".b * 32_768
    input.define_singleton_method(:read) do |_, outbuf|
      reads += 1
      raise expected if reads > 128
      outbuf.replace(block)
    end
    GC.start
    GC.disable
    error = assert_raises(IOError) { AudioWaveform.generate_pcm(input, **OPTIONS) }
    assert_same expected, error
    klass = AudioWaveform.const_get(:Native).const_get(:PcmStream)
    ObjectSpace.each_object(klass) { |stream| assert_operator ObjectSpace.memsize_of(stream), :<, 1024 }
  ensure
    GC.enable
    GC.start
  end

  def test_reader_throw_preserves_payload_and_releases_native_state
    expected = Object.new
    input = Object.new
    input.define_singleton_method(:read) { |*| throw(:stop_pcm, expected) }
    actual = catch(:stop_pcm) { AudioWaveform.generate_pcm(input, **OPTIONS) }
    assert_same expected, actual
  end

  def test_reader_stop_iteration_propagates_instead_of_returning_partial_peaks
    expected = StopIteration.new("producer stopped unexpectedly")
    input = Object.new
    reads = 0
    block = [-2000, 1000].pack("s<*") * 8192
    input.define_singleton_method(:read) do |_, outbuf|
      reads += 1
      raise expected if reads > 1
      outbuf.replace(block)
    end

    error = assert_raises(StopIteration) { AudioWaveform.generate_pcm(input, **OPTIONS) }
    assert_same expected, error
  end

  def test_timeout_while_waiting_for_pipe_data_is_prompt_and_does_not_close_input
    reader, writer = IO.pipe
    reader.binmode
    writer.binmode
    started = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    assert_raises(Timeout::Error) do
      Timeout.timeout(0.05) { AudioWaveform.generate_pcm(reader, **OPTIONS) }
    end
    assert_operator Process.clock_gettime(Process::CLOCK_MONOTONIC) - started, :<, 2
    refute reader.closed?
    refute writer.closed?
  ensure
    reader&.close
    writer&.close
  end

  def test_thread_kill_while_waiting_runs_ensure
    reader, writer = IO.pipe
    reader.binmode
    writer.binmode
    ensured = false
    worker = Thread.new do
      AudioWaveform.generate_pcm(reader, **OPTIONS)
    ensure
      ensured = true
    end
    Timeout.timeout(5) { Thread.pass until worker.status == "sleep" }
    worker.kill
    assert worker.join(2), "stream ignored Thread#kill"
    assert ensured
    refute reader.closed?
  ensure
    reader&.close
    writer&.close
    worker&.kill
    worker&.join(1)
  end
end
