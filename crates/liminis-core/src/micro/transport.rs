//! Opt-in, dilute point-particle Brownian endpoints in a reflecting box.
//!
//! Native f64/libm is a narrow chamber-2 exception, not voxel Q arithmetic.
//! Coefficients use tick-start mass. No excluded volume, resource gradients,
//! adhesion, inertia, or thermal-energy channel is introduced. The arithmetic
//! budget bounds represented endpoint/reflection error, not coefficients or
//! long-time drift. The statistical free-displacement bound is before walls.

use anyhow::{Context, Result, bail, ensure};

use super::{Genome, MicroConfig, MicroState};

const CONTEXT: &str = "liminis/cells/transport/v1";
const FOUNDER: u32 = 1;
const BROWNIAN: u32 = 2;
const ATTEMPTS: u32 = 64;
const BOLTZMANN: f64 = 1.380_649e-23;
const NORMAL_SUPPORT: f64 = 13.0;

fn positive(name: &str, value: f64) -> Result<f64> {
    ensure!(value.is_finite() && value > 0.0, "transport {name} is outside the finite positive numeric range");
    Ok(value)
}

pub(super) fn validate_genome(config: &MicroConfig, genome: &Genome) -> Result<()> {
    match (config.spatial.as_ref(), genome.spatial) {
        (None, None) => Ok(()),
        (Some(_), Some(spatial)) => {
            positive("division radius", spatial.radius_at_division_m)?;
            ensure!(spatial.mobility_scale.is_finite() && (0.0..=1.0).contains(&spatial.mobility_scale),
                "transport mobility scale must be finite and in [0, 1]");
            Ok(())
        }
        _ => bail!("cell spatial genome differs from the chamber format"),
    }
}

pub(super) fn validate_position(config: &MicroConfig, position: Option<[f64; 3]>) -> Result<()> {
    match (config.spatial.as_ref(), position) {
        (None, None) => Ok(()),
        (Some(spatial), Some(position)) => {
            ensure!(position.iter().zip(spatial.dimensions_m).all(|(&x, length)| x.is_finite() && x >= 0.0 && x <= length),
                "cell position must be finite and inside the spatial chamber");
            Ok(())
        }
        _ => bail!("cell position differs from the chamber format"),
    }
}

// Full ordered LE tuple: seed:u64, cell_id:u64, tick:u64, purpose:u32,
// pair:u32, attempt:u32. There is no folded ID/tick or shared biology RNG.
fn draw(seed: u64, id: u64, tick: u64, purpose: u32, pair: u32, attempt: u32) -> [f64; 2] {
    let mut tuple = [0u8; 36];
    tuple[0..8].copy_from_slice(&seed.to_le_bytes());
    tuple[8..16].copy_from_slice(&id.to_le_bytes());
    tuple[16..24].copy_from_slice(&tick.to_le_bytes());
    tuple[24..28].copy_from_slice(&purpose.to_le_bytes());
    tuple[28..32].copy_from_slice(&pair.to_le_bytes());
    tuple[32..36].copy_from_slice(&attempt.to_le_bytes());
    let mut hasher = blake3::Hasher::new_derive_key(CONTEXT);
    hasher.update(&tuple);
    let bytes = hasher.finalize();
    let midpoint = |start| {
        let bits = u64::from_le_bytes(bytes.as_bytes()[start..start + 8].try_into().expect("fixed hash word")) >> 12;
        (bits as f64 + 0.5) * (1.0 / 4_503_599_627_370_496.0)
    };
    [midpoint(0), midpoint(8)]
}

pub(super) fn founder_position(config: &MicroConfig, id: u64) -> Result<Option<[f64; 3]>> {
    let Some(spatial) = &config.spatial else { return Ok(None) };
    let mut position = [0.0; 3];
    for (axis, length) in spatial.dimensions_m.into_iter().enumerate() {
        positive("box dimension", length)?;
        position[axis] = positive("founder position", draw(spatial.transport_seed, id, 0, FOUNDER, axis as u32, 0)[0] * length)?;
        ensure!(position[axis] <= length, "founder position exceeds box");
    }
    Ok(Some(position))
}

fn normal_pair(seed: u64, id: u64, tick: u64, pair: u32) -> Result<[f64; 2]> {
    for attempt in 0..ATTEMPTS {
        let uniform = draw(seed, id, tick, BROWNIAN, pair, attempt);
        let x = 2.0 * uniform[0] - 1.0;
        let y = 2.0 * uniform[1] - 1.0;
        let squared = x * x + y * y;
        if squared > 0.0 && squared < 1.0 {
            let factor = positive("Gaussian factor", (-2.0 * squared.ln() / squared).sqrt())?;
            let result = [x * factor, y * factor];
            ensure!(result.iter().all(|z| z.is_finite() && z.abs() <= NORMAL_SUPPORT),
                "transport Gaussian exceeds supported numeric range");
            return Ok(result);
        }
    }
    bail!("transport Gaussian rejection exhausted {ATTEMPTS} attempts")
}

