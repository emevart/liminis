//! The finite ecotype model is actual chemistry, including birth mutations.
use liminis_core::config::{self, Config};
use liminis_core::ledger::{DomainSums, Ledger};
use liminis_core::numeric::{M32, M64, Q, run_key};
use liminis_core::process::{ProcessId, ROSTER_LEN, RosterEntry, Scratch, Tick};
use liminis_core::world::{Boundary, Grid, LaneRef, Registry, World, WorldLayout};
use liminis_core::worldgen;

const SCENARIO: &str = include_str!("../../../configs/scenarios/living-world.toml");
const TYPES: [&str; 5] = [
    "HARVESTER",
    "FORAGER",
    "GENERALIST",
    "OPPORTUNIST",
    "BLOOMER",
];

fn config(n: u32) -> Config {
    let mut config = config::parse(SCENARIO).expect("parse living-world");
    config.grid.nx = n;
    config.grid.ny = n;
    config.grid.nz = n;
    config
}

struct Culture {
    world: World,
    tick: Tick,
    scratch: Scratch,
    ledger: Ledger,
    before: DomainSums,
    after: DomainSums,
    t: u32,
    key: u32,
    ecotypes: [u32; 5],
}

impl Culture {
    fn new(config: &Config, seed: u64) -> Self {
        let derived = config::validate(config).expect("validate living-world");
        let grid = Grid::new(
            config.grid.nx,
            config.grid.ny,
            config.grid.nz,
            [
                Boundary::Periodic,
                Boundary::Periodic,
                Boundary::Periodic,
                Boundary::Periodic,
                Boundary::Closed,
                Boundary::Exchange,
            ],
        )
        .expect("grid");
        let registry = Registry::new(&derived.decls()).expect("registry");
        let ns = registry.n_substances();
        let mut world = World::new(
            grid,
            registry,
            &WorldLayout {
                enthalpy_lod: u32::from(derived.enthalpy_field().lod),
                velocity_lod: 1,
            },
        )
        .expect("world");
        worldgen::generate(&mut world, &derived, run_key(seed)).expect("worldgen");
        let reservoir = derived.reservoir().expect("reservoir");
        world
            .seed_ghosts(&reservoir.amount_out, M64::new(reservoir.enthalpy_out))
            .expect("ghosts");
        let roster: [RosterEntry; ROSTER_LEN] = config
            .process
            .iter()
            .map(|p| {
                let id = ProcessId::from_id(&p.id).expect("process");
                RosterEntry {
                    id,
                    enabled: p.enabled.unwrap_or_else(|| id.enabled_by_default()),
                    every_n_ticks: p.every_n_ticks,
                }
            })
            .collect::<Vec<_>>()
            .try_into()
            .expect("roster");
        let tick = Tick::new(
            &world,
            &derived,
            config,
            &roster,
            config.dt,
            config.grid.dx,
            seed,
        )
        .expect("tick");
        let scratch = Scratch::new(&world, &tick).expect("scratch");
        let ledger = Ledger::with_reactions(ns, tick.n_reactions()).expect("ledger");
        Self {
            ecotypes: TYPES.map(|id| {
                config
                    .substance
                    .iter()
                    .position(|s| s.id == id)
                    .expect("ecotype") as u32
            }),
            world,
            tick,
            scratch,
            ledger,
            before: DomainSums::new(ns).expect("before"),
            after: DomainSums::new(ns).expect("after"),
            t: 0,
            key: run_key(seed),
        }
    }

    fn advance(&mut self, count: u32) {
        for _ in 0..count {
            self.tick.domain_sums(&self.world, &mut self.before);
            self.tick.advance(
                &mut self.world,
                &mut self.ledger,
                &mut self.scratch,
                self.t,
                self.key,
            );
            self.tick.domain_sums(&self.world, &mut self.after);
            // Explicitly active in release: nonzero reaction extents must close.
            self.ledger
                .assert_closed(self.tick.reaction_nu(self.t), &self.before, &self.after);
            self.t += 1;
        }
    }

    fn amounts(&self) -> Vec<i64> {
        let (small, wide) = self.world.amount_slices();
        small
            .iter()
            .map(|n| n.to_i64())
            .chain(wide.iter().map(|n| n.to_i64()))
            .collect()
    }

