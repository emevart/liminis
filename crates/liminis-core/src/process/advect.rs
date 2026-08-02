//! The advection process: three component-split applications, one Courant
//! number per face, and the buffers between them.
//!
//! Everything `kernels/advect.rs` is not allowed to know. The kernel receives a
//! folded Courant number per face and an axis; this file is where `u`, `dt` and
//! `dx` are turned into that number, where the three axes are ordered, and where
//! the process boundary of ADR-057 is put back together afterwards.
//!
//! # One application is one axis, at the full step
//!
//! Advection is split component-wise (ADR-036), so a tick is three applications
//! of one kernel, each along one axis and each over the **whole** `dt`. That is
//! what makes the Courant condition SPEC section 4.2 actually writes —
//! `max(|u|*dt/dx) <= 1` — the right condition: an unsplit three-dimensional
//! donor scheme would need the *sum* over the axes, three times stricter.
//!
//! The splitting shortens the second inequality of that section the same way
//! and does not remove it: a voxel still gives away the **sum** over the faces
//! matter leaves through, and one application has two of them rather than six.
//! Both live in [`Advect::fold_courant`], which is the only place in the project
//! that ever sees the velocity a face actually has.
//!
//! The reading that would pass every conservation test in this file and cost
//! three times the transport forever is "the three axes share the step",
//! `u*dt/(3*dx)` per application. It conserves exactly, it is stable, and
//! ADR-036 forbids it in a sentence that reads like caution rather than like a
//! rule — which is why `one_application_is_one_axis_at_the_full_step` exists.
//!
//! # Sequentially, not in parallel
//!
//! The three applications compose: the input of each is the output of the last.
//! [`Field::lane_pair_dir_mut`](crate::world::Field::lane_pair_dir_mut) is how,
//! and the alternative — `lane_pair_mut`, which is always `front -> back` — is
//! the silent failure of this file. Under it every axis reads state `N` and
//! overwrites the previous axis's result: each application still conserves
//! exactly, both ledgers stay green, the undershoot bound holds, and the world
//! advects along the last axis only. No conservation test can see it;
//! `the_three_axes_compose_sequentially_not_in_parallel` can.
//!
//! # What is not here
//!
//! The velocity field. ADR-069 makes it prescribed — the curl of a stored
//! potential, read off the 64^3 velocity grid and interpolated onto the fine
//! faces — and neither the stencil nor the mapping of a fine face onto coarse
//! cells is written anywhere. [`Advect::fold_courant`] therefore takes face
//! velocities in m/s **already interpolated**, and the interpolation is a TODO
//! with an address rather than a guess.
//!
//! No substeps, and no `every_n_ticks`. Running an operator once every `n` ticks
//! multiplies the effective step and falsifies a condition already checked at
//! load; ADR-005 and ADR-030 forbid it for a *diffusive* field, and whether the
//! ban reaches a hyperbolic one is settled nowhere — the same gap
//! `kernels/settle.rs` records for settling, and this is the second place that
//! says so.
//!
//! No addition of the pressure velocity to the prescribed one. ADR-069 forbids
//! it outright: the pressure step already spends the full Courant budget of its
//! own step, so the sum of two fields that each satisfy the condition violates
//! it.

use anyhow::{Result, bail};

use super::{Conservation, Invariant};
use crate::kernels::advect::{AdvectParams, advect_voxel_32, advect_voxel_64};
use crate::numeric::{Q, qadd, qsub};
use crate::world::{Boundary, Direction, Face, Field32, Field64, Grid, ParitySplit, Width};

/// The order the three axes are applied in: X, then Y, then Z.
///
/// A choice made in code, and it is worth saying so out loud. ADR-036 makes the
/// order of *operators* part of the world semantics, but no document says
/// whether advection runs X first or alternates the order between ticks — and
/// Lie-Trotter splitting does not commute, so this array moves the result of
/// every run that advects. It is a named constant with a test on it
/// (`the_axis_order_is_the_declared_one`) rather than a literal inside a loop,
/// so that changing it is a visible edit with a failing test attached instead of
/// a silent shift.
pub const AXIS_ORDER: [u32; 3] = [0, 1, 2];

/// How many applications of the kernel make up one tick of advection.
///
/// Three, one per axis, each over the full step. Not a substep count: there is
/// nothing to divide (ADR-036). It is **odd**, which is the whole of what
/// [`AdvectPhase`] needs in order to restore the process boundary.
pub const APPLICATIONS_PER_TICK: u32 = AXIS_ORDER.len() as u32;

/// The default of `enabled` for the advection process, and it is `false`.
///
/// ADR-065 puts the default of a process beside its invariant, in code under
/// `process/`, because the loader materialises the whole process list before
/// hashing — so "the scenario did not write it" stopped meaning "there is no
/// such process". ADR-069 then assigned this one by name: advection enabled
/// while the velocity field is off is step `c` with nothing to advect with, and
/// `configs/scenarios/hello.toml`, which names no process at all, would stop
/// loading.
///
/// Here rather than in `config/`, and that is ADR-065's point rather than a
/// matter of taste: `config/**` is not watched by the guard of ADR-020 (see
/// `version.rs`), so a default that lived there could change every run of every
/// scenario without moving `WORLD_FORMAT_VERSION`.
pub const ENABLED_BY_DEFAULT: bool = false;

/// Advection of one lane of one field: the three axis applications of a tick.
///
/// Holds the folded parameters of each application, and the two numbers the fold
/// of the Courant field needs. One instance serves every lane of a field, unlike
/// [`Diffuse`](super::Diffuse): the velocity field is one field of the world, so
/// every substance is carried by the same faces at the same speed and there is
/// no per-substance coefficient to derive.
#[derive(Clone, Copy, Debug)]
pub struct Advect {
    params: [AdvectParams; 3],
    /// The grid, for the outflow half of [`Advect::fold_courant`] and for
    /// nothing else. That check is per **voxel** rather than per cell: it needs
    /// to know which of a voxel's two faces on an axis are open and where the
    /// upper one lives in the buffer, and `Grid::neighbour` is the project's one
    /// answer to both. Copying its arithmetic into a fourth neighbourhood — the
    /// kernel already keeps a third — is how the two would drift.
    grid: Grid,
    /// The tick, in seconds. Visible in exactly one function past this struct —
    /// see [`Advect::fold_courant`].
    dt: f64,
    /// The voxel edge, in metres, and the **fine** one: the flux is computed on
    /// the 128^3 grid, and substituting the step of the coarse velocity grid
    /// would double the admissible speed with no basis (ADR-069).
    dx: f64,
    n_voxels: u32,
}

