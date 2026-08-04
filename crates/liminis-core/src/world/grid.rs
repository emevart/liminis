//! The grid: one linear index, six faces, one boundary condition per face.
//!
//! Everything here is arithmetic over `u32`. There is no cell type, no
//! iterator and no storage — a grid says where a voxel is and who its
//! neighbours are, and that is all the transport kernels need to know about
//! space (SPEC section 1.1, section 1.6).

use anyhow::{Result, bail};

/// One of the three axes.
///
/// Named because a boundary condition is a property of a face while
/// periodicity is a property of an axis, and the difference is what
/// [`Grid::new`] checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Axis {
    X,
    Y,
    Z,
}

/// One of the six faces of a voxel.
///
/// The discriminants are the numbers the kernel will loop over. The skeleton in
/// `ARCHITECTURE.md` writes
///
/// ```text
/// for face in 0..6u32 {
///     let n = neighbour(p, idx, face);
///     net = net + flux(here, src[n as usize], p.alpha);
/// }
/// ```
///
/// and in WGSL that loop is over a plain integer, because WGSL has no enums. So
/// the mapping from integer to face is fixed here, `#[repr(u32)]`, and pinned by
/// a test: whoever transcribes the kernel reads the numbers off this type rather
/// than inventing an order.
///
/// The order itself carries no meaning. A voxel sums the flux over all six
/// faces, the summands are integers, and integer addition is associative and
/// commutative — so the traversal order of the faces cannot change the result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum Face {
    XMinus = 0,
    XPlus = 1,
    YMinus = 2,
    YPlus = 3,
    ZMinus = 4,
    ZPlus = 5,
}

impl Face {
    /// All six, in discriminant order. The host-side spelling of
    /// `for face in 0..6u32`.
    pub const ALL: [Face; 6] = [
        Face::XMinus,
        Face::XPlus,
        Face::YMinus,
        Face::YPlus,
        Face::ZMinus,
        Face::ZPlus,
    ];

    /// The face pointing the other way along the same axis.
    ///
    /// Conservation in gather form rests on faces pairing up: if `A` gathers
    /// flux across face `f` from `B`, then `B` must gather the exactly opposite
    /// flux across `f.opposite()` from `A` (ADR-034). This function is what
    /// states that pairing, and `face_pairing_is_mutual` is what checks it.
    #[inline]
    #[must_use]
    pub fn opposite(self) -> Face {
        match self {
            Face::XMinus => Face::XPlus,
            Face::XPlus => Face::XMinus,
            Face::YMinus => Face::YPlus,
            Face::YPlus => Face::YMinus,
            Face::ZMinus => Face::ZPlus,
            Face::ZPlus => Face::ZMinus,
        }
    }

    /// The axis this face lies on.
    #[inline]
    #[must_use]
    pub fn axis(self) -> Axis {
        match self {
            Face::XMinus | Face::XPlus => Axis::X,
            Face::YMinus | Face::YPlus => Axis::Y,
            Face::ZMinus | Face::ZPlus => Axis::Z,
        }
    }
}

/// What happens at a face of the domain (SPEC section 1.6).
///
/// No `Default` on purpose. The defaults are per face and they disagree with
/// each other — `periodic` on X and Y, `closed` on `z_min`, `exchange` on
/// `z_max` (`CONFIG_SCHEMA.md` section 4) — so a single default value here would
/// be a wrong answer for four of the six faces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Boundary {
    /// The domain wraps: the last voxel of the axis is the neighbour of the
    /// first. A torus, and the default on X and Y.
    Periodic,
    /// Nothing crosses this face. Solid floor, geothermal inlet, the substrate a
    /// biofilm grows on — the default on `z_min`.
    Closed,
    /// The face exchanges with an outside reservoir through the named channel
    /// `BOUNDARY_EXCHANGE` (SPEC section 7) — the default on `z_max`.
    ///
    /// The neighbour across it is the **ghost cell** [`Grid::ghost_index`],
    /// which is the last element of a field lane and holds the reservoir in
    /// storage units (ADR-059). That is the same trick by which `closed`
    /// returns the voxel itself: the boundary condition is folded on the host
    /// into an *address*, so no transport kernel ever learns that a `Boundary`
    /// exists.
    Exchange,
}

