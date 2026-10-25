#!/usr/bin/env python3
"""Writes the crafted part of tests/golden/corpus/.

The oracle reads these files; it never generates inputs itself, so the Rust tests can read the
same inputs. Everything here is deterministic (fixed seeds), and running the script again must
leave `git status` clean. The Omarchy themes, the historical project files and the real ffprobe
recordings come from import-omarchy-themes.sh, make-historical-projects.sh and record-ffprobe.sh
instead.

Usage: make-corpus.py [corpus-dir]   (default: tests/golden/corpus next to this script)
"""

import json
import math
import os
import random
import shutil
import struct
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CORPUS = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "tests" / "golden" / "corpus"


def bits(value: float) -> int:
    return struct.unpack("<Q", struct.pack("<d", value))[0]


def from_bits(pattern: int) -> float:
    return struct.unpack("<d", struct.pack("<Q", pattern))[0]


def write(path: Path, data) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    if isinstance(data, str):
        data = data.encode("utf-8")
    path.write_bytes(data)


def reset(path: Path) -> None:
    if path.exists():
        shutil.rmtree(path)
    path.mkdir(parents=True)


def dumps(value) -> str:
    """Compact JSON for crafted inputs (Python's own float repr round-trips)."""
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"))


# ---------------------------------------------------------------------------------------------
# numbers/: doubles, qint64 values, and strings to convert to numbers


def edge_doubles() -> list:
    values = [
        0.0, -0.0, 1.0, -1.0, 0.5, -0.5, 1.5, -1.5, 2.0, 2.5, -2.5, 3.0, 10.0, 100.0,
        0.1, 0.2, 0.3, 0.1 + 0.2, 1.0 / 3.0, 2.0 / 3.0, -1.0 / 3.0, 12.5, 0.001, 0.0001,
        1e-5, 1e-6, 1e-7, 1e-9, 1e-12, 1e-13, 0.49999999999999994, 0.5000000000000001,
        4.35, 0.035, 0.28, 0.7, 0.25, 0.75, 0.55, 0.02, 0.8, 0.85, 0.08, 0.04, 0.06, 0.07,
        0.12, 0.16, 0.18, 0.22, 0.35, 0.4, 0.97, 1.2, 1.4, 1.6, 1.8, 2.25, 8.25, 12.5, 23.0,
        40.0, 45.0, 90.0, 135.0, 200.0, 470.0, 1080.0, 1920.0, 1e15, 1e16, 1e17, 1e20, 1e21,
        1e22, 1e23, 123456789012345678.0, 9007199254740993.0, 9007199254740991.0,
        2.0 ** 53, 2.0 ** 53 + 2, 2.0 ** 52 + 0.5, 2.0 ** 31, 2.0 ** 31 - 1, -(2.0 ** 31),
        2.0 ** 31 - 0.5, -(2.0 ** 31) - 0.5, 2.0 ** 32, 2.0 ** 63, -(2.0 ** 63), 2.0 ** 64,
        from_bits(0x43DFFFFFFFFFFFFF), 1.7976931348623157e308, 2.2250738585072014e-308,
        5e-324, 1e-320, 1e-310, 1e300, 1e-300, 1e100, 1e-100, 3.14159265358979,
        math.pi, math.e, 1.1, 2.675, 1.005, 0.015, 0.045, 1234.5678, 1e5, 123456.0,
        999999.0, 1000000.0, 9999999.0, 0.000123, 0.0000123, 0.9999999999999999,
        299792458.0, 6.02214076e23, 1.602176634e-19,
    ]
    patterns = [bits(v) for v in values]
    patterns += [bits(-v) for v in values if v != 0.0]
    patterns += [
        0x0000000000000001, 0x8000000000000001, 0x000FFFFFFFFFFFFF, 0x800FFFFFFFFFFFFF,
        0x0010000000000000, 0x8010000000000000, 0x7FEFFFFFFFFFFFFF, 0xFFEFFFFFFFFFFFFF,
        0x7FF0000000000000, 0xFFF0000000000000, 0x7FF8000000000000, 0xFFF8000000000000,
        0x7FF0000000000001, 0x7FF8000000000123, 0x3FEFFFFFFFFFFFFF, 0x3FF0000000000001,
        0x3FDFFFFFFFFFFFFF, 0x3FE0000000000001,
    ]
    for k in range(-20, 21):
        patterns.append(bits(k * 0.5))
        patterns.append(bits(k * 0.5) + 1)
    for k in range(-30, 31):
        patterns.append(bits(float(f"1e{k}")))
    for k in (-323, -310, -308, -200, -100, -50, 50, 100, 200, 300, 307, 308):
        patterns.append(bits(float(f"1e{k}")))
    for k in range(-60, 71, 1):
        patterns.append(bits(2.0 ** k))
    for k in (52, 53, 54, 55, 56):
        patterns.append(bits(2.0 ** k + 1))
        patterns.append(bits(2.0 ** k - 1))
    return patterns


def nice_decimals(rng: random.Random, count: int) -> list:
    patterns = []
    for _ in range(count):
        digits = rng.randint(1, 17)
        mantissa = rng.randint(10 ** (digits - 1), 10 ** digits - 1)
        exponent = rng.randint(-12, 24) - digits + 1
        value = float(f"{mantissa}e{exponent}")
        if rng.random() < 0.3:
            value = -value
        patterns.append(bits(value))
    return patterns


def unique(patterns: list) -> list:
    seen = set()
    result = []
    for pattern in patterns:
        if pattern not in seen:
            seen.add(pattern)
            result.append(pattern)
    return result


def make_numbers() -> None:
    out = CORPUS / "numbers"
    reset(out)
    rng = random.Random(20261024)
    edges = unique(edge_doubles())
    nice = nice_decimals(rng, 1500)
    ranged = [bits(rng.uniform(-2000.0, 2000.0)) for _ in range(300)]
    ranged += [bits(rng.random()) for _ in range(300)]
    ranged += [bits(round(rng.uniform(0.0, 20.0), rng.randint(0, 6))) for _ in range(300)]
    randoms = [rng.getrandbits(64) for _ in range(5000)]
    everything = unique(edges + nice + ranged + randoms)
    header = ("# IEEE-754 binary64 bit patterns, one per line as 16 lowercase hex digits.\n"
              "# Lines starting with '#' are comments. Generated by tools/oracle/make-corpus.py.\n")
    write(out / "doubles.txt", header + "".join(f"{p:016x}\n" for p in everything))
    # The subset QString::number is run over with every format and precision.
    subset = unique(edges + nice[:350] + ranged[:150] + ranged[300:400] + ranged[600:700]
                    + randoms[:150])
    write(out / "doubles-format.txt", header + "".join(f"{p:016x}\n" for p in subset))

    ints = [0, 1, -1, 2, -2, 7, 10, 42, 100, 255, 256, 1000, 65535, 65536, 2 ** 31 - 1,
            -(2 ** 31), 2 ** 31, -(2 ** 31) - 1, 2 ** 32, 2 ** 32 - 1, 2 ** 53 - 1, 2 ** 53,
            2 ** 53 + 1, -(2 ** 53) - 1, -(2 ** 53), 2 ** 62, 2 ** 63 - 1, -(2 ** 63),
            -(2 ** 63) + 1, 1000000, 1000001, 9000000, 1250000, 123456789012345678,
            999999999999999999, 1000000000000000000]
    ints += [10 ** k for k in range(19)] + [-(10 ** k) for k in range(19)]
    ints += [rng.getrandbits(64) - 2 ** 63 for _ in range(600)]
    ints += [rng.randint(-10 ** 6, 10 ** 6) for _ in range(200)]
    ints += [rng.randint(0, 2 ** 53) for _ in range(200)]
    seen = set()
    lines = []
    for value in ints:
        if value not in seen:
            seen.add(value)
            lines.append(f"{value}\n")
    write(out / "int64.txt", "# Signed 64-bit integers in decimal, one per line.\n" + "".join(lines))

    strings = [
        "", " ", "0", "-0", "+0", "00", "007", "1", "-1", "+1", " 1", "1 ", " 1 ", "\t1\n",
        "1.0", "1.", ".5", "-.5", "+.5", "1.5", "-1.5", "1e5", "1E5", "1e+5", "1e-5", "1e",
        "1e+", "e5", ".", "-", "+", "--1", "+-1", "1,5", "1,000", "1_000", "1 000", "12.5px",
        "0.25", "0.040", "1.0e0", "inf", "-inf", "+inf", "Inf", "INF", "infinity", "Infinity",
        "-Infinity", "nan", "NaN", "-nan", "NAN", "nan(123)", "0x10", "0X1A", "-0x10", "0x",
        "0b101", "010", "0o10", "1e400", "-1e400", "1e-400", "4.9e-324", "2.5e-324",
        "1.7976931348623157e308", "1.7976931348623159e308", "2147483647", "2147483648",
        "-2147483648", "-2147483649", "9223372036854775807", "9223372036854775808",
        "-9223372036854775808", "-9223372036854775809", "18446744073709551616",
        "9007199254740993", "123456789012345678901234567890", "0.1", "0.30000000000000004",
        "3.0000000000000000000000000001", "1" + "0" * 400, "0." + "0" * 400 + "1",
        "true", "false", "abc", "1abc", "abc1", "١٢", "１２", "१",
        "¹", "½", "１.５", "−1", "–1", "1 ", " 1", " 1",
        "1 ", "　" + "1", "+ 1", "- 1", "1e 5", "1 e5", "0.5e1", "5e-1", "-0.0",
        "12", "13.5", "15", "-3", "0.0", "1.0000000000000002", "100", "08", "09", "0e0",
        "\u00001", "1\u0000", "\r1", "1\r", "\v1", "\f1", "１", "٣", "12\n", "6.02214076e23",
    ]
    write(out / "strings.json", json.dumps(strings, ensure_ascii=True, indent=1) + "\n")


# ---------------------------------------------------------------------------------------------
# qjson/: documents, number literals and string literals


def nest(open_: str, close: str, depth: int, inner: str) -> str:
    return open_ * depth + inner + close * depth


def make_qjson() -> None:
    out = CORPUS / "qjson"
    reset(out)
    docs = {}
    docs["empty-object"] = b"{}"
    docs["empty-array"] = b"[]"
    docs["empty-object-spaced"] = b"{ }"
    docs["empty-array-spaced"] = b"[ \n ]"
    docs["nested-empties"] = (
        b'{"a":{},"b":[],"c":[{}],"d":[[]],"e":{"f":{"g":{}}},"h":[[],[[]],{},[{}]],'
        b'"i":{"j":[],"k":{"l":[[[]]]}}}')
    docs["array-of-empties"] = b"[{},[],{},[[],{}],[{},[]]]"
    docs["scalars-in-array"] = b'[null,true,false,0,1,-1,"s",""]'
    docs["scalars-in-object"] = b'{"n":null,"t":true,"f":false,"z":0,"o":1,"m":-1,"s":"s","e":""}'
    docs["top-level-string"] = b'"hello"'
    docs["top-level-number"] = b"42"
    docs["top-level-double"] = b"1.5"
    docs["top-level-null"] = b"null"
    docs["top-level-true"] = b"true"
    docs["deep-array-32"] = nest("[", "]", 32, "1").encode()
    docs["deep-object-32"] = ('{"k":' * 32 + "1" + "}" * 32).encode()
    docs["deep-array-1024"] = nest("[", "]", 1024, "").encode()
    docs["deep-array-1025"] = nest("[", "]", 1025, "").encode()
    docs["deep-object-1024"] = ('{"k":' * 1023 + "{}" + "}" * 1023).encode()
    docs["deep-object-1025"] = ('{"k":' * 1024 + "{}" + "}" * 1024).encode()
    docs["deep-mixed-1024"] = ('[{"k":' * 512 + "0" + "}]" * 512).encode()
    docs["numbers"] = (
        b'[0,-0,0.0,-0.0,1.0,1e5,1E5,1e+5,1E-5,1e-7,123456789012345678,9007199254740993,'
        b'9223372036854775807,9223372036854775808,-9223372036854775808,-9223372036854775809,'
        b'18446744073709551615,18446744073709551616,0.1,0.30000000000000004,4.9e-324,'
        b'2.2250738585072014e-308,1.7976931348623157e308,100,1e2,1.5,-1.5,2147483647,'
        b'2147483648,-2147483648,-2147483649,12345678901234567890123,0.000001,1e-6,'
        b'5e-324,2.5e-324,8.25,0.035,0.28,470,1e21,1e22,1e23,1.0e0,10.0,-10.0]')
    docs["numbers-typical"] = (
        b'{"padding":0.1,"radius":12,"intensity":0.28,"blur":23,"distance":20,"angle":45,'
        b'"offset":{"x":0.035,"y":0.035},"size":0.25,"alpha":0.7,"mass":2.25,"stiffness":200,'
        b'"damping":40,"t":1234567890123,"first_frame_us":1000000,"stopped_us":9000000,'
        b'"x":320.123,"y":179.5,"dx":-1.5,"dy":0.0,"speed":1.2,"out":8.25,"in":0.0}')
    docs["number-1e400"] = b"[1e400]"
    docs["number-minus-1e400"] = b"[-1e400]"
    docs["number-2e308"] = b"[2e308]"
    docs["number-leading-dot"] = b"[.5]"
    docs["number-leading-zero"] = b"[01]"
    docs["number-plus"] = b"[+1]"
    docs["number-trailing-dot"] = b"[1.]"
    docs["number-hex"] = b"[0x10]"
    docs["number-nan"] = b"[NaN]"
    docs["number-infinity"] = b"[Infinity]"
    docs["number-minus"] = b"[-]"
    docs["number-exponent-only"] = b"[1e]"
    docs["number-top-level-leading-zero"] = b"01"
    docs["strings"] = (
        '{"plain":"abc","quote":"\\"","backslash":"\\\\","slash":"\\/","b":"\\b","f":"\\f",'
        '"n":"\\n","r":"\\r","t":"\\t","nul":"\\u0000","us":"\\u001f","del":"\\u007f",'
        '"c1":"\\u0080","eacute":"\\u00e9","EACUTE":"\\u00E9","euro":"\\u20ac",'
        '"pair":"\\ud83d\\ude00","PAIR":"\\uD83D\\uDE00","raw":"é€😀","rawdel":"\x7f",'
        '"lone-high":"\\ud800","lone-low":"\\udc00","reversed":"\\udc00\\ud800",'
        '"high-then-a":"\\ud800a","bom":"\\ufeff","nonchar":"\\uffff","fffe":"\\ufffe",'
        '"mixed":"a\\tb\\nc\\"d\\\\e\\u0001f","empty":"","spaces":"  x  ",'
        '"u2028":"\\u2028\\u2029","long":"' + "x" * 300 + '"}').encode("utf-8")
    docs["key-order"] = (
        '{"b":1,"a":2,"B":3,"A":4,"_":5,"0":6,"10":7,"9":8,"é":9,"ß":10,"\\u00ff":11,'
        '"\\u0100":12,"\\uff01":13,"\\ud83d\\ude00":14,"€":15,"aa":16,"a ":17,"":18,'
        '"a\\u0000b":19,"Z":20,"z":21,"\\u007f":22,"~":23,"\\ud7ff":24,"\\ue000":25,'
        '"\\ud800\\udc00":26,"\\uffff":27,"ab":28,"a\\u0000":29,"\\u0001":30}').encode("utf-8")
    docs["key-order-nested"] = (
        '{"z":{"y":[{"b":1,"a":2}],"x":{"é":1,"e":2,"f":3}},"a":[{"d":4,"c":3}]}').encode("utf-8")
    docs["duplicate-keys"] = b'{"a":1,"b":0,"a":2}'
    docs["duplicate-keys-types"] = b'{"a":1,"a":{"x":1},"a":[1],"a":"s"}'
    docs["duplicate-keys-nested"] = b'{"o":{"k":1,"k":2},"o":{"k":3}}'
    docs["whitespace-all"] = b' \t\r\n{ \t\r\n"a" \t\r\n: \t\r\n[ \t\r\n1 \t\r\n, \t\r\n2 \t\r\n] \t\r\n} \t\r\n'
    docs["whitespace-crlf-indented"] = b'{\r\n    "a": 1,\r\n    "b": [\r\n        true\r\n    ]\r\n}\r\n'
    docs["whitespace-vertical-tab"] = b'{\x0b"a":1}'
    docs["whitespace-form-feed"] = b'{\x0c"a":1}'
    docs["whitespace-nbsp"] = b'{\xc2\xa0"a":1}'
    docs["whitespace-only"] = b"   \n\t "
    docs["empty-file"] = b""
    docs["bom-utf8"] = b'\xef\xbb\xbf{"a":1}'
    docs["bom-utf8-twice"] = b'\xef\xbb\xbf\xef\xbb\xbf{"a":1}'
    docs["bom-utf16le"] = '{"a":1}'.encode("utf-16")
    docs["trailing-garbage"] = b'{"a":1} x'
    docs["trailing-second-document"] = b'{"a":1}{}'
    docs["trailing-bracket"] = b"[1]]"
    docs["trailing-nul"] = b'{"a":1}\x00'
    docs["trailing-newlines"] = b'{"a":1}\n\n\n'
    docs["trailing-comma-array"] = b"[1,]"
    docs["trailing-comma-object"] = b'{"a":1,}'
    docs["leading-comma"] = b"[,1]"
    docs["missing-comma"] = b"[1 2]"
    docs["missing-colon"] = b'{"a" 1}'
    docs["missing-value"] = b'{"a":}'
    docs["key-only"] = b'{"a"}'
    docs["unquoted-key"] = b"{a:1}"
    docs["single-quotes"] = b"{'a':1}"
    docs["unclosed-object"] = b'{"a":1'
    docs["unclosed-array"] = b"[1,2"
    docs["unclosed-string"] = b'["abc'
    docs["mismatched-close"] = b'{"a":1]'
    docs["bare-word"] = b"tru"
    docs["bare-null-prefix"] = b"[nul]"
    docs["true-false-adjacent"] = b"[true false]"
    docs["comment-block"] = b"/* c */ {}"
    docs["comment-line"] = b"// c\n{}"
    docs["control-tab-in-string"] = b'["a\tb"]'
    docs["control-newline-in-string"] = b'["a\nb"]'
    docs["control-nul-in-string"] = b'["a\x00b"]'
    docs["escape-invalid-x"] = b'["\\x41"]'
    docs["escape-invalid-short-u"] = b'["\\u12"]'
    docs["escape-invalid-u-digit"] = b'["\\u12G4"]'
    docs["escape-invalid-letter"] = b'["\\q"]'
    docs["escape-uppercase-U"] = b'["\\U00e9"]'
    docs["utf8-invalid-ff"] = b'["a\xffb"]'
    docs["utf8-overlong"] = b'["\xc0\x80"]'
    docs["utf8-truncated"] = b'["\xe2\x82"]'
    docs["utf8-surrogate"] = b'["\xed\xa0\x80"]'
    docs["utf8-five-byte"] = b'["\xf8\x88\x80\x80\x80"]'
    docs["utf8-above-max"] = b'["\xf4\x90\x80\x80"]'
    docs["utf8-valid-4-byte"] = b'["\xf0\x9f\x98\x80"]'
    docs["utf8-invalid-key"] = b'{"\xff":1}'
    docs["utf8-latin1-outside-string"] = b'{"a":1}\xe9'
    docs["object-key-not-string"] = b"{1:2}"
    docs["object-key-null"] = b"{null:2}"
    docs["array-nested-values"] = b'[[1,[2,[3,[4]]]],{"a":{"b":{"c":[true,null]}}}]'
    docs["large-array"] = ("[" + ",".join(str(i * 7 % 1000 / 8) for i in range(500)) + "]").encode()
    docs["capture-json"] = (CORPUS / "inputlog" / "bundles" / "synthetic"
                            / "capture.json").read_bytes()
    docs["settings-like"] = (
        b'{"systemAudio":true,"microphone":false,"microphoneDevice":"default_input",'
        b'"selfViewPosition":{"x":1,"y":0.5},"webcam":{"enabled":false,"radius":16,'
        b'"shadow":{"enabled":true,"intensity":0.28,"blur":23},"offset":{"x":0.035,"y":0.035}}}')
    for name, data in docs.items():
        write(out / "docs" / f"{name}.json", data)

    literals = [
        "0", "-0", "0.0", "-0.0", "0e0", "0E0", "0e-0", "-0e0", "1", "-1", "1.0", "1.5", "1e5",
        "1E5", "1e+5", "1E+5", "1e-7", "1E-7", "1e05", "1.0e0", "10", "100", "1e2", "1e21",
        "1e22", "1e23", "123456789012345678", "9007199254740991", "9007199254740992",
        "9007199254740993", "9007199254740994", "9223372036854775806", "9223372036854775807",
        "9223372036854775808", "9223372036854775809", "-9223372036854775807",
        "-9223372036854775808", "-9223372036854775809", "18446744073709551615",
        "18446744073709551616", "99999999999999999999", "123456789012345678901234567890",
        "1e400", "-1e400", "1e-400", "-1e-400", "1e308", "1.7976931348623157e308",
        "1.7976931348623158e308", "1.7976931348623159e308", "2e308", "4.9e-324", "5e-324",
        "2.5e-324", "2.4703282292062328e-324", "2.4703282292062327e-324",
        "2.2250738585072014e-308", "2.2250738585072011e-308", "0.1", "0.2", "0.3",
        "0.30000000000000004", "0.1000000000000000055511151231257827",
        "3.141592653589793238462643383279", "1.00000000000000011102230246251565404236316680908203125",
        "1.000000000000000111022302462515654042363166809082031250000001", "2147483647",
        "2147483648", "-2147483648", "-2147483649", "4294967295", "4294967296", "1.5e3",
        "15e2", "0.0000001", "100000000000000000000000", "1e-0", "1e+0", "-1e-7", "8.25",
        "0.035", "0.28", "1.2", "1.4", "1.6", "1.8", "2.675", "1.005", "4.35", "1234.5678",
        "0.000123", "12.5", "1e-5", "1e-6", "1e15", "1e16", "1e17", "2.5", "-2.5", "0.5",
        "00", "01", "-01", "007", ".5", "-.5", "1.", "-1.", "+1", "+0", "--1", "-", "1e",
        "1e+", "1e-", "e5", "1.e5", "1.5.5", "1e5e5", "0x10", "0X10", "0b1", "NaN", "nan",
        "Infinity", "-Infinity", "inf", "1_000", "1,5", "١", "１", " 1", "1 ", "\t1", "1\t",
        "- 1", "1 2", "true", "false", "null", "TRUE", "Null", "1e400000", "1e-400000",
        "1" + "0" * 400, "0." + "0" * 400 + "1", "1" * 50 + "." + "1" * 50,
    ]
    write(out / "number-literals.txt", "".join(f"{x}\n" for x in literals))

    string_literals = [
        '""', '"a"', '"abc"', '"\\""', '"\\\\"', '"\\/"', '"\\b\\f\\n\\r\\t"', '"\\u0000"',
        '"\\u0001"', '"\\u001f"', '"\\u0020"', '"\\u007f"', '"\\u0080"', '"\\u009f"',
        '"\\u00a0"', '"\\u00e9"', '"\\u00E9"', '"\\u0100"', '"\\u07ff"', '"\\u0800"',
        '"\\u20ac"', '"\\ud7ff"', '"\\ue000"', '"\\ufeff"', '"\\ufffd"', '"\\ufffe"',
        '"\\uffff"', '"\\ud83d\\ude00"', '"\\uD83D\\uDE00"', '"\\ud800\\udc00"',
        '"\\udbff\\udfff"', '"\\ud800"', '"\\udbff"', '"\\udc00"', '"\\udfff"',
        '"\\udc00\\ud800"', '"\\ud800\\ud800"', '"\\ud800a"', '"a\\ud800"', '"\\ud800\\u0041"',
        '"\\ud83d"', '"\\ude00"', '"é"', '"€"', '"😀"', '"\x7f"', '"\x01"', '"\x1f"', '"a\tb"',
        '"\\x41"', '"\\u12"', '"\\u12G4"', '"\\q"', '"\\U00e9"', '"\\"', '"abc', 'abc"',
        "'abc'", '"\\u0041\\u0042"', '"\\\\u0041"', '"\\uDEAD"', '"\\u2028"', '"\\u2029"',
        '"\\u0000\\u0000"', '"   "', '"' + "\\u00e9" * 20 + '"',
    ]
    # Lines are UTF-8, except the raw-byte cases written below.
    data = "".join(f"{x}\n" for x in string_literals).encode("utf-8")
    data += b'"\xff"\n"\xc3"\n"\xc0\xaf"\n"\xed\xa0\x80"\n"\xed\xb0\x80"\n"\xf0\x9f\x98"\n'
    data += b'"\xf4\x8f\xbf\xbf"\n"\xf4\x90\x80\x80"\n"\xe9t\xe9"\n"\x80"\n'
    write(out / "string-literals.txt", data)


