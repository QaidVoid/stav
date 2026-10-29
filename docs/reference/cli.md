# The `stav` command

The `stav` command formats, type checks and evaluates Stav programs. It is the `stav-cli` package on crates.io, and [installation](/guide/installation) shows how to install it.

```sh
stav fmt [--check] <PATH>...
stav check [OPTIONS] <ENTRY>
stav eval [OPTIONS] <ENTRY>
```

`stav help <COMMAND>` and `stav <COMMAND> --help` describe every option, and `stav --version` prints the version.

## Options for every command

| Option | Meaning |
|---|---|
| `--message-format human` | Writes diagnostics as source excerpts with the spans underlined. This is the default. |
| `--message-format json` | Writes each diagnostic as one JSON object on its own line, for programs to read. |
| `--color auto` | Colors diagnostics when standard error is a terminal and `NO_COLOR` is unset or empty. This is the default. |
| `--color always` | Always colors diagnostics, even when `NO_COLOR` is set. |
| `--color never` | Never colors diagnostics. |

Diagnostics always go to standard error, and values and file lists to standard output. [Diagnostics](/reference/diagnostics) describes both formats.

## `stav fmt`

`stav fmt` rewrites files in their canonical layout.

```sh
stav fmt app.stav lib/       # format files and directories in place
stav fmt --check .           # list the files that would change
stav fmt - < app.stav        # format standard input to standard output
```

Each path is a file, a directory or `-`:

- A file is formatted in place, whatever its name.
- A directory stands for every `.stav` file below it, in the order of their names. Entries whose names start with a dot are skipped, and so are symbolic links inside the directory. A symbolic link given on the command line is followed.
- `-` reads standard input and writes the formatted text to standard output.

With `--check`, nothing is changed. The command prints the name of every file whose layout would change, `<stdin>` for standard input, and exits with status 1 if there is any. A file with syntax errors is not formatted. Its errors are reported and the command exits with status 1.

Given this file:

<!-- file: server.stav -->

```stav
struct Server {
  name: Str
    port: Int where 1..=65535 = 80
  replicas: Int = 1
}
struct Root { servers: Map<Str, Server> = {} block server }


server web { name="web", port=8080
  replicas=3 }
server db { name = "db" }   # internal only
```

`--check` lists it:

<!-- run: fmt --check server.stav -->

```text
server.stav
```

and formatting it gives:

<!-- run: fmt - < server.stav -->

```stav
struct Server {
  name:     Str
  port:     Int where 1..=65535 = 80
  replicas: Int = 1
}
struct Root { servers: Map<Str, Server> = {} block server }

server web {
  name     = "web"
  port     = 8080
  replicas = 3
}
server db { name = "db" } # internal only
```

### The layout

The formatter works on the lossless syntax tree, so it never changes what a program means and never reorders anything. Formatting its own output again changes nothing.

- Indentation is two spaces.
- Each statement, field, variant and `match` arm goes on its own line. Commas that separate statements in a body that spans several lines become line breaks.
- A body, struct declaration or sum type written on one line stays on one line when it fits.
- Lists, maps, argument lists and parameter lists stay on one line when they fit in 100 columns. Otherwise they put one element on each line, with a trailing comma.
- Runs of consecutive attributes, map entries, `let` bindings and `match` arms align their `=` or `=>`, and runs of consecutive struct fields align their types. A run is not aligned when its longest key is longer than 24 characters.
- Binary operators, `=`, `=>` and `->` have one space on each side. `:` has one space after it and none before it.
- Blank lines between elements are kept, and several in a row become one.
- Comments stay where they are. A comment on its own line stays before the element that follows it, and a comment at the end of a line stays at the end of that line.
- Strings are kept byte for byte, including multi-line strings and the expressions they interpolate.
- The file ends with exactly one line feed.

## `stav check`

`stav check <ENTRY>` loads the entry file and everything it imports and includes, and type checks the whole program without evaluating it. It reports every error it finds and the warnings of the program. It exits with status 0 when there are no errors, warnings or not.

<!-- file: levels.stav -->

```stav
type Level = debug | info

struct Root { verbose: Bool = verbose(Level.debug) }

fn verbose(level: Level) -> Bool = match level {
  debug => true
  info  => false
  _     => false
}
```

<!-- run: check levels.stav -->

```text
warning[W0434]: this arm can never match
  ┌─ levels.stav:8:3
  │
8 │   _     => false
  │   ^
  │
  = help: remove it, or move it before the arms that cover it
```

## `stav eval`

`stav eval <ENTRY>` type checks the program, evaluates it and prints its value. When the program does not check, nothing is evaluated. When evaluation fails, nothing is printed on standard output.

| Option | Meaning |
|---|---|
| `-o json`, `--output json` | Prints pretty JSON. This is the default. |
| `-o toml` | Prints TOML. The printed value must be a struct or a map. |
| `-o yaml` | Prints YAML in block style. |
| `--canonical` | Prints canonical JSON (RFC 8785) on one line. It cannot be combined with `--output`. |
| `--path <ATTR>` | Prints only the attribute at this path. |
| `--fuel <STEPS>` | Sets the number of evaluation steps after which evaluation stops. The default is 50000000. |

<!-- file: service.stav -->

