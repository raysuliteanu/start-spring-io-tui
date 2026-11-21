use std::collections::BTreeMap;

use minijinja::{Environment, syntax::SyntaxConfig, value::Serde};
use serde::Serialize;

use crate::model::{Coordinates, DependencyDetails, ProjectRequest, ProjectType, Scope};

// Versions the service doesn't report in /dependencies. Bump these to match
// what start.spring.io generates (observed 2026-11-20).
pub const DEPENDENCY_MANAGEMENT_PLUGIN_VERSION: &str = "1.1.7";
pub const KOTLIN_VERSION: &str = "2.4.21";
pub const GMAVENPLUS_VERSION: &str = "1.13.1";

/// Dependencies whose online output includes build setup we can't derive from the cache.
const NEEDS_EXTRA_SETUP: &[&str] = &["native", "testcontainers"];
const DEVELOPMENT_ONLY: &[&str] = &["devtools", "docker-compose"];

#[derive(Debug, Default)]
pub struct Generated {
    pub files: Vec<(String, Vec<u8>)>,
    pub warnings: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum OfflineError {
    #[error("unknown build system: {0}")]
    UnknownBuild(String),
    #[error("template error: {0}")]
    Template(String),
}

#[derive(Serialize)]
struct MavenDep {
    group_id: String,
    artifact_id: String,
    version: Option<String>,
    scope: Option<&'static str>,
    optional: bool,
}

#[derive(Serialize)]
struct GradleDep {
    config: &'static str,
    coord: String,
}

#[derive(Serialize)]
struct BomCtx {
    maven_property: String,
    gradle_property: String,
    group_id: String,
    artifact_id: String,
    version: String,
}

#[derive(Serialize)]
struct RepoCtx {
    id: String,
    name: String,
    url: String,
    snapshots: bool,
}

#[derive(Serialize)]
struct BuildCtx<'a> {
    group_id: &'a str,
    artifact_id: &'a str,
    version: &'a str,
    name: &'a str,
    description: &'a str,
    boot_version: &'a str,
    java_version: &'a str,
    war: bool,
    kotlin: bool,
    groovy: bool,
    kotlin_jpa: bool,
    kotlin_version: &'static str,
    dm_version: &'static str,
    gmavenplus_version: &'static str,
    maven_deps: Vec<MavenDep>,
    gradle_deps: Vec<GradleDep>,
    processor_paths: Vec<MavenDep>,
    boms: Vec<BomCtx>,
    repositories: Vec<RepoCtx>,
}

/// Gradle configurations in the order Initializr lists them.
const GRADLE_ORDER: &[&str] = &[
    "implementation",
    "compileOnly",
    "developmentOnly",
    "runtimeOnly",
    "annotationProcessor",
    "providedRuntime",
    "testImplementation",
    "testCompileOnly",
    "testRuntimeOnly",
    "testAnnotationProcessor",
];

fn coords(group_id: &str, artifact_id: &str) -> Coordinates {
    Coordinates {
        group_id: group_id.into(),
        artifact_id: artifact_id.into(),
        version: None,
        scope: Scope::Compile,
        bom: None,
        repository: None,
    }
}

fn gradle_coord(c: &Coordinates) -> String {
    match &c.version {
        Some(v) => format!("{}:{}:{v}", c.group_id, c.artifact_id),
        None => format!("{}:{}", c.group_id, c.artifact_id),
    }
}

fn maven_dep(c: &Coordinates, scope: Option<&'static str>, optional: bool) -> MavenDep {
    MavenDep {
        group_id: c.group_id.clone(),
        artifact_id: c.artifact_id.clone(),
        version: c.version.clone(),
        scope,
        optional,
    }
}

fn camel_case(key: &str) -> String {
    let mut out = String::new();
    for (i, part) in key.split(['-', '.']).enumerate() {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            if i == 0 {
                out.push(first);
            } else {
                out.extend(first.to_uppercase());
            }
            out.push_str(chars.as_str());
        }
    }
    out
}

fn environment() -> Environment<'static> {
    let mut env = Environment::new();
    env.set_syntax(
        SyntaxConfig::builder()
            .trim_blocks(true)
            .lstrip_blocks(true)
            .keep_trailing_newline(true)
            .build()
            .expect("static syntax config is valid"),
    );
    env.add_filter("groovy_str", |s: String| {
        format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
    });
    env.add_filter("kotlin_str", |s: String| {
        format!(
            "\"{}\"",
            s.replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('$', "\\$")
        )
    });
    let templates = [
        ("pom.xml.j2", include_str!("../templates/pom.xml.j2")),
        (
            "build.gradle.j2",
            include_str!("../templates/build.gradle.j2"),
        ),
        (
            "build.gradle.kts.j2",
            include_str!("../templates/build.gradle.kts.j2"),
        ),
        (
            "settings.gradle.j2",
            include_str!("../templates/settings.gradle.j2"),
        ),
        (
            "settings.gradle.kts.j2",
            include_str!("../templates/settings.gradle.kts.j2"),
        ),
    ];
    for (name, source) in templates {
        env.add_template(name, source)
            .expect("embedded templates are valid");
    }
    env
}

