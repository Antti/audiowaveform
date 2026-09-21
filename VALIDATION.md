# Validation

Validated locally on macOS arm64, Rust 1.98.1, and in GitHub Actions on
2026-09-21. This report covers the new Rust core and the Ruby integration
below. It does not assert legal clearance.

## Correctness and boundaries

- All 36 literal numeric contract cases pass through real WAV decoding,
  including integer/float inputs, mono/split output, automatic and manual gain,
  clipping, exact counts, short clips, empty audio, and direct signed 8-bit data.
- 1,488 combinations of channels, frame count, point count, and block size
  match independently computed batch extrema. Bucket boundaries do not depend
  on the incoming decoder block sizes.
- The 17-case synthetic codec matrix passes in mono and split modes. Lossless
  output matches numeric expectations calculated from the source signal.
  Lossy tests check structure/ranges/counts rather than claim bit identity to
  the uncompressed input. MP1 uses newly constructed silent frames.
- Live WebM without a declared track duration produces exactly 110 points.
  Truncated live WebM packets are rejected. EOF validation covers no lacing,
  Xiph, fixed-size, and EBML lacing plus malformed/truncated lace headers.
- Track tests cover video before audio, an explicit reported default audio
  track, and an unsupported selected audio track. Decoder limitations for
  implicit/container defaults are documented in README.md.
- Tests cover invalid options before file access, nonfinite samples, truncated
  RIFF data, inconsistent speaker masks, cancellation, panic containment,
  replay frame-count mismatch, dimension overflow, and failed reservation.
- Exact-count aggregation preserves reserved output/current-buffer pointers.
  Allocation instrumentation confirms equal allocation counts for 2, 110,
  and 10,000 requested points on the same input, and zero allocations while
  iterating `data8()`.

## Memory measurement

Release build, 48 kHz mono 16-bit PCM, 110 output points, separate processes.
Input files are written from a reused 4,096-frame block. The long input is
86,400,000 frames / 172.8 MB of PCM, versus 1,440,000 frames for the short one.

| Input | Gain | Scratch capacity | Peak/current capacities | Peak RSS |
| --- | --- | ---: | ---: | ---: |
| 30 seconds | Fixed | 9,216 B | 456 B | 2,146,304 B |
| 30 minutes | Fixed | 9,216 B | 456 B | 2,146,304 B |
| 30 seconds | Normalize | 9,216 B | 2,216 B | 2,146,304 B |
| 30 minutes | Normalize | 9,216 B | 2,216 B | 2,146,304 B |

Application-owned capacities are identical for recordings 60 times longer.
These are PCM measurements on one machine, not a constant-RSS guarantee for
every decoder/container. Fixed-resolution output necessarily grows with
recording length. Decoder metadata, packet buffers, and allocator overhead are
not included in the reported application capacities.

Reproduce with `python3 tests/measure_memory.py`. Detailed timings and results
are written to ignored `measurements/memory.json`. macOS resource measurements
require permission to read the kernel timing information used by `time -l`.

## Build checks

`cargo fmt --check`, strict Clippy, all-feature tests, and no-default-feature
tests pass. All 11 individual codec feature test runs also pass; their logs
are retained under ignored `measurements/feature-checks/`.
The runtime/build dependency tree contains no old crate, image/PNG library, or
waveform serialization dependency.

`cargo package --offline --allow-dirty` builds and verifies the standalone source
crate. After dual licensing, its 116 archive entries were inspected:
no old workspace, Ruby binding,
audit, Git metadata, build output, or measurement files are included. Cargo's
metadata now declares `MIT OR Apache-2.0`; both license texts are included in
subsequent package checks.

## Ruby integration and packaging

The extension uses the replacement core with all codecs, with no dependency on
its former implementation. `cargo fmt --check` and strict all-target Clippy
passed for both Cargo workspaces; the core all-feature tests passed again.

On macOS arm64:

- Ruby 4.0.5 and Ruby 3.4.7 each passed the final 58-test / 971-assertion suite.
- All 36 exact numeric vectors pass through the Ruby API, including metadata,
  array independence, point ordering, argument errors and omitted export APIs.
- The 17-format codec corpus passes in mono and split modes; lossless peaks
  match the independent fixture expectations.
- GVL progress, native GC pressure/ObjectSpace accounting, parallel reads with
  GC, and RBS runtime checks pass. Interrupted exact-count and fixed-resolution
  decoding of a sparse 500-million-frame WAV returns promptly and reclaims
  native allocations. Safety subprocesses have a 30-second watchdog.