impl Advect {
    /// Fold the parameters of the three applications for one grid.
    ///
    /// `dt` is the tick in seconds and `dx` the **fine** voxel edge in metres.
    ///
    /// # Errors
    ///
    /// Returns an error if `dt` or `dx` is not finite and positive, or if the
    /// grid has an `exchange` face — which cannot be built today for a reason of
    /// its own (`world::Grid::new`).
    pub fn new(grid: &Grid, dt: f64, dx: f64) -> Result<Self> {
        if !dt.is_finite() || dt <= 0.0 {
            bail!("dt {dt} s is not a usable timestep");
        }
        if !dx.is_finite() || dx <= 0.0 {
            bail!("dx {dx} m is not a usable voxel edge");
        }

        // One mask for all three applications: the boundary is a property of the
        // grid, not of the axis being swept.
        let base = AdvectParams {
            nx: grid.nx(),
            ny: grid.ny(),
            nz: grid.nz(),
            periodic_mask: periodic_mask(grid)?,
            axis: AXIS_ORDER[0],
        };
        let params = [
            base,
            AdvectParams {
                axis: AXIS_ORDER[1],
                ..base
            },
            AdvectParams {
                axis: AXIS_ORDER[2],
                ..base
            },
        ];

        Ok(Self {
            params,
            grid: *grid,
            dt,
            dx,
            n_voxels: grid.n_voxels(),
        })
    }

    /// The folded parameters of one application, as the kernel receives them.
    ///
    /// # Panics
    ///
    /// If `application` is not one of the three.
    #[inline]
    #[must_use]
    pub fn params(&self, application: usize) -> AdvectParams {
        self.params[application]
    }

    /// How many cells a Courant buffer for this grid holds: `3 * n_voxels`, one
    /// per face of the domain.
    ///
    /// One cell per face and not two per face: a face is computed once, in the
    /// canonical orientation from the smaller linear index to the larger, and
    /// both voxels sharing it read the same cell (ADR-054).
    // TODO(courant-buffer-owner): who allocates it is not decided. At 128^3 it
    // is 3*128^3*4 = 25.2 MB per run, and no record puts it in the per-voxel
    // budget: ADR-045 prices scratch fields, ADR-067 and ADR-069 both count
    // their own traffic without it, and `world::World` allocates no such buffer.
    // It is a length here rather than a `Vec` for exactly that reason — a
    // process that owned it would be answering a question nobody has asked.
    #[inline]
    #[must_use]
    pub fn courant_len(&self) -> usize {
        3 * self.n_voxels as usize
    }

