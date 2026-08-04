//! What a scenario does not declare: extent exponents, storage scales, storage
//! widths, stoichiometry in storage units, the energy scale, the mass tolerance
//! and the substep count of every field.
//!
//! # `config/validate.rs` runs before this, and the checks here stay anyway
//!
//! The entry point a loader is supposed to call is [`super::validate`], and it
//! runs referential integrity, the domains of definition and the chemistry
//! *before* it calls [`derive`] — so that a typo in a substance name is refused
//! by a message about the typo rather than by a message about scales
//! (`CONFIG_SCHEMA.md` section 10 describes that outcome by name). Several rules
//! below therefore have a twin one stage up, and the duplication is deliberate:
//! [`derive`] is public, it can be called on a config nobody validated, and
//! every refusal here guards the domain its own formulas are defined on. What
//! must not be duplicated is the *tolerance* — `MASS_EPSILON` is `pub(super)`
//! precisely so that the validator reads it rather than writing `1e-6` a second
//! time.
//!
//! All of it is a function of what `CONFIG_SCHEMA.md` section 5 to section 7
//! declares, and none of it is a key (section 9). SPEC section 10 asks that no
//! parameter of a config know about the voxel; the derivation is the mechanism
//! that keeps that true, rather than the discipline with which it is kept.
//!
//! # The order is part of the contract
//!
//! ```text
//! 1. e_r per reaction, from typical_conc of its scarcest participant
//! 2. the overflow ceiling of every substance, from its max_conc
//! 3. k_i = max(ceiling, max e_r over the reactions it takes part in)
//! 4. the storage width, from the amount at max_conc under the raised k_i
//! 5. nu_i = s_i * 2^(k_i - e_r), as an exact integer shift
//! 6. C_v of the typical composition, the two checks that stand on it, and the
//!    k_E window, by trying the two storage widths in turn
//! 7. nu_E and the relative error it induces
//! 8. the mass tolerance
//! 9. the substeps of every field
//! ```
//!
//! Step 3 cannot be folded into step 1 or step 2. `k_i` depends on the maximum
//! of `e_r` over *all* reactions the substance takes part in, so a single pass
//! over substances would make the result depend on the order the reactions were
//! visited in — and a derivation that depends on iteration order is the one
//! construct that could make two runs of the same config differ without any
//! test here noticing (`same_seed_and_config_give_byte_identical_state` would,
//! a whole run later). Nothing on this path is a `HashMap`, for the same reason.
//!
//! # The raise is in the formula, not in a footnote
//!
//! `CONFIG_SCHEMA.md` section 9 writes the scale of a substance as
//!
//! ```text
//! k_i = max( floor(log2( 2^31/8 / (max_conc_i * V_voxel) )), max e_r over the reactions of i )
//! ```
//!
//! and says why the `max` is inside the line rather than in the prose under it:
//! the flat ceiling gives water `k ~= 52` against `e_r = 63`, that is `k_i <
//! e_r` and a fractional `nu`, which a solved system does not contain. The
//! ceiling bounds `k_i` from above so the field cannot overflow at `max_conc`;
//! integrality of `nu_i` bounds it from below. Both halves are solved together
//! (ADR-039), the larger of the two wins, and only then does water come out at
//! the `k = 63` and the `5.1e11` units ADR-040 prints.
//!
//! Two consequences follow and are worth stating, because each looks like a
//! missing check rather than a theorem:
//!
//! - Raising `k_i` is *not* applied to every substance. It happens only where
//!   the ceiling lost. Applied everywhere it would push the abundant substances
//!   past `i32` at `max_conc`, move every amount onto another scale, and leave
//!   conservation exact — a field is compared against itself — while the physics
//!   quietly changed.
//! - After the raise, `nu_i <= beta * typ_conc_i * V_voxel * 2^k_i` and
//!   `nu_i <= i32::MAX` are both theorems rather than conditions on a scenario.
//!   The proof is three lines and it is in [`check_nu_against_the_pool`]; the
//!   checks stay, as guards on this derivation.
//!
//! # The domain is a scenario with matter
//!
//! Every quantity here is a function of the substances, and two of them —
//! `C_cell` and the temperature it stands under — are sums over `c_p` (ADR-062).
//! A scenario declaring no `[[substance]]` therefore has no heat capacity, no
//! temperature and no energy scale, and it is refused rather than derived
//! degenerately. `configs/scenarios/hello.toml` is such a scenario today; why it
//! cannot simply be given a registry is on `super::load`, together with the rest
//! of what wiring this into loading would cost.
//!
//! # The folded condition is not the refusal
//!
//! ADR-042 folds the system into `s_i * max_conc_j / typ_conc_i <= 2^28 * beta`
//! — with the multiplication, which at `beta = 2^-6` is a threshold of `2^22`
//! and not of `2^34`. It is a rule for a scenario author, not a refusal: because
//! of the floor and the ceiling it is only true to within a factor of two, and
//! written as a refusal it would reject the project's own registry, where the
//! pair water-proton misses it by `2^11` (ADR-040, ADR-042,
//! `CONFIG_SCHEMA.md` section 10). What is checked here is `k_i` and `e_r`.

use anyhow::{Context, Result, bail};

use super::{Config, Field as FieldRecord, Layer, Reaction, Substance};
use crate::process::{N_MAX, ProcessId, substeps_and_alpha};
use crate::world::{MAX_SUBSTANCES, R_MAX, SubstanceDecl, Width};

/// Relative tolerance of the mass balance, and of the significance of `nu_E`.
///
/// One constant for both sides on purpose. ADR-043 derives the mass tolerance as
/// `eps * M_turnover` with `eps = 1e-6`; ADR-062 takes the same number into the
/// significance inequality of `nu_E` and says outright that it "is not chosen
/// again here: it is the tolerance of ADR-043, taken so that the energy side is
/// not coarser than the matter side". A second literal destroys exactly that
/// property — somebody raises one of them once so a config loads, and the two
/// sides stop being commensurate without a word.
///
/// `pub(super)` for the same reason it is one constant: `config/validate.rs`
/// needs it for the third check of ADR-033 in the wording of ADR-064 and for the
/// enthalpy agreement of ADR-044, and a second literal there would destroy the
/// property this comment is about.
pub(super) const MASS_EPSILON: f64 = 1.0e-6;

/// Bits of headroom left above the largest amount a field may hold.
///
/// ADR-039 writes the ceiling as `2^31/8`, that is `2^(32-4)`: three binary
/// orders of margin over `i32::MAX` plus the sign bit. For a *substance* the
/// width in this formula is always 32, whatever width the derivation then hands
/// out — feeding the derived width back in would close the loop on itself and
/// give water `k = 84` instead of 63. The generalisation to `2^(w-4)` is legal
/// for the enthalpy field and only there, where ADR-062 states it as the third
/// inequality of the `k_E` system.
const CEILING_HEADROOM_BITS: u32 = 4;

/// The field id whose three extra keys carry the energy scale (ADR-062).
const ENTHALPY_FIELD: &str = "enthalpy";

/// The largest share of the conductive heat flux that diffusing matter is
/// allowed to carry with it (ADR-062).
///
/// The term is discarded by decision and not by oversight: enthalpy is a field
/// of its own and does not follow diffusing matter. What makes the decision
/// checkable is this number — on the registry of SPEC section 2.3 the discarded
/// term is 1.64%, and all of it is the self-diffusion of water, so the threshold
/// is the known worst case with three times of margin. Calibrated the same way
/// `MASS_EPSILON` is calibrated against the proton, and stated here rather than
/// at the comparison so that raising it is a visible act.
const CARRIED_ENTHALPY_LIMIT: f64 = 0.05;

/// The process that moves diffusive fields, taken from the closed roster.
// TODO(process-roster): which process touches which field is still not in the
// corpus. The roster of ADR-065 now fixes the *name*, so the ban of ADR-030 on
// `every_n_ticks > 1` can no longer be walked past by calling the process
// something else — but "which fields does this process read" is a different
// question, and `reads` is not in the config (ADR-034) and not in the roster
// either. When a process declares the fields it touches, this becomes a lookup
// over them instead of a comparison against one id.
const DIFFUSION_PROCESS: &str = ProcessId::Diffusion.id();

/// The process that carries the five light keys, taken from the closed roster
/// (ADR-065) rather than spelled out here.
const LIGHT_PROCESS: &str = ProcessId::Light.id();

/// The process that writes the Courant numbers step `c` reads, from the same
/// roster. Named here for [`check_every_n_ticks`] (ADR-074, ADR-086).
const VELOCITY_PROCESS: &str = ProcessId::VelocityField.id();

/// The default of `enabled` for the light, taken from the process itself.
///
/// `false`, and by decision rather than by omission since ADR-076: light on by
/// default would put an energy input into every scenario in the repository, and
/// step `a` does not dispatch — so `Tick::new` would refuse every one of them.
/// Neither the sink nor the counter width is the reason any more (ADR-084,
/// ADR-083); the whole argument lives at the definition in `process/light.rs`.
use crate::process::light::{ENABLED_BY_DEFAULT as LIGHT_ENABLED_BY_DEFAULT, daily_sample};

/// The default of `enabled` for the velocity field, taken from the process itself
/// (ADR-065, ADR-069).
///
/// `false`, and the arithmetic is in `process/velocity.rs`: `u_conv_max` is
/// required when the process is on, and ADR-065 materialises the full roster
/// before hashing, so a default of `true` would refuse every scenario that never
/// mentions the field.
use crate::process::velocity::VELOCITY_FIELD_ENABLED_BY_DEFAULT;

/// The process whose limiting overflow carries the window of ADR-082, from the
/// same closed roster (ADR-065).
///
/// Named here for [`pressure`] and for [`check_every_n_ticks`], whose ban of
/// ADR-030 reaches it: the rule is stated over the *field*, and pressure moves
/// the same `amount[]` whose substances declare a `diffusivity`.
const PRESSURE_PROCESS: &str = ProcessId::Pressure.id();

/// The default of `enabled` for pressure, taken from the process itself
/// (ADR-065, ADR-082).
///
/// `false`, and assigned by ADR-082 rather than left over: five locks are shut on
/// step `e` and `process/pressure.rs` names them one by one.
use crate::process::pressure::ENABLED_BY_DEFAULT as PRESSURE_ENABLED_BY_DEFAULT;

/// The process the medium of `[physics]` belongs to, from the same closed
/// roster (ADR-065, ADR-085).
const SETTLING_PROCESS: &str = ProcessId::Settling.id();

/// The default of `enabled` for settling, taken from the process itself.
///
/// `false`, and assigned by ADR-085 rather than left over. The record checked
/// the flip and refused it on four counts, of which the mechanical one is the
/// shortest: `SettlePhase::fold` builds a `Settle` for every lane and
/// `settling_velocity` wants a viscosity before it looks at a radius, so
/// settling on by default would make `physics.mu` required of **every** scenario
/// in the repository.
use crate::process::settle::ENABLED_BY_DEFAULT as SETTLING_ENABLED_BY_DEFAULT;

/// The declared run horizon of stage S0, in ticks (SPEC section 13).
///
/// `1e6` and not `1e7`. ADR-004 states the horizon as the *range* `1e6 … 1e7`,
/// and the two ends are used by different records for different quantities:
/// ADR-083 takes the upper end for the width of a channel counter, ADR-082 takes
/// the lower one — the acceptance criterion of S0 — for this ceiling.
/// Substituting `1e7` here loosens the ceiling tenfold, cites the journal just as
/// plausibly, and fails no test but the one that names the number.
const RUN_HORIZON_TICKS: f64 = 1.0e6;

/// Everything derived at load, in one value.
///
/// Opaque: the fields are private and the accessors hand out slices. What a
/// caller can do with this is read it and build the world from it; what it
/// cannot do is assemble one by hand, which would put a second source of truth
/// next to the only place the formulas live.
#[derive(Clone, Debug, PartialEq)]
pub struct Derived {
    v_voxel: f64,
    substances: Vec<DerivedSubstance>,
    reactions: Vec<DerivedReaction>,
    energy: DerivedEnergy,
    fields: Vec<DerivedField>,
    /// `None` when no face is `exchange` — and then no `[boundary.reservoir]`
    /// section exists either, in both directions (`config/validate.rs`).
    reservoir: Option<DerivedReservoir>,
    /// `None` on a scenario whose light process is off, or on which is on and
    /// declares no `i_surface` — the second of which the validator refuses.
    light: Option<DerivedLight>,
    /// `None` on a scenario whose velocity field is off, or on which is on and
    /// declares no `u_conv_max` — the second of which the validator refuses.
    velocity: Option<DerivedVelocity>,
    /// `None` on a scenario whose pressure process is off, or on which is on and
    /// declares no `theta_max` — the second of which the validator refuses.
    pressure: Option<DerivedPressure>,
    /// `None` on a scenario whose settling process is off, or on which is on and
    /// declares no `physics.mu` — the second of which the validator refuses.
    medium: Option<DerivedMedium>,
}

/// The outside reservoir, folded once at load (ADR-059).
///
/// Three numbers, and all three are rounded here and never in a tick: ADR-059
/// says so about the composition in as many words — "the rounding happens once
/// at load, as `nu_E` does in ADR-041, and never at run time".
#[derive(Clone, Debug, PartialEq)]
pub struct DerivedReservoir {
    /// `k_ex*dt/dx` over a whole tick, dimensionless. The stability bound of the
    /// exchanging face is `alpha_ex <= 1`, over `dx` and not over `dx^2`: it is
    /// a Courant-shaped condition and not a diffusive one (ADR-059).
    ///
    /// Over the **fine** `dx`. What the enthalpy field's coarser step does with
    /// the same `k_ex` is not settled — see the note on
    /// [`Derived::reservoir`].
    pub alpha_ex: f64,
    /// The declared exchange velocity, m/s, carried through unchanged.
    ///
    /// Beside the folded number rather than instead of it, and for the reason
    /// [`DerivedSubstance::diffusivity`] gives about its own: the host that folds
    /// a transport phase needs the **input**, because it folds `k_ex*dt/dx` at
    /// the `dx` of whichever grid the field lives on — the fine one for the
    /// amounts, `dx*2^lod` for the enthalpy — and dividing it again by the
    /// substep count. Handing it `alpha_ex` instead would be a second
    /// construction of one coefficient, at a `dx` that is not the one it will
    /// run at.
    pub k_ex: f64,
    /// `round(conc_out_i * units_per_mol_i * V_voxel)` per substance, in that
    /// substance's storage units, in declaration order.
    ///
    /// Per **substance** and not per lane, because that is the order a scenario
    /// declares and the order `World::seed_ghosts` narrows from (ADR-056).
    pub amount_out: Vec<i128>,
    /// The enthalpy of one **coarse** cell of the reservoir at `t_out`, in the
    /// storage units of the energy scale:
    /// `round((t_out - T_ref) * sum_i(conc_out_i * c_p,i) * V_cell * 2^k_E)`.
    ///
    /// Two things here are easy to get wrong and neither is loud.
    ///
    /// `T_ref` is the scenario's, not the thermochemical 298.15 K. ADR-044 keeps
    /// the two reference states apart precisely because one word made both
    /// questions unanswerable, and `h2s-oxidation.toml` sets `T_ref = 298.15`
    /// "out of convenience and not by requirement" — so on the corpus the two
    /// are the same number and only a fixture that separates them can tell.
    ///
    /// `V_cell` is the volume of a cell of the **enthalpy** grid,
    /// `(dx*2^lod)^3`, and not `V_voxel`. At `lod = 2` the miss is a factor of
    /// sixty-four: `t_out` stays inside its declared range, the load passes, the
    /// sign of the flux is right and the energy residual closes — the counter
    /// records whatever moved. Only the magnitude lies.
    pub enthalpy_out: i64,
    /// The thermal conductance of the `exchange` face, W/(m^2*K) (ADR-084):
    /// `k_ex * sum_i(conc_out_i * c_p,i)`.
    ///
    /// Not folded here — obtained by calling `validate::boundary_conductance`,
    /// the same function the refusal judges by. Writing the product a second time
    /// would let the refusal and the report print different numbers, both
    /// plausible, which is the drift that doc names in five other places.
    pub conductance: f64,
    /// The largest absorbed flux the lid can carry away in steady state, W/m^2
    /// (ADR-084): [`DerivedReservoir::conductance`] times `t_max - t_out`. The
    /// threshold `i_surface` is refused above.
    pub flux_ceiling: f64,
    /// An **upper bound** on the enthalpy the `exchange` faces credit to
    /// `BOUNDARY_EXCHANGE` over one tick of a world at `T_ref`, in the storage
    /// units of the energy scale (ADR-084).
    ///
    /// `alpha_ex_coarse * |enthalpy_out| * (coarse cells on the exchanging
    /// faces)`, and every piece of that needs saying.
    ///
    /// **A bound and not the number a run produces**, which is why the report
    /// prints it as one. The gap the face trades across decays inside the tick —
    /// the field takes its own substeps, each one moving a fraction of what is
    /// left — and the neighbours pull the boundary cell back, so a loader cannot
    /// reproduce the run's figure without running it. On the shipped scenario
    /// this bound is `1.4182e18` against the `1.4058e18` ADR-084 measured, which
    /// is the size of the error: under a percent, and on the safe side.
    ///
    /// **From `T_ref` and not from the widest legal gap.** The world starts at
    /// `H = 0`, so this is the first tick's traffic, which is the largest of any
    /// tick a dark scenario has and the one ADR-084 measured. A world whose light
    /// or chemistry drives the field to `t_max` trades more, and the ceiling
    /// above is what bounds that case.
    ///
    /// **`alpha_ex` at the coarse `dx`.** The enthalpy field is the one that
    /// carries energy through the face, and `Tick::new` folds it at
    /// `enthalpy.coarse_dx`; the `alpha_ex` beside this field is at the fine one,
    /// where the same `k_ex` gives a number `2^lod` larger. See the note on
    /// [`Derived::reservoir`] for why the two exist at all.
    pub lid_credit_per_tick: f64,
}

/// One substance after the derivation (ADR-039, ADR-040).
// `Eq` is gone from the derive list as of the `diffusivity` field below: a
// declared coefficient is an `f64`, and the rest of this module is `PartialEq`
// for the same reason.
#[derive(Clone, Debug, PartialEq)]
pub struct DerivedSubstance {
    /// The id as written in TOML.
    pub id: String,
    /// `log2(units_per_mol)`, after the raise.
    pub k: u8,
    /// `log2(units_per_mol)` as the overflow ceiling alone would have it.
    ///
    /// Kept apart from [`DerivedSubstance::k`] for a reason that is not
    /// debugging: `scale_overflow_is_rejected` and
    /// `reaction_with_unrepresentable_concentration_spread_is_rejected` refuse
    /// on one and the same inequality — "does not fit an `i64` either" — and
    /// differ only in where `k_i` came from. There the substance is at fault,
    /// here a pair is (`CONFIG_SCHEMA.md` section 10, "the refusal on
    /// `k_i >= e_r` comes later than it looks"). Without this field the two
    /// messages cannot be told apart.
    pub k_ceiling: u8,
    /// Whether `e_r` won over the ceiling.
    pub raised_to_e_r: bool,
    /// Derived storage width (ADR-040).
    pub width: Width,
    /// Units held by one voxel at `max_conc` and the derived `k`.
    pub amount_at_max: i128,
    /// Units held by one voxel at `typical_conc` and the derived `k`.
    pub amount_at_typical: i128,
    /// The declared diffusion coefficient, m^2/s, carried through unchanged.
    ///
    /// Copied rather than derived, and it earns its place for a reason the other
    /// fields do not have: the substep count of substance transport is derived
    /// **per lane**, in `process/diffuse.rs`, out of exactly this number
    /// (`resolve_field` says so in as many words), and the caller that folds a
    /// `DiffusePhase` holds a `Derived` and a `World` and no `Config`. Reading it
    /// off the scenario there instead would put the loader's output and the
    /// scenario side by side in one function, which is how the two come apart —
    /// the substance order of a `Config` and the substance order of a `Derived`
    /// agree only because nothing has yet had a reason to reorder either.
    pub diffusivity: f64,
    /// The declared settling radius, m, carried through unchanged (ADR-067,
    /// ADR-085).
    ///
    /// Here for the reason [`DerivedSubstance::diffusivity`] gives about its
    /// own, and the three of these arrive together because they are the three
    /// inputs of one `settle::Grain`: the host that folds a `SettlePhase` holds
    /// a `Derived` and a `World` and no `Config`, so without them step `f`
    /// cannot be built at any default at all — which is what
    /// `refuse_if_blocked(Settling)` used to say and what ADR-085 required to be
    /// finished in the same commit.
    pub settling_radius: f64,
    /// The declared molar mass in **grams** per mole, carried through unchanged.
    ///
    /// The unit is not free: the `1e-3` in `settle::grain_density` is the
    /// conversion from grams and admits no other (ADR-067). Read as kg/mol the
    /// grain density is off by a thousand — one way round that is a loud Courant
    /// refusal and the other way round settling simply never happens.
    pub molar_mass: f64,
    /// The declared partial molar volume, m^3/mol, carried through unchanged.
    ///
    /// Zero and negative are both legal in general and neither is legal for a
    /// grain that settles: the density is derived by dividing by it (ADR-067).
    /// The conditional refusal is the validator's.
    pub partial_molar_volume: f64,
    /// `w_s = round(enthalpy_formation * 2^(k_E - k))`: the chemical energy of
    /// **one storage unit** of this substance, in the storage units of the
    /// enthalpy field (ADR-081).
    ///
    /// The weight of this substance on the **left** side of the energy
    /// invariant. `Ledger::residual_energy` holds `H_field + Sum_s w_s * n_s`
    /// against the channels, so a reaction stops being a source of energy and
    /// becomes a transfer between two forms of one quantity — and the identity
    /// `Sum_s nu_s * w_s + nu_E == 0` makes the energy half close by
    /// construction rather than by check.
    ///
    /// A field of the record and not a `[i64; MAX_SUBSTANCES]` beside it, for
    /// the reason [`DerivedSubstance::layer`] gives one screen down: one index
    /// of a substance and not two.
    ///
    /// Filled by step 7 and zero until then, like
    /// [`DerivedReaction::nu_energy`]: it needs `k_E`, and `k_E` needs every
    /// `e_r`.
    pub chemical_weight: i64,
    /// The side of the sediment/water boundary this substance is enriched on
    /// (ADR-077).
    ///
    /// A field of the record and not a `[Layer; MAX_SUBSTANCES]` beside it. The
    /// array is cheaper to write and carries a failure nothing sees: it is a
    /// second index of a substance next to `substances()`, the guard in
    /// `worldgen::generate` — which compares `derived.substances()[s].id`
    /// against `registry.id_of(s)` — does not cover it, and a side landing one
    /// row off puts the oxidation front somewhere else entirely while both
    /// ledgers close. Its tail past `n_substances` is the second half of the
    /// same trap: it answers `sediment` for any index at all instead of
    /// panicking.
    pub layer: Layer,
}

/// One reaction after the derivation (ADR-039, ADR-041, ADR-043).
#[derive(Clone, Debug, PartialEq)]
pub struct DerivedReaction {
    /// The id as written in TOML, and the source of `reaction_id` (ADR-027).
    pub id: String,
    /// Exponent of the extent quantum: one quantum is `2^-e_r` turnovers.
    pub e_r: u8,
    /// Index of the substance that set `e_r` — the scarcest participant.
    ///
    /// Public so a test can assert *whom* the number came from and not only the
    /// number. Taken from the most abundant participant instead, `e_r`
    /// compiles, loads, and gives a world where the chemistry stands still
    /// under a ledger that closes (ADR-039).
    pub scarcest: u32,
    /// Storage-unit stoichiometry, one entry per *distinct* participant.
    pub nu: Vec<Nu>,
    /// `nu_E := -Sum_s nu_s * w_s`, the energy coefficient of the reaction in
    /// the storage units of the enthalpy field, summed at load (ADR-081).
    ///
    /// **Stated in the direction of the field, so an exothermic reaction has
    /// `nu_E > 0`.** `kernels/react.rs` writes it into the per-voxel
    /// accumulator and `kernels/fold.rs` adds that to the enthalpy, so what the
    /// field gains is what the chemical form lost: the sign is decided here,
    /// once, and no kernel flips it. ADR-081 cancels the formula of ADR-041 —
    /// `nu_E = dH * 2^(k_E - e_r)` and its rounding — and leaves the rest of
    /// that record standing; the declared `enthalpy` is now what this sum is
    /// **checked against** rather than what it is computed from.
    pub nu_energy: i64,
    /// How far the summed coefficient stands from the declared enthalpy,
    /// relative: `|nu_E + enthalpy * 2^(k_E - e_r)| / |enthalpy * 2^(k_E-e_r)|`
    /// (ADR-081).
    ///
    /// **Not the rounding error of `nu_E`**, which no longer rounds. What rounds
    /// is `w_s`, and only where `k_s > k_E`; this is the discrepancy that
    /// rounding leaves between the two independent statements of one quantity —
    /// the scenario's declared enthalpy and the sum over the formation
    /// enthalpies of the participants. Refused above `MASS_EPSILON`.
    pub energy_relative_error: f64,
    /// Mass of one turnover, g/mol, over the consumed side of the netted
    /// records.
    pub turnover_mass: f64,
    /// `MASS_EPSILON * turnover_mass`, g/mol (ADR-043).
    pub mass_tolerance: f64,
}

/// One participant of a reaction in storage units.
///
/// One record per *distinct* substance. A substance standing on both sides is
/// netted into a single record whose value carries `s_out - s_in`: the molar
/// stoichiometry in TOML is positive on both sides and the section supplies the
/// sign when the pair is resolved into `nu` (SPEC section 5). Two records
/// instead would pass the element balance and the mass balance — both are taken
/// over molar `s` — and the kernel would apply both, moving twice what the
/// reaction declares, with the ledger closing throughout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Nu {
    /// Substance index, in declaration order.
    pub substance: u32,
    /// `s_net * 2^(k_i - e_r)`. Negative on the consumed side.
    pub value: i64,
}

