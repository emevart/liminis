//! The world in bytes: the four routes the viewer asks for (ADR-070).
//!
//! `http.rs` is the transport, `main.rs` is the router and this is the only
//! place where a `World` turns into something a browser can draw. One file, for
//! the reason `observe/json.rs` is one file: the rules that decide whether a
//! picture is honest — quantise against the *declared* ceiling, read the front
//! buffer, publish a tick and its residual under one lock — hold for every
//! route, and a rule that lives at four call sites holds at three of them.
//!
//! # A frame is a snapshot between ticks, never a window into one
//!
//! The simulation runs in a thread of its own behind a mutex, and the mutex is
//! held for a whole `Tick::advance` or for a whole answer and never across the
//! boundary. That is ADR-057 read from the reader's side: on completion of a
//! process the front buffer holds state `N` for **every** lane, so a route that
//! took the lock mid-tick would see lanes in different states — a picture that
//! looks entirely plausible, over a residual that closes, because `advance` goes
//! on to finish.
//!
//! For the same reason the tick counter and the residual are serialised
//! together, under one acquisition. Published separately they drift apart, and a
//! hole gets attributed to the wrong tick — or, worse, last tick's zero stands
//! beside this tick's counter.
//!
//! # What the ledger half owes, and why it is computed here
//!
//! ADR-059 puts the domain-sum reduction behind `cfg(debug_assertions)`, so
//! `Scratch::before`/`after` are, in a release build, whatever they were
//! initialised to. `liminis serve` is a release build. Taking the residual from
//! there would print `0 - 0 - credited`: an exact zero on a closed domain and a
//! silent lie on the first channel that fires — the fabricated zero ADR-037
//! forbids by name. So the loop performs `domain_sums` → `advance` →
//! `domain_sums` itself, in both profiles, and publishes what it computed.
//!
//! The aggregate over substances is the largest residual by magnitude, with its
//! sign, and never the sum: `+5` on one substance and `-5` on another sum to a
//! green zero, and nothing else in the corpus would catch it — `assert_closed`
//! walks substances one at a time and lives in another build.
//!
//! # No path reaches the filesystem
//!
//! Inherited from `http.rs` and worth restating where the names come in: the
//! substance name in a URL addresses the registry, never a path. An unknown name
//! is a 404 that says which name, and never a volume of zeros of the right
//! length — that picture is indistinguishable from a dead world.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use liminis_core::config::{self, Config, Derived};
use liminis_core::ledger::{DomainSums, Ledger};
use liminis_core::numeric::{M32, M64, run_key};
use liminis_core::process::{Footprint, ProcessId, ROSTER_LEN, RosterEntry, Scratch, Tick};
use liminis_core::version::WORLD_FORMAT_VERSION;
use liminis_core::world::{Boundary, Face, Grid, LaneRef, Registry, World, WorldLayout};
use liminis_core::worldgen;

use crate::http::{Request, Response};

#[path = "ecology.rs"]
mod ecology;

/// The magic of the volume payload: "LMNV", little-endian, as the viewer reads
/// it.
const VOLUME_MAGIC: u32 = 0x4C4D_4E56;

/// Bytes before the first voxel: magic, `nx`, `ny`, `nz`, `scale` as an `f32`,
/// and four bytes of padding.
///
/// The viewer reads `DataView(buf, 0, 24)` and `Uint8Array(buf, 24)`, and it
/// does not check the split: a header off by one byte draws shifted data as a
/// perfectly plausible cloud.
const VOLUME_HEADER: usize = 24;

/// The unit every amount is measured in (`QUANTITIES.md` section 2), spelled the
/// way the viewer spells it.
const UNIT: &str = "mol/m^3";

/// Where the tick rate starts, in ticks a second (ADR-070).
///
/// The number is the viewer's, not a physical quantity: the page declares
/// `value="30"` on its slider and sends no `speed` until the slider is moved, so
/// any other starting rate would silently disagree with its own label.
const DEFAULT_TARGET_TPS: f64 = 30.0;

/// The range of `speed`, the same bounds the page's slider declares.
const MIN_TPS: f64 = 1.0;
const MAX_TPS: f64 = 200.0;

/// How long the loop sleeps between two looks at a paused simulation.
const IDLE: Duration = Duration::from_millis(2);

/// The window the reported rate is measured over.
///
/// Measured and not requested: a rate taken from the slider makes a simulator
/// that cannot keep up look healthy (ADR-070).
const RATE_WINDOW: Duration = Duration::from_millis(250);

/// The world, the tick, the counters and the frame that gets published — all
/// under one mutex.
///
/// Held for exactly one tick or exactly one answer, never across `advance`.
pub struct Sim {
    scenario: Config,
    ecology: ecology::Ecology,
    error: Option<String>,
    world: World,
    tick: Tick,
    ledger: Ledger,
    scratch: Scratch,
    /// The left-hand side of the invariant before the tick, and after it. Two
    /// accumulators reused rather than allocated twice a tick, exactly as
    /// `ledger/mod.rs` intends them.
    before: DomainSums,
    after: DomainSums,
    fields: Vec<FieldMeta>,
    identity: Identity,
    /// Voxel edge, metres. The grid does not carry it — it holds extents and
    /// boundaries and nothing dimensional — and the viewer stamps it under the
    /// specimen.
    dx: f64,
    /// `dx^3`, cubic metres: what a profile divides by to reach mol/m^3.
    v_voxel: f64,
    /// The public tick counter. `u32`, because that is what `Tick::advance`
    /// takes and what `every_n_ticks` is computed against; a `u64` counter
    /// published beside a `u32` phase would part company at 2^32 with nothing
    /// saying so.
    ticks: u32,
    running: bool,
    target_tps: f64,
    measured_tps: f64,
    /// The residuals of the last *completed* tick. `None` is "nothing to
    /// report", and it is not a zero (ADR-071).
    last: Option<Residual>,
    /// Whether the simulation thread is still alive. See [`SimAlive`].
    alive: bool,
}

/// Everything a route knows about one substance.
struct FieldMeta {
    id: String,
    unit: &'static str,
    /// The declared ceiling, mol/m^3 (ADR-041). One byte of a volume is
    /// `max_conc / 255` of it, and the profile is indexed to it.
    max_conc: f64,
    /// The same ceiling in storage units, which is what a byte is computed
    /// against (ADR-072).
    amount_at_max: i128,
    /// `log2(units_per_mol)` of this substance (ADR-039). A profile divides by
    /// it once, at the end.
    k: u8,
    /// The buffer address, and it comes from `World::lane_of` and from nowhere
    /// else: `lane == s` is right at `s == 0` and wrong from there on (ADR-056).
    lane: LaneRef,
}

/// The triple of ADR-020: a run is identified by seed, config hash and world
/// format version.
struct Identity {
    scenario: String,
    seed: u64,
    config_hash: String,
    world_format_version: u32,
}

/// The residuals of one tick. `energy: None` is "nothing to show", not zero
/// (ADR-071).
struct Residual {
    matter: i128,
    energy: Option<i128>,
}

/// Serve the viewer and the world behind it on a local port.
///
/// # Errors
///
/// Returns an error if the scenario cannot be read, validated or built into a
/// world, or if the port cannot be bound.
pub fn run(port: u16, config: &Path, seed: u64) -> Result<()> {
    let scenario = config::load(config)?;
    let sim = build(&scenario, seed)
        .with_context(|| format!("building a world out of {}", config.display()))?;

    let identity = format!(
        "seed={} config_hash={} world_format_version={}",
        sim.identity.seed, sim.identity.config_hash, sim.identity.world_format_version
    );
    let shared = Arc::new(Mutex::new(sim));

    let listener = std::net::TcpListener::bind(("127.0.0.1", port))?;
    println!("liminis viewer on http://127.0.0.1:{port}/");
    println!("{identity}");

    let ticking = Arc::clone(&shared);
    std::thread::Builder::new()
        .name("liminis-sim".into())
        .spawn(move || run_loop(&ticking))
        .context("spawning the simulation thread")?;

    // The router lives in `main.rs` and not here: the viewer page, the 404 and
    // the 405 are the binary's surface, and this module owns only the half that
    // turns a world into bytes.
    let routes = Arc::clone(&shared);
    crate::http::serve(listener, move |request| crate::route(&routes, request))?;
    Ok(())
}

/// Build everything one run needs out of a parsed scenario.
///
/// # Errors
///
/// Returns an error if the scenario does not validate, if its faces cannot be
/// built into a grid, or if a process it enables cannot be dispatched.
fn build(scenario: &Config, seed: u64) -> Result<Sim> {
    let derived = config::validate(scenario).context("validating the scenario")?;
    let config_hash = config::config_hash(scenario).context("hashing the scenario")?;

    let grid = grid_of(scenario).context("the grid of the scenario")?;
    let registry = Registry::new(&derived.decls()).context("the substance registry")?;
    let n_substances = registry.n_substances();

    let mut world = World::new(
        grid,
        registry,
        &WorldLayout {
            enthalpy_lod: u32::from(derived.enthalpy_field().lod),
            // TODO(velocity-lod): no config key declares the coarsening of the
            // velocity field — `world/world.rs` says so at length — so the one
            // in the journal's prose is passed here rather than defaulted to
            // something plausible. ADR-069 names 64^3 against a 128^3 base grid,
            // which is one bit.
            velocity_lod: 1,
        },
    )
    .context("allocating the world")?;

    worldgen::generate(&mut world, &derived, run_key(seed)).context("the initial conditions")?;

    // The ghost cell of every lane, out of `[boundary.reservoir]` (ADR-059).
    //
    // **After worldgen and not before**, because worldgen writes voxels and this
    // writes the element past them; the order does not matter today and saying
    // which is which does. What matters is that it happens at all: an unseeded
    // ghost is a reservoir of nothing, so a lid declared to trade with an
    // atmosphere-saturated ocean would be an infinite sink instead — and every
    // check in the project would stay green over it, because the counter records
    // whatever actually left.
    //
    // A restart re-seeds it the same way rather than reading it back out of a
    // snapshot: the reservoir is a boundary condition of the config and not
    // state (`observe/snapshot.rs`, ADR-059).
    if let Some(reservoir) = derived.reservoir() {
        world
            .seed_ghosts(&reservoir.amount_out, M64::new(reservoir.enthalpy_out))
            .context("seeding the outside reservoir")?;
    }

    let tick = Tick::new(
        &world,
        &derived,
        scenario,
        &roster_of(scenario)?,
        scenario.dt,
        scenario.grid.dx,
        seed,
    )
    .context("folding the tick")?;
    // The footprint line of the load report, **above** the allocation it
    // describes and not below it (ADR-086). ADR-086 refuses a memory ceiling in
    // bytes — every factor of the product is bounded already and the limit itself
    // would have to be invented — and offers this line instead, so that a run at
    // `R = 64` reads its gigabyte before it asks the machine for it. Printed
    // beside the identity, which is the other thing a run has to be able to
    // quote afterwards.
    print!(
        "{}",
        Footprint::of(&world, &tick)
            .context("the memory footprint of the run")?
            .report()
    );

    let scratch = Scratch::new(&world, &tick).context("the scratch buffers")?;
    let fields = fields_of(&world, &derived);
    let mut ecology = ecology::Ecology::new(scenario);
    ecology.observe(&world, &fields, 0);

    Ok(Sim {
        scenario: scenario.clone(),
        ecology,
        error: None,
        fields,
        world,
        tick,
        ledger: Ledger::with_reactions(n_substances, scenario.reaction.len() as u32)
            .context("the ledger")?,
        scratch,
        before: DomainSums::new(n_substances).context("the domain sums before a tick")?,
        after: DomainSums::new(n_substances).context("the domain sums after a tick")?,
        identity: Identity {
            scenario: scenario.name.clone(),
            seed,
            config_hash,
            world_format_version: WORLD_FORMAT_VERSION,
        },
        dx: scenario.grid.dx,
        v_voxel: derived.v_voxel(),
        ticks: 0,
        running: true,
        target_tps: DEFAULT_TARGET_TPS,
        measured_tps: 0.0,
        last: None,
        alive: true,
    })
}

