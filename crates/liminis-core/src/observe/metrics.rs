//! The metric stream: the run header, one record per observed tick, and the
//! splitter that makes the truncation property of ADR-037 checkable from
//! outside.
//!
//! # The header is written by the constructor
//!
//! ADR-037 requires the header to be the first record of the file. A writer that
//! emitted it on the first tick would satisfy that for every run that reaches
//! tick one and for no other, and the run that dies before tick one is the run
//! somebody reads. So [`MetricsWriter::new`] writes it and then returns: a tick
//! record cannot be the first record by construction rather than by discipline.
//!
//! What that does not cover is a *file* whose first record is not a header —
//! a restart appending to an existing stream produces exactly that, and every
//! reader that trusts the first record attributes the second run's numbers to
//! the first run's identity.
// TODO(metrics-restart): whether a restart appends to the existing NDJSON or
// opens a new file is undecided — no record, no CLI flag, no `CONFIG_SCHEMA.md`
// key. The answer decides whether "the first record is the header" is a property
// of the file or only of a writer object, and the acceptance test's meaning
// changes with it. It belongs in `DECISIONS.md` with the wave that gives the
// stream a path.
//!
//! # Every column is an integer
//!
//! Not a style preference: `QUANTITIES.md` section 3 says that the domain sum of
//! water at 256 cubed is `8.59e18`, that this path holds no `f64` at all, and
//! that "one `as f64` in a helper, a **metric** or an error message would make
//! the difference of two sums inexact while leaving it plausible". A residual of
//! one unit against that sum rounds to exactly zero in a double, and the stream
//! would then report a closed ledger because the number was blurred rather than
//! because the tick closed. So [`TickMetrics`] carries `i128` and `i64`, and the
//! only door to a real number in the record — `Record::real` — is not used by
//! anything here.
//!
//! The half of that this file cannot fix: a reader in JavaScript truncates past
//! `2^53`, so the viewer of ADR-016 reads a residual that is exactly right as
//! something else. "The file reads with any tool" is one of the three reasons
//! NDJSON was chosen and it is partly false for exactly these integers.
//!
//! # The roster and the record cannot drift apart
//!
//! The header's `columns` exist so that "a set of numbers" is interpretable six
//! months later. A hand-built roster beside a hand-built record diverges on the
//! first added key, and the stream stays perfectly valid while the header
//! becomes a lie. Here they are one construction: [`columns`] walks a tick of
//! zeroes with the same function that writes a tick of real numbers, and
//! [`MetricsWriter::tick`] holds every key it is about to write against the
//! roster it was given and refuses on the first disagreement.

use std::collections::BTreeSet;
use std::io::Write;

use anyhow::{Context, Result, bail};

use super::json::Record;
use crate::ledger::{CHANNEL_COUNT, Channel, Ledger};

/// Columns whose value legitimately differs between two runs of the same
/// `(seed, config_hash, world_format_version)`.
///
/// Wall time is the one number in an otherwise byte-reproducible stream, and it
/// is also the only performance signal the stream has (E-3,
/// `ticks_per_second_above_threshold_on_reference_scenario`). Dropping the
/// column to make a golden test stable would delete that signal; comparing it
/// would make the test flake on every run. So the exclusion is by name, and the
/// name lives here rather than in whichever comparator is written first.
pub const TIMING_COLUMNS: [&str; 1] = ["tick_nanos"];

/// Extrema and the two numbers a mean is made of, for one field.
///
/// `sum` and `count` rather than a mean, and that is not settled. ADR-037 asks
/// for "means"; `QUANTITIES.md` section 3 forbids an `f64` on this path. The
/// pair is exact and the division is the reader's; a float would be inexact for
/// precisely the fields that matter, and would be the only number in the stream
/// with nothing to check it against.
// TODO(mean-as-pair): whether a mean may be recorded as (sum, count) instead of
// as a number is not decided anywhere. ADR-037 says "means" and permits no
// substitution in as many words; the substitution is made here because the
// alternative violates a rule that *is* written down. A record settles which.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldStat<'a> {
    /// The field's name, as the columns will spell it.
    pub name: &'a str,
    pub min: i128,
    pub max: i128,
    pub sum: i128,
    /// How many cells the three above were taken over. Zero is legal and means
    /// the field is empty; it is not a division waiting to happen, because the
    /// division does not happen here.
    pub count: u64,
}