/// The energy scale (ADR-062).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DerivedEnergy {
    /// `log2(units_per_joule)`, the top of the window.
    pub k_e: u8,
    /// Storage width of the enthalpy field and of the reaction energy delta.
    pub width: Width,
    /// Heat capacity of one coarse cell of the enthalpy field, J/K.
    pub c_cell: f64,
    /// `alpha * C_v`, W/(m*K) — the thermal conductivity the declared pair
    /// implies.
    ///
    /// Derived from nothing new and reported for one reason: ADR-062 calls it
    /// the side cross-check by which "a number declared an order of magnitude
    /// wrong is visible to the eye". Water comes out at 0.585 W/(m*K), and the
    /// refusals above it are inequalities that a plausible-looking
    /// `thermal_diffusivity` can pass.
    pub conductivity: f64,
    /// `Sum D_i c_i c_p,i / (alpha * Sum c_j c_p,j)`, the share of the
    /// conductive heat flux that diffusing matter carries (ADR-062).
    pub carried_by_diffusion: f64,
    /// `C_cell * max(t_max - T_ref, T_ref - t_min)`, J.
    pub h_max: f64,
    /// The lower end of the declared working range of the enthalpy field, K.
    ///
    /// Read off the `[[field]] id = "enthalpy"` record by [`energy_window`],
    /// which needs both ends to derive `H_max` and until ADR-087 dropped them on
    /// the floor. Carried because `process::VelocityConfig` wants them and no
    /// other section of a [`Derived`] holds a temperature at all: the mobility
    /// `L` is calibrated so that `|u_conv| <= u_conv_max` at exactly this span
    /// (ADR-069), and taking them out of the `Config` inside `Tick::new` would
    /// put a second place deciding which `[[field]]` record is the enthalpy.
    pub t_min: f64,
    /// The upper end of the same range, K. See [`DerivedEnergy::t_min`].
    pub t_max: f64,
    /// The window `[lower, upper]` the three inequalities leave.
    pub window: (u32, u32),
    /// Which reaction set the lower bound — the one with the *smallest quantum
    /// of energy*, not the one with the largest enthalpy. The exact twin of
    /// "the quantum is set by the scarcest participant" (ADR-062).
    pub window_lower_set_by: usize,
    /// Which reaction set the `nu_E`-fits-`i32` half of the upper bound.
    ///
    /// Not necessarily the binding one: the field's own ceiling is the other
    /// half and is not a reaction. On the registry of SPEC section 2.3 the
    /// reaction half is 68 (photosynthesis) and the field half is 67, so the
    /// window closes at 67 while this still names photosynthesis — which is how
    /// ADR-062 words it too.
    pub window_upper_set_by: usize,
    /// The largest [`DerivedReaction::energy_relative_error`] over all reactions.
    ///
    /// Same name and same type as before ADR-081, and a **different quantity**:
    /// it used to be the rounding error of `nu_E` (`0.5/nu_E`, `9.2e-9` on the
    /// shipped scenario) and is now the discrepancy of the summed coefficient
    /// against the declared enthalpy. The report line is the only place either
    /// has ever been read, which is why the change is written down here.
    pub worst_relative_error: f64,
    /// Which reaction that discrepancy belongs to.
    pub worst_reaction: usize,
    /// `Sum_s w_s * n_s` over the whole domain at `typical_conc`, in joules
    /// (ADR-081).
    ///
    /// Negative on any ordinary registry, because a formation enthalpy usually
    /// is. Stored here rather than computed by [`Derived::report`], which holds
    /// neither a `Config` nor a grid.
    ///
    /// This is the operational form of a divergence from the frozen spec: SPEC
    /// section 2.1 writes the left side of the invariant as an unweighted sum of
    /// the fields, and after ADR-081 the energy half is weighted. The spec is not
    /// edited (ADR-032); the number is printed instead, and
    /// `load_reports_the_chemical_energy_of_the_domain_in_joules` is what reads
    /// it.
    pub chemical_energy_joules: f64,
    /// The **full** declared span of the enthalpy field over the whole domain:
    /// `2 * H_max * n_cells`, in joules.
    ///
    /// Full, and not `H_max * n_cells`: the field runs from `t_min` to `t_max`
    /// and `H_max` is the larger half. The half-span mistake prints a ratio of
    /// 152 in place of 76 and is invisible without a literal to compare against.
    pub field_span_joules: f64,
    /// How many bits `Sum_s |w_s| * n_s(max_conc)` over the whole domain
    /// occupies, against the 127 of `ledger::DomainSums::energy` (ADR-081).
    ///
    /// Computed through `log2` in `f64` and never by forming the `i128`: the
    /// product this bounds is exactly the one that would wrap while being
    /// judged, and an `i128` multiplication wraps silently in release.
    pub chemical_energy_bits: f64,
    /// What one energy channel counter holds before it overflows, in joules:
    /// `2^127 / 2^k_E` (ADR-075, ADR-083).
    ///
    /// Reported rather than refused, and the reason changed with the width. It
    /// used to be printed because the answer was contested: at `i64` the ceiling
    /// was `2^63/2^k_E = 62.5 mJ` at `k_E = 67`, against `0.16384 J` for one lit
    /// tick of a 128 cubed domain — 2.62 ceilings in a single tick — and that was
    /// the second of the two locks holding the refusal of a lit scenario shut.
    /// ADR-083 settled it: the counter is as wide as the domain sum it closes
    /// against, and at `k_E = 67` this is `2^60 J = 1.15e18 J`, which is `7.0e11`
    /// declared horizons of full sun.
    ///
    /// It goes on being printed because it is a *derived* number that a scenario
    /// can still move: `k_E` comes out of the scenario's own reactions and
    /// enthalpy range (ADR-062), so the ceiling is a fact about this config and
    /// not a constant of the build. Never a literal in code, at either exponent —
    /// that is the rule ADR-062 set and the rule that survived 63 becoming 127.
    pub counter_ceiling_joules: f64,
}

/// The light process after the derivation (ADR-076).
///
/// Present exactly when the process is enabled — by its own record or by the
/// default its module declares (ADR-065) — and `i_surface` is written. The
/// second half is not a rule: a scenario that enables the light and omits the
/// irradiance is refused by `config/validate.rs`, and this module stays derivable
/// on a config nobody validated.
#[derive(Clone, Debug, PartialEq)]
pub struct DerivedLight {
    /// The declared irradiance on the top face of the domain, W/m^2 — and the
    /// **mean over a modulation period**, not the instantaneous value. The
    /// discrete normalisation below is what makes that exact.
    pub i_surface: f64,
    /// `dx^2 * units_per_joule`: what one unit of stored intensity is worth as
    /// storage units per second, folded for `FoldParams::units_per_intensity`.
    ///
    /// The area of a **fine** face, because the fold sums over the fine voxels of
    /// a coarse cell and each term already carries one. The coarse face
    /// `(dx*2^lod)^2` would multiply the entire solar input by `2^(2*lod)` —
    /// sixteen at `lod = 2` — with the shape of `I(z)` unchanged and both halves
    /// of the invariant closing (`kernels/fold.rs`). `dt` is **not** in here: it
    /// is the second argument of `m_delta_64`.
    pub units_per_intensity: f64,
    /// Amplitude of the daily modulation as a fraction of the mean, `[0, 1]`.
    pub daily_fraction: f64,
    /// The daily period in whole ticks, `N = P_d/dt`; zero when there is no
    /// daily modulation.
    pub daily_period_ticks: u32,
    /// `A_N = N / sum_{n<N} max(0, sin(2*pi*n/N))`, the **discrete** normalisation
    /// that gives the multiplier unit mean; one when there is no daily
    /// modulation.
    ///
    /// Discrete and not the continuous `1/pi`, and the difference is percent, not
    /// rounding: at a period of eight ticks the mean would run 5.19% low and at
    /// twenty-four 0.574% low, so the choice of period would silently move the
    /// energy budget of the world (ADR-076). Computed here, once, and never
    /// recomputed by the process.
    pub daily_norm: f64,
    /// Amplitude of the seasonal modulation as a fraction of the mean, `[0, 1]`.
    ///
    /// Needs no normalisation of its own: `sum sin` over a whole number of ticks
    /// is an exact zero.
    pub seasonal_fraction: f64,
    /// The seasonal period in whole ticks; zero when there is no seasonal
    /// modulation.
    pub seasonal_period_ticks: u32,
    /// The peak instantaneous multiplier of the two modulations together,
    /// relative to the mean.
    ///
    /// Taken over the **samples**, `A_N * max_{n<N} max(0, sin(2*pi*n/N))` times
    /// `(1 + f_s)`, and never against `pi`: at `N = 4` — legal under the
    /// three-tick minimum — the samples are `0, 1, 0, 0`, `A_4 = 4` and the peak
    /// is exactly 4.0, which is 27% above `pi`.
    pub peak_multiplier: f64,
    /// How many ticks the declared **mean** irradiance takes to carry one coarse
    /// cell across `H_max`, if it is absorbed whole there:
    /// `H_max / (i_surface * (dx*2^lod)^2 * dt)`.
    ///
    /// Printed apart from [`DerivedLight::peak_multiplier`], and the separation is
    /// the decision: the multiplier has unit mean by construction, so the crossing
    /// takes the same number of ticks whatever the modulation is. Dividing one by
    /// the other gives a ratio of instantaneous rates wearing the units of a time,
    /// and in a report it reads as a conservative estimate.
    // TODO(dark-ceiling): what this prints at `i_surface = 0` — the legal
    // closed-box scenario — is decided by nothing. `f64::INFINITY` is what the
    // arithmetic gives and what is printed; a dash, or dropping the line, would
    // make the report look complete while being less so, which is why the
    // infinity is left visible rather than special-cased here.
    pub ticks_to_ceiling: f64,
}

/// The prescribed velocity field after the derivation (ADR-069, ADR-087).
///
/// Present exactly when the process is enabled — by its own record or by the
/// default its module declares (ADR-065) — and `u_conv_max` is written. The
/// second half is not a rule: a scenario that enables the field and omits the
/// speed is refused by `config/validate.rs`, and this module stays derivable on a
/// config nobody validated.
///
/// **Exactly the four keys of ADR-069 and nothing derived from them.** The
/// mobility `L`, the stencil radius `r`, the octave count and the estimate of
/// `|u|` are derived in `VelocityField::new` out of the three grids, which this
/// module does not have; deriving any of them a second time here would give the
/// world two sources for one number, and the two would part company wherever the
/// world's enthalpy grid and the `[[field]]` record's `lod` disagree — which is
/// the discrepancy ADR-069 already names about `r`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DerivedVelocity {
    /// The convective speed the mobility is calibrated to, m/s. Required when the
    /// process is on and has no default (ADR-069).
    pub u_conv_max: f64,
    /// The structure length `l_c`, m.
    pub l_c: f64,
    /// The stirring amplitude as a fraction of `u_conv_max`, `[0, 1]`. At exactly
    /// zero the noise kernel is not dispatched at all.
    pub stir_fraction: f64,
    /// The stirring period, s. Required when `stir_fraction > 0`.
    pub stir_period: Option<f64>,
}

/// The pressure process after the derivation (ADR-082).
///
/// Present exactly when the process is enabled — by its own record or by the
/// default its module declares (ADR-065) — and `theta_max` is written. The second
/// half is not a rule: a scenario that enables pressure and omits the limiting
/// overflow is refused by `config/validate.rs`, and this module stays derivable on
/// a config nobody validated.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DerivedPressure {
    /// The declared limiting overflow, dimensionless. Required when the process
    /// is on, with no default (ADR-082).
    pub theta_max: f64,
    /// `Theta_sup = sum over V_bar > 0 of max_conc * V_bar`: the peak
    /// **occupancy** `V_occ/V_voxel` the declared ceilings allow.
    ///
    /// The occupancy and never the overflow `theta = Theta - 1`, and the
    /// difference is not a nicety. On the registry the project ships the two are
    /// `1.005285` and `5.285e-3`, so a floor built on the overflow is a hundred
    /// and ninety times too low: every `theta_max` passes it, the scheme
    /// oscillates, matter is conserved exactly, a closed face stays closed, a
    /// uniform occupancy stays a fixed point and both halves of the ledger close.
    /// Only `the_relaxation_oscillates_once_the_occupancy_exceeds_a_sixth_of_theta_max`
    /// can see it.
    ///
    /// A **negative** `V_bar` does not enter. Electrostriction lowers the
    /// occupancy (ADR-067: `PO4^3-` near `-4.0e-5`), so including it would lower
    /// an upper estimate, lower the floor and let an unstable `theta_max` load.
    /// The unfiltered `map(|s| s.max_conc * s.partial_molar_volume).sum()` looks
    /// more correct than the filtered form and compiles the same;
    /// `the_peak_occupancy_ignores_negative_partial_molar_volumes` is the only
    /// thing in the way.
    pub occupancy_sup: f64,
    /// `Theta_typ`, the same expression over `typical_conc`: the working
    /// occupancy the relaxation time is measured at.
    ///
    /// A **separate input** from [`DerivedPressure::occupancy_sup`] and never the
    /// same one twice. On the shipped registry the two differ by 0.2%
    /// (`1.005285` against `1.003288`), so swapping them moves no number in the
    /// report and fails no other test — and inverts the window on a registry
    /// whose dynamic range is wide, which ADR-039 allows up to `2^14`.
    pub occupancy_typ: f64,
    /// `N_max^2*theta_max/(pi^2*Theta_typ)`: how many applications the longest
    /// mode of the closed domain needs to relax by `1/e`.
    ///
    /// `N_max` is the **longest axis of this grid**, not [`N_MAX`] of
    /// `process/diffuse.rs` — a different quantity with the same spelling, the
    /// bound on a substep count (ADR-061), and one this module already imports.
    /// Written with that one the ceiling is the 64-cubed ceiling on every grid.
    pub relaxation_ticks: f64,
    /// How far below the oscillation threshold the declared `theta_max` sits, as
    /// a fraction: `1 - alpha/(1/6)` at `alpha = Theta_typ/theta_max`.
    ///
    /// Printed together with the threshold it is measured against and never
    /// alone: an `alpha` of `8e-4` reads as "small" exactly as well as it reads
    /// as "large" (ADR-082).
    pub stability_margin: f64,
}

/// The medium of `[physics]` after the derivation (ADR-085).
///
/// Present exactly when the settling process is enabled — by its own record or
/// by the default its module declares (ADR-065) — and `mu` is written. The
/// second half is not a rule of this module: a scenario that enables settling
/// and omits the viscosity is refused by `config/validate.rs`, and this module
/// stays derivable on a config nobody validated.
///
/// **Three numbers carried through and nothing derived from them**, on the model
/// of [`DerivedVelocity`]. `k = 2r^2/9`, the grain density and `w` are folded in
/// `process/settle.rs` out of the grid a world actually has; deriving any of
/// them here would give one velocity two sources.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DerivedMedium {
    /// Gravitational acceleration, m/s^2, as declared or defaulted.
    pub g: f64,
    /// Density of the medium, kg/m^3, as declared or defaulted.
    pub rho_medium: f64,
    /// Dynamic viscosity, Pa*s. Required when the process is on, with no
    /// default (ADR-085).
    pub mu: f64,
}

/// One field record after the derivation (ADR-030, ADR-061, ADR-062).
#[derive(Clone, Debug, PartialEq)]
pub struct DerivedField {
    pub id: String,
    /// Coarsening relative to `[grid]`, as declared.
    pub lod: u8,
    /// `ceil(6*D*dt/dx_coarse^2)`, at most [`N_MAX`].
    pub substeps: u32,
    /// `D*dt/(n*dx_coarse^2)`, the number the kernel is handed.
    pub alpha: f64,
    /// The coefficient the two above were derived from, m^2/s. For the enthalpy
    /// record it is `thermal_diffusivity` (ADR-062); for every other record it
    /// is zero, because a field that declares no coefficient does not diffuse.
    ///
    /// Kept beside the results rather than dropped, because the host that folds
    /// the field's transport needs the *inputs*: `process/diffuse.rs` derives
    /// `n` and `alpha` itself out of `D`, `dt` and `dx`, and handing it the
    /// outputs instead would be a second construction of the same pair — the one
    /// thing `substeps_and_alpha` exists to prevent. `Tick::new` checks that the
    /// pair it folds is the pair recorded here.
    pub diffusivity: f64,
    /// `dx * 2^lod`, metres: the edge of a cell of *this* field's grid.
    ///
    /// Derived here and not at the call site for the same reason. `lod` enters
    /// the substep count as `(dx*2^lod)^2`, so a caller that reconstructed the
    /// coarse step with a shift or a `powi` would be a second place where one
    /// step of `lod` divides `n` by four.
    pub coarse_dx: f64,
}

/// Used where a bound is set by no reaction at all, which is legal: a scenario
/// with no chemistry constrains `k_E` from below by nothing.
pub const NO_REACTION: usize = usize::MAX;

impl Derived {
    /// `dx^3`, cubic metres.
    #[inline]
    #[must_use]
    pub fn v_voxel(&self) -> f64 {
        self.v_voxel
    }

    #[inline]
    #[must_use]
    pub fn substances(&self) -> &[DerivedSubstance] {
        &self.substances
    }

    #[inline]
    #[must_use]
    pub fn reactions(&self) -> &[DerivedReaction] {
        &self.reactions
    }

    #[inline]
    #[must_use]
    pub fn energy(&self) -> &DerivedEnergy {
        &self.energy
    }

    #[inline]
    #[must_use]
    pub fn fields(&self) -> &[DerivedField] {
        &self.fields
    }

    /// The folded reservoir, or `None` on a scenario with no exchanging face.
    ///
    /// The two go together in both directions: `config/validate.rs` refuses an
    /// `exchange` face with no `[boundary.reservoir]` and a `[boundary.reservoir]`
    /// no face uses, because a declared reservoir touching nothing reads as a
    /// working boundary to everybody who opens the file.
    // TODO(exchange-on-a-coarse-grid): `alpha_ex` is folded at the **fine** `dx`,
    // which is the one the validator bounds `k_ex` against. The enthalpy field
    // lives on `dx*2^lod`, where the same `k_ex` gives an `alpha_ex` smaller by
    // `2^lod`; whether the lid of the enthalpy field trades at `k_ex*dt/dx_coarse`
    // or at an exchange velocity of its own is written in no record, and there is
    // no number for the second. `process::Diffuse::new` folds whatever `dx` it is
    // handed, so today the coarse field exchanges at the coarse rate.
    #[inline]
    #[must_use]
    pub fn reservoir(&self) -> Option<&DerivedReservoir> {
        self.reservoir.as_ref()
    }

    /// The folded light, or `None` on a scenario whose light process is off or
    /// which declares no `i_surface` (ADR-076).
    ///
    /// The one door to `units_per_intensity`. `FoldParams::units_per_intensity`
    /// has to come from here and from nowhere else: a host that folded
    /// `dx^2 * 2^k_E` for itself would be a second construction of a number whose
    /// two mistakes — the coarse face and the forgotten `dt` — are invisible in
    /// every residual and in the shape of the profile.
    #[inline]
    #[must_use]
    pub fn light(&self) -> Option<&DerivedLight> {
        self.light.as_ref()
    }

    /// The four keys of the velocity field, or `None` on a scenario whose field
    /// is off or which declares no `u_conv_max` (ADR-069, ADR-087).
    ///
    /// The one door to them. `Tick::new` folds `VelocityConfig` out of this and
    /// never out of the `Config` it also holds: a second reader of the
    /// `[[process]]` records standing beside the validator's is precisely what
    /// `DerivedSubstance::diffusivity` exists to prevent, and the two agree on
    /// every scenario that validates.
    #[inline]
    #[must_use]
    pub fn velocity(&self) -> Option<&DerivedVelocity> {
        self.velocity.as_ref()
    }

    /// The limiting overflow and the window it passed, or `None` on a scenario
    /// whose pressure process is off or which declares no `theta_max` (ADR-082).
    ///
    /// **The one door to `theta_max`.** `Pressure::new` takes it as an argument
    /// and has to be handed it from here: a host that picked a number of its own
    /// would be inventing the single input the whole mobility is derived from,
    /// and the scheme conserves exactly, so both halves of the invariant would
    /// close over the invention for ever. The precedent is `Derived::light` and
    /// `units_per_intensity`.
    ///
    /// What is deliberately **not** carried beside it: `partial_molar_volume` and
    /// the `Occupant` table step `e` needs to build an occupancy field. ADR-082
    /// leaves both out — they are the work of the record that lifts the five
    /// locks, and carrying them here would make this derivation look ready for a
    /// dispatch that does not exist.
    #[inline]
    #[must_use]
    pub fn pressure(&self) -> Option<&DerivedPressure> {
        self.pressure.as_ref()
    }

    /// The three constants of the medium, or `None` on a scenario whose settling
    /// process is off or which declares no `physics.mu` (ADR-085).
    ///
    /// **The one door to `g` and `rho_medium`.** `Tick::new` builds
    /// `settle::Medium` out of this and never out of the `Config` it also holds:
    /// a second reader of `[physics]` standing beside the validator's would
    /// agree with it on every scenario that was validated and part company on
    /// the first one that was not. The argument is `DerivedSubstance::diffusivity`'s,
    /// word for word, and `Derived::light` and `Derived::pressure` are the two
    /// nearest precedents.
    #[inline]
    #[must_use]
    pub fn medium(&self) -> Option<&DerivedMedium> {
        self.medium.as_ref()
    }

    /// The one record every scenario has: the enthalpy field (ADR-062).
    ///
    /// A named door rather than a search at the call site, because the host that
    /// dispatches steps `c` and `d` over it (`process/tick.rs`) would otherwise
    /// carry the spelling `"enthalpy"` as a literal of its own — and the spelling
    /// is the loader's, not the tick's.
    ///
    /// # Panics
    ///
    /// Never for a [`Derived`] this module built: a scenario without the record
    /// is refused before any field is resolved, because the energy scale is
    /// derived from its temperature range and there is nowhere else to take it
    /// from.
    #[must_use]
    pub fn enthalpy_field(&self) -> &DerivedField {
        self.fields
            .iter()
            .find(|field| field.id == ENTHALPY_FIELD)
            .expect("a scenario without an enthalpy record does not derive")
    }

    /// The substances as [`crate::world::Registry`] takes them.
    ///
    /// The registry assigns lanes and derives nothing (`world/registry.rs`);
    /// this is the hand-over point between the two.
    #[must_use]
    pub fn decls(&self) -> Vec<SubstanceDecl> {
        self.substances
            .iter()
            .map(|s| SubstanceDecl {
                id: s.id.clone(),
                width: s.width,
                k: s.k,
            })
            .collect()
    }

    /// `w_s` for every substance, in declaration order (ADR-081).
    ///
    /// What `ledger::DomainSums::add_chemical_energy` and the two exchange faces
    /// take. Allocating, so a host calls it once when it folds a tick and never
    /// per tick — fourteen `i64` on the registry of SPEC section 2.3, 112 bytes,
    /// and nothing per voxel.
    #[must_use]
    pub fn chemical_weights(&self) -> Vec<i64> {
        self.substances.iter().map(|s| s.chemical_weight).collect()
    }

    /// Everything derived, as lines a loader can print.
    ///
    /// Not decoration: ADR-041 requires the validator to *name the induced
    /// relative error as a number*, and there is nowhere else for that number to
    /// be said.
    ///
    /// What is missing from it is `load_reports_the_restoration_traffic_per_tick`
    /// (ADR-057), which wants the substep count of every *substance* and the
    /// bytes per tick that follow from the parity of the counts. The counts are
    /// not derived here — substance transport derives its own in
    /// `process/diffuse.rs`, from `diffusivity` and not from a `[[field]]`
    /// record — so the line belongs wherever those two meet, which is the caller
    /// this function does not have yet.
    #[must_use]
    pub fn report(&self) -> String {
        let mut out = format!("V_voxel = {:e} m^3\n", self.v_voxel);
        for s in &self.substances {
            let width = match s.width {
                Width::Bits32 => "i32",
                Width::Bits64 => "i64",
            };
            let source = if s.raised_to_e_r {
                "raised to e_r"
            } else {
                "from its own ceiling"
            };
            out.push_str(&format!(
                "substance {}: k = {} ({source}, ceiling {}), {width}, {:e} units at max_conc\n",
                s.id, s.k, s.k_ceiling, s.amount_at_max as f64
            ));
        }
        for r in &self.reactions {
            out.push_str(&format!(
                "reaction {}: e_r = {} (scarcest {}), nu_E = {}, discrepancy {:e}\n",
                r.id, r.e_r, r.scarcest, r.nu_energy, r.energy_relative_error
            ));
            out.push_str(&format!(
                "reaction {}: turnover {:.4} g/mol, mass tolerance {:e} g/mol\n",
                r.id, r.turnover_mass, r.mass_tolerance
            ));
        }
        let width = match self.energy.width {
            Width::Bits32 => "i32",
            Width::Bits64 => "i64",
        };
        for s in &self.substances {
            out.push_str(&format!(
                "substance {}: w = {} units of enthalpy per storage unit \
                 (enthalpy_formation at k_E)\n",
                s.id, s.chemical_weight
            ));
        }
        out.push_str(&format!(
            "energy: k_E = {}, {width}, window [{}, {}], C_cell = {:e} J/K, H_max = {:e} J, \
             worst discrepancy {:e}\n",
            self.energy.k_e,
            self.energy.window.0,
            self.energy.window.1,
            self.energy.c_cell,
            self.energy.h_max,
            self.energy.worst_relative_error
        ));
        // The left-hand side of the energy invariant, weighed (ADR-081). Printed
        // because SPEC section 2.1 writes that side as an unweighted sum and is
        // frozen (ADR-032): the divergence lives here and in the acceptance name
        // `load_reports_the_chemical_energy_of_the_domain_in_joules`, not in an
        // edit to the spec. The ratio is what makes the two numbers a statement
        // — it does not depend on the size of the grid.
        out.push_str(&format!(
            "energy: chemical energy of the domain {:e} J against a declared \
             field span of {:e} J (2*H_max*n_cells), ratio {:.1}\n",
            self.energy.chemical_energy_joules,
            self.energy.field_span_joules,
            self.energy.chemical_energy_joules.abs() / self.energy.field_span_joules
        ));
        out.push_str(&format!(
            "energy: the domain may hold {:.1} bits of chemical energy against \
             the 127 of the ledger accumulator (ADR-081)\n",
            self.energy.chemical_energy_bits
        ));
        // The ceiling of an energy channel counter, in joules (ADR-075,
        // ADR-083). Printed rather than refused: the width is settled — as wide
        // as the domain sum it closes against — but the ceiling is derived from
        // this scenario's own `k_E`, so it belongs in this scenario's report.
        out.push_str(&format!(
            "energy: channel counter ceiling {:e} J (2^127/2^k_E, ADR-083)\n",
            self.energy.counter_ceiling_joules
        ));
        // The sink of S0 and its price, three numbers (ADR-084). It is printed
        // rather than refused, and the record says why in as many words: a
        // refusal on "the lid trades and `t_out != T_ref`" would cover the only
        // scenario that runs today and nearly every venting world worth writing.
        //
        // The **margin in bits** and deliberately not "the tick the counter
        // overflows on". After ADR-083 the counter is `i128` and no declared
        // world has such a tick — `1.4e18` a tick against `2^127` is `1.2e20`
        // ticks, and the whole integral of a decaying transient stops at about
        // seventy bits long before that — so a line printing an overflow tick
        // would print a number that does not exist. That is the class of decoy
        // ADR-083 killed `crediting_more_solar_than_the_counter_holds_is_refused_not_wrapped`
        // for.
        if let Some(reservoir) = &self.reservoir {
            out.push_str(&format!(
                "boundary: the exchange face conducts {:e} W/(m^2*K) and carries \
                 away at most {:e} W/m^2 in steady state — the sink of S0 \
                 (ADR-084)\n",
                reservoir.conductance, reservoir.flux_ceiling
            ));
            // `log2` of a positive number and a plain `127` otherwise. A world
            // with no exchanging face has no lid traffic and a full counter to
            // spend, which is a true statement and not a special case; taking
            // `log2(0)` for it would print `inf` bits of margin instead.
            let bits = if reservoir.lid_credit_per_tick > 0.0 {
                127.0 - reservoir.lid_credit_per_tick.log2()
            } else {
                127.0
            };
            // The ghost enthalpy stands beside the credit rather than in a line
            // of its own, and that pairing is the point: the credit is a bound
            // and the two are a factor apart that a reader can check by hand —
            // `alpha_ex` times the coarse cells of the exchanging faces — so
            // printing one without the other would leave the bound unauditable
            // without a second run.
            out.push_str(&format!(
                "boundary: the ghost cell holds {} units of enthalpy at t_out, \
                 and the lid credits at most {:e} units per tick from T_ref, \
                 leaving {bits:.1} bits of margin in the channel counter \
                 (ADR-084)\n",
                reservoir.enthalpy_out, reservoir.lid_credit_per_tick
            ));
        }
        if let Some(light) = &self.light {
            out.push_str(&format!(
                "light: i_surface = {:e} W/m^2 (period mean), units_per_intensity = {:e} \
                 units per (W/m^2)/s\n",
                light.i_surface, light.units_per_intensity
            ));
            // Two numbers, on two lines, and the separation is the decision
            // (ADR-076). The multiplier has unit mean by construction, so the
            // crossing takes the same number of ticks whatever the modulation is;
            // dividing one by the other yields a ratio of instantaneous rates
            // wearing the units of a time, and in a report that reads as a
            // conservative estimate.
            out.push_str(&format!(
                "light: {:e} ticks to the declared temperature ceiling at the mean\n",
                light.ticks_to_ceiling
            ));
            out.push_str(&format!(
                "light: peak instantaneous multiplier {:e} x the mean (over the \
                 samples, A_N = {:e}, not pi)\n",
                light.peak_multiplier, light.daily_norm
            ));
            // TODO(minimum-tau): ADR-076 also asks the loader to print the
            // smallest optical depth a fine voxel can have under the declared
            // attenuators, as the input to a runtime precision budget for the
            // energy — the relative error of an absorption is `~2^-24/tau`, not
            // `2^-24`. It cannot be computed here: an attenuator is `k` times a
            // dimensionless measure per storage unit, and which measure that is is
            // settled by no document (`TODO(attenuation-measure)` in
            // `process/light.rs`), while `k_w … k_m` reach no table at all.
        }
        // Not a derived quantity anybody uses, and printed anyway: ADR-062 calls
        // `alpha*C_v` the cross-check by which a `thermal_diffusivity` declared
        // an order of magnitude wrong is visible to the eye. An inequality is
        // not — a wrong number passes both refusals above and moves the substeps
        // instead.
        out.push_str(&format!(
            "thermal: alpha*C_v = {:e} W/(m*K), diffusing matter carries {:.2}% \
             of the conductive flux\n",
            self.energy.conductivity,
            self.energy.carried_by_diffusion * 100.0
        ));
        // Four numbers and not one, and the count is the decision (ADR-082).
        // Both inputs of the window are printed because they differ by 0.2% on the
        // shipped registry — swapped, they move nothing visible and fail no test,
        // and they invert the window wherever the dynamic range is wide. The
        // margin is printed together with the limit it is measured against,
        // because `alpha = 8e-4` reads as "small" exactly as well as it reads as
        // "large". And the relaxation time is what cancels the "128 ticks" of
        // ADR-055: the response is diffusive, not ballistic.
        if let Some(pressure) = &self.pressure {
            out.push_str(&format!(
                "pressure: theta_max = {}, Theta_sup = {:.4e} (from max_conc), \
                 Theta_typ = {:.4e} (from typical_conc)\n",
                pressure.theta_max, pressure.occupancy_sup, pressure.occupancy_typ
            ));
            out.push_str(&format!(
                "pressure: the longest mode relaxes in {:.4e} ticks, at {:.1}% of \
                 margin below the oscillation limit alpha = 1/6 \
                 (alpha = {:.4e})\n",
                pressure.relaxation_ticks,
                pressure.stability_margin * 100.0,
                pressure.occupancy_typ / pressure.theta_max
            ));
        }
        for f in &self.fields {
            out.push_str(&format!(
                "field {}: lod {}, {} substeps, alpha {:e}\n",
                f.id, f.lod, f.substeps, f.alpha
            ));
        }
        out
    }
}

