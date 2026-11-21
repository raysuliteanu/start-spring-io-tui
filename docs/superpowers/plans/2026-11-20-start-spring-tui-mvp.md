# start-spring-io-tui MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Rust TUI that generates Spring Boot starter projects like start.spring.io — online via the live service, offline via locally cached metadata and embedded templates.

**Architecture:** Single binary crate. Pure modules (model, version, offline generator, app state) are unit-tested; a worker thread runs blocking HTTP/cache/disk work and talks to the UI loop over `mpsc` channels; ratatui renders `App` state.

**Tech Stack:** Rust (pinned via `rust-toolchain.toml`), ratatui + crossterm, reqwest (blocking, rustls), serde/serde_json, minijinja, zip, dirs, clap (derive), thiserror, vergen-gix. Dev: mockito, tempfile, assert_cmd.

**Spec:** `docs/superpowers/specs/2026-11-20-start-spring-tui-design.md`

## Global Constraints

- Source control is `jj`; commit with `jj commit -m "<conventional commit>"` (never `jj desc && jj new`, no AI trailers).
- Add crates with `cargo add`. Custom errors use `thiserror`. Prefer `expect("why")` over `unwrap()` in non-test code.
- Before each commit: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` all clean.
- Default service URL `https://start.spring.io`; metadata `Accept: application/vnd.initializr.v2.2+json`; metadata fetch timeout 5s.
- Status bar mode labels exactly: `ONLINE`, `OFFLINE` (`--offline` given), `OFFLINE (no connection)`, plus `Connecting…` while loading.
- `g` writes `./<artifactId>.zip` (zip with top dir `<artifactId>/`) for project types, `./pom.xml` / `./build.gradle` for build-only types, into the process's current directory; never overwrite an existing file.
- Online generation passes `baseDir=<artifactId>`.
- Cache dir: `dirs::cache_dir()/start-spring-io-tui/` with `metadata.json` and `dependencies-<bootVersion>.json`; cache age = file mtime.
- Offline plugin-version constants live in `src/offline.rs`: `DEPENDENCY_MANAGEMENT_PLUGIN_VERSION = "1.1.7"`, `KOTLIN_VERSION = "2.4.21"` (values as observed from the live service on 2026-11-20); others copied from captured fixtures.
- Offline dependencies needing build config the cache doesn't describe (`native`, `testcontainers`) are allowed; generation adds a warning naming them. Any selected id missing from cached details is skipped with a warning.

## Review Focus

1. Odd identifiers (`groupId="Com.Example"`, `artifactId="my-app 2"`) → package derives to a valid Java package (`com.example.myapp2`); empty artifactId refuses to generate with a status message. Tests in Tasks 3 and 8.
2. Target file already exists → error message, existing file untouched, both online and offline. Test in Task 7.
3. Network drops after startup (online generate fails to connect) → falls back to offline generation and mode becomes `OFFLINE (no connection)`. Test in Task 7.
4. Corrupt or partial cache JSON → treated as no cache, not a crash. Test in Task 4.
5. Tiny terminal (40×10) or long filtered list → renders without panic, cursor stays in view. Test in Task 9.

---

### Task 1: Scaffold, CLI and version info

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `build.rs`, `src/main.rs`, `src/cli.rs`, `.gitignore`
- Test: `tests/cli.rs`

**Interfaces:**
- Produces: `cli::Args { pub offline: bool }` (clap derive, `--offline`, `--version` with `long_version`), `cli::version_info() -> VersionInfo { version, git_sha, git_dirty: bool, build_date, rustc }` and `VersionInfo::long() -> String` formatted `"{version} ({sha}[-dirty] built {date}, rustc {rustc})"`. Env vars come from vergen: `VERGEN_GIT_SHA`, `VERGEN_GIT_DIRTY`, `VERGEN_BUILD_DATE`, `VERGEN_RUSTC_SEMVER`.

- [ ] **Step 1:** `cargo init --name start-spring-io-tui`; write `rust-toolchain.toml` with `channel = "1.100.0"`, `components = ["rustfmt", "clippy"]`. `.gitignore` contains `/target`. No `mise.toml` yet (nothing to manage — add when an env var is needed).
- [ ] **Step 2: Write failing test** `tests/cli.rs`:
  - `version_flag_shows_git_and_rustc`: run binary with `--version` (assert_cmd); stdout contains `env!("CARGO_PKG_VERSION")`, `"rustc "` and `" built "`.
  - `help_lists_offline`: `--help` stdout contains `--offline`.
