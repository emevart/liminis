//! Every rule of `CONFIG_SCHEMA.md` section 10 that is not a derivation of
//! scales: referential integrity, domains of definition, grid divisibility, the
//! chemical balances, the transport bounds, the reservoir, the processes and the
//! calibration section.
//!
//! # Why this is a separate entry point and not part of `load`
//!
//! [`validate`] is not called from [`super::load`], and that is a decision
//! rather than an omission. `configs/scenarios/hello.toml` does not survive its
//! own derivation — it declares no `[[substance]]` at all, so there is no
//! `C_cell`, no temperature and no `k_E` window (ADR-062) — and wiring the
//! validator into loading would turn `every_scenario_in_the_repository_loads`
//! red and break the command printed in `README.md` and `CLAUDE.md`. Giving
//! `hello.toml` a registry is not available either: `c_p` is declared for no
//! substance anywhere in the corpus (`CONFIG_SCHEMA.md` section 13 item 23), and
//! a plausible number invented here would be indistinguishable from a decided
//! one. So loading parses, validation is a second call, and the day item 23 is
//! closed the two can meet.
//!
//! # The order of the stages is the contract, not taste
//!
//! ```text
//! 1. referential integrity   ids, and every name that points at one
//! 2. domains of definition   what numbers are legal at all
//! 3. grid divisibility       nx, ny, nz against the coarsest lod
//! 4. chemistry               element balance, composition against mass, energy
//! 5. transport               Courant, outflow, the velocity-field keys
//! 6. boundary and reservoir  the five checks of ADR-059
//! 7. calibration             every path resolves to a number
//! 8. derive                  scales, widths, nu, the energy scale, substeps
//! 9. mass balance            against the tolerance `derive` produced
//! ```
//!
//! Referential integrity comes first because the alternative is described in
//! `CONFIG_SCHEMA.md` section 10 by its outcome: a typo in a substance name
//! inside `inputs` gets refused by a message about scales, and the same typo on
//! both sides of a reaction balances identically and is not refused at all.
//! `the_validator_refuses_before_it_derives` pins the order.
//!
//! The mass balance is the one check that comes *after* the derivation, and for
//! one reason: its tolerance is derived (`eps * M_turnover`, ADR-043) and is
//! read out of [`Derived`] rather than recomputed here. `M_turnover` is the sum
//! over the *consumed* side of the *netted* records; taken over both sides it
//! doubles, taken over the raw tables it counts a substance standing on both
//! sides twice, and either inflates the tolerance until the check that exists to
//! catch a lost proton is the first one that cannot.
//!
//! # What a refusal owes the reader
//!
//! A message here names the inequality that broke, the numbers on both sides of
//! it, and what to change. Half of these rules catch an error that is otherwise
//! visible only as strange dynamics a hundred thousand ticks in, and "the config
//! is invalid" sends the author to look for it alone. Two of them — the pair of
//! ADR-039 — name a *substance and a reaction*, because a config whose fault
//! lies in a pair cannot be fixed by looking at one name.
//!
//! # Where the border actually runs
//!
//! This section used to read "`Q` does not appear here", and since ADR-085 that
//! is false in one place and in exactly one. The border that *is* true: **the
//! validator produces no value of class `M` at all** — the one conversion into
//! storage units lives in [`super::derive`](super::derive), and `NUMERIC.md`
//! puts the wrappers of ADR-022 on the other side of it — and class `Q` reaches
//! it once, in the fold of settling's Courant number.
//!
//! That once is not a slip. `process::settle::settling_courant` compares the
//! number **the kernel receives**, folded into `Q`, and the validator's own
//! `f64` comparison is one-sidedly stricter: over the whole excess densities
//! from 1 to 4000 kg/m^3 the `f64` Courant number at the derived radius limit is
//! over one in 2632 cases. Left in `f64` here, the refusal would print a limit
//! that the same refusal then rejects. So the settling bound is judged by the
//! process — this file calls `settling_velocity` and `settling_courant` rather
//! than writing the chain `r -> k -> rho_bar -> w -> c` a second time — and
//! [`transport`] does not judge it again.

use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;

use super::derive::MASS_EPSILON;
use super::{Config, Derived, Face, Reaction, Scale, Substance, derive};
use crate::ledger::Channel;
use crate::numeric::{Q, qsub};
use crate::process::ProcessId;
use crate::process::settle::{Grain, Medium, settling_courant, settling_velocity};

/// The field id whose `lod` fixes the grid the wide temperature difference is
/// taken on (ADR-062, ADR-069).
const ENTHALPY_FIELD: &str = "enthalpy";

/// The process id of the velocity field, taken from the closed roster.
///
/// A lookup and no longer a literal: the registry of process ids lives under
/// `process/` (ADR-065), and a second spelling here would be a second thing to
/// rename — with the failure being a check that silently stops finding the record
/// it guards, which is how the Courant condition would lose its only live input.
const VELOCITY_FIELD_PROCESS: &str = ProcessId::VelocityField.id();

/// The default of `enabled` for the velocity field, and the one default of the
/// nine S0 processes that a record assigns (ADR-069).
///
/// **Taken from the process, not held here.** ADR-065 puts the default of
/// `enabled` beside the invariant, in the code of the process itself, and a
/// second copy in the validator is two constants of one meaning: they drift, and
/// the direction that drifts silently is a validator that reads an enabled
/// process as disabled — the Courant condition then loses its only live input and
/// the config loads with an advection speed nobody checked.
///
/// `false`, and the reason is arithmetic rather than caution: `u_conv_max` is
/// required when the process is on and has no default, so a default of `true`
/// would refuse every scenario that never mentions the velocity field —
/// `hello.toml` first. The argument is written out at the definition.
use crate::process::velocity::VELOCITY_FIELD_ENABLED_BY_DEFAULT;

/// The process id of the light, from the same closed roster and for the same
/// reason as [`VELOCITY_FIELD_PROCESS`].
const LIGHT_PROCESS: &str = ProcessId::Light.id();

/// The default of `enabled` for the light, taken from the process (ADR-065).
///
/// `false`, and assigned by ADR-076 rather than left over. The argument it used
/// to carry here — no sink and a counter too narrow for one tick — is spent:
/// ADR-084 ratifies the `exchange` face as the sink and ADR-083 widens the
/// counter. What keeps the value is that step `a` still does not dispatch, so a
/// default of `true` would refuse every scenario in the repository at tick
/// assembly. The argument is written out at the definition and never copied here.
use crate::process::light::ENABLED_BY_DEFAULT as LIGHT_ENABLED_BY_DEFAULT;

/// The process id of pressure, from the same closed roster and for the same
/// reason as [`VELOCITY_FIELD_PROCESS`].
const PRESSURE_PROCESS: &str = ProcessId::Pressure.id();

/// The default of `enabled` for pressure, taken from the process (ADR-065,
/// ADR-082).
///
/// `false`, and assigned by ADR-082 rather than left over: five locks are shut on
/// step `e`, and `process/pressure.rs` names them one by one. Read from there and
/// never copied, so that the day the locks come off and the default flips, the
/// predicate below follows without an edit.
use crate::process::pressure::ENABLED_BY_DEFAULT as PRESSURE_ENABLED_BY_DEFAULT;

/// The process id of settling, from the same closed roster and for the same
/// reason as [`VELOCITY_FIELD_PROCESS`].
const SETTLING_PROCESS: &str = ProcessId::Settling.id();

/// The default of `enabled` for settling, taken from the process (ADR-065,
/// ADR-085).
///
/// `false`, and assigned by ADR-085 rather than left over. The record weighed the
/// flip and refused it on four counts; what this file pays for the choice is the
/// refusal one function down, of a substance that declares a radius in a scenario
/// where nothing settles.
use crate::process::settle::ENABLED_BY_DEFAULT as SETTLING_ENABLED_BY_DEFAULT;

/// How the Courant number of one bound was compared, and by whom.
///
/// Two arms and not a `bool`, because the difference is *who* judged rather than
/// *whether* (ADR-085). A bound the operator folded carries the `Q` it folded, so
/// that a reader can see the number the kernel will run on; a bound this file
/// judges carries nothing and meets the two inequalities of [`transport`].
enum CourantVerdict {
    /// Judged here, in `f64`, against the declared upper bound on `|u|`.
    InF64,
    /// Judged by the operator's own fold, on the `Q` the kernel receives — and
    /// therefore **not** judged again in [`transport`], where the same number in
    /// `f64` is stricter by up to three ULP and would refuse what the message
    /// beside it prints as legal.
    FoldedByTheOperator(Q),
}

/// One transport operator and the speed bound that belongs to it.
///
/// A sequence of these rather than one number, and the type is a sequence so
/// that adding them up cannot happen by accident. `CONFIG_SCHEMA.md` section 10
/// forbids it outright: under Lie-Trotter splitting (ADR-036) each operator is
/// applied over a full step and gets by on its own stability condition, so one
/// shared budget would both reject legal scenarios and — worse — become a single
/// number somebody relaxes once for advection and settling together.
struct SpeedBound {
    /// The operator, as the refusal has to name it.
    operator: &'static str,
    /// The conservative upper bound on `|u|`, m/s.
    u_max: f64,
    /// How many faces of a voxel the flow can leave through at once.
    ///
    /// Six for the velocity field: the conservative estimate of SPEC section
    /// 4.2, whose limit is `dx/(6*dt)` and not `dx/(3*dt)` — the factor of three
    /// that zero divergence would give is not taken, because the divergence is
    /// exact on the velocity grid and not on the fine faces (ADR-069). One for
    /// settling: transport runs along a single axis, there is one outgoing face,
    /// and the two inequalities of section 4.2 coincide (ADR-067).
    outgoing_faces: u32,
    /// Who compared this bound's Courant number against one, and on what
    /// (ADR-085).
    courant: CourantVerdict,
}

/// Check a scenario against every rule of `CONFIG_SCHEMA.md` section 10 and
/// return what it leaves out.
///
/// [`Derived`] and not a second aggregate: everything derived is already
/// gathered in one value, and a second type beside it would be a second source
/// of truth about the same numbers.
///
/// # Errors
///
/// Returns an error naming the violated inequality, the numbers on both sides of
/// it and what to change, for any rule of `CONFIG_SCHEMA.md` section 10 — or any
/// refusal of [`derive`].
pub fn validate(config: &Config) -> Result<Derived> {
    referential_integrity(config)?;
    domains(config)?;
    grid_divisibility(config)?;
    chemistry(config)?;
    transport(config)?;
    boundary_and_reservoir(config)?;
    calibration(config)?;

    let derived = derive(config)?;
    mass_balance(config, &derived)?;
    Ok(derived)
}

// --- 1. referential integrity ---------------------------------------------

/// Every `id` is unique in its section, and every name that points at one
/// resolves (`CONFIG_SCHEMA.md` section 10, "rules with no test name").
///
/// `deny_unknown_fields` does not reach any of this. Serde sees struct fields,
/// and `composition`, `inputs`, `outputs`, `rate.km` and `conc_out` are tables
/// whose keys are *data* — the four places a typo is most likely (section 11
/// item 1).
fn referential_integrity(config: &Config) -> Result<()> {
    duplicate_ids(config)?;

    for substance in &config.substance {
        for name in substance.composition.keys() {
            if !config.conserved.contains_key(name) {
                bail!(
                    "substance `{}` declares composition key `{name}`, which is \
                     not a name in `conserved` (declared: {}). An unknown name is \
                     refused rather than skipped: written as a filter — \"sum the \
                     names that were found\" — this rule looks identical and \
                     turns `Fee = 20` back into a silent zero (ADR-064). Add \
                     `{name}` to `[conserved]` with its molar mass, or fix the \
                     spelling",
                    substance.id,
                    names(config.conserved.keys())
                );
            }
        }
    }

    for id in config.initial.layer.keys() {
        if !config.substance.iter().any(|s| s.id == *id) {
            bail!(
                "`[initial.layer]` declares a side for `{id}`, which is not a \
                 declared substance (declared: {}). The keys of this table are \
                 data, so `deny_unknown_fields` does not see them (section 11 \
                 item 1), and a skipped key would be worse than a refusal: the \
                 scenario would load, the canonical form would look right, and \
                 the substance the author meant to lift into the water column \
                 would stay in the sediment with both ledgers closing (ADR-077)",
                names(config.substance.iter().map(|s| &s.id))
            );
        }
    }

    for reaction in &config.reaction {
        for (side, table) in [("inputs", &reaction.inputs), ("outputs", &reaction.outputs)] {
            for id in table.keys() {
                if !config.substance.iter().any(|s| s.id == *id) {
                    bail!(
                        "reaction `{}` names `{id}` in {side}, which is not a \
                         declared substance (declared: {}). The same typo on both \
                         sides balances identically, `0 = 0`, and is not refused \
                         at all — which is why this is checked before the scales \
                         are derived (`CONFIG_SCHEMA.md` section 10)",
                        reaction.id,
                        names(config.substance.iter().map(|s| &s.id))
                    );
                }
            }
        }

        for id in reaction.rate.km.keys() {
            if !config.substance.iter().any(|s| s.id == *id) {
                bail!(
                    "reaction `{}` declares rate.km for `{id}`, which is not a \
                     declared substance (declared: {})",
                    reaction.id,
                    names(config.substance.iter().map(|s| &s.id))
                );
            }
            if !reaction.inputs.contains_key(id) {
                bail!(
                    "reaction `{}` declares rate.km for `{id}`, which is not one \
                     of its inputs ({}). `km` is a half-saturation constant of a \
                     substrate, and a substrate is an input (`CONFIG_SCHEMA.md` \
                     section 6)",
                    reaction.id,
                    names(reaction.inputs.keys())
                );
            }
        }
        for id in reaction.inputs.keys() {
            if !reaction.rate.km.contains_key(id) {
                bail!(
                    "reaction `{}` has input `{id}` with no rate.km: `km` is \
                     required for every input (`CONFIG_SCHEMA.md` section 6). \
                     Without it `S/(K+S)` has no `K`, and the answer is either \
                     zero or one depending on what the kernel author substitutes \
                     — both of which look like working kinetics. Declared km: {}",
                    reaction.id,
                    names(reaction.rate.km.keys())
                );
            }
        }

        catalyst_form(&reaction.catalyst, &reaction.id)?;

        // A non-empty `requires` is a load error until the gate exists (ADR-073),
        // and the refusal is here rather than only in `process/react.rs` because
        // section 10 is the register of *load* refusals: a refusal arriving after
        // `derive` and after the roster is assembled leaves the section naming a
        // rule it does not perform, and hands a config with both a window and an
        // overflowing scale a message about the scale.
        //
        // Nothing is resolved and nothing is enumerated. ADR-073 assigns no
        // roster of field identifiers, no unit for their bounds and no case for
        // their spelling — the single number the corpus prints, `min = 0.02`, is
        // three orders apart between its two readings and there is nothing to
        // calibrate it against — so this refuses every window, at every spelling,
        // which is what makes the missing roster harmless rather than urgent.
        //
        // The door in `process/react.rs` stays: the pair is the one ADR-059
        // already built for `energy_from`, a validator at load and the process in
        // its own words for anybody who came past it.
        if !reaction.requires.is_empty() {
            bail!(
                "reaction `{}` declares {} `requires` window(s), and no gate \
                 exists to satisfy them: the reaction kernel implements none, and \
                 no record says whether a gate is a hard cut-off or a factor, \
                 which fields it reads, in what unit, or how it behaves at the \
                 edge of a window (ADR-073). Accepted and ignored, a window is a \
                 reaction running outside the conditions it declares, with \
                 nothing to fail. Write `requires = []` until the record that \
                 writes the gate arrives",
                reaction.id,
                reaction.requires.len()
            );
        }
    }

    if let Some(reservoir) = &config.boundary.reservoir {
        for id in reservoir.conc_out.keys() {
            if !config.substance.iter().any(|s| s.id == *id) {
                bail!(
                    "[boundary.reservoir] declares conc_out for `{id}`, which is \
                     not a declared substance (declared: {})",
                    names(config.substance.iter().map(|s| &s.id))
                );
            }
        }
        for substance in &config.substance {
            if !reservoir.conc_out.contains_key(&substance.id) {
                bail!(
                    "[boundary.reservoir] names no conc_out for substance `{}` \
                     ({} of {} substances written out). Every substance of the \
                     registry has to be written: a default of zero *is* an \
                     infinite sink, and a statement about physics that strong has \
                     to be printed rather than inherited (ADR-059)",
                    substance.id,
                    reservoir.conc_out.len(),
                    config.substance.len()
                );
            }
        }
    }

    Ok(())
}

/// `id` is unique in each of the four sections.
///
/// Three of the four have a twin in [`derive`], which keeps them as guards on
/// its own domain; the process section has no twin, because `derive` never looks
/// at process ids. The price of "the last one wins" is exact: two `[[reaction]]`
/// records sharing an id share one `reaction_id` (ADR-027) and so one stream of
/// random numbers, the stochastic rounding of the two stops being independent,
/// and nothing fails — both ledgers close either way. For a process it is the
/// canonical form that starts depending on how the file was typed (section 11).
fn duplicate_ids(config: &Config) -> Result<()> {
    let sections: [(&str, Vec<&String>); 4] = [
        (
            "substance",
            config.substance.iter().map(|s| &s.id).collect(),
        ),
        ("reaction", config.reaction.iter().map(|r| &r.id).collect()),
        ("field", config.field.iter().map(|f| &f.id).collect()),
        ("process", config.process.iter().map(|p| &p.id).collect()),
    ];
    for (section, ids) in sections {
        for (i, id) in ids.iter().enumerate() {
            if let Some(first) = ids[..i].iter().position(|other| other == id) {
                bail!(
                    "`{id}` is declared twice in [[{section}]], at index {first} \
                     and at index {i}: an id is a name the rest of the scenario \
                     points at, and two records under one name make which of them \
                     is meant a property of the order the file was typed in"
                );
            }
        }
    }
    Ok(())
}

/// `catalyst` is empty, `guild:<id>` or `expr:<k>` (SPEC section 5).
///
/// The *form* is checked and the id is not resolved: there is no guild registry
/// in S0, it arrives in S1. That is a stage boundary rather than a forgotten
/// check, and it is said out loud because the price of an unrecognised form is
/// named in `CONFIG_SCHEMA.md` section 6: the value of `catalyst` silently fixes
/// the unit of `rate.vmax` in another table (ADR-063).
fn catalyst_form(catalyst: &str, reaction: &str) -> Result<()> {
    if catalyst.is_empty() {
        return Ok(());
    }
    for prefix in ["guild:", "expr:"] {
        if let Some(rest) = catalyst.strip_prefix(prefix) {
            if rest.is_empty() {
                bail!(
                    "reaction `{reaction}` declares catalyst = \"{catalyst}\" \
                     with an empty name after `{prefix}`"
                );
            }
            return Ok(());
        }
    }
    bail!(
        "reaction `{reaction}` declares catalyst = \"{catalyst}\", which is none \
         of the three legal forms: \"\", `guild:<id>` or `expr:<k>` (SPEC section \
         5). The form is not cosmetic — it fixes the unit of rate.vmax in another \
         table, turnovers/(s*m^3) when empty and turnovers/(s*mol of catalyst) \
         when not (ADR-063)"
    );
}

// --- 2. domains of definition ---------------------------------------------

