//! Acceptance criteria of the metric stream (`ACCEPTANCE.md`, section
//! "Наблюдение"; ADR-037, ADR-003, ADR-028, ADR-020).
//!
//! ```text
//! the_first_record_is_the_run_header
//! the_residual_is_written_even_when_it_is_zero
//! a_truncated_tail_does_not_lose_the_rest
//! ```
//!
//! They live in an integration test rather than beside the module for the rule
//! `acceptance_ledger.rs` states at its top: an acceptance test is the outside
//! view, it may use only the public surface of `liminis-core`, and so it cannot
//! be quietly shaped around a private helper it was supposed to be judging.
//!
//! The reader below is part of that outside view rather than scaffolding around
//! it. ADR-037 chose NDJSON so that the file "reads with any tool without a
//! library", and the workspace has no JSON parser to lean on; these seventy
//! lines are what that promise costs a reader, and they fail loudly on anything
//! the writer emits that is not JSON.

use std::collections::BTreeSet;

use liminis_core::ledger::CHANNEL_COUNT;
use liminis_core::observe::{
    FieldStat, METRICS_SCHEMA_VERSION, MetricsWriter, RunHeader, TickMetrics, columns, records,
};
use liminis_core::version::WORLD_FORMAT_VERSION;

// ---------------------------------------------------------------------------
// A reader with no library behind it
// ---------------------------------------------------------------------------

/// One JSON value, as far as this stream ever nests.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Val {
    Str(String),
    /// The digits as printed. Kept as text on purpose: parsing them into an
    /// `f64` here would be the very mistake `QUANTITIES.md` section 3 names
    /// about metrics, committed by the checker instead of by the writer.
    Num(String),
    List(Vec<Val>),
}

impl Val {
    fn str(&self) -> &str {
        match self {
            Val::Str(s) => s,
            other => panic!("expected a string, found {other:?}"),
        }
    }

    fn num(&self) -> &str {
        match self {
            Val::Num(n) => n,
            other => panic!("expected a number, found {other:?}"),
        }
    }

    fn list(&self) -> &[Val] {
        match self {
            Val::List(items) => items,
            other => panic!("expected a list, found {other:?}"),
        }
    }
}

/// Every `(key, value)` of one record, in the order written.
fn pairs(line: &[u8]) -> Vec<(String, Val)> {
    let bytes = line;
    assert_eq!(bytes.first(), Some(&b'{'), "a record starts with an object");
    let mut i = 1;
    let mut out = Vec::new();
    while i < bytes.len() && bytes[i] != b'}' {
        let (key, next) = read_string(bytes, i);
        i = next;
        assert_eq!(bytes[i], b':', "a key is followed by a colon");
        let (value, next) = read_value(bytes, i + 1);
        i = next;
        out.push((key, value));
        if i < bytes.len() && bytes[i] == b',' {
            i += 1;
        }
    }
    assert_eq!(bytes[i], b'}', "a record ends with a closing brace");
    assert_eq!(i, bytes.len() - 1, "a record ends where the line ends");
    out
}

fn read_value(bytes: &[u8], i: usize) -> (Val, usize) {
    match bytes[i] {
        b'"' => {
            let (s, next) = read_string(bytes, i);
            (Val::Str(s), next)
        }
        b'[' => {
            let mut items = Vec::new();
            let mut j = i + 1;
            while bytes[j] != b']' {
                let (value, next) = read_value(bytes, j);
                items.push(value);
                j = next;
                if bytes[j] == b',' {
                    j += 1;
                }
            }
            (Val::List(items), j + 1)
        }
        _ => {
            let mut j = i;
            while !matches!(bytes[j], b',' | b'}' | b']') {
                j += 1;
            }
            let text = String::from_utf8(bytes[i..j].to_vec()).expect("a number is ASCII");
            (Val::Num(text), j)
        }
    }
}

