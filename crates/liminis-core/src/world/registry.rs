//! The substance registry: a substance index resolves to a storage width and a
//! lane inside that width's field (ADR-056).
//!
//! [`Field`](super::Field) addresses lanes; reactions, stoichiometry, metrics
//! and snapshots address substances. For a single-width registry the two
//! coincide; for a mixed-width one they cannot, and this module is the one place
//! that says how. Everything downstream — the two world buffers, the transport
//! call sites, the `lane_of[]` table the reaction kernel is handed — reads the
//! answer from here rather than computing its own.

use anyhow::{Result, bail};

// The two build-time bounds of ADR-041 live in `kernels/react.rs`, which is
// where `ARCHITECTURE.md` declares them and where they belong: they are the size
// of that kernel's local arrays and of nothing else. They were carried here
// while `kernels/react.rs` was a placeholder, under a TODO asking whoever wrote
// the kernel to move them and leave a re-export. That has happened.
//
// A re-export and not a second pair of literals, deliberately: two spellings,
// one of them nudged once, and the validator accepts a registry the kernel has
// no room for.
pub use crate::kernels::react::{R_MAX, S_MAX};

/// How many substances a registry may hold: one less than [`S_MAX`].
///
/// The missing index is enthalpy. It travels in the stoichiometry vector as a
/// participant on equal footing with matter (ADR-041, SPEC section 2.3,
/// `CONFIG_SCHEMA` section 5), so it occupies one of the kernel's `S_MAX` slots.
/// A bound of `S_MAX` instead would leave the enthalpy delta to land in the slot
/// of the last substance: the matter ledger of ADR-003 still closes, because
/// matter and energy are counted separately, while that substance is quietly
/// created and destroyed at the pace of the reactions.
///
/// *Which* index is the energy one is not decided by any ADR and is not decided
/// here — it arrives from the host as `ReactParams::s_energy`. This constant
/// only subtracts a slot; it does not name it.
pub const MAX_SUBSTANCES: u32 = S_MAX as u32 - 1;

// `width_mask` is a `u32` (ADR-041). At `S_MAX > 32` the shift `1u32 << s` stops
// being defined: a panic in debug, a wrapping shift in release, where `1 << 32`
// is 1 and quietly names substance 0.
const _: () = assert!(S_MAX <= u32::BITS as usize);

/// The storage width of one substance's amount.
///
/// Derived by the loader from the declared `typical_conc` and `max_conc`
/// (ADR-040), never chosen by hand. Two variants and no third: the world holds
/// exactly two buffers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Width {
    /// `i32` — thirteen of the fourteen substances of SPEC section 2.3.
    Bits32,
    /// `i64` — water, and anything else whose declared maximum concentration
    /// does not fit `i32` at its derived scale.
    Bits64,
}

/// One substance as the loader hands it over: everything already derived.
///
/// The registry assigns lanes and nothing else. `k` and `width` come in
/// finished, because deriving them here as well would put the derivation of
/// ADR-039 and ADR-040 in two places.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubstanceDecl {
    /// The id as written in TOML: `WATER`, `CO2`, `H_ION` (SPEC section 2.3).
    pub id: String,
    /// Derived storage width (ADR-040).
    pub width: Width,
    /// `log2(units_per_mol)`, **not** `units_per_mol` (`QUANTITIES.md` section
    /// 2, ADR-039). The host needs `2^k` to fold a concentration factor;
    /// substituting `k` for `2^k` breaks nothing loudly and moves every
    /// concentration by twenty-odd binary orders.
    pub k: u8,
}

/// Where a substance's amounts live, and at what scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubstanceSlot {
    /// Which of the two fields holds this substance.
    pub width: Width,
    /// Its lane **inside that field**, not a global substance index. See the
    /// note on [`Registry`] about why the difference is not an implementation
    /// detail.
    pub lane: u32,
    /// `log2(units_per_mol)` (ADR-039), carried through untouched.
    pub k: u8,
}