/// What numbers are legal at all (`CONFIG_SCHEMA.md` section 10, "domain of
/// definition").
///
/// Every rule below is written as "refuse unless finite **and** in range", never
/// as "refuse if out of range", and the difference shows up on exactly one input:
/// `nan` is false in every comparison, so it walks through `max/typical > 16384`,
/// through `min < max` and through every other bare inequality, and reaches the
/// logarithm inside `e_r`.
fn domains(config: &Config) -> Result<()> {
    demand_positive(config.dt, "dt", "the scenario")?;
    demand_positive(config.grid.dx, "grid.dx", "the scenario")?;
    demand_positive(config.beta, "beta", "the scenario")?;
    demand_finite(config.t_ref, "T_ref", "the scenario")?;
    for (axis, extent) in [
        ("nx", config.grid.nx),
        ("ny", config.grid.ny),
        ("nz", config.grid.nz),
    ] {
        if extent < 1 {
            bail!("the scenario declares grid.{axis} = {extent}, and the rule is {axis} >= 1");
        }
    }
    for (name, mass) in &config.conserved {
        demand_non_negative(*mass, &format!("conserved.{name}"), "the scenario")?;
    }

    // The three constants of the medium (ADR-085). Zero is legal for the first
    // two and the permission is deliberate: `g = 0` is a lawful scenario that
    // settles nothing, and a ban would be a statement about physics this project
    // has not made — the same move `partial_molar_volume = 0` (ADR-067) and
    // `i_surface = 0` (ADR-076) got. The price is that a typo and a decision are
    // then indistinguishable from outside the file.
    demand_non_negative(config.physics.g, "physics.g", "the scenario")?;
    demand_non_negative(
        config.physics.rho_medium,
        "physics.rho_medium",
        "the scenario",
    )?;
    // Unconditional, like every other rule in this function, and that is the
    // half worth stating: written instead inside the "settling is on" branch, a
    // negative viscosity would pass validation whole on every scenario in the
    // repository — settling is off in all of them — and the table of
    // `every_domain_rule_refuses_its_own_violation` could not tell, because the
    // refusal `settling_velocity` raises names `mu` as well.
    if let Some(mu) = config.physics.mu {
        demand_positive(mu, "physics.mu", "the scenario")?;
    }

    for substance in &config.substance {
        let owner = format!("substance `{}`", substance.id);
        demand_positive(substance.molar_mass, "molar_mass", &owner)?;
        demand_positive(substance.typical_conc, "typical_conc", &owner)?;
        demand_positive(substance.max_conc, "max_conc", &owner)?;
        let ordered = substance.typical_conc <= substance.max_conc;
        if !ordered {
            bail!(
                "{owner} declares typical_conc = {} mol/m^3 over max_conc = {} \
                 mol/m^3, and the rule is typical_conc <= max_conc. The pair \
                 passes every refusal downstream — the ratio is below one and so \
                 below 2^14 — and yields a scale derived from a ceiling under the \
                 typical pool",
                substance.typical_conc,
                substance.max_conc
            );
        }
        demand_finite(
            substance.partial_molar_volume,
            "partial_molar_volume",
            &owner,
        )?;
        demand_non_negative(substance.settling_radius, "settling_radius", &owner)?;
        demand_non_negative(substance.diffusivity, "diffusivity", &owner)?;
        demand_finite(substance.c_p, "c_p", &owner)?;
        demand_finite(substance.enthalpy_formation, "enthalpy_formation", &owner)?;

        // The conditional rule of ADR-067, and conditional is the whole point:
        // `V_bar(H+)` is exactly zero on the accepted single-ion scale, so an
        // unconditional ban would reject the project's own registry. The division
        // that needs it non-zero is `rho_bar = molar_mass*1e-3/V_bar`, and it
        // happens only where something settles.
        if substance.settling_radius > 0.0 && substance.partial_molar_volume == 0.0 {
            bail!(
                "{owner} declares settling_radius = {} m over zero and \
                 partial_molar_volume = 0 m^3/mol. The grain density is derived \
                 by division, rho_bar = molar_mass*1e-3/partial_molar_volume, and \
                 a zero volume there means an infinite density rather than \
                 physics (ADR-067). Declare the partial molar volume, or set \
                 settling_radius = 0 if the substance does not settle",
                substance.settling_radius
            );
        }
    }

    for reaction in &config.reaction {
        let owner = format!("reaction `{}`", reaction.id);
        demand_positive(reaction.rate.q10, "rate.q10", &owner)?;
        demand_finite(reaction.enthalpy, "enthalpy", &owner)?;
        for (id, km) in &reaction.rate.km {
            demand_positive(*km, &format!("rate.km.{id}"), &owner)?;
        }
    }

    for field in &config.field {
        let owner = format!("field `{}`", field.id);
        if field.lod > 30 {
            bail!(
                "{owner} declares lod = {}, past any grid this project can \
                 address: `coarse_idx = fine_idx >> lod` and the extents are u32",
                field.lod
            );
        }
        if let Some(alpha) = field.thermal_diffusivity {
            demand_positive(alpha, "thermal_diffusivity", &owner)?;
        }
        if let (Some(t_min), Some(t_max)) = (field.t_min, field.t_max) {
            demand_finite(t_min, "t_min", &owner)?;
            demand_finite(t_max, "t_max", &owner)?;
            let ordered = t_min < t_max;
            if !ordered {
                bail!(
                    "{owner} declares t_min = {t_min} K and t_max = {t_max} K, \
                     and the rule is t_min < t_max: the working range is what k_E \
                     and the storage width of the field are derived from (ADR-062)"
                );
            }
        }
    }

    for process in &config.process {
        let owner = format!("process `{}`", process.id);
        if process.every_n_ticks < 1 {
            bail!(
                "{owner} declares every_n_ticks = {}, and the rule is \
                 every_n_ticks >= 1",
                process.every_n_ticks
            );
        }
        if let Some(u_conv_max) = process.u_conv_max {
            demand_positive(u_conv_max, "u_conv_max", &owner)?;
        }
        // Unconditional, like every other rule in this function: a `theta_max`
        // written on the diffusion record is a number nobody reads and it is
        // checked all the same. What it does **not** get is the window of
        // ADR-082 — the floor and the ceiling are conditional on the process
        // being on — so a limiting overflow declared on a *disabled* pressure
        // record parses, reaches the canonical form and `config_hash`, and never
        // meets either inequality. That is exactly the hole `u_conv_max` has on
        // a disabled velocity field; it is inherited knowingly, and closing it
        // belongs to a record rather than to this line.
        if let Some(theta_max) = process.theta_max {
            demand_positive(theta_max, "theta_max", &owner)?;
        }
        if let Some(l_c) = process.l_c {
            demand_positive(l_c, "l_c", &owner)?;
        }
        if let Some(stir_period) = process.stir_period {
            demand_positive(stir_period, "stir_period", &owner)?;
        }
        if !(process.stir_fraction.is_finite() && (0.0..=1.0).contains(&process.stir_fraction)) {
            bail!(
                "{owner} declares stir_fraction = {}, and the rule is \
                 stir_fraction in [0, 1]: it is a fraction of u_conv_max \
                 (ADR-069)",
                process.stir_fraction
            );
        }
        // The five light keys of ADR-076, checked on every record and not only on
        // the light's: a `daily_fraction` written on the pressure process is a
        // number nobody reads, and the domain rules above are all written this way.
        // The keys that depend on the process being *on* are the next function's.
        for (key, fraction) in [
            ("daily_fraction", process.daily_fraction),
            ("seasonal_fraction", process.seasonal_fraction),
        ] {
            if !(fraction.is_finite() && (0.0..=1.0).contains(&fraction)) {
                bail!(
                    "{owner} declares {key} = {fraction}, and the rule is \
                     {key} in [0, 1]: it is a fraction of the declared mean \
                     irradiance i_surface (ADR-076)"
                );
            }
        }
        for (key, period) in [
            ("daily_period", process.daily_period),
            ("seasonal_period", process.seasonal_period),
        ] {
            if let Some(period) = period {
                demand_positive(period, key, &owner)?;
                modulation_period(period, config.dt, key, &owner)?;
            }
        }
        if let Some(i_surface) = process.i_surface {
            demand_non_negative(i_surface, "i_surface", &owner)?;
        }
    }

    if let Some(reservoir) = &config.boundary.reservoir {
        let owner = "[boundary.reservoir]";
        demand_positive(reservoir.k_ex, "k_ex", owner)?;
        demand_finite(reservoir.t_out, "t_out", owner)?;
        for (id, conc) in &reservoir.conc_out {
            demand_non_negative(*conc, &format!("conc_out.{id}"), owner)?;
        }
    }

    // **After the reservoir block and not before it**, which is the same care
    // [`light`] takes with `t_out` one function down and for the same reason.
    // The steady-state ceiling of ADR-084 is `k_ex * sum(conc_out * c_p) *
    // (t_max - t_out)`, so it computes with all three of the keys just checked:
    // a negative `k_ex` or a negative `conc_out` makes the conductance negative,
    // the ceiling negative, every positive `i_surface` past it, and the refusal
    // then blames the light while printing a negative number of watts the lid
    // can carry away. Both orders refuse the world; only this one names the
    // culprit. `every_domain_rule_refuses_its_own_violation` cannot see the
    // difference — its fixtures leave the light off — so the order is held here
    // and by nothing else.
    light(config)?;
    pressure(config)?;

    for entry in &config.calibration {
        let owner = format!("[[calibration]] `{}`", entry.path);
        demand_finite(entry.min, "min", &owner)?;
        demand_finite(entry.max, "max", &owner)?;
    }

    Ok(())
}

/// A modulation period is a whole number of ticks, and at least three of them
/// (ADR-076).
///
/// Both halves are arithmetic rather than taste. The exactness of "`i_surface` is
/// the mean over a period" stands on an integer `N`, because the normalisation
/// `A_N` is a sum over the `N` samples the run takes; at a fractional `N` it stops
/// normalising anything and the key quietly means something else. And at `N = 2`
/// the two samples of `max(0, sin)` are taken at phases `0` and `pi`, both exactly
/// zero: the sum is zero, `A_N` divides by it, and the day is dark for ever at any
/// nonzero amplitude. Three is the shortest period that is degenerate and alive.
fn modulation_period(period: f64, dt: f64, key: &str, owner: &str) -> Result<()> {
    let ticks = period / dt;
    if ticks.fract() != 0.0 {
        bail!(
            "{owner} declares {key} = {period} s at dt = {dt} s, which is {ticks} \
             ticks, and the rule is a whole number of ticks: the normalisation A_N \
             is a sum over the N samples a period is actually sampled at, so a \
             fractional N stops normalising and i_surface stops meaning the mean \
             irradiance over a period (ADR-076)"
        );
    }
    if ticks < 3.0 {
        bail!(
            "{owner} declares {key} = {period} s at dt = {dt} s, which is {ticks} \
             ticks, and the rule is at least three ticks: at two the samples of \
             max(0, sin) fall on phases 0 and pi, both zero, so their sum is zero \
             and the normalisation A_N divides by it — the day would be dark for \
             ever at any nonzero amplitude (ADR-076)"
        );
    }
    Ok(())
}

/// The rules that hold only when the light process is on (ADR-076).
///
/// **The predicate is "enabled", not "wrote `enabled = true`" and not "has a
/// record".** ADR-065 makes an absent record mean the process's own default and
/// has the loader materialise the whole roster before the hash, so
/// `enabled == Some(true)` lets a scenario relying on the default through and a
/// check for the record's presence refuses a legal dark box. The default is read
/// from the process itself, never copied here.
fn light(config: &Config) -> Result<()> {
    let Some(process) = config.process.iter().find(|p| p.id == LIGHT_PROCESS) else {
        return Ok(());
    };
    if !process.enabled.unwrap_or(LIGHT_ENABLED_BY_DEFAULT) {
        return Ok(());
    }

    let Some(i_surface) = process.i_surface else {
        bail!(
            "process `{LIGHT_PROCESS}` is enabled and declares no i_surface: the \
             irradiance on the top face of the domain, W/m^2, is required when the \
             process is on and has no default (ADR-076, on the precedent of \
             u_conv_max). Write i_surface = 0.0 for a closed box, or the mean \
             irradiance over a modulation period"
        );
    };

    // A period is owed exactly when its amplitude is nonzero — the shape
    // `stir_fraction`/`stir_period` already has (ADR-069).
    for (fraction, fraction_key, period, period_key) in [
        (
            process.daily_fraction,
            "daily_fraction",
            process.daily_period,
            "daily_period",
        ),
        (
            process.seasonal_fraction,
            "seasonal_fraction",
            process.seasonal_period,
            "seasonal_period",
        ),
    ] {
        if fraction > 0.0 && period.is_none() {
            bail!(
                "process `{LIGHT_PROCESS}` declares {fraction_key} = {fraction} and \
                 no {period_key}: an amplitude without a period is a modulation \
                 with no clock (ADR-076)"
            );
        }
    }

    if i_surface <= 0.0 {
        // `i_surface = 0.0` is the legal dark box and owes neither rule below a
        // thing. Leaving early rather than guarding each rule keeps the two
        // refusals unreachable for a world that absorbs nothing.
        return Ok(());
    }

    // The two rules of ADR-084, replacing the blanket refusal of ADR-076. Their
    // **order is assigned by that record and is not taste**: the second computes
    // with `k_ex`, which a scenario without a `[boundary.reservoir]` section does
    // not have at all, so it would answer `None` and wave a sealed lit box
    // through if it ran first and this one never ran.
    let exchanging = exchanging_faces(&config.boundary);
    if exchanging.is_empty() || config.boundary.reservoir.is_none() {
        bail!(
            "process `{LIGHT_PROCESS}` declares i_surface = {i_surface} W/m^2 and \
             the domain has {} face(s) declared `exchange`{}: the energy sink of \
             S0 **is** that face and nothing else (ADR-084). Absorbed light \
             enters the enthalpy field, steps `c` and `d` carry it to the \
             boundary, and what crosses an `exchange` face is credited to \
             BOUNDARY_EXCHANGE — RADIATIVE_OUT is still unwritten and step `j` \
             has nothing to put in it. In a sealed box the absorbed energy has \
             nowhere to go, so the field climbs past the declared temperature \
             range and takes the Courant bound proved at load with it. Declare an \
             `exchange` face together with its [boundary.reservoir] section, or \
             write i_surface = 0.0 for a dark box",
            exchanging.len(),
            if config.boundary.reservoir.is_some() {
                " with a [boundary.reservoir] section"
            } else {
                " and no [boundary.reservoir] section"
            }
        );
    }

    // Section 6 owns `t_out`, and this function runs at section 2, so a reservoir
    // above the declared range reaches the ceiling below before anybody has said
    // it is illegal. There the difference `t_max - t_out` is negative, the
    // ceiling is negative, every positive `i_surface` fails it, and the refusal
    // blames the light for a reservoir. The world does not load either way — what
    // would be wrong is the name of the culprit — so the rule stands aside and
    // lets `t_out_outside_the_declared_temperature_range_is_rejected` speak.
    let range = config
        .field
        .iter()
        .find(|f| f.id == ENTHALPY_FIELD)
        .and_then(|f| f.t_min.zip(f.t_max));
    if let Some((t_min, t_max)) = range
        && let Some(reservoir) = &config.boundary.reservoir
        && t_min <= reservoir.t_out
        && reservoir.t_out <= t_max
        && let Some(ceiling) = absorbed_flux_ceiling(config)
        && i_surface > ceiling
    {
        // The comparison is **incident irradiance against a ceiling on absorbed
        // flux**, which refuses more than it strictly has to. That is deliberate
        // and has to be said here, because the obvious repair is a trap: scaling
        // the left side by an absorbed fraction would need a number nobody has
        // derived — the attenuator measure is `TODO(attenuation-measure)` in
        // `process/light.rs` — and it would turn a loud over-refusal into a quiet
        // under-refusal.
        //
        // TODO(A-25, `OPEN_QUESTIONS.md`): the inequality is written per unit
        // area and silently takes the `exchange` face to be the lit lid. An
        // `exchange` face is legal on any of the six (the check above counts a
        // *list*), and the grid need not be cubic, so a world that vents through
        // a side wall under a closed `z_max` gets a sink sized by the area of the
        // lit face. The right comparison of the two areas — and which coarse
        // cells face the sky at all — is the shape of the radiative boundary, and
        // no record settles it; inventing one here would be a number
        // indistinguishable from a decision.
        bail!(
            "process `{LIGHT_PROCESS}` declares i_surface = {i_surface} W/m^2 over \
             the {ceiling} W/m^2 the lid can carry away in steady state: the \
             `exchange` face conducts {} W/(m^2*K) — k_ex times the heat capacity \
             of a cubic metre of the reservoir — against the {} K between t_out = \
             {} K and t_max = {t_max} K of field `{ENTHALPY_FIELD}` (ADR-084). \
             Above that the steady state of the domain sits past t_max, where k_E \
             and the storage width of the enthalpy field stop being derived from \
             anything (ADR-062). The comparison is the *incident* irradiance \
             against a ceiling on the *absorbed* flux, so it refuses somewhat more \
             than it must; lower i_surface, widen the declared range, or raise \
             k_ex",
            boundary_conductance(config).unwrap_or(f64::NAN),
            t_max - reservoir.t_out,
            reservoir.t_out
        );
    }

    Ok(())
}

/// Which of the six faces are declared `exchange`, in the order of
/// `CONFIG_SCHEMA.md` section 4.
///
/// One transcription of the six-face list for the two rules that count them —
/// section 2's lit-scenario refusal and section 6's reservoir obligation. Two
/// transcriptions would be the ordinary way for a face added to
/// [`super::Boundary`] to be seen by one rule and not the other.
///
/// `pub(super)` for a third reader outside this module: `config/derive.rs` needs
/// the same list to size the traffic through the boundary (ADR-084), and it maps
/// each name to the cells that face covers rather than listing the six again.
pub(super) fn exchanging_faces(boundary: &super::Boundary) -> Vec<&'static str> {
    [
        ("x_min", boundary.x_min),
        ("x_max", boundary.x_max),
        ("y_min", boundary.y_min),
        ("y_max", boundary.y_max),
        ("z_min", boundary.z_min),
        ("z_max", boundary.z_max),
    ]
    .into_iter()
    .filter(|(_, face)| *face == Face::Exchange)
    .map(|(name, _)| name)
    .collect()
}

/// The thermal conductance of the `exchange` face, W/(m^2*K) (ADR-084).
///
/// `k_ex * sum_i(conc_out_i * c_p_i)`, and `None` **exactly** when there is no
/// `[boundary.reservoir]` section — never for any other reason, so a caller that
/// gets a number gets one it may compare against.
///
/// **`V_cell` is not in this formula and must not be put into it.** ADR-084
/// writes the conductance as `k_ex * C_cell_out / V_cell`, and
/// `C_cell_out / V_cell` *is* `sum(conc_out * c_p)` identically — the cell volume
/// cancels because both the amounts and the heat capacity are extensive in it.
/// Forming it as a ratio would force a choice between `V_voxel` and `V_cell`,
/// which is a factor of 64 at `lod = 2` (`config/derive.rs` warns about the same
/// substitution from the other side), and a ceiling 64 times too large would pass
/// every lit world while still looking like a guard.
///
/// The sum runs over the **reservoir's** composition and not over the domain's
/// `typical_conc`. On the shipped scenario the two agree to every digit — water
/// is 55500 mol/m^3 in both — so that substitution is invisible there, and only a
/// fixture whose two compositions differ can see it at all.
///
/// A `conc_out` entry naming no substance of the registry contributes nothing:
/// section 1 refuses such a scenario before this runs, and in the direction the
/// mistake would take it — a smaller conductance, a tighter ceiling, a louder
/// refusal — rather than the other way.
pub(super) fn boundary_conductance(config: &Config) -> Option<f64> {
    let reservoir = config.boundary.reservoir.as_ref()?;
    let heat_capacity: f64 = reservoir
        .conc_out
        .iter()
        .filter_map(|(id, conc)| {
            config
                .substance
                .iter()
                .find(|s| &s.id == id)
                .map(|s| conc * s.c_p)
        })
        .sum();
    Some(reservoir.k_ex * heat_capacity)
}

/// The largest absorbed flux the lid can carry away in steady state, W/m^2
/// (ADR-084): [`boundary_conductance`] times `t_max - t_out`.
///
/// `None` when there is no reservoir, or when the `enthalpy` field declares no
/// `t_min`/`t_max` pair — there is no declared range to leave, and the field's
/// own rules refuse that scenario for a better reason.
///
/// `pub(super)` rather than private on purpose: `config/derive.rs` prints this
/// number in the load report, and printing it means **calling this**, not
/// transcribing the product a second time. Two foldings of one coefficient drift
/// apart in silence — the corpus catches that pattern in five places already
/// (`Light::i_surface`, `Modulation::daily_norm`, `substeps_and_alpha`,
/// `DerivedReservoir::k_ex` next to `alpha_ex`) — and here the refusal would
/// judge by one number while the report printed another, both plausible.
pub(super) fn absorbed_flux_ceiling(config: &Config) -> Option<f64> {
    let conductance = boundary_conductance(config)?;
    let reservoir = config.boundary.reservoir.as_ref()?;
    let field = config.field.iter().find(|f| f.id == ENTHALPY_FIELD)?;
    // Both ends are demanded though only `t_max` enters the product: a range with
    // one end missing is not a declared range, and handing back a ceiling for it
    // would put a number exactly where the field's own rule is about to refuse.
    let (_, t_max) = field.t_min.zip(field.t_max)?;
    Some(conductance * (t_max - reservoir.t_out))
}

/// The rules that hold only when the pressure process is on (ADR-082).
///
/// **The predicate is "enabled", not "wrote `enabled = true`" and not "has a
/// record".** [`light`] one function up already writes that rule out; it is
/// repeated here rather than referred to, because the failure it prevents is
/// invisible today. The default of pressure is `false`, so `enabled == Some(true)`
/// and this expression give the same answer on every scenario in the repository —
/// and they part company on the day the five locks come off and the default
/// flips, letting every scenario that relied on the default load with no
/// `theta_max` at all. A check for the record's *presence* fails in the other
/// direction and fails today: it would refuse `hello.toml`, which names no
/// process at all (ADR-065).
///
/// The window itself is not here. `theta_max >= 6*Theta_sup` and the ceiling from
/// the declared run horizon are functions of the registry and of the grid, so
/// they live in `config/derive.rs` beside the other things derived from the
/// declared concentrations.
fn pressure(config: &Config) -> Result<()> {
    let record = config.process.iter().find(|p| p.id == PRESSURE_PROCESS);
    // An absent record resolves to the same default rather than to "off". After
    // `parse` there is no such case — `config::materialise` writes all nine
    // records before the hash (ADR-065) — but `validate` is public and defined on
    // a `Config` somebody assembled by hand, and the early return `light` takes
    // on a missing record is a latent copy of the default this branch avoids.
    let enabled = match record {
        Some(process) => process.enabled.unwrap_or(PRESSURE_ENABLED_BY_DEFAULT),
        None => PRESSURE_ENABLED_BY_DEFAULT,
    };
    if !enabled {
        return Ok(());
    }

    if record.and_then(|p| p.theta_max).is_none() {
        bail!(
            "process `{PRESSURE_PROCESS}` is enabled and declares no theta_max: \
             the limiting overflow is dimensionless, is required when the process \
             is on and has no default (ADR-082, on the precedent of i_surface and \
             u_conv_max). The whole mobility is derived from it — the kernel is \
             handed 1/theta_max, and the stiffness, dt and dx do not survive the \
             derivation — so a plausible number written in as a default would be \
             indistinguishable from a decision, and because the scheme conserves \
             exactly, both halves of the invariant would close over it for ever. \
             Declare theta_max: it has to be at least 6*Theta_sup, the peak \
             occupancy the declared max_conc and partial_molar_volume allow, and \
             small enough that N_max^2*theta_max/(pi^2*Theta_typ) stays inside the \
             declared run horizon (`CONFIG_SCHEMA.md` section 9)"
        );
    }

    Ok(())
}

/// `value > 0` and finite, or a refusal naming the key and the bound.
fn demand_positive(value: f64, key: &str, owner: &str) -> Result<()> {
    let ok = value.is_finite() && value > 0.0;
    if !ok {
        bail!("{owner} declares {key} = {value}, and the rule is {key} > 0 (finite)");
    }
    Ok(())
}

/// `value >= 0` and finite.
fn demand_non_negative(value: f64, key: &str, owner: &str) -> Result<()> {
    let ok = value.is_finite() && value >= 0.0;
    if !ok {
        bail!("{owner} declares {key} = {value}, and the rule is {key} >= 0 (finite)");
    }
    Ok(())
}

/// `value` is finite, with no sign or magnitude asked of it.
fn demand_finite(value: f64, key: &str, owner: &str) -> Result<()> {
    if !value.is_finite() {
        bail!("{owner} declares {key} = {value}, and the rule is that {key} is finite");
    }
    Ok(())
}

// --- 3. grid divisibility --------------------------------------------------

/// `nx`, `ny` and `nz` are multiples of `2^lod` on every axis, for the
/// **coarsest** declared `lod` (`CONFIG_SCHEMA.md` section 3, section 10).
///
/// The coarsest and not the finest. Checked against the smallest declared `lod`
/// the rule is green on any scenario with one fine field: at `nx = 50` and
/// `lod = 2` the covering cell is incomplete, `coarse_idx = fine_idx >> lod`
/// loses voxels on the way up, and the half of the ledger the coarsened field
/// answers for diverges — on a run, not at load.
fn grid_divisibility(config: &Config) -> Result<()> {
    let Some(lod) = config.field.iter().map(|f| f.lod).max() else {
        return Ok(());
    };
    let step = 1u32 << u32::from(lod);
    let coarsest = config
        .field
        .iter()
        .find(|f| f.lod == lod)
        .map_or("<none>", |f| f.id.as_str());
    for (axis, extent) in [
        ("nx", config.grid.nx),
        ("ny", config.grid.ny),
        ("nz", config.grid.nz),
    ] {
        if extent % step != 0 {
            bail!(
                "grid.{axis} = {extent} is not a multiple of 2^{lod} = {step}, \
                 the coarsening of field `{coarsest}`: {extent} = {step} * {} + \
                 {}. Raise {axis} to {} or lower the lod",
                extent / step,
                extent % step,
                (extent / step + 1) * step
            );
        }
    }
    Ok(())
}

