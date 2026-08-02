//! The other stream of ADR-037: the full state, written rarely, for restart.
//!
//! Nothing is shared with `metrics.rs` and that is the point of the record — the
//! two streams differ in everything that matters. This one is binary, is
//! hundreds of megabytes, is written every few thousand ticks, and is read by
//! exactly one program. The other is text, is kilobytes a record, is written
//! every observed tick, and is read by whoever comes along.
//!
//! # Restart is "load the scenario, then fill it"
//!
//! [`snapshot_read_into`] pours a file into a world that has already been built
//! from a config, and refuses on any identity mismatch. It never reconstructs a
//! world out of the bytes. A snapshot has the right *shape* whenever the
//! substance count and the grids match, so nothing about the bytes forces the
//! check; without it a restart carries the old file's provenance and the new
//! file's physics, and the run identity of ADR-020 stops identifying anything.
//!
//! # Both buffers of every field
//!
//! ADR-057's process-boundary invariant is that the front buffer holds state `N`
//! for every lane, restored by at most one swap plus a copy of the minority
//! parity group. A snapshot storing `read()` alone restarts a world whose next
//! swap promotes zeroes for every lane the tick did not write, and
//! `Field::restore_lane` copies `back -> front`. Both residuals go on closing,
//! because — `field.rs` says this about the in-tick twin of the same bug — "a
//! stale state is conserved no worse than a fresh one, so both domain sums stand
//! still while the world quietly stops moving for those lanes". Across a restart
//! there is not even that test looking. So both buffers go in the file.
//!
//! # Substance order, never lane order
//!
//! `registry.rs` states it as a rule about this file: "lanes never leave the
//! process, either: a snapshot is written in substance order, not lane order
//! (ADR-037, ADR-056), or the layout would become part of the format and a
//! scenario that edited one `max_conc` would silently change the meaning of old
//! files".
//!
//! # What is not in the file, and why not
//!
//! The `Q`-valued buffers — light, velocity, the velocity potential. `Q` has a
//! private representation and no byte door: the only way out of it is
//! `debug_f64`, documented as a debug door, whose value is *mode dependent*.
//! Writing it would make the format silently different under `FIXED`, in a file
//! that records no numeric mode.
// TODO(snapshot-q): three things have to be decided together — a
// mode-independent byte door for `Q`, a numeric-mode field in the snapshot
// header, and whether the derived fields need storing at all (light is
// recomputed by its kernel every tick and the velocity field is prescribed, so
// they may be reconstructible rather than restorable). None of it is in the
// corpus; ADR-016 promises "restart from any snapshot" and stops there. Until it
// is decided this file holds the `M`-valued state, which is the state the
// invariant of ADR-003 is taken over, and the omission is loud here rather than
// silent in a restarted run.
//!
//! # What is deliberately still missing
//!
// TODO(snapshot-offthread): ADR-037 requires the write to happen off the main
// thread and prices it: "either a copy of the buffers or double buffering of the
// write. 449 MB of copy is noticeable memory, and that has to be measured rather
// than assumed". Both halves of this file are in-process and synchronous. The
// choice between the two schemes is a measurement nobody has taken, and picking
// one here would settle it by accident.
// TODO(snapshot-layout): the format version below has no rule for when it moves,
// the file is uncompressed, and the grid and registry are checked against the
// reloaded config rather than embedded. Endianness is little, chosen and written
// down here because a format has to have one; none of it is in a record.

use std::io::{Read, Write};

use anyhow::{Context, Result, bail};

use crate::ledger::{Channel, Ledger};
use crate::numeric::{M32, M64};
use crate::world::{Direction, Field, LaneRef, World};

/// The first eight bytes of a snapshot.
pub const SNAPSHOT_MAGIC: [u8; 8] = *b"LIMSNAP\x00";

/// The layout version of the file, independent of everything else in the
/// project: it moves when these bytes are arranged differently, not when the
/// world's semantics change.
pub const SNAPSHOT_FORMAT_VERSION: u32 = 1;

/// How many elements are converted to bytes at a time. A whole lane at 256 cubed
/// is 134 MB; this is 32 kB.
const CHUNK: usize = 4096;

/// The run identity of ADR-020 plus where the run stood.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotIdentity {
    pub seed: u64,
    pub config_hash: String,
    pub world_format_version: u32,
    /// The tick the state is of. Not part of the identity — two snapshots of one
    /// run differ in it — and ignored by [`snapshot_read_into`] on the expected
    /// side, which returns the file's own instead.
    pub tick: u32,
}

