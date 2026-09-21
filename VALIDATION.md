# Validation

Validated on macOS arm64, Rust 1.98.1, on 2026-09-21. This report covers the
new Rust core and the Ruby integration below. It does not assert license
clearance or cross-platform binary verification.

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
crate. After the Ruby notice additions its 113 archive entries were inspected:
no old workspace, Ruby binding,
audit, Git metadata, build output, or measurement files are included. Cargo's
missing-license metadata warning is expected while the release license remains
unselected and publishing is disabled.

## Ruby integration and packaging

The extension uses the replacement core with all codecs, with no dependency on
its former implementation. `cargo fmt --check` and strict all-target Clippy
passed for both Cargo workspaces; the core all-feature tests passed again.

On macOS arm64:

- Ruby 4.0.5 passed the final 54-test / 966-assertion suite. Ruby 3.4.7
  passed the 53-test / 965-assertion integration suite before the additional
  publishing-guard regression; release tests were also checked separately.
- All 36 exact numeric vectors pass through the Ruby API, including metadata,
  array independence, point ordering, argument errors and omitted export APIs.
- The 17-format codec corpus passes in mono and split modes; lossless peaks
  match the independent fixture expectations.
- GVL progress, native GC pressure/ObjectSpace accounting, parallel reads with
  GC, and RBS runtime checks pass. Interrupted exact-count and fixed-resolution
  decoding of a sparse 500-million-frame WAV returns promptly and reclaims
  native allocations. Safety subprocesses have a 30-second watchdog.
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

The configured Linux/macOS/Windows, glibc/musl, and Ruby 3.2/3.3/3.4/4.0 CI
matrix has not been run remotely for this new repository. Ruby 3.2 and 3.3
were not available in this local check. The publishing workflow has not been
carried over; build/test workflows and the guarded publishing helper are local.
Final crate naming, license selection/review, full platform release checks, and
remote-history cutover remain future work. Nothing has been published.