/// One reaction between step 1 and step 5: it has an `e_r` but no `nu` yet,
/// because `nu` needs every substance's `k`, and `k` needs every reaction's
/// `e_r`.
struct Pending {
    id: String,
    e_r: u8,
    scarcest: u32,
    /// Substance index and net molar stoichiometry `s_out - s_in`, in
    /// declaration order of the substances. Netted, so at most one entry per
    /// substance.
    participants: Vec<(u32, i32)>,
    enthalpy: f64,
    turnover_mass: f64,
}

/// Derive everything a scenario leaves out.
///
/// # Errors
///
/// Returns an error if the scenario is outside the domain the formulas are
/// defined on, if the system of ADR-039 has no solution for some pair of a
/// substance and a reaction, if the `k_E` window of ADR-062 is empty, if the
/// mass tolerance comes out above the lightest substance with an empty
/// composition (ADR-043), or if a field needs more than [`N_MAX`] substeps
/// (ADR-061).
pub fn derive(config: &Config) -> Result<Derived> {
    let v_voxel = voxel_volume(config)?;
    check_bounds(config)?;

    // 1. `e_r` per reaction, from `typical_conc`. Nothing about a substance's
    //    own scale takes part: the quantum of a turnover is a property of the
    //    reaction and of the pool that limits it (ADR-039).
    let mut pending = Vec::with_capacity(config.reaction.len());
    for reaction in &config.reaction {
        pending.push(
            prepare_reaction(config, reaction, v_voxel)
                .with_context(|| format!("reaction `{}`", reaction.id))?,
        );
    }

    // 2. The overflow ceiling of every substance, from `max_conc`. Still no
    //    reaction in sight.
    let mut ceilings = Vec::with_capacity(config.substance.len());
    for substance in &config.substance {
        ceilings.push(
            scale_ceiling(substance.max_conc, v_voxel)
                .with_context(|| format!("substance `{}`", substance.id))?,
        );
    }

    // 3. and 4. The raise, and the width that follows from it. A separate pass,
    //    because the maximum runs over every reaction and folding it into
    //    either loop above would make the answer depend on the order.
    let mut substances = Vec::with_capacity(config.substance.len());
    for (s, substance) in config.substance.iter().enumerate() {
        // The side comes out of the same pass as the scales and lands on the same
        // record, so that there is one index of a substance and not two
        // (ADR-077). After `parse` the entry is always there — `materialise`
        // fills the table onto every substance before the hash — and the fallback
        // is what a `Config` assembled by hand would get. It is the same named
        // constant and never a `Default`, and the thing that keeps it honest is
        // `the_canonical_form_names_the_layer_side_of_every_substance`: a
        // materialisation that stopped filling the table would leave the side
        // applied and unprinted, and that test is what goes red.
        let layer = config
            .initial
            .layer
            .get(&substance.id)
            .copied()
            .unwrap_or(Layer::DEFAULT);
        substances.push(
            resolve_substance(
                substance,
                layer,
                ceilings[s],
                demand_of(&pending, s as u32),
                v_voxel,
            )
            .with_context(|| format!("substance `{}`", substance.id))?,
        );
    }

    // 5. `nu`, by an exact integer shift.
    let mut reactions = Vec::with_capacity(pending.len());
    for p in &pending {
        let mut nu = Vec::with_capacity(p.participants.len());
        for &(s, net) in &p.participants {
            let substance = &substances[s as usize];
            let value = nu_exact(net, substance.k, p.e_r)
                .with_context(|| format!("substance `{}`, reaction `{}`", substance.id, p.id))?;
            check_nu_fits_i32(value, &substance.id, &p.id)?;
            check_nu_against_the_pool(
                value,
                config.beta,
                config.substance[s as usize].typical_conc,
                v_voxel,
                substance.k,
                &substance.id,
                &p.id,
            )?;
            nu.push(Nu {
                substance: s,
                value,
            });
        }
        reactions.push(DerivedReaction {
            id: p.id.clone(),
            e_r: p.e_r,
            scarcest: p.scarcest,
            nu,
            // Filled in by step 7: `nu_E` needs `k_E`, and `k_E` needs every
            // `e_r`, so it cannot be known here.
            nu_energy: 0,
            energy_relative_error: 0.0,
            // 8. The mass tolerance. Derived from the turnover, never declared:
            //    a number tuned to today's registry goes stale on tomorrow's,
            //    and stale here means the mass check stops catching anything
            //    (ADR-043).
            turnover_mass: p.turnover_mass,
            mass_tolerance: MASS_EPSILON * p.turnover_mass,
        });
    }
    check_mass_tolerance(config, &reactions)?;

    // 6. and 7. The energy scale, the chemical weight of every substance on it,
    //    and `nu_E` summed out of the weights (ADR-081).
    let field = enthalpy_field(config)?;
    let thermal = thermal_transport(config, field)?;
    let mut energy = energy_window(config, field, &pending, &thermal)?;
    for (s, substance) in substances.iter_mut().enumerate() {
        substance.chemical_weight = chemical_weight(
            config.substance[s].enthalpy_formation,
            energy.k_e,
            substance.k,
        )
        .with_context(|| format!("substance `{}`", substance.id))?;
    }
    let weights: Vec<i64> = substances.iter().map(|s| s.chemical_weight).collect();
    let mut worst_relative_error = 0.0;
    let mut worst_reaction = NO_REACTION;
    for (r, (reaction, p)) in reactions.iter_mut().zip(&pending).enumerate() {
        let summed = summed_energy_coefficient(&reaction.nu, &weights);
        let error = check_summed_against_declared(summed, p.enthalpy, energy.k_e, p.e_r, &p.id)?;
        reaction.nu_energy = summed;
        reaction.energy_relative_error = error;
        if error > worst_relative_error || worst_reaction == NO_REACTION {
            worst_relative_error = error;
            worst_reaction = r;
        }
    }
    energy.worst_relative_error = worst_relative_error;
    energy.worst_reaction = worst_reaction;
    energy.chemical_energy_bits = check_domain_chemical_energy(config, &substances)?;
    let (chemical_energy_joules, field_span_joules) =
        domain_energy_report(config, field, &substances, &energy);
    energy.chemical_energy_joules = chemical_energy_joules;
    energy.field_span_joules = field_span_joules;

    // 9. Substeps, per field and at its own lod.
    let mut fields = Vec::with_capacity(config.field.len());
    for record in &config.field {
        fields.push(
            resolve_field(record, config.dt, config.grid.dx)
                .with_context(|| format!("field `{}`", record.id))?,
        );
    }
    check_every_n_ticks(config, &fields)?;

    // 10. The outside reservoir, rounded once (ADR-059). After the scales,
    //     because a composition in storage units needs every `k_i`, and after
    //     the energy scale, because a temperature in storage units needs `k_E`.
    let reservoir = resolve_reservoir(config, &substances, &energy, v_voxel)?;

    // 11. The light, after the energy scale for the same reason the reservoir is:
    //     `units_per_intensity` is `dx^2 * 2^k_E` (ADR-076).
    let light = resolve_light(config, field, &energy)?;

    // 12. The velocity field. After nothing in particular: the four keys of
    //     ADR-069 are carried through unchanged and nothing here is derived from
    //     them (ADR-087).
    let velocity = resolve_velocity(config);

    // 13. Pressure. After nothing in particular either: the window of ADR-082 is
    //     a function of the declared concentrations, the declared partial molar
    //     volumes and the grid, and none of those depends on a scale.
    let pressure = pressure(config)?;

    // 14. The medium of `[physics]`, and after nothing in particular for the
    //     third time: the three constants of ADR-085 are carried through
    //     unchanged and nothing here is derived from them. The chain
    //     `r -> k -> rho_bar -> w -> c` lives in `process/settle.rs` and is
    //     folded against the grid a world actually has.
    let medium = resolve_medium(config);

    Ok(Derived {
        v_voxel,
        substances,
        reactions,
        energy,
        fields,
        reservoir,
        light,
        velocity,
        pressure,
        medium,
    })
}

/// Carry the three constants of `[physics]` through, and derive nothing
/// (ADR-085).
///
/// Returns `None` when the settling process resolves to disabled against the
/// default the process itself declares (ADR-065), and when it is on and the
/// scenario is silent about `mu` — the last of which is a refusal of
/// `config/validate.rs` and not of this function, on the division of labour
/// `resolve_velocity` writes out: `derive` is public and defined on configs
/// nobody validated, so it describes what it was given rather than judging it.
///
/// An absent `[[process]]` record resolves to the process's own default rather
/// than to "off", and after `parse` there is no such case at all — the roster is
/// materialised before the hash. `validate` is public and defined on a `Config`
/// somebody assembled by hand, which is the case `pressure` guards the same way.
///
/// Infallible on purpose, exactly as `resolve_velocity` is: it copies three
/// numbers and has no domain of its own. The domain of the three lives in
/// `config/validate.rs` among the other rules of section 10.
fn resolve_medium(config: &Config) -> Option<DerivedMedium> {
    let record = config.process.iter().find(|p| p.id == SETTLING_PROCESS);
    let enabled = match record {
        Some(process) => process.enabled.unwrap_or(SETTLING_ENABLED_BY_DEFAULT),
        None => SETTLING_ENABLED_BY_DEFAULT,
    };
    if !enabled {
        return None;
    }
    let mu = config.physics.mu?;

    Some(DerivedMedium {
        g: config.physics.g,
        rho_medium: config.physics.rho_medium,
        mu,
    })
}

/// `Theta_sup = sum over V_bar > 0 of max_conc * V_bar`, dimensionless
/// (ADR-082).
///
/// The filter is the decision. See [`DerivedPressure::occupancy_sup`] for what
/// the unfiltered sum costs and for which test stands in its way.
fn occupancy_sup(config: &Config) -> f64 {
    config
        .substance
        .iter()
        .filter(|s| s.partial_molar_volume > 0.0)
        .map(|s| s.max_conc * s.partial_molar_volume)
        .sum()
}

/// `Theta_typ`, the same expression over `typical_conc` (ADR-082).
///
/// The same filter, in the record's own words — "computed by the same
/// expression" — and the direction of that choice is worth naming rather than
/// leaving to be discovered: a negative `V_bar` left in would lower `Theta_typ`,
/// raise the relaxation time and *tighten* the ceiling, so the filtered form is
/// the permissive one of the two and the conservative reading is the one not
/// taken.
fn occupancy_typ(config: &Config) -> f64 {
    config
        .substance
        .iter()
        .filter(|s| s.partial_molar_volume > 0.0)
        .map(|s| s.typical_conc * s.partial_molar_volume)
        .sum()
}

/// The window of the limiting overflow, and the two numbers a report prints
/// beside it (ADR-082).
///
/// Returns `None` when the process resolves to disabled against the default its
/// own module declares (ADR-065), and when it is on and silent about
/// `theta_max` — the last of which is a refusal of `config/validate.rs` and not
/// of this function, on the division of labour [`resolve_light`] writes out.
///
/// # Errors
///
/// Returns an error when the declared `theta_max` falls outside the window: below
/// `6*Theta_sup`, where the checkerboard mode of the relaxation grows rather than
/// decays, or above the value at which the longest mode of the domain no longer
/// relaxes inside the declared run horizon.
///
/// **The window can be empty, and this function does not say so.** The floor is
/// `6*Theta_sup` and the ceiling is `1e6*pi^2*Theta_typ/N^2`, so the two cross at
/// `N^2 > 1.6449e6 * Theta_typ/Theta_sup` — on a 64-cubed grid that is a ratio of
/// about 402, and ADR-039 allows `max_conc/typical_conc` up to `2^14`. A scenario
/// there gets whichever of the two messages is checked first, and neither says
/// "no legal value exists". ADR-082 assigns no message for the case; the order
/// below is the floor first, because it is the one that is about stability rather
/// than about patience.
fn pressure(config: &Config) -> Result<Option<DerivedPressure>> {
    // The default belongs to the process and is read from it (ADR-065). An absent
    // record resolves to that default rather than to "off": after `parse` there is
    // no such case, because `config::materialise` writes all nine records before
    // the hash, but `derive` is public and defined on a hand-built `Config`.
    let record = config.process.iter().find(|p| p.id == PRESSURE_PROCESS);
    let enabled = match record {
        Some(process) => process.enabled.unwrap_or(PRESSURE_ENABLED_BY_DEFAULT),
        None => PRESSURE_ENABLED_BY_DEFAULT,
    };
    if !enabled {
        return Ok(None);
    }
    let Some(theta_max) = record.and_then(|p| p.theta_max) else {
        return Ok(None);
    };

    let occupancy_sup = occupancy_sup(config);
    let occupancy_typ = occupancy_typ(config);

    // The floor. Linearising the relaxation about a uniform state gives explicit
    // diffusion with `alpha = Theta/theta_max` — the occupancy, because the donor
    // of a face is the whole pool — whose checkerboard growth factor is
    // `1 - 12*alpha`, so `|g| <= 1` exactly at `alpha <= 1/6`.
    let floor = 6.0 * occupancy_sup;
    let above_floor = theta_max.is_finite() && theta_max >= floor;
    if !above_floor {
        bail!(
            "process `{PRESSURE_PROCESS}` declares theta_max = {theta_max}, and \
             the rule is theta_max >= {floor:.4e}, which is 6*Theta_sup at \
             Theta_sup = {occupancy_sup:.4e} — the peak occupancy V_occ/V_voxel \
             the declared max_conc and partial_molar_volume allow. The occupancy \
             and not the overflow theta = Theta - 1: the donor of a face is the \
             whole pool, so the linearised operator is explicit diffusion with \
             alpha = Theta/theta_max, whose checkerboard growth factor is \
             1 - 12*alpha (ADR-082). Below the floor neighbouring voxels swap \
             pools every tick with matter conserved exactly and both halves of \
             the invariant closed, so nothing downstream of the load can see it. \
             Raise theta_max, or lower the max_conc of whatever fills the voxel"
        );
    }

    // The domain of the ceiling, and it is arithmetic rather than policy: a
    // registry where nothing takes up room has no relaxation time, because the
    // formula divides by `Theta_typ`. The author's next move is to declare a
    // partial molar volume, so the message is short.
    if !(occupancy_typ.is_finite() && occupancy_typ > 0.0) {
        bail!(
            "process `{PRESSURE_PROCESS}` is enabled over a registry whose \
             typical occupancy is {occupancy_typ}: no substance declares a \
             positive partial_molar_volume, so nothing takes up room, the \
             overflow field is identically -1 everywhere and the relaxation time \
             N_max^2*theta_max/(pi^2*Theta_typ) is not defined at all (ADR-067, \
             ADR-082)"
        );
    }

    // `N_max` is the longest axis of **this** grid. Named `n_side` and never
    // `N_MAX`, which is imported into this module and means the bound on a
    // substep count (ADR-061): spelled that way the ceiling is the 64-cubed
    // ceiling on every grid, which on the two grids the project ships is either
    // exactly right or wrong in the permissive direction.
    let n_side = f64::from(config.grid.nx.max(config.grid.ny).max(config.grid.nz));
    let relaxation_ticks =
        n_side * n_side * theta_max / (std::f64::consts::PI.powi(2) * occupancy_typ);
    let within_horizon = relaxation_ticks.is_finite() && relaxation_ticks <= RUN_HORIZON_TICKS;
    if !within_horizon {
        bail!(
            "process `{PRESSURE_PROCESS}` declares theta_max = {theta_max}: the \
             longest mode of a domain {n_side} voxels across then relaxes in \
             {relaxation_ticks:.4e} ticks — N_max^2*theta_max/(pi^2*Theta_typ) at \
             Theta_typ = {occupancy_typ:.4e}, the working occupancy the declared \
             typical_conc gives — against the declared run horizon of \
             {RUN_HORIZON_TICKS:e} ticks (SPEC section 13, criterion S0). A domain \
             that never equilibrates inside the horizon it is measured over has no \
             pressure in it, only a drift nobody will watch to the end (ADR-082). \
             Lower theta_max, or coarsen the grid: the ceiling falls as N^2"
        );
    }

    Ok(Some(DerivedPressure {
        theta_max,
        occupancy_sup,
        occupancy_typ,
        relaxation_ticks,
        // `1 - alpha/(1/6)` at `alpha = Theta_typ/theta_max`, computed from the
        // declared concentrations and never written down as a literal: ADR-082
        // prints `1.997e-3` for the shipped registry where an independent
        // recomputation gives `1.986e-3`, and a literal would freeze one of them.
        stability_margin: 1.0 - 6.0 * occupancy_typ / theta_max,
    }))
}

/// Carry the four keys of the velocity field through, and derive nothing
/// (ADR-069, ADR-087).
///
/// Returns `None` when the process record is absent, when it resolves to
/// disabled against the default the process itself declares (ADR-065), and when
/// it is on and silent about `u_conv_max` — the last of which is a refusal of
/// `config/validate.rs` and not of this function, on the division of labour
/// `resolve_light` writes out: `derive` is public and defined on configs nobody
/// validated, so it describes what it was given rather than judging it.
///
/// Infallible on purpose, and that is the difference from `resolve_light`. That
/// function has a domain of its own — a modulation period of two ticks divides by
/// zero — and this one has none: it copies four numbers.
fn resolve_velocity(config: &Config) -> Option<DerivedVelocity> {
    let process = config.process.iter().find(|p| p.id == VELOCITY_PROCESS)?;
    // The default belongs to the process and is read from it (ADR-065): absence
    // of a record means the process's own default, and a second copy of the value
    // here would be a second thing to keep in step.
    if !process.enabled.unwrap_or(VELOCITY_FIELD_ENABLED_BY_DEFAULT) {
        return None;
    }
    let u_conv_max = process.u_conv_max?;

    Some(DerivedVelocity {
        u_conv_max,
        // `l_c` is carried as it was declared, absent and all: the radius
        // `r = round(l_c/(2*dx_coarse))` is derived in `VelocityField::new` from
        // the grid the world actually has, and a `0.0` substituted here would
        // reach that refusal as a *declared* zero rather than as a missing key.
        l_c: process.l_c.unwrap_or(0.0),
        stir_fraction: process.stir_fraction,
        stir_period: process.stir_period,
    })
}

/// Fold the five light keys into what the host and the report need (ADR-076).
///
/// Returns `None` when the process is off, or on and silent about `i_surface`.
/// The second case is a refusal of `config/validate.rs` and not of this function:
/// `derive` is public and defined on configs nobody validated, so it describes
/// what it was given rather than judging it — the same division of labour the
/// duplicate-id checks here keep.
///
/// What it does refuse is its own domain: a modulation whose period is not a
/// whole number of ticks, or is under three of them, has no `A_N` at all — at
/// `N = 2` the two samples of `max(0, sin)` are both zero, the sum is zero and
/// the normalisation divides by it. That is arithmetic and not policy, and the
/// message here is short because the validator's is the one an author reads.
fn resolve_light(
    config: &Config,
    field: &FieldRecord,
    energy: &DerivedEnergy,
) -> Result<Option<DerivedLight>> {
    let Some(process) = config.process.iter().find(|p| p.id == LIGHT_PROCESS) else {
        return Ok(None);
    };
    // The default belongs to the process and is read from it (ADR-065): absence
    // of a record means the process's own default, not the absence of the
    // process, and a second copy of the value here would be a second thing to
    // keep in step.
    if !process.enabled.unwrap_or(LIGHT_ENABLED_BY_DEFAULT) {
        return Ok(None);
    }
    let Some(i_surface) = process.i_surface else {
        return Ok(None);
    };

    let units_per_joule = exp2_exact(i32::from(energy.k_e))?;
    // The area of a **fine** face. See `DerivedLight::units_per_intensity` for
    // what the coarse one would cost, and for why `dt` is not in here.
    let units_per_intensity = config.grid.dx * config.grid.dx * units_per_joule;

    let daily_period_ticks = period_in_ticks(process.daily_period, config.dt, "daily")?;
    let seasonal_period_ticks = period_in_ticks(process.seasonal_period, config.dt, "seasonal")?;
    let (daily_norm, daily_peak_sample) = daily_normalisation(daily_period_ticks);

    // The peak of `m_d(n) * m_s(n)` over the samples. The two maxima are taken
    // independently, which is an upper bound rather than an attained value unless
    // the periods happen to align — and an upper bound is what a report about a
    // ceiling owes the reader.
    let daily_peak =
        1.0 - process.daily_fraction + process.daily_fraction * daily_norm * daily_peak_sample;
    let peak_multiplier = daily_peak * (1.0 + process.seasonal_fraction);

    let coarse_dx = config.grid.dx * exp2_exact(i32::from(field.lod))?;
    // `H_max` joules against what the mean irradiance delivers to one coarse cell
    // in a tick. Infinite at `i_surface = 0`; see `TODO(dark-ceiling)`.
    let per_tick = i_surface * coarse_dx * coarse_dx * config.dt;
    let ticks_to_ceiling = energy.h_max / per_tick;

    Ok(Some(DerivedLight {
        i_surface,
        units_per_intensity,
        daily_fraction: process.daily_fraction,
        daily_period_ticks,
        daily_norm,
        seasonal_fraction: process.seasonal_fraction,
        seasonal_period_ticks,
        peak_multiplier,
        ticks_to_ceiling,
    }))
}

/// A modulation period in seconds as a whole number of ticks, or zero when the
/// key is absent.
fn period_in_ticks(period: Option<f64>, dt: f64, which: &str) -> Result<u32> {
    let Some(period) = period else {
        return Ok(0);
    };
    let ticks = period / dt;
    if !ticks.is_finite() || ticks.fract() != 0.0 || ticks < 3.0 || ticks > f64::from(u32::MAX) {
        bail!(
            "the {which} period {period} s is {ticks} ticks at dt = {dt} s, and \
             the rule is a whole number of ticks and at least three: the exactness \
             of the period mean stands on an integer N, and at N = 2 the discrete \
             normalisation divides by zero (ADR-076)"
        );
    }
    Ok(ticks as u32)
}

/// `(A_N, max_n max(0, sin(2*pi*n/N)))` for a daily period of `n_ticks`.
///
/// `(1, 0)` at `n_ticks == 0`, which is the no-modulation case: the fraction is
/// then zero as well and the multiplier is identically one.
///
/// The samples come from `process::light::daily_sample` and are not recomputed
/// here. They are the samples the run will actually take, at the phase it will
/// take them (zero; ADR-076 declares no phase key) — a normalisation taken off
/// the continuous envelope instead is the `1/pi` that record rejects, and a
/// second transcription of the same formula would put the normalisation and the
/// multiplier out of step by a hair, which is invisible in a way a percent is
/// not.
fn daily_normalisation(n_ticks: u32) -> (f64, f64) {
    if n_ticks == 0 {
        return (1.0, 0.0);
    }
    let mut sum = 0.0;
    let mut peak: f64 = 0.0;
    for step in 0..n_ticks {
        let sample = daily_sample(step, n_ticks);
        sum += sample;
        peak = peak.max(sample);
    }
    (f64::from(n_ticks) / sum, peak)
}

