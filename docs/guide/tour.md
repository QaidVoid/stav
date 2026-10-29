# A tour of the language

This tour walks through the parts of Stav you use every day. Each section is a complete program with the value it evaluates to. The [reference](../reference/lexical-structure) has the precise rules.

## Files, comments and doc comments

A file is a sequence of declarations and statements. Newlines separate them, and so do commas. `#` starts a comment that runs to the end of the line. `##` starts a doc comment, which documents the declaration, field or variant that follows it.

<!-- file: comments.stav -->

```stav
## The settings of the service.
struct Root {
  ## How many requests run at once.
  workers: Int = 4
  timeout: Int = 30 # seconds
}

workers = 8, timeout = 10
```

<!-- run: eval comments.stav -->

```json
{
  "workers": 8,
  "timeout": 10
}
```

## Scalars

Stav has booleans, 64-bit integers, finite floating point numbers and strings. Integers can be written in hexadecimal, octal or binary, and `_` separates digits.

<!-- file: scalars.stav -->

```stav
struct Root {
  enabled: Bool
  mode:    Int
  limit:   Int
  ratio:   Float
  greet:   Str
}

enabled = true
mode    = 0o755
limit   = 1_000_000
ratio   = 0.75
greet   = "hello"
```

<!-- run: eval scalars.stav -->

```json
{
  "enabled": true,
  "mode": 493,
  "limit": 1000000,
  "ratio": 0.75,
  "greet": "hello"
}
```

There is no implicit conversion between types. An `Int` never becomes a `Str` or a `Float` on its own. `toStr`, `toFloat`, `toInt` and `parseInt` convert explicitly, and an integer literal is accepted where a `Float` is expected.

## Strings

Strings come in three forms:

- `"..."` is a single-line string with escapes such as `\n`, `\t`, `\"` and `\u{e9}`.
- `"""` on its own line starts a multi-line string. The indentation of the closing `"""` is removed from every line, and nothing else is escaped, so shell scripts and file contents can be pasted as they are.
- `r"..."` is a raw string with no escapes at all, which suits regular expressions. `r"""` starts a raw multi-line string.

`#{expr}` inserts the text of an expression into a string. Strings, numbers, booleans and the variants of sum types can be inserted. `\#{` writes a literal `#{`.

<!-- file: strings.stav -->

```stav
struct Root {
  greeting: Str
  script:   Str
  pattern:  Str
}

let name = "world"
let count = 3

greeting = "hello #{name}, #{count} times\n"
script   = """
  #!/bin/sh
  printf 'started %s\n' "$HOME"
  echo "#{name}"
  """
pattern  = r"^\d+\.\d+$"
```

<!-- run: eval strings.stav -->

```json
{
  "greeting": "hello world, 3 times\n",
  "script": "#!/bin/sh\nprintf 'started %s\\n' \"$HOME\"\necho \"world\"\n",
  "pattern": "^\\d+\\.\\d+$"
}
```

Inside the multi-line string, `\n` and `$HOME` are kept exactly as written, and only `#{name}` is replaced.

## Lists, sets and maps

<!-- file: collections.stav -->

```stav
struct Root {
  ports: List<Int> = []
  tags:  Set<Str> = []
  env:   Map<Str, Str> = {}
}

ports = [80, 443, 80]
tags  = ["web", "edge", "web"]
env   = { RUST_LOG = "info", "HTTP PROXY" = "none" }
```

<!-- run: eval collections.stav -->

```json
{
  "ports": [
    80,
    443,
    80
  ],
  "tags": [
    "edge",
    "web"
  ],
  "env": {
    "HTTP PROXY": "none",
    "RUST_LOG": "info"
  }
}
```

A list keeps its order and its duplicates. A set drops duplicates and keeps its elements in a canonical order. A map keeps its keys in sorted order. Map keys that are not plain names are written as strings.

