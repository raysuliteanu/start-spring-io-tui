//! Builds offline-generated projects with the locally installed `mvn` and
//! `gradle`. Needs both on PATH, a JDK matching `JAVA_VERSION`, and network
//! access for Maven Central. Run with:
//! `cargo test --test build_offline -- --ignored`

use std::path::Path;
use std::process::Command;

use start_spring_io_tui::model::{DependencyDetails, Metadata, ProjectRequest};
use start_spring_io_tui::offline;

const JAVA_VERSION: &str = "25";

fn generate_into(dir: &Path, ptype: &str, language: &str, packaging: &str, deps: &[&str]) {
    let metadata: Metadata =
        serde_json::from_str(include_str!("fixtures/metadata.json")).expect("metadata fixture");
    let details: DependencyDetails =
        serde_json::from_str(include_str!("fixtures/dependencies-4.2.0.json"))
            .expect("dependencies fixture");
    let mut req = ProjectRequest::from_defaults(&metadata);
    req.project_type = ptype.into();
    req.language = language.into();
    req.packaging = packaging.into();
    req.java_version = JAVA_VERSION.into();
    req.dependencies = deps.iter().map(|d| d.to_string()).collect();
    let ptype = metadata.project_type(ptype).expect("known type");
    let generated = offline::generate(&req, ptype, &details).expect("generates");
    for (path, bytes) in generated.files {
        let path = dir.join(path);
        std::fs::create_dir_all(path.parent().expect("has parent")).expect("mkdir");
        std::fs::write(path, bytes).expect("write");
    }
}

fn build(dir: &Path, tool: &str) {
    let mut cmd = Command::new(tool);
    match tool {
        "mvn" => cmd.args(["-q", "-B", "package", "-DskipTests"]),
        _ => cmd.args(["-q", "build", "-x", "test"]),
    };
    let out = cmd.current_dir(dir).output().expect("build tool runs");
    assert!(
        out.status.success(),
        "{tool} failed in {}:\n{}\n{}",
        dir.display(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
#[ignore = "needs mvn, gradle, a JDK and network"]
fn offline_projects_build() {
    let deps = ["web", "data-jpa", "spring-ai-openai"];
    let java_deps = ["web", "data-jpa", "spring-ai-openai", "lombok", "devtools"];
    let cases: [(&str, &str, &str, &[&str], &str); 4] = [
        ("maven-project", "java", "jar", &java_deps, "mvn"),
        ("gradle-project", "java", "jar", &java_deps, "gradle"),
        ("gradle-project-kotlin", "kotlin", "war", &deps, "gradle"),
        ("maven-project", "groovy", "jar", &deps, "mvn"),
    ];
    for (ptype, language, packaging, deps, tool) in cases {
        let tmp = tempfile::tempdir().expect("tempdir");
        generate_into(tmp.path(), ptype, language, packaging, deps);
        build(tmp.path(), tool);
    }
}
