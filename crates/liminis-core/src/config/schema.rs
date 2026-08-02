//! The scenario schema: one serde type per section of `CONFIG_SCHEMA.md`.
//!
//! Two rules govern the shape of everything below, and both are load-bearing
//! rather than stylistic.
//!
//! **Scalars come before tables in every type.** Field order here *is* the order
//! of the canonical form (`config/hash.rs`), and TOML has no way to write a bare
//! key after a table header — a struct that prints `conserved` between `dt` and
//! `beta` would emit a file it cannot read back. Worth knowing before relying on
//! that: the current `toml` serializer hoists scalars above tables by itself, so
//! a misordered struct still produces valid TOML and nothing goes red. The rule
//! is the intent of `CONFIG_SCHEMA.md` section 11 item 3; it is not guarded by
//! anything, and it cannot be delegated to a failure that no longer happens.
//!
//! **`deny_unknown_fields` on every type, not just on [`Config`].** Without it
//! on the nested types a typo inside `[[substance]]` or `[reaction.rate]` is
//! silently ignored, and the resulting config hashes exactly like a correct one
//! — the failure `an_unknown_field_is_an_error` exists to prevent, one level
//! down. The attribute is written per type and so can be lost per type;
//! `an_unknown_field_inside_a_nested_table_is_an_error` carries one case for
//! each of the ten below, which is what stops a deletion from being silent.
//! Its reach is narrower than it sounds either way: serde sees struct fields,
//! and the keys of `composition`, `inputs`, `outputs`, `rate.km` and `conc_out`
//! are data. Typos there are caught by referential integrity in the validator,
//! not here (`CONFIG_SCHEMA.md` section 10).
//!
//! Maps are [`BTreeMap`], and that is not taste. An insertion-ordered map would
//! carry the key order of the file into the canonical bytes and turn
//! `whitespace_and_key_order_do_not_change_the_hash` red — which it does only
//! because that test reorders the entries of all six of them, `conserved`,
//! `composition`, `inputs`, `outputs`, `km` and `conc_out`: the choice is made
//! per field, so a fixture that exercised one map would say nothing about the
//! other five. A `HashMap` would make
//! the hash non-deterministic *between runs*, which that test cannot see —
//! both hashes are computed in one process under one `RandomState` — and which
//! surfaces a whole run later, in
//! `same_seed_and_config_give_byte_identical_state`.
//!
//! Names collide with `world/` on purpose, and the collision is worth saying out
//! loud: [`Field`] here is a scenario record, `world::Field` is a buffer;
//! [`Boundary`] here is the six-face section of a scenario, `world::Boundary` is
//! the condition on one face; [`Face`] here is that condition, `world::Face` is
//! one of the six faces of a voxel. The names are taken from the TOML keys
//! (`CONFIG_SCHEMA.md` section 7), and spelling them `FieldCfg` would break the
//! uniformity with `Grid`, `Substance` and `Reaction`, which collide with
//! nothing. A swapped import is a compile error nearly always — but not in a
//! generic context, so it is named here rather than left to be discovered.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A scenario configuration, after defaults have been applied.
///
/// Twelve root keys. `seed` is not among them and cannot be: the seed is a run
/// parameter (`--seed`), reaches the generator as the fourth counter of the
/// mixer, and stays out of `config_hash` so that the identity triple does not
/// degenerate (ADR-058, `CONFIG_SCHEMA.md` section 2). A `seed` line in a
/// scenario file is a load error, and `seed_in_the_scenario_file_is_rejected`
/// makes that a decision rather than a by-product of `deny_unknown_fields`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Scenario name. Hashed, though it changes no dynamics: it is read by the
    /// simulator, and that is the whole rule (ADR-038).
    pub name: String,
    /// Timestep, seconds. One second in the eco regime: a field whose
    /// diffusion outruns the tick subdivides its own step rather than
    /// shortening the tick for everyone (ADR-030).
    #[serde(default = "default_dt")]
    pub dt: f64,
    /// Fraction of a typical pool one quantum of extent may consume (ADR-039).
    ///
    /// At `2^-6` a reaction has at least 64 distinguishable steps of extent
    /// across the pool of its scarcest reagent. Lowering it is not free — `e_r`
    /// grows with it, and the system of inequalities in ADR-039 becomes
    /// infeasible.
    #[serde(default = "default_beta")]
    pub beta: f64,
    /// Storage reference state for enthalpy, kelvin (ADR-044).
    ///
    /// Required, no default: it exists so that the bit width is not spent on a
    /// constant offset, and any number picked for the author would be a
    /// scenario decision made by the schema. Must lie inside the temperature
    /// range the enthalpy field declares (ADR-062) — a rule for the validator.
    #[serde(rename = "T_ref")]
    pub t_ref: f64,
    /// Conserved quantities: name to molar mass in **g/mol** (ADR-025,
    /// ADR-064).
    ///
    /// The names are deliberately not enumerated in Rust. That is what lets
    /// `charge = 0.0` here and `charge = -2` in a `composition` land through the
    /// same mechanism, with no change to the format at all.
    #[serde(default = "default_conserved")]
    pub conserved: BTreeMap<String, f64>,
    pub grid: Grid,
    #[serde(default)]
    pub boundary: Boundary,
    #[serde(default)]
    pub substance: Vec<Substance>,
    #[serde(default)]
    pub reaction: Vec<Reaction>,
    #[serde(default)]
    pub field: Vec<Field>,
    #[serde(default)]
    pub process: Vec<Process>,
    /// Calibration coordinates. The one section outside `config_hash`
    /// (ADR-038); see `NOT_HASHED` in `config/hash.rs`.
    ///
    /// `Vec`, not a map, because the order of the entries *is* the order of the
    /// coordinates of the search vector, and it lives in the file so that
    /// refactoring the schema cannot silently repermute a finished search.
    ///
    /// `#[serde(default)]` is not optional here in either sense: the canonical
    /// form drops the section, so without a default it could not be read back
    /// and `the_canonical_form_reloads_to_the_same_hash` would fail. Making it
    /// `Option<Vec<_>>` instead would be worse and quiet — "absent" and "empty"
    /// would become different configs with the same hash.
    #[serde(default)]
    pub calibration: Vec<Calibration>,
}