# ---------------------------------------------------------------------------------------------
# qcolor/: strings to parse

SVG_NAMES = """aliceblue antiquewhite aqua aquamarine azure beige bisque black blanchedalmond blue
blueviolet brown burlywood cadetblue chartreuse chocolate coral cornflowerblue cornsilk crimson
cyan darkblue darkcyan darkgoldenrod darkgray darkgreen darkgrey darkkhaki darkmagenta
darkolivegreen darkorange darkorchid darkred darksalmon darkseagreen darkslateblue
darkslategray darkslategrey darkturquoise darkviolet deeppink deepskyblue dimgray dimgrey
dodgerblue firebrick floralwhite forestgreen fuchsia gainsboro ghostwhite gold goldenrod gray
grey green greenyellow honeydew hotpink indianred indigo ivory khaki lavender lavenderblush
lawngreen lemonchiffon lightblue lightcoral lightcyan lightgoldenrodyellow lightgray
lightgreen lightgrey lightpink lightsalmon lightseagreen lightskyblue lightslategray
lightslategrey lightsteelblue lightyellow lime limegreen linen magenta maroon mediumaquamarine
mediumblue mediumorchid mediumpurple mediumseagreen mediumslateblue mediumspringgreen
mediumturquoise mediumvioletred midnightblue mintcream mistyrose moccasin navajowhite navy
oldlace olive olivedrab orange orangered orchid palegoldenrod palegreen paleturquoise
palevioletred papayawhip peru pink plum powderblue purple red rosybrown royalblue saddlebrown
salmon sandybrown seagreen seashell sienna silver skyblue slateblue slategray slategrey snow
springgreen steelblue tan teal thistle tomato transparent turquoise violet wheat white
whitesmoke yellow yellowgreen""".split()


def make_qcolor() -> None:
    out = CORPUS / "qcolor"
    reset(out)
    rng = random.Random(4242)
    inputs = list(SVG_NAMES)
    inputs += ["Red", "RED", "rEd", "AliceBlue", "ALICEBLUE", "Light Blue", "light blue",
               "light  blue", " red", "red ", " red ", "\tred", "red\n", "re d", "r e d",
               "lightblue2", "redd", "grey50", "rebeccapurple", "none", "Transparent",
               "TRANSPARENT", "color0", "foreground", "accent", "", " ", "#", "##fff"]
    hexes = ["#fff", "#FFF", "#abc", "#ABC", "#aBc", "#000", "#f00", "#0f0", "#00f", "#123",
             "#ffffff", "#FFFFFF", "#000000", "#1a1b26", "#7aa2f7", "#a9b1d6", "#bb9af7",
             "#ff8a00", "#e52e71", "#123456", "#abcdef", "#ABCDEF", "#AbCdEf", "#faf4ed",
             "#575279", "#907aa9", "#80ff0000", "#00ff0000", "#ffff0000", "#7f123456",
             "#0000ff00", "#ff7aa2f7", "#fffa0b0c0", "#fff000000", "#000fff000", "#123456789",
             "#abcabcabc", "#ABCDEF123", "#ffff00000000", "#0000ffff0000", "#00000000ffff",
             "#123456789abc", "#FFFFFFFFFFFF", "#8000ffff0000", "#ff", "#ffff", "#fffff",
             "#fffffff", "#1234567890", "#12345678901", "#1234567890abc", "#ggg", "#gggggg",
             "#12345g", "#12 456", " #123456", "#123456 ", "#123456\n", "123456", "fff",
             "0x123456", "#-12345", "#+12345", "#0x1234", "#１２３４５６", "#é12345",
             "rgb(1,2,3)", "rgba(1,2,3,0.5)", "rgb(255, 0, 0)", "rgba(122,162,247,0.25)",
             "hsl(0,100%,50%)", "rgba(7aa2f7ee)", "#7aa2f7 #bb9af7 45deg", "#7aa2f7ee"]
    inputs += hexes
    for _ in range(60):
        inputs.append("#" + "".join(rng.choice("0123456789abcdefABCDEF")
                                    for _ in range(rng.choice([3, 6, 8, 9, 12]))))
    write(out / "parse.json", json.dumps(inputs, ensure_ascii=True, indent=1) + "\n")


# ---------------------------------------------------------------------------------------------
# inputlog/: event logs and recording bundles


def make_inputlog() -> None:
    out = CORPUS / "inputlog"
    reset(out)
    fixtures = ROOT / "tests" / "fixtures"
    shutil.copy(fixtures / "input.jsonl", out / "fixture-input.jsonl")
    (out / "bundles" / "synthetic").mkdir(parents=True)
    for name in ("capture.json", "input.jsonl"):
        shutil.copy(fixtures / "synthetic" / name, out / "bundles" / "synthetic" / name)

    lines = [
        '{"t":1000000,"k":"m","x":1,"y":2}',
        '{"t":1000001,"k":"x","x":3,"y":4}',
        '{"t":1000002,"x":5,"y":6}',
        '{"t":1000003,"k":"","x":7.25,"y":-8.5}',
        '{"t":1000004,"k":"D","b":"left"}',
        '{"t":1000005,"k":"d","b":"middle","x":10,"y":20}',
        '{"t":1000006,"k":"u","b":"middle","x":10,"y":20}',
        '{"t":1000007,"k":"d","b":"right","x":11,"y":21}',
        '{"t":1000008,"k":"d","b":"LEFT","x":12,"y":22}',
        '{"t":1000009,"k":"d","x":13,"y":23}',
        '{"t":1000010,"k":"s","dx":0.5,"dy":-1,"x":14,"y":24}',
        '{"t":1000011,"k":"s","dx":"1","dy":true}',
        '{"t":1000012,"k":"kd","code":30,"name":"KEY_A","mods":["ctrl","shift"]}',
        '{"t":1000013,"k":"ku","code":30}',
        '{"t":1000014,"k":"kd","code":"31","name":5,"mods":"ctrl"}',
        '{"t":1000015,"k":"kd","code":4294967297,"name":"KEY_B","mods":[1,null,"alt",{"a":1}]}',
        '{"t":1000016,"k":"kd","code":2.5,"name":"KEY_C","mods":[]}',
        '{"t":"1000017","k":"m","x":"1","y":null}',
        '{"t":1000018.9,"k":"m","x":1e300,"y":-0}',
        '{"t":true,"k":"m","x":1,"y":1}',
        '{"t":1.5e6,"k":"m","x":2,"y":2}',
        '{"k":"m","x":3,"y":3}',
        '{"t":-5,"k":"m","x":4,"y":4}',
        '{"t":999999,"k":"m","x":5,"y":5}',
        '{"t":9223372036854775807,"k":"m","x":6,"y":6}',
        '{"t":"abc","k":"m","x":7,"y":7}',
        '{"t":1000019,"k":"m","x":8,"y":9,"extra":{"nested":[1,2]}}',
        '{"t":1000020,"k":"kd","code":57,"name":"KEY_SPACE","mods":["meta"]}',
        '{"t":1000021,"k":"m","x":1.0000000000000002,"y":0.30000000000000004}',
        '{"t":3000000,"k":"m","x":100,"y":100}',
        '{"t":3500000,"k":"d","b":"left","x":100,"y":100}',
    ]
    write(out / "kinds.jsonl", "".join(f"{x}\n" for x in lines))
    write(out / "blank-lines.jsonl",
          '\n   \n{"t":1000000,"k":"m","x":1,"y":2}\n\t\n\n{"t":1100000,"k":"d","b":"left","x":1,"y":2}\n  \n')
    write(out / "crlf.jsonl",
          '{"t":1000000,"k":"m","x":1,"y":2}\r\n{"t":1100000,"k":"d","b":"left","x":1,"y":2}\r\n')
    write(out / "carriage-return-only.jsonl",
          '{"t":1000000,"k":"m","x":1,"y":2}\r{"t":1100000,"k":"m","x":3,"y":4}\r')
    write(out / "no-trailing-newline.jsonl",
          '{"t":1000000,"k":"m","x":1,"y":2}\n{"t":1100000,"k":"m","x":3,"y":4}')
    write(out / "bom.jsonl", b'\xef\xbb\xbf{"t":1000000,"k":"m","x":1,"y":2}\n{"t":1100000,"k":"m","x":3,"y":4}\n')
    write(out / "bad-line-middle.jsonl",
          '{"t":1000000,"k":"m","x":1,"y":2}\n{"t":1100000,"k":"m","x":3,\n{"t":1200000,"k":"m","x":5,"y":6}\n')
    write(out / "bad-line-after-skip.jsonl",
          '{"t":10,"k":"m","x":1,"y":2}\nnot json\n')
    write(out / "array-line.jsonl", '{"t":1000000,"k":"m","x":1,"y":2}\n[1,2,3]\n')
    write(out / "scalar-line.jsonl", '{"t":1000000,"k":"m","x":1,"y":2}\n42\n')
    write(out / "trailing-garbage-line.jsonl", '{"t":1000000,"k":"m","x":1,"y":2} trailing\n')
    write(out / "two-objects-one-line.jsonl", '{"t":1000000,"k":"m","x":1,"y":2}{"t":2}\n')
    write(out / "illegal-number.jsonl", '{"t":1000000,"k":"m","x":1,"y":2}\n{"t":1100000,"k":"m","x":1e400,"y":2}\n')
    write(out / "empty.jsonl", "")
    write(out / "only-whitespace.jsonl", "  \n\n\t\n")
    write(out / "invalid-utf8.jsonl", b'{"t":1000000,"k":"m","b":"\xff","x":1,"y":2}\n')
    write(out / "unicode.jsonl",
          '{"t":1000000,"k":"kd","code":30,"name":"KEY_\\u00e9","mods":["ctrl","\\ud83d\\ude00"]}\n'
          '{"t":1000001,"k":"d","b":"é€","x":1,"y":1}\n')
    long_line = '{"t":1000000,"k":"m","x":1,"y":2,"pad":"' + "p" * 70000 + '"}\n'
    write(out / "long-line.jsonl", long_line + '{"t":1100000,"k":"m","x":3,"y":4}\n')
    write(out / "unsorted.jsonl",
          '{"t":3000000,"k":"m","x":3,"y":3}\n{"t":1000000,"k":"m","x":1,"y":1}\n'
          '{"t":2000000,"k":"d","b":"left","x":2,"y":2}\n{"t":1500000,"k":"d","b":"right","x":2,"y":2}\n')

    bundles = out / "bundles"
    synthetic_input = (fixtures / "synthetic" / "input.jsonl").read_text()
    small = ('{"t":1000000,"k":"m","x":1,"y":2}\n{"t":2000000,"k":"d","b":"left","x":1,"y":2}\n'
             '{"t":5000000,"k":"m","x":3,"y":4}\n{"t":9500000,"k":"m","x":5,"y":6}\n')

    def bundle(name, capture, input_text=small):
        directory = bundles / name
        directory.mkdir(parents=True)
        if capture is not None:
            write(directory / "capture.json", capture)
        if input_text is not None:
            write(directory / "input.jsonl", input_text)

    bundle("missing-capture", None)
    bundle("missing-input", '{"first_frame_us":1000000,"stopped_us":9000000}')
    bundle("capture-invalid-json", '{"first_frame_us":1000000,')
    bundle("capture-array", '[1000000,9000000]')
    bundle("capture-empty", '')
    bundle("capture-no-stopped", '{"first_frame_us":1000000}')
    bundle("capture-stopped-before-first", '{"first_frame_us":5000000,"stopped_us":2000000}')
    bundle("capture-stopped-equal-first", '{"first_frame_us":2000000,"stopped_us":2000000}')
    bundle("capture-strings", '{"first_frame_us":"1000000","stopped_us":"4000000"}')
    bundle("capture-doubles", '{"first_frame_us":1000000.75,"stopped_us":4000000.25}')
    bundle("capture-huge", '{"first_frame_us":1e30,"stopped_us":2e30}')
    bundle("capture-no-first", '{"stopped_us":4000000}')
    bundle("capture-bool", '{"first_frame_us":true,"stopped_us":4000000}')
    bundle("synthetic-short-stop", '{"first_frame_us":1000000,"stopped_us":3500000}',
           synthetic_input)
    bundle("bad-input-line", '{"first_frame_us":1000000,"stopped_us":9000000}',
           small + "oops\n")
    # git keeps no empty directories, so the directory named capture.json holds a placeholder.
    write(bundles / "capture-is-directory" / "capture.json" / ".keep", "")
    write(bundles / "capture-is-directory" / "input.jsonl", small)


# ---------------------------------------------------------------------------------------------
# cursorpath/ and zoomtimeline/: crafted inputs


def make_motion() -> None:
    out = CORPUS / "cursorpath"
    reset(out)
    rng = random.Random(77)
    walk = []
    t = 0.0
    x, y = 0.5, 0.5
    for _ in range(400):
        t += rng.choice([0.001, 0.004, 1 / 120, 1 / 60, 0.02, 0.05, 0.0])
        x = min(1.0, max(0.0, x + rng.uniform(-0.01, 0.01)))
        y = min(1.0, max(0.0, y + rng.uniform(-0.01, 0.01)))
        walk.append([t, x, y])
    pixels = []
    t = 0.0
    for i in range(300):
        t += rng.choice([1 / 240, 1 / 120, 1 / 60, 0.1])
        pixels.append([t, round(rng.uniform(0, 1920), 3), round(rng.uniform(0, 1080), 3)])
    cases = {
        "empty": [],
        "single": [[1.0, 0.25, 0.75]],
        "two": [[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]],
        "same-time": [[0.5, 0.1, 0.1], [0.5, 0.9, 0.9], [0.5, 0.2, 0.3], [1.0, 0.4, 0.4]],
        "close-in-time": [[0.0, 0.0, 0.0], [0.001, 10.0, 10.0], [0.002, 20.0, 20.0],
                          [0.02, 30.0, 30.0], [0.0201, 31.0, 31.0], [0.5, 31.5, 31.5]],
        "close-in-space": [[0.0, 100.0, 100.0], [0.1, 100.5, 100.0], [0.2, 100.9, 100.3],
                           [0.3, 101.0, 100.0], [0.4, 102.0, 102.0], [0.5, 102.0, 102.0]],
        "negative-times": [[-1.0, 0.1, 0.2], [-0.5, 0.3, 0.4], [0.0, 0.5, 0.6], [0.5, 0.7, 0.8]],
        "last-duplicate": [[0.0, 1.0, 1.0], [1.0, 50.0, 50.0], [1.0, 60.0, 60.0]],
        "random-walk-normalized": walk,
        "random-pixels": pixels,
    }
    queries = sorted(set([-1.0, -0.5, -1e-9, 0.0, 1e-9, 0.001, 0.0015, 0.25, 0.5, 0.5000001,
                          0.75, 0.999, 1.0, 1.0000001, 1.5, 2.0, 3.0, 100.0]
                         + [i / 64 for i in range(0, 129)]))
    write(out / "cases.json", json.dumps({
        "samples": cases,
        "decimate": [[fps, md] for fps in (0.0, 0.5, 1.0, 24.0, 30.0, 60.0, 120.0, 240.0, 1e9)
                     for md in (0.0, 1.0 / 1920.0, 0.001, 0.01, 1.0, 2.5, 50.0)],
        "positionAt": queries,
    }, indent=1) + "\n")

    out = CORPUS / "zoomtimeline"
    reset(out)
    clicks = {
        "none": {"clicks": [], "durations": [0.0, 5.0]},
        "single": {"clicks": [1.0], "durations": [-1.0, 0.0, 1.0, 1.5, 2.0, 2.01, 3.0, 3.8, 10.0]},
        "unsorted": {"clicks": [5.0, 1.0, 3.0, 9.0, 0.1], "durations": [10.0, 20.0]},
        "negative": {"clicks": [-1.0, -0.0, 0.0, 0.2, 0.3], "durations": [5.0]},
        "near-end": {"clicks": [8.0, 8.99, 9.0, 9.01], "durations": [10.0]},
        "merge-boundary": {"clicks": [1.0, 3.5 + 2.5 + 0.3, 3.5 + 2.5 + 0.3 + 0.0000001, 14.0],
                           "durations": [30.0]},
        "exact-gap": {"clicks": [1.0, 6.3, 6.30000001, 11.6], "durations": [30.0, 12.0]},
        "duplicates": {"clicks": [2.0, 2.0, 2.0, 2.0], "durations": [10.0]},
        "dense": {"clicks": [i * 0.37 for i in range(40)], "durations": [16.0, 100.0]},
        "short-recording": {"clicks": [0.1, 0.5, 0.9], "durations": [1.0, 1.1, 1.5, 2.0]},
        "end-clamped": {"clicks": [3.0], "durations": [4.0, 4.05, 4.2, 5.5, 6.0]},
    }
    segments = {
        "manual-targets": [
            {"id": "m1", "start": 0.5, "end": 2.0, "level": 2.0, "auto": False, "x": 0.1, "y": 0.9},
            {"id": "m2", "start": 2.5, "end": 4.0, "level": 4.0, "auto": False, "x": 1.5, "y": -0.5},
            {"id": "m3", "start": 4.5, "end": 6.0, "level": 0.5, "auto": False, "x": 0.25, "y": 0.75},
            {"id": "m4", "start": 6.5, "end": 7.5, "level": 1.0, "auto": False, "x": 0.5, "y": 0.5},
        ],
        "overlapping": [
            {"id": "a", "start": 0.0, "end": 5.0, "level": 2.0, "auto": True, "x": 0.5, "y": 0.5},
            {"id": "b", "start": 1.0, "end": 3.0, "level": 3.0, "auto": False, "x": 0.2, "y": 0.2},
            {"id": "c", "start": 1.0, "end": 2.0, "level": 1.5, "auto": False, "x": 0.8, "y": 0.8},
            {"id": "d", "start": 4.0, "end": 8.0, "level": 2.5, "auto": True, "x": 0.5, "y": 0.5},
        ],
        "auto-levels": [
            {"id": "l1", "start": 0.0, "end": 1.5, "level": 1.0, "auto": True, "x": 0.5, "y": 0.5},
            {"id": "l2", "start": 1.6, "end": 3.0, "level": 1.25, "auto": True, "x": 0.5, "y": 0.5},
            {"id": "l3", "start": 3.1, "end": 4.6, "level": 2.0, "auto": True, "x": 0.5, "y": 0.5},
            {"id": "l4", "start": 4.7, "end": 6.0, "level": 4.0, "auto": True, "x": 0.5, "y": 0.5},
            {"id": "l5", "start": 6.1, "end": 7.9, "level": 8.0, "auto": True, "x": 0.5, "y": 0.5},
        ],
        "zero-length": [
            {"id": "z", "start": 2.0, "end": 2.0, "level": 2.0, "auto": True, "x": 0.5, "y": 0.5},
            {"id": "inv", "start": 4.0, "end": 3.0, "level": 2.0, "auto": True, "x": 0.5, "y": 0.5},
        ],
        "outside-samples": [
            {"id": "before", "start": -3.0, "end": -1.0, "level": 2.0, "auto": True, "x": 0.5, "y": 0.5},
            {"id": "after", "start": 50.0, "end": 52.0, "level": 2.0, "auto": True, "x": 0.5, "y": 0.5},
            {"id": "gap", "start": 2.0001, "end": 2.0002, "level": 2.0, "auto": True, "x": 0.5, "y": 0.5},
        ],
    }
    write(out / "cases.json", json.dumps({
        "generate": clicks,
        "segments": segments,
        "snapRatios": [0.0, 0.1, 0.25, 0.4, 0.5, 0.75, 1.0, -0.5, 2.0],
        "clampCenter": {
            "points": [[0.5, 0.5], [0.0, 0.0], [1.0, 1.0], [-1.0, 2.0], [0.25, 0.75],
                       [0.1, 0.9], [0.24999, 0.75001], [0.5, -0.0]],
            "levels": [-1.0, 0.0, 0.5, 1.0, 1.0000001, 1.5, 2.0, 3.0, 4.0, 10.0, 1e9],
        },
    }, indent=1) + "\n")


# ---------------------------------------------------------------------------------------------
# project/: crafted project.json files (the history-*.json files come from git history)


