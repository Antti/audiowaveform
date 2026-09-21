# Audio peak generation contract, revision 1

Status: specified for the replacement; not implemented by this kit. Normative
requirements below apply to the new library. No BBC command-line, file-format,
rendering, or byte-for-byte implementation compatibility is required.

## 1. Scope and terminology

The library reads a seekable local audio file and returns waveform peaks in
memory. A frame is one time instant containing one sample from each source
channel. A point is a minimum/maximum pair for each output channel. Resolution
arguments count **frames**, despite the historical `samples_per_pixel` name.

Let `N` be the actual number of decoded frames, `F` the positive sample rate,
and `C` the output channel count. Container duration estimates must not determine
`N`, point boundaries, or the returned duration. Channel count and sample rate
must remain constant throughout the selected track; otherwise return an error.

Default decoding selects the decoder's default audio track, or the first audio
track if there is no default. Skip non-audio tracks. If the selected audio track
uses an unsupported codec, return an error rather than silently selecting a
different recording. Track selection must be identical across passes.

The first replacement targets the gem's existing codec families through
Symphonia: PCM/ADPCM WAV, FLAC, MP1/MP2/MP3, Ogg Vorbis/FLAC, AAC-LC/ADTS,
AAC-LC/ALAC in M4A/MP4, AIFF, CAF, and supported Matroska/WebM audio tracks.
This is an acceptance target, not a claim of completed decoder coverage.
Codec availability is distinct from container recognition. Multichannel AAC,
HE-AAC, Opus, and Wave64 are not required for the first replacement.

Do not add application-level encoder delay/padding trimming in this revision;
configure optional decoder gapless trimming off where available. `N` counts
frames actually delivered by that configuration. Duration is the decoded
timeline, which can differ from a player's trimmed timeline. A future trimming
policy needs separate requirements and fixtures.

## 2. Ruby generation API

```ruby
AudioWaveform.generate(path,
  points: nil,
  samples_per_pixel: nil,
  pixels_per_second: nil,
  split_channels: false,
  amplitude_scale: nil)
```

- `path` is a String or an object providing `to_path` returning a String.
  Embedded NUL bytes and invalid path types raise `ArgumentError`.
- At most one non-nil resolution keyword is allowed. Explicit `nil` is absent.
  With none, use `samples_per_pixel: 256`.
- `points` is an Integer in `1..4_294_967_295`.
- `samples_per_pixel` is an Integer in `2..4_294_967_295`.
- `pixels_per_second` is an Integer in `1..4_294_967_295`.
- `split_channels` must be `true` or `false`.
- `amplitude_scale` is `nil` (gain 1), a finite nonnegative real Numeric, `:auto`,
  or the String `"auto"`. Convert numeric gains to a finite binary64 value;
  reject complex, negative, NaN, infinite, or nonrepresentable gains.
- Reject invalid/unknown options with `ArgumentError` before decoding or
  allocating a requested waveform. No implicit conversion of float/string
  resolution values to integers.
- Reject unrepresentable dimensions with `AudioWaveform::Error`; all size
  products and bucket-boundary arithmetic must be checked or use a sufficiently
  wide integer. A valid option is not a guarantee that its allocation succeeds.

The file must remain unchanged during generation. Open the input once and replay
that open seekable file if needed. A second pass must agree on frame count,
sample rate, and channel count; otherwise return an error. This does not promise
to detect content edits that preserve those properties. Pipes, URLs, arbitrary
Ruby IOs, and live input are outside this API.

## 3. Point boundaries

### Exact point count

For `points: P` and `N > 0`, return exactly `P` points per output channel.
Point `i`, with `0 <= i < P`, covers the half-open frame interval:

```text
start = floor(i * N / P)
end   = floor((i + 1) * N / P)
```

If `start == end`, use the single frame at `start` instead. Thus a clip with
fewer frames than points repeats samples without inventing silence. A nonempty
interval includes every frame from `start` through `end - 1`.

Examples: `N=7, P=3` gives `[0,2), [2,4), [4,7)`; `N=2, P=5` selects
frames `0, 0, 0, 1, 1`. Never pad a point's extrema with zero.

### Fixed resolution

`samples_per_pixel: S` produces consecutive buckets of at most `S` frames,
including the final partial bucket. The point count is `ceil(N / S)`.

`pixels_per_second: R` resolves to `S = max(2, floor(F / R))`, then follows
fixed-resolution behavior. This integer resolution is approximate; it is not
a promise of exactly `R` points in every second.

### Empty audio

A valid audio stream with known `F` and source channel count but zero frames
returns zero points and duration zero, including with `points:`. An empty file
without a valid audio header is an error. Empty audio must not be confused with
an unsupported track or a decoding failure.

## 4. Channels, gain, and peak quantization

Interpret integer PCM as normalized real values: signed `b`-bit samples use
`sample / 2^(b-1)`; unsigned `b`-bit samples use
`(sample - 2^(b-1)) / 2^(b-1)`. Use the decoder's **valid** sample precision,
not unused bits in its storage container. Float samples retain their finite
numeric values, including values outside `[-1, 1]`. Nonfinite samples are an
error. Decoders may expose converted samples; account for their documented
normalization rather than normalizing them twice.

With `split_channels: false`, take the arithmetic mean of all source channels
in each frame, then find the minimum/maximum of those mixed values in each
bucket. Include every channel, with equal weight and no speaker-specific
weighting. Do not combine channel extrema after aggregation.

