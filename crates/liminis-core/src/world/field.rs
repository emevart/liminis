//! Field buffers: flat, substance-major, double.
//!
//! A field is the thing a kernel is pointed at. It holds one or more quantities
//! over the same grid, laid out so that a transport kernel walks its own
//! substance contiguously (ADR-041), and it holds each of them twice, so that a
//! kernel never reads from the buffer it writes (ADR-034).

use anyhow::{Result, bail};

use super::grid::Grid;
use super::registry::Width;
use crate::numeric::{M32, M64};

/// Which buffer a substep reads and which it writes.
///
/// Before ADR-057 there was no such choice: a substep read `front`, wrote
/// `back`, and the field swapped. Swapping is what a multi-lane field cannot
/// afford — the exchange is one per field, so it drags every other lane forward
/// with it — so a run of substeps alternates the direction instead and leaves
/// the pointers alone. Two substeps of alternating direction land the lane back
/// where it started, which is the whole trick: after `n` of them the lane's
/// state `N` sits in `front` when `n` is even and in `back` when `n` is odd, and
/// nothing else in the field has moved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Direction {
    /// Read state `N` from `front`, write state `N+1` into `back`.
    Forward,
    /// Read state `N` from `back`, write state `N+1` into `front`.
    Backward,
}

/// Which lanes of a field a process has to copy to restore the invariant of
/// ADR-057, and whether it swaps first.
///
/// # The invariant
///
/// > On completion of a process the front buffer holds state `N` **for every
/// > lane**.
///
/// A process that advanced its lanes by different numbers of substeps has
/// broken it — the odd ones ended in `back`, the even ones in `front` — and owes
/// the repair. There are exactly two correct repairs and they produce identical
/// state:
///
/// ```text
/// do not swap, copy the odd lanes   back -> front
/// swap,        copy the even lanes  back -> front
/// ```
///
/// After the swap the odd lanes are already in the new front, so it is the even
/// ones that are behind. Whichever group is **smaller** is copied; the split is
/// a function of the config, because the substep counts are derived at load
/// (ADR-030).
///
/// # Why minority and not "the odd ones"
///
/// The shorter rule — always copy the odd lanes — is one line less and needs no
/// registry. ADR-057 priced it: on the registry of SPEC section 2.3 the even
/// side holds three lanes (H+ at six substeps, O2 and CO2 at two) and the odd
/// side eleven, **and water is on the odd side**. Copying the odd group costs
/// 100.7 MB a tick at 128^3 against 25.2 MB for the minority — four times, and
/// four times precisely because the one 64-bit lane fell on the expensive side.
/// Nothing about the result differs, so only
/// `the_restoration_copies_the_smaller_parity_group` can see the difference.
///
/// # Indexed by lane, never by substance
///
/// [`ParitySplit::from_substeps`] takes an array indexed by **lane**. The
/// natural way to collect substep counts is by substance, and on the registry of
/// SPEC section 2.3 water is first and takes lane 0, so a substance-indexed
/// array shifts every entry after it by one: a set of lanes of exactly the right
/// size is copied, and it is the wrong set. The ledger closes, the invariant is
/// formally "restored", and eleven lanes of thirteen are a phase stale.
///
/// # A lane nobody advanced is even
///
/// Zero substeps is an even number of substeps, and the parity has to be
/// computed from the substeps a process **ran**, not from the ones a table
/// declares. A lane with `D = 0` that is no longer dispatched (ADR-057) but is
/// still counted as `n = 1` lands in the odd group and gets the write buffer's
/// contents promoted into its front — the previous phase's state, and on the
/// first tick zeroes. Zero is a legal amount and nothing falls over.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParitySplit {
    /// Whether the field is swapped before the copies.
    swaps: bool,
    /// The lanes to copy `back -> front`, ascending.
    copy: Vec<u32>,
    /// How many lanes the field this split belongs to must have.
    lanes: u32,
}

impl ParitySplit {
    /// Work out the repair from the substeps each **lane** ran.
    ///
    /// The index is the lane, and the value is the number of substeps that lane
    /// actually took this phase — zero for a lane the process did not dispatch.
    ///
    /// # The tie
    ///
    /// At `|even| == |odd|` both branches cost the same and no record chooses
    /// between them. **Not swapping wins**, and the rule is written down rather
    /// than left to whichever comparison the author typed: the split has to be a
    /// function of the config (ADR-057 — "the split is known at load"), or the
    /// traffic the loader reports and the traffic the run pays would part
    /// company on exactly the configurations where nobody compares them.
    #[must_use]
    pub fn from_substeps(substeps_by_lane: &[u32]) -> Self {
        let mut odd = Vec::new();
        let mut even = Vec::new();
        for (lane, &substeps) in substeps_by_lane.iter().enumerate() {
            let lane = lane as u32;
            if substeps % 2 == 1 {
                odd.push(lane);
            } else {
                even.push(lane);
            }
        }

        let lanes = substeps_by_lane.len() as u32;
        if odd.len() <= even.len() {
            Self {
                swaps: false,
                copy: odd,
                lanes,
            }
        } else {
            Self {
                swaps: true,
                copy: even,
                lanes,
            }
        }
    }