    fn totals(&self) -> [i128; 5] {
        let n = self.world.grid().n_voxels() as usize;
        self.ecotypes.map(|s| match self.world.lane_of(s) {
            LaneRef::Narrow(lane) => self.world.amounts_32().expect("narrow").lane(lane)[..n]
                .iter()
                .map(|v| i128::from(v.to_i64()))
                .sum(),
            LaneRef::Wide(lane) => self.world.amounts_64().expect("wide").lane(lane)[..n]
                .iter()
                .map(|v| i128::from(v.to_i64()))
                .sum(),
        })
    }

    fn single_founder(&mut self, keep: usize) {
        if let Some(field) = self.world.amounts_32_mut() {
            let current = field.read().to_vec();
            field.write_mut().copy_from_slice(&current);
        }
        if let Some(field) = self.world.amounts_64_mut() {
            let current = field.read().to_vec();
            field.write_mut().copy_from_slice(&current);
        }
        let stride = self.world.grid().lane_len() as usize;
        for (i, s) in self.ecotypes.iter().enumerate() {
            if i == keep {
                continue;
            }
            match self.world.lane_of(*s) {
                LaneRef::Narrow(lane) => {
                    let begin = lane as usize * stride;
                    self.world.amounts_32_mut().expect("narrow").write_mut()[begin..begin + stride]
                        .fill(M32::new(0));
                }
                LaneRef::Wide(lane) => {
                    let begin = lane as usize * stride;
                    self.world.amounts_64_mut().expect("wide").write_mut()[begin..begin + stride]
                        .fill(M64::new(0));
                }
            }
        }
        if let Some(field) = self.world.amounts_32_mut() {
            field.swap();
        }
        if let Some(field) = self.world.amounts_64_mut() {
            field.swap();
        }
    }
}

fn shares(totals: [i128; 5]) -> [f64; 5] {
    let total = totals.iter().sum::<i128>() as f64;
    totals.map(|x| x as f64 / total)
}

#[test]
#[should_panic(expected = "is negative at tick 0")]
fn negative_biomass_is_rejected_before_chemistry_in_release_too() {
    let mut culture = Culture::new(&config(8), 42);
    let s = culture.ecotypes[0];
    let stride = culture.world.grid().lane_len() as usize;
    match culture.world.lane_of(s) {
        LaneRef::Narrow(lane) => {
            let field = culture.world.amounts_32_mut().unwrap();
            let current = field.read().to_vec();
            field.write_mut().copy_from_slice(&current);
            field.write_mut()[lane as usize * stride] = M32::new(-1);
            field.swap();
        }
        LaneRef::Wide(lane) => {
            let field = culture.world.amounts_64_mut().unwrap();
            let current = field.read().to_vec();
            field.write_mut().copy_from_slice(&current);
            field.write_mut()[lane as usize * stride] = M64::new(-1);
            field.swap();
        }
    }
    culture.advance(1);
}

#[test]
fn living_ecotypes_share_chemistry_and_have_a_real_resource_tradeoff() {
    let config = config(8);
    config::validate(&config).expect("valid chemistry");
    let reference = config.substance.iter().find(|s| s.id == TYPES[0]).unwrap();
    let mut vmax = [0.0; 5];
    let mut km = [0.0; 5];
    for (i, id) in TYPES.iter().enumerate() {
        let biomass = config.substance.iter().find(|s| s.id == *id).unwrap();
        assert_eq!(biomass.composition, reference.composition);
        assert_eq!(biomass.molar_mass, reference.molar_mass);
        assert_eq!(biomass.enthalpy_formation, reference.enthalpy_formation);
        assert_eq!(biomass.c_p, reference.c_p);
        let branches: Vec<_> = config
            .reaction
            .iter()
            .filter(|r| r.catalyst == format!("guild:{id}") && r.inputs.contains_key("FOOD"))
            .collect();
        vmax[i] = branches.iter().map(|r| r.rate.vmax).sum();
        km[i] = branches[0].rate.km["FOOD"];
        for branch in &branches {
            assert_eq!(branch.inputs, branches[0].inputs);
            assert_eq!(branch.rate.km, branches[0].rate.km);
            assert_eq!(branch.enthalpy, branches[0].enthalpy);
            assert_eq!(branch.rate.q10, branches[0].rate.q10);
        }
        let faithful = branches
            .iter()
            .find(|r| r.outputs.contains_key(*id))
            .unwrap();
        assert!((faithful.rate.vmax / vmax[i] - 0.98).abs() < 1e-12);
    }
    let growth = |i: usize, food: f64| vmax[i] * food / (km[i] + food);
    assert!(
        growth(0, 0.001) > growth(4, 0.001),
        "low-food specialist wins scarcity"
    );
    assert!(
        growth(4, 0.8) > growth(0, 0.8),
        "fast strategy wins abundance"
    );
}

