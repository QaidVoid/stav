# Stav agent rules

Stav is a statically typed, eager and total configuration language. Source files use the `.stav` extension and block syntax. A program evaluates to a value that renders to JSON, TOML or YAML, or converts to Rust types through serde. These rules apply to every change in this repository.

## Language invariants

These properties define Stav. A change that breaks one is a bug, even when every test passes.

- **Total.** Evaluation of a well-typed program always terminates. Functions cannot call themselves directly or indirectly, types cannot be recursive, iteration runs only over finite collections, and `self` dependencies are acyclic. Fuel limits turn large but finite evaluations into errors. Nothing may ever hang.
- **Pure and deterministic.** Evaluation never reads environment variables, the clock, the network, randomness or files, except files the host serves through its loader. The same inputs produce the same value, byte for byte. Hash map iteration order must never reach output, diagnostics or merge order.
- **Static.** A whole program is type checked before anything is evaluated.
- **Eager.** Statements are evaluated in order. The only deferred evaluation is late-bound `self`, resolved after merging.
- **Domain agnostic.** The `stav` crate knows nothing about any application that embeds it. Domain concepts enter only through the host interface: prelude, std catalog, symbol types, host modules, implicit parts, implicit reference targets and host validation.
- **No panics.** Any byte sequence given to the lexer, parser, checker or evaluator produces either a value or diagnostics, never a panic.

## Workspace

- Crates live in `crates/<name>/`. Each crate has one clear responsibility. A new crate is justified only by a heavy dependency or a real boundary, not by tidiness.
  - `stav`: the language library (lexer, parser, lossless syntax tree, formatter, type checker, evaluator, merge engine, renderers, diagnostics).
  - `stav-cli`: the `stav` binary (`fmt`, `check`, `eval`, `repl`).
  - `stav-lsp`: the language server, added once the core language is complete.
- The `stav` library performs no filesystem or network I/O of its own. Source files are read only through the `Loader` trait that the host provides.
- Every crate manifest inherits the workspace package fields and lints, and has a one-line `description`, which crates.io requires.
- Everything targets Rust edition 2024 on the stable toolchain pinned in `rust-toolchain.toml`.

## Version control

- Check which VCS the checkout uses before the first VCS command: run `jj root`. If it succeeds, use `jj` for everything and never run `git` directly, because git commands can desync a colocated jj repository. If it fails, the checkout is plain git, so use `git` for everything.
- Never push and never open pull requests unless the user explicitly asks.
- Commit messages use the semantic format `type: imperative message` or `type(scope): imperative message`, for example `feat(parser): parse tagged blocks`. Keep the first line within 68 characters. Keep bodies short.
- Commit a change once it is a coherent unit and all validation gates pass. Do not commit work that fails a gate.
- Do not hold changes too long either. Do not let more than one task's worth of work pile up uncommitted. Split large tasks into several validated commits.
- With jj, use `jj describe -m "<message>"` to describe the working-copy change and `jj new` to start the next one. Run `jj status` and `jj diff` before describing, so the commit contains only what you intend.
- With git, stage only the files you intend with `git add <paths>`, never `git add -A` or `git add .`, then run `git commit -m "<message>"`. Run `git status` and `git diff --staged` before committing. Never amend, rebase or force anything unless the user asks.

## Releases

- Releases go through release-plz only (`.release-plz.toml` and `.github/workflows/release-plz.yml`). On every push to `main`, it opens or updates a release PR with version bumps and changelogs, and publishes to crates.io when that PR is merged.
- Never run `cargo publish`, never bump versions by hand, and never edit `CHANGELOG.md` files by hand. release-plz derives all of them from the semantic commit messages, so write those messages with care. `feat` and `fix` commits appear in the changelog, and a `!` or a `BREAKING CHANGE:` footer marks a breaking change.
- `semver_check` is on. A breaking change to an API that has been released must be marked as breaking in its commit message. A change to an API that no release has shipped yet is not breaking, so do not mark it.
- Language changes follow the same rule as API changes. Once released, a program that type checked and evaluated must keep doing so with the same value, unless the change is marked as breaking.
- Crates that are not meant for crates.io set `publish = false`, and release-plz skips them.
- When a new publishable crate is added, add it to `changelog_include` of the package that owns the shared changelog in `.release-plz.toml` (the `[[package]]` entry with `changelog_path`).

## Dependencies