fn normal(seed: u64, id: u64, tick: u64) -> Result<[f64; 3]> {
    let xy = normal_pair(seed, id, tick, 0)?;
    // Pair 1 contributes z; its fourth independent Gaussian is discarded.
    let z = normal_pair(seed, id, tick, 1)?;
    Ok([xy[0], xy[1], z[0]])
}

fn sigma(config: &MicroConfig, genome: &Genome, mass: i128) -> Result<f64> {
    let spatial = config.spatial.as_ref().context("transport spatial config missing")?;
    let spatial_genome = genome.spatial.context("transport spatial genome missing")?;
    if spatial_genome.mobility_scale == 0.0 { return Ok(0.0) }
    let fraction = positive("mass fraction", positive("cell mass", mass as f64)? / positive("division mass", genome.division_mass as f64)?)?;
    let radius = positive("current radius", spatial_genome.radius_at_division_m * positive("mass cube root", fraction.cbrt())?)?;
    let thermal = positive("thermal coefficient", BOLTZMANN * config.temperature_kelvin)?;
    let numerator = positive("diffusion numerator", spatial_genome.mobility_scale * thermal)?;
    let viscous = positive("viscous coefficient", 6.0 * std::f64::consts::PI * spatial.viscosity_pa_s)?;
    let denominator = positive("diffusion denominator", viscous * radius)?;
    let diffusion = positive("diffusion coefficient", numerator / denominator)?;
    let doubled = positive("twice diffusion coefficient", 2.0 * diffusion)?;
    let variance = positive("displacement variance", doubled * config.dt_seconds)?;
    positive("displacement sigma", variance.sqrt())
}

// Endpoint/reflection allowance for represented x, sigma, z with |z|<=13.
// Power-of-two checks avoid a rounded quotient weakening either budget.
fn coordinate_budget(length: f64, sigma: f64) -> Result<f64> {
    positive("box dimension", length)?;
    positive("displacement sigma", sigma)?;
    let twice = positive("twice box dimension", 2.0 * length)?;
    let span = positive("outward Gaussian span", positive("Gaussian span", NORMAL_SUPPORT * sigma)?.next_up())?;
    let maximum = positive("outward endpoint magnitude", positive("endpoint magnitude", length + span)?.next_up())?;
    let endpoint_ulp = positive("endpoint ulp", maximum.next_up() - maximum)?;
    let reflection_ulp = positive("reflection ulp", twice.next_up() - twice)?;
    let budget = positive("outward coordinate allowance", positive("coordinate allowance", endpoint_ulp + reflection_ulp)?.next_up())?;
    let box_cost = positive("box error budget", budget * 4_294_967_296.0)?;
    let sigma_cost = positive("sigma error budget", budget * 2048.0)?;
    ensure!(box_cost <= length && sigma_cost <= sigma, "transport coordinate arithmetic exceeds box/sigma error budget");
    Ok(budget)
}

fn reflect(endpoint: f64, length: f64) -> Result<f64> {
    ensure!(endpoint.is_finite(), "transport endpoint is not finite");
    let twice = positive("twice box dimension", 2.0 * positive("box dimension", length)?)?;
    let remainder = endpoint.rem_euclid(twice);
    ensure!(remainder.is_finite() && remainder >= 0.0 && remainder <= twice,
        "transport reflection remainder is outside supported range");
    // A rounded adjustment of a negative tiny remainder can equal 2L.
    let reflected = if remainder <= length { remainder } else { twice - remainder };
    ensure!(reflected.is_finite() && reflected >= 0.0 && reflected <= length,
        "transport reflection is outside the box");
    Ok(reflected)
}

pub(super) fn advance(config: &MicroConfig, state: &mut MicroState) -> Result<()> {
    let Some(spatial) = &config.spatial else { return Ok(()) };
    for cell in &mut state.cells {
        let sigma = sigma(config, &cell.genome, cell.mass)?;
        if sigma == 0.0 { continue; } // Preserve all coordinate bits at zero mobility.
        let normal = normal(spatial.transport_seed, cell.id, state.tick)?;
        let mut position = cell.position_m.context("transport cell position missing")?;
        for axis in 0..3 {
            coordinate_budget(spatial.dimensions_m[axis], sigma)?;
            let endpoint = sigma.mul_add(normal[axis], position[axis]);
            position[axis] = reflect(endpoint, spatial.dimensions_m[axis])?;
        }
        cell.position_m = Some(position);
    }
    Ok(())
}

#[cfg(test)]
#[path = "transport_tests.rs"]
mod tests;
