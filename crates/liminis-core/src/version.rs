//! World semantics version (ADR-020).
//!
//! Bumped on any change that affects dynamics: a constant in a reaction, the
//! order of operations in a kernel, a formula. Refactoring that preserves
//! behaviour changes the code version and leaves this number alone.
//!
//! This constant is the only place the number lives. CI fails a pull request
//! that touches `configs/**`, `crates/liminis-core/src/kernels/**`,
//! `crates/liminis-core/src/process/**` or `crates/liminis-core/src/numeric/
//! rng.rs` without changing it — the order of processes is semantics too
//! (ADR-036) even when no kernel changed, and so is the mixer every draw comes
//! out of (ADR-058).

/// Version of the world semantics. A run is identified by
/// `(seed, config_hash, world_format_version)`.
///
/// Version 2 is the first one that simulates anything: diffusion of one
/// substance, in gather form, over the substeps ADR-030 derives from the
/// coefficient. Version 1 had no kernels at all, so no run of it is comparable
/// with a run of this one.
///
/// Version 3 is where the seed arrives. `rand` takes a fourth counter, the run
/// key derived from the seed on the host (ADR-058), and a fourth round of the
/// mixer folds it in. No formula and no kernel changed, and that is exactly why
/// the bump is easy to miss: what changed is every draw. Stochastic rounding of
/// extent is the only conversion from `Q` to an integer in the chemistry
/// (ADR-027), so a different draw is a different `xi` for every reaction of
/// every voxel of every tick. Nothing from version 2 can go on a plot beside
/// anything from version 3 — including runs of the same seed, because under
/// version 2 the seed reached nothing at all.
/// Version 4 is the scenario schema. The loader knew three keys — `name`, `dt`
/// and `[grid]` — and now knows the whole of `CONFIG_SCHEMA.md` sections 2 to 8:
/// substances, reactions, fields, processes, boundaries and the conserved
/// quantities. The bump is mechanically required, because `configs/**` moved
/// (ADR-020), and it is required on the merits too. `config_hash` is now taken
/// over an explicit projection (ADR-066), and the projection has eleven root
/// keys where the whole struct had three, so the canonical bytes of every
/// scenario in existence have changed. No run of version 3 is comparable with a
/// run of this one, and the reason is the identity rather than the dynamics:
/// nothing about a tick changed, because nothing yet reads the new keys.
///
/// Version 5 is a bound. `process/diffuse.rs` gained `N_MAX = 64` (ADR-061), so
/// there is now a boundary past which a field is refused instead of being
/// counted out: a scenario whose `D`, `dt` and `lod` ask for more than
/// sixty-four substeps used to load and quietly cost up to a gibibyte of traffic
/// per tick per field, and now does not load at all. The set of admissible
/// worlds shrank, which is what this number is for; every world that loaded
/// under version 4 *and* still loads runs bit for bit as it did.
///
/// Version 6 is the validator, and it is the same kind of bump as version 5, on
/// the same grounds and by the same precedent. `config/validate.rs` turns the
/// forty-odd refusals of `CONFIG_SCHEMA.md` section 10 plus referential
/// integrity, the domains of definition and grid divisibility from prose into
/// checks, where before them nothing at all was checked. The set of admissible
/// worlds shrank again; no world that loaded before *and* loads now changed by a
/// single bit, because a validator computes nothing a tick reads.
///
/// The caveat belongs in the comment rather than in a commit message: **CI does
/// not require this increment.** The guard of ADR-020 watches `configs/**`,
/// `crates/liminis-core/src/kernels/**`, `crates/liminis-core/src/process/**`
/// and `numeric/rng.rs`, and it does not watch `config/**` — `CONFIG_SCHEMA.md`
/// section 11 names that blind spot outright. So this number moved because the
/// author moved it, and the reason it should have moved is above.
///
/// Version 7 is light. `kernels/light.rs` turns the Beer-Lambert attenuation of
/// SPEC section 4.6 into a kernel, so this increment is the mechanical one CI
/// does enforce (ADR-020 watches `kernels/**`), and the honest thing to say
/// about this half of the number is the same thing the two halves below say:
/// **it changes no dynamics today**. Nothing calls `light_column`. There is no
/// light entry in `process/`, no light field in `world/`, and the fold of step
/// `i'` that ADR-049 makes the consumer of that field does not exist. Every run
/// of version 6 comes out under version 7 bit for bit as it did.
///
/// What the number records is that step `a` of the tick order now has an
/// operator, and that wiring it will not be neutral. From the moment a scenario
/// carries a light field, the fold of step `i'` has two sources of energy rather
/// than one (ADR-049), and the order of those two steps is itself world
/// semantics — light before the fold means the field belongs to the current
/// tick, light after it would lag by one. That ordering lives in `process/`,
/// which does not exist for light yet, and it will move this number again.
///
/// The kernel does settle one thing that a later increment would find expensive
/// to revisit, and it belongs here rather than in a commit message: a cell of
/// the light field holds what **leaves** the voxel through its bottom face, not
/// what enters through its top. Absorption is derived (ADR-049), so the field is
/// adequate only if the fold can form every difference, and under the mirror
/// convention the absorption of the floor voxel can be formed from nothing —
/// its lower term is the beam that left the domain, which is per-column data no
/// folded scalar can supply. Under this one the single term outside the field is
/// `i_surface`, which the fold receives the way the light kernel does. Flipping
/// it later would change every stored intensity by one factor of `exp(-tau)`.
///
/// One caveat that belongs to this number rather than to a commit message: the
/// kernel is the only place in the project where a stored field comes out of a
/// transcendental. `qexp` in `FLOAT` mode is `f32::exp`, IEEE-754 requires
/// nothing of it, and vendors differ in the last bits (ADR-022, NUMERIC.md
/// section 2). The fold turns `I[z] - I[z-1]` into whole joules, so one ulp can
/// flip a rounding and separate two platforms running the same seed. SPEC
/// section 9 asks only for repeatability from the seed, so this is not a
/// violation of anything — but it is the first place it can happen, and the
/// place to say so is here rather than after the first divergence.
///
/// Version 7 is also advection, and this half of the number needs its own
/// paragraph because the honest thing to say about it is that **it changes no
/// dynamics today**. `kernels/advect.rs` is the second transport kernel: a van
/// Leer limited flux in the canonical orientation of a face (ADR-054), split
/// component-wise so that one application is one axis with the whole step
/// (ADR-036). The increment is mechanically required — the guard of ADR-020
/// watches `kernels/**` — and it would be dishonest to present it as a change to
/// how a world evolves, because no scenario turns advection on: the process
/// defaults to `enabled = false` and there is no velocity field for it to read
/// (ADR-065, ADR-069). Every run of version 6 that loads under version 7 comes
/// out bit for bit as it did.
///
/// What the number does record is that the set of things a world can be asked to
/// do has grown, and that the growth is not neutral in the way version 5 and
/// version 6 were. Two decisions arrive with the kernel and will be visible the
/// first time a scenario switches advection on: transport is second order rather
/// than first, which removes the velocity-shaped numerical diffusion ADR-054 was
/// written about, and the order of the three axis applications is world
/// semantics that lives in `process/` and is not yet named anywhere (ADR-036).
/// The second of those will move this number again on its own.
///
/// Version 7 is also settling, and it shares the one increment of this wave
/// rather than asking for a second: the guard of ADR-020 compares a commit
/// against its base, and light, advection and settling arrive in one.
/// `kernels/settle.rs` gives step `f` of the tick order (SPEC section 8) its
/// first operator — **directed** transport along a single axis, Z, where every
/// kernel before it moved matter down a gradient or not at all. The velocity is
/// the Stokes one, `w = k*(rho_grain - rho_medium)*g/mu` with `k = 2*r^2/9`,
/// folded on the host into a single signed courant number (ADR-067); the kernel
/// knows neither the radius nor the densities nor the viscosity.
///
/// What this half of the number does and does not claim, plainly, because the
/// two halves differ and the difference is the point. A world of version 6 and a
/// world of version 7 are **incomparable only if the scenario switches settling
/// on**. No scenario does and none can yet: `settling_radius` is declared by no
/// substance, and `g` and the density of the medium are not keys of the schema
/// at all, so a settling substance is refused before the first tick. Every run
/// of version 6 that loads under version 7 therefore comes out bit for bit as it
/// did — the same statement version 5 and version 6 make, for the same reason.
///
/// The moment one does switch it on, the divergence is total rather than
/// marginal: settling moves whole units of matter in one direction every tick,
/// with no fixed point on a closed floor (ADR-067 prices that as "the sediment
/// does not stop itself"), and its place in the Lie-Trotter order is world
/// semantics in its own right (ADR-036). That ordering lives in `process/`,
/// which does not exist for settling yet, and it will move this number again.
///
/// Version 8 is the fold. `kernels/fold.rs` gives step `i'` of the tick order its
/// first operator: the energy of one tick collected from the fine grid onto the
/// coarse one, 128^3 into 32^3, out of the two sources ADR-045 and ADR-049 name —
/// the reaction increment written per fine voxel, and the light absorbed there,
/// derived on the fly as `I[z] - I[z-1]` out of the stored field rather than kept
/// in a second one. The increment is the mechanical one CI enforces (ADR-020
/// watches `kernels/**`), and the honest thing to say about it is what version 7
/// says about light, advection and settling: **it changes no dynamics today**.
/// Nothing calls `fold_energy`. There is no fold entry in `process/`, no enthalpy
/// field in `world/`, and no host that folds `joules_per_intensity`. Every run of
/// version 7 comes out under version 8 bit for bit as it did.
///
/// What the number records is that the last of the three steps ADR-049 ties
/// together now exists as code, and that wiring them will not be neutral. ADR-049
/// requires the light at step `a` and the fold at step `i'`, so that the field
/// the fold reads belongs to the current tick; the other order lags it by one and
/// is world semantics in its own right. That ordering lives in `process/`, which
/// exists for neither step, and it will move this number again.
///
/// Two decisions arrive with the kernel that a later increment would find
/// expensive to revisit, and they belong here rather than in a commit message.
///
/// The first is that the light is rounded into whole joules **once per coarse
/// cell**: the sixty-four absorption terms are summed in `Q` and cross into
/// storage units through a single `m_delta_64`. Rounding each fine voxel
/// separately is equally defensible and no record chooses between them, so this
/// is a choice made in code. It matters because of the second consumer: ADR-059
/// puts `SOLAR_IN` on this same step, and a counter that formed the same quantity
/// with its own rounding would disagree with this one in the last unit — on some
/// scenarios and not others, which is the worst way for two implementations of
/// one formula to differ.
///
/// The second is the order of the terms in the difference of intensities.
/// `light.rs` stores what **leaves** a voxel through its bottom face, so what a
/// voxel absorbed is `light[z+1] - light[z]`, and for the topmost voxel the upper
/// term is `i_surface` rather than the field. Both halves are easy to get wrong
/// quietly: the literal `I[z] - I[z-1]` of ADR-049 is written in levels, not
/// cells, and gives a one-voxel shift and the opposite sign, while mirroring the
/// top face — the reflex that closes a boundary in `diffuse.rs` — deletes a whole
/// layer of absorbed energy with the profile `I(z)` staying perfectly right.
/// Flipping either later would change every stored enthalpy of every run.
///
/// Version 8 is also pressure, and it shares the one increment of this wave
/// rather than asking for a second: the guard of ADR-020 compares a commit
/// against its base, and the fold and pressure arrive in one.
/// `kernels/pressure.rs` gives step `e` of the tick order its first operator —
/// the artificial compressibility of ADR-055, as **two** kernels rather than one.
/// `overflow_voxel` writes `V_occ/V_voxel - 1` per voxel out of every occupying
/// lane, by the formula ADR-067 left behind it: amounts times partial molar
/// volumes, with neither a molar mass nor a factor of `1e-3` in it.
/// `relax_voxel_32` and `relax_voxel_64` then move one lane across the six faces
/// of a voxel, in gather form, at a courant number that is the difference of that
/// field across the face.
///
/// The honest thing to say about this half of the number is the one the halves
/// above say: **it changes no dynamics today**. Nothing calls either relaxation,
/// there is no `process/pressure.rs`, no overflow field in `world/`, and
/// `[[process]] id = "pressure"` stands at `enabled = false` (CONFIG_SCHEMA
/// section 12). Every run of version 7 comes out under version 8 bit for bit as
/// it did.
///
/// Two things arrive with the kernel that belong here rather than in a commit
/// message, because both are expensive to revisit and neither is visible in a
/// result.
///
/// The first is that **the declared stiffness cancels**. ADR-055 derives the
/// mobility so that at the declared limiting overflow the displacement over a
/// tick is one voxel, which fixes the whole product `L*k*dt/dx^2` at
/// `1/theta_max` — so the number the kernel receives is `1/theta_max` and `k`
/// takes no part in the arithmetic at all. A wrong `k`, bars for pascals, changes
/// not one bit; the only declared pressure key is inert, and calibrating it
/// calibrates nothing. Worse, `theta_max` is declared nowhere in the corpus
/// (`TODO(theta-max)` in the kernel), so the folded number has no value yet.
/// ADR-055 closed the unit of `k` precisely against a quantity that calibration
/// hides forever, and a quantity that cancels is the same disease from the other
/// side.
///
/// The second is that the overflow field is stored **two-sided**: it holds the
/// signed `V_occ/V_voxel - 1`, not `max(0, .)` of it. SPEC section 3 reads
/// one-sided — "at `V_occ > V_voxel` a pressure arises" — and ADR-055 does not
/// distinguish, so this is a choice made in code out of the reading from which
/// either can be formed. It is not cosmetic the day a scenario switches pressure
/// on: a one-sided field is identically zero on the project's own example, where
/// four substances without a solvent give an occupancy of `4e-4` (ADR-067), and
/// an underfilled domain then draws nothing in, while negative partial molar
/// volumes make a negative occupancy legal in its own right.
///
/// And the ordering, which will move this number again on its own: step `e` is a
/// factor of the Lie-Trotter splitting whose place is world semantics (ADR-036),
/// and ADR-069 forbids adding its velocity to the prescribed field rather than
/// applying the two as separate steps — "pressure already spends the whole
/// Courant budget of its own step". That ordering lives in `process/`, which does
/// not exist for pressure.
///
/// Version 8 is also the reaction kernel, and it shares this wave's one increment
/// rather than asking for a second, by the precedent version 7 sets three times
/// over: the guard of ADR-020 compares a commit against its base, and the fold,
/// the pressure and the chemistry arrive in one.
///
/// This half of the number is **not** of the "changes no dynamics today" kind,
/// even though nothing calls `react_voxel` yet either. What arrives with
/// `kernels/react.rs` is the operator of step `h` of the tick order — and with it
/// the first source of divergence by seed anywhere in the chemistry. The extent
/// of every reaction of every voxel is rounded stochastically off
/// `rand(voxel_idx, tick, reaction_id, run_key)` (ADR-027, ADR-058), the only
/// conversion from `Q` to an integer in the whole of it, so from the moment a
/// scenario carries a reaction two seeds are two worlds, and one seed is one
/// world to the bit. Under version 7 there was nothing for a seed to change.
///
/// Three decisions arrive with the kernel that a later increment would find
/// expensive to revisit, and they belong here rather than in a commit message.
///
/// The first is that `reaction_id` is the third counter of that draw and comes
/// from the reaction's **name**, folded on the host (ADR-027). The function that
/// folds it does not exist yet — `config/derive.rs` checks only that names are
/// unique — and when it is written it is world semantics of the same standing as
/// `numeric/rng.rs`: change the mixer and every draw of every run changes with
/// it. The kernel takes the identifier as a column precisely so that the decision
/// has one home rather than two.
///
/// The second is the rounding of the scaled extent. The competition coefficient
/// multiplies `xi` and the product is rounded **down**, because any other rule
/// breaks the guarantee the coefficient exists for: the sum of the scaled demands
/// would exceed what the voxel holds by up to one unit per reaction, and the pool
/// would go negative with the balance closing exactly. NUMERIC.md section 3 knows
/// two rounding rules and assigns neither to this operation, so this is a choice
/// made in code, marked as such in `kernels/react.rs`, and it moves numbers the
/// day it is revisited.
///
/// The third is that the deltas of one voxel accumulate in an `i64` and are
/// narrowed back through `M::from_i64_clamping`, which this change makes public.
/// The substances of a voxel differ in width, so the accumulator can be neither
/// of them; the alternative inside the kernel is a bare `as i32`, and in release
/// that turns a pool which overflowed into one of the opposite sign, in silence.
pub const WORLD_FORMAT_VERSION: u32 = 8;
