# Imports and modules

Declarations that several programs share belong in a file of their own, which other files import. Such a file is a **library**. A library holds `struct`, `type` and `fn` declarations, top-level `let` bindings and imports of its own. It contributes no values, so it contains no statements.

## Importing a library

`import` loads a file and binds it to an alias. Everything the library declares is reached through the alias, as `alias.name`:

<!-- file: lib/net.stav -->

```stav
## The protocol an endpoint speaks.
type Scheme = http | https

## A network endpoint.
struct Endpoint {
  scheme: Scheme = https
  host:   Str where nonEmpty
  port:   Int where 1..=65535 = 443
}

let default_port = 8080

fn url(e: Endpoint) -> Str = "#{e.scheme}://#{e.host}:#{e.port}"
```

<!-- file: app.stav -->

```stav
import "./lib/net.stav" as net

struct Root {
  api:  net.Endpoint
  urls: List<Str> = []
}

api {
  scheme = http
  host   = "localhost"
  port   = net.default_port
}

urls = [net.url(self.api)]
```

<!-- run: eval app.stav -->

```json
{
  "api": {
    "scheme": "http",
    "host": "localhost",
    "port": 8080
  },
  "urls": [
    "http://localhost:8080"
  ]
}
```

A few rules keep imports predictable:

- Every import has an alias, and an import never adds unqualified names. Two imports can therefore never collide.
- Everything at the top level of a library is public. There is no export list.
- Imports come first in a file, before any other item.
- A library is loaded and checked once per program, however many files import it. Its top-level `let` values are computed once.
- Imports cannot form a cycle.

Types keep their identity across imports. `net.Endpoint` is the struct declared in `lib/net.stav`, and a struct with the same name and fields declared elsewhere is a different type.

Where a type is expected, its variants can still be written bare. `scheme = http` above needs no alias, because `scheme` expects a `net.Scheme`. The qualified form `net.Scheme.http` works everywhere.

## Libraries contain no statements

A statement in an imported file is an error, because only entry files and part files contribute to the value:

<!-- file: lib/broken.stav -->

```stav
let port = 8080

port = 9090
```

<!-- file: broken.stav -->

```stav
import "./lib/broken.stav" as broken

struct Root {
  port: Int = broken.port
}
```

<!-- run: check broken.stav -->

```text
error[E0209]: a library cannot contain statements
  ┌─ lib/broken.stav:3:1
  │
3 │ port = 9090
  │ ^^^^
  │
  = help: move the statement into an entry file or a part file
```

To share statements, write a part instead, either as a function that returns a `Part<S>` or as a part file. [Structuring configuration](./structuring#parts) shows both.

## Paths and the import root

An import path is a string that starts with `./` or `../` and ends with `.stav`. It is relative to the file that contains it.

Every file belongs to an **import root**, a directory that its imports, part files and path literals cannot leave. For the `stav` command, the import root is the directory of the entry file. `--root-dir` names a wider one, for example the root of a repository whose programs share a `lib/` directory:

```sh
stav eval --root-dir . services/api/main.stav
```

## Standard modules

`import "std/<name>"` imports a standard module. Standard modules have no extension, and the program that evaluates Stav decides what they contain. The `stav` command serves them from a directory with `--std`: the module `std/<name>` is the file `<DIR>/<name>.stav`.

<!-- file: modules/units.stav -->

```stav
let kib = 1024
let mib = 1024 * kib

fn megabytes(n: Int) -> Int = n * mib
```

<!-- file: limits.stav -->

```stav
import "std/units" as units

struct Root {
  memory: Int = units.megabytes(512)
  buffer: Int = 64 * units.kib
}
```

<!-- run: eval --std modules limits.stav -->

```json
{
  "memory": 536870912,
  "buffer": 65536
}
```

A program that embeds Stav serves standard modules through its loader, and it can also offer **host modules** under namespaces of its own, such as `"ns/target"`, whose source it generates for each evaluation. [Embedding in Rust](./embedding#what-a-host-can-add) describes both.

## Next

[Embedding in Rust](./embedding) shows how a Rust program loads, checks and evaluates Stav programs and reads their values into its own types.