/// Fold `[boundary.reservoir]` into the three numbers a run needs (ADR-059).
///
/// Every refusal a scenario can meet here — a missing substance, a concentration
/// over `max_conc`, `t_out` outside the field's range, `k_ex` over the Courant
/// bound — is `config/validate.rs`'s and has already happened. What is left is
/// arithmetic and the two ways it overflows.
fn resolve_reservoir(
    config: &Config,
    substances: &[DerivedSubstance],
    energy: &DerivedEnergy,
    v_voxel: f64,
) -> Result<Option<DerivedReservoir>> {
    let Some(reservoir) = &config.boundary.reservoir else {
        return Ok(None);
    };

    let mut amount_out = Vec::with_capacity(config.substance.len());
    for (s, substance) in config.substance.iter().enumerate() {
        let conc = *reservoir.conc_out.get(&substance.id).with_context(|| {
            format!(
                "[boundary.reservoir] names no conc_out for substance `{}`",
                substance.id
            )
        })?;
        amount_out.push(
            amount_in_units(conc, v_voxel, substances[s].k)
                .with_context(|| format!("the reservoir concentration of `{}`", substance.id))?,
        );
    }

    // `V_cell` of the **enthalpy** grid and never `V_voxel`: the ghost cell of
    // that field is a coarse cell, and at `lod = 2` the two differ by
    // sixty-four with nothing downstream noticing.
    let field = enthalpy_field(config)?;
    let coarse_dx = config.grid.dx * exp2_exact(i32::from(field.lod))?;
    let v_cell = coarse_dx * coarse_dx * coarse_dx;
    let c_cell_out: f64 = config
        .substance
        .iter()
        .map(|substance| reservoir.conc_out[&substance.id] * substance.c_p * v_cell)
        .sum();

    // `T_ref` of the scenario, and not the thermochemical reference of the
    // enthalpies of formation (ADR-044).
    let joules = (reservoir.t_out - config.t_ref) * c_cell_out;
    let scaled = joules * exp2_exact(i32::from(energy.k_e))?;
    if !scaled.is_finite() {
        bail!(
            "[boundary.reservoir] at t_out = {} K gives an enthalpy of {joules} J              for one coarse cell, which does not fit the energy scale k_E = {}              (ADR-059, ADR-062)",
            reservoir.t_out,
            energy.k_e
        );
    }
    // `f64::round` is the rounding rule of the system: halves away from zero
    // (`NUMERIC.md` section 3), the same one `amount_in_units` above uses.
    let rounded = scaled.round();
    let Some(enthalpy_out) = (rounded.abs() <= i64::MAX as f64).then_some(rounded as i64) else {
        bail!(
            "[boundary.reservoir] at t_out = {} K gives {rounded} storage units of              enthalpy for one coarse cell, past what the field can hold (ADR-062)",
            reservoir.t_out
        );
    };

    // The three numbers of ADR-084, and the first two come from the validator's
    // own functions rather than from a second folding here — see
    // `DerivedReservoir::conductance`. Both are `Option` there and neither can be
    // `None` at this point: `boundary_conductance` answers `None` exactly when
    // there is no `[boundary.reservoir]`, and this function returned early in
    // that case; `absorbed_flux_ceiling` adds the `t_min`/`t_max` pair, which
    // `energy_window` refused a scenario for lacking before `derive` reached
    // here. `unwrap_or(NAN)` rather than `expect` all the same, because the price
    // of being wrong about that is a report line reading `NaN` and not a panic in
    // a loader.
    let conductance = super::validate::boundary_conductance(config).unwrap_or(f64::NAN);
    let flux_ceiling = super::validate::absorbed_flux_ceiling(config).unwrap_or(f64::NAN);

    Ok(Some(DerivedReservoir {
        alpha_ex: reservoir.k_ex * config.dt / config.grid.dx,
        k_ex: reservoir.k_ex,
        amount_out,
        enthalpy_out,
        conductance,
        flux_ceiling,
        lid_credit_per_tick: lid_credit_per_tick(config, field.lod, enthalpy_out, coarse_dx),
    }))
}

/// An upper bound on what the `exchange` faces credit to `BOUNDARY_EXCHANGE` in
/// one tick of a world at `T_ref`, in storage units of the energy scale
/// (ADR-084).
///
/// See [`DerivedReservoir::lid_credit_per_tick`] for why it is a bound, why the
/// gap is taken from `T_ref`, and why `alpha_ex` is at the coarse `dx`.
///
/// The area counted is the **exchanging** faces and not the lit one. Those can
/// differ — a world may vent through a side wall under a closed lid, and the grid
/// need not be cubic — and this is the traffic number, so it counts what actually
/// trades. The other side of that difference, the one the steady-state ceiling
/// needs, is `TODO(A-25)` in `config/validate.rs`.
///
/// Which faces those are comes from `validate::exchanging_faces` and is not
/// listed again here. Its doc says why: a second transcription of the six is how
/// a face added to [`super::Boundary`] gets seen by one rule and not another.
/// What is written out below is only the mapping from a face to the cells it
/// covers, and it is exhaustive on purpose — a name that function grows and this
/// one does not know is a panic at load, not an area of zero.
fn lid_credit_per_tick(config: &Config, lod: u8, enthalpy_out: i64, coarse_dx: f64) -> f64 {
    let coarse = |extent: u32| f64::from(extent >> u32::from(lod));
    let (nx, ny, nz) = (
        coarse(config.grid.nx),
        coarse(config.grid.ny),
        coarse(config.grid.nz),
    );
    let cells: f64 = super::validate::exchanging_faces(&config.boundary)
        .iter()
        .map(|face| match *face {
            "x_min" | "x_max" => ny * nz,
            "y_min" | "y_max" => nx * nz,
            "z_min" | "z_max" => nx * ny,
            other => unreachable!("`{other}` is not one of the six faces"),
        })
        .sum();
    let alpha_ex_coarse =
        config.boundary.reservoir.as_ref().map_or(0.0, |r| r.k_ex) * config.dt / coarse_dx;
    alpha_ex_coarse * (enthalpy_out as f64).abs() * cells
}

/// `dx^3`, with the two ways it can fail to be a volume ruled out.
fn voxel_volume(config: &Config) -> Result<f64> {
    let dx = config.grid.dx;
    if !dx.is_finite() || dx <= 0.0 {
        bail!("dx {dx} m is not a usable voxel edge");
    }
    let v = dx * dx * dx;
    if !v.is_finite() || v <= 0.0 {
        bail!("dx {dx} m gives a voxel volume of {v} m^3");
    }
    Ok(v)
}

/// The domains the formulas below are defined on, plus the two build-time
/// bounds of the reaction kernel.
///
/// `CONFIG_SCHEMA.md` section 10 lists these under "domain of definition" and
/// gives none of them a test name: they are forced by the formulas rather than
/// chosen. `typical_conc = 0` goes under the logarithm in `e_r` and produces an
/// infinity or a panic before any message; `max_conc < typical_conc` passes all
/// forty refusals — the ratio is below one and so below `2^14` — and yields a
/// scale derived from a ceiling below the typical pool.
fn check_bounds(config: &Config) -> Result<()> {
    if config.substance.len() > MAX_SUBSTANCES as usize {
        bail!(
            "{} substances declared, at most {MAX_SUBSTANCES} can be addressed \
             (ADR-041)",
            config.substance.len()
        );
    }
    if config.reaction.len() > R_MAX {
        bail!(
            "{} reactions declared, the reaction kernel sizes its local arrays \
             at R_MAX = {R_MAX} (ADR-041)",
            config.reaction.len()
        );
    }
    for (r, reaction) in config.reaction.iter().enumerate() {
        if let Some(first) = config.reaction[..r]
            .iter()
            .position(|other| other.id == reaction.id)
        {
            // The referential-integrity rule of `CONFIG_SCHEMA.md` section 10,
            // and the more expensive of the two duplicate ids. `reaction_id` is
            // taken from the name (ADR-027), so two records sharing one id share
            // one stream of random numbers: the stochastic rounding of the two
            // reactions stops being independent, a run of the same seed and
            // config parts from a correct implementation, and nothing fails —
            // the ledger closes either way, because rounding moves whole
            // quanta.
            bail!(
                "reaction `{}` is declared twice, at index {first} and at index \
                 {r}: `reaction_id` is taken from the name (ADR-027), so the two \
                 would draw from one stream of random numbers and their \
                 stochastic rounding would stop being independent",
                reaction.id
            );
        }
    }
    if !config.beta.is_finite() || config.beta <= 0.0 || config.beta > 1.0 {
        bail!(
            "beta = {} is not a fraction of a typical pool (ADR-039)",
            config.beta
        );
    }
    for (s, substance) in config.substance.iter().enumerate() {
        if let Some(first) = config.substance[..s]
            .iter()
            .position(|other| other.id == substance.id)
        {
            bail!(
                "substance `{}` is declared twice, at index {first} and at index \
                 {s}: a reaction naming it resolves to the first, and the second \
                 still takes an index and a lane",
                substance.id
            );
        }
        let (typical, max) = (substance.typical_conc, substance.max_conc);
        if !typical.is_finite() || typical <= 0.0 {
            bail!(
                "substance `{}` declares typical_conc = {typical} mol/m^3, which \
                 goes under the logarithm of e_r",
                substance.id
            );
        }
        if !max.is_finite() || max <= 0.0 {
            bail!(
                "substance `{}` declares max_conc = {max} mol/m^3",
                substance.id
            );
        }
        if max < typical {
            bail!(
                "substance `{}` declares max_conc {max} below typical_conc \
                 {typical} mol/m^3: the scale would be derived from a ceiling \
                 under the typical pool, and every refusal downstream would pass",
                substance.id
            );
        }
        // `substance_dynamic_range_over_2e14_is_rejected`. The resolution at the
        // typical concentration is `2^28 * typ/max` (ADR-039), so a range wider
        // than `2^14` leaves under `2^14` units in a typical pool.
        if max / typical > 16384.0 {
            bail!(
                "substance `{}` declares a dynamic range max_conc/typical_conc = \
                 {} over the limit of 2^14: at that spread the typical pool holds \
                 fewer than 2^14 units and the scale carries no resolution \
                 (ADR-039)",
                substance.id,
                max / typical
            );
        }
    }
    Ok(())
}

/// Step 1 for one reaction: the netted participants, `e_r`, and the turnover
/// mass.
fn prepare_reaction(config: &Config, reaction: &Reaction, v_voxel: f64) -> Result<Pending> {
    let participants = netted_participants(config, reaction)?;
    let (e_r, scarcest) = extent_exponent(config, &participants, v_voxel)?;
    Ok(Pending {
        id: reaction.id.clone(),
        e_r,
        scarcest,
        turnover_mass: turnover_mass(config, &participants),
        participants,
        enthalpy: reaction.enthalpy,
    })
}

/// Resolve `inputs` and `outputs` into one entry per distinct substance.
///
/// In declaration order of the substances, not of either map: the order of the
/// records is the order of `nu` handed to the kernel, and it has to be a
/// property of the registry rather than of how the TOML was typed.
fn netted_participants(config: &Config, reaction: &Reaction) -> Result<Vec<(u32, i32)>> {
    for (side, table) in [("inputs", &reaction.inputs), ("outputs", &reaction.outputs)] {
        for (id, &s) in table {
            if s <= 0 {
                bail!(
                    "{side} names `{id}` with stoichiometry {s}: molar coefficients are positive integers (ADR-026)"
                );
            }
            if !config.substance.iter().any(|it| it.id == *id) {
                bail!("{side} names `{id}`, which is not a declared substance");
            }
        }
    }

    let mut out = Vec::new();
    for (s, substance) in config.substance.iter().enumerate() {
        let inp = reaction.inputs.get(&substance.id).copied().unwrap_or(0);
        let outp = reaction.outputs.get(&substance.id).copied().unwrap_or(0);
        if inp == 0 && outp == 0 {
            continue;
        }
        out.push((s as u32, outp - inp));
    }
    if out.is_empty() {
        bail!("no participants: a reaction with neither inputs nor outputs has no extent");
    }
    Ok(out)
}

/// `e_r = ceil(max over participants log2( s_i / (beta * typ_conc_i * V_voxel) ))`,
/// and which participant the maximum came from.
///
/// The maximum, not the minimum. The quantum of a turnover is set by the
/// participant the reaction runs out of first, and that is the *scarcest* one;
/// tied to the most abundant, `nu` of the scarce participants comes out larger
/// than their whole pool, `xi_max = min floor(amount_i/nu_i)` is zero, and the
/// reaction never proceeds — silently, with the ledger closing on a change of
/// zero (ADR-039, which was written to overturn exactly that reading of
/// ADR-035).
///
/// Participants are taken from `inputs` **and** `outputs`. In the
/// photosynthesis of SPEC section 5 the scarcest participant is the proton and
/// it stands on the right: counted over inputs alone, `e_r` falls from 63 to
/// about 47, water keeps its ceiling of 52, `i64` never happens, and the whole
/// of ADR-040 evaporates without a failing test.
fn extent_exponent(
    config: &Config,
    participants: &[(u32, i32)],
    v_voxel: f64,
) -> Result<(u8, u32)> {
    let mut best = f64::NEG_INFINITY;
    let mut scarcest = None;
    for &(s, net) in participants {
        if net == 0 {
            // A substance standing identically on both sides converts nothing:
            // its `nu` is zero at any scale, so it neither limits the reaction
            // nor constrains `k`.
            continue;
        }
        let substance = &config.substance[s as usize];
        let quanta = f64::from(net.abs()) / (config.beta * substance.typical_conc * v_voxel);
        if !quanta.is_finite() || quanta <= 0.0 {
            bail!(
                "substance `{}` gives s/(beta*typical_conc*V_voxel) = {quanta}",
                substance.id
            );
        }
        if quanta > best {
            best = quanta;
            scarcest = Some(s);
        }
    }
    let Some(scarcest) = scarcest else {
        bail!("every participant nets to zero: the reaction converts nothing");
    };

    let exponent = ceil_log2(best)?;
    // Below zero the quantum would be *coarser* than one turnover, and the
    // kernel's table of extent exponents is unsigned (`ARCHITECTURE.md`, `Rx`).
    // Clamping up is the safe direction: a smaller `e_r` only makes `nu` larger,
    // so the finest representable quantum is taken instead.
    let exponent = exponent.max(0);
    let exponent = u8::try_from(exponent).with_context(|| {
        format!("extent exponent {exponent} does not fit the u8 of world::SubstanceDecl")
    })?;
    Ok((exponent, scarcest))
}

/// Mass of one turnover, g/mol: the consumed side of the netted records.
///
/// One side, and the netted records, and both halves matter. Summed over both
/// sides it is doubled; summed over the raw tables, a substance standing on both
/// sides is counted twice — and either inflates the tolerance, so the check that
/// was supposed to catch a lost proton becomes the one that cannot
/// (ADR-033, ADR-043).
fn turnover_mass(config: &Config, participants: &[(u32, i32)]) -> f64 {
    participants
        .iter()
        .filter(|(_, net)| *net < 0)
        .map(|&(s, net)| f64::from(-net) * config.substance[s as usize].molar_mass)
        .sum()
}

/// The largest `e_r` among the reactions this substance actually converts in,
/// **and the reaction that set it**.
///
/// The pair rather than the number, because the refusal downstream is one of the
/// two of ADR-039 that have to name a substance *and* a reaction: a raise past
/// what an `i64` can hold is a fault of the pair, and a config whose fault lies
/// in a pair cannot be fixed by looking at one name. Returning `u8` alone made
/// that impossible — the reaction never reached the message at all, and the
/// substance arrived only through the caller's `.with_context`.
fn demand_of(pending: &[Pending], s: u32) -> Option<(u8, &str)> {
    pending
        .iter()
        .filter(|p| {
            p.participants
                .iter()
                .any(|&(idx, net)| idx == s && net != 0)
        })
        .map(|p| (p.e_r, p.id.as_str()))
        .max_by_key(|&(e_r, _)| e_r)
}

/// Steps 3 and 4 for one substance: the raise, the width it forces, and the side
/// of the boundary the scenario put it on.
fn resolve_substance(
    substance: &Substance,
    layer: Layer,
    ceiling: i32,
    demand: Option<(u8, &str)>,
    v_voxel: f64,
) -> Result<DerivedSubstance> {
    if ceiling < 0 {
        bail!(
            "the overflow ceiling comes out at {ceiling}: one voxel holds \
             {} mol at max_conc, over the 2^{} the headroom leaves even at one \
             unit per mole. A negative exponent has nowhere to be stored — \
             `world::SubstanceDecl.k` is a u8 and an `as u8` would wrap it into \
             a large positive value in silence",
            substance.max_conc * v_voxel,
            i32::BITS - CEILING_HEADROOM_BITS
        );
    }
    let k_ceiling = u8::try_from(ceiling)
        .with_context(|| format!("scale exponent {ceiling} does not fit a u8"))?;

    // The raise, as `CONFIG_SCHEMA.md` section 9 writes it: inside the formula.
    // A substance no reaction converts has no demand at all, which is not the
    // same as a demand of zero only because the reaction that set it has to
    // reach the message below.
    let (demanded, by_reaction) = demand.map_or((0, "<no reaction>"), |(e_r, id)| (e_r, id));
    let raised_to_e_r = demanded > k_ceiling;
    let k = k_ceiling.max(demanded);

    let amount_at_max = amount_in_units(substance.max_conc, v_voxel, k)?;
    let amount_at_typical = amount_in_units(substance.typical_conc, v_voxel, k)?;

    let width = if amount_at_max > i128::from(i32::MAX) {
        Width::Bits64
    } else {
        Width::Bits32
    };
    if amount_at_max > i128::from(i64::MAX) {
        // The one refusal on this inequality, wearing whichever of its two names
        // fits the cause (`CONFIG_SCHEMA.md` section 10).
        if raised_to_e_r {
            bail!(
                "at k = {k}, raised from its ceiling of {k_ceiling} by the extent \
                 exponent e_r = {demanded} of reaction `{by_reaction}`, one voxel \
                 of substance `{}` holds {amount_at_max} units at max_conc, over \
                 the {} an i64 can hold. The pair `{}` + `{by_reaction}` is at \
                 fault here, not the substance alone: lower max_conc of `{}`, or \
                 raise typical_conc of the scarcest participant of \
                 `{by_reaction}` so that its e_r falls (ADR-039, ADR-040)",
                substance.id,
                i64::MAX,
                substance.id,
                substance.id
            );
        }
        bail!(
            "at k = {k} one voxel holds {amount_at_max} units at max_conc, which \
             does not fit an i64 (ADR-039, ADR-040)"
        );
    }

    Ok(DerivedSubstance {
        id: substance.id.clone(),
        k,
        k_ceiling,
        raised_to_e_r,
        width,
        amount_at_max,
        amount_at_typical,
        diffusivity: substance.diffusivity,
        settling_radius: substance.settling_radius,
        molar_mass: substance.molar_mass,
        partial_molar_volume: substance.partial_molar_volume,
        // Filled in by step 7 for the reason `nu_energy` is: `w_s` needs `k_E`,
        // and `k_E` needs every `e_r` and every `c_p` (ADR-062, ADR-081).
        chemical_weight: 0,
        layer,
    })
}

/// `floor(log2( 2^(32-4) / (max_conc * V_voxel) ))` — the largest scale at which
/// a voxel at `max_conc` still leaves the headroom of ADR-039.
///
/// Computed as the answer to the inequality rather than as the logarithm of the
/// quotient, so that the result is exact: multiplying by a power of two is exact
/// in binary floating point, dividing by a declared concentration is not.
fn scale_ceiling(max_conc: f64, v_voxel: f64) -> Result<i32> {
    let per_voxel = max_conc * v_voxel;
    if !per_voxel.is_finite() || per_voxel <= 0.0 {
        bail!("max_conc * V_voxel = {per_voxel} mol is not a usable amount");
    }
    let headroom = headroom(i32::BITS)?;
    let mut k = floor_log2(headroom / per_voxel)?;
    // Two corrections at most, and only against exact comparisons.
    while k > -1022 && per_voxel * exp2_exact(k)? > headroom {
        k -= 1;
    }
    while k < 1022 && per_voxel * exp2_exact(k + 1)? <= headroom {
        k += 1;
    }
    Ok(k)
}

/// `2^(bits - CEILING_HEADROOM_BITS)`.
fn headroom(bits: u32) -> Result<f64> {
    exp2_exact(i32::try_from(bits - CEILING_HEADROOM_BITS)?)
}

/// `round(conc * V_voxel * 2^k)`, in storage units.
fn amount_in_units(conc: f64, v_voxel: f64, k: u8) -> Result<i128> {
    let amount = conc * v_voxel * exp2_exact(i32::from(k))?;
    if !amount.is_finite() || amount < 0.0 {
        bail!("an amount of {amount} units is not storable");
    }
    if amount > 1.0e30 {
        bail!("an amount of {amount:e} units is past any storable width");
    }
    Ok(amount.round() as i128)
}

/// `nu_i = s_i * 2^(k_i - e_r)`, as an exact integer shift.
///
/// A shift on `i128`, and not `s as f64 * 2f64.powi(k - e_r)`, because the two
/// differ exactly where the derivation could be wrong. With `k < e_r` the real
/// version produces a fraction and the `as i64` behind it cuts the fraction off
/// — the reaction then runs with a stoichiometry nobody wrote, and the element
/// balance does not see it, because that is taken over the molar `s`. Here the
/// same case is a refusal. `i128` rather than `i64` for the same reason one step
/// up: a product past the width is caught rather than wrapped.
fn nu_exact(s: i32, k: u8, e_r: u8) -> Result<i64> {
    if s == 0 {
        // A spectator — a substance standing identically on both sides — has a
        // coefficient of zero on every scale, so no shift has to be an integer
        // for it. The case is not a curiosity: [`demand_of`] deliberately leaves
        // such a participant out of the raise, because a substance the reaction
        // does not convert has no reason to be moved onto another scale (the
        // module doc, "raising `k_i` is *not* applied to every substance"). Its
        // own ceiling therefore knows nothing about this reaction's `e_r`, and
        // `k < e_r` here is the normal state of affairs rather than an unsolved
        // system. Falling through to the refusal below would reject a legal
        // scenario — water written on both sides of a reaction it moderates —
        // with a message pointing at nothing the author can fix.
        return Ok(0);
    }
    if k < e_r {
        bail!(
            "k = {k} is below the extent exponent {e_r}, so 2^(k - e_r) is not an \
             integer. A solved system does not contain this case: the raise of \
             `CONFIG_SCHEMA.md` section 9 is what removes it"
        );
    }
    let shift = u32::from(k - e_r);
    if shift >= 95 {
        bail!("a shift of {shift} bits leaves any storable width");
    }
    let value = i128::from(s) << shift;
    if value > i128::from(i64::MAX) || value < i128::from(i64::MIN) {
        bail!("nu = {value} does not fit an i64");
    }
    Ok(value as i64)
}

/// `nu_i <= i32::MAX` (ADR-039), naming the pair.
///
/// A guard on this derivation rather than a condition on a scenario: see
/// [`check_nu_against_the_pool`] for the substitution that makes both of them
/// theorems while `e_r` is right, and a live check the moment it is not. The
/// message names both sides because a config whose fault lies in a pair cannot
/// be fixed by looking at one name (`CONFIG_SCHEMA.md` section 10).
fn check_nu_fits_i32(value: i64, substance: &str, reaction: &str) -> Result<()> {
    if value > i64::from(i32::MAX) || value < i64::from(i32::MIN) {
        bail!(
            "nu = {value} for substance `{substance}` in reaction `{reaction}` \
             does not fit the i32 of the reaction table (ADR-039, ADR-041). The \
             width of an *amount* does not help here: nu lives in its own table \
             whatever width the substance got"
        );
    }
    Ok(())
}

/// `nu_i <= beta * typ_conc_i * V_voxel * 2^k_i` (ADR-039), naming the pair.
///
/// The quantum of a turnover may not eat more than `beta` of a typical pool.
/// Worth knowing what it can and cannot catch, because the substitution is short:
///
/// ```text
/// nu_i / (beta * typ_i * V * 2^k_i) = s_i / (beta * typ_i * V * 2^e_r) = x_i / 2^e_r
/// ```
///
/// so while `e_r = ceil(log2 max_j x_j)` the ratio is at most one for every
/// participant, and the same substitution bounds `nu_i` by `beta * 2^28`, which
/// is why [`check_nu_fits_i32`] cannot fire either. Both hold *because* `e_r`
/// came from the scarcest participant — and both fire the moment it did not.
/// That is precisely what they are for: `e_r` taken from the most abundant
/// participant is the error ADR-039 was written to overturn, and this is the
/// only check that sees it without knowing which participant was meant.
fn check_nu_against_the_pool(
    value: i64,
    beta: f64,
    typical_conc: f64,
    v_voxel: f64,
    k: u8,
    substance: &str,
    reaction: &str,
) -> Result<()> {
    let pool = beta * typical_conc * v_voxel * exp2_exact(i32::from(k))?;
    #[allow(clippy::cast_precision_loss)]
    if (value.abs() as f64) > pool {
        bail!(
            "one quantum of extent of reaction `{reaction}` moves {value} units \
             of `{substance}`, over the {pool:e} that beta of its typical pool \
             allows (ADR-039)"
        );
    }
    Ok(())
}

/// ADR-043's second half: the derived tolerance has to stay under the lightest
/// molar mass among the substances with an empty `composition`.
///
/// Without it `eps` would be a number somebody raises once so a config loads,
/// and the mass check would stop meaning anything without a sound. With it, the
/// raise runs into a refusal that names a substance.
fn check_mass_tolerance(config: &Config, reactions: &[DerivedReaction]) -> Result<()> {
    let lightest = config
        .substance
        .iter()
        .filter(|s| s.composition.is_empty())
        .min_by(|a, b| a.molar_mass.total_cmp(&b.molar_mass));
    let Some(lightest) = lightest else {
        return Ok(());
    };
    for reaction in reactions {
        if reaction.mass_tolerance >= lightest.molar_mass {
            bail!(
                "the mass tolerance of reaction `{}` comes out at {:e} g/mol, at \
                 or above the {} g/mol of `{}`, the lightest substance with an \
                 empty composition. Past that point the check cannot tell a \
                 rounded molar mass from a lost molecule, which is the one thing \
                 it exists for (ADR-043)",
                reaction.id,
                reaction.mass_tolerance,
                lightest.molar_mass,
                lightest.id
            );
        }
    }
    Ok(())
}

/// The `[[field]]` record that carries the temperature range (ADR-062).
///
/// A missing record is refused here, and `CONFIG_SCHEMA.md` section 7 predicts a
/// different refusal for the same scenario — enthalpy on the fine grid, 84
/// substeps, over `N_MAX`. That one is not computable: `n` needs the field's
/// `thermal_diffusivity`, which lives in the very record that is missing, and
/// section 7's own table makes the key required. So the scenario that forgets
/// the record is stopped one step earlier than the document expects, and the
/// document is right about the scenario that forgets only `lod` —
/// `enthalpy_at_default_lod_is_rejected_over_n_max` is exactly that case.
fn enthalpy_field(config: &Config) -> Result<&FieldRecord> {
    let mut found = None;
    for (i, record) in config.field.iter().enumerate() {
        if config.field[..i].iter().any(|r| r.id == record.id) {
            bail!("field `{}` is declared twice", record.id);
        }
        if record.id == ENTHALPY_FIELD {
            found = Some(record);
        }
    }
    let Some(record) = found else {
        bail!(
            "no `[[field]] id = \"{ENTHALPY_FIELD}\"` record: the energy scale is \
             derived from its declared temperature range and its heat capacity, \
             and there is nowhere else to take them from (ADR-062)"
        );
    };
    Ok(record)
}

/// What the enthalpy record and the substance registry say about heat transport,
/// once each (ADR-062).
struct Thermal {
    /// `Sum_i typical_conc_i * c_p_i`, J/(m^3*K) — the volumetric heat capacity
    /// of the typical composition.
    c_v: f64,
    /// `alpha * C_v`, W/(m*K).
    conductivity: f64,
    /// `Sum D_i c_i c_p,i / (alpha * Sum c_j c_p,j)`.
    carried: f64,
}

