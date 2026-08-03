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
///
/// Version 9 is the process boundary, and it is the increment that changes
/// **nothing**. ADR-057 says so itself, in as many words: "the edit to
/// `crates/liminis-core/src/process/**` requires an increment of
/// `WORLD_FORMAT_VERSION` (ADR-020), and it will have to move even though the
/// world does not change by a unit: the result agrees bit for bit with today's
/// single-lane arrangement. The guard looks at the path, not at the semantics,
/// and this is the case where it fires for nothing. Saying so outright is more
/// honest than going round it."
///
/// So: `process/diffuse.rs` moved, the guard of ADR-020 fires, and every run of
/// version 8 comes out under version 9 bit for bit as it did. That claim is not
/// a promise here — it is `a_substep_run_alternates_direction_instead_of_swapping`,
/// which runs the previous implementation (a swap after every substep) beside
/// the new one (alternating direction, no swap at all) and demands equality, and
/// it is the whole acceptance suite of diffusion and of the ledger, which is
/// unchanged and green.
///
/// What did change is who owes what at the end of a phase. A substep no longer
/// touches the field's pointers, because the exchange is one per field and lanes
/// take different numbers of substeps from the same tick (six for the proton,
/// two for oxygen, one for the rest of the registry). A phase therefore ends
/// with the lanes split by parity, and it repairs that itself, once, by swapping
/// the field and copying the **smaller** of the two parity groups — not the odd
/// one, which on the corpus registry costs four times as much because the single
/// 64-bit lane stands on the odd side (100.7 MB a tick against 25.2 at 128^3).
/// From outside the process nothing of this is visible, and that is the point:
/// the front buffer holds state `N` for every lane, so the six readers of a
/// coherent snapshot — reactions, pressure, settling, phase change, `ledger/`,
/// `observe/` — never learn that parity exists.
///
/// One thing arrives with it that will be visible the first time a scenario
/// carries a substance that does not diffuse: a lane at `D = 0` is no longer
/// dispatched at all. It used to take one substep at `alpha = 0`, which is a
/// copy under another name plus six flux computations per voxel; now it takes
/// zero substeps, which is an even number, and lands in the even group. No value
/// moves by a unit either way — ADR-057 calls this a by-product — and the reason
/// it is worth a paragraph is the failure next door: a lane that stopped being
/// dispatched while still being counted as `n = 1` would be restored as an odd
/// one, and would come back holding the write buffer's contents, which is the
/// previous phase's state and on the first tick zeroes. Zero is a legal amount
/// and no invariant would notice.
///
/// Version 9 is also the `World` aggregate, and this half of the number claims
/// less than it looks like it should. `world/world.rs` allocates the buffers of a
/// run — both amount fields, the coarse enthalpy field, the single-buffered
/// reaction energy accumulator, light, and the prescribed velocity field with its
/// potential — and adds the mapping from a fine voxel to the coarse cell covering
/// it. It is under no guard (`world/**` is not watched by ADR-020) and it runs no
/// tick: nothing constructs a `World` outside tests, because the loader that
/// would derive the widths, the scales and the substep counts does not build one
/// yet.
///
/// Three decisions arrive with it that a later increment would find expensive to
/// revisit, and they belong here rather than in a commit message.
///
/// The first is that the coarse grid **inherits the boundary conditions of the
/// fine one**. No record declares it. `Grid::new` wants six faces and
/// `[Boundary::Periodic; 6]` is the shortest thing to write; under it heat leaves
/// through a closed floor and comes back through the lid, the energy invariant
/// closes exactly — periodic transport conserves no worse than closed — and the
/// temperature field looks entirely plausible.
///
/// The second is that the reaction energy accumulator is `i64` and single
/// buffered. Both halves are already decided (ADR-062 for the width, ADR-045 for
/// the buffering) and both are contradicted by three places in the corpus that
/// still print `i32`, and by the symmetry argument for a second buffer. The width
/// matters because `M32::from_i64_clamping` asserts in debug and **saturates
/// silently in release**; the single buffer matters because the reaction kernel
/// overwrites its cell, so there is no clearing pass to forget, and a second
/// buffer would make a forgotten one invisible.
///
/// The third is that the two coarse grids are named by **role** —
/// `enthalpy_cell_of` and `velocity_cell_of` — and that no `coarse_cell_of(idx,
/// lod)` exists. They are different grids (32^3 and 64^3 against a 128^3 base),
/// the reaction kernel reads the temperature of the covering *enthalpy* cell, and
/// a `cnx`/`cny` taken from the velocity grid gives it a plausible, neighbouring,
/// wrong cell. Temperature is class `Q`, so that error appears in no invariant at
/// all.
///
/// Version 10 is the two transport processes of steps `c` and `f`, and it is the
/// second increment in a row that changes **no bit of any run that exists**.
/// `process/advect.rs` and `process/settle.rs` stopped being placeholders, the
/// guard of ADR-020 looks at the path rather than at the semantics, and neither
/// process is reachable from a scenario: advection has no velocity field to
/// advect with (`process/velocity.rs` is still a placeholder, and ADR-069 leaves
/// the interpolation of `u` onto a fine face unwritten), and a settling
/// substance is refused outright by `config/validate.rs`, because `g` is
/// assigned no value by any document and the density of the medium has no ASCII
/// name. `configs/**` is untouched: the keys these processes read —
/// `settling_radius`, `partial_molar_volume`, `mu`, `u_conv_max` — are all
/// already in the schema.
///
/// Four decisions arrive with them that a later increment would find expensive
/// to revisit.
///
/// The first is `AXIS_ORDER = [0, 1, 2]` in `process/advect.rs`. ADR-036 makes
/// the order of *operators* world semantics and settles that advection is split
/// component-wise, but **no document says in which order the three axes run**,
/// or whether the order alternates between ticks. Lie-Trotter splitting does not
/// commute, so this array moves the result of every run that advects. It is a
/// named constant with a test on it rather than a literal in a loop, so that
/// changing it is a visible edit rather than a silent shift.
///
/// The second is that both Courant conditions are compared on the `Q` the kernel
/// receives and not on the `f64` it was folded from, as ADR-068 requires. At a
/// bound of one — exactly representable in both — this is not the stricter of
/// the two comparisons but the *looser* one, and that is the point: the grain
/// sitting exactly on the radius limit ADR-067 derives, `5.27 um`, has an `f64`
/// Courant of `1.0000000000000002` and reaches the kernel as exactly one. An
/// `f64` comparison would make the record's own limit unreachable, and would
/// stop being merely conservative the day `Q` becomes `FIXED`.
///
/// The third is that a settling lane is dispatched on `w != 0` and not on
/// `settling_radius > 0`. The two differ for a grain that is neutrally buoyant,
/// which is physics rather than a refactor, and if the dispatch and the parity
/// count ever branch on different predicates the lane lands in the odd group and
/// gets the write buffer promoted into its front — the previous phase's state,
/// and zeroes on the first tick, which is a legal amount no invariant would
/// notice (ADR-057).
///
/// The fourth is that `Advect::fold_courant` checks **both** inequalities of
/// SPEC section 4.2 and not only the first. The second — the sum over the faces
/// matter leaves a voxel through — is about non-negativity rather than about
/// stability, and a field passes the first and fails the second whenever a voxel
/// loses matter through both of its faces on one axis at once. The sum is taken
/// over the two faces of the axis being swept and not over all six, which is the
/// reading ADR-067 already gives settling: under Lie-Trotter splitting an
/// operator moves matter only through the faces it sweeps (ADR-036), so the sum
/// degenerates to the terms of its own axis. This does not move
/// `outflow_bound_violation_is_rejected` out of `config/validate.rs` and does not
/// change what loads: the validator bounds the *declared* `u_conv_max` over six
/// faces, six times stricter than the two here, so nothing that loads reaches the
/// refusal. It exists because the kernel's positivity rests on the condition by
/// name and the fold is the only place in the project that sees the velocity a
/// face actually has — including the one step (b) of ADR-069 will interpolate
/// onto it, which a bound on the declared maximum cannot vouch for.
///
/// Version 10 is also steps `a`, `b` and `e`, which share this one increment
/// rather than asking for a fourth: the guard of ADR-020 compares a commit
/// against its base, and light, the prescribed velocity field and pressure arrive
/// in the same wave as the two transport processes above. Three things arrive
/// with them, and all three are orderings or absences rather than formulas —
/// which is exactly the kind of change a commit message loses.
///
/// **Steps `a`, `b` and `e` have an orchestration for the first time.**
/// `process/light.rs` folds the four Beer-Lambert coefficients of SPEC section
/// 4.6 with `dz` and dispatches `light_column` over the `nx*ny` **columns**;
/// `process/velocity.rs` derives `L`, `r`, the octave count and the estimate of
/// `|u|` at load and runs the four dispatches of ADR-069; `process/pressure.rs`
/// takes one overflow field from snapshot `N` and relaxes every lane of both
/// widths against it. Two of the three are still unreachable from a scenario —
/// nothing builds a `World` from a config, and the velocity field defaults to
/// `enabled = false` (ADR-065, ADR-069) — so every run of version 9 that loads
/// comes out bit for bit as it did. What changed is what a world can be asked to
/// do, and the remark version 7 made about advection applies to all three.
///
/// **Light stands before the fold of step `i'`, and the order is the decision.**
/// ADR-049 makes absorption a derived quantity: the fold computes
/// `light[z+1] - light[z]` out of the stored field, so the field has to belong to
/// the current tick. Put the light after the fold and the field lags by exactly
/// one tick while the energy ledger closes exactly — the counter and the enthalpy
/// move together either way — and the only symptom is a transient nobody is
/// plotting yet. The same paragraph explains why `process/light.rs` credits
/// nothing at all: a `SOLAR_IN` credit there would be the same joules counted
/// twice, and the residual would stay at zero while it happened.
///
/// **The velocity of step `e` is never added to the prescribed field of step
/// `b`.** ADR-069 forbids the sum on three independent grounds, of which the
/// first is arithmetic: ADR-055 derives the pressure mobility so that step `e`
/// already spends the whole Courant budget of its own step, and a sum of two
/// fields that each satisfy their own condition satisfies neither. The second is
/// that the pressure flux is divergent by construction — that is its purpose — so
/// the sum would take from the prescribed field the one property it is built as a
/// curl to have, while `prescribed_velocity_is_divergence_free_bit_for_bit` went
/// on being green, because it looks at the buffer `u` and not at a sum. The
/// displacement over a tick is therefore three separate terms and not one budget.
///
/// One thing arrives with the velocity field that a later increment would find
/// expensive to revisit, and it is not in ADR-069. The estimate `||D||_1` is
/// convolved from the three stencils the kernels **implement** — the wide
/// difference, the trilinear interpolation of the potential, the narrow curl —
/// and not from the closed form of the record, which is written for a composition
/// without an interpolation stage. Taken from the formula, `L` comes out wrong,
/// the field passes the ceiling `dx/(6*dt)`, and nothing reports it: the
/// validator checked `u_conv_max` rather than the real maximum, the debug
/// assertion is required never to fire and is absent in release, and the amounts
/// go negative and read as the accepted undershoot of ADR-068.
///
/// Version 10 is also step `h`, the chemistry, which shares this wave's one
/// increment by the same precedent as everything above it: the guard of ADR-020
/// compares a commit against its base, and `process/react.rs` arrives in the same
/// commit as the five other processes. `config/**`, `kernels/**` and `world/**`
/// are untouched by it — the tables it hands the kernel are `config/derive.rs`
/// flattened, not derived a second time (ADR-039, ADR-040).
///
/// Three things arrive with it that a later increment would find expensive to
/// revisit, and none of them is a formula.
///
/// **The whole of the chemistry is one step and one call.** ADR-050 merges the
/// abiotic reactions of step `h`, the microbial ones of step `i` and the cellular
/// chemistry of the APPLY phase into a single application, and `React::apply` is
/// that application: one loop over the voxels, one `react_voxel` each, every
/// reaction of the scenario inside it. Restoring the three steps is not a
/// refactor. It breaks two things and neither of them fails: the competition
/// coefficient of SPEC section 5 stops being shared, so whichever group ran first
/// takes its substrate at full demand — and the *matter* ledger closes exactly,
/// because conservation is a property of `nu` and not of the extent (ADR-027) —
/// while the second dispatch **erases** the energy increment of the first, since
/// the kernel writes its cell of the accumulator rather than adding to it. The
/// energy residual is the only thing that would see the second, and nothing
/// computes it today. `all_chemistry_is_applied_by_one_call` therefore fixes the
/// discrepancy between the two arrangements as *expected*, so that going back is
/// a red test rather than a silent edit.
///
/// **The seed reaches the chemistry.** `React::params` puts the tick and
/// `run_key(seed)` into the two neighbouring `u32` of `ReactParams`, and from
/// there they are the second and fourth counters of every draw
/// `rand(voxel_idx, tick, reaction_id, run_key)` (ADR-058). Under version 9 the
/// key existed and reached no kernel, because no process called one; under this
/// one, two seeds on a scenario with a reaction are two worlds. The canonical
/// order of the counters is part of it and is invisible to the compiler: swapped,
/// the stream stays uniform and unbiased, passes every test in `numeric/rng.rs`,
/// and is incomparable with the reference run.
///
/// **The energy increment stays on the fine grid.** The step writes
/// `energy_delta` per fine voxel and does not touch the enthalpy field at all;
/// collecting it into `32^3` is step `i'` and a separate operator (ADR-045).
/// Folding it here "to be safe" would have step `i'` credit the same joules a
/// second time, and the energy residual would come out doubled exactly where
/// nobody computes it.
///
/// One thing this half of the number does **not** record is a change to any run
/// that exists. No scenario in the repository carries a reaction, nothing builds a
/// `World` from a config, and the process refuses three keys the kernel does not
/// implement — a non-empty `catalyst`, a `requires` window and a named
/// `energy_from`. So the set of admissible worlds shrank again, in the way version
/// 5 and version 6 shrank it, and every run of version 9 that loads comes out bit
/// for bit as it did.
///
/// # Version 11: the splitting order becomes executable, and the roster is closed
///
/// Two changes arrive together and one increment covers both, because both are
/// `process/**` and both are semantics.
///
/// **The Lie-Trotter order becomes a thing that runs.** `process/tick.rs` holds
/// `STEP_ORDER`, and ADR-036 makes it world semantics: swapping two of its nine
/// entries changes the result of every run that dispatches both, and it changes
/// nothing a test can see — both residuals close under any order, every "alone"
/// test and every kernel test stays green, and there is no golden run in the
/// repository to disagree. Under version 10 the order existed as prose in
/// `process/mod.rs`; under this one it is executed. Of the nine steps two are
/// dispatched — advection and diffusion — and the other seven are refused when a
/// roster enables them, each naming the number or the operator that is missing.
///
/// **The enthalpy is transported, and it is the half of the increment that moves
/// a number.** SPEC section 8 names the field on line `c` and on line `d` beside
/// the substances and calls it an ordinary diffusive field updated every tick;
/// under version 10 nothing transported it, because nothing ran a tick at all.
/// Both dispatched steps now run over three fields rather than two — the narrow
/// amounts, the wide amounts and the enthalpy on its own `32³` grid (ADR-062),
/// with its own `alpha` and its own six substeps, refolded from the two inputs
/// `config/derive.rs` derived the record from and checked against them. Leaving
/// it out is the version of this file that no test in the repository could have
/// caught: transport conserves, so both residuals stay at exactly zero for ever,
/// while heat neither conducts nor is carried by the flow in a system where
/// thermal diffusion is the *fastest* transport there is (ADR-028) and the
/// temperature derived from the field goes on looking plausible.
/// `the_enthalpy_field_is_transported_by_the_steps_that_name_it` and
/// `diffusing_matter_does_not_move_enthalpy` are what notice, and the second of
/// them is a criterion `ACCEPTANCE.md` has carried unwritten since ADR-062:
/// it needs a uniform enthalpy field, because a uniform field is the one state
/// the field's own diffusion cannot move, and until this increment there was
/// nothing that held both fields at once to write it against.
///
/// **The process roster is closed and materialised before the hash** (ADR-065).
/// `config::materialise` fills every scenario out to the nine records of
/// `process::ProcessId::ALL`, each with the `enabled` its own module declares,
/// sorted into the order of SPEC section 8. That moves the `config_hash` of
/// **every** scenario in existence without changing a single one of them, which is
/// the side effect ADR-065 requires to be named: the canonical form grows by nine
/// records nobody wrote. The over-caution is one-sided — runs are declared
/// incomparable more often than strictly necessary, and never comparable when
/// they are not — and the shift is guarded, since it arrives with this increment
/// rather than silently.
///
/// Three smaller things ride along, and each of them is a decision rather than a
/// refactor:
///
/// - **the ids of six processes and the default `enabled` of seven of them are
///   assigned here for the first time**, and by code rather than by a record.
///   `CONFIG_SCHEMA.md` section 13 item 23 keeps them open; every one carries a
///   `TODO` naming what has to be decided and where. They enter `config_hash`
///   through the canonical form, so the day the journal settles them the version
///   moves again;
/// - **`reactions_abiotic` is gone.** The name section 12 prints describes the
///   split of the chemistry that ADR-050 rejected, so the roster spells the step
///   `reactions`. A scenario carrying the old spelling no longer loads;
/// - **an unknown or duplicated process id is a load error**, at the moment the
///   roster is materialised. Under version 10 a scenario could call its transport
///   process anything and walk past the ban of ADR-030 on `every_n_ticks > 1` in
///   silence — `every_n_ticks_on_diffusive_field_is_rejected` asserted the hole
///   from the inside — and there is no longer another name to give it. Both
///   refusals are made twice, and the second time is the one that counts:
///   `Tick::new` refuses a repeated id and a diffusion running every other tick
///   over the roster array, which is the door a run goes through and which
///   nothing in the crate builds out of a `Config`.
///
/// What this number does **not** record is a change to the trajectory of any run
/// that existed: nothing built a `World` from a config under version 10 and
/// nothing advanced one, so there is no version-10 run for a version-11 run to
/// differ from. What changed is the identity every future run is compared under.
/// # Version 12: the initial state stops being zero
///
/// `worldgen/` fills the world from the run key: layered noise sets the
/// sediment/water boundary and one octave stack per substance sets what that
/// substance holds around its derived `amount_at_typical` (SPEC section 12.4,
/// ADR-021, ADR-058). Under version 11 a `World` came out of `World::new` zeroed
/// and nothing filled it, so the increment does not change the trajectory of any
/// run that existed — there was no run. What it changes is the identity every
/// future run is compared under, and it changes it completely: the initial state
/// determines every subsequent tick, so two builds that disagree here agree about
/// nothing afterwards.
///
/// Three things inside it are semantics in their own right, and each would move
/// this number on its own:
///
/// - **the `purpose` window.** Every draw is `rand(node, 0, purpose, run_key)`
///   with `purpose = WORLDGEN_BASE + WORLDGEN_SLOTS*octave + slot`; the base, the
///   stride and the slot of a substance all enter the stream, so moving any of
///   them gives a different world under the same seed and the same scenario;
/// - **the spectrum.** The octaves are weighted `2^-k` and normalised by the sum
///   of the weights. Nobody decided that — `TODO(worldgen-spectrum)` says so, and
///   points at its twin `TODO(noise-spectrum)` in `kernels/noise.rs` — and it
///   decides how much of the initial structure sits at the scale of a voxel and
///   how much at the scale of the domain;
/// - **the band.** How far an initial condition may wander from `typical_conc` is
///   declared by no key: there is no `[initial]` section and `CONFIG_SCHEMA.md`
///   section 13 leaves its form open. `TODO(worldgen-excursion)` carries the rule
///   that stands in for it — half of the smaller of the two headrooms — and the
///   day the section arrives, that rule is deleted and this number moves again.
///
/// **The ADR-020 guard moved with it.** `worldgen/**` was outside the pattern in
/// `.github/workflows/ci.yml`, which is the same blind spot ADR-058 closed for
/// `numeric/rng.rs` by widening the regex, and for the same reason: a file that
/// decides every run of the project was invisible to the check that exists to
/// notice exactly that. The pattern now reads `(kernels|process|worldgen)/`, so
/// the next edit under `worldgen/` fails a pull request that forgets this
/// constant instead of passing quietly.
/// # Version 13: the world has a temperature
///
/// `T = T_ref + H / sum(n_i * c_p_i)` (ADR-044) exists as an operator:
/// `kernels/temperature.rs` gathers the denominator from the `2^(3*lod)` fine
/// voxels of a coarse cell and divides, `process/temperature.rs` folds `c_p`,
/// the storage exponents and `k_E` into what it reads, and `world::World`
/// allocates the two coarse `Q` fields it writes. Before this version the
/// quantity `q10` reads existed in `ReactParams` and in no buffer anywhere.
///
/// **No run that existed changes by a single bit.** Step `h` still does not
/// dispatch — `Tick::new` refuses it, and the refusal now names the two arches of
/// the invariant that chemistry needs instead of the temperature it used to name
/// — so nothing calls the operator inside `Tick::advance`, and the two new
/// buffers are written by nobody and read by nobody. What moves is the identity
/// every future run is compared under, and three things inside it are semantics
/// in their own right:
///
/// - **the mapping from a voxel to its cell.** The shift is per axis (SPEC
///   section 1.5), so sixty-four fine voxels share one `T`, and which sixty-four
///   is what the chemistry of every one of them then runs at;
/// - **the moment of the recomputation.** ADR-044 allows one per tick and the
///   tick has two consumers on opposite sides of the transport: step `b` reads
///   `C_cell` before it, step `h` reads `T` after. ADR-079 puts it immediately
///   before `h`, which leaves step `b` with the previous tick's capacity — the
///   lag it already carries and already declared (ADR-069). Under the other
///   placement every reaction runs at a temperature four transport steps old, and
///   both halves of the ledger close exactly either way;
/// - **the answer at a non-positive denominator.** A composition that sums to
///   zero or below has no temperature to derive; ADR-079 settles that the cell
///   answers `T_ref` and divides nothing, in every build profile, and names what
///   that costs. Any other answer is a different world.
///
/// The two coarse fields also cost 131 kB apiece at the eco regime, and ADR-062
/// priced only the first of them: its "+393 kB on the coarse grids" becomes
/// +524 kB. That is a divergence from a record rather than from a frozen
/// document, and ADR-079 is where it is recorded.
///
/// # Version 14: the layer side becomes a key of the scenario
///
/// `[initial]` exists, with one key — a table naming the side of the
/// sediment/water boundary each substance is enriched on, `sediment` | `water` |
/// `uniform`, defaulting to `sediment` and materialised onto every substance
/// before the hash (ADR-077). Until now `worldgen/` put every substance on one
/// side because there was nowhere to declare another, and
/// `oxidation_front_forms_at_predicted_depth` had no initial condition capable
/// of producing it. `configs/scenarios/h2s-oxidation.toml` is the first file to
/// say otherwise, in one line — `O2 = "water"` — which is the whole of what the
/// section buys the scenario it was written for.
///
/// **This increment is one of identity, not of dynamics** — the kind versions 4
/// and 6 are, and not the kind version 3 is. The arithmetic of `amount_at` under
/// a declared `sediment` is untouched, so every world of version 13 comes out
/// under version 14 bit for bit as it did, and the default is what makes that
/// true: it was chosen for it, rather than turning out convenient. What moved is
/// the identity every run is compared under, because `Hashed` grew a twelfth
/// root key and the canonical form of every scenario now carries one line per
/// substance.
///
/// The claim that no world changed rests on the `Sediment` branch staying byte
/// for byte what it was, and there is no recorded world hash in the repository
/// to check it against. `the_written_default_and_the_omitted_section_give_one_world`
/// is the whole of the defence: reordering the terms, `<=` instead of `<` on the
/// relief, hoisting the division out of the branch — each changes every world
/// while this comment goes on saying the opposite.
///
/// Three things inside it are semantics in their own right:
///
/// - **the side is a property of the scenario and never of the seed.** A
///   per-substance sign taken from `run_key` is free and makes
///   `different_seed_gives_a_different_initial_state` pass for a reason it must
///   not: two runs of one scenario would differ by where the oxygen is (ADR-058,
///   ADR-077);
/// - **`uniform` drops the layer term and keeps the noise at full amplitude.**
///   `blended = noise`, not `noise/2`: halving it would make one key govern the
///   side and the width of the actual scatter at once. Nothing compares a width
///   against a declaration, so what holds it is a ratio of the two branches over
///   the floor plane of the domain, where the layer term is present and
///   constant: the accepted form stands at two to one against the layered run
///   and the halved one at one to one
///   (`a_substance_declared_uniform_has_no_layer_step`);
/// - **the band is now a ratified rule.** `excursion = min(typical, max -
///   typical)/2` was carried by a `TODO` and is a consequence of ADR-077, with
///   both bounds as exact integer identities. The arithmetic does not change —
///   the rule is the one that was already running — but the two checks that
///   guard it were tightened from a quarter of the headroom, which nobody had
///   decided, to the half the identity gives.
/// # Version 15: the lid of the world stopped being a wall
///
/// The `exchange` face of SPEC section 1.6 is built rather than refused
/// (ADR-059). `Grid::new` accepts it, `Grid::neighbour` answers with the ghost
/// cell, a field lane grows from `n_voxels` to `n_voxels + 1`, and what crosses
/// the face on steps `c` and `d` is credited to `BOUNDARY_EXCHANGE` on **every**
/// substep. `configs/scenarios/h2s-oxidation.toml` gets its `z_max = "exchange"`
/// and its `[boundary.reservoir]` back, so that scenario is a different world
/// outright: it now holds an oxycline instead of running down.
///
/// **This is an increment of dynamics and of identity at once**, and the three
/// pieces of it are separable:
///
/// - **the lid vents.** Any scenario with an exchanging face now moves matter and
///   enthalpy across it. Nothing conserved before it is conserved now — the
///   invariant is `ChangedThrough(BOUNDARY_EXCHANGE)` rather than `Conserved`,
///   which is the second arm of `process::Conservation` that this wave writes;
/// - **the coefficient follows the address and not the face.** Inside
///   `kernels/diffuse.rs` a face runs at `alpha_ex` when its neighbour *is* the
///   ghost, and at `alpha` otherwise. Keyed on the mask instead, every interior
///   face along the exchanging axis would run at the exchange rate and the domain
///   would gain matter in its middle;
/// - **the lane grew, so every flat address did.** `lane * n_voxels + idx` is
///   `lane * lane_len + idx` now, in `world::Field` and in the four kernels that
///   address an amount lane by hand — `react`, `light`, `pressure`,
///   `temperature`, each of which gained a `lane_len` beside its `n_voxels`. The
///   Courant buffer of step `c` grew the same way and for a sharper reason: the
///   upper face of the last voxel of an axis *is* the ghost's lower face, and a
///   buffer of `3*n_voxels` had no cell for it at all.
///
/// What did **not** change: a scenario with no `exchange` face. Every such world
/// comes out bit for bit as it did — the ghost element is allocated, never read
/// and never summed, `Field::lane` is narrowed to the voxels so no reduction can
/// swallow it, and `alpha_ex` multiplies a flux nobody gathers. That claim is
/// what the whole existing suite standing green over this wave is evidence for.
///
/// Version 16 is the wave of ADR-073, ADR-075 and ADR-076, and it is an increment
/// of **identity** rather than of dynamics — the one place that is worth stating
/// plainly, because all three records touch `kernels/**` or `process/**` and the
/// guard of ADR-020 looks at the path.
///
/// Three things changed, and none of them moves a bit of any world that runs
/// today:
///
/// - **the fold has a second output.** `fold_energy` writes the integer it added
///   to the enthalpy into a slice of one `M64` per coarse cell, and the host
///   reduces that slice into `SOLAR_IN` as the second half of step `i'`
///   (ADR-075). The identity the record asks for is syntactic — one `let`, two
///   readers — because forming the number again rounds a second time and the
///   rounding rule of `NUMERIC.md` section 3 is not additive. The fold is still
///   dispatched from nowhere, so no run gains a joule;
/// - **the light has five keys.** `i_surface` in W/m^2, and the two amplitude and
///   period pairs of the daily and seasonal modulation (ADR-076). The folded
///   multiplier `units_per_intensity = dx^2 * 2^k_E` is derived and is not a key.
///   Every scenario's `config_hash` moves, because the schema grew — and no
///   scenario's behaviour does, because a lit scenario does not load at all: it
///   was refused by two locks, an energy sink that exists nowhere and the width
///   of a channel counter, and version 18 below leaves the first of those
///   standing alone;
/// - **a `requires` window is a load error.** ADR-073 refuses a non-empty
///   `requires` in the validator instead of accepting a window no kernel can
///   satisfy. No scenario in `configs/` writes one, so the set of loadable worlds
///   shrinks by nothing that existed.
///
/// The field `FoldParams::joules_per_intensity` is `units_per_intensity` now, by
/// the decision of ADR-076 and not as tidying: it measures storage units per
/// (W/m^2) per second, and the old name taught the wrong unit in the one place
/// the unit is assigned.
///
/// # Version 17: the velocity field has all four of its buffers
///
/// `world::World` allocates the two that were missing — the coarse potential on
/// the enthalpy grid, where the wide difference of ADR-069 lands, and the stirred
/// copy of the interpolated potential, which is a fourth buffer rather than an
/// addition in place because `stir_potential` reads its source at every octave
/// (ADR-034). With `velocity` and `velocity_potential` beside them a world now
/// owns everything one dispatch of `VelocityField::apply` writes, and
/// `World::velocity_slices_mut` hands them out in that function's argument order.
///
/// **No run that existed changes by a single bit**, and the increment is of
/// identity rather than of dynamics: step `b` still does not dispatch, so the two
/// new buffers are written by nobody and read by nobody, and `Tick::new` still
/// refuses `velocity_field` — for a different reason, which is the second half of
/// this version. The guard of ADR-020 looks at the path and `process/**` was
/// touched, so the number would have to move even if nothing else had.
///
/// Two things inside it are semantics in their own right:
///
/// - **the coarse potential is sized by the enthalpy grid.** A world has two
///   coarse grids and they differ (`lod = 2` against `lod = 1`). Sized by the
///   velocity grid it is eight times too long at the eco layout, which panics —
///   and exactly the right length on a layout whose lods coincide, where the wide
///   difference is then taken at twice the radius, one octave off in the selected
///   wavelength, with the field staying smooth, divergence-free and inside its
///   speed bound;
/// - **`snapshot()` in `tests/acceptance_tick.rs` covers two more fields.** That
///   helper keeps its promise to hold every buffer of a world by hand, so a buffer
///   left out of it makes `a_disabled_process_leaves_every_buffer_bit_for_bit`
///   green about a world it cannot see. `snapshot_covers_every_buffer_the_world_owns`
///   is the check that the list stays complete.
///
/// The third thing inside it is a kernel, and it is the only part of this version
/// that could change a number: **`kernels/potential.rs` implements the answer of
/// ADR-079 for a non-positive `C_cell`.** The record words that answer about a
/// *cell* — "a cell with `C_cell <= 0` answers `T := T_ref`, divides nothing and
/// does not stop in any build profile" — and names this kernel as the second
/// consumer of the denominator beside `kernels/temperature.rs`, which had the
/// guard already. What the answer looks like there is `Q::ZERO`, because that
/// kernel computes the anomaly `T - T_ref` and no `T_ref` may enter it.
///
/// No run changes here either, and for a reason worth stating rather than
/// assuming: the kernel has no caller, step `b` does not dispatch. What the guard
/// removes is a CPU reference (ADR-015 keeps it forever) that stopped in `qdiv` in
/// debug and produced a non-finite `Q` in release, on an input the shader is
/// required to survive — a solvent-free voxel, which is a legal state of a legal
/// world at any tick (ADR-077) and not only while `C_cell` is identically zero.
///
/// What the refusal of step `b` says changed twice over, and both of the texts it
/// replaces had become false. It named `Scratch`, which holds none of the
/// potentials and was never going to; then it named the denominator, which
/// ADR-079 had already answered. What blocks step `b` today is one piece of code
/// and one decision. The code: the four keys of ADR-069 are `[[process]]` keys of
/// the `Config`, `Derived` has no velocity section, and `Tick::new` takes a
/// `&Derived` — so nothing outside `tests/` can build a `VelocityConfig`. The
/// decision: `TODO(courant-fold)`. The last stage of `VelocityField::apply` fills
/// `Scratch::face_courant`, so the tick that dispatches step `b` is the tick step
/// `c` starts moving matter on, while `Scratch::enthalpy_courant` stays zero for
/// want of a fine-to-coarse fold of a flux that no record writes.
///
/// # Version 18: a channel counter is `i128`
///
/// ADR-083. Both halves of the ledger's counter table, and the snapshots the
/// tick's increment is measured from, go from `i64` to `i128` — the width of the
/// domain sum they are held against — and `credit_matter`/`credit_energy` take
/// that width, so the second door `credit_energy_wide` and its checked narrowing
/// disappear.
///
/// **The world does not move by one unit.** Not a field, not a voxel, not a
/// counter's *value*: only the width of the box the value sits in. The guard of
/// ADR-020 fires because `process/**` was edited — `diffuse.rs` and `advect.rs`
/// widen four credit arguments, `react.rs` calls the surviving door — and it
/// fires with nothing behind it. This is the **third** idle firing, after ADR-057
/// and ADR-075, and the number moves anyway for the reason the rule has no
/// exceptions: the guard cannot tell a widening from a change of semantics, and
/// an author who decides that on its behalf once will decide it again.
///
/// What did change for real is a file format, and it has a number of its own:
/// `SNAPSHOT_FORMAT_VERSION` moves 1 -> 2, because the counter block on disk
/// grows from 720 to 1 440 bytes (`observe/snapshot.rs`).
///
/// The run this fixes was not hypothetical. `configs/scenarios/h2s-oxidation.toml`
/// — the one scenario in the repository that runs at all — panicked on the
/// **seventh** tick inside `process/diffuse.rs`, crediting the enthalpy the lid
/// trades with a reservoir ten kelvin colder than the domain starts at: an `i64`
/// energy counter holds `2^63/2^k_E = 62.5 mJ` at `k_E = 67`, one tick credits
/// 0.152 of that, and six ticks fill it. It went past five hundred and sixty-one
/// tests because the suite loaded every scenario and ran none;
/// `the_shipped_scenario_survives_a_thousand_ticks` is what was missing.
///
/// # Version 19: the reaction step reports its extent, and the matter identity
/// grows a second term
///
/// ADR-080. `kernels::react::react_voxel` takes an eleventh binding, `xi_out`,
/// and writes `idx * R + r` — how far each reaction ran in this voxel — beside
/// the amounts and the energy increment. The host reduces that slice in phase 5
/// LEDGER, and `ledger::residual_matter` becomes
/// `Delta n_s == Sum_c credited(c, s) + Sum_r nu_(r,s) * Xi_r`.
/// `process::Conservation` gains a third arm, `Transmutes`, which step `h`
/// declares for matter and no process declares for energy.
///
/// **The world does not move by one unit, and this time the record says so
/// itself.** Step `h` stays undispatchable — `refuse_if_blocked` still refuses an
/// enabled `reactions`, now over the energy arch alone — so no scenario runs a
/// reaction, `xi_out` is written by no dispatch, and every run of version 18
/// comes out under version 19 bit for bit. The guard of ADR-020 fires because
/// `kernels/**` and `process/**` were edited, and it fires idle. That is the
/// **fourth** idle firing, after ADR-057, ADR-075 and ADR-083, and the number
/// moves anyway for the reason the rule has no exceptions: a guard that cannot
/// tell a report from a change of dynamics is a guard, and an author who decides
/// that on its behalf once will decide it again.
///
/// What the number does record is a price that is real the moment step `h`
/// dispatches: `4 * R` bytes per voxel of extent, against the 230 B per voxel of
/// ADR-062. At `R = 1` that is 234 B and 490 MB at 128 cubed; at the ten
/// reactions SPEC section 2.3 is heading for, 270 B and 566 MB; at the build
/// bound `R_MAX = 64`, 486 B and 1 019 MB, where the slice alone is 256 B per
/// voxel — 111% of the state — which is why it is `n_reactions` long and never
/// `R_MAX`.
///
/// What it does **not** record is the energy half. ADR-081 closes that one by
/// weighting the left side, not by a second term on the right, and folding both
/// into one edit would have credited every conversion twice.
///
/// # Version 20: the left side of the energy invariant is weighted, and `nu_E`
/// carries the sign of the field
///
/// ADR-081. Three things in `process/**` moved and every one of them is
/// semantics:
///
/// - `Tick::domain_sums` gained a last door. The left side of the energy
///   identity is now `H_field + Sum_s w_s * n_s`, where
///   `w_s = round(enthalpy_formation_s * 2^(k_E - k_s))` is derived at load, so
///   a reaction stops being a source of energy and becomes a transfer between
///   two forms of one quantity.
/// - `process/diffuse.rs` and `process/advect.rs` credit `BOUNDARY_EXCHANGE`
///   with `Sum_s w_s * Delta n_s` beside the per-substance flow. Matter that
///   leaves through the lid takes its chemical energy with it, and the counter
///   has to say so.
/// - the `ProcessId::Reactions` arm of `refuse_if_blocked` no longer names the
///   ledger, which no longer blocks anything.
///
/// **This firing is not idle, and it is the first of the batch that is not.**
/// Every scenario with a venting face now moves an energy counter it did not
/// move under version 19: on the shipped scenario the lid credits the chemical
/// energy of what it trades, which is `4.55 mJ` on the worst transient tick
/// against the `9.61 mJ` the same face already owed as heat. No amount, no
/// temperature and no field moves — the credit lands on a counter and the
/// residual closes on both sides of it — but a channel total is an observable
/// (ADR-037, ADR-071), and two runs either side of this number are not
/// comparable.
///
/// `nu_E` also changes sign for every exothermic reaction, from
/// `round(dH * 2^(k_E - e_r))` to `-Sum_s nu_s * w_s`. That moves no run in the
/// repository today only because step `h` is dispatched by nothing; the day it
/// is, it is the difference between a reaction that warms its cell and one that
/// cools it.
pub const WORLD_FORMAT_VERSION: u32 = 20;
