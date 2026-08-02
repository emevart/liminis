//! The temperature step: the scenario folded into two tables, and one call of
//! the temperature kernel per coarse cell per tick.
//!
//! Everything `kernels/temperature.rs` is not allowed to know. The kernel gets
//! two read-only arrays and a struct of scalars; this file is where `c_p` in
//! J/(mol K) and the derived storage exponents become one coefficient per
//! substance, where `k_E` becomes joules per storage unit, and where the loop
//! over the coarse cells lives (ADR-034, ADR-044, ADR-062).
//!
//! # This is a step of the tick and not a process of the roster
//!
//! There is no [`super::ProcessId`] entry here, no `enabled` and no
//! `every_n_ticks`, and none of the three is an omission. ADR-065 closed the
//! roster at nine and counted them out; a tenth record is materialised into every
//! scenario and moves `config_hash` for every config in existence, so adding one
//! overturns that record rather than tidying it. The precedent inside the tick is
//! step `i'`, the energy fold, which ADR-065 keeps out of the roster on purpose.
//!
//! The other two follow from the physics rather than from the record. A run with
//! the chemistry on and the temperature off is not a cheaper world but an
//! undefined one, because `T` is an input of the Q10 factor of every reaction
//! (ADR-048). And "every other tick" is not available either: ADR-044 says the
//! denominator is recomputed rather than cached, and ADR-062 prices that at
//! exactly one recomputation per tick — "the composition changes once per tick,
//! so recomputing once per tick *is* recomputing".
//!
//! # Three descriptions of one registry meet here
//!
//! `c_p` comes from the [`Config`], the storage exponent `k` from the
//! [`Derived`], and the lane from the [`Registry`]. Those are three objects that
//! agree only because nothing has yet had a reason to reorder any of them, and a
//! mismatch is silent in the way ADR-056 describes: a coefficient of a plausible
//! size lands on the wrong substance, the denominator stays entirely believable,
//! and a temperature enters no invariant at all. So [`Temperature::new`] checks
//! that all three name the same substances in the same order before it folds
//! anything.
//!
//! # What this file does not fold
//!
//! `Derived::energy().c_cell`. It is a heat capacity, it is on the right grid,
//! and it is exactly the wrong number: it is summed at load out of `typical_conc`
//! and exists to derive `k_E`. ADR-044 requires the denominator to follow the
//! composition of the actual cell, and the failure mode of reusing the folded one
//! is a temperature field that does not depend on what the voxels hold —
//! plausible everywhere, and visible to nothing.

use anyhow::{Result, bail};

use super::{Conservation, Invariant, coarse_shape_agrees};
use crate::config::{Config, Derived};
use crate::kernels::temperature::{Heat, TemperatureParams, temperature_cell};
use crate::numeric::{M32, M64, Q};
use crate::world::{Grid, Registry};

/// One temperature step: the two tables the kernel reads, plus its scalars.
///
/// Built once per run and applied once per tick. Everything in it is a fold of
/// the scenario and of the derivation; nothing in it is state.
#[derive(Clone, Debug)]
pub struct Temperature {
    /// `lane_of[s]`, straight from the registry — the one door from a substance
    /// index to a buffer address (ADR-056).
    lane: Vec<u32>,
    /// `c_p[s] * 2^-k[s]`, J/(K * storage unit), indexed by **substance**.
    capacity_per_unit: Vec<Q>,
    /// Every scalar the kernel takes.
    params: TemperatureParams,
}