/// Write the full state of a run.
///
/// # Errors
///
/// Whatever the sink returns.
pub fn snapshot_write<W: Write>(
    mut sink: W,
    id: &SnapshotIdentity,
    world: &World,
    ledger: &Ledger,
) -> Result<()> {
    let registry = world.registry();
    let grid = world.grid();

    sink.write_all(&SNAPSHOT_MAGIC)
        .context("writing the snapshot magic")?;
    put_u32(&mut sink, SNAPSHOT_FORMAT_VERSION)?;
    put_u64(&mut sink, id.seed)?;
    put_u32(&mut sink, id.world_format_version)?;
    put_u32(&mut sink, id.tick)?;
    put_bytes(&mut sink, id.config_hash.as_bytes())?;

    put_u32(&mut sink, registry.n_substances())?;
    put_u32(&mut sink, grid.n_voxels())?;
    put_u32(&mut sink, world.enthalpy_grid().n_voxels())?;
    put_u32(&mut sink, ledger.n_substances())?;

    // Substance index order (ADR-056), both buffers of each (ADR-057).
    for s in 0..registry.n_substances() {
        match world.lane_of(s) {
            LaneRef::Narrow(lane) => {
                let field = world
                    .amounts_32()
                    .expect("a narrow lane exists only where the narrow field does");
                put_m32(&mut sink, field.lane(lane))?;
                put_m32(&mut sink, field.lane_write(lane))?;
            }
            LaneRef::Wide(lane) => {
                let field = world
                    .amounts_64()
                    .expect("a wide lane exists only where the wide field does");
                put_m64(&mut sink, field.lane(lane))?;
                put_m64(&mut sink, field.lane_write(lane))?;
            }
        }
    }

    put_m64(&mut sink, world.enthalpy().lane(0))?;
    put_m64(&mut sink, world.enthalpy().lane_write(0))?;
    put_m64(&mut sink, world.energy_delta())?;

    for channel in Channel::ALL {
        for s in 0..ledger.n_substances() {
            put_i64(&mut sink, ledger.matter(channel, s))?;
        }
    }
    for channel in Channel::ALL {
        put_i64(&mut sink, ledger.energy(channel))?;
    }

    sink.flush().context("flushing the snapshot")?;
    Ok(())
}

/// Pour a snapshot into a world built from the same scenario, and return the
/// tick it was taken at.
///
/// # Errors
///
/// Returns an error naming which of `seed`, `config_hash` or
/// `world_format_version` differs; if the file's shape does not match the world;
/// if the magic or the format version is not this one; or if the ledger handed
/// in has already counted something — a restart pours into a fresh ledger, and
/// crediting on top of a used one would leave the counters holding the sum of
/// two runs.
pub fn snapshot_read_into<R: Read>(
    mut src: R,
    expect: &SnapshotIdentity,
    world: &mut World,
    ledger: &mut Ledger,
) -> Result<u32> {
    let mut magic = [0u8; 8];
    src.read_exact(&mut magic)
        .context("reading the snapshot magic")?;
    if magic != SNAPSHOT_MAGIC {
        bail!("this is not a liminis snapshot: the first eight bytes are {magic:?}");
    }
    let format = get_u32(&mut src)?;
    if format != SNAPSHOT_FORMAT_VERSION {
        bail!(
            "the snapshot is format version {format} and this build reads \
             {SNAPSHOT_FORMAT_VERSION}"
        );
    }

    let seed = get_u64(&mut src)?;
    let world_format_version = get_u32(&mut src)?;
    let tick = get_u32(&mut src)?;
    let config_hash = String::from_utf8(get_bytes(&mut src)?)
        .context("the config hash in the snapshot is not text")?;

    // All three of ADR-020, and all three named at once: a restart that failed
    // on the seed and would also have failed on the hash should say so in one
    // go rather than in three runs.
    let mut wrong = Vec::new();
    if seed != expect.seed {
        wrong.push(format!("seed {seed} against {}", expect.seed));
    }
    if config_hash != expect.config_hash {
        wrong.push(format!(
            "config_hash {config_hash} against {}",
            expect.config_hash
        ));
    }
    if world_format_version != expect.world_format_version {
        wrong.push(format!(
            "world_format_version {world_format_version} against {}",
            expect.world_format_version
        ));
    }
    if !wrong.is_empty() {
        bail!(
            "the snapshot belongs to another run: {}. A run is identified by \
             (seed, config_hash, world_format_version) (ADR-020), and restarting \
             across the difference would produce a trajectory carrying one \
             file's provenance and another file's physics",
            wrong.join(", ")
        );
    }

    let n_substances = get_u32(&mut src)?;
    let n_voxels = get_u32(&mut src)?;
    let n_enthalpy_cells = get_u32(&mut src)?;
    let ledger_substances = get_u32(&mut src)?;
    expect_shape("substances", n_substances, world.registry().n_substances())?;
    expect_shape("voxels", n_voxels, world.grid().n_voxels())?;
    expect_shape(
        "enthalpy cells",
        n_enthalpy_cells,
        world.enthalpy_grid().n_voxels(),
    )?;
    expect_shape(
        "ledger substances",
        ledger_substances,
        ledger.n_substances(),
    )?;

    for channel in Channel::ALL {
        for s in 0..ledger.n_substances() {
            if ledger.matter(channel, s) != 0 {
                bail!(
                    "the ledger handed in has already counted {} of substance {s} \
                     through {}: a restart fills a fresh ledger, and crediting on \
                     top of a used one leaves the counters holding the sum of two \
                     runs",
                    ledger.matter(channel, s),
                    channel.name()
                );
            }
        }
        if ledger.energy(channel) != 0 {
            bail!(
                "the ledger handed in has already counted energy through {}",
                channel.name()
            );
        }
    }

    for s in 0..world.registry().n_substances() {
        match world.lane_of(s) {
            LaneRef::Narrow(lane) => {
                let field = world
                    .amounts_32_mut()
                    .expect("a narrow lane exists only where the narrow field does");
                get_field_32(&mut src, field, lane)?;
            }
            LaneRef::Wide(lane) => {
                let field = world
                    .amounts_64_mut()
                    .expect("a wide lane exists only where the wide field does");
                get_field_64(&mut src, field, lane)?;
            }
        }
    }

    get_field_64(&mut src, world.enthalpy_mut(), 0)?;
    get_m64(&mut src, world.energy_delta_mut())?;

    for channel in Channel::ALL {
        for s in 0..ledger.n_substances() {
            let units = get_i64(&mut src)?;
            ledger.credit_matter(channel, s, units);
        }
    }
    for channel in Channel::ALL {
        let joules = get_i64(&mut src)?;
        ledger.credit_energy(channel, joules);
    }
    // A snapshot is taken at a tick boundary, so the restored ledger stands at
    // one: everything in the file is the past, and the next tick's residual is
    // measured from here.
    ledger.begin_tick();

    Ok(tick)
}

