# Waveform core

A new Rust implementation of the audio-to-peaks contract in
[contract/SPEC.md](contract/SPEC.md). `waveform-core` is a provisional local
package name; publishing is disabled and a release license has not been chosen.

This standalone Cargo workspace has no dependency on the former library.
It contains audio decoding and peak generation, with no waveform export,
rendering, or production command-line interface. The Ruby binding is a
subsequent integration step, not part of this implementation.

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

## Memory and cancellation

Fixed resolution decodes once. Exact point counts count decoded frames and
replay the same open file, without trusting duration estimates. Empty audio
does not allocate the requested point count. Keep input files unchanged during
generation; replay mismatches in frame count, track, rate, or channel count
are errors.

A decoded-block scratch buffer and channel extrema are reused. Exact output
capacity is reserved once. Fixed-resolution output grows geometrically as
needed. Normalization retains unquantized extrema, then creates the final
16-bit output; this storage grows with output points, not recording duration.
There is no whole-audio buffer or temporary PCM spool.

`generate_with_cancel` accepts a synchronous cancellation predicate, checked
between packets, frame batches, repeated points, and normalization batches.
It provides the hook for a future Ruby interruption handler. Processing panics
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
recognition. Decoder gapless trimming is disabled: duration describes decoded
frames and may include encoder delay/padding.

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
python3 contract/conformance.py check
python3 tests/measure_memory.py
```

All test media is synthetic and included in the repository, so normal Rust
tests need neither Python nor FFmpeg. Rebuilding the tiny numeric WAVs uses
`contract/conformance.py build EMPTY_DIRECTORY`. Rebuilding codec fixtures uses
`python3 tests/build_codec_fixtures.py` and FFmpeg with the listed encoders.
The memory measurement uses `/usr/bin/time` on macOS or Linux and creates
ignored files under `measurements/`.

See [PROVENANCE.md](PROVENANCE.md) for implementation inputs and licensing
status. A fresh Git root is prepared locally; the old remote and its release
tags are not changed by this work.