impl Temperature {
    /// Fold a loaded scenario into the tables of ADR-044.
    ///
    /// `enthalpy_lod` is the coarsening of the grid the enthalpy field lives on,
    /// and `enthalpy_grid` is that grid. Both arrive rather than being taken off a
    /// `World`, on the precedent of `React::new`: the two coarse grids of a world
    /// differ, and a caller that could pass either has to say which.
    ///
    /// # Errors
    ///
    /// Returns an error if the config, the derivation and the registry describe
    /// different registries, if the coarse grid is not the fine one at the
    /// declared `lod`, if `T_ref` is not a usable number, if the energy scale
    /// gives a non-finite or zero value to one storage unit, or if a declared
    /// non-zero `c_p` folds to an exact zero in `Q`.
    pub fn new(
        grid: &Grid,
        enthalpy_grid: &Grid,
        enthalpy_lod: u32,
        registry: &Registry,
        derived: &Derived,
        config: &Config,
    ) -> Result<Self> {
        coarse_shape_agrees(
            grid,
            enthalpy_grid,
            enthalpy_lod,
            "the temperature and its denominator",
        )?;
        check_registries(registry, derived, config)?;

        if !config.t_ref.is_finite() {
            bail!(
                "the scenario declares T_ref = {} K, which is not a usable zero \
                 of enthalpy storage (ADR-044)",
                config.t_ref
            );
        }

        // `2^-k_E`: what one storage unit of enthalpy is worth in joules
        // (ADR-062). Folded here and nowhere else, because the reciprocal is the
        // mistake nothing else can catch — the two are indistinguishable to the
        // compiler, and at the derived `k_E = 67` the wrong one puts every
        // temperature forty binary orders out.
        let k_e = i32::from(derived.energy().k_e);
        let joules_per_unit = 2f64.powi(-k_e);
        let folded_joules = Q::from_f64(joules_per_unit);
        if !joules_per_unit.is_finite() || folded_joules <= Q::ZERO {
            bail!(
                "the derived energy scale k_E = {k_e} makes one storage unit of \
                 enthalpy worth {joules_per_unit} J, which is not a positive \
                 number in `Q`. Every temperature of the run would come out at \
                 T_ref exactly, and nothing counts a temperature (ADR-062)"
            );
        }

        Ok(Self {
            lane: registry.lane_of().to_vec(),
            capacity_per_unit: capacity_per_unit(derived, config)?,
            params: TemperatureParams {
                nx: grid.nx(),
                ny: grid.ny(),
                nz: grid.nz(),
                lod: enthalpy_lod,
                n_voxels: grid.n_voxels(),
                lane_len: grid.lane_len(),
                n_substances: registry.n_substances(),
                width_mask: registry.width_mask(),
                t_ref: Q::from_f64(config.t_ref),
                joules_per_unit: folded_joules,
            },
        })
    }

    /// The folded scalars, as the kernel receives them.
    #[inline]
    #[must_use]
    pub fn params(&self) -> TemperatureParams {
        self.params
    }

    /// Deriving a temperature conserves matter and conserves energy, and it
    /// follows from the signature rather than from physics.
    ///
    /// [`Temperature::apply`] takes every buffer of class `M` as a shared slice
    /// and writes two of class `Q`: it cannot change a quantity either ledger
    /// counts. That is the same argument `process::Light` makes about its own
    /// field, and it is the reason the second arm of [`Conservation`] — the one
    /// that names a channel — is not needed here either.
    #[inline]
    #[must_use]
    pub fn invariant(&self) -> Invariant {
        Invariant {
            matter: Conservation::Conserved,
            energy: Conservation::Conserved,
        }
    }

    /// One application: both coarse fields, rewritten in full.
    ///
    /// Dispatched over the **coarse** cells of the enthalpy grid, `0..n_cells`,
    /// and never over the fine voxels — sixty-four of those share one `T`
    /// (ADR-062). Neither output is double buffered and neither is cleared first:
    /// every cell is written before anything reads it, which is the argument
    /// ADR-045 makes for `energy_delta`.
    ///
    /// `enthalpy` is state `N` — `World::enthalpy().read()`, never `write_mut()`.
    /// `World::temperature_slices_mut` is the accessor that gets this right by
    /// construction; a caller assembling the five slices by hand can get it wrong,
    /// and the wrong version produces an entirely plausible temperature that no
    /// invariant looks at.
    ///
    /// # Panics
    ///
    /// If a buffer does not have the shape this operator was folded for.
    pub fn apply(
        &self,
        amounts_32: &[M32],
        amounts_64: &[M64],
        enthalpy: &[M64],
        heat_capacity: &mut [Q],
        temperature: &mut [Q],
    ) {
        let n_cells = self.n_cells();
        assert_eq!(
            enthalpy.len(),
            n_cells as usize,
            "the enthalpy field holds {} cells against the {n_cells} this \
             operator was folded for",
            enthalpy.len()
        );
        assert_eq!(
            heat_capacity.len(),
            n_cells as usize,
            "the heat capacity field does not have the shape this operator was \
             folded for"
        );
        assert_eq!(
            temperature.len(),
            n_cells as usize,
            "the temperature field does not have the shape this operator was \
             folded for"
        );

        let heat = Heat {
            lane: &self.lane,
            capacity_per_unit: &self.capacity_per_unit,
        };
        for coarse in 0..n_cells {
            temperature_cell(
                amounts_32,
                amounts_64,
                enthalpy,
                heat_capacity,
                temperature,
                &heat,
                &self.params,
                coarse,
            );
        }
    }

