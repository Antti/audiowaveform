# Changelog

## Unreleased

- Generate exact `points:` in one decoding pass for PCM/float WAV and native
  FLAC when an exact header frame count is available and verified. Other inputs
  retain the two-pass path; mismatched counts replay using actual decoded length.

- Enable decoder gapless trimming to remove the leading-delay regression in
  0.3.0 for MP3 and Vorbis/WebM. MP3 padding is also trimmed when reported.
- Add `generate_pcm(io, format:, sample_rate:, channels:, samples_per_pixel:)`
  for one-pass PCM streams, including FFmpeg pipes and StringIO. Reuse bounded
  working buffers, keep mono/split and gain support, and reject incomplete frames.
- Add Rust `PcmStream`, `generate_pcm`, and cancellation variants without seek
  requirements or codec features.

## 0.3.0 (2026-09-21)

- Use the replacement streaming core and newly generated test media.
- Keep exact `points:`, mono/split channels, normalization, metadata and point lookup.
- Return signed 8-/16-bit peak arrays directly, without a temporary Rust peak vector.
- Cancel native generation on Ruby interrupts and retain native GC accounting.
- Continue decoding after harmless thread wakeups and returning signal handlers.
- Remove `save`, `to_dat`, `to_json`, and `to_txt`.
- Calculate duration from decoded frames in every resolution mode.
- Use power-of-two PCM scaling; numeric peaks can differ from 0.2.x.
- Require a boolean for `split_channels` and a real Numeric gain or `:auto`/`"auto"`.
  Numeric strings are no longer accepted as gains. Invalid path/point types raise
  `ArgumentError`; integer point indices outside the waveform raise `IndexError`.
- Clamp pixels-per-second resolution to at least two frames per point.

The replacement is dual-licensed under MIT or Apache-2.0, at your option.
Legacy releases retain their original GPL-3.0-or-later terms.