// --- 4. chemistry ----------------------------------------------------------

/// The element balance, the composition against the declared mass, and the
/// energy side of every reaction.
fn chemistry(config: &Config) -> Result<()> {
    for substance in &config.substance {
        composition_against_mass(config, substance)?;
    }
    for reaction in &config.reaction {
        stoichiometry(reaction)?;
        element_balance_holds(config, reaction)?;
        energy_channel(reaction)?;
        enthalpy_agreement(config, reaction)?;
        superfluous_channel(reaction)?;
    }
    Ok(())
}

/// The third check of ADR-033 in the wording of ADR-064:
/// `molar_mass >= (1 - eps) * sum_e composition[e]*conserved[e]` over the names
/// with non-zero mass, plus the sign rule that makes it worth anything.
///
/// **The one check in the project that sees g/mol swapped for kg/mol.** The mass
/// balance does not: it is a ratio, and a factor of a thousand cancels in it.
///
/// The inequality is non-strict, and that is not politeness — `FE2` has exactly
/// zero margin (55.84500 against `1*55.84500`), and a strict comparison would
/// reject a substance of the project's own registry. `(1 - eps)` stands on the
/// right: `molar_mass*(1+eps) >= sum` is a different check and lets a real
/// shortfall of `1e-6` through.
fn composition_against_mass(config: &Config, substance: &Substance) -> Result<()> {
    let mut sum = 0.0;
    for (name, count) in &substance.composition {
        // Resolved by referential integrity already; a `get` that silently
        // skipped an unknown name is the shape ADR-064 closed.
        let mass = config.conserved[name];
        if mass == 0.0 {
            continue;
        }
        if *count < 0 {
            bail!(
                "substance `{}` declares composition `{name}` = {count} against \
                 conserved.{name} = {mass} g/mol, which is not zero. A negative \
                 entry against a quantity with mass works as a discount to the \
                 mass check and buys the right to declare {} g/mol less than the \
                 truth (ADR-064). Negative entries are legal only where the \
                 conserved quantity weighs nothing, as `charge` does",
                substance.id,
                f64::from(-count) * mass
            );
        }
        sum += f64::from(*count) * mass;
    }
    let heavy_enough = substance.molar_mass >= (1.0 - MASS_EPSILON) * sum;
    if !heavy_enough {
        bail!(
            "substance `{}` declares molar_mass = {} g/mol against a composition \
             weighing {sum} g/mol, and the rule is molar_mass >= (1 - {MASS_EPSILON})*sum, \
             that is >= {}. The gap is {:.1}x. This is the only check in the \
             project that sees g/mol swapped for kg/mol: the mass balance is a \
             ratio and the factor of a thousand cancels in it (ADR-033, ADR-064)",
            substance.id,
            substance.molar_mass,
            (1.0 - MASS_EPSILON) * sum,
            sum / substance.molar_mass
        );
    }
    Ok(())
}

/// Values of `inputs` and `outputs` are integers strictly above zero (ADR-026).
///
/// The side is chosen by the section, never by the sign. A fractional value does
/// not reach this check at all — it does not parse into the `i32` of the schema —
/// and that is asserted too, so that the refusal on zero does not read as the
/// only case.
fn stoichiometry(reaction: &Reaction) -> Result<()> {
    for (side, table) in [("inputs", &reaction.inputs), ("outputs", &reaction.outputs)] {
        for (id, s) in table {
            if *s <= 0 {
                bail!(
                    "reaction `{}` declares {side}.{id} = {s}, and the rule is a \
                     molar coefficient > 0. Balancing a chemical equation *is* \
                     finding positive integer coefficients, and the side is \
                     supplied by the section rather than by the sign (ADR-026)",
                    reaction.id
                );
            }
        }
    }
    Ok(())
}

/// The element balance: for every name of `conserved`, the sum of
/// `s * composition` on the left equals the sum on the right, as an exact
/// equality of integers (ADR-025, ADR-033).
///
/// Over the **molar** `s` and never over `nu` in storage units. Taken from `nu`
/// the balance is scaled by `2^(k_i - e_r)` per participant; where the `k_i`
/// happen to agree — plausible on the four substances of section 12 — the common
/// factor cancels, the check passes on a config it exists to refuse, and the
/// first reaction with participants of differing width goes through undetected.
fn element_balance_holds(config: &Config, reaction: &Reaction) -> Result<()> {
    for (element, residual) in element_balance(config, reaction) {
        if residual != 0 {
            bail!(
                "reaction `{}` does not balance by `{element}`: {} on the left \
                 against {} on the right, a residual of {residual}. The element \
                 balance is an exact equality of integers, not a tolerance \
                 (ADR-025, ADR-033)",
                reaction.id,
                side_sum(config, &reaction.inputs, element),
                side_sum(config, &reaction.outputs, element)
            );
        }
    }
    Ok(())
}

/// Right side minus left side, per conserved quantity, accumulated in `i64`
/// (ADR-064).
///
/// `i64` because the product of a Redfield-sized coefficient with a composition
/// entry has no reason to stay inside an `i32` once a scenario writes a large
/// turnover, and an overflow here would balance a reaction that does not.
fn element_balance<'a>(config: &'a Config, reaction: &Reaction) -> BTreeMap<&'a str, i64> {
    config
        .conserved
        .keys()
        .map(|element| {
            let element = element.as_str();
            let residual = side_sum(config, &reaction.outputs, element)
                - side_sum(config, &reaction.inputs, element);
            (element, residual)
        })
        .collect()
}

/// `sum_i s_i * composition_i[element]` over one side of a reaction.
fn side_sum(config: &Config, table: &BTreeMap<String, i32>, element: &str) -> i64 {
    table
        .iter()
        .map(|(id, s)| {
            let atoms = config
                .substance
                .iter()
                .find(|it| it.id == *id)
                .and_then(|it| it.composition.get(element))
                .copied()
                .unwrap_or(0);
            i64::from(*s) * i64::from(atoms)
        })
        .sum()
}

/// Reference magnitude of the enthalpy agreement, J per turnover.
///
// TODO(enthalpy-tolerance): ADR-044 says the tolerance is "derived the same way
// as in ADR-043, and for the same reason", and ADR-043 derives `eps *
// M_turnover` *and* guards the derived tolerance from below by the lightest
// molar mass among the substances with an empty composition. The energetic twin
// of neither half is named by the corpus: not the reference magnitude
// (`sum |s_i * dH_f,i|`? `|dH_r|`?) and not the lower guard. Taken here as the
// closest structural analogue — the sum of the magnitudes of the terms — with
// `MASS_EPSILON` and not a second literal, and recorded as a question in
// `OPEN_QUESTIONS.md`.
fn enthalpy_tolerance(reference: f64) -> f64 {
    MASS_EPSILON * reference
}

/// The declared `enthalpy` agrees with `sum s_net * enthalpy_formation`, J per
/// turnover (ADR-044).
///
/// Over the **molar** net stoichiometry. Section 10 writes the sum as
/// `sum nu_i * dH_f,i`, and a literal reading of that gives every participant a
/// factor of `2^(k_i - e_r)` — a number about storage, in an equation about
/// thermochemistry.
fn enthalpy_agreement(config: &Config, reaction: &Reaction) -> Result<()> {
    let mut expected = 0.0;
    let mut reference = 0.0;
    for substance in &config.substance {
        let inp = reaction.inputs.get(&substance.id).copied().unwrap_or(0);
        let outp = reaction.outputs.get(&substance.id).copied().unwrap_or(0);
        let net = f64::from(outp - inp);
        expected += net * substance.enthalpy_formation;
        reference += (net * substance.enthalpy_formation).abs();
    }
    let tolerance = enthalpy_tolerance(reference);
    let gap = (reaction.enthalpy - expected).abs();
    let agrees = gap <= tolerance;
    if !agrees {
        bail!(
            "reaction `{}` declares enthalpy = {} J/turnover against {expected} \
             J/turnover from the enthalpies of formation of its participants, a \
             gap of {gap} J over the tolerance of {tolerance} J. The sum is taken \
             over the molar net stoichiometry, so the two sides are comparable \
             whatever storage scale the substances end up on (ADR-044)",
            reaction.id,
            reaction.enthalpy
        );
    }
    Ok(())
}

/// `energy_from` is a name of the closed channel enumeration or the empty string
/// (ADR-059), and `"LIGHT"` is refused by its own message (ADR-049).
fn energy_channel(reaction: &Reaction) -> Result<()> {
    if reaction.energy_from.is_empty() {
        return Ok(());
    }
    // Before the enumeration, deliberately. `"LIGHT"` is printed by SPEC section
    // 5 and its refusal is an accounting one rather than a spelling one, so a
    // reader who wrote it needs the accounting answer and not a list of six
    // names.
    if reaction.energy_from == "LIGHT" {
        bail!(
            "reaction `{}` declares energy_from = \"LIGHT\", which SPEC section 5 \
             prints and ADR-049 makes illegal: the loss of intensity is credited \
             to enthalpy by step i', so by the time step h runs the light is \
             already inside the accounting, and debiting a channel would subtract \
             it a second time — the energy residual would come out non-zero by \
             exactly the photosynthetic flux. Write energy_from = \"\"",
            reaction.id
        );
    }
    if Channel::from_name(&reaction.energy_from).is_none() {
        bail!(
            "reaction `{}` declares energy_from = \"{}\", which is not one of the \
             six channel names ({}) and not the empty string. The registry is \
             closed and lives in code: a channel is the right-hand side of the \
             equation everything else is checked against, and a registry a \
             scenario could extend would let any residual be closed by declaring \
             a seventh channel (ADR-059)",
            reaction.id,
            reaction.energy_from,
            names(Channel::ALL.iter().map(|c| c.name()))
        );
    }
    Ok(())
}

/// A non-empty `energy_from` is legal **only** for a reaction whose enthalpy does
/// not agree with `sum s * dH_f` — and ADR-044 does not load such a reaction.
///
/// So in S0 the non-empty branch is unreachable, and that is a decision rather
/// than a hole: it exists as a form of the validator and not as a working path
/// (`CONFIG_SCHEMA.md` section 6). Should some config ever pass with a non-empty
/// channel, this is the check that turns red.
fn superfluous_channel(reaction: &Reaction) -> Result<()> {
    if !reaction.energy_from.is_empty() {
        bail!(
            "reaction `{}` declares energy_from = \"{}\" while its enthalpy \
             agrees with the enthalpies of formation of its participants. A \
             channel is legal only where the declared enthalpy does *not* agree \
             — and ADR-044 does not load such a reaction, so in S0 the only legal \
             value is the empty string, which means \"out of this voxel's own \
             enthalpy\" (ADR-028, ADR-059)",
            reaction.id,
            reaction.energy_from
        );
    }
    Ok(())
}

// --- 5. transport ----------------------------------------------------------

/// The two inequalities of SPEC section 4.2, one operator at a time.
fn transport(config: &Config) -> Result<()> {
    let lod = enthalpy_lod(config);
    for bound in speed_bounds(config, lod)? {
        // A bound its own module already judged is not judged again here, and
        // the skip is the decision rather than an optimisation (ADR-085). The
        // operator compared the `Q` its kernel receives; this function compares
        // an `f64`, which is stricter by up to three ULP at the bound of one, so
        // a second pass would refuse the very grain whose limit the message
        // beside it prints as the heaviest that loads.
        //
        // The verdict is carried rather than dropped so that the number the
        // kernel will run on stands where the comparison used to, and it is read
        // here: the operator's postcondition restated in the operator's own
        // domain, which is the one place a `Q` reaches this file.
        if let CourantVerdict::FoldedByTheOperator(courant) = bound.courant {
            debug_assert!(
                qsub(Q::ZERO, Q::ONE) <= courant && courant <= Q::ONE,
                "operator `{}` handed back a folded Courant number of {courant:?} \
                 and called it legal",
                bound.operator
            );
            continue;
        }
        let courant = bound.u_max * config.dt / config.grid.dx;
        let stable = courant <= 1.0;
        if !stable {
            bail!(
                "operator `{}` has an upper bound of {} m/s on |u|, giving a \
                 Courant number of u*dt/dx = {courant} over the limit of 1: at dt \
                 = {} s and dx = {} m the operator may not exceed {} m/s. Each \
                 operator is checked on its own — under Lie-Trotter splitting \
                 every operator is applied over a full step and gets by on its \
                 own condition (ADR-036), so this bound is not a share of a \
                 shared budget",
                bound.operator,
                bound.u_max,
                config.dt,
                config.grid.dx,
                config.grid.dx / config.dt
            );
        }
        let faces = f64::from(bound.outgoing_faces);
        let outflow = faces * courant;
        let bounded = outflow <= 1.0;
        if !bounded {
            bail!(
                "operator `{}` has an upper bound of {} m/s on |u|, giving an \
                 outflow number of ({} faces)*u*dt/dx = {outflow} over the limit \
                 of 1: at dt = {} s and dx = {} m the operator may not exceed {} \
                 m/s. This is the stricter of the two conditions of SPEC section \
                 4.2 and it is not the Courant one — a voxel with a diverging \
                 flow gives away the *sum* over its outgoing faces, not the \
                 maximum, and goes negative on a run rather than at load. The \
                 factor of three that zero divergence would give is not taken: \
                 the divergence is exact on the velocity grid, not on the fine \
                 faces (ADR-069)",
                bound.operator,
                bound.u_max,
                bound.outgoing_faces,
                config.dt,
                config.grid.dx,
                config.grid.dx / (faces * config.dt)
            );
        }
    }
    Ok(())
}

/// One bound per transport operator, and the keys each operator requires.
///
/// `derived_lod` is the `lod` of the enthalpy field — the grid the wide
/// temperature difference of ADR-069 is taken on, and therefore the grid the
/// structure length is measured against. It is deliberately *not* the step the
/// Courant condition uses: the flux is computed on the fine grid and substituting
/// the coarse step would double the admissible speed with no basis (ADR-069).
fn speed_bounds(config: &Config, derived_lod: u8) -> Result<Vec<SpeedBound>> {
    let mut bounds = Vec::new();

    if let Some(process) = config
        .process
        .iter()
        .find(|p| p.id == VELOCITY_FIELD_PROCESS)
        && process.enabled.unwrap_or(VELOCITY_FIELD_ENABLED_BY_DEFAULT)
    {
        let Some(u_conv_max) = process.u_conv_max else {
            bail!(
                "process `{}` is enabled and declares no u_conv_max, which is \
                 required and has no default (ADR-069). The key is not a taste: \
                 the mobility L is derived from it so that |u_conv| <= u_conv_max \
                 holds exactly, and without it the Courant condition has no input \
                 at all — the config would load and advect at a speed nobody \
                 checked. Absence of the whole record is a different thing and is \
                 legal: it means the process's own default, which is \
                 enabled = {VELOCITY_FIELD_ENABLED_BY_DEFAULT}",
                process.id
            );
        };
        if process.stir_fraction > 0.0 && process.stir_period.is_none() {
            bail!(
                "process `{}` declares stir_fraction = {} above zero and no \
                 stir_period, which is required in that case and has no default \
                 (ADR-069). At exactly zero the noise kernel is not dispatched at \
                 all, which is what makes the default free",
                process.id,
                process.stir_fraction
            );
        }
        structure_length(config, process.l_c, derived_lod)?;
        bounds.push(SpeedBound {
            operator: VELOCITY_FIELD_PROCESS,
            u_max: (1.0 + process.stir_fraction) * u_conv_max,
            outgoing_faces: 6,
            courant: CourantVerdict::InF64,
        });
    }

    settling(config, &mut bounds)?;

    Ok(bounds)
}

/// The second input of both Courant conditions: one bound per settling
/// substance, and the two refusals that surround them (ADR-067, ADR-085).
///
/// **The chain is not written here.** `r -> k -> rho_bar -> w -> c` lives in
/// `process/settle.rs`, and this function calls it: a second text computing the
/// same velocity would drift from the one the kernel runs on, and the drift
/// would show up as a refusal printing a limit that the same code then rejects.
fn settling(config: &Config, bounds: &mut Vec<SpeedBound>) -> Result<()> {
    let record = config.process.iter().find(|p| p.id == SETTLING_PROCESS);
    // An absent record resolves to the process's own default and never to "off",
    // for the reason `pressure` writes out: after `parse` there is no such case,
    // and `validate` is public and defined on a `Config` somebody assembled by
    // hand.
    let enabled = match record {
        Some(process) => process.enabled.unwrap_or(SETTLING_ENABLED_BY_DEFAULT),
        None => SETTLING_ENABLED_BY_DEFAULT,
    };

    if !enabled {
        // The price ADR-085 pays for keeping the default at `false`, and it is
        // paid as a message rather than as silence. A declared grain in a world
        // where nothing settles is exactly the failure ADR-067 refused a default
        // `settling_radius` for: sediment that never settles, under an entirely
        // green suite, indistinguishable from an honest zero.
        for substance in &config.substance {
            if substance.settling_radius > 0.0 {
                bail!(
                    "substance `{}` declares settling_radius = {} m above zero \
                     and the process `{SETTLING_PROCESS}` is off, so nothing \
                     will ever move it. Absence of the `[[process]]` record is \
                     not the same as switching the process off — it means the \
                     process's own default, which is \
                     enabled = {SETTLING_ENABLED_BY_DEFAULT} (ADR-065, ADR-085). \
                     Either write `[[process]] id = \"{SETTLING_PROCESS}\"` with \
                     enabled = true and a `[physics] mu`, or declare \
                     settling_radius = 0.0 if `{}` does not settle",
                    substance.id,
                    substance.settling_radius,
                    substance.id
                );
            }
        }
        return Ok(());
    }

    // The predicate is the **process** and never "some substance declares a
    // radius", and the code dictates it rather than taste (ADR-085):
    // `SettlePhase::fold` builds a `Settle` for every lane so that a bad
    // declaration is refused at load instead of being ignored for sitting beside
    // a zero, and `settling_velocity` checks the viscosity before it looks at a
    // radius. So a scenario with settling on and no grain at all is refused here
    // too — otherwise the refusal moves from the load to `Tick::new`.
    let Some(mu) = config.physics.mu else {
        bail!(
            "process `{SETTLING_PROCESS}` is enabled and `[physics]` declares no \
             mu, which is required in that case and has no default (ADR-085). \
             The other two constants of the medium have defaults and this one \
             cannot: over the temperature range a scenario declares the viscosity \
             of water moves by a factor of 3.27, its density by 1.18 %, and g by \
             nothing at all. Note that the rule is about the *process* and not \
             about the radii: the settling phase folds a velocity for every lane \
             and checks the viscosity before the branch on a zero radius, so a \
             scenario with settling on and no grain in it needs mu just the same. \
             Declare `[physics] mu = ...` in Pa*s, or switch the process off"
        );
    };
    let medium = Medium {
        rho_medium: config.physics.rho_medium,
        g: config.physics.g,
        mu,
    };

    for substance in &config.substance {
        if substance.settling_radius <= 0.0 {
            continue;
        }
        let grain = Grain {
            settling_radius: substance.settling_radius,
            molar_mass: substance.molar_mass,
            partial_molar_volume: substance.partial_molar_volume,
        };
        let w = settling_velocity(&grain, &medium)
            .with_context(|| format!("substance `{}`", substance.id))?;
        let courant = match settling_courant(w, config.dt, config.grid.dx) {
            Ok(courant) => courant,
            Err(error) => {
                // The bound inverted onto the radius, which is what the author
                // of the scenario can act on — "the velocity is too large" is
                // not. Printed from `medium.g` and from the excess density the
                // process itself derives, so that the limit this message names
                // and the limit the next load accepts are one number.
                let excess = (crate::process::settle::grain_density(
                    substance.molar_mass,
                    substance.partial_molar_volume,
                )? - medium.rho_medium)
                    .abs();
                let limit = (9.0 * medium.mu * config.grid.dx
                    / (2.0 * excess * medium.g * config.dt))
                    .sqrt();
                bail!(
                    "substance `{}`: {error}. At mu = {mu} Pa*s, dx = {} m, \
                     dt = {} s, g = {} m/s^2 and an excess density of {excess} \
                     kg/m^3 the heaviest grain that loads has \
                     r <= sqrt(9*mu*dx/(2*d_rho*g*dt)) = {limit} m = {} um, \
                     against the declared settling_radius = {} m = {} um. The eco \
                     regime at a one-second tick represents silt and finer, not \
                     sand, and that is a statement about the working window of the \
                     model rather than a defect of the scheme (ADR-067)",
                    substance.id,
                    config.grid.dx,
                    config.dt,
                    medium.g,
                    limit * 1.0e6,
                    substance.settling_radius,
                    substance.settling_radius * 1.0e6
                );
            }
        };
        bounds.push(SpeedBound {
            operator: SETTLING_PROCESS,
            // `|w|` and never `w`. A buoyant grain has `w < 0`, its Courant
            // number comes out negative, both inequalities of `transport` hold
            // for any speed at all, and a rising grain of any radius would load.
            // No substance of the corpus is buoyant, so there is no witness in
            // the repository — which is why it is said here.
            u_max: w.abs(),
            // One outgoing face: transport runs along a single axis, so the sum
            // over outgoing faces of SPEC section 4.2 degenerates to one term
            // and the two inequalities coincide (ADR-067).
            outgoing_faces: 1,
            courant: CourantVerdict::FoldedByTheOperator(courant),
        });
    }

    Ok(())
}

