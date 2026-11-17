"""Build a sector-addressable GB18030 bitmap font using GNU Unifont and ICU."""

import argparse
import gzip
from lz4block import compress
import io
import json
from pathlib import Path
import struct
import subprocess



def four_byte_runs(keys):
    """Index consecutive GB18030 code points without reading compressed key pages."""
    runs = []
    for code in keys:
        a, b, c, d = code.to_bytes(4, "big")
        ordinal = (((a - 0x81) * 10 + b - 0x30) * 126 + c - 0x81) * 10 + d - 0x30
        if runs and runs[-1][0] + runs[-1][1] == ordinal:
            runs[-1][1] += 1
        else:
            runs.append([ordinal, 1])
    return runs

def build(primary, upper, output, full, uconv):
    glyphs = {}
    for source in (primary, upper):
        with gzip.open(source, "rt") as f:
            for line in f:
                number, bits = line.strip().split(":")
                cp = int(number, 16)
                data = bytes.fromhex(bits)
                if len(data) == 16:
                    data = b"".join(bytes((b, 0)) for b in data)
                if len(data) == 32 and not 0xD800 <= cp <= 0xDFFF and cp >= 32:
                    glyphs[cp] = data
    points = sorted(glyphs)
    enc = subprocess.run(
        [uconv, "-f", "UTF-8", "-t", "gb18030"],
        input="\n".join(map(chr, points)).encode(),
        stdout=subprocess.PIPE,
        check=True,
    ).stdout.split(b"\n")
    assert len(enc) == len(points)
    box = bytes.fromhex("ffff" + "8001" * 14 + "ffff")
    pairs = bytearray(box * (126 * 190))
    ascii_glyphs = bytearray(32 * 128)
    extra = {}
    locales = json.loads(
        (Path(__file__).resolve().parents[1] / "native-tools/locales.json").read_text()
    )
    essential = {ord(c) for lang in locales for msg in lang for c in msg} | {
        0x20000,
        0x1F600,
        0xFFFD,
    }
    for cp, code in zip(points, enc):
        if len(code) == 1 and code[0] < 128:
            ascii_glyphs[cp * 32 : (cp + 1) * 32] = glyphs[cp]
        elif len(code) == 2:
            a, b = code
            idx = (a - 129) * 190 + b - 64 - (b > 127)
            pairs[idx * 32 : idx * 32 + 32] = glyphs[cp]
        elif len(code) == 4 and (
            full
            or cp in essential
            or 0x3400 <= cp < 0x4DC0
            or 0xAC00 <= cp <= 0xD7A3
            or 0x3000 <= cp <= 0x31FF
            or 0x80 <= cp < 0x300
        ):
            extra[int.from_bytes(code, "big")] = glyphs[cp]
    keys = sorted(extra)
    directory = 32 + len(ascii_glyphs) + len(pairs)
    with io.BytesIO() as f:
        f.write(
            struct.pack(
                "<4sIII16x", b"GBF1", len(keys), directory, directory + 4 * len(keys)
            )
        )
        f.write(ascii_glyphs)
        f.write(pairs)
        for k in keys:
            f.write(struct.pack("<I", k))
        for k in keys:
            f.write(extra[k])
        raw = f.getvalue()
    chunks = [compress(raw[pos : pos + 4096]) for pos in range(0, len(raw), 4096)]
    # Old GBC1 readers follow offsets directly to block zero and skip this index.
    runs = four_byte_runs(keys)
    index = struct.pack("<4sII", b"GBR1", directory + 4 * len(keys), len(runs))
    index += b"".join(struct.pack("<II", first, length) for first, length in runs)
    pos = 12 + 4 * (len(chunks) + 1) + len(index)
    offsets = [pos]
    for chunk in chunks:
        pos += len(chunk)
        offsets.append(pos)
    output.write_bytes(
        struct.pack("<4sII", b"GBC1", len(raw), len(chunks))
        + struct.pack("<" + "I" * len(offsets), *offsets)
        + index
        + b"".join(chunks)
    )
    print(output, output.stat().st_size, "bytes,", len(keys), "four-byte glyphs")


if __name__ == "__main__":
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("primary", type=Path)
    cli.add_argument("upper", type=Path)
    cli.add_argument("output", type=Path)
    cli.add_argument("--full", action="store_true")
    cli.add_argument("--uconv", default="uconv")
    a = cli.parse_args()
    build(a.primary, a.upper, a.output, a.full, a.uconv)
