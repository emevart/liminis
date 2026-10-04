//! Initial conditions out of noise, from the run key and from nothing else
//! (SPEC section 12.4, ADR-058, ADR-021).
//!
//! A layered field: one noise surface sets the sediment/water boundary, and one
//! octave stack per substance sets what that substance holds around its declared
//! typical amount. Every draw is [`crate::numeric::rand`] — the same
//! counter-based generator the chemistry rounds on — so there is no generator
//! with state anywhere on this path, and the whole world is a pure function of
//! `(grid, derivation, run_key)`.
//!
//! # It fills a `World`, it does not build one
//!
//! [`generate`] takes a `&mut World` that a caller has already allocated from a
//! `Registry`, and leaves the process-boundary invariant of ADR-057 satisfied:
//! **the front buffer holds state `N` for every lane** when it returns. Two
//! silent failures live in that sentence and no invariant sees either — a fill
//! left in the write buffer is an empty world in which both ledgers close
//! exactly and the residual is zero for ever, and a swap inside the loop over
//! lanes leaves half the lanes at zero by parity. Zero is a legal amount
//! (`world/field.rs`), so nothing falls over.
//!
//! So the shape is fixed: write every lane of a field into its write buffer,
//! then swap that field **once**, after the last lane.
//!
//! # What it costs, said rather than discovered
//!
//! A voxel computes its own amount out of the counters and reads nothing another
//! voxel wrote, which is what `generation_does_not_depend_on_the_order_of_the_walk`
//! is about and is worth more here than the arithmetic it costs. The arithmetic is
//! not free: at `128^3` with the fourteen substances of SPEC section 2.3 the grid
//! carries six octaves, so a fill is about `2.8e9` hashes, and **half of that is
//! the relief** — the boundary of a column is recomputed under every voxel of that
//! column and again for every substance. Memoising it per column is legal (the
//! relief is a pure function of `(x, y)` and a memo of a pure function carries no
//! state between voxels) and is not written, because it is paid once at load and
//! nothing has measured it yet.
//!
//! # The ceiling is a ceiling
//!
//! `max_conc` is the hard limit of a run and not an estimate (ADR-041): a world
//! that starts above it is a wrong start, and the check ADR-041 promises on the
//! tick boundary is not written yet, so an overflow born here would surface
//! somewhere else entirely. The band a substance is filled inside is therefore
//! bounded *by construction* — see [`excursion_of`] — and then the amounts that
//! were actually written are compared against `amount_at_max` anyway, because a
//! bound by construction is a claim about code that can stop being true.
//!
//! Nothing here clamps. ADR-041 says as much in as many words: a product past
//! the declared limit means a wrong declaration, and the run is to fall over
//! loudly rather than saturate quietly.
//!
//! # Where the numbers come from, and where they do not
//!
//! The stochastic background takes the side of the sediment/water boundary from
//! `[initial.layer]` (ADR-077). A scenario may then replace that background for
//! named substances with `[initial.concentration]` and spherical
//! `[[initial.inoculum]]` overwrites. The band around `typical_conc` remains a
//! consequence and not a key — see [`excursion_of`] — and the spectrum of the
//! octaves and base of the `purpose` counter remain build-time boundaries with
//! an open question against them.
//!
//! Everything else comes from something that already exists: the derived
//! `amount_at_typical` and `amount_at_max` of each substance (ADR-039), the
//! declared side, and the extents of the grid. Nothing is recomputed here.

use anyhow::{Result, bail};

use crate::config::{Derived, DerivedInitial, Layer};
use crate::numeric::{M32, M64, rand};
use crate::world::{Grid, LaneRef, MAX_SUBSTANCES, World};

/// Base of the `purpose` counter space.
// TODO(worldgen-base): this number is a build-time boundary and not a key, and
// that much *is* decided: ADR-077 refused it a place in `[initial]` by the
// argument of ADR-058 — a constant of compile time would become a value of run
// time in every kernel that wanted its own `purpose`, and the window
// `[WORLDGEN_BASE, WORLDGEN_BASE + 256)` would stop being reservable against
// anything, since it would move from scenario to scenario. What is *not*
// decided is the value, and the question is not this file's alone — it is
// `TODO(noise-base)` in `kernels/noise.rs`, one field over. Filed as
// `OPEN_QUESTIONS.md` A-17, where only half of it is closed.
// There is no registry of `purpose` in the corpus, `reaction_id` is taken from
// the *name* of a reaction (ADR-027), and nothing keeps the two spaces apart. A
// collision between an octave of this field and a reaction gives the two one
// stream of draws — under a chemistry that looks entirely plausible.
//
// Taken here the way `NOISE_BASE` is taken there: a constant with a TODO, not as
// knowledge. What the value buys is that the 256 counters this module occupies
// are far from the small integers a hand-written `purpose` would pick, and that
// the window is disjoint from the velocity noise's — which
// `the_purpose_space_is_disjoint_from_the_velocity_noise` holds and is the only
// half of the problem a test can reach. The other half is unbounded, because a
// hash of a reaction name is.
pub const WORLDGEN_BASE: u32 = 0x5747_454E;

/// The largest number of octaves a build supports.
///
/// A build-time bound in the manner of [`NOISE_OCTAVE_MAX`](crate::kernels::noise::NOISE_OCTAVE_MAX),
/// and it sizes the same thing: the slice of the `purpose` counter this module
/// occupies is `WORLDGEN_BASE .. WORLDGEN_BASE + WORLDGEN_SLOTS*WORLDGEN_OCTAVE_MAX`,
/// and without a bound that slice is open-ended and cannot be reserved against
/// anything at all. Eight octaves is a domain of 512 voxels an axis, five past
/// the three a 16-voxel test grid carries.
pub const WORLDGEN_OCTAVE_MAX: u32 = 8;

/// Slots per octave: the relief plus one per substance.
pub const WORLDGEN_SLOTS: u32 = MAX_SUBSTANCES + 1;

/// The slot of the relief field.
pub const RELIEF_SLOT: u32 = 0;

/// The slot of substance `s`.
#[inline]
#[must_use]
pub const fn substance_slot(s: u32) -> u32 {
    1 + s
}

/// The `purpose` counter of one (slot, octave) pair.
///
/// Injective over the whole build, and that is not tidiness: two slots sharing a
/// `purpose` draw **the same number** at every lattice node they share, so two
/// substances come out with identical fields, or an octave correlates with the
/// relief. The corpus is green throughout —
/// `the_purpose_space_is_disjoint_from_the_velocity_noise` is what sees it, and
/// it is the twin of `each_octave_and_component_draws_its_own_stream` one field
/// over.
///
/// A `const fn` of two compile-time quantities and never a value derived from
/// the seed, for the reason ADR-058 gives: a `purpose` that depended on the run
/// would make "taken from the position" and "taken from the seed"
/// indistinguishable.
#[inline]
#[must_use]
pub const fn worldgen_purpose(slot: u32, octave: u32) -> u32 {
    WORLDGEN_BASE + WORLDGEN_SLOTS * octave + slot
}

