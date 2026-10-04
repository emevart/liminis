//! Observation of inherited biomass variants, from the same registry as chemistry.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use super::{Config, FieldMeta, LaneRef, Sim, World};

struct Ecotype {
    id: String,
    vmax: f64,
    km: f64,
    genome: Option<Value>,
    color: &'static str,
}

pub(super) struct Ecology {
    types: Vec<Ecotype>,
    genetics: Option<Value>,
    first_seen: Vec<Option<u32>>,
    resource_ids: Vec<String>,
}

impl Ecology {
    pub(super) fn new(config: &Config) -> Self {
        let ids: BTreeSet<&str> = config
            .reaction
            .iter()
            .filter_map(|r| r.catalyst.strip_prefix("guild:"))
            .collect();
        let mut genomes = BTreeMap::new();
        let genetics = config.genetics.as_ref().map(|g| {
            let resource_ids: Vec<_> = g.growth.iter().map(|p| p.resource.as_str()).collect();
            for genotype in liminis_core::config::decode_genotypes(g) {
                let allocation: Vec<_> = resource_ids
                    .iter()
                    .zip(genotype.resource_allocation)
                    .map(|(resource, fraction)| json!({"resource": resource, "fraction": fraction}))
                    .collect();
                genomes.insert(
                    genotype.id.clone(),
                    json!({
                        "code": genotype.code, "bits": format!("{:02b}", genotype.code),
                        "rate_factor": genotype.rate_factor, "km_factor": genotype.km_factor,
                        "allocation": allocation,
                    }),
                );
            }
            json!({
                "loci": [
                    {"bit": 0, "id": "kinetics", "label": "Growth kinetics",
                     "alleles": ["efficient", "rapid"]},
                    {"bit": 1, "id": "allocation", "label": "Resource allocation",
                     "alleles": ["allocation A", "allocation B"]},
                ],
                "mutation_probability": g.mutation_probability,
                "resources": resource_ids,
            })
        });
        let resource_ids = config
            .substance
            .iter()
            .filter(|s| s.id != "WATER" && !ids.contains(s.id.as_str()))
            .map(|s| s.id.clone())
            .collect();
        let palette = ["#55dfad", "#f4cc69", "#e686b5", "#78baf2", "#b7a1f2"];
        let types: Vec<Ecotype> = ids
            .iter()
            .enumerate()
            .map(|(i, &id)| {
                let growth: Vec<_> = config
                    .reaction
                    .iter()
                    .filter(|r| {
                        r.catalyst.strip_prefix("guild:") == Some(id)
                            && r.outputs.keys().any(|s| ids.contains(s.as_str()))
                            && !r.inputs.keys().any(|s| ids.contains(s.as_str()))
                    })
                    .collect();
                let food = growth
                    .first()
                    .and_then(|r| {
                        r.inputs
                            .keys()
                            .find(|s| s.as_str() != "O2" && s.as_str() != "WATER")
                    })
                    .cloned();
                let km = food
                    .as_ref()
                    .and_then(|food| growth.first().and_then(|r| r.rate.km.get(food)))
                    .copied()
                    .unwrap_or(0.0);
                Ecotype {
                    id: id.to_string(),
                    vmax: growth.iter().map(|r| r.rate.vmax).sum(),
                    km,
                    genome: genomes.get(id).cloned(),
                    color: palette[i % palette.len()],
                }
            })
            .collect();
        let first_seen = vec![None; types.len()];
        Self {
            types,
            genetics,
            first_seen,
            resource_ids,
        }
    }

    /// First nonzero biomass is an observation, not a reconstructed pedigree.
    pub(super) fn observe(&mut self, world: &World, fields: &[FieldMeta], tick: u32) {
        for (t, first_seen) in self.types.iter().zip(&mut self.first_seen) {
            if first_seen.is_none()
                && fields
                    .iter()
                    .find(|f| f.id == t.id)
                    .is_some_and(|f| total_units(world, f) > 0)
            {
                *first_seen = Some(tick);
            }
        }
    }

    pub(super) fn summary(&self, sim: &Sim) -> Value {
        let totals: Vec<f64> = self.types.iter().map(|t| total_mol(sim, &t.id)).collect();
        let biomass: f64 = totals.iter().sum();
        let volume = sim.world.grid().n_voxels() as f64 * sim.v_voxel;
        Value::Array(
            self.types
                .iter()
                .zip(totals)
                .enumerate()
                .map(|(i, (t, total))| {
                    let label = t.id.to_lowercase().replace('_', " ");
                    json!({
                        "id": t.id, "label": label, "color": t.color,
                        "trait": t.vmax, "vmax": t.vmax, "km": t.km,
                        "mean": total / volume, "total_mol": total,
                        "share": if biomass > 0.0 { total / biomass } else { 0.0 },
                        "genome": t.genome, "first_seen_tick": self.first_seen[i],
                    })
                })
                .collect(),
        )
    }

