"""Build the shell's GB18030 reverse map and single-syllable Mandarin input dictionary."""

import argparse
import collections
import struct
import subprocess
from pathlib import Path


def convert(program, args, points):
    result = subprocess.run(
        [program, *args],
        input="\n".join(map(chr, points)).encode(),
        stdout=subprocess.PIPE,
        check=True,
    )
    rows = result.stdout.split(b"\n")
    if len(rows) != len(points):
        raise ValueError("unexpected converter line count")
    return rows


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("output", type=Path)
    cli.add_argument("--uconv", default="uconv")
    args = cli.parse_args()
    points = [n for n in range(128, 65536) if not 0xD800 <= n <= 0xDFFF]
    pairs = bytearray()
    dense = bytearray()
    ranges = []
    for cp, enc in zip(
        points, convert(args.uconv, ["-f", "UTF-8", "-t", "gb18030"], points)
    ):
        if len(enc) == 2:
            if 0x4E00 <= cp < 0x9FA6:
                dense.extend(enc)
            else:
                pairs.extend(struct.pack("<H", cp) + enc)
        elif len(enc) == 4:
            a, b, c, d = enc
            pointer = (((a - 129) * 10 + b - 48) * 126 + c - 129) * 10 + d - 48
            if (
                ranges
                and cp == ranges[-1][1] + 1
                and pointer == ranges[-1][2] + cp - ranges[-1][0]
            ):
                ranges[-1][1] = cp
            else:
                ranges.append([cp, cp, pointer])
        else:
            raise ValueError((cp, enc))
    han = []
    for a in range(0xB0, 0xF8):
        for b in range(0xA1, 0xFF):
            try:
                han.append(ord(bytes((a, b)).decode("gb2312")))
            except UnicodeDecodeError:
                pass
    readings = collections.defaultdict(list)
    for cp, syllable in zip(
        han, convert(args.uconv, ["-x", "Han-Latin; Latin-ASCII; Lower"], han)
    ):
        if syllable.isascii() and syllable.isalpha() and len(syllable) <= 7:
            readings[syllable].append(cp)
    preferred = {
        "zhong": "中",
        "wen": "文",
        "ni": "你",
        "hao": "好",
        "shi": "是",
        "wo": "我",
        "de": "的",
        "an": "安",
        "zhuang": "装",
    }
    for syllable, cp in preferred.items():
        group = readings[syllable.encode()]
        if ord(cp) in group:
            group.remove(ord(cp))
            group.insert(0, ord(cp))
    index = bytearray()
    values = bytearray()
    for syllable, group in sorted(readings.items()):
        index.extend(struct.pack("<8sHH", syllable, len(values) // 2, len(group)))
        values.extend(struct.pack("<" + "H" * len(group), *group))
    ranges_bin = b"".join(struct.pack("<HHI", *row) for row in ranges)
    assert len(dense) == 2 * (0x9FA6 - 0x4E00)
    args.output.write_bytes(
        struct.pack("<4sHHH", b"GBI2", len(pairs) // 4, len(ranges), len(readings))
        + dense
        + pairs
        + ranges_bin
        + index
        + values
    )


if __name__ == "__main__":
    main()