- [ ] **Step 3:** Run `cargo test --test cli` → FAIL.
- [ ] **Step 4:** `build.rs` using `vergen-gix` emitting build date, rustc semver, git sha (short) and dirty flag. Implement `cli.rs`; `main.rs` parses args and exits (TUI comes in Task 9).
- [ ] **Step 5:** `cargo test --test cli` → PASS. Confirm `cargo run -- --version` shows a real sha (repo is a colocated jj repo; `.git` exists).
- [ ] **Step 6:** `jj commit -m "feat: scaffold crate with CLI and build version info"`

### Task 2: Metadata model and version ranges

**Files:**
- Create: `src/model.rs`, `src/version.rs`, `tests/fixtures/metadata.json`, `tests/fixtures/dependencies-4.2.0.json`

**Interfaces:**
- Produces (`model.rs`, all `Deserialize + Serialize + Clone + Debug`, camelCase renames):
  - `Metadata { dependencies: Groups<DependencyGroup>, #[serde(rename="type")] project_type: Select<ProjectType>, packaging: Select<Choice>, java_version: Select<Choice>, language: Select<Choice>, boot_version: Select<Choice>, group_id: TextField, artifact_id: TextField, version: TextField, name: TextField, description: TextField, package_name: TextField }`
  - `Select<T> { default: Option<String>, values: Vec<T> }`, `Groups<T> { values: Vec<T> }`, `Choice { id, name }`, `TextField { default: String }`
  - `ProjectType { id, name, action, tags: ProjectTags }`, `ProjectTags { build: String, dialect: Option<String>, format: String }`
  - `DependencyGroup { name, values: Vec<Dependency> }`, `Dependency { id, name, description: Option<String>, version_range: Option<String> }`
  - `DependencyDetails { boot_version, dependencies: BTreeMap<String, Coordinates>, #[serde(default)] boms: BTreeMap<String, Bom>, #[serde(default)] repositories: BTreeMap<String, Repository> }`
  - `Coordinates { group_id, artifact_id, version: Option<String>, scope: Scope, bom: Option<String>, repository: Option<String> }`
  - `enum Scope { Compile, Runtime, CompileOnly, Provided, AnnotationProcessor, Test }` (camelCase)
  - `Bom { group_id, artifact_id, version, #[serde(default)] repositories: Vec<String> }`, `Repository { name, url, snapshot_enabled: bool }`
- Produces (`version.rs`): `compare(a: &str, b: &str) -> Ordering`, `in_range(range: &str, version: &str) -> bool`.

- [ ] **Step 1:** Capture fixtures:
  `curl -s -H 'Accept: application/vnd.initializr.v2.2+json' https://start.spring.io > tests/fixtures/metadata.json`
  `curl -s 'https://start.spring.io/dependencies?bootVersion=4.2.0' > tests/fixtures/dependencies-4.2.0.json`
- [ ] **Step 2: Write failing tests** (in-module `#[cfg(test)]`):
  - `parses_live_metadata`: fixture parses; `project_type.values.len() == 5`; type `maven-build` has `action == "/pom.xml"`, `tags.format == "build"`; `boot_version.default == Some("4.2.0")`; a dependency with id `web` exists.
  - `parses_dependency_details`: `dependencies["lombok"].scope == Scope::AnnotationProcessor`; `boms["spring-ai"].artifact_id == "spring-ai-bom"`; `repositories["spring-snapshots"].snapshot_enabled`.
  - version: `compare("4.2.0","4.2.1-SNAPSHOT") == Less`; `compare("4.3.0-M1","4.3.0-RC1") == Less`; `compare("4.3.0-RC1","4.3.0-SNAPSHOT") == Less`; `compare("4.3.0-SNAPSHOT","4.3.0") == Less`; `compare("3.3.4.RELEASE","3.3.4") == Equal`.
  - ranges: `in_range("4.0.0","4.2.0")`; `!in_range("4.0.0","3.5.0")`; `in_range("[4.0.0,4.3.0-M1)","4.2.0")`; `!in_range("[4.0.0,4.3.0-M1)","4.3.0")`; `!in_range("[4.0.0,4.1.0-M1)","4.1.0-M1")`; `in_range("(4.0.0,4.2.0]","4.2.0")`.