/// The grid of a scenario: extents and one boundary per face.
///
/// The translation between the spelling a scenario writes and the one the world
/// runs is here and not in `config/`, because it is `Grid::new` that judges the
/// set of faces — a half-periodic axis, and an `exchange` face opposite a
/// periodic one (ADR-034, ADR-059) — and the refusal is worth arriving with the
/// context of which scenario asked for it.
fn grid_of(scenario: &Config) -> Result<Grid> {
    let face = |declared: config::Face| match declared {
        config::Face::Periodic => Boundary::Periodic,
        config::Face::Closed => Boundary::Closed,
        config::Face::Exchange => Boundary::Exchange,
    };
    let boundary = scenario.boundary.clone();
    let mut faces = [Boundary::Closed; 6];
    faces[Face::XMinus as usize] = face(boundary.x_min);
    faces[Face::XPlus as usize] = face(boundary.x_max);
    faces[Face::YMinus as usize] = face(boundary.y_min);
    faces[Face::YPlus as usize] = face(boundary.y_max);
    faces[Face::ZMinus as usize] = face(boundary.z_min);
    faces[Face::ZPlus as usize] = face(boundary.z_max);

    Grid::new(scenario.grid.nx, scenario.grid.ny, scenario.grid.nz, faces)
}

/// The materialised roster of ADR-065, as the tick takes it.
///
/// Nothing in `liminis-core` turns a `Config` into this array — `process/tick.rs`
/// says so — and the reason it is a translation rather than a lookup is that the
/// two live in different crates under different CI guards.
fn roster_of(scenario: &Config) -> Result<[RosterEntry; ROSTER_LEN]> {
    let mut entries = Vec::with_capacity(ROSTER_LEN);
    for record in &scenario.process {
        let id = ProcessId::from_id(&record.id)
            .with_context(|| format!("process `{}` is not in the roster", record.id))?;
        entries.push(RosterEntry {
            id,
            // `parse` materialises the whole roster before hashing (ADR-065), so
            // `None` here means the config never went through it.
            enabled: record.enabled.unwrap_or_else(|| id.enabled_by_default()),
            every_n_ticks: record.every_n_ticks,
        });
    }
    entries.try_into().map_err(|entries: Vec<RosterEntry>| {
        anyhow::anyhow!(
            "the scenario materialised {} process records and the roster holds {ROSTER_LEN}: \
             the config did not come through `config::parse` (ADR-065)",
            entries.len()
        )
    })
}

/// What `/api/state` publishes as `fields`: the substances, and only they.
///
/// The enthalpy field is deliberately absent (ADR-072): it has no declared
/// ceiling in the sense `max_conc` means, and `DerivedEnergy::h_max` has not been
/// decided to be one.
fn fields_of(world: &World, derived: &Derived) -> Vec<FieldMeta> {
    derived
        .substances()
        .iter()
        .enumerate()
        .map(|(s, substance)| FieldMeta {
            id: substance.id.clone(),
            unit: UNIT,
            max_conc: max_conc_of(substance.amount_at_max, substance.k, derived.v_voxel()),
            amount_at_max: substance.amount_at_max,
            k: substance.k,
            lane: world.lane_of(s as u32),
        })
        .collect()
}

/// The declared ceiling in mol/m^3, back out of the units it was derived into.
///
/// Derived rather than copied off the scenario for one reason: `amount_at_max`
/// is what a byte is computed against, so the number the page is told the ramp
/// means has to be the same one, and not a second reading of the config that can
/// disagree with it by a rounding.
fn max_conc_of(amount_at_max: i128, k: u8, v_voxel: f64) -> f64 {
    amount_at_max as f64 / (f64::from(k).exp2() * v_voxel)
}

/// The lock, and what it does about a poisoned mutex.
///
/// A poisoned mutex means the simulation thread died holding it — in a debug
/// build that is `Ledger::assert_closed` inside `Tick::advance`, which is to say
/// exactly the case this whole apparatus exists for. Going silent then would
/// read to the page as "no simulator on this port", which blames the connection.
/// So the routes go on answering, with `running: false` and no residual.
fn lock(shared: &Arc<Mutex<Sim>>) -> MutexGuard<'_, Sim> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The sentinel that outlives a panicking simulation thread.
///
/// Without it the HTTP side keeps serving the last frame: a frozen counter,
/// `running: true` and last tick's zero residual — which is what ADR-037 calls
/// indistinguishable from a check that was switched off, only under a green
/// banner.
struct SimAlive(Arc<Mutex<Sim>>);

impl Drop for SimAlive {
    fn drop(&mut self) {
        let mut sim = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        sim.alive = false;
        sim.running = false;
        sim.measured_tps = 0.0;
        // The residual of a tick that may not have finished is not a residual.
        sim.last = None;
    }
}

/// The simulation thread: one tick at a time, paced at the requested rate.
fn run_loop(shared: &Arc<Mutex<Sim>>) {
    let _alive = SimAlive(Arc::clone(shared));
    let mut mark = Instant::now();
    let mut since = 0u32;

    loop {
        let started = Instant::now();
        let period = {
            let mut sim = lock(shared);
            if !sim.running || !sim.alive {
                sim.measured_tps = 0.0;
                drop(sim);
                mark = Instant::now();
                since = 0;
                std::thread::sleep(IDLE);
                continue;
            }
            if !safe_advance(&mut sim) {
                continue;
            }
            since += 1;
            let elapsed = mark.elapsed();
            if elapsed >= RATE_WINDOW {
                sim.measured_tps = f64::from(since) / elapsed.as_secs_f64();
                mark = Instant::now();
                since = 0;
            }
            Duration::from_secs_f64(1.0 / sim.target_tps.max(MIN_TPS))
        };

        // Outside the lock: a route must be able to answer while the loop waits
        // for its next tick.
        //
        // The `else` is not a rounding case, and leaving it empty is what made
        // the viewer unusable. When the world cannot keep up with the requested
        // rate — which is the normal state at any interesting grid size — the
        // period has already elapsed by the time the tick returns, there is
        // nothing to sleep off, and the loop takes the lock again immediately.
        // A std mutex is not fair: a thread that re-locks at once keeps it, and
        // every waiting request starves. Measured before this line existed: a
        // 62 ms tick, and `/api/volume` answering in fourteen seconds.
        //
        // Yielding is enough and a sleep would not be better. The scheduler only
        // has to run the waiters once; they need the lock for the length of one
        // frame copy, not for a quantum.
        match period.checked_sub(started.elapsed()) {
            Some(rest) => std::thread::sleep(rest),
            None => std::thread::yield_now(),
        }
    }
}

/// One tick, with the two accumulators of the invariant filled **by this loop**
/// rather than taken from `Scratch`.
///
/// See the module header: `Scratch::before`/`after` are filled in a debug build
/// only (ADR-059), and `liminis serve` runs in release.
///
/// # The one line of this that no test holds
///
/// `tick.domain_sums(world, after)` standing *below* `tick.advance` is the whole
/// meaning of the word, and moving it above changes nothing in the corpus. It
/// cannot: every process S0 dispatches conserves each substance exactly, so the
/// two sums are equal on every tick this build can produce, and telling them
/// apart would need a tick whose domain total moves — which needs a channel that
/// credits. The `exchange` face credits one now (ADR-059), so a scenario whose
/// lid vents is exactly the case that can tell the two orders apart, and this
/// stops being held by reading alone the moment such a scenario is run under a
/// debug build.
fn advance_one(sim: &mut Sim) {
    {
        let Sim {
            world,
            tick,
            ledger,
            scratch,
            before,
            after,
            identity,
            ticks,
            ..
        } = sim;

        tick.domain_sums(world, before);
        tick.advance(world, ledger, scratch, *ticks, run_key(identity.seed));
        tick.domain_sums(world, after);
    }

    // Read off the accumulators the three lines above filled, and through a
    // function rather than in place — see [`residual_of`] for why the split is
    // the difference between a computed zero and a written one.
    let residual = residual_of(sim);
    sim.last = Some(residual);

    // Wrapping, and it is the counter `Tick::advance` takes: a wider public
    // counter would disagree with the phase of `every_n_ticks` at 2^32 and say
    // nothing about it.
    sim.ticks = sim.ticks.wrapping_add(1);
    sim.ecology.observe(&sim.world, &sim.fields, sim.ticks);
}