/// Step 6: the two statements ADR-062 turned from prose into checks, and the sum
/// both of them and the `k_E` window are built on.
///
/// One function and one sum. `C_v` is the denominator of the carried-enthalpy
/// estimate and, times `V_cell`, the `C_cell` of the `k_E` system; summing it
/// twice is how two numbers that must agree come apart, which is the reason
/// `process::substeps_and_alpha` returns a pair rather than two functions.
///
/// Neither check is decoration. ADR-028 asserts that thermal diffusion is the
/// fastest transport in the system, and ADR-062 makes the assertion checkable
/// rather than prose; the discarded term of the second is what pays for
/// `Invariant { energy: Conserved }` on `process::Diffuse`, which since ADR-062
/// rests on this check and no longer on the absence of a document.
fn thermal_transport(config: &Config, field: &FieldRecord) -> Result<Thermal> {
    let alpha = thermal_diffusivity_of(field)?;
    if !alpha.is_finite() || alpha <= 0.0 {
        bail!(
            "field `{ENTHALPY_FIELD}` declares thermal_diffusivity = {alpha} \
             m^2/s"
        );
    }

    // `thermal_diffusivity_below_the_fastest_substance_is_rejected`. ADR-028
    // says "two orders faster than molecular" and ADR-062 measures the claim
    // where it is thinnest: against the proton the margin is fifteen times, not
    // a hundred. So the check is the bare inequality — a scenario whose heat
    // crawls slower than its solutes is refused, and one whose margin is thin
    // loads, because the number that decides is the worst substance and not the
    // typical one.
    let mut fastest: Option<(&str, f64)> = None;
    for substance in &config.substance {
        let d = substance.diffusivity;
        if !d.is_finite() || d < 0.0 {
            bail!(
                "substance `{}` declares diffusivity = {d} m^2/s",
                substance.id
            );
        }
        if fastest.is_none_or(|(_, best)| d > best) {
            fastest = Some((substance.id.as_str(), d));
        }
    }
    if let Some((id, d)) = fastest
        && d > alpha
    {
        bail!(
            "field `{ENTHALPY_FIELD}` declares thermal_diffusivity = {alpha:e} \
             m^2/s, below the {d:e} m^2/s of substance `{id}`. Thermal \
             diffusion is the fastest transport in the system (ADR-028); under \
             a scenario where it is not, the enthalpy a diffusing substance \
             carries stops being the negligible term this derivation discards \
             (ADR-062)"
        );
    }

    // The discarded term itself, and it is a ratio of two fluxes: the volume of
    // a cell cancels, so `V_cell` does not appear and `lod` cannot move it.
    let mut c_v = 0.0;
    let mut carried_flux = 0.0;
    let mut largest: Option<(&str, f64)> = None;
    for substance in &config.substance {
        let capacity = substance.typical_conc * substance.c_p;
        c_v += capacity;
        let term = substance.diffusivity * capacity;
        carried_flux += term;
        if largest.is_none_or(|(_, best)| term > best) {
            largest = Some((substance.id.as_str(), term));
        }
    }
    if !c_v.is_finite() || c_v <= 0.0 {
        bail!(
            "the heat capacity of the typical composition comes out at {c_v} \
             J/(m^3*K): there is no temperature to derive an energy scale \
             against (ADR-062)"
        );
    }
    let conductivity = alpha * c_v;
    let carried = carried_flux / conductivity;
    if !carried.is_finite() {
        bail!("the carried-enthalpy estimate comes out at {carried}");
    }
    if carried > CARRIED_ENTHALPY_LIMIT {
        // `enthalpy_carried_by_diffusion_over_five_percent_is_rejected`. The
        // substance is named because on the corpus the whole of the term is one
        // of them — the self-diffusion of water, which moves no enthalpy in a
        // homogeneous solvent anyway — and a reader who does not know which one
        // cannot tell that case from a genuinely coupled one.
        let (id, term) = largest.unwrap_or(("<none>", 0.0));
        bail!(
            "diffusing matter carries {:.2}% of the conductive heat flux, over \
             the {:.0}% at which the term this derivation discards stops being \
             negligible; `{id}` alone carries {:.2}%. Enthalpy does not \
             follow diffusing matter (ADR-062), and `process::Diffuse` declares \
             energy conserved on the strength of this check",
            carried * 100.0,
            CARRIED_ENTHALPY_LIMIT * 100.0,
            term / conductivity * 100.0
        );
    }

    Ok(Thermal {
        c_v,
        conductivity,
        carried,
    })
}

/// The declared `thermal_diffusivity` of the enthalpy record.
///
/// One refusal in one place, because two callers need the number: the substeps
/// of the field (step 9) and the transport checks (step 6). Two messages for one
/// missing key would differ in wording and agree in nothing.
fn thermal_diffusivity_of(field: &FieldRecord) -> Result<f64> {
    field.thermal_diffusivity.ok_or_else(|| {
        anyhow::anyhow!(
            "field `{ENTHALPY_FIELD}` declares no thermal_diffusivity: its \
             substeps are derived from it and from nothing else (ADR-062). The \
             fastest declared `diffusivity` of a substance is not a substitute — \
             the two differ by fifteen times, which is one substep against six"
        )
    })
}

/// Steps 6 and 7: the window of `k_E`, and the width that opens it.
///
/// ```text
/// k_E >= max over reactions ceil( e_r + log2( (0.5/eps) / |dH_r| ) )   nu_E is significant
/// k_E <= min over reactions ( e_r + floor(log2( (2^31-1) / |dH_r| )) ) nu_E fits an i32
/// k_E <= floor(log2( 2^(w-4) / H_max ))                               the field cannot overflow
/// ```
///
/// The width is not chosen, it is tried: `i32` first, `i64` only if the window
/// came out empty. On the registry of SPEC section 2.3 it comes out empty by
/// twenty-six binary orders, and the top of the window is taken — precision is
/// free once the width is paid for (ADR-062). Taken from the bottom instead,
/// every `nu_E` is off by `2^6` with the ledger closing.
fn energy_window(
    config: &Config,
    field: &FieldRecord,
    pending: &[Pending],
    thermal: &Thermal,
) -> Result<DerivedEnergy> {
    let (t_min, t_max) = match (field.t_min, field.t_max) {
        (Some(t_min), Some(t_max)) => (t_min, t_max),
        _ => bail!(
            "field `{ENTHALPY_FIELD}` declares no t_min/t_max: the working range \
             of the enthalpy is what the energy scale is derived from (ADR-062)"
        ),
    };
    if t_min >= t_max || !t_min.is_finite() || !t_max.is_finite() {
        bail!("field `{ENTHALPY_FIELD}` declares t_min {t_min} K and t_max {t_max} K");
    }
    if config.t_ref < t_min || config.t_ref > t_max {
        bail!(
            "T_ref = {} K is outside the declared range [{t_min}, {t_max}] K of \
             the enthalpy field (ADR-062)",
            config.t_ref
        );
    }

    // `V_cell` is the volume of one *coarse* cell of the enthalpy field, and not
    // `V_voxel`. Substituting one for the other moves `k_E` by `3*lod` bits —
    // six at lod 2, a factor of 64 in every `nu_E` — with the energy ledger
    // closing, because the same `nu_E` stands on both sides of it.
    let coarse_dx = config.grid.dx * exp2_exact(i32::from(field.lod))?;
    let v_cell = coarse_dx * coarse_dx * coarse_dx;
    let c_cell = thermal.c_v * v_cell;
    if !c_cell.is_finite() || c_cell <= 0.0 {
        bail!("the heat capacity of a coarse cell comes out at {c_cell} J/K");
    }
    let h_max = c_cell * (t_max - config.t_ref).max(config.t_ref - t_min);
    if !h_max.is_finite() || h_max <= 0.0 {
        bail!("the working range of the enthalpy comes out at {h_max} J");
    }

    let mut lower = 0i32;
    let mut window_lower_set_by = NO_REACTION;
    let mut ceiling = i32::MAX;
    let mut window_upper_set_by = NO_REACTION;
    for (r, p) in pending.iter().enumerate() {
        let magnitude = p.enthalpy.abs();
        if magnitude <= 0.0 || !magnitude.is_finite() {
            // TODO(zero-enthalpy): whether a reaction with dH = 0 is legal, is
            // excluded from both bounds, or is refused is settled by nothing
            // (`CONFIG_SCHEMA.md` section 13, ADR-062 leaves the significance
            // inequality dividing by |dH|). Refused rather than silently
            // dropped: `enthalpy` is a required key with no default, so a zero
            // there is a statement, and a statement nobody has decided how to
            // read is better stopped at the door than answered by a guess.
            bail!(
                "reaction `{}` declares enthalpy = {} J/turnover, and the k_E \
                 system divides by it. Whether such a reaction is legal is \
                 decided by no record (ADR-062)",
                p.id,
                p.enthalpy
            );
        }
        let significant = ceil_log2((0.5 / MASS_EPSILON) / magnitude)? + i32::from(p.e_r);
        if significant > lower || window_lower_set_by == NO_REACTION {
            lower = significant;
            window_lower_set_by = r;
        }
        let fits = floor_log2(f64::from(i32::MAX) / magnitude)? + i32::from(p.e_r);
        if fits < ceiling {
            ceiling = fits;
            window_upper_set_by = r;
        }
    }
    let lower = lower.max(0);

    for width in [Width::Bits32, Width::Bits64] {
        let bits = match width {
            Width::Bits32 => 32,
            Width::Bits64 => 64,
        };
        // The one place the ceiling of ADR-039 is generalised to a width other
        // than 32, and ADR-062 states it as the third inequality.
        let field_ceiling = floor_log2(headroom(bits)? / h_max)?;
        let upper = ceiling.min(field_ceiling);
        if upper < lower {
            continue;
        }
        let k_e = u8::try_from(upper)
            .with_context(|| format!("energy scale exponent {upper} does not fit a u8"))?;
        return Ok(DerivedEnergy {
            k_e,
            width,
            c_cell,
            conductivity: thermal.conductivity,
            carried_by_diffusion: thermal.carried,
            h_max,
            t_min,
            t_max,
            window: (lower as u32, upper as u32),
            window_lower_set_by,
            window_upper_set_by,
            // Both filled by step 7. They are a property of the *weights*, and
            // the weights are a property of the `k_E` this loop is choosing —
            // computing them here would be computing them once per candidate
            // width (ADR-081).
            worst_relative_error: 0.0,
            worst_reaction: NO_REACTION,
            chemical_energy_joules: 0.0,
            field_span_joules: 0.0,
            chemical_energy_bits: 0.0,
            // `2^127/2^k_E`, and `2^127` rather than `i128::MAX` because the
            // difference of one unit is invisible at this scale and the power of
            // two is the number ADR-075 stated and ADR-083 widened. Over the
            // window of ADR-062, `k_E` in [61, 67], the exponent runs 2^60..2^66,
            // every one of them exact in an `f64`.
            counter_ceiling_joules: exp2_exact(127 - i32::from(k_e))?,
        });
    }

    bail!(
        "the k_E window is empty at both storage widths: the significance of \
         nu_E asks for at least 2^{lower} units per joule, while the reactions \
         cap it at 2^{ceiling} and a field holding H_max = {h_max:e} J caps it \
         lower still. An empty window means a nu_E rounded to zero — the \
         reaction proceeds, matter is converted, no heat is released, and the \
         energy ledger closes because the same zero stands on both sides \
         (ADR-062)"
    )
}

/// `w_s = round(enthalpy_formation * 2^(k_E - k))`: what one storage unit of a
/// substance is worth as chemical energy, in the storage units of the enthalpy
/// field (ADR-081).
///
/// The weight of one substance on the **left** side of the energy invariant.
/// Rounded once, at load, and the rounded value is then the truth about that
/// substance for every tick — the same discipline ADR-041 set for `nu_E` and
/// ADR-059 for the composition of the reservoir.
///
/// Exact wherever `k <= k_E`, because the multiplier is then a whole power of
/// two. Where `k > k_E` it rounds, and that is the one place the summed `nu_E`
/// can part company with the declared enthalpy — see
/// [`check_summed_against_declared`], which measures the actual discrepancy
/// rather than its bound.
///
/// # Errors
///
/// If the product is not finite, or does not fit an `i64`. The accumulator it
/// will be multiplied into is an `i128` and the amount it multiplies is up to an
/// `i64`, so a weight past `i64` is a scenario whose chemical energy has no
/// storage at all — and `ledger::DomainSums::add_chemical_energy` takes an
/// `&[i64]`.
fn chemical_weight(enthalpy_formation: f64, k_e: u8, k: u8) -> Result<i64> {
    let exact = enthalpy_formation * exp2_exact(i32::from(k_e) - i32::from(k))?;
    if !exact.is_finite() {
        bail!("enthalpy_formation * 2^(k_E - k) = {exact}");
    }
    // TODO(w-s-rounding): the rounding rule for `w_s` is named by nothing, and
    // this is the second time the corpus has had to pick one. `NUMERIC.md`
    // section 3 fixes halves away from zero for operations on `Q` and stochastic
    // rounding for `xi` (ADR-027); a one-off conversion at load is neither, and
    // `TODO(nu-e-rounding)` covered `nu_E`, which ADR-081 stopped rounding at
    // all. Halves away from zero is taken because it is the rule the document
    // states for every deterministic conversion and because it is the only
    // candidate symmetric in the sign. On the shipped scenario the choice is
    // invisible — `-39700/2` and `-11700/2` are whole and the proton's `h_f` is
    // zero by the single-ion convention — so `floor` written here would show up
    // on the first scenario with an odd formation enthalpy, shifting `nu_E` by
    // up to `Sum|nu_s|/2`. It is a TODO and not a choice made in silence.
    let rounded = exact.round();
    if rounded.abs() > 9.223_372_036_854_775_e18 {
        bail!(
            "w_s = {rounded:e} does not fit the i64 of the weight table: one \
             storage unit of this substance would carry more chemical energy \
             than the ledger can hold for the whole domain (ADR-081)"
        );
    }
    Ok(rounded as i64)
}

/// `nu_E := -Sum_s nu_s * w_s`, the energy coefficient of one reaction in the
/// direction of the field (ADR-081).
///
/// Over the reaction's own **storage-unit** `nu` and never over the molar `s`:
/// the two differ by `2^(k_s - e_r)` per substance, which is a factor of `2^17`
/// on the proton of the shipped scenario alone. Taken over `s` the sum is a
/// plausible number of the wrong magnitude, and every ledger closes over it,
/// because the same `nu_E` stands on both sides.
///
/// The energy record itself is not among `nu` — it is filled from this — so
/// there is no self-reference to guard against.
fn summed_energy_coefficient(nu: &[Nu], weight: &[i64]) -> i64 {
    let mut sum = 0i128;
    for entry in nu {
        sum -= i128::from(entry.value) * i128::from(weight[entry.substance as usize]);
    }
    // Clamped rather than wrapped, and the caller refuses anything past `i32`
    // one line later (ADR-041). An `i64` here holds `2^31 * 2^63` worth of terms
    // with room, and the saturation exists so that an absurd registry reaches a
    // message rather than a wrapped coefficient.
    sum.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}

/// The summed `nu_E` against the enthalpy the scenario declares (ADR-081).
///
/// Two independent statements of one quantity: the scenario's `enthalpy` key,
/// and the sum over the formation enthalpies of the participants at the derived
/// scales. They agree exactly whenever every participant has `k_s <= k_E` — the
/// multiplier is then a whole power of two and nothing rounds — and they part
/// company by up to `0.5 * Sum_{k_s > k_E} |nu_s|` when it does not. On the
/// shipped scenario that bound is `2.4e-3`, two thousand times the tolerance,
/// and the **actual** discrepancy is exactly zero; so what is checked is the
/// discrepancy and not the bound.
///
/// This does **not** replace `enthalpy_agreement` in `config/validate.rs`
/// (ADR-044), and the two are not the same check. That one compares the declared
/// enthalpy against `Sum s_net * dH_f` in J/mol and in `f64`, and catches wrong
/// physics; this one compares integers in storage units, and catches a bit lost
/// in the derivation of the weights. A scenario can pass the first to a part in
/// a million and fail this one by `2.4e-3`.
///
/// Returns the relative discrepancy.
///
/// # Errors
///
/// If the discrepancy is above `MASS_EPSILON`, or if the coefficient does not
/// fit the `i32` of the reaction table (ADR-041).
fn check_summed_against_declared(
    summed: i64,
    enthalpy: f64,
    k_e: u8,
    e_r: u8,
    id: &str,
) -> Result<f64> {
    // The declared enthalpy on the energy scale, in the direction of the field:
    // exothermic is negative in J/turnover and positive here.
    let declared = -enthalpy * exp2_exact(i32::from(k_e) - i32::from(e_r))?;
    if !declared.is_finite() {
        bail!("reaction `{id}`: enthalpy * 2^(k_E - e_r) = {declared}");
    }
    if i128::from(summed).abs() > i128::from(i32::MAX) {
        bail!(
            "reaction `{id}` sums to nu_E = {summed}, which does not fit the i32 \
             of the reaction table (ADR-041, ADR-081)"
        );
    }
    let error = if declared == 0.0 {
        // Unreachable through `derive`: `energy_window` refuses `dH = 0` before
        // this runs (`TODO(zero-enthalpy)`). Guarded anyway, because a division
        // by zero here would answer NaN and NaN compares false against every
        // threshold — a refusal that cannot fire.
        f64::from(summed != 0)
    } else {
        (f64::from(i32::try_from(summed).expect("checked one line up")) - declared).abs()
            / declared.abs()
    };
    if error > MASS_EPSILON {
        bail!(
            "reaction `{id}` declares enthalpy = {enthalpy:e} J/turnover, which \
             is {declared:e} units at k_E = {k_e} and e_r = {e_r}; the formation \
             enthalpies of its participants sum to nu_E = {summed}, a relative \
             discrepancy of {error:e} against a tolerance of {MASS_EPSILON:e}. \
             The two disagree because w_s = round(dH_f * 2^(k_E - k_s)) rounds \
             wherever k_s > k_E (ADR-081). Raise max_conc of the participants \
             whose k_s is above k_E until their scales fall to it — k_i is \
             floor(log2(2^28/(max_conc*V_voxel))) — or move t_min, t_max or \
             T_ref, which are worth about one bit of k_E (ADR-062)"
        );
    }
    Ok(error)
}

/// `Sum_s |w_s| * n_s(max_conc) * n_voxels` against the `i128` of
/// `ledger::DomainSums::energy`, in bits (ADR-081).
///
/// Returns how many bits the estimate occupies; refuses at 127.
///
/// **Computed in `f64` through `log2` and never by forming the `i128`.** The
/// product this bounds is exactly the one that would wrap while being judged:
/// Rust panics on an `i128` overflow in debug only, and `liminis serve` computes
/// the domain sums in **both** profiles, so a check written with the real
/// integers would wrap before it could decide anything.
///
/// Taken at `max_conc` — the largest amount a voxel may legally hold — and not
/// at `typical_conc`, so the answer is a bound on the run and not a snapshot of
/// its first tick. The worst case in the corpus is water of SPEC section 2.3 at
/// 256 cubed: `5.12e11` units per voxel at `w = -4 573 280`, that is `3.93e25`
/// over the domain, 85 bits of 127 with 42 to spare.
// TODO(domain-energy-bound): two things about this bound are settled by no
// record and are chosen here by following ADR-081's own arithmetic. It computes
// the worst corpus case at the **declared** concentration, so that is what is
// used; the alternative is the storage ceiling `2^(w-4)` a voxel can actually
// hold, which is 32 bits more at `i64` and would leave ten bits of margin rather
// than forty-two. And the enthalpy field's own `2*H_max*n_cells` lands in the
// same accumulator and is not added in here — at 76 times smaller it cannot
// change the answer on the corpus, but that is a fact about the corpus and not a
// theorem. Both belong in `DECISIONS.md`.
fn check_domain_chemical_energy(config: &Config, substances: &[DerivedSubstance]) -> Result<f64> {
    let n_voxels =
        f64::from(config.grid.nx) * f64::from(config.grid.ny) * f64::from(config.grid.nz);
    let mut total = 0.0f64;
    for substance in substances {
        // `as f64` on an `i128` amount that `resolve_substance` has already held
        // under `i64::MAX`, and on a weight held under `i64::MAX` one function
        // up. Both lose low bits and neither loses an exponent, which is all this
        // estimate reads.
        total += (substance.chemical_weight as f64).abs() * (substance.amount_at_max as f64);
    }
    total *= n_voxels;
    if !total.is_finite() {
        bail!(
            "the chemical energy of the domain at max_conc overflows an f64 \
             before it can be compared against the i128 of the ledger (ADR-081)"
        );
    }
    if total <= 0.0 {
        // A registry that declares no formation enthalpy at all. Legal, and it
        // makes the left side of the energy invariant the enthalpy field alone,
        // which is what it was before ADR-081.
        return Ok(0.0);
    }
    let bits = total.log2();
    if bits > 127.0 {
        bail!(
            "the chemical energy the domain may hold is {total:e} storage units, \
             which is {bits:.1} bits against the 127 of the ledger's i128 \
             accumulator (`ledger::DomainSums::energy`). It would wrap while \
             being summed, in release without a panic, and the residual would go \
             on closing against a right-hand side that is no longer the truth. \
             Lower max_conc, lower |enthalpy_formation|, or shrink the grid \
             (ADR-081, ADR-083)"
        );
    }
    Ok(bits)
}

/// `Sum_s w_s * n_s(typical)` over the whole domain and `2 * H_max * n_cells`,
/// both in joules (ADR-081).
///
/// The two numbers `Derived::report` prints beside each other, and the ratio
/// between them is the point: on the shipped scenario the chemical term is
/// `1 757 J` against a declared field span of `23.1 J`, seventy-six times
/// larger, and the ratio does not depend on the size of the grid. That is the
/// operational form of the divergence from SPEC section 2.1, which writes the
/// left side of the invariant as an unweighted sum (ADR-032: the spec is not
/// edited, the number is printed).
///
/// The **full** span, `2 * H_max`, because the field runs from `t_min` to
/// `t_max` and `H_max` is the larger of the two halves. `H_max * n_cells` prints
/// a ratio of 152.
fn domain_energy_report(
    config: &Config,
    field: &FieldRecord,
    substances: &[DerivedSubstance],
    energy: &DerivedEnergy,
) -> (f64, f64) {
    let n_voxels =
        f64::from(config.grid.nx) * f64::from(config.grid.ny) * f64::from(config.grid.nz);
    let joules_per_unit = (-f64::from(energy.k_e)).exp2();
    let mut chemical = 0.0f64;
    for substance in substances {
        chemical += (substance.chemical_weight as f64) * (substance.amount_at_typical as f64);
    }
    // The cells of the **enthalpy** grid, `nx >> lod` and so on per axis: the
    // field lives there and `H_max` is the enthalpy of one of its cells
    // (ADR-062). Counted with `V_voxel` in its place the span is sixty-four
    // times too large at `lod = 2` and the ratio comes out at 1.2.
    let cells = f64::from(config.grid.nx >> field.lod)
        * f64::from(config.grid.ny >> field.lod)
        * f64::from(config.grid.nz >> field.lod);
    (
        chemical * n_voxels * joules_per_unit,
        2.0 * energy.h_max * cells,
    )
}

/// Step 9 for one field: its substeps at its own lod (ADR-030, ADR-062).
fn resolve_field(record: &FieldRecord, dt: f64, dx: f64) -> Result<DerivedField> {
    // `dx * 2^lod`, the coarse step. `lod` enters as `(dx*2^lod)^2`, so one step
    // of lod divides `n` by four — the main lever of ADR-030, and the reason the
    // enthalpy field needs six substeps rather than 84.
    let coarse_dx = dx * exp2_exact(i32::from(record.lod))?;
    let diffusivity = if record.id == ENTHALPY_FIELD {
        thermal_diffusivity_of(record)?
    } else {
        // A field that declares no coefficient does not diffuse. The substeps of
        // substance transport are derived per substance from `diffusivity`, in
        // `process/diffuse.rs`, and do not come from a `[[field]]` record.
        record.thermal_diffusivity.unwrap_or(0.0)
    };
    let (substeps, alpha) = substeps_and_alpha(diffusivity, dt, coarse_dx)?;
    debug_assert!(
        substeps <= N_MAX,
        "the bound of ADR-061 belongs to the function that derives the count, \
         and a second check of it here would be a second number"
    );
    Ok(DerivedField {
        id: record.id.clone(),
        lod: record.lod,
        substeps,
        alpha,
        diffusivity,
        coarse_dx,
    })
}

/// ADR-030: skipping ticks multiplies the effective `dt`, which is exactly what
/// the stability condition forbids, so a process touching a diffusive field may
/// not have `every_n_ticks > 1`. ADR-062 makes the enthalpy field a case of it
/// for the first time.
fn check_every_n_ticks(config: &Config, fields: &[DerivedField]) -> Result<()> {
    // **Above the early return below, and that placement is the whole of these
    // two refusals.** The guard after them answers `Ok(())` on a scenario with no
    // diffusive field and no diffusing substance, and a light or velocity refusal
    // written under it would be unreachable on every non-diffusive scenario —
    // passing only because the fixture that tests it happens to diffuse.
    //
    // ADR-086 for the light, ADR-074 for the velocity field, and both for the
    // same reason: the buffers of steps `a` and `b` live in `process::Scratch`
    // because their writer and their reader stand in one tick. Separate the two
    // by a schedule and the buffer becomes inter-tick state of class `Q`, which
    // no snapshot carries — `light` is 8.39 MB per file at 128^3 and
    // `face_courant` 25.17 MB, and a restart applies zeros where the run applied
    // a field.
    for process in &config.process {
        if process.every_n_ticks <= 1 {
            continue;
        }
        if process.id == LIGHT_PROCESS {
            bail!(
                "process `{}` declares every_n_ticks = {}, and the light field it \
                 writes is read inside the same tick: step `a` writes it and step \
                 `i'` folds the absorbed light out of it. A schedule that \
                 separates the two turns a scratch buffer into inter-tick state \
                 of class `Q`, which the snapshot does not carry — 8.39 MB per \
                 file at 128^3 — and a restart would fold a zeroed field \
                 (ADR-086)",
                process.id,
                process.every_n_ticks
            );
        }
        if process.id == VELOCITY_PROCESS {
            bail!(
                "process `{}` declares every_n_ticks = {}, and the Courant \
                 numbers it writes are read inside the same tick: the last stage \
                 of step `b` writes them and step `c` advects with them. At \
                 every_n_ticks = {} step `c` would apply, on {} ticks out of {}, \
                 a face_courant written in some other tick, and a restart would \
                 apply zeros — 25.17 MB per file at 128^3 otherwise (ADR-074, \
                 ADR-086)",
                process.id,
                process.every_n_ticks,
                process.every_n_ticks,
                process.every_n_ticks - 1,
                process.every_n_ticks
            );
        }
    }

    let diffusive: Vec<&str> = fields
        .iter()
        .filter(|f| f.alpha > 0.0)
        .map(|f| f.id.as_str())
        .collect();
    if diffusive.is_empty() && !config.substance.iter().any(|s| s.diffusivity > 0.0) {
        return Ok(());
    }
    // **Below the early return and not above it**, and the placement is a choice
    // between two mistakes rather than an oversight (ADR-082). Section 10 states
    // the ban over the *field* — "forbidden to a process that touches a diffusive
    // field" — so a scenario with no diffusive field and no diffusing substance is
    // outside the rule, and a branch written above the guard would silently widen
    // it to scenarios the rule was never about. The price of standing here is the
    // other mistake, and it is real: on such a scenario pressure at
    // `every_n_ticks = 8` loads. ADR-082 states the rule over the field, so this
    // is where it goes.
    for process in &config.process {
        let moves_a_diffusive_field =
            process.id == DIFFUSION_PROCESS || process.id == PRESSURE_PROCESS;
        if moves_a_diffusive_field && process.every_n_ticks > 1 {
            bail!(
                "process `{}` declares every_n_ticks = {}, and it moves diffusive \
                 fields ({}). Skipping ticks multiplies the effective dt, which \
                 is what the substep count exists to prevent: a rarely updated \
                 diffusive field is unstable by definition, not by oversight \
                 (ADR-030). Pressure falls under the same rule and gets no \
                 acceptance name of its own: the rule is stated over the field, \
                 pressure moves the same amount[] whose substances declare a \
                 diffusivity, and two names on one rule of section 10 would claim \
                 in the register that there are two rules (ADR-082). The product \
                 theta_max*every_n_ticks is not a way round it either — \
                 every_n_ticks enters neither half of the window, because \
                 alpha = Theta/theta_max contains neither dt nor dx",
                process.id,
                process.every_n_ticks,
                if diffusive.is_empty() {
                    "substances".to_string()
                } else {
                    diffusive.join(", ")
                }
            );
        }
    }
    Ok(())
}