/// A regular, isotropic, dense grid: extents, and a boundary condition per face.
///
/// `Copy`, and nothing but scalars, because that is what a kernel parameter
/// struct is allowed to be (ADR-034). The host folds these numbers into whatever
/// `Params` a kernel takes; a kernel never holds a `Grid`, because `kernels/`
/// depends on `numeric/` and on nothing else (`ARCHITECTURE.md`).
///
/// That last point has a consequence worth saying out loud, because it looks
/// like an oversight: the kernel will carry its **own** copy of [`Grid::index`]
/// and [`Grid::neighbour`], written against its own `Params` and transcribable
/// into WGSL line by line — exactly as the skeleton in `ARCHITECTURE.md` shows.
/// This type is the host-side authority the copy has to agree with, and the way
/// to keep the two from drifting is a test that runs both, which lives with the
/// kernel because a test may depend on everything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grid {
    nx: u32,
    ny: u32,
    nz: u32,
    /// Precomputed `nx * ny * nz`, validated to fit a `u32` at construction.
    n_voxels: u32,
    /// Indexed by `Face as usize`.
    boundary: [Boundary; 6],
}

impl Grid {
    /// Build a grid, checking what the neighbourhood needs in order to conserve.
    ///
    /// `boundary` is indexed by [`Face`]: `[x_min, x_max, y_min, y_max, z_min,
    /// z_max]`.
    ///
    /// Three things are refused here.
    ///
    /// **An empty or unaddressable grid.** Every extent must be at least one
    /// voxel, and `nx * ny * nz` must fit a `u32`, because the linear index is a
    /// `u32` everywhere in the system (`QUANTITIES.md` section 1) and on the GPU
    /// it is a `global_invocation_id`.
    ///
    /// **Periodicity on one face of an axis but not the other.** The config
    /// schema states the rule (`CONFIG_SCHEMA.md` section 4) without saying what
    /// goes wrong; what goes wrong is conservation. Take `x_min = periodic` with
    /// `x_max = closed`: voxel `(0, y, z)` gathers flux across its `-X` face from
    /// voxel `(nx-1, y, z)`, but that voxel's `+X` face is closed and gathers
    /// nothing back. The two halves of one face pairing disagree, mass appears at
    /// one end of the axis and vanishes at the other, and no test of the flux
    /// function itself would see it — antisymmetry holds perfectly, it is the
    /// pairing that is broken.
    ///
    /// **An `exchange` face opposite a `periodic` one on the same axis.** The
    /// same pairing argument, one step further out: the periodic half has a
    /// neighbour past the face and the exchanging half has the ghost cell, so
    /// the two ends of the axis are integrated by different neighbourhoods.
    /// `exchange` opposite `closed`, and `exchange` on both faces, are legal —
    /// there the pairing is simply absent, and ADR-059 gives the absent half a
    /// name: the channel counter (see [`Grid::neighbour`]).
    ///
    /// # Errors
    ///
    /// Returns an error in each of the three cases above, naming the face or the
    /// axis at fault.
    pub fn new(nx: u32, ny: u32, nz: u32, boundary: [Boundary; 6]) -> Result<Self> {
        for (extent, axis) in [(nx, Axis::X), (ny, Axis::Y), (nz, Axis::Z)] {
            if extent == 0 {
                bail!("grid axis {axis:?} has no voxels");
            }
        }

        let Some(n_voxels) = nx.checked_mul(ny).and_then(|xy| xy.checked_mul(nz)) else {
            bail!(
                "grid {nx}x{ny}x{nz} has more voxels than a u32 linear index can \
                 address (QUANTITIES.md section 1)"
            );
        };

        for (axis, min, max) in [
            (Axis::X, Face::XMinus, Face::XPlus),
            (Axis::Y, Face::YMinus, Face::YPlus),
            (Axis::Z, Face::ZMinus, Face::ZPlus),
        ] {
            let min_is_periodic = boundary[min as usize] == Boundary::Periodic;
            let max_is_periodic = boundary[max as usize] == Boundary::Periodic;
            if min_is_periodic != max_is_periodic {
                // One message for both shapes of the fault, because they are one
                // fault: `periodic` against `closed` leaves the wrapping half of
                // the pairing gathering from a neighbour that gathers nothing
                // back, and `periodic` against `exchange` leaves it gathering
                // from a voxel whose opposite face reads the ghost cell instead.
                bail!(
                    "axis {axis:?} is periodic on one face and not on the other \
                     ({:?} / {:?}): periodicity is a property of an axis, and a \
                     half-periodic axis breaks the face pairing that gather-form \
                     conservation rests on (ADR-034, ADR-059, CONFIG_SCHEMA.md \
                     section 4). `exchange` opposite `closed` is legal — there \
                     the pairing is absent rather than broken, and its second \
                     half is the BOUNDARY_EXCHANGE counter",
                    boundary[min as usize],
                    boundary[max as usize]
                );
            }
        }

        // The ghost cell is one index past the last voxel, so a grid that fills
        // a `u32` exactly has nowhere to put it (ADR-059).
        if n_voxels == u32::MAX {
            bail!(
                "grid {nx}x{ny}x{nz} fills a u32 linear index exactly, leaving no \
                 room for the ghost cell at index n_voxels (ADR-059)"
            );
        }

        Ok(Self {
            nx,
            ny,
            nz,
            n_voxels,
            boundary,
        })
    }

