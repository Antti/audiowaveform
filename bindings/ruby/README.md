# AudioWaveform for Ruby

A native Ruby interface to the streaming Rust core. Ruby 3.2 or newer is required.
Versions 0.3.0 and later use the replacement core. It is licensed under
MIT or Apache-2.0, at your option; see the root [license notice](../../LICENSE.md).

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
larger than 4,294,967,295. Exact counts use one decoding pass for PCM/float WAV
and native FLAC with an exact header frame count, and nonfragmented AAC/MP4
with parsed playback bounds. The actual playback count is verified. Other inputs
use two passes on the same open file without requiring duration metadata.
A mismatched count, including AAC timestamp rounding beyond decoded EOF,
discards provisional peaks and replays once using the count from that first
pass. Decoding/corruption errors still fail.
Results are unchanged, and no new keyword is required. Keep the file unchanged
during the operation. This method accepts regular seekable files. Use `generate_pcm` below
for raw PCM streams. Neither method fetches URLs.

Decoding releases Ruby's GVL. Ruby interrupts request cooperative cancellation
between decoder packets and processing batches. A blocked filesystem/decoder
call may delay cancellation. Ruby exceptions propagate after native cleanup.

Working audio storage is bounded by decoder blocks; output memory scales with
point count and channels. Every `data` call allocates its returned Ruby array,
without copying peaks into an intermediate Rust vector. Native allocations are
reported to Ruby's GC. Large array conversions periodically process interrupts.

## Raw PCM streams

`generate_pcm` reads an `IO`, `StringIO`, or object implementing
`read(length, outbuf)` from its current position to EOF. It never seeks or closes
the input. This API is available starting with 0.4.0.

```ruby
require "audiowaveform"
require "open3"

rate = 48_000
# duration_seconds comes from the integrating application's known duration.
bucket = [(duration_seconds * rate / 110).ceil, 2].max
command = ["ffmpeg", "-nostdin", "-v", "error", "-i", "surround.m4a",
  "-map", "0:a:0", "-vn", "-ac", "1", "-ar", rate.to_s,
  "-acodec", "pcm_s16le", "-f", "s16le", "pipe:1"]

waveform = Open3.popen2(*command) do |stdin, stdout, process|
  stdin.close
  stdout.binmode
  result = AudioWaveform.generate_pcm(stdout,
    format: :s16le, sample_rate: rate, channels: 1,
    samples_per_pixel: bucket)
  raise "FFmpeg failed" unless process.value.success?
  result
end
peaks = waveform.data(bits: 8)
```

This consumes FFmpeg's mono PCM directly in one pass; it does not create a WAV
or buffer the whole recording. The gem does not launch or depend on FFmpeg.
The caller owns the subprocess and must check its exit status: EOF alone cannot
distinguish a failed producer from a legitimately short recording.

Required keywords are `format:`, `sample_rate:` (positive Integer up to
4,294,967,295), and `channels:` (source channels, Integer 1–65,535). Supported
format names, as Symbols or Strings, are `u8`, `s8`, `s16le`, `s16be`, `s24le`,
`s24be`, `s32le`, `s32be`, `f32le`, `f32be`, `f64le`, and `f64be`. Input is
headerless, interleaved PCM in exactly that format; no format is inferred.

`samples_per_pixel:` defaults to 256 and counts frames, with a minimum of 2.
`split_channels:` and `amplitude_scale:` work as for files. `points:` and
`pixels_per_second:` are not accepted by this Ruby API. The output count is
`ceil(actual_frames / samples_per_pixel)`, including the last partial bucket.
The actual frame count determines duration. An estimated duration can therefore
produce slightly more or fewer than 110 points.

Reads reuse a 32 KiB Ruby buffer and a native copy for thread safety. Decoded
scratch space is fixed by channel count; only the output peaks grow with bucket
count. Native aggregation releases the GVL. Standard Ruby IO blocking reads
support normal Ruby interrupts; custom readers must provide their own blocking
behavior. Read exceptions propagate unchanged and native resources are released
on failure, cancellation, or Ruby `throw`. Incomplete samples/channel frames
at EOF and nonfinite float samples raise `AudioWaveform::Error`.

## Waveform

- `sample_rate`, `channels`, `length` (`size`), `empty?`
- `samples_per_pixel`: nominal frames per point. For exact-count timing use
  `duration / length`; bucket widths may differ by one source frame.
- `duration` (`duration_seconds`): playback frames divided by sample rate,
  including silence from supported leading empty MP4 edits.
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
Opus, and Wave64 are unsupported by file decoding; a caller-managed decoder can
feed their PCM into `generate_pcm`. Decoder gapless trimming removes reported
delay/padding, including MP3 and Vorbis priming. Nonfragmented AAC/MP4 also trims
to the selected track's single, normal-speed media edit and sample timing range
in both passes. Leading empty edits contribute silence before the audio;
intentional recorded silence is preserved. Rounded media timestamps are
supported, though coarse metadata may still round duration slightly.
Fragmented MP4 and iTunSMPB-only delay metadata remain unsupported for gapless
trimming. Multiple media edits, empty edits after media, or non-unit playback
rates raise `AudioWaveform::Error`; use external decoding and `generate_pcm`
for those inputs. See the root README for track-selection limits.

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
`ruby bindings/ruby/script/test_source_gem.rb pkg/audiowaveform-0.4.0.gem`.
Native cross-builds and ABI checks are configured in `.github/workflows`.

Release tags use `ruby-vX.Y.Z`. The release workflow verifies the tag against
the gem version, runs Ruby tests and the complete native build/install matrix,
then publishes through RubyGems Trusted Publishing and attaches the gems to a
GitHub release. Manual workflow runs perform the checks without publishing.
