//! Channel counters, domain sums, and the two residuals (ADR-059).
//!
//! The right-hand side of the invariant of ADR-003, which SPEC section 2.1
//! writes as `delta(sum fields + sum cells) == sum(flows through named
//! channels)` and ADR-028 makes double: matter closes separately, energy closes
//! separately. The left-hand side already exists — it is the fields that are
//! written. Until this module there was no right-hand side in any form, and
//! "closed accounting" was an intention rather than a property.
//!
//! # Three things that are easy to confuse
//!
//! **A counter is not a residual.** [`Ledger::matter`] is the running net flow
//! through one `(channel, substance)` pair over the whole run — it grows for ten
//! million ticks and is `i64` for that reason (`QUANTITIES.md` section 3).
//! [`Ledger::residual_matter`] is the difference between the two sides over
//! **one tick**, and it is required to be an exact zero, not a small number. A
//! residual computed against the running total instead of against the tick's
//! increment is zero on the first tick and lies from the second one on — and
//! never lies at all on a closed domain, which is why it survives review.
//!
//! **A domain sum is not a counter.** Counters accumulate flows through a face;
//! domain sums add up pools, and pools are much larger. The worst case is not
//! the 32-bit substance it looks like: water holds `5.12e11` units per voxel
//! (ADR-040), so at 256 cubed — the target of SPEC section 13 — its domain sum
//! is `8.59e18`, which is 93% of `i64::MAX`. So the reduction runs in `i128`,
//! which costs nothing here: it lives on the orchestration layer, never travels
//! into WGSL (ADR-034), and in a debug build it happens once a tick.
//!
//! **The left side is not one `amount[]`.** Guild fields are measured in moles
//! of `BIOMASS` (`QUANTITIES.md` section 7) and the cell table carries
//! `struct_mass` and `energy` (SPEC section 6.1). A ledger that sums `amount[]`
//! and stops is green through all of S0, because there are no guilds and no
//! cells yet, and then declares a false residual on the first tick where a guild
//! or a cell moves — failing exactly the way it was built to catch. Hence the
//! named doors [`DomainSums::add_guild_lane_32`] and
//! [`DomainSums::add_cell_masses_32`], and a test that feeds them in S0.
//!
//! # No floating point on this path
//!
//! The sum of water at 256 cubed is `8.59e18`, well past the `2^53` where an
//! `f64` stops being exact. One `as f64` in a helper, a metric or an error
//! message and the difference of two sums stops being exact while still looking
//! plausible. Every counted number here is widened through `From`, never through
//! `as`: the `as` casts that remain are array addresses, where a wrong value
//! panics on the bounds check instead of quietly changing an amount.
//!
//! # What is deliberately not here
//!
//! Nothing of ADR-059 is left outside this module and its two callers now. The
//! `exchange` face is built — the ghost cell that makes a field lane
//! `n_voxels + 1` long, `[boundary.reservoir]` and `alpha_ex = k_ex*dt/dx` — and
//! `process::Conservation::ChangedThrough(Channel)` is the arm the transport
//! processes declare when the grid they were folded for vents.
//!
//! What is still open here is the **sign**, and it is open in the record rather
//! than in the code: `TODO(counter-sign)` below. It stopped being untestable
//! with the face: `boundary_outflow_appears_in_channel_counter` is the one place
//! where the other convention can fail, because on a closed domain both give
//! zero.

use anyhow::{Context, Result, bail};

use crate::numeric::{M32, M64};

/// How many external channels there are. Six, and closed (ADR-059).
pub const CHANNEL_COUNT: usize = 6;

/// A named way for matter or energy to cross the boundary of the domain
/// (SPEC section 7).
///
/// **Closed in code, on purpose, and this is the one place where ADR-018 does
/// not reach.** Its list of what is data — substances, reactions, processes,
/// presets, metrics — does not include the channel, and the reason is stronger
/// than "a channel is not a substance": the channel *is* the right-hand side of
/// the equation everything else is checked against. A channel registry a
/// scenario could edit would let any residual be closed by declaring a seventh
/// channel and pointing a process at it, and the check that exists precisely in
/// order to fail would lose the ability to fail — silently, with the config
/// loading and the tests green. The practical border: **a channel is code, its
/// modulation is data.** A scenario that needs a river inflow or another vent
/// configures a reservoir or an event on one of these six.
///
/// The discriminants are an interface, not an implementation detail: they
/// address the counter table here and they will be the channel numbers in the
/// metric stream (ADR-037). Reordering the variants compiles, breaks nothing
/// visible, and changes the meaning of every number ever recorded — the same
/// reason `world::Face` pins its numbering in a test.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Channel {
    /// Energy in, on a daily and seasonal cycle. Counts what was **absorbed**,
    /// not what fell: light that crossed the domain without being absorbed
    /// never entered the accounting at all, and crediting it would create an
    /// input with no destination (ADR-049, ADR-059).
    SolarIn = 0,
    /// Heat at `z = 0`, and H2S, CH4, FE2, MINERAL at the vents.
    GeothermalIn = 1,
    /// `sigma*T^4` off the top layer.
    RadiativeOut = 2,
    /// Two-way exchange with the outside reservoir, through the `exchange`
    /// face. Credited by steps `c` and `d`, on **every** substep, and by nothing
    /// else (ADR-059).
    BoundaryExchange = 3,
    /// A rare event: matter, energy, a crater.
    Impact = 4,
    /// A rare event: a burst from a vent.
    VentBurst = 5,
}

impl Channel {
    /// Every channel, in discriminant order.
    pub const ALL: [Channel; CHANNEL_COUNT] = [
        Channel::SolarIn,
        Channel::GeothermalIn,
        Channel::RadiativeOut,
        Channel::BoundaryExchange,
        Channel::Impact,
        Channel::VentBurst,
    ];

    /// The spelling of the channel, as SPEC section 7 prints it.
    ///
    /// The only place the spelling exists. A validator, a metric name and a
    /// panic message all come through here, so that there is no second list to
    /// drift out of step with this one.
    pub const fn name(self) -> &'static str {
        match self {
            Channel::SolarIn => "SOLAR_IN",
            Channel::GeothermalIn => "GEOTHERMAL_IN",
            Channel::RadiativeOut => "RADIATIVE_OUT",
            Channel::BoundaryExchange => "BOUNDARY_EXCHANGE",
            Channel::Impact => "IMPACT",
            Channel::VentBurst => "VENT_BURST",
        }
    }

    /// The channel with this exact spelling, or `None`.
    ///
    /// The door for the loader. Upper case and nothing else: a scenario naming
    /// `solar_in` or `LIGHT` is rejected rather than guessed at, and ADR-059
    /// says why `LIGHT` in particular has to be rejected — absorbed light is
    /// already inside the accounting by the time a reaction could name it, so
    /// debiting it through a channel would subtract it twice.
    pub fn from_name(name: &str) -> Option<Channel> {
        Channel::ALL.into_iter().find(|c| c.name() == name)
    }

    /// The channel's row in a counter table.
    #[inline]
    const fn row(self) -> usize {
        self as usize
    }
}