def make_project() -> None:
    out = CORPUS / "project"
    for path in out.glob("crafted-*"):
        path.unlink()
    out.mkdir(parents=True, exist_ok=True)
    history = sorted(out.glob("history-*.json"))
    if not history:
        sys.exit("run make-historical-projects.sh first")
    current = json.loads(history[-1].read_text())
    v1 = json.loads(history[1].read_text())        # first typed Project (inset, old shadow)
    phase1 = json.loads(history[0].read_text())    # JSON-backed Project (opacity/offsetY)
    v2camera = json.loads(history[9].read_text())  # defaultsVersion 2 with camera crop

    def crafted(name, value):
        data = value if isinstance(value, (bytes, str)) else json.dumps(value, indent=4) + "\n"
        write(out / f"crafted-{name}.json", data)

    def edit(base, **changes):
        value = json.loads(json.dumps(base))
        for path, replacement in changes.items():
            node = value
            keys = path.split("__")
            for key in keys[:-1]:
                node = node.setdefault(key, {})
            if replacement is DELETE:
                node.pop(keys[-1], None)
            else:
                node[keys[-1]] = replacement
        return value

    crafted("empty-file", b"")
    crafted("whitespace-only", b"  \n")
    crafted("invalid-json", b'{"version": 1, "name": "Broken"')
    crafted("trailing-garbage", b'{"version": 1}\nxyz')
    crafted("top-level-array", b'[{"version": 1}]')
    crafted("top-level-string", b'"project"')
    crafted("empty-object", b"{}")
    crafted("bom", b'\xef\xbb\xbf{"name": "With BOM", "defaultsVersion": 4}')
    crafted("compact-current", json.dumps(current, separators=(",", ":")))
    crafted("current-reordered", json.dumps(dict(reversed(list(current.items()))), indent=2))
    crafted("v1-untouched-defaults", v1)
    crafted("v1-customised-shadow", edit(v1, frame__shadow__intensity=0.61))
    crafted("v1-shadow-disabled", edit(v1, frame__shadow__enabled=False))
    crafted("v1-shadow-no-angle", edit(v1, frame__shadow__angle=DELETE))
    crafted("v1-shadow-angle-45", edit(v1, frame__shadow__angle=45))
    crafted("v1-shadow-nearly-equal", edit(v1, frame__shadow__intensity=0.75000000000001,
                                           frame__shadow__blur=20.0000000001))
    crafted("v1-inset-enabled", edit(v1, frame__inset__enabled=True))
    crafted("v1-inset-width", edit(v1, frame__inset__width=3))
    crafted("v1-inset-colour", edit(v1, frame__inset__color="#123456"))
    crafted("v1-inset-colour-invalid", edit(v1, frame__inset__color="nonsense"))
    crafted("v1-inset-alpha", edit(v1, frame__inset__alpha=0.6))
    crafted("v1-inset-and-border", edit(v1, frame__border={"enabled": True, "width": 2,
                                                            "color": "#ff0000", "alpha": 1}))
    crafted("v1-empty-shadow", edit(v1, frame__shadow={}))
    crafted("v1-empty-inset", edit(v1, frame__inset={}))
    crafted("v1-no-frame", edit(v1, frame=DELETE))
    crafted("v1-frame-not-object", edit(v1, frame=[1, 2]))
    crafted("phase1-opacity-offsety", phase1)
    crafted("phase1-old-defaults", edit(phase1, frame__shadow={
        "enabled": True, "opacity": 0.75, "blur": 20, "offsetY": 25}))
    crafted("phase1-opacity-and-intensity", edit(phase1, frame__shadow={
        "enabled": True, "opacity": 0.75, "intensity": 0.5, "blur": 20, "offsetY": 25,
        "distance": 30}))
    crafted("v2-camera-rounded-square", edit(v2camera, camera__shape="rounded",
                                             camera__crop="square", camera__radius=12))
    crafted("v2-camera-rounded-original", edit(v2camera, camera__shape="rounded",
                                               camera__crop="original"))
    crafted("v2-camera-square-square", edit(v2camera, camera__shape="square", camera__crop="square"))
    crafted("v2-camera-square-original", edit(v2camera, camera__shape="square",
                                              camera__crop="original"))
    crafted("v2-camera-square-no-crop", edit(v2camera, camera__shape="square", camera__crop=DELETE))
    crafted("v2-camera-round", edit(v2camera, camera__shape="round", camera__crop="square"))
    crafted("v2-camera-unknown-shape", edit(v2camera, camera__shape="hexagon"))
    crafted("v2-camera-shape-number", edit(v2camera, camera__shape=3))
    crafted("v2-camera-not-object", edit(v2camera, camera="enabled"))
    crafted("v2-no-camera", edit(v2camera, camera=DELETE))
    crafted("v0-camera-crop", {"name": "No version", "camera": {"shape": "rounded", "crop": "square",
                                                                "radius": 9}})
    v3 = edit(current, defaultsVersion=3, camera__shadow={
        "enabled": False, "intensity": 0.55, "blur": 18, "distance": 18},
        camera__border={"enabled": False, "width": 2, "color": "#ffffff", "alpha": 0.7},
        camera__offset={"x": 0.02, "y": 0.02})
    crafted("v3-untouched-camera", v3)
    crafted("v3-customised-shadow", edit(v3, camera__shadow__intensity=0.4))
    crafted("v3-customised-border", edit(v3, camera__border__width=4))
    crafted("v3-customised-offset", edit(v3, camera__offset__x=0.05))
    crafted("v3-camera-shape-square", edit(v3, camera__shape="square"))
    crafted("v3-shadow-bool", edit(v3, camera__shadow=True))
    crafted("v3-no-camera-shadow", edit(v3, camera__shadow=DELETE, camera__border=DELETE,
                                        camera__offset=DELETE))
    crafted("v4-keeps-width-2", edit(current, camera__border__width=2,
                                     camera__offset={"x": 0.02, "y": 0.02}))
    crafted("v5-future", edit(current, defaultsVersion=5, version=2))
    crafted("defaults-version-string", edit(current, defaultsVersion="4"))
    crafted("defaults-version-double", edit(current, defaultsVersion=3.5))
    crafted("defaults-version-negative", edit(v1, defaultsVersion=-1))
    crafted("aspect-null", edit(current, aspect=None))
    crafted("aspect-16-9", edit(current, aspect="16:9"))
    crafted("aspect-invalid", edit(current, aspect="21:9"))
    crafted("aspect-number", edit(current, aspect=1.5))
    crafted("aspect-missing", edit(current, aspect=DELETE))
    crafted("aspect-auto-string", edit(current, aspect="auto"))
    edited = edit(current,
                  name="Edited é€\U0001F600 \"quoted\"\n",
                  crop={"x": 0.1, "y": 0.2, "w": 0.5, "h": 0.6},
                  clips=[
                      {"id": "c1", "in": 0, "out": 2.5, "speed": 1},
                      {"id": "c1-s1", "in": 2.5, "out": 5.125, "speed": 1.6, "layout": "camera"},
                      {"id": "c2", "in": 5.125, "out": 8.25, "speed": 0.75, "layout": "screen"},
                      {"id": "m1", "in": 0.5, "out": 3, "speed": 1, "media": "media/demo.mp4",
                       "fit": "framed"},
                      {"id": "m2", "in": 0, "out": 1, "speed": 2, "media": "media/./b.mp4",
                       "fit": "stretch"},
                      {"id": "m3", "in": 0, "out": 1, "media": "../outside.mp4"},
                      {"id": "m4", "in": 0, "out": 1, "media": "media/../screen.mp4"},
                      {"id": "m5", "in": 0, "out": 1, "media": "/abs/media/x.mp4"},
                      {"id": "m6", "in": 0, "out": 1, "media": ""},
                      {"id": "m7", "in": 0, "out": 1, "media": 5},
                      {"id": "m8", "in": 0, "out": 1, "media": "media\\x.mp4"},
                      {"id": "c3", "in": 8.25, "out": 9, "layout": "sideways"},
                      {"id": 7, "in": "1", "out": None, "speed": "fast"},
                      "not-an-object",
                      [1, 2],
                  ],
                  zooms=[
                      {"id": "z1", "start": 1.2, "end": 3.7, "level": 2, "target": "auto"},
                      {"id": "z2", "start": 4, "end": 6.5, "level": 3.5,
                       "target": {"x": 0.25, "y": 0.75}},
                      {"id": "z3", "start": 7, "end": 7.5, "target": {"x": "a"}},
                      {"id": "z4", "start": 8, "end": 9, "level": "2", "target": None},
                      {"start": 9.5},
                  ],
                  zoomStyle={"spring": {"mass": 1, "stiffness": 150}, "snapToEdgesRatio": 0.4,
                             "instantAnimation": True, "motionBlur": 1.5},
                  background={"type": "gradient", "wallpaper": "/some/wall.png",
                              "gradient": {"angle": 90, "stops": [["#112233", 0], ["#80ff0000", 0.5],
                                                                  ["bogus", 1], ["#fff"], "x",
                                                                  ["#abc", "0.7"]]},
                              "color": "#fff000fff", "image": "media/bg.png", "blur": 12.5},
                  cursor={"visible": False, "size": 2, "smoothing": 0.0,
                          "spring": {"damping": 10}, "clickEffect": "none", "clickShrink": 0.5,
                          "rotateOnXMovementRatio": 0, "hideWhenIdleMs": 1500, "style": "dot",
                          "ringColor": "#80ff8800", "clickSound": "soft"},
                  keystrokes={"enabled": True, "position": "top-left", "size": 3,
                              "showOnlyShortcuts": False, "holdMs": 20},
                  audio={"desktop": False, "mic": False, "volume": 0.35},
                  export={"format": "gif", "fps": 30, "height": 720, "quality": "web",
                          "gif": {"fps": 10, "height": 360, "quality": "small", "loop": False}},
                  unknownTopLevel={"keep": "me"},
                  )
    edited["camera"].update({"enabled": True, "position": "top-left", "size": 0.4,
                             "shape": "rectangle", "radius": 4, "flipHorizontal": True,
                             "rotation": 90, "scaleDuringZoom": 1, "fullFrame": "square",
                             "unknownCameraKey": 1})
    crafted("edited", edited)
    crafted("wrong-types", {
        "version": "1", "defaultsVersion": 4, "name": 42, "aspect": ["16:9"], "crop": [0, 0, 1, 1],
        "clips": {"id": "c1"}, "zooms": "none", "zoomStyle": [], "background": "black",
        "frame": {"padding": "0.2", "radius": None, "shadow": [], "border": "on"},
        "cursor": {"visible": "yes", "size": "big", "smoothing": "true", "spring": 5,
                   "hideWhenIdleMs": "500", "style": 1, "ringColor": 255, "clickSound": True},
        "keystrokes": {"enabled": 1, "size": "2", "holdMs": 2.7},
        "audio": {"desktop": 0, "mic": "false", "volume": "loud"},
        "camera": {"enabled": "true", "rotation": "90", "radius": "8", "mirror": 1,
                   "offset": [0.1, 0.1], "fullFrame": 1},
        "export": {"fps": 29.97, "height": "720", "gif": {"fps": 12.5, "loop": "no"}},
    })
    crafted("clamps", edit(current, keystrokes__size=0.1, keystrokes__holdMs=999999,
                           zoomStyle__motionBlur=-2, cursor__hideWhenIdleMs=None))
    crafted("clamps-high", edit(current, keystrokes__size=99, keystrokes__holdMs=-5,
                                zoomStyle__motionBlur=0.5, cursor__hideWhenIdleMs=-20))
    crafted("cursor-styles", edit(current, cursor__style="hand", cursor__clickSound="loud",
                                  cursor__smoothing=0.5))
    crafted("cursor-macos-style", edit(current, cursor__style="macos", cursor__smoothing=True))
    crafted("camera-mirror-legacy", edit(current, camera__mirror=True,
                                         camera__flipHorizontal=DELETE))
    crafted("camera-mirror-and-flip", edit(current, camera__mirror=True,
                                           camera__flipHorizontal=False))
    crafted("camera-rotations", edit(current, camera__rotation=45))
    crafted("camera-rotation-270", edit(current, camera__rotation=270))
    crafted("camera-rotation-double", edit(current, camera__rotation=180.0))
    crafted("camera-inset-legacy", edit(current, camera__inset={"enabled": True, "width": 3,
                                                                "color": "#abcdef", "alpha": 0.8},
                                        camera__border=DELETE))
    crafted("gradient-no-stops-key", edit(current, background__gradient={"angle": 10}))
    crafted("gradient-empty-stops", edit(current, background__gradient={"angle": 10, "stops": []}))
    crafted("colours-alpha-and-12-bit", edit(current, background__color="#7f112233",
                                             frame__border__color="#123456789abc",
                                             cursor__ringColor="red",
                                             camera__border__color="transparent"))
    crafted("unknown-keys", edit(current, extra=[1, {"a": None}], frame__extra=True,
                                 camera__extra="x"))
    crafted("missing-sections", {"version": 1, "defaultsVersion": 4, "name": "Bare"})
    crafted("only-name", {"name": "Only a name"})
    crafted("name-null", edit(current, name=None))
    crafted("numbers-precision", edit(current, frame__padding=0.1 + 0.2, frame__radius=1e-7,
                                      audio__volume=1.0000000000000002, crop__w=2 ** 53 + 1,
                                      clips=[{"id": "c1", "in": 0, "out": 123456789.123456789,
                                              "speed": 1}]))
    crafted("big-integers", b'{"defaultsVersion": 4, "version": 9223372036854775807, '
                            b'"export": {"fps": 1e3, "height": 4294967296, '
                            b'"gif": {"fps": -2147483649, "height": 2147483647}}, '
                            b'"keystrokes": {"holdMs": 1e400}}')
    crafted("duplicate-keys", b'{"defaultsVersion": 1, "name": "First", "name": "Second", '
                              b'"frame": {"padding": 0.5}, "frame": {"radius": 3}}')
    crafted("v1-unicode-name", edit(v1, name="Рекорд 한"))


DELETE = object()


# ---------------------------------------------------------------------------------------------
# themes-crafted/: hand-written theme states


def make_themes() -> None:
    out = CORPUS / "themes-crafted"
    reset(out)

    def theme(name, colors=None, shell=None, theme_name=None):
        directory = out / name
        directory.mkdir(parents=True)
        if colors is not None:
            write(directory / "colors.toml", colors)
        if shell is not None:
            write(directory / "shell.toml", shell)
        if theme_name is not None:
            write(directory / "theme.name", theme_name)

    theme("missing-files")
    write(out / "missing-files" / ".keep", "")  # git keeps no empty directories
    theme("empty-files", "", "", "")
    theme("light-inferred", 'background = "#faf4ed"\nforeground = "#575279"\naccent = "#907aa9"\n')
    theme("light-explicit-mode-light",
          'mode = "light"\nbackground = "#1a1b26"\nforeground = "#a9b1d6"\naccent = "#7aa2f7"\n')
    theme("dark-mode-on-light-background",
          'mode = "dark"\nbackground = "#faf4ed"\nforeground = "#575279"\naccent = "#907aa9"\n',
          theme_name="Rosé Dawn \n")
    theme("light-with-bright-foreground",
          'background = "#ffffff"\nforeground = "#222222"\naccent = "#0055ff"\n'
          'bright_foreground = "#000000"\ndark_background = "#eeeeee"\n')
    theme("alpha-colours",
          'accent = "#807aa2f7"\nbackground = "#cc1a1b26"\nforeground = "#00a9b1d6"\n'
          'color4 = "#ffbb9af7"\nlighter_background = "#8024283b"\ndark_background = "#13141c"\n'
          'red = "#40f7768e"\n')
    theme("twelve-bit-colours",
          'accent = "#7aa7a2a2f7f7"\nbackground = "#1a11b126"\nforeground = "#abc"\n')
    theme("named-colours",
          'accent = "cornflowerblue"\nbackground = "Black"\nforeground = "light gray"\n'
          'color4 = "rebeccapurple"\n')
    theme("invalid-colours",
          'accent = "nonsense"\nbackground = "#12345"\nforeground = ""\nlighter_background = "x"\n'
          'dark_background = "#zzzzzz"\ncolor4 = "#1234567"\nbright_foreground = "nope"\n',
          'mode = "dark"\n')
    theme("toml-syntax",
          "# comment line\n"
          "accent='#ff0000'   # single quotes and a comment\n"
          '  background   =   "#101010"  \n'
          'foreground = #eeeeee\n'
          'color4 = 00ff00 # unquoted\n'
          'red = "#00ff00" "#0000ff"\n'
          'mode = dark\n'
          'bright_foreground = "#ffffff\n'
          'lighter_background = "#202020"#trailing\n'
          'dark_background = "#0a0a0a" ; not a comment\n'
          'dotted.key = "#123456"\n'
          'accent = "#00aaff"\n'
          '[section]\n'
          'accent = "#ff00ff"\n',
          "[bar]\nactive = urgent\n")
    theme("toml-crlf-bom",
          b'\xef\xbb\xbfaccent = "#ff0000"\r\nbackground = "#202020"\r\nforeground = "#dddddd"\r\n'
          b'mode = "dark"\r\n',
          b'[font]\r\nbase-size = 14\r\n[controls]\r\nnormal-fill-alpha = 0.5\r\n')
    theme("toml-sections",
          'accent = "#7aa2f7"\nbackground = "#1a1b26"\nforeground = "#a9b1d6"\n',
          "[ font ]\nbase-size = 20\n[font]\nbody = 13\n[font] # trailing comment\n"
          "caption = 9\n  [spacing]  \nscale = 2\n[spacing.extra]\nxs = 100\n"
          "[controls\nnormal-fill-alpha = 0.9\n[]\nxxs = 50\n")
    theme("shell-fonts",
          'accent = "#7aa2f7"\nbackground = "#1a1b26"\nforeground = "#a9b1d6"\n',
          "[font]\nbase-size = 15\ncaption = 10\nbody-small = 11.5\nbody = 0\nsubtitle = -4\n"
          "title = 18\nheading = abc\ndisplay = 30\n")
    theme("shell-fonts-fractional-base",
          'accent = "#7aa2f7"\nbackground = "#1a1b26"\nforeground = "#a9b1d6"\n',
          "[font]\nbase-size = 13.5\n[spacing]\nscale = 1.25\n")
    theme("shell-fonts-tiny-base",
          'accent = "#7aa2f7"\nbackground = "#1a1b26"\nforeground = "#a9b1d6"\n',
          "[font]\nbase-size = 0.2\n")
    theme("shell-fonts-huge-base",
          'accent = "#7aa2f7"\nbackground = "#1a1b26"\nforeground = "#a9b1d6"\n',
          "[font]\nbase-size = 40\n[spacing]\nscale-with-font = false\nscale = 0.5\n")
    theme("shell-spacing-overrides",
          'accent = "#7aa2f7"\nbackground = "#1a1b26"\nforeground = "#a9b1d6"\n',
          "[font]\nbase-size = 16\n[spacing]\nscale = 1.5\nscale-with-font = true\nxxs = 1.5\n"
          "xs = 2.5\nsm = -3\nmd = abc\ncontrol-gap = 7.49\ncontrol-height = 32\n"
          "popup-row-height = 1e2\nrow-gap = 0x10\npanel-padding = inf\nhuge = nan\n")
    theme("shell-spacing-scale-with-font-off",
          'accent = "#7aa2f7"\nbackground = "#1a1b26"\nforeground = "#a9b1d6"\n',
          "[font]\nbase-size = 18\n[spacing]\nscale-with-font = False\n")
    theme("shell-controls",
          'accent = "#7aa2f7"\nbackground = "#1a1b26"\nforeground = "#a9b1d6"\nred = "#f7768e"\n',
          "[controls]\nnormal-color = accent\nnormal-fill-alpha = 0.5\nnormal-border = urgent\n"
          "normal-border-width = 2.5\nnormal-border-alpha = 2\nhover-cursor-color = Foreground\n"
          "hover-cursor-fill-alpha = -1\nhover-cursor-border = background\n"
          "hover-cursor-border-width = -3\nhover-cursor-border-alpha = nan\n"
          "selected-color = TEXT\nselected-fill-alpha = 0.333\nselected-border = #00ff00\n"
          'selected-border-width = 3.5\nselected-border-alpha = "0.5"\n'
          'pressed-color = "hyprland.active-border"\npressed-fill-alpha = inf\n'
          'selection-color = "hyprland.active-border-foreground"\nselection-fill-alpha = 1e-3\n'
          "[bar]\nactive = accent\n")
    theme("shell-surfaces",
          'accent = "#7aa2f7"\nbackground = "#1a1b26"\nforeground = "#a9b1d6"\n',
          "[popups]\nbackground = #000000\nbackground-alpha = 0.5\ntext = \"#ffffff\"\n"
          'border = "rgba(7aa2f7ee) rgba(c4a7e7ee) 45deg"\nborder-alpha = 0.75\n'
          "[tooltip]\nbackground = \"#7aa2f7 #bb9af7 45deg\"\nbackground-alpha = 0.9\n"
          "text = foreground\nborder = urgent\n"
          "[menu]\nbackground = \"#112233\"\nbackground-alpha = 1.5\ntext = accent\n"
          "border = \"\"\nborder-alpha = 0\nscrim = black\nscrim-alpha = 0.25\n"
          'selected-background = "#ff0000"\nselected-background-alpha = 0.12\n'
          'selected-text = "#00ff00"\n'
          "[bar]\nactive = \"  Accent  \"\n")
    theme("attention-from-red",
          'accent = "#7aa2f7"\nbackground = "#1a1b26"\nforeground = "#a9b1d6"\nred = "#ff0000"\n')
    theme("attention-red-invalid",
          'accent = "#7aa2f7"\nbackground = "#1a1b26"\nforeground = "#a9b1d6"\nred = "bogus"\n')
    theme("attention-bar-urgent",
          'accent = "#7aa2f7"\nbackground = "#1a1b26"\nforeground = "#a9b1d6"\nred = "#ff0000"\n',
          "[bar]\nactive = urgent\n")
    theme("accent-foreground-threshold-dark",
          'accent = "#757575"\nbackground = "#000000"\nforeground = "#ffffff"\n')
    theme("accent-foreground-threshold-light",
          'accent = "#767676"\nbackground = "#000000"\nforeground = "#ffffff"\n')
    theme("luminance-threshold-just-dark",
          'background = "#a0a0a0"\nforeground = "#000000"\naccent = "#ff0000"\n')
    theme("luminance-threshold-just-light",
          'background = "#a1a1a1"\nforeground = "#000000"\naccent = "#ff0000"\n')
    theme("mode-uppercase-dark",
          'mode = "Dark"\nbackground = "#ffffff"\nforeground = "#000000"\naccent = "#ff0000"\n')
    theme("theme-name-only", None, None, "  spaced name\t\n\n")
    theme("theme-name-unicode", 'accent = "#7aa2f7"\n', None, "thème-ü\U0001F600\n")


# ---------------------------------------------------------------------------------------------
# cliptimeline/, layouttimeline/: clip lists


def clip(id_: str, in_: float, out: float, speed: float = 1.0, layout: str = None,
         media: str = None, fit: str = None) -> dict:
    """A Clip as the oracle builds it: missing keys keep Clip's defaults."""
    value = {"id": id_, "in": in_, "out": out, "speed": speed}
    if layout is not None:
        value["layout"] = layout
    if media is not None:
        value["media"] = media
    if fit is not None:
        value["fit"] = fit
    return value


def speed_run(prefix: str, speeds: list, span: float) -> list:
    return [clip(f"{prefix}{i + 1}", i * span, (i + 1) * span, speed) for i, speed in enumerate(speeds)]