/// Substance index -> (width, lane), plus the flat table the reaction kernel
/// reads.
///
/// # Lanes are not substance indices
///
/// A lane is numbered *within its width class*, in declaration order. For the
/// registry of SPEC section 2.3 — water wide, thirteen others narrow — the table
/// comes out `[0, 0, 1, 2, …, 12]`: the narrow field holds thirteen lanes, the
/// wide one holds one, and `amount` costs 60 bytes per voxel per buffer rather
/// than the 168 of two full-size parallel arrays (ADR-056).
///
/// The price is a rule that has to be obeyed rather than noticed:
///
/// ```text
/// lane != s          — everywhere, and by rule rather than by accident
/// s * n_voxels + idx — a bug inside a kernel, always
/// ```
///
/// It is a rule and not an observation because breaking it is silent. A kernel
/// that addresses by substance index reads a lane belonging to some other
/// substance; the ledger still closes, because nothing was lost — the wrong
/// thing was read. And on the registry the project actually carries, water
/// stands first and takes lane 0, so the wrong call is *right* for `s == 0` and
/// off by one for everything else.
///
/// The bearing test of ADR-056 — `mixed_width_storage_matches_uniform_width_storage`,
/// which runs the reaction kernel over a mixed-width registry and an artificially
/// uniform one and demands the same result — is written, in
/// `tests/acceptance_reactions.rs`. It could not be until `kernels/react.rs`
/// existed, and until it did, an addressing mistake inside a kernel was caught
/// by nothing at all. It covers the one kernel that reads by substance index;
/// the `kernel-lint` grep of `.claude/rules/kernels.md` covers the shape of the
/// mistake everywhere else.
///
/// # What the registry does not do
///
/// It does not derive `k` or the width: those come from the loader, out of the
/// declared concentrations (ADR-039, ADR-040). It cannot check them either — it
/// sees neither `c_max` nor `V_voxel`. In particular, `k = ⌊log₂(2³¹/8 /
/// (c_max·V))⌋` is computed in floating point and can in principle come out
/// negative, and an `as u8` on the loader's side would wrap that into a large
/// positive value in silence. That debt belongs to the loader, and is named here
/// only so nobody assumes this type absorbed it.
///
/// It does not see reactions, so it cannot guarantee that `nu_sub[j] <
/// n_substances`. The kernel indexes `lane_of[]` with values out of the reaction
/// tables: on the CPU that panics, in WGSL it reads past the buffer without a
/// signal. The guarantee belongs to the validator, and is written down here
/// rather than left implied there.
///
/// # Lifetime
///
/// Built once, at load, and read from then on: lanes resolve before the first
/// tick, exactly like `kᵢ`, `e_r` and `νᵢ` (ADR-056). Nothing beyond the flat
/// table travels into the run — 14 `u32`s, 56 bytes for the whole world.
///
/// Lanes never leave the process, either: a snapshot is written in substance
/// order, not lane order (ADR-037, ADR-056), or the layout would become part of
/// the format and a scenario that edited one `max_conc` would silently change
/// the meaning of old files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Registry {
    /// Substance ids, indexed by substance index.
    ids: Vec<String>,
    /// Slots, indexed by substance index.
    slots: Vec<SubstanceSlot>,
    /// `slots[s].lane`, flattened. Built in the same pass, not a second one.
    lane_of: Vec<u32>,
    n32: u32,
    n64: u32,
    width_mask: u32,
}

