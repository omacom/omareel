# Golden fixtures

What the C++ code at the tag `rust-migration-base` (commit `f3c9a1b`) produces over a corpus of
inputs, recorded by `tools/oracle` so the Rust port in `crates/omareel_core` can be tested against
it byte for byte. See `docs/rust-migration.md` for how they are used: once a C++ file is deleted,
its goldens are what is left of it, so **never regenerate them from new code**, and change them
only in a commit that says why.

| Path | What |
|---|---|
| `corpus/` | Inputs. Committed; the oracle only reads them. |
| `expected/` | Outputs (about 9.2 MB, 2.3 MB compressed). Committed; written only by `tools/oracle/generate-golden.sh`. |
| `shims/` | Stand-ins for `fc-match` and `hyprctl`, first on `PATH` while the oracle runs. |
| `.gitattributes` | `-text` everywhere: BOMs, CRLF and NUL bytes in these files are on purpose. |

`expected/qt-version.txt` holds the Qt version the oracle ran against (`qVersion()` and a newline;
6.11.2 for the current goldens). Qt's behaviour is the reference, so a Qt upgrade that changes
any output must be looked at before the goldens are regenerated.

## Regenerating

```
tools/oracle/generate-golden.sh            # rewrite expected/ from the tag
tools/oracle/generate-golden.sh --check    # compare instead; exit 1 on any difference
```

The script refuses to run unless the tag exists, on a machine other than x86_64 (pass
`--allow-arch` to experiment; do not commit the result), or as root (the project family records
permission errors). It exports the tag's `src/` with `git archive rust-migration-base src`, builds
`tools/oracle` against it (`-ffp-contract=off`, C++17, Release), and runs
`omareel-oracle tests/golden/corpus <out>` under `env -i` with only: fresh empty `HOME`,
`XDG_CONFIG_HOME`, `XDG_CACHE_HOME`, `XDG_STATE_HOME`, `XDG_DATA_HOME`, `XDG_RUNTIME_DIR` (0700)
and `TMPDIR`, all siblings in one scratch directory; `TZ=UTC`, `LC_ALL=C`, `LANG=C`,
`QT_QPA_PLATFORM=offscreen`, `PATH=tests/golden/shims:/usr/bin:/bin`, `OMAREEL_ORACLE_HERMETIC=1`
(the oracle refuses to run without it, because it writes under `$HOME`); `umask 022`. The new tree
is swapped in with a rename, so `expected/` is never half-written. Running it twice must leave
`git status` clean.

The oracle's working directory is `$HOME` for every family, and its scratch files live under
`$HOME/oracle-scratch/<family>/`.

### Where the corpus came from

