//! The metadata spec's fixed choices and limits, for clients that offer them
//! (the website's metadata form and simulation editor) instead of copying
//! them. `mdr-meta spec` prints this as JSON.
//!
//! Everything here is read from `constants`, which `Meta::findings` checks
//! against, so a value offered from here is one the check accepts.

use crate::constants;
use anyhow::Result;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Serialize)]
pub struct Spec {
    /// The metadata file format version (`toml_version`)
    pub toml_version: u32,

    /// Each `software_name` and the `software_version`s it accepts.
    /// CUSTOM's only version is "NA".
    pub software: BTreeMap<&'static str, &'static [&'static str]>,

    /// `[water] model`
    pub water_models: &'static [&'static str],

    /// `[[solutes]] name`
    pub solute_names: &'static [&'static str],

    /// `[[additional_files]] file_type` as stored; any other text is stored
    /// as "Miscellaneous"
    pub additional_file_types: &'static [&'static str],

    /// `[[ligands]] sequence_type`
    pub sequence_types: &'static [&'static str],

    pub ranges: Ranges,
}

/// Numeric limits, inclusive unless the field says otherwise
#[derive(Debug, Serialize)]
pub struct Ranges {
    pub temperature_kelvin: Range<u32>,
    pub integration_timestep_fs: Range<f64>,
    pub sampling_frequency_ps: Range<f64>,
    pub water_density_kg_m3: Range<f64>,
    /// Exclusive at both ends
    pub solute_concentration_mol_liter_exclusive: Range<f64>,
}

#[derive(Debug, Serialize)]
pub struct Range<T> {
    pub min: T,
    pub max: T,
}

impl Spec {
    pub fn current() -> Self {
        Spec {
            toml_version: constants::METADATA_TOML_VERSION,
            software: constants::VALID_SOFTWARE.clone(),
            water_models: constants::VALID_WATER_MODEL,
            solute_names: constants::VALID_SOLUTE_NAME,
            additional_file_types: constants::ADDITIONAL_FILE_TYPES,
            sequence_types: &["protein", "dna", "rna"],
            ranges: Ranges {
                temperature_kelvin: Range {
                    min: constants::TEMP_K_MIN,
                    max: constants::TEMP_K_MAX,
                },
                integration_timestep_fs: Range {
                    min: constants::TIMESTEP_FS_MIN,
                    max: constants::TIMESTEP_FS_MAX,
                },
                sampling_frequency_ps: Range {
                    min: constants::SAMPLING_FREQUENCY_PS_MIN,
                    max: constants::SAMPLING_FREQUENCY_PS_MAX,
                },
                water_density_kg_m3: Range {
                    min: constants::WATER_DENSITY_MIN,
                    max: constants::WATER_DENSITY_MAX,
                },
                solute_concentration_mol_liter_exclusive: Range {
                    min: constants::SOLUTE_CONCENTRATION_EXCLUSIVE_MIN,
                    max: constants::SOLUTE_CONCENTRATION_EXCLUSIVE_MAX,
                },
            },
        }
    }

    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self).map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::{Meta, SequenceType, Water};

    /// What the spec offers, the check accepts: a minimal file with each
    /// offered software/version, and each water model, has no finding on
    /// those fields.
    #[test]
    fn every_offered_value_passes_the_check() {
        let spec = Spec::current();
        for (name, versions) in &spec.software {
            for version in *versions {
                let mut meta = Meta::example_minimal();
                meta.software_name = Some(name.to_string());
                meta.software_version = Some(version.to_string());
                let found = meta.check(None);
                assert!(
                    !found.iter().any(|f| f.starts_with("software_name:")
                        || f.starts_with("software_version:")),
                    "{name} {version}: {found:?}"
                );
            }
        }
        for model in spec.water_models {
            let mut meta = Meta::example_minimal();
            meta.water = Some(Water {
                model: model.to_string(),
                density_kg_m3: spec.ranges.water_density_kg_m3.min,
            });
            let found = meta.check(None);
            assert!(
                !found.iter().any(|f| f.contains("water")),
                "{model}: {found:?}"
            );
        }
        for sequence_type in spec.sequence_types {
            assert!(sequence_type.parse::<SequenceType>().is_ok());
        }
    }

    #[test]
    fn json_has_the_documented_keys() {
        let json: serde_json::Value =
            serde_json::from_str(&Spec::current().to_json().unwrap()).unwrap();
        assert_eq!(json["software"]["CUSTOM"], serde_json::json!(["NA"]));
        assert!(
            json["water_models"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("TIP3P"))
        );
        assert_eq!(json["ranges"]["temperature_kelvin"]["min"], 275);
        assert_eq!(json["toml_version"], 2);
    }
}