impl Registry {
    /// Assign lanes to already-derived substances.
    ///
    /// Lanes are handed out **in declaration order, within each width class**.
    /// ADR-056 says "a lane index inside its own width class" and does not name
    /// an order; declaration order is taken because it is the only stable one
    /// available and it already carries meaning — the substance index is the
    /// same order. Anything else (a hash iteration, say) would change no result
    /// and fail no test, since lanes do not leave the process, and would simply
    /// stop being a property of the config.
    ///
    /// # Errors
    ///
    /// Returns an error if more than [`MAX_SUBSTANCES`] substances are declared,
    /// if two share an id, or if the list is empty.
    ///
    /// The count is checked **first**, before a single bit of the mask is set:
    /// `1u32 << s` at `s >= 32` panics in debug and wraps in release.
    ///
    /// Two of the three refusals are this file's decisions rather than
    /// quotations. No ADR requires either. Duplicate ids follow the reasoning of
    /// `deny_unknown_fields` in `config.rs` — a typo is an error, not a silently
    /// ignored line: a second `CO2` would take an index, a lane and memory,
    /// would appear in the ledger and the snapshot, and no reaction in TOML
    /// could ever name it, with no invariant disturbed. The empty registry
    /// follows `Field::new` ("a field with no lanes holds nothing"): a world
    /// with no substances runs and does nothing and never complains. If either
    /// is worth more than a comment, it wants an entry in `DECISIONS.md`.
    pub fn new(substances: &[SubstanceDecl]) -> Result<Self> {
        let n = substances.len();
        if n == 0 {
            bail!("a registry with no substances holds nothing");
        }
        if n > MAX_SUBSTANCES as usize {
            bail!(
                "{n} substances declared, but at most {MAX_SUBSTANCES} can be \
                 addressed: the reaction kernel sizes its local arrays at \
                 S_MAX = {S_MAX} (ADR-041) and one of those indices is reserved \
                 for enthalpy, which is a participant of the stoichiometry \
                 vector like any substance (SPEC section 2.3)"
            );
        }

        let mut ids: Vec<String> = Vec::with_capacity(n);
        let mut slots = Vec::with_capacity(n);
        let mut lane_of = Vec::with_capacity(n);
        let mut n32 = 0u32;
        let mut n64 = 0u32;
        let mut width_mask = 0u32;

        for (s, decl) in substances.iter().enumerate() {
            // Linear scan rather than a set: `n <= 31`, and a `HashMap` here
            // would be the one construct that could make lane assignment depend
            // on iteration order.
            if let Some(first) = ids.iter().position(|id| *id == decl.id) {
                bail!(
                    "substance `{}` is declared twice, at index {first} and at \
                     index {s}; only the first is reachable by name, and the \
                     second would still hold an index, a lane and memory",
                    decl.id
                );
            }

            let lane = match decl.width {
                Width::Bits32 => {
                    let lane = n32;
                    n32 += 1;
                    lane
                }
                Width::Bits64 => {
                    let lane = n64;
                    n64 += 1;
                    // By substance index, not by lane. On the SPEC 2.3 registry
                    // the two masks agree bit for bit, because the one wide
                    // substance is first and takes lane 0 — an error here would
                    // first show up on a scenario with a second bulk component,
                    // which is to say when nobody is looking for it.
                    width_mask |= 1 << s;
                    lane
                }
            };

            ids.push(decl.id.clone());
            slots.push(SubstanceSlot {
                width: decl.width,
                lane,
                k: decl.k,
            });
            // Same pass, same value. A second, independent pass would be a
            // second source of truth about one mapping, and a disagreement
            // between them means the host and the reaction kernel address
            // different lanes of the same substance.
            lane_of.push(lane);
        }

        Ok(Self {
            ids,
            slots,
            lane_of,
            n32,
            n64,
            width_mask,
        })
    }

    /// Where substance `s` lives.
    ///
    /// # Panics
    ///
    /// If `s` is not a substance index of this registry. Host-side code, so a
    /// panic rather than a silent clamp: the alternative is reading somebody
    /// else's lane.
    #[inline]
    #[must_use]
    pub fn slot(&self, s: u32) -> SubstanceSlot {
        self.slots[s as usize]
    }

