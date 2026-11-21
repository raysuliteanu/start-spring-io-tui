# Prior art

Date: 2026-11-20

Existing tools similar to start-spring-io-tui (see
`superpowers/specs/2026-11-20-start-spring-tui-design.md`).

**Summary:** several terminal clients for start.spring.io already exist. None
found combines online-first generation with an automatic fallback to offline
generation from cached data, so that part of our design looks new.

## Closest match: tsb / `tspring` (Rust)

[huseyinbabal/tsb](https://www.github.com/huseyinbabal/tsb), published on
crates.io as [`tspring`](https://docs.rs/crate/tspring/1.0.0). Source checked
in detail.

- Stack: Rust with ratatui, crossterm, reqwest, minijinja and clap, with
  templates embedded via `include_str!`. Very close to our Approach A.
- It always builds projects locally from those templates and never downloads
  `starter.zip`. Dependency coordinates come from a `dependencies.json`
  bundled into the binary (a config-dir file can override it). Unknown
  dependency ids are silently skipped.
- It still loads the options (Boot versions, dependency groups, etc.) from
  start.spring.io over the network. No on-disk cache was found, so in
  practice it doesn't work offline.
- Creating projects isn't its main job: the main tool is a k9s-style monitor
  for running apps through Actuator. The "new project" wizard (`tsb new`, or
  `N` in the TUI) is secondary, and `tsb new` is an inline prompt flow rather
  than full-screen.
- Only one release (1.0.0, 2026-03-24); 15 stars.
- Takeaways for us: a fixed, bundled dependency list goes stale. Our
  per-Boot-version `/dependencies` cache avoids that. Silently skipping
  unknown dependencies is the "doesn't build, and nobody's told" outcome our
  anti-goals rule out.

## Other terminal clients (not checked in depth)

Features and maintenance status are from search summaries and may be out of
date.

- **Java:** [Dan Vega's spring-initializr-tui](https://www.danvega.dev/blog/2026/03/14/spring-initializr-tui)
  is a TUI over the live service. It can also find installed IDEs and open the
  new project in one. Probably the best-known; worth a look for UX ideas.
- **Go:** [eslam-allam/spring-initializer-go](https://pkg.go.dev/github.com/eslam-allam/spring-initializer-go)
  has a layout modelled on the website.
  [justanoobcoder/spring](https://www.github.com/justanoobcoder/spring) uses
  Bubble Tea; almost no users.
- **Rust:** spring-tui (punixcorn/spring-tui) has keyboard-driven selection
  and can export choices to a config file for reuse.
  [xspring](https://docs.rs/crate/xspring/0.1.3) has an interactive prompt
  mode. [spring-boot-cli](https://docs.rs/crate/spring-boot-cli/0.1.1/source/README.md)
  is a plain command-line tool, not a TUI.
- **Shell:** [spring-starter-terminal](https://awesome.ecosyste.ms/projects/github.com%2Fthemkat%2Fspring-starter-terminal)
  is a script built on `dialog`.
- **Official:** [Spring CLI's `initializr` command](https://docs.spring.io/spring-cli/reference/0.8/initializr.html)
  asks follow-up questions when options are missing. It can point at a
  self-hosted Initializr server, which is the official way to avoid depending
  on start.spring.io.

## Our differentiators

- Online-first with automatic fallback to offline.
- Per-Boot-version dependency data cached ahead of time.
- Mode (`ONLINE` / `OFFLINE`) always visible.
- Goal of offline output that matches the online result.

Idea to borrow: spring-tui's config export fits the "pre-filling selections
from CLI flags" item in the design's "Later" list.
