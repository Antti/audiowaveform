# Provenance record

Prepared on 2026-09-21 for the user's request to replace the GPL-associated
Rust core while retaining the Ruby audio-to-peaks use case.

## Inputs used to prepare this kit

1. User requirements in this conversation: bounded memory, direct signed 8-bit
   and 16-bit arrays, exact point counts without duration metadata, no CLI
   compatibility, and no DAT/JSON/TXT exports.
2. The public Ruby interface declarations and usage documentation at repository
   revision `6b5e886f38022e8a74649db80608fa8cafef4ab1`, used to identify API names
   and behavior to retain. The specification is newly written; those documents
   are not bundled here.
3. Explicit product decisions recorded in README.md and SPEC.md, including
   exact duration in every mode and the replacement's quantization convention.
4. Public platform documentation for WAV structure:
   [Microsoft WAVEFORMATEX](https://learn.microsoft.com/en-us/windows/win32/api/mmreg/ns-mmreg-waveformatex),
   [Microsoft WAVEFORMATEXTENSIBLE](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ksmedia/ns-ksmedia-waveformatextensible),
   and [Python wave](https://docs.python.org/3/library/wave.html).

The specification authoring context previously examined the existing project
and its provenance audit. This kit therefore does **not** claim that its author
was unexposed to the old source. It is a specification-side artifact intended
for review before transfer to an independent implementer, not a certification
of a clean-room process or legal clearance.

## Numeric and test material

The vector inputs are newly selected numeric sequences: zeros, positive and
negative ramps, impulses, opposite-polarity stereo, integer range boundaries,
and exactly representable binary fractions. None is sampled from a recording.
No audio, image, expected output, or helper from the old fixture/test suite is
copied into this kit. No old executable or library is queried for expectations.

Expected peak arrays are literal values chosen from the written arithmetic
rules. `conformance.py` cross-checks them with rational arithmetic and writes
uncompressed WAV files directly from the listed samples. Its non-streaming
model is intentionally simple test infrastructure for tiny inputs; it is not
a candidate production implementation or a memory-efficient algorithm to port.

The Ruby runner consumes the literal expectations. JSON is only a portable
description of test inputs/outputs, independent of any former waveform schema.

## Implementation provenance still required

The implementer should record the exact handoff contents/hashes, allowed public
documentation, any other code read or reused, and dependency licenses. They
should not receive the old implementation, audit, old fixtures, or git history.
Fresh implementation does not itself clear reused Ruby files: review each
candidate file independently and record its ownership and source references.

The user states their Rust/Ruby contributions were personal and that they have
no BBC relicensing permission. No existing GPL work is relicensed by this kit.
Selection of a license for a future reviewed replacement is a separate step.