fn read_string(bytes: &[u8], i: usize) -> (String, usize) {
    assert_eq!(bytes[i], b'"', "a string starts with a quote");
    let mut out: Vec<u8> = Vec::new();
    let mut j = i + 1;
    while bytes[j] != b'"' {
        if bytes[j] == b'\\' {
            match bytes[j + 1] {
                b'"' => {
                    out.push(b'"');
                    j += 2;
                }
                b'\\' => {
                    out.push(b'\\');
                    j += 2;
                }
                b'u' => {
                    let hex =
                        std::str::from_utf8(&bytes[j + 2..j + 6]).expect("an escape is ASCII");
                    let code = u32::from_str_radix(hex, 16).expect("an escape is four hex digits");
                    let ch = char::from_u32(code).expect("an escape names a character");
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                    j += 6;
                }
                other => panic!("a record carries an escape this reader does not know: \\{other}"),
            }
        } else {
            out.push(bytes[j]);
            j += 1;
        }
    }
    (
        String::from_utf8(out).expect("a string value is UTF-8"),
        j + 1,
    )
}

fn value_of<'a>(pairs: &'a [(String, Val)], key: &str) -> &'a Val {
    &pairs
        .iter()
        .find(|(k, _)| k == key)
        .unwrap_or_else(|| panic!("no key {key} in the record"))
        .1
}

// ---------------------------------------------------------------------------
// A run to write about
// ---------------------------------------------------------------------------

const IDS: [&str; 3] = ["WATER", "O2", "H2S"];
const FIELDS: [&str; 2] = ["enthalpy", "energy_delta"];

fn roster() -> Vec<String> {
    columns(&IDS, &FIELDS)
}

fn header<'a>(columns: &'a [&'a str]) -> RunHeader<'a> {
    RunHeader {
        seed: 42,
        config_hash: "b3sum-of-the-canonical-form",
        world_format_version: WORLD_FORMAT_VERSION,
        toolchain: "1.97.1",
        code_version: "0.1.0",
        metrics_schema_version: METRICS_SCHEMA_VERSION,
        columns,
    }
}

/// A tick of a closed domain: every channel silent, both residuals zero. The S0
/// case, and the one the writer must not be allowed to shorten.
struct Tick {
    totals: Vec<i128>,
    channel_matter: Vec<i128>,
    channel_energy: [i128; CHANNEL_COUNT],
    residual_matter: Vec<i128>,
    residual_energy: i128,
    fields: Vec<FieldStat<'static>>,
}

impl Tick {
    fn new() -> Tick {
        Tick {
            totals: vec![1_000, 2_000, 3_000],
            channel_matter: vec![0; IDS.len() * CHANNEL_COUNT],
            channel_energy: [0; CHANNEL_COUNT],
            residual_matter: vec![0; IDS.len()],
            residual_energy: 0,
            fields: FIELDS
                .iter()
                .map(|&name| FieldStat {
                    name,
                    min: -7,
                    max: 11,
                    sum: 40,
                    count: 8,
                })
                .collect(),
        }
    }

    fn metrics<'a>(&'a self, tick: u32, ids: &'a [&'a str]) -> TickMetrics<'a> {
        TickMetrics {
            tick,
            substance_id: ids,
            substance_total: &self.totals,
            energy_total: 8_590_000_000_000_000_000,
            channel_matter: &self.channel_matter,
            channel_energy: &self.channel_energy,
            residual_matter: &self.residual_matter,
            residual_energy: self.residual_energy,
            n_cells: 0,
            n_organisms: 0,
            field: &self.fields,
            tick_nanos: 1_234_567,
        }
    }
}

// ---------------------------------------------------------------------------
// the_first_record_is_the_run_header
// ---------------------------------------------------------------------------