    /// How many invocations one application costs: the cells of the coarse grid.
    ///
    /// Public so that the dispatch domain is checkable from a test rather than
    /// only from prose, on the precedent of `Light::columns`. The extents are
    /// shifted per axis and never taken off the count of fine voxels: on a grid
    /// where an axis does not divide, `n_voxels >> (3*lod)` is larger than the
    /// number of covering cubes and the edge of the domain is covered by nothing.
    #[inline]
    #[must_use]
    pub fn n_cells(&self) -> u32 {
        let p = &self.params;
        (p.nx >> p.lod) * (p.ny >> p.lod) * (p.nz >> p.lod)
    }
}

/// `c_p[s] * 2^-k[s]`, J/(K * storage unit), one entry per substance.
///
/// **Indexed by substance**, like `Rx::conc_per_unit` and for the same reason
/// (ADR-056): laid out by lane it shifts a contribution by `2^(k_i - k_j)`, tens
/// of binary orders, with every balance closing exactly.
///
/// # The refusal here is deliberately not the one next door
///
/// `React`'s `concentration_per_unit` refuses on `folded <= Q::ZERO`, and copying
/// that line would be a disaster in the quiet direction: it would reject aqueous
/// sulfate, whose partial molar heat capacity is about `-293 J/(mol K)` and is
/// physics, and the proton, which declares `c_p = 0` by the same single-ion
/// convention that gives it a zero partial molar volume — that is, it would
/// reject the only scenario in this repository with chemistry in it.
///
/// Writing no check at all is the other failure and it is silent as well: a `c_p`
/// that is genuinely non-zero and folds to an exact zero in `Q` drops the
/// substance out of the denominator without a word. So the refusal is exactly
/// "declared non-zero, folded to zero", and it names the substance and its `k` the
/// way `concentration_per_unit` does.
fn capacity_per_unit(derived: &Derived, config: &Config) -> Result<Vec<Q>> {
    let mut out = Vec::with_capacity(derived.substances().len());
    for (s, substance) in derived.substances().iter().enumerate() {
        let c_p = config.substance[s].c_p;
        let k = i32::from(substance.k);
        let per_unit = c_p * 2f64.powi(-k);
        if !c_p.is_finite() || !per_unit.is_finite() {
            bail!(
                "substance `{}` declares c_p = {c_p} J/(mol K), which at k = {k} \
                 gives one storage unit a heat capacity of {per_unit} J/K",
                substance.id
            );
        }
        let folded = Q::from_f64(per_unit);
        // Sign preserved, zero allowed, and only an underflow refused. See the
        // doc comment: `folded <= Q::ZERO` would reject both the sulfate and the
        // proton of the corpus registry.
        if c_p != 0.0 && folded == Q::ZERO {
            bail!(
                "substance `{}` declares c_p = {c_p} J/(mol K) and at k = {k} one \
                 storage unit of it is worth {per_unit} J/K, which underflows to \
                 zero in `Q`. The substance would drop out of the denominator of \
                 the temperature silently, and a denominator missing a term is a \
                 plausible number that enters no invariant (ADR-044, ADR-062)",
                substance.id
            );
        }
        out.push(folded);
    }
    Ok(out)
}