/// The fixed-point shift of the noise.
///
/// Twenty bits, which is what the arithmetic below can carry and no more: the
/// widest product is a corner weight times a corner value times the interpolation
/// of three axes, and at `2^20` that is `2^60` before the shifts bring it back.
/// The noise never touches `Q` — this module runs once, on the host, at load, and
/// is not a kernel (ADR-015 governs `kernels/**` and nothing else) — so the
/// mode-independence a `Q` would buy is not worth an unnecessary rounding class.
const UNIT_SHIFT: u32 = 20;

/// One in the fixed point of the noise.
const UNIT: i64 = 1 << UNIT_SHIFT;

/// The second counter of every draw: the tick the initial state belongs to.
///
/// Zero, and named rather than written as a literal at four call sites. The
/// canonical order of `rand` is `(voxel_idx, tick, purpose, run_key)`
/// (`numeric/rng.rs`), and the initial state is the state before tick zero has
/// run; the noise of `kernels/noise.rs` walks this counter with the tick and this
/// one does not, which is the whole difference between a field that stirs and a
/// field that is a starting condition.
const WORLDGEN_TICK: u32 = 0;

/// What one substance was filled with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubstanceFill {
    /// The id as written in TOML.
    pub id: String,
    /// The lowest amount written over the domain.
    pub floor: i128,
    /// The highest amount written over the domain.
    pub peak: i128,
    /// Units held by one voxel at `typical_conc` (ADR-039).
    pub amount_at_typical: i128,
    /// Units held by one voxel at `max_conc` (ADR-039).
    pub amount_at_max: i128,
    /// Whether the field came out constant.
    ///
    /// Not `config::Layer::Uniform`, which is one word away and a different
    /// thing: that one says the substance has no layer term, this one says the
    /// field has no variation at all. A substance declared `uniform` carries its
    /// own octaves and is not constant. Reading the two as one disarms the only
    /// alarm this report has — a world filled with a constant is a world in
    /// which both invariants close exactly, for ever.
    pub uniform: bool,
    /// The half-width of the band the fill stays inside, around
    /// [`SubstanceFill::amount_at_typical`]. See [`excursion_of`].
    pub excursion: i128,
}

/// What a generation did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldgenReport {
    /// How many octaves the extents carry.
    pub octaves: u32,
    /// Lattice nodes of the relief at its finest octave.
    pub relief_nodes: u32,
    /// One record per substance, in substance order.
    pub per_substance: Vec<SubstanceFill>,
}

impl WorldgenReport {
    /// The report as lines a loader can print.
    ///
    /// Printed at load, and that is the only reason a degenerate fill is
    /// distinguishable from a working one: a world filled with a constant is a
    /// legal world in which both invariants close exactly, for ever.
    #[must_use]
    pub fn report(&self) -> String {
        let mut out = format!(
            "worldgen: {} octaves, {} relief nodes\n",
            self.octaves, self.relief_nodes
        );
        for fill in &self.per_substance {
            out.push_str(&format!(
                "substance {}: actual [{}, {}], typical {}, ceiling {}, uniform {}\n",
                fill.id,
                fill.floor,
                fill.peak,
                fill.amount_at_typical,
                fill.amount_at_max,
                fill.uniform
            ));
        }
        out
    }
}

/// Fill the front buffers of `world` from the run key.
///
/// `run_key` is the fourth counter of every draw and arrives already folded:
/// [`crate::numeric::run_key`] collapses the 64-bit seed on the host, once
/// (ADR-058). Folding it again here would put the fold in two places, and a
/// `u64` parameter is what that mistake looks like.
///
/// # Errors
///
/// Returns an error if the derivation and the registry hold different substances
/// or hold them in a different order, if the grid carries no octave of noise, if
/// a substance's declared band does not fit its derived storage width, if an
/// amount that was written falls outside `[0, amount_at_max]`, or if a declared
/// inoculum reaches no voxel centre.
pub fn generate(world: &mut World, derived: &Derived, run_key: u32) -> Result<WorldgenReport> {
    let n = world.registry().n_substances() as usize;
    if derived.substances().len() != n {
        bail!(
            "the derivation holds {} substances and the registry {n}",
            derived.substances().len()
        );
    }
    // The same substances **in the same order**, and not merely the same number
    // of them. `config/derive.rs` says on `DerivedSubstance::diffusivity` that
    // the two orders "agree only because nothing has yet had a reason to reorder
    // either"; out of step, one substance's amounts land on another's lane, the
    // ceiling is compared against the wrong limit, and both invariants close.
    for (s, substance) in derived.substances().iter().enumerate() {
        let registered = world.registry().id_of(s as u32);
        if substance.id != registered {
            bail!(
                "the derivation and the registry disagree at index {s}: the \
                 derivation holds `{}` there and the registry holds `{registered}`. \
                 The amounts of one substance would land on the lane of the other, \
                 the ceiling would be compared against the wrong limit, and both \
                 invariants would close (ADR-056)",
                substance.id
            );
        }
    }

    let octaves = carried_octaves(world.grid())?;

    let mut fills: Vec<SubstanceFill> = derived
        .substances()
        .iter()
        .map(|s| SubstanceFill {
            id: s.id.clone(),
            floor: 0,
            peak: 0,
            amount_at_typical: s.amount_at_typical,
            amount_at_max: s.amount_at_max,
            excursion: excursion_of(s.amount_at_typical, s.amount_at_max),
            uniform: true,
        })
        .collect();
    // The declared sides, in the order of the derivation and taken from the same
    // slice as the fills in the same order — never from a table keyed by name a
    // second time. The report does not grow a field for them: what prints the
    // side of a substance is the canonical form of the config, which names every
    // one of them (ADR-077).
    let layers: Vec<Layer> = derived.substances().iter().map(|s| s.layer).collect();
    fill_world(
        world,
        derived.initial(),
        &mut fills,
        &layers,
        octaves,
        run_key,
    )?;

    Ok(WorldgenReport {
        octaves,
        // The finest octave of the relief has `2^octaves` nodes per axis: octave
        // `k` carries `2^(k+1)`, and the last one run is `k = octaves - 1`.
        relief_nodes: 1 << octaves,
        per_substance: fills,
    })
}

/// The half-width of the band a substance is filled inside.
///
/// `min(typical, max - typical) / 2`, by integer division towards zero, and
/// ratified as a **rule** by ADR-077 rather than left to a key. The two
/// constraints it satisfies are both named by the corpus:
///
///   - the peak stays clear of `amount_at_max`, which is a hard ceiling and not
///     an estimate (ADR-041), and clear by a margin rather than by a rounding —
///     a peak sitting *on* the limit is what a clamp looks like, and ADR-041
///     forbids clamping;
///   - the floor stays positive, or a substance starts absent from part of the
///     domain and the difference between "the scenario declared none here" and
///     "the generator wandered below zero and was clamped" is invisible
///     afterwards.
///
/// Both are exact integer identities and not margins: from
/// `2*excursion <= min(typical, max - typical)` follow
/// `2*(typical - excursion) >= typical` and
/// `2*(typical + excursion) <= typical + max`, which is what
/// `the_band_is_bounded_by_both_headrooms` asserts in the unit and
/// `worldgen_respects_declared_max_conc` on what was written.
///
/// Half of the headroom *to the ceiling* is the form that suggests itself first
/// and does not survive the corpus: the oxygen of the acceptance fixture
/// declares `typical_conc = 0.05` against `max_conc = 1.0`, and half of that
/// headroom puts the floor 8.5 typical pools below zero — on three substances of
/// five (ADR-077). A declared fraction was refused for a second reason worth
/// keeping in sight here: a fraction is a float, the half-width has to be an
/// integer, and no record assigns a rounding rule to `e*min(typ, H)` — while the
/// division below is exact and truncates.
///
/// Zero is a legal answer — a substance whose typical amount rounds to zero, or
/// whose declared maximum is its typical, is filled with a constant, and
/// [`SubstanceFill::uniform`] is what says so in the report.
#[inline]
#[must_use]
fn excursion_of(typical: i128, max: i128) -> i128 {
    let headroom = max - typical;
    if headroom <= 0 {
        return 0;
    }
    typical.min(headroom) / 2
}

