use eyre::{Context, Result};
use indexmap::IndexMap;
use serde::de::{Deserializer, MapAccess, Visitor};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::path::Path;

pub type StageMap = IndexMap<String, Stage>;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Pipeline {
    pub name: String,
    pub description: String,
    pub output: OutputConfig,
    #[serde(default)]
    pub references: Vec<String>,
    #[serde(deserialize_with = "deserialize_stage_map")]
    pub stages: StageMap,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OutputConfig {
    pub destination: String,
    pub filename: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Stage {
    #[serde(skip_deserializing)]
    pub name: String,
    pub description: String,
    #[serde(rename = "fabric-pattern")]
    pub fabric_pattern: String,
    #[serde(default)]
    pub references: Vec<String>,
    #[serde(default)]
    pub review: bool,
}

pub fn deserialize_stage_map<'de, D>(deserializer: D) -> Result<StageMap, D::Error>
where
    D: Deserializer<'de>,
{
    struct StageMapVisitor;
    impl<'de> Visitor<'de> for StageMapVisitor {
        type Value = StageMap;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a map of stage names to stage definitions")
        }

        fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
        where
            M: MapAccess<'de>,
        {
            let mut stages = StageMap::new();
            while let Some((name, mut stage)) = map.next_entry::<String, Stage>()? {
                stage.name = name.clone();
                stages.insert(name, stage);
            }
            Ok(stages)
        }
    }
    deserializer.deserialize_map(StageMapVisitor)
}

impl Pipeline {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content =
            fs::read_to_string(&path).context(format!("failed to read pipeline file: {}", path.as_ref().display()))?;
        let pipeline: Self =
            serde_yaml::from_str(&content).context(format!("failed to parse pipeline: {}", path.as_ref().display()))?;
        pipeline.validate()?;
        Ok(pipeline)
    }

    pub fn validate(&self) -> Result<()> {
        if self.name.is_empty() {
            return Err(eyre::eyre!("pipeline name is empty"));
        }
        if self.stages.is_empty() {
            return Err(eyre::eyre!("pipeline '{}' has no stages", self.name));
        }
        for (name, stage) in &self.stages {
            if name.is_empty() {
                return Err(eyre::eyre!("stage has empty name in pipeline '{}'", self.name));
            }
            if stage.fabric_pattern.is_empty() {
                return Err(eyre::eyre!(
                    "stage '{}' has no fabric-pattern in pipeline '{}'",
                    name,
                    self.name
                ));
            }
        }
        Ok(())
    }

    /// Get all references for a stage (pipeline-level + stage-level)
    pub fn all_references_for_stage(&self, stage_index: usize, global_refs: &[String]) -> Vec<String> {
        let mut refs: Vec<String> = global_refs.to_vec();
        refs.extend(self.references.clone());
        if let Some((_, stage)) = self.stages.get_index(stage_index) {
            refs.extend(stage.references.clone());
        }
        refs.dedup();
        refs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn sample_pipeline_yaml() -> &'static str {
        r#"name: techspec
description: "Research, outline, draft, and review a technical specification"
output:
  destination: "docs/design/"
  filename: "{date}-{slug}.md"
references:
  - references/voice.md
stages:
  research:
    description: "Gather context"
    fabric-pattern: extract_article_wisdom
    review: false
  outline:
    description: "Create outline"
    fabric-pattern: create_outline
    references:
      - references/templates/techspec.md
    review: true
  draft:
    description: "Write full draft"
    fabric-pattern: write_document
    review: true
"#
    }

    #[test]
    fn test_load_pipeline() {
        let mut tmp = NamedTempFile::with_suffix(".yml").expect("failed to create temp file");
        write!(tmp, "{}", sample_pipeline_yaml()).expect("failed to write");
        let pipeline = Pipeline::load(tmp.path()).expect("failed to load");
        assert_eq!(pipeline.name, "techspec");
        assert_eq!(pipeline.stages.len(), 3);
        let (_, research) = pipeline.stages.get_index(0).unwrap();
        assert_eq!(research.name, "research");
        assert!(!research.review);
        let (_, outline) = pipeline.stages.get_index(1).unwrap();
        assert!(outline.review);
    }

    #[test]
    fn test_validate_empty_name() {
        let yaml = r#"name: ""
description: "test"
output:
  destination: "."
  filename: "out.md"
stages:
  s1:
    description: "d"
    fabric-pattern: p
"#;
        let mut tmp = NamedTempFile::with_suffix(".yml").expect("failed to create temp file");
        write!(tmp, "{}", yaml).expect("failed to write");
        assert!(Pipeline::load(tmp.path()).is_err());
    }

    #[test]
    fn test_validate_no_stages() {
        let yaml = r#"name: test
description: "test"
output:
  destination: "."
  filename: "out.md"
stages: {}
"#;
        let mut tmp = NamedTempFile::with_suffix(".yml").expect("failed to create temp file");
        write!(tmp, "{}", yaml).expect("failed to write");
        assert!(Pipeline::load(tmp.path()).is_err());
    }

    #[test]
    fn test_all_references_for_stage() {
        let mut tmp = NamedTempFile::with_suffix(".yml").expect("failed to create temp file");
        write!(tmp, "{}", sample_pipeline_yaml()).expect("failed to write");
        let pipeline = Pipeline::load(tmp.path()).expect("failed to load");
        let global = vec!["references/global.md".to_string()];
        let refs = pipeline.all_references_for_stage(1, &global);
        assert!(refs.contains(&"references/global.md".to_string()));
        assert!(refs.contains(&"references/voice.md".to_string()));
        assert!(refs.contains(&"references/templates/techspec.md".to_string()));
    }
}
