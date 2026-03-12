use eyre::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Pipeline {
    pub name: String,
    pub description: String,
    pub output: OutputConfig,
    #[serde(default)]
    pub references: Vec<String>,
    pub stages: Vec<Stage>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OutputConfig {
    pub destination: String,
    pub filename: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Stage {
    pub name: String,
    pub description: String,
    pub pattern: String,
    #[serde(default)]
    pub references: Vec<String>,
    #[serde(default)]
    pub review: bool,
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
        for (i, stage) in self.stages.iter().enumerate() {
            if stage.name.is_empty() {
                return Err(eyre::eyre!("stage {} has no name in pipeline '{}'", i, self.name));
            }
            if stage.pattern.is_empty() {
                return Err(eyre::eyre!(
                    "stage '{}' has no pattern in pipeline '{}'",
                    stage.name,
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
        if let Some(stage) = self.stages.get(stage_index) {
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
  - name: research
    description: "Gather context"
    pattern: extract_article_wisdom
    review: false
  - name: outline
    description: "Create outline"
    pattern: create_outline
    references:
      - references/templates/techspec.md
    review: true
  - name: draft
    description: "Write full draft"
    pattern: write_document
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
        assert_eq!(pipeline.stages[0].name, "research");
        assert!(!pipeline.stages[0].review);
        assert!(pipeline.stages[1].review);
    }

    #[test]
    fn test_validate_empty_name() {
        let yaml = r#"name: ""
description: "test"
output:
  destination: "."
  filename: "out.md"
stages:
  - name: s1
    description: "d"
    pattern: p
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
stages: []
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
