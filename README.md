# start-spring-io-tui

A terminal UI for [start.spring.io](https://start.spring.io): pick the build,
language, Spring Boot version, project metadata and dependencies, then press
`g` to download a starter project into the current directory.

It uses the live service by default and falls back to an offline mode when the
service can't be reached (or when you pass `--offline`), generating a buildable
project from data cached during earlier online runs.

## Usage

```sh
cargo run --release            # online, falls back to offline automatically
cargo run --release -- --offline
cargo run --release -- --version
```

The status bar always shows the mode: `ONLINE`, `OFFLINE` (you passed
`--offline`) or `OFFLINE (no connection)`, plus the cache age when offline.

### Keys

| Key | Action |
|---|---|
| `Tab` | switch between the Project and Dependencies panes |
| `↑` `↓` | move between fields / dependencies |
| `←` `→` | change a choice (project type, language, Boot, Java, packaging) |
| `Enter` | edit a text field (Enter or Esc to finish) |
| `/` | fuzzy-search dependencies by name, id or category (Esc clears) |
| `Space` | select / deselect the dependency under the cursor |
| `X` | clear all selected dependencies |
| `g` | generate |
| `r` | retry the connection (after an automatic fall back to offline) |
| `?` | about (version, commit, build date, rustc) |
| `q`, `Ctrl-C` | quit |

### Output

`g` writes `./<artifactId>.zip` (unpacks to an `<artifactId>/` directory, like
the website's download) or, for the build-file-only types, `./pom.xml` /
`./build.gradle`. Existing files are never overwritten.

## Offline mode

Whenever it's online the app caches the service's options and the dependency
details for every release Boot version (SNAPSHOT versions when you select
them) under your OS cache directory, e.g.
`~/Library/Caches/start-spring-io-tui/` on macOS or
`~/.cache/start-spring-io-tui/` on Linux. Offline, Boot versions with cached
data are marked `(cached)`; the first run must be online.

Offline projects are a buildable minimum, not a copy of the website's output.
Known gaps:

- no Maven/Gradle wrapper, `HELP.md` or `.gitattributes` — build with your
  installed `mvn` / `gradle`;
- Spring Boot 4's per-module test starters (e.g.
  `spring-boot-starter-webmvc-test`) are replaced by `spring-boot-starter-test`;
- dependencies that need extra build setup (`native`, `testcontainers`) are
  added without it, and the status bar says so;
- plugin versions not supplied by the service (Kotlin, dependency-management,
  GMavenPlus) are constants in `src/offline.rs` and need bumping as the service
  moves on.

Offline output for Boot 4.2.0 has been verified to build with Maven 3.10.1 and
Gradle 9.9.1 on Java 25 (`cargo test --test build_offline -- --ignored`).
Spring Boot 4 itself requires at least Maven 3.6.3 and Gradle 8.14.

## Development

Rust toolchain is pinned in `rust-toolchain.toml`. Tests use
[insta](https://insta.rs) snapshots for generated files and the rendered
screen; review changes with `cargo insta review`.

```sh
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

Source control is [jj](https://jj-vcs.github.io/jj/). After `jj commit`,
push with `scripts/jj-push [bookmark]` (default `main`): it moves the bookmark
to the latest commit (`@-`) and pushes it.
