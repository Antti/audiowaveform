#!/usr/bin/env python3
"""Check the replacement contract and build tiny, wholly synthetic WAV inputs.

This batch/rational model is test infrastructure, not production waveform code.
No third-party packages, existing implementation, or legacy fixtures are used.
"""

import argparse
from fractions import Fraction
import hashlib
import io
import json
from pathlib import Path
import struct
import uuid
import wave


ROOT = Path(__file__).resolve().parent
VECTOR_PATH = ROOT / "vectors.json"
WIDTHS = {"pcm_u8": 1, "pcm_s16le": 2, "pcm_s24le": 3,
          "pcm_s32le": 4, "float32le": 4}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def normalized(sample, encoding):
    if encoding == "float32le":
        # Model the actual float32 payload, not a higher-precision decimal input.
        value = struct.unpack("<f", struct.pack("<f", float(sample)))[0]
        return Fraction(value)
    bits = WIDTHS[encoding] * 8
    value = sample - 128 if encoding == "pcm_u8" else sample
    return Fraction(value, 1 << (bits - 1))


def model(fixture, options):
    """Small-input specification oracle, intentionally independent of streaming."""
    frames = [[normalized(s, fixture["encoding"]) for s in frame]
              for frame in fixture["frames"]]
    count = len(frames)
    split = options.get("split_channels", False)
    channels = fixture["channels"] if split else 1
    if not split:
        frames = [[sum(frame) / len(frame)] for frame in frames]
    points = options.get("points")
    if points is not None:
        scale = max(2, count // points)
        ranges = [(i * count // points, (i + 1) * count // points)
                  for i in range(points)] if count else []
    else:
        rate = options.get("pixels_per_second")
        scale = (max(2, fixture["rate"] // rate) if rate is not None
                 else options.get("samples_per_pixel") or 256)
        ranges = [(start, min(start + scale, count))
                  for start in range(0, count, scale)]
    gain = options.get("amplitude_scale")
    if gain == "auto":
        maximum = max((abs(s) for f in frames for s in f), default=Fraction(0))
        gain = Fraction(32767, 32768) / maximum if maximum else Fraction(1)
    else:
        # Mirror the specified binary64 conversion of Ruby numeric gains.
        gain = Fraction(float(gain)) if gain is not None else Fraction(1)
    data = []
    for start, end in ranges:
        bucket = frames[start:end] if end > start else [frames[start]]
        for channel in range(channels):
            samples = [f[channel] for f in bucket]
            for extremum in (min(samples), max(samples)):
                # int(Fraction) truncates toward zero without float overflow.
                data.append(max(-32768, min(32767, int(extremum * gain * 32768))))
    return {
        "frames": count, "sample_rate": fixture["rate"], "channels": channels,
        "length": len(ranges), "samples_per_pixel": scale,
        "data16": data, "data8": [int(Fraction(v, 256)) for v in data],
    }


def chunk(name, data):
    return name + struct.pack("<I", len(data)) + data + b"\0" * (len(data) % 2)


def wav_bytes(fixture):
    encoding = fixture["encoding"]
    width = WIDTHS[encoding]
    channels = fixture["channels"]
    rate = fixture["rate"]
    block = width * channels
    floating = encoding == "float32le"
    extensible = channels > 2 or (width > 2 and not floating)
    tag = 0xFFFE if extensible else (3 if floating else 1)
    fmt = struct.pack("<HHIIHH", tag, channels, rate, rate * block, block, width * 8)
    if extensible:
        mask = {1: 0x4, 2: 0x3, 6: 0x3F}[channels]
        subtype = uuid.UUID("00000001-0000-0010-8000-00aa00389b71").bytes_le
        fmt += struct.pack("<HHI", 22, width * 8, mask) + subtype
    elif floating:
        fmt += struct.pack("<H", 0)  # WAVEFORMATEX cbSize
    payload = bytearray()
    for frame in fixture["frames"]:
        for sample in frame:
            if floating:
                payload.extend(struct.pack("<f", float(sample)))
            else:
                payload.extend(sample.to_bytes(width, "little", signed=width != 1))
    chunks = chunk(b"fmt ", fmt)
    if floating:
        chunks += chunk(b"fact", struct.pack("<I", len(fixture["frames"])))
    chunks += chunk(b"data", bytes(payload))
    return b"RIFF" + struct.pack("<I", len(chunks) + 4) + b"WAVE" + chunks


def verify_wav(fixture, encoded):
    require(encoded[:4] == b"RIFF" and encoded[8:12] == b"WAVE", "WAV signature")
    require(struct.unpack_from("<I", encoded, 4)[0] == len(encoded) - 8, "RIFF size")
    chunks = {}
    offset = 12
    while offset < len(encoded):
        name, size = struct.unpack_from("<4sI", encoded, offset)
        require(name not in chunks, "duplicate WAV chunk")
        chunks[name] = encoded[offset + 8:offset + 8 + size]
        require(len(chunks[name]) == size, "truncated WAV chunk")
        offset += 8 + size + size % 2
    require(offset == len(encoded), "WAV padding")
    fmt = chunks[b"fmt "]
    tag, channels, rate, byte_rate, block, bits = struct.unpack_from("<HHIIHH", fmt)
    width = WIDTHS[fixture["encoding"]]
    require((channels, rate, bits) == (fixture["channels"], fixture["rate"], width * 8),
            "WAV format metadata")
    require(block == channels * width and byte_rate == rate * block, "WAV byte rate")
    payload = chunks[b"data"]
    require(len(payload) == len(fixture["frames"]) * block, "WAV data size")
    if tag == 0xFFFE:
        require(len(fmt) == 40, "extensible format length")
        extra_size, valid_bits, mask = struct.unpack_from("<HHI", fmt, 16)
        require(extra_size == 22 and valid_bits == bits, "valid precision")
        require(bin(mask).count("1") == channels, "speaker mask population")
        require(str(uuid.UUID(bytes_le=fmt[24:40])) ==
                "00000001-0000-0010-8000-00aa00389b71", "PCM subtype")
    elif tag == 3:
        require(struct.unpack("<I", chunks[b"fact"])[0] == len(fixture["frames"]),
                "float WAV frame count")
    else:
        require(tag == 1, "PCM format tag")
        # A separate standard-library reader checks the conventional PCM files.
        with wave.open(io.BytesIO(encoded), "rb") as reader:
            require(reader.getnframes() == len(fixture["frames"]), "wave frame count")
            require(reader.readframes(reader.getnframes()) == payload, "wave payload")
    recovered = []
    for offset in range(0, len(payload), width):
        raw = payload[offset:offset + width]
        sample = (struct.unpack("<f", raw)[0] if tag == 3 else
                  int.from_bytes(raw, "little", signed=width != 1))
        recovered.append(normalized(sample, fixture["encoding"]))
    expected = [normalized(s, fixture["encoding"])
                for frame in fixture["frames"] for s in frame]
    require(recovered == expected, "WAV sample round trip")


def check(corpus):
    require(corpus["revision"] == 1, "unsupported contract revision")
    names = set()
    for name, fixture in corpus["fixtures"].items():
        require(fixture["channels"] > 0 and fixture["rate"] > 0, name + ": dimensions")
        require(all(len(f) == fixture["channels"] for f in fixture["frames"]),
                name + ": inconsistent channel count")
        verify_wav(fixture, wav_bytes(fixture))
    for case in corpus["cases"]:
        name = case["id"]
        require(name not in names, "duplicate case: " + name)
        names.add(name)
        result = model(corpus["fixtures"][case["fixture"]], case["options"])
        require(result == case["expected"], name + ": expected values differ from formulas")
        for bits in (8, 16):
            data = result["data" + str(bits)]
            require(len(data) == 2 * result["length"] * result["channels"], name + ": layout")
            require(all(-(1 << (bits - 1)) <= v < (1 << (bits - 1)) for v in data),
                    name + ": output range")
            require(all(lo <= hi for lo, hi in zip(data[::2], data[1::2])), name + ": extrema")
    # Exercise partition arithmetic beyond the hand-picked examples. Nonempty
    # ranges partition every source frame; repeated singleton ranges stay valid.
    for count in range(1, 33):
        for points in range(1, 41):
            ranges = [(i * count // points, (i + 1) * count // points)
                      for i in range(points)]
            visited = [j for start, end in ranges for j in range(start, end)]
            require(visited == list(range(count)), "partition coverage")
            require(all(0 <= start < count and start <= end <= count for start, end in ranges),
                    "partition boundaries")
    # Large integer arithmetic without large input or output allocations.
    count, points = (1 << 64) - 1, (1 << 32) - 1
    require(points * count // points == count, "wide final boundary")
    print(f"PASS: {len(corpus['cases'])} literal cases, {len(corpus['fixtures'])} WAV definitions, "
          "1,280 partitions, and wide-integer boundary arithmetic")


def build(corpus, destination):
    if destination.exists():
        require(destination.is_dir() and not any(destination.iterdir()),
                "destination must be absent or empty; existing files are never overwritten")
    destination.mkdir(parents=True, exist_ok=True)
    contents = {name + ".wav": wav_bytes(f) for name, f in corpus["fixtures"].items()}
    contents["invalid_header.wav"] = b"synthetic non-audio input\n"
    for name, value in (("nonfinite_nan", "nan"), ("nonfinite_inf", "inf")):
        contents[name + ".wav"] = wav_bytes({"encoding": "float32le", "rate": 8000,
                                             "channels": 1, "frames": [[value]]})
    hashes = {}
    for name, data in sorted(contents.items()):
        (destination / name).write_bytes(data)
        hashes[name] = hashlib.sha256(data).hexdigest()
    manifest = {"revision": 1,
                "vectors_sha256": hashlib.sha256(VECTOR_PATH.read_bytes()).hexdigest(),
                "files": hashes}
    (destination / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(f"Built {len(contents)} synthetic inputs in {destination}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("check")
    builder = sub.add_parser("build")
    builder.add_argument("destination", type=Path)
    args = parser.parse_args()
    corpus = json.loads(VECTOR_PATH.read_text())
    try:
        check(corpus)
        if args.command == "build":
            build(corpus, args.destination)
    except (ValueError, KeyError, OverflowError, struct.error) as error:
        parser.exit(1, f"FAIL: {error}\n")


if __name__ == "__main__":
    main()
