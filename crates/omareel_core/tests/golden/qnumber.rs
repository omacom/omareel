//! qnumber: `compat::qnumber` against `QString::number`, `QByteArray::number` and
//! `QString::toDouble`/`toInt`/`toLongLong` (README "qnumber").

use crate::support::*;
use omareel_core::compat::qnumber::{self, FLOATING_POINT_SHORTEST};

const PRECISIONS: [i32; 8] = [0, 1, 2, 3, 6, 8, 17, FLOATING_POINT_SHORTEST];

#[test]
fn number_txt() {
    let mut out = Text::new();
    for value in read_doubles("numbers/doubles-format.txt") {
        for format in ['f', 'e', 'g'] {
            let mut cells = vec![hex(value), format.to_string()];
            cells.extend(PRECISIONS.map(|precision| qnumber::number_f64(value, format, precision)));
            out.row(cells);
        }
    }
    assert_golden("qnumber/number.txt", &out.into_string());
}

#[test]
fn bytearray_shortest_txt() {
    let mut out = Text::new();
    for value in read_doubles("numbers/doubles.txt") {
        out.row([
            hex(value),
            qnumber::byte_array_number(value, 'g', FLOATING_POINT_SHORTEST),
            qnumber::number_f64(value, 'g', 6),
        ]);
    }
    assert_golden("qnumber/bytearray-shortest.txt", &out.into_string());
}

fn ok_cell(ok: bool) -> String {
    if ok { "ok" } else { "fail" }.into()
}

/// `parse.txt` without its last two cells: the base-0 `QByteArray::toInt(&ok, 0)` that
/// `qEnvironmentVariableIntValue` uses has no Rust counterpart yet.
#[test]
fn parse_txt() {
    let mut out = Text::new();
    for item in read_json("numbers/strings.json").to_array() {
        let text = item.to_string_or("");
        let (double, double_ok) = qnumber::to_double_with_ok(&text);
        let int = qnumber::to_int(&text);
        let long = qnumber::to_long_long(&text);
        out.row([
            quote(&text),
            num(double),
            hex(double),
            ok_cell(double_ok),
            int.unwrap_or(0).to_string(),
            ok_cell(int.is_some()),
            long.unwrap_or(0).to_string(),
            ok_cell(long.is_some()),
        ]);
    }
    let expected: String = rows(&read_expected("qnumber/parse.txt"))
        .into_iter()
        .map(|cells| {
            assert_eq!(cells.len(), 10, "parse.txt rows have ten cells");
            cells[..8].join("\t") + "\n"
        })
        .collect();
    assert_same("qnumber/parse.txt (without the base-0 cells)", expected.as_bytes(), out.into_string().as_bytes());
}