def make_timelines() -> None:
    out = CORPUS / "cliptimeline"
    reset(out)
    timelines = {
        "empty": [],
        "single": [clip("c1", 0.0, 8.0)],
        "single-zero-length": [clip("c1", 2.0, 2.0)],
        "speeds-slow": speed_run("s", [0.5, 0.75, 1.0, 1.2, 1.4], 1.0),
        "speeds-fast": speed_run("f", [1.6, 1.8, 2.0, 3.0, 4.0], 1.5),
        "speeds-fastest": speed_run("x", [8.0, 16.0, 24.0], 4.0),
        "cuts": [clip("c1", 0.0, 2.0), clip("c2", 3.0, 5.0), clip("c3", 6.5, 8.0)],
        "unsorted": [clip("c1", 5.0, 7.0), clip("c2", 1.0, 3.0), clip("c3", 3.0, 4.5, 2.0)],
        "overlapping": [clip("c1", 0.0, 4.0), clip("c2", 2.0, 6.0, 2.0), clip("c3", 3.5, 5.0, 0.5)],
        "zero-length-clips": [clip("c1", 0.0, 2.0), clip("c2", 2.0, 2.0), clip("c3", 2.0, 4.0, 1.2),
                              clip("c4", 4.0, 4.0, 3.0)],
        "inserted-media": [clip("c1", 0.0, 3.0), clip("m1", 0.0, 2.0, media="media/a.mp4", fit="fill"),
                           clip("c2", 3.0, 6.0, 1.5), clip("m2", 1.0, 4.0, 2.0, media="media/b.mp4",
                                                            fit="framed"),
                           clip("c3", 6.0, 8.0, layout="camera")],
        "media-first": [clip("m1", 0.0, 1.5, media="media/a.mp4"), clip("c1", 1.0, 5.0)],
        "media-last": [clip("c1", 0.0, 4.0), clip("m1", 0.5, 3.0, media="media/a.mp4")],
        "media-only": [clip("m1", 0.0, 2.0, media="media/a.mp4"),
                       clip("m2", 0.5, 1.5, 0.5, media="media/b.mp4")],
        "media-adjacent": [clip("c1", 0.0, 2.0), clip("m1", 0.0, 1.0, media="media/a.mp4"),
                           clip("m2", 0.0, 1.0, media="media/b.mp4"), clip("c2", 2.0, 4.0)],
        "inverted": [clip("c1", 0.0, 2.0), clip("c2", 4.0, 3.0), clip("c3", 5.0, 6.0)],
        "degenerate-speeds": [clip("c1", 0.0, 1.0, 0.0), clip("c2", 1.0, 2.0, -1.0),
                              clip("c3", 2.0, 3.0)],
        "tiny": [clip("c1", 0.0, 1e-10), clip("c2", 1.0, 1.05), clip("c3", 2.0, 2.3, 3.0)],
    }
    write(out / "cases.json", json.dumps({
        "timelines": timelines,
        "splitSequences": {
            "single": [2.0, 4.0, 1.0, 1.0, 6.5, 0.0, 8.0, 3.999999999, 3.9999999995],
            "inserted-media": [1.0, 4.0, 4.5, 6.0, 1.0],
            "speeds-fast": [0.5, 0.5, 2.0, 3.25],
        },
    }, indent=1) + "\n")

    out = CORPUS / "layouttimeline"
    reset(out)
    layouts = {
        "empty": [],
        "single-camera": [clip("c1", 0.0, 3.0, layout="camera")],
        "single-default": [clip("c1", 0.0, 3.0)],
        "layouts": [clip("c1", 0.0, 2.0), clip("c2", 2.0, 4.0, layout="camera"),
                    clip("c3", 4.0, 6.0, layout="screen"), clip("c4", 6.0, 8.0, layout="default"),
                    clip("c5", 8.0, 9.0, layout="camera")],
        "short-clips": [clip("c1", 0.0, 1.0, layout="camera"), clip("c2", 1.0, 1.2),
                        clip("c3", 1.2, 1.25, layout="screen"), clip("c4", 1.25, 3.0, layout="camera"),
                        clip("c5", 3.0, 3.5, layout="screen")],
        "speeds": [clip("c1", 0.0, 2.0), clip("c2", 2.0, 4.0, 4.0, layout="camera"),
                   clip("c3", 4.0, 6.0, 0.5, layout="screen"), clip("c4", 6.0, 7.0, 24.0, layout="camera"),
                   clip("c5", 7.0, 8.0, 1.4)],
        "media-between": [clip("c1", 0.0, 2.0, layout="camera"),
                          clip("m1", 0.0, 1.0, media="media/a.mp4", layout="camera"),
                          clip("c2", 2.0, 4.0), clip("m2", 0.0, 1.0, media="media/a.mp4"),
                          clip("m3", 0.0, 0.5, media="media/b.mp4", fit="fill"),
                          clip("c3", 4.0, 6.0, layout="screen"), clip("c4", 6.0, 7.0, layout="camera")],
        "media-first": [clip("m1", 0.0, 1.0, media="media/a.mp4"), clip("c1", 0.0, 2.0, layout="camera")],
        "zero-length": [clip("c1", 0.0, 2.0, layout="camera"), clip("c2", 2.0, 2.0, layout="screen"),
                        clip("c3", 2.0, 4.0), clip("c4", 4.0, 4.0, layout="camera")],
        "unknown-layouts": [clip("c1", 0.0, 1.0, layout="camera"), clip("c2", 1.0, 2.0, layout="bogus"),
                            clip("c3", 2.0, 3.0, layout=""), clip("c4", 3.0, 4.0, layout="Camera"),
                            clip("c5", 4.0, 5.0, layout="screen ")],
        "inverted": [clip("c1", 0.0, 2.0, layout="camera"), clip("c2", 3.0, 2.0, layout="screen"),
                     clip("c3", 3.0, 5.0)],
    }
    write(out / "cases.json", json.dumps({"timelines": layouts}, indent=1) + "\n")


# ---------------------------------------------------------------------------------------------
# keystroketrack/: key logs and Keystrokes settings


def key_line(t_us: int, name, mods=None, kind: str = "kd", code: int = 30) -> str:
    value = {"t": t_us, "k": kind, "code": code}
    if name is not None:
        value["name"] = name
    if mods is not None:
        value["mods"] = mods
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"))


def key_log(path: Path, entries: list) -> None:
    """entries: (seconds after the first frame, name, mods[, kind]); first frame is 1000000 us."""
    lines = []
    for entry in entries:
        seconds, name, mods = entry[0], entry[1], entry[2]
        kind = entry[3] if len(entry) > 3 else "kd"
        lines.append(key_line(1000000 + round(seconds * 1e6), name, mods, kind))
    write(path, "".join(f"{line}\n" for line in lines))


def make_keystrokes() -> None:
    out = CORPUS / "keystroketrack"
    reset(out)
    labels = ["KEY_ESC", "KEY_ENTER", "KEY_SPACE", "KEY_TAB", "KEY_BACKSPACE", "KEY_DELETE", "KEY_UP",
              "KEY_DOWN", "KEY_LEFT", "KEY_RIGHT", "KEY_PAGEUP", "KEY_PAGEDOWN", "KEY_LEFTBRACE",
              "KEY_RIGHTBRACE", "KEY_MINUS", "KEY_EQUAL", "KEY_COMMA", "KEY_DOT", "KEY_SLASH",
              "KEY_SEMICOLON", "KEY_APOSTROPHE", "ESC", "ENTER", "Esc", "KEY_KP1", "KEY_KP0", "KP7",
              "KEY_KPPLUS", "KEY_KPENTER", "KEY_KP", "KEY_KPA", "KEY_F1", "KEY_F12", "KEY_F24",
              "KEY_CAPSLOCK", "KEY_PRINT_SCREEN", "KEY_SYSRQ", "KEY_HOME", "KEY_END", "KEY_INSERT",
              "KEY_GRAVE", "KEY_BACKSLASH", "KEY_102ND", "KEY_VOLUMEUP", "KEY_BRIGHTNESS_CYCLE",
              "KEY_MULTI__UNDERSCORE", "KEY__LEADING", "KEY_TRAILING_", "KEY_ spaced  words ",
              "KEY_", "KEY__", "KEY_KEY_A", "key_a", "Return", "PAGE UP", "kEy_tAb",
              "KEY_ÉCLAIR", "KEY_ßTRASSE", "KEY_ǆUNGLA", "KEY_İSTANBUL", "KEY_ΣΊΣΥΦΟΣ",
              "KEY_😀SMILE", "KEY_ÀB_ÇD", "KEY_ĳSSEL", "KEY_ŉ_X"]
    key_log(out / "labels.jsonl", [(0.5 + i * 0.1, name, []) for i, name in enumerate(labels)])

    singles = ["a", "Z", "KEY_a", "KEY_Q", "7", "KEY_0", "_", "KEY_-", " ", ".", "é", "KEY_É", "ß",
               "ǅ", "ǆ", "Ⅻ", "½", "٣", "ж", "ー", "中", "́", " ", "😀", "KEY_😀", "ﬀ",
               "ŉ", "\ud800", "KEY_\udc00", "\u0000", "\t", "", "KEY_"]
    entries = []
    t = 0.25
    for name in singles:
        entries.append((t, name, []))
        entries.append((t + 0.7, name, ["shift"]))
        t += 1.5
    # Python's json writes lone surrogates as \udXXX escapes, which QJsonDocument turns into
    # unpaired UTF-16 units; NUL and TAB are escaped as well.
    lines = [json.dumps({"t": 1000000 + round(s * 1e6), "k": "kd", "code": 30, "name": n, "mods": m},
                        ensure_ascii=True, separators=(",", ":")) for s, n, m in entries]
    write(out / "single-characters.jsonl", "".join(f"{line}\n" for line in lines))

    key_log(out / "modifiers.jsonl", [
        (0.5, "KEY_LEFTCTRL", []), (0.55, "KEY_RIGHTSHIFT", []), (0.6, "KEY_LEFTALT", []),
        (0.65, "KEY_LEFTMETA", []), (0.7, "KEY_RIGHTCTRL", ["ctrl"]), (0.75, "CTRL", []),
        (0.8, "KEY_FN_SHIFTED", []), (0.85, "KEY_ALTERNATE", []), (0.9, "KEY_METADATA", []),
        (2.0, "KEY_C", ["ctrl"]), (3.0, "KEY_C", ["shift"]), (4.0, "KEY_C", ["alt"]),
        (5.0, "KEY_C", ["meta"]), (6.0, "KEY_T", ["ctrl", "shift", "alt", "meta"]),
        (7.0, "KEY_T", ["meta", "alt", "shift", "ctrl"]), (8.0, "KEY_X", ["ctrl", "ctrl", "shift"]),
        (9.0, "KEY_X", ["Ctrl", "SHIFT", "hyper", "super", ""]), (10.0, "KEY_X", ["hyper"]),
        (11.0, "KEY_Y", None), (12.0, "KEY_1", ["shift"]), (13.0, "KEY_ENTER", ["meta"]),
        (14.0, "KEY_KP5", ["alt", "ctrl"]), (15.0, "a", ["ctrl"]),
    ])
    key_log(out / "grouping.jsonl", [
        # Gaps around the 0.6 s threshold: microsecond times make 0.6 not exactly 0.6.
        (0.5, "KEY_A", ["ctrl"]), (1.1, "KEY_B", ["ctrl"]), (1.699999, "KEY_C", ["ctrl"]),
        (2.300001, "KEY_D", ["ctrl"]), (2.9, "KEY_E", ["ctrl"]),
        # The six-key cap: three two-key shortcuts fill a group; the fourth starts a new one.
        (5.0, "KEY_F", ["ctrl"]), (5.1, "KEY_G", ["ctrl"]), (5.2, "KEY_H", ["ctrl"]),
        (5.3, "KEY_I", ["ctrl"]),
        # A five-key chord after a two-key group overflows it; one more key still fits.
        (7.0, "KEY_J", ["alt"]), (7.2, "KEY_K", ["ctrl", "shift", "alt", "meta"]),
        (7.3, "KEY_ESC", []),
        # A single event can't hold more than five keys (four modifiers and its key).
        (9.0, "KEY_L", ["ctrl", "shift", "alt", "meta", "ctrl"]),
        # Same time, then going back in time.
        (11.0, "KEY_M", ["ctrl"]), (11.0, "KEY_N", ["ctrl"]), (10.5, "KEY_O", ["ctrl"]),
        (12.5, "KEY_P", ["ctrl"]),
        # Plain typing between shortcuts.
        (14.0, "KEY_A", []), (14.1, "KEY_B", []), (14.2, "KEY_S", ["ctrl"]), (14.3, "KEY_C", []),
        (14.4, "KEY_SPACE", []), (14.5, "KEY_D", []),
    ])
    repeats = [(0.5, "KEY_RIGHT", [])]
    repeats += [(0.5 + 0.5 + i * 0.033, "KEY_RIGHT", []) for i in range(30)]
    repeats += [(2.5, "KEY_RIGHT", [], "ku")]
    repeats += [(3.0, "KEY_Z", ["ctrl"])] + [(3.5 + i * 0.05, "KEY_Z", ["ctrl"]) for i in range(8)]
    repeats += [(4.5, "KEY_Z", [], "ku"), (5.0, "KEY_LEFTSHIFT", []), (5.2, "KEY_A", ["shift"]),
                (8.0, "KEY_LEFTSHIFT", [], "ku"), (8.5, "KEY_B", [], "ku"), (9.0, None, ["ctrl"]),
                (9.1, "", ["ctrl"]), (9.2, "KEY_Q", ["ctrl"], "kd"), (9.25, "KEY_Q", ["ctrl"], "ku")]
    key_log(out / "repeats-and-holds.jsonl", repeats)

    settings = {
        "disabled": {},
        "enabled": {"enabled": True},
        "all-keys": {"enabled": True, "showOnlyShortcuts": False},
        "hold-0": {"enabled": True, "showOnlyShortcuts": False, "holdMs": 0},
        "hold-1": {"enabled": True, "holdMs": 1},
        "hold-negative": {"enabled": True, "showOnlyShortcuts": False, "holdMs": -400},
        "hold-max": {"enabled": True, "holdMs": 2147483647},
        "hold-min": {"enabled": True, "holdMs": -2147483648},
        "hold-5000": {"enabled": True, "holdMs": 5000},
        "size-position": {"enabled": True, "size": 3.5, "position": "top-left"},
    }
    every = sorted(settings)
    logs = ["inputlog/fixture-input.jsonl", "inputlog/kinds.jsonl", "inputlog/unicode.jsonl",
            "keystroketrack/labels.jsonl", "keystroketrack/single-characters.jsonl",
            "keystroketrack/modifiers.jsonl", "keystroketrack/grouping.jsonl",
            "keystroketrack/repeats-and-holds.jsonl"]
    runs = [[log, ["enabled", "all-keys"]] for log in logs]
    runs[0][1] = every
    runs[3][1] = ["all-keys"]
    runs[6][1] = every
    runs[7][1] = ["enabled"]
    write(out / "cases.json", json.dumps({"settings": settings, "runs": runs}, indent=1) + "\n")


# ---------------------------------------------------------------------------------------------
# motiontrack/: MotionTrack::build inputs


def event_log(path: Path, events: list) -> None:
    """events: dicts with t in seconds after the first frame (1000000 us)."""
    lines = []
    for event in events:
        value = dict(event)
        value["t"] = 1000000 + round(value["t"] * 1e6)
        lines.append(json.dumps(value, separators=(",", ":")))
    write(path, "".join(f"{line}\n" for line in lines))


def make_motiontrack() -> None:
    out = CORPUS / "motiontrack"
    reset(out)
    clicks = [{"t": 0.0, "k": "m", "x": 960, "y": 540}]
    for i in range(1, 80):
        t = i * 0.05
        clicks.append({"t": round(t, 6), "k": "m", "x": round(960 + 700 * math.sin(t * 1.7), 3),
                       "y": round(540 + 400 * math.cos(t * 2.3), 3)})
    clicks += [
        {"t": 0.4, "k": "d", "b": "left", "x": 1200, "y": 300},
        {"t": 0.52, "k": "u", "b": "left", "x": 1200, "y": 300},
        {"t": 0.9, "k": "d", "b": "right", "x": 100, "y": 900},
        {"t": 1.3, "k": "d", "b": "left", "x": 500, "y": 500},
        {"t": 1.35, "k": "d", "b": "left", "x": 500, "y": 500},
        {"t": 1.4, "k": "u", "b": "left", "x": 500, "y": 500},
        {"t": 1.45, "k": "u", "b": "left", "x": 500, "y": 500},
        {"t": 1.8, "k": "u", "b": "middle", "x": 10, "y": 10},
        {"t": 2.0, "k": "d", "b": "middle", "x": 1900, "y": 1000},
        {"t": 3.2, "k": "u", "b": "middle", "x": 1900, "y": 1000},
        {"t": 3.6, "k": "s", "dx": 0, "dy": -1, "x": 800, "y": 400},
        {"t": 3.95, "k": "d", "b": "left", "x": 960, "y": 540},
        {"t": 4.2, "k": "d", "b": "left", "x": 0, "y": 0},
    ]
    clicks.sort(key=lambda e: e["t"])
    event_log(out / "clicks-1080p.jsonl", clicks)
    event_log(out / "no-moves.jsonl", [
        {"t": 0.5, "k": "d", "b": "left", "x": 100, "y": 100},
        {"t": 0.6, "k": "u", "b": "left", "x": 100, "y": 100},
        {"t": 1.5, "k": "d", "b": "left", "x": 300, "y": 200},
    ])
    event_log(out / "single-move.jsonl", [{"t": 1.0, "k": "m", "x": 50, "y": 300}])
    idle = []
    for start, end, x0, x1 in ((0.0, 0.5, 100, 500), (2.0, 2.2, 500, 520), (2.9, 3.0, 520, 100),
                               (5.5, 5.6, 100, 600)):
        steps = round((end - start) / 0.01)
        for i in range(steps + 1):
            idle.append({"t": round(start + (end - start) * i / steps, 6), "k": "m",
                         "x": round(x0 + (x1 - x0) * i / steps, 3), "y": 180})
    idle.append({"t": 4.0, "k": "d", "b": "left", "x": 100, "y": 180})
    idle.append({"t": 4.1, "k": "u", "b": "left", "x": 100, "y": 180})
    event_log(out / "idle-gaps.jsonl", idle)

    synthetic = {"input": "inputlog/bundles/synthetic", "duration": 8.0, "width": 640,
                 "height": 360, "captureFps": 60.0, "zooms": "generate"}
    cases = {
        "synthetic-defaults": {},
        "synthetic-no-zooms": {"zooms": []},
        "synthetic-no-smoothing": {"set": {"cursor.smoothing": False}},
        "synthetic-instant-zoom": {"set": {"zoomStyle.instantAnimation": True, "zoomStyle.motionBlur": 0.5}},
        "synthetic-bouncy-springs": {"set": {"cursor.spring": [0.5, 600.0, 4.0],
                                             "zoomStyle.spring": [1.0, 300.0, 3.0]}},
        "synthetic-snapping-springs": {"set": {"cursor.spring": [0.0, 470.0, 70.0],
                                               "zoomStyle.spring": [2.25, 0.0, 40.0]}},
        "synthetic-hide-idle-300": {"set": {"cursor.hideWhenIdleMs": 300, "cursor.clickShrink": 0.5,
                                            "cursor.rotateOnXMovementRatio": 2.0}},
        "synthetic-hide-idle-0": {"set": {"cursor.hideWhenIdleMs": 0,
                                          "cursor.rotateOnXMovementRatio": -1.0}},
        "synthetic-cursor-hidden": {"set": {"cursor.visible": False, "cursor.clickEffect": "none",
                                            "cursor.rotateOnXMovementRatio": 0.0}},
        "synthetic-short-30fps": {"duration": 3.0, "captureFps": 30.0,
                                  "set": {"zoomStyle.snapToEdgesRatio": 0.0}},
        "synthetic-duration-0": {"duration": 0.0},
        "synthetic-duration-negative": {"duration": -1.0},
        "clicks-1080p": {"input": "motiontrack/clicks-1080p.jsonl", "duration": 4.0, "width": 1920,
                         "height": 1080, "zooms": [
                             {"id": "z1", "start": 0.5, "end": 2.0, "level": 2.0, "auto": True, "x": 0.5, "y": 0.5},
                             {"id": "z2", "start": 2.5, "end": 3.5, "level": 3.0, "auto": False, "x": 0.9, "y": 0.1}],
                         "set": {"zoomStyle.instantAnimation": True, "cursor.clickShrink": 1.2}},
        "clicks-zero-size": {"input": "motiontrack/clicks-1080p.jsonl", "duration": 1.0, "width": 0,
                             "height": 0, "captureFps": 0.0, "zooms": []},
        "no-moves": {"input": "motiontrack/no-moves.jsonl", "duration": 2.0, "zooms": "generate"},
        "single-move": {"input": "motiontrack/single-move.jsonl", "duration": 2.0, "zooms": [],
                        "set": {"cursor.hideWhenIdleMs": 100}},
        "idle-gaps": {"input": "motiontrack/idle-gaps.jsonl", "duration": 6.0, "zooms": "generate",
                      "set": {"cursor.hideWhenIdleMs": 500, "cursor.smoothing": False}},
    }
    for name, case in cases.items():
        merged = dict(synthetic)
        merged.update(case)
        cases[name] = merged
    write(out / "cases.json", json.dumps(cases, indent=1) + "\n")


# ---------------------------------------------------------------------------------------------
# recordingpreferences/: settings.json shapes


def make_recordingpreferences() -> None:
    out = CORPUS / "recordingpreferences"
    reset(out)
    files = {}
    files["valid-full"] = json.dumps({
        "captureBackend": "portal", "countdownBeforeRecording": True, "microphone": False,
        "microphoneDevice": "alsa_input.usb-Blue_Yeti", "selfViewEnabled": False,
        "selfViewPosition": {"x": 0.25, "y": 0.75}, "selfViewSize": "L", "systemAudio": False,
        "webcam": {"enabled": True, "device": "/dev/video0", "chosenDevice": "/dev/video4",
                   "captureHeight": 720, "rotation": 270, "flipHorizontal": True,
                   "position": "top-left", "size": 0.3, "shape": "square", "radius": 8,
                   "crop": "square", "shadow": {"enabled": False, "intensity": 0.5, "blur": 10,
                                                "distance": 5, "angle": 90},
                   "border": {"enabled": False, "width": 2, "color": "#000000", "alpha": 1},
                   "scaleDuringZoom": 1, "offset": {"x": 0.05, "y": 0.02}},
    }, indent=4) + "\n"
    files["empty-object"] = "{}"
    files["empty-file"] = ""
    files["invalid-json"] = '{"systemAudio": false, "microphone": false,'
    files["top-level-array"] = '[{"systemAudio": false}]'
    files["bom"] = '﻿{"systemAudio": false, "selfViewSize": "S"}'
    files["duplicate-keys"] = '{"microphone": false, "microphone": true, "selfViewSize": "S", "selfViewSize": "L"}'
    files["null-values"] = json.dumps({key: None for key in (
        "systemAudio", "microphone", "microphoneDevice", "selfViewEnabled", "selfViewSize",
        "selfViewPosition", "captureBackend", "countdownBeforeRecording", "webcam")})
    files["wrong-types"] = json.dumps({
        "systemAudio": "false", "microphone": 0, "microphoneDevice": 5, "selfViewEnabled": [],
        "selfViewSize": 2, "selfViewPosition": "1,1", "captureBackend": False,
        "countdownBeforeRecording": "true", "webcam": "on"})
    files["self-view-small-lowercase"] = '{"selfViewSize": "s", "selfViewPosition": {"x": 0, "y": 0}}'
    files["self-view-unknown-size"] = '{"selfViewSize": "xl", "selfViewPosition": {"x": -0.5, "y": 1.5}}'
    files["self-view-eszett"] = '{"selfViewSize": "ß", "selfViewPosition": {"x": "0.2", "y": null}}'
    files["self-view-dotless-i"] = '{"selfViewSize": " l ", "selfViewPosition": [0.1, 0.2]}'
    files["self-view-position-extremes"] = ('{"selfViewSize": "M", "selfViewPosition": '
                                           '{"x": 1e308, "y": -0.0, "z": 3}}')
    webcam = {
        "unknown-keys": {"enabled": True, "device": "/dev/video1", "future": {"nested": [1, "two", None]},
                         "label": "Caméra 📷", "size": "large", "zz": 1.5},
        "legacy-inset": {"inset": {"enabled": True, "width": 3, "color": "#ff0000", "alpha": 0.5},
                         "shape": "round"},
        "inset-and-border": {"inset": {"width": 3}, "border": {"width": 9}},
        "legacy-mirror": {"mirror": True},
        "mirror-and-flip": {"mirror": True, "flipHorizontal": False},
        "mirror-not-bool": {"mirror": "yes"},
        "height-720-rotation-90": {"captureHeight": 720, "rotation": 90},
        "height-480-rotation-45": {"captureHeight": 480, "rotation": 45},
        "height-string-rotation-string": {"captureHeight": "720", "rotation": "90"},
        "height-fraction-rotation-negative": {"captureHeight": 720.5, "rotation": -90},
        "height-720.0-rotation-180.0": {"captureHeight": 720.0, "rotation": 180.0},
        "height-huge-rotation-360": {"captureHeight": 1e10, "rotation": 360},
        "height-1080-rotation-270": {"captureHeight": 1080, "rotation": 270, "flipHorizontal": 1},
        "device-missing": {},
        "chosen-device-missing": {"device": "/dev/video0"},
        "device-wrong-types": {"device": 3, "chosenDevice": None, "enabled": "true"},
        "not-object": None,
    }
    for name, value in webcam.items():
        files[f"webcam-{name}"] = json.dumps({"webcam": [] if value is None else value},
                                             ensure_ascii=False)
    for name, text in files.items():
        write(out / f"{name}.json", text)