/// The five things ADR-037 names, plus the code version `main.rs` already prints
/// and the counter of the metric list.
///
/// `toolchain` and `code_version` are two different attestations and neither
/// implies the other: the first is the compiler, the second is the commit.
// TODO(toolchain-attestation): how the toolchain version reaches this field is
// undecided. `include_str!("rust-toolchain.toml")` attests the *pin*; a
// `build.rs` running `rustc -vV` attests the compiler that actually ran, and the
// workspace has no `build.rs`. ADR-024 puts "the compiler version" into
// reproducibility beside the seed and the config without choosing between them,
// and they differ precisely when somebody overrides the pin — which is the case
// the field exists for. The caller supplies the string until a record decides.
#[derive(Clone, Copy, Debug)]
pub struct RunHeader<'a> {
    pub seed: u64,
    pub config_hash: &'a str,
    pub world_format_version: u32,
    pub toolchain: &'a str,
    pub code_version: &'a str,
    pub metrics_schema_version: u32,
    /// Every key a tick record carries, in the order it carries them. Build it
    /// with [`columns`]; a roster assembled by hand is a roster that drifts.
    pub columns: &'a [&'a str],
}

/// One observed tick, already reduced.
///
/// Nothing here is computed by this module: it reports both sides of the
/// invariant of ADR-003 and the difference, and recomputes neither. In
/// particular the residuals arrive as arguments, which is the only shape under
/// which the caller is forced to have obtained them.
///
/// **Every slice is per substance in substance index order (ADR-056), never in
/// lane order.** `DomainSums` is indexed by substance and `Field::lane` by lane,
/// and the two coincide only for the first substance of a width class; a column
/// labelled from a lane is a number that is right and a name that is wrong, with
/// the residual still zero. Label through `Registry::id_of(s)`.
#[derive(Clone, Copy, Debug)]
pub struct TickMetrics<'a> {
    pub tick: u32,
    /// `Registry::id_of(s)` for every substance, in index order.
    pub substance_id: &'a [&'a str],
    /// The domain sum of each substance, in its storage units.
    pub substance_total: &'a [i128],
    /// The domain sum of energy, in joules.
    pub energy_total: i128,
    /// The running counter of every `(channel, substance)` pair, addressed as
    /// [`channel_slot`] says. Build it with [`channel_matter_totals`] rather
    /// than by walking `Ledger`'s table: the transposed address stays inside the
    /// table and merely permutes the pairs, which closes the ledger against
    /// another substance's flow without ever leaving the array.
    pub channel_matter: &'a [i64],
    /// The running energy counter of every channel, in discriminant order.
    pub channel_energy: &'a [i64; CHANNEL_COUNT],
    /// `Ledger::residual_matter` per substance. Required to be zero, and written
    /// whether or not it is (ADR-037).
    pub residual_matter: &'a [i128],
    /// `Ledger::residual_energy`. A separate number from the matter residual and
    /// not derivable from it (ADR-028).
    pub residual_energy: i128,
    /// Zero in S0, and a column all the same: a run where cells appear and the
    /// count is absent from the earlier records is a run whose history cannot be
    /// plotted.
    pub n_cells: u64,
    pub n_organisms: u64,
    /// Extrema and means of the key fields.
    pub field: &'a [FieldStat<'a>],
    /// Wall time of the tick. See [`TIMING_COLUMNS`].
    // TODO(tick-time-unit): `QUANTITIES.md` has no row for tick time and no
    // record names one — wall-clock nanoseconds as an integer, seconds as a
    // float, whole tick or per phase are all undeclared. Integer nanoseconds is
    // the narrowest of them and the only one that can be re-expressed as any of
    // the others without loss, which is why it is what the field is called; a
    // number in a stream whose header exists to make numbers interpretable
    // deserves a row in `QUANTITIES.md`, and that is a document change, not a
    // guess to be made here.
    pub tick_nanos: u64,
}