/// The config, the derivation and the registry describe the same substances, in
/// the same order.
///
/// Three descriptions and three sources of one table, so all three are checked
/// rather than two. `c_p` is read off the config by position, `k` off the
/// derivation by position, and the lane off the registry by index; any pair
/// coming apart puts a coefficient of a plausible size on the wrong substance
/// with nothing lost, which is the failure ADR-056 spends a page on.
fn check_registries(registry: &Registry, derived: &Derived, config: &Config) -> Result<()> {
    let n = derived.substances().len();
    if registry.n_substances() as usize != n || config.substance.len() != n {
        bail!(
            "the derivation carries {n} substances, the registry {} and the \
             scenario {}: the three came from different configs, and `c_p`, `k` \
             and the lane of one substance would be combined for another \
             (ADR-056)",
            registry.n_substances(),
            config.substance.len()
        );
    }
    for (s, substance) in derived.substances().iter().enumerate() {
        let index = s as u32;
        if registry.id_of(index) != substance.id || config.substance[s].id != substance.id {
            bail!(
                "substance {s} is `{}` in the registry, `{}` in the scenario and \
                 `{}` in the derivation: the heat capacity of one substance would \
                 be applied to the amounts of another",
                registry.id_of(index),
                config.substance[s].id,
                substance.id
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{parse, validate};
    use crate::process::ProcessId;
    use crate::world::{Boundary, LaneRef, World, WorldLayout};

    // --- the fixture ------------------------------------------------------
    //
    // Assembled from a TOML text in the test rather than read out of
    // `configs/`, for the reason `process/react.rs` gives about its own: a
    // scenario file is watched by the CI guard of ADR-020, and a fixture written
    // for a test would drag `WORLD_FORMAT_VERSION` along for something that is
    // not a change of semantics.
    //
    // The numbers are the ones `configs/scenarios/h2s-oxidation.toml` carries,
    // because the two properties this file is about are exactly the ones that
    // scenario documents at length: **`SO4` declares a negative `c_p`** — about
    // `-293 J/(mol K)`, which is physics and not a typo — and **`H_ION` declares
    // `c_p = 0`** by the same single-ion convention that gives it a zero partial
    // molar volume. A fixture with three cheerful positive heat capacities would
    // load under a `folded <= Q::ZERO` check copied from `process/react.rs`, and
    // that check would then reject the only scenario in the repository with
    // chemistry in it.

    const ENTHALPY_LOD: u32 = 2;
    const NX: u32 = 8;
    const NY: u32 = 12;
    const NZ: u32 = 16;

    /// Substance indices of the fixture, in declaration order.
    const WATER: u32 = 0;
    const SO4: u32 = 1;
    const H_ION: u32 = 2;
    const N_SUBSTANCES: u32 = 3;

    fn scenario(extra: &str) -> String {
        format!(
            r#"
name = "process-temperature-fixture"
dt = 1.0
beta = 0.015625
T_ref = 298.15

[conserved]
S = 32.06500

[grid]
nx = {NX}
ny = {NY}
nz = {NZ}
dx = 1.0e-4

[boundary]
x_min = "periodic"
x_max = "periodic"
y_min = "periodic"
y_max = "periodic"
z_min = "closed"
z_max = "closed"

[[substance]]
id = "WATER"
composition = {{}}
molar_mass = 18.01528
diffusivity = 2.3e-9
typical_conc = 55500.0
max_conc = 55600.0
partial_molar_volume = 1.807e-5
settling_radius = 0.0
c_p = 75.3
enthalpy_formation = -285830.0

[[substance]]
id = "SO4"
composition = {{ S = 1 }}
molar_mass = 96.06260
diffusivity = 1.0e-9
typical_conc = 28.0
max_conc = 40.0
partial_molar_volume = 1.4e-5
settling_radius = 0.0
c_p = -293.0
enthalpy_formation = -909270.0

[[substance]]
id = "H_ION"
composition = {{}}
molar_mass = 1.007940
diffusivity = 9.3e-9
typical_conc = 1.0e-4
max_conc = 5.0e-4
partial_molar_volume = 0.0
settling_radius = 0.0
c_p = 0.0
enthalpy_formation = 0.0
{extra}
[[field]]
id = "enthalpy"
lod = {ENTHALPY_LOD}
thermal_diffusivity = 1.4e-7
t_min = 273.15
t_max = 323.15
"#
        )
    }

    /// Everything a test needs, in one value.
    #[derive(Debug)]
    struct Fixture {
        world: World,
        operator: Temperature,
    }

    fn build(text: &str) -> Result<Fixture> {
        let config = parse(text).unwrap();
        let derived = validate(&config)?;
        let registry = Registry::new(&derived.decls()).unwrap();
        let grid = Grid::new(
            NX,
            NY,
            NZ,
            [
                Boundary::Periodic,
                Boundary::Periodic,
                Boundary::Periodic,
                Boundary::Periodic,
                Boundary::Closed,
                Boundary::Closed,
            ],
        )
        .unwrap();
        let world = World::new(
            grid,
            registry,
            &WorldLayout {
                enthalpy_lod: ENTHALPY_LOD,
                // A binary order finer than the enthalpy grid, as ADR-069 has it
                // in the eco regime. Different on purpose: it is the grid the
                // temperature must *not* be folded against.
                velocity_lod: 1,
            },
        )
        .unwrap();

        let operator = Temperature::new(
            world.grid(),
            world.enthalpy_grid(),
            ENTHALPY_LOD,
            world.registry(),
            &derived,
            &config,
        )?;
        Ok(Fixture { world, operator })
    }

    fn fixture() -> Fixture {
        build(&scenario("")).unwrap()
    }

    /// A fourth substance whose declared `c_p` is ordinary and whose *folded*
    /// coefficient is not.
    ///
    /// A concentration this small drives the derived `k` up towards two hundred —
    /// `k` is the exponent that makes `max_conc` fill the ceiling of ADR-039 — and
    /// `75.3 * 2^-199` is far below anything `Q` can represent. Nothing else about
    /// the substance is unusual, which is the point: the refusal has to be about
    /// the *fold* and not about a number that looks wrong in the file.
    const UNDERFLOWING: &str = r#"
[[substance]]
id = "TRACE"
composition = {}
molar_mass = 18.01528
diffusivity = 1.0e-12
typical_conc = 1.0e-40
max_conc = 2.0e-40
partial_molar_volume = 0.0
settling_radius = 0.0
c_p = 75.3
enthalpy_formation = 0.0
"#;

    /// Put an amount of each named substance into state `N` of every voxel,
    /// through `World::lane_of` — the one door from a substance index to a buffer
    /// address (ADR-056).
    ///
    /// Every lane is staged and then **one** swap per field promotes the lot. One
    /// swap per substance would undo the previous one, which is the same parity
    /// trap `ADR-057` and `Field::restore_lane` are about — and it is silent,
    /// because a field of zeros is a legal composition right up to the moment the
    /// denominator comes out negative.
    fn fill(world: &mut World, amounts: &[(u32, i64)]) {
        let n_voxels = world.grid().n_voxels();
        let lane_len = world.grid().lane_len();
        for &(s, value) in amounts {
            match world.lane_of(s) {
                LaneRef::Narrow(lane) => {
                    let field = world.amounts_32_mut().expect("a narrow class");
                    let at = (lane * lane_len) as usize;
                    field.write_mut()[at..at + n_voxels as usize]
                        .fill(M32::new(i32::try_from(value).unwrap()));
                }
                LaneRef::Wide(lane) => {
                    let field = world.amounts_64_mut().expect("a wide class");
                    let at = (lane * lane_len) as usize;
                    field.write_mut()[at..at + n_voxels as usize].fill(M64::new(value));
                }
            }
        }
        if let Some(field) = world.amounts_32_mut() {
            field.swap();
        }
        if let Some(field) = world.amounts_64_mut() {
            field.swap();
        }
    }

    /// One application through the combined accessor, which is how a host runs
    /// it.
    fn apply(f: &mut Fixture) {
        let (src32, src64, enthalpy, capacity, temperature) = f.world.temperature_slices_mut();
        f.operator
            .apply(src32, src64, enthalpy, capacity, temperature);
    }

    #[test]
    fn a_heat_capacity_that_underflows_to_zero_is_refused() {
        // And, more importantly, the two mirrors of it load. `SO4` declares
        // `c_p = -293` and `H_ION` declares `c_p = 0`; a `folded <= Q::ZERO`
        // check copied from `concentration_per_unit` in `process/react.rs` would
        // refuse both and take the only scenario in the repository with chemistry
        // in it with them.
        let f = fixture();
        let table = &f.operator.capacity_per_unit;
        assert_eq!(table.len(), N_SUBSTANCES as usize);
        assert!(
            table[WATER as usize] > Q::ZERO,
            "the solvent has to carry the denominator"
        );
        assert!(
            table[SO4 as usize] < Q::ZERO,
            "a negative partial molar heat capacity is physics, not a typo"
        );
        assert_eq!(
            table[H_ION as usize],
            Q::ZERO,
            "the proton declares c_p = 0 by convention and has to load"
        );

        // The underflow itself, on a fourth substance whose declared `c_p` is
        // ordinary and whose folded coefficient is not. Without the refusal the
        // substance drops out of the denominator in silence, and a denominator
        // missing a term is a plausible number nothing counts.
        let err = build(&scenario(UNDERFLOWING))
            .expect_err("a c_p that folds to an exact zero has to be refused")
            .to_string();
        assert!(
            err.contains("TRACE"),
            "the substance has to be named: {err}"
        );
        assert!(
            err.contains("75.3"),
            "the declared c_p has to be named: {err}"
        );
    }

    #[test]
    fn the_enthalpy_is_read_from_the_front_buffer() {
        // `enthalpy_mut().write_mut()` in place of `enthalpy().read()` gives a
        // temperature out of state `N+1`, or out of whatever the last swap left
        // behind. Neither the compiler nor a grep can tell the two lines apart,
        // and `T` is class `Q` and enters no invariant, so nothing else can
        // either.
        let mut f = fixture();
        fill(&mut f.world, &[(WATER, 1 << 20)]);

        // Two different numbers in the two buffers, so that the answer says which
        // one was read.
        let cells = f.world.enthalpy_grid().n_voxels() as usize;
        f.world.enthalpy_mut().write_mut().fill(M64::new(1 << 40));
        f.world.enthalpy_mut().swap();
        f.world
            .enthalpy_mut()
            .write_mut()
            .fill(M64::new(-(1 << 40)));

        apply(&mut f);
        let front = f.world.temperature().to_vec();

        // The front now holds what the back held, and the temperature has to
        // follow it.
        f.world.enthalpy_mut().swap();
        apply(&mut f);
        assert_eq!(front.len(), cells);
        for (cell, was) in front.iter().enumerate() {
            assert_ne!(
                *was,
                f.world.temperature()[cell],
                "coarse cell {cell} did not follow the front buffer"
            );
        }
        // And the sign of the excess over `T_ref` flipped with it, which the two
        // enthalpies were chosen to make visible.
        let t_ref = f.operator.params().t_ref;
        assert!(front[0] > t_ref);
        assert!(f.world.temperature()[0] < t_ref);
    }

    #[test]
    fn the_temperature_operator_is_not_a_roster_process() {
        // ADR-065 closed the roster at nine and counted them out. A tenth record
        // is materialised into every scenario and moves `config_hash` for every
        // config that exists — that is an overturning of the record dressed as a
        // convenience, and the precedent for keeping an operator out is step `i'`,
        // which ADR-065 excludes on purpose.
        assert_eq!(ProcessId::ALL.len(), super::super::ROSTER_LEN);
        assert_eq!(super::super::ROSTER_LEN, 9);
        for id in ProcessId::ALL {
            assert!(
                !id.id().contains("temperature"),
                "`{}` looks like a temperature process",
                id.id()
            );
        }
        assert!(ProcessId::from_id("temperature").is_none());
    }

    #[test]
    fn the_coarse_grid_is_the_enthalpy_one_and_never_the_velocity_one() {
        // A world has two coarse grids and they differ. Folded against the
        // velocity one, every cell of the sum gathers a plausible neighbouring
        // cube and the temperature is believable everywhere — and in no invariant,
        // because it is class `Q`. The refusal lives in `process/mod.rs` in one
        // copy, shared with `process/react.rs`.
        let config = parse(&scenario("")).unwrap();
        let derived = validate(&config).unwrap();
        let registry = Registry::new(&derived.decls()).unwrap();
        let f = fixture();

        let err = Temperature::new(
            f.world.grid(),
            f.world.velocity_grid(),
            ENTHALPY_LOD,
            &registry,
            &derived,
            &config,
        )
        .expect_err("the velocity grid is not the enthalpy grid")
        .to_string();
        assert!(err.contains("enthalpy"), "{err}");

        // And the right one is accepted, so the test is not passing on a typo.
        assert_eq!(
            f.operator.n_cells(),
            f.world.enthalpy_grid().n_voxels(),
            "one invocation per coarse cell of the enthalpy grid"
        );
        assert_ne!(f.operator.n_cells(), f.world.grid().n_voxels());
    }

    #[test]
    fn one_storage_unit_of_enthalpy_is_two_to_the_minus_k_e_joules() {
        // `2^k_E` and `2^-k_E` are indistinguishable to the compiler and to a
        // grep, and at the derived `k_E` the wrong one puts every temperature tens
        // of binary orders out. Loud, and worth pinning anyway: what is quiet is
        // the neighbouring factor of `2^(3*lod)` from a denominator gathered on
        // the wrong grid, and only an exact expectation separates the two.
        let f = fixture();
        let derived = validate(&parse(&scenario("")).unwrap()).unwrap();
        let k_e = i32::from(derived.energy().k_e);
        assert!(k_e > 0, "the fixture has to exercise a real scale");
        assert_eq!(
            f.operator.params().joules_per_unit,
            Q::from_f64(2f64.powi(-k_e))
        );
        assert_eq!(f.operator.params().t_ref, Q::from_f64(298.15));
        assert_eq!(f.operator.params().lod, ENTHALPY_LOD);
        assert_eq!(f.operator.params().n_substances, N_SUBSTANCES);
    }

    #[test]
    fn a_registry_from_another_scenario_is_refused() {
        // Three descriptions of one substance table meet in `new`: `c_p` from the
        // config, `k` from the derivation, the lane from the registry. Any pair
        // coming apart puts a coefficient of a plausible size on the wrong
        // substance, with nothing lost and no balance moving (ADR-056).
        let config = parse(&scenario("")).unwrap();
        let derived = validate(&config).unwrap();
        let f = fixture();

        let other = Registry::new(&derived.decls()[..2]).unwrap();
        let err = Temperature::new(
            f.world.grid(),
            f.world.enthalpy_grid(),
            ENTHALPY_LOD,
            &other,
            &derived,
            &config,
        )
        .expect_err("a registry of the wrong length has to be refused")
        .to_string();
        assert!(err.contains("ADR-056"), "{err}");
    }

    #[test]
    fn the_operator_writes_only_the_two_coarse_fields() {
        // The signature is the whole of the claim: every buffer of class `M`
        // arrives as a shared slice, so neither ledger can move. Asserted over
        // values as well, because the day this takes `&mut` slices the statement
        // in `invariant()` becomes a lie that nothing else checks.
        let mut f = fixture();
        fill(&mut f.world, &[(WATER, 1 << 20), (SO4, 1 << 8)]);
        f.world.enthalpy_mut().write_mut().fill(M64::new(1 << 40));
        f.world.enthalpy_mut().swap();

        let before_32 = f.world.amounts_32().map(|field| field.read().to_vec());
        let before_64 = f.world.amounts_64().map(|field| field.read().to_vec());
        let before_h = f.world.enthalpy().read().to_vec();

        apply(&mut f);

        assert_eq!(
            f.world.amounts_32().map(|field| field.read().to_vec()),
            before_32
        );
        assert_eq!(
            f.world.amounts_64().map(|field| field.read().to_vec()),
            before_64
        );
        assert_eq!(f.world.enthalpy().read(), &before_h[..]);
        assert_eq!(
            f.operator.invariant(),
            Invariant {
                matter: Conservation::Conserved,
                energy: Conservation::Conserved,
            }
        );

        // And both outputs really were written, or the assertions above are about
        // a step that did nothing.
        assert!(f.world.heat_capacity().iter().all(|&c| c > Q::ZERO));
        assert!(
            f.world
                .temperature()
                .iter()
                .all(|&t| t != Q::ZERO && t.debug_f64().is_finite())
        );
    }
}
