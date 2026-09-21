# Implementation record

This project was implemented on 2026-09-21 from the separately prepared
behavior contract and newly generated test corpus. It implements the Rust
core. The reviewed personal Ruby integration was subsequently reused and
adapted; see [its separate record](bindings/ruby/PROVENANCE.md).

## Inputs and process

- The supplied contract and exact-arithmetic numeric corpus under `contract/`.
  Its original specification-side provenance record is retained there.
- Symphonia 0.6.1 public API documentation and registry source, consulted for
  decoder/container interfaces, sample conversion semantics, end-of-stream
  behavior, and track flags. It remains an external dependency; its source is
  not vendored into this project.
- [Microsoft WAVEFORMATEX](https://learn.microsoft.com/en-us/windows/win32/api/mmreg/ns-mmreg-waveformatex)
  and [WAVEFORMATEXTENSIBLE](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ksmedia/ns-ksmedia-waveformatextensible)
  for WAV extents and channel-mask validation.
- [RFC 8794](https://www.rfc-editor.org/rfc/rfc8794.html) and
  [Matroska block/lacing documentation](https://www.matroska.org/technical/notes.html)
  for validating a complete unknown-length WebM document at EOF.
- Rust standard-library APIs and documentation.

The core production source, core tests, and initial implementation
documentation were written for this project. No former core implementation,
core test helper, legacy recording, or golden output was copied into the new
core. The old implementation
was not used as a numerical oracle for the initial implementation. Codec media is generated from new numeric
signals; the recipe, commands, tool version, and hashes are recorded in
`tests/build_codec_fixtures.py` and the fixture manifests.

The implementing assistant's broader conversation context previously examined
the old project and provenance audit. This is therefore **not a claim of an
unexposed clean-room implementer**. The current implementation work used the
contract, new test material, and the public dependency/format sources listed
above. Git author identity and a new root commit do not establish legal
independence or ownership by themselves.

## License and distribution status

On 2026-09-21, the author selected `MIT OR Apache-2.0` for the replacement Rust
core and reviewed personal Ruby integration. Both license texts and the choice
of terms are included in source/native gems; see [LICENSE.md](LICENSE.md).
The replacement Ruby release line starts at `0.3.0`; crates.io publishing remains disabled
pending final crate naming. No old GPL code or release is relicensed by this
project. The provenance limitations above remain applicable; license selection
and a new Git history do not themselves establish legal independence.

The core's only direct runtime dependency is Symphonia, under MPL-2.0. The locked
runtime/build graph contains MPL-2.0 Symphonia packages and dependencies with
MIT, Apache-2.0, or Zlib alternatives; it has no dependency on the former crate.
`serde_json` and `tempfile` are development-only test dependencies, not waveform
serialization functionality. Dependency obligations still apply to future
source and native gem distributions; the current source package does not
bundle dependency code.

## History cutover

This project starts an independent root history at Antti/audiowaveform. The
original repository is preserved as
[Antti/audiowaveform-legacy](https://github.com/Antti/audiowaveform-legacy),
including its original branches, tags, GitHub releases, and pull request history.
Its Ruby gems 0.1.0, 0.2.0, and 0.2.1 remain published under their original terms.
The archived release sources are not part of this project's Git ancestry.

Ruby integration and source/native package checks are recorded in
[VALIDATION.md](VALIDATION.md). The repository cutover is separate from gem/crate
publication; cross-platform release checks still apply.

## Ruby dependency and packaging follow-up

Ruby requires Magnus (MIT), rb-sys/rb-sys-env (MIT or Apache-2.0), and their
locked dependencies, in addition to the unchanged decoder dependency. Exact
crate versions, source locations, and original notices are recorded in
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md). The inventory includes build
and target-conditional dependencies, not just code linked on this host.
No vendored implementation of the former core is in either gem format.

The retained Ruby GVL helper was adapted using the installed Ruby C headers,
[Ruby's GVL/interrupt API documentation](https://docs.ruby-lang.org/capi/en/master/d6/dfb/include_2ruby_2thread_8h.html),
and Magnus 0.8.2 public API/source. Array conversion uses Magnus's protected
append API with a fixed stack buffer of immediate Ruby integers. It does not
copy the old core's peak conversion or serializers.

## PCM streaming and timing follow-up

After 0.3.0, comparisons against installed 0.2.1/0.3.0 gems and inspection of
legacy delay handling identified a timing regression. The correction uses
Symphonia 0.6.1's public gapless decoder option. Regression tests use this
repository's synthetic tone fixtures and their known source length, with no
copied legacy golden arrays. Raw PCM conversion/streaming was implemented from
standard signed PCM scaling, IEEE float encodings, and Rust byte-conversion APIs;
no legacy raw-input implementation was used or copied.

## AAC/MP4 timing follow-up

The focused MP4 timing reader was written for this project using Apple's
[AAC priming explanation](https://developer.apple.com/documentation/quicktime-file-format/background_aac_encoding),
[edit-list format](https://developer.apple.com/documentation/quicktime-file-format/edit_list_atom),
and time-to-sample/media/movie-header field definitions. Symphonia 0.6.1's
registry source was consulted for track IDs, packet timestamps, and its current
edit-list limitation; no parser implementation was copied. No legacy core code
was read or reused for this change. New arithmetic tone/silence fixtures and
encoding commands are recorded in `tests/build_aac_fixtures.py` and
`tests/fixtures/aac/manifest.json`. Peak expectations use independent batch
arithmetic over untrimmed Symphonia output sliced at known fixture boundaries.
