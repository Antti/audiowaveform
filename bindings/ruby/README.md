# AudioWaveform for Ruby

A native Ruby interface to the streaming Rust core. Ruby 3.2 or newer is required.
This local `0.3.0.pre.1` build is not published; release licensing is pending.

```ruby
require "audiowaveform"

waveform = AudioWaveform.generate("recording.m4a", points: 110)
peaks = waveform.data(bits: 8) # 220 signed integers: min, max, min, max, ...
waveform.point(0)             # [minimum, maximum] at the stored 16-bit depth
waveform.duration            # actual decoded seconds
```

## Generation

`generate` accepts a String path or an object with `to_path`, and these keywords:

| Keyword | Meaning / default |
| --- | --- |
| `points:` | Exact positive point count; empty audio returns zero points. |
| `samples_per_pixel:` | Frames per point, at least 2; defaults to 256. |
| `pixels_per_second:` | Positive target rate; resolves to `max(2, sample_rate / rate)` frames. |
| `split_channels:` | `false` mixes all channels by their arithmetic mean; `true` keeps each channel. Only booleans are accepted. |
| `amplitude_scale:` | `nil` uses gain 1; a finite nonnegative real Numeric applies gain; `:auto` or `"auto"` normalizes globally. |

At most one resolution option may be non-nil. Each resolution is an Integer no
larger than 4,294,967,295. Exact counts use two decoding passes on the same open
file and do not require duration metadata. Keep the file unchanged during the
operation. Regular seekable files are supported; streams and URLs are not.

Decoding releases Ruby's GVL. Ruby interrupts request cooperative cancellation
between decoder packets and processing batches. A blocked filesystem/decoder
call may delay cancellation. Ruby exceptions propagate after native cleanup.

Working audio storage is bounded by decoder blocks; output memory scales with
point count and channels. Every `data` call allocates its returned Ruby array,
without copying peaks into an intermediate Rust vector. Native allocations are
reported to Ruby's GC. Large array conversions periodically process interrupts.

## Waveform

- `sample_rate`, `channels`, `length` (`size`), `empty?`
- `samples_per_pixel`: nominal frames per point. For exact-count timing use
  `duration / length`; bucket widths may differ by one source frame.
- `duration` (`duration_seconds`): decoded frames divided by sample rate.
- `storage_bits` (`bits`): always 16.
- `data(bits: 16)`: a new Array of signed integers, ordered by point, channel,
  then minimum/maximum. `bits: 8` converts with signed truncation by 256.
- `point(index, channel: 0)`: a new two-integer array at 16-bit depth.

Results retain their own data; changing a returned array does not alter them.
Invalid arguments raise `ArgumentError`; out-of-range integer point/channel
indices raise `IndexError`; input/decoder failures raise `AudioWaveform::Error`.
`Waveform.new` is private. There are no library DAT/JSON/TXT export methods,
file-writing methods, renderer, or CLI.

## Formats and migration

The extension enables the core's `all-formats`: supported WAV, FLAC, Ogg,
AAC/M4A, ALAC, MP1/MP2/MP3, AIFF, CAF, and Matroska/WebM audio codecs. Container
recognition does not imply every codec is supported. Multichannel AAC, HE-AAC,
Opus, and Wave64 are unsupported. AAC padding/delay is not trimmed, so decoded
duration can differ from playback. See the root README for track-selection limits.

Version 0.3 removes exports and tightens argument coercion. PCM quantization and
fixed-resolution duration also change; do not expect byte-identical 0.2 peaks.
See CHANGELOG.md and the root `contract/SPEC.md` for the replacement semantics.

## Build and check

From the repository root, with Rust and Ruby development headers available:

```sh
bundle install
bundle exec rake
bundle exec rake build
bundle exec rake native gem
ruby bindings/ruby/script/test_native_gem.rb PATH_TO_NATIVE_GEM
```

The normal rake task compiles, runs contract/codec/GC/cancellation tests, and
validates RBS. Source installation is checked with
`ruby bindings/ruby/script/test_source_gem.rb pkg/audiowaveform-0.3.0.pre.1.gem`.
Native cross-builds and ABI checks are configured in `.github/workflows`.