/// Sum a lane exactly, widening every element into the accumulator.
///
/// One `i128` add per element and nothing else. A partial sum in the storage
/// width would be the whole bug this module exists to avoid: the pool of water
/// alone reaches 93% of `i64` at the grid size the roadmap names, and a lane is
/// a whole field of one substance. That is a claim about the *inside* of these
/// two functions, so it has a test of its own —
/// `a_lane_whose_own_sum_passes_i64_is_still_exact`. Nothing else in the corpus
/// can fail on it: every other lane fed anywhere sums to a small fraction of
/// `i64`, the water acceptance test included, because it tiles.
fn total_32(lane: &[M32]) -> i128 {
    lane.iter().map(|v| i128::from(v.to_i64())).sum()
}

fn total_64(lane: &[M64]) -> i128 {
    lane.iter().map(|v| i128::from(v.to_i64())).sum()
}

/// The left-hand side of the invariant: everything the domain holds, per
/// substance and in energy, summed exactly.
///
/// Filled once before a tick and once after it, and the difference of the two is
/// what [`Ledger::residual_matter`] and [`Ledger::residual_energy`] hold against
/// the channels. Every accumulator is `i128` — see the module header for why
/// `i64` is not enough at the grid size the roadmap already names.
///
/// # What goes in, and through which door
///
/// The doors are named after the thing being summed rather than after the
/// buffer's type, because the type does not say which half of the invariant a
/// quantity belongs to and the name has to.
///
/// - amounts of a substance: [`add_field_lane_32`](Self::add_field_lane_32),
///   [`add_field_lane_64`](Self::add_field_lane_64);
/// - guild fields, in moles of `BIOMASS`:
///   [`add_guild_lane_32`](Self::add_guild_lane_32);
/// - `struct_mass` of the cell table, also in moles of `BIOMASS`:
///   [`add_cell_masses_32`](Self::add_cell_masses_32),
///   [`add_cell_masses_64`](Self::add_cell_masses_64);
/// - enthalpy: [`add_enthalpy_lane_32`](Self::add_enthalpy_lane_32), and it
///   goes into the energy half only;
/// - `energy` of the cell table:
///   [`add_cell_energy_32`](Self::add_cell_energy_32),
///   [`add_cell_energy_64`](Self::add_cell_energy_64).
///
/// # What does not go in: `BT[g][t]`
///
/// The trait sums of `QUANTITIES.md` section 7 are class `M` and are stored the
/// same way a guild field is, so adding them into the per-substance sum
/// type-checks and compiles. It would also be wrong: their unit is
/// "mole times trait unit", they are the one class-`M` quantity that is not a
/// physical amount, and the residual would come out non-zero in proportion to
/// the trait value — which reads exactly like a bug in biomass transport.
// TODO(bt-invariant): whether `BT[g][t]` has an invariant of its own is not
// decided anywhere. It is class `M` and must be conserved by transport
// (`QUANTITIES.md` section 7), but it is neither moles of a substance nor
// joules, so neither residual is its home. It is accepted by no door here, on
// purpose; the decision belongs in `DECISIONS.md`, with the wave that writes
// guild transport.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomainSums {
    /// One accumulator per substance, in that substance's storage units.
    matter: Vec<i128>,
    /// One accumulator for the whole domain, in joules.
    energy: i128,
}

impl DomainSums {
    /// Accumulators for `n_substances` substances, all zero.
    ///
    /// # Errors
    ///
    /// Returns an error if `n_substances` is zero: a domain with no substances
    /// has nothing to close, and sums that were never fed balance against a
    /// ledger that counted nothing — `0 - 0 == 0`, on every tick, for ever.
    pub fn new(n_substances: u32) -> Result<Self> {
        if n_substances == 0 {
            bail!("domain sums over no substances close against anything (ADR-003)");
        }
        let width =
            usize::try_from(n_substances).context("more substances than this host can index")?;
        Ok(Self {
            matter: vec![0; width],
            energy: 0,
        })
    }

    /// How many substances this accumulates.
    pub fn n_substances(&self) -> u32 {
        // The constructor took a `u32` and the length has not changed since.
        self.matter.len() as u32
    }

    /// Back to all zeroes, keeping the allocation.
    ///
    /// The two `DomainSums` of a tick are reused across ticks; the reduction
    /// runs once a tick, and allocating twice a tick for it would be a cost with
    /// no reason behind it.
    pub fn clear(&mut self) {
        self.matter.fill(0);
        self.energy = 0;
    }

    /// The accumulator of one substance, or a panic naming the range.
    #[track_caller]
    fn matter_mut(&mut self, substance: u32) -> &mut i128 {
        let width = self.matter.len();
        let Some(slot) = usize::try_from(substance)
            .ok()
            .and_then(|s| self.matter.get_mut(s))
        else {
            panic!("substance {substance} is outside the {width} this domain accounts for");
        };
        slot
    }

    /// Add one lane of a 32-bit amount field for `substance`.
    #[track_caller]
    pub fn add_field_lane_32(&mut self, substance: u32, lane: &[M32]) {
        *self.matter_mut(substance) += total_32(lane);
    }

    /// Add one lane of a 64-bit amount field for `substance`. Water, and
    /// anything else the loader gave the wide width to (ADR-040).
    #[track_caller]
    pub fn add_field_lane_64(&mut self, substance: u32, lane: &[M64]) {
        *self.matter_mut(substance) += total_64(lane);
    }

    /// Add one guild field. `biomass` is the substance index the sum lands on.
    ///
    /// `guild[g]` is measured in moles of `BIOMASS` (`QUANTITIES.md`
    /// section 7), so it is part of the left-hand side of that substance and of
    /// no other. The argument carries the one thing that is not obvious from a
    /// buffer of `M32`: which substance is being added to.
    #[track_caller]
    pub fn add_guild_lane_32(&mut self, biomass: u32, guild: &[M32]) {
        *self.matter_mut(biomass) += total_32(guild);
    }

    /// Add the `struct_mass` column of the cell table, in moles of `BIOMASS`
    /// (SPEC section 6.1).
    ///
    /// Rows, not voxels: the cell table is a list, and a domain sum does not
    /// care where a row lives.
    #[track_caller]
    pub fn add_cell_masses_32(&mut self, biomass: u32, struct_mass: &[M32]) {
        *self.matter_mut(biomass) += total_32(struct_mass);
    }

    /// The same column at the wide width. Which width the loader derives for
    /// `BIOMASS` is not this module's business (ADR-039, ADR-040).
    #[track_caller]
    pub fn add_cell_masses_64(&mut self, biomass: u32, struct_mass: &[M64]) {
        *self.matter_mut(biomass) += total_64(struct_mass);
    }