    /// Fold face velocities in m/s into the Courant numbers the kernel reads.
    ///
    /// **The one place where `u`, `dt` and `dx` are visible at once.** Past this
    /// function they exist only as `u*dt/dx`, and the kernel cannot recombine
    /// them in the wrong order because it never sees them (`ARCHITECTURE.md`,
    /// ADR-034).
    ///
    /// # Both inequalities of SPEC section 4.2, and they are not one check
    ///
    /// The first is per face and is about stability: `max(|u|*dt/dx) <= 1`. The
    /// second is per **voxel** and is about non-negativity: a voxel gives away
    /// the sum over the faces matter *leaves* through, and a field can pass the
    /// first and fail the second. Two faces of one voxel at `c = -0.9` below and
    /// `c = +0.9` above are each legal on their own, and one application takes
    /// that voxel from 1000 to -800 — with the domain total unchanged to the
    /// unit, because the matter is in the neighbours. ADR-068's floor for that
    /// voxel is `1000 - 1 = 999`, so this is not the accepted undershoot but the
    /// collapse of positivity, and both ledgers close over it in silence.
    ///
    /// The sum runs over the faces of **one axis**, not six, and that is the
    /// reading ADR-067 already gives the neighbouring operator: under
    /// Lie-Trotter splitting an operator moves matter only through the faces it
    /// sweeps (ADR-036), so the sum of SPEC section 4.2 degenerates to the terms
    /// of that operator's own axis. Settling has one term there and its two
    /// inequalities coincide; advection has two, and they do not.
    ///
    /// This does not make `outflow_bound_violation_is_rejected`
    /// (`config/validate.rs`) redundant, and it is not that refusal moved. The
    /// validator bounds `|u|` by the declared `u_conv_max` over six faces at
    /// load, which is six times stricter than the two faces here, so **no
    /// loadable scenario reaches this refusal today**. It is here because the
    /// kernel's positivity rests on it by name (`kernels/advect.rs`) and because
    /// a Courant buffer is not only ever built from `u_conv_max`: step (b) of
    /// ADR-069 interpolates the curl of a stored potential onto the fine faces,
    /// and a divergence introduced there is exactly what a bound on the declared
    /// maximum cannot see. The cost is one extra pass over `3*n_voxels` at the
    /// fold, which happens once per velocity field rather than once per voxel.
    ///
    /// # The layout and the sign, which are one statement
    ///
    /// `out[axis * n_voxels + idx]` is the Courant number on the **lower** face
    /// of voxel `idx` along `axis`, and it is **positive toward the larger
    /// index**. Both halves come from the kernel and neither is negotiable here:
    /// a voxel reads its lower face at `idx` and its upper face at the index of
    /// its upper neighbour, so the two voxels sharing a face read one cell and
    /// compute one integer (ADR-054).
    ///
    /// Fold `u` as the *outgoing* velocity of a face instead and matter travels
    /// upstream: both ledgers stay green, antisymmetry holds, both widths agree,
    /// and the only symptom is a plume going the wrong way in a field nobody has
    /// looked at yet. `the_courant_cell_is_the_lower_face_and_points_up_the_index`
    /// is what notices.
    ///
    /// Every cell is written, including the ones behind a closed wall that the
    /// kernel never reads. Skipping them looks correct for exactly as long as
    /// the axis stays closed: on a periodic axis that same cell **is** the
    /// wrapping face, so the bug would surface on a change of boundary condition
    /// in a config rather than on a change of code.
    ///
    /// # What it does not do
    ///
    /// It does not interpolate. ADR-069 reads `u` off the 64^3 velocity grid,
    /// and neither the stencil nor the mapping of a fine face onto coarse cells
    /// is written down anywhere, so the velocities arrive here already on the
    /// fine faces.
    // TODO(velocity-interpolation): step (b) of ADR-069 — the trilinear read of
    // the stored potential's curl onto the fine faces — belongs to
    // `process/velocity.rs`, which is a placeholder. Guessing the stencil here
    // would put a second, silent addressing scheme underneath the only advective
    // speed the Courant condition has.
    ///
    /// # Errors
    ///
    /// Returns an error if either slice is not [`Advect::courant_len`] long, if
    /// a velocity is not finite, if a face violates the Courant condition, or if
    /// the outgoing faces of one voxel on one axis sum over one. Every refusal
    /// names the axis and the voxel, because "some face is too fast" is not
    /// something a scenario author can act on.
    pub fn fold_courant(&self, u_face_mps: &[f64], out: &mut [Q]) -> Result<()> {
        if u_face_mps.len() != self.courant_len() || out.len() != self.courant_len() {
            bail!(
                "the Courant fold wants {} cells in and {} out — three axes over \
                 {} voxels, one cell per face (ADR-054) — and was given {} and {}",
                self.courant_len(),
                self.courant_len(),
                self.n_voxels,
                u_face_mps.len(),
                out.len()
            );
        }

        for axis in 0..3u32 {
            for idx in 0..self.n_voxels {
                let cell = (axis * self.n_voxels + idx) as usize;
                let u = u_face_mps[cell];
                if !u.is_finite() {
                    bail!(
                        "face velocity {u} m/s on the lower face of voxel {idx} \
                         along axis {axis} is not a usable speed"
                    );
                }

                let courant = u * self.dt / self.dx;
                let folded = Q::from_f64(courant);
                if !courant_is_within_one(folded) {
                    bail!(
                        "the lower face of voxel {idx} along axis {axis} has \
                         u*dt/dx = {courant} at u = {u} m/s, dt = {} s and \
                         dx = {} m, over the Courant condition of SPEC section \
                         4.2. One application is one axis over the full step \
                         (ADR-036), so the condition is max(|u|*dt/dx) <= 1 and \
                         there is no budget to divide between the axes",
                        self.dt,
                        self.dx
                    );
                }

                out[cell] = folded;
            }
        }

        // The second inequality, and it runs afterwards because it cannot run
        // during the fold: the upper face of a voxel is the *lower* face of its
        // upper neighbour, so half the pair is a cell that has not been written
        // yet.
        for axis in 0..3u32 {
            let (below, above) = match axis {
                0 => (Face::XMinus, Face::XPlus),
                1 => (Face::YMinus, Face::YPlus),
                _ => (Face::ZMinus, Face::ZPlus),
            };
            for idx in 0..self.n_voxels {
                // `Grid::neighbour` answers with the voxel itself wherever a
                // face carries nothing — a closed wall, or an axis one voxel
                // deep that closes onto itself — and that is exactly the set of
                // faces the kernel skips. A face nobody crosses takes nothing
                // out of the pool and must not be counted, or a sealed floor
                // would refuse configs that cannot move a unit.
                let down = self.grid.neighbour(idx, below);
                let up = self.grid.neighbour(idx, above);

                // Positive points up the index (see above), so the lower face
                // takes matter out of this voxel when it is negative and the
                // upper one when it is positive. Anything else is an inflow, and
                // an inflow is not credit against an outflow: the matter arrives
                // in the same application it leaves in, from state `N`.
                let lower = out[(axis * self.n_voxels + idx) as usize];
                let upper = out[(axis * self.n_voxels + up) as usize];
                let mut outflow = Q::ZERO;
                if down != idx && lower < Q::ZERO {
                    outflow = qsub(outflow, lower);
                }
                if up != idx && upper > Q::ZERO {
                    outflow = qadd(outflow, upper);
                }

                if outflow > Q::ONE {
                    bail!(
                        "voxel {idx} gives away {} of itself along axis {axis} in \
                         one application: its lower face carries u*dt/dx = {} and \
                         its upper face {}, and each of them is inside the Courant \
                         condition on its own. This is the second, stricter \
                         inequality of SPEC section 4.2 — the sum over the \
                         *outgoing* faces — and it is about non-negativity rather \
                         than about stability: the voxel comes out below zero by \
                         more than the rounding bound of ADR-068 while the domain \
                         total stays exact to the unit, because the matter is in \
                         the neighbours. The sum is over one axis and not over six \
                         because a split operator only moves matter through the \
                         faces it sweeps (ADR-036, and ADR-067 reading the same \
                         degeneration for settling)",
                        outflow.debug_f64(),
                        lower.debug_f64(),
                        upper.debug_f64()
                    );
                }
            }
        }

        Ok(())
    }

    /// Advection conserves matter, and does not touch energy.
    ///
    /// The first half holds by construction rather than by accuracy: a face is
    /// one integer applied to both sides with opposite signs, so the sum over
    /// the domain is unchanged exactly (ADR-005, ADR-054).
    ///
    /// The second half is a statement about this process, not about physics: it
    /// writes one lane of one amount field. Enthalpy is a field of its own on
    /// its own grid, and no transport process in the project carries it along
    /// with the matter (ADR-062, ADR-067).
    // TODO(channels): matter is `Conserved` only because `world::Grid::new`
    // refuses an `exchange` face today, so nothing can leave the domain at all.
    // The moment that face exists this becomes "conserved except through
    // BOUNDARY_EXCHANGE" (ADR-028, ADR-059), and `Conservation` has no arm to
    // say it with. The caveat belongs in the code rather than in a plan, because
    // on the day the arm arrives nothing else will point at this line.
    #[inline]
    #[must_use]
    pub fn invariant(&self) -> Invariant {
        Invariant {
            matter: Conservation::Conserved,
            energy: Conservation::Conserved,
        }
    }
}

