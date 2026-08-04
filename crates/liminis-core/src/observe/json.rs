//! The only place in the crate where a byte can break the format.
//!
//! Everything else under `observe/` is a caller of this file, which is why it is
//! a file: escaping, the ban on an interior newline, the printing of an `i128`
//! and the refusal of a non-finite `f64` are four rules that hold for the whole
//! stream, and a rule that lives at four call sites is a rule that holds at
//! three of them.
//!
//! # Hand-rolled, and why
//!
//! The workspace has `anyhow`, `blake3`, `clap`, `proptest`, `serde` and `toml`
//! and no JSON serializer, and ADR-037 rejected writing metrics into a database
//! as a dependency: "a run must start with one command without installing
//! anything (C-5)". The same argument covers a crate that would only be here to
//! print eight hundred integers a tick.
//!
//! # What this refuses to write
//!
//! **A newline inside a value.** `substance.id` is a free TOML string —
//! `Registry::new` checks duplicates and the count of thirty-one and nothing
//! about characters — so an id holding `\n` arrives here from a scenario file.
//! Unescaped, it splits one record into two lines that both look like NDJSON to
//! a line-oriented reader, and the truncation property that ADR-037 chose the
//! format for stops holding for the whole file rather than for that record. The
//! escaping here is structural, not cosmetic.
//!
//! **A non-finite number.** `NaN` and `inf` are what Rust prints and neither is
//! JSON. In `FLOAT` mode a non-finite `Q` is exactly what overflow looks like,
//! and `numeric::float`'s guard against it is a `debug_assert`, so in release
//! the value travels into a metric. A strict reader then aborts the file and a
//! lenient one skips the line — either way the one tick where the physics broke
//! is the one tick missing from the record of it. So [`Record::real`] returns an
//! error and writes nothing, and the caller decides whether that ends the run.

use anyhow::{Result, bail};

/// One NDJSON line under construction.
///
/// Built by [`Record::new`], filled by the typed setters, closed by
/// [`Record::finish`], which is also where the terminating newline comes from —
/// a caller that had to remember to append it would eventually not, and the last
/// record of a completed run would be indistinguishable from a torn one.
#[derive(Clone, Debug)]
pub struct Record {
    out: String,
}

impl Record {
    /// Open a record of the given kind: `{"record":"<kind>"` and nothing else.
    ///
    /// The kind is the first key of every line so that a reader can dispatch on
    /// it without having parsed the rest, and so that "the first record is the
    /// header" is checkable by looking at the first thirty bytes of a file.
    #[must_use]
    pub fn new(kind: &str) -> Record {
        let mut out = String::from("{");
        push_string(&mut out, "record");
        out.push(':');
        push_string(&mut out, kind);
        Record { out }
    }

    /// A string-valued column.
    pub fn text(&mut self, key: &str, value: &str) {
        self.key(key);
        push_string(&mut self.out, value);
    }

    /// An integer-valued column, printed exactly.
    ///
    /// `i128` rather than `i64` because both sides of the invariant of ADR-003
    /// are summed in `i128` (`ledger/mod.rs`), and narrowing on the way out
    /// would put the one cast `QUANTITIES.md` section 3 names — "one `as f64` in
    /// a helper, a **metric** or an error message" — in the last place before
    /// the file.
    pub fn int(&mut self, key: &str, value: i128) {
        self.key(key);
        push_int(&mut self.out, value);
    }

    /// A column holding a list of integers.
    pub fn int_list(&mut self, key: &str, values: &[i128]) {
        self.key(key);
        self.out.push('[');
        for (i, value) in values.iter().enumerate() {
            if i > 0 {
                self.out.push(',');
            }
            push_int(&mut self.out, *value);
        }
        self.out.push(']');
    }

    /// A column holding a list of strings. The header's roster is the one that
    /// matters.
    pub fn text_list(&mut self, key: &str, values: &[&str]) {
        self.key(key);
        self.out.push('[');
        for (i, value) in values.iter().enumerate() {
            if i > 0 {
                self.out.push(',');
            }
            push_string(&mut self.out, value);
        }
        self.out.push(']');
    }

    /// A column holding a real number.
    ///
    /// # Errors
    ///
    /// Returns an error, and writes nothing at all, if the value is `NaN` or
    /// infinite. See the module header: the alternative is a line that a strict
    /// reader rejects and a lenient one skips, produced by exactly the tick a
    /// reader most wants.
    pub fn real(&mut self, key: &str, value: f64) -> Result<()> {
        if !value.is_finite() {
            bail!(
                "the metric {key} came out {value}, which is not JSON. In FLOAT \
                 mode a non-finite Q is what overflow looks like, and the guard \
                 that would have caught it upstream is a debug assertion \
                 (numeric/float.rs, ADR-037)"
            );
        }
        self.key(key);
        // `{:?}` is the shortest text that reads back as the same f64, and it
        // stays inside the JSON number grammar for every finite value —
        // including the exponent form, which `{}` never produces and which is
        // the difference between six characters and three hundred digits.
        self.out.push_str(&format!("{value:?}"));
        Ok(())
    }

