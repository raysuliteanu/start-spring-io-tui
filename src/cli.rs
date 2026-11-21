use std::sync::LazyLock;

use clap::Parser;

static LONG_VERSION: LazyLock<String> = LazyLock::new(|| version_info().long());

#[derive(Debug, Parser)]
#[command(version, long_version = LONG_VERSION.as_str(), about = "A TUI for start.spring.io")]
pub struct Args {
    /// Don't contact start.spring.io; generate from cached data
    #[arg(long)]
    pub offline: bool,
}

#[derive(Debug, Clone)]
pub struct VersionInfo {
    pub version: &'static str,
    pub git_sha: &'static str,
    pub git_dirty: bool,
    pub build_date: &'static str,
    pub rustc: &'static str,
}

impl VersionInfo {
    pub fn long(&self) -> String {
        let dirty = if self.git_dirty { "-dirty" } else { "" };
        format!(
            "{} ({}{dirty} built {}, rustc {})",
            self.version, self.git_sha, self.build_date, self.rustc
        )
    }
}

pub fn version_info() -> VersionInfo {
    VersionInfo {
        version: env!("CARGO_PKG_VERSION"),
        git_sha: env!("VERGEN_GIT_SHA"),
        git_dirty: env!("VERGEN_GIT_DIRTY") == "true",
        build_date: env!("VERGEN_BUILD_DATE"),
        rustc: env!("VERGEN_RUSTC_SEMVER"),
    }
}
