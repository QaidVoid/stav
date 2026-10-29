# Structuring configuration

A Stav program does not assemble its value by hand. Every statement **contributes** a value to a path, such as `server.port` or `upstreams.primary.weight`, and every contribution has a **priority**. Once all statements have run, the contributions are merged into the final value, path by path.

This chapter shows how blocks, priorities, parts and part files use that model to split configuration into pieces that fit together.

## Blocks

A block is a name, an optional label and a body in braces. The body's statements are typed against a struct, and they contribute to the attribute the block stands for. Which attribute that is depends on the schema:

<!-- file: blocks.stav -->

```stav
type Build =
  | native(features: List<Str> = [])
  | script(command: Str)

struct Limits {
  memory: Int = 512
  cpus:   Int = 1
}

struct Job {
  build:  Build
  limits: Limits = {}
  env:    Map<Str, Str> = {}
}

struct Root {
  jobs: Map<Str, Job> = {} block job
}

job finder {
  build native { features = ["simd"] }
  limits { memory = 2048 }
  env { RUST_LOG = "debug" }
}

job docs {
  build script { command = "make html" }
}
```

<!-- run: eval -o yaml blocks.stav -->

```yaml
jobs:
  docs:
    build:
      script:
        command: make html
    limits:
      memory: 512
      cpus: 1
    env: {}
  finder:
    build:
      native:
        features:
          - simd
    limits:
      memory: 2048
      cpus: 1
    env:
      RUST_LOG: debug
```

Four kinds of block appear here:

- `job finder { ... }` is a **labeled block**. The field `jobs` declares `block job`, so each `job <label>` block fills the entry `<label>` of the map.
- `limits { ... }` fills a **struct field**. Its body is typed against `Limits`.
- `env { ... }` fills a **map field**. Each attribute in the body is an entry of the map.
- `build native { ... }` is a **tagged block**. `build` is a sum type, the label `native` picks the variant, and the body sets its payload fields.

A block and a dotted path are interchangeable. `limits { memory = 2048 }` inside the job is the same as `limits.memory = 2048`, and at the top level it is the same as `jobs.finder.limits.memory = 2048`.

## Contributions merge

Because statements contribute instead of assigning, the same path can be written more than once. How the contributions combine depends on the type at the path:

<!-- file: merge.stav -->

```stav
struct Root {
  hosts:  List<Str> = []
  groups: Set<Str> = []
  env:    Map<Str, Str> = {}
  port:   Int = 80
}

hosts = ["a.example.org"]
hosts = ["b.example.org"]

groups = ["web", "admin"]
groups = ["web", "ops"]

env { LANG = "C.UTF-8" }
env { TZ = "UTC" }

port = 8080
port = 8080
```

<!-- run: eval merge.stav -->

```json
{
  "hosts": [
    "a.example.org",
    "b.example.org"
  ],
  "groups": [
    "admin",
    "ops",
    "web"
  ],
  "env": {
    "LANG": "C.UTF-8",
    "TZ": "UTC"
  },
  "port": 8080
}
```

- Lists concatenate, in the order of the statements.
- Sets combine, without duplicates.
- Maps and structs merge entry by entry and field by field.
- Single values, such as numbers, strings and variants, must agree. Two equal values are fine.

Two different single values at the same priority are a conflict. Stav never picks one silently. It reports every contribution involved:

<!-- file: conflict.stav -->

```stav
struct Root {
  port: Int = 80
}

port = 8080
port = 9090
```

<!-- run: eval conflict.stav -->

```text
error[E0501]: conflicting values for `port`
  ┌─ conflict.stav:5:8
  │
5 │ port = 8080
  │        ^^^^
  │        │
  │        8080 (plain)
6 │ port = 9090
  │        ---- 9090 (plain)
  │
  = help: keep one of them, or mark the one that should win with `force(...)`
```

## Priorities

Every contribution has one of four priorities, from weakest to strongest:

| Priority | Written as |
| --- | --- |
| declared default | `port: Int = 80` in a struct |
| default | `port = default(8080)` |
| plain | `port = 8080` |
| force | `port = force(443)` |

At each path, only the contributions with the highest priority take part in the merge. The others are discarded. A plain assignment therefore beats a `default(...)`, and `force(...)` beats everything:

<!-- file: priorities.stav -->

```stav
struct Root {
  port:  Int = 80
  hosts: List<Str> = []
}

port = default(8080)
port = 9090

hosts = ["a.example.org"]
hosts = ["b.example.org"]
hosts = force(["only.example.org"])
```

<!-- run: eval priorities.stav -->

```json
{
  "port": 9090,
  "hosts": [
    "only.example.org"
  ]
}
```

