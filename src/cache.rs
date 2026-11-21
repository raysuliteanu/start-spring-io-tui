use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::model::{DependencyDetails, Metadata};

pub struct Cache {
    dir: PathBuf,
}

impl Cache {
    pub fn new(dir: PathBuf) -> Cache {
        Cache { dir }
    }

    pub fn default_dir() -> Option<PathBuf> {
        dirs::cache_dir().map(|d| d.join("start-spring-io-tui"))
    }

    pub fn store_metadata(&self, raw: &str) -> io::Result<()> {
        self.store("metadata.json", raw)
    }

    pub fn load_metadata(&self) -> Option<(Metadata, SystemTime)> {
        let path = self.dir.join("metadata.json");
        let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok()?;
        Some((load(&path)?, modified))
    }

    pub fn store_dependencies(&self, boot: &str, raw: &str) -> io::Result<()> {
        self.store(&deps_file(boot), raw)
    }

    pub fn load_dependencies(&self, boot: &str) -> Option<DependencyDetails> {
        load(&self.dir.join(deps_file(boot)))
    }

    pub fn has_dependencies(&self, boot: &str) -> bool {
        self.load_dependencies(boot).is_some()
    }

    fn store(&self, name: &str, raw: &str) -> io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        std::fs::write(self.dir.join(name), raw)
    }
}

fn deps_file(boot: &str) -> String {
    format!("dependencies-{boot}.json")
}

/// Missing or unparseable cache files are treated as absent.
fn load<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_roundtrip() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cache = Cache::new(tmp.path().join("nested"));
        cache
            .store_metadata(include_str!("../tests/fixtures/metadata.json"))
            .expect("store");
        cache
            .store_dependencies(
                "4.2.0",
                include_str!("../tests/fixtures/dependencies-4.2.0.json"),
            )
            .expect("store");
        let (meta, _) = cache.load_metadata().expect("cached metadata");
        assert_eq!(meta.artifact_id.default, "demo");
        assert!(cache.load_dependencies("4.2.0").is_some());
        assert!(cache.has_dependencies("4.2.0"));
        assert!(!cache.has_dependencies("4.1.3"));
    }

    #[test]
    fn corrupt_cache_is_none() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("metadata.json"), "{not json").expect("write");
        assert!(
            Cache::new(tmp.path().to_path_buf())
                .load_metadata()
                .is_none()
        );
    }
}
