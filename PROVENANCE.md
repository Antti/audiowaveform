# Implementation record

This project was implemented on 2026-09-21 from the separately prepared
behavior contract and newly generated test corpus. It implements the Rust
core; no Ruby wrapper or release workflow has been copied or migrated.

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

Production source, tests, manifests, and implementation documentation were
written for this project. No old Rust/C++ implementation, old test helper,
legacy recording, or golden output was copied into it. The old implementation
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

Publishing is disabled. No permissive license is asserted yet, and no old GPL
code or release is relicensed by this project. Complete the source/provenance
review and select the new project's license before distribution. Existing
release licenses and source obligations remain independent of Git history.

The only direct runtime dependency is Symphonia, under MPL-2.0. The locked
runtime/build graph contains MPL-2.0 Symphonia packages and dependencies with
MIT, Apache-2.0, or Zlib alternatives; it has no dependency on the former crate.
`serde_json` and `tempfile` are development-only test dependencies, not waveform
serialization functionality. Dependency obligations still apply to future
source and native gem distributions; the current source package does not
bundle dependency code.

## History cutover

The user wants eventually to replace the existing remote history. This local
project starts a new root history without modifying that remote. Before the
cutover, finish Ruby integration, source/native packaging, license/notice
review, and release checks. Preserve an accessible archive of old releases and
their corresponding source. The remote branch replacement should be a
deliberate cutover from the reviewed root, not a force-push from the old port.
