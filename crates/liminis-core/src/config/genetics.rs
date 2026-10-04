//! Compile the deliberately bounded two-locus model into ordinary chemistry.

use anyhow::{Result, bail};

use super::{Config, GeneticPathway, Genetics, Genotype, Reaction, Substance};

const N_GENOTYPES: u8 = 4;

/// Decode the four public genotype records in stable numeric order.
#[must_use]
pub fn decode_genotypes(genetics: &Genetics) -> [Genotype; N_GENOTYPES as usize] {
    std::array::from_fn(|code| {
        let code = code as u8;
        let fast = code & 0b01 != 0;
        let alternate = code & 0b10 != 0;
        let preferred = genetics.preferred_fraction;
        Genotype {
            code,
            id: format!("{}{:02b}", genetics.prefix, code),
            rate_factor: if fast { genetics.fast_vmax_factor } else { 1.0 },
            km_factor: if fast { genetics.fast_km_factor } else { 1.0 },
            resource_allocation: if alternate {
                [1.0 - preferred, preferred]
            } else {
                [preferred, 1.0 - preferred]
            },
        }
    })
}

pub(super) fn materialise(config: &mut Config) -> Result<()> {
    let Some(genetics) = config.genetics.clone() else {
        return Ok(());
    };
    check_program(&genetics)?;
    if config
        .substance
        .iter()
        .any(|substance| substance.id == genetics.biomass.id)
    {
        bail!(
            "genetics biomass id `{}` is a template placeholder and must not be declared as an ordinary substance; only the four generated clones are lanes",
            genetics.biomass.id
        );
    }

    let genotypes = decode_genotypes(&genetics);
    for genotype in &genotypes {
        let mut substance = genetics.biomass.clone();
        substance.id.clone_from(&genotype.id);
        insert_substance(&mut config.substance, substance)?;
        config
            .initial
            .concentration
            .entry(genotype.id.clone())
            .or_insert(0.0);
    }

    let mut generated = Vec::new();
    for parent in &genotypes {
        for (path_index, pathway) in genetics.growth.iter().enumerate() {
            let allocation = parent.resource_allocation[path_index];
            for (child, mutation_weight) in offspring(parent.code, genetics.mutation_probability) {
                let weight = allocation * mutation_weight;
                if weight == 0.0 {
                    continue;
                }
                let child_id = &genotypes[child as usize].id;
                let reaction = growth_reaction(&genetics, pathway, parent, child_id, weight);
                if reaction.rate.vmax == 0.0 {
                    continue;
                }
                generated.push(reaction);
            }
        }
        let mut turnover = genetics.turnover.clone();
        turnover.id = format!("{}_turnover", parent.id);
        replace_key(&mut turnover.inputs, &genetics.biomass.id, &parent.id);
        replace_key(&mut turnover.rate.km, &genetics.biomass.id, &parent.id);
        turnover.catalyst = format!("guild:{}", parent.id);
        generated.push(turnover);
    }
    // A canonical document may contain old branches after its genes are edited.
    for existing in &config.reaction {
        let reserved = genotypes.iter().any(|parent| {
            existing.id == format!("{}_turnover", parent.id)
                || existing.catalyst == format!("guild:{}", parent.id)
                || existing.inputs.contains_key(&parent.id)
                || existing.outputs.contains_key(&parent.id)
                || genetics.growth.iter().any(|pathway| {
                    existing
                        .id
                        .starts_with(&format!("{}__{}_to_", pathway.reaction.id, parent.id))
                })
        });
        if reserved && !generated.iter().any(|reaction| reaction.id == existing.id) {
            bail!(
                "genetics reserved reaction `{}` is not a branch of the declared program",
                existing.id
            );
        }
    }
    for reaction in generated {
        insert_reaction(&mut config.reaction, reaction)?;
    }
    Ok(())
}

fn offspring(parent: u8, mutation_probability: f64) -> [(u8, f64); 3] {
    [
        (parent, 1.0 - mutation_probability),
        (parent ^ 0b01, mutation_probability / 2.0),
        (parent ^ 0b10, mutation_probability / 2.0),
    ]
}