    /// Whether the repair swaps the field before copying.
    #[inline]
    #[must_use]
    pub fn swaps(&self) -> bool {
        self.swaps
    }

    /// The lanes copied `back -> front`, ascending. Possibly empty — a field
    /// whose lanes all took the same parity of substeps pays nothing.
    #[inline]
    #[must_use]
    pub fn lanes_to_copy(&self) -> &[u32] {
        &self.copy
    }

    /// How many lanes the field this split was built for holds.
    #[inline]
    #[must_use]
    pub fn lanes(&self) -> u32 {
        self.lanes
    }

    /// The restoration **traffic** in bytes per tick: read plus write, which is
    /// twice the bytes copied.
    ///
    /// Traffic and not the size of the copy, because traffic is what ADR-057
    /// prices and what the loader is required to report
    /// (`load_reports_the_restoration_traffic_per_tick`): three lanes of `i32`
    /// at 128^3 are 25.2 MB of copy and **50.3 MB of traffic**. Two numbers
    /// computed by two rules — one printed at load, one paid at run — is a
    /// report that lies, and the report exists precisely because a calibration
    /// nudge to a diffusivity carries a lane across the parity threshold without
    /// saying a word.
    ///
    /// The argument is the **lane length** and not the voxel count, and the two
    /// differ by the ghost cell since ADR-059. One element in `2^21` at 128^3, so
    /// the reported megabytes do not move — but ADR-057's own words are that "two
    /// numbers computed by two rules is a report that lies", and
    /// [`Field::restore_lane`] copies the whole lane, ghost included.
    #[must_use]
    pub fn bytes_per_tick(&self, lane_len: u32, width: Width) -> u64 {
        let per_element = match width {
            Width::Bits32 => 4u64,
            Width::Bits64 => 8,
        };
        2 * self.copy.len() as u64 * u64::from(lane_len) * per_element
    }
}

/// A double-buffered field over a grid.
///
/// # Layout
///
/// One flat array per buffer, addressed **substance-major** (ADR-041):
///
/// ```text
/// buffer[lane * lane_len + idx]
/// ```
///
/// with `idx = x + y*NX + z*NX*NY` inside a lane (SPEC section 1.1). A lane is
/// therefore a contiguous run of `lane_len` entries, which is what
/// [`Field::lane_pair_mut`] hands to a transport kernel: from inside such a
/// kernel the lane is the whole world and the index is a plain `idx`, exactly
/// the `src: &[M]` of the skeleton in `ARCHITECTURE.md`.
///
/// # The last element of a lane is the ghost cell
///
/// `lane_len == n_voxels + 1` (ADR-059). The extra element holds the outside
/// reservoir in storage units, and `Grid::neighbour` hands its index back for
/// every `exchange` face — so the boundary condition arrives at a kernel as an
/// address and never as a `Boundary`.
///
/// Which accessor sees it is the whole of the safety here, and the two groups
/// point opposite ways:
///
/// - [`Field::lane`] and [`Field::lane_write`] are **narrowed to `n_voxels`**.
///   Everything that reduces over the domain goes through them — the ledger, the
///   undershoot bound, the volume export, the floor/peak band of worldgen — and
///   a reduction that swallowed the ghost would report the reservoir as part of
///   the world. That failure is invisible to both residuals: the ghost is
///   constant, so it cancels in `after - before`, and only the absolute sums
///   lie;
/// - [`Field::lane_pair_mut`] and [`Field::lane_pair_dir_mut`] are **widened to
///   `lane_len`**, because a kernel has to be able to read the address the grid
///   gave it. Forgetting the widening is loud: the kernel indexes one past the
///   end of the slice and panics.
///
/// [`Field::set_ghost`] is the only way to write it, and it writes **both**
/// buffers — see the note there for what a single-buffer seeding does.
///
/// ADR-041 chose this orientation over the voxel-major one with the reason
/// spelled out: transport reads one substance and would rather have it
/// contiguous, reactions read all substances of one voxel and can afford a
/// fixed stride, and a tick contains more transport than reaction.
///
/// # Two buffers, and which is which
///
/// [`Field::read`] is state `N`. [`Field::write_mut`] is state `N+1` under
/// construction. They are separate allocations, so the borrow checker enforces
/// the rule that ADR-034 states in prose — no kernel can read from the buffer it
/// writes — and on the GPU they become a `var<storage, read>` and a
/// `var<storage, read_write>` binding.
///
/// [`Field::swap`] exchanges them, in constant time: two `Vec`s trade pointers,
/// nothing is copied. The old state `N` becomes the next write buffer, still
/// holding its data, which is what makes the exchange cheap and also what makes
/// the precondition below matter.
///
/// **Whoever swaps must have written every voxel of every lane.** The write
/// buffer starts a substep holding state `N-1`, not zeroes; a kernel that skips
/// a voxel does not leave it unchanged, it leaves it two steps stale. A gather
/// kernel dispatched over `0..n_voxels` writes every voxel by construction, so
/// this costs nothing to satisfy — but it has to be said, because the failure is
/// silent and looks like a physics bug.
///
/// # Widths
///
/// Instantiate as [`Field32`] or [`Field64`]. Which one a substance gets is
/// derived by the loader from its declared concentrations, never chosen here
/// (ADR-040); in the default registry exactly one substance comes out 64-bit,
/// and it is water.
///
/// A `Field` addresses **lanes, not substances**, and it still does not resolve
/// the difference: that is [`Registry`](super::Registry)'s job, and its answer
/// is the only one (ADR-056). Storage is compact — a `Field32` of thirteen lanes
/// and a `Field64` of one for the registry of SPEC section 2.3, not two
/// full-size parallel arrays with dead slots — so a lane index equals a
/// substance index nowhere, by rule rather than by accident. Anything that has a
/// substance index and wants a buffer address goes through
/// `Registry::slot` or `Registry::lane_of`; nothing computes
/// `s * n_voxels + idx` for itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field<T> {
    lanes: u32,
    n_voxels: u32,
    /// `n_voxels + 1`: the voxels of one lane and the ghost cell after them
    /// (ADR-059). Stored rather than recomputed so that there is one place the
    /// stride of the flat buffer comes from.
    lane_len: u32,
    /// State `N`.
    front: Vec<T>,
    /// State `N+1`, under construction.
    back: Vec<T>,
}

