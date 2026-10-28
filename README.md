# Stav

Stav is a statically typed, eager and total configuration language. A program is a set of `.stav` files written in block syntax. Stav type checks the whole program before it evaluates anything, merges the contributions of every file by priority, and produces a value that renders to JSON, TOML or YAML or converts to Rust types through serde.

- **Total.** Evaluation always terminates. Functions cannot call themselves, types cannot be recursive, loops run over finite collections, and fuel limits stop evaluations that are finite but too large.
- **Deterministic.** Evaluation reads nothing except the files the host serves. The same inputs give the same value, byte for byte.
- **Static.** Type errors, unknown names and broken imports are reported before anything runs, with the closest valid name as a hint.

Stav is young. Until 1.0, a minor release may change the language and the API.

## Example

`app.stav` declares its schema and then fills it in:

```stav
type Level = debug | info | warn | error

struct Upstream {
  url:    Str where matches(r"https?://.*")
  weight: Int where 1..=100 = 10
}

struct Root {
  listen:    Str = "127.0.0.1:8080"
  log_level: Level = info
  upstreams: Map<Str, Upstream> = {} block upstream
}

fn local(port: Int where 1..=65535) -> Upstream =
  Upstream { url = "http://127.0.0.1:#{port}" }

log_level = warn

upstream primary { url = "https://a.example.org", weight = 90 }
upstream backup { url = "https://b.example.org" }

upstreams.dev = local(9000)
```

`stav eval -o toml app.stav` prints:

```toml
listen = "127.0.0.1:8080"
log_level = "warn"

[upstreams.backup]
url = "https://b.example.org"
weight = 10

[upstreams.dev]
url = "http://127.0.0.1:9000"
weight = 10

[upstreams.primary]
url = "https://a.example.org"
weight = 90
```

With `weight = 900` for `primary`, `stav eval app.stav` prints nothing on standard output, exits with status 1 and reports:

```text
error[E0803]: value violates a refinement
   ┌─ app.stav:19:60
   │
 5 │   weight: Int where 1..=100 = 10
   │           ----------------- constraint declared here
   ·
19 │ upstream primary { url = "https://a.example.org", weight = 900 }
   │                                                            ^^^
   │                                                            │
   │                                                            900 is not in 1..=100
```

## Installation

```sh
cargo install stav-cli
```

This installs the `stav` command. Building it needs Rust 1.89 or newer.

## Commands

- `stav fmt <PATH>...` formats files in place. A directory stands for every `.stav` file below it, skipping hidden entries and symbolic links. `--check` lists the files that would change instead of changing them, and `-` formats standard input to standard output.
- `stav check <ENTRY>` type checks a program and reports its errors and warnings.
- `stav eval <ENTRY>` type checks and evaluates a program and prints its value as JSON. `-o toml` and `-o yaml` select another format, `--canonical` prints canonical JSON (RFC 8785) on one line, and `--path upstreams.dev` prints a single attribute.

`check` and `eval` take the root struct `Root` from the entry file, and `--root` names another one. Imports and part files cannot leave the directory of the entry file unless `--root-dir` names a wider one. `--std <DIR>` serves the modules `std/<name>` from the files `<DIR>/<name>.stav`. `--message-format json` writes diagnostics as one JSON object per line.

The exit status is 0 on success, 1 when a diagnostic is an error or `fmt --check` finds a file that would change, 2 on a usage error, and 3 when a file cannot be read or written.

## Embedding

The `stav` crate is the whole language as a library. It performs no I/O of its own: a `Host` hands it source files through a `Loader`, and can add a prelude, standard modules, domain types and validation. `MemoryLoader` serves files from memory, and a host that reads from disk implements `Loader` itself.

Add `stav` to your dependencies, and `serde` with the `derive` feature to convert values into your own types:

```rust
use std::collections::BTreeMap;

use serde::Deserialize;
use stav::{AttrPath, Diagnostics, Host, ImportRoot, Limits, MemoryLoader, RootSpec};

#[derive(Debug, Deserialize)]
struct Upstream {
    url: String,
    weight: u8,
}

const SOURCE: &str = r#"
struct Upstream {
  url:    Str
  weight: Int where 1..=100 = 10
}

struct Root { upstreams: Map<Str, Upstream> = {} block upstream }

upstream primary { url = "https://a.example.org", weight = 90 }
upstream backup { url = "https://b.example.org" }
"#;

fn main() -> Result<(), Diagnostics> {
    let host = Host::new(MemoryLoader::new().with_file("app/main.stav", SOURCE));
    let root = RootSpec {
        root_struct: "Root".to_owned(),
        entries: vec!["app/main.stav".into()],
        import_roots: vec![ImportRoot::new("app")],
    };
    let checked = stav::check(&host, &root)?;
    let evaluated = checked.eval(&Limits::default())?;
    let upstreams: BTreeMap<String, Upstream> = evaluated.get(&AttrPath::new(["upstreams"]))?;
    assert_eq!(upstreams["backup"].url, "https://b.example.org");
    assert_eq!(upstreams["backup"].weight, 10);
    Ok(())
}
```

`check` and `eval` return `Diagnostics` on failure. Its `Display` form lists every error with its code, file, line and column.

## License

Licensed under either of the [Apache License, Version 2.0](LICENSE-APACHE) or the [MIT license](LICENSE-MIT), at your option.
