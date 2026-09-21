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
was not used as a numerical oracle. Codec media is generated from new numeric
signals; the recipe, commands, tool version, and hashes are recorded in
`tests/build_codec_fixtures.py` and the fixture manifests.

The implementing assistant's broader conversation context previously examined
the old project and provenance audit. This is therefore **not a claim of an
unexposed clean-room implementer**. The current implementation work used the
contract, new test material, and the public dependency/format sources listed
above. Git author identity and a new root commit do not establish legal
independence or ownership by themselves.

## License and distribution status

Publishing is disabled. The local Ruby prerelease is `0.3.0.pre.1`, with a
`Nonstandard` marker and an explicit publishing-script guard; this marker does
not grant a release license. No permissive license is asserted yet, and no old GPL
code or release is relicensed by this project. Complete the source/provenance
review and select the new project's license before distribution. Existing
release licenses and source obligations remain independent of Git history.

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
publication; release licensing and cross-platform checks still apply.

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