/// A field of 32-bit amounts. The common case: thirteen of the fourteen
/// substances in the default registry (ADR-040).
pub type Field32 = Field<M32>;

/// A field of 64-bit amounts. Water, and anything else whose declared maximum
/// concentration does not fit 32 bits at its derived scale (ADR-040).
pub type Field64 = Field<M64>;

impl<T: Copy + Default> Field<T> {
    /// Allocate both buffers, zeroed.
    ///
    /// # Errors
    ///
    /// Returns an error if `lanes` is zero, or if `lanes * lane_len` does not
    /// fit a `u32`. The second bound is not about this crate's `Vec`s, which are
    /// indexed by `usize`: it is about the kernels, which compute
    /// `lane * lane_len + idx` in `u32` because that is what WGSL gives them
    /// (ADR-041). A buffer that only a 64-bit host can address is a buffer the
    /// GPU path cannot use.
    pub fn new(grid: &Grid, lanes: u32) -> Result<Self> {
        if lanes == 0 {
            bail!("a field with no lanes holds nothing");
        }
        let n_voxels = grid.n_voxels();
        // The lane length and not the voxel count: the ghost cell of ADR-059 is
        // an element of every lane, so it is part of the stride and part of the
        // bound.
        let lane_len = grid.lane_len();
        let Some(total) = lanes.checked_mul(lane_len) else {
            bail!(
                "a field of {lanes} lanes over {n_voxels} voxels and a ghost cell \
                 each is longer than a u32 index can address, and kernels address \
                 it in u32 (ADR-041, ADR-059)"
            );
        };

        Ok(Self {
            lanes,
            n_voxels,
            lane_len,
            front: vec![T::default(); total as usize],
            back: vec![T::default(); total as usize],
        })
    }
}

impl<T> Field<T> {
    /// How many quantities share this field.
    #[inline]
    #[must_use]
    pub fn lanes(&self) -> u32 {
        self.lanes
    }

    /// Voxels per lane. The grid's `n_voxels`, kept here so a kernel call site
    /// does not have to carry the grid around as well.
    #[inline]
    #[must_use]
    pub fn n_voxels(&self) -> u32 {
        self.n_voxels
    }

    /// Elements per lane: `n_voxels + 1`, the voxels and the ghost cell
    /// (ADR-059). The stride of the flat buffer.
    #[inline]
    #[must_use]
    pub fn lane_len(&self) -> u32 {
        self.lane_len
    }

    /// State `N`: the whole flat buffer, every lane, `lane * lane_len + idx`.
    ///
    /// **Ghost cells included**, because this is the buffer a kernel is pointed
    /// at rather than a view of the domain. Anything summing over the world
    /// wants [`Field::lane`], which is narrowed.
    #[inline]
    #[must_use]
    pub fn read(&self) -> &[T] {
        &self.front
    }

    /// State `N+1`: the whole flat buffer, to be filled.
    #[inline]
    pub fn write_mut(&mut self) -> &mut [T] {
        &mut self.back
    }

    /// Both whole buffers at once, `(state N, state N+1)`.
    ///
    /// For a kernel that needs every lane of a voxel in one pass — the reaction
    /// kernel of ADR-041 is the one that does.
    #[inline]
    pub fn pair_mut(&mut self) -> (&[T], &mut [T]) {
        (&self.front, &mut self.back)
    }

    /// State `N` of one lane: the **`n_voxels` voxels**, indexed by plain `idx`.
    ///
    /// Narrowed, and the narrowing is the point. A lane is `n_voxels + 1`
    /// elements long since ADR-059 and the last of them is the outside
    /// reservoir; every reduction over the domain comes through here, and one
    /// that swallowed the ghost would count the reservoir as part of the world.
    ///
    /// That mistake survives both residuals. The ghost is constant, so it
    /// cancels in `after - before` and `ledger_residual_is_zero_over_1e6_ticks`
    /// stays green; what lies is the absolute sum, and with it the volume export
    /// and the floor/peak band of worldgen. `a_domain_reduction_skips_the_ghost_
    /// element` is the only thing that sees it, which is why it is a test of its
    /// own rather than a corollary of a conservation test.
    #[inline]
    #[must_use]
    pub fn lane(&self, lane: u32) -> &[T] {
        &self.front[self.voxel_range(lane)]
    }

