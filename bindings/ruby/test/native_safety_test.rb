# frozen_string_literal: true

require_relative "test_helper"
require "open3"
require "rbconfig"
require "timeout"

class NativeSafetyTest < Minitest::Test
  include AudioWaveformTestSupport

  def test_generation_allows_other_ruby_threads_to_progress
    with_long_wav(10_000_000) do |path|
      assert_ruby_success(<<~'CODE', path)
        require "audiowaveform"
        ready = Queue.new
        running = true
        ticks = 0
        worker = Thread.new { ready << true; ticks += 1 while running }
        ready.pop
        before = ticks
        waveform = AudioWaveform.generate(ARGV.fetch(0), points: 110)
        abort "GVL was not released" unless ticks > before
        abort "wrong result" unless waveform.length == 110
        running = false
        worker.join
      CODE
    end
  end

  def test_native_waveform_allocations_are_visible_to_ruby_gc
    with_long_wav(5_000_000) do |path|
      assert_ruby_success(<<~'CODE', path)
        require "audiowaveform"
        require "objspace"
        GC.start
        GC.disable
        before = GC.stat(:malloc_increase_bytes)
        waveform = AudioWaveform.generate(ARGV.fetch(0), samples_per_pixel: 2)
        bytes = waveform.length * waveform.channels * 4
        abort "native buffer omitted from GC accounting" if GC.stat(:malloc_increase_bytes) - before < bytes
        abort "native buffer omitted from object size" if ObjectSpace.memsize_of(waveform) < bytes
        waveform = nil
        GC.enable
        GC.start
        before = GC.count
        12.times do
          AudioWaveform.generate(ARGV.fetch(0), samples_per_pixel: 2)
          String.new(capacity: 1024)
        end
        abort "native allocations did not trigger GC" if GC.count == before
      CODE
    end
  end

  def test_interrupt_cancels_both_decoding_modes_and_reclaims_native_memory
    with_long_wav(500_000_000) do |path|
      assert_ruby_success(<<~'CODE', path)
        require "audiowaveform"
        require "timeout"
        # Warm Timeout's helper and all Ruby exception classes before measuring.
        begin
          Timeout.timeout(0.001) { sleep 0.1 }
        rescue Timeout::Error
        end
        GC.start
        GC.disable
        before = GC.stat(:malloc_increase_bytes)
        started = Process.clock_gettime(Process::CLOCK_MONOTONIC)
        [{points: 110}, {samples_per_pixel: 2}].each do |options|
          4.times do
            begin
              Timeout.timeout(0.02) { AudioWaveform.generate(ARGV.fetch(0), **options) }
              abort "generation completed before interruption"
            rescue Timeout::Error
            end
          end
        end
        elapsed = Process.clock_gettime(Process::CLOCK_MONOTONIC) - started
        abort "native cancellation was not prompt: #{elapsed}" if elapsed > 5
        increase = GC.stat(:malloc_increase_bytes) - before
        abort "interrupted generation leaked #{increase} bytes" if increase > 2_000_000
        GC.enable
        GC.start
      CODE
    end
  end

  def test_wakeup_preserves_generation_and_the_open_input
    %w[points samples_per_pixel].each do |resolution|
      with_long_wav(100_000_000) do |path|
        assert_ruby_success(<<~'CODE', path, resolution)
          require "audiowaveform"
          target = Thread.current
          interrupter = Thread.new do
            Thread.pass until target.status == "sleep"
            # Allow the decoder to open the input before removing its pathname.
            sleep 0.02
            File.unlink(ARGV.fetch(0)) unless Gem.win_platform?
            3.times do
              target.wakeup
              sleep 0.005
            end
          end
          options = ARGV.fetch(1) == "points" ? {points: 110} : {samples_per_pixel: 4096}
          waveform = AudioWaveform.generate(ARGV.fetch(0), **options)
          interrupter.value
          expected_length = options[:points] || (100_000_000.fdiv(4096)).ceil
          abort "wrong point count" unless waveform.length == expected_length
          abort "lost decoded frames" unless waveform.duration == 6250.0
          abort "changed peaks" unless waveform.data.all?(&:zero?)
        CODE
      end
    end
  end

  def test_signal_handler_can_return_normally_during_generation
    skip "USR1 is unavailable" unless Signal.list.key?("USR1") && !Gem.win_platform?

    with_long_wav(100_000_000) do |path|
      assert_ruby_success(<<~'CODE', path)
        require "audiowaveform"
        handled = 0
        Signal.trap("USR1") { GC.start; handled += 1 }
        target = Thread.current
        [{points: 110}, {samples_per_pixel: 4096}].each do |options|
          before = handled
          interrupter = Thread.new do
            Thread.pass until target.status == "sleep"
            3.times do |index|
              Process.kill("USR1", Process.pid)
              Thread.pass until handled > before + index
            end
          end
          waveform = AudioWaveform.generate(ARGV.fetch(0), **options)
          # The first signal is sent while native generation has the GVL
          # released. Ruby may schedule later signal/handler round trips after
          # generation returns, so wait for the sender before checking them.
          interrupter.value
          abort "signal handler did not run" unless handled == before + 3
          expected_length = options[:points] || (100_000_000.fdiv(4096)).ceil
          abort "wrong result after signal" unless waveform.length == expected_length && waveform.data.all?(&:zero?)
        end
      CODE
    end
  end

  def test_signal_handler_unwinds_with_its_original_exception_or_throw_value
    skip "USR1 is unavailable" unless Signal.list.key?("USR1") && !Gem.win_platform?

    with_long_wav(500_000_000) do |path|
      assert_ruby_success(<<~'CODE', path)
        require "audiowaveform"
        target = Thread.current
        [{points: 110}, {samples_per_pixel: 2}].each do |options|
          [:raise, :throw].each do |kind|
            expected = kind == :raise ? RuntimeError.new("signal interruption") : Object.new
            Signal.trap("USR1") do
              GC.start
              kind == :raise ? raise(expected) : throw(:cancel, expected)
            end
            interrupter = Thread.new do
              Thread.pass until target.status == "sleep"
              sleep 0.02
              Process.kill("USR1", Process.pid)
            end
            actual = catch(:cancel) do
              begin
                AudioWaveform.generate(ARGV.fetch(0), **options)
                abort "generation ignored signal interruption"
              rescue RuntimeError => error
                error
              end
            end
            interrupter.value
            abort "changed signal payload" unless actual.equal?(expected)
          end
        end
      CODE
    end
  end

  def test_thread_kill_stops_generation_and_runs_ensure
    with_long_wav(500_000_000) do |path|
      assert_ruby_success(<<~'CODE', path)
        require "audiowaveform"
        [{points: 110}, {samples_per_pixel: 2}].each do |options|
          ensured = false
          worker = Thread.new do
            begin
              AudioWaveform.generate(ARGV.fetch(0), **options)
              abort "generation finished before Thread#kill"
            ensure
              ensured = true
            end
          end
          Thread.pass until worker.status == "sleep"
          sleep 0.02
          worker.kill
          abort "Thread#kill did not stop decoding promptly" unless worker.join(2)
          abort "Thread#kill skipped Ruby ensure" unless ensured
        end
      CODE
    end
  end

  def test_peak_arrays_survive_gc_and_concurrent_access
    waveform = AudioWaveform.generate(fixture("stereo.wav"), points: 110, split_channels: true)
    expected = waveform.data
    threads = 4.times.map do
      Thread.new do
        12.times do
          GC.start
          raise "mutated native data" unless waveform.data == expected
          raise "bad signed conversion" unless waveform.data(bits: 8) == expected.map { |v| v.negative? ? -((-v) / 256) : v / 256 }
          raise "bad point order" unless waveform.point(10, channel: 1) == expected.slice(42, 2)
        end
      end
    end
    threads.each(&:value)
  end

  def test_rbs_accepts_string_and_pathname_inputs
    assert_ruby_success(<<~'CODE', fixture("uneven.wav"), File.join(AudioWaveformTestSupport::ROOT, "sig"))
      require "shellwords"
      ENV["RBS_TEST_TARGET"] = "AudioWaveform,AudioWaveform::Waveform"
      ENV["RBS_TEST_OPT"] = ["-I", ARGV.fetch(1)].shelljoin
      ENV["RBS_TEST_LOGLEVEL"] = "error"
      require "rbs/test/setup"
      require "audiowaveform"
      require "pathname"
      [ARGV.fetch(0), Pathname(ARGV.fetch(0))].each do |input|
        waveform = AudioWaveform.generate(input, points: 3, amplitude_scale: "auto")
        waveform.data(bits: 8)
        waveform.point(0)
      end
    CODE
  end

  private

  def with_long_wav(frames)
    Dir.mktmpdir do |directory|
      path = File.join(directory, "silence.wav")
      write_silence_wav(path, frame_count: frames)
      yield path
    end
  end

  def assert_ruby_success(script, *arguments)
    Open3.popen2e(RbConfig.ruby, "-I", File.expand_path("../lib", __dir__), "-e", script, *arguments) do |input, output, child|
      input.close
      reader = Thread.new { output.read }
      unless child.join(30)
        Process.kill("KILL", child.pid)
        flunk "native safety subprocess exceeded 30 seconds"
      end
      assert child.value.success?, reader.value
    end
  end
end