/// How many octaves the extents of a grid carry, refusing a grid that carries
/// none.
///
/// Octave `k` has a wavelength of `extent / 2^k`, so its lattice has `2^(k+1)`
/// nodes per axis and a spacing of `extent >> (k+1)` voxels. Two conditions, and
/// the second is the one that is easy to leave out:
///
/// - the spacing is at least two voxels. Below that the lattice *is* the mesh and
///   the octave is white noise, which is the artefact `kernels/noise.rs` rejects
///   the same way;
/// - the spacing **divides** the extent. The wrap is `& (nodes - 1)`, which is
///   the modulo of a power of two and is a modulo of nothing at all when `nodes`
///   is not one: an extent that does not divide gives a skewed lattice and a
///   seam, under a fill that looks entirely healthy.
///
/// Every axis has to carry the octave, so the answer is the smallest of the
/// three — the substance noise is three-dimensional even though the relief is
/// not.
fn carried_octaves(grid: &Grid) -> Result<u32> {
    let octaves = octave_count(grid);
    if octaves == 0 {
        bail!(
            "a grid of {}x{}x{} voxels carries no octave of noise: octave `k` has \
             a lattice spacing of `n >> (k + 1)` voxels, and needs that spacing to \
             be at least two voxels and to divide the extent, which some axis of \
             this grid fails even at `k = 0`. What comes out of a zero octave \
             count is not a poor initial state but a flat one — every column of \
             the domain identical, no horizontal structure of any kind — and a \
             world like that is one in which both invariants close exactly and \
             nothing the scenario is about ever happens",
            grid.nx(),
            grid.ny(),
            grid.nz()
        );
    }
    Ok(octaves)
}

/// How many octaves the extents carry, as a number. The refusal is
/// [`carried_octaves`]; this is the arithmetic under it, kept apart so that the
/// count can be asserted against the extents that produce it.
fn octave_count(grid: &Grid) -> u32 {
    let mut octaves = 0;
    while octaves < WORLDGEN_OCTAVE_MAX {
        let carried = [grid.nx(), grid.ny(), grid.nz()].into_iter().all(|extent| {
            let spacing = extent >> (octaves + 1);
            // No overflow: `spacing` was shifted down by the same amount.
            spacing >= 2 && (spacing << (octaves + 1)) == extent
        });
        if !carried {
            break;
        }
        octaves += 1;
    }
    octaves
}

/// Write every lane, then leave state `N` in the front buffer of every field.
///
/// The swap is per **field** and happens after the last lane of that field, which
/// is the only correct place for it: `Field::swap` exchanges the buffers of every
/// lane at once (`world/field.rs`), so a swap inside the loop would carry the
/// lanes already written back out again and leave half of them at zero by parity.
fn fill_world(
    world: &mut World,
    initial: &DerivedInitial,
    fills: &mut [SubstanceFill],
    layers: &[Layer],
    octaves: u32,
    run_key: u32,
) -> Result<()> {
    // Copied rather than borrowed: the writes below take `&mut World`, and a
    // `Grid` is four numbers and six boundary conditions.
    let grid = *world.grid();
    let n_voxels = grid.n_voxels();
    let mut inoculum_hits = vec![0u32; initial.inocula.len()];

    // The sides arrive in the order the fills were built in, which is the order
    // of the derivation, which `generate` has already checked against the
    // registry. A `zip` would truncate on a mismatch instead of saying so, and
    // the world would come out with the last substance on the default side.
    assert_eq!(
        fills.len(),
        layers.len(),
        "one declared side per substance filled"
    );

    for (s, fill) in fills.iter_mut().enumerate() {
        let level = Level {
            typical: fill.amount_at_typical,
            excursion: fill.excursion,
            layer: layers[s],
        };
        let slot = substance_slot(s as u32);
        let lane = world.lane_of(s as u32);
        let background = initial
            .concentration
            .iter()
            .find(|decl| decl.substance == s as u32)
            .map(|decl| decl.amount);
        let inocula: Vec<_> = initial
            .inocula
            .iter()
            .enumerate()
            .filter(|(_, inoculum)| inoculum.substance == s as u32)
            .collect();

        // The theorem the narrowing below stands on, stated and checked rather
        // than assumed. `version.rs` names the alternative as the defect it
        // exists to prevent: a bare `as i32` turns a pool that overflowed into
        // one of the opposite sign, in silence, and a negative pool is the one
        // thing ADR-068's three division rules say no invariant sees.
        let width_max = match lane {
            LaneRef::Narrow(_) => i128::from(i32::MAX),
            LaneRef::Wide(_) => i128::from(i64::MAX),
        };
        if fill.amount_at_max > width_max {
            bail!(
                "substance `{}` holds {} units at max_conc, past the {width_max} of \
                 the storage width the derivation gave it (ADR-040)",
                fill.id,
                fill.amount_at_max
            );
        }
        let lowest_possible = level.typical - level.excursion;
        let highest_possible = level.typical + level.excursion;
        if lowest_possible < 0 || highest_possible > fill.amount_at_max {
            bail!(
                "substance `{}` would be filled over [{lowest_possible}, \
                 {highest_possible}], outside the [0, {}] its declared \
                 concentrations allow (ADR-041)",
                fill.id,
                fill.amount_at_max
            );
        }

        let mut floor = i128::MAX;
        let mut peak = i128::MIN;
        match lane {
            LaneRef::Narrow(lane) => {
                let field = world.amounts_32_mut().expect("the narrow field");
                let (_, dst) = field.lane_pair_mut(lane);
                for idx in 0..n_voxels {
                    let stochastic = || amount_at(&grid, level, slot, octaves, idx, run_key);
                    let background = background.unwrap_or_else(stochastic);
                    let amount = if inocula.is_empty() {
                        background
                    } else {
                        declared_amount_at(
                            &grid,
                            initial.dx,
                            idx,
                            background,
                            &inocula,
                            &mut inoculum_hits,
                        )
                    };
                    floor = floor.min(amount);
                    peak = peak.max(amount);
                    dst[idx as usize] = M32::from_i64_clamping(narrowed(amount));
                }
            }
            LaneRef::Wide(lane) => {
                let field = world.amounts_64_mut().expect("the wide field");
                let (_, dst) = field.lane_pair_mut(lane);
                for idx in 0..n_voxels {
                    let stochastic = || amount_at(&grid, level, slot, octaves, idx, run_key);
                    let background = background.unwrap_or_else(stochastic);
                    let amount = if inocula.is_empty() {
                        background
                    } else {
                        declared_amount_at(
                            &grid,
                            initial.dx,
                            idx,
                            background,
                            &inocula,
                            &mut inoculum_hits,
                        )
                    };
                    floor = floor.min(amount);
                    peak = peak.max(amount);
                    dst[idx as usize] = M64::from_i64_clamping(narrowed(amount));
                }
            }
        }

        // The band above is a bound by construction, and this is the same bound
        // measured on what was actually written. Both, because a bound by
        // construction is a claim about code and the code is what changes.
        if floor < 0 || peak > fill.amount_at_max {
            bail!(
                "substance `{}` was filled over [{floor}, {peak}], outside the \
                 [0, {}] `max_conc` declares. `max_conc` is the hard ceiling of a \
                 run and not an estimate, and a world that starts above it does \
                 not survive its first tick (ADR-041)",
                fill.id,
                fill.amount_at_max
            );
        }
        fill.floor = floor;
        fill.peak = peak;
        fill.uniform = floor == peak;
    }

    for (inoculum, hits) in initial.inocula.iter().zip(inoculum_hits) {
        if hits == 0 {
            let id = world.registry().id_of(inoculum.substance);
            bail!(
                "initial inoculum of substance `{id}` at [{}, {}, {}] m with \
                 radius {} m contains no voxel centre",
                inoculum.center[0],
                inoculum.center[1],
                inoculum.center[2],
                inoculum.radius
            );
        }
    }

    // One swap per field, after the last lane of it. See the function doc.
    if let Some(field) = world.amounts_32_mut() {
        field.swap();
    }
    if let Some(field) = world.amounts_64_mut() {
        field.swap();
    }
    Ok(())
}