- [ ] **Step 3:** `cargo test` → FAIL.
- [ ] **Step 4:** Implement. Version format `major.minor.patch[-|.]qualifier`; qualifier order M(n) < RC(n) < SNAPSHOT/BUILD-SNAPSHOT < none/RELEASE. A bare range `"X"` means `>= X`; brackets are inclusive, parens exclusive.
- [ ] **Step 5:** `cargo test` → PASS.
- [ ] **Step 6:** `jj commit -m "feat: add Initializr metadata model and version ranges"`

### Task 3: Project request

**Files:**
- Modify: `src/model.rs`

**Interfaces:**
- Produces: `ProjectRequest { project_type, language, boot_version, java_version, packaging, group_id, artifact_id, version, name, description, package_name: String, dependencies: Vec<String> }` with
  - `ProjectRequest::from_defaults(m: &Metadata) -> Self` (each field from the metadata default; dependencies empty)
  - `ProjectRequest::query_pairs(&self) -> Vec<(&'static str, String)>` — keys `type, language, bootVersion, javaVersion, packaging, groupId, artifactId, version, name, description, packageName, baseDir, dependencies` (deps comma-joined; `baseDir = artifact_id`)
  - `derive_package_name(group: &str, artifact: &str) -> String`
  - `Metadata::project_type(&self, id: &str) -> Option<&ProjectType>`

- [ ] **Step 1: Failing tests:**
  - `defaults_from_metadata`: fixture → `project_type == "gradle-project"`, `group_id == "com.example"`, `artifact_id == "demo"`, `package_name == "com.example.demo"`.
  - `query_has_base_dir_and_joined_deps`: with deps `["web","lombok"]` → contains `("dependencies","web,lombok")` and `("baseDir","demo")`.
  - `derive_package`: `("com.example","demo") → "com.example.demo"`; `("Com.Example","my-app 2") → "com.example.myapp2"`; `("com.example","2fast") → "com.example._2fast"`; `("com.example","") → "com.example"`.