    /// Add the enthalpy field, in joules.
    ///
    /// Energy only, and there is no way to say otherwise: enthalpy has no
    /// substance argument, so it cannot be added to a substance by a slip of
    /// the hand. `i32` on a 32-cubed grid (`QUANTITIES.md` section 3, ADR-045).
    pub fn add_enthalpy_lane_32(&mut self, lane: &[M32]) {
        self.energy += total_32(lane);
    }

    /// The same field at the wide width, which is the width it actually has.
    ///
    /// `world::World` stores enthalpy as a `Field64`, because the window for
    /// `k_E` at `i32` is empty and the miss is twenty-six binary orders
    /// (ADR-062). The narrow door above predates that record — three places in
    /// the corpus still print `i32` for this quantity — and both are kept
    /// because a `DerivedEnergy` carries a derived width and this module does not
    /// get to decide which one a scenario ended up with.
    pub fn add_enthalpy_lane_64(&mut self, lane: &[M64]) {
        self.energy += total_64(lane);
    }

    /// Add the `energy` column of the cell table, in joules (SPEC section 6.1).
    pub fn add_cell_energy_32(&mut self, energy: &[M32]) {
        self.energy += total_32(energy);
    }

    /// The same column at the wide width.
    pub fn add_cell_energy_64(&mut self, energy: &[M64]) {
        self.energy += total_64(energy);
    }

    /// Everything the domain holds of `substance`, in its storage units.
    ///
    /// # Panics
    ///
    /// If `substance` is outside the range this was built for.
    #[track_caller]
    pub fn matter(&self, substance: u32) -> i128 {
        let width = self.matter.len();
        let Some(&sum) = usize::try_from(substance)
            .ok()
            .and_then(|s| self.matter.get(s))
        else {
            panic!("substance {substance} is outside the {width} this domain accounts for");
        };
        sum
    }

    /// Everything the domain holds in energy, in joules.
    pub fn energy(&self) -> i128 {
        self.energy
    }
}

/// The channel counters, and the two residuals they close.
///
/// One signed `i64` per `(channel, substance)` pair and one per
/// `(channel, energy)` pair, plus a snapshot of both taken at the start of the
/// tick. Ninety counters at the fourteen substances of SPEC section 2.3 — 720
/// bytes, 1 440 with the snapshot; at `world::registry::MAX_SUBSTANCES`, which
/// is thirty-one because the thirty-second index belongs to enthalpy, 192
/// counters, 1 536 bytes and 3 072 with the snapshot. Neither the table nor the
/// snapshot is per voxel, so the byte budget of ADR-045 does not move at all.
///
/// # Signed, and which way
///
/// **A credit is positive when the quantity entered the domain.** Under that
/// convention the invariant of ADR-003 reads literally — `delta(fields + cells)
/// == sum(flows)` — and so does the residual formula of ADR-059. The
/// antisymmetry paragraph of that same record describes the *pairing* (the
/// domain and the counter are the two sides of one exchange, and their sum over
/// a flow is zero), not the sign of the stored number.
// TODO(counter-sign): ADR-059 states both conventions in one record — the
// antisymmetry paragraph has the counter take `-f` where the voxel takes `+f`,
// the residual paragraph has `residual = delta(domain) - sum(counters) == 0`,
// and those differ by exactly a sign. The choice above is the one that makes
// both SPEC section 2.1 and the residual formula true as written, but the record
// does not make it, and existing records are not edited: this needs a new entry
// in `DECISIONS.md`. Until it exists, the convention lives in
// `a_counter_is_signed_from_the_domains_point_of_view` — the only place where
// getting it backwards can fail, since on a closed domain both sides are zero.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ledger {
    n_substances: u32,
    /// `channel * n_substances + substance`, running net over the whole run.
    matter: Vec<i64>,
    /// One per channel, running net over the whole run.
    energy: [i64; CHANNEL_COUNT],
    /// Both of the above as they stood at the last [`Ledger::begin_tick`].
    matter_at_tick_start: Vec<i64>,
    energy_at_tick_start: [i64; CHANNEL_COUNT],
}

impl Ledger {
    /// Counters for `n_substances` substances, all zero.
    ///
    /// # Errors
    ///
    /// Returns an error if `n_substances` is zero — a ledger with nothing to
    /// count closes against anything — or if the table would be longer than a
    /// `usize` can address.
    ///
    /// It does **not** check the build bound on the number of substances.
    /// `world::registry::MAX_SUBSTANCES` already refuses a registry above it,
    /// and a ledger is built from a registry's count; repeating the check here
    /// would give the bound a second home and a second error message to drift
    /// out of step with the first.
    pub fn new(n_substances: u32) -> Result<Self> {
        if n_substances == 0 {
            bail!("a ledger over no substances closes against anything (ADR-003)");
        }
        let width =
            usize::try_from(n_substances).context("more substances than this host can index")?;
        let Some(len) = width.checked_mul(CHANNEL_COUNT) else {
            bail!("a counter table for {n_substances} substances is longer than a usize");
        };
        Ok(Self {
            n_substances,
            matter: vec![0; len],
            energy: [0; CHANNEL_COUNT],
            matter_at_tick_start: vec![0; len],
            energy_at_tick_start: [0; CHANNEL_COUNT],
        })
    }

    /// How many substances this counts.
    pub fn n_substances(&self) -> u32 {
        self.n_substances
    }

    /// The counter table is `channel`-major: one contiguous row of substances
    /// per channel.
    ///
    /// Which way round the two indices go cannot be got wrong loudly — at six
    /// substances the transposed formula stays inside the table and merely
    /// shuffles the pairs — so the address is computed here, once.
    #[track_caller]
    fn slot(&self, channel: Channel, substance: u32) -> usize {
        assert!(
            substance < self.n_substances,
            "substance {substance} is outside the {} this ledger counts",
            self.n_substances
        );
        // Both factors are indices, not counted quantities, and both came out
        // of a `usize` in the constructor.
        channel.row() * self.n_substances as usize + substance as usize
    }

    /// Take the snapshot the tick's increment is measured from.
    ///
    /// Called at the top of the tick. Everything credited afterwards is what
    /// [`Ledger::residual_matter`] and [`Ledger::residual_energy`] hold the
    /// change of the domain against; the running totals go on growing across it.
    pub fn begin_tick(&mut self) {
        self.matter_at_tick_start.copy_from_slice(&self.matter);
        self.energy_at_tick_start = self.energy;
    }

    /// Credit `units` of `substance` to `channel`: positive when the matter
    /// entered the domain.
    ///
    /// Outside any `cfg`, on purpose. ADR-059 puts the *reduction* in the debug
    /// build; the counters tick in release too, and folding both into one
    /// `cfg(debug_assertions)` would produce the rare thing that is worse than a
    /// wrong debug build — a release build that silently keeps no accounting at
    /// all.
    ///
    /// # Panics
    ///
    /// If `substance` is out of range, and — in **both** build profiles — if
    /// the counter would overflow. This is the opposite of `M::add`, where
    /// wrapping in release is documented and safe because the storage width was
    /// derived from `max_conc`. A wrapped counter turns a converging ledger into
    /// a diverging one with nothing in the output to see, and a credit happens
    /// once per channel per tick on the host, where `checked_add` costs nothing.
    #[track_caller]
    pub fn credit_matter(&mut self, channel: Channel, substance: u32, units: i64) {
        let slot = self.slot(channel, substance);
        let counter = self.matter[slot];
        let Some(sum) = counter.checked_add(units) else {
            panic!(
                "ledger counter overflowed: channel {}, substance {substance}, \
                 {counter} + {units}. Wrapping here would change the sign of a \
                 counter and leave the ledger closing against a right-hand side \
                 that is no longer the truth (ADR-059).",
                channel.name()
            );
        };
        self.matter[slot] = sum;
    }