# ---------------------------------------------------------------------------------------------
# mediaprobe/ffprobe/crafted-*: ffprobe answers no real file gives (record-ffprobe.sh records
# the real ones)


def make_mediaprobe() -> None:
    out = CORPUS / "mediaprobe" / "ffprobe"
    out.mkdir(parents=True, exist_ok=True)
    for path in out.glob("crafted-*"):
        shutil.rmtree(path)

    def stream(**fields) -> dict:
        value = {"codec_type": "video", "width": 320, "height": 240, "sample_aspect_ratio": "1:1",
                 "r_frame_rate": "30/1", "avg_frame_rate": "30/1", "duration": "2.000000"}
        value.update(fields)
        return {k: v for k, v in value.items() if v is not None}

    def answer(streams, format_=None) -> str:
        root = {"programs": [], "stream_groups": [], "streams": streams,
                "format": {"duration": "2.000000"} if format_ is None else format_}
        return json.dumps(root, indent=4) + "\n"

    audio = {"codec_type": "audio", "r_frame_rate": "0/0", "avg_frame_rate": "0/0", "duration": "2.000000"}
    frames = "".join(f"{i / 30:.6f}\n" for i in range(60))
    short_frames = "0.000000\n1.000000\n"
    cases = {
        "rotate-tag": {"video": answer([stream(tags={"rotate": "90"})])},
        "rotate-tag-number": {"video": answer([stream(tags={"rotate": 90})])},
        "rotate-tag-and-side-data": {"video": answer([stream(tags={"rotate": "90"},
                                                             side_data_list=[{"rotation": 180}])])},
        "rotate-side-data-several": {"video": answer([stream(side_data_list=[
            {"rotation": 90}, {"side_data_type": "Display Matrix"}, {"rotation": -180}])])},
        "rotate-minus-270": {"video": answer([stream(side_data_list=[{"rotation": -270}])])},
        "rotate-450": {"video": answer([stream(side_data_list=[{"rotation": 450}])])},
        "rotate-fraction": {"video": answer([stream(side_data_list=[{"rotation": 90.5}])])},
        "rotate-string": {"video": answer([stream(side_data_list=[{"rotation": "90"}])])},
        "sar-zero": {"video": answer([stream(sample_aspect_ratio="0:1")])},
        "sar-na": {"video": answer([stream(sample_aspect_ratio="N/A")])},
        "sar-slash": {"video": answer([stream(sample_aspect_ratio="3/2")])},
        "sar-plain-number": {"video": answer([stream(sample_aspect_ratio="1.5")])},
        "sar-divide-by-zero": {"video": answer([stream(sample_aspect_ratio="1:0")])},
        "sar-three-parts": {"video": answer([stream(sample_aspect_ratio="1:2:3")])},
        "sar-missing": {"video": answer([stream(sample_aspect_ratio=None)])},
        "sar-rounding-half": {"video": answer([stream(width=1, height=3, sample_aspect_ratio="5:2",
                                                      side_data_list=[{"rotation": 90}])])},
        "sar-rounding-853": {"video": answer([stream(width=853, height=480, sample_aspect_ratio="3:2")])},
        "fps-average-unknown": {"video": answer([stream(avg_frame_rate="0/0", r_frame_rate="30000/1001")])},
        "fps-average-too-high": {"video": answer([stream(avg_frame_rate="500/1", r_frame_rate="25/1")])},
        "fps-average-240": {"video": answer([stream(avg_frame_rate="240/1", r_frame_rate="25/1")])},
        "fps-none": {"video": answer([stream(avg_frame_rate="N/A", r_frame_rate="0/0")])},
        "fps-negative": {"video": answer([stream(avg_frame_rate="-30/1", r_frame_rate=None)])},
        "fps-plain-number": {"video": answer([stream(avg_frame_rate="29.97", r_frame_rate="1:0")])},
        "duration-na": {"video": answer([stream(duration="N/A")], {"duration": "2.5"})},
        "duration-number": {"video": answer([stream(duration=2.5)], {"duration": "3.25"})},
        "duration-zero-no-format": {"video": answer([stream(duration="0")], {})},
        "duration-negative": {"video": answer([stream(duration="-1")], {"duration": "-2"})},
        "duration-format-number": {"video": answer([stream(duration=None)], {"duration": 4})},
        "width-string": {"video": answer([stream(width="640")])},
        "height-zero": {"video": answer([stream(height=0)])},
        "two-video-streams": {"video": answer([stream(), stream(width=1920, height=1080,
                                                               duration="9.000000"), audio])},
        "audio-before-video": {"video": answer([audio, stream(width=640, height=360)])},
        "no-streams": {"video": json.dumps({"format": {"duration": "3.0"}}) + "\n"},
        "streams-not-array": {"video": json.dumps({"streams": {"0": stream()}}) + "\n"},
        "garbage-stdout": {"video": "not json\n"},
        "empty-stdout": {"video": ""},
        "exit-1-with-json": {"video": answer([stream()]), "video-stderr": "  something failed \n\n",
                             "video-status": 1},
        "exit-2-no-stderr": {"video": "", "video-status": 2},
        "exit-0-with-stderr": {"video": answer([stream()]), "video-stderr": "[h264 @ 0x0] warning\n"},
        "frames-unsorted-and-odd-lines": {
            "video": answer([stream()]),
            "frames": "0.500000\n0.100000\n\n N/A\n0.300000\r\n 0.200000 \n1e1\n-0.033333\n"
                      "0.400000,\ninf\n+0.250000\n0x10\n1.5e-3\n.75\n0.300000\n"},
        "frames-exit-1": {"video": answer([stream()]), "frames": frames, "frames-status": 1},
        "frames-empty": {"video": answer([stream()]), "frames": ""},
        "frames-no-trailing-newline": {"video": answer([stream()]), "frames": "0.000000\n0.033333"},
    }
    for name, case in cases.items():
        directory = out / f"crafted-{name}.mp4"
        for kind, key in (("probeVideo", "video"), ("probeFrameTimes", "frames")):
            write(directory / f"{kind}.stdout", case.get(key, short_frames if key == "frames" else ""))
            write(directory / f"{kind}.stderr", case.get(f"{key}-stderr", ""))
            write(directory / f"{kind}.status", f"{case.get(f'{key}-status', 0)}\n")


# ---------------------------------------------------------------------------------------------
# composition/: projects and editor/exporter environments


def make_composition() -> None:
    out = CORPUS / "composition"
    reset(out)
    synthetic = ROOT / "tests" / "fixtures" / "synthetic"
    bundle = out / "bundles" / "keys"
    bundle.mkdir(parents=True)
    shutil.copy(synthetic / "capture.json", bundle / "capture.json")
    lines = (synthetic / "input.jsonl").read_text().splitlines()
    keys = [(1.2, "KEY_S", ["ctrl"]), (1.5, "KEY_H", []), (1.6, "KEY_I", []), (2.6, "KEY_Z", ["ctrl"]),
            (2.8, "KEY_Z", ["ctrl", "shift"]), (4.4, "KEY_ENTER", []), (5.1, "KEY_TAB", ["alt"]),
            (6.9, "KEY_SPACE", ["meta"]), (7.7, "KEY_Q", ["ctrl"])]
    for seconds, name, mods in keys:
        lines.append(key_line(1000000 + round(seconds * 1e6), name, mods))
        lines.append(json.dumps({"t": 1000000 + round(seconds * 1e6) + 80000, "k": "ku", "code": 30},
                                separators=(",", ":")))
    lines.sort(key=lambda line: json.loads(line)["t"])
    write(bundle / "input.jsonl", "".join(f"{line}\n" for line in lines))

    def project(**sections) -> dict:
        value = {"version": 1, "defaultsVersion": 4, "name": "Composition", "clips": [clip("c1", 0.0, 8.0)],
                 "zooms": []}
        value.update(sections)
        return value

    zooms = [{"id": "z1", "start": 1.0, "end": 3.0, "level": 2.0, "auto": True},
             {"id": "z2", "start": 4.5, "end": 6.0, "level": 3.0, "auto": False, "x": 0.8, "y": 0.2}]
    cases = {
        "plain": {"project": project(cursor={"clickEffect": "none"})},
        "zooms-and-cuts": {"project": project(
            clips=[clip("c1", 0.0, 3.0), clip("c2", 4.0, 8.0, 2.0)],
            zooms=[{"id": z["id"], "start": z["start"], "end": z["end"], "level": z["level"],
                    "target": "auto" if z["auto"] else {"x": z["x"], "y": z["y"]}} for z in zooms],
            cursor={"clickEffect": "ripple", "hideWhenIdleMs": 400})},
        "camera-layouts": {
            "project": project(camera={"enabled": True},
                               clips=[clip("c1", 0.0, 2.0), clip("c2", 2.0, 4.0, layout="camera"),
                                      clip("c3", 4.0, 6.0, layout="screen"), clip("c4", 6.0, 8.0)],
                               cursor={"clickEffect": "circle"}),
            "camera": {"offset": 0.75, "duration": 6.0}},
        "camera-bubble-off": {
            "project": project(camera={"enabled": False},
                               clips=[clip("c1", 0.0, 3.0, layout="camera"), clip("c2", 3.0, 8.0, 1.5)]),
            "camera": {"offset": -0.5, "duration": 20.0}},
        "inserted-media": {
            "project": project(
                camera={"enabled": True}, keystrokes={"enabled": True, "showOnlyShortcuts": False},
                clips=[clip("c1", 0.0, 2.5), clip("m1", 0.0, 1.5, media="media/a.mp4", fit="fill"),
                       clip("c2", 2.5, 6.0, 1.5), clip("m2", 0.5, 2.0, 2.0, media="media/b.mp4", fit="framed"),
                       clip("c3", 6.0, 8.0, layout="camera")],
                cursor={"clickEffect": "ripple", "visible": False}),
            "bundle": "composition/bundles/keys", "camera": {"offset": 0.25, "duration": 8.0},
            "inserts": {"media/a.mp4": [1280, 720, 3.0, 30.0, True],
                        "media/b.mp4": [720, 1280, 4.0, 25.0, False]},
            "mediaPlayerSource": "media/a.mp4"},
        "inserted-media-missing": {
            "project": project(clips=[clip("m1", 0.0, 2.0, media="media/missing.mp4"), clip("c1", 0.0, 8.0),
                                      clip("m2", 0.0, 1.0, media="media/a.mp4", fit="framed")]),
            "inserts": {"media/a.mp4": [1920, 1080, 1.0, 60.0, True], "media/missing.mp4": None},
            "mediaFrameReady": False, "mediaPlayerSource": "media/a.mp4"},
        "keystrokes": {
            "project": project(keystrokes={"enabled": True, "holdMs": 400},
                               clips=[clip("c1", 0.0, 4.0), clip("c2", 5.0, 8.0, 0.75)]),
            "bundle": "composition/bundles/keys"},
        "no-motion-yet": {"project": project(keystrokes={"enabled": True}), "motion": False,
                          "bundle": "composition/bundles/keys",
                          "camera": {"offset": 0.0, "duration": 8.0}},
    }
    defaults = {"bundle": "inputlog/bundles/synthetic", "sourceDuration": 8.0, "width": 640,
                "height": 360, "captureFps": 60.0, "camera": None, "inserts": {},
                "mediaFrameReady": True, "mediaPlayerSource": "", "motion": True}
    for name, case in cases.items():
        merged = dict(defaults)
        merged.update(case)
        cases[name] = merged
    write(out / "cases.json", json.dumps(cases, indent=1, ensure_ascii=False) + "\n")


# ---------------------------------------------------------------------------------------------
# The recording side (src/record, src/ui): crafted answers of the programs it asks (hyprctl,
# gpu-screen-recorder, pactl) next to the real-* recordings of record-recording-tools.sh, and the
# cases of the recorder, regionpicker, captureexclusion, camera, recordingbar, microphone,
# screencapture and launcher families.


def crafted(directory: Path, files: dict) -> None:
    """Replaces the crafted-* files of a directory that also holds recordings."""
    directory.mkdir(parents=True, exist_ok=True)
    for path in directory.glob("crafted-*"):
        path.unlink()
    for name, data in files.items():
        write(directory / f"crafted-{name}", data)


def monitor(name: str, width: int, height: int, x=0, y=0, scale=1, transform=0, focused=False,
            reserved=(0, 0, 0, 0), disabled=False, **extra) -> dict:
    value = {"id": 0, "name": name, "description": f"Oracle {name}", "make": "Oracle",
             "model": name, "serial": "", "width": width, "height": height,
             "refreshRate": 60.0, "x": x, "y": y,
             "activeWorkspace": {"id": 1, "name": "1"}, "specialWorkspace": {"id": 0, "name": ""},
             "reserved": list(reserved) if reserved is not None else None, "scale": scale,
             "transform": transform, "focused": focused, "dpmsStatus": True, "vrr": False,
             "disabled": disabled, "currentFormat": "XRGB8888", "mirrorOf": "none"}
    value.update(extra)
    return {k: v for k, v in value.items() if v is not None}


def pretty(value) -> str:
    return json.dumps(value, indent=4, ensure_ascii=False) + "\n"


def make_tool_answers() -> None:
    hyprctl = CORPUS / "hyprctl"
    two = [monitor("DP-1", 2560, 1440, scale=1.25, reserved=(0, 30, 0, 0)),
           monitor("HDMI-A-1", 1920, 1080, x=2048, focused=True)]
    for index, value in enumerate(two):
        value["id"] = index
    rotated = [monitor(f"ROT-{t}", 2560, 1600, x=1600 * t, scale=1.6, transform=t, focused=t == 3)
               for t in range(8)]
    for index, value in enumerate(rotated):
        value["id"] = index
    edge = monitor("eDP-1", 1921, 1081, scale=1.5, focused=True)
    without = monitor("NOFIELDS", 1280, 720)
    for key in ("x", "y", "scale", "transform", "focused", "reserved", "disabled"):
        del without[key]
    crafted(hyprctl / "monitors", {
        "two.json": pretty(two),
        "rotated.json": pretty(rotated),
        "disabled.json": pretty([monitor("DP-1", 1920, 1080, disabled=True, focused=True),
                                 monitor("DP-2", 2560, 1440, x=1920)]),
        "all-disabled.json": pretty([monitor("DP-1", 1920, 1080, disabled=True)]),
        "odd-size.json": pretty([edge]),
        "scales.json": pretty([monitor("S0", 1920, 1080, scale=0), monitor("S-NEG", 1920, 1080, scale=-2),
                               monitor("S-THIRD", 2560, 1440, scale=1.3333333333333333),
                               monitor("S-STRING", 1920, 1080, scale="2"),
                               monitor("S-HALF", 1920, 1080, scale=0.5),
                               monitor("S-TINY", 1920, 1080, scale=0.001)]),
        "missing-fields.json": pretty([without, {"name": "BARE"}, {}]),
        "wrong-types.json": pretty([monitor("STR", "1920", "1080", x="10", y=None, focused="true",
                                            transform="1", disabled="false", reserved="0,0,0,0"),
                                    monitor(7, 1920.5, 1080.5, x=12.75, y=-0.5, transform=1.0)]),
        "reserved.json": pretty([monitor("R-SHORT", 1920, 1080, reserved=[10]),
                                 monitor("R-EMPTY", 1920, 1080, reserved=[]),
                                 monitor("R-LONG", 1920, 1080, reserved=[1, 2, 3, 4, 5]),
                                 monitor("R-STRINGS", 1920, 1080, reserved=["5", 6, None, 7.5]),
                                 monitor("R-OBJECT", 1920, 1080, reserved={"top": 30})]),
        "negative-positions.json": pretty([monitor("LEFT", 1920, 1080, x=-1920, y=-200),
                                           monitor("MAIN", 3840, 2160, scale=2, focused=True)]),
        "focus-none.json": pretty([monitor("A", 1920, 1080), monitor("B", 1920, 1080, x=1920)]),
        "focus-both.json": pretty([monitor("A", 1920, 1080, focused=True),
                                   monitor("B", 1920, 1080, x=1920, focused=True)]),
        "duplicate-names.json": pretty([monitor("DUP", 1920, 1080), monitor("DUP", 1280, 720, x=1920, focused=True)]),
        "huge.json": pretty([monitor("HUGE", 3000000000, 2147483647, x=1e300, y=-1e300)]),
        "unicode.json": pretty([monitor("Écran \"1\" ☃", 1920, 1080, focused=True)]),
        "empty-array.json": "[]\n",
        "object.json": pretty({"name": "DP-1", "width": 1920}),
        "null.json": "null\n",
        "scalars.json": "[1, \"DP-1\", null, true, [2]]\n",
        "garbage.txt": "unknown request\n",
        "empty.txt": "",
        "truncated.json": pretty(two)[:120],
    })
    clients = [
        {"address": "0x1", "mapped": True, "hidden": False, "at": [12, 38], "size": [1896, 1030],
         "workspace": {"id": 1, "name": "1"}, "floating": False, "class": "kitty", "title": "Window 1"},
        {"address": "0x2", "mapped": True, "hidden": False, "at": [400, 300], "size": [640, 480],
         "workspace": {"id": 1, "name": "1"}, "floating": True, "class": "float", "title": "Window 2"},
        {"address": "0x3", "mapped": True, "hidden": True, "at": [0, 0], "size": [100, 100],
         "workspace": {"id": 1, "name": "1"}, "class": "hidden", "title": "Window 3"},
        {"address": "0x4", "mapped": True, "hidden": False, "at": [2060, 10], "size": [1000, 700],
         "workspace": {"id": 2, "name": "2"}, "class": "other", "title": "Window 4"},
        {"address": "0x5", "at": [-50, -40], "size": [300, 200], "workspace": {"id": 1}},
        {"address": "0x6", "at": [10, 20, 30], "size": [300, 200], "workspace": {"id": 1}},
        {"address": "0x7", "at": [10, 20], "size": [300], "workspace": {"id": 1}},
        {"address": "0x8", "at": [12.5, 20.25], "size": [300.75, 200], "workspace": {"id": 1}},
        {"address": "0x9", "at": ["10", 20], "size": [300, 200], "workspace": {"id": 1}},
        {"address": "0xa", "at": [5, 5], "size": [0, 0], "workspace": {"id": 1}},
        {"address": "0xb", "at": [5, 5], "size": [-10, 20], "workspace": {"id": 1}},
        {"address": "0xc", "at": [100, 100], "size": [200, 200], "workspace": {"id": "1"}},
        {"address": "0xd", "at": [100, 100], "size": [200, 200], "hidden": "true", "workspace": {"id": 1}},
        {"address": "0xe", "at": [100, 100], "size": [200, 200]},
        {"address": "0xf", "at": [3000000000, 1], "size": [2147483647, 1], "workspace": {"id": 1}},
    ]
    special = [dict(clients[0], workspace={"id": -98, "name": "special:magic"}, title="Window 1")]
    crafted(hyprctl / "clients", {
        "mixed.json": pretty(clients),
        "special.json": pretty(special),
        "none.json": "[]\n",
        "object.json": pretty(clients[0]),
        "garbage.txt": "not json\n",
        "empty.txt": "",
    })
    crafted(hyprctl / "activeworkspace", {
        "one.json": pretty({"id": 1, "name": "1", "monitor": "DP-1", "windows": 3}),
        "two.json": pretty({"id": 2, "name": "2", "monitor": "HDMI-A-1", "windows": 1}),
        "special.json": pretty({"id": -98, "name": "special:magic", "monitor": "DP-1"}),
        "string-id.json": pretty({"id": "1", "name": "1"}),
        "missing-id.json": pretty({"name": "1"}),
        "array.json": "[{\"id\": 1}]\n",
        "garbage.txt": "no such request\n",
        "empty.txt": "",
    })

    def layer(namespace: str) -> dict:
        return {"address": "0x1", "x": 0, "y": 0, "w": 100, "h": 40, "namespace": namespace, "pid": 1}

    def layers(monitors: dict) -> str:
        return pretty({name: {"levels": levels} for name, levels in monitors.items()})

    clean = {"0": [layer("omarchy-background")], "1": [], "2": [layer("waybar")], "3": []}
    ours = {"0": [layer("omarchy-background")], "2": [layer("omareel-bar")], "3": []}
    crafted(hyprctl / "layers", {
        "clean-two.json": layers({"DP-1": clean, "HDMI-A-1": clean}),
        "overlay-on-dp1.json": layers({"DP-1": ours, "HDMI-A-1": clean}),
        "overlay-on-hdmi.json": layers({"DP-1": clean, "HDMI-A-1": {"3": [layer("omareel-selfview")]}}),
        "near-prefixes.json": layers({"DP-1": {"0": [layer("omareel"), layer("xomareel-bar"),
                                                      layer("Omareel-bar"), layer(" omareel-bar"),
                                                      layer("omarecord-bar")]}}),
        "prefix-alone.json": layers({"DP-1": {"1": [layer("omareel-")]}}),
        "levels-array.json": pretty({"DP-1": {"levels": [[layer("omareel-bar")]]}}),
        "levels-missing.json": pretty({"DP-1": {}}),
        "layer-not-object.json": pretty({"DP-1": {"levels": {"0": ["omareel-bar", 3, None]}}}),
        "namespace-not-string.json": pretty({"DP-1": {"levels": {"0": [{"namespace": 7}]}}}),
        "level-not-array.json": pretty({"DP-1": {"levels": {"0": {"namespace": "omareel-bar"}}}}),
        "monitor-not-object.json": pretty({"DP-1": "omareel-bar"}),
        "empty-object.json": "{}\n",
        "array.json": "[]\n",
        "garbage.txt": "unknown request\n",
        "trailing-garbage.json": layers({"DP-1": clean}) + "x",
        "empty.txt": "",
    })
    good = "a" * 40
    crafted(hyprctl / "plugin-list", {
        "ours.json": pretty([{"name": "other", "version": "1"}, {"name": "omareel-capture-exclude", "version": "0.1.0"}]),
        "others.json": pretty([{"name": "hyprexpo"}, {"name": "omareel-capture-exclude-old"}]),
        "none.json": "[]\n",
        "no-plugins.txt": "no plugins loaded\n",
        "name-not-string.json": pretty([{"name": 1}]),
        "object.json": pretty({"name": "omareel-capture-exclude"}),
        "twice.json": pretty([{"name": "omareel-capture-exclude"}, {"name": "omareel-capture-exclude"}]),
    })
    crafted(hyprctl / "version", {
        "a40.json": pretty({"branch": "", "commit": good, "version": "0.0.0", "tag": "v0.0.0"}),
        "b40.json": pretty({"commit": "b" * 40}),
        "uppercase.json": pretty({"commit": good.upper()}),
        "short.json": pretty({"commit": "abc123"}),
        "dirty.json": pretty({"commit": good + "-dirty"}),
        "missing.json": pretty({"version": "0.56.2"}),
        "garbage.txt": "unknown request\n",
    })

    def status(built=good, prefixes=("omareel-",), **extra) -> str:
        value = {"version": "0.1.0", "built_hash": built, "prefixes": list(prefixes) if prefixes is not None else None,
                 "monitors": [{"name": "DP-1", "capturing": False, "excluded_layers": 0}]}
        value.update(extra)
        return json.dumps({k: v for k, v in value.items() if v is not None}, separators=(",", ":"))

    crafted(hyprctl / "exclude-status", {
        "a40.json": status(),
        "b40.json": status("b" * 40),
        "no-prefix.json": status(prefixes=()),
        "other-prefix.json": status(prefixes=("omarecord-", "omareel")),
        "two-prefixes.json": status(prefixes=("x-", "omareel-")),
        "prefixes-missing.json": status(prefixes=None),
        "prefixes-string.json": status(prefixes=None).replace('"monitors"', '"prefixes":"omareel-","monitors"'),
        "hash-missing.json": status(built=None),
        "hash-number.json": status(built=None, built_hash=1),
        "unknown-request.txt": "unknown request\n",
        "empty.txt": "",
    })
    crafted(hyprctl / "getoption-cursor", {
        "2.json": '{"option": "cursor:no_hardware_cursors", "int": 2, "set": false }\n',
        "1.json": '{"option": "cursor:no_hardware_cursors", "int": 1, "set": true }\n',
        "0.json": '{"option": "cursor:no_hardware_cursors", "int": 0, "set": true }\n',
        "negative.json": '{"option": "cursor:no_hardware_cursors", "int": -1, "set": true }\n',
        "string.json": '{"option": "cursor:no_hardware_cursors", "int": "2", "set": true }\n',
        "custom-type.json": '{"option": "cursor:no_hardware_cursors", "custom": "2", "set": true }\n',
        "no-such-option.txt": "no such option\n",
    })

    gsr = CORPUS / "gpu-screen-recorder"
    crafted(gsr / "list-capture-options", {
        "two-monitors.txt": "window\nfocused\nDP-1|2560x1440\nHDMI-A-1|1920x1080\nregion\nportal\n",
        "crlf.txt": "DP-1|2560x1440\r\nHDMI-A-1|1920x1080\r\n",
        "spaces.txt": "  DP-1 | 2560x1440  \n\tHDMI-A-1\t|\t1920x1080\t\nDell U2720Q|3840x2160\n",
        "duplicates.txt": "DP-1|2560x1440\nDP-1|1920x1080\nDP-2|1920x1080\n",
        "malformed.txt": "DP-1|\n|1920x1080\nDP-2|1920x1080x\nDP-3|1920X1080\nDP-4|1920x\nDP-5 1920x1080\n"
                         "DP-6||1920x1080\nDP-7|1920x1080|\nDP-8|-1920x1080\nDP-9|+1920x1080\nDP|10|1920x1080\n",
        "unicode.txt": "Écran☃|1920x1080\nＤＰ-1|1920x1080\nDP-1|１９２０x1080\n",
        "no-newline.txt": "DP-1|2560x1440",
        "only-region.txt": "region\nportal\n",
        "empty.txt": "",
    })
    crafted(gsr / "list-audio-devices", {
        "mixed.txt": "default_output|Default output\ndefault_input|Default input\n"
                     "alsa_output.usb-Speaker.analog-stereo.monitor|Monitor of Speaker\n"
                     "alsa_input.usb-Blue_Yeti-00.analog-stereo|Yeti Stereo Microphone Analog Stereo\n"
                     "alsa_input.usb-Insta360_Insta360_Link_2-02.mono-fallback|Insta360 Link 2 Mono\n"
                     "bluez_input.00_11_22_33_44_55|Headset\n"
                     "alsa_input.platform-mtk_sdw_mc.HiFi__Mic__source|Digital Microphone\n",
        "whitespace.txt": "  default_input  |  Default input  \r\n\talsa_input.x\t|\tTabbed\t\n\n\n"
                          "alsa_input.y|\nalsa_input.z|a|b|c\n",
        "malformed.txt": "|Nothing\nno separator line\nalsa_input.bad\n|\nALSA_INPUT.upper|Upper\n"
                         "alsa_input|Bare prefix\nalsa_input.|Prefix dot\n default_input|Leading space\n",
        "duplicates.txt": "alsa_input.a|First\nalsa_input.a|Second\ndefault_input|Default input\n",
        "only-outputs.txt": "default_output|Default output\nalsa_output.pci.hdmi-stereo.monitor|HDMI\n",
        "unicode.txt": "alsa_input.usb-Röde|Röde NT-USB ☃\n",
        "empty.txt": "",
    })
    crafted(CORPUS / "pactl" / "list-short-sources", {
        "dock.txt": "45\talsa_output.pci-0000_00_1f.3.analog-stereo.monitor\tPipeWire\ts32le 2ch 48000Hz\tSUSPENDED\n"
                    "46\talsa_input.usb-Insta360_Insta360_Link_2-02.mono-fallback\tPipeWire\ts16le 1ch 48000Hz\tRUNNING\n"
                    "47\talsa_input.platform-mtk_sdw_mc.HiFi__Mic__source\tPipeWire\ts32le 2ch 48000Hz\tIDLE\n",
        "malformed.txt": "no tabs here\n\n10\t\tPipeWire\n11\tonly-name\n\t\n12\tspaced name\tPipeWire\r\n",
        "empty.txt": "",
    })


