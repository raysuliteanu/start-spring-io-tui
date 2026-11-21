use std::collections::BTreeSet;
use std::time::SystemTime;

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::model::{Dependency, Metadata, ProjectRequest, derive_package_name, sanitize_package};
use crate::version::in_range;
use crate::worker::{Event, Job};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Connecting,
    Online,
    Offline {
        requested: bool,
        cached_at: Option<SystemTime>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Project,
    Dependencies,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Project,
    Language,
    Boot,
    Java,
    Packaging,
    Group,
    Artifact,
    Name,
    Description,
    Package,
}

pub const FIELDS: [Field; 10] = [
    Field::Project,
    Field::Language,
    Field::Boot,
    Field::Java,
    Field::Packaging,
    Field::Group,
    Field::Artifact,
    Field::Name,
    Field::Description,
    Field::Package,
];

impl Field {
    pub fn label(self) -> &'static str {
        match self {
            Field::Project => "Project",
            Field::Language => "Language",
            Field::Boot => "Spring Boot",
            Field::Java => "Java",
            Field::Packaging => "Packaging",
            Field::Group => "Group",
            Field::Artifact => "Artifact",
            Field::Name => "Name",
            Field::Description => "Description",
            Field::Package => "Package name",
        }
    }

    pub fn is_text(self) -> bool {
        matches!(
            self,
            Field::Group | Field::Artifact | Field::Name | Field::Description | Field::Package
        )
    }
}

#[derive(Debug)]
pub struct App {
    pub mode: Mode,
    pub metadata: Option<Metadata>,
    pub req: ProjectRequest,
    pub pane: Pane,
    pub field: Field,
    pub editing: bool,
    pub filter: String,
    pub filtering: bool,
    pub dep_cursor: usize,
    pub cached_boots: BTreeSet<String>,
    pub status: String,
    pub show_about: bool,
    pub quit: bool,
    package_edited: bool,
    name_edited: bool,
}

impl App {
    pub fn new(mode: Mode) -> App {
        App {
            mode,
            metadata: None,
            req: ProjectRequest::default(),
            pane: Pane::Project,
            field: Field::Project,
            editing: false,
            filter: String::new(),
            filtering: false,
            dep_cursor: 0,
            cached_boots: BTreeSet::new(),
            status: String::new(),
            show_about: false,
            quit: false,
            package_edited: false,
            name_edited: false,
        }
    }

    pub fn mode_label(&self) -> String {
        match self.mode {
            Mode::Connecting => "Connecting…",
            Mode::Online => "ONLINE",
            Mode::Offline {
                requested: true, ..
            } => "OFFLINE",
            Mode::Offline { .. } => "OFFLINE (no connection)",
        }
        .to_string()
    }

    fn requested_offline(&self) -> bool {
        matches!(
            self.mode,
            Mode::Offline {
                requested: true,
                ..
            }
        )
    }

    fn set_metadata(&mut self, metadata: Metadata) {
        // Keep the user's selections when metadata arrives again (e.g. retry).
        if self.metadata.is_none() {
            self.req = ProjectRequest::from_defaults(&metadata);
        }
        self.metadata = Some(metadata);
    }

    pub fn on_event(&mut self, ev: Event) -> Option<Job> {
        match ev {
            Event::Online(metadata) => {
                self.mode = Mode::Online;
                self.set_metadata(metadata);
                self.status.clear();
                let boots = self
                    .boot_ids()
                    .into_iter()
                    .filter(|b| !b.contains("SNAPSHOT"))
                    .collect();
                return Some(Job::FetchDependencies(boots));
            }
            Event::Offline {
                metadata,
                cached_at,
                cached_boots,
            } => {
                self.mode = Mode::Offline {
                    requested: self.requested_offline(),
                    cached_at,
                };
                if let Some(metadata) = metadata {
                    self.set_metadata(metadata);
                }
                self.cached_boots.extend(cached_boots);
            }
            Event::DependenciesCached(boot) => {
                self.cached_boots.insert(boot);
            }
            Event::Generated(Ok(report)) => {
                if report.fell_back_offline {
                    self.mode = Mode::Offline {
                        requested: false,
                        cached_at: None,
                    };
                }
                self.status = format!("Wrote {}", report.path.display());
                if report.fell_back_offline {
                    self.status.push_str(" (offline: no connection)");
                }
                if !report.warnings.is_empty() {
                    self.status.push_str(" · ");
                    self.status.push_str(&report.warnings.join("; "));
                }
            }
            Event::Generated(Err(failure)) => {
                if failure.fell_back_offline {
                    self.mode = Mode::Offline {
                        requested: false,
                        cached_at: None,
                    };
                }
                self.status = failure.message;
            }
        }
        None
    }