/// Where the counter of one `(channel, substance)` pair lives in
/// [`TickMetrics::channel_matter`].
///
/// Channel-major, one contiguous row of substances per channel — the shape
/// `Ledger` uses internally, spelled once here so that the filler and the reader
/// of the slice cannot disagree. Both of them are in this file and both go
/// through this function.
#[must_use]
pub fn channel_slot(n_substances: usize, channel: Channel, substance: usize) -> usize {
    channel as usize * n_substances + substance
}

/// The running matter counters of every pair, laid out for [`TickMetrics`].
///
/// Goes through `Ledger::matter`, which is the ledger's own door and applies its
/// own bounds check. Nothing outside `ledger/` computes an address into that
/// table.
#[must_use]
pub fn channel_matter_totals(ledger: &Ledger) -> Vec<i64> {
    let n = ledger.n_substances() as usize;
    let mut out = vec![0; n * CHANNEL_COUNT];
    for channel in Channel::ALL {
        for substance in 0..ledger.n_substances() {
            out[channel_slot(n, channel, substance as usize)] = ledger.matter(channel, substance);
        }
    }
    out
}

/// The running energy counters, in discriminant order.
#[must_use]
pub fn channel_energy_totals(ledger: &Ledger) -> [i64; CHANNEL_COUNT] {
    let mut out = [0; CHANNEL_COUNT];
    for channel in Channel::ALL {
        out[channel as usize] = ledger.energy(channel);
    }
    out
}

/// Every column of a tick record, in the order a record writes them.
///
/// This is the roster for [`RunHeader::columns`], and it is not a second list:
/// it is the keys of an actual record, walked over a tick of zeroes by the same
/// function that walks a tick of real numbers.
#[must_use]
pub fn columns(substance_id: &[&str], field_name: &[&str]) -> Vec<String> {
    let n = substance_id.len();
    let zeros = vec![0i128; n];
    let counters = vec![0i64; n * CHANNEL_COUNT];
    let fields: Vec<FieldStat<'_>> = field_name
        .iter()
        .map(|&name| FieldStat {
            name,
            min: 0,
            max: 0,
            sum: 0,
            count: 0,
        })
        .collect();
    let metrics = TickMetrics {
        tick: 0,
        substance_id,
        substance_total: &zeros,
        energy_total: 0,
        channel_matter: &counters,
        channel_energy: &[0; CHANNEL_COUNT],
        residual_matter: &zeros,
        residual_energy: 0,
        n_cells: 0,
        n_organisms: 0,
        field: &fields,
        tick_nanos: 0,
    };

    let mut out = Vec::new();
    walk(&metrics, &mut |key, _| out.push(key.to_string()));
    out
}