def volumedetect_lines(n_samples, mean=None, peak=None, histogram=None, address="0x1") -> str:
    tag = f"[Parsed_volumedetect_0 @ {address}] "
    lines = [f"{tag}n_samples: {n_samples}\n"]
    if mean is not None:
        lines.append(f"{tag}mean_volume: {mean} dB\n")
    if peak is not None:
        lines.append(f"{tag}max_volume: {peak} dB\n")
    if histogram is not None:
        lines.append(f"{tag}histogram_{histogram}db: 12\n")
    return "".join(lines)


def make_recorder() -> None:
    out = CORPUS / "recorder"
    probe = "[Parsed_volumedetect_0 @ 0xbfe75fef2360] n_samples: 0\n"
    crafted(out / "volumedetect", {
        "two-graphs.log": probe + volumedetect_lines(49152, "-91.0", "-91.0", 91, "0xf76498003ae0"),
        "speech.log": probe + volumedetect_lines(960000, "-38.2", "-6.4", 6),
        "probe-only.log": probe,
        "no-report.log": "size=N/A time=00:00:10.00\n",
        "empty.log": "",
        "max-before-samples.log": "[Parsed_volumedetect_0 @ 0x1] max_volume: -12.5 dB\n"
                                  "[Parsed_volumedetect_0 @ 0x1] n_samples: 48000\n",
        "samples-after-max-zero.log": volumedetect_lines(48000, "-20.0", "-3.0") + probe,
        "minus-inf.log": volumedetect_lines(48000, "-inf", "-inf"),
        "inf-and-nan.log": volumedetect_lines(48000, "nan", "inf"),
        "plus-and-exponent.log": volumedetect_lines(48000, None, "+5.0")
                                 + "[Parsed_volumedetect_1 @ 0x2] max_volume: -1e1 dB\n",
        "comma-decimal.log": volumedetect_lines(48000, None, "-6,5"),
        "unit-attached.log": "[Parsed_volumedetect_0 @ 0x1] max_volume: -6.4dB\n",
        "double-space.log": "[Parsed_volumedetect_0 @ 0x1] max_volume:  -6.4 dB\n",
        "garbage-value.log": volumedetect_lines(48000, None, "loud"),
        "samples-garbage.log": "[Parsed_volumedetect_0 @ 0x1] n_samples: many\n",
        "samples-negative.log": "[Parsed_volumedetect_0 @ 0x1] n_samples: -0\n",
        "samples-zero-padded.log": "[Parsed_volumedetect_0 @ 0x1] n_samples: 00\n",
        "crlf.log": volumedetect_lines(48000, "-20.0", "-3.0").replace("\n", "\r\n"),
        "no-bracket-space.log": "[Parsed_volumedetect_0 @ 0x1]max_volume: -6.4 dB\n",
        "two-brackets.log": "[aac @ 0x1] [Parsed_volumedetect_0 @ 0x2] max_volume: -6.4 dB\n",
        "mention-only.log": "Parsed_volumedetect max_volume: -6.4 dB\n",
        "other-filter.log": "[Parsed_astats_0 @ 0x1] max_volume: -6.4 dB\n",
        "second-mic.log": volumedetect_lines(48000, "-30.0", "-10.0")
                          + volumedetect_lines(48000, "-50.0", "-40.0", None, "0x2").replace("_0 @", "_1 @"),
        "leading-text.log": "frame=1 [Parsed_volumedetect_0 @ 0x1] max_volume: -6.4 dB\n",
        "unicode.log": "[Parsed_volumedetect_0 @ 0x1] max_volume: −6.4 dB\n",
        "huge.log": volumedetect_lines(9223372036854775807, None, "-1e400"),
    })
    timestamps = out / "timestamps"
    reset(timestamps)
    for name, data in {
        "gsr.ts": "1234567\n",
        "native.ts": "monotonic_microsec\trealtime_microsec\n1234567\t1790000000000000\n",
        "header-only.ts": "monotonic_microsec\trealtime_microsec\n",
        "empty.ts": "",
        "blank-lines.ts": "\n\n   \n987654\t1\n",
        "crlf.ts": "monotonic_microsec\trealtime_microsec\r\n42\t43\r\n",
        "zero.ts": "0\t1790000000000000\n",
        "negative.ts": "-5\t1\n",
        "plus.ts": "+77\t1\n",
        "spaces.ts": "  555  \t 1\n",
        "space-separated.ts": "555 1790000000000000\n",
        "hex.ts": "0x10\t1\n",
        "decimal.ts": "12.5\t1\n",
        "huge.ts": "9223372036854775807\t1\n",
        "overflow.ts": "9223372036854775808\t1\n",
        "two-values.ts": "x\t1\n100\t2\n200\t3\n",
        "no-newline.ts": "31337",
        "nul.ts": b"12\x0034\n",
        "bom.ts": "﻿99\t1\n",
    }.items():
        write(timestamps / name, data)
    output = out / "output"
    reset(output)
    lines = "".join(f"line {i:04d} of the recorder's output\n" for i in range(140))
    for name, data in {
        "gsr-error.txt": "gsr info: Capturing region\ngsr error: failed to create encoder: h264 not supported\n",
        "crlf.txt": "first\r\nlast line\r\n\r\n",
        "trailing-blank.txt": "the reason\n\n   \n\t\n",
        "whitespace-only.txt": " \n\t\n  \n",
        "empty.txt": "",
        "no-newline.txt": "a\nb without newline",
        "invalid-utf8.bin": b"ok line\n\xff\xfe broken \xc3\n",
        "nul.bin": b"before\x00after\n",
        "big.txt": lines,
        "unicode.txt": "Fehler: Gerät nicht gefunden ☃\n",
        "carriage-return.txt": "progress 1\rprogress 2\rdone",
    }.items():
        write(output / name, data)
    state = out / "state"
    reset(state)
    recording = {
        "pid": 0, "bundle": "/home/user/Videos/Recording 2026-10-25 10-00-00.omareel",
        "started_us": 123456789012, "daemon_started_us": 123456700000, "monitor": "DP-1",
        "no_open": False, "webcam": True, "camera_device": "/dev/video2", "camera_height": 1080,
        "camera_rotation": 90, "camera_flip_horizontal": True, "capture_mode": "region",
        "capture_x": 10.5, "capture_y": 20, "capture_width": 640, "capture_height": 360,
        "capture_backend": "ext-image-copy-capture", "overlay_exclusion": "plugin",
        "overlay_monitor": "DP-1", "capture_started": True, "countdown_requested": False,
        "countdown_active": False, "countdown_ready": False, "countdown_end_us": 0,
        "countdown_width": 240, "countdown_height": 240, "bar_visible": True, "bar_width": 448,
        "bar_height": 40, "selfview": True, "selfview_safe": True, "selfview_monitor": "DP-1",
        "selfview_size": "M", "selfview_pixels": 160, "selfview_x": 16, "selfview_y": 16,
        "camera_status": "ready", "camera_record": True, "camera_stop": False}
    for name, data in {
        "recording.json": json.dumps(recording, indent=4) + "\n",
        "minimal.json": '{"pid": 1, "started_us": 5, "monitor": "HDMI-A-1", "webcam": false}\n',
        "strings.json": '{"pid": "0", "started_us": "77", "monitor": 3, "webcam": "true", "command_sequence": "5"}\n',
        "doubles.json": '{"pid": -1, "started_us": 1.5e6, "webcam": 1, "command_sequence": 2.5}\n',
        "huge.json": '{"pid": 0, "started_us": 9007199254740993, "command_sequence": 9223372036854775806}\n',
        "nulls.json": '{"pid": null, "started_us": null, "monitor": null, "webcam": null, "command_sequence": null}\n',
        "nested.json": '{"command": "rotate", "command_sequence": 3, "extra": {"a": [1, 2, {"b": null}]}}\n',
        "array.json": "[1, 2, 3]\n",
        "invalid.json": '{"pid": 0,\n',
        "empty.json": "",
        "bom.json": "﻿{\"monitor\": \"DP-1\"}\n",
        "duplicate-keys.json": '{"monitor": "first", "monitor": "second"}\n',
        "trailing-garbage.json": '{"monitor": "DP-1"} trailing\n',
        "unicode.json": '{"monitor": "Écran ☃", "bundle": "/tmp/r\\u00e9c\\u2028.omareel"}\n',
    }.items():
        write(state / name, data)

    def cursor(*samples):
        return [list(sample) for sample in samples]

    def device(t, kind, **fields):
        value = {"t": t, "kind": kind}
        value.update(fields)
        return value

    region = {"x": 100.0, "y": 50.0, "scale": 2.0}
    input_log = {
        "empty": {"region": region, "cursor": [], "devices": []},
        "moves-only": {"region": region, "cursor": cursor((1000, 100, 50), (2000, 100.5, 50.25),
                                                          (3000, 1379.5, 769.75), (4000, 99, 49))},
        "clicks-and-scrolls": {"region": region,
            "cursor": cursor((1000, 300, 200), (5000, 310, 205), (9000, 500, 400)),
            "devices": [device(500, "ButtonDown", button="left"), device(1000, "ButtonUp", button="left"),
                        device(4999, "ButtonDown", button="right"), device(5000, "Scroll", dx=0, dy=-1),
                        device(7000, "Scroll", dx=2, dy=0), device(9000, "ButtonUp", button="right"),
                        device(12000, "ButtonDown", button="middle"), device(12001, "ButtonDown", button="")]},
        "keys": {"region": {"x": 0.0, "y": 0.0, "scale": 1.0}, "cursor": cursor((100, 1, 2)),
            "devices": [device(200, "KeyDown", code=29, name="KEY_LEFTCTRL", mods=["ctrl"]),
                        device(300, "KeyDown", code=31, name="KEY_S", mods=["ctrl"]),
                        device(350, "KeyUp", code=31, name="KEY_S", mods=["ctrl"]),
                        device(400, "KeyUp", code=29, name="KEY_LEFTCTRL", mods=[]),
                        device(500, "KeyDown", code=57, name="KEY_SPACE", mods=[]),
                        device(600, "KeyDown", code=0, name="", mods=["shift", "alt", "meta", "ctrl"]),
                        device(700, "KeyDown", code=240, name="KEY_UNKNOWN", mods=["shift", "shift"])]},
        "ties-and-order": {"region": region,
            "cursor": cursor((1000, 1, 1), (1000, 2, 2), (3000, 3, 3), (2000, 4, 4)),
            "devices": [device(1000, "KeyDown", code=30, name="KEY_A", mods=[]),
                        device(1000, "ButtonDown", button="left"), device(500, "ButtonUp", button="left"),
                        device(2000, "KeyUp", code=30), device(3000, "Scroll", dx=-3, dy=4)]},
        "before-first-sample": {"region": region, "cursor": cursor((5000, 640, 360)),
            "devices": [device(1, "ButtonDown", button="left"), device(4999, "Scroll", dy=1)]},
        "no-cursor": {"region": region, "cursor": [],
            "devices": [device(10, "ButtonDown", button="left"), device(20, "Scroll", dx=1, dy=1)]},
        "negative-and-large-times": {"region": region,
            "cursor": cursor((-5, 0, 0), (9007199254740993, 1, 1), (9223372036854775807, 2, 2)),
            "devices": [device(-10, "ButtonDown", button="left"), device(9223372036854775807, "KeyUp", code=1)]},
        "fractional-scale": {"region": {"x": 0.3, "y": -0.1, "scale": 1.3333333333333333},
            "cursor": cursor((1, 0.1, 0.2), (2, 1919.9, 1079.95), (3, -4, 1e-7)),
            "devices": [device(2, "ButtonDown", button="left")]},
        "zero-scale": {"region": {"x": 10.0, "y": 10.0, "scale": 0.0}, "cursor": cursor((1, 15, 25)),
            "devices": [device(1, "ButtonDown", button="left")]},
        "unicode-names": {"region": region, "cursor": [],
            "devices": [device(1, "KeyDown", code=30, name="KEY_Ä \"q\" \\", mods=["Strg", "☃"])]},
    }
    microphone = {
        "devices": ["default_input", "alsa_input.usb-Insta360_Insta360_Link_2-02.mono-fallback",
                    "alsa_input.usb-Elgato_Cam_Link_4K-02.analog-stereo", "", "Default_Input",
                    " default_input", "alsa_input.platform-mtk_sdw_mc.HiFi__Mic__source"],
        "sources": {
            "none": [],
            "laptop-and-camera": ["alsa_input.platform-mtk_sdw_mc.HiFi__Mic__source",
                                  "alsa_input.usb-Insta360_Insta360_Link_2-02.mono-fallback"],
            "empty-name": [""],
            "with-default": ["default_input"],
            "duplicates": ["alsa_input.usb-Elgato_Cam_Link_4K-02.analog-stereo",
                           "alsa_input.usb-Elgato_Cam_Link_4K-02.analog-stereo"],
        },
    }
    two = "hyprctl/monitors/crafted-two.json"
    placement = {}
    for size in ("S", "M", "L", "XL", "", "m"):
        placement[f"size-{size or 'empty'}"] = {"webcam": True, "selfView": True, "size": size,
                                                "monitor": "DP-1", "x": 0.5, "y": 0.5, "monitors": two}
    # qRound of the position is undefined past int's range, so the positions stay within it.
    for x, y in ((0, 0), (1, 1), (0.25, 0.75), (-1, 2), (0.5000001, 0.4999999), (1000, -1000)):
        placement[f"position-{x}-{y}"] = {"webcam": True, "selfView": True, "size": "M",
                                          "monitor": "HDMI-A-1", "x": x, "y": y, "monitors": two}
    placement.update({
        "no-webcam": {"webcam": False, "selfView": True, "size": "L", "monitor": "DP-1", "x": 0.5,
                      "y": 0.5, "monitors": two},
        "no-selfview": {"webcam": True, "selfView": False, "size": "S", "monitor": "DP-1", "x": 0.5,
                        "y": 0.5, "monitors": two},
        "unknown-monitor": {"webcam": True, "selfView": True, "size": "M", "monitor": "DP-9", "x": 0.5,
                            "y": 0.5, "monitors": two},
        "hyprctl-fails": {"webcam": True, "selfView": True, "size": "M", "monitor": "DP-1", "x": 0.5,
                          "y": 0.5, "monitors": None},
        "tiny-monitor": {"webcam": True, "selfView": True, "size": "L", "monitor": "TINY", "x": 1,
                         "y": 1, "monitors": "recorder/monitors-tiny.json"},
        "scaled-odd": {"webcam": True, "selfView": True, "size": "M", "monitor": "eDP-1", "x": 1,
                       "y": 1, "monitors": "hyprctl/monitors/crafted-odd-size.json"},
        "disabled-target": {"webcam": True, "selfView": True, "size": "M", "monitor": "DP-1", "x": 1,
                            "y": 1, "monitors": "hyprctl/monitors/crafted-disabled.json"},
        "duplicate-names": {"webcam": True, "selfView": True, "size": "M", "monitor": "DUP", "x": 1,
                            "y": 1, "monitors": "hyprctl/monitors/crafted-duplicate-names.json"},
        "real-desktop": {"webcam": True, "selfView": True, "size": "M", "monitor": "HDMI-A-1", "x": 0.3,
                         "y": 0.9, "monitors": "hyprctl/monitors/real-desktop.json"},
    })
    write(out / "monitors-tiny.json", pretty([monitor("TINY", 300, 200, scale=1.5)]))
    value_after = {
        "arguments": [[], ["--fps"], ["--fps", "60"], ["--fps", "60", "--fps", "30"], ["--dir", "--fps", "5"],
                      ["--x", "-1.5", "--y"], ["--fps", ""], ["--FPS", "60"], ["x", "--fps", "--fps"]],
        "names": ["--fps", "--dir", "--x", "--y", "", "60"],
    }
    cursor_at = {
        "samples": {"empty": [], "one": [[1000, 1.5, 2.5]],
                    "three": [[1000, 1, 1], [2000, 2, 2], [3000, 3, 3]],
                    "ties": [[1000, 1, 1], [1000, 2, 2], [1000, 3, 3], [2000, 4, 4]]},
        "times": [-9223372036854775807, 0, 999, 1000, 1001, 1999, 2000, 2500, 3000, 3001,
                  9223372036854775807],
    }
    position_object = {
        "points": [[0, 0], [100, 50], [100.5, 50.25], [-1e300, 1e300], [1919.9999999999998, 0.1]],
        "regions": [{"x": 0.0, "y": 0.0, "scale": 1.0}, {"x": 100.0, "y": 50.0, "scale": 2.0},
                    {"x": 0.1, "y": 0.2, "scale": 1.25}, {"x": -1920.0, "y": 0.0, "scale": 0.0},
                    {"x": 0.0, "y": 0.0, "scale": -1.0}],
    }
    write(out / "cases.json", json.dumps({
        "inputLog": input_log, "microphone": microphone, "selfViewPlacement": placement,
        "valueAfter": value_after, "cursorAt": cursor_at, "positionObject": position_object,
    }, indent=1, ensure_ascii=False, allow_nan=True) + "\n")


def answer(argv, stdout=None, status=0, stdout_file=None, **extra) -> dict:
    value = {"argv": argv, "status": status}
    if stdout is not None:
        value["stdout"] = stdout
    if stdout_file is not None:
        value["stdoutFile"] = stdout_file
    value.update(extra)
    return value