With `split_channels: true`, aggregate each channel independently, retaining
the decoder's channel order. One gain applies to all channels.

For numeric gain `g`, multiply a bucket extremum `x` by `g`. For automatic gain,
let `M` be the greatest absolute sample value across the complete output signal
after mixing (or across all retained channels). If `M > 0`, use
`g = (32767 / 32768) / M`; silence remains zero. Normalization is global, not
per bucket or per channel, and is determined before integer quantization.

Convert extrema to signed 16-bit integers as follows:

```text
q16(x) = saturate_to_range(-32768, 32767, trunc_toward_zero(x * g * 32768))
q8(v)  = trunc_toward_zero(v / 256), where v is the stored q16 value
```

Clamp in a way that remains safe for large finite gains; intermediate overflow
must not panic or introduce NaN. Silence times any valid gain stays zero.
Do not clamp samples before gain/mixing. `q8` must use truncation toward zero,
not arithmetic right shift: `-255 -> 0`, `-256 -> -1`, `-32768 -> -128`.

The formulas describe ideal arithmetic. Binary64 accumulation/scaling is
acceptable; the provided rational and power-of-two vectors must pass exactly.
No extra rounding to an intermediate 16-bit PCM buffer is permitted before
mixing, scaling, or automatic normalization.

Flatten output in **point, channel, min/max** order. For two channels:

```text
[point0_channel0_min, point0_channel0_max,
 point0_channel1_min, point0_channel1_max,
 point1_channel0_min, point1_channel0_max, ...]
```

## 5. Returned Ruby object

| Method | Contract |
| --- | --- |
| `sample_rate` | `F`, an Integer. |
| `channels` | 1 for mixed output, source channel count for split output. |
| `length`, `size` | Number of points **per channel**, not array element count. |
| `empty?` | Whether `length == 0`. |
| `duration`, `duration_seconds` | `N / F` as a Float, for every resolution mode. |
| `samples_per_pixel` | Resolved `S` for fixed resolution; `max(2, floor(N/P))` for exact points. A compatibility convenience, not a timing field. |
| `bits`, `storage_bits` | 16; canonical stored peak precision. No serialization preference exists. |
| `data(bits: 16)` | A new flat Array of signed Integers. Only Integer 8 or 16 is accepted. |
| `point(index, channel: 0)` | A new two-element Array of canonical 16-bit values. |

`data` and `point` do not mutate the waveform. Mutating a returned Array must
not affect subsequent calls. Reject invalid `bits` or non-Integer point/channel
arguments with `ArgumentError`. Reject negative or out-of-range integer point
or channel indexes with `IndexError`. No negative-index wrapping.

For exact points, point spacing is `duration / length` when nonempty. For fixed
resolution, bucket starts are `i*S/F`; the final bucket ends at `duration`.
Do not use `duration/length` to position fixed buckets when the last is partial.

No library-specific `save`, `to_dat`, `to_json`, or `to_txt` method is required.
Generic methods added to Object by unrelated Ruby libraries are outside this
contract. No core serde/export module or binary format header is required.

## 6. Errors, native safety, and resource behavior

- File access, unsupported codec/container, malformed stream, and reported
  decoding failures raise `AudioWaveform::Error`. Do not return successful
  partial peaks after a decoder reports corruption. A decoder's ordinary EOF
  is not itself an error; detect declared missing data when the container API
  exposes it. Error text is descriptive but not a compatibility contract.
- Rust library calls return typed errors; malformed inputs and invalid options
  must not panic. A panic at the Ruby boundary must not unwind into Ruby/C.
- Release Ruby's GVL for decoding and aggregation. Do not call Ruby APIs from
  that region. Protect input/result lifetimes, handle Ruby interruption, clean
  up native resources, and account for retained native peak memory to Ruby GC.
- Fixed resolution needs one decode pass. Exact points can count frames and
  then replay for aggregation, without trusting container duration. Automatic
  normalization may retain unquantized extrema, not all decoded frames.
- Working storage owned by peak generation is `O(B*K + L*C)`, where `B` is
  maximum decoded block length, `K` source channels, and `L` output points.
  At a fixed target count and channel count, it must not grow with duration.
  Decoder/container metadata allocations are measured separately; do not claim
  constant whole-process memory for every container.
- Reuse scratch buffers between decoded blocks. Reserve known output sizes
  with fallible allocation; grow unknown fixed-resolution output geometrically.
  Do not allocate a heap object per frame, channel, or point. No whole-audio
  buffer, temporary PCM file, or JSON round trip is allowed.
- A call to `data` necessarily allocates its returned Ruby Array. Fill that
  final array directly; avoid a second full-size converted native output array.
- No FFmpeg process or external executable may be needed at runtime.

## 7. Rust boundary

Expose a small decoding/generation API and an owned peak result, with typed
resolution/channel/gain options. Keep exact frame count for timing. Do not make
Ruby symbols, format serialization headers, pixel/color types, or the legacy
crate's types part of the core. Concrete Rust type names are the independent
implementer's choice; no Rust API stability or existing Rust caller
compatibility is required for this replacement.

Keep codec features explicit and rendering/export dependencies absent. The Ruby
extension enables the supported codec set. A source gem includes only the new
core, reviewed Ruby binding/support files, and necessary dependency notices.
The old workspace, CLI, renderer, fixtures, and manuals must not be included.