fn render(env: &Environment, name: &str, ctx: &impl Serialize) -> Result<Vec<u8>, OfflineError> {
    env.get_template(name)
        .and_then(|t| t.render(Serde(ctx)))
        .map(String::into_bytes)
        .map_err(|e| OfflineError::Template(e.to_string()))
}

pub fn build_files(
    req: &ProjectRequest,
    ptype: &ProjectType,
    details: &DependencyDetails,
) -> Result<Generated, OfflineError> {
    let mut warnings = Vec::new();
    let mut selected: Vec<(&str, &Coordinates)> = Vec::new();
    for id in &req.dependencies {
        if NEEDS_EXTRA_SETUP.contains(&id.as_str()) {
            warnings.push(format!("{id}: extra build setup not generated offline"));
        }
        match details.dependencies.get(id) {
            Some(c) => selected.push((id, c)),
            None => warnings.push(format!("{id}: no cached coordinates, skipped")),
        }
    }
    let has = |id: &str| selected.iter().any(|(s, _)| *s == id);

    let kotlin = req.language == "kotlin";
    let groovy = req.language == "groovy";
    let war = req.packaging == "war";
    let kotlin_jpa = kotlin && has("data-jpa");

    let mut maven = Vec::new();
    let mut gradle: Vec<(&'static str, String)> = Vec::new();
    let mut processor_paths = Vec::new();

    if !selected
        .iter()
        .any(|(_, c)| c.artifact_id.starts_with("spring-boot-starter"))
    {
        let c = coords("org.springframework.boot", "spring-boot-starter");
        maven.push(maven_dep(&c, None, false));
        gradle.push(("implementation", gradle_coord(&c)));
    }
    for (id, c) in &selected {
        let coord = gradle_coord(c);
        if *id == "lombok" {
            for config in [
                "compileOnly",
                "annotationProcessor",
                "testCompileOnly",
                "testAnnotationProcessor",
            ] {
                gradle.push((config, coord.clone()));
            }
            maven.push(maven_dep(c, None, true));
            processor_paths.push(maven_dep(c, None, false));
            continue;
        }
        if DEVELOPMENT_ONLY.contains(id) {
            gradle.push(("developmentOnly", coord));
            maven.push(maven_dep(c, Some("runtime"), true));
            continue;
        }
        let (config, scope, optional) = match c.scope {
            Scope::Compile => ("implementation", None, false),
            Scope::Runtime => ("runtimeOnly", Some("runtime"), false),
            Scope::CompileOnly => ("compileOnly", None, true),
            Scope::Provided if war => ("providedRuntime", Some("provided"), false),
            Scope::Provided => ("compileOnly", Some("provided"), false),
            Scope::AnnotationProcessor => ("annotationProcessor", None, true),
            Scope::Test => ("testImplementation", Some("test"), false),
        };
        if c.scope == Scope::AnnotationProcessor {
            processor_paths.push(maven_dep(c, None, false));
        }
        gradle.push((config, coord));
        maven.push(maven_dep(c, scope, optional));
    }

    if kotlin {
        let reflect = coords("org.jetbrains.kotlin", "kotlin-reflect");
        gradle.push(("implementation", gradle_coord(&reflect)));
        maven.push(maven_dep(&reflect, None, false));
        maven.push(maven_dep(
            &coords("org.jetbrains.kotlin", "kotlin-stdlib"),
            None,
            false,
        ));
        if has("web") || has("webflux") {
            let jackson = coords("tools.jackson.module", "jackson-module-kotlin");
            gradle.push(("implementation", gradle_coord(&jackson)));
            maven.push(maven_dep(&jackson, None, false));
        }
    }
    if groovy {
        let g = coords("org.apache.groovy", "groovy");
        gradle.push(("implementation", gradle_coord(&g)));
        maven.push(maven_dep(&g, None, false));
    }
    if war {
        gradle.push((
            "providedRuntime",
            gradle_coord(&coords(
                "org.springframework.boot",
                "spring-boot-starter-tomcat-runtime",
            )),
        ));
        maven.push(maven_dep(
            &coords("org.springframework.boot", "spring-boot-starter-tomcat"),
            Some("provided"),
            false,
        ));
    }
    let test = coords("org.springframework.boot", "spring-boot-starter-test");
    gradle.push(("testImplementation", gradle_coord(&test)));
    maven.push(maven_dep(&test, Some("test"), false));
    if kotlin {
        let kt = coords("org.jetbrains.kotlin", "kotlin-test-junit5");
        gradle.push(("testImplementation", gradle_coord(&kt)));
        maven.push(maven_dep(&kt, Some("test"), false));
    }
    gradle.push((
        "testRuntimeOnly",
        "org.junit.platform:junit-platform-launcher".into(),
    ));

    gradle.sort_by_key(|(config, _)| GRADLE_ORDER.iter().position(|c| c == config));
    let maven_rank = |d: &MavenDep| match d.scope {
        None if !d.optional => 0,
        Some("test") => 3,
        Some("provided") => 2,
        _ => 1,
    };
    maven.sort_by_key(maven_rank);

    let mut boms = BTreeMap::new();
    let mut repo_ids = Vec::new();
    for (_, c) in &selected {
        repo_ids.extend(c.repository.clone());
        if let Some(key) = &c.bom
            && let Some(bom) = details.boms.get(key)
        {
            repo_ids.extend(bom.repositories.iter().cloned());
            boms.insert(key.clone(), bom);
        }
    }
    repo_ids.sort();
    repo_ids.dedup();

    let ctx = BuildCtx {
        group_id: &req.group_id,
        artifact_id: &req.artifact_id,
        version: &req.version,
        name: &req.name,
        description: &req.description,
        boot_version: &req.boot_version,
        java_version: &req.java_version,
        war,
        kotlin,
        groovy,
        kotlin_jpa,
        kotlin_version: KOTLIN_VERSION,
        dm_version: DEPENDENCY_MANAGEMENT_PLUGIN_VERSION,
        gmavenplus_version: GMAVENPLUS_VERSION,
        maven_deps: maven,
        gradle_deps: gradle
            .into_iter()
            .map(|(config, coord)| GradleDep { config, coord })
            .collect(),
        processor_paths,
        boms: boms
            .into_iter()
            .map(|(key, b)| BomCtx {
                maven_property: format!("{key}.version"),
                gradle_property: format!("{}Version", camel_case(&key)),
                group_id: b.group_id.clone(),
                artifact_id: b.artifact_id.clone(),
                version: b.version.clone(),
            })
            .collect(),
        repositories: repo_ids
            .into_iter()
            .filter_map(|id| {
                details.repositories.get(&id).map(|r| RepoCtx {
                    id: id.clone(),
                    name: r.name.clone(),
                    url: r.url.clone(),
                    snapshots: r.snapshot_enabled,
                })
            })
            .collect(),
    };

    let env = environment();
    let project = ptype.tags.format == "project";
    let files = match (ptype.tags.build.as_str(), ptype.tags.dialect.as_deref()) {
        ("maven", _) => vec![("pom.xml", "pom.xml.j2")],
        ("gradle", Some("kotlin")) if project => vec![
            ("build.gradle.kts", "build.gradle.kts.j2"),
            ("settings.gradle.kts", "settings.gradle.kts.j2"),
        ],
        ("gradle", _) if project => vec![
            ("build.gradle", "build.gradle.j2"),
            ("settings.gradle", "settings.gradle.j2"),
        ],
        ("gradle", _) => vec![("build.gradle", "build.gradle.j2")],
        (other, _) => return Err(OfflineError::UnknownBuild(other.into())),
    };
    let files = files
        .into_iter()
        .map(|(path, template)| Ok((path.to_string(), render(&env, template, &ctx)?)))
        .collect::<Result<_, OfflineError>>()?;
    Ok(Generated { files, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::{details, metadata};

    pub(crate) fn request(
        ptype: &str,
        lang: &str,
        packaging: &str,
        deps: &[&str],
    ) -> ProjectRequest {
        let mut r = ProjectRequest::from_defaults(&metadata());
        r.project_type = ptype.into();
        r.language = lang.into();
        r.packaging = packaging.into();
        r.java_version = "21".into();
        r.name = "demo".into();
        r.dependencies = deps.iter().map(|d| d.to_string()).collect();
        r
    }

    pub(crate) fn generate_build(r: &ProjectRequest) -> Generated {
        let m = metadata();
        let ptype = m.project_type(&r.project_type).expect("known type");
        build_files(r, ptype, &details()).expect("generates")
    }

    fn file<'a>(g: &'a Generated, path: &str) -> &'a str {
        let (_, bytes) = g
            .files
            .iter()
            .find(|(p, _)| p == path)
            .unwrap_or_else(|| panic!("{path} not generated"));
        std::str::from_utf8(bytes).expect("utf8")
    }

    fn paths(g: &Generated) -> Vec<&str> {
        g.files.iter().map(|(p, _)| p.as_str()).collect()
    }

    /// Artifact ids declared in a build file (Maven XML or Gradle coordinates).
    fn artifact_ids(text: &str) -> Vec<String> {
        let maven = regex::Regex::new(r"<artifactId>([^<]+)</artifactId>").expect("regex");
        let gradle = regex::Regex::new(r#"['"]([\w.\-]+):([\w.\-]+)[:'"]"#).expect("regex");
        maven
            .captures_iter(text)
            .map(|c| c[1].to_string())
            .chain(gradle.captures_iter(text).map(|c| c[2].to_string()))
            .collect()
    }

    fn assert_matches_online(fixture: &str, generated: &str) {
        let known_gap = regex::Regex::new(r"^spring-boot-starter-.+-test$").expect("regex");
        for id in artifact_ids(fixture) {
            if known_gap.is_match(&id) {
                continue;
            }
            assert!(
                artifact_ids(generated).contains(&id),
                "offline output is missing {id}:\n{generated}"
            );
        }
    }

    const DEPS: &[&str] = &["web", "data-jpa", "spring-ai-openai"];

    #[test]
    fn matches_online_maven_java_jar() {
        let g = generate_build(&request("maven-build", "java", "jar", DEPS));
        let pom = file(&g, "pom.xml");
        assert_matches_online(
            include_str!("../tests/fixtures/online/maven-java-jar.pom.xml"),
            pom,
        );
        assert!(pom.contains("<spring-ai.version>2.1.0-M1</spring-ai.version>"));
        insta::assert_snapshot!(pom);
    }

    #[test]
    fn matches_online_maven_groovy_jar() {
        let g = generate_build(&request("maven-project", "groovy", "jar", DEPS));
        let pom = file(&g, "pom.xml");
        assert_matches_online(
            include_str!("../tests/fixtures/online/maven-groovy-jar.pom.xml"),
            pom,
        );
        insta::assert_snapshot!(pom);
    }

    #[test]
    fn matches_online_maven_kotlin_war() {
        let g = generate_build(&request("maven-project", "kotlin", "war", DEPS));
        let pom = file(&g, "pom.xml");
        assert_matches_online(
            include_str!("../tests/fixtures/online/maven-kotlin-war.pom.xml"),
            pom,
        );
        insta::assert_snapshot!(pom);
    }

    #[test]
    fn matches_online_gradle_java_jar() {
        let deps = ["web", "data-jpa", "spring-ai-openai", "lombok", "devtools"];
        let g = generate_build(&request("gradle-build", "java", "jar", &deps));
        let build = file(&g, "build.gradle");
        assert_matches_online(
            include_str!("../tests/fixtures/online/gradle-java-jar.build.gradle"),
            build,
        );
        assert!(build.contains("springAiVersion"));
        assert!(build.contains("2.1.0-M1"));
        assert!(build.contains("developmentOnly 'org.springframework.boot:spring-boot-devtools'"));
        insta::assert_snapshot!(build);
    }

    #[test]
    fn gradle_kotlin_war_has_kts_settings() {
        let g = generate_build(&request("gradle-project-kotlin", "kotlin", "war", DEPS));
        assert_eq!(paths(&g), ["build.gradle.kts", "settings.gradle.kts"]);
        let build = file(&g, "build.gradle.kts");
        assert_matches_online(
            include_str!("../tests/fixtures/online/gradle-kotlin-kotlin-war.build.gradle.kts"),
            build,
        );
        assert!(build.contains("providedRuntime"));
        assert_eq!(
            file(&g, "settings.gradle.kts"),
            "rootProject.name = \"demo\"\n"
        );
        insta::assert_snapshot!(build);
    }

    #[test]
    fn build_only_maven_is_single_file() {
        let g = generate_build(&request("maven-build", "java", "jar", &[]));
        assert_eq!(paths(&g), ["pom.xml"]);
    }

    #[test]
    fn build_only_gradle_is_single_file() {
        let g = generate_build(&request("gradle-build", "java", "jar", &[]));
        assert_eq!(paths(&g), ["build.gradle"]);
        assert!(
            file(&g, "build.gradle").contains("'org.springframework.boot:spring-boot-starter'")
        );
    }

    #[test]
    fn maven_lombok_java25_has_processor_paths() {
        let mut r = request("maven-build", "java", "jar", &["lombok"]);
        r.java_version = "25".into();
        let g = generate_build(&r);
        let pom = file(&g, "pom.xml");
        assert!(pom.contains("<annotationProcessorPaths>"), "{pom}");
        assert!(pom.contains("<artifactId>lombok</artifactId>"));
    }

    #[test]
    fn native_warns() {
        let g = generate_build(&request("maven-build", "java", "jar", &["native"]));
        assert!(
            g.warnings
                .contains(&"native: extra build setup not generated offline".to_string()),
            "{:?}",
            g.warnings
        );
    }

    #[test]
    fn missing_coordinates_warns() {
        let g = generate_build(&request("maven-build", "java", "jar", &["nope"]));
        assert!(
            g.warnings
                .contains(&"nope: no cached coordinates, skipped".to_string())
        );
    }
}