/// `r = round(l_c/(2*dx_coarse)) >= 1`, where `dx_coarse` is the step of the
/// **enthalpy** grid (ADR-069).
///
/// Taken from the fine `dx` instead, `r` at `lod = 2` comes out four times too
/// large, the refusal never fires, the wide difference degenerates into a narrow
/// one — and with it the wavelength selection for whose sake ADR-069 rejected the
/// bare horizontal Laplacian. The velocity field goes on looking like a velocity
/// field, and `heated_bottom_produces_net_vertical_transport` passes on a grid
/// artefact: word for word the outcome ADR-054 rejected superbee for.
fn structure_length(config: &Config, l_c: Option<f64>, derived_lod: u8) -> Result<()> {
    let Some(l_c) = l_c else {
        bail!(
            "process `{VELOCITY_FIELD_PROCESS}` is enabled and declares no l_c: \
             the radius of the wide stencil, r = round(l_c/(2*dx_coarse)), is \
             derived from it and from nothing else (ADR-069)"
        );
    };
    let dx_coarse = config.grid.dx * f64::from(1u32 << u32::from(derived_lod));
    let r = (l_c / (2.0 * dx_coarse)).round();
    let resolved = r >= 1.0;
    if !resolved {
        bail!(
            "process `{VELOCITY_FIELD_PROCESS}` declares l_c = {l_c} m, giving a \
             wide-stencil radius of r = round(l_c/(2*dx_coarse)) = round({}) = {r} \
             cells, under the limit of 1. `dx_coarse` here is the step of the \
             *enthalpy* grid, grid.dx * 2^lod = {} m * 2^{derived_lod} = \
             {dx_coarse} m, because that is the grid the wide temperature \
             difference is taken on. Below one cell the wide difference \
             degenerates into a narrow one and the wavelength selection \
             disappears with it (ADR-069). Declare l_c >= {} m",
            l_c / (2.0 * dx_coarse),
            config.grid.dx,
            2.0 * dx_coarse * 0.5
        );
    }
    Ok(())
}

/// The `lod` of the `[[field]] id = "enthalpy"` record, or zero when there is
/// none — in which case [`derive`] refuses the scenario for a better reason.
fn enthalpy_lod(config: &Config) -> u8 {
    config
        .field
        .iter()
        .find(|f| f.id == ENTHALPY_FIELD)
        .map_or(0, |f| f.lod)
}

// --- 6. boundary and reservoir ---------------------------------------------

/// Periodicity is a property of the axis, and the five checks of ADR-059.
fn boundary_and_reservoir(config: &Config) -> Result<()> {
    let boundary = &config.boundary;
    for (axis, low, high) in [
        ("x", boundary.x_min, boundary.x_max),
        ("y", boundary.y_min, boundary.y_max),
        ("z", boundary.z_min, boundary.z_max),
    ] {
        if (low == Face::Periodic) != (high == Face::Periodic) {
            bail!(
                "axis {axis} declares {axis}_min = {low:?} and {axis}_max = \
                 {high:?}, and periodicity is a property of the *axis*: both \
                 faces have to be periodic together (`CONFIG_SCHEMA.md` section \
                 4). A half-periodic axis gives a grid where the neighbour past \
                 the face exists on one side and not on the other, so the \
                 antisymmetry of the flux breaks on one face of the domain — and \
                 `flux_is_antisymmetric` runs on a pair of values and does not \
                 see it"
            );
        }
    }

    let exchanging = exchanging_faces(boundary);

    let Some(reservoir) = &config.boundary.reservoir else {
        if exchanging.is_empty() {
            return Ok(());
        }
        bail!(
            "{} of the six faces declared `exchange` ({}) and there is no \
             [boundary.reservoir] section. The section is required exactly when \
             some face is `exchange`, and `exchange` is the default on z_max, so \
             nearly always (ADR-059)",
            exchanging.len(),
            exchanging.join(", ")
        );
    };
    if exchanging.is_empty() {
        bail!(
            "[boundary.reservoir] is declared and no face is `exchange`: the \
             section is required exactly when some face is, in both directions. A \
             declared reservoir that touches nothing reads as a working boundary \
             (ADR-059)"
        );
    }

    for substance in &config.substance {
        // In mol/m^3, against the declared numbers rather than against rounded
        // storage units: the ghost cell lives in the same units as the field, and
        // comparing after rounding loses the case that sits exactly on the bound.
        let conc = reservoir.conc_out[&substance.id];
        let under_ceiling = conc <= substance.max_conc;
        if !under_ceiling {
            bail!(
                "[boundary.reservoir] declares conc_out.{} = {conc} mol/m^3 over \
                 the max_conc = {} mol/m^3 of that substance. max_conc is a hard \
                 ceiling on the run and not an estimate (ADR-041), so a ghost \
                 cell above it overflows the field on the first exchange",
                substance.id,
                substance.max_conc
            );
        }
    }

    if let Some(field) = config.field.iter().find(|f| f.id == ENTHALPY_FIELD)
        && let (Some(t_min), Some(t_max)) = (field.t_min, field.t_max)
        && !(t_min <= reservoir.t_out && reservoir.t_out <= t_max)
    {
        bail!(
            "[boundary.reservoir] declares t_out = {} K outside the declared \
             range [{t_min}, {t_max}] K of field `{ENTHALPY_FIELD}`. The price is \
             not pedantry: k_E and the storage width of the enthalpy field are \
             derived from that range (ADR-062), so a ghost cell outside it \
             overflows the field on the first exchange instead of failing the \
             load (ADR-059)",
            reservoir.t_out
        );
    }

    let alpha_ex = reservoir.k_ex * config.dt / config.grid.dx;
    let stable = alpha_ex <= 1.0;
    if !stable {
        bail!(
            "[boundary.reservoir] declares k_ex = {} m/s, giving alpha_ex = \
             k_ex*dt/dx = {alpha_ex} over the limit of 1: at dt = {} s and dx = \
             {} m that is k_ex <= {} m/s. The condition is over dx and not over \
             dx^2 — written by analogy with the diffusive n = ceil(6*D*dt/dx^2) \
             it would bound a different quantity entirely (ADR-059)",
            reservoir.k_ex,
            config.dt,
            config.grid.dx,
            config.grid.dx / config.dt
        );
    }

    Ok(())
}

// --- 7. calibration --------------------------------------------------------

/// Every `path` resolves to a number that is in the hash, and every window is an
/// interval (ADR-038).
fn calibration(config: &Config) -> Result<()> {
    if config.calibration.is_empty() {
        return Ok(());
    }
    // Walked over `toml::Value::try_from(config)` rather than over a mirror of
    // the schema written in Rust: a hand-written matcher over the fields of
    // `Config` would drift from the schema exactly as silently as a second
    // projection in `hash.rs` would, and ADR-066 introduced exhaustive
    // destructuring precisely so that the compiler catches such drift.
    let root = toml::Value::try_from(config)
        .context("projecting the config into a toml value to resolve calibration paths")?;

    for entry in &config.calibration {
        let is_interval = entry.min < entry.max;
        if !is_interval {
            bail!(
                "[[calibration]] `{}` declares min = {} and max = {}, and the \
                 rule is min < max. A degenerate window is a coordinate the \
                 search walks around forever",
                entry.path,
                entry.min,
                entry.max
            );
        }
        let positive_low = entry.min > 0.0;
        if entry.scale == Scale::Log && !positive_low {
            bail!(
                "[[calibration]] `{}` declares scale = \"log\" with min = {}, and \
                 the rule is min > 0 on a logarithmic scale, where a non-positive \
                 bound has no meaning at all",
                entry.path,
                entry.min
            );
        }
        if entry.path.split('.').next() == Some("calibration") {
            bail!(
                "[[calibration]] `{}` points inside [[calibration]], the one \
                 section outside config_hash (ADR-038). A number that is not in \
                 the hash cannot be in the model: two runs of different physics \
                 would get one identity",
                entry.path
            );
        }
        let Some(value) = resolve_path(&root, &entry.path) else {
            bail!(
                "[[calibration]] path `{}` leads nowhere. Paths are dotted and \
                 address arrays by `id` rather than by index \
                 (`reaction.h2s_oxidation.rate.vmax`), and a path that resolves to \
                 nothing is an error rather than an empty coordinate (ADR-038)",
                entry.path
            );
        };
        if !(value.is_float() || value.is_integer()) {
            bail!(
                "[[calibration]] path `{}` resolves to a {}, not to a number. A \
                 path that stops at a table \"resolves\" and is not a coordinate: \
                 the search driver would get one that moves nothing, and the \
                 whole search would run in a space one dimension smaller than its \
                 author thinks (ADR-038)",
                entry.path,
                value.type_str()
            );
        }
    }
    Ok(())
}

/// Resolve a dotted path, addressing arrays of tables by their `id` (ADR-038,
/// `CONFIG_SCHEMA.md` section 8).
fn resolve_path<'a>(root: &'a toml::Value, path: &str) -> Option<&'a toml::Value> {
    let mut cursor = root;
    for segment in path.split('.') {
        cursor = match cursor {
            toml::Value::Table(table) => table.get(segment)?,
            toml::Value::Array(items) => items
                .iter()
                .find(|item| item.get("id").and_then(toml::Value::as_str) == Some(segment))?,
            _ => return None,
        };
    }
    Some(cursor)
}

// --- 9. the mass balance ---------------------------------------------------

/// `|sum s_out*M - sum s_in*M| <= mass_tolerance`, with the tolerance read out of
/// [`Derived`] (ADR-033, ADR-043).
fn mass_balance(config: &Config, derived: &Derived) -> Result<()> {
    for (reaction, derived_reaction) in config.reaction.iter().zip(derived.reactions()) {
        let left = side_mass(config, &reaction.inputs);
        let right = side_mass(config, &reaction.outputs);
        let gap = (right - left).abs();
        let balances = gap <= derived_reaction.mass_tolerance;
        if !balances {
            bail!(
                "reaction `{}` does not balance by mass: {left} g/mol on the left \
                 against {right} g/mol on the right, a gap of {gap} g/mol over \
                 the tolerance of {} g/mol. The tolerance is derived rather than \
                 declared — {MASS_EPSILON} of the turnover mass {} g/mol \
                 (ADR-043) — and the turnover is the *consumed* side of the \
                 *netted* records: computed over both sides it doubles, and the \
                 check written to catch a lost proton is the first one that stops \
                 seeing it",
                reaction.id,
                derived_reaction.mass_tolerance,
                derived_reaction.turnover_mass
            );
        }
    }
    Ok(())
}

/// `sum_i s_i * molar_mass_i` over one side of a reaction, g/mol.
fn side_mass(config: &Config, table: &BTreeMap<String, i32>) -> f64 {
    table
        .iter()
        .map(|(id, s)| {
            let mass = config
                .substance
                .iter()
                .find(|it| it.id == *id)
                .map_or(0.0, |it| it.molar_mass);
            f64::from(*s) * mass
        })
        .sum()
}