- Only crates.io dependencies. No git dependencies, no path dependencies outside this workspace, and no `[patch]` sections. This holds for transitive dependencies too.
- Declare every dependency once, in `[workspace.dependencies]` of the root `Cargo.toml`, with its version and the smallest feature set that every user of it needs. Use `default-features = false` when the defaults are not needed. This covers dev-dependencies and the workspace's own crates too, which are listed with both `path` and `version`. The fuzz crate is outside the workspace and declares its own.
- Crates inherit dependencies with `name.workspace = true` and add only the extra features they need with `features = [...]`. Crate manifests never carry their own versions.
- To add a dependency, add its entry to `[workspace.dependencies]` (cargo has no command for that table), then run `cargo add -p <crate> <name>` (with `--dev` for dev-dependencies), which writes the inheriting entry. To upgrade, change the version once in the root table and run `cargo update -p <name>`. To remove one, run `cargo remove -p <crate> <name>` and delete the root entry once no crate uses it.
- Keep the `stav` library's dependency tree small, because every program that embeds Stav inherits it. Dependencies needed only by the CLI or the language server belong in those crates.
- Vet every dependency before adding it, and state the result in the commit body. Check that it:
  - is needed, because the standard library or an existing dependency cannot do the job in a few lines,
  - has a permissive license (MIT, Apache-2.0, BSD, Zlib, ISC or Unicode),
  - is maintained, with a release in the last 12 months or a stable, finished scope,
  - pulls no git sources and downloads nothing at build time (inspect `build.rs` if one exists),
  - fits the MSRV,
  - adds no duplicate major versions (`cargo tree -d`) unless there is no alternative.

## Code

- Keep it simple. Do not add abstractions, generics, traits, feature flags or configuration for cases nothing uses yet. If a change seems to need that, stop and ask first.
- Optimize only with a measurement in hand. When complexity is added for performance, say so in a doc comment or commit body together with the measurement that justified it.
- Library code must not panic on bad input. Use `expect` only for internal invariants, and write the message as the invariant that was broken. Document every possible panic under a `# Panics` heading.
- Every crate uses `#![forbid(unsafe_code)]`.
- Use ordered collections (`BTreeMap`, `IndexMap`, sorted vectors) wherever iteration order can reach a value, a rendered output or a diagnostic.

## Diagnostics

- Every error has a stable code (`E0xxx`), a primary span and a message that says what is wrong. Add a help line whenever a fix can be suggested, for example the closest valid name.
- Messages start lowercase and have no trailing period.
- An error code never changes meaning once it has been released. Retired codes are not reused.
- Report as many independent errors as possible in one run, but never a cascade caused by an earlier error.

## Comments and documentation

- Every public item has a doc comment. That includes modules, structs, enums, variants, fields, traits, functions, methods, constants and type aliases. Say what it is for and how it behaves. Cover `# Errors` and `# Panics` where they apply, and give examples for entry-point APIs.
- No narration and no obvious comments. Never restate what the code does. Write an inline comment only when the code cannot show why something is done: a non-obvious invariant, an edge case of the grammar or a deliberate trade-off.
- Never reference other projects, languages, frameworks or applications in code, comments, docs or commit messages. That includes any program that embeds Stav. Describe the behavior itself instead of what inspired it.
- Never reference design documents, specifications, planning notes, decision records, section numbers, milestones or task IDs in code, comments, docs, tests or commit messages. The code and its documentation must stand on their own.
- Public standards and formats that the code implements may be named, for example an RFC, POSIX, ELF or the JSON and TOML formats, because naming them tells the reader exactly what behavior to expect.
- Do not use em-dashes in any prose, and do not substitute them with `--`, a spaced hyphen or a semicolon. Write complete sentences.

## Tests

- Unit tests live in a sibling `<module>_tests.rs` file, never inline in the implementation file. Wire it at the bottom of the module:

  ```rust
  #[cfg(test)]
  #[path = "lexer_tests.rs"]
  mod tests;
  ```

  For `foo/mod.rs` the file is `foo/mod_tests.rs`.
- Integration tests that exercise a crate through its public API go in `crates/<name>/tests/`. Language behavior is tested with fixture programs under `crates/stav/tests/fixtures/`: each `.stav` file has its expected output (a rendered value or the diagnostics) as an `insta` snapshot.
- Test behavior, not implementation. A test must fail if a user-visible behavior or a documented invariant breaks. Do not write tests for getters, derives, constructors or anything the compiler already guarantees.
- Name each test after the behavior it checks, for example `equal_priority_scalars_conflict`.
- Use property tests (`proptest`) for invariants over many inputs: formatting is idempotent, parsing the formatted text gives the same tree, merge results do not depend on hash order, and evaluation is deterministic. Fix seeds when a failure is found and keep the regression file.
- Snapshot tests use `insta`. Review every snapshot change on purpose with `cargo insta review` or by reading the diff. Never accept snapshots blindly.
- Fuzz targets live in `fuzz/` (cargo-fuzz, nightly) and are not part of the workspace or the gates. The lexer, parser, checker and evaluator each have one, and none may panic or hang on any input. When you change one of them, run its fuzz target for at least five minutes before committing.
- Benchmarks are not tests and do not run in the gates.

## Validation gates

Run these from the workspace root before claiming any work is done and before every commit:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
! grep -q 'source = "git+' Cargo.lock
```

- Never claim work is done, fixed or passing without running the gates and reading their output. If a gate fails, fix it or report the failure exactly as it appeared. Never silence a lint with `allow` to pass a gate unless the lint is wrong for that spot, and then write the reason next to the `allow`.
- Verify behavior, not only compilation. Run the relevant fixture, test or `stav` command and check the result.
