# frozen_string_literal: true

require_relative "audiowaveform/version"
require "rbconfig"

# Platform gems contain one extension per Ruby minor version. Source builds put
# the extension directly under audiowaveform/.
native_extension = File.join(
  __dir__, "audiowaveform", RUBY_VERSION[/\A\d+\.\d+/],
  "audiowaveform_ruby.#{RbConfig::CONFIG.fetch('DLEXT')}"
)
if File.file?(native_extension)
  require native_extension
else
  require "audiowaveform/audiowaveform_ruby"
end

module AudioWaveform
  class << self
    # Generates waveform data from an audio file.
    def generate(
      input,
      samples_per_pixel: nil,
      pixels_per_second: nil,
      points: nil,
      split_channels: false,
      amplitude_scale: nil
    )
      scale_kind, scale_value = resolve_scale(samples_per_pixel, pixels_per_second, points)
      amplitude_kind, amplitude_value = resolve_amplitude_scale(amplitude_scale)

      unless split_channels.equal?(true) || split_channels.equal?(false)
        raise ArgumentError, "split_channels must be true or false"
      end

      Native.generate(
        resolve_path(input),
        scale_kind,
        scale_value,
        split_channels,
        amplitude_kind,
        amplitude_value
      )
    end

    private

    def resolve_path(input)
      path = File.path(input)
      raise ArgumentError, "path must not contain NUL bytes" if path.include?("\0")
      path
    rescue TypeError
      raise ArgumentError, "path must be a String or provide to_path"
    end

    def resolve_scale(samples_per_pixel, pixels_per_second, points)
      if [samples_per_pixel, pixels_per_second, points].count { |value| !value.nil? } > 1
        raise ArgumentError, "samples_per_pixel, pixels_per_second, and points are mutually exclusive"
      end

      unless points.nil?
        value = positive_integer(points, :points)

        return ["points", value]
      end

      unless pixels_per_second.nil?
        ["pixels_per_second", positive_integer(pixels_per_second, :pixels_per_second)]
      else
        value = samples_per_pixel.nil? ? 256 : samples_per_pixel
        ["samples_per_pixel", positive_integer(value, :samples_per_pixel, minimum: 2)]
      end
    end

    def resolve_amplitude_scale(value)
      return ["none", 0.0] if value.nil?
      return ["auto", 0.0] if value.equal?(:auto) || (value.is_a?(String) && value == "auto")

      unless value.is_a?(Numeric) && !value.is_a?(Complex)
        raise ArgumentError, "amplitude_scale must be a real number or :auto"
      end
      numeric = Float(value)
      unless numeric.finite? && numeric >= 0.0
        raise ArgumentError, "amplitude_scale must be a finite non-negative number or :auto"
      end

      ["fixed", numeric]
    rescue TypeError, ArgumentError, RangeError
      raise ArgumentError, "amplitude_scale must be a finite non-negative number or :auto"
    end

    def positive_integer(value, name, minimum: 1)
      unless value.is_a?(Integer) && value.between?(minimum, 0xffff_ffff)
        raise ArgumentError, "#{name} must be an integer between #{minimum} and 4294967295"
      end

      value
    end
  end

  class Waveform
    private_class_method :new

    alias size length
    alias bits storage_bits
    alias duration_seconds duration

    # Returns interleaved [minimum, maximum, ...] values at 8 or 16 bits.
    def data(bits: 16)
      __data(validate_bits(bits))
    end

    # Returns the [minimum, maximum] pair at +index+ for +channel+.
    def point(index, channel: 0)
      unless index.is_a?(Integer) && channel.is_a?(Integer)
        raise ArgumentError, "index and channel must be integers"
      end
      unless index.between?(0, length - 1) && channel.between?(0, channels - 1)
        raise IndexError, "waveform point is outside the available channel or index range"
      end

      value = __point(channel, index)
      return value if value

      raise IndexError, "waveform point is outside the available channel or index range"
    end

    private

    def validate_bits(bits)
      return bits if bits.is_a?(Integer) && (bits == 8 || bits == 16)

      raise ArgumentError, "bits must be either 8 or 16"
    end
  end

  private_constant :Native
end