fn expect_shape(what: &str, in_file: u32, in_world: u32) -> Result<()> {
    if in_file != in_world {
        bail!(
            "the snapshot holds {in_file} {what} and the world built from the \
             config has {in_world}"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Bytes. Little-endian throughout — see the TODO in the module header.
// ---------------------------------------------------------------------------

fn put_u32<W: Write>(sink: &mut W, value: u32) -> Result<()> {
    sink.write_all(&value.to_le_bytes())
        .context("writing a snapshot field")?;
    Ok(())
}

fn put_u64<W: Write>(sink: &mut W, value: u64) -> Result<()> {
    sink.write_all(&value.to_le_bytes())
        .context("writing a snapshot field")?;
    Ok(())
}

fn put_i64<W: Write>(sink: &mut W, value: i64) -> Result<()> {
    sink.write_all(&value.to_le_bytes())
        .context("writing a snapshot field")?;
    Ok(())
}

fn put_bytes<W: Write>(sink: &mut W, bytes: &[u8]) -> Result<()> {
    let len = u32::try_from(bytes.len()).context("a snapshot string longer than a u32")?;
    put_u32(sink, len)?;
    sink.write_all(bytes).context("writing a snapshot string")?;
    Ok(())
}

fn put_m32<W: Write>(sink: &mut W, lane: &[M32]) -> Result<()> {
    let mut buf = Vec::with_capacity(CHUNK * 4);
    for chunk in lane.chunks(CHUNK) {
        buf.clear();
        for value in chunk {
            buf.extend_from_slice(&value.raw().to_le_bytes());
        }
        sink.write_all(&buf).context("writing a snapshot lane")?;
    }
    Ok(())
}

fn put_m64<W: Write>(sink: &mut W, lane: &[M64]) -> Result<()> {
    let mut buf = Vec::with_capacity(CHUNK * 8);
    for chunk in lane.chunks(CHUNK) {
        buf.clear();
        for value in chunk {
            buf.extend_from_slice(&value.raw().to_le_bytes());
        }
        sink.write_all(&buf).context("writing a snapshot lane")?;
    }
    Ok(())
}

fn get_u32<R: Read>(src: &mut R) -> Result<u32> {
    let mut buf = [0u8; 4];
    src.read_exact(&mut buf)
        .context("reading a snapshot field")?;
    Ok(u32::from_le_bytes(buf))
}

fn get_u64<R: Read>(src: &mut R) -> Result<u64> {
    let mut buf = [0u8; 8];
    src.read_exact(&mut buf)
        .context("reading a snapshot field")?;
    Ok(u64::from_le_bytes(buf))
}

fn get_i64<R: Read>(src: &mut R) -> Result<i64> {
    let mut buf = [0u8; 8];
    src.read_exact(&mut buf)
        .context("reading a snapshot field")?;
    Ok(i64::from_le_bytes(buf))
}

fn get_bytes<R: Read>(src: &mut R) -> Result<Vec<u8>> {
    let len = get_u32(src)? as usize;
    let mut out = vec![0u8; len];
    src.read_exact(&mut out)
        .context("reading a snapshot string")?;
    Ok(out)
}

fn get_m32<R: Read>(src: &mut R, lane: &mut [M32]) -> Result<()> {
    let mut buf = vec![0u8; CHUNK * 4];
    for chunk in lane.chunks_mut(CHUNK) {
        let bytes = &mut buf[..chunk.len() * 4];
        src.read_exact(bytes).context("reading a snapshot lane")?;
        for (value, raw) in chunk.iter_mut().zip(bytes.chunks_exact(4)) {
            *value = M32::new(i32::from_le_bytes(raw.try_into().expect("four bytes")));
        }
    }
    Ok(())
}

fn get_m64<R: Read>(src: &mut R, lane: &mut [M64]) -> Result<()> {
    let mut buf = vec![0u8; CHUNK * 8];
    for chunk in lane.chunks_mut(CHUNK) {
        let bytes = &mut buf[..chunk.len() * 8];
        src.read_exact(bytes).context("reading a snapshot lane")?;
        for (value, raw) in chunk.iter_mut().zip(bytes.chunks_exact(8)) {
            *value = M64::new(i64::from_le_bytes(raw.try_into().expect("eight bytes")));
        }
    }
    Ok(())
}

/// Fill both buffers of one lane, front first.
///
/// `lane_pair_dir_mut` hands out `(state N, state N+1)` in the direction asked
/// for, so `Backward` is how the front buffer is written without a swap — and
/// without a swap there is no order to get wrong between the two halves.
fn get_field_32<R: Read>(src: &mut R, field: &mut Field<M32>, lane: u32) -> Result<()> {
    let (_, front) = field.lane_pair_dir_mut(lane, Direction::Backward);
    get_m32(src, front)?;
    let (_, back) = field.lane_pair_dir_mut(lane, Direction::Forward);
    get_m32(src, back)
}

fn get_field_64<R: Read>(src: &mut R, field: &mut Field<M64>, lane: u32) -> Result<()> {
    let (_, front) = field.lane_pair_dir_mut(lane, Direction::Backward);
    get_m64(src, front)?;
    let (_, back) = field.lane_pair_dir_mut(lane, Direction::Forward);
    get_m64(src, back)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{Boundary, Grid, Registry, SubstanceDecl, Width, WorldLayout};

    const LAYOUT: WorldLayout = WorldLayout {
        enthalpy_lod: 1,
        velocity_lod: 1,
    };

    fn registry() -> Registry {
        Registry::new(&[
            SubstanceDecl {
                id: "WATER".into(),
                width: Width::Bits64,
                k: 29,
            },
            SubstanceDecl {
                id: "O2".into(),
                width: Width::Bits32,
                k: 15,
            },
        ])
        .unwrap()
    }

    fn world() -> World {
        let grid = Grid::new(4, 4, 4, [Boundary::Closed; 6]).unwrap();
        World::new(grid, registry(), &LAYOUT).unwrap()
    }

    fn identity() -> SnapshotIdentity {
        SnapshotIdentity {
            seed: 42,
            config_hash: "b3sum".into(),
            world_format_version: 4,
            tick: 1_234,
        }
    }

    /// Fill every buffer with values that differ between the two buffers and
    /// between lanes, so that a swapped, shared or dropped buffer shows up.
    fn fill(world: &mut World) {
        let narrow = world.amounts_32_mut().unwrap();
        for (i, value) in narrow.write_mut().iter_mut().enumerate() {
            *value = M32::new(-1_000 - i as i32);
        }
        narrow.swap();
        for (i, value) in narrow.write_mut().iter_mut().enumerate() {
            *value = M32::new(7_000 + i as i32);
        }

        let wide = world.amounts_64_mut().unwrap();
        for (i, value) in wide.write_mut().iter_mut().enumerate() {
            *value = M64::new(5_000_000_000 + i as i64);
        }
        wide.swap();
        for (i, value) in wide.write_mut().iter_mut().enumerate() {
            *value = M64::new(-9_000_000_000 - i as i64);
        }

        let enthalpy = world.enthalpy_mut();
        for (i, value) in enthalpy.write_mut().iter_mut().enumerate() {
            *value = M64::new(11 + i as i64);
        }
        enthalpy.swap();
        for (i, value) in enthalpy.write_mut().iter_mut().enumerate() {
            *value = M64::new(-22 - i as i64);
        }

        for (i, value) in world.energy_delta_mut().iter_mut().enumerate() {
            *value = M64::new(333 + i as i64);
        }
    }

    fn ledger() -> Ledger {
        let mut ledger = Ledger::new(2).unwrap();
        ledger.credit_matter(Channel::GeothermalIn, 1, 7_777);
        ledger.credit_matter(Channel::Impact, 0, -13);
        ledger.credit_energy(Channel::SolarIn, 4_200);
        ledger
    }

    #[test]
    fn a_snapshot_round_trips_both_buffers_of_every_field() {
        // What storing `read()` alone would cost is invisible to every
        // conservation test in the project: the restored back buffer is zeroes,
        // the next swap promotes them for whatever the tick did not write,
        // `restore_lane` copies `back -> front`, and both residuals go on
        // closing, because a stale state is conserved exactly as well as a fresh
        // one (ADR-057, `field.rs`).
        let mut source = world();
        fill(&mut source);
        let source_ledger = ledger();

        let mut bytes = Vec::new();
        snapshot_write(&mut bytes, &identity(), &source, &source_ledger).unwrap();

        let mut restored = world();
        let mut restored_ledger = Ledger::new(2).unwrap();
        let tick = snapshot_read_into(&bytes[..], &identity(), &mut restored, &mut restored_ledger)
            .unwrap();

        assert_eq!(tick, 1_234);
        // `Field`'s equality covers both `Vec`s, which is the whole point.
        assert_eq!(restored.amounts_32(), source.amounts_32());
        assert_eq!(restored.amounts_64(), source.amounts_64());
        assert_eq!(restored.enthalpy(), source.enthalpy());
        assert_eq!(restored.energy_delta(), source.energy_delta());

        for channel in Channel::ALL {
            for s in 0..2 {
                assert_eq!(
                    restored_ledger.matter(channel, s),
                    source_ledger.matter(channel, s),
                    "{} substance {s}",
                    channel.name()
                );
            }
            assert_eq!(
                restored_ledger.energy(channel),
                source_ledger.energy(channel)
            );
        }
    }

    #[test]
    fn a_snapshot_of_another_run_is_refused() {
        let mut source = world();
        fill(&mut source);
        let mut bytes = Vec::new();
        snapshot_write(&mut bytes, &identity(), &source, &ledger()).unwrap();

        for (name, wrong) in [
            (
                "seed",
                SnapshotIdentity {
                    seed: 43,
                    ..identity()
                },
            ),
            (
                "config_hash",
                SnapshotIdentity {
                    config_hash: "another".into(),
                    ..identity()
                },
            ),
            (
                "world_format_version",
                SnapshotIdentity {
                    world_format_version: 5,
                    ..identity()
                },
            ),
        ] {
            let mut restored = world();
            let mut restored_ledger = Ledger::new(2).unwrap();
            let error = snapshot_read_into(&bytes[..], &wrong, &mut restored, &mut restored_ledger)
                .unwrap_err()
                .to_string();
            assert!(error.contains(name), "{name} was not named: {error}");
        }

        // The tick is not part of the identity: two snapshots of one run differ
        // in it, and a restart that refused on it could never resume.
        let mut restored = world();
        let mut restored_ledger = Ledger::new(2).unwrap();
        assert!(
            snapshot_read_into(
                &bytes[..],
                &SnapshotIdentity {
                    tick: 0,
                    ..identity()
                },
                &mut restored,
                &mut restored_ledger,
            )
            .is_ok()
        );
    }

    #[test]
    fn a_snapshot_poured_into_a_used_ledger_is_refused() {
        let mut source = world();
        fill(&mut source);
        let mut bytes = Vec::new();
        snapshot_write(&mut bytes, &identity(), &source, &ledger()).unwrap();

        let mut restored = world();
        let mut used = ledger();
        let error = snapshot_read_into(&bytes[..], &identity(), &mut restored, &mut used)
            .unwrap_err()
            .to_string();
        assert!(error.contains("already counted"), "{error}");
    }
}