/// Generates the per-width application loop.
///
/// One text for both storage widths, for the reason ADR-040 gives on the kernel
/// side: the width of an amount is derived at load time, so both have to exist,
/// and two hand-written copies drift — here they would drift in the alternation,
/// which is the one place a difference looks like physics.
macro_rules! define_advance {
    ($name:ident, $field:ty, $voxel:ident) => {
        #[doc = concat!("Run one tick of advection — three axes — on one lane of a `", stringify!($field), "`.")]
        ///
        /// **The applications compose.** Each reads what the previous wrote,
        /// which is what alternating [`Direction`] buys and what `lane_pair_mut`
        /// would quietly destroy: it is always `front -> back`, so all three
        /// axes would read state `N` and the last would win.
        ///
        /// **Not one swap.** The exchange is one per field
        /// (`world::Field::swap`), so swapping between applications would drag
        /// every other lane forward with this one. The run alternates inside
        /// this lane's own two slices and leaves the field's pointers where it
        /// found them (ADR-057).
        ///
        /// Three is odd, so this lane's new state ends in the **back** buffer.
        /// Nobody outside the process may see that: [`AdvectPhase`] puts it
        /// right before it returns.
        ///
        /// # Panics
        ///
        /// If the field does not have the shape this process was folded
        /// against, if `lane` is not one of its lanes, or if the Courant buffer
        /// is not [`Advect::courant_len`] long. All three are programming errors
        /// rather than bad scenarios, and all three would otherwise surface as a
        /// quietly wrong world.
        pub fn $name(&self, field: &mut $field, lane: u32, courant: &[Q]) {
            let n_voxels = field.n_voxels();
            assert_eq!(
                n_voxels, self.n_voxels,
                "the field does not have the shape this process was folded for"
            );
            assert!(
                lane < field.lanes(),
                "lane {lane} of a field holding {}",
                field.lanes()
            );
            assert_eq!(
                courant.len(),
                self.courant_len(),
                "the Courant buffer holds {} cells and this grid has {} faces",
                courant.len(),
                self.courant_len()
            );

            for (application, params) in self.params.iter().enumerate() {
                // Forward, backward, forward: the output of one application is
                // the input of the next, and the field's pointers never move.
                let dir = if application % 2 == 0 {
                    Direction::Forward
                } else {
                    Direction::Backward
                };
                let (src, dst) = field.lane_pair_dir_mut(lane, dir);
                // Every voxel, in one pass, reading state N and writing N+1.
                // Nothing else may go in this loop: a second pass over the same
                // pair would be a kernel reading what it wrote, and a skipped
                // voxel is not left unchanged but left a tick stale.
                for idx in 0..n_voxels {
                    $voxel(src, courant, dst, params, idx);
                }
            }
        }
    };
}

impl Advect {
    define_advance!(advance_lane_32, Field32, advect_voxel_32);
    define_advance!(advance_lane_64, Field64, advect_voxel_64);
}

/// One advection phase over a whole field: every lane advanced by three axis
/// applications, the process-boundary invariant restored once.
///
/// # Every lane, and no exceptions
///
/// Unlike [`DiffusePhase`](super::DiffusePhase) there is no per-lane coefficient
/// and therefore no lane to skip: one velocity field carries every substance
/// across the same faces. So every lane takes exactly [`APPLICATIONS_PER_TICK`]
/// applications, every lane has the same parity, and the split degenerates —
/// all lanes odd, the even group empty, one swap and nothing to copy.
///
/// That the split is *computed* rather than written down as "swap and copy
/// nothing" is deliberate: it is the same code path diffusion takes, and it
/// stays right on the day a lane stops being dispatched for a reason nobody has
/// thought of yet. The parity of a lane has to come from the applications the
/// phase **ran** (ADR-057).
#[derive(Clone, Debug)]
pub struct AdvectPhase {
    advect: Advect,
    split: ParitySplit,
    /// The width this phase was folded for. Carried only so that
    /// [`AdvectPhase::restoration_bytes_per_tick`] can report bytes rather than
    /// lanes.
    width: Width,
    lanes: u32,
    n_voxels: u32,
}

impl AdvectPhase {
    /// # Errors
    ///
    /// The errors of [`Advect::new`].
    pub fn new_32(grid: &Grid, lanes: u32, dt: f64, dx: f64) -> Result<Self> {
        Self::fold(grid, lanes, dt, dx, Width::Bits32)
    }

    /// # Errors
    ///
    /// The errors of [`Advect::new`].
    pub fn new_64(grid: &Grid, lanes: u32, dt: f64, dx: f64) -> Result<Self> {
        Self::fold(grid, lanes, dt, dx, Width::Bits64)
    }

    fn fold(grid: &Grid, lanes: u32, dt: f64, dx: f64, width: Width) -> Result<Self> {
        let advect = Advect::new(grid, dt, dx)?;
        // From the applications the phase will actually run, never from a
        // declared count — the distinction that costs diffusion a test of its
        // own (ADR-057). Here it is the same number for every lane, and
        // computing it is still cheaper than remembering why it was safe not to.
        let applications = vec![APPLICATIONS_PER_TICK; lanes as usize];

        Ok(Self {
            advect,
            split: ParitySplit::from_substeps(&applications),
            width,
            lanes,
            n_voxels: grid.n_voxels(),
        })
    }

    /// How the field is put back together at the end of the phase.
    #[inline]
    #[must_use]
    pub fn parity_split(&self) -> &ParitySplit {
        &self.split
    }

    /// The restoration traffic in bytes per tick, read plus write
    /// (`ParitySplit::bytes_per_tick`).
    ///
    /// Zero for advection as it stands, and the zero is the interesting part:
    /// every lane runs the same odd number of applications, so the minority
    /// group is empty and the whole repair is one exchange of two pointers
    /// (ADR-057).
    #[inline]
    #[must_use]
    pub fn restoration_bytes_per_tick(&self) -> u64 {
        self.split.bytes_per_tick(self.n_voxels, self.width)
    }

    /// Advection conserves matter and does not touch energy — see
    /// [`Advect::invariant`], which this phase repeats lane by lane.
    #[inline]
    #[must_use]
    pub fn invariant(&self) -> Invariant {
        self.advect.invariant()
    }
}

/// Generates the per-width phase, for the reason [`define_advance`] gives: two
/// hand-written copies would drift in the restoration, which is the one place a
/// difference looks like physics.
macro_rules! define_phase_apply {
    ($name:ident, $field:ty, $advance:ident, $width:expr) => {
        #[doc = concat!("Run one tick of advection over every lane of a `", stringify!($field), "`.")]
        ///
        /// Returns with the process-boundary invariant of ADR-057 restored: the
        /// front buffer holds the new state for **every** lane. No process may
        /// leave a split parity behind — not even to save the next process a
        /// copy.
        ///
        /// # Panics
        ///
        /// If the field's lane count or shape is not the one this phase was
        /// folded for, or if the Courant buffer is the wrong length.
        pub fn $name(&self, field: &mut $field, courant: &[Q]) {
            assert_eq!(
                self.width, $width,
                "this phase was folded for the other storage width"
            );
            assert_eq!(
                field.lanes(),
                self.lanes,
                "the field holds {} lanes and this phase was folded for {}",
                field.lanes(),
                self.lanes
            );
            assert_eq!(
                field.n_voxels(),
                self.n_voxels,
                "the field does not have the shape this phase was folded for"
            );

            for lane in 0..self.lanes {
                self.advect.$advance(field, lane, courant);
            }
            field.restore_boundary(&self.split);
        }
    };
}