/// Keep a failed world stopped and observable until an explicit reset.
fn safe_advance(sim: &mut Sim) -> bool {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| advance_one(sim))) {
        Ok(()) => {
            if sim
                .last
                .as_ref()
                .is_some_and(|r| r.matter != 0 || r.energy.is_some_and(|e| e != 0))
            {
                sim.error = Some("the conservation ledger did not close".into());
            }
        }
        Err(error) => {
            sim.error = Some(
                error
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| error.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "the simulation stopped during a tick".into()),
            );
            sim.last = None;
        }
    }
    if sim.error.is_some() {
        sim.running = false;
        sim.alive = false;
        sim.measured_tps = 0.0;
    }
    sim.alive
}

/// The largest signed matter residual, not a sum that could cancel errors.
/// Actual scheduled reaction extents and stoichiometry participate in release
/// builds too. The synthetic-hole test distinguishes a checked zero from a
/// hardcoded one; abiotic transport-only worlds retain ADR-071's energy `None`.
fn residual_of(sim: &Sim) -> Residual {
    let matter = (0..sim.before.n_substances())
        .map(|s| {
            sim.ledger
                .residual_matter(sim.tick.reaction_nu(sim.ticks), s, &sim.before, &sim.after)
        })
        .max_by_key(|residual| residual.unsigned_abs())
        .unwrap_or(0);

    Residual {
        matter,
        energy: if sim.tick.enabled(ProcessId::Reactions) {
            Some(sim.ledger.residual_energy(&sim.before, &sim.after))
        } else {
            None
        },
    }
}

/// The four routes, once `main.rs` has decided the method is the right one.
///
/// Each takes the lock once and answers out of one acquisition: a tick counter
/// and a residual published from two would eventually name different ticks.
pub fn route(shared: &Arc<Mutex<Sim>>, request: &Request) -> Response {
    let path = request.path.as_str();

    if path == "/api/control" {
        let mut sim = lock(shared);
        return apply_control(&mut sim, &request.body);
    }

    if path == "/api/state" {
        let sim = lock(shared);
        return Response::json(state_json(&sim));
    }

    if path == "/api/ecology" {
        let sim = lock(shared);
        let z = request
            .query
            .split('&')
            .find_map(|part| part.strip_prefix("z="));
        let z = match z.map(str::parse::<u32>).transpose() {
            Ok(Some(z)) if z < sim.world.grid().nz() => z,
            Ok(None) => sim.world.grid().nz() / 2,
            _ => return Response::error(400, "z must name a layer inside the grid"),
        };
        return Response::json(sim.ecology.frame(&sim, z).to_string());
    }

    if let Some(field) = path.strip_prefix("/api/volume/") {
        let sim = lock(shared);
        return match volume_of(&sim, field) {
            Some(bytes) => Response::bytes(bytes),
            None => Response::error(404, &no_such_field(&sim, field)),
        };
    }

    if let Some(field) = path.strip_prefix("/api/profile/") {
        let sim = lock(shared);
        return match profile_json(&sim, field) {
            Some(body) => Response::json(body),
            None => Response::error(404, &no_such_field(&sim, field)),
        };
    }

    Response::error(404, "no such route")
}

/// The refusal an unknown substance gets, and it names the ones there are.
///
/// The alternative — zeros of the right length — is a picture of a dead world,
/// and nothing about it says the name was wrong.
fn no_such_field(sim: &Sim, field: &str) -> String {
    let declared: Vec<&str> = sim.fields.iter().map(|meta| meta.id.as_str()).collect();
    format!(
        "no substance `{field}` in this run. The scenario declares: {}",
        declared.join(", ")
    )
}

/// The run identity, the grid, the fields and both residuals, in one object
/// taken under one lock.
fn state_json(sim: &Sim) -> String {
    let grid = sim.world.grid();
    // A dead simulation is not a running one, whatever the flag last said.
    let running = sim.running && sim.alive;

    let mut out = String::with_capacity(512);
    out.push_str("{\"scenario\":");
    push_json_string(&mut out, &sim.identity.scenario);

    // A string, and that is the whole point of it: `JSON.parse` truncates past
    // 2^53 — `observe/mod.rs` wrote it down about its own stream — and a run
    // shown under a seed it was not started with is a run nobody can repeat.
    out.push_str(",\"seed\":");
    push_json_string(&mut out, &sim.identity.seed.to_string());

    out.push_str(",\"config_hash\":");
    push_json_string(&mut out, &sim.identity.config_hash);
    out.push_str(",\"world_format_version\":");
    push_int(&mut out, i128::from(sim.identity.world_format_version));
    out.push_str(",\"tick\":");
    push_int(&mut out, i128::from(sim.ticks));

    // Measured, never requested (ADR-070). Zero while paused, because that is
    // the rate at which ticks are happening.
    out.push_str(",\"ticks_per_second\":");
    push_real(&mut out, if running { sim.measured_tps } else { 0.0 });
    out.push_str(",\"running\":");
    out.push_str(if running { "true" } else { "false" });
    out.push_str(",\"alive\":");
    out.push_str(if sim.alive { "true" } else { "false" });
    out.push_str(",\"target_tps\":");
    push_real(&mut out, sim.target_tps);
    out.push_str(",\"sim_time\":");
    push_real(&mut out, f64::from(sim.ticks) * sim.scenario.dt);
    out.push_str(",\"error\":");
    match &sim.error {
        Some(error) => push_json_string(&mut out, error),
        None => out.push_str("null"),
    }
    out.push_str(",\"ecology\":");
    out.push_str(&sim.ecology.summary(sim).to_string());

    out.push_str(",\"grid\":{\"nx\":");
    push_int(&mut out, i128::from(grid.nx()));
    out.push_str(",\"ny\":");
    push_int(&mut out, i128::from(grid.ny()));
    out.push_str(",\"nz\":");
    push_int(&mut out, i128::from(grid.nz()));
    out.push_str(",\"dx\":");
    push_real(&mut out, sim.dx);
    out.push('}');

    out.push_str(",\"fields\":[");
    for (i, meta) in sim.fields.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"id\":");
        push_json_string(&mut out, &meta.id);
        // `label` is the id until a key declares otherwise: `CONFIG_SCHEMA.md`
        // has no display name, and one invented here would be a scenario
        // decision made by a route (ADR-072).
        out.push_str(",\"label\":");
        push_json_string(&mut out, &meta.id);
        out.push_str(",\"unit\":");
        push_json_string(&mut out, meta.unit);
        out.push_str(",\"max\":");
        push_real(&mut out, meta.max_conc);
        out.push('}');
    }
    out.push(']');

    // Three-valued, per half, and the two states never merge (ADR-071): a
    // number when the check ran and could have failed, `null` when there was
    // nothing to check.
    out.push_str(",\"residual\":{\"matter\":");
    match &sim.last {
        Some(residual) => push_int(&mut out, residual.matter),
        None => out.push_str("null"),
    }
    out.push_str(",\"energy\":");
    match sim.last.as_ref().and_then(|residual| residual.energy) {
        Some(joules) => push_int(&mut out, joules),
        None => out.push_str("null"),
    }
    out.push_str("}}");

    out
}

/// The volume of one substance: a self-describing header and one byte per voxel.
///
/// `None` when no substance has that id. A volume of zeros of the right length
/// is a picture of a dead world, and it is indistinguishable from a result.
fn volume_of(sim: &Sim, field: &str) -> Option<Vec<u8>> {
    let meta = sim.fields.iter().find(|meta| meta.id == field)?;
    let grid = sim.world.grid();

    let mut out = Vec::with_capacity(VOLUME_HEADER + grid.n_voxels() as usize);
    out.extend_from_slice(&VOLUME_MAGIC.to_le_bytes());
    // The payload carries its own extents rather than trusting the ones
    // `/api/state` gave: the two arrive from different requests, and a frame
    // drawn against a stale shape is a picture of nothing.
    out.extend_from_slice(&grid.nx().to_le_bytes());
    out.extend_from_slice(&grid.ny().to_le_bytes());
    out.extend_from_slice(&grid.nz().to_le_bytes());
    // What one byte is worth, in the field's own unit. Nothing in the picture
    // depends on it — the shader takes a normalised sample — so an error here
    // is invisible to the eye and plain to anyone reading the volume with a
    // program.
    out.extend_from_slice(&((meta.max_conc / 255.0) as f32).to_le_bytes());
    out.extend_from_slice(&[0u8; 4]);
    debug_assert_eq!(
        out.len(),
        VOLUME_HEADER,
        "the viewer reads 24 bytes of head"
    );

    // `Field::lane` and never `lane_write`: the front buffer is state `N` for
    // every lane (ADR-057), and the write side of a lane that took an even
    // number of substeps is byte for byte the same thing — which is why only a
    // world with lanes of both parities can tell them apart.
    match meta.lane {
        LaneRef::Narrow(lane) => {
            let lane = sim
                .world
                .amounts_32()
                .expect("a narrow lane exists only if the narrow field does")
                .lane(lane);
            out.extend(
                lane.iter()
                    .map(|amount| quantise(amount.to_i64(), meta.amount_at_max)),
            );
        }
        LaneRef::Wide(lane) => {
            let lane = sim
                .world
                .amounts_64()
                .expect("a wide lane exists only if the wide field does")
                .lane(lane);
            out.extend(
                lane.iter()
                    .map(|amount| quantise(amount.to_i64(), meta.amount_at_max)),
            );
        }
    }

    Some(out)
}

/// The vertical profile of one substance, mol/m^3 per layer, floor first.
///
/// The stream of ADR-037's second half: a vector observable, on its own route
/// and its own frequency, and never a column of the metric stream.
fn profile_json(sim: &Sim, field: &str) -> Option<String> {
    let meta = sim.fields.iter().find(|meta| meta.id == field)?;
    let grid = sim.world.grid();

    let mut out = String::from("{\"z\":[");
    for z in 0..grid.nz() {
        if z > 0 {
            out.push(',');
        }
        push_int(&mut out, i128::from(z));
    }

    // Index 0 is the floor, because the page puts index 0 at the bottom and
    // reads nothing else. An array collected downwards draws an inverted world
    // beside a correct volume, and only somebody comparing the two panels on
    // purpose would ever see it.
    out.push_str("],\"mean\":[");
    for z in 0..grid.nz() {
        if z > 0 {
            out.push(',');
        }
        let mean = match meta.lane {
            LaneRef::Narrow(lane) => layer_mean_32(
                sim.world
                    .amounts_32()
                    .expect("a narrow lane exists only if the narrow field does")
                    .lane(lane),
                grid,
                z,
                meta.k,
                sim.v_voxel,
            ),
            LaneRef::Wide(lane) => layer_mean_64(
                sim.world
                    .amounts_64()
                    .expect("a wide lane exists only if the wide field does")
                    .lane(lane),
                grid,
                z,
                meta.k,
                sim.v_voxel,
            ),
        };
        push_real(&mut out, mean);
    }
    out.push_str("]}");

    Some(out)
}

