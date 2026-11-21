# start-spring-io-tui — Design

Date: 2026-11-20
Status: approved

## Intent

A terminal UI, written in Rust, that does what https://start.spring.io does:
pick a project type, language, Spring Boot version, project metadata and
dependencies, then produce a starter project on disk.

Why: the existing `spring init` CLI talks to the same service but is not
user-friendly (you must know option names and dependency ids up front). A
TUI that shows the options and lets you search/select dependencies is the
improvement. It must also keep working without network (e.g. on a plane).

## Goals (MVP)

- **Online mode (default):** load options from the live Initializr service and
  generate projects via the service itself.
- **Offline mode:** used automatically when the service is unreachable, or
  forced with `--offline`. Generates a *buildable minimum* project locally.
- **Version info:** `--version` and an in-TUI About view show build version,
  git commit, dirty flag, build date and rustc version.

## Non-goals (MVP)

- Byte-for-byte replica of the website's output when offline. In particular,
  no Maven/Gradle wrapper offline, no `HELP.md`. (A full replica is the
  longer-term direction; see "Later".)
- Sharing/exploring features of the website (Explore, Share link).
- Shelling out to `spring init`.

## Anti-goals

- Offline mode silently producing a project that doesn't build.
- Network calls blocking the UI (frozen terminal).
- Pretending to be online when not: current mode must always be visible.

## Constraints

- Rust; toolchain pinned with `rust-toolchain.toml` (not mise).
- `mise` manages environment only (`mise.toml` with `[env]`, no Rust tool).
- Source control: `jj`, colocated git repo (required for `vergen-gix`).
- Initializr API facts (verified against the live service 2026-11-20):
  - `GET https://start.spring.io` with
    `Accept: application/vnd.initializr.v2.2+json` returns the options
    (types, packaging, javaVersion, language, bootVersion, text fields with
    defaults, dependency groups with id/name/description/versionRange).
  - The options do **not** contain Maven coordinates.
  - `GET /dependencies?bootVersion=X` returns, for that Boot version only:
    `dependencies` (id → groupId, artifactId, optional version, scope,
    optional bom, optional repository), `boms`, `repositories`. It returns
    400 for Boot versions outside the supported range.
  - Each project type carries an `action` (`/starter.zip`, `/pom.xml`,
    `/build.gradle`) and `tags` (build, dialect, format).
  - Generation: `GET {action}?type=…&language=…&bootVersion=…&groupId=…
    &artifactId=…&name=…&description=…&packageName=…&packaging=…
    &javaVersion=…&version=…&dependencies=a,b,c`.

## Approaches considered

- **A. ratatui + blocking reqwest on a worker thread + minijinja templates
  (chosen).** Single binary; UI never blocks; templates embedded with
  `include_str!` and rendered at runtime, so adding files toward a full
  replica is just adding templates.
- B. Same with tokio/async reqwest. Gains cancellable in-flight requests;
  overkill for ~3 request kinds. Rejected for now.
- C. Shell out to `spring init` online. Depends on the tool being replaced
  and does nothing for offline. Rejected.

## Design

### Crate layout (single binary crate `start-spring-io-tui`)

- `main.rs` — clap args, terminal setup/teardown, run loop.
- `cli.rs` — `--offline`, `--version` (long version from vergen).
- `api.rs` — Initializr HTTP client: fetch metadata, fetch dependency
  details for a Boot version, download generation result. Base URL is a
  parameter (default `https://start.spring.io`) so tests can point at a
  local mock server.
- `model.rs` — serde types for metadata and dependency details; the
  user's current selections (`ProjectRequest`).
- `cache.rs` — on-disk cache under `dirs::cache_dir()/start-spring-io-tui/`:
  `metadata.json`, `dependencies-<bootVersion>.json`, each stored with a
  fetched-at timestamp.
- `offline.rs` — local generator using minijinja templates.
- `output.rs` — write the zip or single build file to the current
  directory (no extraction); offline zips built with the `zip` crate.
- `app.rs` — app state + key handling (pure, unit-testable).
- `ui.rs` — ratatui rendering.
- `worker.rs` — background thread: receives jobs, runs api/cache calls,
  sends results back over an `mpsc` channel.