    /// Close the record. The returned line ends with its newline.
    #[must_use]
    pub fn finish(mut self) -> String {
        self.out.push_str("}\n");
        self.out
    }

    fn key(&mut self, key: &str) {
        self.out.push(',');
        push_string(&mut self.out, key);
        self.out.push(':');
    }
}

fn push_string(out: &mut String, value: &str) {
    out.push('"');
    escape_into(out, value);
    out.push('"');
}

fn push_int(out: &mut String, value: i128) {
    // `Display` for `i128` prints every digit; there is no float on this path,
    // which is the whole point of the type.
    out.push_str(&value.to_string());
}

/// Escape a string into a JSON string body.
///
/// Three rules and no fourth: `"`, `\`, and every character below `0x20` as
/// `\u00XX`. The short forms `\n` and `\t` are deliberately not used — one rule
/// for the whole control range has no second branch to get wrong, and a reader
/// that handles the numeric escape at all handles every control byte with it.
fn escape_into(out: &mut String, value: &str) {
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            ch if (ch as u32) < 0x20 => {
                out.push('\\');
                out.push('u');
                // Four hex digits, lower case, always: a reader that special
                // cases the width of the escape is a reader that gets it wrong.
                for shift in [12u32, 8, 4, 0] {
                    let digit = (ch as u32 >> shift) & 0xf;
                    out.push(char::from_digit(digit, 16).expect("a nibble is a hex digit"));
                }
            }
            ch => out.push(ch),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_newline_in_a_name_cannot_split_a_record() {
        let mut record = Record::new("tick");
        record.text("id", "WA\nTER\r\"\\\u{1}");
        let line = record.finish();

        assert_eq!(
            line.matches('\n').count(),
            1,
            "the only newline in a record is its terminator"
        );
        assert!(line.ends_with("}\n"));
        // The escapes are spelled by concatenation rather than written out,
        // because a source file holding the six characters of a numeric escape
        // beside a test about numeric escapes is a file every tool in the chain
        // gets a chance to rewrite.
        for escape in [
            concat!("\\", "u000a"),
            concat!("\\", "u000d"),
            concat!("\\", "u0001"),
        ] {
            assert!(line.contains(escape), "{escape} is missing from {line}");
        }
        assert!(line.contains(r#"\""#), "{line}");
        assert!(line.contains(r"\\"), "{line}");
    }

    #[test]
    fn a_non_finite_number_is_refused_rather_than_written() {
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut record = Record::new("tick");
            record.int("before", 1);
            assert!(record.real("q", bad).is_err(), "{bad} was accepted");
            // And nothing was written: a half-written key would leave a line
            // that is not JSON either.
            let line = record.finish();
            assert!(!line.contains("\"q\""), "{line}");
            assert!(!line.contains("NaN") && !line.contains("inf"), "{line}");
        }

        let mut record = Record::new("tick");
        assert!(record.real("q", -0.5).is_ok());
        assert!(record.finish().contains("-0.5"));
    }

    #[test]
    fn an_integer_wider_than_a_double_is_written_exactly() {
        // 8.59e18 is the domain sum of water at 256 cubed (`QUANTITIES.md`
        // section 3), five orders past the 2^53 where an f64 stops counting by
        // ones. A residual of one unit against it rounds to exactly zero in an
        // f64, and the stream would then report a closed ledger because the
        // number was blurred rather than because the tick closed.
        let values: [i128; 6] = [
            9_007_199_254_740_993,
            8_590_000_000_000_000_000,
            8_590_000_000_000_000_001,
            i128::from(i64::MAX),
            i128::from(i64::MIN),
            -170_141_183_460_469_231_731_687_303_715_884_105_728,
        ];
        for value in values {
            let mut record = Record::new("tick");
            record.int("v", value);
            let line = record.finish();
            let printed = line.split("\"v\":").nth(1).unwrap().trim_end_matches("}\n");
            assert_eq!(
                printed.parse::<i128>().unwrap(),
                value,
                "{value} came back as {printed}"
            );
        }

        let mut record = Record::new("tick");
        record.int_list("v", &values);
        let line = record.finish();
        for value in values {
            assert!(line.contains(&value.to_string()), "{value} is missing");
        }
    }
}