/// A comma-separated list, for the "declared: …" half of a refusal.
fn names<T: std::fmt::Display>(items: impl Iterator<Item = T>) -> String {
    let list: Vec<String> = items.map(|item| format!("`{item}`")).collect();
    if list.is_empty() {
        "none".to_string()
    } else {
        list.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse;
    use proptest::prelude::*;

    // --- fixtures ---------------------------------------------------------
    //
    // TOML constants rather than files under `configs/`, exactly as in
    // `derive.rs`: a scenario file is covered by the CI guard of ADR-020, and a
    // fixture written for a test would drag `WORLD_FORMAT_VERSION` along for
    // something that is not a change of semantics.
    //
    // **Numbers marked as placeholders are placeholders.** `c_p` is declared for
    // no substance anywhere in the corpus, `enthalpy_formation` for none either,
    // and `partial_molar_volume` for neither `H2S` nor `O2`
    // (`CONFIG_SCHEMA.md` section 13 item 23); the same goes for a working
    // `u_conv_max`. They are written because the schema requires them and
    // because the worked example has to be able to pass. Do not copy them into
    // `configs/`.
    //
    // Everything else is from the corpus: the molar masses are SPEC section 2.3
    // to seven significant figures (ADR-043), the diffusivities are SPEC section
    // 1.7, the concentrations, the enthalpy `-8.46e5 J/turnover`, the
    // temperature range and the thermal diffusivity are `CONFIG_SCHEMA.md`
    // section 12 and ADR-062.

    /// The scenario of `CONFIG_SCHEMA.md` section 12, whole, with the five
    /// numbers the corpus does not name filled in as placeholders.
    ///
    /// The heat capacities differ per substance for a reason that is not physics
    /// — they are placeholders either way — but so that a fixture can rewrite
    /// exactly one substance's block by anchoring on its `c_p` line.
    const WORKED_EXAMPLE: &str = r#"
name = "h2s-oxidation"
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
nx = 64
ny = 64
nz = 64
dx = 1.0e-4

[boundary]
z_min = "closed"
z_max = "exchange"

[boundary.reservoir]
t_out = 298.15
k_ex = 1.0e-5
conc_out = { H2S = 0.0, O2 = 0.25, SO4 = 28.0, H_ION = 1.0e-4 }

[physics]
g = 9.80665
rho_medium = 1000.0
mu = 1.0e-3

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

[[field]]
id = "enthalpy"
lod = 2
thermal_diffusivity = 1.4e-7
t_min = 273.15
t_max = 323.15

[[process]]
id = "diffusion"
enabled = true

[[process]]
id = "reactions"
enabled = true

[[process]]
id = "advection"
enabled = true

[[process]]
id = "velocity_field"
enabled = true
l_c = 3.2e-3
u_conv_max = 1.0e-5

[[process]]
id = "pressure"
enabled = false

[[calibration]]
path = "reaction.h2s_oxidation.rate.q10"
min = 1.5
max = 3.0
scale = "linear"
"#;

    /// The whole `[boundary.reservoir]` section, for the fixtures that remove it.
    const RESERVOIR: &str = r#"
[boundary.reservoir]
t_out = 298.15
k_ex = 1.0e-5
conc_out = { H2S = 0.0, O2 = 0.25, SO4 = 28.0, H_ION = 1.0e-4 }
"#;

    /// The whole velocity-field process record.
    const VELOCITY_FIELD: &str = r#"
[[process]]
id = "velocity_field"
enabled = true
l_c = 3.2e-3
u_conv_max = 1.0e-5
"#;

    /// A header for the fixtures that count records: closed on every face, so
    /// that no reservoir is owed, and with nothing but the enthalpy field.
    const COUNTING_HEADER: &str = r#"
name = "counting"
dt = 1.0
beta = 0.015625
T_ref = 298.15

[grid]
nx = 64
ny = 64
nz = 64
dx = 1.0e-4

[boundary]
x_min = "closed"
x_max = "closed"
y_min = "closed"
y_max = "closed"
z_min = "closed"
z_max = "closed"

[[field]]
id = "enthalpy"
lod = 2
thermal_diffusivity = 1.4e-7
t_min = 273.15
t_max = 323.15
"#;

    /// Water and the proton, the one pair ADR-040 was written for, on the
    /// numbers of SPEC section 2.3 and ADR-040: 55.5 mol/l against `1e-4`.
    ///
    /// `s_proton` is the stoichiometry of the proton, and it is the whole point
    /// of the fixture: it is what sets `e_r`, and `e_r` is what water's scale is
    /// raised to.
    fn water_and_proton(s_proton: i64) -> String {
        // Placeholder, as everywhere here: no `enthalpy_formation` is named by
        // the corpus. Chosen so that the declared enthalpy at `s_proton = 13` is
        // the `4.94e7 J/turnover` order ADR-062 does its arithmetic on — and, since
        // ADR-081, so that it is a whole multiple of `2^10`: the proton's scale is
        // `k = 74` against `k_E = 67`, so its weight `round(dH_f * 2^-7)` is the
        // one thing in this fixture that can round, and a rounded weight puts the
        // summed `nu_E` a part in sixty thousand from the declared enthalpy — over
        // the tolerance, and refused. `3 800 064 = 2^10 * 3711`.
        let h_f_proton = 3_800_064.0;
        let enthalpy = h_f_proton * s_proton as f64;
        format!(
            r#"
name = "water-and-proton"
dt = 1.0
beta = 0.015625
T_ref = 298.15

[grid]
nx = 64
ny = 64
nz = 64
dx = 1.0e-4

[boundary]
x_min = "closed"
x_max = "closed"
y_min = "closed"
y_max = "closed"
z_min = "closed"
z_max = "closed"

[[substance]]
id = "WATER"
molar_mass = 18.01528
typical_conc = 55500.0
max_conc = 55500.0
partial_molar_volume = 1.8e-5
settling_radius = 0.0
diffusivity = 2.3e-9
c_p = 75.3
enthalpy_formation = 0.0
composition = {{}}

[[substance]]
id = "H_ION"
molar_mass = 1.007940
typical_conc = 1.0e-4
max_conc = 1.0e-2
partial_molar_volume = 0.0
settling_radius = 0.0
diffusivity = 9.3e-9
c_p = 100.0
enthalpy_formation = {h_f_proton:e}
composition = {{}}

[[reaction]]
id = "photosynthesis"
enthalpy = {enthalpy:e}
inputs = {{ WATER = 106 }}
outputs = {{ H_ION = {s_proton} }}

[reaction.rate]
vmax = 1.0e-6
t_vmax = 298.15
q10 = 2.0
km = {{ WATER = 0.01 }}

[[field]]
id = "enthalpy"
lod = 2
thermal_diffusivity = 1.4e-7
t_min = 273.15
t_max = 323.15
"#
        )
    }

    /// Two reactions over one registry, so that a substance's scale is raised by
    /// one of them and its `nu` is taken in the other.
    ///
    /// That combination is the only way a legal-looking config can break
    /// `nu <= i32::MAX`: inside a single reaction the inequality is a theorem
    /// (see `check_nu_against_the_pool` in `derive.rs`), because `e_r` came from
    /// the scarcest participant. Across two it is not — water's `k` is 87 here,
    /// set by the proton in `proton_pump`, and `brining` runs at `e_r = 41`.
    const TWO_REACTIONS: &str = r#"
name = "two-reactions"
dt = 1.0
beta = 0.015625
T_ref = 298.15

[grid]
nx = 64
ny = 64
nz = 64
dx = 1.0e-4

[boundary]
x_min = "closed"
x_max = "closed"
y_min = "closed"
y_max = "closed"
z_min = "closed"
z_max = "closed"

[[substance]]
id = "WATER"
molar_mass = 18.01528
typical_conc = 55500.0
max_conc = 55500.0
partial_molar_volume = 1.8e-5
settling_radius = 0.0
diffusivity = 2.3e-9
c_p = 75.3
enthalpy_formation = 0.0
composition = {}

[[substance]]
id = "H_ION"
molar_mass = 1.007940
typical_conc = 1.0e-4
max_conc = 1.0e-2
partial_molar_volume = 0.0
settling_radius = 0.0
diffusivity = 9.3e-9
c_p = 100.0
enthalpy_formation = 1.0
composition = {}

[[substance]]
id = "BRINE"
molar_mass = 18.01528
typical_conc = 30.0
max_conc = 100.0
partial_molar_volume = 1.8e-5
settling_radius = 0.0
diffusivity = 1.0e-9
c_p = 100.0
enthalpy_formation = 100.0
composition = {}

[[reaction]]
id = "proton_pump"
enthalpy = 2.0e8
inputs = { WATER = 1 }
outputs = { H_ION = 200000000 }

[reaction.rate]
vmax = 1.0e-6
t_vmax = 298.15
q10 = 2.0
km = { WATER = 0.01 }

[[reaction]]
id = "brining"
enthalpy = 100.0
inputs = { WATER = 1 }
outputs = { BRINE = 1 }

[reaction.rate]
vmax = 1.0e-6
t_vmax = 298.15
q10 = 2.0
km = { WATER = 0.01 }

[[field]]
id = "enthalpy"
lod = 2
thermal_diffusivity = 1.4e-7
t_min = 273.15
t_max = 323.15
"#;

    /// One filler `[[substance]]` record, for counting.
    fn filler_substance(i: usize) -> String {
        format!(
            r#"
[[substance]]
id = "FILL{i}"
molar_mass = 10.0
typical_conc = 1.0
max_conc = 10.0
partial_molar_volume = 1.0e-5
settling_radius = 0.0
diffusivity = 1.0e-9
c_p = 100.0
enthalpy_formation = 0.0
composition = {{}}
"#
        )
    }

    /// One filler `[[reaction]]`, for counting. Balanced by element (both
    /// participants are untracked), by mass (equal molar masses) and by
    /// enthalpy, so that nothing but the count can refuse it.
    fn filler_reaction(i: usize) -> String {
        format!(
            r#"
[[reaction]]
id = "rxn{i}"
enthalpy = 1000.0
inputs = {{ A = 1 }}
outputs = {{ B = 1 }}

[reaction.rate]
vmax = 1.0e-6
t_vmax = 298.15
q10 = 2.0
km = {{ A = 0.01 }}
"#
        )
    }

    /// The two substances the filler reactions run between.
    const COUNTING_PAIR: &str = r#"
[[substance]]
id = "A"
molar_mass = 10.0
typical_conc = 1.0
max_conc = 10.0
partial_molar_volume = 1.0e-5
settling_radius = 0.0
diffusivity = 1.0e-9
c_p = 100.0
enthalpy_formation = 0.0
composition = {}

[[substance]]
id = "B"
molar_mass = 10.0
typical_conc = 1.0
max_conc = 10.0
partial_molar_volume = 1.0e-5
settling_radius = 0.0
diffusivity = 1.0e-9
c_p = 100.0
enthalpy_formation = 1000.0
composition = {}
"#;

    fn validated(text: &str) -> Derived {
        validate(&parse(text).expect("the fixture must parse")).expect("the fixture must validate")
    }

    /// The refusal a fixture earns, from whichever stage refuses it.
    ///
    /// Parse **and** validate, because since ADR-065 the load has two stages that
    /// can refuse: `parse` materialises the process roster and rejects an unknown
    /// or duplicated process id there, `validate` holds every rule of section 10.
    /// A helper that only unwrapped `parse` would turn a refusal moving between
    /// the two into a panic reading "the fixture must parse".
    fn refusal(text: &str) -> String {
        let refused = parse(text).and_then(|config| validate(&config).map(|_| ()));
        format!("{:#}", refused.expect_err("the fixture must be refused"))
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

    /// Append a `[[substance]]` record together with the reservoir entry it
    /// obliges. The obligation is the rule of ADR-059, not the fixture's
    /// convenience: every substance of the registry has to be named in
    /// `conc_out`.
    fn with_substance(text: &str, id: &str, record: &str) -> String {
        let extended = swap(
            text,
            "H_ION = 1.0e-4 }",
            &format!("H_ION = 1.0e-4, {id} = 0.0 }}"),
        );
        format!("{extended}{record}")
    }

    fn assert_names(message: &str, wanted: &[&str]) {
        for want in wanted {
            assert!(
                message.contains(want),
                "the refusal has to name `{want}`; it said:\n{message}"
            );
        }
    }

    // --- the worked example ------------------------------------------------

    #[test]
    fn the_worked_example_passes_every_check() {
        // Without this every refusal below is green for a validator that rejects
        // everything. It also fails the moment a new check catches the corpus's
        // own example, which is the other half of what it is for.
        let derived = validated(WORKED_EXAMPLE);

        assert_eq!(derived.reactions().len(), 1);
        let reaction = &derived.reactions()[0];
        assert_eq!(reaction.e_r, 61, "`CONFIG_SCHEMA.md` section 12 prints 61");
        assert_eq!(
            derived.substances()[reaction.scarcest as usize].id,
            "H_ION",
            "the quantum is set by the scarcest participant (ADR-039)"
        );
        assert_eq!(
            derived.substances().iter().map(|s| s.k).collect::<Vec<_>>(),
            vec![64, 67, 61, 74],
            "the scales of the table in section 12"
        );
        assert!(derived.energy().k_e > 0);
    }

    // --- the chemical balances ---------------------------------------------

    #[test]
    fn reaction_unbalanced_by_element_is_rejected() {
        // Sulphur removed from the sulphate: 1 on the left, 0 on the right.
        let text = swap(
            WORKED_EXAMPLE,
            "enthalpy_formation = -846000.0\ncomposition = { S = 1 }",
            "enthalpy_formation = -846000.0\ncomposition = {}",
        );
        assert_names(
            &refusal(&text),
            &["h2s_oxidation", "`S`", "1 on the left", "0 on the right"],
        );

        // The property the numbers alone do not pin: the sums are taken over the
        // *molar* stoichiometry. Taken over `nu` in storage units they carry a
        // factor of `2^(k_i - e_r)` each, and where the `k_i` agree the common
        // factor cancels and the check passes on a config it exists to refuse.
        let config = parse(WORKED_EXAMPLE).expect("parses");
        let balance = element_balance(&config, &config.reaction[0]);
        assert_eq!(balance["S"], 0);
        assert_eq!(balance["C"], 0);
        assert_eq!(
            side_sum(&config, &config.reaction[0].inputs, "S"),
            1,
            "one sulphur in, from H2S, in moles and not in storage units"
        );
    }

    #[test]
    fn charge_is_tracked_as_a_conserved_quantity_of_zero_mass() {
        // The promise of ADR-025 that was prose until ADR-064: a conserved
        // quantity of zero mass, with signed entries. -2 + 2*1 = 0.
        let text = swap(WORKED_EXAMPLE, "C = 12.01070", "C = 12.01070\ncharge = 0.0");
        let text = swap(
            &text,
            "enthalpy_formation = -846000.0\ncomposition = { S = 1 }",
            "enthalpy_formation = -846000.0\ncomposition = { S = 1, charge = -2 }",
        );
        let text = swap(
            &text,
            "c_p = 103.0\nenthalpy_formation = 0.0\ncomposition = {}",
            "c_p = 103.0\nenthalpy_formation = 0.0\ncomposition = { charge = 1 }",
        );
        let derived = validated(&text);
        assert_eq!(derived.reactions()[0].e_r, 61, "charge moves no scale");

        let config = parse(&text).expect("parses");
        assert_eq!(
            element_balance(&config, &config.reaction[0])["charge"],
            0,
            "a quantity of zero mass stays inside the element balance; dropping \
             it along with the third check would silently stop tracking charge"
        );

        // And the sign is checked rather than filtered: unbalanced charge is a
        // refusal like any other.
        let unbalanced = swap(
            &text,
            "composition = { S = 1, charge = -2 }",
            "composition = { S = 1, charge = -3 }",
        );
        assert_names(&refusal(&unbalanced), &["charge", "h2s_oxidation"]);
    }

    #[test]
    fn reaction_unbalanced_by_mass_is_rejected() {
        // One proton lost: 1.00794 g/mol against a tolerance of 1e-6 * 98.07848.
        let text = swap(
            WORKED_EXAMPLE,
            "outputs = { SO4 = 1, H_ION = 2 }",
            "outputs = { SO4 = 1, H_ION = 1 }",
        );
        let message = refusal(&text);
        assert_names(
            &message,
            &[
                "h2s_oxidation",
                "98.07848",
                "97.07054",
                "does not balance by mass",
            ],
        );

        // The tolerance is read out of `Derived`, not recomputed here. On the
        // worked example the turnover is the consumed side of the netted records,
        // 34.08088 + 2*31.99880 = 98.07848 g/mol.
        let derived = validated(WORKED_EXAMPLE);
        let tolerance = derived.reactions()[0].mass_tolerance;
        assert!(
            (derived.reactions()[0].turnover_mass - 98.07848).abs() < 1e-9,
            "turnover mass came out {}",
            derived.reactions()[0].turnover_mass
        );
        assert!(
            tolerance < 1.00794,
            "a tolerance at or above the lightest untracked substance cannot tell \
             a rounded mass from a lost molecule"
        );

        // A registry rounded to the seventh digit still loads: that is the whole
        // reason the tolerance is derived rather than zero.
        let rounded = swap(
            WORKED_EXAMPLE,
            "molar_mass = 34.08088",
            "molar_mass = 34.08089",
        );
        validated(&rounded);
    }

    #[test]
    fn mass_tolerance_above_the_lightest_untracked_substance_is_rejected() {
        // The second half of ADR-043, run through `validate` rather than through
        // `derive` so that the name guards what a user sees. A turnover heavy
        // enough makes `eps * M_turnover` exceed the proton.
        let text = swap(
            WORKED_EXAMPLE,
            "inputs = { H2S = 1, O2 = 2 }",
            "inputs = { H2S = 100000, O2 = 2 }",
        );
        let text = swap(
            &text,
            "outputs = { SO4 = 1, H_ION = 2 }",
            "outputs = { SO4 = 100000, H_ION = 2 }",
        );
        // The declared enthalpy scales with the turnover, or the agreement of
        // ADR-044 refuses the fixture before the tolerance is ever derived.
        let text = swap(&text, "enthalpy = -846000.0", "enthalpy = -8.46e10");
        let message = refusal(&text);
        assert_names(&message, &["1.00794", "H_ION"]);
    }

    #[test]
    fn reaction_enthalpy_disagreeing_with_formation_enthalpies_is_rejected() {
        let text = swap(
            WORKED_EXAMPLE,
            "enthalpy = -846000.0",
            "enthalpy = -845000.0",
        );
        let message = refusal(&text);
        assert_names(
            &message,
            &[
                "h2s_oxidation",
                "-845000",
                "-846000",
                "J/turnover",
                "tolerance",
            ],
        );

        // The tolerance comes from a named constant, not from a number picked so
        // that the fixture passes: it is `MASS_EPSILON` of the reference
        // magnitude, and `MASS_EPSILON` is the one epsilon of ADR-043 (ADR-062
        // says outright that it is not chosen again).
        assert!((enthalpy_tolerance(846_000.0) - 0.846).abs() < 1e-12);
        assert!((MASS_EPSILON - 1.0e-6).abs() < 1e-18);

        // And the sum is over the molar `s`, not over `nu`: adding a spectator
        // whose storage scale is nowhere near the others does not move it.
        let derived = validated(WORKED_EXAMPLE);
        assert!(
            derived.substances().iter().map(|s| s.k).max().unwrap()
                - derived.substances().iter().map(|s| s.k).min().unwrap()
                >= 13,
            "the fixture's participants have to differ in scale for that to mean \
             anything"
        );
    }

    #[test]
    fn reaction_unbalanced_by_energy_is_rejected() {
        // A converging enthalpy plus a channel: the channel is superfluous.
        let superfluous = swap(
            WORKED_EXAMPLE,
            "energy_from = \"\"",
            "energy_from = \"SOLAR_IN\"",
        );
        assert_names(
            &refusal(&superfluous),
            &[
                "h2s_oxidation",
                "SOLAR_IN",
                "agrees with the enthalpies of formation",
            ],
        );

        // A non-converging enthalpy plus any channel: the previous check refuses
        // it, and ADR-044 therefore never loads the reaction the non-empty branch
        // exists for. In S0 that branch is unreachable, and it is a decision
        // rather than a hole — should a config ever pass with a non-empty
        // channel, this test is what turns red.
        let disagreeing = swap(&superfluous, "enthalpy = -846000.0", "enthalpy = -800000.0");
        assert_names(&refusal(&disagreeing), &["-800000", "-846000"]);
    }

    #[test]
    fn channel_name_outside_the_enumeration_is_rejected() {
        let text = swap(
            WORKED_EXAMPLE,
            "energy_from = \"\"",
            "energy_from = \"SUNSHINE\"",
        );
        let message = refusal(&text);
        assert_names(
            &message,
            &[
                "SUNSHINE",
                "SOLAR_IN",
                "GEOTHERMAL_IN",
                "RADIATIVE_OUT",
                "BOUNDARY_EXCHANGE",
                "IMPACT",
                "VENT_BURST",
            ],
        );
        // One registry of names for the project, resolved through `ledger`. A
        // second list of strings in `config/` would let a scenario name a channel
        // the ledger does not know, and there would be something to close a
        // residual with (ADR-059).
        assert!(Channel::from_name("SUNSHINE").is_none());
        assert!(Channel::from_name("SOLAR_IN").is_some());
    }

    #[test]
    fn energy_from_light_is_rejected() {
        let text = swap(
            WORKED_EXAMPLE,
            "energy_from = \"\"",
            "energy_from = \"LIGHT\"",
        );
        let message = refusal(&text);
        assert_names(&message, &["LIGHT", "ADR-049", "second time"]);
        assert!(
            !message.contains("VENT_BURST"),
            "the LIGHT case has to fire before the enumeration check: the reason \
             is an accounting one, and a reader who wrote it needs that answer \
             rather than a list of six names. It said:\n{message}"
        );
    }

    #[test]
    fn substance_lighter_than_its_declared_composition_is_rejected() {
        // FE2, with exactly zero margin: 55.84500 against 1*55.84500. A strict
        // comparison would reject a substance of the project's own registry.
        let fe2 = r#"
[[substance]]
id = "FE2"
molar_mass = 55.84500
typical_conc = 0.01
max_conc = 1.0
partial_molar_volume = -2.4e-5
settling_radius = 0.0
diffusivity = 7.0e-10
c_p = 100.0
enthalpy_formation = 0.0
composition = { Fe = 1 }
"#;
        validated(&with_substance(WORKED_EXAMPLE, "FE2", fe2));

        // DET_L written in kg/mol: 3.553237 against 106*12.01070 + 16*14.00670 +
        // 30.97376 = 1528.21516, a gap of 430x. This is the one check in the
        // project that sees the swapped unit; the mass balance is a ratio and the
        // factor of a thousand cancels in it.
        let det_l = r#"
[[substance]]
id = "DET_L"
molar_mass = 3.553237
typical_conc = 0.01
max_conc = 1.0
partial_molar_volume = 1.0e-5
settling_radius = 0.0
diffusivity = 5.0e-10
c_p = 100.0
enthalpy_formation = 0.0
composition = { C = 106, N = 16, P = 1 }
"#;
        let message = refusal(&with_substance(WORKED_EXAMPLE, "DET_L", det_l));
        assert_names(&message, &["DET_L", "3.553237", "1528.21516", "430"]);
    }

    #[test]
    fn composition_key_absent_from_conserved_is_rejected() {
        // `Fee = 20` instead of `Fe = 20`: a load error, not a silent zero. The
        // direction is the assertion — written as a filter ("sum the names that
        // were found") this rule looks the same and returns exactly the state
        // ADR-064 closed.
        let text = swap(
            WORKED_EXAMPLE,
            "enthalpy_formation = -846000.0\ncomposition = { S = 1 }",
            "enthalpy_formation = -846000.0\ncomposition = { Fee = 20 }",
        );
        let message = refusal(&text);
        assert_names(&message, &["SO4", "Fee", "`Fe`"]);
    }

    #[test]
    fn negative_composition_entry_with_nonzero_molar_mass_is_rejected() {
        let text = swap(
            WORKED_EXAMPLE,
            "enthalpy_formation = -846000.0\ncomposition = { S = 1 }",
            "enthalpy_formation = -846000.0\ncomposition = { S = 1, C = -1 }",
        );
        let message = refusal(&text);
        assert_names(&message, &["SO4", "C", "-1", "12.0107"]);

        // The paired case: the same value against a quantity of zero mass is
        // legal, and that is what makes `charge` work at all.
        let legal = swap(WORKED_EXAMPLE, "C = 12.01070", "C = 12.01070\ncharge = 0.0");
        let legal = swap(
            &legal,
            "enthalpy_formation = -846000.0\ncomposition = { S = 1 }",
            "enthalpy_formation = -846000.0\ncomposition = { S = 1, charge = -1 }",
        );
        let legal = swap(
            &legal,
            "c_p = 103.0\nenthalpy_formation = 0.0\ncomposition = {}",
            "c_p = 103.0\nenthalpy_formation = 0.0\ncomposition = { charge = 1 }",
        );
        // Charge balances: -1 + 2*1 = +1 on the right, 0 on the left, so this one
        // is refused by the element balance and *not* by the sign rule.
        let message = refusal(&legal);
        assert!(
            message.contains("does not balance by `charge`"),
            "a negative entry against a massless quantity is legal; what is left \
             is the ordinary element balance. It said:\n{message}"
        );
    }

    // --- the required keys, refused by the schema itself --------------------

    #[test]
    fn substance_without_molar_mass_is_rejected() {
        // Through `parse` and not through `validate`: the key is required and has
        // no default, so serde refuses it. The test exists so that this is a
        // decision rather than a coincidence — exactly like
        // `seed_in_the_scenario_file_is_rejected`.
        let text = swap(WORKED_EXAMPLE, "molar_mass = 34.08088\n", "");
        let message = format!("{:#}", parse(&text).expect_err("must be refused"));
        assert_names(&message, &["molar_mass"]);
    }

    #[test]
    fn substance_without_partial_molar_volume_is_rejected() {
        // Required by the formula and not by taste: `V_occ = sum
        // (amount_i/units_per_mol_i)*V_bar_i`, and without the term neither
        // pressure nor displacement is computable (ADR-067).
        let text = swap(WORKED_EXAMPLE, "partial_molar_volume = 3.5e-5\n", "");
        let message = format!("{:#}", parse(&text).expect_err("must be refused"));
        assert_names(&message, &["partial_molar_volume"]);
    }

    #[test]
    fn substance_without_settling_radius_is_rejected() {
        // A default of "does not settle" would make a forgotten key
        // indistinguishable from an honest zero: `MINERAL` without a radius gives
        // a world where sediment never settles, under an entirely green suite.
        let text = swap(
            WORKED_EXAMPLE,
            "settling_radius = 0.0\ndiffusivity = 1.6e-9",
            "diffusivity = 1.6e-9",
        );
        let message = format!("{:#}", parse(&text).expect_err("must be refused"));
        assert_names(&message, &["settling_radius"]);
    }

    #[test]
    fn settling_substance_with_zero_partial_molar_volume_is_rejected() {
        // The conditional rule of ADR-067. The proton with `V_bar = 0` and a zero
        // radius loads — an unconditional ban would reject the project's own
        // registry — and the same proton with a radius does not.
        validated(WORKED_EXAMPLE);
        let text = swap(
            WORKED_EXAMPLE,
            "partial_molar_volume = 0.0\nsettling_radius = 0.0",
            "partial_molar_volume = 0.0\nsettling_radius = 1.0e-6",
        );
        let message = refusal(&text);
        assert_names(
            &message,
            &["H_ION", "partial_molar_volume", "settling_radius"],
        );
    }

    // --- settling: the two refusals and the one comparison (ADR-085) --------

    /// The excess density the Courant witness of ADR-085 is built on,
    /// `1353 kg/m^3` over fresh water.
    ///
    /// Not the `1650` of the worked example of ADR-067, and the difference is
    /// arithmetic rather than taste. At the derived radius limit the `f64`
    /// Courant number is exactly `1.0` for `1650` at `g = 9.80665` — the worked
    /// example lands in the bucket where the two comparisons agree — while
    /// `1353` is where the excess over one is largest over every whole excess
    /// density from 1 to 4000: `1.0000000000000007`, three ULP, and still an
    /// exact one after the fold into `Q`. A witness taken from the "== 1" bucket
    /// is green and testifies to nothing.
    const WITNESS_DELTA_RHO: f64 = 1353.0;

    /// A grain of the given radius, whose density is
    /// `1000 + WITNESS_DELTA_RHO`.
    ///
    /// `rho_bar = molar_mass*1e-3/V_bar`, so a partial molar volume of
    /// `1e-3 m^3/mol` makes the molar mass in g/mol numerically equal to the
    /// grain density in kg/m^3, and the derivation exact in `f64`. Every number
    /// here is a fixture: no substance of the corpus declares a radius, and
    /// `MINERAL` cannot — ADR-046 leaves its density undeclared and its partial
    /// molar volume is named by nothing (ADR-085).
    fn grain_record(settling_radius: f64) -> String {
        format!(
            r#"
[[substance]]
id = "GRAIN"
molar_mass = {}
typical_conc = 0.1
max_conc = 1.0
partial_molar_volume = 1.0e-3
settling_radius = {settling_radius:e}
diffusivity = 0.0
c_p = 100.0
enthalpy_formation = 0.0
composition = {{}}
"#,
            1000.0 + WITNESS_DELTA_RHO
        )
    }

    /// The whole settling process record, switched on.
    const SETTLING_ON: &str = "\n[[process]]\nid = \"settling\"\nenabled = true\n";

    /// The heaviest grain the one-axis condition admits at the witness excess
    /// density: `r <= sqrt(9*mu*dx/(2*d_rho*g*dt))` (ADR-067).
    ///
    /// Written out here rather than read off the validator, so that "the limit
    /// the message prints" and "the limit that loads" are compared against a
    /// third number instead of against each other.
    fn witness_radius_limit() -> f64 {
        (9.0 * 1.0e-3 * 1.0e-4 / (2.0 * WITNESS_DELTA_RHO * 9.806_65 * 1.0)).sqrt()
    }

    #[test]
    fn a_settling_substance_in_a_scenario_with_settling_disabled_is_rejected() {
        // ADR-085 keeps the default of the settling process at `false` and pays
        // for it with this refusal rather than with a flipped roster. The
        // argument is the one ADR-067 made about `settling_radius` itself:
        // sediment that never settles under an entirely green suite is
        // indistinguishable from an honest zero, and the difference has to
        // arrive as a message rather than as silence.
        let text = with_substance(WORKED_EXAMPLE, "GRAIN", &grain_record(1.0e-6));
        let message = refusal(&text);
        assert_names(&message, &["GRAIN", "settling_radius", "settling"]);

        // The other half, and without it this test is green on a refusal that
        // arrives from somewhere else entirely.
        validated(&format!("{text}{SETTLING_ON}"));
    }

    #[test]
    fn a_scenario_that_enables_settling_without_a_viscosity_is_rejected() {
        // The predicate is "the process is on" and never "some substance has a
        // radius", and that is dictated by the code rather than by taste:
        // `SettlePhase::fold` builds a `Settle` for **every** lane, and
        // `settling_velocity` checks `medium.mu <= 0.0` before the branch on a
        // zero radius. So a phase built at all wants a usable viscosity whatever
        // the radii are — which is why this fixture declares **no grain**.
        let no_mu = swap(WORKED_EXAMPLE, "mu = 1.0e-3\n", "");
        let message = refusal(&format!("{no_mu}{SETTLING_ON}"));
        assert_names(&message, &["mu", "physics", "settling"]);

        // Written by the radius instead, the fixture above would load and drop
        // the refusal on `Tick::new` — a load error turned into an assembly
        // error. The paired half: the same scenario with the viscosity back.
        validated(&format!("{WORKED_EXAMPLE}{SETTLING_ON}"));
    }

    #[test]
    fn the_settling_courant_is_compared_the_same_way_in_the_validator_and_in_the_process() {
        use crate::process::settle::{Grain, Medium, settling_courant, settling_velocity};

        let limit = witness_radius_limit();
        let medium = Medium {
            rho_medium: 1000.0,
            g: 9.806_65,
            mu: 1.0e-3,
        };
        let grain = Grain {
            settling_radius: limit,
            molar_mass: 1000.0 + WITNESS_DELTA_RHO,
            partial_molar_volume: 1.0e-3,
        };

        // The premise the whole test rests on, asserted rather than assumed: at
        // this excess density the `f64` Courant number at the limit is over one,
        // and the fold into `Q` brings it back to exactly one.
        let w = settling_velocity(&grain, &medium).expect("the witness velocity");
        assert!(w * 1.0 / 1.0e-4 > 1.0, "the f64 courant is not over one");
        assert!(
            settling_courant(w, 1.0, 1.0e-4).is_ok(),
            "the process accepts the grain sitting exactly on the limit"
        );

        // And so does the validator. Compared in `f64` here, the record's own
        // limit would be unreachable for 2632 of the 4000 whole excess densities
        // — the message would print a radius the same message then refuses.
        let at_the_limit = with_substance(WORKED_EXAMPLE, "GRAIN", &grain_record(limit));
        validated(&format!("{at_the_limit}{SETTLING_ON}"));

        // Past the fold: refused by both, and the refusal names the limit it
        // will accept. `2^-20` and not one ULP — one ULP of the radius moves the
        // `f64` Courant by four ULP and the `f32` behind `Q` cannot see it, so a
        // test written on "one ULP more is refused" would be asserting the
        // opposite of what this comparison is for.
        let over = limit * (1.0 + 2.0f64.powi(-20)).sqrt();
        let w_over = settling_velocity(
            &Grain {
                settling_radius: over,
                ..grain
            },
            &medium,
        )
        .expect("the velocity past the limit");
        assert!(settling_courant(w_over, 1.0, 1.0e-4).is_err());

        let past = with_substance(WORKED_EXAMPLE, "GRAIN", &grain_record(over));
        let message = refusal(&format!("{past}{SETTLING_ON}"));
        assert_names(&message, &["GRAIN", "settling_radius"]);
        assert!(
            message.contains(&format!("{limit}")),
            "the refusal has to print the limit it will accept, {limit} m; it \
             said:\n{message}"
        );
    }

    // --- scales, widths and the pair of ADR-039 -----------------------------

    #[test]
    fn substance_dynamic_range_over_2e14_is_rejected() {
        let text = swap(WORKED_EXAMPLE, "max_conc = 10.0", "max_conc = 100000.0");
        let message = refusal(&text);
        assert_names(&message, &["H2S", "max_conc", "typical_conc", "2^14"]);
        // The one refusal of the three from ADR-039 that is *not* about a pair:
        // it compares two keys of one substance and knows nothing about
        // reactions (`CONFIG_SCHEMA.md` section 13 item 21).
        assert!(
            !message.contains("h2s_oxidation"),
            "this refusal names one substance and no reaction. It said:\n{message}"
        );
    }

    #[test]
    fn scale_overflow_is_rejected() {
        // The amount at `max_conc` does not fit, with `k` from the substance's
        // *own* ceiling. It differs from the next test only in where `k` came
        // from, and the difference has to be visible in the message, or two
        // different defects get fixed blind.
        let text = swap(
            WORKED_EXAMPLE,
            "typical_conc = 0.1",
            "typical_conc = 1.0e31",
        );
        let text = swap(&text, "max_conc = 10.0", "max_conc = 1.0e31");
        let message = refusal(&text);
        assert_names(
            &message,
            &["H2S", "ceiling", "-36", "10000000000000000000", "2^28"],
        );
        assert!(
            !message.contains("h2s_oxidation"),
            "the substance is at fault here, not a pair. It said:\n{message}"
        );
    }

    #[test]
    fn reaction_with_unrepresentable_concentration_spread_is_rejected() {
        // On the real registry of SPEC section 2.3 and not on an invented one:
        // the only incompatibility the project knows is its own, water and the
        // proton in one reaction with `i32` for water.

        // (1) At the declared pair, water's `k` is raised to `e_r`, an `i32` is
        //     not enough, and water moves to `i64`. That is *not* a refusal —
        //     ADR-040 rejected "refuse to load such registries" outright, because
        //     the registry so rejected is the project's own. Through `derive`
        //     directly: the invented reaction does not balance by mass, which is
        //     a statement about the fixture and not about the scales.
        let config = parse(&water_and_proton(13)).expect("parses");
        let derived = derive(&config).expect("the pair is representable");
        let water = derived
            .substances()
            .iter()
            .find(|s| s.id == "WATER")
            .expect("water");
        assert_eq!(derived.reactions()[0].e_r, 63);
        assert_eq!(water.k_ceiling, 52, "the flat ceiling gives water k = 52");
        assert_eq!(
            water.k, 63,
            "raised to e_r by the proton in the same reaction"
        );
        assert!(water.raised_to_e_r);
        assert_eq!(water.width, crate::world::Width::Bits64);
        assert!(
            (water.amount_at_max as f64 - 5.119e11).abs() < 1e9,
            "ADR-040 prints 5.1e11 units; got {}",
            water.amount_at_max
        );

        // (2) The same pair with the stoichiometry raised further gives an amount
        //     that does not fit an `i64` either, and *that* is the refusal.
        let message = refusal(&water_and_proton(500_000_000));

        // (3) The message names the pair. Until this wave `resolve_substance` did
        //     not receive the reaction at all: the substance arrived through
        //     `.with_context` and the reaction arrived from nowhere, so a config
        //     whose fault lies in a pair could not be fixed from the message.
        assert_names(&message, &["WATER", "photosynthesis", "i64", "max_conc"]);
    }

    #[test]
    fn stoichiometry_in_storage_units_fits_i32() {
        // `nu_i = s_i * 2^(k_i - e_r) <= 2^31 - 1`, naming the pair. Inside one
        // reaction this is a theorem (see below); it takes two reactions to break
        // it, because water's scale is raised by `proton_pump` and its `nu` is
        // taken in `brining`.
        let message = refusal(TWO_REACTIONS);
        assert_names(&message, &["WATER", "brining", "nu", "i32"]);

        // The second inequality, `nu_i <= beta * typ_conc_i * V_voxel * 2^k_i`,
        // cannot be broken by any config while `e_r` comes from the scarcest
        // participant: substituting the definition of `e_r` gives
        // `nu_i / (beta*typ_i*V*2^k_i) = (s_i/(beta*typ_i*V)) / 2^e_r <= 1`. It is
        // the only check that sees an `e_r` taken from the most abundant
        // participant instead, without knowing which participant was meant — so
        // what is asserted here is the theorem, with the margin it holds by.
        let derived = validated(WORKED_EXAMPLE);
        let config = parse(WORKED_EXAMPLE).expect("parses");
        let reaction = &derived.reactions()[0];
        for entry in &reaction.nu {
            let substance = &config.substance[entry.substance as usize];
            let pool = config.beta
                * substance.typical_conc
                * derived.v_voxel()
                * 2f64.powi(i32::from(derived.substances()[entry.substance as usize].k));
            assert!(
                (entry.value.abs() as f64) <= pool,
                "nu = {} of `{}` over beta of its typical pool {pool:e}",
                entry.value,
                substance.id
            );
            assert!(entry.value.abs() <= i64::from(i32::MAX));
        }
    }

    #[test]
    fn reaction_stoichiometry_must_be_positive_integers() {
        // Zero and negative: the side is chosen by the section, never by the sign.
        let zero = swap(
            WORKED_EXAMPLE,
            "inputs = { H2S = 1, O2 = 2 }",
            "inputs = { H2S = 0, O2 = 2 }",
        );
        assert_names(
            &refusal(&zero),
            &["h2s_oxidation", "inputs.H2S", "0", "> 0"],
        );

        let negative = swap(
            WORKED_EXAMPLE,
            "inputs = { H2S = 1, O2 = 2 }",
            "inputs = { H2S = -1, O2 = 2 }",
        );
        assert_names(&refusal(&negative), &["-1", "> 0"]);

        // A fractional value never reaches the check at all — it does not parse
        // into the `i32` of the schema — and that is asserted too, so that the
        // refusal on zero does not read as the only case.
        let fractional = swap(
            WORKED_EXAMPLE,
            "inputs = { H2S = 1, O2 = 2 }",
            "inputs = { H2S = 0.5, O2 = 2 }",
        );
        assert!(
            parse(&fractional).is_err(),
            "a fractional coefficient is not an approximation, it is an error \
             (ADR-026), and the type is what says so"
        );
    }

    // --- the energy scale and the temperature range -------------------------

    #[test]
    fn energy_scale_incompatible_with_a_reaction_enthalpy_is_rejected() {
        // A tiny enthalpy pushes the significance bound of `nu_E` past what the
        // field can hold at either width. An empty window means a `nu_E` rounded
        // to zero: the reaction proceeds, matter is converted, no heat is
        // released, and the energy ledger closes because the same zero stands on
        // both sides.
        let text = swap(WORKED_EXAMPLE, "enthalpy = -846000.0", "enthalpy = -1.0e-6");
        let text = swap(
            &text,
            "enthalpy_formation = -846000.0",
            "enthalpy_formation = -1.0e-6",
        );
        let message = refusal(&text);
        assert_names(&message, &["k_E window is empty", "H_max"]);
    }

    #[test]
    fn t_ref_outside_the_declared_temperature_range_is_rejected() {
        let text = swap(WORKED_EXAMPLE, "T_ref = 298.15", "T_ref = 400.0");
        let message = refusal(&text);
        assert_names(&message, &["T_ref", "400", "273.15", "323.15"]);
    }

    // --- fields and processes ----------------------------------------------

    #[test]
    fn field_over_n_max_substeps_is_rejected() {
        // Enthalpy at the default `lod = 0`: 84 substeps against N_MAX = 64.
        let text = swap(WORKED_EXAMPLE, "lod = 2", "lod = 0");
        let message = refusal(&text);
        assert_names(&message, &["enthalpy", "84", "64"]);
    }

    #[test]
    fn every_n_ticks_on_diffusive_field_is_rejected() {
        let text = swap(
            WORKED_EXAMPLE,
            "id = \"diffusion\"\nenabled = true",
            "id = \"diffusion\"\nenabled = true\nevery_n_ticks = 2",
        );
        let message = refusal(&text);
        assert_names(&message, &["diffusion", "every_n_ticks", "2"]);

        // The hole this test used to record — the ban tied to a literal, so that
        // a scenario calling its transport process something else walked past
        // ADR-030 in silence — is closed by the roster of ADR-065: there is no
        // other name to give it. Asserted from the other side, so that reopening
        // the roster reopens this too.
        let renamed = swap(
            WORKED_EXAMPLE,
            "id = \"diffusion\"\nenabled = true",
            "id = \"transport\"\nenabled = true\nevery_n_ticks = 2",
        );
        assert_names(&refusal(&renamed), &["transport", "roster"]);

        // The third input, and it is an *extension of this name* rather than a
        // name of its own (ADR-082). Section 10 states the ban over the **field**
        // and not over the operator, and pressure moves the same `amount[]` whose
        // substances declare a `diffusivity`; two acceptance names on one rule
        // would claim in the register that there are two rules, and the next
        // record to touch the rule would fix one of them.
        let pressure = swap(
            WORKED_EXAMPLE,
            "id = \"pressure\"\nenabled = false",
            "id = \"pressure\"\nenabled = true\ntheta_max = 0.5\nevery_n_ticks = 2",
        );
        let message = refusal(&pressure);
        assert_names(&message, &["pressure", "every_n_ticks", "2", "field"]);

        // And the hole ADR-082 closes with a number: the product
        // `theta_max*every_n_ticks` enters neither half of the window. A
        // `theta_max` under the floor is refused whatever the schedule is, and a
        // schedule over one is refused whatever `theta_max` is — two independent
        // refusals, where one inequality over the product would have let
        // `theta_max = 0.005` at `every_n_ticks = 8` through on a product of 0.04.
        let both = swap(
            WORKED_EXAMPLE,
            "id = \"pressure\"\nenabled = false",
            "id = \"pressure\"\nenabled = true\ntheta_max = 0.005\nevery_n_ticks = 8",
        );
        refusal(&both);
        let floor_only = swap(
            WORKED_EXAMPLE,
            "id = \"pressure\"\nenabled = false",
            "id = \"pressure\"\nenabled = true\ntheta_max = 0.005",
        );
        assert_names(&refusal(&floor_only), &["theta_max", "0.005"]);
        let schedule_only = swap(
            WORKED_EXAMPLE,
            "id = \"pressure\"\nenabled = false",
            "id = \"pressure\"\nenabled = true\ntheta_max = 0.5\nevery_n_ticks = 8",
        );
        assert_names(&refusal(&schedule_only), &["every_n_ticks", "8"]);
    }

    /// The pressure record with a chosen `theta_max`, and nothing else moved.
    fn with_pressure(text: &str, theta_max: &str) -> String {
        swap(
            text,
            "id = \"pressure\"\nenabled = false",
            &format!("id = \"pressure\"\nenabled = true\ntheta_max = {theta_max}"),
        )
    }

    /// `Theta_sup` of [`WORKED_EXAMPLE`], recomputed here out of the declared
    /// `max_conc` and `partial_molar_volume` rather than read back from the
    /// derivation: `sum over V_bar > 0 of max_conc * V_bar`.
    ///
    /// The proton contributes exactly zero — `V_bar(H+) = 0` on the accepted
    /// single-ion scale (ADR-067) — and there is no water in this fixture, which
    /// is why the number is three orders below the 1.005 of the shipped registry.
    fn worked_example_occupancy_sup() -> f64 {
        10.0 * 3.5e-5 + 1.0 * 3.1e-5 + 100.0 * 1.4e-5
    }

    /// `Theta_typ` of [`WORKED_EXAMPLE`], the same expression over `typical_conc`.
    fn worked_example_occupancy_typ() -> f64 {
        0.1 * 3.5e-5 + 0.25 * 3.1e-5 + 28.0 * 1.4e-5
    }

    #[test]
    fn pressure_without_theta_max_is_rejected() {
        // The limiting overflow is required when the process is on and has no
        // default (ADR-082, on the precedent of `i_surface` and `u_conv_max`). A
        // plausible number put in as a default would be indistinguishable from a
        // decision, and because the scheme conserves exactly both halves of the
        // invariant would close over it for ever.
        let enabled = swap(
            WORKED_EXAMPLE,
            "id = \"pressure\"\nenabled = false",
            "id = \"pressure\"\nenabled = true",
        );
        let message = refusal(&enabled);
        assert_names(&message, &["pressure", "theta_max", "no default"]);

        // The predicate, and this is the half with teeth. `enabled.unwrap_or(the
        // process's own default)` and never `enabled == Some(true)`: a scenario
        // that writes no pressure record at all is legal today, because the
        // default is `false` — and on the day the locks come off and the default
        // flips, the comparison against `Some(true)` would let every such scenario
        // through with no `theta_max` at all, while a check for the record's
        // presence refuses this legal one today (ADR-065).
        let removed = swap(
            WORKED_EXAMPLE,
            "\n[[process]]\nid = \"pressure\"\nenabled = false\n",
            "\n",
        );
        validated(&removed);
    }

    #[test]
    fn theta_max_below_six_times_the_declared_peak_occupancy_is_rejected() {
        // The floor of the window is `6*Theta_sup` — over the **occupancy**
        // `V_occ/V_voxel` and not over the overflow `theta = Theta - 1`. On the
        // shipped registry the two differ by a factor of 190, and every `theta_max`
        // would pass a floor built on the overflow while the scheme oscillated
        // with matter conserved exactly and both ledgers green (ADR-082).
        let sup = worked_example_occupancy_sup();
        let floor = 6.0 * sup;
        let message = refusal(&with_pressure(WORKED_EXAMPLE, "0.005"));
        assert_names(
            &message,
            &[
                "theta_max",
                "0.005",
                &format!("{floor:.4e}"),
                &format!("{sup:.4e}"),
            ],
        );
        validated(&with_pressure(WORKED_EXAMPLE, "0.5"));

        // The assertion with teeth: the upper estimate is taken over `max_conc`
        // and never over `typical_conc`. The two differ by 0.2% on the shipped
        // registry, so swapping them moves no number in the report and fails no
        // other test — and inverts the window on a registry with the dynamic range
        // ADR-039 allows.
        let richer = swap(
            &with_pressure(WORKED_EXAMPLE, "0.005"),
            "max_conc = 100.0",
            "max_conc = 200.0",
        );
        let raised = 6.0 * (10.0 * 3.5e-5 + 1.0 * 3.1e-5 + 200.0 * 1.4e-5);
        assert!(raised > floor * 1.5, "the fixture does not move the floor");
        assert_names(&refusal(&richer), &[&format!("{raised:.4e}")]);

        let typical = swap(
            &with_pressure(WORKED_EXAMPLE, "0.005"),
            "typical_conc = 28.0",
            "typical_conc = 56.0",
        );
        assert_names(&refusal(&typical), &[&format!("{floor:.4e}")]);
    }

    #[test]
    fn theta_max_beyond_the_declared_run_horizon_is_rejected() {
        // The ceiling is derived from the declared horizon rather than set as a
        // constant: `N_max^2*theta_max/(pi^2*Theta_typ) <= 1e6` ticks (SPEC
        // section 13, criterion S0). At 64 voxels across and this registry the
        // ceiling is 0.97, so `theta_max = 2` asks for a domain that never
        // equilibrates inside the horizon it is measured over (ADR-082).
        let typ = worked_example_occupancy_typ();
        let tau = 64.0 * 64.0 * 2.0 / (std::f64::consts::PI.powi(2) * typ);
        let message = refusal(&with_pressure(WORKED_EXAMPLE, "2.0"));
        assert_names(&message, &[&format!("{tau:.4e}"), "1e6", "64"]);
        validated(&with_pressure(WORKED_EXAMPLE, "0.5"));

        // The ceiling is a function of the grid and not a constant. Written with
        // `N_MAX = 64` from `process/diffuse.rs` — a different quantity with the
        // same spelling, and one this module already imports — it compiles, gives
        // the 64-cubed ceiling on every grid, and both shipped grids either agree
        // with it or are let through.
        let coarser = swap(
            &with_pressure(WORKED_EXAMPLE, "2.0"),
            "nx = 64\nny = 64\nnz = 64",
            "nx = 32\nny = 32\nnz = 32",
        );
        validated(&coarser);
    }

    #[test]
    fn load_reports_the_pressure_relaxation_time_and_the_stability_margin() {
        // ADR-082 asks the loader to print the relaxation time of the longest mode
        // and the margin the declared `theta_max` leaves against the oscillation
        // threshold — **with the threshold it is measured against**, because
        // `alpha = 8e-4` reads as "small" exactly as well as it reads as "large".
        // Both inputs of the window are printed beside it: on a registry whose
        // dynamic range is wide the two come apart, and the report is the only
        // place that is visible.
        let sup = worked_example_occupancy_sup();
        let typ = worked_example_occupancy_typ();
        let theta_max = 0.5;
        let tau = 64.0 * 64.0 * theta_max / (std::f64::consts::PI.powi(2) * typ);
        let margin = 1.0 - 6.0 * typ / theta_max;

        let report = validated(&with_pressure(WORKED_EXAMPLE, "0.5")).report();
        for wanted in [
            format!("{tau:.4e}"),
            format!("{:.1}", margin * 100.0),
            format!("{sup:.4e}"),
            format!("{typ:.4e}"),
            "1/6".to_string(),
        ] {
            assert!(
                report.contains(&wanted),
                "the report has to name `{wanted}`:\n{report}"
            );
        }

        // And no line at all on a scenario whose pressure is off, which is every
        // scenario in the repository today.
        assert!(
            !validated(WORKED_EXAMPLE).report().contains("pressure:"),
            "a scenario with pressure off has no pressure line"
        );
    }

    #[test]
    fn the_peak_occupancy_ignores_negative_partial_molar_volumes() {
        // Not an acceptance name; the one guard against a sum that looks more
        // correct than the rule. Electrostriction makes `V_bar` negative —
        // `PO4^3-` is about `-4.0e-5` (ADR-067) — so a substance declared with one
        // *lowers* an unfiltered sum, lowers the floor of the window, and lets an
        // unstable `theta_max` load with nothing else in the project able to see
        // it.
        let sup = |text: &str| -> f64 {
            validated(text)
                .pressure()
                .expect("the fixture enables pressure")
                .occupancy_sup
        };

        let base = with_pressure(WORKED_EXAMPLE, "0.5");
        let electrostrictive = with_substance(
            &base,
            "PO4",
            r#"
[[substance]]
id = "PO4"
molar_mass = 94.97136
typical_conc = 1.0e-3
max_conc = 1.0e-1
partial_molar_volume = -4.0e-5
settling_radius = 0.0
diffusivity = 8.0e-10
c_p = 104.0
enthalpy_formation = 0.0
composition = { P = 1 }
"#,
        );

        assert_eq!(
            sup(&electrostrictive),
            sup(&base),
            "a negative partial molar volume moved the peak occupancy"
        );

        // And the floor moved with it or it did not: the same fixture at a
        // `theta_max` under the floor has to be refused by the *same* number.
        let under = swap(&electrostrictive, "theta_max = 0.5", "theta_max = 0.005");
        assert_names(
            &refusal(&under),
            &[&format!("{:.4e}", 6.0 * worked_example_occupancy_sup())],
        );
    }

    #[test]
    fn the_two_occupancy_bounds_do_not_share_an_input() {
        // Not an acceptance name either. The floor is taken over `max_conc` and
        // the ceiling over `typical_conc`, and on the shipped registry the two
        // come out at 1.005285 and 1.003288 — 0.2% apart. Swapped, they move no
        // number in the report by anything a reader would notice and fail no other
        // test; on a registry with the dynamic range ADR-039 allows, `2^14`, the
        // window inverts.
        let bounds = |text: &str| -> (f64, f64) {
            let derived = validated(text);
            let pressure = derived.pressure().expect("the fixture enables pressure");
            (pressure.occupancy_sup, pressure.occupancy_typ)
        };

        let base = with_pressure(WORKED_EXAMPLE, "0.5");
        let (sup, typ) = bounds(&base);

        let richer_ceiling = swap(&base, "max_conc = 100.0", "max_conc = 200.0");
        let (moved_sup, same_typ) = bounds(&richer_ceiling);
        assert!(moved_sup > sup, "max_conc did not move Theta_sup");
        assert_eq!(same_typ, typ, "max_conc moved Theta_typ");

        let richer_typical = swap(&base, "typical_conc = 28.0", "typical_conc = 56.0");
        let (same_sup, moved_typ) = bounds(&richer_typical);
        assert_eq!(same_sup, sup, "typical_conc moved Theta_sup");
        assert!(moved_typ > typ, "typical_conc did not move Theta_typ");
    }

    #[test]
    fn substance_count_over_s_max_is_rejected() {
        // At most `MAX_SUBSTANCES = S_MAX - 1 = 31`: one of the thirty-two
        // indices is enthalpy's, which is a participant of the `nu` vector beside
        // any substance (ADR-041).
        let fillers =
            |n: usize| -> String { (0..n).map(filler_substance).collect::<Vec<_>>().concat() };
        validated(&format!("{COUNTING_HEADER}{}", fillers(31)));
        let message = refusal(&format!("{COUNTING_HEADER}{}", fillers(32)));
        assert_names(&message, &["32", "31"]);
    }

    #[test]
    fn reaction_count_over_r_max_is_rejected() {
        // At most `R_MAX = 64`: the size of the local `want[R_MAX]` array in the
        // reaction kernel, where allocation is forbidden.
        let with_reactions = |n: usize| -> String {
            let reactions: String = (0..n).map(filler_reaction).collect::<Vec<_>>().concat();
            format!("{COUNTING_HEADER}{COUNTING_PAIR}{reactions}")
        };
        validated(&with_reactions(64));
        let message = refusal(&with_reactions(65));
        assert_names(&message, &["65", "64"]);
    }

    // --- transport ----------------------------------------------------------

    #[test]
    fn courant_violation_is_rejected() {
        // `u*dt/dx <= 1` on the conservative estimate `(1 + stir_fraction) *
        // u_conv_max` (ADR-069). At dx = 100 um and dt = 1 s the limit is
        // 100 um/s.
        let text = swap(WORKED_EXAMPLE, "u_conv_max = 1.0e-5", "u_conv_max = 2.0e-4");
        let message = refusal(&text);
        assert_names(&message, &["velocity_field", "Courant", "0.0001"]);

        // The second assertion matters more than the first: the condition has
        // *two* inputs and they do not add up. Each operator carries its own
        // inequality with its own name in the message, because under Lie-Trotter
        // splitting every operator is applied over a full step (ADR-036) and a
        // shared budget is a single number somebody relaxes once.
        let config = parse(WORKED_EXAMPLE).expect("parses");
        let bounds = speed_bounds(&config, enthalpy_lod(&config)).expect("bounds");
        assert_eq!(bounds.len(), 1, "one operator, one inequality");
        assert_eq!(bounds[0].operator, "velocity_field");

        // The second input arrived with ADR-085, and this is where it lands: a
        // scenario that enables settling and declares a grain carries **two**
        // bounds, and the two do not add up. One outgoing face against six,
        // because settling transports along a single axis (ADR-067), and a
        // verdict of its own, because the operator folded it.
        let settling = format!(
            "{}{SETTLING_ON}",
            with_substance(WORKED_EXAMPLE, "GRAIN", &grain_record(1.0e-6))
        );
        let config = parse(&settling).expect("parses");
        let bounds = speed_bounds(&config, enthalpy_lod(&config)).expect("bounds");
        assert_eq!(bounds.len(), 2, "two operators, two inequalities");
        assert_eq!(bounds[1].operator, "settling");
        assert_eq!(bounds[1].outgoing_faces, 1);
        assert!(
            matches!(bounds[1].courant, CourantVerdict::FoldedByTheOperator(_)),
            "the settling bound was judged in f64, where the record's own limit \
             is unreachable for two thirds of the whole excess densities"
        );
        assert!(
            matches!(bounds[0].courant, CourantVerdict::InF64),
            "the velocity field carries no folded number and is judged here"
        );
    }

    #[test]
    fn outflow_bound_violation_is_rejected() {
        // The load-bearing case: a config between `dx/(6*dt)` and `dx/dt` passes
        // the Courant check and fails this one. Without it the two checks could
        // be the same line, the second would stay green forever, and a voxel with
        // a diverging flow would go negative on a run.
        let text = swap(WORKED_EXAMPLE, "u_conv_max = 1.0e-5", "u_conv_max = 5.0e-5");
        let message = refusal(&text);
        assert_names(&message, &["velocity_field", "outflow", "6 faces"]);
        assert!(
            !message.contains("Courant number"),
            "5e-5 m/s passes the Courant condition (0.5) and fails the outflow \
             one (3.0); the messages must not be the same line. It said:\n{message}"
        );

        // The limit is `dx/(6*dt) = 16.7 um/s` at dx = 100 um, dt = 1 s, and the
        // numbers of the corpus — 100 um/s in SPEC section 3, 50 um/s in ADR-054
        // — do not load. That is not a defect of the validator (ADR-069).
        for speed in ["1.0e-4", "5.0e-5"] {
            let text = swap(
                WORKED_EXAMPLE,
                "u_conv_max = 1.0e-5",
                &format!("u_conv_max = {speed}"),
            );
            assert!(!refusal(&text).is_empty());
        }
        // And 16.7 um/s is where it stops being refused.
        validated(&swap(
            WORKED_EXAMPLE,
            "u_conv_max = 1.0e-5",
            "u_conv_max = 1.66e-5",
        ));
        assert!(
            !refusal(&swap(
                WORKED_EXAMPLE,
                "u_conv_max = 1.0e-5",
                "u_conv_max = 1.67e-5"
            ))
            .is_empty()
        );
    }

    #[test]
    fn velocity_field_without_u_conv_max_is_rejected() {
        // Enabled and without the key: refused.
        let text = swap(WORKED_EXAMPLE, "u_conv_max = 1.0e-5\n", "");
        assert_names(&refusal(&text), &["velocity_field", "u_conv_max"]);

        // No record at all: legal, and the default is `false`. Absence is not
        // enabling — reading `None` as `true` breaks `hello.toml` and every
        // scenario without the section.
        validated(&swap(WORKED_EXAMPLE, VELOCITY_FIELD, ""));

        // Disabled and without the key: legal. The mirror error is quieter — an
        // enabled process without `u_conv_max` treated as disabled would throw
        // away the only live input of the Courant condition.
        let disabled = swap(
            WORKED_EXAMPLE,
            "id = \"velocity_field\"\nenabled = true\nl_c = 3.2e-3\nu_conv_max = 1.0e-5",
            "id = \"velocity_field\"\nenabled = false",
        );
        validated(&disabled);
    }

    /// A `[[process]]` record for the light, with whatever keys a case needs.
    fn light_record(body: &str) -> String {
        format!("\n[[process]]\nid = \"light\"\n{body}\n")
    }

    /// The worked example with a light record appended.
    fn with_light(body: &str) -> String {
        format!("{WORKED_EXAMPLE}{}", light_record(body))
    }

    #[test]
    fn light_enabled_without_an_irradiance_is_rejected() {
        // The precedent is `velocity_field_without_u_conv_max_is_rejected`
        // (ADR-069): a parameter with no default, required exactly when its
        // process is on.
        assert_names(
            &refusal(&with_light("enabled = true")),
            &["light", "i_surface"],
        );

        // **Three cases, because the predicate is "enabled", not "wrote
        // `enabled = true`".** ADR-065: an absent record means the process's own
        // default, not an absent process, and the loader materialises the whole
        // roster before the hash. A check written as `enabled == Some(true)` lets
        // through a scenario relying on the default; a check written as "is there
        // a record" refuses a legal dark box.
        //
        // A record with no `enabled` and no record at all both take the light's
        // default, which is `false` (ADR-076), so both are legal here — and the
        // day that default changes, this test is what turns red rather than a
        // scenario.
        const { assert!(!LIGHT_ENABLED_BY_DEFAULT) };
        validated(&with_light("k_w = 0.04"));
        validated(WORKED_EXAMPLE);

        // Enabled *with* the key is legal as far as this rule goes; the lit
        // scenario meets its own refusal below.
        validated(&with_light("enabled = true\ni_surface = 0.0"));
    }

    #[test]
    fn a_negative_surface_irradiance_is_rejected() {
        // Not tidiness. A negative irradiance gives a negative absorption in every
        // voxel and so *removes* energy through a channel SPEC section 7 declares
        // an input — and the residual stays at zero while it happens, because the
        // counter and the enthalpy move together. The same class as
        // `a_negative_amount_amplifies_the_beam_rather_than_being_clamped`, except
        // that this one is preventable at load.
        for bad in ["-1.0", "nan", "inf"] {
            let text = with_light(&format!("enabled = true\ni_surface = {bad}"));
            assert_names(&refusal(&text), &["i_surface"]);
        }

        // Zero stays legal: it is the closed-box scenario, and it costs no branch
        // in any kernel (ADR-076).
        validated(&with_light("enabled = true\ni_surface = 0.0"));
    }

    #[test]
    fn a_modulation_fraction_outside_the_unit_interval_is_rejected() {
        // Both fractions, not one: a check written for the day and copied nowhere
        // is green and silent about the season.
        for key in ["daily_fraction", "seasonal_fraction"] {
            for bad in ["-0.1", "1.5", "nan"] {
                let text = with_light(&format!(
                    "enabled = true\ni_surface = 0.0\n{key} = {bad}\n{key_period} = 4.0",
                    key_period = if key == "daily_fraction" {
                        "daily_period"
                    } else {
                        "seasonal_period"
                    }
                ));
                assert_names(&refusal(&text), &[key]);
            }
        }

        // The boundaries themselves are legal, on the model of `stir_fraction`.
        validated(&with_light(
            "enabled = true\ni_surface = 0.0\ndaily_fraction = 1.0\ndaily_period = 4.0\n\
             seasonal_fraction = 0.0",
        ));
    }

    #[test]
    fn a_modulation_period_that_is_not_a_whole_number_of_ticks_is_rejected() {
        // The exactness of the period mean stands on an integer `N`: at a
        // fractional one `A_N` stops normalising and `i_surface` quietly stops
        // meaning "the mean irradiance over a period".
        //
        // **At `dt != 1`**, or the test checks that the period is a whole number
        // of *seconds* and not that the quotient is a whole number of ticks. The
        // whole corpus runs at `dt = 1`, so that mistake would be invisible
        // everywhere else.
        // `u_conv_max` moves with `dt`: doubling the tick doubles every Courant
        // number, and without this the fixture is refused for the transport
        // rather than for the period.
        let text = swap(WORKED_EXAMPLE, "dt = 1.0", "dt = 2.0");
        let text = swap(&text, "u_conv_max = 1.0e-5", "u_conv_max = 5.0e-6");
        let text = format!(
            "{text}{}",
            light_record(
                "enabled = true\ni_surface = 0.0\ndaily_fraction = 0.5\ndaily_period = 9.0"
            )
        );
        assert_names(&refusal(&text), &["daily_period", "9", "2", "4.5"]);

        // The same period at the same `dt`, made whole: eight seconds is four
        // ticks.
        let text = swap(WORKED_EXAMPLE, "dt = 1.0", "dt = 2.0");
        let text = swap(&text, "u_conv_max = 1.0e-5", "u_conv_max = 5.0e-6");
        validated(&format!(
            "{text}{}",
            light_record(
                "enabled = true\ni_surface = 0.0\ndaily_fraction = 0.5\ndaily_period = 8.0"
            )
        ));
    }

    #[test]
    fn a_modulation_period_under_three_ticks_is_rejected() {
        // Three, and not the natural "at least two". At `N = 2` the samples of
        // `max(0, sin)` are taken at phases `0` and `pi`, both exactly zero: the
        // sum is zero, `A_N` divides by it, and the day is dark for ever at any
        // `f_d > 0`. At `N = 3` the pattern is degenerate but alive,
        // `A_3 = 3.464`.
        let two =
            with_light("enabled = true\ni_surface = 0.0\ndaily_fraction = 1.0\ndaily_period = 2.0");
        assert_names(&refusal(&two), &["daily_period", "2", "three"]);

        let three =
            with_light("enabled = true\ni_surface = 0.0\ndaily_fraction = 1.0\ndaily_period = 3.0");
        validated(&three);

        // And the seasonal half by the same rule.
        let two = with_light(
            "enabled = true\ni_surface = 0.0\nseasonal_fraction = 1.0\nseasonal_period = 2.0",
        );
        assert_names(&refusal(&two), &["seasonal_period", "three"]);
    }

    /// [`WORKED_EXAMPLE`] with nothing that trades with the outside: `z_max`
    /// closed and the whole `[boundary.reservoir]` section gone.
    ///
    /// Both have to go together. A reservoir left behind on a world with no
    /// `exchange` face is refused by section 6 on its own
    /// (`exchange_face_without_a_reservoir_is_rejected` runs that mirror), and a
    /// fixture refused for *that* would prove nothing at all about the light.
    fn sealed_box() -> String {
        swap(
            &swap(WORKED_EXAMPLE, "z_max = \"exchange\"", "z_max = \"closed\""),
            RESERVOIR,
            "",
        )
    }

    #[test]
    fn a_lit_scenario_without_an_exchange_face_is_refused() {
        // The first of the two rules ADR-084 puts where the blanket refusal of
        // ADR-076 stood. The sink of S0 is the `exchange` face that is already
        // built — steps `c` and `d` credit `BOUNDARY_EXCHANGE` with the enthalpy
        // that crosses it — so a sealed box is a world where absorbed light has
        // nowhere to go, and that is what this refuses.
        let sealed = sealed_box();
        let message = refusal(&format!(
            "{sealed}{}",
            light_record("enabled = true\ni_surface = 0.7")
        ));
        assert_names(&message, &["i_surface", "exchange"]);

        // The mirror, and it carries the weight rather than decorating: the same
        // sealed box with the light off at the source has to load. A rule that
        // failed it would be forbidding a legal dark closed world instead of a
        // lit sealed one, and every assertion above would still be green.
        validated(&format!(
            "{sealed}{}",
            light_record("enabled = true\ni_surface = 0.0")
        ));
    }

    #[test]
    fn a_lit_scenario_whose_steady_state_leaves_the_declared_range_is_refused() {
        // The second rule, and the two ways of getting the ceiling wrong are
        // caught by two different halves of this test. Which half catches which
        // is worth writing down, because they are very unequal in size and the
        // obvious reading has them the other way round.
        //
        // The **pair astride the threshold** catches a ceiling off by the volume:
        // dividing `C_cell_out` by `V_voxel` instead of `V_cell` is 64 times too
        // large at `lod = 2`, so `0.75` would sit far under it, the refusal would
        // never fire and `refusal` below would fail. The `0.7` half is the mirror
        // for a ceiling too small.
        //
        // The **exact number in the message** is what catches the other one, and
        // it is the only thing that does. Summing over the domain's
        // `typical_conc` instead of the reservoir's `conc_out` moves the ceiling
        // by the one substance in which the two differ here — H2S at 0.1 against
        // 0.0 — which is 0.7228 against 0.7203, four tenths of a percent, deep
        // *inside* the pair and invisible to it. `assert_names` sees it because
        // it demands the printed `0.720315`, so relaxing that substring to
        // something looser throws the property away with no test turning red.
        //
        // The fixture is deliberately [`WORKED_EXAMPLE`] and **not** the shipped
        // scenario, for that same substitution: the shipped one carries WATER at
        // 55500 mol/m^3 in both tables, which swamps everything else, so the two
        // sums agree there to every digit a report would print. Moving this test
        // onto the shipped scenario as the "more real" one would lose the
        // property without failing.
        //
        // Recomputed from the fixture rather than quoted from ADR-084:
        //   conductance = k_ex * sum(conc_out_i * c_p_i)
        //               = 1e-5 * (0.0*100 + 0.25*101 + 28.0*102 + 1e-4*103)
        //               = 1e-5 * 2881.2603 = 2.8812603e-2 W/(m^2*K)
        //   ceiling     = conductance * (t_max - t_out)
        //               = 2.8812603e-2 * (323.15 - 298.15) = 0.720315 W/m^2
        let message = refusal(&with_light("enabled = true\ni_surface = 0.75"));
        assert_names(&message, &["i_surface", "0.75", "0.720315", "323.15"]);

        // The other side of the same threshold loads.
        validated(&with_light("enabled = true\ni_surface = 0.7"));
    }

    #[test]
    fn structure_length_below_the_temperature_cell_is_rejected() {
        // The same `l_c` at two coarsenings, with different outcomes: `dx_coarse`
        // is the step of the *enthalpy* grid. Taken from the fine `dx`, `r` at
        // `lod = 2` comes out four times too large, the refusal never fires, the
        // wide difference degenerates into a narrow one, and the wavelength
        // selection of ADR-069 disappears under a velocity field that still looks
        // like a velocity field.
        //
        // The thermal diffusivity is lowered so that `lod = 0` stays under N_MAX
        // and the two cases differ in one thing only.
        let base = swap(
            WORKED_EXAMPLE,
            "thermal_diffusivity = 1.4e-7",
            "thermal_diffusivity = 1.0e-7",
        );
        let base = swap(&base, "l_c = 3.2e-3", "l_c = 3.0e-4");

        // lod 0: dx_coarse = 1e-4, r = round(1.5) = 2.
        validated(&swap(&base, "lod = 2", "lod = 0"));

        // lod 2: dx_coarse = 4e-4, r = round(0.375) = 0.
        let message = refusal(&base);
        assert_names(&message, &["l_c", "0.0003", "enthalpy", "0.0004"]);
    }

    #[test]
    fn stir_period_missing_with_stirring_on_is_rejected() {
        let text = swap(
            WORKED_EXAMPLE,
            "l_c = 3.2e-3",
            "l_c = 3.2e-3\nstir_fraction = 0.5",
        );
        assert_names(&refusal(&text), &["stir_period", "stir_fraction", "0.5"]);

        // Zero is a declared branch and not "unset": at exactly zero the noise
        // kernel is not dispatched at all, which is what makes the default free.
        validated(&swap(
            WORKED_EXAMPLE,
            "l_c = 3.2e-3",
            "l_c = 3.2e-3\nstir_fraction = 0.0",
        ));
        validated(&swap(
            WORKED_EXAMPLE,
            "l_c = 3.2e-3",
            "l_c = 3.2e-3\nstir_fraction = 0.5\nstir_period = 100.0",
        ));
    }

    // --- the boundary and the reservoir -------------------------------------

    #[test]
    fn exchange_face_without_a_reservoir_is_rejected() {
        // Both directions. A face without the section:
        assert_names(
            &refusal(&swap(WORKED_EXAMPLE, RESERVOIR, "")),
            &["z_max", "exchange"],
        );
        // and the section without a face, because a declared reservoir that
        // touches nothing reads as a working boundary.
        let closed = swap(WORKED_EXAMPLE, "z_max = \"exchange\"", "z_max = \"closed\"");
        assert_names(&refusal(&closed), &["reservoir", "exchange"]);
    }

    #[test]
    fn reservoir_missing_a_substance_is_rejected() {
        // From the registry to the table and not the other way round: a default
        // of zero *is* an infinite sink. Written as "every key of conc_out
        // resolves" the rule is green on an empty table.
        let text = swap(WORKED_EXAMPLE, ", H_ION = 1.0e-4 }", " }");
        assert_names(&refusal(&text), &["H_ION", "conc_out", "infinite sink"]);
    }

    #[test]
    fn a_reservoir_naming_an_unknown_substance_is_rejected() {
        let text = swap(
            WORKED_EXAMPLE,
            "H_ION = 1.0e-4 }",
            "H_ION = 1.0e-4, H_IONN = 1.0 }",
        );
        assert_names(&refusal(&text), &["H_IONN", "conc_out"]);
    }

    #[test]
    fn reservoir_concentration_above_max_conc_is_rejected() {
        // In mol/m^3 against the declared numbers, not against rounded storage
        // units: the case sitting exactly on the bound is lost after rounding.
        let text = swap(WORKED_EXAMPLE, "SO4 = 28.0,", "SO4 = 500.0,");
        assert_names(&refusal(&text), &["SO4", "500", "100"]);
        // Exactly on the bound is legal.
        validated(&swap(WORKED_EXAMPLE, "SO4 = 28.0,", "SO4 = 100.0,"));
    }

    #[test]
    fn exchange_velocity_above_the_courant_limit_is_rejected() {
        // `alpha_ex = k_ex*dt/dx <= 1`: at dx = 100 um and dt = 1 s that is
        // `k_ex <= 1e-4 m/s`, and the piston velocity of oxygen, 1e-5, leaves a
        // factor of ten.
        let message = refusal(&swap(WORKED_EXAMPLE, "k_ex = 1.0e-5", "k_ex = 2.0e-4"));
        assert_names(&message, &["k_ex", "alpha_ex", "0.0002", "0.0001"]);
        validated(&swap(WORKED_EXAMPLE, "k_ex = 1.0e-5", "k_ex = 1.0e-4"));
        // Written over dx^2, by analogy with the diffusive `n`, the bound would
        // be a different quantity entirely.
        assert!(message.contains("dx^2"));
    }

    #[test]
    fn t_out_outside_the_declared_temperature_range_is_rejected() {
        // The fifth check of ADR-059, and the only one of the five without a
        // name. The price is not pedantry: `k_E` and the storage width of the
        // field are derived from that range, so a ghost cell outside it overflows
        // the field on the first exchange instead of failing the load.
        let message = refusal(&swap(WORKED_EXAMPLE, "t_out = 298.15", "t_out = 400.0"));
        assert_names(&message, &["t_out", "400", "273.15", "323.15"]);
    }

    #[test]
    fn a_periodic_face_without_its_partner_is_rejected() {
        // Periodicity is a property of the *axis* (section 4). A half-periodic
        // axis gives a grid whose neighbour past the face exists on one side and
        // not on the other; `flux_is_antisymmetric` runs on a pair of values and
        // does not see it.
        let text = swap(
            WORKED_EXAMPLE,
            "[boundary]\nz_min",
            "[boundary]\nx_min = \"closed\"\nz_min",
        );
        assert_names(&refusal(&text), &["x", "Periodic", "Closed"]);
    }

    // --- processes -----------------------------------------------------------

    #[test]
    fn an_unknown_process_id_is_rejected() {
        // The roster is closed (ADR-065), so a misspelled id names no process at
        // all. Refused while the roster is materialised — that is, before the
        // hash — because a stray record would otherwise reach the canonical form
        // and give a scenario an identity built partly out of a typo.
        let text = swap(WORKED_EXAMPLE, "id = \"pressure\"", "id = \"presure\"");
        assert_names(&refusal(&text), &["presure"]);
    }

    #[test]
    fn a_duplicate_process_id_is_rejected() {
        // Checkable without a registry of names, unlike its neighbour. "The last
        // one wins" would make the canonical form depend on the order the records
        // were typed in, that is `config_hash` on how the file was set (section
        // 11). `deny_unknown_fields` does not see this: serde sees the fields of
        // a struct, and `id` is a value.
        let text = swap(
            WORKED_EXAMPLE,
            "id = \"pressure\"\nenabled = false",
            "id = \"pressure\"\nenabled = false\n\n[[process]]\nid = \"pressure\"\nenabled = true",
        );
        assert_names(&refusal(&text), &["pressure", "process", "twice"]);
    }

    // --- referential integrity ----------------------------------------------

    #[test]
    fn a_reaction_naming_an_unknown_substance_is_rejected() {
        let text = swap(
            WORKED_EXAMPLE,
            "inputs = { H2S = 1, O2 = 2 }",
            "inputs = { H2Sx = 1, O2 = 2 }",
        );
        assert_names(&refusal(&text), &["H2Sx", "inputs", "h2s_oxidation"]);
    }

    #[test]
    fn km_missing_for_a_reaction_input_is_rejected() {
        // `S/(K+S)` without `K` is either zero or one depending on what the
        // kernel author substitutes, and both look like working kinetics.
        let text = swap(
            WORKED_EXAMPLE,
            "km = { H2S = 0.01, O2 = 0.01 }",
            "km = { H2S = 0.01 }",
        );
        assert_names(&refusal(&text), &["O2", "km", "h2s_oxidation"]);
    }

    #[test]
    fn km_naming_a_substance_outside_the_inputs_is_rejected() {
        // One of the four "name -> value" tables where `deny_unknown_fields` does
        // not work and where a typo is likeliest (section 11 item 1).
        let outside = swap(
            WORKED_EXAMPLE,
            "km = { H2S = 0.01, O2 = 0.01 }",
            "km = { H2S = 0.01, O2 = 0.01, SO4 = 0.01 }",
        );
        assert_names(&refusal(&outside), &["SO4", "km", "inputs"]);

        let unknown = swap(
            WORKED_EXAMPLE,
            "km = { H2S = 0.01, O2 = 0.01 }",
            "km = { H2S = 0.01, O2 = 0.01, NOPE = 0.01 }",
        );
        assert_names(&refusal(&unknown), &["NOPE", "km"]);
    }

    #[test]
    fn initial_layer_naming_an_unknown_substance_is_rejected() {
        // The fifth "name -> value" table of the schema, and the newest
        // (ADR-077). `deny_unknown_fields` sees the fields of a struct and the
        // keys here are data, so the misspelling is referential integrity —
        // stage 1 — and not serde's business.
        let text = format!("{WORKED_EXAMPLE}\n[initial.layer]\nO_2 = \"water\"\n");
        assert_names(&refusal(&text), &["O_2", "initial.layer", "O2", "H2S"]);

        // The stage matters as much as the refusal. A typo in a substance name
        // caught by the scales is reported as an error of the scales, and the
        // author is sent to look for it in another section entirely — which is
        // what `the_validator_refuses_before_it_derives` is about.
        let with_a_broken_scale = swap(&text, "typical_conc = 0.1", "typical_conc = 1.0e31");
        let with_a_broken_scale =
            swap(&with_a_broken_scale, "max_conc = 10.0", "max_conc = 1.0e31");
        let message = refusal(&with_a_broken_scale);
        assert_names(&message, &["O_2"]);
        assert!(
            !message.contains("ceiling"),
            "referential integrity comes before the derivation; it said:\n{message}"
        );

        // And the materialisation did not quietly drop the unknown key on the
        // way: if it rebuilt the table from the registry instead of filling in
        // what the file left out, this refusal would be unreachable, the
        // canonical form would look correct, and the world would simply have no
        // oxycline in it.
        let spelled_right = format!("{WORKED_EXAMPLE}\n[initial.layer]\nO2 = \"water\"\n");
        validated(&spelled_right);
    }

    #[test]
    fn a_catalyst_outside_the_declared_forms_is_rejected() {
        assert_names(
            &refusal(&swap(
                WORKED_EXAMPLE,
                "catalyst = \"\"",
                "catalyst = \"M_PHOTO\"",
            )),
            &["M_PHOTO", "guild:", "expr:", "vmax"],
        );
        assert_names(
            &refusal(&swap(
                WORKED_EXAMPLE,
                "catalyst = \"\"",
                "catalyst = \"guild:\"",
            )),
            &["guild:"],
        );

        // The *form* is checked and the id is not resolved: there is no guild
        // registry in S0, it arrives in S1. That is a stage boundary rather than
        // a forgotten check, and this is where it is said.
        validated(&swap(
            WORKED_EXAMPLE,
            "catalyst = \"\"",
            "catalyst = \"guild:M_PHOTO\"",
        ));
        validated(&swap(
            WORKED_EXAMPLE,
            "catalyst = \"\"",
            "catalyst = \"expr:k1\"",
        ));
    }

    /// A window on a field is a load error until the gate exists (ADR-073).
    ///
    /// **Both bounds, or the test checks serde instead of the rule.**
    /// `schema::Requirement` declares `max: f64` with no `serde(default)`, so
    /// the one-sided `{ field = "LIGHT", min = 0.02 }` of SPEC section 5 dies in
    /// `parse` with "missing field `max`" and never reaches a validator at all —
    /// which is the second half of the divergence ADR-073 records against the
    /// frozen spec, and this record does not repair it.
    ///
    /// The field name is deliberately a plausible one. ADR-073 assigns no roster
    /// of field identifiers, no unit and no case: the refusal is the same for
    /// every spelling, which is precisely why no roster was worth freezing ahead
    /// of the kernel that would read it.
    #[test]
    fn a_requires_window_is_rejected_until_the_gate_exists() {
        let window = "requires = [{ field = \"enthalpy\", min = 273.15, max = 323.15 }]";
        let text = swap(
            WORKED_EXAMPLE,
            "catalyst = \"\"",
            &format!("catalyst = \"\"\n{window}"),
        );

        // It parses. The refusal has to come from the validator and be about the
        // window — not from serde and about a missing key.
        parse(&text).expect("a two-sided window parses; the refusal is the validator's");

        assert_names(
            &refusal(&text),
            &["h2s_oxidation", "requires", "1", "requires = []"],
        );

        // And it comes before `derive`: a config with both a window and a scale
        // that overflows is refused for the window. Section 10 is the register of
        // *load* refusals, and `the_validator_refuses_before_it_derives` pins
        // that order for the whole corpus.
        let text = swap(&text, "typical_conc = 0.1", "typical_conc = 1.0e31");
        let text = swap(&text, "max_conc = 10.0", "max_conc = 1.0e31");
        let message = refusal(&text);
        assert_names(&message, &["requires"]);
        assert!(
            !message.contains("ceiling"),
            "the window is refused before the scales are derived; it said:\n{message}"
        );
    }

    #[test]
    fn reaction_without_t_vmax_is_rejected() {
        // The key is mandatory with no default (ADR-048), so the refusal comes
        // from the schema rather than from a rule of section 10 — and it has to
        // name `t_vmax` rather than the reaction, because the author's next move
        // is to look for a temperature and there are three in the corpus.
        assert_names(
            &refusal(&swap(WORKED_EXAMPLE, "t_vmax = 298.15\n", "")),
            &["t_vmax"],
        );
    }

    #[test]
    #[ignore = "the rule behind this name outlived its cause: after ADR-039 the \
                resolution does not depend on dx at all, so the description in \
                ACCEPTANCE.md — an eco-regime scenario at a micro-regime dx — \
                describes a mechanism ADR-039 abolished. The meaningful rule left \
                is the dx window of ADR-002, 1e-6 to 1e-3 m, and section 13 item \
                17 says it has to be either written down as a decision or the \
                name dropped — but not in silence"]
    fn scale_underflow_is_rejected() {
        let text = swap(WORKED_EXAMPLE, "dx = 1.0e-4", "dx = 1.0e-8");
        assert_names(&refusal(&text), &["dx", "1e-6", "1e-3"]);
    }

    // --- domains of definition ----------------------------------------------

    /// One case per line of the "domain of definition" list of section 10:
    /// the key the refusal has to name, the passage to rewrite, the violating
    /// rewrite, and the `nan` rewrite (empty where the key is an integer).
    const DOMAIN_CASES: &[(&str, &str, &str, &str)] = &[
        ("dt", "dt = 1.0", "dt = 0.0", "dt = nan"),
        ("grid.dx", "dx = 1.0e-4", "dx = 0.0", "dx = nan"),
        ("nx", "nx = 64", "nx = 0", ""),
        ("beta", "beta = 0.015625", "beta = 0.0", "beta = nan"),
        ("conserved.C", "C = 12.01070", "C = -1.0", "C = nan"),
        (
            "molar_mass",
            "molar_mass = 34.08088",
            "molar_mass = 0.0",
            "molar_mass = nan",
        ),
        (
            "typical_conc",
            "typical_conc = 0.1",
            "typical_conc = 0.0",
            "typical_conc = nan",
        ),
        (
            "max_conc",
            "max_conc = 10.0",
            "max_conc = 0.0",
            "max_conc = nan",
        ),
        (
            "typical_conc",
            "typical_conc = 0.1",
            "typical_conc = 20.0",
            "",
        ),
        (
            "partial_molar_volume",
            "partial_molar_volume = 3.5e-5",
            "partial_molar_volume = inf",
            "partial_molar_volume = nan",
        ),
        (
            "settling_radius",
            "settling_radius = 0.0\ndiffusivity = 1.6e-9",
            "settling_radius = -1.0\ndiffusivity = 1.6e-9",
            "settling_radius = nan\ndiffusivity = 1.6e-9",
        ),
        // The three keys of `[physics]` (ADR-085). Zero is legal for the first
        // two and is deliberately not a violation here: `g = 0` is a lawful
        // scenario that settles nothing, and a ban would be a statement about
        // physics this record does not make.
        ("physics.g", "g = 9.80665", "g = -9.80665", "g = nan"),
        (
            "physics.rho_medium",
            "rho_medium = 1000.0",
            "rho_medium = -1000.0",
            "rho_medium = nan",
        ),
        // Violated with the settling process **off** — which is what
        // `WORKED_EXAMPLE` leaves it — so that the rule is checked where it
        // lives. Moved into the branch "when settling is on", a negative
        // viscosity would pass validation whole on every scenario in the
        // repository, and this table could not tell: the refusal
        // `settling_velocity` raises names `mu` as well.
        ("physics.mu", "mu = 1.0e-3", "mu = 0.0", "mu = nan"),
        (
            "rate.km.H2S",
            "km = { H2S = 0.01, O2 = 0.01 }",
            "km = { H2S = 0.0, O2 = 0.01 }",
            "km = { H2S = nan, O2 = 0.01 }",
        ),
        ("rate.q10", "q10 = 2.0", "q10 = 0.0", "q10 = nan"),
        (
            "thermal_diffusivity",
            "thermal_diffusivity = 1.4e-7",
            "thermal_diffusivity = 0.0",
            "thermal_diffusivity = nan",
        ),
        ("t_min", "t_min = 273.15", "t_min = 400.0", "t_min = nan"),
        (
            "every_n_ticks",
            "id = \"diffusion\"\nenabled = true",
            "id = \"diffusion\"\nenabled = true\nevery_n_ticks = 0",
            "",
        ),
        ("k_ex", "k_ex = 1.0e-5", "k_ex = 0.0", "k_ex = nan"),
        ("conc_out.O2", "O2 = 0.25,", "O2 = -0.25,", "O2 = nan,"),
        ("t_out", "t_out = 298.15", "t_out = 400.0", "t_out = nan"),
        (
            "u_conv_max",
            "u_conv_max = 1.0e-5",
            "u_conv_max = 0.0",
            "u_conv_max = nan",
        ),
        ("l_c", "l_c = 3.2e-3", "l_c = 0.0", "l_c = nan"),
        (
            "stir_fraction",
            "l_c = 3.2e-3",
            "l_c = 3.2e-3\nstir_fraction = 2.0",
            "l_c = 3.2e-3\nstir_fraction = nan",
        ),
        (
            "stir_period",
            "l_c = 3.2e-3",
            "l_c = 3.2e-3\nstir_fraction = 0.5",
            "l_c = 3.2e-3\nstir_fraction = 0.5\nstir_period = nan",
        ),
        ("min", "min = 1.5", "min = 3.0", "min = nan"),
        (
            "min",
            "min = 1.5\nmax = 3.0\nscale = \"linear\"",
            "min = -1.0\nmax = 3.0\nscale = \"log\"",
            "",
        ),
    ];

    #[test]
    fn every_domain_rule_refuses_its_own_violation() {
        // One name and not twenty-five: the list of section 10 *is* the
        // criterion, and a table goes red when a line drops out of it, which
        // twenty-five separate tests cannot do.
        for (key, from, to, _) in DOMAIN_CASES {
            let text = swap(WORKED_EXAMPLE, from, to);
            let message = refusal(&text);
            assert!(
                message.contains(key),
                "the refusal for `{key}` has to name the key and the bound it \
                 broke; it said:\n{message}"
            );
        }
    }

    proptest! {
        // A property over the position of the key rather than twenty-five
        // literals: what is asserted is that *no* domain rule lets a `nan`
        // through, and the twenty-five cases are the sample space.
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn a_not_a_number_never_passes_a_domain_rule(case in 0usize..DOMAIN_CASES.len()) {
            // `nan` is false in every comparison, so a rule written as "refuse if
            // out of range" passes it: `max/typical > 16384.0` passes it,
            // `min < max` passes it, and the number reaches the logarithm inside
            // `e_r`. The correct form is "refuse unless finite *and* in range",
            // and the difference is visible only here.
            let (key, from, _, nan) = DOMAIN_CASES[case];
            if nan.is_empty() {
                return Ok(());
            }
            let text = swap(WORKED_EXAMPLE, from, nan);
            let config = parse(&text).expect("the fixture must parse");
            let error = validate(&config).expect_err("a nan must never validate");
            let message = format!("{error:#}");
            prop_assert!(
                message.contains(key),
                "the refusal for a nan in `{key}` has to name the key; it said:\n{message}"
            );
        }
    }

    #[test]
    fn a_grid_not_divisible_by_its_coarsest_lod_is_rejected() {
        // Against the *coarsest* declared lod. Checked against the finest the
        // rule is green on any scenario with one fine field: the covering cell is
        // incomplete, `coarse_idx = fine_idx >> lod` loses voxels on the way up,
        // and the half of the ledger the coarsened field answers for diverges.
        let text = swap(WORKED_EXAMPLE, "nx = 64", "nx = 50");
        let message = refusal(&text);
        assert_names(&message, &["nx", "50", "2^2", "4", "enthalpy"]);
    }

    // --- calibration ---------------------------------------------------------

    #[test]
    fn calibration_path_that_leads_nowhere_is_rejected() {
        let path = |p: &str| {
            swap(
                WORKED_EXAMPLE,
                "path = \"reaction.h2s_oxidation.rate.q10\"",
                &format!("path = \"{p}\""),
            )
        };
        // A reaction that does not exist.
        assert_names(
            &refusal(&path("reaction.nope.rate.q10")),
            &["leads nowhere"],
        );
        // A key that does not exist inside a reaction that does.
        assert_names(
            &refusal(&path("reaction.h2s_oxidation.rate.nope")),
            &["leads nowhere"],
        );
        // A path that stops at a *table*: it "resolves" and is not a coordinate.
        // The search driver would get one that moves nothing, and the whole
        // search would run one dimension short of what its author thinks.
        assert_names(
            &refusal(&path("reaction.h2s_oxidation.rate")),
            &["not to a number", "table"],
        );
        // A path into `[[calibration]]`, the one section outside `config_hash`: a
        // number that is not in the hash cannot be in the model, or two runs of
        // different physics get one identity.
        assert_names(
            &refusal(&path("calibration.min")),
            &["config_hash", "identity"],
        );

        // Arrays are addressed by `id` and not by index (ADR-038).
        let config = parse(WORKED_EXAMPLE).expect("parses");
        let root = toml::Value::try_from(&config).expect("projects");
        assert!(resolve_path(&root, "reaction.h2s_oxidation.rate.vmax").is_some());
        assert!(resolve_path(&root, "reaction.0.rate.vmax").is_none());
        assert!(resolve_path(&root, "substance.H_ION.max_conc").is_some());
    }

    #[test]
    fn a_calibration_window_that_is_not_an_interval_is_rejected() {
        let text = swap(
            WORKED_EXAMPLE,
            "min = 1.5\nmax = 3.0",
            "min = 3.0\nmax = 3.0",
        );
        assert_names(&refusal(&text), &["min < max"]);

        let log = swap(
            WORKED_EXAMPLE,
            "min = 1.5\nmax = 3.0\nscale = \"linear\"",
            "min = 0.0\nmax = 3.0\nscale = \"log\"",
        );
        assert_names(&refusal(&log), &["log", "min > 0"]);
    }

    // --- the two contracts over all of the above -----------------------------

    #[test]
    fn every_n_ticks_on_the_velocity_field_is_rejected() {
        // ADR-074 assigned this name and the refusal, and never applied it:
        // `check_every_n_ticks` compared `process.id` against the diffusion
        // process alone. ADR-086 adds the second argument and the same address —
        // the validator, not `Tick::new`, so that the refusal stays a refusal of
        // *load* and stays in the table above.
        //
        // At `every_n_ticks = 4` step `c` would advect, on three ticks out of
        // four, with a `face_courant` written in some other tick, and a restart
        // would advect with zeros — the buffer is `Scratch`'s and no snapshot
        // carries it (25.17 MB per file at 128^3 otherwise).
        let text = swap(
            WORKED_EXAMPLE,
            "id = \"velocity_field\"\nenabled = true",
            "id = \"velocity_field\"\nenabled = true\nevery_n_ticks = 4",
        );
        let message = refusal(&text);
        assert!(
            message.contains("velocity_field") && message.contains("face_courant"),
            "the refusal has to name the process and the buffer; it said:\n{message}"
        );
    }

    #[test]
    fn every_n_ticks_on_the_light_process_is_rejected() {
        // The light field is written by step `a` and read by step `i'` of the
        // **same** tick, so a schedule that separates them turns a `Scratch`
        // buffer into inter-tick state of class `Q` the snapshot does not carry —
        // 8.39 MB per file at 128^3 (ADR-086).
        //
        // The scenario declares the process disabled: the refusal is about the
        // schedule and not about the process running, and a light process that is
        // *enabled* is refused earlier and for another reason entirely (ADR-076).
        let text = swap(
            WORKED_EXAMPLE,
            "id = \"pressure\"\nenabled = false",
            "id = \"pressure\"\nenabled = false\n\n[[process]]\nid = \"light\"\nenabled = false\nevery_n_ticks = 3",
        );
        let message = refusal(&text);
        assert!(
            message.contains("light") && message.contains("every_n_ticks = 3"),
            "the refusal has to name the process and the schedule; it said:\n{message}"
        );
    }

    #[test]
    fn every_refusal_names_the_numbers_it_compared() {
        // The mechanical form of the load-bearing requirement of this wave: a
        // refusal names the culprit — or the pair, where ADR-039 asks for a pair
        // — both sides of the broken inequality as numbers, and what to change.
        // Half of these rules catch an error otherwise visible only as strange
        // dynamics a hundred thousand ticks in, and "the config is invalid" sends
        // the author to look for it alone.
        let cases: Vec<(&str, String, Vec<&str>)> = vec![
            (
                "element balance",
                swap(
                    WORKED_EXAMPLE,
                    "enthalpy_formation = -846000.0\ncomposition = { S = 1 }",
                    "enthalpy_formation = -846000.0\ncomposition = {}",
                ),
                vec!["h2s_oxidation", "`S`", "1", "0"],
            ),
            (
                "mass balance",
                swap(
                    WORKED_EXAMPLE,
                    "outputs = { SO4 = 1, H_ION = 2 }",
                    "outputs = { SO4 = 1, H_ION = 1 }",
                ),
                vec!["h2s_oxidation", "98.07848", "97.07054", "tolerance"],
            ),
            (
                "enthalpy agreement",
                swap(
                    WORKED_EXAMPLE,
                    "enthalpy = -846000.0",
                    "enthalpy = -845000.0",
                ),
                vec!["h2s_oxidation", "-845000", "-846000", "1000"],
            ),
            (
                "composition against mass",
                swap(WORKED_EXAMPLE, "molar_mass = 96.06260", "molar_mass = 1.0"),
                vec!["SO4", "1", "32.065", "molar_mass >="],
            ),
            (
                "dynamic range",
                swap(WORKED_EXAMPLE, "max_conc = 10.0", "max_conc = 100000.0"),
                vec!["H2S", "2^14"],
            ),
            (
                "the pair of ADR-039",
                water_and_proton(500_000_000),
                vec!["WATER", "photosynthesis", "max_conc"],
            ),
            (
                "nu against i32",
                TWO_REACTIONS.to_string(),
                vec!["WATER", "brining", "i32"],
            ),
            (
                "Courant",
                swap(WORKED_EXAMPLE, "u_conv_max = 1.0e-5", "u_conv_max = 2.0e-4"),
                vec!["velocity_field", "0.0002", "0.0001"],
            ),
            (
                "outflow",
                swap(WORKED_EXAMPLE, "u_conv_max = 1.0e-5", "u_conv_max = 5.0e-5"),
                vec!["velocity_field", "6 faces", "0.00005", "3 over the limit"],
            ),
            (
                "structure length",
                swap(
                    swap(
                        WORKED_EXAMPLE,
                        "thermal_diffusivity = 1.4e-7",
                        "thermal_diffusivity = 1.0e-7",
                    )
                    .as_str(),
                    "l_c = 3.2e-3",
                    "l_c = 3.0e-4",
                ),
                vec!["l_c", "0.0003", "0.0004", "enthalpy"],
            ),
            (
                "reservoir concentration",
                swap(WORKED_EXAMPLE, "SO4 = 28.0,", "SO4 = 500.0,"),
                vec!["SO4", "500", "100"],
            ),
            (
                "exchange velocity",
                swap(WORKED_EXAMPLE, "k_ex = 1.0e-5", "k_ex = 2.0e-4"),
                vec!["k_ex", "0.0002", "0.0001"],
            ),
            (
                "t_out",
                swap(WORKED_EXAMPLE, "t_out = 298.15", "t_out = 400.0"),
                vec!["t_out", "400", "273.15", "323.15"],
            ),
            (
                "grid divisibility",
                swap(WORKED_EXAMPLE, "nx = 64", "nx = 50"),
                vec!["nx", "50", "4"],
            ),
            (
                "substeps over N_MAX",
                swap(WORKED_EXAMPLE, "lod = 2", "lod = 0"),
                vec!["enthalpy", "84", "64"],
            ),
            (
                "T_ref outside the range",
                swap(WORKED_EXAMPLE, "T_ref = 298.15", "T_ref = 400.0"),
                vec!["T_ref", "400", "273.15", "323.15"],
            ),
            // The two refusals ADR-086 adds beside the diffusive one. They belong
            // in this table by ADR-074's own consequence — "a refusal by
            // every_n_ticks does have numbers and does go in the table" — and the
            // numbers they compare are the schedule and the tick it separates the
            // writer from the reader by.
            (
                "every_n_ticks on the velocity field",
                swap(
                    WORKED_EXAMPLE,
                    "id = \"velocity_field\"\nenabled = true",
                    "id = \"velocity_field\"\nenabled = true\nevery_n_ticks = 4",
                ),
                vec!["velocity_field", "4", "3 ticks out of 4", "face_courant"],
            ),
            (
                "every_n_ticks on the light",
                swap(
                    WORKED_EXAMPLE,
                    "id = \"pressure\"\nenabled = false",
                    "id = \"pressure\"\nenabled = false\n\n[[process]]\nid = \"light\"\nenabled = false\nevery_n_ticks = 3",
                ),
                vec!["light", "3", "step `a`", "step `i'`"],
            ),
        ];
        for (label, text, wanted) in cases {
            let message = refusal(&text);
            for want in wanted {
                assert!(
                    message.contains(want),
                    "the refusal for `{label}` has to name `{want}`; it said:\n{message}"
                );
            }
        }
    }

    #[test]
    fn the_validator_refuses_before_it_derives() {
        // Both faults at once: a typo in a substance name inside `inputs` and a
        // scale that overflows. The message has to be about the typo. The order
        // of the stages is a contract and not taste — section 10 describes the
        // outcome "a reaction refused by the wrong message" by name, and a
        // `derive` called first produces exactly it.
        let text = swap(
            WORKED_EXAMPLE,
            "inputs = { H2S = 1, O2 = 2 }",
            "inputs = { H2Sx = 1, O2 = 2 }",
        );
        let text = swap(&text, "typical_conc = 0.1", "typical_conc = 1.0e31");
        let text = swap(&text, "max_conc = 10.0", "max_conc = 1.0e31");
        let message = refusal(&text);
        assert_names(&message, &["H2Sx"]);
        assert!(
            !message.contains("ceiling"),
            "referential integrity comes first; it said:\n{message}"
        );
    }

    #[test]
    fn a_duplicate_id_in_any_section_is_refused_by_the_validator_too() {
        // The twin of `derive::tests::a_duplicate_id_in_any_section_is_refused`.
        // Uniqueness is a rule of referential integrity (section 10) and the
        // validator owns it; the checks in `derive` stay as guards on its own
        // domain, because `derive` is public and can be called on a config nobody
        // validated.
        let text = swap(WORKED_EXAMPLE, "id = \"H2S\"", "id = \"O2\"");
        assert_names(&refusal(&text), &["O2", "substance", "twice"]);
    }
}