def make_regionpicker() -> None:
    out = CORPUS / "regionpicker"
    reset(out)
    rects = ["10,20 300x200", "0,0 1x1", "-10,-20 300x200", "10.5,20.25 300.75x200.5", " 10,20 300x200 ",
             "\t10,20\t300x200\t", "10,20  300x200", "10, 20 300x200", "10,20 300X200", "10,20 300x200x",
             "10,20 -300x200", "+10,20 300x200", "10,20 300x200\n", "10,20\n300x200", ".5,1 2x3",
             "5.,1 2x3", "1e3,0 2x3", "10,20 0x0", "999999999999999999999,0 1x1", "١٠,٢٠ ٣٠٠x٢٠٠",
             "10,20 300x200 extra", "", "monitor:DP-1", "10,20", "10 20 300x200",
             "-0,-0 0.0x0.000", "00010,0020 0300x0200"]
    two = "hyprctl/monitors/crafted-two.json"
    workspaces = "hyprctl/activeworkspace"
    clients = "hyprctl/clients"
    rectangles = [
        {"workspace": f"{workspaces}/{w}", "clients": f"{clients}/{c}"}
        for w in ("crafted-one.json", "crafted-two.json", "crafted-special.json", "crafted-string-id.json",
                  "crafted-missing-id.json", "crafted-array.json", "crafted-garbage.txt", "real-desktop.json")
        for c in ("crafted-mixed.json", "crafted-special.json", "crafted-none.json", "crafted-object.json",
                  "crafted-garbage.txt", "real-desktop.json")]
    rectangles += [{"workspace": None, "clients": f"{clients}/crafted-mixed.json"},
                   {"workspace": f"{workspaces}/crafted-one.json", "clients": None}]
    snap = {
        "rectangles": ["12,38 1896x1030\n400,300 640x480\n", "400,300 640x480\n12,38 1896x1030\n",
                       "", "garbage\n10,10 5x5\n", "0,0 0x0\n", "-50,-40 300x200\n", "10,20 300x200",
                       "\n\n10,20 300x200\n\n", "10.5,20.5 1.5x1.5\n"],
        "points": [[12, 38], [11.999, 38], [1907, 1067], [1908, 1068], [400, 300], [1039.5, 779.9],
                   [10, 10], [14.999, 14.999], [15, 15], [0, 0], [-50, -40], [249.999, 159.999],
                   [10.5, 20.5], [12, 22]],
    }
    monitor_for = {
        "sets": [two, "hyprctl/monitors/crafted-negative-positions.json", "hyprctl/monitors/crafted-empty-array.json",
                 "hyprctl/monitors/crafted-rotated.json", "hyprctl/monitors/real-desktop.json"],
        "rects": [[0, 0, 10, 10], [2047, 0, 2, 2], [2046, 0, 2, 2], [2048, 1000, 0, 0], [5000, 5000, 1, 1],
                  [-1920, -200, 0, 0], [-1, -1, 2, 2], [100, 100, -300, -300], [0, 1151.9, 0, 0.2],
                  [1e300, 0, 1e300, 0], [1919.5, 1079.5, 1, 1]],
    }
    options = "gpu-screen-recorder/list-capture-options"

    def gsr_options(file=None, status=0, hang=0) -> dict:
        extra = {"hang": hang} if hang else {}
        if file is None:
            return answer(["gpu-screen-recorder", "--list-capture-options"], "", status, **extra)
        return answer(["gpu-screen-recorder", "--list-capture-options"], None, status,
                      stdout_file=f"{options}/{file}", **extra)

    capturable = {}
    for requested in ("DP-1", "HDMI-A-1"):
        for name, reply in (("two-monitors", gsr_options("crafted-two-monitors.txt")),
                            ("only-region", gsr_options("crafted-only-region.txt")),
                            ("duplicates", gsr_options("crafted-duplicates.txt")),
                            ("spaces", gsr_options("crafted-spaces.txt")),
                            ("empty", gsr_options("crafted-empty.txt")),
                            ("fails", gsr_options("crafted-two-monitors.txt", 1)),
                            ("real-desktop", gsr_options("real-desktop.txt"))):
            capturable[f"{requested} {name}"] = {"monitors": two, "requested": requested,
                                                 "env": {"OMAREEL_CAPTURE": "gsr"}, "answers": [reply]}
    capturable.update({
        "DP-1 capture env GSR uppercase": {"monitors": two, "requested": "DP-1", "env": {"OMAREEL_CAPTURE": "GSR"},
                                           "answers": [gsr_options("crafted-two-monitors.txt")]},
        "DP-1 preferences gsr": {"monitors": two, "requested": "DP-1", "captureBackend": "gsr",
                                 "answers": [gsr_options("crafted-two-monitors.txt")]},
        "DP-1 auto without a compositor": {"monitors": two, "requested": "DP-1",
                                           "answers": [gsr_options("crafted-two-monitors.txt")]},
        "DP-1 gpu-screen-recorder missing": {"monitors": two, "requested": "DP-1", "env": {"OMAREEL_CAPTURE": "gsr"},
                                             "programs": ["hyprctl"], "answers": []},
        "DP-1 gpu-screen-recorder hangs": {"monitors": two, "requested": "DP-1", "env": {"OMAREEL_CAPTURE": "gsr"},
                                           "answers": [gsr_options(None, 0, 6)]},
    })
    monitors_answer = lambda file=two, status=0, **extra: answer(["hyprctl", "-j", "monitors"], None, status,
                                                                 stdout_file=file, **extra)
    window_answers = [answer(["hyprctl", "-j", "activeworkspace"], None, stdout_file=f"{workspaces}/crafted-one.json"),
                      answer(["hyprctl", "-j", "clients"], None, stdout_file=f"{clients}/crafted-mixed.json")]
    region = ["omarchy-capture-region", "smart", "--match-monitor"]
    slurp = ["slurp", "-f", "%x,%y %wx%h"]
    window = ["slurp", "-r", "-f", "%x,%y %wx%h"]
    no_omarchy = ["hyprctl", "slurp", "gpu-screen-recorder"]
    gsr_env = {"OMAREEL_CAPTURE": "gsr"}
    two_options = gsr_options("crafted-two-monitors.txt")
    pick = {
        "fullscreen focused": {"mode": "Fullscreen", "answers": [monitors_answer()]},
        "fullscreen focus none": {"mode": "Fullscreen",
                                  "answers": [monitors_answer("hyprctl/monitors/crafted-focus-none.json")]},
        "fullscreen focus both": {"mode": "Fullscreen",
                                  "answers": [monitors_answer("hyprctl/monitors/crafted-focus-both.json")]},
        "fullscreen rotated": {"mode": "Fullscreen", "answers": [monitors_answer("hyprctl/monitors/crafted-rotated.json")]},
        "fullscreen odd size": {"mode": "Fullscreen", "answers": [monitors_answer("hyprctl/monitors/crafted-odd-size.json")]},
        "fullscreen scales": {"mode": "Fullscreen", "answers": [monitors_answer("hyprctl/monitors/crafted-scales.json")]},
        "fullscreen real desktop": {"mode": "Fullscreen",
                                    "answers": [monitors_answer("hyprctl/monitors/real-desktop.json")]},
        "fullscreen not capturable": {"mode": "Fullscreen", "env": gsr_env,
                                      "answers": [monitors_answer(), gsr_options("crafted-duplicates.txt")]},
        "fullscreen capturable": {"mode": "Fullscreen", "env": gsr_env, "answers": [monitors_answer(), two_options]},
        "monitors fail": {"mode": "Fullscreen", "answers": [monitors_answer(status=1, stderr="Couldn't connect to Hyprland\n")]},
        "monitors fail quietly": {"mode": "Region", "answers": [monitors_answer("hyprctl/monitors/crafted-empty.txt", 2)]},
        "monitors empty": {"mode": "Region", "answers": [monitors_answer("hyprctl/monitors/crafted-empty-array.json")]},
        "monitors garbage": {"mode": "Window", "answers": [monitors_answer("hyprctl/monitors/crafted-garbage.txt")]},
        "region rectangle": {"mode": "Region", "answers": [monitors_answer(), answer(region, "10,20 300x200\n")]},
        "region on second monitor": {"mode": "Region", "answers": [monitors_answer(), answer(region, "2100,100 641x359\n")]},
        "region fractional": {"mode": "Region", "answers": [monitors_answer(), answer(region, "10.5,20.25 300.75x200.5")]},
        "region outside every monitor": {"mode": "Region", "answers": [monitors_answer(), answer(region, "9000,9000 10x10")]},
        "region monitor": {"mode": "Region", "answers": [monitors_answer(), answer(region, "monitor:HDMI-A-1\n")]},
        "region monitor not capturable": {"mode": "Region", "env": gsr_env,
                                          "answers": [monitors_answer(), answer(region, "monitor:DP-1"),
                                                      gsr_options("crafted-duplicates.txt")]},
        "region unknown monitor": {"mode": "Region", "answers": [monitors_answer(), answer(region, "monitor:DP-9\n")]},
        "region empty monitor name": {"mode": "Region", "answers": [monitors_answer(), answer(region, "monitor:")]},
        "region click": {"mode": "Region", "answers": [monitors_answer(), answer(region, "2500,500 1x1\n")]},
        "region click not capturable": {"mode": "Region", "env": gsr_env,
                                        "answers": [monitors_answer(), answer(region, "2500,500 1x1\n"),
                                                    gsr_options("crafted-duplicates.txt")]},
        "region zero size": {"mode": "Region", "answers": [monitors_answer(), answer(region, "100,100 0x0\n")]},
        "region cancelled": {"mode": "Region", "answers": [monitors_answer(), answer(region, "", 1)]},
        "region cancelled with output": {"mode": "Region",
                                         "answers": [monitors_answer(), answer(region, "10,20 300x200\n", 130)]},
        "region unparsable": {"mode": "Region", "answers": [monitors_answer(), answer(region, "  what is this  \n")]},
        "region empty answer": {"mode": "Region", "answers": [monitors_answer(), answer(region, "")]},
        "region slurp": {"mode": "Region", "programs": no_omarchy,
                         "answers": [monitors_answer(), *window_answers, answer(slurp, "10,20 300x200\n", stdin=True)]},
        "region slurp click on window": {"mode": "Region", "programs": no_omarchy,
                                         "answers": [monitors_answer(), *window_answers,
                                                     answer(slurp, "500,400 2x2\n", stdin=True)]},
        "region slurp click on hidden window": {"mode": "Region", "programs": no_omarchy,
                                                "answers": [monitors_answer(), *window_answers,
                                                            answer(slurp, "50,50 1x1\n", stdin=True)]},
        "region slurp click outside windows": {"mode": "Region", "programs": no_omarchy,
                                               "answers": [monitors_answer(), *window_answers,
                                                           answer(slurp, "2500,1000 1x1\n", stdin=True)]},
        "region slurp without workspace": {"mode": "Region", "programs": no_omarchy,
                                           "answers": [monitors_answer(),
                                                       answer(["hyprctl", "-j", "activeworkspace"], "", 1),
                                                       answer(slurp, "500,400 1x1\n", stdin=True)]},
        "region slurp clients fail": {"mode": "Region", "programs": no_omarchy,
                                      "answers": [monitors_answer(), window_answers[0],
                                                  answer(["hyprctl", "-j", "clients"], "", 1),
                                                  answer(slurp, "500,400 1x1\n", stdin=True)]},
        "region slurp missing": {"mode": "Region", "programs": ["hyprctl"],
                                 "answers": [monitors_answer(), *window_answers]},
        "window": {"mode": "Window", "answers": [monitors_answer(), *window_answers,
                                                 answer(window, "400,300 640x480\n", stdin=True)]},
        "window tiny": {"mode": "Window", "answers": [monitors_answer(), *window_answers,
                                                      answer(window, "400,300 2x2\n", stdin=True)]},
        "window cancelled": {"mode": "Window", "answers": [monitors_answer(), *window_answers,
                                                           answer(window, "", 1, stdin=True)]},
        "window no rectangles": {"mode": "Window",
                                 "answers": [monitors_answer(),
                                             answer(["hyprctl", "-j", "activeworkspace"], None,
                                                    stdout_file=f"{workspaces}/crafted-two.json"),
                                             answer(["hyprctl", "-j", "clients"], "[]"),
                                             answer(window, "10,10 100x100\n", stdin=True)]},
        "window real desktop": {"mode": "Window",
                                "answers": [monitors_answer("hyprctl/monitors/real-desktop.json"),
                                            answer(["hyprctl", "-j", "activeworkspace"], None,
                                                   stdout_file=f"{workspaces}/real-desktop.json"),
                                            answer(["hyprctl", "-j", "clients"], None,
                                                   stdout_file=f"{clients}/real-desktop.json"),
                                            answer(window, "12,38 1896x1030\n", stdin=True)]},
    }
    write(out / "cases.json", json.dumps({
        "rects": rects, "rectangles": rectangles, "snap": snap,
        "monitorFor": monitor_for, "capturable": capturable, "pick": pick,
    }, indent=1, ensure_ascii=False) + "\n")


def make_captureexclusion() -> None:
    out = CORPUS / "captureexclusion"
    reset(out)
    a40, b40 = "a" * 40, "b" * 40
    hashes = ["", a40, b40, a40.upper(), "0123456789abcdef0123456789abcdef01234567", a40[:39], a40 + "a",
              a40 + "\n", " " + a40, "g" * 40, "unknown", "efb50993780079460b0cbed1363e2166a2de1d9f"]
    plugins = "hyprctl/plugin-list"
    status = "hyprctl/exclude-status"
    version = "hyprctl/version"

    def hy(args, file=None, status_=0, stdout=None, **extra):
        return answer(["hyprctl", *args], stdout, status_, stdout_file=file, **extra)

    def scenario(plugin_list, exclude, version_file, extra=()):
        answers = []
        if plugin_list is not None:
            answers.append(hy(["-j", "plugin", "list"], f"{plugins}/{plugin_list}"))
        if exclude is not None:
            answers.append(hy(["omareel-exclude", "status"], f"{status}/{exclude}"))
        if version_file is not None:
            answers.append(hy(["-j", "version"], f"{version}/{version_file}"))
        return answers + list(extra)

    statuses = {
        "loaded": scenario("crafted-ours.json", "crafted-a40.json", "crafted-a40.json"),
        "real desktop": scenario("real-desktop.json", "real-desktop.json", "real-desktop.json"),
        "hash mismatch": scenario("crafted-ours.json", "crafted-a40.json", "crafted-b40.json"),
        "uppercase commit": scenario("crafted-ours.json", "crafted-a40.json", "crafted-uppercase.json"),
        "short commit": scenario("crafted-ours.json", "crafted-a40.json", "crafted-short.json"),
        "dirty commit": scenario("crafted-ours.json", "crafted-a40.json", "crafted-dirty.json"),
        "commit missing": scenario("crafted-ours.json", "crafted-a40.json", "crafted-missing.json"),
        "version garbage": scenario("crafted-ours.json", "crafted-a40.json", "crafted-garbage.txt"),
        "version fails": scenario("crafted-ours.json", "crafted-a40.json", None,
                                  [hy(["-j", "version"], None, 1, stdout=f'{{"commit": "{a40}"}}')]),
        "no prefix": scenario("crafted-ours.json", "crafted-no-prefix.json", "crafted-a40.json"),
        "other prefixes": scenario("crafted-ours.json", "crafted-other-prefix.json", "crafted-a40.json"),
        "two prefixes": scenario("crafted-ours.json", "crafted-two-prefixes.json", "crafted-a40.json"),
        "prefixes missing": scenario("crafted-ours.json", "crafted-prefixes-missing.json", "crafted-a40.json"),
        "prefixes string": scenario("crafted-ours.json", "crafted-prefixes-string.json", "crafted-a40.json"),
        "built hash missing": scenario("crafted-ours.json", "crafted-hash-missing.json", "crafted-a40.json"),
        "built hash number": scenario("crafted-ours.json", "crafted-hash-number.json", "crafted-a40.json"),
        "status unknown request": scenario("crafted-ours.json", "crafted-unknown-request.txt", "crafted-a40.json"),
        "status empty": scenario("crafted-ours.json", "crafted-empty.txt", "crafted-a40.json"),
        "status fails": scenario("crafted-ours.json", None, "crafted-a40.json",
                                 [hy(["omareel-exclude", "status"], f"{status}/crafted-a40.json", 1)]),
        "plugin twice": scenario("crafted-twice.json", "crafted-a40.json", "crafted-a40.json"),
        "other plugins": scenario("crafted-others.json", None, None),
        "no plugins": scenario("crafted-none.json", None, None),
        "no plugins text": scenario("crafted-no-plugins.txt", None, None),
        "plugin name not string": scenario("crafted-name-not-string.json", None, None),
        "plugin list object": scenario("crafted-object.json", None, None),
        "plugin list fails": [hy(["-j", "plugin", "list"], f"{plugins}/crafted-ours.json", 1)],
        "plugin list hangs": [hy(["-j", "plugin", "list"], None, 0, stdout="[]", hang=5)],
        "hyprctl missing": None,
    }

    load = lambda reply, status_=0, **extra: answer(["hyprctl", "plugin", "load", "@PLUGIN@"], reply, status_, **extra)
    not_loaded = [hy(["-j", "plugin", "list"], f"{plugins}/crafted-none.json")]
    loaded_after = [hy(["-j", "plugin", "list"], f"{plugins}/crafted-none.json", call=1),
                    hy(["-j", "plugin", "list"], f"{plugins}/crafted-ours.json"),
                    hy(["omareel-exclude", "status"], f"{status}/crafted-a40.json"),
                    hy(["-j", "version"], f"{version}/crafted-a40.json")]
    version_a = [hy(["-j", "version"], f"{version}/crafted-a40.json")]
    ensure = {
        "path missing": {"plugin": "missing", "answers": []},
        "path is a directory": {"plugin": "directory", "answers": []},
        "path empty": {"plugin": "empty-variable", "answers": []},
        "already loaded": {"plugin": "file", "hash": b40, "answers": loaded_after[1:]},
        "already loaded without override": {"plugin": None, "answers": loaded_after[1:]},
        "no hash file": {"plugin": "file", "hash": None, "answers": not_loaded + version_a},
        "hash mismatch": {"plugin": "file", "hash": b40, "answers": not_loaded + version_a},
        "hash with newline": {"plugin": "file", "hash": a40 + "\n", "answers": loaded_after + [load("ok")]},
        "hash with spaces": {"plugin": "file", "hash": "  " + a40 + " \r\n", "answers": loaded_after + [load("ok")]},
        "hash uppercase": {"plugin": "file", "hash": a40.upper(), "answers": not_loaded
                           + [hy(["-j", "version"], None, stdout=f'{{"commit": "{a40.upper()}"}}')]},
        "loads": {"plugin": "file", "hash": a40, "answers": loaded_after + [load("ok")]},
        "loads but status stays unloaded": {"plugin": "file", "hash": a40,
                                            "answers": not_loaded + version_a + [load("ok")]},
        "load reply with newline": {"plugin": "file", "hash": a40, "answers": loaded_after + [load("ok\n")]},
        "load reply spaced": {"plugin": "file", "hash": a40, "answers": loaded_after + [load("  ok  \n\n")]},
        "load reply Ok": {"plugin": "file", "hash": a40, "answers": not_loaded + version_a + [load("Ok")]},
        "load refused": {"plugin": "file", "hash": a40, "answers": not_loaded + version_a
                         + [load("error: plugin could not be loaded:\n  version mismatch\n")]},
        "load exit status": {"plugin": "file", "hash": a40, "answers": not_loaded + version_a + [load("ok", 1)]},
        "load exit status quiet": {"plugin": "file", "hash": a40, "answers": not_loaded + version_a + [load("", 3)]},
        "load stderr only": {"plugin": "file", "hash": a40,
                             "answers": not_loaded + version_a + [load(None, 1, stderr="couldn't connect\n")]},
        "load stdout and stderr": {"plugin": "file", "hash": a40,
                                   "answers": not_loaded + version_a + [load("ok", 0, stderr="warning\n")]},
        "load hangs": {"plugin": "file", "hash": a40, "answers": not_loaded + version_a + [load("ok", 0, hang=5)]},
        "relative path": {"plugin": "relative", "hash": a40, "answers": loaded_after + [load("ok")]},
    }
    layers = "hyprctl/layers"
    off = {"files": sorted(p.name for p in (CORPUS / layers).iterdir()),
           "monitors": ["DP-1", "HDMI-A-1", "", "dp-1", "DP-9"]}
    overlay = {"monitors": [[], ["DP-3"], ["DP-3", "DP-5"], ["DP-5", "DP-3"], ["DP-3", "DP-3", "DP-5"],
                            ["", "DP-3"], ["DP-3", ""], ["dp-3", "DP-5"]],
               "recorded": ["DP-3", "DP-5", "", "dp-3", "DP-9"]}
    cursor = "hyprctl/getoption-cursor"
    get = lambda file, status_=0: hy(["-j", "getoption", "cursor:no_hardware_cursors"], f"{cursor}/{file}", status_)
    evaluate = lambda value, status_=0, **extra: hy(["eval", f"hl.config({{ cursor = {{ no_hardware_cursors = {value} }} }})"],
                                                    None, status_, stdout="ok", **extra)
    keyword = lambda value, status_=0: hy(["keyword", "cursor:no_hardware_cursors", str(value)], None, status_, stdout="ok")
    cursors = {
        "automatic": [get("crafted-2.json"), evaluate(0), evaluate(2)],
        "real desktop": [get("real-desktop.json"), evaluate(0), evaluate(2)],
        "software": [get("crafted-1.json"), evaluate(0), evaluate(1)],
        "already hardware": [get("crafted-0.json")],
        "negative": [get("crafted-negative.json"), evaluate(0), evaluate(-1)],
        "string value": [get("crafted-string.json")],
        "custom type": [get("crafted-custom-type.json")],
        "no such option": [get("crafted-no-such-option.txt")],
        "getoption fails": [get("crafted-2.json", 1)],
        "hyprlang config": [get("crafted-1.json"), evaluate(0, 1), keyword(0), evaluate(1, 1), keyword(1)],
        "nothing works": [get("crafted-2.json"), evaluate(0, 1), keyword(0, 1)],
        "eval works only first": [get("crafted-2.json"), evaluate(0), evaluate(2, 1, call=1), keyword(2)],
    }
    write(out / "cases.json", json.dumps({
        "hashes": hashes, "status": statuses, "ensureLoaded": ensure, "overlaysOffMonitor": off,
        "overlayMonitor": overlay, "hardwareCursor": cursors,
    }, indent=1, ensure_ascii=False) + "\n")