    /// Voxels along X.
    #[inline]
    #[must_use]
    pub fn nx(&self) -> u32 {
        self.nx
    }

    /// Voxels along Y.
    #[inline]
    #[must_use]
    pub fn ny(&self) -> u32 {
        self.ny
    }

    /// Voxels along Z.
    #[inline]
    #[must_use]
    pub fn nz(&self) -> u32 {
        self.nz
    }

    /// `nx * ny * nz`, the length of one lane of a field buffer.
    #[inline]
    #[must_use]
    pub fn n_voxels(&self) -> u32 {
        self.n_voxels
    }

    /// The address of the ghost cell: `n_voxels`, one past the last voxel.
    ///
    /// **One ghost per lane, shared by every `exchange` face of the grid**,
    /// because `[boundary.reservoir]` is one section (ADR-059,
    /// `CONFIG_SCHEMA.md` section 4). Six exchanging faces therefore trade with
    /// one reservoir, at one composition and one temperature.
    #[inline]
    #[must_use]
    pub fn ghost_index(&self) -> u32 {
        self.n_voxels
    }

    /// The length of one lane of a field buffer: `n_voxels + 1` (ADR-059).
    ///
    /// **The one place the length is written down.** A lane is the voxels
    /// followed by the ghost cell, so it is `n_voxels` that is the exception
    /// now: everything that walks the domain — a ledger reduction, an
    /// undershoot bound, an export — takes `n_voxels`, and everything that
    /// hands a slice to a kernel takes this.
    #[inline]
    #[must_use]
    pub fn lane_len(&self) -> u32 {
        // Checked at construction: `n_voxels < u32::MAX`.
        self.n_voxels + 1
    }

    /// The boundary condition on one face of the domain.
    #[inline]
    #[must_use]
    pub fn boundary(&self, face: Face) -> Boundary {
        self.boundary[face as usize]
    }

    /// Bit `f` set: face `f` exchanges with the reservoir.
    ///
    /// The same encoding as the periodic mask a transport process folds — the
    /// bit index is the discriminant of [`Face`] — and deliberately the same
    /// *shape*, because ADR-059 rejected the alternative by name: a kernel that
    /// received a table of boundary conditions would have learned about the
    /// config. Two bit masks say everything the lookup needs and say it in the
    /// one form WGSL indexes cheaply.
    #[inline]
    #[must_use]
    pub fn exchange_mask(&self) -> u32 {
        let mut mask = 0u32;
        for face in Face::ALL {
            if self.boundary[face as usize] == Boundary::Exchange {
                mask |= 1 << (face as u32);
            }
        }
        mask
    }

    /// Whether any face of this grid vents.
    ///
    /// What a process's declared invariant turns on: a grid with no exchanging
    /// face conserves matter outright, and one with an exchanging face conserves
    /// it only through `BOUNDARY_EXCHANGE` (ADR-028, ADR-059).
    #[inline]
    #[must_use]
    pub fn has_exchange(&self) -> bool {
        self.exchange_mask() != 0
    }

    /// How many of a voxel's six faces carry flux at all.
    ///
    /// The `f` of the undershoot bound of ADR-068,
    /// `min(src) - floor(f/2) - 3*ceil(spread/2^24)`, and it counts **open**
    /// faces rather than un-closed ones. A face is open when its neighbour is
    /// somebody else: a closed wall folds onto the voxel and carries nothing, a
    /// degenerate axis folds onto it for a different reason and also carries
    /// nothing, and an `exchange` face reaches the ghost cell and is therefore
    /// open like any other.
    ///
    /// Getting that last one wrong is silent. A bound written from the count of
    /// *closed* faces is one unit too tight on the plane of exchange, and only
    /// on that plane, on a grid whose other tests are all about closed lids.
    #[inline]
    #[must_use]
    pub fn open_faces(&self, idx: u32) -> u32 {
        Face::ALL
            .into_iter()
            .filter(|&face| self.neighbour(idx, face) != idx)
            .count() as u32
    }

    /// The linear index of a voxel: `x + y*NX + z*NX*NY` (SPEC section 1.1).
    ///
    /// X varies fastest, so a stencil along X walks memory in order. That is the
    /// whole reason the formula is written this way round, and it is why the
    /// natural traversal — the plain `0..n_voxels` a kernel dispatch does — comes
    /// out as `z` outermost and `x` innermost, exactly the order SPEC asks for.
    /// There is no iterator in this module for that reason: `0..n_voxels` *is*
    /// the traversal, and a second way to spell it could only disagree.
    #[inline]
    #[must_use]
    pub fn index(&self, x: u32, y: u32, z: u32) -> u32 {
        debug_assert!(
            x < self.nx && y < self.ny && z < self.nz,
            "({x}, {y}, {z}) is outside a {}x{}x{} grid",
            self.nx,
            self.ny,
            self.nz
        );
        x + y * self.nx + z * self.nx * self.ny
    }

