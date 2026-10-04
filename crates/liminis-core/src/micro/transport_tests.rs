use super::*;

#[test]
fn represented_reflection_fixtures_include_walls_multiwrap_and_negative_roundoff() {
    // Independent exact-rational modulo of the represented y/L inputs.
    let rows: [(u64, u64, u64, bool); 12] = [
        (0x0000000000000000, 0x3ff0000000000000, 0x0000000000000000, true),
        (0x8000000000000001, 0x3ff0000000000000, 0x0000000000000001, false),
        (0xbc90000000000000, 0x3ff0000000000000, 0x3c90000000000000, false),
        (0x3fefffffffffffff, 0x3ff0000000000000, 0x3fefffffffffffff, true),
        (0x3ff0000000000001, 0x3ff0000000000000, 0x3feffffffffffffe, true),
        (0x3fffffffffffffff, 0x3ff0000000000000, 0x3cb0000000000000, true),
        (0x4000000000000001, 0x3ff0000000000000, 0x3cc0000000000000, true),
        (0xc000000000000001, 0x3ff0000000000000, 0x3cc0000000000000, true),
        (0x4130000040000000, 0x3ff0000000000000, 0x3fd0000000000000, true),
        (0x40f9999a1999999a, 0x3fb999999999999a, 0x3fa0000000000000, false),
        (0xc0f9999a1999999a, 0x3fb999999999999a, 0x3fa0000000000000, false),
        (0x4000000000000001, 0x4000000000000000, 0x3ffffffffffffffe, true),
    ];
    for (y, length, expected, exact) in rows {
        let length = f64::from_bits(length);
        let actual = reflect(f64::from_bits(y), length).unwrap();
        if exact {
            assert_eq!(actual.to_bits(), expected, "y={y:016x}");
        } else {
            let allowance = (2.0 * length).next_up() - 2.0 * length;
            assert!((actual - f64::from_bits(expected)).abs() <= allowance, "y={y:016x}");
        }
    }
}

#[test]
fn fma_preserves_cancellation_and_numeric_budget_refuses_unsupported_scales() {
    let x = 1.0;
    let sigma = f64::from_bits(0x3ff0000000000001);
    let z = f64::from_bits(0xbfefffffffffffff);
    assert_eq!(sigma.mul_add(z, x).to_bits(), 0xbc9ffffffffffffe);
    assert_eq!(sigma * z + x, 0.0);
    assert!(coordinate_budget(1.0, 2.0f64.powi(-40)).is_err());
    assert_eq!(coordinate_budget(1.0, 2.0f64.powi(-39)).unwrap().to_bits(), 0x3cc8000000000001);
    assert_eq!(coordinate_budget(1.0, 32768.0).unwrap().to_bits(), 0x3dd0000800000001);
    assert!(coordinate_budget(1.0, 131072.0).is_err());
    for invalid in [0.0, -1.0, f64::INFINITY, f64::NAN, f64::MAX] {
        assert!(coordinate_budget(invalid, 1.0).is_err());
        assert!(coordinate_budget(1.0, invalid).is_err());
    }
}