    /// Credit `joules` to `channel`: positive when the energy entered the
    /// domain. The rules of [`Ledger::credit_matter`] apply unchanged.
    ///
    /// # Panics
    ///
    /// On overflow, in both build profiles.
    #[track_caller]
    pub fn credit_energy(&mut self, channel: Channel, joules: i64) {
        let counter = self.energy[channel.row()];
        let Some(sum) = counter.checked_add(joules) else {
            panic!(
                "ledger counter overflowed: channel {}, energy, {counter} + \
                 {joules}. Wrapping here would change the sign of a counter and \
                 leave the ledger closing against a right-hand side that is no \
                 longer the truth (ADR-059).",
                channel.name()
            );
        };
        self.energy[channel.row()] = sum;
    }

    /// Credit an energy flow that was accumulated in `i128`, narrowing it back
    /// through a checked conversion.
    ///
    /// The one caller today is the solar reduction of ADR-075, and the width is
    /// its accumulator's rather than this counter's. The exact sum of the fold's
    /// solar slice does not fit an `i64` at the declared span of the enthalpy
    /// field — `32 768 x 1.97e18 = 6.46e22` against `9.22e18` — so a reduction
    /// that accumulated in `i64` would wrap into a plausible negative *before*
    /// any conversion, and a `try_from` over an already-wrapped number is silent.
    /// Hence: widen, add, and narrow exactly once, here.
    ///
    /// Outside every `cfg`, like [`Ledger::credit_energy`]. The counters tick in
    /// both build profiles, and a release build that quietly skipped a credit
    /// would give a diverging ledger with no panic anywhere — the rare thing that
    /// is worse than a wrong debug build.
    ///
    /// # Panics
    ///
    /// If `units` does not fit an `i64` — naming the channel, the number and
    /// open question A-20 — and on overflow of the counter itself, in both build
    /// profiles.
    ///
    /// This is a refusal and not a loss of account: saturating or wrapping here
    /// would leave the ledger closing against a right-hand side that is no longer
    /// the truth. Whether an `i64` counter is the right width at all is open
    /// question A-20 (A-19 in ADR-075 and ADR-076, which were drafted while that
    /// number was free), and at `k_E = 67` the answer is visibly "no" — the
    /// counter holds `2^63/2^67 = 62.5 mJ` against the `0.16384 J` one lit tick
    /// of a 128 cubed domain delivers (ADR-075, ADR-076). Until A-20 is answered
    /// no scenario may declare `i_surface > 0`, which is what keeps this panic
    /// unreachable rather than imminent.
    #[track_caller]
    pub fn credit_energy_wide(&mut self, channel: Channel, units: i128) {
        let Ok(narrow) = i64::try_from(units) else {
            panic!(
                "ledger credit does not fit the counter: channel {}, energy, \
                 {units} units against an i64 counter. The width of a channel \
                 counter is open question A-20 — A-19 in ADR-075 and ADR-076, \
                 which were drafted while that number was free — and at k_E = 67 \
                 an i64 holds 2^63/2^67 = 62.5 mJ, while one lit tick of a 128^3 \
                 domain is 0.16384 J, which is 2.62 ceilings (ADR-075, ADR-076). \
                 Wrapping or saturating here would keep the run going with a \
                 ledger that closes against a right-hand side that is no longer \
                 the truth",
                channel.name()
            );
        };
        self.credit_energy(channel, narrow);
    }

    /// The running net flow of `substance` through `channel` over the whole run.
    #[track_caller]
    pub fn matter(&self, channel: Channel, substance: u32) -> i64 {
        self.matter[self.slot(channel, substance)]
    }

    /// The running net energy flow through `channel` over the whole run.
    pub fn energy(&self, channel: Channel) -> i64 {
        self.energy[channel.row()]
    }

    /// What this tick has credited so far, measured from
    /// [`Ledger::begin_tick`].
    ///
    /// This, and not [`Ledger::matter`], is the right-hand side of the residual.
    /// The two have the same type and nearly the same name, and they differ from
    /// the second tick on.
    #[track_caller]
    pub fn matter_this_tick(&self, channel: Channel, substance: u32) -> i64 {
        let slot = self.slot(channel, substance);
        self.matter[slot] - self.matter_at_tick_start[slot]
    }

    /// The same for energy.
    pub fn energy_this_tick(&self, channel: Channel) -> i64 {
        self.energy[channel.row()] - self.energy_at_tick_start[channel.row()]
    }

    /// Everything credited for one substance this tick, over all six channels.
    fn credited_matter(&self, substance: u32) -> i128 {
        Channel::ALL
            .into_iter()
            .map(|channel| i128::from(self.matter_this_tick(channel, substance)))
            .sum()
    }

    /// Everything credited in energy this tick, over all six channels.
    fn credited_energy(&self) -> i128 {
        Channel::ALL
            .into_iter()
            .map(|channel| i128::from(self.energy_this_tick(channel)))
            .sum()
    }

    /// `delta(domain) - sum(channels)` for one substance over this tick.
    ///
    /// Required to be an exact zero (ADR-003, ADR-059). Not "small": both sides
    /// are integers and the schemes conserve by construction rather than by
    /// accuracy, so anything but zero means matter appeared or vanished without
    /// a name.
    ///
    /// # Panics
    ///
    /// If `substance` is outside the count, or if either `DomainSums` accounts
    /// for a different number of substances than this ledger counts — the two
    /// are built from one registry and there is nothing else that says so.
    #[track_caller]
    pub fn residual_matter(&self, substance: u32, before: &DomainSums, after: &DomainSums) -> i128 {
        // Both sums are held against the ledger's own count, not merely against
        // each other, and the reason is that the two mistakes fail in opposite
        // directions. Sums *narrower* than the ledger are loud already:
        // `assert_closed` walks `0..n_substances` and `DomainSums::matter`
        // panics on the first index past its end. Sums *wider* than it are the
        // silent one — the walk covers a prefix, every residual in it is zero,
        // and the tick is reported closed with the rest of the registry never
        // looked at. Nothing else ties the two constructions to one count: a
        // `Ledger` is built from a number and so is a `DomainSums`.
        assert!(
            before.n_substances() == self.n_substances && after.n_substances() == self.n_substances,
            "the ledger counts {} substances, the domain sums account for {} \
             before the tick and {} after it",
            self.n_substances,
            before.n_substances(),
            after.n_substances()
        );
        after.matter(substance) - before.matter(substance) - self.credited_matter(substance)
    }