`force` on a list replaces it, because the plain contributions no longer take part.

`default(...)` and `force(...)` are markers, not functions. They are only allowed as the whole right-hand side of an attribute. Around a struct or map literal, a marker applies to every field the literal sets.

## Parts

A **part** is a reusable, typed fragment of a body. `part { ... }` is an expression of type `Part<S>`, where `S` is the struct whose body it belongs to. Parts are usually returned from functions, so that they can take parameters:

<!-- file: parts.stav -->

```stav
struct Server {
  name:     Str
  replicas: Int where 1..=64 = 1
  env:      Map<Str, Str> = {}
}

struct Root {
  servers: Map<Str, Server> = {} block server
}

fn web(name: Str, replicas: Int where 1..=64 = 2) -> Part<Root> = part {
  server "#{name}" {
    name     = name
    replicas = default(replicas)
    env { RUST_LOG = "info" }
  }
}

include web("api")
include web("admin", replicas = 1)

server api {
  replicas = 8
}
```

<!-- run: eval -o toml parts.stav -->

```toml
[servers.admin]
name = "admin"
replicas = 1

[servers.admin.env]
RUST_LOG = "info"

[servers.api]
name = "api"
replicas = 8

[servers.api.env]
RUST_LOG = "info"
```

`include` adds a part's contributions at the place where it is included. The body of the part is type checked once, against `Root`, when the function is checked. Including a part into a body of a different struct is an error.

The part sets `replicas` with `default(...)`. That lets the plain `replicas = 8` in `server api` win without a conflict and without `force`. This is the rule for anything shared:

::: tip Writing parts that others include
Set single values with `default(...)`, so that the user's plain assignments win. Contribute to lists, sets and maps at plain priority. A `default(...)` contribution to a list would be dropped as soon as the user adds a plain entry, and the part's entries would disappear.
:::

A part does not see the final value it contributes to. Everything it depends on arrives as a parameter. If two parts need the same input, pass it to both, usually from a top-level `let`.

### A part merges once

Parts often build on each other, and two parts may include the same third one. A part is merged into a block only once, however often and through however many other parts it is included there:

<!-- file: once.stav -->

```stav
struct Root { packages: List<Str> = [] }

fn dbus() -> Part<Root> = part {
  packages = ["dbus"]
}

fn audio() -> Part<Root> = part {
  include dbus()
  packages = ["pipewire"]
}

fn bluetooth() -> Part<Root> = part {
  include dbus()
  packages = ["bluez"]
}

include audio()
include bluetooth()
```

<!-- run: eval -o toml once.stav -->

```toml
packages = ["dbus", "pipewire", "bluez"]
```

Both `audio()` and `bluetooth()` include `dbus()`, and `"dbus"` is still in the list once. Calling the same function with equal arguments gives the same part, so `dbus()` counts once. A function called with different arguments gives a different part, and each of them is merged. The same holds for part files. [Merging rules](/reference/merging#a-part-merges-once-per-block) state the exact rule.

## Part files

A part can also be a file. A **part file** contains statements, `let` bindings and `import` declarations, but no `struct`, `type` or `fn` declarations. `include` with a string literal that names the file adds it where it stands:

<!-- file: service.stav -->

```stav
struct Database {
  url:  Str
  pool: Int where 1..=100 = 10
}

struct Root {
  name:     Str
  database: Database
  features: Set<Str> = []
}

name     = "inventory"
features = ["search"]

database {
  url = "postgres://localhost/inventory"
}

include "./production.stav"
```

<!-- file: production.stav -->

```stav
# Production settings, kept in a file of their own.
features = ["audit"]

database {
  url  = force("postgres://db.internal/inventory")
  pool = 40
}
```

<!-- run: eval -o toml service.stav -->

```toml
name = "inventory"
features = ["audit", "search"]

[database]
url = "postgres://db.internal/inventory"
pool = 40
```

The part file is typed as a body of the struct of the block that includes it. Included at the top level, it is a body of `Root`. Included inside `database { }`, it would be a body of `Database`. Paths in `include` are relative to the including file and cannot leave the directory of the entry file.

A part file can be included from different places. Included into blocks of different attributes, it contributes to each of them. Included into the same block again, it adds nothing, like any other part. Part files cannot include each other in a cycle.

## Several entry files

The `stav` command evaluates one entry file, and further files join through `include` and `import`. A program that embeds Stav can also pass several **entry files**. Each is a body of the root struct, and they are evaluated in order and merged as if they were one file. This is how a host layers a base configuration and an overlay without either file naming the other. [Embedding in Rust](./embedding) shows how.

## Next

Right-hand sides that read `self` are evaluated after merging. [Late-bound self](./self) explains what that makes possible.
