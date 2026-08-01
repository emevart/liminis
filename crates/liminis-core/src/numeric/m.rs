//! `M` — extensive, counted quantities. Two storage widths, one text.
//!
//! Amounts of substance, energy, occupied volume, guild trait fields: everything
//! that enters the per-tick invariant of ADR-003 and has to come out exactly
//! equal. Always integer (ADR-004, ADR-026), and independent of the `FLOAT` /
//! `FIXED` mode — exact accounting is needed either way.
//!
//! # Why a macro
//!
//! The storage width is not a choice made here. It is derived at load time from
//! the declared typical and maximum concentrations, and in the default registry
//! exactly one substance comes out 64-bit — water (ADR-039, ADR-040). So both
//! widths have to exist, and the question is how the axis is expressed in Rust.
//!
//! - Two hand-written types is the option NUMERIC.md section 5 already rejected
//!   for shaders, in the same words and for the same reason: they drift apart
//!   within a month, and the drift surfaces as "the 64-bit substance somehow has
//!   different dynamics". The argument is about two texts, not about shaders.
//! - A generic over the width is forbidden inside kernels (ADR-015), and it
//!   would not survive the port anyway: WGSL has neither generics nor a
//!   preprocessor.
//!
//! What is left is the mechanism the project already committed to for shaders:
//! generate both from one text. A `macro_rules!` here is the same tool as the
//! build-time templater there, applied one level down. Edit the macro body and
//! both widths move together; there is no place to edit only one of them.

/// Generates one storage width of the `M` class.
///
/// Invoked twice, immediately below. Everything about the two widths that could
/// differ lives in the two invocations, and everything that must not differ
/// lives in this body.
macro_rules! define_m {
    ($name:ident, $repr:ty) => {
        #[doc = concat!("An extensive, counted quantity stored in `", stringify!($repr), "`.")]
        ///
        /// Which width a substance gets is derived by the loader from its
        /// declared concentrations, never declared by hand (ADR-040). A kernel
        /// receives the width the templater instantiated it with; it does not
        /// choose one and does not branch on it, except through the
        /// `width_mask` of the reaction kernel, where the branch is on the
        /// substance index and therefore uniform across voxels.
        ///
        /// Unlike `Q`, this type does implement `+`, `-` and unary `-`. The
        /// prohibition in ADR-022 is about `Q` and exists because the mode
        /// switch would silently change what a bare operator means. Integer
        /// addition means the same thing in every mode, and the skeleton in
        /// `ARCHITECTURE.md` is written with the operators.
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        // A field buffer is a flat array of these, and on the GPU it is a flat
        // array of the representation. The layouts have to be the same one.
        #[repr(transparent)]
        pub struct $name($repr);

        impl $name {
            /// No amount at all.
            pub const ZERO: Self = Self(0);

            /// The most negative representable amount.
            pub const MIN: Self = Self(<$repr>::MIN);

            /// The largest representable amount. The loader derives scales so
            /// that a full voxel stays well below this (ADR-039); reaching it at
            /// run time means the derivation was fed a wrong `max_conc`.
            pub const MAX: Self = Self(<$repr>::MAX);

            /// Wrap a raw count.
            #[inline(always)]
            pub const fn new(raw: $repr) -> Self {
                Self(raw)
            }

            /// The raw count.
            ///
            /// Public, unlike the representation of `Q`. An `M` is exact and
            /// mode-independent, so unwrapping it costs nothing and reading it
            /// back out is the normal way to write into a flat buffer or to add
            /// into a ledger counter.
            #[inline(always)]
            pub const fn raw(self) -> $repr {
                self.0
            }

            /// The raw count widened to `i64`.
            ///
            /// Channel counters accumulate over a whole run, not over a tick,
            /// and QUANTITIES.md section 3 requires them 64-bit for that reason:
            /// at 1e7 ticks an `i32` counter overflows inside the declared
            /// horizon, and a ledger that overflows stops being a ledger.
            #[inline(always)]
            // In the 64-bit instantiation this cast is a no-op. That is the
            // price of one text for two widths, and it is the same price
            // NUMERIC.md section 5 pays in the shader templater.
            #[allow(clippy::unnecessary_cast)]
            pub const fn to_i64(self) -> i64 {
                self.0 as i64
            }

            /// Storage bounds widened to `i64`, so that one narrowing text
            /// serves both widths.
            #[allow(clippy::unnecessary_cast)]
            pub(in crate::numeric) const MIN_I64: i64 = <$repr>::MIN as i64;

            #[allow(clippy::unnecessary_cast)]
            pub(in crate::numeric) const MAX_I64: i64 = <$repr>::MAX as i64;

            /// Narrow an `i64` into this width, clamping.
            ///
            /// Clamping is a guard, not a policy. An amount outside the range
            /// means the width derivation of ADR-040 was fed the wrong declared
            /// concentrations, and the debug assertion says so; in release the
            /// clamp keeps a wrong number from becoming a number of the opposite
            /// sign, which is what a plain `as` cast would do.
            #[inline(always)]
            #[allow(clippy::unnecessary_cast)]
            pub(in crate::numeric) fn from_i64_clamping(v: i64) -> Self {
                debug_assert!(
                    (Self::MIN_I64..=Self::MAX_I64).contains(&v),
                    concat!(
                        stringify!($name),
                        " overflowed: {} does not fit. The storage width is \
                         derived from max_conc (ADR-040), so this is a wrong \
                         declaration, not a wrong kernel."
                    ),
                    v
                );
                Self(v.clamp(Self::MIN_I64, Self::MAX_I64) as $repr)
            }
        }

        impl core::ops::Add for $name {
            type Output = Self;

            /// Exact. Overflow panics in debug and wraps in release, which is
            /// the standard Rust behaviour and the right one here: the widths
            /// are derived so that it cannot happen, and if it does, a panic in
            /// a test run is how it should be found.
            #[inline(always)]
            fn add(self, rhs: Self) -> Self {
                Self(self.0 + rhs.0)
            }
        }

        impl core::ops::Sub for $name {
            type Output = Self;

            #[inline(always)]
            fn sub(self, rhs: Self) -> Self {
                Self(self.0 - rhs.0)
            }
        }

        impl core::ops::Neg for $name {
            type Output = Self;

            /// Needed by the flux functions: antisymmetry is stated as
            /// `f(a, b) == -f(b, a)` and is tested that way (ADR-034).
            #[inline(always)]
            fn neg(self) -> Self {
                Self(-self.0)
            }
        }

        impl core::ops::AddAssign for $name {
            #[inline(always)]
            fn add_assign(&mut self, rhs: Self) {
                self.0 += rhs.0;
            }
        }

        impl core::ops::SubAssign for $name {
            #[inline(always)]
            fn sub_assign(&mut self, rhs: Self) {
                self.0 -= rhs.0;
            }
        }
    };
}