/// Apply the declared spatial initial state to one stochastic voxel amount.
///
/// A concentration override has already replaced the stochastic amount before
/// this function is called. An inoculum then overwrites the voxel when its
/// centre lies in the closed sphere. Same-substance spheres cannot overlap (the
/// config validator rejects them), so this loop has no declaration-order
/// semantics; different substances necessarily write different lanes.
fn declared_amount_at(
    grid: &Grid,
    dx: f64,
    idx: u32,
    background: i128,
    inocula: &[(usize, &crate::config::DerivedInoculum)],
    hits: &mut [u32],
) -> i128 {
    let (x, y, z) = grid.coords(idx);
    let mut amount = background;

    for &(inoculum_idx, inoculum) in inocula {
        if voxel_centre_in_closed_sphere([x, y, z], dx, inoculum.center, inoculum.radius) {
            amount = inoculum.amount;
            hits[inoculum_idx] += 1;
        }
    }

    amount
}

/// Whether one voxel centre lies in a declared closed sphere.
///
/// Shared with config validation so the validator cannot reject a sphere that
/// world generation would seed. The comparison is in voxel units: decimal metre
/// coordinates such as `0.00015` cannot generally be represented exactly, and
/// comparing their metre squares made mathematically tangent centres disagree by
/// one ulp. Sixteen arithmetic ulps preserve the closed boundary without a
/// physically meaningful expansion.
pub(crate) fn voxel_centre_in_closed_sphere(
    voxel: [u32; 3],
    dx: f64,
    declared_centre: [f64; 3],
    declared_radius: f64,
) -> bool {
    let centre = voxel.map(|coordinate| f64::from(coordinate) + 0.5);
    let declared_centre = declared_centre.map(|coordinate| coordinate / dx);
    let radius = declared_radius / dx;
    let distance_squared = centre
        .iter()
        .zip(declared_centre)
        .map(|(voxel, declared)| {
            let delta = *voxel - declared;
            delta * delta
        })
        .sum::<f64>();
    let radius_squared = radius * radius;
    let tolerance = 16.0 * f64::EPSILON * distance_squared.abs().max(radius_squared.abs()).max(1.0);
    distance_squared <= radius_squared + tolerance
}

/// An `i128` amount inside the storage range, as an `i64`.
///
/// The twin of `M::from_i64_clamping` one width up, and written the same way and
/// for the same reason: `amount as i64` is the shorter text and it wraps, so an
/// amount that escaped the band above would arrive as a plausible number of the
/// opposite sign instead of as a saturated one. The callers have already refused
/// the band that could reach either bound, so this saturates in no run that
/// loads — which is exactly why it must not be a cast.
#[inline]
fn narrowed(amount: i128) -> i64 {
    i64::try_from(amount).unwrap_or(if amount.is_negative() {
        i64::MIN
    } else {
        i64::MAX
    })
}

/// One octave of noise at one voxel: the eight lattice corners around it, faded
/// together.
///
/// Value noise, in the fixed point of [`UNIT`], and the shape is
/// `kernels/noise.rs`'s: one draw per corner, eight corners, a smoothstep fade
/// whose derivative vanishes at both ends so that the summed field is continuous
/// in its first derivative across every lattice plane. Plain linear interpolation
/// would leave a grid of creases, visible to the eye and in no invariant.
///
/// The lattice wraps in every axis: `& (nodes - 1)` is the modulo of a power of
/// two, and [`octave_count`] is what guarantees `nodes` is one.
fn lattice_value(
    grid: &Grid,
    x: u32,
    y: u32,
    z: u32,
    octave: u32,
    purpose: u32,
    run_key: u32,
) -> i64 {
    let spacing_x = grid.nx() >> (octave + 1);
    let spacing_y = grid.ny() >> (octave + 1);
    let spacing_z = grid.nz() >> (octave + 1);
    debug_assert!(
        spacing_x >= 2 && spacing_y >= 2 && spacing_z >= 2,
        "octave {octave} has a lattice spacing under two voxels on a {}x{}x{} grid, \
         which `octave_count` was supposed to have refused",
        grid.nx(),
        grid.ny(),
        grid.nz()
    );

    let nodes_x = grid.nx() / spacing_x;
    let nodes_y = grid.ny() / spacing_y;
    let nodes_z = grid.nz() / spacing_z;

    let jx = x / spacing_x;
    let jy = y / spacing_y;
    let jz = z / spacing_z;

    // The voxel centre sits at `(rem + 0.5)/spacing` inside its lattice cell,
    // which over the denominator `2*spacing` is the integer `2*rem + 1`.
    let fade_x = fade(unit_fraction(2 * (x - jx * spacing_x) + 1, 2 * spacing_x));
    let fade_y = fade(unit_fraction(2 * (y - jy * spacing_y) + 1, 2 * spacing_y));
    let fade_z = fade(unit_fraction(2 * (z - jz * spacing_z) + 1, 2 * spacing_z));

    let mut value = 0;
    for corner in 0..8u32 {
        let cx = if corner & 1 == 1 {
            (jx + 1) & (nodes_x - 1)
        } else {
            jx
        };
        let cy = if corner & 2 == 2 {
            (jy + 1) & (nodes_y - 1)
        } else {
            jy
        };
        let cz = if corner & 4 == 4 {
            (jz + 1) & (nodes_z - 1)
        } else {
            jz
        };

        let node = cx + cy * nodes_x + cz * nodes_x * nodes_y;
        // Read as an `i32`, a uniform `u32` is a uniform signed value; the shift
        // brings it into `[-UNIT, UNIT)`. A reinterpretation and not a narrowing
        // — the same one `kernels/noise.rs` makes on the same draw.
        let corner_value = i64::from(rand(node, WORLDGEN_TICK, purpose, run_key) as i32)
            >> (i32::BITS - 1 - UNIT_SHIFT);

        let weight_x = if corner & 1 == 1 {
            fade_x
        } else {
            UNIT - fade_x
        };
        let weight_y = if corner & 2 == 2 {
            fade_y
        } else {
            UNIT - fade_y
        };
        let weight_z = if corner & 4 == 4 {
            fade_z
        } else {
            UNIT - fade_z
        };

        let weight = (((weight_x * weight_y) >> UNIT_SHIFT) * weight_z) >> UNIT_SHIFT;
        value += (weight * corner_value) >> UNIT_SHIFT;
    }

    value
}

