//! Numeric foundation: the two classes of quantity, the wrappers over one of
//! them, the rounding rule, and the crossings between them.
//!
//! Everything else in the crate is written on top of this module, so the
//! properties it holds are worth stating once, here.
//!
//! # Two classes, not one
//!
//! **`M` — extensive, counted quantities.** Amounts of substance, energy,
//! volume. Always integer, with a per-quantity scale (ADR-004, ADR-026). The
//! mode does not touch this class: exact accounting is needed either way. The
//! storage width is `i32` or `i64` and it is *derived* at load time from the
//! declared typical and maximum concentrations (ADR-039, ADR-040) — which is
//! why both widths exist here, and why they come out of one macro rather than
//! two hand-written texts.
//!
//! **`Q` — intensive and derived quantities.** Concentrations, reaction rates,
//! expression, coefficients, every intermediate result inside a kernel. This is
//! the class the mode switches: `FLOAT` today, possibly `FIXED` later
//! (ADR-022, NUMERIC.md section 8).
//!
//! There is no bare `M` type here. `ARCHITECTURE.md`'s skeleton writes `M`, and
//! that is a placeholder for whichever width the loader derived; a default alias
//! would be a silent vote for `i32`, and the one substance the default registry
//! needs in `i64` is water — the most abundant thing in the world (ADR-040).
//! Kernels get `M32` or `M64` from the templater, exactly as they get one of two
//! shader variants.
//!
//! # Crossing between the classes
//!
//! NUMERIC.md section 1 allows exactly three crossings, and they are the only
//! places where rounding is allowed to happen:
//!
//! ```text
//! q_conc(amount: M, ...)             -> Q    amount to concentration
//! m_delta(rate: Q, dt)               -> M    rate to change in amount
//! xi(rate: Q, dt, volume, e_r, rng)  -> i32  rate to whole quanta of extent
//! ```
//!
//! The first two round with one deterministic rule, halves away from zero. The
//! third rounds stochastically, and that is a deliberate exception with its own
//! reason (ADR-027); see [`xi`].
//!
//! # What the type system enforces
//!
//! `Q` implements no `Deref` and exposes no field, so a bare operator over two
//! `Q` does not compile (ADR-022). That is the first line of defence and it is
//! worth more than a linter, because it holds without being run. `scripts/
//! check_bare_q.py` is the second line, for WGSL templates, which have no type
//! system at all.
//!
//! `M` is the opposite case: `+` and `-` over `M` are exact, identical in both
//! modes, and used freely by kernels (see the skeleton in `ARCHITECTURE.md`).
//!
//! # What a mode owes
//!
//! `FIXED` is not written and must not be guessed at — NUMERIC.md section 8 says
//! `FLOAT` now, and the scales it would need are only visible from real runs.
//! What is fixed today is the shape of the seam, so that writing it later is a
//! new file rather than a rewrite. A mode module defines:
//!
//! - `Q`, with a private representation, no `Deref`, `ZERO`, `ONE`,
//!   `from_f64` and `debug_f64`;
//! - the ten wrappers of NUMERIC.md section 2: `qadd`, `qsub`, `qmul`, `qdiv`,
//!   `qexp`, `qlog`, `qpow`, `qsqrt`, `qrcp`, `qsigmoid`;
//! - the bridge the rest of this module uses and nobody else can:
//!   `round_half_away_from_zero`, `floor_i64`, `frac`, `from_i64`,
//!   `unit_from_u32`, `exp2_from_u8`.
//!
//! Nothing outside `mod.rs` names the mode, so switching it is one line here.

mod convert;
mod float;
mod m;
mod rng;

// The mode switch, and the whole of it. When `FIXED` arrives it lands as a
// sibling module offering the same public names, and this line becomes
// `use fixed as mode;` behind a cfg. Everything downstream — the wrappers, the
// conversions, both `M` widths, every kernel — is written against `mode` and
// against the names re-exported below, so switching costs a rebuild rather than
// a rewrite (ADR-022, NUMERIC.md section 8).
use float as mode;

pub use convert::{
    m_delta_32, m_delta_64, q_conc_32, q_conc_64, q_round_32, q_round_64, stochastic_round, xi,
};
pub use m::{M32, M64};
pub use mode::{Q, qadd, qdiv, qexp, qlog, qmul, qpow, qrcp, qsigmoid, qsqrt, qsub};
pub use rng::rand;