    /// The coordinates of a linear index. The inverse of [`Grid::index`].
    ///
    /// Needed wherever the axes have to be told apart — the boundary conditions
    /// here, and the LOD coarsening of SPEC section 1.5, which shifts each
    /// coordinate separately because shifting the linear index instead would mix
    /// the bit fields of X, Y and Z together.
    #[inline]
    #[must_use]
    pub fn coords(&self, idx: u32) -> (u32, u32, u32) {
        debug_assert!(
            idx < self.n_voxels,
            "index {idx} is outside a grid of {} voxels",
            self.n_voxels
        );
        let plane = self.nx * self.ny;
        let z = idx / plane;
        let within_plane = idx - z * plane;
        let y = within_plane / self.nx;
        let x = within_plane - y * self.nx;
        (x, y, z)
    }

    /// The neighbour of a voxel across one of its six faces.
    ///
    /// **A closed face returns the voxel itself**, and that is not a sentinel
    /// standing in for "no neighbour" — it is the answer that makes the face
    /// carry no flux without a single branch in the kernel.
    ///
    /// The argument is one line. A flux function in gather form is antisymmetric,
    /// `f(a, b) == -f(b, a)` (ADR-034), and an antisymmetric function of two
    /// equal arguments is zero: `f(a, a) == -f(a, a)`, so `f(a, a) == 0`. Reading
    /// the cell's own value across a closed face therefore contributes exactly
    /// zero to the sum, for *any* flux function that already obeys the property
    /// the whole scheme rests on. An `Option` would have bought the same zero at
    /// the price of a branch that WGSL has no way to express.
    ///
    /// One thing the trick does not cover, and it is worth knowing before
    /// advection arrives: a *directional* flux need not vanish on equal
    /// arguments. Diffusion's `alpha * (b - a)` does. An upwind advective flux
    /// carries `v * a` regardless of what the neighbour holds, so a closed face
    /// there needs its face velocity to be zero — a property of the velocity
    /// field, which this lookup neither knows nor promises.
    ///
    /// A degenerate axis behaves the same way for a different reason: an extent
    /// of one voxel is its own neighbour in both directions even when periodic,
    /// because the torus closes onto itself. So `neighbour(idx, f) == idx` means
    /// "this face carries nothing", not "this face is closed".
    ///
    /// **An `exchange` face returns [`Grid::ghost_index`]**, by the same trick
    /// read the other way: the boundary condition is an address (ADR-059). The
    /// flux across it is the ordinary flux of the ordinary kernel, against a
    /// cell that holds the reservoir.
    ///
    /// What that breaks is not the antisymmetry of the flux — `f(a, b) ==
    /// -f(b, a)` is a property of a function and goes on holding — but the
    /// **pairing**: the ghost cell is nobody's voxel, so no second cell gathers
    /// the opposite. ADR-059 names the second participant: the
    /// `BOUNDARY_EXCHANGE` counter takes what the voxel gained, negated, and the
    /// sum over the domain plus the counter is identically zero. That is why
    /// `face_pairing_is_mutual` walks `0..n_voxels` and skips a face whose
    /// neighbour is the ghost: there is nothing there to pair with, by
    /// construction rather than by oversight.
    ///
    /// **The ghost cell is its own neighbour across all six faces**, and the
    /// early return that makes it so is load-bearing rather than tidy. Falling
    /// through to [`Grid::coords`] would decode `n_voxels` as a coordinate one
    /// past the end of Z; the `debug_assert!` there catches it in a debug build
    /// and vanishes in release, where [`Grid::index`] folds the out-of-range
    /// coordinate back into a number that depends on the extents — sometimes the
    /// ghost, sometimes a voxel in the middle of the domain. A neighbourhood
    /// that depends on the shape of the grid is a defect that does not reproduce
    /// everywhere.
    ///
    /// # Panics
    ///
    /// In debug builds, if `idx` is neither a voxel nor the ghost cell.
    #[inline]
    #[must_use]
    pub fn neighbour(&self, idx: u32, face: Face) -> u32 {
        if idx == self.n_voxels {
            return idx;
        }
        let (x, y, z) = self.coords(idx);
        let boundary = self.boundary[face as usize];
        let (coord, extent) = match face.axis() {
            Axis::X => (x, self.nx),
            Axis::Y => (y, self.ny),
            Axis::Z => (z, self.nz),
        };
        // Whether this voxel is the one standing against the face. The
        // comparison is on the coordinate and not on the result of a step,
        // because "there is no neighbour" and "the neighbour is me" are the same
        // number across a closed face and only one of them reaches the ghost.
        let against_the_face = match face {
            Face::XMinus | Face::YMinus | Face::ZMinus => coord == 0,
            Face::XPlus | Face::YPlus | Face::ZPlus => coord + 1 == extent,
        };
        if against_the_face && boundary == Boundary::Exchange {
            return self.ghost_index();
        }

        let periodic = boundary == Boundary::Periodic;
        match face {
            Face::XMinus => self.index(step_down(x, self.nx, periodic), y, z),
            Face::XPlus => self.index(step_up(x, self.nx, periodic), y, z),
            Face::YMinus => self.index(x, step_down(y, self.ny, periodic), z),
            Face::YPlus => self.index(x, step_up(y, self.ny, periodic), z),
            Face::ZMinus => self.index(x, y, step_down(z, self.nz, periodic)),
            Face::ZPlus => self.index(x, y, step_up(z, self.nz, periodic)),
        }
    }
}