    /// State `N+1` of one lane, read only.
    ///
    /// The door a snapshot needs and the only one that was missing: the file
    /// holds **both** buffers of every field (ADR-037, ADR-057) and is written
    /// in substance order (ADR-056), so it walks lane by lane over a `&World`,
    /// and every other way to see the second buffer — [`Field::write_mut`],
    /// [`Field::pair_mut`] — takes `&mut self` in order to hand out a slice to
    /// write into. This one writes nothing.
    ///
    /// Not for a kernel. A kernel that read the buffer it writes would break the
    /// gather form of ADR-034, and the two accessors above are how it gets its
    /// slices; this returns state `N+1` to a reader that is not in a tick at all.
    ///
    /// Narrowed to the voxels, like [`Field::lane`], and for the same reason.
    #[inline]
    #[must_use]
    pub fn lane_write(&self, lane: u32) -> &[T] {
        &self.back[self.voxel_range(lane)]
    }

    /// One lane of each buffer, `(state N, state N+1)`.
    ///
    /// The transport kernel's view of the world, and the `world.pair_mut(s)` of
    /// the skeleton in `ARCHITECTURE.md`: inside the kernel the slices are
    /// indexed by the voxel index alone, and the lane has vanished from the
    /// arithmetic.
    #[inline]
    pub fn lane_pair_mut(&mut self, lane: u32) -> (&[T], &mut [T]) {
        let range = self.lane_range(lane);
        (&self.front[range.clone()], &mut self.back[range])
    }

    /// One lane of each buffer, in the direction asked for.
    ///
    /// [`Field::lane_pair_mut`] is this at [`Direction::Forward`]. The other
    /// direction exists so that a run of substeps can alternate instead of
    /// swapping (ADR-057): a swap is one per field and would carry every other
    /// lane along with it, while alternating touches nothing but this lane's
    /// two slices.
    ///
    /// Which buffer holds the lane's state `N` afterwards is then a matter of
    /// counting: `front` after an even number of substeps, `back` after an odd
    /// one. Restoring the invariant from that is [`Field::restore_boundary`].
    #[inline]
    pub fn lane_pair_dir_mut(&mut self, lane: u32, dir: Direction) -> (&[T], &mut [T]) {
        let range = self.lane_range(lane);
        match dir {
            Direction::Forward => (&self.front[range.clone()], &mut self.back[range]),
            Direction::Backward => (&self.back[range.clone()], &mut self.front[range]),
        }
    }

    /// Exchange the buffers: state `N+1` becomes state `N`.
    ///
    /// Constant time — the two `Vec`s trade pointers. Read the note on
    /// [`Field`] about what the caller owes before calling this: the buffer
    /// being promoted has to have been written in full.
    ///
    /// The exchange is for the **whole field**: every lane advances together.
    /// A per-lane swap does not exist and cannot: ADR-057 settled that the
    /// granularity stays the field, and that a process which advanced its lanes
    /// by different numbers of substeps repairs the difference with one swap and
    /// a copy of the smaller parity group — see [`Field::restore_boundary`].
    #[inline]
    pub fn swap(&mut self) {
        core::mem::swap(&mut self.front, &mut self.back);
    }

    /// Where a lane lives in a buffer, ghost cell included. The one place the
    /// layout formula is written down.
    #[inline]
    fn lane_range(&self, lane: u32) -> core::ops::Range<usize> {
        let start = self.lane_start(lane);
        start..start + self.lane_len as usize
    }

    /// The voxels of a lane, without the ghost cell. What a reduction over the
    /// domain gets.
    #[inline]
    fn voxel_range(&self, lane: u32) -> core::ops::Range<usize> {
        let start = self.lane_start(lane);
        start..start + self.n_voxels as usize
    }

    #[inline]
    fn lane_start(&self, lane: u32) -> usize {
        debug_assert!(
            lane < self.lanes,
            "lane {lane} of a field with {} lanes",
            self.lanes
        );
        // No overflow: `lanes * lane_len` was checked to fit a u32 at
        // construction, and `lane < lanes`.
        (lane * self.lane_len) as usize
    }

    /// The index of one lane's ghost cell in the flat buffer.
    #[inline]
    fn ghost_at(&self, lane: u32) -> usize {
        self.lane_start(lane) + self.n_voxels as usize
    }
}