/// Every `(key, value)` of one tick record, in order.
///
/// The single place the shape of a tick is written down. `columns` reads the
/// keys out of it and [`MetricsWriter::tick`] writes the pairs out of it, so a
/// column added to one is a column added to both.
fn walk(m: &TickMetrics<'_>, emit: &mut impl FnMut(&str, i128)) {
    let n = m.substance_id.len();
    let mut key = String::new();

    emit("tick", i128::from(m.tick));

    for (s, id) in m.substance_id.iter().enumerate() {
        compose(&mut key, &["total.", id]);
        emit(&key, m.substance_total[s]);
    }
    emit("total.energy", m.energy_total);

    for channel in Channel::ALL {
        for (s, id) in m.substance_id.iter().enumerate() {
            compose(&mut key, &["channel.", channel.name(), ".", id]);
            emit(
                &key,
                i128::from(m.channel_matter[channel_slot(n, channel, s)]),
            );
        }
        compose(&mut key, &["channel.", channel.name(), ".energy"]);
        emit(&key, i128::from(m.channel_energy[channel as usize]));
    }

    // Both residuals, unconditionally, whatever they hold. A written zero is
    // proof that the check ran; a missing key is indistinguishable from a run
    // with the check switched off (ADR-037). There is no branch here on purpose.
    for (s, id) in m.substance_id.iter().enumerate() {
        compose(&mut key, &["residual.", id]);
        emit(&key, m.residual_matter[s]);
    }
    emit("residual.energy", m.residual_energy);

    emit("n_cells", i128::from(m.n_cells));
    emit("n_organisms", i128::from(m.n_organisms));

    for stat in m.field {
        compose(&mut key, &["field.", stat.name, ".min"]);
        emit(&key, stat.min);
        compose(&mut key, &["field.", stat.name, ".max"]);
        emit(&key, stat.max);
        compose(&mut key, &["field.", stat.name, ".sum"]);
        emit(&key, stat.sum);
        compose(&mut key, &["field.", stat.name, ".count"]);
        emit(&key, i128::from(stat.count));
    }

    emit("tick_nanos", i128::from(m.tick_nanos));
}

/// Build a key in place, reusing the buffer across a hundred-odd columns.
fn compose(key: &mut String, parts: &[&str]) {
    key.clear();
    for part in parts {
        key.push_str(part);
    }
}

/// The append-only stream of ADR-037.
///
/// Generic over the sink so that a test writes into a `Vec<u8>` and a run writes
/// into a file; nothing here decides where the file is.
// TODO(metrics-destination): the path of the stream, whether it is derived from
// `(seed, config_hash)`, whether a directory is created, and how often a tick is
// observed at all are named by no record, no CLI flag and no `CONFIG_SCHEMA.md`
// key — section "Sections whose shape is not yet decided" lists "the frequency
// of snapshots and metrics (ADR-037, SPEC section 8 step 6)" as having content
// and no key. So the cadence is the caller's argument and the sink is the
// caller's object, until a record settles the key.
#[derive(Debug)]
pub struct MetricsWriter<W: Write> {
    sink: W,
    columns: Vec<String>,
    records_written: u64,
}

impl<W: Write> MetricsWriter<W> {
    /// Open a stream and write its header.
    ///
    /// # Errors
    ///
    /// Returns an error if the roster is empty or names a column twice — two
    /// keys of one name in a record make its value for that name depend on which
    /// reader reads it — or if the sink refuses the header.
    pub fn new(sink: W, header: &RunHeader<'_>) -> Result<Self> {
        if header.columns.is_empty() {
            bail!(
                "a run header with an empty roster names nothing, and the set of \
                 numbers under it stops being interpretable (ADR-037)"
            );
        }
        let mut seen = BTreeSet::new();
        for column in header.columns {
            if !seen.insert(*column) {
                bail!(
                    "the roster names the column {column} twice, so a record \
                     would carry that key twice and its value would depend on \
                     which reader read it"
                );
            }
        }

        let mut record = Record::new("header");
        record.int("seed", i128::from(header.seed));
        record.text("config_hash", header.config_hash);
        record.int(
            "world_format_version",
            i128::from(header.world_format_version),
        );
        record.text("toolchain", header.toolchain);
        record.text("code_version", header.code_version);
        record.int(
            "metrics_schema_version",
            i128::from(header.metrics_schema_version),
        );
        record.text_list("columns", header.columns);

        let mut writer = MetricsWriter {
            sink,
            columns: header.columns.iter().map(|c| (*c).to_string()).collect(),
            records_written: 0,
        };
        // Before returning, not on the first tick: see the module header.
        writer.emit(record.finish())?;
        Ok(writer)
    }