/// `2^exp`, exactly.
///
/// Built out of the exponent field rather than computed, because every use below
/// depends on it being exact: a product by a power of two is exact in binary
/// floating point, and that is what makes the comparisons in [`floor_log2`] and
/// [`scale_ceiling`] decide the inequality rather than approximate it.
fn exp2_exact(exp: i32) -> Result<f64> {
    if !(-1022..=1023).contains(&exp) {
        bail!("2^{exp} is outside the range of an f64");
    }
    Ok(f64::from_bits(((exp + 1023) as u64) << 52))
}

/// The largest `k` with `2^k <= x`.
///
/// Not `x.log2().floor()`. IEEE-754 promises bit-exactness for `+`, `-`, `*`,
/// `/` and the square root and nothing at all for `log2` (`NUMERIC.md` section
/// 2 says as much about `exp`/`log`), so on an exact power of two the library
/// may hand back `n - 1`. An off-by-one in `k_i` doubles or halves *every*
/// amount of that substance and fails no conservation test — those compare a
/// field with itself. The library value is used as a seed and then corrected
/// against exact comparisons, which is the only defence there is.
fn floor_log2(x: f64) -> Result<i32> {
    if !x.is_finite() || x <= 0.0 {
        bail!("log2({x}) is not defined");
    }
    if !x.is_normal() {
        bail!(
            "log2({x:e}) of a subnormal is not derived here: the scale it would produce has no exact power of two to sit on"
        );
    }
    let mut k = x.log2().floor() as i32;
    k = k.clamp(-1022, 1023);
    while k > -1022 && exp2_exact(k)? > x {
        k -= 1;
    }
    while k < 1023 && exp2_exact(k + 1)? <= x {
        k += 1;
    }
    Ok(k)
}