impl<T: Copy> Field<T> {
    /// Seed the ghost cell of one lane with the reservoir, **in both buffers**.
    ///
    /// # Both, and this is the whole function
    ///
    /// A run of substeps alternates direction rather than swapping (ADR-057),
    /// so substep 0 reads `front` and substep 1 reads `back`. A ghost seeded
    /// into `front` alone therefore holds the reservoir on even substeps and a
    /// zero on odd ones, and the lid becomes an infinite sink every other
    /// substep.
    ///
    /// Nothing else in the project can notice that. Zero is a legal amount; the
    /// flux stays antisymmetric against whatever the ghost holds; the channel
    /// counter records exactly what left, because it is computed from the same
    /// buffer the kernel read; and both residuals close to the unit. The same
    /// applies after [`Field::restore_lane`], which copies the lane whole — an
    /// unseeded `back` would overwrite a correctly seeded `front`.
    ///
    /// # Panics
    ///
    /// In debug builds, if `lane` is not one of this field's lanes.
    pub fn set_ghost(&mut self, lane: u32, value: T) {
        let at = self.ghost_at(lane);
        self.front[at] = value;
        self.back[at] = value;
    }

    /// What the ghost cell of one lane holds, out of the front buffer.
    ///
    /// [`Field::set_ghost`] keeps the two buffers equal, so which one this reads
    /// is not a choice a caller has to make.
    ///
    /// # Panics
    ///
    /// In debug builds, if `lane` is not one of this field's lanes.
    #[must_use]
    pub fn ghost(&self, lane: u32) -> T {
        self.front[self.ghost_at(lane)]
    }

    /// Copy one lane `back -> front`, so that the front buffer holds it.
    ///
    /// **Always in that direction**, and after whatever swap the caller has
    /// already done — [`Field::restore_boundary`] is the caller that gets the
    /// order right. The direction is the one load-bearing thing in this
    /// function and it is invisible to every conservation test in the project:
    /// `front -> back` leaves the copied lanes exactly one phase stale, and a
    /// stale state is conserved no worse than a fresh one, so both domain sums
    /// stand still while the world quietly stops moving for those lanes. Only a
    /// comparison against the same lane run alone can see it, which is what
    /// `restoration_copies_back_to_front_after_the_optional_swap` is.
    pub fn restore_lane(&mut self, lane: u32) {
        let range = self.lane_range(lane);
        self.front[range.clone()].copy_from_slice(&self.back[range]);
    }