    /// The same for energy — a separate number, because ADR-028 made the
    /// invariant double and one axis cannot express a process that conserves
    /// matter while moving energy.
    pub fn residual_energy(&self, before: &DomainSums, after: &DomainSums) -> i128 {
        after.energy() - before.energy() - self.credited_energy()
    }

    /// Both residuals, or a panic naming what did not close.
    ///
    /// This is phase 5 LEDGER of the tick order (SPEC section 8): after APPLY,
    /// before OBSERVE, in the debug build. A function rather than a
    /// `debug_assert!` buried inside the reduction, because the caller has to be
    /// able to see it — the phase decides whether it runs, the reduction does
    /// not, and the two are not the same `cfg`.
    ///
    /// # Panics
    ///
    /// If any residual is non-zero. The message names the substance, both sides
    /// and the per-channel breakdown: the residual alone says that the tick did
    /// not close and nothing about which channel failed to be credited. Also if
    /// the sums and the ledger disagree about how many substances there are —
    /// see [`Ledger::residual_matter`], which is where that is caught, and which
    /// this always reaches because a ledger over zero substances is refused.
    #[track_caller]
    pub fn assert_closed(&self, before: &DomainSums, after: &DomainSums) {
        for substance in 0..self.n_substances {
            let residual = self.residual_matter(substance, before, after);
            assert!(
                residual == 0,
                "the matter ledger did not close: substance {substance} is off \
                 by {residual}. The domain went {} -> {} (delta {}), the \
                 channels credited {} this tick: {}",
                before.matter(substance),
                after.matter(substance),
                after.matter(substance) - before.matter(substance),
                self.credited_matter(substance),
                self.breakdown_matter(substance)
            );
        }

        let residual = self.residual_energy(before, after);
        assert!(
            residual == 0,
            "the energy ledger did not close: off by {residual}. The domain went \
             {} -> {} (delta {}), the channels credited {} this tick: {}",
            before.energy(),
            after.energy(),
            after.energy() - before.energy(),
            self.credited_energy(),
            self.breakdown_energy()
        );
    }

