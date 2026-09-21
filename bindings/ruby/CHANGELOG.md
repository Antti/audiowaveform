# Changelog

## 0.3.0.pre.1 (unreleased)

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