- [ ] **Step 2:** Run → FAIL. **Step 3:** Implement (lowercase; drop chars that aren't ASCII alphanumeric, `_` or `.`; prefix `_` to any segment starting with a digit; drop empty segments). **Step 4:** Run → PASS.
- [ ] **Step 5:** `jj commit -m "feat: add project request and package name derivation"`

### Task 4: HTTP client and cache

**Files:**
- Create: `src/api.rs`, `src/cache.rs`

**Interfaces:**
- Produces (`api.rs`):
  - `Client::new(base_url: &str) -> Client`
  - `Client::metadata(&self) -> Result<(Metadata, String), ApiError>` (parsed + raw JSON, raw is what gets cached)
  - `Client::dependencies(&self, boot: &str) -> Result<(DependencyDetails, String), ApiError>`
  - `Client::generate(&self, action: &str, req: &ProjectRequest) -> Result<Vec<u8>, ApiError>`
  - `enum ApiError { Connect(String), Status { status: u16, body: String }, Decode(String) }` (thiserror); `ApiError::is_connectivity(&self) -> bool` (true only for `Connect`, which covers connect failures and timeouts)
- Produces (`cache.rs`):
  - `Cache::new(dir: PathBuf) -> Cache`, `Cache::default_dir() -> Option<PathBuf>`
  - `store_metadata(&self, raw: &str) -> io::Result<()>`, `load_metadata(&self) -> Option<(Metadata, SystemTime)>`
  - `store_dependencies(&self, boot: &str, raw: &str) -> io::Result<()>`, `load_dependencies(&self, boot: &str) -> Option<DependencyDetails>`, `has_dependencies(&self, boot: &str) -> bool`

- [ ] **Step 1: Failing tests** (mockito server; tempfile dirs):
  - `metadata_sends_accept_header`: mock `GET /` matching the `Accept` header returns the fixture → `Ok`.
  - `status_error_is_not_connectivity`: mock 400 on `/dependencies?bootVersion=3.5.0` → `Err(Status{status:400,..})`, `!is_connectivity()`.
  - `unreachable_is_connectivity`: `Client::new("http://127.0.0.1:9")` → `metadata()` error `is_connectivity()`.
  - `generate_hits_action_with_query`: mock `GET /starter.zip` matching query `baseDir=demo` → returns bytes `b"PK"`.
  - `cache_roundtrip`: store then load metadata and dependencies; `has_dependencies("4.2.0")`.
  - `corrupt_cache_is_none`: write `"{not json"` to `metadata.json` → `load_metadata()` is `None`.
- [ ] **Step 2:** Run → FAIL. **Step 3:** Implement (reqwest blocking, rustls, 5s timeout for metadata/dependencies, 30s for generate; create cache dir on store). **Step 4:** Run → PASS.
- [ ] **Step 5:** `jj commit -m "feat: add Initializr HTTP client and on-disk cache"`

### Task 5: Offline build files

**Files:**
- Create: `src/offline.rs`, `templates/pom.xml.j2`, `templates/build.gradle.j2`, `templates/build.gradle.kts.j2`, `templates/settings.gradle.j2`, `templates/settings.gradle.kts.j2`, `tests/fixtures/online/*` (reference outputs)

**Interfaces:**
- Consumes: `ProjectRequest`, `ProjectType`, `DependencyDetails`, `Scope`.
- Produces: `offline::build_files(req: &ProjectRequest, ptype: &ProjectType, details: &DependencyDetails) -> Result<Generated, OfflineError>` where `Generated { files: Vec<(String, Vec<u8>)>, warnings: Vec<String> }` (paths relative, no `baseDir` prefix); `enum OfflineError { UnknownBuild(String), Template(String) }`.

Scope mapping (Gradle configuration / Maven):

| Scope | Gradle | Maven |
|---|---|---|
| compile | `implementation` | no scope |
| runtime | `runtimeOnly` | `runtime` |
| compileOnly | `compileOnly` | `<optional>true</optional>` |
| provided | `providedRuntime` if war else `compileOnly` | `provided` |
| annotationProcessor | `annotationProcessor` | `<optional>true</optional>` + compiler plugin `annotationProcessorPaths` |
| test | `testImplementation` | `test` |

Rules (spec "Builder's calls"): add `spring-boot-starter` if no selected coordinate's artifactId starts with `spring-boot-starter`; always `spring-boot-starter-test` (test) and, for Gradle, `testRuntimeOnly 'org.junit.platform:junit-platform-launcher'`; devtools & docker-compose → Gradle `developmentOnly`, Maven `runtime` + optional; lombok → Gradle `compileOnly` + `annotationProcessor` (+ test variants, as in online output); Kotlin + web → `tools.jackson.module:jackson-module-kotlin`; Kotlin → `kotlin-reflect` (+ Maven `kotlin-stdlib`, `kotlin-test-junit5`); Kotlin + data-jpa → JPA/all-open config; Groovy → `org.apache.groovy:groovy`; war → `spring-boot-starter-tomcat` provided/`providedRuntime`; BOMs of selected deps → Maven `dependencyManagement` with `<x.version>` properties / Gradle `dependencyManagement { imports { mavenBom … } }`; repositories referenced by those deps/BOMs added after `mavenCentral()`. `native`/`testcontainers` → warning `"<id>: extra build setup not generated offline"`; selected id missing from details → skipped with warning `"<id>: no cached coordinates, skipped"`. Exact template text: follow the captured online fixtures.

- [ ] **Step 1:** Capture online references into `tests/fixtures/online/` (Boot 4.2.0, Java 21, group `com.example`, artifact `demo`, deps `web,data-jpa,spring-ai-openai`):
  `maven-java-jar.pom.xml` (`/pom.xml?type=maven-build&language=java&packaging=jar…`), `maven-groovy-jar.pom.xml`, `gradle-java-jar.build.gradle` (`/build.gradle?type=gradle-build&language=java&packaging=jar…&dependencies=web,data-jpa,spring-ai-openai,lombok,devtools`), and `gradle-kotlin-kotlin-war.build.gradle.kts` (unzip `build.gradle.kts` from `/starter.zip?type=gradle-project-kotlin&language=kotlin&packaging=war…`). Read them to derive templates and the Kotlin/Groovy plugin versions.
- [ ] **Step 2: Failing tests:**
  - `matches_online_declared_dependencies` (one case per fixture): every `artifactId` declared in the online file, except those matching `spring-boot-starter-.+-test`, appears in the offline output; BOM `spring-ai-bom` and its version `2.1.0-M1` appear.
  - `gradle_kotlin_war_has_kts_settings`: files are exactly `build.gradle.kts`, `settings.gradle.kts`; `providedRuntime` present.
  - `build_only_maven_is_single_file`: `maven-build` → files `["pom.xml"]`.
  - `build_only_gradle_is_single_file`: `gradle-build` → files `["build.gradle"]`.
  - `maven_lombok_java25_has_processor_paths`: Java 25 + lombok → pom contains `<annotationProcessorPaths>` and `<artifactId>lombok</artifactId>`.
  - `native_warns`: deps `["native"]` → warnings contains `"native: extra build setup not generated offline"`.
  - `missing_coordinates_warns`: deps `["nope"]` → warning `"nope: no cached coordinates, skipped"`.
- [ ] **Step 3:** Run → FAIL. **Step 4:** Implement: compute a template context in Rust (dependency lines already mapped per table) and render embedded (`include_str!`) minijinja templates. **Step 5:** Run → PASS.
- [ ] **Step 6:** `jj commit -m "feat: generate offline Maven and Gradle build files"`

### Task 6: Offline sources and zip

**Files:**
- Modify: `src/offline.rs`
- Create: `templates/Application.{java,kt,groovy}.j2`, `templates/ApplicationTests.{java,kt,groovy}.j2`, `templates/ServletInitializer.{java,kt,groovy}.j2`, `templates/application.properties.j2`, `templates/gitignore.j2`

**Interfaces:**
- Produces: `offline::generate(req, ptype, details) -> Result<Generated, OfflineError>` (build files from Task 5 + sources for `format == "project"`), `offline::to_zip(base_dir: &str, files: &[(String, Vec<u8>)]) -> Vec<u8>` (every entry prefixed `"{base_dir}/"`), `offline::application_class_name(name: &str) -> String`.

- [ ] **Step 1: Failing tests:**
  - `java_project_layout`: Maven/Java/jar → files include `src/main/java/com/example/demo/DemoApplication.java`, `src/test/java/com/example/demo/DemoApplicationTests.java`, `src/main/resources/application.properties` (content `spring.application.name=demo\n`), `.gitignore`.
  - `kotlin_war_has_servlet_initializer`: → `src/main/kotlin/com/example/demo/ServletInitializer.kt`.
  - `class_name`: `"demo" → "DemoApplication"`, `"my app" → "MyAppApplication"`, `"" → "Application"`.
  - `zip_has_base_dir`: `to_zip("demo", files)` read back with `zip::ZipArchive` → every name starts with `demo/`.
- [ ] **Step 2:** Run → FAIL. **Step 3:** Implement; source text copies what the service generates — download `/starter.zip` for Java, Kotlin (war) and Groovy once to read from (not committed). **Step 4:** Run → PASS.
- [ ] **Step 5:** `jj commit -m "feat: generate offline project sources and zip"`

### Task 7: Generation orchestration, output and worker

**Files:**
- Create: `src/generate.rs`, `src/worker.rs`

**Interfaces:**
- Consumes: `Client`, `ApiError::is_connectivity`, `Cache`, `offline::generate`, `offline::to_zip`.
- Produces:
  - `generate::target_file(req: &ProjectRequest, ptype: &ProjectType) -> String` (`"{artifact}.zip"` / `"pom.xml"` / `"build.gradle"`)
  - `generate::run(client: Option<&Client>, cache: &Cache, req: &ProjectRequest, ptype: &ProjectType, out_dir: &Path) -> Result<Report, GenerateError>`; `Report { path: PathBuf, warnings: Vec<String>, fell_back_offline: bool }`; `enum GenerateError { Exists(PathBuf), NoCachedDependencies(String), Api(ApiError), Offline(OfflineError), Io(io::Error) }` with Display `"{path} already exists"` and `"No cached dependency data for Boot {v} — choose a cached version"`. `client == None` means offline. Writes with `OpenOptions::create_new`.
  - `worker::spawn(client: Option<Client>, cache: Cache, out_dir: PathBuf) -> (Sender<Job>, Receiver<Event>)`
  - `enum Job { LoadMetadata, FetchDependencies(Vec<String>), Generate { req: ProjectRequest, ptype: ProjectType, online: bool } }`
  - `enum Event { Online(Metadata), Offline { metadata: Option<Metadata>, cached_at: Option<SystemTime> }, DependenciesCached(String), Generated(Result<Report, String>) }`. `LoadMetadata`: fetch + store, else load cache → `Offline`. `FetchDependencies`: fetch + store each, emitting `DependenciesCached` per success, ignoring failures.

- [ ] **Step 1: Failing tests** (`generate.rs`, mockito + tempdir):
  - `online_writes_zip`: mock `/starter.zip` returns bytes → `demo.zip` written with those bytes; `!fell_back_offline`.
  - `refuses_existing_file`: pre-create `demo.zip` with `"keep"` → `Err(Exists)`, file still `"keep"` (online and offline).
  - `falls_back_offline_on_connect_error`: client at `http://127.0.0.1:9`, cache seeded with `dependencies-4.2.0.json` fixture → `Ok`, `fell_back_offline`, zip contains `demo/build.gradle`.
  - `offline_without_cached_boot_errors`: offline, empty cache → `Err(NoCachedDependencies("4.2.0"))`.
  - `build_only_writes_pom`: offline `maven-build` → `pom.xml` written.
- [ ] **Step 2:** Run → FAIL. **Step 3:** Implement `generate.rs`, then `worker.rs` as a thin loop over `generate::run`/`Client`/`Cache`. **Step 4:** Run → PASS.
- [ ] **Step 5:** `jj commit -m "feat: add generation with offline fallback and background worker"`

### Task 8: App state and key handling

**Files:**
- Create: `src/app.rs`

**Interfaces:**
- Consumes: `Metadata`, `ProjectRequest`, `derive_package_name`, `version::in_range`, `Job`, `Event`.
- Produces:
  - `enum Mode { Connecting, Online, Offline { requested: bool, cached_at: Option<SystemTime> } }`
  - `enum Pane { Project, Dependencies }`; `enum Field { Project, Language, Boot, Java, Packaging, Group, Artifact, Name, Description, Package }` (this order)
  - `App::new(mode: Mode) -> App`; pub fields `mode, metadata: Option<Metadata>, req: ProjectRequest, pane, field, editing: bool, filter: String, filtering: bool, dep_cursor: usize, cached_boots: BTreeSet<String>, status: String, show_about: bool, quit: bool`
  - `App::on_key(&mut self, key: KeyEvent) -> Option<Job>`, `App::on_event(&mut self, ev: Event) -> Option<Job>`
  - `App::mode_label(&self) -> String`, `App::visible_dependencies(&self) -> Vec<(&str, &Dependency, bool)>` (group name, dep, compatible)

Keys: Tab switches pane; ↑/↓ moves field or dep cursor; ←/→ cycles single-select values; Enter toggles text editing (typing edits, Backspace deletes, Enter/Esc ends); `/` starts filter (typing filters, Esc clears and ends, Enter ends); Space toggles the dep under the cursor if compatible; `g` → `Job::Generate`; `r` → `Job::LoadMetadata` only in `Offline { requested: false }`; `?` toggles About; `q` and Ctrl-C set `quit`. While editing or filtering, letters are text, not commands.

- [ ] **Step 1: Failing tests:**
  - `labels`: `Online → "ONLINE"`, `Offline{requested:true} → "OFFLINE"`, `Offline{requested:false} → "OFFLINE (no connection)"`, `Connecting → "Connecting…"`.
  - `online_event_prefetches_release_boots`: `on_event(Online(fixture))` returns `FetchDependencies` with no `SNAPSHOT` versions and including `"4.2.0"`.
  - `package_follows_artifact_until_edited`: editing artifact to `myapp` → package `com.example.myapp`, name `myapp`; after editing package to `x.y`, editing artifact leaves package `x.y`.
  - `incompatible_dep_cannot_be_toggled` and `boot_change_drops_incompatible`: with a dep whose range excludes the boot, Space does nothing; switching boot to an excluding version removes a selected dep and `status` names it.
  - `filter_matches_name_id_description_case_insensitive`: filter `"WEB"` includes `web`.
  - `empty_artifact_refuses_generate`: artifact `""` → `g` returns `None`, `status` non-empty.
  - `retry_only_when_degraded`: `r` returns `Some(LoadMetadata)` for `Offline{requested:false}`, `None` for `requested:true`.
  - `q_while_editing_is_text`: editing Group, typing `q` appends to group, `quit == false`.
  - `fell_back_sets_offline`: `Generated(Ok(Report{fell_back_offline:true,..}))` → mode `Offline{requested:false,..}`, status mentions the path and any warnings.
- [ ] **Step 2:** Run → FAIL. **Step 3:** Implement. Boot change while `Online` to a version not in `cached_boots` → `Job::FetchDependencies(vec![v])`. **Step 4:** Run → PASS.
- [ ] **Step 5:** `jj commit -m "feat: add TUI app state and key handling"`

### Task 9: Rendering and main loop

**Files:**
- Create: `src/ui.rs`
- Modify: `src/main.rs`

**Interfaces:**
- Consumes: `App`, `worker::spawn`, `Cache`, `Client`, `cli::Args`, `cli::version_info`.
- Produces: `ui::draw(frame: &mut Frame, app: &App)`.

Layout per spec sketch: left "Project" pane (fields, `◂ value ▸` for selects; in offline mode Boot values in `cached_boots` show ` (cached)`), right "Dependencies (/ to search)" pane (grouped, `[x]`/`[ ]`, incompatible dimmed, "Selected: …" line), one-line status bar: `{mode_label}[ · cache {age} old] · {status or key help}`. `?` shows a centered About popup with all `VersionInfo` fields. No cache in offline → centered message `No cached options — run once while online` and only `q` works.

- [ ] **Step 1: Failing tests** (ratatui `TestBackend`):
  - `status_bar_shows_mode`: 80×24, `Offline{requested:true}` → buffer contains `OFFLINE` and not `no connection`.
  - `about_popup_shows_sha`: `show_about` → buffer contains `VERGEN_GIT_SHA` value.
  - `tiny_terminal_does_not_panic`: 40×10 with fixture metadata and filter `""`, dep cursor at last item → draws.
  - `no_cache_message`: `Offline` with `metadata: None` → contains `No cached options`.
- [ ] **Step 2:** Run → FAIL. **Step 3:** Implement `ui.rs`; `main.rs`: `ratatui::init()` (restores on panic), build `Cache::new(Cache::default_dir().expect(...))` (exit with error if `None`), `--offline` → load cache synchronously and start in `Offline{requested:true}` with worker client `None`; else start `Connecting`, send `LoadMetadata`. Loop: draw, poll key 100ms → `on_key`, drain `try_recv` → `on_event`, forward returned `Job`s; `ratatui::restore()` on exit. **Step 4:** Run → PASS.
- [ ] **Step 5:** Manual check: `cargo run` → loads online, select web + lombok, `g` → `demo.zip` in cwd; `cargo run -- --offline` → status `OFFLINE`, `g` with a new artifact name produces a zip.
- [ ] **Step 6:** `jj commit -m "feat: render TUI and wire main event loop"`

### Task 10: Build verification and README

**Files:**
- Create: `tests/build_offline.rs`, `README.md`

- [ ] **Step 1:** `tests/build_offline.rs` with `#[ignore]` test `offline_projects_build`: for each Task 5 reference combination, offline-generate into a tempdir, unzip, run `mvn -q -B package -DskipTests` or `gradle -q build -x test` and assert success. Run with `cargo test --test build_offline -- --ignored`; expected PASS with Maven 3.9+ and Gradle 9+ installed. Fix any template issue found, adding a unit test for it.
- [ ] **Step 2:** `README.md`: what it does, keys, `--offline`, cache location, minimum Maven/Gradle for Boot 4 (from Step 1 results), known offline gaps (no wrapper, no `HELP.md`, Boot 4 per-module test starters, `native`/`testcontainers` setup).
- [ ] **Step 3:** Full check: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` clean.
- [ ] **Step 4:** `jj commit -m "test: verify offline projects build and add README"`