    pub(super) fn frame(&self, sim: &Sim, z: u32) -> Value {
        let ecotypes = self.summary(sim);
        let types = ecotypes.as_array().expect("ecotype array");
        let biomass: f64 = types
            .iter()
            .map(|t| t["total_mol"].as_f64().unwrap_or(0.0))
            .sum();
        let mean_vmax: f64 = types
            .iter()
            .map(|t| t["share"].as_f64().unwrap_or(0.0) * t["vmax"].as_f64().unwrap_or(0.0))
            .sum();
        let diversity: f64 = -types
            .iter()
            .map(|t| {
                let p = t["share"].as_f64().unwrap_or(0.0);
                if p > 0.0 { p * p.ln() } else { 0.0 }
            })
            .sum::<f64>();
        let grid = sim.world.grid();
        let food = self
            .resource_ids
            .iter()
            .find(|id| id.as_str() == "FOOD")
            .map(String::as_str);
        let mut cells = Vec::with_capacity((grid.nx() * grid.ny()) as usize);
        for y in 0..grid.ny() {
            for x in 0..grid.nx() {
                let idx = grid.index(x, y, z) as usize;
                let amounts: Vec<f64> = self
                    .types
                    .iter()
                    .map(|t| concentration(sim, &t.id, idx))
                    .collect();
                let local: f64 = amounts.iter().sum();
                let dominant = amounts
                    .iter()
                    .enumerate()
                    .max_by(|a, b| a.1.total_cmp(b.1))
                    .map_or(0, |(i, _)| i);
                let shares: Vec<f64> = amounts
                    .iter()
                    .map(|n| if local > 0.0 { n / local } else { 0.0 })
                    .collect();
                let resources: serde_json::Map<String, Value> = self
                    .resource_ids
                    .iter()
                    .map(|id| (id.clone(), json!(concentration(sim, id, idx))))
                    .collect();
                cells.push(
                    json!({"x": x, "y": y, "biomass": local, "dominant": dominant,
                    "shares": shares, "resource": food.map_or(0.0, |s| concentration(sim, s, idx)),
                    "oxygen": concentration(sim, "O2", idx), "resources": resources}),
                );
            }
        }
        let volume = f64::from(grid.n_voxels()) * sim.v_voxel;
        let resources: Vec<Value> = sim
            .fields
            .iter()
            .filter(|f| !self.types.iter().any(|t| t.id == f.id))
            .map(|f| json!({"id": f.id, "mean": total_mol(sim, &f.id) / volume}))
            .collect();
        json!({"tick": sim.ticks, "running": sim.running && sim.alive, "alive": sim.alive,
            "error": sim.error, "seed": sim.identity.seed.to_string(),
            "scenario": sim.identity.scenario, "config_hash": sim.identity.config_hash,
            "world_format_version": sim.identity.world_format_version,
            "sim_time": f64::from(sim.ticks) * sim.scenario.dt,
            "grid": {"nx": grid.nx(), "ny": grid.ny(), "nz": grid.nz(), "dx": sim.dx},
            "slice_z": z, "ecotypes": ecotypes, "genetics": self.genetics,
            "resources": resources, "cells": cells,
            "metrics": {"biomass_mol": biomass, "mean_vmax": mean_vmax, "diversity": diversity}})
    }
}

fn field<'a>(sim: &'a Sim, id: &str) -> Option<&'a FieldMeta> {
    sim.fields.iter().find(|f| f.id == id)
}

fn total_mol(sim: &Sim, id: &str) -> f64 {
    let Some(f) = field(sim, id) else { return 0.0 };
    (total_units(&sim.world, f) as f64 / f64::from(f.k).exp2()).max(0.0)
}

fn total_units(world: &World, f: &FieldMeta) -> i128 {
    let n = world.grid().n_voxels() as usize;
    match f.lane {
        LaneRef::Narrow(lane) => world.amounts_32().expect("narrow field").lane(lane)[..n]
            .iter()
            .map(|n| i128::from(n.to_i64()))
            .sum(),
        LaneRef::Wide(lane) => world.amounts_64().expect("wide field").lane(lane)[..n]
            .iter()
            .map(|n| i128::from(n.to_i64()))
            .sum(),
    }
}

fn concentration(sim: &Sim, id: &str, idx: usize) -> f64 {
    let Some(f) = field(sim, id) else { return 0.0 };
    let amount = match f.lane {
        LaneRef::Narrow(lane) => {
            sim.world.amounts_32().expect("narrow field").lane(lane)[idx].to_i64()
        }
        LaneRef::Wide(lane) => sim.world.amounts_64().expect("wide field").lane(lane)[idx].to_i64(),
    };
    (amount as f64 / (f64::from(f.k).exp2() * sim.v_voxel)).max(0.0)
}