/// The octaves summed and normalised back into `[-UNIT, UNIT]`.
///
// TODO(worldgen-spectrum): the *shape* of the spectrum is still decided by
// nobody, but it is no longer unmentioned: ADR-077 refused it a key in
// `[initial]`, and by measurement rather than by preference. The justification a
// key would have had — "it decides how much structure sits at the scale of a
// voxel", that is
// `the_initial_field_has_structure_larger_than_a_voxel` — is false. The mean
// step between neighbours over a span runs 0.0566 -> 0.0366 for exponents from 0
// to 3 on the layered branch and 0.0714 -> 0.0437 on the uniform one, against a
// threshold of 0.125: what guarantees structure larger than a voxel is the
// ladder of octaves, whose finest lattice never steps under two voxels and whose
// zero count is a load error. Two further reasons are recorded there: a declared
// exponent would be quantised in `derive.rs` while the raw value is hashed, so
// `1.00` and `1.01` would give one world under two identities; and the question
// is half of `TODO(noise-spectrum)` in `kernels/noise.rs`, which a kernel cannot
// read without a new `Params` field (ADR-015). The value stays a build-time
// boundary. Filed as `OPEN_QUESTIONS.md` A-17.
///
/// The normalisation is by the sum of the weights and not by the number of
/// octaves: octave `k` is weighted `2^-k` and the divisor is `sum_k 2^-k`, so the
/// result is inside `[-UNIT, UNIT]` whatever the draws are and whatever the
/// octave count is. That bound is what [`amount_at`] rests on to keep a fill
/// inside its band, so it is a property and not an observation.
fn summed_noise(grid: &Grid, x: u32, y: u32, z: u32, slot: u32, octaves: u32, run_key: u32) -> i64 {
    if octaves == 0 {
        return 0;
    }
    let mut summed = 0;
    let mut weights = 0;
    for octave in 0..octaves {
        let purpose = worldgen_purpose(slot, octave);
        summed += lattice_value(grid, x, y, z, octave, purpose, run_key) >> octave;
        weights += UNIT >> octave;
    }
    summed * UNIT / weights
}

/// The height of the sediment/water boundary over one column, in voxels.
///
/// A surface and not a number: SPEC section 12.4 asks for layered noise, and a
/// relief taken from a linear index instead of the decoded coordinates gives a
/// stripe along X — the class of mistake `covering_cell` describes in
/// `world/world.rs`.
///
/// The boundary is kept strictly inside the domain, with a voxel to spare on each
/// side. A boundary on the floor or on the lid leaves one of the two layers out
/// of the world entirely, which is not a degenerate initial condition but a
/// different one, and nothing downstream would say so.
fn relief_height(grid: &Grid, x: u32, y: u32, octaves: u32, run_key: u32) -> i64 {
    let nz = i64::from(grid.nz());
    let base = nz / 2;
    let span = (base - 1).min(nz - 2 - base).max(0);
    if span == 0 {
        // Four voxels or fewer, and there is nowhere to put a boundary that
        // leaves both layers in the world. Flat, and the report's octave count is
        // what tells a reader the world is this small.
        return base;
    }
    // Its own slot, so the relief and the chemistry never share a stream. The
    // plane `z = 0` and not the voxel's own `z`: the boundary is a property of
    // the column.
    let noise = summed_noise(grid, x, y, 0, RELIEF_SLOT, octaves, run_key);

    // Halves away from zero, which is the project's rounding rule (NUMERIC.md,
    // ADR-034) and here is the difference between a surface and a plane.
    // Truncation is the shorter text and it biases every column towards `base`:
    // a lattice value is a weighted average of eight uniform draws whose weights
    // sum to one, so it concentrates well inside `[-UNIT, UNIT]` — over a smooth
    // row, the whole row can sit under one voxel of relief and come out flat,
    // with the noise present and doing nothing. That is what
    // `the_layer_boundary_varies_horizontally` failed on.
    let scaled = noise * span;
    base + (scaled + scaled.signum() * (UNIT / 2)) / UNIT
}

/// The level a substance is filled around, how far it may wander from it, and
/// which side of the boundary it is enriched on.
///
/// The side rides here rather than arriving as a seventh argument of
/// [`amount_at`]: it belongs to the same substance as the two numbers beside it,
/// and a parameter list is where two things that belong together fall out of
/// step.
#[derive(Clone, Copy, Debug)]
struct Level {
    /// Units held by one voxel at `typical_conc` (ADR-039).
    typical: i128,
    /// The half-width of the band, from [`excursion_of`].
    excursion: i128,
    /// The declared side (ADR-077), carried through from
    /// `DerivedSubstance::layer`.
    layer: Layer,
}