- `build.rs` — `vergen-gix` emitting build/git/rustc env vars.

### Startup and mode selection

1. If `--offline`: mode = Offline (requested). Load metadata from cache.
2. Else: draw UI immediately with "Connecting…", fetch metadata on the
   worker. On success: mode = Online, write cache. On failure (connect
   error, timeout ~5s, non-2xx): mode = Offline (degraded), load cache.
3. No cache available in Offline: show a clear message ("No cached
   options — run once while online") and allow only quit.
4. When Online, after metadata loads, prefetch `/dependencies` for every
   non-SNAPSHOT Boot version in the list (a handful of small requests, in
   the background) and cache them, so offline generation works later for
   any release version. SNAPSHOT versions are fetched on demand when
   selected.

The status bar always shows `ONLINE`, `OFFLINE` (when `--offline` was
given) or
`OFFLINE (no connection)`, plus cache age when offline.

### Screen

Two panes plus a status bar, modeled on the website:

```
┌ Project ───────────┐┌ Dependencies (/ to search) ─────────┐
│ Project  Gradle-Groovy ◂▸ ││ [x] Spring Web          Web   │
│ Language Java ◂▸   ││ [ ] Spring Data JPA     SQL         │
│ Boot     4.2.0 ◂▸  ││ ...                                 │
│ Java     21 ◂▸     ││                                     │
│ Packaging Jar ◂▸   ││ Selected: web, actuator             │
│ Group    com.example│                                     │
│ Artifact demo      ││                                     │
│ Name / Description ││                                     │
│ Package  com.example.demo                                 │
└────────────────────┘└─────────────────────────────────────┘
 ONLINE · Tab pane · ↑↓ field · ←→ choose · Space toggle · g generate · ? about · q quit
```

- Single-select fields cycle with ←/→ over the metadata's values; defaults
  come from the metadata.
- Text fields are edited inline (Enter to edit, Enter/Esc to finish).
- Package name auto-derives from `groupId + "." + artifactId` (with
  invalid identifier characters removed) until the user edits it
  directly — same behaviour as the website. Name defaults to artifact id
  the same way.
- Dependencies pane: grouped list; `/` filters by name/id/description
  (case-insensitive substring); Space toggles. Dependencies whose
  `versionRange` excludes the selected Boot version are shown dimmed and
  cannot be selected; if a Boot version change makes a selected one
  incompatible, it is deselected and the status bar says so.
- `?` opens an About popup with the vergen info.
- `q` / Ctrl-C quits (restoring the terminal, also on panic).

### Generation (`g`)

Output goes to the current directory (where the TUI was started), as a
download — nothing is extracted, mirroring the website's browser download:

- project types: `./<artifactId>.zip` (the zip contains an
  `<artifactId>/` top-level directory, as the service's does);
- build-only types: `./pom.xml` / `./build.gradle` / `./build.gradle.kts`.

The target file must not already exist — otherwise error, nothing
written. Result and path shown in the status bar.

- **Online:** request `{action}?…` from the service and write the response
  bytes to the target file as-is.
  If the request fails with a connectivity error, switch to degraded
  Offline and generate offline, telling the user.
- **Offline:** builds the same output locally — a zip with the same
  layout (see below) or the single build file. Requires cached `dependencies-<bootVersion>.json`; if
  missing, error "No cached dependency data for Boot X — choose a cached
  version" (cached versions are marked in the Boot selector).

### Offline project contents (buildable minimum)

For build `maven`: `pom.xml`. For `gradle` groovy: `build.gradle` +
`settings.gradle`. For `gradle` kotlin: `build.gradle.kts` +
`settings.gradle.kts`. Plus, for project (not build-only) types:

- `src/main/<lang>/<package path>/<Name>Application.<ext>`
- `src/test/<lang>/<package path>/<Name>ApplicationTests.<ext>`
- `src/main/resources/application.properties` (`spring.application.name=<name>`)
- `.gitignore`
- for war packaging: `ServletInitializer.<ext>`

Build file rules:

- Spring Boot plugin/parent at the selected Boot version; Java toolchain
  / `java.version` = selected Java version; packaging jar/war.
- Each selected dependency from the cached details with its scope mapped
  (`compile`, `runtime`, `provided`/`compileOnly`, `test`,
  `annotationProcessor`). If no selected dependency is a starter, add
  `spring-boot-starter`. Always add `spring-boot-starter-test` (test).
- BOMs referenced by selected dependencies go into
  dependencyManagement; repositories referenced by those BOMs or
  dependencies are added.
- Kotlin: kotlin jvm + spring plugins, `kotlin-reflect`; Groovy: groovy
  plugin/gmavenplus and `groovy` dependency.
- Plugin versions for Kotlin/Groovy/dependency-management that the
  service doesn't supply are constants in `offline.rs`, documented as
  the thing to bump.

Success criterion: offline output for the reference combinations (below)
builds with locally installed `mvn` / `gradle`, and its resolved
dependency list matches the online output for the same inputs.

### Testing

- Unit: metadata parsing (live-shaped fixtures captured into
  `tests/fixtures/`), request URL building, package-name derivation, versionRange
  matching, app key handling.
- Offline generator: render reference combinations (Maven/Java/jar,
  Gradle-Groovy/Java/jar, Gradle-Kotlin/Kotlin/war, Maven/Groovy/jar,
  with web + data-jpa + a BOM dependency) and assert on content.
- API/online: tests against a local mock HTTP server.
- Manual/ignored test: build the offline reference projects with `mvn`
  and `gradle`, and diff dependencies against online output.

### Version info

`build.rs` uses `vergen-gix` (+ vergen for build/rustc). clap
`long_version` shows `version (sha[-dirty] built <date>, rustc X)`;
About popup shows the same fields.

## Decided by the user

- Online by default; automatic fall back to offline; `--offline` flag.
- MVP offline = buildable minimum (option 2); full replica later.
- Approach A (ratatui, blocking reqwest worker thread, minijinja).
- `rust-toolchain.toml` for Rust; mise for env only; jj for VCS.
- Version info via vergen + gix.
- Status bar: `OFFLINE` for `--offline`, `OFFLINE (no connection)` when
  degraded.
- `g` downloads (does not extract) a zip / build file into the current
  directory. Extract (`G`) and choosing a location are for later.
- Offline, dependencies that need build config the cache doesn't describe
  (e.g. GraalVM Native, some Testcontainers/Docker setups) are **allowed
  with a warning**: offline output includes what can be generated, and the
  status bar names what was omitted or approximated.
  *Alternative kept on file:* mark such dependencies "unavailable offline"
  and disable them — stricter, guarantees buildability, fewer choices.

## Builder's calls (made by Claude, open to correction)

- Crate/binary name `start-spring-io-tui` (matches directory).
- No bundled metadata snapshot: first run must be online; without cache,
  offline mode just says so.
- Prefetch dependency details for all release Boot versions when online.
- Refuse to overwrite an existing output file.
- Offline output is also a zip (same layout as online) so `g` behaves
  identically in both modes.
- Cache under the OS cache dir, no expiry (refreshed whenever online).
- No CLI flags beyond `--offline`/`--version`/`--help`.
- "Matches online output" means: the offline build file declares the same
  selected dependencies (coordinates + scope) and BOMs as the online one
  for the reference combinations. Extra things Initializr adds that
  `/dependencies` doesn't report (e.g. Boot 4 per-module test starters)
  are known gaps, listed in the README, not required for MVP.
- A small rule table in `offline.rs` covers the well-known special cases:
  devtools / docker-compose → `developmentOnly` (Gradle) / optional
  (Maven); lombok → `compileOnly` + `annotationProcessor`;
  Kotlin + web → `jackson-module-kotlin`.
- No local Maven/Gradle version detection: README states the minimum
  Gradle/Maven versions for the Boot versions in use.
- After degrading to offline (not forced), `r` retries the connection;
  on success the mode switches back to Online. No automatic retry.
- Build-only types offline write exactly one file, same as online
  (no `settings.gradle`).

## Later (toward full replica)

Wrapper files (copied from a cached online zip), `HELP.md`, `.gitattributes`,
Explore (preview files before writing), download-and-extract (e.g. `G`),
choose download location, Share link,
pre-filling selections from CLI flags.
