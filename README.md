# Waveform core

A new Rust implementation of the audio-to-peaks contract in
[contract/SPEC.md](contract/SPEC.md). `waveform-core` is a provisional local
package name; crates.io publishing is not yet enabled.

This standalone Cargo workspace has no dependency on the former library.
It contains audio decoding and peak generation, with no waveform export,
rendering, or production command-line interface. A separately built Ruby
extension under `bindings/ruby` exposes
the same core; see [the Ruby guide](bindings/ruby/README.md).

## Use

```rust
use waveform_core::{generate, Error, Options, Resolution};

fn main() -> Result<(), Error> {
    let waveform = generate("recording.wav", Options {
        resolution: Resolution::Points(110),
        ..Options::default()
    })?;

    let signed_16bit_peaks: &[i16] = waveform.data16();
    let signed_8bit_peaks: Vec<i8> = waveform.data8().collect();
    let seconds = waveform.duration();
    Ok(())
}
```

Peaks are interleaved in point/channel/min-max order. `data16()` borrows the
stored values; `data8()` is an allocation-free iterator using signed division
with truncation toward zero. Collect it only when an owned array is needed.
`point(index, channel)` returns an optional pair without allocating.

`Resolution` supports exact point counts, frames per point, and points per
second. `ChannelMode` supports an arithmetic mono mix and separate channels.
`Gain` supports a finite nonnegative multiplier or global normalization.
Every result retains the actual decoded frame count for duration, including
the final partial bucket in fixed-resolution mode.

## Raw PCM streams

`generate_pcm(reader, format, sample_rate, channels, options)` accepts any Rust
`Read`, including pipes, without seeking. For caller-managed reads,
`PcmStream::new(...)`, `push(bytes)`, and `finish()` provide the same accumulator.
PCM streaming is available even with `--no-default-features`.

`PcmFormat` supports `u8`, `s8`, signed 16/24/32-bit integers and 32/64-bit
floats, with explicit little/big endian variants (`s16le`, `f32be`, etc.).
Samples must be interleaved. Rate and source channel count are required; channel
count fits `u16`. Stream resolution is frames per point or points per second;
exact `Points` is rejected before reading. Mono/split channels and both gain
modes are supported. A final partial bucket is included; incomplete samples or
channel frames at EOF are errors. Nonfinite float samples are errors.

Working buffers are reused and bounded by channel count, independently of input
length. Retained output still grows with the number of buckets. Cancellation
variants check between reads and processing batches; a Rust `Read` implementation
must itself arrange interruptible blocking I/O. Ruby IO reads remain interruptible
by Ruby and never run inside an unprotected native callback. See the
[Ruby streaming example](bindings/ruby/README.md#raw-pcm-streams).

## Memory and cancellation

Fixed resolution decodes once. Exact point counts also decode once for PCM/float
WAV and native FLAC with an exact header frame count. The count and signal
metadata are checked against the actual decoded audio. Missing/unsupported
metadata uses a counting pass and replay of the same open file. If a header
count disagrees, provisional peaks are discarded and the completed first pass
supplies the actual count for replay: at most two decoding passes. Decoder
errors, checksum failures, and truncation remain errors.

This optimization never uses duration estimates and preserves the same peak
boundaries and values. AAC/M4A, MP3, Ogg/WebM and other formats still use the
two-pass path for exact points. Empty audio does not allocate the requested
point count. Keep input files unchanged during generation; replay mismatches
in frame count, track, rate, or channel count are errors.

A decoded-block scratch buffer and channel extrema are reused. Exact output
capacity is reserved once. Fixed-resolution output grows geometrically as
needed. Normalization retains unquantized extrema, then creates the final
16-bit output; this storage grows with output points, not recording duration.
There is no whole-audio buffer or temporary PCM spool.

`generate_with_cancel` accepts a synchronous cancellation predicate, checked
between packets, frame batches, repeated points, and normalization batches.
The Ruby extension connects this hook to Ruby interrupts. Processing panics
are contained as `Error::InternalPanic` when Rust unwinding is enabled; process
aborts and allocation failures inside dependencies are not unwinding panics.

`statistics()` reports application-owned scratch and peak capacities. It does
not count decoder/container metadata or allocator overhead. See
[VALIDATION.md](VALIDATION.md) for measured short/long recording results.

## Codec features

The default feature is `wav`. Enable `all-formats` for the complete set, or
select `wav`, `aiff`, `caf`, `flac`, `ogg`, `aac`, `m4a`, `mkv`, `mp1`, `mp2`,
and `mp3` individually. `--no-default-features` compiles without registering
audio formats/codecs. Symphonia is the only direct runtime dependency.

AAC-LC decoding targets mono/stereo. Multichannel AAC, HE-AAC, Opus, and Wave64
are outside this revision. Codec availability is separate from container
recognition. Decoder gapless trimming is enabled, removing delay/padding where
Symphonia supplies it (including MP3 and Vorbis priming). Duration counts the
frames actually delivered after trimming. AAC/MP4 edit-list trimming is still
limited by the decoder, so AAC duration can differ from playback.

Track selection uses explicit default-audio flags exposed by Symphonia, then
the first reported audio track. Unsupported selected audio is an error.
Symphonia 0.6.1 does not expose MP4 default/enabled preferences, and omits a
Matroska default flag when the file relies on the schema's implicit value.
Those cases consequently use the first-audio fallback. The test corpus covers
explicit defaults, video-before-audio, and unsupported selected tracks.

The WAV reader checks RIFF extents and inconsistent nonzero channel masks.
For live Matroska/WebM, a narrow workaround verifies complete element extents
and block/lacing headers before accepting Symphonia's end-of-file error for an
unknown-length document. Truncated payloads and decoder errors remain errors.

## Development

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo test --no-default-features
bundle install
bundle exec rake # Ruby extension, contract/safety tests, and RBS
python3 contract/conformance.py check
python3 tests/measure_memory.py
```

All test media is synthetic and included in the repository, so normal Rust
tests need neither Python nor FFmpeg. Rebuilding the tiny numeric WAVs uses
`contract/conformance.py build EMPTY_DIRECTORY`. Rebuilding codec fixtures uses
`python3 tests/build_codec_fixtures.py` and FFmpeg with the listed encoders.
The memory measurement uses `/usr/bin/time` on macOS or Linux and creates
ignored files under `measurements/`.

## License

The replacement core and Ruby integration are licensed under either
[MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
See [LICENSE.md](LICENSE.md) and [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md)
for dependency terms.

## Legacy releases

This repository starts an independent Git history for the replacement core and
Ruby integration. The former implementation, its release tags, and the sources
for Ruby gems 0.1.0, 0.2.0, and 0.2.1 are preserved in
[audiowaveform-legacy](https://github.com/Antti/audiowaveform-legacy).
Those gems remain available under their original GPL-3.0-or-later terms.

See [PROVENANCE.md](PROVENANCE.md) for implementation inputs and the replacement's
licensing status. Changing repositories does not relicense the legacy releases.