An empty literal such as `[]` or `{}` needs a type from its context. `let xs = []` alone is an error, while `let xs: List<Str> = []` is fine.

## Structs

A `struct` declares named fields with types, optional defaults and optional refinements. A field without a default must be set, unless its type is an `Option`, which defaults to `none`.

A struct literal names the struct and sets some fields. Fields it leaves out take their defaults. When the expected type is already known, the struct name can be left out.

<!-- file: structs.stav -->

```stav
struct Point {
  x: Int = 0
  y: Int = 0
}

struct Root {
  origin: Point = {}
  corners: List<Point> = []
}

corners = [Point { x = 1 }, { x = 4, y = 3 }]
```

<!-- run: eval structs.stav -->

```json
{
  "origin": {
    "x": 0,
    "y": 0
  },
  "corners": [
    {
      "x": 1,
      "y": 0
    },
    {
      "x": 4,
      "y": 3
    }
  ]
}
```

Struct types are nominal. Two structs with the same fields are still different types.

## Sum types and `match`

A `type` declares a sum type: a value that is exactly one of its variants. A variant is either a bare name or carries named payload fields.

<!-- file: sums.stav -->

```stav
type Ready =
  | immediate
  | notify(fd: Int where 0..=1023)
  | check(cmd: List<Str>, interval: Int = 1)

fn describe(r: Ready) -> Str = match r {
  immediate => "ready at start"
  notify(fd) => "notifies on fd #{fd}"
  check(cmd, interval = i) if i > 10 => "slow check: #{join(cmd, " ")}"
  check(cmd, ..) => "checks with #{join(cmd, " ")}"
}

struct Root {
  ready: List<Ready> = []
  notes: List<Str> = []
}

let all: List<Ready> = [immediate, notify(3), check(cmd = ["pg_isready"], interval = 30)]

ready = all
notes = [for r in all -> describe(r)]
```

<!-- run: eval sums.stav -->

```json
{
  "ready": [
    "immediate",
    {
      "notify": {
        "fd": 3
      }
    },
    {
      "check": {
        "cmd": [
          "pg_isready"
        ],
        "interval": 30
      }
    }
  ],
  "notes": [
    "ready at start",
    "notifies on fd 3",
    "slow check: pg_isready"
  ]
}
```

A variant with a payload is built with call syntax, by position or by name, and fields with defaults can be left out. In JSON, a bare variant renders as its name and a variant with a payload as an object keyed by its name.

The bare names `immediate`, `notify` and `check` mean variants only where a `Ready` is expected. That is why the `let` above is annotated with `List<Ready>`. `Ready.immediate` works anywhere.

`match` must be exhaustive. Every variant needs an arm, unless a `_` arm catches the rest. A guard after `if` refines an arm, and `..` in a pattern ignores the remaining payload fields. A missing variant is an error that names it:

<!-- file: missing.stav -->

```stav
type Level = debug | info | warn | error

fn verbose(l: Level) -> Bool = match l {
  debug => true
  info => true
}

struct Root {
  verbose: Bool = verbose(warn)
}
```

<!-- run: check missing.stav -->

```text
error[E0434]: non-exhaustive `match` on `Level`
  ┌─ missing.stav:3:32
  │
3 │ fn verbose(l: Level) -> Bool = match l {
  │                                ^^^^^^^
  │
  = note: missing: `warn`, `error`
  = help: add the missing arms or a `_ => ...` arm
```

## Options

`Option<T>` holds `none` or a value of `T`. A `T` is accepted wherever an `Option<T>` is expected, so there is no constructor to write. `a ?? b` unwraps an option or falls back to `b`, and `a?.field` reads a field of an optional struct.

<!-- file: options.stav -->

```stav
struct Tls {
  cert: Str
  port: Int = 443
}

struct Root {
  tls:   Option<Tls>
  proxy: Option<Str>
  port:  Int = 0
}

tls { cert = "/etc/cert.pem" }

port = self.tls?.port ?? 80
```

