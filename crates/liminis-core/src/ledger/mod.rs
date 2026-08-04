//! Channel counters, domain sums, and the two residuals (ADR-059).
//!
//! The right-hand side of the invariant of ADR-003, which SPEC section 2.1
//! writes as `delta(sum fields + sum cells) == sum(flows through named
//! channels)` and ADR-028 makes double: matter closes separately, energy closes
//! separately. The left-hand side already exists — it is the fields that are
//! written. Until this module there was no right-hand side in any form, and
//! "closed accounting" was an intention rather than a property.
//!
//! # Four things that are easy to confuse
//!
//! **A summed extent is not a counter.** `Xi_r` — [`Ledger::extent`] — is how far
//! reaction `r` ran over the whole domain this tick, and it is the second term on
//! the right of the matter identity (ADR-080). It is **not** a channel: the
//! registry below stays closed at six names, `Channel::ALL` does not grow, and
//! `a_closed_domain_leaves_every_channel_counter_at_zero` is true on a domain
//! where chemistry is running. A channel is a door in the boundary; a reaction is
//! not a door, and a seventh channel would let any residual be closed by
//! declaring one. It differs from a counter in lifetime too: a counter
//! accumulates over the run because its total is an observable, and `Xi` is
//! cleared by [`Ledger::begin_tick`] because it has no reader outside the tick.
//!
//! **A counter is not a residual.** [`Ledger::matter`] is the running net flow
//! through one `(channel, substance)` pair over the whole run, and it is `i128`
//! — **the width of the domain sum it is held against** (ADR-083,
//! `QUANTITIES.md` section 3). That is a relation and not a margin: a residual is
//! `left - right`, so for as long as the two sides had different widths there
//! was a place where the narrow one could wrap and the wide one could not, and
//! the residual still came out zero, because the wrap happened *before* the
//! subtraction. `a_channel_counter_is_as_wide_as_the_domain_sum_it_closes_against`
//! is the guard on exactly that.
//!
//! The margin is the other half, and it was not academic: an `i64` counter held
//! `2^63/2^k_E = 62.5 mJ` at `k_E = 67`, against `0.16384 J` — 2.62 ceilings —
//! for one lit tick of a 128 cubed domain, and the shipped scenario overflowed
//! it on the **seventh** tick through the lid alone, with no light and no
//! chemistry (ADR-083). At `i128` the same counter holds `2^127/2^k_E = 2^60 J`,
//! which is `7.04e11` declared horizons of full sun.
//!
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
//! The **sign** is no longer open either. ADR-084 ratifies it as the increment
//! of the domain — inbound positive, outbound negative — which is the convention
//! this module already implemented and the one under which SPEC section 2.1 and
//! the residual formula of ADR-059 both read as printed. The record cancels the
//! antisymmetry paragraph of ADR-059, where the counter took `-f`, and leaves the
//! rest of ADR-059 standing whole. Nothing here changed to make that true, which
//! is precisely why it is held by tests rather than by this paragraph:
//! `a_counter_is_signed_from_the_domains_point_of_view` below, in both directions
//! and on the stored number as well as on the residual, and
//! `boundary_outflow_appears_in_channel_counter` from outside — on a closed
//! domain either convention gives zero, so those are the only two places where
//! getting it backwards can fail at all.

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

/// The stoichiometry the extent term is multiplied by, borrowed and never
/// copied (ADR-080).
///
/// The four tables `process::React` already hands the kernel, in the same
/// layout: `nu` and `nu_sub` by **entry**, `begin` and `len` by **reaction**
/// (ADR-041). It is built from `React::rx()` and from nothing else, which is the
/// point rather than an optimisation — the coefficient the residual multiplies
/// `Xi_r` by has to be the coefficient the kernel applied, so it is one number
/// read twice and never a number formed a second time (ADR-075). The molar `s`
/// of the scenario is the wrong one and loudly so on most substances and
/// silently so on one: `nu_i = s_i * 2^(k_i - e_r)`, which is a factor of
/// `2^17` on the proton of the shipped scenario and exactly one on any substance
/// whose `k_i` happens to equal `e_r`.
#[derive(Clone, Copy, Debug)]
pub struct Nu<'a> {
    /// Storage coefficients, all reactions end to end, signed (ADR-039).
    pub nu: &'a [i32],
    /// Which substance `nu[j]` belongs to. The reserved index of enthalpy
    /// appears here like any other participant and is past the last substance
    /// (ADR-041), which is why the sum below never sees it.
    pub nu_sub: &'a [u32],
    /// Where reaction `r` starts in `nu`.
    pub begin: &'a [u32],
    /// How many entries reaction `r` has.
    pub len: &'a [u32],
}