#[test]
fn birth_mutations_create_neighbours_and_removing_them_prevents_new_types() {
    let config = config(8);
    let mut mutated = Culture::new(&config, 42);
    mutated.single_founder(2);
    assert_eq!(mutated.totals()[1], 0);
    assert_eq!(mutated.totals()[3], 0);
    mutated.advance(100);
    assert!(
        mutated.totals()[1] > 0 && mutated.totals()[3] > 0,
        "a generalist parent must create both neighbouring offspring types"
    );

    let mut faithful = config;
    faithful.reaction.retain(|r| {
        let Some(parent) = r.catalyst.strip_prefix("guild:") else {
            return true;
        };
        !r.inputs.contains_key("FOOD") || r.outputs.contains_key(parent)
    });
    let mut culture = Culture::new(&faithful, 42);
    culture.single_founder(2);
    culture.advance(100);
    for (i, total) in culture.totals().iter().enumerate() {
        if i != 2 {
            assert_eq!(*total, 0, "no route can create an unseeded type");
        }
    }
}

#[test]
fn living_culture_changes_heritable_shares_and_closes_both_ledgers_each_tick() {
    let mut culture = Culture::new(&config(8), 42);
    let before = shares(culture.totals());
    culture.advance(600);
    let after = shares(culture.totals());
    assert!(
        before.iter().zip(after).any(|(a, b)| (a - b).abs() > 0.02),
        "selection must materially change shares: {before:?} -> {after:?}"
    );
    assert!((0..culture.tick.n_reactions()).any(|r| culture.ledger.extent(r) > 0));
    assert!(culture.totals().iter().all(|x| *x > 0));
}

#[test]
fn living_culture_replays_identically_from_the_same_seed() {
    let config = config(8);
    let mut first = Culture::new(&config, 42);
    let mut replay = Culture::new(&config, 42);
    let other = Culture::new(&config, 43);
    assert_ne!(first.amounts(), other.amounts());
    first.advance(80);
    replay.advance(80);
    assert_eq!(first.amounts(), replay.amounts());
    assert_eq!(
        first.world.enthalpy().read(),
        replay.world.enthalpy().read()
    );
}

#[test]
#[ignore = "full 24-cubed release soak; run explicitly before shipping the local viewer"]
fn living_world_10k_ticks_closes_integer_ledgers_and_stays_within_ceilings() {
    let mut culture = Culture::new(&config(24), 42);
    let before = shares(culture.totals());
    culture.advance(10_000);
    let after = shares(culture.totals());
    assert!(culture.totals().iter().all(|x| *x > 0));
    assert!(before.iter().zip(after).any(|(a, b)| (a - b).abs() > 0.02));
    println!("seed=42 ticks=10000 initial_shares={before:?} final_shares={after:?}");
}

#[test]
fn living_catalysts_are_fresh_and_cannot_create_unseeded_life() {
    let config = config(8);
    let mut clean = Culture::new(&config, 42);
    let mut poisoned = Culture::new(&config, 42);
    assert!(!poisoned.scratch.buffers().catalyst.is_empty());
    for _ in 0..10 {
        poisoned
            .scratch
            .buffers_mut()
            .catalyst
            .fill(Q::from_f64(1_000_000.0));
        clean.advance(1);
        poisoned.advance(1);
        assert_eq!(clean.amounts(), poisoned.amounts());
        assert_eq!(
            clean.world.enthalpy().read(),
            poisoned.world.enthalpy().read()
        );
    }
    let mut sterile = Culture::new(&config, 42);
    sterile.single_founder(usize::MAX);
    sterile.advance(100);
    assert_eq!(
        sterile.totals(),
        [0; 5],
        "food cannot create life without a parent catalyst"
    );
}