/// What one voxel holds of one substance.
///
/// Two terms, and the first is what makes this a layered field rather than
/// `rand(voxel_idx, ...)`:
///
/// - the **layer**: below the relief is sediment and above it is water, and the
///   two sit on opposite sides of the declared typical amount. Which end of the
///   z axis is which is not a detail of this line: swap the two and every
///   measurement taken across the boundary still agrees with itself, because a
///   partition read off a substance's own deviation is relabelled along with the
///   field — the default world comes out stratified upside down with both
///   ledgers closing. What holds the orientation is the floor and the lid of the
///   domain, the two planes the relief cannot cross, and
///   `layer_sides_are_anticorrelated_across_the_boundary` reads them. Which side
///   a substance is rich on is the scenario's to say — `[initial.layer]`, ADR-077 —
///   and a substance may say `uniform` and have no layer term at all. White noise
///   passes both seed tests, passes the ceiling, looks like a filled world, and
///   leaves stratification, the oxycline and the oxidation front with no initial
///   condition at all — a failure that would surface waves later and in another
///   file;
/// - the **octaves**: the substance's own noise stack, on its own slot of the
///   `purpose` space, so that two substances are not one field under two names.
///
/// The side comes from the config and never from `run_key`. A per-substance sign
/// taken from the key costs nothing and separates the worlds of two seeds for
/// free — and it would make the oxycline a property of the seed, so that "why is
/// there no oxidation front at seed 7" has no answer in any file (ADR-058,
/// ADR-077). `the_layer_side_does_not_change_with_the_seed` is the only thing
/// that tells the two apart, because the rejected form makes
/// `different_seed_gives_a_different_initial_state` greener rather than red.
///
/// The result is inside `[typical - excursion, typical + excursion]` by
/// construction, on both branches: the noise alone is inside `[-UNIT, UNIT]`,
/// the mean of it with a term of amplitude `UNIT` is too, and the last division
/// truncates towards zero.
///
/// Truncation and not a shift. `>> UNIT_SHIFT` is arithmetically a floor, so for
/// a negative blend the two differ by one unit — that is, in every voxel of one
/// half of the world. No invariant sees the difference: it shows up only as
/// another world under the same seed and the same version, and there is no
/// recorded world hash in the repository to disagree with (NUMERIC.md, ADR-034,
/// ADR-077).
fn amount_at(grid: &Grid, level: Level, slot: u32, octaves: u32, idx: u32, run_key: u32) -> i128 {
    if level.excursion == 0 {
        return level.typical;
    }
    let (x, y, z) = grid.coords(idx);
    let noise = summed_noise(grid, x, y, z, slot, octaves, run_key);
    let blended = match level.layer {
        // No layer term at all, and the substance's own noise at full amplitude
        // — not halved. `blended = noise/2` would keep today's spread and make
        // one key govern two things at once, the side and the width of the
        // actual scatter: a substance declared `uniform` would take half the band
        // of its layered neighbour under an identical declaration (ADR-077,
        // rejected by name). No declaration is compared against a width to catch
        // that; what catches it is the ratio of the two branches over the floor
        // plane, where the layer term is present and constant, so the halved form
        // stands at one to one instead of two to one
        // (`a_substance_declared_uniform_has_no_layer_step`, (d)).
        Layer::Uniform => noise,
        Layer::Sediment | Layer::Water => {
            let below = i64::from(z) < relief_height(grid, x, y, octaves, run_key);
            let rich_below = matches!(level.layer, Layer::Sediment);
            let layer = if below == rich_below { UNIT } else { -UNIT };
            (layer + noise) / 2
        }
    };
    level.typical + (level.excursion * i128::from(blended)) / i128::from(UNIT)
}

/// The smoothstep fade `t*t*(3 - 2*t)`, in the fixed point of [`UNIT`].
#[inline]
fn fade(t: i64) -> i64 {
    let square = (t * t) >> UNIT_SHIFT;
    (square * (3 * UNIT - 2 * t)) >> UNIT_SHIFT
}

