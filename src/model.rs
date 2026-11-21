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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectRequest {
    pub project_type: String,
    pub language: String,
    pub boot_version: String,
    pub java_version: String,
    pub packaging: String,
    pub group_id: String,
    pub artifact_id: String,
    pub version: String,
    pub name: String,
    pub description: String,
    pub package_name: String,
    pub dependencies: Vec<String>,
}

impl ProjectRequest {
    pub fn from_defaults(m: &Metadata) -> Self {
        fn default<T>(s: &Select<T>) -> String {
            s.default.clone().unwrap_or_default()
        }
        Self {
            project_type: default(&m.project_type),
            language: default(&m.language),
            boot_version: default(&m.boot_version),
            java_version: default(&m.java_version),
            packaging: default(&m.packaging),
            group_id: m.group_id.default.clone(),
            artifact_id: m.artifact_id.default.clone(),
            version: m.version.default.clone(),
            name: m.name.default.clone(),
            description: m.description.default.clone(),
            package_name: m.package_name.default.clone(),
            dependencies: Vec::new(),
        }
    }

    pub fn query_pairs(&self) -> Vec<(&'static str, String)> {
        vec![
            ("type", self.project_type.clone()),
            ("language", self.language.clone()),
            ("bootVersion", self.boot_version.clone()),
            ("javaVersion", self.java_version.clone()),
            ("packaging", self.packaging.clone()),
            ("groupId", self.group_id.clone()),
            ("artifactId", self.artifact_id.clone()),
            ("version", self.version.clone()),
            ("name", self.name.clone()),
            ("description", self.description.clone()),
            ("packageName", self.package_name.clone()),
            ("baseDir", self.artifact_id.clone()),
            ("dependencies", self.dependencies.join(",")),
        ]
    }
}

/// Mirrors the website: `group.artifact`, lowercased, reduced to valid Java
/// identifier segments.
pub fn derive_package_name(group: &str, artifact: &str) -> String {
    format!("{group}.{artifact}")
        .to_lowercase()
        .split('.')
        .map(|seg| {
            seg.chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect::<String>()
        })
        .filter(|seg| !seg.is_empty())
        .map(|seg| {
            if seg.starts_with(|c: char| c.is_ascii_digit()) {
                format!("_{seg}")
            } else {
                seg
            }
        })
        .collect::<Vec<_>>()
        .join(".")
}

impl Metadata {
    pub fn project_type(&self, id: &str) -> Option<&ProjectType> {
        self.project_type.values.iter().find(|t| t.id == id)
    }
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

    #[test]
    fn defaults_from_metadata() {
        let r = ProjectRequest::from_defaults(&metadata());
        assert_eq!(r.project_type, "gradle-project");
        assert_eq!(r.group_id, "com.example");
        assert_eq!(r.artifact_id, "demo");
        assert_eq!(r.package_name, "com.example.demo");
    }

    #[test]
    fn query_has_base_dir_and_joined_deps() {
        let mut r = ProjectRequest::from_defaults(&metadata());
        r.dependencies = vec!["web".into(), "lombok".into()];
        let q = r.query_pairs();
        assert!(q.contains(&("dependencies", "web,lombok".into())));
        assert!(q.contains(&("baseDir", "demo".into())));
    }

    #[test]
    fn derive_package() {
        assert_eq!(
            derive_package_name("com.example", "demo"),
            "com.example.demo"
        );
        assert_eq!(
            derive_package_name("Com.Example", "my-app 2"),
            "com.example.myapp2"
        );
        assert_eq!(
            derive_package_name("com.example", "2fast"),
            "com.example._2fast"
        );
        assert_eq!(derive_package_name("com.example", ""), "com.example");
    }
}