def make_camera() -> None:
    out = CORPUS / "camera"
    reset(out)

    def node(name, interface, index):
        return {"files": {f"dev/{name}": "", f"sys/class/video4linux/{name}/index": f"{index}\n"},
                "dirs": [f"sys/devices/{interface}"],
                "links": {f"sys/class/video4linux/{name}/device": f"sys/devices/{interface}"}}

    def merge(*parts):
        value = {"files": {}, "dirs": [], "links": {}}
        for part in parts:
            value["files"].update(part.get("files", {}))
            value["dirs"] += part.get("dirs", [])
            value["links"].update(part.get("links", {}))
        return value

    usb = {"files": {"sys/devices/usb7/7-1/idVendor": "636e\n", "sys/devices/usb7/7-1/idProduct": "0bdb\n",
                     "sys/devices/usb7/7-1/serial": "200901010001\n",
                     "sys/devices/usb1/1-1.1/idVendor": "2e1a\n", "sys/devices/usb1/1-1.1/idProduct": "4c04\n"}}
    by_id = "dev/v4l/by-id/"
    by_path = "dev/v4l/by-path/"
    laptop = merge(usb, node("video0", "usb7/7-1/7-1:1.0", 0), node("video1", "usb7/7-1/7-1:1.0", 1),
                   node("video2", "usb7/7-1/7-1:1.2", 0), node("video4", "usb1/1-1.1/1-1.1:1.0", 0),
                   {"links": {
                       by_id + "usb-ASUS_FHD_webcam_200901010001-video-index0": "dev/video2",
                       by_id + "usb-ASUS_FHD_webcam_200901010001-video-index1": "dev/video1",
                       by_id + "usb-Insta360_Insta360_Link_2-video-index0": "dev/video4",
                       by_path + "platform-x-usbv2-0:1:1.0-video-index0": "dev/video0",
                       by_path + "platform-x-usb-0:1:1.0-video-index0": "dev/video0",
                       by_path + "platform-x-usb-0:1:1.2-video-index0": "dev/video2",
                       by_path + "platform-y-usb-0:1.1:1.0-video-index0": "dev/video4"}})
    queries = ["dev/video0", "dev/video1", "dev/video2", "dev/video4", "dev/video9",
               by_id + "usb-Insta360_Insta360_Link_2-video-index0", by_path + "platform-x-usb-0:1:1.0-video-index0",
               "dev/../dev/video4", "", "/dev/oracle-camera-that-does-not-exist"]
    trees = {
        "laptop-and-dock": [
            {"label": "plugged in", **laptop, "queries": queries},
            {"label": "dock renumbered", "remove": ["dev/video4"],
             **merge(node("video6", "usb1/1-1.1/1-1.1:1.0", 0),
                     {"links": {by_id + "usb-Insta360_Insta360_Link_2-video-index0": "dev/video6"}}),
             "queries": queries + ["dev/video6"]},
            {"label": "unplugged", "remove": ["dev/video6"], "queries": queries + ["dev/video6"]},
        ],
        "only-usbv-by-path": [{"label": "one camera", **merge(
            node("video0", "usb3/3-2/3-2:1.0", 0),
            {"files": {"sys/devices/usb3/3-2/idVendor": "046d\n", "sys/devices/usb3/3-2/idProduct": "085e\n"},
             "links": {by_path + "pci-0000:00:14.0-usbv2-0:2:1.0-video-index0": "dev/video0",
                       by_path + "pci-0000:00:14.0-usbv3-0:2:1.0-video-index0": "dev/video0"}}),
            "queries": ["dev/video0"]}],
        "no-links": [{"label": "bare node", **node("video3", "platform/csi", 0), "queries": ["dev/video3"]}],
        "no-sys": [{"label": "by-id only", "files": {"dev/video5": ""},
                    "links": {by_id + "usb-Camera-video-index0": "dev/video5",
                              by_path + "usb-0:1-video-index0": "dev/video5"},
                    "queries": ["dev/video5"]}],
        "identical-cameras": [{"label": "two without serials", **merge(
            {"files": {"sys/devices/usb2/2-1/idVendor": "1234\n", "sys/devices/usb2/2-1/idProduct": "5678\n",
                       "sys/devices/usb2/2-2/idVendor": "1234\n", "sys/devices/usb2/2-2/idProduct": "5678\n"}},
            node("video0", "usb2/2-1/2-1:1.0", 0), node("video2", "usb2/2-2/2-2:1.0", 0),
            {"links": {by_id + "usb-Generic_Camera-video-index0": "dev/video2",
                       by_path + "pci-usb-0:1:1.0-video-index0": "dev/video0",
                       by_path + "pci-usb-0:2:1.0-video-index0": "dev/video2"}}),
            "queries": ["dev/video0", "dev/video2"]}],
        "sorting": [{"label": "several links to one node", "files": {"dev/video7": ""},
                     "links": {by_id + "z-camera-video-index0": "dev/video7",
                               by_id + "a-camera-video-index0": "dev/video7",
                               by_id + "B-camera-video-index0": "dev/video7",
                               by_id + "usbv-camera-video-index0": "dev/video7"},
                     "queries": ["dev/video7"]}],
        "chained-links": [{"label": "link to a link", "files": {"dev/video8": ""},
                           "links": {"dev/camera": "dev/video8", by_id + "usb-Chained-video-index0": "dev/camera"},
                           "queries": ["dev/video8", "dev/camera"]}],
    }
    ids = ["/dev/video6", "v4l2 /dev/video6 usb", "v4l2:/dev/video0", "/dev/video10 /dev/video11",
           "obs-virtual", "", "/dev/videoX", "/dev/video", "prefix/dev/video3/suffix rest", "/DEV/VIDEO1",
           "/dev/v4l/by-id/usb-x-video-index0", " /dev/video2", "/dev/video2\t3", "é/dev/video4☃ x"]

    def fmt(pixel, width, height, low, high):
        return [pixel, width, height, low, high]

    devices = {
        "webcam-1080p": [fmt("Jpeg", 1920, 1080, 5, 30), fmt("Jpeg", 1280, 720, 5, 30), fmt("YUYV", 640, 480, 5, 30),
                         fmt("YUYV", 1920, 1080, 5, 5)],
        "webcam-60fps": [fmt("Jpeg", 1920, 1080, 30, 60), fmt("Jpeg", 1920, 1080, 30, 30), fmt("NV12", 1280, 720, 60, 60)],
        "ir-sensor": [fmt("Y8", 640, 360, 15, 15), fmt("Y8", 340, 340, 2, 29.98)],
        "just-under-30": [fmt("YUYV", 1920, 1080, 29.99, 29.99), fmt("YUYV", 1280, 720, 29.98, 29.98)],
        "ties": [fmt("YUYV", 1920, 1081, 30, 30), fmt("Jpeg", 1920, 1079, 30, 30), fmt("NV12", 1921, 1080, 30, 30)],
        "only-4k": [fmt("Jpeg", 3840, 2160, 30, 30), fmt("Jpeg", 2560, 1440, 30, 30)],
        "only-small": [fmt("YUYV", 320, 240, 30, 30), fmt("YUYV", 160, 120, 60, 60)],
        "high-rate": [fmt("Jpeg", 1920, 1080, 120, 120), fmt("Jpeg", 1280, 720, 30, 30)],
        "no-formats": [],
    }
    write(out / "cases.json", json.dumps({"trees": trees, "ids": ids, "devices": devices}, indent=1,
                                         ensure_ascii=False) + "\n")


def make_microphone() -> None:
    out = CORPUS / "microphone"
    reset(out)

    def pack(fmt, values):
        return struct.pack("<" + fmt * len(values), *values).hex()

    buffers = {
        "UInt8": {"silence": pack("B", [128] * 8), "zeros-are-full-scale": pack("B", [0, 128]),
                  "max": pack("B", [255]), "quiet": pack("B", [129, 127]), "odd": "8080ff",
                  "one-byte": "ff", "empty": "", "ramp": bytes(range(0, 256, 17)).hex()},
        "Int16": {"silence": pack("h", [0] * 6), "full-scale": pack("h", [0, 32767, 0]),
                  "minimum": pack("h", [-32768]), "minus-20-dbfs": pack("h", [100, -3277, 50]),
                  "tiny": pack("h", [1]), "sixteen": pack("h", [16]), "odd-bytes": pack("h", [0, 32767]) + "7f",
                  "single-byte": "7f", "empty": "", "stereo-pairs": pack("h", [1000, -2000, 3000, -4000]),
                  "minus-60-dbfs": pack("h", [33]), "minus-61-dbfs": pack("h", [29])},
        "Int32": {"silence": pack("i", [0, 0]), "full-scale": pack("i", [2147483647]),
                  "minimum": pack("i", [-2147483648]), "half": pack("i", [1073741824]),
                  "tiny": pack("i", [2147]), "odd-bytes": pack("i", [1073741824]) + "ffffff", "three-bytes": "ffffff",
                  "empty": ""},
        "Float": {"silence": pack("f", [0.0, -0.0]), "half": pack("f", [0.5]), "one": pack("f", [1.0]),
                  "over-one": pack("f", [1.5, -2.0]), "nan": "0000c07f", "nan-then-half": "0000c07f" + pack("f", [0.5]),
                  "half-then-nan": pack("f", [0.5]) + "0000c07f", "infinity": "0000807f", "minus-infinity": "000080ff",
                  "denormal": "01000000", "tiny": pack("f", [1e-3]), "minus-60-dbfs": pack("f", [0.001]),
                  "odd-bytes": pack("f", [0.25]) + "0000", "empty": ""},
    }
    sequences = {
        "Int16 attack and decay": ["Int16", ["full-scale", "silence", "silence", "silence", "silence", "silence",
                                             "silence", "silence", "silence", "silence", "silence", "silence",
                                             "silence", "silence", "silence", "silence", "silence", "silence"]],
        "Int16 small changes": ["Int16", ["minus-20-dbfs", "minus-20-dbfs", "stereo-pairs", "minus-20-dbfs",
                                          "tiny", "minus-60-dbfs", "empty"]],
        "Float nan": ["Float", ["half", "nan", "nan-then-half", "half-then-nan", "infinity", "silence"]],
        "UInt8 zeros": ["UInt8", ["silence", "zeros-are-full-scale", "quiet", "silence"]],
    }
    write(out / "cases.json", json.dumps({"buffers": buffers, "channels": [1, 2, 6],
                                          "sequences": sequences}, indent=1) + "\n")


def make_screencapture() -> None:
    out = CORPUS / "screencapture"
    reset(out)
    rings = {
        "the tag's test": [4, [["acquire"], ["acquire"], ["acquire"], ["acquire"], ["acquire"], ["markQueued", 1],
                               ["markQueued", 0], ["takeQueued"], ["takeQueued"], ["state", 1], ["release", 1],
                               ["state", 1], ["acquire"], ["markQueued", 8]]],
        "drops count every refusal": [2, [["acquire"], ["acquire"], ["acquire"], ["acquire"], ["release", 0],
                                          ["acquire"], ["acquire"]]],
        "queue keeps its order": [5, [["acquire"], ["acquire"], ["acquire"], ["markQueued", 2], ["markQueued", 0],
                                      ["markQueued", 1], ["takeQueued"], ["takeQueued"], ["takeQueued"],
                                      ["takeQueued"]]],
        "invalid transitions": [3, [["markQueued", 0], ["release", 0], ["takeQueued"], ["acquire"],
                                    ["markQueued", 0], ["markQueued", 0], ["release", 0], ["takeQueued"],
                                    ["markQueued", -1], ["release", -1], ["release", 3], ["state", -1],
                                    ["state", 3], ["state", 99]]],
        "release while queued": [3, [["acquire"], ["markQueued", 0], ["release", 0], ["takeQueued"], ["acquire"],
                                     ["markQueued", 0], ["takeQueued"]]],
        "release while capturing": [2, [["acquire"], ["release", 0], ["acquire"], ["markQueued", 0],
                                        ["takeQueued"], ["release", 0], ["release", 0]]],
        "requeued slot": [2, [["acquire"], ["markQueued", 0], ["takeQueued"], ["release", 0], ["acquire"],
                              ["markQueued", 0], ["takeQueued"]]],
        "stale queue entry": [2, [["acquire"], ["markQueued", 0], ["release", 0], ["acquire"], ["acquire"],
                                  ["markQueued", 1], ["takeQueued"], ["takeQueued"], ["markQueued", 0],
                                  ["takeQueued"]]],
        "one slot": [1, [["acquire"], ["acquire"], ["markQueued", 0], ["acquire"], ["takeQueued"], ["release", 0],
                         ["acquire"]]],
        "zero slots": [0, [["acquire"], ["acquire"], ["state", 0], ["markQueued", 0], ["takeQueued"]]],
        "negative slots": [-3, [["acquire"], ["acquire"]]],
        "five slots, the capture's ring": [5, [["acquire"]] * 6 + [["markQueued", i] for i in range(5)]
                                           + [["takeQueued"]] * 6],
    }
    crops = []
    for width, height, stride in ((3840, 2160, 15360), (1920, 1080, 7680), (1920, 1080, 7936), (4, 3, 16),
                                  (4, 3, 20), (4, 3, 15), (0, 3, 0), (4, 0, 16), (5, 3, 20), (-4, 3, 16)):
        for crop in ([0, 0, 0, 0], [0, 0, width, height], [320, 240, 1280, 960], [0, 0, width, 1],
                     [1, 1, 2, 1], [0, 1, 4, 2], [-1, -1, 3, 3], [2, 1, 10, 10], [width, 0, 4, 4],
                     [0, 0, -2, 2], [1, 0, 1, 1], [0, 2, 4, 1]):
            crops.append({"size": [width, height], "stride": stride, "crop": crop})
    headers = {"sizes": [[1920, 1080], [3840, 2160], [2, 2], [0, 0], [-2, 4], [65536, 1], [1281, 721]],
               "fps": [60, 30, 1, 0, -5, 7, 144, 1000000000, 2147483647]}
    frames = {"timestamps": [0, 1, 16667, 1000000, -1, -9223372036854775807, 72057594037927935,
                             9223372036854775807],
              "bytes": [0, 1, 32, 8294400, 33177600, 4294967296, -4]}
    encoders = []
    for size in ([440, 234], [1280, 720], [1224, 1224], [1225, 1225], [1920, 1080], [2560, 1440], [3840, 2160],
                 [7680, 4320], [2, 2]):
        for fps in (60, 30, 144, 1, 240):
            for preferred, gpu in ((False, False), (True, False), (True, True), (False, True)):
                encoders.append({"size": size, "fps": fps, "preferred": preferred, "gpuConversion": gpu})
    for conversion in ("cpu", "CPU", "gpu", ""):
        for lossless in ("lossless", "Lossless", ""):
            for preferred, gpu in ((True, True), (False, False)):
                encoders.append({"size": [1920, 1080], "fps": 60, "preferred": preferred, "gpuConversion": gpu,
                                 "conversion": conversion, "encoder": lossless})
    trims = []
    for width, height in ((3840, 2160), (1921, 1081), (2, 2), (1, 1)):
        for crop in ([0, 0, 0, 0], [0, 0, width, height], [10, 20, 641, 361], [10, 20, 640, 360],
                     [width - 5, height - 5, 100, 100], [-3, -3, 10, 10], [width, height, 10, 10],
                     [0, 0, 1, 1], [0, 0, 2, 3], [1, 1, -4, 4], [5, 5, 0, 7]):
            trims.append({"size": [width, height], "crop": crop})
    writer = {
        "steady 60": {"fps": 60, "size": [4, 2], "times": [1000000 + round(i * 1e6 / 60) for i in range(6)],
                      "stop": 1000000 + 100000},
        "still screen": {"fps": 30, "size": [4, 2], "times": [5000000], "stop": 7000000},
        "stop before the last tick": {"fps": 30, "size": [4, 2], "times": [5000000, 5033333], "stop": 5040000},
        "stop on the last frame": {"fps": 30, "size": [4, 2], "times": [5000000, 5033333], "stop": 5033333},
        "no stop time": {"fps": 30, "size": [4, 2], "times": [5000000, 5100000], "stop": 0},
        "same tick twice": {"fps": 60, "size": [4, 2], "times": [1000000, 1008000, 1008334, 1016667, 1024000],
                            "stop": 1050000},
        "out of order": {"fps": 60, "size": [4, 2], "times": [2000000, 2050000, 2030000, 2100000, 1990000],
                         "stop": 2200000},
        "gaps": {"fps": 10, "size": [2, 2], "times": [0, 1, 250000, 2000000, 2049999, 2050000], "stop": 3000000},
        "first frame at zero": {"fps": 60, "size": [2, 1], "times": [0, 16667], "stop": 1000},
        "cropped": {"fps": 30, "size": [6, 3], "crop": [2, 1, 2, 2], "times": [1000, 34333, 67666], "stop": 100000},
        "cropped one row": {"fps": 30, "size": [6, 3], "crop": [0, 2, 6, 1], "times": [1000, 34333], "stop": 0},
        "padded stride": {"fps": 30, "size": [3, 2], "stride": 16, "times": [1000, 34333], "stop": 70000},
        "odd fps": {"fps": 7, "size": [2, 2], "times": [100, 142957, 285814, 400000], "stop": 1000000},
        "high fps": {"fps": 1000, "size": [2, 1], "times": [10, 1010, 1510, 2010, 2499, 2500], "stop": 5000},
        "many frames": {"fps": 60, "size": [2, 1], "times": [round(i * 1e6 / 60) + 7 for i in range(40)],
                        "stop": round(41 * 1e6 / 60)},
        "nothing captured": {"fps": 60, "size": [4, 2], "times": [], "stop": 5000000},
        "timestamp unwritable": {"fps": 60, "size": [4, 2], "times": [1000, 2000], "stop": 5000,
                                 "timestampPath": "missing-directory/screen.mp4.ts"},
        "encoder closed": {"fps": 60, "size": [4, 2], "times": [1000, 20000], "stop": 50000, "encoder": "closed"},
        "aborted": {"fps": 60, "size": [4, 2], "times": [1000, 20000], "stop": 50000, "abort": True},
    }
    write(out / "cases.json", json.dumps({
        "rings": rings, "crops": crops, "streamHeaders": headers, "frameHeaders": frames,
        "encoders": encoders, "trims": trims, "writer": writer,
    }, indent=1) + "\n")


def make_launcher() -> None:
    out = CORPUS / "launcher"
    reset(out)
    names = ["Cam Link 4K: Cam Link 4K", "Integrated Webcam: Integrated W", "Integrated Webcam: Integrated Webcam",
             "Insta360 Link 2: Insta360 Link 2", "Insta360 Link 2: Insta360 Link 2 Control",
             "HD Pro Webcam C920", "USB Camera: USB Camera: Video", ": stream", "device: ", "device:  ",
             "a: b", "Camera :Camera", "  Spaced  :  Spaced  ", "Laptop Camera: Laptop", "Laptop: Laptop Camera",
             "", ":", ": ", "x: x: x", "Cámara: Cám", "Cam: cam", "Cam:Cam"]
    audio = "gpu-screen-recorder/list-audio-devices"
    files = sorted(p.name for p in (CORPUS / audio).iterdir())
    audio_cases = []
    for file in files:
        for chosen, current in (("default_input", "default_input"),
                                ("alsa_input.usb-Blue_Yeti-00.analog-stereo", "default_input"),
                                ("alsa_input.unplugged", "alsa_input.unplugged"),
                                ("alsa_input.pci-0000_00_1f.3.analog-stereo", "default_input")):
            audio_cases.append({"file": f"{audio}/{file}", "chosen": chosen, "current": current, "status": 0})
    audio_cases += [{"file": f"{audio}/crafted-mixed.txt", "chosen": "default_input", "current": "alsa_input.x",
                     "status": 1},
                    {"file": None, "chosen": "alsa_input.y", "current": "alsa_input.y", "status": 0}]

    def camera(id_, description, formats=None):
        return {"id": id_, "description": description,
                "formats": formats if formats is not None else [["Jpeg", 1920, 1080, 5, 30]]}

    ir = [["Y8", 640, 360, 15, 15]]
    stable = {"/dev/video0": "/dev/v4l/by-id/usb-Laptop-video-index0", "/dev/video2": "/dev/v4l/by-path/usb-0:1:1.2",
              "/dev/video4": "/dev/v4l/by-id/usb-Insta360_Link_2-video-index0",
              "/dev/video6": "/dev/v4l/by-id/usb-Insta360_Link_2-video-index0",
              "/dev/video8": "/dev/video8"}
    laptop = [camera("/dev/video0", "Integrated Webcam: Integrated W"), camera("/dev/video1", "Integrated Webcam: Integrated I", ir),
              camera("/dev/video4", "Insta360 Link 2: Insta360 Link 2")]
    twins = [camera("/dev/video4", "USB Camera: USB Camera"), camera("/dev/video6", "USB Camera: USB Camera")]
    webcams = {
        "laptop, chosen plugged in": {"cameras": laptop, "device": "/dev/v4l/by-id/usb-Laptop-video-index0",
                                      "chosen": "/dev/v4l/by-id/usb-Insta360_Link_2-video-index0"},
        "laptop, chosen unplugged": {"cameras": laptop[:2], "device": "/dev/v4l/by-id/usb-Insta360_Link_2-video-index0",
                                     "chosen": "/dev/v4l/by-id/usb-Insta360_Link_2-video-index0"},
        "laptop, current listed": {"cameras": laptop, "device": "/dev/v4l/by-id/usb-Laptop-video-index0",
                                   "chosen": "/dev/v4l/by-id/usb-Missing"},
        "laptop, nothing listed": {"cameras": laptop, "device": "/dev/video9", "chosen": ""},
        "twins share a name": {"cameras": twins, "device": "/dev/v4l/by-id/usb-Insta360_Link_2-video-index0",
                               "chosen": ""},
        "duplicate node": {"cameras": [camera("v4l2 /dev/video0 a", "First: First"),
                                       camera("/dev/video0", "Second: Second")],
                           "device": "", "chosen": ""},
        "ids without nodes": {"cameras": [camera("obs-virtual", "OBS Virtual Camera"), camera("", "Nothing")],
                              "device": "/dev/video2", "chosen": "/dev/video2", "exists": ["/dev/video2"]},
        "only an ir sensor": {"cameras": [camera("/dev/video1", "IR: IR", ir)], "device": "/dev/video2",
                              "chosen": "/dev/video2"},
        "no cameras, saved path exists": {"cameras": [], "device": "/dev/video2", "chosen": "/dev/video2",
                                          "exists": ["/dev/video2"]},
        "no cameras, saved path missing": {"cameras": [], "device": "/dev/video2", "chosen": "/dev/video2"},
        "no cameras, chosen elsewhere": {"cameras": [], "device": "/dev/video2", "chosen": "/dev/video8",
                                         "exists": ["/dev/video2"]},
        "three of a name": {"cameras": [camera("/dev/video0", "Cam: Cam"), camera("/dev/video2", "Cam: Cam"),
                                        camera("/dev/video8", "Cam"), camera("/dev/video4", "Other: Other")],
                            "device": "/dev/video8", "chosen": "/dev/video8"},
        "stable path maps two nodes together": {"cameras": [camera("/dev/video4", "A: A"), camera("/dev/video6", "B: B")],
                                                "device": "", "chosen": "/dev/v4l/by-id/usb-Insta360_Link_2-video-index0"},
    }
    write(out / "cases.json", json.dumps({"cameraNames": names, "audio": audio_cases, "stable": stable,
                                          "webcams": webcams}, indent=1, ensure_ascii=False) + "\n")


def make_recording_side() -> None:
    make_tool_answers()
    make_recorder()
    make_regionpicker()
    make_captureexclusion()
    make_camera()
    make_microphone()
    make_screencapture()
    make_launcher()


def main() -> None:
    os.umask(0o022)
    make_numbers()
    make_inputlog()
    make_qjson()
    make_qcolor()
    make_motion()
    make_project()
    make_themes()
    make_timelines()
    make_keystrokes()
    make_motiontrack()
    make_recordingpreferences()
    make_mediaprobe()
    make_composition()
    make_recording_side()


if __name__ == "__main__":
    main()