    /// Write one tick record.
    ///
    /// # Errors
    ///
    /// Returns an error if the slices disagree about how many substances there
    /// are — a mismatch would label somebody else's number — if the keys leave
    /// the roster the header promised, or if the sink refuses the line.
    pub fn tick(&mut self, m: &TickMetrics<'_>) -> Result<()> {
        check_shape(m)?;

        let mut record = Record::new("tick");
        let columns = &self.columns;
        let mut index = 0usize;
        let mut drift: Option<String> = None;
        walk(m, &mut |key, value| {
            if drift.is_none() {
                match columns.get(index) {
                    Some(promised) if promised == key => {}
                    Some(promised) => {
                        drift = Some(format!(
                            "column {index} is {promised} in the header and {key} \
                             in the tick"
                        ));
                    }
                    None => {
                        drift = Some(format!(
                            "the tick writes {key} past the end of a roster of {} \
                             columns",
                            columns.len()
                        ));
                    }
                }
            }
            index += 1;
            record.int(key, value);
        });

        if let Some(message) = drift {
            bail!(
                "the tick record and the run header have drifted apart: \
                 {message}. The header exists so that the numbers under it are \
                 interpretable later (ADR-037), and a stream whose roster is \
                 wrong is valid JSON that lies about itself"
            );
        }
        if index != self.columns.len() {
            bail!(
                "the tick wrote {index} columns and the header promised {}",
                self.columns.len()
            );
        }

        self.emit(record.finish())
    }

    /// Push whatever the sink is buffering.
    ///
    /// # Errors
    ///
    /// Whatever the sink returns.
    pub fn flush(&mut self) -> Result<()> {
        self.sink.flush().context("flushing the metric stream")?;
        Ok(())
    }

    /// How many records this writer has written, the header included.
    #[must_use]
    pub fn records_written(&self) -> u64 {
        self.records_written
    }

    /// The sink, for a caller that wants to look at what has been written so
    /// far. A test is that caller.
    #[must_use]
    pub fn sink(&self) -> &W {
        &self.sink
    }

    /// Give the sink back.
    #[must_use]
    pub fn into_inner(self) -> W {
        self.sink
    }

    fn emit(&mut self, line: String) -> Result<()> {
        self.sink
            .write_all(line.as_bytes())
            .context("writing a record to the metric stream")?;
        self.records_written += 1;
        Ok(())
    }
}

fn check_shape(m: &TickMetrics<'_>) -> Result<()> {
    let n = m.substance_id.len();
    if m.substance_total.len() != n {
        bail!(
            "{n} substance ids against {} totals: every per-substance column is \
             labelled by index (ADR-056), so a mismatch labels somebody else's \
             number",
            m.substance_total.len()
        );
    }
    if m.residual_matter.len() != n {
        bail!(
            "{n} substance ids against {} residuals: ADR-003 wants one residual \
             per substance and ADR-037 wants every one of them written",
            m.residual_matter.len()
        );
    }
    if m.channel_matter.len() != n * CHANNEL_COUNT {
        bail!(
            "{n} substances over {CHANNEL_COUNT} channels want {} counters, not {}",
            n * CHANNEL_COUNT,
            m.channel_matter.len()
        );
    }
    Ok(())
}

/// Split a stream into the records that are complete.
///
/// The reader side of the property ADR-037 chose NDJSON for. It exists so that
/// the property is checkable rather than merely believed: a torn last tail is
/// reported by [`Records::truncated_tail`] and never yielded as a record.
#[must_use]
pub fn records(bytes: &[u8]) -> Records<'_> {
    Records { rest: bytes }
}

/// Complete records of a stream. See [`records`].
#[derive(Clone, Debug)]
pub struct Records<'a> {
    rest: &'a [u8],
}

