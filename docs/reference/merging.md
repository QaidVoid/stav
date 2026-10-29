# Merging rules

A Stav program does not build its value statement by statement. Every statement contributes to attributes of the root struct, and once evaluation has finished, the contributions to each attribute are merged into one value. This page describes exactly how. The guide page on [structuring configuration](/guide/structuring) introduces the same ideas with a worked example.

## Contributions

Evaluating a body produces contributions. A contribution has four parts:

- a **path**, the attribute it contributes to, such as `servers.web.port`,
- a **priority**, one of the four levels below,
- a **value**, or a deferred value when the right side reads [`self`](/guide/self),
- an **order key**, which places it among all other contributions.

Every statement form produces contributions:

| Statement | Contributes |
|---|---|
| `a.b.c = e` | `e` to the attribute at `a.b.c` |
| `kind label { ... }` | the statements of the body, below the attribute the block maps to |
| `include e` | the statements of the part `e`, at the place of the `include`, unless the part is [already included](#a-part-merges-once-per-block) into the same block |
| `if c { ... } else { ... }` | the statements of the branch that `c` selects |
| `for x in e { ... }` | the statements of the body, once per element of `e` |

Conditions, `for` collections, block labels and `include` expressions are evaluated at once and cannot read `self`. The set of contributions therefore never depends on a merged value. [Expressions and statements](/reference/expressions) describes each statement in full.

## Priorities

Each contribution has one of four priorities, from lowest to highest:

| Priority | Written as |
|---|---|
| declared default | the `= expr` of a struct field or a payload field |
| `default` | `attribute = default(expr)` |
| plain | `attribute = expr` |
| `force` | `attribute = force(expr)` |

At every attribute, only the contributions with the highest priority present take part in the merge. Lower ones are discarded.

<!-- file: priorities.stav -->

```stav
struct Root {
  declared:  Int = 1
  defaulted: Int = 1
  plain:     Int = 1
  forced:    Int = 1
}

defaulted = default(2)

plain = default(2)
plain = 3

forced = 3
forced = force(4)
```

<!-- run: eval priorities.stav -->

```json
{
  "declared": 1,
  "defaulted": 2,
  "plain": 3,
  "forced": 4
}
```

`default(...)` and `force(...)` are priority markers, not functions. They are valid only as the whole right side of an attribute statement. Anywhere else they are [`E0441`](/reference/error-codes#e0441):

<!-- file: marker.stav -->

```stav
struct Root { port: Int = 80 }

port = default(8080) + 1
```

<!-- run: check marker.stav -->

```text
error[E0441]: `default` is a priority marker, valid only as the whole right side of an attribute
  ┌─ marker.stav:3:8
  │
3 │ port = default(8080) + 1
  │        ^^^^^^^^^^^^^
  │
  = help: write `attribute = default(...)`
```

A marker around a struct or map literal applies to every contribution the literal makes. In the next program, `force` wins the field `a`, and the plain contribution still sets `b`, because nothing else contributes to `b` at a higher priority.

<!-- file: marker-literal.stav -->

```stav
struct Settings {
  a: Int = 1
  b: Int = 2
}

struct Root { settings: Settings = {} }

settings = force({ a = 5 })
settings = { a = 3, b = 4 }
```

<!-- run: eval marker-literal.stav -->

```json
{
  "settings": {
    "a": 5,
    "b": 4
  }
}
```

## How each type merges

The merge walks the root struct from the top and merges every attribute by its type.

| Type | Rule |
|---|---|
| `Bool`, `Int`, `Float`, `Str`, `Path`, nullary sum types, symbol types, `Ref<T>` | The winners must be equal. Different values are [`E0501`](/reference/error-codes#e0501). |
| sum type with payloads, set as a whole value | Like a scalar. |
| sum type with payloads, set by tagged blocks | The variant is selected like a scalar, then the payload merges field by field. See [tagged blocks](#tagged-blocks). |
| `List<T>` | The winners are concatenated in order. |
| `Set<T>` | The union of the winners, in canonical order. |
| `Map<K, V>` | Key by key. Each key merges as a `V`. |
| struct | Field by field. Each field merges by its own type. |
| `Option<T>` | Present when anything sets it or something inside it, then merged as a `T`. See [options](#options). |
| `Data` | By the shape of the winning values. See [`Data`](#data). |

### Scalars

Two plain contributions with the same value are accepted. Two plain contributions with different values conflict, and the diagnostic cites every winner with its priority. When one of them comes from a part file, it also says where that file was included.

<!-- file: conflict.stav -->

```stav
struct Root { hostname: Str }

hostname = "forge"
include "./laptop.stav"
```

<!-- file: laptop.stav -->

```stav
hostname = "laptop"
```

<!-- run: eval conflict.stav -->

```text
error[E0501]: conflicting values for `hostname`
  ┌─ conflict.stav:3:12
  │
3 │ hostname = "forge"
  │            ^^^^^^^
  │            │
  │            "forge" (plain)
  │
  ┌─ laptop.stav:1:12
  │
1 │ hostname = "laptop"
  │            -------- "laptop" (plain), included from conflict.stav:4:1
  │
  = help: keep one of them, or mark the one that should win with `force(...)`
```

To resolve it, remove one of the contributions, mark the one that should win with `force(...)`, or mark the one that should give way with `default(...)`.

### Lists and sets

Lists concatenate and sets take the union. Because only the highest priority takes part, a single `force(...)` replaces a list, while two plain contributions add up.

<!-- file: collections.stav -->

```stav
struct Root {
  tags:     List<Str> = ["declared"]
  replaced: List<Str> = []
  zones:    Set<Str> = []
}

tags = ["a"]
tags = ["b"]

replaced = ["p1"]
replaced = force(["f"])
replaced = ["p2"]

zones = ["west", "east"]
zones = ["east", "north"]
```

<!-- run: eval -o toml collections.stav -->

```toml
tags = ["a", "b"]
replaced = ["f"]
zones = ["east", "north", "west"]
```

The declared default `["declared"]` of `tags` is not part of the result, because the two plain contributions outrank it.

### Structs, maps and literals

Structs merge field by field and maps merge key by key. A struct or map literal on the right side of an attribute contributes each field or key it sets, at the priority of the statement. Fields it leaves out contribute nothing. That is why a literal, an attribute path and a block can all add to the same value:

<!-- file: literals.stav -->

```stav
struct Kernel {
  modules: List<Str> = []
  params:  Map<Str, Str> = {}
  quiet:   Bool = false
}

struct Server {
  name: Str
  port: Int = 80
}

struct Root {
  kernel:  Kernel = {}
  limits:  Map<Str, Int> = {}
  servers: List<Server> = []
}

kernel         = { modules = ["virt-host"] }
kernel.modules = ["pci-passthru"]
kernel { params { console = "ttyS0" } }

limits       = { files = 1024 }
limits.procs = 64

servers = [Server { name = "a" }]
```

<!-- run: eval -o toml literals.stav -->

```toml
[kernel]
modules = ["virt-host", "pci-passthru"]
quiet = false

[kernel.params]
console = "ttyS0"

[limits]
files = 1024
procs = 64

[[servers]]
name = "a"
port = 80
```

Inside a list, a struct literal is a complete value. Its omitted fields take their declared defaults right away, as `port` does in `servers` above.

### Options

An `Option<T>` attribute is present when a contribution sets it or anything inside it, and that contribution outranks every `none`. A present option then merges as a `T`, with the declared defaults of `T` filling the rest. An option that nothing sets is `none`, and so is an option whose highest contribution is `none`.

<!-- file: options.stav -->

```stav
struct Tls {
  cert: Str = "/etc/cert.pem"
  port: Int = 443
}

struct Root {
  tls:     Option<Tls>
  plain:   Option<Tls>
  dropped: Option<Tls> = {}
}

tls { port = 8443 }
dropped = none
```

<!-- run: eval options.stav -->

```json
{
  "tls": {
    "cert": "/etc/cert.pem",
    "port": 8443
  },
  "plain": null,
  "dropped": null
}
```

A `none` is a value like any other. At equal priority it conflicts with a contribution that makes the option present:

<!-- file: option-conflict.stav -->

```stav
struct Tls { port: Int = 443 }

struct Root { tls: Option<Tls> }

tls = none
tls { port = 8443 }
```

<!-- run: eval option-conflict.stav -->

```text
error[E0501]: conflicting values for `tls`
  ┌─ option-conflict.stav:5:7
  │
5 │ tls = none
  │       ^^^^
  │       │
  │       none (plain)
6 │ tls { port = 8443 }
  │ -------------------
  │ │            │
  │ │            8443 (plain)
  │ present (plain)
  │
  = help: keep one of them, or mark the one that should win with `force(...)`
```

Writing `tls = default(none)` instead lets the block win.

### Tagged blocks

A field whose type is a sum type can be set with a tagged block, such as `build native { features = ["simd"] }`. The label names the variant, and the body sets its payload fields. A tagged block makes two kinds of contributions:

- a **selection** of the variant, at plain priority,
- one contribution per payload attribute, each at its own priority.

The selections decide the variant like a scalar. The selections with the highest priority must agree, otherwise the merge reports [`E0502`](/reference/error-codes#e0502). An attribute statement such as `build = force(script(target = "x"))` selects at its own priority and also contributes its payload. Payload contributions that belong to a variant that was not selected are dropped. The remaining payload contributions merge field by field, like a struct.

<!-- file: tagged.stav -->

```stav
type Build =
  | native(features: List<Str> = [], bins: List<Str> = [])
  | script(target: Str = "all")

struct Job { build: Build }

struct Root { jobs: Map<Str, Job> = {} block job }

job a { build native { features = ["a"] } }
job a { build native { features = ["b"], bins = ["finder"] } }

job b {
  build native { features = ["dropped"] }
  build script { }
  build = force(script(target = "x"))
}
```

<!-- run: eval -o toml tagged.stav -->

```toml
[jobs.a.build.native]
features = ["a", "b"]
bins = ["finder"]

[jobs.b.build.script]
target = "x"
```

The two blocks for `a` merge their `features`. In `b`, the forced selection of `script` discards the `native` block together with its payload. Without the `force`, the two blocks would disagree:

<!-- file: tagged-conflict.stav -->

```stav
type Build =
  | native(features: List<Str> = [])
  | script(target: Str = "all")

struct Job { build: Build }

struct Root { jobs: Map<Str, Job> = {} block job }

job c {
  build native { }
  build script { }
}
```

<!-- run: eval tagged-conflict.stav -->

```text
error[E0502]: tagged blocks select different variants of `jobs.c.build`
   ┌─ tagged-conflict.stav:10:3
   │
10 │   build native { }
   │   ^^^^^^^^^^^^^^^^
   │   │
   │   `native` (plain)
11 │   build script { }
   │   ---------------- `script` (plain)
   │
   = help: keep the blocks of one variant, or select the variant with `force(...)`
```

A payload sum set only through whole values, such as `build = script(target = "x")`, merges like a scalar.

### Data

`Data` holds any renderable value, so its merge follows the shape of the winning values at run time. Maps merge key by key, lists concatenate, sets take the union, and anything else merges as a scalar. Map keys come out in canonical order.

<!-- file: data.stav -->

```stav
struct Root {
  settings: Data = none
  replaced: Data = none
}

settings = { port = 80, hosts = ["a"], tls = { enabled = true } }
settings = { hosts = ["b"], tls = { cert = "/etc/cert.pem" } }
settings = { port = 80 }

replaced = { kind = "map" }
replaced = force(["a", "list"])
```

<!-- run: eval data.stav -->

```json
{
  "settings": {
    "hosts": [
      "a",
      "b"
    ],
    "port": 80,
    "tls": {
      "cert": "/etc/cert.pem",
      "enabled": true
    }
  },
  "replaced": [
    "a",
    "list"
  ]
}
```

Winners of different shapes at equal priority are [`E0503`](/reference/error-codes#e0503). A higher priority contribution replaces lower ones whatever their shape, as `replaced` shows, so `force(...)` or `default(...)` resolves every such conflict.

<!-- file: data-conflict.stav -->

```stav
struct Root { settings: Data = none }

settings = { port = 80 }
settings = ["a", "list"]
```

<!-- run: eval data-conflict.stav -->

```text
error[E0503]: `Data` values of different shapes for `settings`
  ┌─ data-conflict.stav:3:12
  │
3 │ settings = { port = 80 }
  │            ^^^^^^^^^^^^^
  │            │
  │            a map (plain)
4 │ settings = ["a", "list"]
  │            ------------- a list (plain)
  │
  = help: a higher priority contribution replaces the others: mark one with `force(...)`, or the others with `default(...)`
```

## Order

Every contribution carries an order key, a sequence of numbers compared from left to right:

1. Each entry file gets the next number, in the order the host lists them. A host's implicit parts come before every entry file.
2. The n-th statement of a body appends n.
3. The contributions of an `include` are numbered below the `include` statement, in the part's own statement order. A part that is [merged once](#a-part-merges-once-per-block) keeps the place of its first `include`.
4. The i-th iteration of a `for` statement appends i.

List concatenation follows the order keys, and so does the order of diagnostics. Nothing depends on hashing or memory addresses, so the same program always gives the same value.

<!-- file: order.stav -->

```stav
struct Root { steps: List<Str> = [] }

steps = ["first"]
include "./extra.stav"
for name in ["loop-1", "loop-2"] {
  steps = [name]
}
steps = ["last"]
```

<!-- file: extra.stav -->

```stav
steps = ["included-1"]
steps = ["included-2"]
```

<!-- run: eval -o toml order.stav -->

```toml
steps = ["first", "included-1", "included-2", "loop-1", "loop-2", "last"]
```

The `stav` command evaluates a single entry file, so the examples on this page spread contributions over files with part files and `include`. A program that embeds Stav can list several entry files in the `entries` field of `RootSpec`, and they merge in the order given. See [embedding in Rust](/guide/embedding).

## A part merges once per block

An `include` adds the contributions of a part only if the same part has not been included into the same block before. A later `include` of that part into that block adds nothing. The rule applies to everything that is evaluated for one root: the host's implicit parts, every entry file, and every part and part file included from them, however deeply.

Two parts are the same part when

- both are the same part file, or
- both come from the same `part { }` expression, evaluated with equal values for every local binding in scope at it: the parameters of the function around it, `let` bindings, loop variables, pattern bindings and the bindings a lambda captured.

Evaluation is pure, so this is what makes a call of the same function with equal arguments give the same part, whether an argument is written out or left to its default. Values compare as `==` compares them. Functions, which `==` cannot compare, are equal when they are the same top-level function, or come from the same lambda expression with equal captured bindings. Parts compare by the rule above. A part bound once with `let` and included twice is the same part both times.

The block of an `include` is the attribute its statement contributes to: the root for a statement at the top level of an entry file, and the attribute of the enclosing block otherwise. An `include` inside a part counts at the block where the part itself is included, so a part that two other parts include is merged once when both land in the same block. Included into two different blocks, such as two named blocks with different labels, a part contributes to each of them.

<!-- file: once.stav -->

```stav
struct Service { after: List<Str> = [] }

struct Root {
  ports:    List<Int> = []
  services: Map<Str, Service> = {} block service
}

fn sshd(port: Int = 22) -> Part<Root> = part {
  ports = [port]
}

fn after(name: Str) -> Part<Service> = part {
  after = [name]
}

include sshd()
include sshd(port = 22)
include sshd(port = 2222)

service web {
  include after("network")
  include after("network")
  include after("database")
}

service worker {
  include after("network")
}
```

<!-- run: eval -o toml once.stav -->

```toml
ports = [22, 2222]

[services.web]
after = ["network", "database"]

[services.worker]
after = ["network"]
```

`sshd()` and `sshd(port = 22)` are the same part, `sshd(port = 2222)` is another one. `after("network")` is merged once into `service web` and once into `service worker`.

Including the same part file twice in a row into the same body is warning [W0703](/reference/error-codes#w0703), because the second `include` has no effect.

## Writing shared parts

A part that other configurations include, such as a function that returns `Part<S>` or a library of part files, should follow one rule so that its users can override it without `force`:

- Set every scalar with `default(...)`. A plain assignment by the user then wins.
- Contribute to lists, sets and maps at plain priority, never with `default(...)`.

The second rule matters because only the highest priority takes part in a merge. A `default(...)` contribution to a list is discarded as soon as anyone makes a plain contribution to the same list, so the part's entries would silently disappear:

<!-- file: shared.stav -->

```stav
struct Server {
  replicas: Int = 1
  env:      Map<Str, Str> = {}
  flags:    List<Str> = []
}

struct Root { server: Server }

fn web(replicas: Int) -> Part<Server> = part {
  replicas = default(replicas)
  flags    = ["--web"]
  env { LOG = "info" }
}

server {
  include web(2)
  replicas = 8
  flags    = ["--verbose"]
}
```

<!-- run: eval -o toml shared.stav -->

```toml
[server]
replicas = 8
flags = ["--web", "--verbose"]

[server.env]
LOG = "info"
```

The user's `replicas = 8` beats the part's `default(2)`, and both lists of flags are kept.

## Late values

A right side that reads `self`, directly or through a `let` that does, is deferred. It takes part in the merge with its priority like any other contribution, but its value is computed only after the merge has picked the winners, and only after the attributes it reads are final. A deferred contribution that loses to a higher priority is never evaluated.

Before any deferred value is computed, the `self` dependencies of the winners are checked for cycles, block instance by block instance. A cycle is [`E0607`](/reference/error-codes#e0607). [Late-bound self](/guide/self) covers this in detail.

<!-- file: late.stav -->

```stav
struct Download {
  version: Str
  url:     Str = "https://example.org/tool-#{self.version}.tar.gz"
}

struct Root { downloads: Map<Str, Download> = {} block download }

download tool { version = "1.2.3" }
include "./bump.stav"
```

<!-- file: bump.stav -->

```stav
download tool { version = force("1.3.0") }
```

<!-- run: eval -o toml late.stav -->

```toml
[downloads.tool]
version = "1.3.0"
url = "https://example.org/tool-1.3.0.tar.gz"
```

## Checks on the merged value

Some checks need the final value, so they run after merging:

- A required field, one without a default whose type is not an `Option`, must have a value ([`E0801`](/reference/error-codes#e0801)).
- Refinements of struct fields are checked on the merged value ([`E0803`](/reference/error-codes#e0803)).
- Every `Ref<T>` must name a block that exists in the merged value, or a label the host declares ([`E0802`](/reference/error-codes#e0802)).

<!-- file: refs.stav -->

```stav
struct Unit {
  exec:  List<Str> where nonEmpty
  after: List<Ref<Unit>> = []
}

struct Root { units: Map<Str, Unit> = {} block unit }

unit db { exec = ["/usr/bin/db"] }
unit api { exec = ["/usr/bin/api"], after = [db, cache] }
```

<!-- run: eval refs.stav -->

```text
error[E0802]: `units.api.after` refers to `cache`, which is not a declared `unit`
  ┌─ refs.stav:9:50
  │
9 │ unit api { exec = ["/usr/bin/api"], after = [db, cache] }
  │                                                  ^^^^^
  │
  = note: references to `Unit` name the blocks of `unit`
```