/// Grid extent and spacing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grid {
    #[serde(default = "default_extent")]
    pub nx: u32,
    #[serde(default = "default_extent")]
    pub ny: u32,
    #[serde(default = "default_extent")]
    pub nz: u32,
    /// Voxel edge, metres. Selects the regime: 1e-6 is micro, 1e-4 is eco
    /// (ADR-013).
    #[serde(default = "default_dx")]
    pub dx: f64,
}

/// Boundary conditions: six faces, and the reservoir an `exchange` face trades
/// with.
///
/// Six faces rather than three axes is a schema choice, and a forced one: SPEC
/// section 1.6 calls the condition independent per axis and then gives a default
/// in which Z has two different ends. Periodicity stays a property of the axis,
/// so the validator requires both faces of an axis to be `periodic` together.
///
/// The reservoir goes last because it is a table and the six faces are scalars
/// (`CONFIG_SCHEMA.md` section 4, section 11 item 3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Boundary {
    #[serde(default = "default_periodic")]
    pub x_min: Face,
    #[serde(default = "default_periodic")]
    pub x_max: Face,
    #[serde(default = "default_periodic")]
    pub y_min: Face,
    #[serde(default = "default_periodic")]
    pub y_max: Face,
    /// Solid floor, geothermal inlet, the substrate a biofilm grows on.
    #[serde(default = "default_closed")]
    pub z_min: Face,
    #[serde(default = "default_exchange")]
    pub z_max: Face,
    /// Required exactly when some face is `exchange` — which is the default on
    /// `z_max`, so nearly always. Both halves of that are the validator's
    /// (`exchange_face_without_a_reservoir_is_rejected`), which is why the type
    /// is an `Option` and not a plain field.
    pub reservoir: Option<Reservoir>,
}

impl Default for Boundary {
    fn default() -> Self {
        Self {
            x_min: default_periodic(),
            x_max: default_periodic(),
            y_min: default_periodic(),
            y_max: default_periodic(),
            z_min: default_closed(),
            z_max: default_exchange(),
            reservoir: None,
        }
    }
}

