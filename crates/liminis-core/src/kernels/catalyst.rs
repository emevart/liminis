//! Gather eco biomass into a reaction's concentration column (ADR-091).

use crate::numeric::{M32, M64, Q, q_conc_32, q_conc_64};

#[derive(Clone, Copy, Debug)]
pub struct CatalystParams {
    pub source_lane: u32,
    pub source_wide: bool,
    pub lane_len: u32,
    pub column: u32,
    pub conc_per_unit: Q,
}

pub fn catalyst_voxel(src32: &[M32], src64: &[M64], dst: &mut [Q], p: &CatalystParams, idx: u32) {
    let address = (p.source_lane * p.lane_len + idx) as usize;
    let concentration = if p.source_wide {
        q_conc_64(src64[address], p.conc_per_unit)
    } else {
        q_conc_32(src32[address], p.conc_per_unit)
    };
    dst[(p.column * p.lane_len + idx) as usize] = if concentration > Q::ZERO {
        concentration
    } else {
        Q::ZERO
    };
}