/// One step down an axis, or what the boundary says instead.
///
/// Written with a comparison rather than a modulo: `(coord + extent - 1) %
/// extent` is the same answer and one integer division, which on a GPU is the
/// expensive instruction in the whole neighbourhood lookup.
///
/// A `bool` rather than a [`Boundary`], and it is the third condition that made
/// it one: `exchange` does not answer with a coordinate at all — its answer is
/// the ghost cell, which has no `(x, y, z)` — so it is resolved by
/// [`Grid::neighbour`] before this is reached. Two conditions are left here, and
/// they are exactly the two the kernels' own copies carry (`kernels/diffuse.rs`).
#[inline]
fn step_down(coord: u32, extent: u32, periodic: bool) -> u32 {
    if coord > 0 {
        coord - 1
    } else if periodic {
        extent - 1
    } else {
        coord
    }
}

/// One step up an axis, or what the boundary says instead.
#[inline]
fn step_up(coord: u32, extent: u32, periodic: bool) -> u32 {
    if coord + 1 < extent {
        coord + 1
    } else if periodic {
        0
    } else {
        coord
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::numeric::{M32, Q, q_conc_32, q_round_32, qmul};

    /// A deliberately non-cubic grid. A cubic one hides every bug that mixes up
    /// the axes, because all three strides are equal.
    const NX: u32 = 3;
    const NY: u32 = 4;
    const NZ: u32 = 5;

    fn periodic() -> Grid {
        Grid::new(NX, NY, NZ, [Boundary::Periodic; 6]).unwrap()
    }

    /// Periodic in X and Y, solid floor and solid lid in Z. Not the eco-regime
    /// default — that one vents, and it is [`vented`] below.
    fn floored() -> Grid {
        Grid::new(
            NX,
            NY,
            NZ,
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

    #[test]
    fn the_face_numbering_is_the_one_the_kernel_will_loop_over() {
        // The kernel writes `for face in 0..6u32` and WGSL has no enums, so
        // these numbers are an interface, not an implementation detail.
        assert_eq!(Face::XMinus as u32, 0);
        assert_eq!(Face::XPlus as u32, 1);
        assert_eq!(Face::YMinus as u32, 2);
        assert_eq!(Face::YPlus as u32, 3);
        assert_eq!(Face::ZMinus as u32, 4);
        assert_eq!(Face::ZPlus as u32, 5);

        for (i, face) in Face::ALL.into_iter().enumerate() {
            assert_eq!(face as usize, i);
            assert_eq!(face.opposite().opposite(), face);
            assert_ne!(face.opposite(), face);
            assert_eq!(face.opposite().axis(), face.axis());
        }
    }

    #[test]
    fn index_and_coords_are_inverse() {
        let grid = periodic();
        assert_eq!(grid.n_voxels(), NX * NY * NZ);

        for idx in 0..grid.n_voxels() {
            let (x, y, z) = grid.coords(idx);
            assert!(x < NX && y < NY && z < NZ, "{idx} decoded out of range");
            assert_eq!(grid.index(x, y, z), idx);
        }

        for z in 0..NZ {
            for y in 0..NY {
                for x in 0..NX {
                    assert_eq!(grid.coords(grid.index(x, y, z)), (x, y, z));
                }
            }
        }
    }

    #[test]
    fn the_linear_order_is_z_then_y_then_x() {
        // SPEC section 1.1: traversal in the order z -> y -> x, X varying
        // fastest. The claim of this module is that the plain `0..n_voxels` of a
        // kernel dispatch already *is* that traversal, so this test walks the
        // nested loops and expects consecutive indices.
        let grid = periodic();
        let mut expected = 0u32;
        for z in 0..NZ {
            for y in 0..NY {
                for x in 0..NX {
                    assert_eq!(grid.index(x, y, z), expected);
                    expected += 1;
                }
            }
        }
        assert_eq!(expected, grid.n_voxels());

        // And the same statement from the other side: stepping one index along
        // moves one voxel along X, which is what makes an X stencil sequential
        // in memory.
        assert_eq!(grid.index(1, 0, 0), grid.index(0, 0, 0) + 1);
        assert_eq!(grid.index(0, 1, 0), grid.index(0, 0, 0) + NX);
        assert_eq!(grid.index(0, 0, 1), grid.index(0, 0, 0) + NX * NY);
    }

    #[test]
    fn a_periodic_face_wraps_on_each_axis() {
        let grid = periodic();

        // X, at both ends.
        assert_eq!(
            grid.neighbour(grid.index(0, 2, 3), Face::XMinus),
            grid.index(NX - 1, 2, 3)
        );
        assert_eq!(
            grid.neighbour(grid.index(NX - 1, 2, 3), Face::XPlus),
            grid.index(0, 2, 3)
        );

        // Y.
        assert_eq!(
            grid.neighbour(grid.index(1, 0, 3), Face::YMinus),
            grid.index(1, NY - 1, 3)
        );
        assert_eq!(
            grid.neighbour(grid.index(1, NY - 1, 3), Face::YPlus),
            grid.index(1, 0, 3)
        );

        // Z.
        assert_eq!(
            grid.neighbour(grid.index(1, 2, 0), Face::ZMinus),
            grid.index(1, 2, NZ - 1)
        );
        assert_eq!(
            grid.neighbour(grid.index(1, 2, NZ - 1), Face::ZPlus),
            grid.index(1, 2, 0)
        );

        // The interior does not wrap, and the step is the one the axis owns:
        // this is where a lookup that shifted the linear index instead of the
        // coordinate would come apart.
        let here = grid.index(1, 2, 3);
        assert_eq!(grid.neighbour(here, Face::XPlus), grid.index(2, 2, 3));
        assert_eq!(grid.neighbour(here, Face::YPlus), grid.index(1, 3, 3));
        assert_eq!(grid.neighbour(here, Face::ZPlus), grid.index(1, 2, 4));
        assert_eq!(grid.neighbour(here, Face::XMinus), grid.index(0, 2, 3));
        assert_eq!(grid.neighbour(here, Face::YMinus), grid.index(1, 1, 3));
        assert_eq!(grid.neighbour(here, Face::ZMinus), grid.index(1, 2, 2));
    }

    #[test]
    fn a_closed_face_returns_the_cell_itself() {
        let grid = floored();

        for y in 0..NY {
            for x in 0..NX {
                let floor = grid.index(x, y, 0);
                let lid = grid.index(x, y, NZ - 1);
                assert_eq!(grid.neighbour(floor, Face::ZMinus), floor);
                assert_eq!(grid.neighbour(lid, Face::ZPlus), lid);
            }
        }

        // The closed axis does not leak into the periodic ones.
        let floor = grid.index(0, 0, 0);
        assert_eq!(
            grid.neighbour(floor, Face::XMinus),
            grid.index(NX - 1, 0, 0)
        );
        assert_eq!(
            grid.neighbour(floor, Face::YMinus),
            grid.index(0, NY - 1, 0)
        );
        assert_eq!(grid.neighbour(floor, Face::ZPlus), grid.index(0, 0, 1));
    }

    #[test]
    fn a_closed_face_carries_no_flux() {
        // The point of returning the cell itself. This is the diffusive flux of
        // the skeleton in ARCHITECTURE.md, written through the public numeric
        // API: alpha * (there - here), rounded by the one rule of the system.
        let flux = |here: M32, there: M32, alpha: Q| {
            let conc_per_unit = Q::from_f64(1.0);
            q_round_32(qmul(q_conc_32(there - here, conc_per_unit), alpha))
        };

        // Antisymmetric, which is what makes the argument work at all.
        let alpha = Q::from_f64(0.125);
        for a in [-97i32, -1, 0, 1, 1000] {
            for b in [-97i32, -1, 0, 1, 1000] {
                let ab = flux(M32::new(a), M32::new(b), alpha);
                let ba = flux(M32::new(b), M32::new(a), alpha);
                assert_eq!(ab, -ba, "flux is not antisymmetric at ({a}, {b})");
            }
        }

        // So reading one's own value across a closed face contributes nothing,
        // at any amount and any alpha, with no branch anywhere.
        let grid = floored();
        let amount = M32::new(123_456);
        let floor = grid.index(2, 1, 0);
        let neighbour = grid.neighbour(floor, Face::ZMinus);
        assert_eq!(neighbour, floor);
        assert_eq!(flux(amount, amount, alpha), M32::ZERO);
    }

    #[test]
    fn face_pairing_is_mutual() {
        // The structural half of conservation. Antisymmetry of the flux says the
        // two ends of a face pairing cancel; this says the pairing exists — that
        // if `here` gathers from `there` across `f`, then `there` gathers from
        // `here` across the opposite face. Break this and mass moves without
        // anything in the flux function looking wrong.
        //
        // `0..n_voxels`, so the ghost cell is not walked, and a face that
        // reaches it is skipped: the face of the domain has no second voxel and
        // therefore no pairing at all. That is not a hole — ADR-059 names the
        // second participant, and it is the `BOUNDARY_EXCHANGE` counter, which
        // this file cannot see.
        for grid in [periodic(), floored(), vented()] {
            for idx in 0..grid.n_voxels() {
                for face in Face::ALL {
                    let there = grid.neighbour(idx, face);
                    if there == idx || there == grid.ghost_index() {
                        // A closed face, an axis one voxel deep, or the face of
                        // the domain. None of the three has a voxel on the far
                        // side to pair with.
                        continue;
                    }
                    assert_eq!(
                        grid.neighbour(there, face.opposite()),
                        idx,
                        "face {face:?} of voxel {idx} is not paired"
                    );
                }
            }
        }
    }

    #[test]
    fn an_axis_one_voxel_deep_is_its_own_neighbour() {
        // Legal, and the flat case a two-dimensional scenario would use. Both
        // faces of the degenerate axis fold onto the cell, so both carry zero
        // flux, which is the physically right answer: there is nothing to
        // exchange with.
        let grid = Grid::new(4, 4, 1, [Boundary::Periodic; 6]).unwrap();
        for idx in 0..grid.n_voxels() {
            assert_eq!(grid.neighbour(idx, Face::ZMinus), idx);
            assert_eq!(grid.neighbour(idx, Face::ZPlus), idx);
        }

        let single = Grid::new(1, 1, 1, [Boundary::Periodic; 6]).unwrap();
        for face in Face::ALL {
            assert_eq!(single.neighbour(0, face), 0);
        }
    }

    /// The eco-regime default of SPEC section 1.6 in full: periodic in X and Y,
    /// a solid floor, and a lid that vents (`CONFIG_SCHEMA.md` section 4).
    fn vented() -> Grid {
        Grid::new(
            NX,
            NY,
            NZ,
            [
                Boundary::Periodic,
                Boundary::Periodic,
                Boundary::Periodic,
                Boundary::Periodic,
                Boundary::Closed,
                Boundary::Exchange,
            ],
        )
        .unwrap()
    }

    /// `ACCEPTANCE.md`, from ADR-059.
    #[test]
    fn an_exchange_face_returns_the_ghost_and_the_ghost_is_its_own_neighbour() {
        let grid = vented();
        let ghost = grid.ghost_index();
        assert_eq!(ghost, NX * NY * NZ);
        assert_eq!(grid.lane_len(), ghost + 1);
        assert!(grid.has_exchange());
        assert_eq!(grid.exchange_mask(), 1 << (Face::ZPlus as u32));

        // Every voxel of the top layer reaches the ghost across `+Z`, and no
        // interior voxel reaches it across anything.
        for idx in 0..grid.n_voxels() {
            let (_, _, z) = grid.coords(idx);
            for face in Face::ALL {
                let there = grid.neighbour(idx, face);
                let expected = face == Face::ZPlus && z == NZ - 1;
                assert_eq!(
                    there == ghost,
                    expected,
                    "voxel {idx} at z = {z}, face {face:?}"
                );
                assert!(there <= ghost, "voxel {idx}, face {face:?} left the lane");
            }
        }

        // And the ghost is its own neighbour across all six. The early return
        // that makes it so is the whole of this assertion: without it `coords`
        // decodes a coordinate one past the end of Z, and in release the answer
        // depends on the extents.
        for face in Face::ALL {
            assert_eq!(grid.neighbour(ghost, face), ghost, "ghost across {face:?}");
        }
    }

    /// `ACCEPTANCE.md`, from ADR-059 and ADR-068.
    #[test]
    fn the_undershoot_bound_counts_the_exchange_face_as_open() {
        // `f` in `min(src) - floor(f/2) - 3*ceil(spread/2^24)` is the number of
        // faces that carry flux, and the face of exchange carries flux. A bound
        // written from the count of *closed* faces is one unit too tight on the
        // plane of exchange, and `a_voxel_on_a_closed_boundary_stays_within_the_
        // five_face_bound` cannot see it: that name is about a closed lid.
        let vented = vented();
        let sealed = floored();
        for y in 0..NY {
            for x in 0..NX {
                let lid = vented.index(x, y, NZ - 1);
                assert_eq!(vented.open_faces(lid), 6, "the vented lid at ({x}, {y})");
                assert_eq!(sealed.open_faces(lid), 5, "the sealed lid at ({x}, {y})");
                assert_eq!(vented.open_faces(vented.index(x, y, 0)), 5, "the floor");
            }
        }
        // The interior is six either way, so the fixture is not asserting that
        // every voxel has six faces.
        assert_eq!(vented.open_faces(vented.index(1, 2, 2)), 6);
        assert_eq!(sealed.open_faces(sealed.index(1, 2, 2)), 6);

        // The bound itself, as ADR-068 writes it, over a pool far below 2^24 so
        // that the `f32` term is its minimum of three.
        let allowance = |faces: u32| i64::from(faces / 2) + 3;
        assert_eq!(allowance(vented.open_faces(vented.index(0, 0, NZ - 1))), 6);
        assert_eq!(allowance(sealed.open_faces(sealed.index(0, 0, NZ - 1))), 5);
    }

    /// `ACCEPTANCE.md`, from ADR-059.
    #[test]
    fn an_exchange_axis_paired_with_periodic_is_refused() {
        // The half-periodic argument, one step further out. On the periodic half
        // the neighbour past the face exists; on the exchanging half it is the
        // ghost. The two ends of the axis are then integrated by different
        // neighbourhoods, which is the same broken pairing as `periodic` against
        // `closed`.
        for (min, max) in [
            (Boundary::Periodic, Boundary::Exchange),
            (Boundary::Exchange, Boundary::Periodic),
        ] {
            let mut boundary = [Boundary::Closed; 6];
            boundary[Face::XMinus as usize] = min;
            boundary[Face::XPlus as usize] = max;
            let err = Grid::new(NX, NY, NZ, boundary).unwrap_err().to_string();
            assert!(err.contains("periodic"), "unhelpful message: {err}");
            assert!(err.contains("Exchange"), "unhelpful message: {err}");
        }

        // `exchange` opposite `closed` is legal: there the pairing is absent
        // rather than broken, and its second half is the channel counter.
        let mut boundary = [Boundary::Closed; 6];
        boundary[Face::ZPlus as usize] = Boundary::Exchange;
        assert!(Grid::new(NX, NY, NZ, boundary).is_ok());

        // Six exchanging faces are legal too, and they share one ghost, because
        // `[boundary.reservoir]` is one section (ADR-059).
        let all = Grid::new(NX, NY, NZ, [Boundary::Exchange; 6]).unwrap();
        assert_eq!(all.exchange_mask(), 0b11_1111);
        let low = all.index(0, 0, 0);
        let high = all.index(NX - 1, NY - 1, NZ - 1);
        for face in [Face::XMinus, Face::YMinus, Face::ZMinus] {
            assert_eq!(all.neighbour(low, face), all.ghost_index());
            assert_ne!(all.neighbour(high, face), all.ghost_index());
        }
        for face in [Face::XPlus, Face::YPlus, Face::ZPlus] {
            assert_eq!(all.neighbour(high, face), all.ghost_index());
            assert_ne!(all.neighbour(low, face), all.ghost_index());
        }
    }

    #[test]
    fn a_half_periodic_axis_is_refused() {
        let mut boundary = [Boundary::Periodic; 6];
        boundary[Face::XPlus as usize] = Boundary::Closed;
        let err = Grid::new(NX, NY, NZ, boundary).unwrap_err().to_string();
        assert!(err.contains("periodic"), "unhelpful message: {err}");

        // Both closed is fine: it is an axis with two walls, not a broken torus.
        let mut boundary = [Boundary::Periodic; 6];
        boundary[Face::XMinus as usize] = Boundary::Closed;
        boundary[Face::XPlus as usize] = Boundary::Closed;
        assert!(Grid::new(NX, NY, NZ, boundary).is_ok());
    }

    #[test]
    fn an_unaddressable_grid_is_refused() {
        assert!(Grid::new(0, 4, 4, [Boundary::Periodic; 6]).is_err());
        assert!(Grid::new(4, 0, 4, [Boundary::Periodic; 6]).is_err());
        assert!(Grid::new(4, 4, 0, [Boundary::Periodic; 6]).is_err());

        // 2^11 cubed is 2^33 voxels: past what a u32 linear index can reach.
        let err = Grid::new(2048, 2048, 2048, [Boundary::Periodic; 6])
            .unwrap_err()
            .to_string();
        assert!(err.contains("u32"), "unhelpful message: {err}");

        // The largest grid SPEC section 1.1 contemplates still fits, with room.
        assert!(Grid::new(256, 256, 256, [Boundary::Periodic; 6]).is_ok());
    }
}
