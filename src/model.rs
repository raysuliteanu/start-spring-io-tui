use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    pub dependencies: Groups<DependencyGroup>,
    #[serde(rename = "type")]
    pub project_type: Select<ProjectType>,
    pub packaging: Select<Choice>,
    pub java_version: Select<Choice>,
    pub language: Select<Choice>,
    pub boot_version: Select<Choice>,
    pub group_id: TextField,
    pub artifact_id: TextField,
    pub version: TextField,
    pub name: TextField,
    pub description: TextField,
    pub package_name: TextField,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Select<T> {
    pub default: Option<String>,
    pub values: Vec<T>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Groups<T> {
    pub values: Vec<T>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextField {
    #[serde(default)]
    pub default: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectType {
    pub id: String,
    pub name: String,
    pub action: String,
    pub tags: ProjectTags,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectTags {
    pub build: String,
    pub dialect: Option<String>,
    pub format: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyGroup {
    pub name: String,
    pub values: Vec<Dependency>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dependency {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub version_range: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyDetails {
    pub boot_version: String,
    pub dependencies: BTreeMap<String, Coordinates>,
    #[serde(default)]
    pub boms: BTreeMap<String, Bom>,
    #[serde(default)]
    pub repositories: BTreeMap<String, Repository>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Coordinates {
    pub group_id: String,
    pub artifact_id: String,
    pub version: Option<String>,
    pub scope: Scope,
    pub bom: Option<String>,
    pub repository: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Scope {
    Compile,
    Runtime,
    CompileOnly,
    Provided,
    AnnotationProcessor,
    Test,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bom {
    pub group_id: String,
    pub artifact_id: String,
    pub version: String,
    #[serde(default)]
    pub repositories: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Repository {
    pub name: String,
    pub url: String,
    pub snapshot_enabled: bool,
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn metadata() -> Metadata {
        serde_json::from_str(include_str!("../tests/fixtures/metadata.json"))
            .expect("fixture parses")
    }

    pub(crate) fn details() -> DependencyDetails {
        serde_json::from_str(include_str!("../tests/fixtures/dependencies-4.2.0.json"))
            .expect("fixture parses")
    }

    #[test]
    fn parses_live_metadata() {
        let m = metadata();
        assert_eq!(m.project_type.values.len(), 5);
        let mb = m
            .project_type
            .values
            .iter()
            .find(|t| t.id == "maven-build")
            .expect("maven-build exists");
        assert_eq!(mb.action, "/pom.xml");
        assert_eq!(mb.tags.format, "build");
        assert_eq!(m.boot_version.default.as_deref(), Some("4.2.0"));
        assert!(
            m.dependencies
                .values
                .iter()
                .flat_map(|g| &g.values)
                .any(|d| d.id == "web")
        );
    }

    #[test]
    fn parses_dependency_details() {
        let d = details();
        assert_eq!(d.dependencies["lombok"].scope, Scope::AnnotationProcessor);
        assert_eq!(d.boms["spring-ai"].artifact_id, "spring-ai-bom");
        assert!(d.repositories["spring-snapshots"].snapshot_enabled);
    }
}