impl AdvectPhase {
    define_phase_apply!(apply_32, Field32, advance_lane_32, Width::Bits32);
    define_phase_apply!(apply_64, Field64, advance_lane_64, Width::Bits64);
}

fn courant_is_within_one(courant: Q) -> bool {
    let magnitude = if courant >= Q::ZERO {
        courant
    } else {
        qsub(Q::ZERO, courant)
    };
    magnitude <= Q::ONE
}

fn periodic_mask(grid: &Grid) -> Result<u32> {
    let mut mask = 0u32;
    for face in Face::ALL {
        match grid.boundary(face) {
            Boundary::Periodic => mask |= 1 << (face as u32),
            Boundary::Closed => {}
            Boundary::Exchange => bail!(
                "face {face:?} is an exchange face: matter crossing it belongs \
                 in the BOUNDARY_EXCHANGE channel (SPEC section 7), and there \
                 are no channel counters yet"
            ),
        }
    }
    Ok(mask)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::numeric::{M32, M64};
    use crate::world::Field;

    /// The eco regime of SPEC section 1.7: a one-second tick and a 100 um voxel.
    const DT: f64 = 1.0;
    const DX: f64 = 1.0e-4;

    fn torus(nx: u32, ny: u32, nz: u32) -> Grid {
        Grid::new(nx, ny, nz, [Boundary::Periodic; 6]).unwrap()
    }

    /// Periodic in X and Y, solid floor and lid in Z — the eco-regime default of
    /// SPEC section 1.6, minus the face that cannot be built yet.
    fn floored(nx: u32, ny: u32, nz: u32) -> Grid {
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
        .unwrap()
    }

    fn boxed(nx: u32, ny: u32, nz: u32) -> Grid {
        Grid::new(nx, ny, nz, [Boundary::Closed; 6]).unwrap()
    }

    /// The velocity that gives a Courant number of `c` at this `dt` and `dx`.
    ///
    /// The velocities in this file are never taken from a scenario, and that is
    /// not laziness: the corpus bounds `u_conv_max` at 16.7 um/s for
    /// `dx = 100 um` and `dt = 1 s` (ADR-069) and names no working value at
    /// all, so a test reading one out of a config would be reading a number
    /// nobody decided.
    fn velocity_for(c: f64) -> f64 {
        c * DX / DT
    }

    /// A Courant field built out of face velocities, so that every test goes
    /// through the fold rather than around it.
    fn courant_for(grid: &Grid, u: impl Fn(u32, u32) -> f64) -> Vec<Q> {
        let advect = Advect::new(grid, DT, DX).unwrap();
        let n_voxels = grid.n_voxels();
        let mut velocities = vec![0.0f64; advect.courant_len()];
        for axis in 0..3u32 {
            for idx in 0..n_voxels {
                velocities[(axis * n_voxels + idx) as usize] = u(axis, idx);
            }
        }
        let mut courant = vec![Q::ZERO; advect.courant_len()];
        advect.fold_courant(&velocities, &mut courant).unwrap();
        courant
    }

    /// A face velocity field with a divergence: neighbouring faces disagree, so
    /// voxels are genuinely sources and sinks. A uniform field would let a
    /// wrong-signed fold pass.
    fn divergent(axis: u32, idx: u32) -> f64 {
        let phase = (axis * 7 + idx * 13) % 9;
        velocity_for(f64::from(phase) / 40.0 - 0.1)
    }

    fn seed_lane_32(field: &mut Field32, lane: u32, amounts: impl Fn(u32) -> i32) {
        let n_voxels = field.n_voxels();
        let buffer = field.write_mut();
        for idx in 0..n_voxels {
            buffer[(lane * n_voxels + idx) as usize] = M32::new(amounts(idx));
        }
    }

    fn seed_lane_64(field: &mut Field64, lane: u32, amounts: impl Fn(u32) -> i64) {
        let n_voxels = field.n_voxels();
        let buffer = field.write_mut();
        for idx in 0..n_voxels {
            buffer[(lane * n_voxels + idx) as usize] = M64::new(amounts(idx));
        }
    }

    fn total_32(field: &Field32) -> i64 {
        field.read().iter().map(|v| v.to_i64()).sum()
    }

    fn total_64(field: &Field64) -> i64 {
        field.read().iter().map(|v| v.to_i64()).sum()
    }

    /// A pattern that varies along all three axes, so that dropping one axis
    /// application is visible in the result.
    fn pattern(grid: &Grid, idx: u32) -> i32 {
        let (x, y, z) = grid.coords(idx);
        1_000 + (x as i32) * 137 + (y as i32) * 311 + (z as i32) * 719
    }

    /// `ACCEPTANCE.md`, section "Conservation".
    ///
    /// Eight ticks of three applications each, on all three boundary
    /// arrangements the project can build, under a uniform Courant field and a
    /// divergent one. The sums are compared as integers — exactly, not to a
    /// tolerance — because a flux scheme conserves by construction and not by
    /// accuracy (ADR-005).
    ///
    /// The last assertion is the one that keeps the rest honest: a conserving
    /// no-op passes everything above it.
    #[test]
    fn advection_alone_conserves_exactly() {
        for grid in [torus(5, 6, 7), floored(5, 6, 7), boxed(5, 6, 7)] {
            for uniform in [true, false] {
                let phase = AdvectPhase::new_32(&grid, 1, DT, DX).unwrap();
                assert_eq!(
                    phase.invariant(),
                    Invariant {
                        matter: Conservation::Conserved,
                        energy: Conservation::Conserved,
                    }
                );
                let courant = if uniform {
                    courant_for(&grid, |_, _| velocity_for(0.25))
                } else {
                    courant_for(&grid, divergent)
                };

                let mut field: Field32 = Field::new(&grid, 1).unwrap();
                seed_lane_32(&mut field, 0, |idx| (idx as i32 * 7919) % 100_000);
                field.swap();
                let before = total_32(&field);
                let seeded = field.read().to_vec();

                for tick in 0..8 {
                    phase.apply_32(&mut field, &courant);
                    assert_eq!(total_32(&field), before, "tick {tick} moved the total");
                }
                assert_ne!(
                    field.read(),
                    seeded.as_slice(),
                    "nothing moved, and a conserving no-op would pass every \
                     assertion above"
                );
            }
        }
    }

    /// `ACCEPTANCE.md`, section "Conservation", the advective half of a name
    /// whose diffusive half lives in `process/diffuse.rs`.
    ///
    /// Water at `5.1e12` units: the `f32` step of the pool is `2^19`, so a flux
    /// comes back quantised in hundreds of thousands of units and the sum still
    /// has to be exact. It is, because the difference inside the limiter is
    /// taken in `M` and the face is one integer applied twice (ADR-060,
    /// ADR-054).
    #[test]
    fn transport_of_a_64_bit_substance_conserves_exactly() {
        let grid = floored(4, 5, 6);
        let phase = AdvectPhase::new_64(&grid, 1, DT, DX).unwrap();
        let courant = courant_for(&grid, divergent);

        let mut field: Field64 = Field::new(&grid, 1).unwrap();
        seed_lane_64(&mut field, 0, |idx| {
            5_100_000_000_000 + i64::from(idx) * 977
        });
        field.swap();

        let before = total_64(&field);
        let seeded = field.read().to_vec();
        for _ in 0..8 {
            phase.apply_64(&mut field, &courant);
            assert_eq!(total_64(&field), before);
        }
        assert_ne!(field.read(), seeded.as_slice());
    }

    /// The composition of the three axes, and the one failure no conservation
    /// test can see.
    ///
    /// Recomputes the tick by hand — three kernel calls, each reading what the
    /// previous wrote — and compares bit for bit. Then denies the three shapes
    /// the parallel mistake can take: the result of one axis alone, for each of
    /// the three.
    ///
    /// Under `lane_pair_mut` (always `front -> back`) every application would
    /// read state `N` and the last one would win. Each application conserves
    /// exactly on its own, so the ledger closes, the undershoot bound holds and
    /// both widths agree while the world advects along Z only.
    #[test]
    fn the_three_axes_compose_sequentially_not_in_parallel() {
        let grid = torus(4, 5, 6);
        let advect = Advect::new(&grid, DT, DX).unwrap();
        let courant = courant_for(&grid, |axis, idx| {
            velocity_for(0.1 + f64::from(axis) * 0.2 + f64::from(idx % 3) * 0.05)
        });
        let n_voxels = grid.n_voxels();

        let seed: Vec<M32> = (0..n_voxels)
            .map(|idx| M32::new(pattern(&grid, idx)))
            .collect();

        // By hand, sequentially: the input of each application is the output of
        // the previous one.
        let mut sequential = seed.clone();
        for application in 0..3usize {
            let mut next = vec![M32::ZERO; n_voxels as usize];
            for idx in 0..n_voxels {
                advect_voxel_32(
                    &sequential,
                    &courant,
                    &mut next,
                    &advect.params(application),
                    idx,
                );
            }
            sequential = next;
        }

        let phase = AdvectPhase::new_32(&grid, 1, DT, DX).unwrap();
        let mut field: Field32 = Field::new(&grid, 1).unwrap();
        seed_lane_32(&mut field, 0, |idx| pattern(&grid, idx));
        field.swap();
        phase.apply_32(&mut field, &courant);
        assert_eq!(field.read(), sequential.as_slice());

        // And it is not any of the three single-axis results, which is what a
        // parallel composition collapses to.
        for application in 0..3usize {
            let mut alone = vec![M32::ZERO; n_voxels as usize];
            for idx in 0..n_voxels {
                advect_voxel_32(
                    &seed,
                    &courant,
                    &mut alone,
                    &advect.params(application),
                    idx,
                );
            }
            assert_ne!(
                field.read(),
                alone.as_slice(),
                "the tick equals application {application} applied to state N \
                 alone: the axes overwrote each other instead of composing"
            );
        }
    }

    /// ADR-036, in the form that can fail.
    ///
    /// One application is one axis over the **whole** step. Dividing the budget
    /// between the three axes conserves exactly, stays stable, passes every
    /// acceptance test in this file and makes transport three times slower
    /// forever.
    #[test]
    fn one_application_is_one_axis_at_the_full_step() {
        let grid = torus(3, 3, 3);
        let u = velocity_for(0.6);
        let courant = courant_for(&grid, |_, _| u);

        let full_step = Q::from_f64(u * DT / DX);
        let third_of_it = Q::from_f64(u * DT / (3.0 * DX));
        assert_ne!(full_step, third_of_it);
        for cell in &courant {
            assert_eq!(*cell, full_step);
            assert_ne!(*cell, third_of_it);
        }
    }

    /// The cell of the buffer, and which way a positive number points.
    ///
    /// One nonzero velocity, one cell, and two claims that fail apart: the two
    /// voxels touched are the ones that share **that** face, and matter moved to
    /// the **larger** linear index.
    ///
    /// Folded as an outgoing face velocity instead, matter travels upstream with
    /// conservation, antisymmetry and both widths still green. Addressed as
    /// `idx*3 + axis` instead, a different pair of voxels moves — which is why
    /// the loop runs over all three axes rather than over X, where the two
    /// layouts agree.
    #[test]
    fn the_courant_cell_is_the_lower_face_and_points_up_the_index() {
        let grid = boxed(4, 4, 4);
        let phase = AdvectPhase::new_32(&grid, 1, DT, DX).unwrap();
        let n_voxels = grid.n_voxels();

        for axis in 0..3u32 {
            // An interior face: the lower face of the voxel one step up the axis
            // from (1, 1, 1).
            let (x, y, z) = match axis {
                0 => (2, 1, 1),
                1 => (1, 2, 1),
                _ => (1, 1, 2),
            };
            let acceptor = grid.index(x, y, z);
            let donor = match axis {
                0 => grid.index(x - 1, y, z),
                1 => grid.index(x, y - 1, z),
                _ => grid.index(x, y, z - 1),
            };

            let courant = courant_for(&grid, |a, idx| {
                if a == axis && idx == acceptor {
                    velocity_for(0.5)
                } else {
                    0.0
                }
            });

            let mut field: Field32 = Field::new(&grid, 1).unwrap();
            seed_lane_32(&mut field, 0, |_| 1_000);
            field.swap();
            phase.apply_32(&mut field, &courant);

            let moved: Vec<u32> = (0..n_voxels)
                .filter(|&idx| field.read()[idx as usize] != M32::new(1_000))
                .collect();
            assert_eq!(
                moved,
                vec![donor.min(acceptor), donor.max(acceptor)],
                "axis {axis}: the face at cell axis*n_voxels + {acceptor} moved \
                 the wrong pair of voxels"
            );
            assert!(
                field.read()[acceptor as usize].to_i64() > 1_000,
                "axis {axis}: a positive Courant number sent matter to the \
                 smaller index"
            );
            assert!(field.read()[donor as usize].to_i64() < 1_000);
        }
    }

    /// The refusal, and what it is compared against.
    ///
    /// Two halves. The first is the plain one: a face over the condition is an
    /// error naming the axis and the voxel.
    ///
    /// The second is the one ADR-068 asks for, and it comes out the other way
    /// round from the way it reads. The bound is one, and one is exactly
    /// representable in both `f64` and the `f32` behind `Q`, so no value at or
    /// below one can climb above it in the cast — the failure ADR-068 shows on
    /// `1/6` cannot be reproduced at this bound, and saying so is worth more
    /// than a test that pretends otherwise. What is left is the same
    /// requirement seen from the other side: the number compared is the number
    /// the kernel runs on. A face whose `f64` Courant exceeds one by less than
    /// half a `Q` step arrives at the kernel as exactly one and is legal; an
    /// `f64` comparison would refuse it and, with it, the grain and the
    /// velocity that sit exactly on their own derived limits.
    #[test]
    fn the_courant_fold_refuses_a_face_over_one_and_compares_it_as_a_q() {
        let grid = torus(3, 3, 3);
        let advect = Advect::new(&grid, DT, DX).unwrap();
        let n_voxels = grid.n_voxels();
        let bad = grid.index(1, 2, 0);
        let mut out = vec![Q::ZERO; advect.courant_len()];

        // Axis 1, so that the refusal has an axis to name that is not the one a
        // flat `idx` would report.
        let mut velocities = vec![0.0f64; advect.courant_len()];
        velocities[(n_voxels + bad) as usize] = velocity_for(1.5);
        let error = advect.fold_courant(&velocities, &mut out).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("axis 1"), "{message}");
        assert!(message.contains(&format!("voxel {bad}")), "{message}");

        // Exactly one: the velocity that crosses a voxel in a tick.
        let mut velocities = vec![0.0f64; advect.courant_len()];
        velocities[bad as usize] = DX / DT;
        assert!(advect.fold_courant(&velocities, &mut out).is_ok());
        assert_eq!(out[bad as usize], Q::ONE);

        // Over one in `f64` by less than half a step of `Q`, and exactly one
        // once folded. Accepted, because the kernel will run on exactly one.
        let over = 1.0 + 2.0f64.powi(-25);
        assert!(over > 1.0);
        assert_eq!(Q::from_f64(over), Q::ONE);
        let mut velocities = vec![0.0f64; advect.courant_len()];
        velocities[bad as usize] = velocity_for(over);
        assert!(advect.fold_courant(&velocities, &mut out).is_ok());
        assert_eq!(out[bad as usize], Q::ONE);

        // Over one after the fold: refused.
        let mut velocities = vec![0.0f64; advect.courant_len()];
        velocities[bad as usize] = velocity_for(1.0 + 2.0f64.powi(-20));
        assert!(advect.fold_courant(&velocities, &mut out).is_err());
    }

    /// The second inequality of SPEC section 4.2, which is not the first one
    /// with a different constant.
    ///
    /// A five-voxel torus, one axis, and two faces that a voxel loses matter
    /// through at once: `-0.9` below it and `+0.9` above it. Each face is well
    /// inside `|c| <= 1`, and each on its own folds without complaint — the
    /// first half of the test is exactly that, so the refusal cannot be mistaken
    /// for the Courant one.
    ///
    /// What the refusal prevents is then shown rather than asserted about: the
    /// same buffer built by hand, one application, and the voxel comes out at
    /// `-800`. The domain total is unchanged to the unit and both faces are
    /// antisymmetric, so no conservation test in the project can see it; ADR-068
    /// bounds that voxel at `1000 - 1 = 999`, so it is not the accepted
    /// undershoot either. `kernels/advect.rs` names this condition as the source
    /// of its positivity, and the fold is the only place that has the velocities
    /// to check it with.
    ///
    /// The last third holds the bound where SPEC puts it: a sum of exactly one
    /// is legal and empties the voxel to zero, so a `<` in place of the `<=`
    /// would refuse the very state the inequality is drawn around.
    #[test]
    fn a_voxel_losing_matter_through_both_faces_of_an_axis_is_refused() {
        let grid = torus(5, 1, 1);
        let advect = Advect::new(&grid, DT, DX).unwrap();
        let mut out = vec![Q::ZERO; advect.courant_len()];

        // Each face on its own: legal, and legal at nine tenths of the whole
        // step, so the refusal below is not the Courant condition in disguise.
        for face in [2usize, 3] {
            let mut velocities = vec![0.0f64; advect.courant_len()];
            velocities[face] = velocity_for(if face == 2 { -0.9 } else { 0.9 });
            assert!(advect.fold_courant(&velocities, &mut out).is_ok());
        }

        // Together they empty voxel 2 through both of its faces at once.
        let mut velocities = vec![0.0f64; advect.courant_len()];
        velocities[2] = velocity_for(-0.9);
        velocities[3] = velocity_for(0.9);
        let message = advect
            .fold_courant(&velocities, &mut out)
            .unwrap_err()
            .to_string();
        assert!(message.contains("voxel 2"), "{message}");
        assert!(message.contains("axis 0"), "{message}");

        // And this is what it prevents. The buffer is built by hand, because the
        // fold now refuses to build it.
        let mut courant = vec![Q::ZERO; advect.courant_len()];
        courant[2] = Q::from_f64(-0.9);
        courant[3] = Q::from_f64(0.9);
        let phase = AdvectPhase::new_32(&grid, 1, DT, DX).unwrap();
        let mut field: Field32 = Field::new(&grid, 1).unwrap();
        seed_lane_32(&mut field, 0, |_| 1_000);
        field.swap();
        let before = total_32(&field);
        phase.apply_32(&mut field, &courant);

        assert_eq!(total_32(&field), before, "the matter is in the neighbours");
        assert_eq!(field.read()[2].to_i64(), -800);
        // ADR-068's floor for this voxel: the stencil minimum, less one for the
        // two open faces, less nothing at all for a pool this far below 2^24.
        assert!(field.read()[2].to_i64() < 1_000 - 1);

        // Exactly one is the bound and it is inclusive: the voxel empties and
        // stops there.
        let mut velocities = vec![0.0f64; advect.courant_len()];
        velocities[2] = velocity_for(-0.5);
        velocities[3] = velocity_for(0.5);
        advect.fold_courant(&velocities, &mut out).unwrap();
        let mut field: Field32 = Field::new(&grid, 1).unwrap();
        seed_lane_32(&mut field, 0, |_| 1_000);
        field.swap();
        phase.apply_32(&mut field, &out);
        assert_eq!(field.read()[2].to_i64(), 0);
    }

    /// `ACCEPTANCE.md`, section "Conservation", from ADR-057.
    ///
    /// Three is odd, so every lane ends in the back buffer, the split swaps, and
    /// the list of lanes to copy is empty. What the test asserts is not that
    /// arrangement but its consequence: after the phase the front buffer holds
    /// the new state of every lane.
    ///
    /// A process that did not restore the boundary at all is invisible to every
    /// conservation test here — the sums are taken over the front buffer and add
    /// up perfectly over stale data — so the reference has to be built without
    /// the phase, and it is: three applications of the kernel by hand, each
    /// reading what the previous wrote.
    ///
    /// The two sides have to be able to fail apart, which is where the earlier
    /// form of this test went wrong. A reference computed by running
    /// `AdvectPhase::apply_32` on a **single-lane** field cannot: every lane of
    /// advection runs the same three applications, so a one-lane split has the
    /// same shape as a three-lane one, and no restoration behaviour whatsoever
    /// moves one side without moving the other by the same amount.
    ///
    /// The intermediate is named for the same reason. **Two** applications is
    /// exactly what the front buffer holds when the restoration is dropped, so
    /// asserting that two differs from three is what gives the equality below
    /// something it can fail.
    ///
    /// Two things this name cannot see here, and both have a home elsewhere. A
    /// restoration copying the *odd* lanes rather than the smaller group is not
    /// wrong on this phase, only more expensive — every lane is odd, so the odd
    /// group and the whole field are the same set — and the choice is watched by
    /// `world::field::the_restoration_copies_the_smaller_parity_group`. A marker
    /// planted in the write buffer says nothing here either: every lane is
    /// dispatched, and application 0 writes `front -> back` over the marker
    /// before anything could read it. A marker earns its place where a lane is
    /// **not** dispatched, which is
    /// `process/settle.rs::a_lane_that_was_not_dispatched_is_not_left_a_tick_stale`.
    #[test]
    fn a_process_returns_with_state_n_in_the_front_buffer_for_every_lane() {
        let grid = floored(4, 4, 4);
        let lanes = 3u32;
        let phase = AdvectPhase::new_32(&grid, lanes, DT, DX).unwrap();
        assert!(phase.parity_split().swaps());
        assert!(phase.parity_split().lanes_to_copy().is_empty());
        assert_eq!(phase.restoration_bytes_per_tick(), 0);

        let advect = Advect::new(&grid, DT, DX).unwrap();
        let courant = courant_for(&grid, divergent);
        let n_voxels = grid.n_voxels();
        let seed = |lane: u32, idx: u32| pattern(&grid, idx) * (1 + lane as i32);

        let mut field: Field32 = Field::new(&grid, lanes).unwrap();
        for lane in 0..lanes {
            seed_lane_32(&mut field, lane, |idx| seed(lane, idx));
        }
        field.swap();
        phase.apply_32(&mut field, &courant);

        for lane in 0..lanes {
            // By hand, out of this lane's own seed: three applications, and the
            // state after two of them, which is what the front buffer holds if
            // the restoration is dropped.
            let mut state: Vec<M32> = (0..n_voxels).map(|idx| M32::new(seed(lane, idx))).collect();
            let mut after_two = Vec::new();
            for application in 0..3usize {
                let mut next = vec![M32::ZERO; n_voxels as usize];
                for idx in 0..n_voxels {
                    advect_voxel_32(
                        &state,
                        &courant,
                        &mut next,
                        &advect.params(application),
                        idx,
                    );
                }
                state = next;
                if application == 1 {
                    after_two.clone_from(&state);
                }
            }
            assert_ne!(
                state, after_two,
                "lane {lane}: the third application changed nothing, so the \
                 equality below would hold over an unrestored buffer too"
            );
            assert_eq!(
                field.lane(lane),
                state.as_slice(),
                "lane {lane} did not come back in the front buffer"
            );
        }
    }

    /// The mask this process folds is the one the grid declares, face by face.
    ///
    /// The `exchange` arm cannot be reached from here: `world::Grid::new`
    /// refuses to build such a grid at all. The refusal in `periodic_mask`
    /// stands anyway, for the same reason `process/diffuse.rs` keeps its own —
    /// the nearest neighbour of "exchange" is "closed", and a face quietly
    /// sealed is exactly the failure nobody would see.
    #[test]
    fn the_boundary_mask_says_what_the_grid_says() {
        assert_eq!(periodic_mask(&torus(3, 3, 3)).unwrap(), 0b11_1111);
        assert_eq!(periodic_mask(&floored(3, 3, 3)).unwrap(), 0b00_1111);
        assert_eq!(periodic_mask(&boxed(3, 3, 3)).unwrap(), 0);

        for grid in [torus(3, 3, 3), floored(3, 3, 3), boxed(3, 3, 3)] {
            let advect = Advect::new(&grid, DT, DX).unwrap();
            for application in 0..3usize {
                let mask = advect.params(application).periodic_mask;
                for face in Face::ALL {
                    let bit = (mask >> (face as u32)) & 1 == 1;
                    assert_eq!(bit, grid.boundary(face) == Boundary::Periodic, "{face:?}");
                }
            }
        }
    }

    /// The order of the three applications is a choice made in code.
    ///
    /// This test proves nothing about physics and stands here for exactly that
    /// reason: the order of *operators* is world semantics (ADR-036), and the
    /// order of the three axis applications inside advection is named by no
    /// document at all. Changing it is then a visible edit with a red test
    /// rather than a silent shift in every run.
    #[test]
    fn the_axis_order_is_the_declared_one() {
        assert_eq!(AXIS_ORDER, [0, 1, 2]);
        assert_eq!(APPLICATIONS_PER_TICK, 3);
        const { assert!(!ENABLED_BY_DEFAULT) };

        let advect = Advect::new(&torus(3, 3, 3), DT, DX).unwrap();
        for (application, axis) in AXIS_ORDER.iter().enumerate() {
            assert_eq!(advect.params(application).axis, *axis);
        }
    }
}