/// ADR-037: "the first record in the file is the run header: `seed`,
/// `config_hash`, `WORLD_FORMAT_VERSION`, the toolchain version, the roster of
/// columns. Without it a set of numbers is not interpretable six months later."
///
/// Two halves, and the second is the one that can go on failing for years. The
/// first is that the header exists at all and exists *before* any tick — a
/// writer that emits it lazily on the first tick has a file whose header is not
/// the first record whenever the run dies before tick one, which is exactly the
/// run somebody will be reading. The second is that the roster tells the truth:
/// a header listing columns that are not the keys of a tick record is a valid
/// NDJSON file that lies about itself, and nothing else in the corpus can see
/// it. That is why the key set is compared in both directions.
#[test]
fn the_first_record_is_the_run_header() {
    let roster = roster();
    let refs: Vec<&str> = roster.iter().map(String::as_str).collect();
    let mut writer = MetricsWriter::new(Vec::new(), &header(&refs)).unwrap();

    // Nothing has been ticked, and the file already stands on its own.
    assert_eq!(writer.records_written(), 1);
    let written: Vec<&[u8]> = records(writer.sink()).collect();
    assert_eq!(written.len(), 1, "the header is written by the constructor");

    let head = pairs(written[0]);
    assert_eq!(value_of(&head, "record").str(), "header");
    assert_eq!(value_of(&head, "seed").num(), "42");
    assert_eq!(
        value_of(&head, "config_hash").str(),
        "b3sum-of-the-canonical-form"
    );
    assert_eq!(
        value_of(&head, "world_format_version").num(),
        WORLD_FORMAT_VERSION.to_string()
    );
    assert_eq!(value_of(&head, "toolchain").str(), "1.97.1");
    assert_eq!(value_of(&head, "code_version").str(), "0.1.0");
    assert_eq!(
        value_of(&head, "metrics_schema_version").num(),
        METRICS_SCHEMA_VERSION.to_string()
    );
    let listed = value_of(&head, "columns").list();
    assert!(!listed.is_empty(), "a header with no roster names nothing");

    // The half that is not a tautology: what the header promises is what a tick
    // record carries.
    let tick = Tick::new();
    writer.tick(&tick.metrics(0, &IDS)).unwrap();
    assert_eq!(writer.records_written(), 2);

    let written: Vec<&[u8]> = records(writer.sink()).collect();
    assert_eq!(written.len(), 2);
    let body = pairs(written[1]);
    assert_eq!(value_of(&body, "record").str(), "tick");

    // `record` is the kind discriminator rather than an observable, and it is
    // the one key of a line that is not a column.
    let keys: BTreeSet<String> = body
        .iter()
        .map(|(k, _)| k.clone())
        .filter(|k| k != "record")
        .collect();
    let promised: BTreeSet<String> = listed.iter().map(|v| v.str().to_string()).collect();
    let from_the_roster: BTreeSet<String> = roster.iter().cloned().collect();

    assert_eq!(
        promised, from_the_roster,
        "the header printed a roster other than the one it was given"
    );
    assert_eq!(
        keys,
        promised,
        "the header's roster and the tick's keys have drifted apart: \
         written but not promised {:?}, promised but not written {:?}",
        keys.difference(&promised).collect::<Vec<_>>(),
        promised.difference(&keys).collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// the_residual_is_written_even_when_it_is_zero
// ---------------------------------------------------------------------------

/// ADR-037: "the residual is written although it is required to be zero. A
/// written zero is proof that the check ran; a missing line is
/// indistinguishable from a disabled check."
///
/// The tick fed here is the S0 one — a closed domain, every channel silent,
/// both residuals exactly zero — because that is the tick under which every
/// optimisation that deletes the column looks harmless. `if r != 0`, a skip-zero
/// list, a "compact" mode: each leaves valid JSON, leaves every other column
/// meaning what it meant, and leaves a file that cannot be told apart from a run
/// with the check switched off.
#[test]
fn the_residual_is_written_even_when_it_is_zero() {
    let roster = roster();
    let refs: Vec<&str> = roster.iter().map(String::as_str).collect();
    let mut writer = MetricsWriter::new(Vec::new(), &header(&refs)).unwrap();

    let closed = Tick::new();
    writer.tick(&closed.metrics(0, &IDS)).unwrap();

    let written: Vec<&[u8]> = records(writer.sink()).collect();
    let body = pairs(written[1]);

    let residuals: Vec<&(String, Val)> = body
        .iter()
        .filter(|(k, _)| k.starts_with("residual."))
        .collect();
    assert_eq!(
        residuals.len(),
        IDS.len() + 1,
        "ADR-028 makes the invariant double: one residual per substance and one \
         for energy, and neither can be derived from the other. Found {:?}",
        residuals.iter().map(|(k, _)| k).collect::<Vec<_>>()
    );
    for id in IDS {
        assert_eq!(
            value_of(&body, &format!("residual.{id}")).num(),
            "0",
            "the matter residual of {id} was not written as a zero"
        );
    }
    assert_eq!(value_of(&body, "residual.energy").num(), "0");

    // And the same two keys carry a residual that is not zero, so that the test
    // cannot be passed by a writer that prints the constant 0. One substance off
    // by three, energy off by minus five: the shape of a tick where matter
    // closed and energy did not, which ADR-028 exists to make expressible.
    let mut broken = Tick::new();
    broken.residual_matter[1] = 3;
    broken.residual_energy = -5;
    writer.tick(&broken.metrics(1, &IDS)).unwrap();

    let written: Vec<&[u8]> = records(writer.sink()).collect();
    let body = pairs(written[2]);
    assert_eq!(value_of(&body, "residual.WATER").num(), "0");
    assert_eq!(value_of(&body, "residual.O2").num(), "3");
    assert_eq!(value_of(&body, "residual.H2S").num(), "0");
    assert_eq!(value_of(&body, "residual.energy").num(), "-5");
    assert_eq!(
        body.iter()
            .filter(|(k, _)| k.starts_with("residual."))
            .count(),
        IDS.len() + 1
    );
}

// ---------------------------------------------------------------------------
// a_truncated_tail_does_not_lose_the_rest
// ---------------------------------------------------------------------------

/// The first of the three properties ADR-037 chose NDJSON for: "a torn last tail
/// is discarded without losing the rest".
///
/// The property is about the file, so it is checked at every byte offset a run
/// can die at, and over the only inputs that can break it. `substance.id` is a
/// free TOML string — `Registry::new` checks duplicates and the count of
/// thirty-one and nothing about characters — so an id holding a newline is
/// reachable from a scenario file rather than hypothetical, and an unescaped one
/// splits a record into two halves that both look like NDJSON to a line-oriented
/// reader. From there the property does not hold for the whole file, and nothing
/// says so.
#[test]
fn a_truncated_tail_does_not_lose_the_rest() {
    // A newline, a quote, a backslash and a control byte, in the names that
    // become keys of every record.
    let ids: [&str; 3] = ["WA\nTER", "O\"2", "H2S\\\u{1}"];
    let fields: [&str; 1] = ["ent\nhalpy"];
    let roster = columns(&ids, &fields);
    let refs: Vec<&str> = roster.iter().map(String::as_str).collect();
    let mut writer = MetricsWriter::new(Vec::new(), &header(&refs)).unwrap();

    let mut tick = Tick::new();
    tick.fields = fields
        .iter()
        .map(|&name| FieldStat {
            name,
            min: 0,
            max: 1,
            sum: 2,
            count: 3,
        })
        .collect();
    for n in 0..3 {
        writer.tick(&tick.metrics(n, &ids)).unwrap();
    }
    let bytes = writer.into_inner();

    let whole: Vec<&[u8]> = records(&bytes).collect();
    assert_eq!(whole.len(), 4, "a header and three ticks");
    for (i, record) in whole.iter().enumerate() {
        assert!(
            !record.contains(&b'\n'),
            "record {i} carries an interior newline, and a line-oriented reader \
             sees two records where one was written"
        );
        // Not merely newline-free: still a record, whatever the ids were. The
        // header is the seven fields of ADR-024 and ADR-037 plus the kind; a
        // tick is the roster plus the kind.
        let expected = if i == 0 { 8 } else { roster.len() + 1 };
        assert_eq!(pairs(record).len(), expected, "record {i}");
    }

    for k in 0..=bytes.len() {
        let prefix = &bytes[..k];
        let mut reader = records(prefix);
        let got: Vec<&[u8]> = reader.by_ref().collect();

        let complete = prefix.iter().filter(|&&b| b == b'\n').count();
        assert_eq!(
            got.len(),
            complete,
            "at {k} bytes the reader yielded {} records over {complete} newlines",
            got.len()
        );
        for (i, record) in got.iter().enumerate() {
            assert_eq!(
                record, &whole[i],
                "at {k} bytes record {i} is not the one the whole file holds"
            );
        }

        // What is left over is reported rather than yielded. A reader that
        // handed the incomplete tail back as a record would parse a truncated
        // object, and the numbers it did contain would look complete.
        let tail = records(prefix).truncated_tail();
        let consumed: usize = got.iter().map(|r| r.len() + 1).sum();
        assert_eq!(tail, &prefix[consumed..]);
        assert!(!tail.contains(&b'\n'));
    }

    // The last record ends with its newline, so a file that was never truncated
    // has no tail at all. Omitting it would make the final tick of every
    // completed run unreadable by this rule.
    assert!(records(&bytes).truncated_tail().is_empty());
}