/// `numerator / denominator`, in the fixed point of [`UNIT`].
///
/// An exact division rather than a shift, unlike the twin in `kernels/noise.rs`:
/// there the lattice spacing is a power of two because the velocity grid's
/// extents are, and here an extent of six carries an octave at a spacing of three
/// (see [`octave_count`]).
#[inline]
fn unit_fraction(numerator: u32, denominator: u32) -> i64 {
    debug_assert!(numerator < denominator, "{numerator} is not a fraction");
    i64::from(numerator) * UNIT / i64::from(denominator)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernels::noise::{NOISE_BASE, NOISE_OCTAVE_MAX};
    use crate::world::{Boundary, Registry, SubstanceDecl, Width, WorldLayout};
    use std::collections::BTreeSet;

    const N: u32 = 16;

    /// A grid periodic in X and Y, closed floor and lid.
    fn grid(nx: u32, ny: u32, nz: u32) -> Grid {
        Grid::new(
            nx,
            ny,
            nz,
            [
                Boundary::Periodic,
                Boundary::Periodic,
                Boundary::Periodic,
                Boundary::Periodic,
                Boundary::Closed,
                Boundary::Closed,
            ],
        )
        .expect("the grid")
    }

    /// A band wide enough that the noise is not quantised away.
    fn level() -> Level {
        Level {
            typical: 1_000_000,
            excursion: 500_000,
            layer: Layer::Sediment,
        }
    }

    /// The same band with no layer term, which is the other branch of
    /// [`amount_at`] and half of what this module can be asked to produce.
    fn flat_level() -> Level {
        Level {
            layer: Layer::Uniform,
            ..level()
        }
    }

    #[test]
    fn the_purpose_space_is_disjoint_from_the_velocity_noise() {
        // Two noise fields sharing a `purpose` draw the same number at the nodes
        // they share and correlate — with the whole corpus green. The argument is
        // `each_octave_and_component_draws_its_own_stream`'s, one field over.
        let mut seen = BTreeSet::new();
        for octave in 0..WORLDGEN_OCTAVE_MAX {
            for slot in 0..WORLDGEN_SLOTS {
                seen.insert(worldgen_purpose(slot, octave));
            }
        }
        let width = WORLDGEN_SLOTS * WORLDGEN_OCTAVE_MAX;
        assert_eq!(seen.len(), width as usize, "the map is not injective");
        assert_eq!(*seen.iter().next().expect("non-empty"), WORLDGEN_BASE);
        assert_eq!(
            *seen.iter().next_back().expect("non-empty"),
            WORLDGEN_BASE + width - 1,
            "the window is not contiguous"
        );
        assert!(
            WORLDGEN_BASE.checked_add(width).is_some(),
            "the window runs off the end of a u32"
        );

        let noise_end = NOISE_BASE + 3 * NOISE_OCTAVE_MAX;
        assert!(
            WORLDGEN_BASE >= noise_end || WORLDGEN_BASE + width <= NOISE_BASE,
            "the window overlaps the velocity noise at [{NOISE_BASE}, {noise_end})"
        );
    }

    #[test]
    fn the_lattice_wraps_without_a_seam_in_x_and_y() {
        // Every octave's wavelength divides the domain, so the field is periodic
        // in X and Y for nothing. A seam is invisible in every invariant the
        // project has: transport across it conserves exactly as it does anywhere.
        let grid = grid(N, N, N);
        let octaves = octave_count(&grid);
        let key = 12345;

        for z in [1u32, 5] {
            for y in [0u32, 7] {
                let row: Vec<i64> = (0..N)
                    .map(|x| summed_noise(&grid, x, y, z, substance_slot(0), octaves, key))
                    .collect();
                let inside = (1..N as usize)
                    .map(|x| (row[x] - row[x - 1]).abs())
                    .max()
                    .expect("a non-empty row");
                let seam = (row[0] - row[N as usize - 1]).abs();
                // Against a vacuous pass: on a constant row both sides are zero
                // and the comparison is `0 <= 0`. The sibling test in
                // `kernels/noise.rs` is backed by
                // `the_summed_octaves_stay_inside_the_declared_amplitude`; here
                // the guard has to stand in the same test.
                assert!(inside > 0, "the row is constant at (z = {z}, y = {y})");
                assert!(
                    seam * 2 <= inside * 3,
                    "a seam of {seam} against the largest interior step {inside} \
                     at (z = {z}, y = {y})"
                );
            }
        }

        for z in [1u32, 5] {
            for x in [0u32, 7] {
                let column: Vec<i64> = (0..N)
                    .map(|y| summed_noise(&grid, x, y, z, substance_slot(0), octaves, key))
                    .collect();
                let inside = (1..N as usize)
                    .map(|y| (column[y] - column[y - 1]).abs())
                    .max()
                    .expect("a non-empty column");
                let seam = (column[0] - column[N as usize - 1]).abs();
                assert!(inside > 0, "the column is constant at (z = {z}, x = {x})");
                assert!(seam * 2 <= inside * 3, "a seam of {seam} against {inside}");
            }
        }
    }

    #[test]
    fn the_layer_boundary_varies_horizontally() {
        // The off-by-one `kernels/noise.rs` writes down about itself: a lattice
        // of `2^k` nodes instead of `2^(k+1)` makes octave zero a constant, the
        // longest wave of the relief disappears, and the rest still looks like
        // noise. And the relief taken from a linear index instead of the decoded
        // coordinates gives a stripe along X rather than a surface — the class of
        // mistake `covering_cell` describes in `world/world.rs`.
        let grid = grid(N, N, N);
        let octaves = octave_count(&grid);
        let key = 4242;

        let along_x: Vec<i64> = (0..N)
            .map(|x| relief_height(&grid, x, 3, octaves, key))
            .collect();
        let along_y: Vec<i64> = (0..N)
            .map(|y| relief_height(&grid, 3, y, octaves, key))
            .collect();

        assert!(
            along_x.iter().any(|h| *h != along_x[0]),
            "the layer boundary is constant along X: {along_x:?}"
        );
        assert!(
            along_y.iter().any(|h| *h != along_y[0]),
            "the layer boundary is constant along Y: {along_y:?}"
        );

        // And the boundary is inside the domain, or the layer it bounds is not
        // in the world at all.
        for height in along_x.iter().chain(along_y.iter()) {
            assert!(
                (1..i64::from(N) - 1).contains(height),
                "a layer boundary at {height} on {N} voxels"
            );
        }

        // The vertical profiles of two horizontal points differ, which is what
        // makes it a surface rather than a number.
        let column = |x: u32, y: u32| -> Vec<i128> {
            (0..N)
                .map(|z| {
                    let idx = x + y * N + z * N * N;
                    amount_at(&grid, level(), substance_slot(0), octaves, idx, key)
                })
                .collect()
        };
        assert_ne!(
            column(0, 0),
            column(9, 5),
            "two identical vertical profiles"
        );
    }

    #[test]
    fn the_initial_field_has_structure_larger_than_a_voxel() {
        // The one test that separates the layered noise of SPEC section 12.4 from
        // `rand(voxel_idx, ...)`. White noise passes both seed tests, passes the
        // ceiling, and looks like a filled world — and leaves stratification, the
        // oxycline and the oxidation front with no initial condition at all. The
        // failure would surface waves later and in another file.
        //
        // Over **both** branches of `amount_at`, because ADR-077 refused the
        // `spectrum` key by measuring this very statistic on both of them (0.0566
        // layered and 0.0714 uniform against the threshold of 0.125). A test left
        // on one branch would leave half the claim unchecked exactly where the
        // record measured it. What guarantees structure larger than a voxel is
        // the ladder of octaves — the lattice spacing is never under two voxels
        // and a count of zero is a load error — and not the weights, so the
        // threshold has to stay coarse: sharpened, it would become a blessing for
        // an exponent nobody decided.
        let grid = grid(N, N, N);
        let octaves = octave_count(&grid);
        let key = 99;

        for level in [level(), flat_level()] {
            let at = |x: u32, y: u32, z: u32| -> i128 {
                let idx = x + y * N + z * N * N;
                amount_at(&grid, level, substance_slot(0), octaves, idx, key)
            };

            let mut lowest = i128::MAX;
            let mut highest = i128::MIN;
            let mut steps: i128 = 0;
            let mut pairs: i128 = 0;
            for z in 0..N {
                for y in 0..N {
                    for x in 0..N {
                        let here = at(x, y, z);
                        lowest = lowest.min(here);
                        highest = highest.max(here);
                        if x + 1 < N {
                            steps += (at(x + 1, y, z) - here).abs();
                            pairs += 1;
                        }
                        if y + 1 < N {
                            steps += (at(x, y + 1, z) - here).abs();
                            pairs += 1;
                        }
                        if z + 1 < N {
                            steps += (at(x, y, z + 1) - here).abs();
                            pairs += 1;
                        }
                    }
                }
            }

            let span = highest - lowest;
            assert!(span > 0, "the field is constant on {:?}", level.layer);
            // A coarse threshold, an eighth of the span. White noise over the same
            // band lands near a third of it.
            assert!(
                steps * 8 <= span * pairs,
                "on {:?} the mean step between neighbours is {} of a span of \
                 {span}, which is white noise rather than a field",
                level.layer,
                steps as f64 / pairs as f64
            );
        }
    }

    #[test]
    fn generation_does_not_depend_on_the_order_of_the_walk() {
        // The property ADR-034 states for a kernel, one level up: the fill runs
        // over the voxels in the order it likes, and the result may not know.
        // Fails on an accumulator carried between voxels and on any read of a
        // voxel already written.
        let grid = grid(N, N, N);
        let octaves = octave_count(&grid);
        let key = 31337;

        let decls = vec![
            SubstanceDecl {
                id: "A".to_string(),
                width: Width::Bits32,
                k: 20,
            },
            SubstanceDecl {
                id: "B".to_string(),
                width: Width::Bits64,
                k: 20,
            },
        ];
        let registry = Registry::new(&decls).expect("the registry");
        let mut world = World::new(
            grid,
            registry,
            &WorldLayout {
                enthalpy_lod: 2,
                velocity_lod: 1,
            },
        )
        .expect("the world");

        let mut fills = vec![
            SubstanceFill {
                id: "A".to_string(),
                floor: 0,
                peak: 0,
                amount_at_typical: 1_000_000,
                amount_at_max: 4_000_000,
                excursion: 500_000,
                uniform: false,
            },
            SubstanceFill {
                id: "B".to_string(),
                floor: 0,
                peak: 0,
                amount_at_typical: 8_000_000_000,
                amount_at_max: 32_000_000_000,
                excursion: 4_000_000_000,
                uniform: false,
            },
        ];
        // One on a side and one without, so that neither branch of `amount_at`
        // is the only one the walk is checked over.
        let layers = [Layer::Water, Layer::Uniform];
        let initial = DerivedInitial {
            dx: 1.0,
            concentration: Vec::new(),
            inocula: Vec::new(),
        };
        fill_world(&mut world, &initial, &mut fills, &layers, octaves, key).expect("the fill");

        let n_voxels = grid.n_voxels();
        for (s, fill) in fills.iter().enumerate() {
            let level = Level {
                typical: fill.amount_at_typical,
                excursion: fill.excursion,
                layer: layers[s],
            };
            let slot = substance_slot(s as u32);
            // Backwards, and compared to the buffer the forward walk produced.
            let mut expected = vec![0i128; n_voxels as usize];
            for idx in (0..n_voxels).rev() {
                expected[idx as usize] = amount_at(&grid, level, slot, octaves, idx, key);
            }
            let written: Vec<i128> = match world.lane_of(s as u32) {
                LaneRef::Narrow(lane) => world
                    .amounts_32()
                    .expect("the narrow field")
                    .lane(lane)
                    .iter()
                    .map(|m| i128::from(m.raw()))
                    .collect(),
                LaneRef::Wide(lane) => world
                    .amounts_64()
                    .expect("the wide field")
                    .lane(lane)
                    .iter()
                    .map(|m| i128::from(m.raw()))
                    .collect(),
            };
            assert_eq!(written, expected, "substance {s}");
        }
    }

    #[test]
    fn declared_initial_state_keeps_species_identity_across_storage_widths() {
        let grid = grid(N, N, N);
        let registry = Registry::new(&[
            SubstanceDecl {
                id: "narrow".to_string(),
                width: Width::Bits32,
                k: 20,
            },
            SubstanceDecl {
                id: "wide".to_string(),
                width: Width::Bits64,
                k: 20,
            },
        ])
        .expect("the registry");
        let mut world = World::new(
            grid,
            registry,
            &WorldLayout {
                enthalpy_lod: 2,
                velocity_lod: 1,
            },
        )
        .expect("the world");
        let mut fills = vec![
            SubstanceFill {
                id: "narrow".to_string(),
                floor: 0,
                peak: 0,
                amount_at_typical: 1_000,
                amount_at_max: 10_000,
                excursion: 500,
                uniform: false,
            },
            SubstanceFill {
                id: "wide".to_string(),
                floor: 0,
                peak: 0,
                amount_at_typical: 8_000_000_000,
                amount_at_max: 32_000_000_000,
                excursion: 4_000_000_000,
                uniform: false,
            },
        ];
        let initial = DerivedInitial {
            dx: 1.0e-4,
            concentration: vec![
                crate::config::DerivedConcentration {
                    substance: 0,
                    amount: 0,
                },
                crate::config::DerivedConcentration {
                    substance: 1,
                    amount: 0,
                },
            ],
            inocula: vec![
                crate::config::DerivedInoculum {
                    substance: 0,
                    center: [0.00015; 3],
                    radius: 0.0001,
                    amount: 123,
                },
                crate::config::DerivedInoculum {
                    substance: 1,
                    center: [0.00015; 3],
                    radius: 0.0001,
                    amount: 8_000_000_123,
                },
            ],
        };

        fill_world(
            &mut world,
            &initial,
            &mut fills,
            &[Layer::Uniform; 2],
            octave_count(&grid),
            7,
        )
        .expect("the fill");

        let narrow = world.amounts_32().expect("the narrow field").lane(0);
        let wide = world.amounts_64().expect("the wide field").lane(0);
        let occupied = narrow.iter().filter(|amount| amount.raw() != 0).count();
        assert_eq!(occupied, 7, "the closed radius-dx ball");
        for (narrow, wide) in narrow.iter().zip(wide) {
            if narrow.raw() == 0 {
                assert_eq!(wide.raw(), 0);
            } else {
                assert_eq!(narrow.raw(), 123);
                assert_eq!(wide.raw(), 8_000_000_123);
            }
        }
        assert_eq!((fills[0].floor, fills[0].peak), (0, 123));
        assert_eq!((fills[1].floor, fills[1].peak), (0, 8_000_000_123));
    }

    #[test]
    fn a_grid_that_carries_no_octave_is_refused() {
        // `spacing = n >> (k+1)` with the wrap `& (nodes - 1)` needs the spacing
        // to divide the extent. An extent that does not is not a panic but a
        // skewed lattice and a seam, under a fill that looks entirely healthy.
        assert_eq!(octave_count(&grid(N, N, N)), 3);
        assert_eq!(octave_count(&grid(4, 4, 4)), 1);
        assert_eq!(
            octave_count(&grid(6, 8, 8)),
            1,
            "six carries one octave and no more"
        );
        assert_eq!(octave_count(&grid(2, 8, 8)), 0, "two carries none at all");

        // And a count of zero is a **refusal** and not a number. This half is the
        // one the name promises: `carried_octaves` is what `generate` calls, and a
        // test that asserted only the count above would stay green while a
        // two-voxel axis was filled with a field that has no horizontal structure
        // at all — every column of the domain identical, both invariants closing
        // exactly, nothing the scenario is about ever happening.
        assert!(carried_octaves(&grid(N, N, N)).is_ok());
        let refused = carried_octaves(&grid(2, 8, 8))
            .expect_err("a grid carrying no octave must be refused")
            .to_string();
        assert!(
            refused.contains('2') && refused.contains('8'),
            "the message must name the extents it measured: {refused}"
        );

        // What a zero count would have produced, so that the refusal is read
        // against the thing it prevents rather than against a claim. Not a
        // constant — the sediment and the water column are still two slabs — but
        // flat: one column, repeated over the whole domain.
        let flat = grid(2, 8, 8);
        let slot = substance_slot(0);
        let column = |x: u32, y: u32| -> Vec<i128> {
            (0..flat.nz())
                .map(|z| amount_at(&flat, level(), slot, 0, flat.index(x, y, z), 7))
                .collect()
        };
        assert_eq!(
            column(0, 0),
            column(1, 5),
            "a zero octave count is not flat"
        );
    }

    #[test]
    fn the_band_is_bounded_by_both_headrooms() {
        // The rule `excursion_of` is, stated where it can fail rather than only
        // where it is used. Both bounds have a name in the corpus: the ceiling is
        // ADR-041's and the floor is the difference between "declared absent" and
        // "wandered below zero and was clamped".
        //
        // Water is the case that separates the two: its typical amount is 99% of
        // its maximum, so half of the typical would put the peak far past the
        // ceiling, and half of the headroom does not.
        assert_eq!(excursion_of(100, 200), 50, "the two headrooms are equal");
        assert_eq!(excursion_of(100, 1000), 50, "the floor binds");
        assert_eq!(excursion_of(1000, 1100), 50, "the ceiling binds");
        assert_eq!(excursion_of(100, 100), 0, "no headroom at all");
        assert_eq!(excursion_of(0, 100), 0, "nothing to fill");

        // Both bounds in the form ADR-077 ratified them, as exact integer
        // identities rather than as margins chosen here. From the single
        // inequality `2*excursion <= min(typical, max - typical)` follow
        // `2*(typical - excursion) >= typical` and
        // `2*(typical + excursion) <= typical + max`, and the quarter that used
        // to stand on the second line was decided by nobody. The pair `(0, 100)`
        // is the degenerate case: no band at all, and both identities still hold.
        for (typical, max) in [
            (100i128, 200i128),
            (100, 1000),
            (1000, 1100),
            (7, 9),
            (0, 100),
        ] {
            let excursion = excursion_of(typical, max);
            assert!(
                2 * (typical - excursion) >= typical,
                "the floor of ({typical}, {max}) is under half of the typical"
            );
            assert!(
                2 * (typical + excursion) <= typical + max,
                "the peak of ({typical}, {max}) is over the midpoint between the \
                 typical and the ceiling"
            );
        }
    }
}