- Repeated `Thread#wakeup` and returning signal handlers preserve generation
  in both resolution modes. On Unix, generation also completes after removing
  the input pathname during decoding, checking that wakeups do not reopen it.
  Signal exceptions and `throw` retain their original payloads across native
  cleanup; `Thread#kill` returns promptly and runs Ruby `ensure` blocks.
  Signal tests require Unix `USR1` and are skipped on Windows.
- A local source gem was built, installed into an isolated gem directory, and
  exercised against all numeric vectors and codecs. Its build used only the
  packaged replacement sources and normal Cargo/Ruby build dependencies.
- A local Ruby 4.0 macOS arm64 native gem was built and installed with an empty
  gem environment. It needs neither rb_sys nor install-time compilation and
  passes the same numeric vectors and codec matrix.
- The smoke checks verify the loaded extension's actual installation path and
  exclude both BUNDLE_* and BUNDLER_* environment variables. This catches Ruby
  4's BUNDLER_SETUP hook otherwise reactivating the checkout during tests.
- Source/native package checks reject old crate/build directories, require
  notices/signatures, and distinguish source/native contents. An inventory of
  56 locked external Cargo crates includes original license texts and exact
  versioned source URLs. Publishing rejects the pending-license marker before
  any network operation.

Ruby peak conversion preallocates the Ruby array and uses a 256-value stack
buffer; it does not allocate an intermediate Rust peak vector. The final Ruby
array is an intentional allocation. `Waveform::allocated_bytes()` reports the
retained native capacity for ObjectSpace; transient decoding buffers are not
counted as retained waveform storage.

Remote checks passed for the dual-licensed sources at `e7ec5aa`:

