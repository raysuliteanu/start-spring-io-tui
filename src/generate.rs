use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::api::{ApiError, Client};
use crate::cache::Cache;
use crate::model::{ProjectRequest, ProjectType};
use crate::offline::{self, OfflineError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub path: PathBuf,
    pub warnings: Vec<String>,
    pub fell_back_offline: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum GenerateError {
    #[error("{} already exists", .0.display())]
    Exists(PathBuf),
    #[error("No cached dependency data for Boot {0} — choose a cached version")]
    NoCachedDependencies(String),
    #[error(transparent)]
    Api(#[from] ApiError),
    #[error(transparent)]
    Offline(#[from] OfflineError),
    #[error("cannot write output: {0}")]
    Io(#[from] io::Error),
    /// The service was unreachable and the offline fallback failed too.
    #[error("No connection; {0}")]
    AfterFallback(Box<GenerateError>),
}

/// A failed generation, as reported to the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub message: String,
    /// The online request couldn't connect, so we tried (and failed) offline.
    pub fell_back_offline: bool,
}

impl GenerateError {
    pub fn fell_back_offline(&self) -> bool {
        matches!(self, GenerateError::AfterFallback(_))
    }
}

pub fn target_file(req: &ProjectRequest, ptype: &ProjectType) -> String {
    match (ptype.tags.format.as_str(), ptype.tags.build.as_str()) {
        ("project", _) => format!("{}.zip", req.artifact_id),
        (_, "maven") => "pom.xml".into(),
        _ => "build.gradle".into(),
    }
}

/// Generates into `out_dir`. `client == None` means offline.
pub fn run(
    client: Option<&Client>,
    cache: &Cache,
    req: &ProjectRequest,
    ptype: &ProjectType,
    out_dir: &Path,
) -> Result<Report, GenerateError> {
    let path = out_dir.join(target_file(req, ptype));
    // Checked up front so we don't download for nothing; create_new below
    // still guards against the file appearing in between.
    if path.exists() {
        return Err(GenerateError::Exists(path));
    }
    let mut fell_back_offline = false;
    if let Some(client) = client {
        // A captive portal answers 200 with HTML; only a real zip counts as online success.
        let is_project = ptype.tags.format == "project";
        match client.generate(&ptype.action, req) {
            Ok(bytes) if is_project && !bytes.starts_with(b"PK\x03\x04") => {
                fell_back_offline = true
            }
            Ok(bytes) => {
                write_new(&path, &bytes)?;
                return Ok(Report {
                    path,
                    warnings: Vec::new(),
                    fell_back_offline,
                });
            }
            Err(e) if e.is_connectivity() => fell_back_offline = true,
            Err(e) => return Err(e.into()),
        }
    }
    let result = generate_offline(cache, req, ptype, &path);
    match result {
        Ok(warnings) => Ok(Report {
            path,
            warnings,
            fell_back_offline,
        }),
        Err(e) if fell_back_offline => Err(GenerateError::AfterFallback(Box::new(e))),
        Err(e) => Err(e),
    }
}

fn generate_offline(
    cache: &Cache,
    req: &ProjectRequest,
    ptype: &ProjectType,
    path: &Path,
) -> Result<Vec<String>, GenerateError> {
    let details = cache
        .load_dependencies(&req.boot_version)
        .ok_or_else(|| GenerateError::NoCachedDependencies(req.boot_version.clone()))?;
    let generated = offline::generate(req, ptype, &details)?;
    let bytes = if ptype.tags.format == "project" {
        offline::to_zip(&req.artifact_id, &generated.files)
    } else {
        generated
            .files
            .into_iter()
            .next()
            .map(|(_, bytes)| bytes)
            .unwrap_or_default()
    };
    write_new(path, &bytes)?;
    Ok(generated.warnings)
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), GenerateError> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| match e.kind() {
            io::ErrorKind::AlreadyExists => GenerateError::Exists(path.to_path_buf()),
            _ => GenerateError::Io(e),
        })?;
    file.write_all(bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::metadata;

    const DEPS: &str = include_str!("../tests/fixtures/dependencies-4.2.0.json");

    fn setup(ptype: &str) -> (tempfile::TempDir, Cache, ProjectRequest, ProjectType) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cache = Cache::new(tmp.path().join("cache"));
        let m = metadata();
        let mut req = ProjectRequest::from_defaults(&m);
        req.project_type = ptype.into();
        req.name = "demo".into();
        let ptype = m.project_type(ptype).expect("known type").clone();
        (tmp, cache, req, ptype)
    }

    fn zip_names(path: &Path) -> Vec<String> {
        let file = std::fs::File::open(path).expect("zip exists");
        let mut zip = zip::ZipArchive::new(file).expect("valid zip");
        (0..zip.len())
            .map(|i| {
                zip.by_index(i)
                    .expect("entry")
                    .name()
                    .expect("utf8")
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn target_files() {
        let (_tmp, _, req, ptype) = setup("gradle-project");
        assert_eq!(target_file(&req, &ptype), "demo.zip");
        let (_tmp, _, req, ptype) = setup("maven-build");
        assert_eq!(target_file(&req, &ptype), "pom.xml");
        let (_tmp, _, req, ptype) = setup("gradle-build");
        assert_eq!(target_file(&req, &ptype), "build.gradle");
    }

    #[test]
    fn online_writes_zip() {
        let (tmp, cache, req, ptype) = setup("gradle-project");
        let mut server = mockito::Server::new();
        server
            .mock("GET", "/starter.zip")
            .match_query(mockito::Matcher::Any)
            .with_body(b"PK\x03\x04rest")
            .create();
        let client = Client::new(&server.url());
        let report = run(Some(&client), &cache, &req, &ptype, tmp.path()).expect("ok");
        assert_eq!(report.path, tmp.path().join("demo.zip"));
        assert!(!report.fell_back_offline);
        assert_eq!(
            std::fs::read(&report.path).expect("written"),
            b"PK\x03\x04rest"
        );
    }

    #[test]
    fn refuses_existing_file() {
        let (tmp, cache, req, ptype) = setup("gradle-project");
        cache.store_dependencies("4.2.0", DEPS).expect("seed");
        let existing = tmp.path().join("demo.zip");
        std::fs::write(&existing, "keep").expect("write");

        let err = run(None, &cache, &req, &ptype, tmp.path()).expect_err("offline exists");
        assert!(matches!(err, GenerateError::Exists(_)), "{err:?}");

        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/starter.zip")
            .match_query(mockito::Matcher::Any)
            .with_body(b"PK")
            .expect(0)
            .create();
        let client = Client::new(&server.url());
        let err = run(Some(&client), &cache, &req, &ptype, tmp.path()).expect_err("online exists");
        assert!(matches!(err, GenerateError::Exists(_)), "{err:?}");
        mock.assert();
        assert_eq!(std::fs::read_to_string(&existing).expect("read"), "keep");
    }

    #[test]
    fn falls_back_offline_on_connect_error() {
        let (tmp, cache, req, ptype) = setup("gradle-project");
        cache.store_dependencies("4.2.0", DEPS).expect("seed");
        let client = Client::new("http://127.0.0.1:9");
        let report = run(Some(&client), &cache, &req, &ptype, tmp.path()).expect("ok");
        assert!(report.fell_back_offline);
        assert!(zip_names(&report.path).contains(&"demo/build.gradle".to_string()));
    }

    #[test]
    fn offline_without_cached_boot_errors() {
        let (tmp, cache, req, ptype) = setup("gradle-project");
        let err = run(None, &cache, &req, &ptype, tmp.path()).expect_err("no cache");
        assert!(
            matches!(&err, GenerateError::NoCachedDependencies(v) if v == "4.2.0"),
            "{err:?}"
        );
        assert_eq!(
            err.to_string(),
            "No cached dependency data for Boot 4.2.0 — choose a cached version"
        );
    }

    #[test]
    fn build_only_writes_pom() {
        let (tmp, cache, req, ptype) = setup("maven-build");
        cache.store_dependencies("4.2.0", DEPS).expect("seed");
        let report = run(None, &cache, &req, &ptype, tmp.path()).expect("ok");
        assert_eq!(report.path, tmp.path().join("pom.xml"));
        assert!(
            std::fs::read_to_string(&report.path)
                .expect("written")
                .contains("<artifactId>spring-boot-starter</artifactId>")
        );
    }

    #[test]
    fn non_zip_response_falls_back_offline() {
        let (tmp, cache, req, ptype) = setup("gradle-project");
        cache.store_dependencies("4.2.0", DEPS).expect("seed");
        let mut server = mockito::Server::new();
        server
            .mock("GET", "/starter.zip")
            .match_query(mockito::Matcher::Any)
            .with_body("<html>Sign in to the wifi</html>")
            .create();
        let client = Client::new(&server.url());
        let report = run(Some(&client), &cache, &req, &ptype, tmp.path()).expect("ok");
        assert!(report.fell_back_offline);
        assert!(zip_names(&report.path).contains(&"demo/build.gradle".to_string()));
    }

    #[test]
    fn failed_fallback_reports_no_connection() {
        let (tmp, cache, req, ptype) = setup("gradle-project");
        let client = Client::new("http://127.0.0.1:9");
        let err = run(Some(&client), &cache, &req, &ptype, tmp.path()).expect_err("no cache");
        assert!(err.fell_back_offline(), "{err:?}");
        assert!(
            err.to_string().contains("No cached dependency data"),
            "{err}"
        );
    }
}