- `corpus/themes/<name>/`: every theme of Omarchy 4.0.0.alpha (`corpus/themes/OMARCHY_VERSION`),
  copied by `tools/oracle/import-omarchy-themes.sh`. These files come from Omarchy
  (<https://github.com/basecamp/omarchy>, MIT licence, © David Heinemeier Hansson).
  `colors.toml` is the theme's own file. No theme ships a `shell.toml`; Omarchy generates one into
  `~/.local/state/omarchy/current/theme/` from `default/themed/shell.toml.tpl` and the theme's
  `shell.<section>.toml` overrides, so the script runs Omarchy's own
  `omarchy-theme-set-templates` in a scratch `HOME` and keeps the result. `theme.name` holds the
  directory name and a newline, as Omarchy writes it.
- `corpus/project/history-NN-<date>-<commit>.json`: the `project.json` that
  `Project::defaults("Recording NN", 8.25).save()` wrote at every commit up to the tag that changed
  `src/core/Project.cpp`, each built from that commit's `Project.h`/`.cpp`
  (`tools/oracle/make-historical-projects.sh`). These are the real historical shapes: the
  JSON-backed phase 1 (`opacity`, `offsetY`, `smoothing: 0.8`), `inset`, camera `crop`, defaults
  versions 2, 3 and 4.
- `corpus/inputlog/fixture-input.jsonl` and `corpus/inputlog/bundles/synthetic/` are copies of the
  tag's `tests/fixtures/input.jsonl` and `tests/fixtures/synthetic/` (640x360, 60 fps, 8 s).
- Everything else is written by `tools/oracle/make-corpus.py` (fixed seeds; rerunning it must not
  change anything). Read it for what each crafted file is for; the file names say it too.

## Formatting rules

Every `.txt` file is UTF-8 with LF line endings, and every line ends with `\n`, the last one
included. A line is one row; its cells are joined by a single TAB, and no cell contains a TAB or a
newline. A line `== <label>` starts a section; the rows after it belong to it until the next
`== ` line. Rows and sections appear in exactly the order this document gives; when a list comes
from iterating a corpus directory, files (or directories) are taken in byte-wise order of their
UTF-8 names, hidden ones included; when it comes from a JSON object in the corpus, in the order
`QJsonObject` iterates (keys in ascending UTF-16 code unit order).

The cell formats, named in the per-file descriptions below:

- **num**: a double, printed by C's `printf("%.17g")` in the C locale (glibc), except that every
  NaN is `nan` (sign and payload dropped), +∞ is `inf` and −∞ is `-inf`. `%.17g` means 17
  significant digits, correctly rounded; exponent style (`d.dddde±XX`, at least two exponent
  digits) when the decimal exponent is below −4 or at least 17, fixed style otherwise; then trailing
  zeros and a trailing `.` are removed. Negative zero is `-0`. Examples: `0.10000000000000001`,
  `1`, `10000000000000000`, `1e+17`, `1.0000000000000001e-05`, `4.9406564584124654e-324`. It
  round-trips every finite double. Float values (`QColor::alphaF()` and friends) are widened to
  double first.
- **hex**: the IEEE-754 bit pattern of a double as 16 lowercase hex digits, most significant
  first (`3ff0000000000000` is 1.0, `8000000000000000` is −0.0).
- **int**: decimal, `-` for negatives. **bool**: `true` or `false`.
- **str** (a `QString`): `"` + contents + `"`. The UTF-16 string is decoded into code points; an
  unpaired surrogate stands for itself. `"` becomes `\"`, `\` becomes `\\`. These are written as
  `\u{X}` with X the code point in lowercase hex without leading zeros: U+0000–U+001F, U+007F,
  U+0080–U+009F, U+2028, U+2029, U+FEFF, U+FFFE, U+FFFF and unpaired surrogates (`\u{d800}`).
  Everything else is its UTF-8 encoding. A null and an empty `QString` both print `""`.
- **bytes**: raw bytes as `"` + contents + `"`: bytes 0x20–0x7E stay (except `"` → `\"` and
  `\` → `\\`), every other byte is `\xNN` (lowercase hex).
- **strlist** (a `QStringList`): `[` + the items as **str** joined by `,` + `]`; `[]` when empty.
- **color** (a `QColor`): `<spec> <rgba64> <argb>` separated by single spaces. `<spec>` is the
  `QColor::Spec` name (`Invalid`, `Rgb`, `Hsv`, `Cmyk`, `Hsl`, `ExtendedRgb`); `<rgba64>` is
  `rgba64()` as four 4-digit lowercase hex numbers, red green blue alpha, with no separators;
  `<argb>` is `name(QColor::HexArgb)`. For `ExtendedRgb` four more space-separated **num** follow:
  `redF() greenF() blueF() alphaF()`.
- **json**: the exact bytes of `QJsonDocument::toJson(QJsonDocument::Compact)` (one line; Qt
  escapes control characters, so no TAB or newline occurs). **wrapped**: a single `QJsonValue` as
  the **json** of `QJsonDocument(QJsonArray{value})`, so `[0.5]`, `["auto"]`, `[null]`.
- **typed** (a `QJsonValue`), nine cells: `<type>`, `toInt(-7)` int, `toInteger(-7)` int,
  `toDouble(-7)` num, `toBool(false)` bool, `toString("<def>")` str,
  `toVariant().typeName()` (or `<invalid>` when it is null), `toVariant().toString()` str,
  `toVariant().toLongLong()` int. `<type>` is the `QJsonValue::Type` name: `Null`, `Bool`, `Double`,
  `String`, `Array`, `Object`, `Undefined`. The variant type tells how Qt stored a number:
  `qlonglong` for integers (Qt keeps integral values that fit in 64 bits as integers, even
  `QJsonValue(2.0)`), `double` otherwise.
- **path**: a path or message with the hermetic directories put back as variables: occurrences of
  the values of `HOME`, `XDG_CONFIG_HOME`, `XDG_CACHE_HOME`, `XDG_STATE_HOME`, `XDG_DATA_HOME`,
  `XDG_RUNTIME_DIR`, `TMPDIR` (as set when the oracle started) and of the corpus directory become
  `$HOME`, `$XDG_CONFIG_HOME`, ..., `$CORPUS`. The longest value wins, and a value only matches
  when the character after it is not a letter, digit, `.`, `_` or `-`. The result is then a **str**.

Bytes files (`*.json` under `qjson/docs`, `qjson/built`, `project/load`, `project/tojson`,
`project/defaults`) hold exactly the bytes Qt produced, with nothing added.

## qjson: QJsonDocument and QJsonValue

Inputs: `corpus/qjson/docs/*.json` (whole documents as raw bytes), `corpus/qjson/number-literals.txt`
and `corpus/qjson/string-literals.txt` (one literal per line), `corpus/numbers/doubles.txt` and
`corpus/numbers/int64.txt`.

- `parse.txt`: one row per file in `corpus/qjson/docs`, `<name>` being the file name without
  `.json`. `QJsonDocument::fromJson(bytes, &error)`; on success
  `<name> ok <kind> <error.offset int> <error.errorString() str>` with kind `object`, `array` or
  `null`; on failure `<name> error <error.error int> <error.offset int> <error.errorString() str>`.
  Qt 6.11 accepts only an object or an array at the top level.
- `docs/<name>.compact.json`: `toJson(Compact)` of every document that parsed.
- `docs/<name>.indented.json`: `toJson(Indented)`, only for documents nested at most 64 deep
  (a scalar has depth 0, a container 1 + the deepest child, an empty container 1). The
  `deep-*-1024` probes of Qt's nesting limit get only their compact file and `parse.txt` row.
- `typed.txt`: a section `== <name>` for every document with an indented file, then one row per
  value in pre-order, the root first: `<pointer str> <typed>`. The pointer is `""` for the root
  and the parent's pointer + `/` + the key (`~` written `~0`, `/` written `~1`) or the decimal
  array index. Object members come in `QJsonObject` iteration order, which is ascending UTF-16
  code unit order of the keys (see `key-order`); duplicate keys keep the last value.
- `number-literals.txt`, `string-literals.txt`: one row per line of the corpus file (split on LF,
  the empty piece after the final LF dropped). Each literal is parsed as the document
  `[` + literal + `]`. Row: `<line number int, from 1> <literal bytes>` and then either
  `error <error.error int> <error.offset int> <errorString str>` (the offset counts the added `[`)
  or `ok <array size int> <json of the document> <typed of element 0>` (the typed cells are
  missing when the array is empty).
- `doubles.txt`: one row per double of `corpus/numbers/doubles.txt`, in file order:
  `<hex> <wrapped of QJsonValue(d)> <type of element 0 after parsing that text back>
  <its toVariant().typeName()> <hex of its toDouble(-7)>`. NaN and the infinities are written
  `[null]`.
- `int64.txt`: one row per value of `corpus/numbers/int64.txt`:
  `<value int> <wrapped of QJsonValue(qint64)> <variant typeName after parsing back>
  <toVariant().toLongLong() int> <hex of toDouble(-7)>`.
- `built/<name>.indented.json`, `built/<name>.compact.json`: documents built in C++ by
  `builtDocuments()` in `tools/oracle/cases/qjson.cpp`: `scalars` (inserted 2.0, −0.0, int,
  qint64 extremes, 2^53+1, large doubles, `double(0.1f)`, NaN, ±∞, bools, null, strings),
  `undefined-in-object` (inserting `Undefined` removes the key, including one that existed),
  `undefined-in-array`, `undefined-initializer-list`, `nested` (insert, replace, remove, array
  insert/replace/prepend/removeAt), `strings` (control characters, NUL, unpaired surrogates in
  values and keys, non-BMP keys), `latin1-keys` (`QLatin1String` and `QString` keys mixed),
  `numbers` (a `QJsonArray` of doubles and qint64s), `from-variant-map`
  (`QJsonValue::fromVariant(QVariantMap)` with every variant type), `null-document`
  (`QJsonDocument()`: both files are empty), `empty-object`, `empty-array`, `empty-nested`.
- `built-typed.txt`: like `typed.txt`, for every built document.
- `values.txt`: `<label> <typed> <wrapped>` for single values built in C++ (`builtValues()` in
  `qjson.cpp`); the label says how each was made, for example `double 2^63` or
  `fromVariant(float 0.1)`.

## qnumber: QString::number, QByteArray::number, QString::toDouble/toInt/toLongLong

- `number.txt`: for every double of `corpus/numbers/doubles-format.txt` (a 1297-value subset of
  `doubles.txt`), three rows, for the formats `f`, `e` and `g`:
  `<hex> <format> <r0> <r1> <r2> <r3> <r6> <r8> <r17> <rShortest>`, where `<rP>` is
  `QString::number(d, format, P)` as raw UTF-8 and Shortest is
  `QLocale::FloatingPointShortest` (−128).
- `bytearray-shortest.txt`: every double of `doubles.txt`:
  `<hex> <QByteArray::number(d, 'g', QLocale::FloatingPointShortest)> <QString::number(d)>`
  (the second is the default `'g'`, precision 6).
- `parse.txt`: every string of `corpus/numbers/strings.json`: `<s str> <toDouble num> <toDouble hex>
  <ok> <toInt int> <ok> <toLongLong int> <ok> <s.toUtf8().toInt(&ok, 0) int> <ok>`, each `<ok>`
  being `ok` or `fail`. The base-0 conversion is the one `qEnvironmentVariableIntValue` uses.

## qround: qRound, qRound64, qFuzzyCompare, qFuzzyIsNull, clamping

- `round.txt`: `<hex> <num> <qRound int or -> <qRound64 int or ->`. qRound is evaluated when the
  value is finite and `|d| < 2147483647`, qRound64 when finite and `|d| < 9.2e18`; outside
  that Qt asserts or overflows, so the cell is `-`. On x86_64 Qt computes
  `int(d + copysign(0.5, d))` (truncation), not `round()`: `0.49999999999999994` rounds to 1.
  Inputs, in order: every double of `doubles.txt`; then `i/4` for `i = -400..400`, each odd `i`
  followed by `nextafter(i/4, -inf)` and `nextafter(i/4, +inf)`; then each of 2147483646.5,
  2147483646.4999998, −2147483647.5, 4503599627370495.5, 4503599627370496.5,
  9007199254740991.0, 4611686018427387904.5 followed by its negation.
- `fuzzy-compare.txt`: `<hex a> <hex b> <qFuzzyCompare(a, b) bool>` for all ordered pairs of 36
  values (`fuzzyValues` in `tools/oracle/cases/numbers.cpp`; the hex columns list them).
- `fuzzy-is-null.txt`: `<hex> <qFuzzyIsNull bool>` for the same values.
- `bound.txt`: for x in NaN, −NaN, −∞, −1, −0.5, −0, 0, 1e−300, 0.25, 0.5, 0.75, 1, the double
  after 1, 1.5, +∞: `<hex x>` and then the hex of `qBound(0.0, x, 1.0)`, `std::clamp(x, 0.0, 1.0)`,
  `qMax(0.0, x)`, `qMax(x, 0.0)`, `qMin(x, 1.0)`, `qMin(1.0, x)`, `std::max(0.0, x)`,
  `std::max(x, 0.0)`, `std::min(x, 1.0)`, `std::min(1.0, x)`, `std::clamp(x, -0.0, 0.0)`,
  `qBound(-0.0, x, 0.0)`. These differ for NaN and signed zeros, and the core uses all of them.

## qcolor: QColor

- `parse.txt`: every string of `corpus/qcolor/parse.json` (all SVG names, case and space
  variants, hex forms of every length, invalid ones): `<s str> <isValid bool> <color of QColor(s)>
  <name() raw> <redF greenF blueF alphaF as four num joined by spaces>
  <QColor::isValidColorName(s) bool>`.
- `color-names.txt`: `<name str> <color>` for every entry of `QColor::colorNames()`, in Qt's order.
- `lighter-darker-grid.txt`: for r, g, b each in 0, 17, 34, ... 255 (r outermost, then g, then b)
  and each factor of 50, 100, 101, 106, 108, 110, 112, 115, 118, 120, 150, 200, 300:
  `<QColor(r,g,b).name()> <factor> <lighter rgba64> <lighter name()> <darker rgba64>
  <darker name()>`, rgba64 being the 16 hex digits of **color**. (Two more cells with the spec
  names would follow if a result were not `Rgb`; none is.)
- `lighter-darker-special.txt`: `<input color> <factor> <lighter color> <darker color>` for the
  factors −10, 0, 1, 99 and the list above, over: the colours parsed from `#7aa2f7`, `#1a1b26`,
  `#a9b1d6`, `#faf4ed`, `#80ff0000`, `#00000000`, `#123456789abc`, `#fff000fff`, `#ffffff`,
  `#000000`, `#808080`, `#ff0000`, `#00ff00`, `#0000ff`, `#010101`, `#fefefe`;
  `fromRgbF(0.1f, 0.2f, 0.3f, 0.4f)`, `fromRgbF(1.2f, -0.1f, 0.5f)` (ExtendedRgb),
  `fromHsv(200, 128, 250)`, `fromHsl(10, 200, 100)`, `fromCmyk(10, 20, 30, 40)`, `QColor()`; and 64
  `QColor::fromRgba64` colours whose channels the input cell gives.
- `set-alpha-f.txt`: `QColor("#7aa2f7")` after `setAlphaF(a)` (a float parameter: the double is
  converted first, as Theme does): `<a hex> <a num> <alpha() int> <rgba64().alpha() int>
  <alphaF() num> <color>`. Values: i/1000 for i = 0..1000, i/255 for i = 0..255, the alphas
  Theme uses, a few tiny and out-of-range ones and NaN.
- `from-rgb-f.txt`: `<r num> <g num> <b num> <a num> <color of QColor::fromRgbF(r, g, b, a)>
  <redF greenF blueF alphaF>`; the arguments are floats, so the printed doubles are converted
  first. Values out of [0, 1] give ExtendedRgb.

## easing: Omareel::Easing::easeInOutCubic

- `ease-in-out-cubic.txt`: `<t hex> <t num> <result num> <result hex>`. Inputs in order: i/4096 for
  i = −1024..5120; i/1000 for i = 0..1000; the doubles below and above 0.5, 0 and 1; −0, 1e−300,
  5e−324, 0.49999999999999994, 1e300, −1e300, +∞, −∞, NaN; then the values of
  `doubles-format.txt` within [−0.5, 1.5]. It uses glibc's `pow`.

## inputlog: InputLog

- `cases.txt`: one section per call. `== <label>`, then `error <path>` (the error string, which the
  oracle sets to `<untouched>` before the call), `count <n>`, n event rows, and
  `moves <indices>`, `clickDowns <indices>` (`clickDowns(false)`), `clickDowns(leftRightOnly)
  <indices>` (`clickDowns(true)`), `keyCount <int>`. An event row is
  `<monotonicUs int> <time num> <kind> <position.x num> <position.y num> <button str> <dx num>
  <dy num> <keyCode int> <keyName str> <modifiers strlist>`, kind being `Move`, `ButtonDown`,
  `ButtonUp`, `Scroll`, `KeyDown` or `KeyUp`. `<indices>` are the 0-based positions in the event
  list of what the accessor returned, joined by `,`, or `-` for none.

  The calls, in order: for every file in `corpus/inputlog/` (not the `bundles` directory),
  `load(<file>, first, duration)` with (first, duration) = (1000000, −1), (1000000, 0),
  (1000000, 0.5), (1000000, 2), (0, −1), (2000000, −1), labelled
  `load <file> first=<first> duration=<duration num>`; then `load` (first 1000000, duration −1) of
  `missing.jsonl`, of the `bundles` directory and of an empty path, labelled
  `load missing.jsonl ...`, `load <directory> ...`, `load <empty path> ...`; then for every
  directory in `corpus/inputlog/bundles/`, `loadBundle(<dir>, duration)` for duration −1, 3 and 0,
  labelled `loadBundle <dir> duration=<duration num>`; then `loadBundle` of a missing bundle and
  of `synthetic/` with a trailing slash (duration −1).

## cursorpath: CursorPath

The sample sets: the crafted ones in `corpus/cursorpath/cases.json` `samples` (each sample a
`[time, x, y]` triple), in key order, then `synthetic-normalized` and `synthetic-pixels`: the
moves of `InputLog::loadBundle(corpus/inputlog/bundles/synthetic, -1)` as
`CursorSample{time, position}`, with the position divided by (640, 360) for the normalised set,
as MotionTrack does.

- `decimate.txt`: for each set and each `[fps, minDistance]` of `cases.json` `decimate`:
  `<set> <fps num> <minDistance num> <result size int> <indices>`, the indices being the 0-based
  positions in the input of the samples `decimate` returned, joined by `,` (`-` when empty).
  After those, one row per set for the default argument: `<set> 60 default <size> <indices>`.
- `position-at.txt`: `<set> <time num> <x num> <y num>` of `positionAt(samples, time)`: for the
  crafted sets at every time of `cases.json` `positionAt`; for the synthetic sets at i/120,
  i = −12..972. Last, set `synthetic-normalized-decimated` (`decimate(synthetic-normalized, 60,
  1/640)`, MotionTrack's input) over the same grid.

## zoomtimeline: ZoomTimeline

Crafted segments in `corpus/zoomtimeline/cases.json` are `{id, start, end, level, auto, x, y}`
for `ZoomSegment{id, start, end, level, automaticTarget = auto, target = (x, y)}`. A segment row is
`<id str> <start num> <end num> <level num> <automaticTarget bool> <target.x num> <target.y num>`.

- `generate.txt`: for each case of `cases.json` `generate` (key order) and each of its durations,
  `== <case> duration=<num>` and the segments of `generate(clicks, duration)`. Then the row
  `synthetic clicks <time num>...` (the times of the synthetic bundle's `clickDowns(true)`), and
  sections `== synthetic duration=8`, `5` and `3` for those clicks.
- `target-at.txt`: sections `== segments=<S> samples=<P> snapToEdgesRatio=<R>`, each with rows
  `<time num> <scale num> <center.x num> <center.y num>` of `targetAt(segments, samples, time,
  style)` for time = i/30, i = −15..255. The sample sets are `synthetic`
  (`decimate(synthetic-normalized, 60, 1/640)`), and `random-walk-normalized`, `empty`, `single`
  from `corpus/cursorpath/cases.json`. In order: S = `synthetic-generated`
  (`generate(synthetic clicks, 8)`) with P = `synthetic` for every ratio of `cases.json`
  `snapRatios` (R is the ratio as num; the rest of `ZoomStyle` default); every crafted segment set
  with every sample set and a default `ZoomStyle` (R = `default`); then
  `== segments=synthetic-generated samples=synthetic style=<omitted>`, the call without its style
  argument; then S = `project:<file>` for `crafted-edited.json`, `crafted-clamps.json` and
  `crafted-wrong-types.json` of `corpus/project/`, using the zooms and zoom style of
  `Project::fromJson` of that file, with P = `synthetic` and R its `snapToEdgesRatio`.
- `clamp-center.txt`: `<x num> <y num> <level num> <result x num> <result y num>` of
  `clampCenter(point, level)` for every point and level of `cases.json` `clampCenter`.
- `target-to-json.txt`: `<set> <id str> <wrapped of targetToJson(segment)>` for every segment of
  `synthetic-generated` and the crafted sets, then four segments built in C++ with set `explicit`:
  automatic with a target, and manual ones with (−0, 1e−300), (1e21, 123456789.125) and (NaN, ∞).

## project: Project

- `load.txt`: one row per call of `Project::load(path, &error)`, the error string set to
  `<untouched>` before: `<label> <rewritten|unchanged> <error path> <mode>`. `rewritten` means the
  file's inode or bytes changed (load saves migrated projects with `QSaveFile`, which replaces the
  file); `<mode>` is the file's permission bits after the call as 4-digit octal, missing when the
  file does not exist. First every file of `corpus/project/` (label: the file name), each copied to
  `$HOME/oracle-scratch/project/<file>/project.json` with mode 0644. Then:
  `special-missing-file.json` (no such file), `special-directory.json` (a directory),
  `special-unreadable.json` (mode 0000), `special-legacy-in-read-only-directory.json` (a
  migratable file in a 0500 directory: the save fails), `special-legacy-read-only-file.json`
  (migratable, mode 0440), `special-legacy-mode-0750.json` (migratable, mode 0750: the mode
  survives the rewrite), all under `$HOME/oracle-scratch/project/special/`; the migratable files
  are copies of `history-02-2026-09-02-96d74f9.json`.
- `load/<label>`: the file's bytes after `load`, only for the rows marked `rewritten`.
- `tojson/<label>`: `QJsonDocument(project.toJson()).toJson(Indented)` of what `load` returned
  (`Project()` when it failed).
- `fields.txt`: a section `== <label>` per `load` call with every member of the returned Project,
  so what `toJson()` drops (colour alpha and 16-bit channels, `Clip::isMedia()`) is checked too.
  Rows `<field> <value>`, in this order: `version` int, `defaultsVersion` int, `name` str, `aspect`
  str, `crop` (x y w h as num joined by spaces), `clips.size` int and per clip i
  `clips[i].id` str, `.in`, `.out`, `.speed` num, `.layout`, `.media`, `.fit` str, `.isMedia`
  bool; `zooms.size` and per zoom `zooms[i].id` str, `.start`, `.end`, `.level` num,
  `.automaticTarget` bool, `.target` (x y); `zoomStyle.spring.mass`, `.stiffness`, `.damping`,
  `zoomStyle.snapToEdgesRatio`, `zoomStyle.instantAnimation` bool, `zoomStyle.motionBlur`;
  `background.type` str, `background.wallpaper` str, `background.gradient.angle`,
  `background.gradient.stops.size` and per stop `background.gradient.stops[i].color` color,
  `.position`; `background.color` color, `background.image` str, `background.blur`;
  `frame.padding`, `frame.radius`, `frame.shadow.enabled` bool, `.intensity`, `.blur`,
  `.distance`, `.angle`, `frame.border.enabled` bool, `.width`, `.color` color, `.alpha`;
  `cursor.visible` bool, `cursor.size`, `cursor.smoothing` bool, `cursor.spring.mass`,
  `.stiffness`, `.damping`, `cursor.clickEffect` str, `cursor.clickShrink`,
  `cursor.rotateOnXMovementRatio`, `cursor.hideWhenIdleMs` int, `cursor.style` str,
  `cursor.ringColor` color, `cursor.clickSound` str; `keystrokes.enabled` bool,
  `keystrokes.position` str, `keystrokes.size`, `keystrokes.showOnlyShortcuts` bool,
  `keystrokes.holdMs` int; `audio.desktop` bool, `audio.mic` bool, `audio.volume`;
  `camera.enabled` bool, `camera.position` str, `camera.size`, `camera.shape` str,
  `camera.radius`, `camera.flipHorizontal` bool, `camera.rotation` int, `camera.shadow.*` and
  `camera.border.*` as for the frame, `camera.scaleDuringZoom`, `camera.offset` (x y),
  `camera.fullFrame` str; `exportSettings.format` str, `.fps` int, `.height` int, `.quality` str,
  `exportSettings.gif.fps` int, `.height` int, `.quality` str, `.loop` bool. Unmarked values are
  num.
- `migrate.txt`: per file of `corpus/project/`: when the bytes are not a JSON object,
  `<file> - <errorString str>`; otherwise `<file> <changed bool> <json of
  migrateDefaults(object, &changed)>` and then `<file> (again) <changed bool> same` for migrating
  the result once more (the json of the second result replaces `same` if it ever differs). A row
  `<file> (fromJson differs from load) <json>` would follow if `Project::fromJson` of the raw
  object disagreed with `load`; there is none.
- `save.txt`: `Project::defaults("Saved", 3.5).save(path, &error)`:
  `<label> <ok bool> <error path> <matches-toJson|differs|-> <mode or ->`, the fourth cell telling
  whether the file now holds `toJson(Indented)`. Labels: `new file`, `missing directory`,
  `path is a directory`, `read-only directory`, `empty path`.
- `defaults/<label>.json`: `toJson(Indented)` of `defaults()` (`no-arguments`),
  `defaults("Demo", 12.5)`, `defaults("", 0)` (`empty-name-0`), `defaults("Résumé 🎬 \"q\"", 1e-9)`
  (`unicode-tiny`), `defaults("x", -5)`, `defaults("Long", 86400.123456789)`,
  `defaults("NaN", NaN)` and `Project()` (`default-constructed`).
- `defaults-fields.txt`: the fields of those, as in `fields.txt`.
- `helpers.txt`: `allowedClipSpeeds <num>...`; `allowedClipLayouts <strlist>`;
  `allowedClipFits <strlist>`; `allowedAspects <strlist>`; `CurrentDefaultsVersion <int>`;
  `sanitizedMediaPath <in str> <out str>` rows; `springFromJson <input JSON text> <mass num>
  <stiffness num> <damping num> <json of springToJson(result)>` with the fallback
  `Spring{1.5, 250, 25}`; `springToJson <mass> <stiffness> <damping> <json>`; and
  `setJson(json()) of defaults("Via setJson", 2) <json>`.

## omarchypaths: OmarchyPaths

The oracle builds a fake Omarchy tree in the hermetic `$HOME` (`buildTree()` in
`tools/oracle/cases/omarchypaths.cpp`): themes in `$HOME/.config/omarchy/themes` and
`$HOME/.local/share/omarchy/themes` with backgrounds of mixed-case extensions, hidden files,
other extensions, a directory named like an image, a link to another theme's background, a broken
link, a theme directory that is a link, an empty theme, a file where `backgrounds` should be, a
hidden theme, non-ASCII names; and the state directory `$HOME/.local/state/omarchy/current` with
`theme.name`, a `background` link and `theme/backgrounds/`.

`themeBackgroundGroups()` also scans the hard-coded `/usr/share/omarchy/themes`, which the oracle
cannot redirect. That root is scanned last and nothing in the fake tree links into it, so the
oracle drops every path that does not lie inside a hermetic directory and every group left without
paths; what remains does not depend on the machine. A Rust port should take the system root as a
parameter and run these cases with it missing.

- `state.txt`: one section per state of the tree, `== <what changed>`, in the order of
  `runOmarchyPaths()` (each step changes the previous state). Rows: `stateRoot <path>`,
  `currentThemeName <str>`, `currentBackground <path>`; per group of `themeBackgroundGroups()`
  `group <name str> <label str> <current bool> <typeName of current> <the map's keys strlist>`
  followed by `  path <path>` rows (two spaces, then `path`); then `themeBackgrounds <path>` per
  entry of `themeBackgrounds()`.
- `recordings-directory.txt`: `<label> <recordingsDirectory() path>` under the
  `OMAREEL_RECENTS_DIR` values, `$XDG_CONFIG_HOME/user-dirs.dirs` contents and `XDG_CONFIG_HOME`
  settings the labels describe (`$HOME` inside a label is literal text from user-dirs.dirs or
  the hermetic home).
- `wallpaper-thumbnail-path.txt`: `<label> <input path> <identity path> <hash check> <result
  path>`. The identity is what the code hashes: `absoluteFilePath()` UTF-8, NUL, `size()`, NUL,
  `lastModified().toMSecsSinceEpoch()` (shown with `\u{0}` for the NULs; a missing file has size
  0 and time 0). The hash check is `hash=sha256(identity)` when the file name is the lowercase hex
  SHA-256 of the identity plus `.jpg`. When the identity contains a hermetic directory the hash is
  machine-dependent, so the result shows `{sha256(identity)}` in its place. Existing files are made
  with fixed sizes and modification times (`utimensat`).
- `generate-wallpaper-thumbnail.txt`: `<label> <returned bool> <thumbnail exists bool>
  <WxH of the thumbnail, or -> <its image format, or ->` for `generateWallpaperThumbnail`. The JPEG
  bytes depend on libjpeg and are not recorded.

## theme: Theme

For every directory of `corpus/themes/` and then of `corpus/themes-crafted/`, the oracle makes a
state directory holding the case's `theme.name`, `theme/colors.toml` and `theme/shell.toml` (those
it has), sets `OMAREEL_OMARCHY_STATE_DIR` to it and constructs a `Theme`.

- `cases.txt`: first the row `bundledUiFamily() <str>`. Then one section per `Theme`:
  `== themes/<name>`, `== themes-crafted/<name>`, then `== variant tokyo-night <label>` with the
  label's environment (`OMAREEL_THEME_RADIUS`, or the shims' variables below), then two
  `setFontFamilies` variants, then four `== reload: ...` sections for one Theme whose private
  `reload()` slot is invoked after changing its files and environment. A section may start with
  `environment <label>`. Then one row per Q_PROPERTY in `QMetaObject` order (the order of
  `Theme.h`): `<property> <value>`, the value being a color, str, int, bool, or for the
  `QVariantMap` properties (`font`, `space`, `type`) `{key=value,...}` with keys in `QVariantMap`
  order and int values. Then `controlFill(<f>,<h>,<s>) <color>`,
  `controlBorder(...) <color>` and `controlBorderWidth(...) <int>` for every combination of
  (focused, hot, selected), false before true, focused outermost.

`tests/golden/shims/hyprctl` answers `getoption decoration:rounding -j` with
`{"option": "decoration:rounding", "int": 10, "set": true }` unless `OMAREEL_ORACLE_HYPRCTL_OUTPUT`
replaces the output or `OMAREEL_ORACLE_HYPRCTL_EXIT` sets its exit status;
`tests/golden/shims/fc-match` answers `-f %{family[0]} monospace` with `Oracle Mono` unless
`OMAREEL_ORACLE_FC_MATCH` / `OMAREEL_ORACLE_FC_MATCH_EXIT` say otherwise. Both reject other
arguments with exit status 64.

## Machine dependence and what is not captured

- Float results are defined on x86_64. qRound there is add-half-and-truncate (ARM64 uses
  `round()`), and `pow` (Easing, Theme's luminance) comes from glibc.
- `fontFamily` is `sans-serif` because the oracle does not link the app's resources, so
  `bundledUiFamily()` finds no Inter and returns `""` (the app shows `Inter`). `monoFamily` and the
  radius come from the shims; on a desktop they come from fontconfig and Hyprland.
- `themeBackgroundGroups()` entries under `/usr/share/omarchy/themes` are dropped (see above).
- Thumbnail JPEG bytes are not recorded, only outcome and size.
- The permission cases assume an ordinary user; the error strings Qt builds can contain paths,
  which appear as `$HOME/oracle-scratch/...`.
- Theme's file watcher and its 220 ms reload timer need an event loop, which the oracle does not
  run; `reload()` is called directly instead.
- Not covered yet (later pull requests): ClipTimeline, CameraTimeline, LayoutTimeline,
  KeystrokeTrack, MotionTrack, RecordingMetadata and RecordingPreferences.