/// The smallest `k` with `x <= 2^k`.
fn ceil_log2(x: f64) -> Result<i32> {
    let k = floor_log2(x)?;
    if exp2_exact(k)? == x {
        Ok(k)
    } else {
        Ok(k + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse;
    use proptest::prelude::*;

    // --- fixtures ---------------------------------------------------------
    //
    // Fixtures are TOML constants here rather than files under `configs/`,
    // exactly as in `tests/config_hash.rs`: a scenario file is covered by the CI
    // guard of ADR-020, and a fixture written for a test would drag
    // `WORLD_FORMAT_VERSION` along for a reason that is not a change of
    // semantics.
    //
    // **Numbers marked as placeholders are placeholders.** `partial_molar_volume`,
    // `enthalpy_formation` and every `c_p` but water's are named by no document
    // (`CONFIG_SCHEMA.md` section 13 item 23); they are written because the
    // schema requires them and they are read by nothing here. Do not copy them
    // into `configs/`.

    /// Header: the eco regime of `CONFIG_SCHEMA.md` section 12.
    /// `dx = 1e-4` gives `V_voxel = 1e-12 m^3`, `beta = 2^-6`.
    const HEADER: &str = "\
name = \"derive-fixture\"
dt = 1e0
beta = 1.5625e-2
T_ref = 2.9815e2

[grid]
nx = 64
ny = 64
nz = 64
dx = 1e-4
";

    /// Heat capacity of water, J/(mol*K).
    ///
    /// **Reconstructed, not declared**, in the same way `CONFIG_SCHEMA.md`
    /// section 2 reconstructs the molar masses of P and S from a cross-check:
    /// ADR-062 prints `4.179e6 J/(m^3*K)` for water at 55.5 mol/l, and
    /// `4.179e6 / 55500 = 75.3`. No document prints the number itself (section
    /// 13 item 23), and this is a derivation standing in for a declaration.
    const C_P_WATER: f64 = 75.3;

    /// Placeholder heat capacity for everything else. It is not physics; on the
    /// registries below water carries 99.8% of `C_cell` anyway (ADR-062).
    const C_P_PLACEHOLDER: f64 = 100.0;

    /// One `[[substance]]` record with every required key of section 5 written
    /// out.
    ///
    /// **`enthalpy_formation` is an argument and not a zero, and it is
    /// load-bearing since ADR-081.** It used to be `0e0` for every fixture in
    /// this module, which was legal while `nu_E` was rounded from the declared
    /// enthalpy and stopped being legal the moment `nu_E` became
    /// `-Sum_s nu_s * w_s`: a registry of zero formation enthalpies gives every
    /// reaction `nu_E = 0` against a declared enthalpy that is not, and the load
    /// is refused. So every fixture below is thermochemically consistent, and the
    /// rule that keeps it painless is worth stating once: a weight rounds only
    /// where `k_s > k_E`, so a participant above the energy scale is given
    /// `enthalpy_formation = 0` and every other one an enthalpy that makes the
    /// molar sum exact — and then the summed coefficient equals the declared one
    /// to the unit, on every fixture here.
    #[allow(clippy::too_many_arguments)]
    // Eight arguments against a clippy threshold of seven, and the eighth is
    // `enthalpy_formation`, which ADR-081 made load-bearing. A struct here would
    // buy the lint and cost every call site its shape.
    fn substance(
        id: &str,
        molar_mass: f64,
        typical: f64,
        max: f64,
        diffusivity: f64,
        c_p: f64,
        enthalpy_formation: f64,
        composition: &str,
    ) -> String {
        format!(
            "
[[substance]]
id = \"{id}\"
molar_mass = {molar_mass:e}
typical_conc = {typical:e}
max_conc = {max:e}
partial_molar_volume = 1e-5
settling_radius = 0e0
diffusivity = {diffusivity:e}
c_p = {c_p:e}
enthalpy_formation = {enthalpy_formation:e}
composition = {composition}
"
        )
    }

    /// Formation enthalpies for the four substances of `CONFIG_SCHEMA.md`
    /// section 12, J/mol.
    ///
    /// Aqueous standard values, except sulfate, which is moved by 170 J/mol from
    /// its real `-909270` so that the sum comes out at the round `-8.46e5` the
    /// worked example declares:
    /// `-909100 + 0 - (-39700) - 2*(-11700) = -846000` exactly. The proton is
    /// zero by the single-ion convention, and that is what keeps the summed
    /// coefficient exact — its `k` is above `k_E` on every registry here, so its
    /// weight is the one that would round.
    const H_F_H2S: f64 = -39700.0;
    const H_F_O2: f64 = -11700.0;
    const H_F_SO4: f64 = -909100.0;
    const H_F_PROTON: f64 = 0.0;

    /// Formation enthalpy of liquid water, J/mol (SPEC section 2.3).
    const H_F_WATER: f64 = -285830.0;

    fn reaction(id: &str, enthalpy: f64, inputs: &str, outputs: &str) -> String {
        format!(
            "
[[reaction]]
id = \"{id}\"
enthalpy = {enthalpy:e}
inputs  = {inputs}
outputs = {outputs}

[reaction.rate]
vmax = 1e-6
t_vmax = 298.15
q10 = 2e0
km = {{ }}
"
        )
    }

    /// The `[[field]]` record of section 12: thermal diffusivity of water, the
    /// temperature range of ADR-062.
    fn enthalpy_record(lod: u8, thermal_diffusivity: f64) -> String {
        format!(
            "
[[field]]
id = \"enthalpy\"
lod = {lod}
thermal_diffusivity = {thermal_diffusivity:e}
t_min = 2.7315e2
t_max = 3.2315e2
"
        )
    }

    /// The four substances of `CONFIG_SCHEMA.md` section 12, in its order.
    fn spec_12_substances(so4_max: f64) -> String {
        [
            substance(
                "H2S",
                34.08088,
                0.1,
                10.0,
                1.6e-9,
                C_P_PLACEHOLDER,
                H_F_H2S,
                "{ S = 1 }",
            ),
            substance(
                "O2",
                31.99880,
                0.25,
                1.0,
                2.1e-9,
                C_P_PLACEHOLDER,
                H_F_O2,
                "{}",
            ),
            substance(
                "SO4",
                96.06260,
                28.0,
                so4_max,
                1.0e-9,
                C_P_PLACEHOLDER,
                H_F_SO4,
                "{ S = 1 }",
            ),
            substance(
                "H_ION",
                1.007940,
                1.0e-4,
                1.0e-2,
                9.3e-9,
                C_P_PLACEHOLDER,
                H_F_PROTON,
                "{}",
            ),
        ]
        .concat()
    }

    /// The scenario of `CONFIG_SCHEMA.md` section 12: four substances, the
    /// abiotic oxidation of sulphide, the enthalpy field at lod 2.
    fn spec_12() -> String {
        format!(
            "{HEADER}{}{}{}",
            spec_12_substances(100.0),
            reaction(
                "h2s_oxidation",
                -8.46e5,
                "{ H2S = 1, O2 = 2 }",
                "{ SO4 = 1, H_ION = 2 }"
            ),
            enthalpy_record(2, 1.4e-7)
        )
    }

    /// Water and the proton, and the reaction that holds both.
    ///
    /// **A reduction of the registry to the pair ADR-040 was written for.** The
    /// full registry of SPEC section 2.3 cannot be substituted: `typical_conc`
    /// and `max_conc` exist for four of its fourteen substances
    /// (`CONFIG_SCHEMA.md` section 13). Adding participants can only *raise*
    /// `e_r`, so this proves a weaker statement than the full registry would —
    /// which is why the full-registry twin below is written and ignored.
    ///
    /// Water at 55.5 mol/l = 55500 mol/m^3 (ADR-040); its `max_conc` is named
    /// nowhere, and for the solvent the typical concentration *is* the maximum.
    fn water_and_proton() -> String {
        format!(
            "{HEADER}{}{}{}{}",
            substance(
                "WATER", 18.01528, 55500.0, 55500.0, 2.3e-9, C_P_WATER, H_F_WATER, "{}",
            ),
            substance(
                "H_ION",
                1.007940,
                1.0e-4,
                1.0e-2,
                9.3e-9,
                C_P_PLACEHOLDER,
                H_F_PROTON,
                "{}"
            ),
            reaction(
                "photosynthesis",
                // `-106 * H_F_WATER`, and not the round `4.95e7` this fixture
                // used to declare: the reaction is a reduction of photosynthesis
                // to the water-proton pair, so its enthalpy is whatever the pair
                // makes it, and after ADR-081 that is checked rather than assumed.
                // It moves neither `e_r` nor `k_E`, which is why the reduction is
                // still the one ADR-062 does its arithmetic on.
                3.029_798e7,
                "{ WATER = 106 }",
                "{ H_ION = 13 }"
            ),
            enthalpy_record(2, 1.4e-7)
        )
    }

    /// The two reactions ADR-062 does its arithmetic on, over the substances of
    /// section 12 plus water. Photosynthesis is reduced to the water-proton pair
    /// for the reason given on [`water_and_proton`].
    fn two_reactions() -> String {
        format!(
            "{HEADER}{}{}{}{}{}",
            spec_12_substances(100.0),
            substance(
                "WATER", 18.01528, 55500.0, 55500.0, 2.3e-9, C_P_WATER, H_F_WATER, "{}",
            ),
            reaction(
                "h2s_oxidation",
                -8.46e5,
                "{ H2S = 1, O2 = 2 }",
                "{ SO4 = 1, H_ION = 2 }"
            ),
            reaction(
                "photosynthesis",
                3.029_798e7,
                "{ WATER = 106 }",
                "{ H_ION = 13 }"
            ),
            enthalpy_record(2, 1.4e-7)
        )
    }

    /// The photosynthesis of SPEC section 5, whole, with the molar masses of
    /// SPEC section 2.3 to seven significant figures.
    ///
    /// `typical_conc` and `max_conc` of CO2, N_MIN, P_MIN and BIOMASS are named
    /// by nothing (`CONFIG_SCHEMA.md` section 13 item 23) and are placeholders;
    /// they are chosen so that none of them is scarcer than the proton, which is
    /// what SPEC section 5 and ADR-040 rest on.
    fn photosynthesis(multiplier: i32) -> String {
        let m = multiplier;
        format!(
            "{HEADER}{}{}{}{}{}{}{}{}{}",
            substance(
                "CO2",
                44.00950,
                0.02,
                2.0,
                1.9e-9,
                C_P_PLACEHOLDER,
                0e0,
                "{ C = 1 }"
            ),
            substance(
                "N_MIN",
                18.03846,
                1.0e-3,
                0.1,
                1.6e-9,
                C_P_PLACEHOLDER,
                0e0,
                "{ N = 1 }"
            ),
            substance(
                "P_MIN",
                94.97136,
                2.0e-4,
                0.02,
                0.8e-9,
                C_P_PLACEHOLDER,
                0e0,
                "{ P = 1 }"
            ),
            substance(
                "WATER", 18.01528, 55500.0, 55500.0, 2.3e-9, C_P_WATER, H_F_WATER, "{}",
            ),
            substance(
                "BIOMASS",
                3553.237,
                0.1,
                2.0,
                0e0,
                C_P_PLACEHOLDER,
                // `4.95e7 + 106 * H_F_WATER`, so that the reaction's declared
                // enthalpy is the sum over its participants exactly: the other
                // five carry zero, and water and biomass are both at or below
                // `k_E`, so no weight rounds and the summed `nu_E` comes out at
                // `-792 000 000` to the unit (ADR-081).
                1.920_202e7,
                "{ C = 106, N = 16, P = 1 }"
            ),
            substance(
                "O2",
                31.99880,
                0.25,
                1.0,
                2.1e-9,
                C_P_PLACEHOLDER,
                0e0,
                "{}",
            ),
            substance(
                "H_ION",
                1.007940,
                1.0e-4,
                1.0e-2,
                9.3e-9,
                C_P_PLACEHOLDER,
                H_F_PROTON,
                "{}"
            ),
            reaction(
                "photosynthesis",
                // Scaled with the stoichiometry, and it has to be: `m` turnovers
                // written as one reaction release `m` times the enthalpy, and
                // after ADR-081 the declared number is checked against the sum
                // over the participants rather than merely carried.
                4.95e7 * f64::from(m),
                &format!(
                    "{{ CO2 = {}, N_MIN = {}, P_MIN = {}, WATER = {} }}",
                    106 * m,
                    16 * m,
                    m,
                    106 * m
                ),
                &format!(
                    "{{ BIOMASS = {}, O2 = {}, H_ION = {} }}",
                    m,
                    106 * m,
                    13 * m
                )
            ),
            enthalpy_record(2, 1.4e-7)
        )
    }

    /// A registry whose energy scale fits `i32`: concentrations two orders above
    /// the worked example's and an enthalpy an order larger, which lifts the
    /// bottom of the `k_E` window until the whole of it sits inside 32 bits.
    ///
    /// Extracted so that two tests can use it. The second one needs a scenario
    /// with a `k_E` that is **not** 67, or a report line checked against a
    /// literal would pass on a hard-coded constant.
    fn narrow_energy_scale() -> String {
        format!(
            "{HEADER}{}{}{}",
            [
                substance(
                    "H2S",
                    34.08088,
                    28.0,
                    100.0,
                    1.6e-9,
                    C_P_PLACEHOLDER,
                    0e0,
                    "{ S = 1 }"
                ),
                substance(
                    "O2",
                    31.99880,
                    28.0,
                    100.0,
                    2.1e-9,
                    C_P_PLACEHOLDER,
                    0e0,
                    "{}",
                ),
                substance(
                    "SO4",
                    96.06260,
                    28.0,
                    100.0,
                    1.0e-9,
                    C_P_PLACEHOLDER,
                    // Every substance of this registry sits at `k = 61` against
                    // `k_E = 43`, so every weight rounds: the whole enthalpy of
                    // the reaction is carried by sulfate and its value is a power
                    // of two, `-2^23`, chosen so that `w = -32` is exact and the
                    // summed `nu_E` matches the declared enthalpy to the unit
                    // (ADR-081). Any other number here is refused, and the
                    // refusal is the point of
                    // `summed_energy_coefficient_disagreeing_with_the_declared_enthalpy_is_rejected`.
                    -8.388_608e6,
                    "{ S = 1 }"
                ),
                substance(
                    "H_ION",
                    1.007940,
                    28.0,
                    100.0,
                    9.3e-9,
                    C_P_PLACEHOLDER,
                    0e0,
                    "{}"
                ),
            ]
            .concat(),
            reaction(
                "h2s_oxidation",
                -8.388_608e6,
                "{ H2S = 1, O2 = 2 }",
                "{ SO4 = 1, H_ION = 2 }"
            ),
            enthalpy_record(2, 1.4e-7)
        )
    }

    fn derived(text: &str) -> Derived {
        derive(&parse(text).expect("the fixture must parse")).expect("the fixture must derive")
    }

    fn refusal(text: &str) -> String {
        format!(
            "{:#}",
            derive(&parse(text).expect("the fixture must parse"))
                .expect_err("the fixture must be refused")
        )
    }

    fn substance_named<'a>(d: &'a Derived, id: &str) -> &'a DerivedSubstance {
        d.substances()
            .iter()
            .find(|s| s.id == id)
            .unwrap_or_else(|| panic!("no substance `{id}`"))
    }

    fn reaction_named<'a>(d: &'a Derived, id: &str) -> &'a DerivedReaction {
        d.reactions()
            .iter()
            .find(|r| r.id == id)
            .unwrap_or_else(|| panic!("no reaction `{id}`"))
    }

    fn nu_of(r: &DerivedReaction, d: &Derived, id: &str) -> i64 {
        let s = d
            .substances()
            .iter()
            .position(|it| it.id == id)
            .unwrap_or_else(|| panic!("no substance `{id}`")) as u32;
        let mut found = None;
        for entry in &r.nu {
            if entry.substance == s {
                assert!(
                    found.is_none(),
                    "`{id}` got two nu records in `{}`: netted into one, or the \
                     kernel applies both and moves twice what the reaction says",
                    r.id
                );
                found = Some(entry.value);
            }
        }
        found.unwrap_or_else(|| panic!("`{id}` is not a participant of `{}`", r.id))
    }

    /// Rewrite one passage of a fixture, refusing to be a no-op.
    fn swap(text: &str, from: &str, to: &str) -> String {
        assert_eq!(
            text.matches(from).count(),
            1,
            "the fixture must contain `{from}` exactly once"
        );
        text.replace(from, to)
    }

    // --- the derivation ---------------------------------------------------

    #[test]
    fn extent_exponent_is_derived_from_the_scarcest_participant() {
        // `CONFIG_SCHEMA.md` section 12, where every number is from the corpus.
        // log2(s/(beta*typ*V)) comes out 60.15 for H_ION, 49.18 for H2S, 48.86
        // for O2 and 41.06 for SO4; the ceiling of the largest is 61, the number
        // section 12 prints ("at e_r = 61"). Taken from the most abundant
        // participant, SO4, it would be 42 — nineteen binary orders out.
        let d = derived(&spec_12());
        let rx = reaction_named(&d, "h2s_oxidation");
        assert_eq!(rx.e_r, 61);
        assert_eq!(
            d.substances()[rx.scarcest as usize].id,
            "H_ION",
            "e_r has to come from the participant the reaction runs out of first"
        );

        // The trap this fixture alone cannot see. On these four the scales come
        // out 64/67/61/74, so `k_min` is 61 and *coincides* with `e_r` — one
        // scenario cannot tell the rule of ADR-039 from the `e_r = k_min` of
        // ADR-035, which it overturned. Hence the second case below.
        let scales: Vec<u8> = d.substances().iter().map(|s| s.k).collect();
        assert_eq!(scales, vec![64, 67, 61, 74]);
        assert_eq!(scales.iter().copied().min(), Some(61));

        // Same scenario, SO4 declared at max_conc 50 instead of 100: `k_min`
        // moves to 62 while `e_r` stays 61, because `e_r` never looked at
        // `max_conc` at all.
        let text = format!(
            "{HEADER}{}{}{}",
            spec_12_substances(50.0),
            reaction(
                "h2s_oxidation",
                -8.46e5,
                "{ H2S = 1, O2 = 2 }",
                "{ SO4 = 1, H_ION = 2 }"
            ),
            enthalpy_record(2, 1.4e-7)
        );
        let d = derived(&text);
        let rx = reaction_named(&d, "h2s_oxidation");
        assert_eq!(rx.e_r, 61);
        assert_eq!(
            d.substances().iter().map(|s| s.k).min(),
            Some(62),
            "with SO4 at max_conc 50 the smallest scale is 62"
        );
        assert_ne!(rx.e_r, 62, "e_r is not k_min (ADR-039 over ADR-035)");

        // A property, not a number: doubling the typical concentration of the
        // most abundant participant does not move `e_r` at all, doubling the
        // scarcest one lowers it by exactly one per octave.
        let abundant = swap(&spec_12(), "typical_conc = 2.8e1", "typical_conc = 5.6e1");
        assert_eq!(reaction_named(&derived(&abundant), "h2s_oxidation").e_r, 61);

        let scarce = swap(&spec_12(), "typical_conc = 1e-4", "typical_conc = 2e-4");
        assert_eq!(reaction_named(&derived(&scarce), "h2s_oxidation").e_r, 60);

        // And the participants come from `inputs` *and* `outputs`: the mirror of
        // the reaction, with the scarce participant moved to the left, has to
        // give the same number. Counted over inputs alone, the photosynthesis of
        // SPEC section 5 loses the proton entirely.
        let mirrored = swap(
            &swap(&spec_12(), "enthalpy = -8.46e5", "enthalpy = 8.46e5"),
            "inputs  = { H2S = 1, O2 = 2 }\noutputs = { SO4 = 1, H_ION = 2 }",
            "inputs  = { SO4 = 1, H_ION = 2 }\noutputs = { H2S = 1, O2 = 2 }",
        );
        let d = derived(&mirrored);
        let rx = reaction_named(&d, "h2s_oxidation");
        assert_eq!(rx.e_r, 61);
        assert_eq!(d.substances()[rx.scarcest as usize].id, "H_ION");
    }

    #[test]
    fn storage_width_is_derived_not_declared() {
        // (a) There is no key. A width written in TOML is not a width the loader
        // ignores — it is a load error, and that is checked rather than assumed.
        let declared = swap(&spec_12(), "id = \"H2S\"", "id = \"H2S\"\nwidth = \"i64\"");
        assert!(
            parse(&declared).is_err(),
            "`width` inside [[substance]] has to be an unknown key"
        );

        // (c) The negative control first: this registry comes out entirely
        // 32-bit, so the test below can say "no" as well as "yes".
        let d = derived(&spec_12());
        for s in d.substances() {
            assert_eq!(s.width, Width::Bits32, "{}", s.id);
        }

        // (b) Move `max_conc` of one substance by octaves and watch the width
        // follow. Raising `max_conc` *lowers* the ceiling; while the ceiling is
        // at or above `e_r = 61` the amount at `max_conc` sits just under 2^28,
        // and once the raise has lifted `k` more than three binary orders over
        // the ceiling it stops fitting an i32 (`CONFIG_SCHEMA.md` section 9).
        //
        // The whole sweep, not one point: an implementation that flips to i64
        // and back would pass a single-point check. Monotone, and exactly one
        // crossing.
        let mut widths = Vec::new();
        for octave in 0..=14u32 {
            let text = format!(
                "{HEADER}{}{}{}",
                spec_12_substances(28.0 * f64::from(1u32 << octave)),
                reaction(
                    "h2s_oxidation",
                    -8.46e5,
                    "{ H2S = 1, O2 = 2 }",
                    "{ SO4 = 1, H_ION = 2 }"
                ),
                enthalpy_record(2, 1.4e-7)
            );
            let d = derived(&text);
            let so4 = substance_named(&d, "SO4");
            assert_eq!(
                reaction_named(&d, "h2s_oxidation").e_r,
                61,
                "the sweep must not move e_r, or it is testing two things at once"
            );
            assert_eq!(so4.k, so4.k_ceiling.max(61));
            assert_eq!(so4.raised_to_e_r, so4.k_ceiling < 61);
            if !so4.raised_to_e_r {
                assert!(
                    so4.amount_at_max > (1 << 27) && so4.amount_at_max <= (1 << 28),
                    "the ceiling leaves the amount at max_conc just under 2^28, \
                     octave {octave}: {}",
                    so4.amount_at_max
                );
            }
            widths.push(so4.width);
        }
        let expected: Vec<Width> = (0..=14u32)
            .map(|octave| {
                if octave < 6 {
                    Width::Bits32
                } else {
                    Width::Bits64
                }
            })
            .collect();
        assert_eq!(
            widths, expected,
            "the width has to cross once, at the octave where the raised scale \
             stops fitting an i32, and never cross back"
        );
    }

    #[test]
    fn water_is_stored_in_64_bits_in_the_default_registry() {
        // The worked example of ADR-040, on the pair it was written for.
        let d = derived(&water_and_proton());
        let rx = reaction_named(&d, "photosynthesis");

        // The proton sets the extent exponent, and it stands on the *right*.
        // Counted over inputs alone, e_r falls to about 47, water keeps its
        // ceiling of 52, i64 never happens and no conservation test notices.
        assert_eq!(rx.e_r, 63);
        assert_eq!(d.substances()[rx.scarcest as usize].id, "H_ION");

        let water = substance_named(&d, "WATER");
        assert_eq!(water.k_ceiling, 52, "the flat ceiling of ADR-040, k ~= 52");
        assert_eq!(
            water.k, 63,
            "raised to e_r, which is what makes nu integral"
        );
        assert!(water.raised_to_e_r);
        assert_eq!(water.width, Width::Bits64);
        let units = water.amount_at_max as f64;
        assert!(
            (5.118e11..5.120e11).contains(&units),
            "ADR-040 prints 5.1e11 units of water in one voxel: {units:e}"
        );
        assert!(water.amount_at_max > i128::from(i32::MAX));

        // A test about the derivation, not about water: "everything in i64"
        // would pass the assertions above.
        let proton = substance_named(&d, "H_ION");
        assert_eq!(proton.width, Width::Bits32);
        assert_eq!(proton.k, 74);
        assert!(!proton.raised_to_e_r);

        // And the raise made `nu` integral rather than merely large: at k = e_r
        // the shift is zero and water's coefficient is its molar one.
        assert_eq!(nu_of(rx, &d, "WATER"), -106);
        assert_eq!(nu_of(rx, &d, "H_ION"), 13 * (1 << 11));

        // The other half of the rule, and the half a one-reaction fixture cannot
        // see: the raise is taken over the reactions a substance *converts in*,
        // not over the config. On the two-reaction fixture the two readings
        // differ — SO4 stands in the oxidation alone, at `e_r = 61`, while
        // photosynthesis asks 63 of water and of the proton. Raised everywhere,
        // every stored amount of SO4 would be four times what it is, exactly and
        // conservatively, and the world would be a different one.
        let both = derived(&two_reactions());
        let scales: Vec<u8> = both.substances().iter().map(|s| s.k).collect();
        assert_eq!(scales, vec![64, 67, 61, 74, 63], "H2S O2 SO4 H_ION WATER");
        let raised: Vec<bool> = both.substances().iter().map(|s| s.raised_to_e_r).collect();
        assert_eq!(raised, vec![false, false, false, false, true]);
        assert_eq!(reaction_named(&both, "h2s_oxidation").e_r, 61);
        assert_eq!(reaction_named(&both, "photosynthesis").e_r, 63);
        assert_eq!(
            substance_named(&both, "SO4").k,
            61,
            "SO4 converts in no reaction whose e_r is 63"
        );
    }

    /// The same claim on the whole registry of SPEC section 2.3.
    ///
    /// Ignored, and not because it fails: `typical_conc` and `max_conc` exist
    /// for four of the fourteen substances and `max_conc` of water for none
    /// (`CONFIG_SCHEMA.md` section 13 item 23). The numbers below are
    /// placeholders, so what the test would assert is the arithmetic of its own
    /// fixture. Written now so that the gap is visible, in the shape ADR-061
    /// prescribed for `enthalpy_at_default_lod_is_rejected_over_n_max`.
    #[test]
    #[ignore = "typical_conc/max_conc for ten of the fourteen substances of SPEC 2.3 are named by nothing (CONFIG_SCHEMA.md section 13 item 23)"]
    fn water_is_stored_in_64_bits_on_the_full_spec_registry() {
        let d = derived(&photosynthesis(1));
        assert_eq!(reaction_named(&d, "photosynthesis").e_r, 63);
        assert_eq!(substance_named(&d, "WATER").width, Width::Bits64);
        for id in ["CO2", "N_MIN", "P_MIN", "BIOMASS", "O2", "H_ION"] {
            assert_eq!(substance_named(&d, id).width, Width::Bits32, "{id}");
        }
    }

    #[test]
    fn nu_is_an_exact_integer_shift_and_a_shared_participant_is_netted() {
        // (a) The shift is exact and taken on integers. The proton of the worked
        // example: k = 74 against e_r = 63.
        assert_eq!(nu_exact(13, 74, 63).unwrap(), 13 * 2048);
        let wide = nu_exact(12_345_679, 102, 63).unwrap();
        assert_eq!(i128::from(wide), i128::from(12_345_679) << 39);

        // And the case the floating-point version would answer instead of
        // refusing: at k < e_r the product is a fraction and the `as i64` behind
        // it cuts the fraction off, so the reaction runs with a stoichiometry
        // nobody wrote. The element balance is taken over the molar `s` and does
        // not see it.
        let truncated = (3.0 * 2f64.powi(62 - 63)) as i64;
        assert_eq!(truncated, 1, "1.5 turns into 1 without a word");
        assert!(
            nu_exact(3, 62, 63).is_err(),
            "k below e_r is not a solved system"
        );
        assert!(nu_exact(3, 200, 0).is_err());

        // The refusal names the pair, not one name: a config whose fault lies in
        // a pair cannot be fixed by looking at either half.
        let err = check_nu_fits_i32(1 << 40, "WATER", "photosynthesis")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("WATER") && err.contains("photosynthesis"),
            "{err}"
        );

        // (b) The second inequality of the same refusal, on the corpus: one
        // quantum of extent moves 16384 units of the proton against the 2.95e4
        // that beta of its typical pool allows.
        let d = derived(&spec_12());
        let rx = reaction_named(&d, "h2s_oxidation");
        assert_eq!(nu_of(rx, &d, "H_ION"), 16384);
        let proton = substance_named(&d, "H_ION");
        let pool = 1.5625e-2 * 1.0e-4 * 1.0e-12 * exp2_exact(i32::from(proton.k)).unwrap();
        assert!((2.95e4..2.96e4).contains(&pool), "{pool:e}");
        assert!(f64::from(16384) <= pool);

        // (c) A substance standing on both sides gets one record, netted — and
        // the fixture has *one* reaction, which is the whole point. A spectator
        // is left out of the raise on purpose (`demand_of`, and the theorem in
        // the module doc: a substance the reaction does not convert has no
        // reason to move onto another scale), so nothing lifts water's ceiling
        // of 52 to this reaction's `e_r` of 61 and the shift `2^(k - e_r)` is
        // fractional. `nu` is exactly zero all the same, because zero is zero on
        // every scale. Built on `two_reactions()` instead, the case would be
        // vacuous: photosynthesis raises water to 63 there, and the path this
        // asserts is never taken.
        let text = format!(
            "{HEADER}{}{}{}{}",
            spec_12_substances(100.0),
            substance(
                "WATER", 18.01528, 55500.0, 55500.0, 2.3e-9, C_P_WATER, H_F_WATER, "{}",
            ),
            reaction(
                "h2s_oxidation",
                -8.46e5,
                "{ H2S = 1, O2 = 2, WATER = 1 }",
                "{ SO4 = 1, H_ION = 2, WATER = 1 }"
            ),
            enthalpy_record(2, 1.4e-7)
        );
        let d = derived(&text);
        let rx = reaction_named(&d, "h2s_oxidation");
        assert_eq!(rx.nu.len(), 5, "one record per distinct participant");
        assert_eq!(nu_of(rx, &d, "WATER"), 0, "1 in and 1 out is no conversion");
        let water = substance_named(&d, "WATER");
        assert_eq!(rx.e_r, 61);
        assert_eq!(water.k, 52, "a spectator is not raised to e_r");
        assert!(!water.raised_to_e_r);
        assert!(
            water.k < rx.e_r,
            "if this stops holding the case is no longer the one it was written \
             for: the shift has to be fractional and nu exact anyway"
        );

        // And the turnover is taken over one side and over the netted records:
        // water cancels and does not inflate it.
        let expected = 34.08088 + 2.0 * 31.99880;
        assert!(
            (rx.turnover_mass - expected).abs() < 1e-9,
            "turnover {} against {expected}",
            rx.turnover_mass
        );
    }

    #[test]
    fn mass_tolerance_is_derived_not_declared() {
        // The photosynthesis of SPEC section 5 against the masses of section 2.3.
        let d = derived(&photosynthesis(1));
        let rx = reaction_named(&d, "photosynthesis");
        assert!(
            (rx.turnover_mass - 6958.2134).abs() < 1e-4,
            "turnover mass {} g/mol",
            rx.turnover_mass
        );
        assert!(
            (rx.mass_tolerance - 6.958_213_4e-3).abs() < 1e-9,
            "tolerance {:e} g/mol",
            rx.mass_tolerance
        );

        // The registry of the project itself loads: the residual of SPEC section
        // 5 is 3.8e-4 g/mol against a tolerance of 7.0e-3, eighteen times over.
        let left: f64 = 106.0 * 44.00950 + 16.0 * 18.03846 + 94.97136 + 106.0 * 18.01528;
        let right: f64 = 3553.237 + 106.0 * 31.99880 + 13.0 * 1.007940;
        let residual = (left - right).abs();
        assert!(
            (3.7e-4..3.9e-4).contains(&residual),
            "residual {residual:e}"
        );
        assert!(residual < rx.mass_tolerance);
        assert!(rx.mass_tolerance / residual > 18.0);

        // Derived, not declared, in two ways. Doubling the whole stoichiometry
        // doubles the turnover and the tolerance with it — a declared constant
        // would not have moved.
        let twice = derived(&photosynthesis(2));
        let doubled = reaction_named(&twice, "photosynthesis");
        assert!((doubled.turnover_mass - 2.0 * rx.turnover_mass).abs() < 1e-6);
        assert!((doubled.mass_tolerance - 2.0 * rx.mass_tolerance).abs() < 1e-12);

        // And the key does not exist.
        let declared = swap(
            &photosynthesis(1),
            "[reaction.rate]",
            "mass_tolerance = 1e-3\n\n[reaction.rate]",
        );
        assert!(
            parse(&declared).is_err(),
            "`mass_tolerance` has to be unknown"
        );

        // The second half of ADR-043: the tolerance stays under the lightest
        // molar mass among substances with an empty composition — the proton at
        // 1.00794, with 145 times of margin here. A turnover 145 times larger
        // turns that fraction over, and the refusal names the substance.
        assert!(1.007940 / rx.mass_tolerance > 144.0);
        let err = refusal(&photosynthesis(145));
        assert!(
            err.contains("H_ION"),
            "the refusal must name the substance: {err}"
        );
        assert!(err.contains("tolerance"), "{err}");
    }

    /// `ACCEPTANCE.md`, from ADR-041 and amended by ADR-081.
    ///
    /// **The content changed and the name did not, because the name is what the
    /// document accepts the stage by.** `nu_E` no longer rounds — it is
    /// `-Sum_s nu_s * w_s`, summed out of the formation enthalpies of the
    /// participants — so what this asserts is (a) that it is an integer settled
    /// before the first tick, (b) the numbers of ADR-062 **with the sign
    /// inverted**, because ADR-081 states the coefficient in the direction of
    /// the field, and (c) the discrepancy against the declared enthalpy as a
    /// number. The rounding rule moved with it, from `nu_E` to `w_s`, and (d)
    /// follows it there.
    #[test]
    fn reaction_energy_delta_is_integral_after_load() {
        let d = derived(&two_reactions());
        assert_eq!(d.energy().k_e, 67);

        // (b) The numbers of ADR-062, sign included — and the sign is the whole
        // of ADR-081. `h2s_oxidation` gives heat up, so its coefficient is
        // **positive** in the direction of the enthalpy field: `kernels/fold.rs`
        // adds it, the cell warms, and what the field gains the chemical form of
        // the substances lost. Photosynthesis takes heat in and is negative.
        // Under the old definition both wore the sign of the declared enthalpy
        // and an exothermic reaction cooled its cell — with every ledger closing,
        // because the same `nu_E` stands on both sides of it.
        let h2s = reaction_named(&d, "h2s_oxidation");
        assert_eq!(h2s.e_r, 61);
        assert_eq!(
            h2s.nu_energy, 54_144_000,
            "846000 J/turnover at k_E = 67 and e_r = 61: 846000 * 2^6, positive"
        );
        let photo = reaction_named(&d, "photosynthesis");
        assert_eq!(photo.e_r, 63);
        assert_eq!(photo.nu_energy, -484_767_680, "-106 * w_WATER, negative");
        assert!(
            h2s.nu_energy > 0 && photo.nu_energy < 0,
            "exothermic is positive and endothermic negative in the direction of \
             the field (ADR-081)"
        );

        // (a) It is the sum over the participants and not a second reading of the
        // declared enthalpy, which is what the lazy implementation would be —
        // `-round(dH * 2^(k_E - e_r))`, one sign flipped and nothing defined.
        // The two agree on every scenario whose weights are exact, so the thing
        // that separates them is the arithmetic below and the refusal in
        // `summed_energy_coefficient_disagreeing_with_the_declared_enthalpy_is_rejected`.
        let weights: Vec<i64> = d.chemical_weights();
        let by_hand: i64 = h2s
            .nu
            .iter()
            .map(|entry| -entry.value * weights[entry.substance as usize])
            .sum();
        assert_eq!(by_hand, h2s.nu_energy);
        assert_eq!(
            weights[substance_index(&d, "SO4")],
            -58_182_400,
            "-909100 J/mol at k = 61 and k_E = 67 is -909100 * 2^6"
        );
        assert_eq!(
            weights[substance_index(&d, "H_ION")],
            0,
            "zero by the single-ion convention, which is why the one participant \
             above k_E rounds nothing"
        );

        // (c) The discrepancy is named as a number, and on this fixture it is an
        // exact zero: every participant with a formation enthalpy of its own sits
        // at or below `k_E`, so `w_s = h_f * 2^(k_E - k_s)` is a whole shift.
        assert_eq!(h2s.energy_relative_error, 0.0);
        assert_eq!(photo.energy_relative_error, 0.0);
        assert_eq!(d.energy().worst_relative_error, 0.0);
        assert!(d.report().contains("discrepancy"), "{}", d.report());

        // (d) The rounding rule, on both signs and on the tie — moved from `nu_E`
        // to `w_s`, because that is the only quantity on this path that still
        // rounds. It rounds exactly where `k_s > k_E`, and every one of these is
        // a mutation no conservation test can see: the same weight stands on both
        // sides of the energy ledger, so a weight quantised one unit further from
        // zero closes it exactly. Under `floor` every substance with a negative
        // formation enthalpy — which is the corpus case — would be quantised
        // down, systematically and identically, forever.
        assert_eq!(
            chemical_weight(0.75, 67, 67).unwrap(),
            1,
            "0.75 goes up, not down"
        );
        assert_eq!(
            chemical_weight(-0.75, 67, 67).unwrap(),
            -1,
            "away from zero, not toward it"
        );
        // The tie, where away-from-zero parts from ties-to-even: two is even, so
        // the banker's rule would answer two.
        assert_eq!(
            chemical_weight(2.5, 67, 67).unwrap(),
            3,
            "halves away from zero (`NUMERIC.md` section 3)"
        );
        // And the same thing said through the exponent rather than through a
        // fraction, which is the shape it has in the derivation: a substance two
        // binary orders above the energy scale.
        assert_eq!(chemical_weight(150.0, 72, 74).unwrap(), 38, "37.5 -> 38");
        assert_eq!(chemical_weight(-150.0, 72, 74).unwrap(), -38);
    }

    /// `ACCEPTANCE.md`, section "Refusals" (ADR-081).
    ///
    /// A scenario whose declared enthalpy agrees with the sum over the formation
    /// enthalpies of its participants **in joules per mole** — the check ADR-044
    /// makes and `config/validate.rs` runs — and whose *integer* coefficient
    /// disagrees, because `w_s = round(dH_f * 2^(k_E - k_s))` rounds wherever
    /// `k_s > k_E`.
    ///
    /// **The fixture is the load-bearing part of this test.** The shipped
    /// scenario cannot produce this failure at all: `-39700/2` and `-11700/2` are
    /// whole, the proton's formation enthalpy is zero by the single-ion
    /// convention, and its actual discrepancy is exactly zero. So the failure has
    /// to be built, and it has to be built on an **odd** formation enthalpy
    /// carried by a substance whose scale is above the energy scale — otherwise
    /// the lazy implementation of ADR-081, `nu_E = -round(dH * 2^(k_E - e_r))`
    /// with one sign flipped and nothing defined, passes this test as well as the
    /// real one.
    ///
    /// Here the registry of `CONFIG_SCHEMA.md` section 12 comes out at
    /// `k_E = 72` with the proton at `k = 74`, and the proton is given
    /// `150 J/mol`: `150 * 2^-2` is `37.5`, a genuine half, and the weight rounds
    /// to 38. Sulfate is moved by the same 300 J/mol the two protons of the
    /// reaction add, so the **molar** sum is still exactly the declared `-8.46e5`
    /// and only the integers disagree.
    #[test]
    fn summed_energy_coefficient_disagreeing_with_the_declared_enthalpy_is_rejected() {
        // The proton at 150 J/mol, and sulfate moved by 2 * 150 so that
        // `-909400 + 2*150 + 39700 + 2*11700 = -846000` to the last digit.
        let odd_proton = format!(
            "{HEADER}{}{}{}{}{}{}",
            substance(
                "H2S",
                34.08088,
                0.1,
                10.0,
                1.6e-9,
                C_P_PLACEHOLDER,
                H_F_H2S,
                "{ S = 1 }",
            ),
            substance(
                "O2",
                31.99880,
                0.25,
                1.0,
                2.1e-9,
                C_P_PLACEHOLDER,
                H_F_O2,
                "{}",
            ),
            substance(
                "SO4",
                96.06260,
                28.0,
                100.0,
                1.0e-9,
                C_P_PLACEHOLDER,
                -909_400.0,
                "{ S = 1 }",
            ),
            substance(
                "H_ION",
                1.007940,
                1.0e-4,
                1.0e-2,
                9.3e-9,
                C_P_PLACEHOLDER,
                150.0,
                "{}",
            ),
            reaction(
                "h2s_oxidation",
                -8.46e5,
                "{ H2S = 1, O2 = 2 }",
                "{ SO4 = 1, H_ION = 2 }"
            ),
            enthalpy_record(2, 1.4e-7)
        );

        // The molar identity ADR-044 checks holds exactly, so this scenario is
        // not refused by the check that already existed — which is the whole
        // point of writing a second one.
        let molar = -909_400.0 + 2.0 * 150.0 - H_F_H2S - 2.0 * H_F_O2;
        assert_eq!(molar, -846_000.0, "the fixture must pass the ADR-044 check");

        let err = refusal(&odd_proton);
        assert!(err.contains("h2s_oxidation"), "{err}");
        assert!(
            err.contains("54_144_000".replace('_', "").as_str())
                || err.contains("1732599808")
                || err.contains("nu_E"),
            "the refusal must print both numbers: {err}"
        );
        assert!(
            err.contains("max_conc"),
            "the way out has to be named: {err}"
        );

        // And the same registry with the proton back at zero loads, so what was
        // refused is the rounding and not the shape of the fixture. `spec_12`
        // differs from the above in exactly two numbers.
        let d = derived(&spec_12());
        assert_eq!(d.energy().k_e, 72, "the proton sits above the energy scale");
        assert_eq!(substance_named(&d, "H_ION").k, 74);
        assert_eq!(substance_named(&d, "H_ION").chemical_weight, 0);
        let rx = reaction_named(&d, "h2s_oxidation");
        assert_eq!(rx.nu_energy, 1_732_608_000, "846000 * 2^(72 - 61)");
        assert_eq!(rx.energy_relative_error, 0.0);
    }

    /// `ACCEPTANCE.md`, section "Refusals" (ADR-081, ADR-083).
    ///
    /// The chemical energy the domain may hold is an `i128` in
    /// `ledger::DomainSums`, and a registry can declare its way past it. Refused
    /// at load rather than wrapped at run time — Rust panics on an `i128`
    /// overflow in debug only, and `liminis serve` computes the domain sums in
    /// **both** build profiles, so a wrap in release leaves a residual closing
    /// against a right-hand side that is no longer the truth.
    ///
    /// **Reachable only by declaration.** The estimate per voxel is
    /// `|h_f| * max_conc * V_voxel * 2^k_E`, and both of its factors are already
    /// held under `i64` — the weight by `chemical_weight` and the amount by
    /// `resolve_substance` — so a registry of ordinary numbers cannot come near
    /// it. The fixture below therefore declares an absurd pair: a formation
    /// enthalpy of `-2^84 J/mol` on a substance whose scale is raised to 96 by
    /// its reaction's extent exponent.
    #[test]
    fn chemical_energy_of_the_domain_past_the_ledger_accumulator_is_rejected() {
        // `A + X -> B + Y`: two pairs, and each is there for one reason.
        //
        // `A` and `B` are a trace pair at `1e-15 mol/m^3`, and their only job is
        // to push the extent exponent up to 96 — a substance cannot do that for
        // itself, because ADR-039 caps `max_conc/typical_conc` at `2^14` and a
        // scale raised to its own `e_r` therefore holds about `2^20` units, not
        // enough to overflow anything. `X` and `Y` ride that exponent instead:
        // their scales are raised from 61 to 96 and one voxel of them holds
        // `7.9e18` units, an `i64` all but full.
        //
        // Their formation enthalpies are `-2^84 J/mol` and `-2^84 + 2^50`, both
        // whole powers of two on the energy scale, so every weight is exact and
        // the summed coefficient matches the declared enthalpy to the unit: this
        // fixture is refused by the domain bound and by nothing else. The
        // difference `2^50` is what the reaction declares, and it cancels in the
        // sum — `nu_E` is `-2^21`, comfortably inside an `i32`, while each weight
        // is `2^55`.
        let absurd = |max_conc: f64| -> String {
            format!(
                "{HEADER}{}{}{}{}{}{}{}",
                substance(
                    "WATER", 18.01528, 55500.0, 55500.0, 2.3e-9, C_P_WATER, H_F_WATER, "{}",
                ),
                substance(
                    "A",
                    50.0,
                    1.0e-15,
                    1.6e-11,
                    1.0e-9,
                    C_P_PLACEHOLDER,
                    0e0,
                    "{ N = 1 }",
                ),
                substance(
                    "B",
                    50.0,
                    1.0e-15,
                    1.6e-11,
                    1.0e-9,
                    C_P_PLACEHOLDER,
                    0e0,
                    "{ N = 1 }",
                ),
                substance(
                    "X",
                    100.0,
                    max_conc / 16384.0,
                    max_conc,
                    1.0e-9,
                    C_P_PLACEHOLDER,
                    -1.934_281_311_383_406_7e25,
                    "{ C = 1 }",
                ),
                substance(
                    "Y",
                    100.0,
                    max_conc / 16384.0,
                    max_conc,
                    1.0e-9,
                    C_P_PLACEHOLDER,
                    -1.934_281_311_270_816_7e25,
                    "{ C = 1 }",
                ),
                reaction(
                    "x_to_y",
                    1.125_899_906_842_624e15,
                    "{ A = 1, X = 1 }",
                    "{ B = 1, Y = 1 }"
                ),
                enthalpy_record(2, 1.4e-7)
            )
        };

        // The passing side first, so that what the refusal below reports is the
        // one number that moved. The same absurd enthalpies at a thousandth of
        // the concentration fit, and the loader says by how much.
        let d = derived(&absurd(1.0e-3));
        assert_eq!(d.energy().k_e, 67);
        assert_eq!(substance_named(&d, "X").k, 96, "raised to e_r");
        assert_eq!(reaction_named(&d, "x_to_y").e_r, 96);
        assert_eq!(reaction_named(&d, "x_to_y").nu_energy, -2_097_152);
        assert_eq!(reaction_named(&d, "x_to_y").energy_relative_error, 0.0);
        assert!(
            (120.0..121.0).contains(&d.energy().chemical_energy_bits),
            "the passing fixture stands at {} bits",
            d.energy().chemical_energy_bits
        );

        let err = refusal(&absurd(1.0));
        assert!(err.contains("127"), "the bound has to be named: {err}");
        assert!(
            err.contains("max_conc"),
            "the way out has to be named: {err}"
        );

        // The worst case the corpus itself can reach, and it is nowhere near:
        // water of SPEC section 2.3 at 256 cubed, `5.12e11` units per voxel at
        // `w = -4 573 280`. Eighty-five bits of a hundred and twenty-seven, with
        // forty-two to spare — the number ADR-081 prints, recomputed here off a
        // registry rather than quoted.
        let spec_2_3_at_256 = swap(
            &swap(
                &swap(&water_and_proton(), "nx = 64", "nx = 256"),
                "ny = 64",
                "ny = 256",
            ),
            "nz = 64",
            "nz = 256",
        );
        let big = derived(&spec_2_3_at_256);
        let water = substance_named(&big, "WATER");
        assert_eq!(water.k, 63);
        assert_eq!(water.chemical_weight, -4_573_280);
        assert_eq!(water.amount_at_max, 511_897_148_045);
        assert!(
            (85.0..85.1).contains(&big.energy().chemical_energy_bits),
            "the worst corpus case stands at {} bits, not 85",
            big.energy().chemical_energy_bits
        );
        assert!(
            big.report().contains("bits of chemical energy"),
            "{}",
            big.report()
        );
    }

    /// Where a substance stands in the declaration order.
    fn substance_index(d: &Derived, id: &str) -> usize {
        d.substances()
            .iter()
            .position(|s| s.id == id)
            .unwrap_or_else(|| panic!("no substance `{id}`"))
    }

    #[test]
    fn enthalpy_storage_width_is_derived_not_declared() {
        // The structural half, and it runs. The window is solved by trying the
        // widths in turn: a scenario whose lower bound is above the field's
        // ceiling at w = 32 gets i64, not a refusal and not a truncation.
        let d = derived(&two_reactions());
        let energy = d.energy();
        assert_eq!(energy.width, Width::Bits64);
        assert_eq!(energy.window, (61, 67));
        assert_eq!(energy.k_e, 67, "the top of the window, not the bottom");
        assert!(u32::from(energy.k_e) == energy.window.1);

        // And *who* set the bottom: the reaction with the smallest quantum of
        // energy, not the one with the largest enthalpy. Photosynthesis carries
        // 4.95e7 J against 8.46e5 and asks for k_E >= 57; the oxidation of
        // sulphide asks for 61. The exact twin of "the quantum is set by the
        // scarcest participant" (ADR-062).
        assert_eq!(
            d.reactions()[energy.window_lower_set_by].id,
            "h2s_oxidation"
        );
        assert_eq!(
            d.reactions()[energy.window_upper_set_by].id,
            "photosynthesis"
        );

        // A config that fits w = 32 stays at w = 32 — otherwise the test above
        // would pass on a derivation that answers i64 to everything.
        let d = derived(&narrow_energy_scale());
        assert_eq!(d.energy().width, Width::Bits32);
        assert_eq!(d.energy().k_e, 43);
        assert_eq!(d.energy().window, (39, 43));

        // There is no key for it either.
        let declared = swap(
            &two_reactions(),
            "id = \"enthalpy\"\nlod = 2",
            "id = \"enthalpy\"\nwidth = \"i64\"\nlod = 2",
        );
        assert!(parse(&declared).is_err());
    }

    /// The numeric half of the same claim, on the registry of SPEC section 2.3.
    ///
    /// Ignored: `c_p` is declared for no substance in the corpus
    /// (`CONFIG_SCHEMA.md` section 13 item 23), and without it there is no
    /// `C_cell`, no `H_max` and no third inequality. Water's is reconstructed
    /// above from ADR-062's `4.179e6 J/(m^3*K)`, which is a reconstruction and
    /// not a declaration; the other thirteen would be invented outright.
    #[test]
    #[ignore = "c_p is declared for no substance in the corpus (CONFIG_SCHEMA.md section 13 item 23)"]
    fn enthalpy_storage_width_on_the_full_spec_registry() {
        let d = derived(&photosynthesis(1));
        let energy = d.energy();
        assert!(
            (2.6e-4..2.8e-4).contains(&energy.c_cell),
            "C_cell = {:e} J/K against the 2.680e-4 of ADR-062",
            energy.c_cell
        );
        assert!((6.6e-3..6.8e-3).contains(&energy.h_max));
        assert_eq!(energy.width, Width::Bits64);
        assert_eq!(energy.k_e, 67);
    }

    #[test]
    fn enthalpy_substeps_are_derived_from_thermal_diffusivity() {
        // The record of `CONFIG_SCHEMA.md` section 12: thermal_diffusivity of
        // water 1.4e-7 m^2/s, dt = 1 s, dx = 1e-4 m, lod = 2 so the coarse step
        // is 4e-4 m. 6*D*dt/dx^2 = 5.25, so six substeps — ADR-030's "six" stops
        // being a quotation.
        let d = derived(&spec_12());
        assert_eq!(d.fields().len(), 1);
        let field = &d.fields()[0];
        assert_eq!(field.id, "enthalpy");
        assert_eq!(field.lod, 2);
        assert_eq!(field.substeps, 6);
        assert!((field.alpha - 5.25 / 36.0).abs() < 1e-12, "{}", field.alpha);
        assert!(
            field.alpha <= 1.0 / 6.0,
            "alpha {} over the limit",
            field.alpha
        );

        // lod enters as (dx*2^lod)^2, so n falls by four per step of lod: 84,
        // 21, 6. Not as dx^2/2^lod, and not as a factor on n.
        let coarser = derived(&swap(&spec_12(), "lod = 2", "lod = 1"));
        assert_eq!(coarser.fields()[0].substeps, 21);

        // At lod = 0 it is 84 and not 96: sixteen multiplies 5.25 *before* the
        // ceiling, not after. ADR-045 and `CONFIG_SCHEMA.md` section 7 print 96
        // and ADR-062 corrects them. Both numbers are over N_MAX, so the refusal
        // cannot tell them apart — the wrong one would survive inside the
        // message of the refusal and inside any lod that fell under the bound.
        let err = refusal(&swap(&spec_12(), "lod = 2", "lod = 0"));
        assert!(err.contains("84"), "the refusal must print n: {err}");
        assert!(
            !err.contains("96"),
            "16*ceil(5.25), not ceil(16*5.25): {err}"
        );

        // The coefficient is the field's `thermal_diffusivity` and not the
        // fastest `diffusivity` of a substance. Both are in m^2/s and both are
        // in this fixture, so the two have to be pulled apart in both
        // directions: moving the fastest substance leaves the field where it
        // was, moving the field's own key moves it. Nineteen is the sediment of
        // ADR-062, `alpha ~= 5e-7`, and it is a corpus number rather than an
        // invented one.
        let slower_proton = derived(&swap(
            &spec_12(),
            "diffusivity = 9.3e-9",
            "diffusivity = 9.3e-11",
        ));
        assert_eq!(slower_proton.fields()[0].substeps, 6);
        let sediment = derived(&swap(
            &spec_12(),
            "thermal_diffusivity = 1.4e-7",
            "thermal_diffusivity = 5e-7",
        ));
        assert_eq!(sediment.fields()[0].substeps, 19);

        // Taking the proton's 9.3e-9 for the field would give one substep
        // instead of six, and a field at one substep does not blow up — it walks
        // into a chessboard over a hundred thousand ticks. It is not reachable:
        // at that coefficient diffusing matter carries more heat than the field
        // conducts, and the check of ADR-062 stops it before the substeps are
        // counted at all.
        let err = refusal(&swap(
            &spec_12(),
            "thermal_diffusivity = 1.4e-7",
            "thermal_diffusivity = 9.3e-9",
        ));
        assert!(err.contains("carries"), "{err}");

        // ADR-030's other half, which ADR-062 gives the enthalpy field as its
        // first case: skipping ticks multiplies the effective dt.
        let skipping = format!(
            "{}\n[[process]]\nid = \"diffusion\"\nenabled = true\nevery_n_ticks = 2\n",
            spec_12()
        );
        let err = refusal(&skipping);
        assert!(err.contains("every_n_ticks"), "{err}");
        assert!(
            err.contains("enthalpy"),
            "the message must name the field: {err}"
        );
    }

    /// `ACCEPTANCE.md`, from ADR-061. ADR-061 prescribed `#[ignore]` while the
    /// diffusion coefficient of the enthalpy field was named nowhere; ADR-062
    /// closed that in the same batch, so it runs from the first day.
    #[test]
    fn enthalpy_at_default_lod_is_rejected_over_n_max() {
        // The scenario that forgets `lod` gets enthalpy on the fine grid, where
        // n is 84 rather than six — 896 passes over a cell instead of one
        // (ADR-062). Refused, rather than growing sixteen times more expensive
        // without a word.
        let forgotten = swap(&spec_12(), "lod = 2\n", "");
        let err = refusal(&forgotten);
        assert!(err.contains("enthalpy"), "the field has to be named: {err}");
        assert!(err.contains("84"), "its n has to be named: {err}");
        assert!(err.contains("64"), "the bound has to be named: {err}");
        // ceil(log4(84/64)) = 1: one step of coarsening fixes it, and the
        // message says which one rather than listing both exits of ADR-030.
        assert!(err.contains("lod"), "the fix has to be named: {err}");
    }

    #[test]
    fn thermal_diffusivity_below_the_fastest_substance_is_rejected() {
        // ADR-028 asserts that thermal diffusion is the fastest transport in the
        // system; ADR-062 turns the assertion into this inequality, measured
        // where it is thinnest. On the corpus the margin over the proton is
        // fifteen times and over a typical solute a hundred and forty.
        let d = derived(&spec_12());
        let fastest: f64 = 9.3e-9;
        assert!(
            (1.4e-7 / fastest - 15.0).abs() < 1.0,
            "the margin of ADR-062 is fifteen times, not a hundred"
        );
        assert_eq!(d.fields()[0].substeps, 6);

        // A scenario whose heat crawls two orders slower than its solutes. With
        // neither check it derives cleanly and reports one substep, a number
        // that looks like every other; what it describes is a world where the
        // term this derivation discards is the larger one. Both checks see it,
        // and this one has to answer first, because it is the one that names the
        // substance the author has to look at.
        let err = refusal(&swap(
            &spec_12(),
            "thermal_diffusivity = 1.4e-7",
            "thermal_diffusivity = 1e-11",
        ));
        assert!(
            err.contains("H_ION"),
            "the refusal must name the substance that beats it: {err}"
        );
        assert!(err.contains("thermal_diffusivity"), "{err}");

        // The direction matters and is easy to write backwards: a *faster* field
        // is legal, and refusing it would reject the sediment of ADR-062 along
        // with every scenario whose solutes are slow.
        let faster = derived(&swap(
            &spec_12(),
            "thermal_diffusivity = 1.4e-7",
            "thermal_diffusivity = 5e-7",
        ));
        assert_eq!(faster.fields()[0].substeps, 19);
    }

    #[test]
    fn enthalpy_carried_by_diffusion_over_five_percent_is_rejected() {
        // ADR-062 decided that enthalpy does not follow diffusing matter, and
        // paid for the decision with this estimate of the term it discards:
        // `Sum D_i c_i c_p,i / (alpha * Sum c_j c_p,j)`. On the corpus registry
        // it is 1.64%, and the whole of it is the self-diffusion of water, which
        // in a homogeneous solvent moves no enthalpy at all.
        let d = derived(&two_reactions());
        let carried = d.energy().carried_by_diffusion;
        assert!(
            (0.0163..0.0166).contains(&carried),
            "ADR-062 prints 1.64%: {}%",
            carried * 100.0
        );

        // The side cross-check of the same record, and the reason it is
        // reported rather than only compared: `alpha * C_v` is the thermal
        // conductivity of water, 0.585 W/(m*K). A `thermal_diffusivity` wrong by
        // an order of magnitude passes both inequalities here and is visible in
        // this number.
        assert!(
            (0.58..0.59).contains(&d.energy().conductivity),
            "ADR-062 prints 0.585 W/(m*K): {}",
            d.energy().conductivity
        );

        // Above the threshold it is a refusal. `1e-8` is over the fastest
        // declared `diffusivity` of this registry, so the check above passes and
        // this one is the only thing between the scenario and a world where
        // `process::Diffuse` declares energy conserved while a tenth of the heat
        // travels with the matter.
        let text = swap(
            &spec_12(),
            "thermal_diffusivity = 1.4e-7",
            "thermal_diffusivity = 1e-8",
        );
        assert!(derived(&spec_12()).energy().carried_by_diffusion < 0.05);
        let err = refusal(&text);
        assert!(err.contains("10.1"), "the share has to be a number: {err}");
        assert!(err.contains('%'), "{err}");
        assert!(
            err.contains("SO4"),
            "the largest contributor has to be named: {err}"
        );

        // And the estimate is a ratio of two fluxes, so the volume of a cell
        // cancels: `lod` cannot move it. Written with `V_cell` in one half only,
        // it would move by 2^(3*lod) and the threshold would mean nothing.
        let coarser = derived(&swap(&two_reactions(), "lod = 2", "lod = 1"));
        assert!((coarser.energy().carried_by_diffusion - carried).abs() < 1e-15);
    }

    #[test]
    fn a_duplicate_id_in_any_section_is_refused() {
        // Referential integrity, `CONFIG_SCHEMA.md` section 10: an `id` is
        // unique within its section. The substance and the field are refused
        // because a second record still takes an index and a lane; the reaction
        // is the expensive one, and it had no check at all until this one.
        let twice = format!(
            "{HEADER}{}{}{}{}",
            spec_12_substances(100.0),
            reaction(
                "h2s_oxidation",
                -8.46e5,
                "{ H2S = 1, O2 = 2 }",
                "{ SO4 = 1, H_ION = 2 }"
            ),
            reaction(
                "h2s_oxidation",
                -8.46e5,
                "{ H2S = 1, O2 = 2 }",
                "{ SO4 = 1, H_ION = 2 }"
            ),
            enthalpy_record(2, 1.4e-7)
        );
        let err = refusal(&twice);
        assert!(err.contains("h2s_oxidation"), "{err}");
        assert!(
            err.contains("ADR-027"),
            "the price is a shared stream of random numbers, and the message \
             has to say so: {err}"
        );

        // The same two records with the second one renamed derive, so the
        // refusal is about the id and not about two reactions over one set of
        // substances. Two reactions with one id are legal TOML and legal against
        // the schema; what they are not is two reactions.
        let renamed = format!(
            "{HEADER}{}{}{}{}",
            spec_12_substances(100.0),
            reaction(
                "h2s_oxidation",
                -8.46e5,
                "{ H2S = 1, O2 = 2 }",
                "{ SO4 = 1, H_ION = 2 }"
            ),
            reaction(
                "h2s_oxidation_in_the_dark",
                -8.46e5,
                "{ H2S = 1, O2 = 2 }",
                "{ SO4 = 1, H_ION = 2 }"
            ),
            enthalpy_record(2, 1.4e-7)
        );
        assert!(parse(&twice).is_ok(), "the duplicate has to reach derive");
        assert_eq!(derived(&renamed).reactions().len(), 2);

        // The other two sections, which had checks and no test.
        let substance_twice = format!(
            "{HEADER}{}{}{}{}",
            spec_12_substances(100.0),
            substance(
                "O2",
                31.99880,
                0.25,
                1.0,
                2.1e-9,
                C_P_PLACEHOLDER,
                H_F_O2,
                "{}",
            ),
            reaction(
                "h2s_oxidation",
                -8.46e5,
                "{ H2S = 1, O2 = 2 }",
                "{ SO4 = 1, H_ION = 2 }"
            ),
            enthalpy_record(2, 1.4e-7)
        );
        assert!(refusal(&substance_twice).contains("O2"));

        let field_twice = format!("{}{}", spec_12(), enthalpy_record(2, 1.4e-7));
        assert!(refusal(&field_twice).contains("enthalpy"));
    }

    #[test]
    fn the_folded_representability_condition_is_not_the_refusal() {
        // (a) The folded condition of ADR-042, with the multiplication:
        // s_i * max_conc_j / typ_conc_i <= 2^28 * beta, which at beta = 2^-6 is
        // 2^22 and not 2^34. On the pair water-proton of the photosynthesis:
        // 13 * 55500 / 1e-4 = 7.2e9 = 2^32.75, missing 2^22 by 2^10.75 — the
        // "miss of 2^11" ADR-040 prints. Written with a division the threshold
        // becomes 2^34, the pair passes, and ADR-040's number stops adding up.
        let beta = 1.5625e-2;
        let folded = 13.0 * 55500.0 / 1.0e-4;
        let correct = 2f64.powi(28) * beta;
        let inverted = 2f64.powi(28) / beta;
        assert!((correct - 2f64.powi(22)).abs() < 1.0);
        assert!((inverted - 2f64.powi(34)).abs() < 1.0);
        let miss = (folded / correct).log2();
        assert!((10.7..10.8).contains(&miss), "miss of 2^{miss}");
        assert!(folded < inverted, "the inverted form lets the pair through");

        // (b) And the registry loads anyway. The folded form is a rule for a
        // reader and not a condition of refusal (ADR-042); written as one it
        // would reject the project's own registry, and it would take
        // `water_is_stored_in_64_bits_in_the_default_registry` down with it.
        // What is checked is k_i and e_r — and the refusal comes only when the
        // raised amount fits neither width.
        let d = derived(&water_and_proton());
        assert_eq!(substance_named(&d, "WATER").width, Width::Bits64);
    }

    // --- the two logarithms -----------------------------------------------

    #[test]
    fn log2_floor_and_ceil_are_exact_on_powers_of_two() {
        // The only defence against an off-by-one in `k_i` or `e_r`. Such an
        // error doubles or halves every amount of a substance and fails no
        // conservation test — those compare a field with itself.
        for exponent in -60..=60 {
            let x = exp2_exact(exponent).unwrap();
            assert_eq!(floor_log2(x).unwrap(), exponent, "2^{exponent}");
            assert_eq!(ceil_log2(x).unwrap(), exponent, "2^{exponent}");

            // And the neighbours on either side, where the two functions part.
            let above = f64::from_bits(x.to_bits() + 1);
            assert_eq!(floor_log2(above).unwrap(), exponent);
            assert_eq!(ceil_log2(above).unwrap(), exponent + 1);
            let below = f64::from_bits(x.to_bits() - 1);
            assert_eq!(floor_log2(below).unwrap(), exponent - 1);
            assert_eq!(ceil_log2(below).unwrap(), exponent);
        }

        // Refused rather than wrapped, on everything that is not a positive
        // normal number. `world/registry.rs` names this debt outright: an
        // `as u8` on the loader's side turns a negative exponent into a large
        // positive one in silence.
        for x in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::MIN_POSITIVE / 2.0] {
            assert!(floor_log2(x).is_err(), "{x:e}");
            assert!(ceil_log2(x).is_err(), "{x:e}");
        }
        assert!(exp2_exact(1024).is_err());
        assert!(exp2_exact(-1023).is_err());
    }

    proptest! {
        /// The invariants, over every positive normal `f64` in the range the
        /// derivation can reach: `2^k <= x < 2^(k+1)` and `2^(k-1) < x <= 2^k`,
        /// both checked against exact powers of two rather than against another
        /// call to `log2`.
        #[test]
        fn log2_floor_and_ceil_bracket_their_argument(x in 1e-300f64..1e300f64) {
            let floor = floor_log2(x).unwrap();
            prop_assert!(exp2_exact(floor).unwrap() <= x);
            prop_assert!(x < exp2_exact(floor + 1).unwrap());

            let ceil = ceil_log2(x).unwrap();
            prop_assert!(x <= exp2_exact(ceil).unwrap());
            prop_assert!(exp2_exact(ceil - 1).unwrap() < x);

            prop_assert!(ceil == floor || ceil == floor + 1);
        }
    }

    // --- refusals ---------------------------------------------------------

    #[test]
    fn a_scenario_outside_the_domain_of_the_formulas_is_refused() {
        // Every one of these passes the schema and none of them is a physical
        // scenario; `CONFIG_SCHEMA.md` section 10 lists them under "domain of
        // definition", where they have no test name of their own.
        let zero_typical = swap(&spec_12(), "typical_conc = 1e-4", "typical_conc = 0e0");
        assert!(refusal(&zero_typical).contains("typical_conc"));

        // max_conc under typical_conc passes all forty refusals: the ratio is
        // below one and so below 2^14.
        let inverted = swap(&spec_12(), "max_conc = 1e-2", "max_conc = 1e-5");
        assert!(refusal(&inverted).contains("max_conc"));

        // `substance_dynamic_range_over_2e14_is_rejected`.
        let wide = swap(&spec_12(), "max_conc = 1e-2", "max_conc = 1e2");
        assert!(refusal(&wide).contains("2^14"), "{}", refusal(&wide));

        // A reaction whose declared enthalpy is exactly zero: the significance
        // inequality divides by it and no record says whether that is legal.
        let zero_enthalpy = swap(&spec_12(), "enthalpy = -8.46e5", "enthalpy = 0e0");
        assert!(refusal(&zero_enthalpy).contains("ADR-062"));

        // T_ref outside the declared range, which is where H_max comes from.
        let outside = swap(&spec_12(), "t_max = 3.2315e2", "t_max = 2.9e2");
        assert!(refusal(&outside).contains("T_ref"));

        // And the scenario with no enthalpy field at all: there is nowhere else
        // to take a temperature range from.
        let no_field = swap(&spec_12(), "id = \"enthalpy\"", "id = \"light\"");
        assert!(refusal(&no_field).contains("enthalpy"));
    }

    #[test]
    fn the_report_names_every_derived_quantity() {
        let d = derived(&two_reactions());
        let report = d.report();
        for expected in [
            "V_voxel",
            "substance WATER",
            "i64",
            "reaction h2s_oxidation",
            "e_r = 61",
            "mass tolerance",
            "k_E = 67",
            "window [61, 67]",
            "field enthalpy",
            "6 substeps",
        ] {
            assert!(report.contains(expected), "missing `{expected}`:\n{report}");
        }
    }

    #[test]
    fn load_reports_the_energy_counter_ceiling_in_joules() {
        // ADR-075 and ADR-083: the ceiling of an energy channel counter is
        // `2^127/2^k_E`, and at `k_E = 67` that is `2^60 J = 1.15e18 J`. What the
        // report owes the reader is the number, in joules, beside the substep
        // counts.
        //
        // **Three literals move together or not at all**, and this is the third:
        // `counter_ceiling_joules` computes `exp2_exact(127 - k_e)`, `report`
        // prints the formula, and the line below recomputes the expectation with
        // an exponent of its own. That is deliberate — a test that read the
        // constant back would be an identity — and it is also the trap: leave
        // this at 63 and the "fix" is to bend the test to a report that says
        // 62.5 mJ against a counter holding 1.15e18 J, with CI green.
        //
        // Two scenarios with **different** `k_E`, and the expectation computed
        // rather than written down: `k_E` is derived and depends on the scenario,
        // so a test on one fixture is green on a hard-coded `0.0625`.
        let wide = derived(&two_reactions());
        let narrow = derived(&narrow_energy_scale());
        assert_ne!(
            wide.energy().k_e,
            narrow.energy().k_e,
            "both fixtures derive the same k_E; the claim below is a constant"
        );

        for d in [&wide, &narrow] {
            let k_e = i32::from(d.energy().k_e);
            let expected = 2.0f64.powi(127 - k_e);
            assert_eq!(d.energy().counter_ceiling_joules, expected);
            let printed = format!("{expected:e}");
            assert!(
                d.report().contains(&printed),
                "the report has to print the counter ceiling `{printed}` J at \
                 k_E = {k_e}:\n{}",
                d.report()
            );
        }
    }

    /// [`two_reactions`] with the lid trading against a reservoir ten kelvin
    /// below `T_ref`, which is the arrangement ADR-084 measured on the shipped
    /// scenario.
    ///
    /// The composition is the shipped one plus water, and water is what makes it
    /// a fixture rather than a curiosity: it carries 99.9% of the heat capacity,
    /// so a reservoir without it would have a conductance three orders too small
    /// and every number below would be about rounding.
    fn venting(faces: &str) -> String {
        swap(
            &two_reactions(),
            "dx = 1e-4\n",
            &format!(
                "dx = 1e-4\n\n[boundary]\n{faces}\n\n[boundary.reservoir]\n\
                 t_out = 2.8815e2\nk_ex = 1e-5\n\
                 conc_out = {{ H2S = 0e0, O2 = 2.5e-1, SO4 = 2.8e1, \
                 H_ION = 1e-4, WATER = 5.55e4 }}\n"
            ),
        )
    }

    #[test]
    fn load_reports_the_lid_credit_per_tick_and_the_counter_margin_in_bits() {
        // ADR-084. The sink of S0 is the built `exchange` face, and the record
        // refuses to hide its price behind a load-time refusal — a world whose
        // lid trades against a `t_out` unequal to `T_ref` is nearly every venting
        // world worth writing, including the only shipped one. So the loader
        // prints what that trade costs the counter, and the two rules beside it
        // refuse only the lit scenarios that cannot work at all.
        //
        // **The margin is in bits and not in ticks**, which is the half of the
        // name that carries an argument. After ADR-083 the counter is `i128`, and
        // `1.4e18` units a tick against `2^127` is `1.2e20` ticks — no declared
        // world has an overflow tick at all, and the whole integral of the
        // transient stops near seventy bits regardless. A line printing "the tick
        // the counter overflows on" would print a number that does not exist,
        // which is the decoy ADR-083 killed a whole acceptance name for.
        let d = derived(&venting("z_min = \"closed\""));
        let reservoir = d.reservoir().expect("the fixture declares a reservoir");

        // Recomputed from the fixture rather than read back from the struct, and
        // not quoted from ADR-084 either — that record's numbers are the shipped
        // scenario's registry, not this one's.
        //   sum(conc_out * c_p) = 0*100 + 0.25*100 + 28*100 + 1e-4*100
        //                       + 55500*75.3
        //                       = 4 181 975.01 J/(m^3*K)
        //   conductance         = 1e-5 * that = 41.8197501 W/(m^2*K)
        //   ceiling             = conductance * (323.15 - 288.15)
        //                       = 1463.6912535 W/m^2
        let conductance = 1.0e-5 * (25.0 + 2800.0 + 0.01 + 55500.0 * C_P_WATER);
        assert!((reservoir.conductance - conductance).abs() < 1e-9);
        assert!((reservoir.flux_ceiling - conductance * 35.0).abs() < 1e-9);

        // The credit is `alpha_ex * |H_out| * cells`, and every factor is checked
        // rather than the product: at `lod = 2` a 64-cubed grid has 16^2 = 256
        // coarse cells on a face, and `alpha_ex` is at the **coarse** `dx`, where
        // `1e-5 * 1 / 4e-4 = 0.025` — a quarter of the 0.1 the fine-grid
        // `alpha_ex` beside it carries. Taking the fine one would inflate this by
        // four while leaving the sign, the residual and every conservation test
        // untouched.
        assert_eq!(reservoir.alpha_ex, 0.1);
        let expected = 0.025 * (reservoir.enthalpy_out as f64).abs() * 256.0;
        assert_eq!(reservoir.lid_credit_per_tick, expected);
        assert!(
            reservoir.enthalpy_out < 0,
            "a colder reservoir draws heat out"
        );

        // A **bound**, and on the safe side of the number a run produces: the gap
        // decays across the substeps inside the tick, so the run credits less.
        // Stated as an inequality against the pessimal alternative — crediting
        // the whole ghost enthalpy of every face cell every tick — because the
        // run's own figure is not reproducible at load.
        assert!(reservoir.lid_credit_per_tick < (reservoir.enthalpy_out as f64).abs() * 256.0);

        // The area counted is the exchanging faces, not one face. A second
        // venting face doubles the traffic, and nothing else about the scenario
        // moves — which is the mistake a single hard-coded lid would hide on
        // every non-cubic or side-venting world.
        let both = derived(&venting("z_min = \"exchange\""));
        let both = both.reservoir().expect("the fixture declares a reservoir");
        assert_eq!(
            both.lid_credit_per_tick,
            2.0 * reservoir.lid_credit_per_tick
        );

        // And the report says all of it, with the margin in bits: `2.5e18` a tick
        // is about 61.1 bits, so the margin against 127 is about 65.9.
        let report = d.report();
        let bits = 127.0 - reservoir.lid_credit_per_tick.log2();
        assert!((61.0..70.0).contains(&bits), "{bits}");
        for expected in [
            format!("{:e}", reservoir.conductance),
            format!("{:e}", reservoir.flux_ceiling),
            format!("{}", reservoir.enthalpy_out),
            format!("{:e}", reservoir.lid_credit_per_tick),
            format!("{bits:.1} bits of margin"),
        ] {
            assert!(
                report.contains(&expected),
                "missing `{expected}`:\n{report}"
            );
        }
    }

    #[test]
    fn load_reports_the_ticks_to_the_declared_temperature_ceiling() {
        // ADR-076. Two numbers, **printed apart**: how many ticks the declared
        // *mean* irradiance takes to cross the declared temperature range if it
        // is absorbed whole in one coarse cell, and how many times the peak
        // instantaneous multiplier exceeds the mean.
        //
        // Apart, and that is the point of the name. Dividing the first by the
        // second reads as a conservative estimate and is not one: the peak is a
        // ratio of instantaneous rates and not a time, and the multiplier has
        // unit mean by construction, so the crossing takes the same number of
        // ticks whatever the modulation is.
        //
        // Called through `derive` directly. The blanket refusal of `i_surface > 0`
        // is gone (ADR-084), but this fixture declares no `[boundary.reservoir]`
        // while `z_max` defaults to `exchange`, so `validate` refuses it twice
        // over — once for the reservoir it owes and once as a lit scenario with
        // no sink. Giving it one would change `C_cell` and every scale below.
        //
        // The peak is specified against `A_N * max_n max(0, sin(2*pi*n/N))` and
        // not against `pi`: at `N = 4` — legal under this record's own minimum of
        // three ticks — the samples are `0, 1, 0, 0`, `A_4 = 4`, and the peak is
        // exactly 4.0, which is 27% above `pi`.
        let text = swap(
            &two_reactions(),
            "[[field]]",
            "[[process]]\nid = \"light\"\nenabled = true\ni_surface = 1.0e3\n\
             daily_fraction = 1.0\ndaily_period = 4.0\n\n[[field]]",
        );
        let d = derived(&text);
        let light = d.light().expect("an enabled, declared light derives");

        // `A_4 = 4 / (0 + 1 + 0 + 0) = 4`, and the peak of the samples is
        // `4 * 1 = 4`.
        assert_eq!(light.daily_period_ticks, 4);
        assert_eq!(light.daily_norm, 4.0);
        assert_eq!(light.peak_multiplier, 4.0);
        assert!(
            light.peak_multiplier > std::f64::consts::PI,
            "the peak is taken over the samples, not over the envelope"
        );

        // The crossing, from already-derived quantities: `H_max` joules against
        // `i_surface * (dx*2^lod)^2 * dt` joules a tick.
        let coarse_dx = d.enthalpy_field().coarse_dx;
        let per_tick = light.i_surface * coarse_dx * coarse_dx * 1.0;
        assert_eq!(light.ticks_to_ceiling, d.energy().h_max / per_tick);

        let report = d.report();
        for expected in ["ticks to", "peak", "units_per_intensity"] {
            assert!(report.contains(expected), "missing `{expected}`:\n{report}");
        }

        // Printed apart: the two numbers are not one product.
        assert!(
            !report.contains(&format!(
                "{:e}",
                light.ticks_to_ceiling / light.peak_multiplier
            )),
            "the report divides the crossing by the peak, which is a ratio of \
             rates and not a time:\n{report}"
        );
    }

    #[test]
    fn the_derivation_does_not_depend_on_the_order_it_ran_in() {
        // Two derivations of one config are equal, and the scales do not depend
        // on which reaction was visited first: `k_i` takes the maximum of `e_r`
        // over every reaction the substance converts in, and a single pass over
        // substances would make that a property of the file order.
        let a = derived(&two_reactions());
        let b = derived(&two_reactions());
        assert_eq!(a, b);

        let swapped = swap(
            &two_reactions(),
            "[[reaction]]\nid = \"h2s_oxidation\"",
            "[[reaction]]\nid = \"zzz_h2s_oxidation\"",
        );
        let d = derived(&swapped);
        for id in ["H2S", "O2", "SO4", "H_ION", "WATER"] {
            assert_eq!(
                substance_named(&d, id).k,
                substance_named(&a, id).k,
                "the scale of {id} moved when a reaction was renamed"
            );
        }
    }

    #[test]
    fn the_declarations_handed_to_the_registry_carry_the_derived_pair() {
        use crate::world::Registry;

        let d = derived(&two_reactions());
        let decls = d.decls();
        assert_eq!(decls.len(), d.substances().len());
        for (decl, derived) in decls.iter().zip(d.substances()) {
            assert_eq!(decl.id, derived.id);
            assert_eq!(decl.k, derived.k);
            assert_eq!(decl.width, derived.width);
        }
        let registry = Registry::new(&decls).unwrap();
        assert_eq!(registry.lanes(Width::Bits64), 1, "water, and water alone");
        assert_eq!(registry.lanes(Width::Bits32), 4);
    }
}