fn growth_reaction(
    genetics: &Genetics,
    pathway: &GeneticPathway,
    parent: &Genotype,
    child_id: &str,
    branch_weight: f64,
) -> Reaction {
    let mut reaction = pathway.reaction.clone();
    reaction.id = format!("{}__{}_to_{}", pathway.reaction.id, parent.id, child_id);
    reaction.catalyst = format!("guild:{}", parent.id);
    replace_key(&mut reaction.outputs, &genetics.biomass.id, child_id);
    reaction.rate.vmax *= parent.rate_factor * branch_weight;
    *reaction
        .rate
        .km
        .get_mut(&pathway.resource)
        .expect("checked by check_program") *= parent.km_factor;
    reaction
}

fn replace_key<T>(map: &mut std::collections::BTreeMap<String, T>, old: &str, new: &str) {
    let value = map
        .remove(old)
        .expect("placeholder checked by check_program");
    map.insert(new.to_owned(), value);
}

fn insert_substance(existing: &mut Vec<Substance>, expected: Substance) -> Result<()> {
    if let Some(found) = existing.iter().find(|item| item.id == expected.id) {
        if found != &expected {
            bail!(
                "genetics generated substance `{}` but a different record with that id is already declared",
                expected.id
            );
        }
    } else {
        existing.push(expected);
    }
    Ok(())
}

fn insert_reaction(existing: &mut Vec<Reaction>, expected: Reaction) -> Result<()> {
    if let Some(found) = existing.iter().find(|item| item.id == expected.id) {
        if found != &expected {
            bail!(
                "genetics generated reaction `{}` but a different record with that id is already declared",
                expected.id
            );
        }
    } else {
        existing.push(expected);
    }
    Ok(())
}

fn check_program(genetics: &Genetics) -> Result<()> {
    if genetics.prefix.is_empty() {
        bail!("genetics.prefix must not be empty");
    }
    demand_probability(
        genetics.mutation_probability,
        "genetics.mutation_probability",
    )?;
    demand_probability(genetics.preferred_fraction, "genetics.preferred_fraction")?;
    if !(genetics.fast_vmax_factor.is_finite() && genetics.fast_vmax_factor > 1.0) {
        bail!("genetics.fast_vmax_factor must be finite and > 1");
    }
    if !(genetics.fast_km_factor.is_finite() && genetics.fast_km_factor > genetics.fast_vmax_factor)
    {
        bail!(
            "genetics.fast_km_factor must be finite and greater than fast_vmax_factor ({})",
            genetics.fast_vmax_factor
        );
    }
    if genetics.growth.len() != 2 {
        bail!(
            "genetics.growth must contain exactly 2 pathways, found {}",
            genetics.growth.len()
        );
    }
    if genetics.growth[0].resource == genetics.growth[1].resource {
        bail!("the two genetics.growth pathways must nominate different resources");
    }
    let placeholder = &genetics.biomass.id;
    if placeholder.is_empty() {
        bail!("genetics.biomass.id is the placeholder and must not be empty");
    }
    let genotypes = decode_genotypes(genetics);
    if genotypes.iter().any(|g| g.id == *placeholder) {
        bail!("genetics.biomass.id must not equal a generated genotype id");
    }
    if genetics.growth[0].reaction.id == genetics.growth[1].reaction.id {
        bail!("genetics.growth reaction template ids must be unique");
    }
    for (index, pathway) in genetics.growth.iter().enumerate() {
        let reaction = &pathway.reaction;
        reject_genotype_references(reaction, &genotypes, &format!("genetics.growth[{index}]"))?;
        if reaction.id.is_empty() {
            bail!("genetics.growth[{index}].reaction.id must not be empty");
        }
        if reaction.catalyst != format!("guild:{placeholder}") {
            bail!("genetics.growth[{index}].reaction.catalyst must be `guild:{placeholder}`");
        }
        if reaction.inputs.contains_key(placeholder) {
            bail!("genetics.growth[{index}] must not consume biomass `{placeholder}`");
        }
        if reaction.outputs.get(placeholder) != Some(&1) {
            bail!("genetics.growth[{index}] must output exactly one `{placeholder}` biomass");
        }
        if !reaction.inputs.contains_key(&pathway.resource)
            || !reaction.rate.km.contains_key(&pathway.resource)
        {
            bail!(
                "genetics.growth[{index}] resource `{}` must occur in reaction.inputs and reaction.rate.km",
                pathway.resource
            );
        }
    }
    reject_genotype_references(&genetics.turnover, &genotypes, "genetics.turnover")?;
    if genetics.turnover.catalyst != format!("guild:{placeholder}") {
        bail!("genetics.turnover.catalyst must be `guild:{placeholder}`");
    }
    if !genetics.turnover.inputs.contains_key(placeholder) {
        bail!("genetics.turnover must consume biomass placeholder `{placeholder}`");
    }
    if !genetics.turnover.rate.km.contains_key(placeholder) {
        bail!("genetics.turnover must declare rate.km for biomass placeholder `{placeholder}`");
    }
    if genetics.turnover.outputs.contains_key(placeholder) {
        bail!("genetics.turnover must not output biomass placeholder `{placeholder}`");
    }
    Ok(())
}