    /// Restore the process-boundary invariant of ADR-057: the front buffer holds
    /// state `N` for every lane.
    ///
    /// One swap at most, one pass over each lane of the minority at most, and
    /// never more than that per tick. See [`ParitySplit`] for which group is
    /// copied and why it is the smaller one rather than the odd one.
    ///
    /// # Panics
    ///
    /// If the split was built for a different number of lanes. A split of the
    /// wrong length would copy a set of lanes of plausible size and leave the
    /// rest of the field a phase behind, which nothing downstream can detect.
    pub fn restore_boundary(&mut self, split: &ParitySplit) {
        assert_eq!(
            split.lanes(),
            self.lanes,
            "a parity split of {} lanes applied to a field of {}: the invariant \
             would be restored for a set of lanes of the right size and the \
             wrong membership (ADR-057)",
            split.lanes(),
            self.lanes
        );

        if split.swaps() {
            self.swap();
        }
        for &lane in split.lanes_to_copy() {
            self.restore_lane(lane);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{Boundary, Face};

    const NX: u32 = 3;
    const NY: u32 = 4;
    const NZ: u32 = 5;
    const LANES: u32 = 3;

    fn grid() -> Grid {
        Grid::new(NX, NY, NZ, [Boundary::Periodic; 6]).unwrap()
    }

    fn field() -> Field32 {
        Field::new(&grid(), LANES).unwrap()
    }

    /// A value that depends on both coordinates, so that a lane mix-up cannot
    /// pass by accident.
    fn pattern(lane: u32, idx: u32) -> M32 {
        M32::new((1 + lane as i32) * 1000 + idx as i32)
    }

    fn fill_write_buffer(field: &mut Field32) {
        let n_voxels = field.n_voxels();
        let lane_len = field.lane_len();
        let lanes = field.lanes();
        let buffer = field.write_mut();
        for lane in 0..lanes {
            for idx in 0..n_voxels {
                buffer[(lane * lane_len + idx) as usize] = pattern(lane, idx);
            }
        }
    }

    #[test]
    fn a_field_starts_zeroed_and_the_right_size() {
        let field = field();
        assert_eq!(field.lanes(), LANES);
        assert_eq!(field.n_voxels(), NX * NY * NZ);
        // The buffer carries a ghost cell per lane and the lane view does not
        // (ADR-059).
        assert_eq!(field.lane_len(), NX * NY * NZ + 1);
        assert_eq!(field.read().len(), (LANES * (NX * NY * NZ + 1)) as usize);
        assert!(field.read().iter().all(|&v| v == M32::ZERO));
        assert_eq!(field.lane(0).len(), (NX * NY * NZ) as usize);
        assert_eq!(field.lane_write(0).len(), (NX * NY * NZ) as usize);
    }

    #[test]
    fn a_swap_promotes_the_write_buffer_without_losing_data() {
        let mut field = field();

        // State N is zero; write state N+1 in full, then exchange.
        fill_write_buffer(&mut field);
        assert!(field.read().iter().all(|&v| v == M32::ZERO));
        field.swap();

        for lane in 0..LANES {
            for idx in 0..field.n_voxels() {
                assert_eq!(
                    field.read()[(lane * field.lane_len() + idx) as usize],
                    pattern(lane, idx),
                    "lane {lane}, voxel {idx} did not survive the swap"
                );
            }
        }

        // And the old state N is not gone: it is the next write buffer, intact.
        // That is what makes the exchange free, and it is why a kernel has to
        // write every voxel rather than assume a clean sheet.
        assert!(field.write_mut().iter().all(|&v| v == M32::ZERO));

        // Two swaps are the identity.
        field.swap();
        assert!(field.read().iter().all(|&v| v == M32::ZERO));
        field.swap();
        assert_eq!(field.read()[0], pattern(0, 0));
    }

    #[test]
    fn the_two_buffers_are_different_memory() {
        // The prose rule of ADR-034 — never read from the buffer you write — is
        // held up here by the type system rather than by discipline: `pair_mut`
        // could not hand out both references at once if they overlapped.
        let mut field = field();
        let (src, dst) = field.pair_mut();
        assert!(!core::ptr::eq(src.as_ptr(), dst.as_ptr()));
        assert_eq!(src.len(), dst.len());

        // Widened to the ghost cell, because the address `Grid::neighbour`
        // hands a kernel for an `exchange` face is `n_voxels` (ADR-059).
        let (src, dst) = field.lane_pair_mut(1);
        assert!(!core::ptr::eq(src.as_ptr(), dst.as_ptr()));
        assert_eq!(src.len(), (NX * NY * NZ + 1) as usize);
        assert_eq!(dst.len(), (NX * NY * NZ + 1) as usize);
    }

    #[test]
    fn a_lane_is_a_contiguous_run_of_the_flat_buffer() {
        // The substance-major layout of ADR-041, stated twice and checked
        // against itself: what the flat buffer holds at `lane * n_voxels + idx`
        // is what the lane slice holds at `idx`.
        let mut field = field();
        fill_write_buffer(&mut field);
        field.swap();

        for lane in 0..LANES {
            let slice = field.lane(lane);
            assert_eq!(slice.len(), field.n_voxels() as usize);
            for idx in 0..field.n_voxels() {
                assert_eq!(slice[idx as usize], pattern(lane, idx));
                assert_eq!(
                    slice[idx as usize],
                    field.read()[(lane * field.lane_len() + idx) as usize]
                );
            }
        }
    }

    #[test]
    fn writing_one_lane_leaves_the_others_alone() {
        // The property a transport kernel relies on: it is handed one lane and
        // cannot reach past it, whatever it does with its indices.
        let mut field = field();
        fill_write_buffer(&mut field);
        field.swap();
        fill_write_buffer(&mut field);

        {
            let (_, dst) = field.lane_pair_mut(1);
            for value in dst.iter_mut() {
                *value = M32::new(-1);
            }
        }
        field.swap();

        for lane in 0..LANES {
            for idx in 0..field.n_voxels() {
                let expected = if lane == 1 {
                    M32::new(-1)
                } else {
                    pattern(lane, idx)
                };
                assert_eq!(field.lane(lane)[idx as usize], expected);
            }
        }
    }

    #[test]
    fn a_gather_over_the_grid_reads_state_n_and_writes_state_n_plus_one() {
        // The shape of a substep, without a kernel: every voxel computes from
        // the neighbours it finds in `src` and writes only its own cell in
        // `dst`. The point of the test is the last assertion — the result does
        // not depend on the order the voxels were visited in, because nothing
        // any voxel wrote was visible to any other.
        let grid = grid();
        let mut field: Field32 = Field::new(&grid, 1).unwrap();
        for idx in 0..grid.n_voxels() {
            field.write_mut()[idx as usize] = M32::new(idx as i32);
        }
        field.swap();

        let mut forwards = field.clone();
        {
            let (src, dst) = forwards.lane_pair_mut(0);
            for idx in 0..grid.n_voxels() {
                let mut sum = M32::ZERO;
                for face in Face::ALL {
                    sum += src[grid.neighbour(idx, face) as usize];
                }
                dst[idx as usize] = sum;
            }
        }
        forwards.swap();

        let mut backwards = field.clone();
        {
            let (src, dst) = backwards.lane_pair_mut(0);
            for idx in (0..grid.n_voxels()).rev() {
                let mut sum = M32::ZERO;
                for face in Face::ALL {
                    sum += src[grid.neighbour(idx, face) as usize];
                }
                dst[idx as usize] = sum;
            }
        }
        backwards.swap();

        assert_eq!(forwards.read(), backwards.read());
        assert_ne!(forwards.read(), field.read());
    }

    #[test]
    fn both_widths_lay_out_the_same_way() {
        // The two widths differ in what a cell holds and in nothing else. If
        // they ever differed in where a lane starts, a 64-bit substance would
        // quietly read someone else's amounts.
        let grid = grid();
        let narrow: Field32 = Field::new(&grid, LANES).unwrap();
        let wide: Field64 = Field::new(&grid, LANES).unwrap();

        assert_eq!(narrow.lanes(), wide.lanes());
        assert_eq!(narrow.n_voxels(), wide.n_voxels());
        assert_eq!(narrow.read().len(), wide.read().len());
        for lane in 0..LANES {
            assert_eq!(narrow.lane(lane).len(), wide.lane(lane).len());
            assert_eq!(narrow.lane_len(), wide.lane_len());
        }
    }

    #[test]
    fn a_64_bit_field_holds_what_a_32_bit_one_cannot() {
        // The pool ADR-040 exists for: water, over the i32 ceiling.
        let grid = grid();
        let mut field: Field64 = Field::new(&grid, 1).unwrap();
        let water = M64::new(5_100_000_000_000);
        field.write_mut()[7] = water;
        field.swap();
        assert_eq!(field.read()[7], water);
        assert!(water.to_i64() > M32::MAX.to_i64());
    }

    /// `ACCEPTANCE.md`, section "Conservation", from ADR-057.
    #[test]
    fn the_restoration_copies_the_smaller_parity_group() {
        // The rule, pinned as a rule and not as a result. Both branches leave
        // the field in the same state, so the only thing that separates
        // "copy the minority" from "copy the odd ones" is the count — and the
        // difference is 100.7 MB a tick against 25.2 at 128^3, because the one
        // 64-bit lane of the registry stands on the odd side (ADR-057).

        // Three even against ten odd: the shape of the registry of SPEC
        // section 2.3, where H+ takes six substeps and O2 and CO2 take two.
        let mut substeps = vec![1u32; 13];
        substeps[0] = 6;
        substeps[4] = 2;
        substeps[9] = 2;
        let split = ParitySplit::from_substeps(&substeps);
        assert!(
            split.swaps(),
            "the minority is even, so the field must swap"
        );
        assert_eq!(split.lanes_to_copy(), &[0, 4, 9]);

        // The mirror: ten even, three odd. Same arithmetic, other branch.
        let mut substeps = vec![2u32; 13];
        substeps[1] = 1;
        substeps[5] = 3;
        substeps[12] = 1;
        let split = ParitySplit::from_substeps(&substeps);
        assert!(!split.swaps(), "the minority is odd, so nothing may swap");
        assert_eq!(split.lanes_to_copy(), &[1, 5, 12]);

        // The tie, which no record settles: not swapping wins, and it has to be
        // the same answer every time, because the loader prints this traffic at
        // load and the run pays it later.
        let split = ParitySplit::from_substeps(&[1, 2, 3, 4]);
        assert!(!split.swaps());
        assert_eq!(split.lanes_to_copy(), &[0, 2]);
        assert_eq!(
            ParitySplit::from_substeps(&[1, 2, 3, 4]),
            ParitySplit::from_substeps(&[5, 6, 7, 8]),
            "the split is a function of the parities and of nothing else"
        );

        // A field of one parity pays nothing at all.
        assert!(
            ParitySplit::from_substeps(&[2, 4, 6])
                .lanes_to_copy()
                .is_empty()
        );
        assert!(
            ParitySplit::from_substeps(&[1, 3, 5])
                .lanes_to_copy()
                .is_empty()
        );
    }

    #[test]
    fn the_restoration_traffic_is_read_plus_write() {
        // ADR-057 prices the restoration of the SPEC 2.3 registry at 25.2 MB of
        // copy and **50.3 MB of traffic** at 128^3, and it is the traffic the
        // loader is required to print. Two numbers computed by two rules is a
        // report that lies.
        const N_VOXELS: u32 = 128 * 128 * 128;
        let mut substeps = vec![1u32; 13];
        substeps[0] = 6;
        substeps[4] = 2;
        substeps[9] = 2;
        let split = ParitySplit::from_substeps(&substeps);

        // The number ADR-057 prices, over the voxels.
        let priced = split.bytes_per_tick(N_VOXELS, Width::Bits32);
        assert_eq!(priced, 2 * 3 * u64::from(N_VOXELS) * 4);
        assert_eq!(priced, 50_331_648);

        // And the number actually paid, over the lane. `restore_lane` copies the
        // lane whole, ghost cell included (ADR-059), so this is what the loader
        // has to print: one element per copied lane more, twenty-four bytes at
        // this width, and the argument for taking it from the same rule as the
        // copy is ADR-057's own — two numbers computed by two rules is a report
        // that lies.
        const LANE_LEN: u32 = N_VOXELS + 1;
        let traffic = split.bytes_per_tick(LANE_LEN, Width::Bits32);
        assert_eq!(traffic, 2 * 3 * u64::from(LANE_LEN) * 4);
        assert_eq!(traffic - priced, 2 * 3 * 4);

        // The same three lanes at the other width cost twice as much, which is
        // the whole reason the parity of the *water* lane is the expensive one.
        assert_eq!(split.bytes_per_tick(LANE_LEN, Width::Bits64), 2 * traffic);
        assert_eq!(
            ParitySplit::from_substeps(&[2, 2]).bytes_per_tick(LANE_LEN, Width::Bits64),
            0
        );
    }

    /// `ACCEPTANCE.md`, section "Conservation", from ADR-057.
    #[test]
    fn restoration_copies_back_to_front_after_the_optional_swap() {
        // The direction is the one load-bearing thing in `restore_lane`, and no
        // conservation test in the project can see it: `front -> back` leaves the
        // minority lanes exactly one phase stale, and a stale state is conserved
        // no worse than a fresh one, so both domain sums stand still.
        //
        // Distinguishable values in *both* buffers, so that "the front holds
        // something plausible" is not enough to pass.
        let mut field = field();
        for lane in 0..LANES {
            for idx in 0..field.n_voxels() {
                let at = (lane * field.lane_len() + idx) as usize;
                field.write_mut()[at] = pattern(lane, idx);
            }
        }
        field.swap();
        for lane in 0..LANES {
            for idx in 0..field.n_voxels() {
                let at = (lane * field.lane_len() + idx) as usize;
                field.write_mut()[at] = -pattern(lane, idx);
            }
        }

        // `front` holds `pattern`, `back` holds `-pattern`, and the result of the
        // substeps is by construction the one in `back`.
        field.restore_lane(1);
        for idx in 0..field.n_voxels() {
            assert_eq!(
                field.lane(1)[idx as usize],
                -pattern(1, idx),
                "voxel {idx} of the restored lane holds what the phase started \
                 with, not what it produced"
            );
        }
        // And it reached past nobody: the other lanes are untouched.
        for lane in [0, 2] {
            for idx in 0..field.n_voxels() {
                assert_eq!(field.lane(lane)[idx as usize], pattern(lane, idx));
            }
        }
    }

    #[test]
    fn the_boundary_restoration_swaps_before_it_copies() {
        // The two branches of ADR-057 written out on one field, and the check is
        // that they agree: the same substep counts restored either way put the
        // same state in the front buffer. Only the number of lanes copied
        // differs, and that is `the_restoration_copies_the_smaller_parity_group`.
        let seed = |field: &mut Field32, sign: i32| {
            let n_voxels = field.n_voxels();
            let lane_len = field.lane_len();
            let lanes = field.lanes();
            let buffer = field.write_mut();
            for lane in 0..lanes {
                for idx in 0..n_voxels {
                    buffer[(lane * lane_len + idx) as usize] =
                        M32::new(sign * pattern(lane, idx).to_i64() as i32);
                }
            }
        };

        // Lane 0 ended odd (its state is in `back`), lanes 1 and 2 ended even.
        let mut field = field();
        seed(&mut field, 1);
        field.swap();
        seed(&mut field, -1);

        let split = ParitySplit::from_substeps(&[1, 2, 2]);
        assert!(!split.swaps());
        assert_eq!(split.lanes_to_copy(), &[0]);
        field.restore_boundary(&split);

        for idx in 0..field.n_voxels() {
            assert_eq!(field.lane(0)[idx as usize], -pattern(0, idx));
            assert_eq!(field.lane(1)[idx as usize], pattern(1, idx));
            assert_eq!(field.lane(2)[idx as usize], pattern(2, idx));
        }
    }

    #[test]
    #[should_panic(expected = "parity split")]
    fn a_split_built_for_another_field_is_refused() {
        // A split of the wrong length copies a set of lanes of plausible size
        // and leaves the rest a phase behind, which nothing downstream detects.
        let mut field = field();
        field.restore_boundary(&ParitySplit::from_substeps(&[1, 1]));
    }

    #[test]
    fn alternating_direction_leaves_the_pointers_alone() {
        // The property the whole scheme rests on: an even number of alternating
        // substeps is the identity on the buffers, so a lane taking two of them
        // is back where it started while its neighbour, which took one, is not
        // — and neither has dragged the other anywhere.
        let mut field = field();
        let n_voxels = field.n_voxels();
        let front_before = field.read().as_ptr();

        for step in 0..4u32 {
            let dir = if step % 2 == 0 {
                Direction::Forward
            } else {
                Direction::Backward
            };
            let (src, dst) = field.lane_pair_dir_mut(0, dir);
            for idx in 0..n_voxels as usize {
                dst[idx] = src[idx] + M32::new(1);
            }
        }

        assert!(
            core::ptr::eq(field.read().as_ptr(), front_before),
            "a substep run moved the field's pointers"
        );
        // Four increments, and after an even number of them the answer is in
        // `front`. A run that swapped instead would put it there too — what it
        // would also do is carry lanes 1 and 2 along, which is what
        // `mixed_substep_lanes_diffuse_as_if_each_were_alone` catches.
        for idx in 0..n_voxels {
            assert_eq!(field.lane(0)[idx as usize], M32::new(4));
        }
    }

    #[test]
    fn an_unaddressable_field_is_refused() {
        let grid = grid();
        assert!(Field::<M32>::new(&grid, 0).is_err());

        // 256^3 voxels is 2^24; 256 lanes of it is 2^32, one past the index.
        let big = Grid::new(256, 256, 256, [Boundary::Periodic; 6]).unwrap();
        let err = Field::<M32>::new(&big, 256).unwrap_err().to_string();
        assert!(err.contains("u32"), "unhelpful message: {err}");
    }
}