define_m!(M32, i32);
define_m!(M64, i64);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_is_exact_in_both_widths() {
        assert_eq!(M32::new(7) + M32::new(-9), M32::new(-2));
        assert_eq!(M32::new(7) - M32::new(-9), M32::new(16));
        assert_eq!(-M32::new(7), M32::new(-7));
        assert_eq!(M32::ZERO + M32::new(5), M32::new(5));

        assert_eq!(M64::new(7) + M64::new(-9), M64::new(-2));
        assert_eq!(M64::new(7) - M64::new(-9), M64::new(16));
        assert_eq!(-M64::new(7), M64::new(-7));
        assert_eq!(M64::ZERO + M64::new(5), M64::new(5));

        let mut acc = M64::ZERO;
        acc += M64::new(3);
        acc -= M64::new(10);
        assert_eq!(acc, M64::new(-7));
    }

    #[test]
    fn a_64_bit_amount_holds_what_a_32_bit_one_cannot() {
        // The pool that made ADR-040 necessary: water, over the i32 ceiling.
        let water = 5_100_000_000_000i64;
        assert!(water > M32::MAX.to_i64());
        assert_eq!(M64::new(water).to_i64(), water);
    }

    #[test]
    fn narrowing_keeps_what_fits() {
        assert_eq!(M32::from_i64_clamping(17), M32::new(17));
        assert_eq!(M32::from_i64_clamping(M32::MAX_I64), M32::MAX);
        assert_eq!(M32::from_i64_clamping(M32::MIN_I64), M32::MIN);
        assert_eq!(M64::from_i64_clamping(i64::MAX), M64::MAX);
    }

    // The out-of-range case has two behaviours on purpose, and both are tested:
    // loud in debug, clamped in release. A plain `as i32` cast would do neither
    // — `i32::MAX + 1` becomes `i32::MIN`, a full pool turning into a negative
    // one with nothing to see in the diff.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "overflowed")]
    fn narrowing_out_of_range_is_loud_in_debug() {
        let _ = M32::from_i64_clamping(M32::MAX_I64 + 1);
    }

    #[test]
    #[cfg(not(debug_assertions))]
    fn narrowing_out_of_range_clamps_in_release() {
        assert_eq!(M32::from_i64_clamping(M32::MAX_I64 + 1), M32::MAX);
        assert_eq!(M32::from_i64_clamping(M32::MIN_I64 - 1), M32::MIN);
    }

    #[test]
    fn the_layout_is_the_representation() {
        // Field buffers are flat arrays of M and, on the GPU, flat arrays of the
        // representation. If these ever differ, every upload is wrong.
        assert_eq!(size_of::<M32>(), size_of::<i32>());
        assert_eq!(size_of::<M64>(), size_of::<i64>());
        assert_eq!(align_of::<M32>(), align_of::<i32>());
        assert_eq!(align_of::<M64>(), align_of::<i64>());
    }
}
