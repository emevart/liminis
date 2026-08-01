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

use super::{Config, Field as FieldRecord, Reaction, Substance};
use crate::process::{N_MAX, substeps_and_alpha};
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

/// The process that moves diffusive fields, as `CONFIG_SCHEMA.md` section 12
/// spells it.
// TODO(process-roster): which process touches which field is not in the corpus.
// The registry of processes is closed and lives under `process/` (ADR-065), and
// it is not written; section 12 names five ids and does not say what each one
// reads. So the ban of ADR-030 on `every_n_ticks > 1` is applied to the one id
// the corpus does name for transport. When the roster exists, this becomes a
// lookup of the fields a process declares in `reads`.
const DIFFUSION_PROCESS: &str = "diffusion";

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
}

/// One substance after the derivation (ADR-039, ADR-040).
#[derive(Clone, Debug, PartialEq, Eq)]
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
    /// `round(enthalpy * 2^(k_E - e_r))`, rounded once and before the first
    /// tick (ADR-041).
    pub nu_energy: i64,
    /// `|nu_E - exact| / |exact|`, the error that rounding induced.
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
    /// The largest relative error `nu_E` rounding induced, over all reactions.
    pub worst_relative_error: f64,
    /// Which reaction that error belongs to.
    pub worst_reaction: usize,
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
                "reaction {}: e_r = {} (scarcest {}), nu_E = {}, relative error {:e}\n",
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
        out.push_str(&format!(
            "energy: k_E = {}, {width}, window [{}, {}], C_cell = {:e} J/K, H_max = {:e} J, \
             worst relative error {:e}\n",
            self.energy.k_e,
            self.energy.window.0,
            self.energy.window.1,
            self.energy.c_cell,
            self.energy.h_max,
            self.energy.worst_relative_error
        ));
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
        substances.push(
            resolve_substance(
                substance,
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

    // 6. and 7. The energy scale, and `nu_E` on it.
    let field = enthalpy_field(config)?;
    let thermal = thermal_transport(config, field)?;
    let energy = energy_window(config, field, &pending, &thermal)?;
    for (r, p) in reactions.iter_mut().zip(&pending) {
        let (nu_energy, error) = quantize_enthalpy(p.enthalpy, energy.k_e, p.e_r)?;
        r.nu_energy = nu_energy;
        r.energy_relative_error = error;
    }

    // 9. Substeps, per field and at its own lod.
    let mut fields = Vec::with_capacity(config.field.len());
    for record in &config.field {
        fields.push(
            resolve_field(record, config.dt, config.grid.dx)
                .with_context(|| format!("field `{}`", record.id))?,
        );
    }
    check_every_n_ticks(config, &fields)?;

    Ok(Derived {
        v_voxel,
        substances,
        reactions,
        energy,
        fields,
    })
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

/// Steps 3 and 4 for one substance: the raise, and the width it forces.
fn resolve_substance(
    substance: &Substance,
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
        let mut worst_relative_error = 0.0;
        let mut worst_reaction = NO_REACTION;
        for (r, p) in pending.iter().enumerate() {
            let (_, error) = quantize_enthalpy(p.enthalpy, k_e, p.e_r)?;
            if error > worst_relative_error || worst_reaction == NO_REACTION {
                worst_relative_error = error;
                worst_reaction = r;
            }
        }
        return Ok(DerivedEnergy {
            k_e,
            width,
            c_cell,
            conductivity: thermal.conductivity,
            carried_by_diffusion: thermal.carried,
            h_max,
            window: (lower as u32, upper as u32),
            window_lower_set_by,
            window_upper_set_by,
            worst_relative_error,
            worst_reaction,
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

/// `nu_E = round(dH * 2^(k_E - e_r))` and the relative error that rounding
/// induced.
///
/// Rounded **once**, at load, and the rounded value is declared to be the true
/// enthalpy of the reaction (ADR-041). Left as an `f64` in the reaction table
/// and rounded at every application, it would bring back the independent
/// rounding of energy that ADR-026 and ADR-027 rejected twice and ADR-041
/// removed a third time — a residual accumulating one quantum per application.
///
/// The sign travels with it. The bounds of the window are taken over `|dH|`, and
/// a sign lost there turns an exothermic reaction endothermic under a ledger
/// that closes.
fn quantize_enthalpy(enthalpy: f64, k_e: u8, e_r: u8) -> Result<(i64, f64)> {
    let exact = enthalpy * exp2_exact(i32::from(k_e) - i32::from(e_r))?;
    if !exact.is_finite() {
        bail!("dH * 2^(k_E - e_r) = {exact}");
    }
    // TODO(nu-e-rounding): the rounding rule for `nu_E` is named by nothing.
    // `NUMERIC.md` section 3 fixes halves away from zero for operations on `Q`
    // and stochastic rounding for `xi` (ADR-027); this is neither — it is a
    // one-off conversion at load. Halves away from zero is taken because it is
    // the rule the document states for every deterministic conversion, and
    // because it is symmetric in the sign, which the stochastic rule is not
    // reproducible in. One bit of the enthalpy of every reaction rests on this,
    // so it is a TODO and not a choice made in silence.
    let rounded = exact.round();
    if rounded.abs() > f64::from(i32::MAX) {
        bail!("nu_E = {rounded:e} does not fit the i32 of the reaction table (ADR-041)");
    }
    let error = if exact == 0.0 {
        0.0
    } else {
        (rounded - exact).abs() / exact.abs()
    };
    Ok((rounded as i64, error))
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
    })
}

/// ADR-030: skipping ticks multiplies the effective `dt`, which is exactly what
/// the stability condition forbids, so a process touching a diffusive field may
/// not have `every_n_ticks > 1`. ADR-062 makes the enthalpy field a case of it
/// for the first time.
fn check_every_n_ticks(config: &Config, fields: &[DerivedField]) -> Result<()> {
    let diffusive: Vec<&str> = fields
        .iter()
        .filter(|f| f.alpha > 0.0)
        .map(|f| f.id.as_str())
        .collect();
    if diffusive.is_empty() && !config.substance.iter().any(|s| s.diffusivity > 0.0) {
        return Ok(());
    }
    for process in &config.process {
        if process.id == DIFFUSION_PROCESS && process.every_n_ticks > 1 {
            bail!(
                "process `{}` declares every_n_ticks = {}, and it moves diffusive \
                 fields ({}). Skipping ticks multiplies the effective dt, which \
                 is what the substep count exists to prevent: a rarely updated \
                 diffusive field is unstable by definition, not by oversight \
                 (ADR-030)",
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
    fn substance(
        id: &str,
        molar_mass: f64,
        typical: f64,
        max: f64,
        diffusivity: f64,
        c_p: f64,
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
enthalpy_formation = 0e0
composition = {composition}
"
        )
    }

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
                "{ S = 1 }",
            ),
            substance("O2", 31.99880, 0.25, 1.0, 2.1e-9, C_P_PLACEHOLDER, "{}"),
            substance(
                "SO4",
                96.06260,
                28.0,
                so4_max,
                1.0e-9,
                C_P_PLACEHOLDER,
                "{ S = 1 }",
            ),
            substance(
                "H_ION",
                1.007940,
                1.0e-4,
                1.0e-2,
                9.3e-9,
                C_P_PLACEHOLDER,
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
            substance("WATER", 18.01528, 55500.0, 55500.0, 2.3e-9, C_P_WATER, "{}"),
            substance(
                "H_ION",
                1.007940,
                1.0e-4,
                1.0e-2,
                9.3e-9,
                C_P_PLACEHOLDER,
                "{}"
            ),
            reaction(
                "photosynthesis",
                4.95e7,
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
            substance("WATER", 18.01528, 55500.0, 55500.0, 2.3e-9, C_P_WATER, "{}"),
            reaction(
                "h2s_oxidation",
                -8.46e5,
                "{ H2S = 1, O2 = 2 }",
                "{ SO4 = 1, H_ION = 2 }"
            ),
            reaction(
                "photosynthesis",
                4.95e7,
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
                "{ C = 1 }"
            ),
            substance(
                "N_MIN",
                18.03846,
                1.0e-3,
                0.1,
                1.6e-9,
                C_P_PLACEHOLDER,
                "{ N = 1 }"
            ),
            substance(
                "P_MIN",
                94.97136,
                2.0e-4,
                0.02,
                0.8e-9,
                C_P_PLACEHOLDER,
                "{ P = 1 }"
            ),
            substance("WATER", 18.01528, 55500.0, 55500.0, 2.3e-9, C_P_WATER, "{}"),
            substance(
                "BIOMASS",
                3553.237,
                0.1,
                2.0,
                0e0,
                C_P_PLACEHOLDER,
                "{ C = 106, N = 16, P = 1 }"
            ),
            substance("O2", 31.99880, 0.25, 1.0, 2.1e-9, C_P_PLACEHOLDER, "{}"),
            substance(
                "H_ION",
                1.007940,
                1.0e-4,
                1.0e-2,
                9.3e-9,
                C_P_PLACEHOLDER,
                "{}"
            ),
            reaction(
                "photosynthesis",
                4.95e7,
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
            &spec_12(),
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
            substance("WATER", 18.01528, 55500.0, 55500.0, 2.3e-9, C_P_WATER, "{}"),
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

    #[test]
    fn reaction_energy_delta_is_integral_after_load() {
        // Two corpus reactions and one synthetic. The synthetic one exists
        // because both corpus enthalpies land exactly on the derived scale, so
        // the error they induce is zero and proves nothing. Its enthalpy is
        // chosen so that `dH * 2^(k_E - e_r)` is 79_012_307.75 *exactly* — the
        // product of an f64 by a power of two is exact — and a fractional part
        // over one half is what tells the rounding rules apart.
        let text = format!(
            "{}{}",
            two_reactions(),
            reaction(
                "synthetic_fractional",
                1.234_567_308_593_75e6,
                "{ H2S = 1, O2 = 2 }",
                "{ SO4 = 1, H_ION = 2 }"
            )
        );
        let d = derived(&text);
        assert_eq!(d.energy().k_e, 67);

        // (b) The numbers of ADR-062, sign included. The sign is lost easily —
        // the window is computed over |dH| — and lost, an exothermic reaction
        // becomes endothermic under a ledger that still closes.
        let h2s = reaction_named(&d, "h2s_oxidation");
        assert_eq!(h2s.e_r, 61);
        assert_eq!(h2s.nu_energy, -54_144_000, "-8.46e5 * 2^6");
        let photo = reaction_named(&d, "photosynthesis");
        assert_eq!(photo.e_r, 63);
        assert_eq!(photo.nu_energy, 792_000_000, "4.95e7 * 2^4");
        assert!(h2s.nu_energy < 0 && photo.nu_energy > 0);

        // (a) Rounded once, and before the first tick. The behaviour, not the
        // type: a coefficient kept as a real and rounded at every application
        // gives a different sum as soon as it has a fractional part.
        let synthetic = reaction_named(&d, "synthetic_fractional");
        let exact: f64 = 1.234_567_308_593_75e6 * 64.0;
        assert_eq!(exact, 79_012_307.75, "the fixture has to be exact");
        assert_eq!(
            synthetic.nu_energy, 79_012_308,
            "halves away from zero; `floor` and `trunc` both answer 79_012_307 \
             here, and the literal is written out rather than recomputed as \
             `exact.round()`, which would assert the implementation against \
             itself"
        );
        let applications = 1_000_000i64;
        assert_ne!(
            applications * synthetic.nu_energy,
            (applications as f64 * exact).round() as i64,
            "if this holds, the fixture no longer has a fractional part and the \
             assertion below is vacuous"
        );

        // (c) The error is named as a number, and it is bounded by the
        // significance inequality that set the bottom of the window.
        assert_eq!(h2s.energy_relative_error, 0.0);
        assert_eq!(photo.energy_relative_error, 0.0);
        let expected = (synthetic.nu_energy as f64 - exact).abs() / exact.abs();
        assert!((synthetic.energy_relative_error - expected).abs() < 1e-18);
        assert!(synthetic.energy_relative_error <= 0.5 / (synthetic.nu_energy as f64).abs());
        assert!(d.energy().worst_relative_error <= MASS_EPSILON);
        assert!(d.energy().worst_relative_error > 0.0);
        assert_eq!(d.energy().worst_reaction, 2);
        assert!(d.report().contains("relative error"), "{}", d.report());

        // (d) The rule itself, on both signs and on the tie. Every one of these
        // is a mutation no conservation test can see: the same `nu_E` stands on
        // both sides of the energy ledger, so an enthalpy quantised one unit
        // further from zero at every application closes it exactly (ADR-041).
        // Under `floor` every exothermic reaction — which is the corpus case —
        // would be quantised down, systematically and identically, forever.
        let (positive, _) = quantize_enthalpy(79_012_307.75 / 64.0, 67, 61).unwrap();
        assert_eq!(positive, 79_012_308, "0.75 goes up, not down");
        let (negative, _) = quantize_enthalpy(-79_012_307.75 / 64.0, 67, 61).unwrap();
        assert_eq!(negative, -79_012_308, "away from zero, not toward it");
        assert_eq!(positive, -negative, "symmetric in the sign, which is why");
        // The tie, where away-from-zero parts from ties-to-even: two is even, so
        // the banker's rule would answer two.
        let (tie, _) = quantize_enthalpy(2.5 / 64.0, 67, 61).unwrap();
        assert_eq!(tie, 3, "halves away from zero (`NUMERIC.md` section 3)");
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
        let narrow = format!(
            "{HEADER}{}{}{}",
            [
                substance(
                    "H2S",
                    34.08088,
                    28.0,
                    100.0,
                    1.6e-9,
                    C_P_PLACEHOLDER,
                    "{ S = 1 }"
                ),
                substance("O2", 31.99880, 28.0, 100.0, 2.1e-9, C_P_PLACEHOLDER, "{}"),
                substance(
                    "SO4",
                    96.06260,
                    28.0,
                    100.0,
                    1.0e-9,
                    C_P_PLACEHOLDER,
                    "{ S = 1 }"
                ),
                substance(
                    "H_ION",
                    1.007940,
                    28.0,
                    100.0,
                    9.3e-9,
                    C_P_PLACEHOLDER,
                    "{}"
                ),
            ]
            .concat(),
            reaction(
                "h2s_oxidation",
                -8.46e6,
                "{ H2S = 1, O2 = 2 }",
                "{ SO4 = 1, H_ION = 2 }"
            ),
            enthalpy_record(2, 1.4e-7)
        );
        let d = derived(&narrow);
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
            substance("O2", 31.99880, 0.25, 1.0, 2.1e-9, C_P_PLACEHOLDER, "{}"),
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
