//! Causal acceptance for founder genetics, cross-feeding, and real colony spread.
use liminis_core::config::{self, Config};
use liminis_core::ledger::{DomainSums, Ledger};
use liminis_core::numeric::{M64, run_key};
use liminis_core::process::{ProcessId, ROSTER_LEN, RosterEntry, Scratch, Tick};
use liminis_core::world::{Boundary, Grid, LaneRef, Registry, World, WorldLayout};
use liminis_core::worldgen;

const SOURCE: &str = include_str!("../../../configs/scenarios/genetic-colony.toml");
const IDS: [&str; 4] = ["G00", "G01", "G10", "G11"];

// Return source declarations so each experiment changes genes before expansion.
fn source(n: u32, mutation: f64) -> Config {
    let mut config: Config = toml::from_str(SOURCE).expect("scenario source");
    config.grid.nx = n;
    config.grid.ny = n;
    config.grid.nz = n;
    config
        .genetics
        .as_mut()
        .expect("genetics")
        .mutation_probability = mutation;
    let extent = f64::from(n) * config.grid.dx;
    config.initial.inoculum[0].center = [extent / 2.0; 3];
    config.initial.inoculum[0].radius = 0.00035_f64.min(extent * 0.2);
    config
}

struct Culture {
    config: Config,
    derived: config::Derived,
    world: World,
    tick: Tick,
    scratch: Scratch,
    ledger: Ledger,
    before: DomainSums,
    after: DomainSums,
    t: u32,
    key: u32,
}

impl Culture {
    fn new(mut config: Config, seed: u64) -> Self {
        config::materialise(&mut config).expect("materialise genes and defaults");
        let derived = config::validate(&config).expect("validate genetic colony");
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
        worldgen::generate(&mut world, &derived, run_key(seed)).expect("founder generation");
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
            &config,
            &roster,
            config.dt,
            config.grid.dx,
            seed,
        )
        .expect("tick");
        let scratch = Scratch::new(&world, &tick).expect("scratch");
        let ledger = Ledger::with_reactions(ns, tick.n_reactions()).expect("ledger");
        Self {
            config,
            derived,
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
            self.ledger
                .assert_closed(self.tick.reaction_nu(self.t), &self.before, &self.after);
            self.t += 1;
        }
    }

    fn amounts(&self, id: &str) -> Vec<i64> {
        let s = self
            .config
            .substance
            .iter()
            .position(|s| s.id == id)
            .expect("substance") as u32;
        let n = self.world.grid().n_voxels() as usize;
        match self.world.lane_of(s) {
            LaneRef::Narrow(lane) => self.world.amounts_32().expect("narrow").lane(lane)[..n]
                .iter()
                .map(|v| v.to_i64())
                .collect(),
            LaneRef::Wide(lane) => self.world.amounts_64().expect("wide").lane(lane)[..n]
                .iter()
                .map(|v| v.to_i64())
                .collect(),
        }
    }

    fn concentration(&self, id: &str) -> Vec<f64> {
        let s = self
            .config
            .substance
            .iter()
            .position(|s| s.id == id)
            .expect("substance");
        let factor =
            2.0_f64.powi(i32::from(self.derived.substances()[s].k)) * self.derived.v_voxel();
        self.amounts(id)
            .into_iter()
            .map(|n| n as f64 / factor)
            .collect()
    }

    fn mean(&self, id: &str) -> f64 {
        let values = self.concentration(id);
        values.iter().sum::<f64>() / values.len() as f64
    }

    fn totals(&self) -> [i128; 4] {
        IDS.map(|id| self.amounts(id).into_iter().map(i128::from).sum())
    }

    fn front_radius(&self, threshold: f64) -> f64 {
        let fields = IDS.map(|id| self.concentration(id));
        let center = self.config.initial.inoculum[0].center;
        let dx = self.config.grid.dx;
        let mut radius: f64 = 0.0;
        for i in 0..self.world.grid().n_voxels() {
            let biomass: f64 = fields.iter().map(|f| f[i as usize]).sum();
            if biomass < threshold {
                continue;
            }
            let (x, y, z) = self.world.grid().coords(i);
            let squared: f64 = [x, y, z]
                .iter()
                .enumerate()
                .map(|(axis, c)| {
                    let delta = (f64::from(*c) + 0.5) * dx - center[axis];
                    delta * delta
                })
                .sum();
            radius = radius.max(squared.sqrt());
        }
        radius
    }
}

fn uniform(code: usize, food: f64, detritus: f64) -> Config {
    let mut config = source(8, 0.0);
    config.initial.inoculum.clear();
    for id in IDS {
        config.initial.concentration.insert(id.into(), 0.0);
    }
    config.initial.concentration.insert(IDS[code].into(), 0.01);
    config.initial.concentration.insert("FOOD".into(), food);
    config.initial.concentration.insert("O2".into(), 0.25);
    config.initial.concentration.insert("DET".into(), detritus);
    config
        .boundary
        .reservoir
        .as_mut()
        .unwrap()
        .conc_out
        .insert("FOOD".into(), food);
    config
}