impl Nu<'_> {
    /// No reactions at all — the vector of every process of the roster but step
    /// `h` (ADR-080).
    ///
    /// Legal only while every `Xi_r` of the tick is zero, and
    /// [`Ledger::residual_matter`] says so with an assertion rather than by
    /// treating the term as absent: an extent that was reduced and no
    /// stoichiometry to convert it with is a residual that silently forgets a
    /// whole reaction. The assertion is indexwise rather than on the length —
    /// every reaction this vector does not describe has to have moved nothing —
    /// so a tick without chemistry closes against `EMPTY` on a ledger that has
    /// room for reactions, and a tick with chemistry does not.
    pub const EMPTY: Nu<'static> = Nu {
        nu: &[],
        nu_sub: &[],
        begin: &[],
        len: &[],
    };
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

    /// Add the **chemical** energy of everything the domain holds:
    /// `Sum_s weight[s] * matter[s]` (ADR-081).
    ///
    /// `weight[s]` is `w_s = round(enthalpy_formation_s * 2^(k_E - k_s))`, the
    /// chemical energy of one storage unit of substance `s` in the storage units
    /// of the enthalpy field, derived once at load (`config/derive.rs`). No
    /// buffer is read: the amounts were already summed by the doors above, and
    /// this weighs the accumulators they filled.
    ///
    /// **The last door, and nothing here can enforce it.** It reads
    /// [`DomainSums::matter`], so called before any `add_field_lane_*` it weighs
    /// a half-filled table — and no residual can see that, because the same
    /// short sum taken before and after cancels in `after - before` and stays
    /// zero for ever. Calling it twice is the mirror of the same failure: on a
    /// closed domain the doubled term cancels the same way, and on a venting one
    /// it breaks loudly. The only witness is an *absolute* number, which is what
    /// `load_reports_the_chemical_energy_of_the_domain_in_joules` prints.
    ///
    /// **Why the weight is on the left and not a term on the right.** A reaction
    /// converts `Delta n_s = nu_s * xi` and moves the enthalpy field by
    /// `nu_E * xi`; with `nu_E := -Sum_s nu_s * w_s` the sum of the two is
    /// identically zero, so the energy half closes **by construction** rather
    /// than by check, and no channel for the heat of reaction has to be invented
    /// — a channel is a door out of the domain and a reaction is not a door
    /// (ADR-028, ADR-059).
    ///
    /// # Panics
    ///
    /// If `weight` is not one entry per substance. A short table would silently
    /// drop every substance past its end, and a long one is a table built for
    /// another registry — both of which give a residual that closes.
    #[track_caller]
    pub fn add_chemical_energy(&mut self, weight: &[i64]) {
        assert_eq!(
            weight.len(),
            self.matter.len(),
            "the weight table holds {} entries against {} substances (ADR-081)",
            weight.len(),
            self.matter.len()
        );
        for (s, &w) in weight.iter().enumerate() {
            // `i64 * i128` at the accumulator's width, and the margin is what
            // makes it safe rather than the type: the worst case the corpus can
            // reach is water of SPEC section 2.3 at 256 cubed — `5.12e11` units
            // per voxel at `w = -4 573 280` — which is `3.93e25`, 85 bits of 127.
            // That margin is guaranteed by the load-time refusal
            // `chemical_energy_of_the_domain_past_the_ledger_accumulator_is_rejected`
            // and by nothing here: Rust panics on an `i128` overflow in debug
            // only, and `liminis serve` computes these sums in **both** profiles.
            self.energy += i128::from(w) * self.matter[s];
        }
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
/// One signed `i128` per `(channel, substance)` pair and one per
/// `(channel, energy)` pair, plus a snapshot of both taken at the start of the
/// tick — the snapshots at the same width as the totals, because
/// [`Ledger::matter_this_tick`] and not [`Ledger::matter`] is the right-hand
/// side of every residual (ADR-083).
///
/// Ninety counters at the fourteen substances of SPEC section 2.3 — 1 440 bytes,
/// 2 880 with the snapshot; at `world::registry::MAX_SUBSTANCES`, which is
/// thirty-one because the thirty-second index belongs to enthalpy, 192 counters,
/// 3 072 bytes and 6 144 with the snapshot. The width cost 1 440 bytes of that
/// and 3 072 at the bound. Neither the table nor the snapshot is per voxel, so
/// the byte budget of ADR-045 does not move in any digit it prints.
///
/// # Signed, and which way
///
/// **A counter holds the increment of the domain** (ADR-084): positive when the
/// quantity entered, negative when it left. Under that convention the invariant
/// of ADR-003 reads literally — `delta(fields + cells) == sum(flows)` — and so
/// does the residual formula of ADR-059, with no sign correction anywhere.
///
/// ADR-059 stated both conventions in one record and picked neither: its
/// antisymmetry paragraph had the counter take `-f` where the voxel took `+f`,
/// its residual paragraph had `residual = delta(domain) - sum(counters) == 0`,
/// and the two differ by exactly a sign. ADR-084 cancels the first and ratifies
/// the second, so the antisymmetry of ADR-059 is left describing what it is
/// actually about — the *pairing* of the domain and the counter as two sides of
/// one exchange — and not the sign of the stored number.
///
/// The ratification cost zero lines here, and that is a fact about the code
/// rather than a saving: it had guessed right. The reverse convention would have
/// wanted a negation at four dispatched credit sites (matter and energy in each
/// of `process/diffuse.rs` and `process/advect.rs`) plus `process::credit_solar`,
/// and in the residual formula; a missed one shows up as a residual of **exactly
/// twice the flow**, which reads as broken transport rather than as a flipped
/// sign.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ledger {
    n_substances: u32,
    /// `channel * n_substances + substance`, running net over the whole run.
    matter: Vec<i128>,
    /// One per channel, running net over the whole run.
    energy: [i128; CHANNEL_COUNT],
    /// Both of the above as they stood at the last [`Ledger::begin_tick`].
    ///
    /// The same width as the totals, and narrowing them alone would compile:
    /// `matter` would read back correctly while `matter_this_tick` — the actual
    /// right-hand side of every residual — lied about everything past `i64`, with
    /// no panic anywhere and every small-number residual test still green
    /// (ADR-083).
    matter_at_tick_start: Vec<i128>,
    energy_at_tick_start: [i128; CHANNEL_COUNT],
    /// How many reactions the extent table below accounts for. Zero for a
    /// ledger built by [`Ledger::new`].
    n_reactions: u32,
    /// `Xi_r` for this tick: the domain-wide extent of every reaction, in quanta
    /// of `2^(-e_r)` of a turnover (ADR-080).
    ///
    /// **Per tick, and cleared by [`Ledger::begin_tick`] like nothing else here
    /// is.** A channel counter accumulates over the run because its running
    /// total is an observable — ADR-037 asks for it and ADR-071 publishes it —
    /// and `Xi` has no reader outside the tick at all: it is not in
    /// `/api/state`, not in the metric stream, and not a quantity of the world.
    /// So there is no snapshot beside it, and no question about its width: the
    /// ceiling of one tick at 128 cubed is `1.21e12` against `1.7e38`.
    extent: Vec<i128>,
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
        Ledger::with_reactions(n_substances, 0)
    }

    /// The same, with room for the extent of `n_reactions` reactions (ADR-080).
    ///
    /// The second door rather than a changed first one, and the reason is the
    /// direction the forgotten door fails in. [`Ledger::new`] leaves the extent
    /// table empty, so `Sum_r nu * Xi` is identically zero and the residual of a
    /// reacting tick comes out non-zero by the whole of what the chemistry
    /// moved — loud, on the first tick, naming the substances. A default that
    /// silently sized itself from somewhere would fail the other way.
    ///
    /// # Errors
    ///
    /// The errors of [`Ledger::new`], and if the extent table would be longer
    /// than a `usize` can address.
    pub fn with_reactions(n_substances: u32, n_reactions: u32) -> Result<Self> {
        if n_substances == 0 {
            bail!("a ledger over no substances closes against anything (ADR-003)");
        }
        let width =
            usize::try_from(n_substances).context("more substances than this host can index")?;
        let Some(len) = width.checked_mul(CHANNEL_COUNT) else {
            bail!("a counter table for {n_substances} substances is longer than a usize");
        };
        let reactions =
            usize::try_from(n_reactions).context("more reactions than this host can index")?;
        Ok(Self {
            n_substances,
            matter: vec![0; len],
            energy: [0; CHANNEL_COUNT],
            matter_at_tick_start: vec![0; len],
            energy_at_tick_start: [0; CHANNEL_COUNT],
            n_reactions,
            extent: vec![0; reactions],
        })
    }

    /// How many substances this counts.
    pub fn n_substances(&self) -> u32 {
        self.n_substances
    }

    /// How many reactions this accounts the extent of. Zero for a ledger built
    /// by [`Ledger::new`].
    pub fn n_reactions(&self) -> u32 {
        self.n_reactions
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
        // And the extent goes to zero rather than being snapshotted, because it
        // is the one quantity here that is per tick (ADR-080). A tick that does
        // not dispatch step `h` therefore has `Xi == 0` for every reaction by
        // construction, and the whole second term of the matter identity
        // vanishes — which is the same statement as "the roster's other eight
        // processes declare `Xi = 0`". Left standing, last tick's extent would be
        // credited a second time against a field that did not move; that is the
        // stale-accumulator failure of ADR-045 relocated from the kernel's slice
        // into this table, and `a_tick_without_chemistry_leaves_every_extent_total_at_zero`
        // is the guard.
        self.extent.fill(0);
    }

    /// Reduce the kernel's per-voxel extent report into `Xi_r` for this tick
    /// (ADR-080).
    ///
    /// `xi_out` is the slice `kernels::react::react_voxel` wrote: one block of
    /// `n_reactions` cells per voxel, voxel-major. This is the host side of the
    /// division of labour ADR-041 states in as many words — "the reaction kernel
    /// does not touch the channel counters ... the sums for the ledger are taken
    /// by reduction in the LEDGER phase" — and it runs in **phase 5**, not phase
    /// 4. The rule of `process/tick.rs` that every credit happens above the
    /// LEDGER phase is about **channels**, and `Xi` is not a channel.
    ///
    /// **Overwrites, and does not accumulate.** Called at most once a tick, after
    /// the one dispatch of the whole chemistry (ADR-050); an accumulating
    /// reduction would be a second place last tick's extent could survive, after
    /// the slice itself.
    ///
    /// # Panics
    ///
    /// If `xi_out` is shorter than `n_voxels * n_reactions`, or if this ledger
    /// was built by [`Ledger::new`] and has no room for an extent at all — the
    /// second because a reduction into nothing is silent and leaves the residual
    /// short by the whole of what the chemistry moved.
    #[track_caller]
    pub fn reduce_extent(&mut self, xi_out: &[M32], n_voxels: u32) {
        assert!(
            self.n_reactions > 0,
            "this ledger has no extent table and was handed the report of a \
             dispatch: build it with Ledger::with_reactions (ADR-080)"
        );
        let reactions = self.n_reactions as usize;
        let cells = (n_voxels as usize) * reactions;
        assert!(
            xi_out.len() >= cells,
            "the extent report holds {} cells against {n_voxels} voxels times {} \
             reactions",
            xi_out.len(),
            self.n_reactions
        );

        self.extent.fill(0);
        for idx in 0..n_voxels as usize {
            for r in 0..reactions {
                // Widened per element, for the reason `total_32` gives one screen
                // up: the accumulator is `i128` and so is every partial sum. At
                // 128 cubed the ceiling of one tick is `1.21e12` against
                // `1.7e38`, so the width is not what is being defended here — the
                // habit is. A bare `+=` rather than the `checked_add` the counters
                // use, and the asymmetry is deliberate: a counter accumulates over
                // the whole run and this is cleared every tick, so there is no
                // horizon to argue about — and the reduction runs only in the
                // debug build, where an overflow panics on its own.
                self.extent[r] += i128::from(xi_out[idx * reactions + r].to_i64());
            }
        }
    }

    /// The domain-wide extent of reaction `r` over this tick, in quanta of
    /// `2^(-e_r)` of a turnover (ADR-080).
    ///
    /// Zero on every tick that did not dispatch step `h`, and zero for a ledger
    /// built by [`Ledger::new`]. Not an observable and not published — see the
    /// field's own comment for why it has no snapshot.
    ///
    /// # Panics
    ///
    /// If `reaction` is outside the count this ledger was built for.
    #[track_caller]
    pub fn extent(&self, reaction: u32) -> i128 {
        let width = self.extent.len();
        let Some(&total) = usize::try_from(reaction)
            .ok()
            .and_then(|r| self.extent.get(r))
        else {
            panic!("reaction {reaction} is outside the {width} this ledger accounts for");
        };
        total
    }

    /// Credit `units` of `substance` to `channel`: **the increment of the
    /// domain**, positive when the matter entered and negative when it left
    /// (ADR-059, ADR-084).
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
    pub fn credit_matter(&mut self, channel: Channel, substance: u32, units: i128) {
        let slot = self.slot(channel, substance);
        let counter = self.matter[slot];
        let Some(sum) = counter.checked_add(units) else {
            panic!(
                "ledger counter overflowed: channel {}, substance {substance}, \
                 {counter} + {units}. Wrapping here would change the sign of a \
                 counter and leave the ledger closing against a right-hand side \
                 that is no longer the truth (ADR-059, ADR-084).",
                channel.name()
            );
        };
        self.matter[slot] = sum;
    }

    /// Credit `joules` to `channel`: the increment of the domain, positive when
    /// the energy entered and negative when it left (ADR-059, ADR-084). The rules
    /// of [`Ledger::credit_matter`] apply unchanged, and the outward direction is
    /// the one the shipped scenario runs — its lid carries enthalpy out through
    /// `BOUNDARY_EXCHANGE` on every substep.
    ///
    /// # Panics
    ///
    /// On overflow, in both build profiles.
    #[track_caller]
    pub fn credit_energy(&mut self, channel: Channel, joules: i128) {
        let counter = self.energy[channel.row()];
        let Some(sum) = counter.checked_add(joules) else {
            panic!(
                "ledger counter overflowed: channel {}, energy, {counter} + \
                 {joules}. Wrapping here would change the sign of a counter and \
                 leave the ledger closing against a right-hand side that is no \
                 longer the truth (ADR-059, ADR-084).",
                channel.name()
            );
        };
        self.energy[channel.row()] = sum;
    }

    /// The running net flow of `substance` through `channel` over the whole run.
    #[track_caller]
    pub fn matter(&self, channel: Channel, substance: u32) -> i128 {
        self.matter[self.slot(channel, substance)]
    }

    /// The running net energy flow through `channel` over the whole run.
    pub fn energy(&self, channel: Channel) -> i128 {
        self.energy[channel.row()]
    }

    /// What this tick has credited so far, measured from
    /// [`Ledger::begin_tick`].
    ///
    /// This, and not [`Ledger::matter`], is the right-hand side of the residual.
    /// The two have the same type and nearly the same name, and they differ from
    /// the second tick on.
    #[track_caller]
    pub fn matter_this_tick(&self, channel: Channel, substance: u32) -> i128 {
        let slot = self.slot(channel, substance);
        self.matter[slot] - self.matter_at_tick_start[slot]
    }

    /// The same for energy.
    pub fn energy_this_tick(&self, channel: Channel) -> i128 {
        self.energy[channel.row()] - self.energy_at_tick_start[channel.row()]
    }

    /// Everything credited for one substance this tick, over all six channels.
    ///
    /// **This sum stopped being unoverflowable by construction with ADR-083, and
    /// the change is worth naming rather than discovering.** While the counters
    /// were `i64` these six terms were widened into an `i128` accumulator, so no
    /// arrangement of six counters could overflow it; now six `i128` values are
    /// added at their own width, and `residual_matter` subtracts three of them
    /// with bare operators. At the declared quantities that is unreachable —
    /// after `10^7` ticks of full sun the widest counter stands 39 bits below the
    /// ceiling — but the failure class is precisely the one ADR-083 is about: a
    /// wrap happens *before* the subtraction, so the residual comes out zero at
    /// the moment the accounting is lost. The door used to be locked by the type
    /// and is now locked only by the magnitude.
    fn credited_matter(&self, substance: u32) -> i128 {
        Channel::ALL
            .into_iter()
            .map(|channel| self.matter_this_tick(channel, substance))
            .sum()
    }

    /// Everything credited in energy this tick, over all six channels. See
    /// [`Ledger::credited_matter`] for what the width of the terms costs.
    fn credited_energy(&self) -> i128 {
        Channel::ALL
            .into_iter()
            .map(|channel| self.energy_this_tick(channel))
            .sum()
    }

    /// `delta(domain) - sum(channels) - sum over reactions of nu * Xi` for one
    /// substance over this tick.
    ///
    /// Required to be an exact zero (ADR-003, ADR-059, ADR-080). Not "small":
    /// both sides are integers and the schemes conserve by construction rather
    /// than by accuracy, so anything but zero means matter appeared or vanished
    /// without a name.
    ///
    /// # The second term, and what it buys
    ///
    /// ADR-003 prints one term on the right, and every process of the roster but
    /// step `h` reports `Xi == 0`, so for them this is that equation unchanged.
    /// What the term adds is the thing a ledger is for: the residual now compares
    /// **two independent witnesses of one action** — the field, and the kernel's
    /// own report of how far each reaction ran — instead of one witness against
    /// itself. It catches a write that landed in the wrong lane, a lost write, a
    /// double application, a dispatch that missed part of the domain, and a
    /// saturation that was not symmetric.
    ///
    /// # The epistemic boundary, said here because the next reader will not
    /// otherwise find it
    ///
    /// **The extent is credited with the same `xi` that applied `Delta n = nu *
    /// xi`.** A `xi` that is wrongly computed, negative, or not bounded by the
    /// substrate is therefore invisible to this residual and always will be: one
    /// number enters both sides and cancels. Its standing is exactly that of a
    /// channel counter (ADR-059) — the ledger checks that the report and the
    /// field agree, never that either is right. Those classes are guarded by the
    /// kernel's own "alone" tests, `a_negative_pool_caps_the_extent_at_zero` and
    /// its neighbours, and by nothing in this module. A residual that closes on a
    /// reacting tick is **not** evidence that the reaction rate is right, and
    /// reading it as such is the mistake this paragraph exists to prevent: the
    /// ledger did not start policing chemistry.
    ///
    /// # Energy does not get a term like this
    ///
    /// Deliberately, and it is worth a sentence because the pull of symmetry is
    /// strong. ADR-081 closes the energy half by weighting the **left** side
    /// (`H_field + sum over s of w_s * n_s`), which makes the contribution of a
    /// reaction to that side identically zero; a second term on the right there
    /// would credit one conversion twice and break the residual by exactly its
    /// own size on the first reacting tick. See [`Ledger::residual_energy`].
    ///
    /// # Panics
    ///
    /// If `substance` is outside the count, or if either `DomainSums` accounts
    /// for a different number of substances than this ledger counts — the two
    /// are built from one registry and there is nothing else that says so — or if
    /// `nu` does not describe a reaction whose extent was reduced.
    #[track_caller]
    pub fn residual_matter(
        &self,
        nu: Nu<'_>,
        substance: u32,
        before: &DomainSums,
        after: &DomainSums,
    ) -> i128 {
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
        after.matter(substance)
            - before.matter(substance)
            - self.credited_matter(substance)
            - self.transmuted_matter(nu, substance)
    }

    /// `sum over reactions of nu_(r,s) * Xi_r` for one substance (ADR-080).
    ///
    /// The stoichiometry is borrowed from the process that applied it and never
    /// formed a second time: `nu_i = s_i * 2^(k_i - e_r)` in storage units, which
    /// is a factor of `2^17` away from the molar `s` on the proton of the shipped
    /// scenario and exactly equal to it on any substance whose `k_i` happens to
    /// be `e_r` — so a second derivation is green on the fixture that has one
    /// and wrong on the registry that does not.
    ///
    /// The enthalpy record sits in `nu` like any other participant and its
    /// reserved index is past the last substance (ADR-041), so this loop never
    /// matches it. That is a property of the index and not of a filter here: an
    /// `s_energy` inside the substance range would land the enthalpy coefficient
    /// on a substance, and `kernels/react.rs` refuses that in its own preamble.
    ///
    /// # Panics
    ///
    /// If any reaction whose extent was reduced is not described by `nu`. That
    /// is the direction that fails silently — the term would be short by a whole
    /// reaction and the residual would report the shortfall as missing matter,
    /// naming the substances and not the cause.
    #[track_caller]
    fn transmuted_matter(&self, nu: Nu<'_>, substance: u32) -> i128 {
        for (r, &total) in self.extent.iter().enumerate().skip(nu.begin.len()) {
            assert!(
                total == 0,
                "reaction {r} ran to an extent of {total} and the stoichiometry \
                 given to the residual describes {} reactions: the term would \
                 forget it whole (ADR-080)",
                nu.begin.len()
            );
        }

        let mut total = 0i128;
        for r in 0..nu.begin.len().min(self.extent.len()) {
            let extent = self.extent[r];
            if extent == 0 {
                continue;
            }
            for j in nu.begin[r]..nu.begin[r] + nu.len[r] {
                if nu.nu_sub[j as usize] == substance {
                    total += i128::from(nu.nu[j as usize]) * extent;
                }
            }
        }
        total
    }

    /// The same for energy — a separate number, because ADR-028 made the
    /// invariant double and one axis cannot express a process that conserves
    /// matter while moving energy.
    ///
    /// **No extent term here, and its absence is a decision rather than an
    /// omission** (ADR-080). ADR-081 closes this half by weighting the left side
    /// — `H_field + sum over s of w_s * n_s`, with `nu_E` defined as
    /// `-sum over s of nu_s * w_s` — which makes the contribution of a reaction
    /// to the left side identically zero. Adding `sum over reactions of nu_E *
    /// Xi_r` on the right on top of that would credit one conversion twice, and
    /// the residual would diverge by exactly that amount on the first tick with
    /// any chemistry in it. Conservation by construction beats conservation by
    /// checking (ADR-034), so the energy axis takes the weighted left side and
    /// this function keeps one term on the right.
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
    /// If any residual is non-zero. The message names the substance, both sides,
    /// the per-channel breakdown and the per-reaction one: the residual alone
    /// says that the tick did not close and nothing about which channel failed to
    /// be credited or which reaction moved what. Also if the sums and the ledger
    /// disagree about how many substances there are — see
    /// [`Ledger::residual_matter`], which is where that is caught, and which this
    /// always reaches because a ledger over zero substances is refused.
    #[track_caller]
    pub fn assert_closed(&self, nu: Nu<'_>, before: &DomainSums, after: &DomainSums) {
        for substance in 0..self.n_substances {
            let residual = self.residual_matter(nu, substance, before, after);
            assert!(
                residual == 0,
                "the matter ledger did not close: substance {substance} is off \
                 by {residual}. The domain went {} -> {} (delta {}), the \
                 channels credited {} this tick: {}. The reactions moved {}: {}",
                before.matter(substance),
                after.matter(substance),
                after.matter(substance) - before.matter(substance),
                self.credited_matter(substance),
                self.breakdown_matter(substance),
                self.transmuted_matter(nu, substance),
                self.breakdown_extent(nu, substance)
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

    /// One `r=nu*Xi` term per reaction that touched this substance, for the panic
    /// message.
    ///
    /// Only the reactions with a non-zero extent and only the ones whose vector
    /// names this substance: at `R_MAX` a full listing would be sixty-four terms
    /// of which the interesting one is somewhere in the middle, and a message
    /// nobody reads is a message that is not there.
    fn breakdown_extent(&self, nu: Nu<'_>, substance: u32) -> String {
        let mut terms = Vec::new();
        for r in 0..nu.begin.len().min(self.extent.len()) {
            let extent = self.extent[r];
            if extent == 0 {
                continue;
            }
            for j in nu.begin[r]..nu.begin[r] + nu.len[r] {
                if nu.nu_sub[j as usize] == substance {
                    terms.push(format!(
                        "reaction {r}: nu={} * Xi={extent} = {}",
                        nu.nu[j as usize],
                        i128::from(nu.nu[j as usize]) * extent
                    ));
                }
            }
        }
        if terms.is_empty() {
            "no reaction moved this substance".to_owned()
        } else {
            terms.join(", ")
        }
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
        let value = |channel: Channel, substance: u32| -> i128 {
            (channel as i128 + 1) * 1_000 + i128::from(substance) + 1
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
            let credited: i128 = Channel::ALL
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
        ledger.assert_closed(Nu::EMPTY, &before, &after);
    }

    /// One substance, one voxel, so that the arithmetic is the whole test.
    fn sums_of_one(amount: i32) -> DomainSums {
        let mut sums = DomainSums::new(1).unwrap();
        sums.add_field_lane_32(0, &[M32::new(amount)]);
        sums
    }

    #[test]
    fn a_counter_is_signed_from_the_domains_point_of_view() {
        // ADR-084 ratifies the convention ADR-059 stated twice and picked
        // neither time: a counter holds the **increment of the domain**. This is
        // the one place inside the crate where getting it backwards can fail at
        // all, because on a closed domain both sides of the residual are zero.
        //
        // Ratifying it cost zero lines of logic — the code had guessed right —
        // and that is exactly why the assertions below have to be this wide. A
        // comment declaring the convention stays green under either one; only a
        // test holds it.
        const K: i32 = 7_000;
        let before = sums_of_one(1_000);
        let after = sums_of_one(1_000 + K);

        // Inbound: the domain grew by K, and the counter says so.
        let mut ledger = Ledger::new(1).unwrap();
        ledger.begin_tick();
        ledger.credit_matter(Channel::BoundaryExchange, 0, i128::from(K));
        assert_eq!(ledger.residual_matter(Nu::EMPTY, 0, &before, &after), 0);

        // The other convention, and what it costs: not a small error, but twice
        // the flow, on every tick where anything crosses the boundary.
        let mut backwards = Ledger::new(1).unwrap();
        backwards.begin_tick();
        backwards.credit_matter(Channel::BoundaryExchange, 0, -i128::from(K));
        assert_eq!(
            backwards.residual_matter(Nu::EMPTY, 0, &before, &after),
            i128::from(2 * K)
        );

        // Energy reads the same way inbound.
        let mut before_e = DomainSums::new(1).unwrap();
        before_e.add_enthalpy_lane_32(&[M32::new(50)]);
        let mut after_e = DomainSums::new(1).unwrap();
        after_e.add_enthalpy_lane_32(&[M32::new(50 + K)]);

        let mut energy = Ledger::new(1).unwrap();
        energy.begin_tick();
        energy.credit_energy(Channel::SolarIn, i128::from(K));
        assert_eq!(energy.residual_energy(&before_e, &after_e), 0);

        // **Outbound**, which is the half the shipped scenario actually runs:
        // the lid of `configs/scenarios/h2s-oxidation.toml` carries enthalpy out
        // through `BOUNDARY_EXCHANGE` on every substep, so that counter is
        // negative all run long. A ledger that stored the magnitude of what it
        // was handed would pass the inbound half entire and fail here.
        let mut out_before = DomainSums::new(1).unwrap();
        out_before.add_enthalpy_lane_32(&[M32::new(50 + K)]);
        let mut out_after = DomainSums::new(1).unwrap();
        out_after.add_enthalpy_lane_32(&[M32::new(50)]);

        let mut leaving = Ledger::new(1).unwrap();
        leaving.begin_tick();
        leaving.credit_energy(Channel::BoundaryExchange, -i128::from(K));
        assert_eq!(leaving.residual_energy(&out_before, &out_after), 0);

        // And the sign of the **stored** number, not only of the residual.
        // Negating the credit and the residual formula together moves no
        // residual at all, so every assertion above stays green while
        // `/api/state` (ADR-071) and the metric stream (ADR-037) publish a
        // counter whose sign is the opposite of the one `QUANTITIES.md`
        // section 3 declares. These two assertions are what catches that
        // *inside* the crate; the other guard against it is
        // `boundary_outflow_appears_in_channel_counter`
        // (`tests/acceptance_boundary.rs`), and that one is the stronger of the
        // pair — it reads the counter a dispatched `DiffusePhase::apply_32` left
        // behind, so the sign it judges is one a call site chose rather than one
        // a test handed in. Two places, and the module doc above names both:
        // deleting either on the grounds that the other covers it leaves the
        // convention held by prose.
        assert!(
            leaving.energy(Channel::BoundaryExchange) < 0,
            "an outward channel holds a negative counter (ADR-084)"
        );
        assert_eq!(ledger.matter(Channel::BoundaryExchange, 0), i128::from(K));
    }

    #[test]
    fn the_residual_is_the_tick_delta_not_the_running_total() {
        let mut ledger = Ledger::new(1).unwrap();

        // Tick one: a hundred units in.
        ledger.begin_tick();
        ledger.credit_matter(Channel::Impact, 0, 100);
        assert_eq!(
            ledger.residual_matter(Nu::EMPTY, 0, &sums_of_one(0), &sums_of_one(100)),
            0
        );

        // Tick two: a hundred and fifty more. An implementation that measured
        // the residual against the running total instead of against the
        // snapshot is green on the first tick and wrong from here on — and on a
        // closed domain it is never wrong at all, which is how it survives.
        ledger.begin_tick();
        ledger.credit_matter(Channel::Impact, 0, 150);
        assert_eq!(
            ledger.residual_matter(Nu::EMPTY, 0, &sums_of_one(100), &sums_of_one(250)),
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
        ledger.credit_energy(Channel::SolarIn, i128::from(JOULES));
        assert_eq!(ledger.residual_matter(Nu::EMPTY, 0, &before, &after), 0);
        assert_eq!(ledger.residual_matter(Nu::EMPTY, 1, &before, &after), 0);
        assert_eq!(ledger.residual_energy(&before, &after), 0);
        ledger.assert_closed(Nu::EMPTY, &before, &after);

        // And the two halves are genuinely separate numbers: the same tick with
        // the energy credit missing closes on matter and not on energy. Folded
        // into one counter or one residual, this case is inexpressible.
        let mut half_blind = Ledger::new(2).unwrap();
        half_blind.begin_tick();
        assert_eq!(half_blind.residual_matter(Nu::EMPTY, 0, &before, &after), 0);
        assert_eq!(
            half_blind.residual_energy(&before, &after),
            i128::from(JOULES)
        );

        // The opposite shape — energy leaves while matter stays, which is what
        // the upkeep of an expression channel does (ADR-029), here through the
        // outward energy channel of SPEC section 7.
        let mut ledger = Ledger::new(2).unwrap();
        ledger.begin_tick();
        ledger.credit_energy(Channel::RadiativeOut, -i128::from(JOULES));
        assert_eq!(ledger.residual_matter(Nu::EMPTY, 0, &after, &before), 0);
        assert_eq!(ledger.residual_energy(&after, &before), 0);
        ledger.assert_closed(Nu::EMPTY, &after, &before);
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
        const N: i128 = 2 * IN_GUILD as i128 + 2 * IN_CELLS as i128;

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
        assert_eq!(after.matter(BIOMASS) - before.matter(BIOMASS), N);

        let mut ledger = Ledger::new(1).unwrap();
        ledger.begin_tick();
        ledger.credit_matter(Channel::Impact, BIOMASS, N);
        assert_eq!(
            ledger.residual_matter(Nu::EMPTY, BIOMASS, &before, &after),
            0
        );
        ledger.assert_closed(Nu::EMPTY, &before, &after);

        // The same tick with the left side cut down to `amount[]`: off by the
        // whole of what the guilds and the cells did, and in S0 there is nothing
        // else to notice it.
        let amount_only = || {
            let mut sums = DomainSums::new(1).unwrap();
            sums.add_field_lane_32(BIOMASS, &[M32::new(500), M32::new(500)]);
            sums
        };
        assert_eq!(
            ledger.residual_matter(Nu::EMPTY, BIOMASS, &amount_only(), &amount_only()),
            -N
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
        ledger.credit_matter(Channel::BoundaryExchange, 1, i128::MAX);
        ledger.credit_matter(Channel::BoundaryExchange, 1, 1);
    }

    #[test]
    #[should_panic(expected = "channel RADIATIVE_OUT, energy")]
    fn an_energy_counter_overflow_is_loud_too() {
        let mut ledger = Ledger::new(1).unwrap();
        ledger.credit_energy(Channel::RadiativeOut, i128::MIN);
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
        ledger.assert_closed(Nu::EMPTY, &before, &after);
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
        ledger.assert_closed(Nu::EMPTY, &before, &after);
    }

    /// The inside view of ADR-080, and the three things the acceptance names
    /// cannot reach from outside.
    ///
    /// `the_matter_residual_closes_across_a_tick_with_chemistry_in_it` and its two
    /// neighbours judge the term through a real kernel dispatch. What they cannot
    /// do is hand the ledger a reduction it has no vector for, or a table it has
    /// no room for: those are refusals of this module against a caller, and a
    /// caller that made them would be a host being written, not a scenario being
    /// run.
    #[test]
    fn the_extent_term_refuses_what_it_cannot_account_for() {
        // (1) A ledger with no room for an extent refuses the reduction outright
        //     rather than dropping it. Dropping it is the quiet failure: the
        //     residual then comes out short by the whole of what the chemistry
        //     moved, and the message names the substances rather than the
        //     forgotten reduction.
        let mut narrow = Ledger::new(1).unwrap();
        assert_eq!(
            narrow.n_reactions(),
            0,
            "the one-argument constructor made room for an extent nobody asked for"
        );
        let refusal = refusal_of(|| {
            narrow.reduce_extent(&[M32::new(3)], 1);
        })
        .expect("a reduction into a ledger with no extent table was accepted");
        assert!(
            refusal.contains("with_reactions"),
            "the refusal has to name the constructor, and says: {refusal}"
        );

        // (2) A report shorter than `n_voxels * n_reactions`. This is the shape
        //     mistake with no symptom: a slice of `n_voxels` cells is what every
        //     other per-voxel buffer looks like, it is long enough for the first
        //     voxel of a one-reaction scenario, and past that the blocks overlap.
        let mut ledger = Ledger::with_reactions(1, 2).unwrap();
        assert_eq!(ledger.n_reactions(), 2);
        assert!(
            refusal_of(|| {
                ledger.reduce_extent(&[M32::new(1); 3], 2);
            })
            .is_some(),
            "a report of three cells was accepted for two voxels of two reactions"
        );

        // (3) The reduction is the sum of the slice, per reaction and not per
        //     voxel. Voxel-major `idx * R + r`, so a transposed reader gets
        //     plausible numbers out of the same cells: here reaction 0 sums to 4
        //     and reaction 1 to 40, and transposed they would come out 13 and 31.
        let report = [M32::new(1), M32::new(10), M32::new(3), M32::new(30)];
        ledger.reduce_extent(&report, 2);
        assert_eq!(ledger.extent(0), 4);
        assert_eq!(ledger.extent(1), 40);

        // (4) And a residual handed no stoichiometry for a reaction that ran
        //     refuses instead of forgetting it. This is what makes `Nu::EMPTY`
        //     safe to pass from a tick that dispatches no chemistry: the day one
        //     does, the constant stops compiling into a green run.
        let before = DomainSums::new(1).unwrap();
        let after = DomainSums::new(1).unwrap();
        let refusal = refusal_of(|| {
            let _ = ledger.residual_matter(Nu::EMPTY, 0, &before, &after);
        })
        .expect("a reduced extent with no stoichiometry was accepted");
        assert!(
            refusal.contains("forget it whole"),
            "the refusal has to say what would be lost, and says: {refusal}"
        );

        // (5) The reduction overwrites. Called twice — which one dispatch of the
        //     whole chemistry never does (ADR-050), and which is exactly how a
        //     second dispatch would arrive — the totals are the last report and
        //     not the sum of two.
        ledger.reduce_extent(&report, 2);
        assert_eq!(ledger.extent(0), 4, "the reduction accumulated");
    }

    #[test]
    #[should_panic(expected = "reaction 2 is outside the 2")]
    fn a_reaction_outside_the_ledger_is_loud() {
        let ledger = Ledger::with_reactions(1, 2).unwrap();
        let _ = ledger.extent(2);
    }

    #[test]
    fn a_channel_counter_is_as_wide_as_the_domain_sum_it_closes_against() {
        // The guard on the decision of ADR-083 itself, and the thing it guards
        // is a *relation*: the residual is `left - right`, so while the two
        // sides have different widths there is a place where one of them can
        // wrap and the other cannot, and the residual stays zero because the
        // wrap happened **before** the subtraction. Asserted through the return
        // types of the two public doors rather than through the size of either
        // struct, because a struct's size says nothing about what a caller can
        // read out of it.
        let ledger = Ledger::new(1).unwrap();
        let sums = DomainSums::new(1).unwrap();

        assert_eq!(
            std::mem::size_of_val(&ledger.matter(Channel::SolarIn, 0)),
            std::mem::size_of_val(&sums.matter(0)),
            "the matter counter and the matter domain sum are different widths"
        );
        assert_eq!(
            std::mem::size_of_val(&ledger.energy(Channel::SolarIn)),
            std::mem::size_of_val(&sums.energy()),
            "the energy counter and the energy domain sum are different widths"
        );

        // And the width, absolutely. Without this the assertions above are
        // satisfied by narrowing *both* halves back to `i64` in one edit, which
        // is the one change that restores exactly the asymmetry ADR-083 removed
        // — silently, and with the equality above still true.
        assert_eq!(std::mem::size_of::<i128>(), 16);
        assert_eq!(
            std::mem::size_of_val(&ledger.matter(Channel::SolarIn, 0)),
            16
        );

        // **What this cannot see, and it is the dangerous half.**
        // `matter_at_tick_start` and `energy_at_tick_start` are private and have
        // no getter, so no width comparison reaches them. Widening the running
        // totals and leaving the snapshots `i64` compiles the moment a narrowing
        // is inserted into `begin_tick`, and then `matter_this_tick` — which is
        // the right-hand side of every residual, `matter` is not — lies about
        // everything past `i64` while `matter` reads back correctly. Every
        // existing residual test is built on small numbers and stays green. So
        // the snapshots are held by behaviour instead: credit past the old
        // range, take the snapshot, credit again, and demand the increment
        // exactly.
        const PAST: i128 = 1 << 100;
        let mut ledger = Ledger::new(1).unwrap();
        ledger.credit_matter(Channel::BoundaryExchange, 0, PAST);
        ledger.credit_energy(Channel::BoundaryExchange, -PAST);
        ledger.begin_tick();
        ledger.credit_matter(Channel::BoundaryExchange, 0, 7);
        ledger.credit_energy(Channel::BoundaryExchange, -9);
        assert_eq!(ledger.matter_this_tick(Channel::BoundaryExchange, 0), 7);
        assert_eq!(ledger.energy_this_tick(Channel::BoundaryExchange), -9);
        assert_eq!(ledger.matter(Channel::BoundaryExchange, 0), PAST + 7);
        assert_eq!(ledger.energy(Channel::BoundaryExchange), -PAST - 9);
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
    fn an_energy_credit_past_the_i64_range_reaches_the_counter_exactly() {
        // The claim is a **value**, not a signature: a runtime test cannot see a
        // parameter type, and a name promising that it could would be a lie about
        // what the test looks at. So the assertions are equalities, unit for unit.
        // `assert!(x > i64::MAX)` would be the weak form and a saturating counter
        // passes it — the equality is what a saturation cannot survive.
        // Through `From` and never through `as`, which is this module's own rule
        // for every counted number (see the header).
        let past = i128::from(i64::MAX) + 1;

        let mut ledger = Ledger::new(1).unwrap();
        ledger.credit_energy(Channel::SolarIn, past);
        assert_eq!(ledger.energy(Channel::SolarIn), past);

        // And it goes on adding rather than stopping at a boundary: a counter
        // that clamped anywhere in the old range agrees with the line above and
        // not with this one.
        ledger.credit_energy(Channel::SolarIn, past);
        assert_eq!(ledger.energy(Channel::SolarIn), 2 * past);

        // The real number rather than a boundary substituted for it: one lit tick
        // of a 128 cubed domain spread over the 32 768 coarse cells of the
        // enthalpy grid is `32 768 x 1.97e18 = 6.46e22` at the declared span of
        // the field (ADR-062, ADR-075) — the exact fixture that used to carry
        // `#[should_panic]` in `process/react.rs`, because the second door
        // refused it. There is one door now and it takes the whole number.
        const SPAN: i128 = 1_970_000_000_000_000_000;
        const CELLS: i128 = 32_768;
        let mut solar = Ledger::new(1).unwrap();
        solar.credit_energy(Channel::SolarIn, SPAN * CELLS);
        assert_eq!(
            solar.energy(Channel::SolarIn),
            64_552_960_000_000_000_000_000
        );
        assert!(i64::try_from(solar.energy(Channel::SolarIn)).is_err());

        // Matter reads the same way. Two tables (ADR-028), two chances to get it
        // wrong, and getting one right says nothing about the other.
        let mut matter = Ledger::new(1).unwrap();
        matter.credit_matter(Channel::BoundaryExchange, 0, -past);
        assert_eq!(matter.matter(Channel::BoundaryExchange, 0), -past);
    }

    #[test]
    fn a_credit_past_the_i128_counter_still_panics_rather_than_wrapping() {
        // The heir of the refusal ADR-075 asked for: `checked_add` stays, the
        // panic stays, only the boundary moved. Three claims, of three different
        // shapes, which is why the panic is caught rather than expected —
        // `#[should_panic(expected = ...)]` takes one substring and ends the test
        // at the panic, and the third claim is about the state *after* it.
        //
        // (1) the boundary itself, not its neighbourhood: `i128::MAX` is a legal
        //     counter value and one past it is not. A check written `>` where it
        //     wanted `>=` misses by exactly one and by nothing else;
        // (2) the message names the channel and both numbers: a panic saying only
        //     "overflow" sends the reader to look through six counters;
        // (3) the counter stands where it was and has not wrapped into a
        //     plausible negative. This is the whole difference between a refusal
        //     and a wrap, and a test that ends at the panic cannot look at it.
        let mut ledger = Ledger::new(1).unwrap();
        ledger.credit_energy(Channel::SolarIn, i128::MAX);
        assert_eq!(ledger.energy(Channel::SolarIn), i128::MAX);

        // Seven and not one, so that the increment is a substring worth looking
        // for: every large counter contains a "1" somewhere, and an assertion
        // that cannot fail is an assertion that is not there.
        let message = refusal_of(|| {
            ledger.credit_energy(Channel::SolarIn, 7);
        })
        .expect("a credit past i128::MAX was accepted");
        for expected in [
            Channel::SolarIn.name(),
            i128::MAX.to_string().as_str(),
            "+ 7",
        ] {
            assert!(
                message.contains(expected),
                "the refusal has to name `{expected}`, and says: {message}"
            );
        }
        assert_eq!(
            ledger.energy(Channel::SolarIn),
            i128::MAX,
            "the counter moved on a credit that was refused"
        );

        // Symmetrically at the other end of the type: a counter holds the net and
        // an outward channel runs negative all run long (ADR-059).
        let mut ledger = Ledger::new(1).unwrap();
        ledger.credit_energy(Channel::RadiativeOut, i128::MIN);
        assert!(
            refusal_of(|| {
                ledger.credit_energy(Channel::RadiativeOut, -1);
            })
            .is_some(),
            "one unit past i128::MIN was accepted"
        );
        assert_eq!(ledger.energy(Channel::RadiativeOut), i128::MIN);

        // And the matter half, which is a separate table and a separate function
        // (ADR-028): both of them can be got wrong on their own.
        let mut ledger = Ledger::new(1).unwrap();
        ledger.credit_matter(Channel::BoundaryExchange, 0, i128::MAX);
        assert_eq!(ledger.matter(Channel::BoundaryExchange, 0), i128::MAX);
        let message = refusal_of(|| {
            ledger.credit_matter(Channel::BoundaryExchange, 0, 1);
        })
        .expect("one unit past i128::MAX was accepted by the matter half");
        assert!(
            message.contains(Channel::BoundaryExchange.name()),
            "the refusal has to name the channel, and says: {message}"
        );
        assert_eq!(ledger.matter(Channel::BoundaryExchange, 0), i128::MAX);

        let mut ledger = Ledger::new(1).unwrap();
        ledger.credit_matter(Channel::BoundaryExchange, 0, i128::MIN);
        assert!(
            refusal_of(|| {
                ledger.credit_matter(Channel::BoundaryExchange, 0, -1);
            })
            .is_some(),
            "one unit past i128::MIN was accepted by the matter half"
        );
        assert_eq!(ledger.matter(Channel::BoundaryExchange, 0), i128::MIN);
    }
}