fn reject_genotype_references(
    reaction: &Reaction,
    genotypes: &[Genotype; N_GENOTYPES as usize],
    owner: &str,
) -> Result<()> {
    for genotype in genotypes {
        if reaction.inputs.contains_key(&genotype.id)
            || reaction.outputs.contains_key(&genotype.id)
            || reaction.rate.km.contains_key(&genotype.id)
        {
            bail!(
                "{owner} must use the biomass placeholder, not generated genotype `{}`",
                genotype.id
            );
        }
    }
    Ok(())
}

fn demand_probability(value: f64, key: &str) -> Result<()> {
    if !(value.is_finite() && (0.0..=1.0).contains(&value)) {
        bail!("{key} must be finite and in [0, 1], found {value}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{canonical, config_hash, parse};

    const PROGRAM: &str = r#"
name = "genetics-test"
T_ref = 298.15

[grid]
nx = 2
ny = 2
nz = 2
dx = 1e-4

[genetics]
prefix = "E"
mutation_probability = 0.02
preferred_fraction = 0.8
fast_vmax_factor = 3.0
fast_km_factor = 20.0

[genetics.biomass]
id = "BIOMASS_TEMPLATE"
molar_mass = 30.0
typical_conc = 0.01
max_conc = 1.0
partial_molar_volume = 1e-3
settling_radius = 0.0
diffusivity = 1e-9
c_p = 100.0
enthalpy_formation = -100.0
composition = { C = 1 }

[[genetics.growth]]
resource = "FOOD"
reaction = { id = "food-template", enthalpy = 0.0, catalyst = "guild:BIOMASS_TEMPLATE", inputs = { FOOD = 1, O2 = 1 }, outputs = { BIOMASS_TEMPLATE = 1 }, rate = { vmax = 2.0, t_vmax = 298.15, q10 = 2.0, km = { FOOD = 0.1, O2 = 0.2 } } }

[[genetics.growth]]
resource = "DET"
reaction = { id = "det-template", enthalpy = 0.0, catalyst = "guild:BIOMASS_TEMPLATE", inputs = { DET = 1, O2 = 1 }, outputs = { BIOMASS_TEMPLATE = 1 }, rate = { vmax = 4.0, t_vmax = 298.15, q10 = 2.0, km = { DET = 0.3, O2 = 0.2 } } }

[genetics.turnover]
id = "turnover-template"
enthalpy = 0.0
catalyst = "guild:BIOMASS_TEMPLATE"
inputs = { BIOMASS_TEMPLATE = 1 }
outputs = { DET = 1 }
rate = { vmax = 0.01, t_vmax = 298.15, q10 = 2.0, km = { BIOMASS_TEMPLATE = 0.01 } }
"#;

    #[test]
    fn decoder_and_compiler_make_four_lanes_and_twenty_eight_reactions() {
        let config = parse(PROGRAM).expect("genetics program parses");
        let genotypes = decode_genotypes(config.genetics.as_ref().expect("genetics"));
        assert_eq!(
            genotypes.each_ref().map(|g| g.id.as_str()),
            ["E00", "E01", "E10", "E11"]
        );
        assert!((genotypes[0].resource_allocation[0] - 0.8).abs() < 1e-12);
        assert!((genotypes[0].resource_allocation[1] - 0.2).abs() < 1e-12);
        assert!((genotypes[2].resource_allocation[0] - 0.2).abs() < 1e-12);
        assert!((genotypes[2].resource_allocation[1] - 0.8).abs() < 1e-12);
        assert_eq!(
            (genotypes[1].rate_factor, genotypes[1].km_factor),
            (3.0, 20.0)
        );
        assert_eq!(config.substance.len(), 4);
        assert_eq!(config.reaction.len(), 28);
        assert!(
            ["E00", "E01", "E10", "E11"]
                .iter()
                .all(|id| config.initial.concentration.get(*id) == Some(&0.0))
        );
        let turnover = config
            .reaction
            .iter()
            .find(|reaction| reaction.id == "E00_turnover")
            .expect("E00 turnover");
        assert_eq!(turnover.catalyst, "guild:E00");
        assert!(turnover.inputs.contains_key("E00"));
        assert!(turnover.rate.km.contains_key("E00"));
        assert!(!turnover.rate.km.contains_key("BIOMASS_TEMPLATE"));

        let branches: Vec<_> = config
            .reaction
            .iter()
            .filter(|reaction| reaction.id.starts_with("food-template__E01_to_"))
            .collect();
        assert_eq!(branches.len(), 3);
        let total_vmax: f64 = branches.iter().map(|reaction| reaction.rate.vmax).sum();
        assert!((total_vmax - 2.0 * 3.0 * 0.8).abs() < 1e-12);
        assert!(
            branches
                .iter()
                .all(|reaction| reaction.rate.km["FOOD"] == 2.0)
        );
        assert!(
            branches
                .iter()
                .all(|reaction| reaction.rate.km["O2"] == 0.2)
        );
    }

    #[test]
    fn zero_mutation_omits_zero_rate_branches() {
        let text = PROGRAM.replace("mutation_probability = 0.02", "mutation_probability = 0.0");
        let config = parse(&text).expect("zero mutation parses");
        assert_eq!(config.reaction.len(), 12, "8 births plus 4 deaths");
        assert!(
            config
                .reaction
                .iter()
                .all(|reaction| reaction.rate.vmax > 0.0)
        );

        let text = PROGRAM.replace("vmax = 2.0", "vmax = 0.0");
        let config = parse(&text).expect("zero pathway parses");
        assert_eq!(config.reaction.len(), 16, "12 births plus 4 deaths");
        assert!(
            config
                .reaction
                .iter()
                .all(|reaction| reaction.rate.vmax > 0.0)
        );
    }

    #[test]
    fn reaction_names_follow_templates_not_path_positions() {
        let original = parse(PROGRAM).expect("program parses");
        let mut reordered: Config = toml::from_str(PROGRAM).expect("source program");
        reordered
            .genetics
            .as_mut()
            .expect("genetics")
            .growth
            .swap(0, 1);
        materialise(&mut reordered).expect("reordered pathways");
        let names = |config: &Config| {
            config
                .reaction
                .iter()
                .map(|r| r.id.clone())
                .collect::<std::collections::BTreeSet<_>>()
        };
        assert_eq!(names(&original), names(&reordered));
    }

    #[test]
    fn ambiguous_templates_and_stale_generated_branches_are_rejected() {
        for case in 0..4 {
            let mut config: Config = toml::from_str(PROGRAM).expect("source program");
            let genetics = config.genetics.as_mut().expect("genetics");
            match case {
                0 => genetics.biomass.id = "E00".to_owned(),
                1 => genetics.turnover.catalyst = "abiotic".to_owned(),
                2 => genetics.growth[0].reaction.id.clear(),
                3 => genetics.growth[1].reaction.id = genetics.growth[0].reaction.id.clone(),
                _ => unreachable!(),
            }
            assert!(materialise(&mut config).is_err(), "case {case}");
        }
        let original = parse(PROGRAM).expect("original program");
        let mut renamed = original.clone();
        renamed.genetics.as_mut().expect("genetics").growth[0]
            .reaction
            .id = "new-template".to_owned();
        assert!(
            materialise(&mut renamed)
                .expect_err("old physiology rejected")
                .to_string()
                .contains("reserved reaction")
        );
        let stale = original
            .reaction
            .iter()
            .find(|r| r.id == "food-template__E00_to_E01")
            .expect("mutation branch")
            .clone();
        let mut zero =
            parse(&PROGRAM.replace("mutation_probability = 0.02", "mutation_probability = 0.0"))
                .expect("zero mutation program");
        zero.reaction.push(stale);
        assert!(
            materialise(&mut zero)
                .expect_err("stale mutant rejected")
                .to_string()
                .contains("reserved reaction")
        );
    }

    #[test]
    fn raw_templates_cannot_reference_generated_genotype_lanes() {
        for turnover in [false, true] {
            for map in 0..3 {
                let mut config: Config = toml::from_str(PROGRAM).expect("source program");
                let genetics = config.genetics.as_mut().expect("genetics");
                let reaction = if turnover {
                    &mut genetics.turnover
                } else {
                    &mut genetics.growth[0].reaction
                };
                match map {
                    0 => {
                        reaction.inputs.insert("E11".to_owned(), 1);
                    }
                    1 => {
                        reaction.outputs.insert("E11".to_owned(), 1);
                    }
                    2 => {
                        reaction.rate.km.insert("E11".to_owned(), 0.1);
                    }
                    _ => unreachable!(),
                }
                assert!(
                    materialise(&mut config)
                        .expect_err("direct genotype reference rejected")
                        .to_string()
                        .contains("generated genotype")
                );
            }
        }
    }

    #[test]
    fn materialisation_is_idempotent_and_canonical_reload_is_exact() {
        let config = parse(PROGRAM).expect("program parses");
        let text = canonical(&config).expect("canonical form");
        let reloaded = parse(&text).expect("canonical form reloads");
        assert_eq!(config, reloaded);
        assert_eq!(
            config_hash(&config).expect("hash"),
            config_hash(&reloaded).expect("reloaded hash")
        );
    }

    #[test]
    fn a_mismatched_generated_collision_is_rejected() {
        let mut config = parse(PROGRAM).expect("program parses");
        config
            .substance
            .iter_mut()
            .find(|substance| substance.id == "E00")
            .expect("generated E00")
            .molar_mass += 1.0;
        let message = materialise(&mut config)
            .expect_err("collision must fail")
            .to_string();
        assert!(message.contains("E00") && message.contains("different record"));

        let mut config = parse(PROGRAM).expect("program parses");
        config
            .reaction
            .iter_mut()
            .find(|reaction| reaction.id.starts_with("food-template__E00_to_"))
            .expect("generated growth branch")
            .rate
            .vmax += 1.0;
        let message = materialise(&mut config)
            .expect_err("reaction collision must fail")
            .to_string();
        assert!(message.contains("reaction") && message.contains("different record"));
    }

    #[test]
    fn invalid_genetics_programs_are_rejected_before_expansion() {
        let base = parse(PROGRAM).expect("program parses");
        let cases = [
            ("mutation_probability", 0),
            ("preferred_fraction", 1),
            ("fast_vmax_factor", 2),
            ("fast_km_factor", 3),
            ("growth", 4),
        ];
        for (key, case) in cases {
            let mut config = base.clone();
            let genetics = config.genetics.as_mut().expect("genetics");
            match case {
                0 => genetics.mutation_probability = 1.1,
                1 => genetics.preferred_fraction = f64::NAN,
                2 => genetics.fast_vmax_factor = 1.0,
                3 => genetics.fast_km_factor = genetics.fast_vmax_factor,
                4 => {
                    genetics.growth.pop();
                }
                _ => unreachable!(),
            }
            let message = materialise(&mut config)
                .expect_err("invalid genetics must fail")
                .to_string();
            assert!(message.contains(key), "expected `{key}` in: {message}");
        }
    }

    #[test]
    fn every_top_level_genetics_number_changes_the_hash() {
        let original = parse(PROGRAM).expect("program parses");
        let original_hash = config_hash(&original).expect("hash");
        for (from, to) in [
            ("mutation_probability = 0.02", "mutation_probability = 0.03"),
            ("preferred_fraction = 0.8", "preferred_fraction = 0.7"),
            ("fast_vmax_factor = 3.0", "fast_vmax_factor = 4.0"),
            ("fast_km_factor = 20.0", "fast_km_factor = 21.0"),
        ] {
            let changed = parse(&PROGRAM.replace(from, to)).expect("changed program parses");
            assert_ne!(original_hash, config_hash(&changed).expect("changed hash"));
        }
    }
}