impl<'a> Iterator for Records<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<&'a [u8]> {
        let end = self.rest.iter().position(|&b| b == b'\n')?;
        let line = &self.rest[..end];
        self.rest = &self.rest[end + 1..];
        Some(line)
    }
}

impl<'a> Records<'a> {
    /// The bytes after the last newline of what is left: an incomplete record,
    /// or nothing at all if the stream ends where a record ended.
    ///
    /// On a fresh iterator this is the torn tail of the whole stream, which is
    /// the question a reader of a killed run asks.
    #[must_use]
    pub fn truncated_tail(&self) -> &'a [u8] {
        match self.rest.iter().rposition(|&b| b == b'\n') {
            Some(last) => &self.rest[last + 1..],
            None => self.rest,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::Ledger;

    const IDS: [&str; 2] = ["WATER", "O2"];
    const FIELDS: [&str; 1] = ["enthalpy"];

    fn roster() -> Vec<String> {
        columns(&IDS, &FIELDS)
    }

    fn writer(columns: &[&str]) -> MetricsWriter<Vec<u8>> {
        MetricsWriter::new(
            Vec::new(),
            &RunHeader {
                seed: 1,
                config_hash: "hash",
                world_format_version: 4,
                toolchain: "1.97.1",
                code_version: "0.1.0",
                metrics_schema_version: super::super::METRICS_SCHEMA_VERSION,
                columns,
            },
        )
        .unwrap()
    }

    fn tick(counters: &[i64], residual: &[i128]) -> Vec<u8> {
        let roster = roster();
        let refs: Vec<&str> = roster.iter().map(String::as_str).collect();
        let mut writer = writer(&refs);
        let totals = [1i128, 2];
        let fields = [FieldStat {
            name: "enthalpy",
            min: -1,
            max: 2,
            sum: 3,
            count: 4,
        }];
        writer
            .tick(&TickMetrics {
                tick: 7,
                substance_id: &IDS,
                substance_total: &totals,
                energy_total: 9,
                channel_matter: counters,
                channel_energy: &[0; CHANNEL_COUNT],
                residual_matter: residual,
                residual_energy: 0,
                n_cells: 0,
                n_organisms: 0,
                field: &fields,
                tick_nanos: 5,
            })
            .unwrap();
        writer.into_inner()
    }

    #[test]
    fn the_roster_is_the_keys_a_record_writes() {
        let bytes = tick(&[0; IDS.len() * CHANNEL_COUNT], &[0; IDS.len()]);
        let line = records(&bytes).nth(1).unwrap();
        let text = std::str::from_utf8(line).unwrap();

        // Not a set comparison: the order matters too, because the roster is
        // read positionally by anything that turns the stream into a table.
        let written: Vec<&str> = text
            .split(",\"")
            .skip(1)
            .map(|part| part.split('"').next().unwrap())
            .collect();
        assert_eq!(written, roster());
    }

    #[test]
    fn a_tick_that_leaves_the_roster_is_refused_rather_than_written() {
        // The drift the header cannot survive: a record carrying a key the
        // header never promised. The stream would stay valid JSON.
        let short: Vec<String> = roster().into_iter().take(3).collect();
        let refs: Vec<&str> = short.iter().map(String::as_str).collect();
        let mut writer = writer(&refs);
        let totals = [1i128, 2];
        let counters = [0i64; IDS.len() * CHANNEL_COUNT];
        let residual = [0i128; IDS.len()];
        let fields = [FieldStat {
            name: "enthalpy",
            min: 0,
            max: 0,
            sum: 0,
            count: 0,
        }];
        let error = writer
            .tick(&TickMetrics {
                tick: 0,
                substance_id: &IDS,
                substance_total: &totals,
                energy_total: 0,
                channel_matter: &counters,
                channel_energy: &[0; CHANNEL_COUNT],
                residual_matter: &residual,
                residual_energy: 0,
                n_cells: 0,
                n_organisms: 0,
                field: &fields,
                tick_nanos: 0,
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("drifted apart"), "{error}");
        // And nothing was written: a partial record would be worse than none.
        assert_eq!(writer.records_written(), 1);
    }

    #[test]
    fn a_roster_naming_a_column_twice_is_refused() {
        let error = MetricsWriter::new(
            Vec::new(),
            &RunHeader {
                seed: 1,
                config_hash: "hash",
                world_format_version: 4,
                toolchain: "1.97.1",
                code_version: "0.1.0",
                metrics_schema_version: super::super::METRICS_SCHEMA_VERSION,
                columns: &["tick", "tick"],
            },
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("twice"), "{error}");

        // The case this catches in the field: a substance whose id collides with
        // the name the energy column already has.
        let colliding = columns(&["energy", "O2"], &FIELDS);
        let refs: Vec<&str> = colliding.iter().map(String::as_str).collect();
        assert!(
            MetricsWriter::new(
                Vec::new(),
                &RunHeader {
                    seed: 1,
                    config_hash: "hash",
                    world_format_version: 4,
                    toolchain: "1.97.1",
                    code_version: "0.1.0",
                    metrics_schema_version: super::super::METRICS_SCHEMA_VERSION,
                    columns: &refs,
                }
            )
            .is_err()
        );
    }

    #[test]
    fn a_slice_of_the_wrong_length_is_refused() {
        let roster = roster();
        let refs: Vec<&str> = roster.iter().map(String::as_str).collect();
        let mut writer = writer(&refs);
        let totals = [1i128];
        let counters = [0i64; IDS.len() * CHANNEL_COUNT];
        let residual = [0i128; IDS.len()];
        let error = writer
            .tick(&TickMetrics {
                tick: 0,
                substance_id: &IDS,
                substance_total: &totals,
                energy_total: 0,
                channel_matter: &counters,
                channel_energy: &[0; CHANNEL_COUNT],
                residual_matter: &residual,
                residual_energy: 0,
                n_cells: 0,
                n_organisms: 0,
                field: &[],
                tick_nanos: 0,
            })
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("2 substance ids against 1 totals"),
            "{error}"
        );
    }

    #[test]
    fn the_counter_table_is_read_through_the_ledgers_own_door() {
        // The transposed address stays inside the table and merely permutes the
        // pairs, so the failure is a plausible number in the wrong column
        // (`ledger/mod.rs`). Every pair gets a distinct value, and the record
        // must carry each of them under its own name.
        let mut ledger = Ledger::new(2).unwrap();
        let value = |channel: Channel, substance: u32| -> i64 {
            (channel as i64 + 1) * 1_000 + i64::from(substance) + 1
        };
        for channel in Channel::ALL {
            for substance in 0..2 {
                ledger.credit_matter(channel, substance, value(channel, substance));
            }
        }

        let counters = channel_matter_totals(&ledger);
        let bytes = tick(&counters, &[0; IDS.len()]);
        let text = String::from_utf8(bytes).unwrap();
        for channel in Channel::ALL {
            for (s, id) in IDS.iter().enumerate() {
                let expected = format!(
                    "\"channel.{}.{id}\":{}",
                    channel.name(),
                    value(channel, s as u32)
                );
                assert!(text.contains(&expected), "{expected} is missing");
            }
        }
    }

    #[test]
    fn a_stream_splits_only_on_complete_records() {
        let bytes = b"{\"a\":1}\n{\"b\":2}\n{\"c\"".to_vec();
        let found: Vec<&[u8]> = records(&bytes).collect();
        assert_eq!(found, vec![&b"{\"a\":1}"[..], &b"{\"b\":2}"[..]]);
        assert_eq!(records(&bytes).truncated_tail(), b"{\"c\"");
        assert!(records(b"").next().is_none());
        assert!(records(b"{\"a\":1}\n").truncated_tail().is_empty());
    }
}
