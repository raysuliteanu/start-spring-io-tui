use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::SystemTime;

use crate::api::Client;
use crate::cache::Cache;
use crate::generate::{self, Report};
use crate::model::{Metadata, ProjectRequest, ProjectType};

#[derive(Debug)]
// Jobs are sent once per user action, so variant size doesn't matter.
#[allow(clippy::large_enum_variant)]
pub enum Job {
    LoadMetadata,
    FetchDependencies(Vec<String>),
    Generate {
        req: ProjectRequest,
        ptype: ProjectType,
        online: bool,
    },
}

#[derive(Debug)]
pub enum Event {
    Online(Metadata),
    Offline {
        metadata: Option<Metadata>,
        cached_at: Option<SystemTime>,
        /// Boot versions with cached dependency details (offline-generatable).
        cached_boots: Vec<String>,
    },
    DependenciesCached(String),
    Generated(Result<Report, String>),
}

/// Runs blocking network/cache/disk work off the UI thread. The thread ends
/// when the returned `Sender` is dropped.
pub fn spawn(
    client: Option<Client>,
    cache: Cache,
    out_dir: PathBuf,
) -> (Sender<Job>, Receiver<Event>) {
    let (job_tx, job_rx) = mpsc::channel::<Job>();
    let (event_tx, event_rx) = mpsc::channel();
    std::thread::spawn(move || {
        for job in job_rx {
            let sent = match job {
                Job::LoadMetadata => {
                    let online = client.as_ref().and_then(|c| c.metadata().ok());
                    match online {
                        Some((metadata, raw)) => {
                            // Caching is best effort; a failed write only costs offline use later.
                            let _ = cache.store_metadata(&raw);
                            event_tx.send(Event::Online(metadata))
                        }
                        None => {
                            let cached = cache.load_metadata();
                            let cached_boots = cached
                                .iter()
                                .flat_map(|(m, _)| &m.boot_version.values)
                                .filter(|b| cache.has_dependencies(&b.id))
                                .map(|b| b.id.clone())
                                .collect();
                            event_tx.send(Event::Offline {
                                cached_boots,
                                cached_at: cached.as_ref().map(|(_, t)| *t),
                                metadata: cached.map(|(m, _)| m),
                            })
                        }
                    }
                }
                Job::FetchDependencies(boots) => {
                    let Some(client) = &client else { continue };
                    let mut sent = Ok(());
                    for boot in boots {
                        if let Ok((_, raw)) = client.dependencies(&boot)
                            && cache.store_dependencies(&boot, &raw).is_ok()
                        {
                            sent = event_tx.send(Event::DependenciesCached(boot));
                        }
                    }
                    sent
                }
                Job::Generate { req, ptype, online } => {
                    let client = client.as_ref().filter(|_| online);
                    let result = generate::run(client, &cache, &req, &ptype, &out_dir)
                        .map_err(|e| e.to_string());
                    event_tx.send(Event::Generated(result))
                }
            };
            if sent.is_err() {
                break;
            }
        }
    });
    (job_tx, event_rx)
}

#[cfg(test)]
mod tests {
    use super::*;

    const METADATA: &str = include_str!("../tests/fixtures/metadata.json");

    #[test]
    fn load_metadata_online_caches_it() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut server = mockito::Server::new();
        server.mock("GET", "/").with_body(METADATA).create();
        let (jobs, events) = spawn(
            Some(Client::new(&server.url())),
            Cache::new(tmp.path().to_path_buf()),
            tmp.path().to_path_buf(),
        );
        jobs.send(Job::LoadMetadata).expect("worker alive");
        assert!(matches!(events.recv().expect("event"), Event::Online(_)));
        assert!(
            Cache::new(tmp.path().to_path_buf())
                .load_metadata()
                .is_some()
        );
    }

    #[test]
    fn load_metadata_unreachable_uses_cache() {
        let tmp = tempfile::tempdir().expect("tempdir");
        Cache::new(tmp.path().to_path_buf())
            .store_metadata(METADATA)
            .expect("seed");
        let (jobs, events) = spawn(
            Some(Client::new("http://127.0.0.1:9")),
            Cache::new(tmp.path().to_path_buf()),
            tmp.path().to_path_buf(),
        );
        jobs.send(Job::LoadMetadata).expect("worker alive");
        match events.recv().expect("event") {
            Event::Offline {
                metadata: Some(_),
                cached_at: Some(_),
                ..
            } => {}
            other => panic!("unexpected {other:?}"),
        }
    }
}