#[test]
fn a_single_founder_creates_new_genotypes_only_through_single_locus_births() {
    let mut culture = Culture::new(source(8, 0.02), 42);
    let initial = culture.totals();
    assert!(initial[0] > 0);
    assert_eq!(&initial[1..], &[0, 0, 0]);
    assert_eq!(culture.mean("DET"), 0.0);
    culture.advance(1);
    assert_eq!(
        culture.totals()[3],
        0,
        "G00 cannot flip both loci in one birth"
    );
    culture.advance(299);
    assert!(
        culture.totals().iter().all(|n| *n > 0),
        "successive births must discover all four genotypes"
    );

    let mut faithful = Culture::new(source(8, 0.0), 42);
    faithful.advance(300);
    assert_eq!(
        &faithful.totals()[1..],
        &[0, 0, 0],
        "no mutation, no unseeded genotype"
    );
}

#[test]
fn inherited_speed_and_affinity_reverse_fitness_between_poor_and_rich_food() {
    let gain = |code, food| {
        let mut culture = Culture::new(uniform(code, food, 0.0), 42);
        let before = culture.mean(IDS[code]);
        culture.advance(30);
        culture.mean(IDS[code]) - before
    };
    let slow_poor = gain(0, 0.001);
    let fast_poor = gain(1, 0.001);
    let slow_rich = gain(0, 0.8);
    let fast_rich = gain(1, 0.8);
    println!(
        "30-tick biomass gains mol/m3: poor slow={slow_poor} fast={fast_poor}; rich slow={slow_rich} fast={fast_rich}"
    );
    assert!(
        slow_poor > fast_poor,
        "affinity must win scarcity: {slow_poor} vs {fast_poor}"
    );
    assert!(
        fast_rich > slow_rich,
        "speed must win abundance: {fast_rich} vs {slow_rich}"
    );
}

#[test]
fn producer_turnover_supplies_detritus_that_changes_consumer_growth() {
    let mut producer = Culture::new(uniform(0, 0.8, 0.0), 42);
    producer.advance(700);
    let detritus = producer.mean("DET");
    assert!(
        detritus > 0.001,
        "actual producer detritus must accumulate: {detritus}"
    );

    let mut no_turnover = uniform(0, 0.8, 0.0);
    no_turnover.genetics.as_mut().unwrap().turnover.rate.vmax = 0.0;
    let mut no_turnover = Culture::new(no_turnover, 42);
    no_turnover.advance(700);
    assert_eq!(
        no_turnover.mean("DET"),
        0.0,
        "detritus has no external or spontaneous source"
    );

    // Matched recipient cultures differ only by the producer-conditioned resource.
    let mut fed = Culture::new(uniform(2, 0.8, detritus), 42);
    let mut unfed = Culture::new(uniform(2, 0.8, 0.0), 42);
    let initial = fed.mean("G10");
    fed.advance(300);
    unfed.advance(300);
    println!(
        "producer DET={detritus} mol/m3; consumer biomass initial={initial}, fed={}, unfed={} after 300 ticks",
        fed.mean("G10"),
        unfed.mean("G10")
    );
    assert!(
        fed.mean("G10") > initial,
        "producer detritus enables net consumer growth"
    );
    assert!(
        unfed.mean("G10") < initial,
        "the weak FOOD pathway cannot pay turnover alone"
    );
    assert!(fed.mean("G10") > unfed.mean("G10") * 1.1);
}

#[test]
fn the_colony_expands_a_measured_biomass_contour_and_replays_exactly() {
    let mut first = Culture::new(source(8, 0.02), 42);
    let mut replay = Culture::new(source(8, 0.02), 42);
    first.advance(100);
    replay.advance(100);
    for substance in &first.config.substance {
        assert_eq!(first.amounts(&substance.id), replay.amounts(&substance.id));
    }
    assert_eq!(
        first.world.enthalpy().read(),
        replay.world.enthalpy().read()
    );

    let mut colony = Culture::new(source(16, 0.02), 42);
    let threshold = 0.0003; // 1% of the declared 0.03 mol/m3 inoculum concentration.
    let before_radius = colony.front_radius(threshold);
    let before_mass: i128 = colony.totals().iter().sum();
    colony.advance(2_000);
    let after_radius = colony.front_radius(threshold);
    println!("front threshold={threshold} mol/m3 radius {before_radius} -> {after_radius} m");
    assert!(after_radius > before_radius + 2.0 * colony.config.grid.dx);
    assert!(colony.totals().iter().sum::<i128>() > before_mass);
}

#[test]
#[ignore = "full 24-cubed genetic-colony release soak; run before changing the default"]
fn genetic_colony_10k_ticks_closes_both_ledgers_and_retains_evolved_populations() {
    let mut colony = Culture::new(source(24, 0.02), 42);
    let threshold = 0.0003;
    let before_radius = colony.front_radius(threshold);
    for _ in 0..10 {
        colony.advance(1_000);
        println!(
            "genetic-colony soak tick={} DET_mean={}",
            colony.t,
            colony.mean("DET")
        );
    }
    let after_radius = colony.front_radius(threshold);
    let totals = colony.totals();
    assert!(totals.iter().all(|n| *n > 0));
    assert!(after_radius > before_radius + 2.0 * colony.config.grid.dx);
    let total = totals.iter().sum::<i128>() as f64;
    println!(
        "seed=42 ticks=10000 shares={:?} radius={before_radius}->{after_radius} DET_mean={}",
        totals.map(|n| n as f64 / total),
        colony.mean("DET")
    );
}