```stav
struct Root {
  name:   Str = "web"
  port:   Int = 80
  labels: Map<Str, Str> = {}
}

labels."app.tier" = "frontend"
labels.team       = "core"
```

<!-- run: eval service.stav -->

```json
{
  "name": "web",
  "port": 80,
  "labels": {
    "app.tier": "frontend",
    "team": "core"
  }
}
```

<!-- run: eval -o yaml service.stav -->

```yaml
name: web
port: 80
labels:
  app.tier: frontend
  team: core
```

Canonical JSON sorts keys and has no whitespace, so equal values always print the same bytes:

<!-- run: eval --canonical service.stav -->

```json
{"labels":{"app.tier":"frontend","team":"core"},"name":"web","port":80}
```

### Selecting an attribute

`--path` takes an attribute path written the way diagnostics print paths. Keys are separated by dots. A key that contains anything other than ASCII letters, digits, `_` and `-` goes in double quotes, with the escapes of a string literal. An element of a list or set is named by its position, counted from zero, as in `--path servers.0`. Quote the whole argument for the shell.

<!-- run: eval --path 'labels."app.tier"' service.stav -->

```json
"frontend"
```

<!-- run: eval -o toml --path labels service.stav -->

```toml
"app.tier" = "frontend"
team = "core"
```

A path that names no attribute is [`E0805`](/reference/error-codes#e0805):

<!-- run: eval --path owner service.stav -->

```text
error[E0805]: there is no attribute `owner`
```

TOML needs a table at the top, so an attribute that is not a struct or a map cannot be printed as TOML ([`E0804`](/reference/error-codes#e0804)):

<!-- run: eval -o toml --path port service.stav -->

```text
error[E0804]: cannot render `port` as TOML: TOML needs a table at the top
  ┌─ service.stav:3:17
  │
3 │   port:   Int = 80
  │                 ^^
```

### Fuel

`--fuel` sets the step limit. Raise it for a program that legitimately needs more work, or lower it to keep untrusted input cheap. The other limits keep their defaults. [Totality and limits](/reference/totality) describes how steps are counted.

## Selecting a program

`check` and `eval` share these options.

| Option | Meaning |
|---|---|
| `--root <STRUCT>` | The root struct, declared in the entry file. The default is `Root`. |
| `--root-dir <DIR>` | The directory that imports, part files and path literals cannot leave. The default is the directory of the entry file. |
| `--std <DIR>` | Serves the module `std/<name>` from the file `<DIR>/<name>.stav`. |

The `stav` command takes one entry file. A program that embeds Stav can evaluate several entry files as one program, which [embedding in Rust](/guide/embedding) shows.

### The root struct

The entry file is a body of the root struct. Choosing another struct with `--root` evaluates the same file against another schema:

<!-- file: modes.stav -->

```stav
struct Dev { debug: Bool = true }

struct Root { debug: Bool = false }
```

<!-- run: eval --root Dev modes.stav -->

```json
{
  "debug": true
}
```

<!-- run: eval --root Prod modes.stav -->

```text
error[E0710]: there is no root struct `Prod`
  ┌─ modes.stav:1:1
  │
1 │ struct Dev { debug: Bool = true }
  │ ^
  │
  = help: declare `struct Prod { ... }` in the entry file
```

### The root directory

Files can only reach files inside their import root. By default that is the directory of the entry file, so an import that climbs out of it is [`E0702`](/reference/error-codes#e0702):

<!-- file: project/shared/site.stav -->

```stav
let region = "eu-north"
```

<!-- file: project/conf/main.stav -->

```stav
import "../shared/site.stav" as site

struct Root { region: Str = site.region }
```

<!-- run: eval project/conf/main.stav -->

```text
error[E0702]: the import path `../shared/site.stav` leaves its import root
  ┌─ project/conf/main.stav:1:8
  │
1 │ import "../shared/site.stav" as site
  │        ^^^^^^^^^^^^^^^^^^^^^
```

`--root-dir` widens the import root. The entry file must lie inside it.

<!-- run: eval --root-dir project project/conf/main.stav -->

```json
{
  "region": "eu-north"
}
```

### Standard modules

Imports of the form `"std/<name>"` name standard modules, which the host serves. The `stav` command has none of its own. `--std` names a directory to serve them from:

<!-- file: std/net.stav -->

```stav
## The port a service listens on by default.
fn default_port(tls: Bool) -> Int = if tls { 443 } else { 80 }
```

<!-- file: app/main.stav -->

```stav
import "std/net" as net

struct Root { port: Int = net.default_port(true) }
```

<!-- run: eval --std std app/main.stav -->

```json
{
  "port": 443
}
```

Without `--std`, the module does not exist:

<!-- run: eval app/main.stav -->

```text
error[E0701]: there is no module `std/net`
  ┌─ app/main.stav:1:8
  │
1 │ import "std/net" as net
  │        ^^^^^^^^^
```

[Imports and modules](/guide/modules) describes imports in full.

## Exit status

| Status | Meaning |
|---|---|
| 0 | Success. Warnings do not change the status. |
| 1 | A diagnostic is an error, or `fmt --check` found a file that would change. |
| 2 | The command line is wrong, for example an unknown option or value. |
| 3 | A file or a stream could not be read or written outside of diagnostics, for example a missing entry file. |

<!-- run: eval missing.stav -->

```text
error: cannot read `missing.stav`: no such file or directory
```