/// `play`, `pause`, `step`, `speed`, `reset` and a refusal for anything else.
///
/// A command swallowed in silence looks exactly like a simulator that has hung,
/// so every action this does not understand is answered 400 and named back.
fn apply_control(sim: &mut Sim, body: &[u8]) -> Response {
    let Ok(command) = serde_json::from_slice::<serde_json::Value>(body) else {
        return Response::error(400, "the control body must be valid JSON");
    };
    let Some(action) = command.get("action").and_then(serde_json::Value::as_str) else {
        return Response::error(
            400,
            "the control body names no action. Use play, pause, step, speed or reset",
        );
    };

    match action {
        "play" | "run" => {
            if !sim.alive {
                return Response::error(
                    503,
                    "the simulation has stopped; reset to start a new run",
                );
            }
            sim.running = true;
        }
        "pause" => {
            sim.running = false;
            // Reported at once rather than left to the loop to notice: the page
            // polls immediately after the POST, and a rate that lingers for one
            // frame is a paused simulation claiming to be running.
            sim.measured_tps = 0.0;
        }
        "step" => {
            if !sim.alive {
                return Response::error(
                    503,
                    "the simulation has stopped; reset to start a new run",
                );
            }
            sim.running = false;
            sim.measured_tps = 0.0;
            // Performed here and not queued. `pending_steps += 1` races the
            // poll the page makes immediately after this response, and the
            // button looks unpressed every other time.
            if !safe_advance(sim) {
                return Response::error(503, sim.error.as_deref().unwrap_or("the tick failed"));
            }
        }
        "speed" => {
            let Some(value) = command.get("value").and_then(serde_json::Value::as_f64) else {
                return Response::error(400, "speed without a value");
            };
            if !value.is_finite() || !(MIN_TPS..=MAX_TPS).contains(&value) {
                return Response::error(
                    400,
                    &format!(
                        "speed {value} is outside the {MIN_TPS} to {MAX_TPS} ticks a second \
                         the page's slider declares"
                    ),
                );
            }
            sim.target_tps = value;
        }
        "reset" => {
            let seed = match command.get("seed") {
                None => sim.identity.seed,
                Some(serde_json::Value::String(seed)) => match seed.parse::<u64>() {
                    Ok(seed) => seed,
                    Err(_) => {
                        return Response::error(
                            400,
                            "seed must be a decimal unsigned 64-bit integer",
                        );
                    }
                },
                _ => {
                    return Response::error(
                        400,
                        "seed must be a string to preserve its exact value",
                    );
                }
            };
            match build(&sim.scenario, seed) {
                Ok(mut reset) => {
                    reset.target_tps = sim.target_tps;
                    reset.running = sim.running;
                    *sim = reset;
                }
                Err(error) => return Response::error(400, &format!("reset failed: {error:#}")),
            }
        }
        other => {
            return Response::error(
                400,
                &format!(
                    "`{other}` is not a control action. Use play, pause, step, speed or reset"
                ),
            );
        }
    }

    // The new state, so that the page's poll and this answer cannot disagree
    // about what the command did.
    Response::json(state_json(sim))
}

/// One byte against the substance's **declared** ceiling, never against the
/// maximum of the frame (ADR-072).
///
/// The frame's maximum is the tempting one and it is the quiet failure: the
/// picture stays beautiful, a still world starts breathing, and the oxycline
/// crawls when only the extremum moved. Nothing about the amounts changed, so no
/// conservation test can see it.
fn quantise(amount: i64, amount_at_max: i128) -> u8 {
    if amount <= 0 || amount_at_max <= 0 {
        return 0;
    }
    let amount = i128::from(amount);
    if amount >= amount_at_max {
        return 255;
    }
    // Rounded to nearest, and both ways of not doing so are visible on the
    // screen: truncation reads a full voxel as 254 and looks like physics,
    // rounding up paints an empty voxel 1 and the world stops having empty
    // space.
    let byte = (2 * amount * 255 + amount_at_max) / (2 * amount_at_max);
    byte.clamp(0, 255) as u8
}

/// The mean of layer `z` in mol/m^3: the sum is exact in `i128`, and the
/// division happens once, at the end.
fn layer_mean_32(lane: &[M32], grid: &Grid, z: u32, k: u8, v_voxel: f64) -> f64 {
    let per_layer = (grid.nx() * grid.ny()) as usize;
    let start = z as usize * per_layer;
    let sum: i128 = lane[start..start + per_layer]
        .iter()
        .map(|amount| i128::from(amount.to_i64()))
        .sum();
    mean_conc(sum, per_layer, k, v_voxel)
}

/// See [`layer_mean_32`].
fn layer_mean_64(lane: &[M64], grid: &Grid, z: u32, k: u8, v_voxel: f64) -> f64 {
    let per_layer = (grid.nx() * grid.ny()) as usize;
    let start = z as usize * per_layer;
    let sum: i128 = lane[start..start + per_layer]
        .iter()
        .map(|amount| i128::from(amount.to_i64()))
        .sum();
    mean_conc(sum, per_layer, k, v_voxel)
}

/// Storage units to mol/m^3, once, over an exact sum.
///
/// The same curves in storage units have the same *shape*, and the page indexes
/// every series to its declared maximum — so only the absolute numbers in the
/// tooltip and the table would be wrong, which is exactly what a reader reads.
fn mean_conc(sum: i128, voxels: usize, k: u8, v_voxel: f64) -> f64 {
    // The one `as f64` `QUANTITIES.md` section 3 allows: in a metric, at the
    // end, over a sum that was taken exactly.
    sum as f64 / (voxels as f64 * f64::from(k).exp2() * v_voxel)
}

