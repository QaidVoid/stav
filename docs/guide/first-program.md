# A first program

This page builds a small configuration for a web service, one step at a time. Every step is a complete program that you can save as `app.stav` and evaluate.

## The root struct

A Stav program describes one value, and the type of that value is the **root struct**. By default the `stav` command uses the struct named `Root` in the entry file.

<!-- file: app.stav -->

```stav
struct Root {
  name: Str
  port: Int = 8080
}

name = "inventory"
```

`name` has no default, so the program must set it. `port` has a default of `8080`, which applies unless something sets another value. The statement `name = "inventory"` sets the attribute `name` of the root.

Evaluate the program:

```sh
stav eval app.stav
```

<!-- run: eval app.stav -->

```json
{
  "name": "inventory",
  "port": 8080
}
```

The output is JSON by default. `-o toml` and `-o yaml` select the other formats.

## Types catch mistakes early

Change the port to a string and evaluate again:

<!-- file: app.stav -->

```stav
struct Root {
  name: Str
  port: Int = 8080
}

name = "inventory"
port = "8080"
```

<!-- run: eval app.stav -->

```text
error[E0401]: expected `Int`, found `Str`
  ┌─ app.stav:7:8
  │
7 │ port = "8080"
  │        ^^^^^^
```

Stav reports the error before it evaluates anything. Nothing is printed on standard output, and the command exits with status 1. Every diagnostic has a code, here `E0401`, and the [error code index](../reference/error-codes) explains each one.

## Refinements

A type can be narrowed with `where`. A port must lie between 1 and 65535, and a name must not be empty:

<!-- file: app.stav -->

```stav
struct Root {
  name: Str where nonEmpty
  port: Int where 1..=65535 = 8080
}

name = "inventory"
port = 80_000
```

<!-- run: eval app.stav -->

```text
error[E0803]: value violates a refinement
  ┌─ app.stav:7:8
  │
3 │   port: Int where 1..=65535 = 8080
  │         ------------------- constraint declared here
  ·
7 │ port = 80_000
  │        ^^^^^^
  │        │
  │        80000 is not in 1..=65535
```

Refinements are checked on the final value, after everything has been merged. The diagnostic shows both the value and the constraint it breaks.

## Blocks

Configuration is easier to read in sections. A field whose type is a struct can be filled with a **block**: the name of the field, then a body in braces.

<!-- file: app.stav -->

```stav
struct Tls {
  cert: Str
  key:  Str
}

struct Server {
  host: Str = "0.0.0.0"
  port: Int where 1..=65535 = 8080
  tls:  Option<Tls>
}

struct Root {
  name:   Str where nonEmpty
  server: Server = {}
}

name = "inventory"

server {
  port = 8443
  tls {
    cert = "/etc/inventory/cert.pem"
    key  = "/etc/inventory/key.pem"
  }
}
```

<!-- run: eval -o toml app.stav -->

```toml
name = "inventory"

[server]
host = "0.0.0.0"
port = 8443

[server.tls]
cert = "/etc/inventory/cert.pem"
key = "/etc/inventory/key.pem"
```

Inside `server { }` the statements are typed against `Server`, so `port` means `server.port`. The same could be written as `server.port = 8443` at the top level. Blocks and dotted paths are two ways to say the same thing.

`tls` is an `Option<Tls>`. An option without a default is `none`, and a block or assignment under it makes it present. The default `server: Server = {}` means "a `Server` with every field at its own default".

## Named blocks

Many configurations hold a collection of similar things: upstreams, users, jobs. A map field with a `block` clause lets each entry be written as a labeled block.

<!-- file: app.stav -->

```stav
type Level = debug | info | warn | error

struct Upstream {
  url:    Str where matches(r"https?://.*")
  weight: Int where 1..=100 = 10
}

struct Root {
  name:      Str where nonEmpty
  log_level: Level = info
  upstreams: Map<Str, Upstream> = {} block upstream
}

name      = "inventory"
log_level = warn

upstream primary { url = "https://a.example.org", weight = 90 }
upstream backup { url = "https://b.example.org" }
```

<!-- run: eval -o yaml app.stav -->

```yaml
name: inventory
log_level: warn
upstreams:
  backup:
    url: "https://b.example.org"
    weight: 10
  primary:
    url: "https://a.example.org"
    weight: 90
```

`block upstream` says that a block `upstream <label> { ... }` adds the entry `<label>` to `upstreams`. Map keys are always printed in sorted order, so the output does not depend on the order of the blocks.

`Level` is a **sum type** with four variants. Where a `Level` is expected, the bare name `warn` is the variant. Any other name is an error:

<!-- file: typo.stav -->

```stav
type Level = debug | info | warn | error

struct Root {
  log_level: Level = info
}

log_level = wran
```

<!-- run: check typo.stav -->

```text
error[E0302]: unknown name `wran`, which is not a variant of `Level`
  ┌─ typo.stav:7:13
  │
7 │ log_level = wran
  │             ^^^^
  │
  = help: did you mean `warn`?
```

## Picking out one value

`--path` prints a single attribute instead of the whole value:

<!-- run: eval --path upstreams.primary.url app.stav -->

```json
"https://a.example.org"
```

## Formatting

`stav fmt` rewrites files in their canonical layout: two spaces of indentation, one statement per line, and aligned `=` and `:` in runs of related lines. Run it with `--check` to list the files that would change without changing them:

```sh
stav fmt --check .
```

## Next

The [tour of the language](./tour) covers functions, `match`, collections, strings and the rest of the expression language. [Structuring configuration](./structuring) shows how to split a program into files and reusable parts.