/// The condition on one face of the domain.
///
/// Not `world::Boundary`, which is the same three cases on the runtime side.
/// This one is the spelling a scenario writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub enum Face {
    /// The domain wraps along this axis: a 3-torus.
    Periodic,
    /// Nothing crosses this face.
    Closed,
    /// The face trades with the outside through the channel
    /// `BOUNDARY_EXCHANGE` (SPEC section 7).
    Exchange,
}

/// The outside an `exchange` face trades with (ADR-059).
///
/// Three quantities, and the third is the one that surprises: the reservoir
/// declares a temperature, not only a composition. Enthalpy is a diffusive field
/// of its own and crosses an `exchange` face under its own steam, so its ghost
/// cell needs a value too — without `t_out` the top of the world would be
/// thermally sealed by a default nobody wrote.
///
/// The reservoir does not deplete. A finite outside would be state, and
/// `Δ(fields + cells)` would grow a third term that is neither a field nor a
/// cell (ADR-003, ADR-059). If a finite outside is ever needed, it is a voxel,
/// not a boundary condition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reservoir {
    /// Outside temperature, kelvin. Must lie inside the enthalpy field's
    /// declared range, or the first exchange overflows the field instead of the
    /// load failing.
    pub t_out: f64,
    /// Exchange velocity, m/s. One number for every substance is a declared
    /// simplification (ADR-059); physically it scales as `D^(1/2)…D^(2/3)`.
    pub k_ex: f64,
    /// Outside concentration, mol/m^3, per substance of the registry.
    ///
    /// Every substance has to be written out, and that is not pedantry: a
    /// default of zero *is* an infinite sink, and a statement about physics that
    /// strong has to be printed rather than inherited. A table, so it goes after
    /// the scalars.
    pub conc_out: BTreeMap<String, f64>,
}

/// One substance of the registry.
///
/// At most `S_MAX = 32` of these minus the index enthalpy occupies, so at most
/// thirty-one (ADR-041) — the honest price of forbidding allocation in a kernel,
/// where the demand vector is `[S_MAX]` and has to have a fixed size.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Substance {
    pub id: String,
    /// Molar mass, **g/mol** — as in the whole of SPEC section 2.3.
    ///
    /// Declared, not derived, and this is the one place where ADR-025 was
    /// overturned in its consequence (ADR-033): with `conserved = C, N, P, S,
    /// Fe` the composition of water, oxygen and the proton is empty, and a
    /// derived mass would come out zero — and a zero mass for water fails the
    /// mass balance by itself.
    pub molar_mass: f64,
    /// Typical concentration, mol/m^3. Input to `e_r` (ADR-039), and through
    /// `C_cell` to the energy scale as well (ADR-062): a careless number here
    /// moves more than the quantum of extent.
    pub typical_conc: f64,
    /// Maximum concentration, mol/m^3.
    ///
    /// Not only an input to the derived scale but a hard ceiling on the run: a
    /// product that passes it means a wrong declaration, not saturation, and the
    /// run is supposed to stop loudly (ADR-041).
    pub max_conc: f64,
    /// Partial molar volume, m^3/mol (ADR-067).
    ///
    /// Replaces the former `density`; there is no `density` key. The sign
    /// carries meaning — an ion orders the water around it more tightly than
    /// water packs itself, so `V̄` is negative for several — and zero is legal:
    /// `V̄(H+)` is exactly zero on the accepted single-ion scale. Forbidding
    /// zero unconditionally would reject the project's own registry; the
    /// division that needs it non-zero is guarded conditionally, at
    /// `settling_radius > 0`.
    pub partial_molar_volume: f64,
    /// Settling radius, m (ADR-067). Required, with no default, because
    /// "does not settle" as a default would make a forgotten key
    /// indistinguishable from an honest zero — and sediment that never settles
    /// under an entirely green test suite.
    pub settling_radius: f64,
    /// Diffusion coefficient, m^2/s. Required — a schema choice, made because
    /// the substep count is derived from the `D` of each.
    pub diffusivity: f64,
    /// Heat capacity, J/(mol·K), constant (ADR-044).
    pub c_p: f64,
    /// Enthalpy of formation at 298.15 K, J/mol (ADR-044). The thermochemical
    /// reference state, which is a different state from `T_ref`.
    pub enthalpy_formation: f64,
    /// Atoms of each conserved quantity per formula unit — signed, and last
    /// because it is a table.
    ///
    /// Signed because an unsigned type could not hold the `charge = -2` ADR-025
    /// promised, so the mechanism the free-form names exist for did not work
    /// even in the type (ADR-064). The sign is not free: a negative entry
    /// against a quantity with non-zero mass is a load error, or it works as a
    /// discount and `C = -1` buys the right to declare 12.01 g/mol less than the
    /// truth.
    ///
    /// The default of `{}` is a schema choice with a price named in
    /// `CONFIG_SCHEMA.md` section 5 and kept here: a forgotten section is
    /// indistinguishable from an honestly empty one, the element balance passes
    /// as `0 = 0`, and only the mass check catches it. Not a defect to be fixed
    /// — no ADR asks for the refusal that fixing it would introduce.
    #[serde(default)]
    pub composition: BTreeMap<String, i32>,
}