    /// How many lanes that width's field needs.
    ///
    /// May be zero, and legally so: a registry that came out entirely 32-bit
    /// needs no wide field at all. [`Field::new`](super::Field::new) refuses
    /// zero lanes, so the world holds an `Option<Field64>` and hands the kernel
    /// an empty slice; `width_mask == 0` is what guarantees nobody looks into it
    /// (ADR-056). The mirror case is just as legal — ADR-040's rule is general,
    /// not about water — so the narrow field is optional in exactly the same
    /// way. Allocating one dummy lane instead would shift the whole addressing
    /// of that class by one.
    #[inline]
    #[must_use]
    pub fn lanes(&self, width: Width) -> u32 {
        match width {
            Width::Bits32 => self.n32,
            Width::Bits64 => self.n64,
        }
    }

    /// Bit `s` set means substance `s` is stored 64-bit (ADR-041).
    ///
    /// A scalar, kept apart from [`Registry::lane_of`] on purpose: this is the
    /// branch condition of the reaction kernel, and it has to live in the
    /// uniform buffer. Packing the width into the top bit of the lane table
    /// would save four bytes for the whole world and move that condition into a
    /// storage buffer, where it stops being a register and starts going to
    /// memory on every access (ADR-056, rejected).
    #[inline]
    #[must_use]
    pub fn width_mask(&self) -> u32 {
        self.width_mask
    }

    /// The flat `lane_of[s]` table the reaction kernel is handed, read-only.
    ///
    /// Exactly `n_substances` long, never padded to [`S_MAX`]: a zero tail would
    /// answer every out-of-range index with lane 0, which on the SPEC 2.3
    /// registry is water's. The kernel's loop bound is `p.n_substances`.
    #[inline]
    #[must_use]
    pub fn lane_of(&self) -> &[u32] {
        &self.lane_of
    }

    /// How many substances. Also `NIN`'s middle term (ADR-053).
    #[inline]
    #[must_use]
    pub fn n_substances(&self) -> u32 {
        self.ids.len() as u32
    }

    /// The id of substance `s`, for messages and snapshots.
    ///
    /// # Panics
    ///
    /// If `s` is not a substance index of this registry.
    #[inline]
    #[must_use]
    pub fn id_of(&self, s: u32) -> &str {
        &self.ids[s as usize]
    }

