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
    /// Nameable, not yet runnable: [`Grid::new`] refuses a grid that has one.
    /// See the TODO on [`Grid::new`] for what is missing.
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
    /// **Any `exchange` face**, for now. See the TODO below.
    ///
    /// # Errors
    ///
    /// Returns an error in each of the three cases above, naming the face or the
    /// axis at fault.
    // TODO(exchange): an `exchange` face is refused rather than approximated,
    // and this is where it stops being refused.
    //
    // The face is not hard — it is a flux to a reservoir instead of to a
    // neighbour — but every number it needs is missing, and guessing any of them
    // silently changes the physics of the top of the domain:
    //
    // - the ghost cell. ADR-059 settled the mechanism: a field lane grows to
    //   `n_voxels + 1`, the last element holds the reservoir in storage units,
    //   and `Grid::neighbour` returns its index for this face — the same trick
    //   by which `closed` already returns the voxel itself and therefore
    //   carries no flux without a branch. That is a change to `world::Field`
    //   and to every bounds check over a lane, and it is not written;
    // - the reservoir itself. The channel is two-way, so the outside has a
    //   composition, a temperature and an exchange rate, and `CONFIG_SCHEMA.md`
    //   section 13 item 6 records that none of them is declared anywhere. The
    //   `[boundary.reservoir]` section and its validator are a wave of their
    //   own.
    //
    // The channel counter is no longer among them: `ledger::Channel` and the
    // `(channel, substance)` table exist, so matter leaving through this face
    // has somewhere to land — `BOUNDARY_EXCHANGE` (SPEC section 7, ADR-059) —
    // and `boundary_outflow_appears_in_channel_counter` (ACCEPTANCE.md) is
    // waiting for the face rather than for the ledger.
    //
    // Treating the face as `closed` in the meantime would compile, pass every
    // test in this file, and quietly seal the lid on a world that is supposed to
    // vent — which is the class of error this project refuses by construction.
    // So: loud, and at load time.
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

        for face in Face::ALL {
            if boundary[face as usize] == Boundary::Exchange {
                bail!(
                    "boundary condition `exchange` on face {face:?} is not \
                     implemented: it moves matter into the BOUNDARY_EXCHANGE \
                     channel (SPEC section 7) and there are no channel counters \
                     yet, nor a declared reservoir (CONFIG_SCHEMA.md section 13, \
                     item 6)"
                );
            }
        }

        for (axis, min, max) in [
            (Axis::X, Face::XMinus, Face::XPlus),
            (Axis::Y, Face::YMinus, Face::YPlus),
            (Axis::Z, Face::ZMinus, Face::ZPlus),
        ] {
            let min_is_periodic = boundary[min as usize] == Boundary::Periodic;
            let max_is_periodic = boundary[max as usize] == Boundary::Periodic;
            if min_is_periodic != max_is_periodic {
                bail!(
                    "axis {axis:?} is periodic on one face and not on the other \
                     ({:?} / {:?}): periodicity is a property of an axis, and a \
                     half-periodic axis breaks the face pairing that gather-form \
                     conservation rests on (ADR-034, CONFIG_SCHEMA.md section 4)",
                    boundary[min as usize],
                    boundary[max as usize]
                );
            }
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

    /// The boundary condition on one face of the domain.
    #[inline]
    #[must_use]
    pub fn boundary(&self, face: Face) -> Boundary {
        self.boundary[face as usize]
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
    #[inline]
    #[must_use]
    pub fn neighbour(&self, idx: u32, face: Face) -> u32 {
        let (x, y, z) = self.coords(idx);
        let boundary = self.boundary[face as usize];
        match face {
            Face::XMinus => self.index(step_down(x, self.nx, boundary), y, z),
            Face::XPlus => self.index(step_up(x, self.nx, boundary), y, z),
            Face::YMinus => self.index(x, step_down(y, self.ny, boundary), z),
            Face::YPlus => self.index(x, step_up(y, self.ny, boundary), z),
            Face::ZMinus => self.index(x, y, step_down(z, self.nz, boundary)),
            Face::ZPlus => self.index(x, y, step_up(z, self.nz, boundary)),
        }
    }
}

/// One step down an axis, or what the boundary says instead.
///
/// Written with a comparison rather than a modulo: `(coord + extent - 1) %
/// extent` is the same answer and one integer division, which on a GPU is the
/// expensive instruction in the whole neighbourhood lookup.
#[inline]
fn step_down(coord: u32, extent: u32, boundary: Boundary) -> u32 {
    if coord > 0 {
        coord - 1
    } else {
        match boundary {
            Boundary::Periodic => extent - 1,
            Boundary::Closed => coord,
            Boundary::Exchange => unreachable_exchange(),
        }
    }
}

/// One step up an axis, or what the boundary says instead.
#[inline]
fn step_up(coord: u32, extent: u32, boundary: Boundary) -> u32 {
    if coord + 1 < extent {
        coord + 1
    } else {
        match boundary {
            Boundary::Periodic => 0,
            Boundary::Closed => coord,
            Boundary::Exchange => unreachable_exchange(),
        }
    }
}

/// Unreachable by construction: [`Grid::new`] refuses a grid with an `exchange`
/// face, and `Grid`'s fields are private, so there is no other way to build one.
///
/// A panic rather than a fallback on purpose. If the check in `Grid::new` is
/// ever loosened before the channel counters exist, this says so on the first
/// tick instead of quietly sealing the face.
#[cold]
#[inline(never)]
fn unreachable_exchange() -> ! {
    unreachable!(
        "an `exchange` face reached the neighbourhood lookup; Grid::new refuses \
         those until the BOUNDARY_EXCHANGE channel counters exist — see the TODO \
         on Grid::new"
    )
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

    /// The eco-regime default of SPEC section 1.6, minus the one face that
    /// cannot be built yet: periodic in X and Y, solid floor and lid in Z.
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
        for grid in [periodic(), floored()] {
            for idx in 0..grid.n_voxels() {
                for face in Face::ALL {
                    let there = grid.neighbour(idx, face);
                    if there == idx {
                        // A closed face, or an axis one voxel deep. Either way
                        // the face carries no flux and there is nothing to pair.
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

    #[test]
    fn an_exchange_face_is_refused_until_the_channel_counters_exist() {
        // The default scenario asks for `z_max = "exchange"`
        // (CONFIG_SCHEMA.md section 4). Until the ledger exists, that grid does
        // not load — loudly, rather than behaving like a closed lid.
        let mut boundary = [Boundary::Periodic; 6];
        boundary[Face::ZMinus as usize] = Boundary::Closed;
        boundary[Face::ZPlus as usize] = Boundary::Exchange;

        let err = Grid::new(NX, NY, NZ, boundary).unwrap_err().to_string();
        assert!(err.contains("exchange"), "unhelpful message: {err}");
        assert!(
            err.contains("BOUNDARY_EXCHANGE"),
            "unhelpful message: {err}"
        );
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