/// One reaction of the registry. At most `R_MAX = 64` (ADR-041).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reaction {
    /// The reaction's name, and the source of its `reaction_id` — taken from
    /// the name and never from the position in the file, or swapping two
    /// records would change the stream of random numbers behind stochastic
    /// rounding and so change the run (ADR-027).
    pub id: String,
    /// Enthalpy of one turnover, **J** (not kJ).
    pub enthalpy: f64,
    /// `guild:<id>`, `expr:<k>`, or empty for an abiotic reaction.
    ///
    /// This value silently fixes the unit of `rate.vmax` (ADR-063), and it
    /// does so from another table. Nothing in the types can express that, and
    /// no signature key is introduced: the branch belongs to a check that runs
    /// after the whole record is parsed, because TOML tables are unordered and
    /// the unit is not knowable at the moment `vmax` is read.
    #[serde(default = "default_empty_string")]
    pub catalyst: String,
    /// A channel name, or empty for "out of this voxel's own enthalpy".
    ///
    /// The set of channel names is closed and lives in code: a channel is the
    /// right-hand side of the equation everything else is checked against, and
    /// a registry a scenario could extend would let any residual be closed by
    /// declaring a seventh channel (ADR-059). The line runs: the channel is
    /// code, its modulation is data.
    #[serde(default = "default_empty_string")]
    pub energy_from: String,
    /// Stoichiometry of the left-hand side: substance id to a positive integer.
    ///
    /// Molar and integral by definition — balancing a chemical equation *is*
    /// finding integer coefficients (ADR-026). There is no sign anywhere in the
    /// TOML; the side supplies it when the pair is resolved into `ν`.
    pub inputs: BTreeMap<String, i32>,
    /// Stoichiometry of the right-hand side. Same rules as `inputs`.
    pub outputs: BTreeMap<String, i32>,
    /// Field windows the reaction needs to proceed.
    #[serde(default)]
    pub requires: Vec<Requirement>,
    pub rate: Rate,
}

/// A window on a field a reaction requires (SPEC section 5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requirement {
    pub field: String,
    pub min: f64,
    pub max: f64,
}

/// Kinetics of one reaction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rate {
    /// Maximum rate, in **turnovers** — and a turnover is not a mole. For a
    /// Redfield-composition reaction one turnover is 106 moles of carbon, so
    /// rewriting the stoichiometry without touching `vmax` moves the rate by two
    /// orders of magnitude in silence.
    ///
    /// The unit is conditional on `catalyst` (ADR-063): turnovers/(s·m^3) when
    /// it is empty, turnovers/(s·mol of catalyst) when it is not. Both forms are
    /// legal and no refusal follows from the fork — which is exactly why it
    /// costs discipline and has nowhere to be checked.
    pub vmax: f64,
    /// The temperature `vmax` was measured at, kelvin. Mandatory, no default
    /// (ADR-048).
    ///
    /// `vmax` and `t_vmax` are one measurement rather than two parameters —
    /// `vmax` is by definition the rate at `t_vmax` — so holding them apart is
    /// what would let them fall out of step. A legal consequence worth knowing
    /// when reading someone else's scenario: two `vmax` in one config may refer
    /// to different temperatures, and that is not an error.
    ///
    /// Not `T_ref`, and the distinction is the whole reason ADR-048 exists.
    /// `T_ref` is an arbitrary per-scenario zero for *storing* enthalpy
    /// (ADR-044); whoever searched the corpus for a reference temperature would
    /// have found it first, and tying kinetics to it would make the speed of
    /// all chemistry depend on where enthalpy is counted from — a shift with no
    /// physical meaning, caught by neither the ledger nor any balance.
    ///
    /// The validator checks that the key is present. It cannot check that the
    /// number is the temperature the rate was actually measured at.
    pub t_vmax: f64,
    pub q10: f64,
    /// Half-saturation constant per input, mol/m^3. Unaffected by the size of a
    /// turnover: it is in substrate concentrations. A table, so it goes last.
    pub km: BTreeMap<String, f64>,
}