<!-- run: eval options.stav -->

```json
{
  "tls": {
    "cert": "/etc/cert.pem",
    "port": 443
  },
  "proxy": null,
  "port": 443
}
```

`self` is the final value of the enclosing block, here the root. [Late-bound self](./self) explains it.

## Let, if and functions

`let` binds a name for the rest of the file or body. `if` is an expression and always needs an `else`. A `let ... in` expression binds names for a single expression.

Functions are declared with `fn`. Parameters and the return type are always annotated. Parameters may have defaults and refinements, and arguments can be passed by name.

<!-- file: functions.stav -->

```stav
fn url(host: Str, tls: Bool = true, port: Int where 1..=65535 = 443) -> Str =
  let scheme = if tls { "https" } else { "http" }
  in "#{scheme}://#{host}:#{port}"

struct Root {
  urls: List<Str> = []
}

let local = url("localhost", tls = false, port = 8080)

urls = [url("example.org"), local]
```

<!-- run: eval functions.stav -->

```json
{
  "urls": [
    "https://example.org:443",
    "http://localhost:8080"
  ]
}
```

A function cannot call itself, directly or through another function. Stav is total, so repetition is written with loops over finite collections instead. A lambda is written `fn(x: Int) = x * 2` and can be passed to builtins such as `map`, `filter` and `fold`.

<!-- file: lambdas.stav -->

```stav
fn factorial(n: Int where 0..=20) -> Int =
  fold(range(1, n + 1), 1, fn(acc: Int, i: Int) = acc * i)

struct Root {
  doubled:   List<Int> = []
  evens:     List<Int> = []
  factorial: Int = 0
}

let numbers = [1, 2, 3, 4, 5, 6]

doubled   = map(numbers, fn(x: Int) = x * 2)
evens     = filter(numbers, fn(x: Int) = x % 2 == 0)
factorial = factorial(10)
```

<!-- run: eval lambdas.stav -->

```json
{
  "doubled": [
    2,
    4,
    6,
    8,
    10,
    12
  ],
  "evens": [
    2,
    4,
    6
  ],
  "factorial": 3628800
}
```

[Builtins](../reference/builtins) lists every built-in function.

## Comprehensions

A comprehension builds a list or a map from another collection. An `if` clause filters the elements.

<!-- file: comprehensions.stav -->

```stav
struct Root {
  hosts: List<Str> = []
  ports: Map<Str, Int> = {}
}

let services = { api = 8000, admin = 8001, metrics = 9100 }

hosts = [for name, port in services if port < 9000 -> "#{name}.internal:#{port}"]
ports = {for name, port in services -> "#{name}-tls" = port + 1000}
```

<!-- run: eval comprehensions.stav -->

```json
{
  "hosts": [
    "admin.internal:8001",
    "api.internal:8000"
  ],
  "ports": {
    "admin-tls": 9001,
    "api-tls": 9000,
    "metrics-tls": 10100
  }
}
```

## Statements: `if` and `for`

In a body, `if` and `for` are statements. They contribute the statements of their bodies to the value, conditionally or once per element.

<!-- file: statements.stav -->

```stav
struct Root {
  replicas: Int = 1
  hosts:    List<Str> = []
}

let production = true
let regions = ["eu", "us"]

if production {
  replicas = 3
}

for region in regions {
  hosts = ["#{region}.example.org"]
}
```

<!-- run: eval statements.stav -->

```json
{
  "replicas": 3,
  "hosts": [
    "eu.example.org",
    "us.example.org"
  ]
}
```

The `for` loop contributes `hosts` twice, and the two lists are concatenated. That is merging, and [the next chapter](./structuring) is about it.

The condition of an `if` statement and the collection of a `for` statement cannot read `self`. They must be known before merging starts, because they decide which contributions exist.

## Next

[Structuring configuration](./structuring) covers blocks, merging, priorities, parts and part files.