    fn breakdown_matter(&self, substance: u32) -> String {
        Channel::ALL
            .into_iter()
            .map(|channel| {
                format!(
                    "{}={}",
                    channel.name(),
                    self.matter_this_tick(channel, substance)
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn breakdown_energy(&self) -> String {
        Channel::ALL
            .into_iter()
            .map(|channel| format!("{}={}", channel.name(), self.energy_this_tick(channel)))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

// TODO(ledger-phase): nobody calls `begin_tick` or `assert_closed` yet, because
// the tick loop of SPEC section 8 does not exist. Two things move with it:
//
// - the gate. ADR-059 wants the reduction in the debug build and the counters in
//   every build; whether that is a `cfg(debug_assertions)` around the call in
//   the orchestrator or a `debug_assert!` at the call site is a decision for the
//   wave that writes the phases;
// - the prohibition. "After phase 4 no channel is written" (ADR-059): a process
//   crediting during OBSERVE would make the *next* tick's residual wrong, and it
//   would not name the culprit. Nothing here can catch that today — there are no
//   phases to be after.

#[cfg(test)]
mod tests {
    use super::*;

    /// The refusal `f` panicked with, or `None` if it returned.
    ///
    /// `#[should_panic(expected = ...)]` takes one substring and ends the test
    /// where the panic is, and the three claims about this refusal are not of
    /// that shape: one is a conjunction (the message names the channel *and* the
    /// number *and* the question), one is a pair (accepted here, refused one
    /// past here), and one is about the state the ledger is left in — which a
    /// test that ends at the panic cannot look at, and which is the whole
    /// difference between a refusal and a wrap.
    ///
    /// The panic hook is left alone deliberately: silencing it is global state
    /// shared with every other test in this binary, and libtest already discards
    /// the captured output of a test that passes.
    fn refusal_of(f: impl FnOnce()) -> Option<String> {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
            Ok(()) => None,
            Err(payload) => Some(
                payload
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                    .unwrap_or_else(|| "a panic carrying no message at all".to_owned()),
            ),
        }
    }

    #[test]
    fn the_channel_numbering_is_the_one_metrics_and_configs_will_name() {
        // These numbers address the counter table and will be the channel
        // numbers of the metric stream (ADR-037). Reordering the variants
        // compiles and changes the meaning of every number ever recorded.
        assert_eq!(Channel::SolarIn as u32, 0);
        assert_eq!(Channel::GeothermalIn as u32, 1);
        assert_eq!(Channel::RadiativeOut as u32, 2);
        assert_eq!(Channel::BoundaryExchange as u32, 3);
        assert_eq!(Channel::Impact as u32, 4);
        assert_eq!(Channel::VentBurst as u32, 5);

        assert_eq!(Channel::ALL.len(), CHANNEL_COUNT);
        for (i, channel) in Channel::ALL.into_iter().enumerate() {
            assert_eq!(channel as usize, i);
            // The round trip, so that the loader's door and the metric's label
            // cannot disagree about a channel.
            assert_eq!(Channel::from_name(channel.name()), Some(channel));
        }

        // The spelling of SPEC section 7, upper case, written out here because
        // `name()` is the only place it exists.
        assert_eq!(Channel::SolarIn.name(), "SOLAR_IN");
        assert_eq!(Channel::GeothermalIn.name(), "GEOTHERMAL_IN");
        assert_eq!(Channel::RadiativeOut.name(), "RADIATIVE_OUT");
        assert_eq!(Channel::BoundaryExchange.name(), "BOUNDARY_EXCHANGE");
        assert_eq!(Channel::Impact.name(), "IMPACT");
        assert_eq!(Channel::VentBurst.name(), "VENT_BURST");

        // `LIGHT` is the name SPEC section 5 prints and ADR-059 rejects; lower
        // case is not a spelling of anything.
        assert_eq!(Channel::from_name("LIGHT"), None);
        assert_eq!(Channel::from_name("solar_in"), None);
        assert_eq!(Channel::from_name(""), None);
    }

    #[test]
    fn every_reader_of_the_counter_table_agrees_with_the_writer() {
        // Three substances against six channels, asymmetric on purpose, and a
        // distinct value in every one of the eighteen pairs.
        //
        // `channel * n + substance` and `substance * CHANNEL_COUNT + channel`
        // both stay inside an eighteen-element table, so getting the address
        // wrong is never a bounds error. A table that is consistently one shape
        // or the other is only a layout and cannot be seen from out here; two
        // things can, and both are silent otherwise: a counter shared by two
        // pairs, and a reader that computes the address differently from the
        // writer. The second is the dangerous one — `matter_this_tick` is the
        // right-hand side of the residual, so a disagreement there closes the
        // ledger against another substance's flow.
        const N: u32 = 3;
        let value = |channel: Channel, substance: u32| -> i64 {
            (channel as i64 + 1) * 1_000 + i64::from(substance) + 1
        };

        let mut ledger = Ledger::new(N).unwrap();
        ledger.begin_tick();
        for channel in Channel::ALL {
            for substance in 0..N {
                ledger.credit_matter(channel, substance, value(channel, substance));
            }
        }
        for channel in Channel::ALL {
            for substance in 0..N {
                assert_eq!(
                    ledger.matter(channel, substance),
                    value(channel, substance),
                    "{} substance {substance}",
                    channel.name()
                );
                assert_eq!(
                    ledger.matter_this_tick(channel, substance),
                    value(channel, substance),
                    "{} substance {substance}, this tick",
                    channel.name()
                );
            }
        }

        // And through the residual, which is where a mixed-up row would land in
        // production: each substance gained exactly what its own six channels
        // credited it, and nothing else.
        let mut before = DomainSums::new(N).unwrap();
        let mut after = DomainSums::new(N).unwrap();
        for substance in 0..N {
            let credited: i64 = Channel::ALL
                .into_iter()
                .map(|channel| value(channel, substance))
                .sum();
            let start = 1_000 * i32::try_from(substance + 1).unwrap();
            before.add_field_lane_32(substance, &[M32::new(start)]);
            after.add_field_lane_32(
                substance,
                &[M32::new(start + i32::try_from(credited).unwrap())],
            );
        }
        ledger.assert_closed(&before, &after);
    }

    /// One substance, one voxel, so that the arithmetic is the whole test.
    fn sums_of_one(amount: i32) -> DomainSums {
        let mut sums = DomainSums::new(1).unwrap();
        sums.add_field_lane_32(0, &[M32::new(amount)]);
        sums
    }

    #[test]
    fn a_counter_is_signed_from_the_domains_point_of_view() {
        // ADR-059 names two opposite conventions and picks neither; this is the
        // one place where getting it backwards can fail at all, because on a
        // closed domain both sides of the residual are zero.
        const K: i32 = 7_000;
        let before = sums_of_one(1_000);
        let after = sums_of_one(1_000 + K);

        let mut ledger = Ledger::new(1).unwrap();
        ledger.begin_tick();
        ledger.credit_matter(Channel::BoundaryExchange, 0, i64::from(K));
        assert_eq!(ledger.residual_matter(0, &before, &after), 0);

        // The other convention, and what it costs: not a small error, but twice
        // the flow, on every tick where anything crosses the boundary.
        let mut backwards = Ledger::new(1).unwrap();
        backwards.begin_tick();
        backwards.credit_matter(Channel::BoundaryExchange, 0, -i64::from(K));
        assert_eq!(
            backwards.residual_matter(0, &before, &after),
            i128::from(2 * K)
        );

        // Energy reads the same way.
        let mut before_e = DomainSums::new(1).unwrap();
        before_e.add_enthalpy_lane_32(&[M32::new(50)]);
        let mut after_e = DomainSums::new(1).unwrap();
        after_e.add_enthalpy_lane_32(&[M32::new(50 + K)]);

        let mut energy = Ledger::new(1).unwrap();
        energy.begin_tick();
        energy.credit_energy(Channel::SolarIn, i64::from(K));
        assert_eq!(energy.residual_energy(&before_e, &after_e), 0);
    }

    #[test]
    fn the_residual_is_the_tick_delta_not_the_running_total() {
        let mut ledger = Ledger::new(1).unwrap();

        // Tick one: a hundred units in.
        ledger.begin_tick();
        ledger.credit_matter(Channel::Impact, 0, 100);
        assert_eq!(
            ledger.residual_matter(0, &sums_of_one(0), &sums_of_one(100)),
            0
        );

        // Tick two: a hundred and fifty more. An implementation that measured
        // the residual against the running total instead of against the
        // snapshot is green on the first tick and wrong from here on — and on a
        // closed domain it is never wrong at all, which is how it survives.
        ledger.begin_tick();
        ledger.credit_matter(Channel::Impact, 0, 150);
        assert_eq!(
            ledger.residual_matter(0, &sums_of_one(100), &sums_of_one(250)),
            0
        );

        assert_eq!(ledger.matter_this_tick(Channel::Impact, 0), 150);
        assert_eq!(ledger.matter(Channel::Impact, 0), 250);
        // The two numbers have the same type and nearly the same name, and on
        // the second tick they differ. That is the whole trap.
        assert_ne!(
            ledger.matter_this_tick(Channel::Impact, 0),
            ledger.matter(Channel::Impact, 0)
        );

        // The energy half falls into the same trap through a different door.
        // ADR-028 makes the invariant double, so `begin_tick` takes two
        // snapshots and either can be forgotten on its own; every other energy
        // case in this module credits once after a single `begin_tick`, where
        // `energy_this_tick` and `energy` are equal by construction and a
        // missing snapshot is invisible. Two ticks are what make them differ.
        let joules = |j: i32| {
            let mut sums = DomainSums::new(1).unwrap();
            sums.add_enthalpy_lane_32(&[M32::new(j)]);
            sums
        };

        let mut energy = Ledger::new(1).unwrap();
        energy.begin_tick();
        energy.credit_energy(Channel::SolarIn, 100);
        assert_eq!(energy.residual_energy(&joules(0), &joules(100)), 0);

        energy.begin_tick();
        energy.credit_energy(Channel::SolarIn, 150);
        assert_eq!(energy.residual_energy(&joules(100), &joules(250)), 0);

        assert_eq!(energy.energy_this_tick(Channel::SolarIn), 150);
        assert_eq!(energy.energy(Channel::SolarIn), 250);
        assert_ne!(
            energy.energy_this_tick(Channel::SolarIn),
            energy.energy(Channel::SolarIn)
        );
    }

    #[test]
    fn matter_and_energy_residuals_are_separate() {
        const JOULES: i32 = 4_200;

        // Light: matter untouched, energy in (SPEC section 4.6, ADR-049).
        let mut before = DomainSums::new(2).unwrap();
        before.add_field_lane_32(0, &[M32::new(10), M32::new(20)]);
        before.add_field_lane_64(1, &[M64::new(30), M64::new(40)]);
        before.add_enthalpy_lane_32(&[M32::new(1_000)]);

        let mut after = DomainSums::new(2).unwrap();
        after.add_field_lane_32(0, &[M32::new(10), M32::new(20)]);
        after.add_field_lane_64(1, &[M64::new(30), M64::new(40)]);
        after.add_enthalpy_lane_32(&[M32::new(1_000 + JOULES)]);

        let mut ledger = Ledger::new(2).unwrap();
        ledger.begin_tick();
        ledger.credit_energy(Channel::SolarIn, i64::from(JOULES));
        assert_eq!(ledger.residual_matter(0, &before, &after), 0);
        assert_eq!(ledger.residual_matter(1, &before, &after), 0);
        assert_eq!(ledger.residual_energy(&before, &after), 0);
        ledger.assert_closed(&before, &after);

        // And the two halves are genuinely separate numbers: the same tick with
        // the energy credit missing closes on matter and not on energy. Folded
        // into one counter or one residual, this case is inexpressible.
        let mut half_blind = Ledger::new(2).unwrap();
        half_blind.begin_tick();
        assert_eq!(half_blind.residual_matter(0, &before, &after), 0);
        assert_eq!(
            half_blind.residual_energy(&before, &after),
            i128::from(JOULES)
        );

        // The opposite shape — energy leaves while matter stays, which is what
        // the upkeep of an expression channel does (ADR-029), here through the
        // outward energy channel of SPEC section 7.
        let mut ledger = Ledger::new(2).unwrap();
        ledger.begin_tick();
        ledger.credit_energy(Channel::RadiativeOut, -i64::from(JOULES));
        assert_eq!(ledger.residual_matter(0, &after, &before), 0);
        assert_eq!(ledger.residual_energy(&after, &before), 0);
        ledger.assert_closed(&after, &before);
    }

    #[test]
    fn the_left_side_takes_guild_fields_and_cells_too() {
        // The error ADR-059 spells out: a ledger that sums one `amount[]` and
        // stops declares a false residual on the first tick where a guild or a
        // cell moves. In S0 there are neither, so the doors are fed directly —
        // exactly so that the mistake does not wait for S1 and then look like
        // "the ledger broke when guilds arrived".
        const BIOMASS: u32 = 0;
        const IN_GUILD: i32 = 150;
        const IN_CELLS: i64 = 350;
        // Two guild voxels and two cell rows, so N is what the domain gained.
        const N: i64 = 2 * IN_GUILD as i64 + 2 * IN_CELLS;

        let build = |guild: i32, cells: i64| {
            let mut sums = DomainSums::new(1).unwrap();
            // The amount field does not move at all across this tick.
            sums.add_field_lane_32(BIOMASS, &[M32::new(500), M32::new(500)]);
            sums.add_guild_lane_32(BIOMASS, &[M32::new(guild), M32::new(guild)]);
            sums.add_cell_masses_64(BIOMASS, &[M64::new(cells), M64::new(cells)]);
            sums
        };

        let before = build(50, 100);
        let after = build(50 + IN_GUILD, 100 + IN_CELLS);
        assert_eq!(
            after.matter(BIOMASS) - before.matter(BIOMASS),
            i128::from(N)
        );

        let mut ledger = Ledger::new(1).unwrap();
        ledger.begin_tick();
        ledger.credit_matter(Channel::Impact, BIOMASS, N);
        assert_eq!(ledger.residual_matter(BIOMASS, &before, &after), 0);
        ledger.assert_closed(&before, &after);

        // The same tick with the left side cut down to `amount[]`: off by the
        // whole of what the guilds and the cells did, and in S0 there is nothing
        // else to notice it.
        let amount_only = || {
            let mut sums = DomainSums::new(1).unwrap();
            sums.add_field_lane_32(BIOMASS, &[M32::new(500), M32::new(500)]);
            sums
        };
        assert_eq!(
            ledger.residual_matter(BIOMASS, &amount_only(), &amount_only()),
            i128::from(-N)
        );

        // Enthalpy and cell energy reach the energy half and nothing else. That
        // enthalpy cannot be handed to `add_field_lane_*` is a fact about the
        // signatures rather than about anybody's discipline: it has no substance
        // argument to give.
        let mut sums = DomainSums::new(1).unwrap();
        sums.add_enthalpy_lane_32(&[M32::new(11), M32::new(22)]);
        sums.add_cell_energy_32(&[M32::new(33)]);
        sums.add_cell_energy_64(&[M64::new(44)]);
        assert_eq!(sums.matter(BIOMASS), 0);
        assert_eq!(sums.energy(), 110);
    }

    #[test]
    #[should_panic(expected = "channel BOUNDARY_EXCHANGE, substance 1")]
    fn a_counter_overflow_is_loud_rather_than_wrapping() {
        // Loud in both profiles, unlike `M::add`. A wrapped counter would change
        // sign and turn a converging ledger into a diverging one with nothing in
        // the output to see.
        let mut ledger = Ledger::new(2).unwrap();
        ledger.credit_matter(Channel::BoundaryExchange, 1, i64::MAX);
        ledger.credit_matter(Channel::BoundaryExchange, 1, 1);
    }

    #[test]
    #[should_panic(expected = "channel RADIATIVE_OUT, energy")]
    fn an_energy_counter_overflow_is_loud_too() {
        let mut ledger = Ledger::new(1).unwrap();
        ledger.credit_energy(Channel::RadiativeOut, i64::MIN);
        ledger.credit_energy(Channel::RadiativeOut, -1);
    }

    #[test]
    #[should_panic(expected = "substance 1")]
    fn assert_closed_names_what_did_not_close() {
        let mut before = DomainSums::new(2).unwrap();
        before.add_field_lane_32(0, &[M32::new(7)]);
        before.add_field_lane_64(1, &[M64::new(7)]);

        let mut after = DomainSums::new(2).unwrap();
        after.add_field_lane_32(0, &[M32::new(7)]);
        // Substance 1 gained nine units and no channel says where from.
        after.add_field_lane_64(1, &[M64::new(16)]);

        let ledger = Ledger::new(2).unwrap();
        ledger.assert_closed(&before, &after);
    }

    #[test]
    fn an_empty_accumulator_is_refused() {
        // Sums with nothing in them balance perfectly against a ledger that
        // counted nothing, so zero substances is not a degenerate case to be
        // tolerated — it is a residual that cannot fail.
        assert!(DomainSums::new(0).is_err());
        assert!(Ledger::new(0).is_err());
    }

    // An out-of-range substance is not a bounds error waiting to happen: in a
    // table of this shape it lands on another channel's counter and reads back
    // a perfectly plausible number. So it is refused rather than indexed, at
    // both ends.
    #[test]
    #[should_panic(expected = "substance 2 is outside the 2")]
    fn a_substance_outside_the_ledger_is_loud() {
        let ledger = Ledger::new(2).unwrap();
        let _ = ledger.matter(Channel::Impact, 2);
    }

    #[test]
    #[should_panic(expected = "substance 2 is outside the 2")]
    fn a_substance_outside_the_domain_sums_is_loud() {
        let mut sums = DomainSums::new(2).unwrap();
        sums.add_field_lane_32(2, &[M32::new(1)]);
    }

    #[test]
    #[should_panic(expected = "the ledger counts 1 substances, the domain sums account for 3")]
    fn sums_and_ledger_of_different_widths_are_refused() {
        // The quiet direction: sums wider than the ledger. `assert_closed`
        // would walk substance 0, find it closed, and say nothing about the
        // other two — a check that reports closure having examined a prefix,
        // which is the one shape of failure this module cannot afford.
        let ledger = Ledger::new(1).unwrap();
        let before = DomainSums::new(3).unwrap();
        let after = DomainSums::new(3).unwrap();
        ledger.assert_closed(&before, &after);
    }

    #[test]
    fn a_lane_whose_own_sum_passes_i64_is_still_exact() {
        // `total_64` widens every element and adds in `i128`. The natural thing
        // to write instead — `i128::from(lane.iter().map(M64::to_i64).sum())`,
        // natural because `to_i64` hands you an `i64` — keeps the accumulator
        // `i128` and makes the partial sum the storage width, and it passes
        // every other test here and in `acceptance_ledger.rs`: they all feed
        // lanes whose own sum sits far below `i64`, the water test included,
        // because it tiles at 65 536 elements to avoid a 134 MB allocation.
        //
        // A lane is a whole field of one substance, so the real case is not
        // hypothetical: at 256 cubed one lane of water sums to 93% of
        // `i64::MAX` (ADR-059), and the grid after that one crosses it. The
        // boundary value is substituted rather than reached, exactly as
        // `channel_counters_do_not_overflow_at_1e7_ticks` substitutes 10^10
        // instead of running ten million ticks — sixteen million elements would
        // be that same 134 MB, to prove the same arithmetic. `M64::MAX` is the
        // ceiling of the type rather than of any substance, and the type is all
        // `total_64` can see: the per-substance ceiling is derived by a loader
        // it never meets (ADR-039, ADR-040).
        let mut sums = DomainSums::new(1).unwrap();
        sums.add_field_lane_64(0, &[M64::MAX; 4]);
        assert_eq!(sums.matter(0), 4 * i128::from(i64::MAX));
        assert!(i64::try_from(sums.matter(0)).is_err());

        // The narrow width cannot be brought to the same failure at all: it
        // would take 4.3e9 elements of `i32::MAX` to overflow an `i64` partial
        // sum. `total_32` is written the same way regardless — the property is
        // "widen, then add", not "widen when it would otherwise wrap", and a
        // reader who finds the two halves written differently will reasonably
        // conclude that the difference means something.
        let mut narrow = DomainSums::new(1).unwrap();
        narrow.add_field_lane_32(0, &[M32::MAX; 4]);
        assert_eq!(narrow.matter(0), 4 * i128::from(i32::MAX));
    }

    #[test]
    fn crediting_more_solar_than_the_counter_holds_is_refused_not_wrapped() {
        // `ACCEPTANCE.md`, and the configuration that will actually reach it is
        // not this one. The solar reduction of ADR-075 accumulates in `i128`
        // because the exact sum of the slice does not fit an `i64` at the
        // declared span of the enthalpy field, and the counter it pays into is
        // `i64` (ADR-059). At `k_E = 67` the counter tops out at
        // `2^63/2^67 = 62.5 mJ`, while the upper face of a 128 cubed domain at
        // `dx = 1e-4 m` takes `0.16384 J` of full sun in one second — **2.62144
        // ceilings in a single tick** (ADR-076). So the real configuration of
        // this test is the first lit tick of the first lit scenario, and it is
        // unreachable for exactly as long as ADR-076's refusal stands: no
        // scenario may declare `i_surface > 0` until energy has a sink *and*
        // A-20 has an answer.
        //
        // Substituted here rather than reached, the way
        // `channel_counters_do_not_overflow_at_1e7_ticks` substitutes instead of
        // running ten million ticks. What is on trial is the conversion: written
        // `as i64` this wraps to a plausible negative and the ledger goes on
        // closing against a right-hand side that is no longer the truth.
        //
        // **"Not wrapped" is the half a `#[should_panic]` cannot state.** A
        // conversion written `as i64` does not panic at all, so the absence of a
        // panic is one failure; a conversion that panicked *after* touching the
        // counter would be the other, and it is the one a test ending at the
        // panic cannot see. Hence the counter is read afterwards and has to be
        // exactly where it started.
        let mut ledger = Ledger::new(1).unwrap();
        let refusal = refusal_of(|| {
            ledger.credit_energy_wide(Channel::SolarIn, i128::from(i64::MAX) + 1);
        });
        let message = refusal.expect("one unit past i64::MAX was credited instead of refused");
        assert!(message.contains("A-20"), "the refusal says: {message}");
        assert_eq!(
            ledger.energy(Channel::SolarIn),
            0,
            "the counter moved on a credit that was refused"
        );
    }

    #[test]
    fn the_wide_credit_takes_the_boundary_and_refuses_past_it() {
        // The boundary itself, not its neighbourhood: `i64::MAX` is a legal
        // credit and `i64::MAX + 1` is not. A conversion written with `>` where
        // it wanted `>=`, or one that reserved a unit of headroom it was never
        // asked for, is caught by the pair and by nothing else.
        //
        // **Both sides here rather than one side each side of the file.** The
        // accepting half alone leaves the name a promise the body does not keep,
        // and a conversion that refused every credit would pass it; the refusing
        // half alone is the test above. Two tests that only mean something read
        // together are one test whose halves can be deleted separately.
        let mut ledger = Ledger::new(1).unwrap();
        ledger.credit_energy_wide(Channel::SolarIn, i128::from(i64::MAX));
        assert_eq!(ledger.energy(Channel::SolarIn), i64::MAX);

        let mut past = Ledger::new(1).unwrap();
        assert!(
            refusal_of(|| {
                past.credit_energy_wide(Channel::SolarIn, i128::from(i64::MAX) + 1);
            })
            .is_some(),
            "one unit past i64::MAX was accepted"
        );

        // And the sign travels: the same pair on the other end of the type.
        let mut ledger = Ledger::new(1).unwrap();
        ledger.credit_energy_wide(Channel::SolarIn, i128::from(i64::MIN));
        assert_eq!(ledger.energy(Channel::SolarIn), i64::MIN);

        let mut past = Ledger::new(1).unwrap();
        assert!(
            refusal_of(|| {
                past.credit_energy_wide(Channel::SolarIn, i128::from(i64::MIN) - 1);
            })
            .is_some(),
            "one unit past i64::MIN was accepted"
        );
    }

    #[test]
    fn the_wide_credit_names_the_channel_it_refused() {
        // The message has to carry the channel and the number as well as A-20:
        // a panic that says only "does not fit" sends the reader to look for the
        // overflow in six counters, and one that names only the question sends
        // him to a record instead of to the credit that broke.
        //
        // All three substrings in one assertion, which is why the panic is caught
        // rather than expected: `#[should_panic(expected = ...)]` takes one of
        // them, and the two it does not take are exactly the two that can be
        // deleted from the format string without any test noticing.
        let units = i128::from(i64::MIN) - 1;
        let mut ledger = Ledger::new(1).unwrap();
        let message = refusal_of(|| {
            ledger.credit_energy_wide(Channel::SolarIn, units);
        })
        .expect("a credit past i64::MIN was accepted");

        for expected in [Channel::SolarIn.name(), units.to_string().as_str(), "A-20"] {
            assert!(
                message.contains(expected),
                "the refusal has to name `{expected}`, and says: {message}"
            );
        }
    }
}