/// One field record: resolution, and the three keys the enthalpy field needs.
///
/// Not `world::Field`, which is the double buffer this record describes.
///
/// Resolution is declared on the field and enabling on the process, and SPEC
/// section 1.5 separates them deliberately.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub id: String,
    /// Coarsening relative to `[grid]`: `coarse_idx = fine_idx >> lod`.
    ///
    /// The only legal way to make a fast field cheap — the substep count falls
    /// quadratically with the step. The default of `0` is a schema choice and
    /// its price is quiet: a scenario that forgets the enthalpy record gets
    /// enthalpy on the fine grid, where `n` is 84 rather than the six ADR-030
    /// names. What stops it is `N_MAX` in `process/diffuse.rs`, not this
    /// default.
    #[serde(default)]
    pub lod: u8,
    /// Thermal diffusivity, m^2/s. Required when `id = "enthalpy"` — a rule of
    /// the validator, conditional on a *value*, which is why the type is an
    /// `Option` (ADR-062).
    pub thermal_diffusivity: Option<f64>,
    /// Bottom of the declared temperature range, kelvin.
    ///
    /// Kelvin and not joules, and not for uniformity: joules per cell is a
    /// number that knows about the voxel, and changing `dx` or `lod` would
    /// devalue the declared range in silence (SPEC section 10).
    pub t_min: Option<f64>,
    /// Top of the declared temperature range, kelvin. `T_ref` must lie inside.
    pub t_max: Option<f64>,
}

/// One process record: whether it runs, how often, and its parameters.
///
/// One flat type for all nine processes. Grouping is a schema choice
/// (`CONFIG_SCHEMA.md` section 7), and an internally tagged enum on `id` does
/// not combine with `deny_unknown_fields` in serde. The price is named rather
/// than hidden: this type accepts `l_c` on diffusion and `k_w` on pressure. Such
/// a key parses, reaches the canonical form and the hash, is read by nobody, and
/// no ADR assigns a refusal to it — so two runs differing by a stray key on an
/// unrelated process are declared different runs.
///
/// Absence of a record means the process's *default*, not the process's absence
/// (ADR-065). The trap is exactly one, and it is worth repeating here: deleting
/// a block no longer switches a process off. The only way off is
/// `enabled = false`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Process {
    /// Name from the closed registry under `process/`. `deny_unknown_fields`
    /// cannot help here — serde sees struct fields, and this is a value; an
    /// unknown or duplicated name is the validator's business.
    pub id: String,
    /// `Option`, because the default is declared by the process itself, next to
    /// its own invariant (ADR-065), and putting a default here is the variant
    /// that decision rejected outright: `config/` is not covered by the CI
    /// guard, so flipping it would change the semantics of every run without
    /// moving `WORLD_FORMAT_VERSION`.
    pub enabled: Option<bool>,
    /// `> 1` is forbidden on a process that touches a diffusive field: skipping
    /// ticks multiplies the effective `dt` (ADR-030).
    #[serde(default = "default_every_n_ticks")]
    pub every_n_ticks: u32,
    /// Viscosity of the medium, Pa·s. Needed wherever `w_sed` is derived, that
    /// is with at least one substance at `settling_radius > 0` (ADR-067).
    pub mu: Option<f64>,
    /// Upper bound on the convective speed, m/s (ADR-069). Required when the
    /// velocity-field process is on, with no default.
    pub u_conv_max: Option<f64>,
    /// Structure length, m (ADR-069). An ordinary process key, and deliberately
    /// so: `[[calibration]]` addresses a key by path and does not stand in for
    /// one, or a number outside `config_hash` would be steering the model.
    pub l_c: Option<f64>,
    /// Stirring period, s. Required when `stir_fraction > 0`.
    pub stir_period: Option<f64>,
    /// Stirring amplitude as a fraction of `u_conv_max`. Default `0` (ADR-069,
    /// `CONFIG_SCHEMA.md` section 7) — the one process parameter whose default a
    /// decision names, and therefore the one that is not an `Option`.
    ///
    /// Zero is not "unset" here, it is a declared branch, and two later readers
    /// would have to invent a number for `None`: the conservative speed bound
    /// the validator checks Courant against is `(1 + stir_fraction)·u_conv_max`,
    /// and at exactly zero the noise kernel is not dispatched at all — a branch
    /// on the host, which is what makes the default free (ADR-069).
    #[serde(default)]
    pub stir_fraction: f64,
    /// Light attenuation, 1/m (SPEC section 4.6). Whether a scenario that omits
    /// these is refused, and under what condition, is assigned by no decision
    /// (`CONFIG_SCHEMA.md` section 13 item 23) — they are here so that a
    /// scenario can at least write them.
    pub k_w: Option<f64>,
    pub k_b: Option<f64>,
    pub k_d: Option<f64>,
    pub k_m: Option<f64>,
}