- [Rust CI](https://github.com/Antti/audiowaveform/actions/runs/35603312670)
  passed formatting, Clippy, and tests.
- [Ruby CI](https://github.com/Antti/audiowaveform/actions/runs/35603312772)
  passed on Ruby 3.2/3.3/3.4/4.0, macOS and Windows, including source packaging.
- [Native gems](https://github.com/Antti/audiowaveform/actions/runs/35603312759)
  passed all seven platform builds and 28 installation checks across Ruby
  3.2/3.3/3.4/4.0: x86_64 and aarch64 Linux with glibc or musl, x86_64 and arm64
  macOS, and x64 Windows UCRT.

The `0.3.0` version bump passed the release-helper tests and an isolated source
gem installation with all numeric vectors and codec checks. The restored
release workflow repeats the Ruby and native matrix before publishing a tag.
Final Rust crate naming and crates.io publishing remain future work.
The legacy repository is preserved at
[Antti/audiowaveform-legacy](https://github.com/Antti/audiowaveform-legacy);
the active repository has independent history.

## PCM streaming and gapless follow-up (Ruby 0.4.0)

Local Rust all-feature and no-default-feature tests, formatting checks, and
strict Clippy for both workspaces pass. New coverage verifies all 12 PCM
encodings with split samples/frames, short/interrupted reads, final partial
buckets, empty input, mixing/gain precision, invalid metadata, truncated input,
nonfinite values, cancellation, and panic containment. A fixed-resolution
stream performs one pass and accepts readers with no seek implementation.

The PCM allocation test records zero allocations across 100 pushes after
initialization when no new output bucket is needed. Increasing input from
110,000 to 11,000,000 frames while keeping 110 output points produces identical
scratch/output capacities, with either fixed gain or normalization.

Ruby 4.0.5 passes 71 tests / 1,391 assertions and RBS validation, including IO
pipes, StringIO/current position, reused read buffers, errors and `throw`
payloads, timeout/thread cancellation while blocked on a pipe, and native
storage cleanup before GC. An isolated source-gem installation exercises the
new PCM API and MP3 trimming in addition to the numeric/codec checks. Package
activation explicitly selects the installed artifact even when a globally
installed native gem shares its version.

A manual end-to-end check generated a three-second, six-channel AAC/M4A,
decoded/downmixed it with FFmpeg into a mono s16le pipe, and consumed that pipe
through the Ruby API. It produced 144,000 frames, 110 points at 1,310 frames per
bucket, and 220 signed 8-bit values. All 16-bit peaks matched independent batch
min/max arithmetic over FFmpeg's PCM output; the production path used no WAV or
whole-recording buffer.

Gapless regression tests require the 12,000-frame synthetic MP3 to recover its
source length and a non-silent first bucket. Vorbis/Ogg and Vorbis/WebM likewise
reject the leading priming buckets seen in 0.3.0, allowing their documented
container-tail granularity. AAC/MP4 edit-list trimming remains a decoder
limitation; this does not promise bit-identical legacy peaks.

## Exact-point metadata fast path (Ruby 0.4.0)

PCM/float WAV contract vectors and native FLAC now report one decoding pass
for exact `points`, with unchanged literal peaks. AAC/M4A, MP3, ADPCM, Ogg,
CAF/AIFF, and Matroska/WebM still report two passes. Live WebM's missing-duration
case explicitly verifies that fallback.

FLAC fixtures with missing or understated STREAMINFO sample counts produce
identical output to the fast path in two passes, for fixed/normalized gain,
mono/split output, and counts both below and above the input frame count.
Truncated FLAC, overstated lengths reported as truncation, and failed decoded
MD5 checks remain errors. WAV tests cover misleading filename extensions and
bogus FACT counts; the optimization uses detected PCM data extents. Empty WAV
with `points: u32::MAX` retains zero peak allocation.

Rust all-feature/no-default-feature tests and strict Clippy pass, along with
Ruby's 71 tests / 1,391 assertions and RBS validation. No Ruby API change is
needed; `statistics().decode_passes` records the actual Rust decoding work.

The subsequent internal refactor separates fixed-resolution generation,
exact-point generation, and replay, keeps provisional counts with their peaks,
and gives peak storage ownership of gain handling. Ruby GVL/interrupt code is
extracted into a private module with only visibility and formatting changes.
All-feature, no-default-feature, WAV-only, and FLAC-only Rust tests still pass,
including peak, pass-count, and allocation checks. Formatting and strict Clippy
pass in both workspaces. Ruby passes 71 tests / 1,391 assertions and RBS checks;
an isolated source-gem installation verifies that the new module is packaged
and builds correctly.

The Ruby 0.4.0 version bump passes 72 Ruby tests / 1,393 assertions and RBS
validation. The built `audiowaveform-0.4.0.gem` installs into an isolated gem
directory and passes the contract vectors, codec matrix, PCM streaming, and
MP3 gapless checks. Both unpublished Rust packages remain at 0.1.0.

## AAC/MP4 playback trimming (unreleased)

Local all-feature, no-default-feature, and M4A-only Rust tests pass, along with
formatting and strict all-target Clippy for both workspaces. Ten parser tests
cover edit-list/header versions 0/1, different movie/media/sample clocks, selected
track IDs, partial packet boundaries, malformed/truncated atoms and timing
tables, unsupported edits, overflow, cancellation, and shared-file cursor
restoration. Eight integration tests decode newly generated AAC fixtures and
compare peaks with independent batch arithmetic over the known playback slice.

The 44.1 kHz 50 ms fixture decodes to 4,096 raw frames but retains exactly 2,205
frames and 110 points. Other cases cover 32/48/88.2/96 kHz, one-frame and odd-length
clips, mono/stereo, fixed/normalized gain, direct 8-bit output, true silence and
silent edges, changed edit offsets (including 2,112), empty playback, absent
edits, coarse movie-clock rounding, video before multiple audio tracks,
impossible sample-table lengths, and the documented fragmented-MP4 fallback.
Exact points and fixed-size buckets both use the same trimmed frame range.

The metadata reader uses fixed-size stack buffers and seeks past media; it
retains neither packet tables nor decoded audio. A manual comparison of 50 ms
and 30-second mono AAC at 44.1 kHz, each producing 110 points, reports identical
application capacities: 8,192 bytes of sample scratch and 456 bytes of
peak/current storage. Decoder/container allocations are outside these figures.

Ruby 4.0.5 passes 73 tests / 1,689 assertions and RBS validation. An isolated
source-gem installation passes the numeric/codec/PCM checks plus an AAC check
requiring a 50 ms duration and 110 points at 44.1, 88.2, and 96 kHz. This is a
local development build of the unchanged 0.4.0 package version, not a published
release.

The compatibility follow-up covers leading empty edits (including versions 0/1
and summed movie-clock durations), coarse media timestamps with and without an
edit, and a final media timestamp rounded beyond the decoded sample count.
An FFmpeg start-offset remux now yields a 150 ms waveform including its leading
silence. Both passes use that same timeline. Millisecond-clock fixtures retain
the frames inside their declared ranges without failing exact-time equality;
rounding past decoded EOF is clamped, while full-tick jumps, cumulative drift,
and larger shortages still fail. Source-gem smoke tests exercise both fixes.

Increasing a leading empty edit to ten seconds preserves application buffer
capacities with fixed and normalized gain at 110 points. A much larger empty
edit is cancelled during generation in both resolution modes, without building
a whole-gap sample buffer. The original 50 ms regression remains covered.

Playback bounds use the initialized AAC decoder's sample rate from
AudioSpecificConfig, which can differ from the container sample-entry rate.
The 88.2 and 96 kHz fixtures failed with `AAC/MP4 sample rate changed` before
this correction. They now retain exactly 4,410 and 4,800 frames, respectively,
with peaks checked in both resolution modes and installed-gem coverage.