/// A JSON string, escaped.
///
/// `substance.id` is a free TOML string — `Registry::new` guards duplicates and
/// the count of thirty-one and nothing about characters — so a quote reaches
/// here from a scenario file. Unescaped it breaks the object, the page falls
/// into its `catch` and prints "Lost the simulator", which blames the
/// connection.
fn push_json_string(out: &mut String, value: &str) {
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// An integer, printed exactly.
///
/// `i128` because both sides of the invariant of ADR-003 are summed in `i128`,
/// and narrowing on the way out would put a cast in the last place before the
/// reader.
fn push_int(out: &mut String, value: i128) {
    out.push_str(&value.to_string());
}

/// A real number, or `null` if it is not one.
///
/// `NaN` and `inf` are what Rust prints and neither is JSON. `observe/json.rs`
/// refuses the record; here the reader is a page that has a way to say "nothing
/// to show", so it is told that instead of being handed a document it cannot
/// parse.
fn push_real(out: &mut String, value: f64) {
    if value.is_finite() {
        out.push_str(&format!("{value:e}"));
    } else {
        out.push_str("null");
    }
}

#[cfg(test)]
pub(crate) mod fixture {
    //! A scenario with matter, chemistry and an enthalpy field.
    //!
    //! No file under `configs/` will do: `hello.toml` declares no substance and
    //! does not survive its own derivation (`config::load` says so at length).
    //! So the scenario is written out here, on the model of
    //! `crates/liminis-core/tests/acceptance_tick.rs`.
    //!
    //! **The numbers marked as placeholders are placeholders.** `c_p`,
    //! `enthalpy_formation` and `partial_molar_volume` are declared for no
    //! substance anywhere in the corpus (`CONFIG_SCHEMA.md` section 13 item 23).
    //! Do not copy this into `configs/`.
    //!
    //! Two properties of the fixture are load-bearing. The domain is closed on
    //! all six faces, so every channel counter stays at zero and any matter
    //! created or destroyed shows up as a non-zero residual on the tick it
    //! happened. And `WATER` is declared **second** and is the only 64-bit
    //! substance, so it takes lane 0 of the wide field while the narrow
    //! substances take lanes 0..3 of the narrow one: a lane is never a substance
    //! index, which is the miss ADR-056 exists for.

    use super::{Sim, build};
    use std::sync::{Arc, Mutex};

    pub const SCENARIO: &str = r#"
name = "viewer-fixture"
dt = 1.0
beta = 0.015625
T_ref = 298.15

[conserved]
C = 12.01070
N = 14.00670
P = 30.97376
S = 32.06500
Fe = 55.84500

[grid]
nx = 4
ny = 4
nz = 4
dx = 1.0e-4

[boundary]
x_min = "closed"
x_max = "closed"
y_min = "closed"
y_max = "closed"
z_min = "closed"
z_max = "closed"

[[substance]]
id = "H2S"
molar_mass = 34.08088
typical_conc = 0.1
max_conc = 10.0
partial_molar_volume = 3.5e-5
settling_radius = 0.0
diffusivity = 1.6e-9
c_p = 100.0
enthalpy_formation = 0.0
composition = { S = 1 }

[[substance]]
id = "WATER"
molar_mass = 18.01528
typical_conc = 55000.0
max_conc = 55600.0
partial_molar_volume = 1.8e-5
settling_radius = 0.0
diffusivity = 2.3e-9
c_p = 75.3
enthalpy_formation = -285830.0
composition = {}

[[substance]]
id = "O2"
molar_mass = 31.99880
typical_conc = 0.25
max_conc = 1.0
partial_molar_volume = 3.1e-5
settling_radius = 0.0
diffusivity = 2.1e-9
c_p = 101.0
enthalpy_formation = 0.0
composition = {}

[[substance]]
id = "SO4"
molar_mass = 96.06260
typical_conc = 28.0
max_conc = 100.0
partial_molar_volume = 1.4e-5
settling_radius = 0.0
diffusivity = 1.0e-9
c_p = 102.0
enthalpy_formation = -846000.0
composition = { S = 1 }

[[substance]]
id = "H_ION"
molar_mass = 1.007940
typical_conc = 1.0e-4
max_conc = 1.0e-2
partial_molar_volume = 0.0
settling_radius = 0.0
diffusivity = 9.3e-9
c_p = 103.0
enthalpy_formation = 0.0
composition = {}

[[reaction]]
id = "h2s_oxidation"
enthalpy = -846000.0
catalyst = ""
energy_from = ""
inputs = { H2S = 1, O2 = 2 }
outputs = { SO4 = 1, H_ION = 2 }

[reaction.rate]
vmax = 1.0e-6
t_vmax = 298.15
q10 = 2.0
km = { H2S = 0.01, O2 = 0.01 }

[[reaction]]
id = "water_formation"
enthalpy = -571660.0
catalyst = ""
energy_from = ""
inputs = { H_ION = 4, O2 = 1 }
outputs = { WATER = 2 }

[reaction.rate]
vmax = 1.0e-6
t_vmax = 298.15
q10 = 2.0
km = { H_ION = 0.01, O2 = 0.01 }

[[field]]
id = "enthalpy"
lod = 2
thermal_diffusivity = 1.4e-7
t_min = 273.15
t_max = 323.15
"#;

    /// The fixture at seed 42, behind the mutex the routes take.
    pub fn sim() -> Arc<Mutex<Sim>> {
        sim_of(SCENARIO, 42)
    }

    /// The fixture, or a variation of it, at a chosen seed.
    pub fn sim_of(scenario: &str, seed: u64) -> Arc<Mutex<Sim>> {
        let config = liminis_core::config::parse(scenario).expect("the fixture must parse");
        Arc::new(Mutex::new(
            build(&config, seed).expect("the fixture must build a world"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Method;
    use crate::json::Json;
    use fixture::{sim, sim_of};
    use liminis_core::ledger::Channel;

    /// Substance indices of the fixture, in declaration order.
    const H2S: u32 = 0;
    const O2: u32 = 2;

    fn get(shared: &Arc<Mutex<Sim>>, path: &str) -> Response {
        route(
            shared,
            &Request {
                method: Method::Get,
                path: path.into(),
                query: String::new(),
                body: Vec::new(),
            },
        )
    }

    fn post(shared: &Arc<Mutex<Sim>>, path: &str, body: &str) -> Response {
        route(
            shared,
            &Request {
                method: Method::Post,
                path: path.into(),
                query: String::new(),
                body: body.as_bytes().to_vec(),
            },
        )
    }

    /// `/api/state`, parsed.
    fn state(shared: &Arc<Mutex<Sim>>) -> Json {
        let response = get(shared, "/api/state");
        assert_eq!(response.status, 200, "the state route refused");
        Json::parse(&String::from_utf8(response.body).expect("the state route wrote UTF-8"))
            .expect("the state route wrote JSON")
    }

    fn tick_of(shared: &Arc<Mutex<Sim>>) -> u32 {
        lock(shared).ticks
    }

    fn u32_at(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
    }

    fn f32_at(bytes: &[u8], at: usize) -> f32 {
        f32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
    }

    /// Write every lane of both amount fields and promote them into the front
    /// buffer, which is where a process expects state `N` (ADR-057).
    ///
    /// The whole field is written and swapped once, never lane by lane: a swap
    /// inside the loop leaves half the lanes a phase behind by parity, and zero
    /// is a legal amount, so nothing falls over.
    fn paint(sim: &mut Sim, amount: impl Fn(u32, u32) -> i64) {
        let n_voxels = sim.world.grid().n_voxels();
        let lane_len = sim.world.grid().lane_len();
        let n_substances = sim.world.registry().n_substances();
        for s in 0..n_substances {
            match sim.world.lane_of(s) {
                LaneRef::Narrow(lane) => {
                    let field = sim.world.amounts_32_mut().expect("a narrow field");
                    let buffer = field.write_mut();
                    for idx in 0..n_voxels {
                        buffer[(lane * lane_len + idx) as usize] =
                            M32::from_i64_clamping(amount(s, idx));
                    }
                }
                LaneRef::Wide(lane) => {
                    let field = sim.world.amounts_64_mut().expect("a wide field");
                    let buffer = field.write_mut();
                    for idx in 0..n_voxels {
                        buffer[(lane * lane_len + idx) as usize] = M64::new(amount(s, idx));
                    }
                }
            }
        }
        if let Some(field) = sim.world.amounts_32_mut() {
            field.swap();
        }
        if let Some(field) = sim.world.amounts_64_mut() {
            field.swap();
        }
    }

    /// The front lane of one substance, as amounts.
    fn front_amounts(sim: &Sim, s: u32) -> Vec<i64> {
        match sim.world.lane_of(s) {
            LaneRef::Narrow(lane) => sim
                .world
                .amounts_32()
                .expect("a narrow field")
                .lane(lane)
                .iter()
                .map(|m| m.to_i64())
                .collect(),
            LaneRef::Wide(lane) => sim
                .world
                .amounts_64()
                .expect("a wide field")
                .lane(lane)
                .iter()
                .map(|m| m.to_i64())
                .collect(),
        }
    }

    /// The **write** lane of one substance: the half-state a route must never
    /// show.
    fn write_amounts(sim: &Sim, s: u32) -> Vec<i64> {
        match sim.world.lane_of(s) {
            LaneRef::Narrow(lane) => sim
                .world
                .amounts_32()
                .expect("a narrow field")
                .lane_write(lane)
                .iter()
                .map(|m| m.to_i64())
                .collect(),
            LaneRef::Wide(lane) => sim
                .world
                .amounts_64()
                .expect("a wide field")
                .lane_write(lane)
                .iter()
                .map(|m| m.to_i64())
                .collect(),
        }
    }

    fn meta<'a>(sim: &'a Sim, id: &str) -> &'a FieldMeta {
        sim.fields
            .iter()
            .find(|field| field.id == id)
            .expect("the fixture declares this substance")
    }

    // --- the volume -------------------------------------------------------

    #[test]
    fn the_server_serves_a_volume_of_the_declared_shape() {
        // The header is checked byte by byte because the viewer does not check
        // it at all: it reads `DataView(buf, 0, 24)` and `Uint8Array(buf, 24)`,
        // so a header off by one byte draws shifted data as a plausible cloud.
        let shared = sim();
        {
            let mut guard = lock(&shared);
            advance_one(&mut guard);
        }

        let state = state(&shared);
        let grid = state.get("grid").expect("the state route declares a grid");
        let nx = grid.get("nx").and_then(Json::as_u32).expect("nx");
        let ny = grid.get("ny").and_then(Json::as_u32).expect("ny");
        let nz = grid.get("nz").and_then(Json::as_u32).expect("nz");

        let response = get(&shared, "/api/volume/O2");
        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "application/octet-stream");
        let body = response.body;
        assert_eq!(
            body.len(),
            VOLUME_HEADER + (nx * ny * nz) as usize,
            "the payload is a header and one byte per voxel"
        );

        assert_eq!(u32_at(&body, 0), VOLUME_MAGIC);
        assert_eq!(u32_at(&body, 4), nx);
        assert_eq!(u32_at(&body, 8), ny);
        assert_eq!(u32_at(&body, 12), nz);
        assert_eq!(
            &body[20..24],
            &[0, 0, 0, 0],
            "the padding is four zero bytes"
        );

        // What one byte is worth. Nothing in the picture depends on it — the
        // shader takes a normalised sample — so an error here is invisible to
        // the eye and visible only to whoever reads the volume with a program.
        let declared = state
            .get("fields")
            .and_then(Json::as_array)
            .expect("fields")
            .iter()
            .find(|field| field.get("id").and_then(Json::as_str) == Some("O2"))
            .and_then(|field| field.get("max"))
            .and_then(Json::as_f64)
            .expect("O2 declares a maximum");
        let scale = f64::from(f32_at(&body, 16));
        assert!(
            (scale * 255.0 - declared).abs() <= declared * 1e-5,
            "one byte is {scale} of {declared}, which is not a two hundred and fifty-fifth"
        );
    }

    #[test]
    fn a_volume_byte_is_fixed_by_the_declared_maximum_not_by_the_frame() {
        // A transfer function rescaled per frame makes a still world breathe and
        // makes the oxycline crawl when only the extremum moved. No conservation
        // test sees it: nothing about the amounts changed.
        let shared = sim();
        let ceiling = {
            let guard = lock(&shared);
            meta(&guard, "O2").amount_at_max
        };
        let half = i64::try_from(ceiling / 2).expect("the fixture is far inside i64");
        let full = i64::try_from(ceiling).expect("the fixture is far inside i64");

        let byte_of = |shared: &Arc<Mutex<Sim>>, at: usize| -> u8 {
            let response = get(shared, "/api/volume/O2");
            assert_eq!(response.status, 200);
            response.body[VOLUME_HEADER + at]
        };

        // Frame one: voxel 0 at half the ceiling, voxel 1 at the ceiling.
        {
            let mut guard = lock(&shared);
            paint(&mut guard, move |s, idx| {
                if s != O2 {
                    0
                } else if idx == 0 {
                    half
                } else if idx == 1 {
                    full
                } else {
                    0
                }
            });
        }
        let first = byte_of(&shared, 0);
        assert_eq!(byte_of(&shared, 1), 255, "the declared ceiling is 255");
        assert_eq!(byte_of(&shared, 2), 0, "an empty voxel is 0");

        // Frame two: the same voxel, and a frame whose maximum is half of what
        // it was.
        {
            let mut guard = lock(&shared);
            paint(
                &mut guard,
                move |s, idx| {
                    if s == O2 && idx <= 1 { half } else { 0 }
                },
            );
        }
        assert_eq!(
            byte_of(&shared, 0),
            first,
            "the byte moved when only the frame's maximum did"
        );
    }

    #[test]
    fn the_volume_is_indexed_the_way_the_grid_is() {
        // A transposed volume draws a perfectly plausible cloud, and nothing
        // else can see it: `texImage3D(nx, ny, nz)` accepts any order.
        let shared = sim();
        let (nx, ny, one) = {
            let mut guard = lock(&shared);
            let grid = *guard.world.grid();
            let full = i64::try_from(meta(&guard, "H2S").amount_at_max).expect("inside i64");
            let one = grid.index(1, 2, 3);
            paint(
                &mut guard,
                move |s, idx| {
                    if s == H2S && idx == one { full } else { 0 }
                },
            );
            (grid.nx(), grid.ny(), one)
        };
        assert_eq!(
            one,
            1 + 2 * nx + 3 * nx * ny,
            "the fixture's own arithmetic"
        );

        let response = get(&shared, "/api/volume/H2S");
        assert_eq!(response.status, 200);
        let voxels = &response.body[VOLUME_HEADER..];
        assert_eq!(voxels[one as usize], 255);
        for (idx, byte) in voxels.iter().enumerate() {
            if idx as u32 != one {
                assert_eq!(*byte, 0, "voxel {idx} is not empty");
            }
        }
    }

    #[test]
    fn the_volume_reads_the_front_buffer_and_never_the_write_side() {
        // Two misses at once, and both draw a plausible picture: `field.lane(s)`
        // is a substance index used as a buffer address (ADR-056), and
        // `lane_write` is the half-state of a lane that has not been promoted
        // (ADR-057).
        let shared = sim();
        let mut guard = lock(&shared);
        paint(&mut guard, |s, idx| {
            1_000_000 + i64::from(s) * 7_919 + i64::from(idx) * 131
        });
        advance_one(&mut guard);

        let n_substances = guard.world.registry().n_substances();
        let mut a_lane_is_not_its_substance = false;
        let mut a_write_side_differs = false;
        for s in 0..n_substances {
            match guard.world.lane_of(s) {
                LaneRef::Narrow(lane) | LaneRef::Wide(lane) => {
                    a_lane_is_not_its_substance |= lane != s;
                }
            }
            a_write_side_differs |= front_amounts(&guard, s) != write_amounts(&guard, s);
        }
        assert!(
            a_lane_is_not_its_substance,
            "the fixture must place a substance on a lane of another number"
        );
        assert!(
            a_write_side_differs,
            "the fixture must leave the two buffers of a lane different, or \
             reading the write side would be invisible here"
        );

        for s in 0..n_substances {
            let id = guard.world.registry().id_of(s).to_string();
            let front = front_amounts(&guard, s);
            let ceiling = meta(&guard, &id).amount_at_max;
            let want: Vec<u8> = front
                .iter()
                .map(|amount| quantise(*amount, ceiling))
                .collect();
            let got = volume_of(&guard, &id).expect("the substance is declared");
            assert_eq!(&got[VOLUME_HEADER..], &want[..], "substance {id}");
        }
    }

    #[test]
    fn an_unknown_field_is_a_404_and_not_an_empty_volume() {
        // A volume of zeros of the right length is a picture of a dead world,
        // and it is indistinguishable from a result.
        let shared = sim();
        for path in ["/api/volume/NOPE", "/api/profile/NOPE"] {
            let response = get(&shared, path);
            assert_eq!(response.status, 404, "{path} answered something");
            let body = String::from_utf8(response.body).expect("UTF-8");
            assert!(body.contains("NOPE"), "{path} did not name it: {body}");
        }
    }

    // --- the profile ------------------------------------------------------

    #[test]
    fn the_profile_runs_from_the_floor_upwards() {
        // The page uses `mean` alone and puts index 0 at the bottom, so an array
        // collected downwards draws an inverted world beside a correct volume.
        let shared = sim();
        let nz = {
            let mut guard = lock(&shared);
            let grid = *guard.world.grid();
            let per_layer = grid.nx() * grid.ny();
            let typical = i64::try_from(meta(&guard, "SO4").amount_at_max / 4).expect("inside i64");
            paint(
                &mut guard,
                move |_, idx| {
                    if idx < per_layer { typical } else { 0 }
                },
            );
            grid.nz()
        };

        let response = get(&shared, "/api/profile/SO4");
        assert_eq!(response.status, 200);
        let profile = Json::parse(&String::from_utf8(response.body).expect("UTF-8")).expect("JSON");
        let mean = profile.get("mean").and_then(Json::as_array).expect("mean");
        let z = profile.get("z").and_then(Json::as_array).expect("z");

        assert_eq!(mean.len(), nz as usize);
        assert_eq!(z.len(), nz as usize);
        for (layer, value) in z.iter().enumerate() {
            assert_eq!(value.as_u32(), Some(layer as u32), "z is 0..nz-1, in order");
        }
        assert!(
            mean[0].as_f64().expect("a number") > 0.0,
            "the floor was filled and reads empty"
        );
        assert_eq!(
            mean[nz as usize - 1].as_f64(),
            Some(0.0),
            "the top was empty and reads full: the profile is upside down"
        );

        // And the unit. The same curves in storage units have the same *shape* —
        // the page normalises each series to its declared maximum — so only the
        // absolute numbers lie, which is exactly what a reader reads.
        {
            let mut guard = lock(&shared);
            let ceiling = i64::try_from(meta(&guard, "SO4").amount_at_max).expect("inside i64");
            paint(&mut guard, move |_, _| ceiling);
        }
        let response = get(&shared, "/api/profile/SO4");
        let profile = Json::parse(&String::from_utf8(response.body).expect("UTF-8")).expect("JSON");
        let mean = profile.get("mean").and_then(Json::as_array).expect("mean");
        let declared = {
            let guard = lock(&shared);
            meta(&guard, "SO4").max_conc
        };
        for (layer, value) in mean.iter().enumerate() {
            let got = value.as_f64().expect("a number");
            assert!(
                (got - declared).abs() <= declared * 1e-6,
                "layer {layer} reads {got} where the declared maximum is {declared} mol/m^3"
            );
        }
    }

    // --- the residual -----------------------------------------------------

    #[test]
    fn the_state_route_reports_both_residuals() {
        let shared = sim();
        {
            let mut guard = lock(&shared);
            advance_one(&mut guard);
        }

        let state = state(&shared);
        let residual = state
            .get("residual")
            .expect("the state route reports a residual");
        assert_eq!(
            residual.get("matter").and_then(Json::as_i128),
            Some(0),
            "a written zero is the proof the check ran (ADR-037)"
        );
        assert_eq!(
            residual.get("energy"),
            Some(&Json::Null),
            "the energy half has nothing to show, and that is not a zero (ADR-071)"
        );

        // The same number, computed in this test out of its own accumulators
        // rather than read back out of the cache the route printed from.
        let (want, published) = {
            let mut guard = lock(&shared);
            let n = guard.world.registry().n_substances();
            let mut before = DomainSums::new(n).expect("before");
            let mut after = DomainSums::new(n).expect("after");
            guard.tick.domain_sums(&guard.world, &mut before);
            advance_one(&mut guard);
            guard.tick.domain_sums(&guard.world, &mut after);
            let want: Vec<i128> = (0..n)
                .map(|s| {
                    guard.ledger.residual_matter(
                        guard.tick.reaction_nu(guard.ticks.wrapping_sub(1)),
                        s,
                        &before,
                        &after,
                    )
                })
                .collect();
            let published = guard.last.as_ref().expect("a tick was completed").matter;
            (want, published)
        };
        for (s, residual) in want.iter().enumerate() {
            assert_eq!(*residual, 0, "substance {s} did not close");
        }
        let worst = want
            .iter()
            .copied()
            .max_by_key(|residual| residual.unsigned_abs())
            .expect("the fixture declares substances");
        assert_eq!(published, worst);
    }

    #[test]
    fn the_residual_is_unreported_before_the_first_tick() {
        // A zero here would mean "the check ran and it closed" on a tick that
        // never happened. The page branches on `typeof s.residual.matter`.
        let shared = sim();
        let state = state(&shared);
        let residual = state.get("residual").expect("a residual object");
        assert_eq!(residual.get("matter"), Some(&Json::Null));
        assert_eq!(residual.get("energy"), Some(&Json::Null));
        assert_eq!(state.get("tick").and_then(Json::as_u32), Some(0));
    }

    /// Put a hole of the test's own into the accumulators the publication
    /// reads, so that [`residual_of`] can be driven against a residual that is
    /// not zero.
    ///
    /// There is no other way to obtain one. Every process S0 dispatches
    /// conserves each substance exactly, and `Ledger::assert_closed` fires
    /// inside `Tick::advance` in this build — so a tick that arrived at the
    /// publication with a hole would have panicked on the way, and every
    /// residual reachable through `advance_one` is structurally zero.
    fn hole(sim: &mut Sim, before: &[i32], after: &[i32]) {
        assert_eq!(before.len(), after.len(), "one pair of sums per substance");
        let n = u32::try_from(before.len()).expect("a fixture of a few substances");
        let mut b = DomainSums::new(n).expect("the sums before");
        let mut a = DomainSums::new(n).expect("the sums after");
        for (s, (&lhs, &rhs)) in before.iter().zip(after).enumerate() {
            b.add_field_lane_32(s as u32, &[M32::new(lhs)]);
            a.add_field_lane_32(s as u32, &[M32::new(rhs)]);
        }
        // The ledger is replaced too, and at the same width: `residual_matter`
        // refuses sums that account for a different number of substances than
        // the ledger counts, and that refusal is the point of it.
        sim.ledger = Ledger::new(n).expect("a ledger of the same width");
        sim.before = b;
        sim.after = a;
    }

    #[test]
    fn the_published_residual_is_computed_and_not_written() {
        // The load-bearing property of ADR-037: a written zero is proof the
        // check ran, so a zero the route publishes has to be distinguishable
        // from a constant. On a closed domain it is not — see `hole` — so the
        // hole is made here, in the two accumulators the publication reads.
        let shared = sim();
        let mut guard = lock(&shared);

        // Substance 0 gained seven units nothing credited, substance 1 lost
        // three. Asymmetric on purpose: the aggregate is `+7` as written and
        // `-7` with the two accumulators the other way round, so the sign of
        // this number is what says which is subtracted from which.
        hole(&mut guard, &[100, 100], &[107, 97]);
        assert_eq!(
            residual_of(&guard).matter,
            7,
            "the published residual is not the largest hole with its sign"
        );

        // And the right-hand side of ADR-003 is subtracted. Credit the seven
        // units to a channel and substance 0 closes, which leaves the loss of
        // three as the worst; a publication that never looked at the ledger
        // would still be reporting seven.
        guard.ledger.credit_matter(Channel::GeothermalIn, 0, 7);
        assert_eq!(
            residual_of(&guard).matter,
            -3,
            "the channel counters were not held against the change of the domain"
        );

        // The energy half stays unreported whatever the matter half did: it has
        // no source in S0 and therefore nothing it could fail against (ADR-071).
        assert!(residual_of(&guard).energy.is_none());
    }

    #[test]
    fn the_reported_matter_residual_does_not_cancel_between_substances() {
        // The sum over substances is zero at `+5` and `-5`, and nothing else in
        // the corpus catches it: `assert_closed` walks substances one at a time
        // and lives in a debug build.
        let shared = sim();
        let mut guard = lock(&shared);
        hole(&mut guard, &[0, 0], &[5, -5]);

        assert_ne!(
            residual_of(&guard).matter,
            0,
            "two holes in opposite directions were reported as a closed ledger"
        );
    }

    #[test]
    fn a_dead_simulation_stops_claiming_a_closed_ledger() {
        // In a debug build `Ledger::assert_closed` panics inside `Tick::advance`
        // — that is, exactly when the residual is non-zero. Without the
        // sentinel the HTTP side goes on serving a frozen counter, `running:
        // true` and last tick's zero.
        let shared = sim();
        {
            let mut guard = lock(&shared);
            advance_one(&mut guard);
            guard.running = true;
        }
        drop(SimAlive(Arc::clone(&shared)));

        let state = state(&shared);
        assert_eq!(state.get("running"), Some(&Json::Bool(false)));
        assert_eq!(
            state.get("residual").and_then(|r| r.get("matter")),
            Some(&Json::Null)
        );
        assert_eq!(
            state.get("ticks_per_second").and_then(Json::as_f64),
            Some(0.0)
        );
    }

    // --- control ----------------------------------------------------------

    #[test]
    fn control_pause_actually_stops_the_tick_counter() {
        // Both halves matter. Without the first, the test is green on a
        // simulator that never ran at all — the same hole
        // `energy_ledger_residual_is_zero_over_10k_ticks` names.
        let shared = sim();
        {
            let mut guard = lock(&shared);
            guard.target_tps = MAX_TPS;
            guard.running = true;
        }
        let ticking = Arc::clone(&shared);
        std::thread::spawn(move || run_loop(&ticking));

        assert!(
            wait_for(&shared, |sim| sim.ticks >= 3),
            "the tick counter never moved"
        );

        let response = post(&shared, "/api/control", "{\"action\":\"pause\"}");
        assert_eq!(response.status, 200);

        // Five periods at the requested rate, twice over: the first sleep lets
        // an in-flight tick land, the second is the measurement.
        std::thread::sleep(Duration::from_millis(60));
        let first = tick_of(&shared);
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(
            tick_of(&shared),
            first,
            "the loop kept running under a flag"
        );

        let state = state(&shared);
        assert_eq!(state.get("running"), Some(&Json::Bool(false)));
        assert_eq!(
            state.get("ticks_per_second").and_then(Json::as_f64),
            Some(0.0)
        );

        assert_eq!(
            post(&shared, "/api/control", "{\"action\":\"play\"}").status,
            200
        );
        assert!(
            wait_for(&shared, move |sim| sim.ticks > first + 2),
            "play did not restart the loop"
        );

        lock(&shared).running = false;
    }

    #[test]
    fn step_advances_exactly_one_tick_while_paused() {
        // `pending_steps += 1` races the poll the viewer makes immediately after
        // the POST, and the button looks unpressed every other time.
        let shared = sim();
        lock(&shared).running = false;
        let before = tick_of(&shared);

        let response = post(&shared, "/api/control", "{\"action\":\"step\"}");
        assert_eq!(response.status, 200);
        assert_eq!(
            state(&shared).get("tick").and_then(Json::as_u32),
            Some(before + 1),
            "the next poll did not see the step"
        );
        assert_eq!(state(&shared).get("running"), Some(&Json::Bool(false)));
    }

    #[test]
    fn the_reported_rate_is_measured_and_not_the_requested_one() {
        // A rate printed from the slider makes a simulator that cannot keep up
        // look healthy.
        let shared = sim();
        let requested = 1.0e6;
        {
            let mut guard = lock(&shared);
            guard.target_tps = requested;
            guard.running = true;
        }
        let ticking = Arc::clone(&shared);
        std::thread::spawn(move || run_loop(&ticking));

        assert!(
            wait_for(&shared, |sim| sim.measured_tps > 0.0),
            "no rate was ever measured"
        );
        let measured = state(&shared)
            .get("ticks_per_second")
            .and_then(Json::as_f64)
            .expect("a rate");
        assert!(
            measured > 0.0 && measured < requested,
            "{measured} ticks a second were reported against {requested} requested"
        );

        assert_eq!(
            post(&shared, "/api/control", "{\"action\":\"pause\"}").status,
            200
        );
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(
            state(&shared)
                .get("ticks_per_second")
                .and_then(Json::as_f64),
            Some(0.0),
            "a paused simulation reported a rate"
        );
    }

    #[test]
    fn an_unknown_control_action_is_refused_rather_than_ignored() {
        // A command swallowed in silence looks like a simulator that has hung.
        let shared = sim();
        lock(&shared).running = false;
        let before = tick_of(&shared);

        let response = post(&shared, "/api/control", "{\"action\":\"fast-forward\"}");
        assert_eq!(response.status, 400);
        let body = String::from_utf8(response.body).expect("UTF-8");
        assert!(body.contains("fast-forward"), "unhelpful: {body}");

        let response = post(
            &shared,
            "/api/control",
            "{\"action\":\"speed\",\"value\":0}",
        );
        assert_eq!(response.status, 400);
        let body = String::from_utf8(response.body).expect("UTF-8");
        assert!(body.contains("speed"), "unhelpful: {body}");

        let guard = lock(&shared);
        assert_eq!(guard.ticks, before);
        assert!(!guard.running);
        assert_eq!(guard.target_tps, DEFAULT_TARGET_TPS);
    }

    #[test]
    fn control_parses_json_and_preserves_an_exact_reset_seed() {
        let shared = sim();
        assert_eq!(
            post(&shared, "/api/control", "{\"action\":\"pause\"}garbage").status,
            400
        );
        assert_eq!(
            post(
                &shared,
                "/api/control",
                "{\"action\":\"reset\",\"seed\":42}"
            )
            .status,
            400
        );
        assert_eq!(
            post(
                &shared,
                "/api/control",
                "{\"action\":\"reset\",\"seed\":\"18446744073709551615\"}"
            )
            .status,
            200
        );
        assert_eq!(
            state(&shared).get("seed").and_then(Json::as_str),
            Some("18446744073709551615")
        );
        assert_eq!(tick_of(&shared), 0);
        assert!(lock(&shared).running, "a running reset continues the run");
        assert_eq!(
            post(
                &shared,
                "/api/control",
                "{\"action\":\"reset\",\"seed\":\"18446744073709551616\"}"
            )
            .status,
            400
        );
    }

    #[test]
    fn a_ceiling_failure_is_visible_and_a_reset_recovers() {
        let shared = sim();
        {
            let mut guard = lock(&shared);
            let above = i64::try_from(meta(&guard, "O2").amount_at_max).unwrap() + 1;
            paint(&mut guard, |_, _| above);
        }
        let response = post(&shared, "/api/control", "{\"action\":\"step\"}");
        assert_eq!(response.status, 503);
        assert!(
            String::from_utf8(response.body)
                .unwrap()
                .contains("ceiling")
        );
        let guard = lock(&shared);
        assert!(!guard.alive && !guard.running && guard.last.is_none());
        drop(guard);
        assert_eq!(
            post(&shared, "/api/control", "{\"action\":\"reset\"}").status,
            200
        );
        assert_eq!(
            post(&shared, "/api/control", "{\"action\":\"step\"}").status,
            200
        );
        assert_eq!(tick_of(&shared), 1);
    }

    #[test]
    fn the_ecology_frame_reports_registry_biomass_and_growth_only_traits() {
        let mut scenario =
            config::parse(include_str!("../../../configs/scenarios/living-world.toml")).unwrap();
        scenario.grid.nx = 8;
        scenario.grid.ny = 8;
        scenario.grid.nz = 8;
        let mut sim = build(&scenario, 42).unwrap();
        let frame = sim.ecology.frame(&sim, 3);
        assert_eq!(frame["scenario"], scenario.name);
        assert_eq!(frame["config_hash"], sim.identity.config_hash.to_string());
        assert_eq!(frame["world_format_version"], WORLD_FORMAT_VERSION);
        assert_eq!(frame["seed"], "42");
        assert_eq!(frame["cells"].as_array().unwrap().len(), 64);
        let types = frame["ecotypes"].as_array().unwrap();
        assert_eq!(types.len(), 5);
        let shares: f64 = types.iter().map(|t| t["share"].as_f64().unwrap()).sum();
        assert!((shares - 1.0).abs() < 1e-12);
        let harvester = types.iter().find(|t| t["id"] == "HARVESTER").unwrap();
        assert!((harvester["vmax"].as_f64().unwrap() - 0.0012).abs() < 1e-12);
        advance_one(&mut sim);
        let last = sim.last.as_ref().unwrap();
        assert_eq!(last.matter, 0);
        assert_eq!(last.energy, Some(0));
        assert!(frame["genetics"].is_null());
        assert!(types.iter().all(|t| t["genome"].is_null()));
        assert!(types.iter().all(|t| t["first_seen_tick"] == 0));
        assert_eq!(
            frame["cells"][0]["resources"]["FOOD"],
            frame["cells"][0]["resource"]
        );
    }

    fn small_genetic_scenario() -> Config {
        let mut scenario = config::parse(include_str!(
            "../../../configs/scenarios/genetic-colony.toml"
        ))
        .unwrap();
        scenario.grid.nx = 8;
        scenario.grid.ny = 8;
        scenario.grid.nz = 8;
        scenario.initial.inoculum[0].center = [0.0004; 3];
        scenario.initial.inoculum[0].radius = 0.00015;
        scenario
    }

    #[test]
    fn genetic_observation_uses_the_decoder_and_actual_first_appearance() {
        let scenario = small_genetic_scenario();
        let genotypes = config::decode_genotypes(scenario.genetics.as_ref().unwrap());
        let mut sim = build(&scenario, 42).unwrap();
        let frame = sim.ecology.frame(&sim, 3);
        assert_eq!(frame["genetics"]["mutation_probability"], 0.02);
        assert_eq!(
            frame["genetics"]["resources"],
            serde_json::json!(["FOOD", "DET"])
        );
        let types = frame["ecotypes"].as_array().unwrap();
        assert_eq!(types.len(), 4);
        for genotype in genotypes {
            let t = types.iter().find(|t| t["id"] == genotype.id).unwrap();
            assert_eq!(t["genome"]["code"], genotype.code);
            assert_eq!(t["genome"]["bits"], format!("{:02b}", genotype.code));
            assert_eq!(t["genome"]["rate_factor"], genotype.rate_factor);
            assert_eq!(t["genome"]["km_factor"], genotype.km_factor);
            for (i, expected) in genotype.resource_allocation.iter().enumerate() {
                assert_eq!(t["genome"]["allocation"][i]["fraction"], *expected);
            }
            if genotype.code == 0 {
                assert_eq!(t["first_seen_tick"], 0);
                assert!(t["total_mol"].as_f64().unwrap() > 0.0);
            } else {
                assert!(t["first_seen_tick"].is_null());
                assert_eq!(t["total_mol"], 0.0);
            }
        }
        for cell in frame["cells"].as_array().unwrap() {
            assert_eq!(cell["resources"]["DET"], 0.0);
            assert_eq!(cell["resources"]["FOOD"], cell["resource"]);
            assert_eq!(cell["resources"]["O2"], cell["oxygen"]);
            assert!(cell["resources"].get("WATER").is_none());
        }

        advance_one(&mut sim);
        let frame = sim.ecology.frame(&sim, 3);
        let types = frame["ecotypes"].as_array().unwrap();
        let double_mutant = types.iter().find(|t| t["genome"]["code"] == 3).unwrap();
        assert!(double_mutant["first_seen_tick"].is_null());
        for _ in 1..100 {
            advance_one(&mut sim);
        }
        let frame = sim.ecology.frame(&sim, 3);
        for t in frame["ecotypes"].as_array().unwrap() {
            if t["total_mol"].as_f64().unwrap() > 0.0 {
                assert!(t["first_seen_tick"].as_u64().unwrap() <= u64::from(sim.ticks));
            }
        }
        assert_eq!(sim.last.as_ref().unwrap().matter, 0);
        assert_eq!(sim.last.as_ref().unwrap().energy, Some(0));
    }

    #[test]
    fn a_genetic_reset_clears_first_appearance_history() {
        let scenario = small_genetic_scenario();
        let shared = Arc::new(Mutex::new(build(&scenario, 42).unwrap()));
        {
            let mut sim = lock(&shared);
            sim.running = false;
            for _ in 0..100 {
                advance_one(&mut sim);
            }
            assert!(
                sim.ecology.frame(&sim, 3)["ecotypes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|t| t["genome"]["code"] != 0 && !t["first_seen_tick"].is_null())
            );
        }
        assert_eq!(
            post(
                &shared,
                "/api/control",
                "{\"action\":\"reset\",\"seed\":\"42\"}"
            )
            .status,
            200
        );
        let sim = lock(&shared);
        assert!(
            !sim.running,
            "a paused reset exposes the actual initial state"
        );
        assert_eq!(sim.ticks, 0);
        let frame = sim.ecology.frame(&sim, 3);
        assert_eq!(frame["scenario"], scenario.name);
        assert_eq!(frame["config_hash"], sim.identity.config_hash.to_string());
        assert_eq!(frame["world_format_version"], WORLD_FORMAT_VERSION);
        assert_eq!(frame["seed"], "42");
        for t in frame["ecotypes"].as_array().unwrap() {
            if t["genome"]["code"] == 0 {
                assert_eq!(t["first_seen_tick"], 0);
            } else {
                assert!(t["first_seen_tick"].is_null());
            }
        }
    }

    // --- the identity -----------------------------------------------------

    #[test]
    fn a_seed_past_two_to_the_53_is_not_truncated_by_the_page() {
        // `JSON.parse` truncates past 2^53, which `observe/mod.rs` already wrote
        // down about its own stream. The identity shown on the page has to be
        // the one the run was started with.
        let seed = 12_345_678_901_234_567_890u64;
        let shared = sim_of(fixture::SCENARIO, seed);
        assert_eq!(
            state(&shared).get("seed").and_then(Json::as_str),
            Some(seed.to_string().as_str()),
            "the seed has to travel as a string"
        );
    }

    #[test]
    fn a_substance_id_that_would_break_the_json_is_escaped() {
        // `substance.id` is a free TOML string: `Registry::new` guards
        // duplicates and the count of thirty-one and nothing about characters.
        // Unescaped, a quote breaks the object and the page falls into its
        // `catch` and blames the connection.
        let awkward = "O\"2\nZ";
        let scenario = fixture::SCENARIO.replace("id = \"O2\"", "id = \"O\\\"2\\nZ\"");
        let scenario = scenario.replace("O2 = 2", "\"O\\\"2\\nZ\" = 2");
        let scenario = scenario.replace("O2 = 0.01", "\"O\\\"2\\nZ\" = 0.01");
        let scenario = scenario.replace("O2 = 1 }", "\"O\\\"2\\nZ\" = 1 }");
        let shared = sim_of(&scenario, 42);

        let state = state(&shared);
        let ids: Vec<&str> = state
            .get("fields")
            .and_then(Json::as_array)
            .expect("fields")
            .iter()
            .filter_map(|field| field.get("id").and_then(Json::as_str))
            .collect();
        assert!(
            ids.contains(&awkward),
            "the id did not arrive verbatim: {ids:?}"
        );
    }

    /// The shipped scenario, run for a thousand ticks, with both residuals held
    /// against the counters after every one of them.
    ///
    /// **The gap this fills is not a width, it is a kind.** The acceptance suite
    /// had `every_scenario_in_the_repository_loads`, which parses a file; nothing
    /// anywhere *ran* one. So the panic ADR-083 was written against — the
    /// `BOUNDARY_EXCHANGE` energy counter overflowing an `i64` on the **seventh**
    /// tick of `configs/scenarios/h2s-oxidation.toml`, one command and no process
    /// beyond diffusion — went past all five hundred and sixty-one tests in the
    /// corpus without touching one of them.
    ///
    /// # Three ways this test can be empty, and what is done about each
    ///
    /// **One: asserting nothing.** `Ledger::assert_closed` fires inside
    /// `Tick::advance` under `cfg(debug_assertions)` only, so under
    /// `cargo test --release` a thousand ticks would prove "it did not fall
    /// over" — which is true of a completely broken ledger the moment the
    /// counter stops overflowing. Hence the call below is this test's own, out
    /// of this test's own accumulators, in every profile.
    ///
    /// **Two: the wrong world.** A `World` built by hand skips
    /// `World::seed_ghosts`, so the lid trades with a reservoir of nothing and
    /// the counter grows *faster* — red before the fix and green after it, for a
    /// reason having nothing to do with the shipped scenario. So the world comes
    /// through `build`, which is the function `liminis serve` itself calls.
    ///
    /// **Three: not reading the counter.** A run that quietly stopped crediting
    /// would close every tick and finish in silence. The last assertion is
    /// therefore that `BOUNDARY_EXCHANGE` has left the `i64` range altogether:
    /// on the old width this run could not have got here at all.
    ///
    /// # Why one scenario, singular
    ///
    /// `configs/` holds two and only one of them runs. `config::load` is
    /// `read_to_string` plus `parse` — it does not call the validator — so
    /// `hello.toml` passes `every_scenario_in_the_repository_loads` and then
    /// fails in `build`: one of its six faces defaults to `exchange` (`z_max`)
    /// and it declares no `[boundary.reservoir]`. Generalising this test to
    /// "every scenario in `configs/`" would go red on that, for a reason that is
    /// not this one; skipping what does not build would make it green over the
    /// empty set. Whether that is a defect of the file or of `config::load` is
    /// ADR-078, which is not implemented in this tree.
    // TODO(test-time-budget): this test costs about seventeen minutes in a debug
    // build — a thousand ticks of 48^3 with five substances at the measured one
    // second a tick — against about twelve seconds when it fails at tick seven.
    // No document in the corpus gives `cargo test` a time budget: not a record,
    // not `ACCEPTANCE.md`, not CI; E-3 in `OPEN_QUESTIONS.md` is about a
    // throughput benchmark and not about a ceiling on acceptance time. The three
    // ways out — leave it, mark it `#[ignore]`, or shrink the grid — are a
    // decision and not a code change, and `#[ignore]` in particular is exactly
    // what would have let tick seven through again. It runs unmarked until a
    // record says otherwise.
    #[test]
    fn the_shipped_scenario_survives_a_thousand_ticks() {
        const TICKS: u32 = 1_000;
        const SEED: u64 = 42;

        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../configs/scenarios/h2s-oxidation.toml");
        let scenario = config::load(&path).expect("the shipped scenario parses");
        let mut sim = build(&scenario, SEED).expect("the shipped scenario builds");

        let n_substances = sim.world.registry().n_substances();
        let mut before = DomainSums::new(n_substances).expect("the sums before a tick");
        let mut after = DomainSums::new(n_substances).expect("the sums after a tick");

        for tick in 0..TICKS {
            sim.tick.domain_sums(&sim.world, &mut before);
            advance_one(&mut sim);
            sim.tick.domain_sums(&sim.world, &mut after);
            sim.ledger
                .assert_closed(sim.tick.reaction_nu(tick), &before, &after);
            assert_eq!(sim.ticks, tick + 1, "the tick counter skipped");
        }

        // The lid trades enthalpy with a reservoir ten kelvin colder than the
        // domain starts at, every substep of the enthalpy field, so this counter
        // is the one that overflowed. Below `i64::MIN` and not merely "large":
        // a counter that stayed inside the old range would mean the run no
        // longer reaches the case this test exists for.
        let counter = sim.ledger.energy(Channel::BoundaryExchange);
        assert!(
            counter < i128::from(i64::MIN),
            "the boundary counter stands at {counter}, still inside an i64: a \
             thousand ticks of the shipped scenario no longer reach the width \
             this test is about"
        );
    }

    /// Wait up to five seconds for a predicate over the simulation.
    fn wait_for(shared: &Arc<Mutex<Sim>>, done: impl Fn(&Sim) -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if done(&lock(shared)) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }
}