/// One coordinate of the search vector (ADR-038).
///
/// The order of these records in the file *is* the order of the coordinates.
/// The section stays out of `config_hash`: it is read by the search driver and
/// not by the tick loop, so it cannot affect the dynamics — and were it hashed,
/// a sweep would hand every probe its own run identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Calibration {
    /// Dotted path to the key, addressing arrays by `id` rather than by index
    /// (`reaction.h2s_oxidation.rate.vmax`) — a schema choice, made so that the
    /// mistake ADR-027 removed from `reaction_id` is not reintroduced here.
    pub path: String,
    pub min: f64,
    pub max: f64,
    /// Required. ADR-038 names no default, so making one up would be a schema
    /// edit without a journal entry; a required key is the schema choice that
    /// decides nothing on the journal's behalf.
    pub scale: Scale,
}

/// The scale a calibrated parameter is searched on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub enum Scale {
    Linear,
    /// Declared per parameter because reaction rates span orders of magnitude,
    /// and a linear search spends nearly every probe in the top decade.
    Log,
}

fn default_dt() -> f64 {
    1.0
}

fn default_beta() -> f64 {
    0.015625
}

/// The five default conserved quantities, name to molar mass in g/mol.
///
/// TODO(CONFIG_SCHEMA.md section 13 item 23): no document prints this table.
/// ADR-064 prints three of the five — `C = 12.01070`, `N = 14.00670`,
/// `Fe = 55.84500` — and `P = 30.97376` with `S = 32.06500` are *reconstructed*
/// in `CONFIG_SCHEMA.md` section 2 from its own cross-check
/// `P4S3 = 220.09004 = 4·30.97376 + 3·32.06500`. They stand here as a
/// derivation, not as a declared default, and any edit to any digit moves the
/// `config_hash` of every scenario (ADR-064). Seven significant figures is not
/// cosmetic either: at `Fe = 55.85` the sum for `FE2` exceeds the declared
/// `55.84500`, and the project's own registry fails its own check.
fn default_conserved() -> BTreeMap<String, f64> {
    BTreeMap::from([
        ("C".to_string(), 12.010_70),
        ("N".to_string(), 14.006_70),
        ("P".to_string(), 30.973_76),
        ("S".to_string(), 32.065_00),
        ("Fe".to_string(), 55.845_00),
    ])
}

fn default_extent() -> u32 {
    64
}

fn default_dx() -> f64 {
    1.0e-4
}

fn default_periodic() -> Face {
    Face::Periodic
}

fn default_closed() -> Face {
    Face::Closed
}

fn default_exchange() -> Face {
    Face::Exchange
}

fn default_empty_string() -> String {
    String::new()
}

fn default_every_n_ticks() -> u32 {
    1
}