    pub fn on_key(&mut self, key: KeyEvent) -> Option<Job> {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.quit = true;
            return None;
        }
        if self.metadata.is_none() {
            if key.code == KeyCode::Char('q') {
                self.quit = true;
            }
            return None;
        }
        if self.editing {
            self.edit_key(key.code);
            return None;
        }
        if self.filtering {
            self.filter_key(key.code);
            return None;
        }
        if self.show_about {
            match key.code {
                KeyCode::Char('q') => self.quit = true,
                _ => self.show_about = false,
            }
            return None;
        }
        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => self.show_about = true,
            KeyCode::Tab | KeyCode::BackTab => {
                self.pane = match self.pane {
                    Pane::Project => Pane::Dependencies,
                    Pane::Dependencies => Pane::Project,
                }
            }
            KeyCode::Char('/') => {
                self.pane = Pane::Dependencies;
                self.filtering = true;
            }
            KeyCode::Char('g') => return self.generate(),
            KeyCode::Char('r') if self.degraded() => {
                self.mode = Mode::Connecting;
                self.status = "Retrying connection…".into();
                return Some(Job::LoadMetadata);
            }
            code => match self.pane {
                Pane::Project => return self.project_key(code),
                Pane::Dependencies => self.dependencies_key(code),
            },
        }
        None
    }

    fn degraded(&self) -> bool {
        matches!(
            self.mode,
            Mode::Offline {
                requested: false,
                ..
            }
        )
    }

    fn generate(&mut self) -> Option<Job> {
        if self.mode == Mode::Connecting {
            self.status = "Still connecting…".into();
            return None;
        }
        if self.req.artifact_id.trim().is_empty() {
            self.status = "Artifact is required".into();
            return None;
        }
        // The artifact names the output file, so keep it a plain file name.
        let artifact = &self.req.artifact_id;
        if artifact.starts_with('.')
            || !artifact
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        {
            self.status = "Artifact may only use letters, digits, '.', '_' and '-'".into();
            return None;
        }
        if sanitize_package(&self.req.package_name).is_empty() {
            self.status = "Package name is required".into();
            return None;
        }
        let ptype = self
            .metadata
            .as_ref()?
            .project_type(&self.req.project_type)?
            .clone();
        self.status = "Generating…".into();
        Some(Job::Generate {
            req: self.req.clone(),
            ptype,
            online: self.mode == Mode::Online,
        })
    }

    fn project_key(&mut self, code: KeyCode) -> Option<Job> {
        let index = FIELDS.iter().position(|f| *f == self.field).unwrap_or(0);
        match code {
            KeyCode::Up => self.field = FIELDS[index.saturating_sub(1)],
            KeyCode::Down => self.field = FIELDS[(index + 1).min(FIELDS.len() - 1)],
            KeyCode::Enter if self.field.is_text() => self.editing = true,
            KeyCode::Left => return self.cycle(-1),
            KeyCode::Right => return self.cycle(1),
            _ => {}
        }
        None
    }

    fn select_ids(&self, field: Field) -> Vec<String> {
        let Some(m) = &self.metadata else {
            return Vec::new();
        };
        match field {
            Field::Project => m.project_type.values.iter().map(|v| v.id.clone()).collect(),
            Field::Language => m.language.values.iter().map(|v| v.id.clone()).collect(),
            Field::Boot => self.boot_ids(),
            Field::Java => m.java_version.values.iter().map(|v| v.id.clone()).collect(),
            Field::Packaging => m.packaging.values.iter().map(|v| v.id.clone()).collect(),
            _ => Vec::new(),
        }
    }

    fn boot_ids(&self) -> Vec<String> {
        self.metadata
            .iter()
            .flat_map(|m| &m.boot_version.values)
            .map(|v| v.id.clone())
            .collect()
    }

    fn cycle(&mut self, step: isize) -> Option<Job> {
        let ids = self.select_ids(self.field);
        if ids.is_empty() {
            return None;
        }
        let value = match self.field {
            Field::Project => &mut self.req.project_type,
            Field::Language => &mut self.req.language,
            Field::Boot => &mut self.req.boot_version,
            Field::Java => &mut self.req.java_version,
            Field::Packaging => &mut self.req.packaging,
            _ => return None,
        };
        let current = ids.iter().position(|id| id == value).unwrap_or(0) as isize;
        let next = (current + step).rem_euclid(ids.len() as isize) as usize;
        *value = ids[next].clone();
        if self.field == Field::Boot {
            return self.boot_changed();
        }
        None
    }

    fn boot_changed(&mut self) -> Option<Job> {
        let boot = self.req.boot_version.clone();
        let removed: Vec<String> = self
            .req
            .dependencies
            .iter()
            .filter(|id| !self.is_compatible(id))
            .cloned()
            .collect();
        self.req.dependencies.retain(|id| !removed.contains(id));
        self.status = if removed.is_empty() {
            String::new()
        } else {
            format!(
                "Removed {} (not compatible with Boot {boot})",
                removed.join(", ")
            )
        };
        (self.mode == Mode::Online && !self.cached_boots.contains(&boot))
            .then(|| Job::FetchDependencies(vec![boot]))
    }

    fn is_compatible(&self, id: &str) -> bool {
        self.metadata
            .iter()
            .flat_map(|m| &m.dependencies.values)
            .flat_map(|g| &g.values)
            .find(|d| d.id == id)
            .is_some_and(|d| self.dep_compatible(d))
    }

    fn dep_compatible(&self, d: &Dependency) -> bool {
        d.version_range
            .as_deref()
            .is_none_or(|r| in_range(r, &self.req.boot_version))
    }

    fn edit_key(&mut self, code: KeyCode) {
        let field = self.field;
        let value = match field {
            Field::Group => &mut self.req.group_id,
            Field::Artifact => &mut self.req.artifact_id,
            Field::Name => &mut self.req.name,
            Field::Description => &mut self.req.description,
            Field::Package => &mut self.req.package_name,
            _ => {
                self.editing = false;
                return;
            }
        };
        match code {
            KeyCode::Char(c) => value.push(c),
            KeyCode::Backspace => {
                value.pop();
            }
            KeyCode::Enter | KeyCode::Esc => {
                self.editing = false;
                return;
            }
            _ => return,
        }
        match field {
            Field::Package => self.package_edited = true,
            Field::Name => self.name_edited = true,
            _ => {}
        }
        if matches!(field, Field::Group | Field::Artifact) {
            if !self.package_edited {
                self.req.package_name =
                    derive_package_name(&self.req.group_id, &self.req.artifact_id);
            }
            if field == Field::Artifact && !self.name_edited {
                self.req.name = self.req.artifact_id.clone();
            }
        }
    }

    fn filter_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char(c) => self.filter.push(c),
            KeyCode::Backspace => {
                self.filter.pop();
            }
            KeyCode::Esc => {
                self.filter.clear();
                self.filtering = false;
            }
            KeyCode::Enter => self.filtering = false,
            _ => return,
        }
        self.dep_cursor = 0;
    }

    fn dependencies_key(&mut self, code: KeyCode) {
        let len = self.visible_dependencies().len();
        match code {
            KeyCode::Up => self.dep_cursor = self.dep_cursor.saturating_sub(1),
            KeyCode::Down => self.dep_cursor = (self.dep_cursor + 1).min(len.saturating_sub(1)),
            KeyCode::Char('X') => {
                self.req.dependencies.clear();
                self.status = "Cleared all dependencies".into();
            }
            KeyCode::Char(' ') => {
                let Some((_, dep, compatible)) =
                    self.visible_dependencies().get(self.dep_cursor).cloned()
                else {
                    return;
                };
                if !compatible {
                    self.status = format!(
                        "{} is not compatible with Boot {}",
                        dep.name, self.req.boot_version
                    );
                    return;
                }
                let id = dep.id.clone();
                if let Some(pos) = self.req.dependencies.iter().position(|d| *d == id) {
                    self.req.dependencies.remove(pos);
                } else {
                    self.req.dependencies.push(id);
                }
            }
            _ => {}
        }
    }

    /// (group name, dependency, compatible with the selected Boot version)
    pub fn visible_dependencies(&self) -> Vec<(&str, &Dependency, bool)> {
        let all = self
            .metadata
            .iter()
            .flat_map(|m| &m.dependencies.values)
            .flat_map(|g| g.values.iter().map(move |d| (g.name.as_str(), d)))
            .map(|(g, d)| (g, d, self.dep_compatible(d)));
        if self.filter.is_empty() {
            return all.collect();
        }
        let pattern = Pattern::parse(&self.filter, CaseMatching::Smart, Normalization::Smart);
        let mut matcher = Matcher::new(Config::DEFAULT);
        let mut buf = Vec::new();
        let mut scored: Vec<_> = all
            .filter_map(|item| {
                let haystack = format!("{} {} {}", item.1.name, item.1.id, item.0);
                pattern
                    .score(Utf32Str::new(&haystack, &mut buf), &mut matcher)
                    .map(|score| (score, item))
            })
            .collect();
        scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        scored.into_iter().map(|(_, item)| item).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generate::{Failure, Report};
    use crate::model::tests::metadata;
    use std::path::PathBuf;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ch(c: char) -> KeyEvent {
        key(KeyCode::Char(c))
    }

    fn online_app() -> App {
        let mut app = App::new(Mode::Connecting);
        app.on_event(Event::Online(metadata()));
        app
    }

    fn type_text(app: &mut App, text: &str) {
        for c in text.chars() {
            app.on_key(ch(c));
        }
    }

    fn focus(app: &mut App, field: Field) {
        app.field = field;
        app.pane = Pane::Project;
    }

    /// Replaces a text field's value via the keyboard.
    fn edit(app: &mut App, field: Field, value: &str) {
        focus(app, field);
        app.on_key(key(KeyCode::Enter));
        for _ in 0..64 {
            app.on_key(key(KeyCode::Backspace));
        }
        type_text(app, value);
        app.on_key(key(KeyCode::Enter));
    }

    fn cursor_to(app: &mut App, id: &str) {
        app.pane = Pane::Dependencies;
        app.dep_cursor = app
            .visible_dependencies()
            .iter()
            .position(|(_, d, _)| d.id == id)
            .expect("dependency visible");
    }

    fn set_boot(app: &mut App, boot: &str) -> Option<Job> {
        focus(app, Field::Boot);
        for _ in 0..20 {
            if app.req.boot_version == boot {
                return None;
            }
            if let Some(job) = app.on_key(key(KeyCode::Left))
                && app.req.boot_version == boot
            {
                return Some(job);
            }
        }
        panic!("boot {boot} not reachable");
    }

    #[test]
    fn labels() {
        assert_eq!(App::new(Mode::Online).mode_label(), "ONLINE");
        let off = |requested| Mode::Offline {
            requested,
            cached_at: None,
        };
        assert_eq!(App::new(off(true)).mode_label(), "OFFLINE");
        assert_eq!(App::new(off(false)).mode_label(), "OFFLINE (no connection)");
        assert_eq!(App::new(Mode::Connecting).mode_label(), "Connecting…");
    }

    #[test]
    fn online_event_prefetches_release_boots() {
        let mut app = App::new(Mode::Connecting);
        match app.on_event(Event::Online(metadata())) {
            Some(Job::FetchDependencies(boots)) => {
                assert!(boots.contains(&"4.2.0".to_string()));
                assert!(boots.iter().all(|b| !b.contains("SNAPSHOT")), "{boots:?}");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(app.mode, Mode::Online);
        assert_eq!(app.req.artifact_id, "demo");
    }

    #[test]
    fn package_follows_artifact_until_edited() {
        let mut app = online_app();
        edit(&mut app, Field::Artifact, "myapp");
        assert_eq!(app.req.package_name, "com.example.myapp");
        assert_eq!(app.req.name, "myapp");
        edit(&mut app, Field::Package, "x.y");
        edit(&mut app, Field::Artifact, "other");
        assert_eq!(app.req.package_name, "x.y");
    }

    #[test]
    fn incompatible_dep_cannot_be_toggled() {
        let mut app = online_app();
        cursor_to(&mut app, "vaadin");
        app.on_key(ch(' '));
        assert!(app.req.dependencies.is_empty());
    }

    #[test]
    fn boot_change_drops_incompatible() {
        let mut app = online_app();
        set_boot(&mut app, "4.1.3");
        cursor_to(&mut app, "vaadin");
        app.on_key(ch(' '));
        assert_eq!(app.req.dependencies, ["vaadin"]);
        set_boot(&mut app, "4.2.0");
        assert!(app.req.dependencies.is_empty());
        assert!(app.status.contains("vaadin"), "{}", app.status);
    }

    #[test]
    fn boot_change_online_fetches_uncached_version() {
        let mut app = online_app();
        let job = set_boot(&mut app, "4.2.1-SNAPSHOT");
        assert!(
            matches!(&job, Some(Job::FetchDependencies(v)) if v == &["4.2.1-SNAPSHOT"]),
            "{job:?}"
        );
    }

    #[test]
    fn fuzzy_filter_ranks_best_first() {
        let mut app = online_app();
        app.on_key(ch('/'));
        type_text(&mut app, "sweb");
        assert_eq!(app.visible_dependencies()[0].1.id, "web");
    }

    #[test]
    fn fuzzy_filter_matches_category() {
        let mut app = online_app();
        app.filter = "ai".into();
        let visible: Vec<&str> = app
            .visible_dependencies()
            .iter()
            .map(|(_, d, _)| d.id.as_str())
            .collect();
        let m = metadata();
        let ai = m
            .dependencies
            .values
            .iter()
            .find(|g| g.name == "AI")
            .expect("AI group");
        for dep in &ai.values {
            assert!(visible.contains(&dep.id.as_str()), "{} missing", dep.id);
        }
    }

    #[test]
    fn empty_filter_is_grouped() {
        let app = online_app();
        let expected: Vec<String> = metadata()
            .dependencies
            .values
            .iter()
            .flat_map(|g| g.values.iter().map(|d| d.id.clone()))
            .collect();
        let visible: Vec<String> = app
            .visible_dependencies()
            .iter()
            .map(|(_, d, _)| d.id.clone())
            .collect();
        assert_eq!(visible, expected);
    }

    #[test]
    fn clear_all_deps() {
        let mut app = online_app();
        app.req.dependencies = vec!["web".into(), "lombok".into()];
        app.pane = Pane::Dependencies;
        app.on_key(ch('X'));
        assert!(app.req.dependencies.is_empty());
    }

    #[test]
    fn filter_typing_is_text_and_esc_clears() {
        let mut app = online_app();
        app.on_key(ch('/'));
        type_text(&mut app, "qgX");
        assert_eq!(app.filter, "qgX");
        assert!(!app.quit);
        app.on_key(key(KeyCode::Esc));
        assert!(app.filter.is_empty());
        assert!(!app.filtering);
    }

    #[test]
    fn empty_artifact_refuses_generate() {
        let mut app = online_app();
        edit(&mut app, Field::Artifact, "");
        assert!(app.on_key(ch('g')).is_none());
        assert!(!app.status.is_empty());
    }

    #[test]
    fn generate_sends_job() {
        let mut app = online_app();
        match app.on_key(ch('g')) {
            Some(Job::Generate { req, ptype, online }) => {
                assert!(online);
                assert_eq!(req.artifact_id, "demo");
                assert_eq!(ptype.id, "gradle-project");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn retry_only_when_degraded() {
        let mut degraded = App::new(Mode::Offline {
            requested: false,
            cached_at: None,
        });
        degraded.on_event(Event::Offline {
            metadata: Some(metadata()),
            cached_at: None,
            cached_boots: vec![],
        });
        assert!(matches!(degraded.on_key(ch('r')), Some(Job::LoadMetadata)));
        assert_eq!(degraded.mode, Mode::Connecting);

        let mut forced = App::new(Mode::Offline {
            requested: true,
            cached_at: None,
        });
        forced.on_event(Event::Offline {
            metadata: Some(metadata()),
            cached_at: None,
            cached_boots: vec![],
        });
        assert!(forced.on_key(ch('r')).is_none());
        assert_eq!(forced.mode_label(), "OFFLINE");
    }

    #[test]
    fn q_while_editing_is_text() {
        let mut app = online_app();
        focus(&mut app, Field::Group);
        app.on_key(key(KeyCode::Enter));
        app.on_key(ch('q'));
        assert_eq!(app.req.group_id, "com.exampleq");
        assert!(!app.quit);
    }

    #[test]
    fn q_quits() {
        let mut app = online_app();
        app.on_key(ch('q'));
        assert!(app.quit);
    }

    #[test]
    fn fell_back_sets_offline() {
        let mut app = online_app();
        app.on_event(Event::Generated(Ok(Report {
            path: PathBuf::from("/tmp/demo.zip"),
            warnings: vec!["native: extra build setup not generated offline".into()],
            fell_back_offline: true,
        })));
        assert_eq!(app.mode_label(), "OFFLINE (no connection)");
        assert!(app.status.contains("/tmp/demo.zip"), "{}", app.status);
        assert!(app.status.contains("native"), "{}", app.status);
    }

    #[test]
    fn offline_event_records_cached_boots() {
        let mut app = App::new(Mode::Connecting);
        app.on_event(Event::Offline {
            metadata: Some(metadata()),
            cached_at: None,
            cached_boots: vec!["4.2.0".into()],
        });
        assert_eq!(app.mode_label(), "OFFLINE (no connection)");
        assert!(app.cached_boots.contains("4.2.0"));
    }

    #[test]
    fn unsafe_artifact_refuses_generate() {
        for bad in ["../x", "a/b", "/tmp/x", "a\\b", "my app", ".hidden"] {
            let mut app = online_app();
            app.req.artifact_id = bad.into();
            assert!(app.on_key(ch('g')).is_none(), "{bad} accepted");
            assert!(app.status.contains("Artifact"), "{bad}: {}", app.status);
        }
    }

    #[test]
    fn empty_package_refuses_generate() {
        let mut app = online_app();
        app.req.package_name = "---".into();
        assert!(app.on_key(ch('g')).is_none());
        assert!(app.status.contains("Package"), "{}", app.status);
    }

    #[test]
    fn failed_fallback_sets_offline() {
        let mut app = online_app();
        app.on_event(Event::Generated(Err(Failure {
            message: "No cached dependency data".into(),
            fell_back_offline: true,
        })));
        assert_eq!(app.mode_label(), "OFFLINE (no connection)");
        assert!(app.status.contains("No cached"), "{}", app.status);
    }
}