    /// The index of the substance named `id`, if there is one.
    ///
    /// The direction the loader resolves reaction participants in. Linear, and
    /// deliberately: with at most thirty-one ids it is a scan of one cache line
    /// or two, run at load and never in a tick.
    #[must_use]
    pub fn index_of(&self, id: &str) -> Option<u32> {
        self.ids.iter().position(|it| it == id).map(|s| s as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{Boundary, Field32, Field64, Grid};

    /// The registry of SPEC section 2.3, in declaration order.
    const SPEC_IDS: [&str; 14] = [
        "WATER", "O2", "CO2", "N_MIN", "P_MIN", "H2S", "SO4", "CH4", "FE2", "H_ION", "DET_L",
        "DET_R", "BIOMASS", "MINERAL",
    ];

    fn decl(id: &str, width: Width, k: u8) -> SubstanceDecl {
        SubstanceDecl {
            id: id.to_string(),
            width,
            k,
        }
    }

    /// SPEC section 2.3: fourteen substances, `WATER` the one 64-bit one
    /// (ADR-040).
    ///
    /// The `k` values are `s`, deliberately unphysical: only two real scales
    /// exist anywhere in the corpus (ADR-040's worked example, 71 for the proton
    /// and 63 for water), and a plausible table of the other twelve invented
    /// here would outlive this test as a silent default. The registry does not
    /// interpret `k` at all, so any value exercises it equally.
    fn spec_decls() -> Vec<SubstanceDecl> {
        SPEC_IDS
            .iter()
            .enumerate()
            .map(|(s, id)| {
                let width = if *id == "WATER" {
                    Width::Bits64
                } else {
                    Width::Bits32
                };
                decl(id, width, s as u8)
            })
            .collect()
    }

    /// The same registry with a second bulk component: `CH4` (index 7, middle of
    /// the list) widened. Two wide substances, the second of them not at index
    /// zero — which is what makes a mask built over lanes distinguishable from a
    /// mask built over substance indices.
    fn two_wide_decls() -> Vec<SubstanceDecl> {
        let mut decls = spec_decls();
        decls[7].width = Width::Bits64;
        decls
    }

    fn grid() -> Grid {
        Grid::new(4, 4, 4, [Boundary::Periodic; 6]).unwrap()
    }

    /// Every substance sits in exactly one (width, lane) slot, and the lanes of
    /// each width class are `0..lanes(width)` with no gaps and no repeats.
    fn assert_bijection(reg: &Registry) {
        let n = reg.n_substances();

        let mut seen32 = vec![false; reg.lanes(Width::Bits32) as usize];
        let mut seen64 = vec![false; reg.lanes(Width::Bits64) as usize];
        for s in 0..n {
            let slot = reg.slot(s);
            let seen = match slot.width {
                Width::Bits32 => &mut seen32,
                Width::Bits64 => &mut seen64,
            };
            assert!(
                (slot.lane as usize) < seen.len(),
                "substance {s} got lane {} of a class holding {} lanes: a lane \
                 the field cannot address",
                slot.lane,
                seen.len()
            );
            assert!(
                !seen[slot.lane as usize],
                "lane {} of {:?} handed out twice; substance {s} is the second. \
                 Two substances on one lane read each other's amounts and the \
                 ledger still closes, because nothing was lost — the wrong \
                 thing was read",
                slot.lane, slot.width
            );
            seen[slot.lane as usize] = true;
        }
        assert!(seen32.iter().all(|&b| b), "a hole in the narrow class");
        assert!(seen64.iter().all(|&b| b), "a hole in the wide class");
        assert_eq!(
            reg.lanes(Width::Bits32) + reg.lanes(Width::Bits64),
            n,
            "the two classes together must hold every substance and nothing else"
        );

        // One table, one source of truth. Built as a second independent pass it
        // could disagree with `slot()`, and then the host (transport, via
        // `Field::lane_pair_mut`) and the reaction kernel would address
        // different lanes of the same substance.
        assert_eq!(
            reg.lane_of().len(),
            n as usize,
            "the table must be exactly n_substances long: padded to S_MAX with \
             zeroes, an index past the end silently reads lane 0"
        );
        for s in 0..n {
            assert_eq!(reg.lane_of()[s as usize], reg.slot(s).lane);
        }
    }

    #[test]
    fn every_substance_occupies_exactly_one_lane() {
        assert_bijection(&Registry::new(&spec_decls()).unwrap());
        assert_bijection(&Registry::new(&two_wide_decls()).unwrap());
    }

    #[test]
    fn lanes_are_compact_within_a_width_class() {
        // The count ADR-056 pins the memory budget to: thirteen narrow lanes and
        // one wide one, 13*4 + 1*8 = 60 bytes per voxel per buffer, 226 per
        // voxel all told, 474 MB at 128^3. Parallel full-size arrays would give
        // 14 and 14 — and would pass the bijection test above.
        let reg = Registry::new(&spec_decls()).unwrap();
        assert_eq!(reg.lanes(Width::Bits32), 13);
        assert_eq!(reg.lanes(Width::Bits64), 1);
    }

    #[test]
    fn the_lane_table_is_not_the_identity() {
        // ADR-056: `lane == s` stops being true anywhere. On this registry the
        // rule has an exact shape — water is first in its own class and keeps
        // lane 0, every other substance sits one below its index.
        //
        // Pinned as a table rather than as `assert_ne!(lane, s)` in a loop:
        // that assertion is false at s == 0 and vacuous on a single-width
        // registry, so it would fail honest code and pass the very shortcut it
        // is meant to catch.
        let reg = Registry::new(&spec_decls()).unwrap();
        assert_eq!(
            reg.lane_of(),
            &[0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]
        );
    }

    #[test]
    fn the_width_mask_names_the_same_substances_as_the_slots() {
        // Run on the two-wide registry on purpose. On the SPEC 2.3 registry the
        // one wide substance is first and takes lane 0, so a mask built over
        // lanes equals a mask built over indices bit for bit — the error is
        // invisible on the only registry the project carries around.
        let reg = Registry::new(&two_wide_decls()).unwrap();
        assert_eq!(reg.width_mask(), 0b1000_0001);

        for s in 0..reg.n_substances() {
            let bit = reg.width_mask() & (1 << s) != 0;
            assert_eq!(bit, reg.slot(s).width == Width::Bits64, "bit {s}");
        }
        assert_eq!(
            reg.width_mask() >> reg.n_substances(),
            0,
            "a bit set past the last substance names a substance that is not there"
        );
    }

    #[test]
    fn widening_a_substance_does_not_renumber_the_others() {
        // ADR-056 rejected "renumber so the wide ones come last": a substance
        // index is not private. It indexes `nu_sub[]`, the regulatory network's
        // input vector (NIN = K + n_substances + 5 + 6, ADR-053), the metrics
        // and the snapshots — so a single `max_conc` edited in TOML would
        // permute the genome's inputs without changing the size of W.
        let narrow = Registry::new(&spec_decls()).unwrap();
        let widened = Registry::new(&two_wide_decls()).unwrap();

        assert_eq!(narrow.n_substances(), widened.n_substances());
        for s in 0..narrow.n_substances() {
            assert_eq!(narrow.id_of(s), widened.id_of(s), "index {s} moved");
        }
        for id in SPEC_IDS {
            assert_eq!(narrow.index_of(id), widened.index_of(id), "{id} moved");
        }
        assert_eq!(narrow.index_of("CH4"), Some(7));
        assert_eq!(widened.index_of("CH4"), Some(7));

        // Deliberately *not* asserted: that the lanes stayed put. They must not
        // have. Widening CH4 takes it out of the narrow class, so
        // lanes(Bits32) goes 13 -> 12, lanes(Bits64) 1 -> 2, and every narrow
        // substance after CH4 shifts down one. Asserting otherwise would pin a
        // hole in the narrow class.
        assert_eq!(widened.lanes(Width::Bits32), 12);
        assert_eq!(widened.lanes(Width::Bits64), 2);
        assert_ne!(narrow.lane_of(), widened.lane_of());
    }

    #[test]
    fn a_single_width_registry_allocates_one_field() {
        // All narrow.
        let decls: Vec<_> = SPEC_IDS
            .iter()
            .enumerate()
            .map(|(s, id)| decl(id, Width::Bits32, s as u8))
            .collect();
        let reg = Registry::new(&decls).unwrap();
        assert_eq!(reg.lanes(Width::Bits32), 14);
        assert_eq!(reg.lanes(Width::Bits64), 0);
        assert_eq!(reg.width_mask(), 0);
        // The one registry where `lane == s` holds. It holds because there is a
        // single class, not because lanes track indices, and it does not
        // generalise: see `the_lane_table_is_not_the_identity`.
        assert_eq!(
            reg.lane_of(),
            &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13]
        );

        let grid = grid();
        assert!(Field32::new(&grid, reg.lanes(Width::Bits32)).is_ok());
        let err = Field64::new(&grid, reg.lanes(Width::Bits64))
            .unwrap_err()
            .to_string();
        assert!(err.contains("no lanes"), "unhelpful message: {err}");

        // All wide — legal by the same derivation (ADR-040: "the rule is
        // general, not about water"), and the mirror image: the narrow class is
        // the empty one. Code that treats Field32 as mandatory either fails
        // here or allocates one dummy lane, and then the whole narrow
        // addressing shifts by one.
        let decls: Vec<_> = SPEC_IDS
            .iter()
            .enumerate()
            .map(|(s, id)| decl(id, Width::Bits64, s as u8))
            .collect();
        let reg = Registry::new(&decls).unwrap();
        assert_eq!(reg.lanes(Width::Bits32), 0);
        assert_eq!(reg.lanes(Width::Bits64), 14);
        assert_eq!(reg.width_mask(), 0x3FFF);
        assert!(Field64::new(&grid, reg.lanes(Width::Bits64)).is_ok());
        assert!(Field32::new(&grid, reg.lanes(Width::Bits32)).is_err());
    }

    #[test]
    fn substance_count_over_s_max_is_rejected() {
        let many = |n: u32| -> Vec<SubstanceDecl> {
            (0..n)
                .map(|s| decl(&format!("S{s}"), Width::Bits32, 0))
                .collect()
        };

        // The boundary is 31, not 32: one of the S_MAX indices belongs to
        // enthalpy (SPEC section 2.3, CONFIG_SCHEMA section 5). An
        // implementation that only refuses at 33 passes any "too many is
        // refused" test and leaves the reaction kernel without a slot for the
        // enthalpy delta, which then lands on the last substance — the matter
        // ledger of ADR-003 still closes, because matter and energy are counted
        // apart, while a substance is quietly created and destroyed at the pace
        // of the reactions.
        assert_eq!(
            Registry::new(&many(MAX_SUBSTANCES)).unwrap().n_substances(),
            31
        );

        // An error, not a panic: the count has to be checked *before* the mask
        // is built, because `1u32 << 32` panics in debug and yields 1 in
        // release — silently setting the bit of substance 0.
        let err = Registry::new(&many(MAX_SUBSTANCES + 1))
            .unwrap_err()
            .to_string();
        assert!(err.contains("32"), "the declared count is missing: {err}");
        assert!(err.contains("31"), "the bound is missing: {err}");
        assert!(
            err.contains("enthalpy"),
            "the message must say why the bound is one below S_MAX: {err}"
        );

        assert!(Registry::new(&many(100)).is_err());
    }

    #[test]
    fn k_and_id_travel_through_the_registry_untouched() {
        // The registry neither derives `k` nor checks it: the derivation lives
        // in the loader (ADR-039, ADR-040), and a second one here would be a
        // second source of truth. These two numbers are the worked example of
        // ADR-040 at e_r = 63.
        let decls = vec![
            decl("H_ION", Width::Bits32, 71),
            decl("WATER", Width::Bits64, 63),
        ];
        let reg = Registry::new(&decls).unwrap();
        assert_eq!(reg.slot(0).k, 71);
        assert_eq!(reg.slot(1).k, 63);

        let reg = Registry::new(&spec_decls()).unwrap();
        for s in 0..reg.n_substances() {
            assert_eq!(reg.slot(s).k, s as u8);
            assert_eq!(reg.index_of(reg.id_of(s)), Some(s));
        }
        assert_eq!(reg.index_of("NO_SUCH_SUBSTANCE"), None);
    }

    #[test]
    fn the_same_declarations_build_the_same_registry() {
        // Lanes are assigned in declaration order and in no other order. This
        // is the only test that can catch a numbering that depends on hash
        // iteration order: lanes never leave the process — the snapshot is
        // written in substance order (ADR-037, ADR-056) — so such a numbering
        // would change no result and fail no other test. It would just stop
        // being a property of the config.
        let decls = two_wide_decls();
        let a = Registry::new(&decls).unwrap();
        let b = Registry::new(&decls).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.lane_of(), b.lane_of());
    }

    #[test]
    fn two_substances_with_the_same_id_are_rejected() {
        let mut decls = spec_decls();
        decls[9] = decl("CO2", Width::Bits32, 9);
        let err = Registry::new(&decls).unwrap_err().to_string();
        assert!(err.contains("CO2"), "the name is missing: {err}");
        assert!(
            err.contains('2') && err.contains('9'),
            "both indices: {err}"
        );
    }

    #[test]
    fn an_empty_registry_is_rejected() {
        assert!(Registry::new(&[]).is_err());
    }
}
