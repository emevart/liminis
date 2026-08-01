//! Field buffers: flat, substance-major, double.
//!
//! A field is the thing a kernel is pointed at. It holds one or more quantities
//! over the same grid, laid out so that a transport kernel walks its own
//! substance contiguously (ADR-041), and it holds each of them twice, so that a
//! kernel never reads from the buffer it writes (ADR-034).

use anyhow::{Result, bail};

use super::grid::Grid;
use crate::numeric::{M32, M64};

/// A double-buffered field over a grid.
///
/// # Layout
///
/// One flat array per buffer, addressed **substance-major** (ADR-041):
///
/// ```text
/// buffer[lane * n_voxels + idx]
/// ```
///
/// with `idx = x + y*NX + z*NX*NY` inside a lane (SPEC section 1.1). A lane is
/// therefore a contiguous run of `n_voxels` entries, which is what
/// [`Field::lane_pair_mut`] hands to a transport kernel: from inside such a
/// kernel the lane is the whole world and the index is a plain `idx`, exactly
/// the `src: &[M]` of the skeleton in `ARCHITECTURE.md`.
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
    /// Returns an error if `lanes` is zero, or if `lanes * n_voxels` does not
    /// fit a `u32`. The second bound is not about this crate's `Vec`s, which are
    /// indexed by `usize`: it is about the kernels, which compute
    /// `lane * n_voxels + idx` in `u32` because that is what WGSL gives them
    /// (ADR-041). A buffer that only a 64-bit host can address is a buffer the
    /// GPU path cannot use.
    pub fn new(grid: &Grid, lanes: u32) -> Result<Self> {
        if lanes == 0 {
            bail!("a field with no lanes holds nothing");
        }
        let n_voxels = grid.n_voxels();
        let Some(total) = lanes.checked_mul(n_voxels) else {
            bail!(
                "a field of {lanes} lanes over {n_voxels} voxels is longer than a \
                 u32 index can address, and kernels address it in u32 (ADR-041)"
            );
        };

        Ok(Self {
            lanes,
            n_voxels,
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

    /// State `N`: the whole flat buffer, every lane, `lane * n_voxels + idx`.
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

    /// State `N` of one lane: `n_voxels` entries, indexed by plain `idx`.
    #[inline]
    #[must_use]
    pub fn lane(&self, lane: u32) -> &[T] {
        &self.front[self.lane_range(lane)]
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

    /// Exchange the buffers: state `N+1` becomes state `N`.
    ///
    /// Constant time — the two `Vec`s trade pointers. Read the note on
    /// [`Field`] about what the caller owes before calling this: the buffer
    /// being promoted has to have been written in full.
    ///
    /// The exchange is for the **whole field**: every lane advances together.
    // TODO(swap-granularity): the skeleton in `ARCHITECTURE.md` writes
    // `world.swap(s)`, one substance at a time, and this exchange cannot serve
    // that call without a decision nobody has taken.
    //
    // The two are not the same operation, and the difference is forced by
    // ADR-030: substances get different substep counts from the same tick — six
    // for the proton, two for oxygen, one for most — so after a diffusion phase
    // an odd-substep lane and an even-substep one have changed buffers a
    // different number of times. Advancing lanes independently over one shared
    // pair of buffers costs one of two things, and neither is free:
    //
    // - a parity bit per lane, so a lane knows which buffer holds its state N.
    //   O(1), but there is then no single flat array holding state N for every
    //   substance at once, which is precisely what the reaction kernel of
    //   ADR-041 is handed;
    // - a copy of the lane between the buffers on every advance. Keeps the flat
    //   array coherent, and pays a full buffer round trip per substep on the
    //   hottest loop in the tick — on the GPU, per dispatch.
    //
    // A process whose substances all take the same number of substeps needs
    // neither: it writes every lane and swaps once, which is what this method
    // does and what the first kernel needs. Beyond that it is a question for the
    // journal, not for this file.
    #[inline]
    pub fn swap(&mut self) {
        core::mem::swap(&mut self.front, &mut self.back);
    }

    /// Where a lane lives in a buffer. The one place the layout formula is
    /// written down.
    #[inline]
    fn lane_range(&self, lane: u32) -> core::ops::Range<usize> {
        debug_assert!(
            lane < self.lanes,
            "lane {lane} of a field with {} lanes",
            self.lanes
        );
        // No overflow: `lanes * n_voxels` was checked to fit a u32 at
        // construction, and `lane < lanes`.
        let start = (lane * self.n_voxels) as usize;
        start..start + self.n_voxels as usize
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
        let lanes = field.lanes();
        let buffer = field.write_mut();
        for lane in 0..lanes {
            for idx in 0..n_voxels {
                buffer[(lane * n_voxels + idx) as usize] = pattern(lane, idx);
            }
        }
    }

    #[test]
    fn a_field_starts_zeroed_and_the_right_size() {
        let field = field();
        assert_eq!(field.lanes(), LANES);
        assert_eq!(field.n_voxels(), NX * NY * NZ);
        assert_eq!(field.read().len(), (LANES * NX * NY * NZ) as usize);
        assert!(field.read().iter().all(|&v| v == M32::ZERO));
        assert_eq!(field.lane(0).len(), (NX * NY * NZ) as usize);
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
                    field.read()[(lane * field.n_voxels() + idx) as usize],
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

        let (src, dst) = field.lane_pair_mut(1);
        assert!(!core::ptr::eq(src.as_ptr(), dst.as_ptr()));
        assert_eq!(src.len(), (NX * NY * NZ) as usize);
        assert_eq!(dst.len(), (NX * NY * NZ) as usize);
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
                    field.read()[(lane * field.n_voxels() + idx) as usize]
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
