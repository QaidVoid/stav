# Types

Every Stav expression has a type, and the whole program is type checked before any of it is evaluated. This page lists the built-in types, the types a program declares, and the rules that decide which values fit where.

## Built-in types

| Type | Values | Notes |
|---|---|---|
| `Bool` | `true` and `false` | |
| `Int` | signed 64-bit integers | overflow is [E0601](/reference/error-codes#e0601) |
| `Float` | finite binary64 numbers | a result that is not finite is [E0602](/reference/error-codes#e0602) |
| `Str` | UTF-8 strings | |
| `Path` | paths inside an import root | see [path literals](/reference/lexical-structure#path-literals) |
| `Data` | any value that can be rendered | see [`Data`](#data) |
| `List<T>` | ordered sequences | |
| `Set<T>` | sets in a canonical order | `T` must be a [key type](#key-types) |
| `Map<K, V>` | maps in a canonical key order | `K` must be `Str` or a sum type without payloads |
| `Option<T>` | `none` or a value of `T` | see [options](#options) |
| `Ref<T>` | a reference to a labeled block of the struct `T` | see [references](#references) |
| `Part<S>` | a fragment of a body of the struct `S` | see [parts](/guide/structuring) |
| `Fn(A, B) -> R` | functions | see [function types](#function-types) |

Every `struct` and `type` declared in scope is a type as well, and so is every [symbol type](#symbol-types) the host program registers.

## Nominal typing

Structs and sum types are nominal. Two declarations with the same fields or the same variants are still different types, and a value of one never fits the other.

<!-- file: nominal.stav -->

```stav
struct A { x: Int = 0 }

struct B { x: Int = 0 }

struct Root { a: A = {} }

a = B { x = 1 }
```

<!-- run: check nominal.stav -->

```text
error[E0401]: expected `A`, found `B`
  ┌─ nominal.stav:7:5
  │
7 │ a = B { x = 1 }
  │     ^^^^^^^^^^^
```

A type is identified by the file that declares it and its name, so a type imported from a module never unifies with a local type of the same name. Functions and types that a program declares are not generic. Only the built-in type constructors and the [builtins](/reference/builtins) are.

## Key types

A key type can be a `Set` element. Key types are `Bool`, `Int`, `Str`, `Path`, sum types whose variants have no payload, symbol types and `Ref<T>`. `Float`, collections, structs and sum types with payloads are not key types.

Map keys are stricter. A map key is `Str` or a sum type whose variants have no payload. Anything else is [E0411](/reference/error-codes#e0411):

<!-- file: keys.stav -->

```stav
struct Point { x: Float, y: Float }

struct Root {
  points: Set<Point> = []
  ports:  Map<Int, Str> = {}
}
```

<!-- run: check keys.stav -->

```text
error[E0411]: `Point` cannot be a set element
  ┌─ keys.stav:4:15
  │
4 │   points: Set<Point> = []
  │               ^^^^^
  │
  = help: set elements are `Bool`, `Int`, `Str`, `Path`, sum types without fields, symbol types or `Ref<T>`

error[E0411]: `Int` cannot be a map key
  ┌─ keys.stav:5:15
  │
5 │   ports:  Map<Int, Str> = {}
  │               ^^^
  │
  = help: map keys are `Str` or a sum type whose variants have no fields
```

## Structs

A struct declares named fields. Each field has a type and may have a default value and a [`block` clause](/reference/expressions#blocks).

<!-- file: structs.stav -->

```stav
## A web upstream.
struct Upstream {
  ## The base URL.
  url:    Str where matches(r"https?://.*")
  weight: Int where 1..=100 = 10
  tags:   Set<Str> = []
  backup: Option<Str>
}

struct Root { upstreams: Map<Str, Upstream> = {} block upstream }

upstream primary { url = "https://a.example.org" }
upstream backup {
  url    = "https://b.example.org"
  weight = 5
  backup = "https://c.example.org"
}
```

<!-- run: eval structs.stav -->

```json
{
  "upstreams": {
    "backup": {
      "url": "https://b.example.org",
      "weight": 5,
      "tags": [],
      "backup": "https://c.example.org"
    },
    "primary": {
      "url": "https://a.example.org",
      "weight": 10,
      "tags": [],
      "backup": null
    }
  }
}
```

- A field without a default whose type is not an `Option` is required. A required field that is still missing in the final value is [E0801](/reference/error-codes#e0801).
- A field of type `Option<T>` without a default defaults to `none`.
- A default is an expression evaluated in the scope of the file that declares the struct. It may read other fields through [`self`](/guide/self). A default has the lowest [priority](/reference/merging), so any assignment replaces it.
- A field cannot hold a function or a part, at any depth ([E0412](/reference/error-codes#e0412)). Functions and parts are not data.
- A type cannot contain itself, directly or through `List`, `Set`, `Map`, `Option` or a variant payload ([E0410](/reference/error-codes#e0410)). Every value is therefore a finite tree. To point at another value, use a [reference](#references).

<!-- file: recursive.stav -->

```stav
struct Node { children: List<Node> = [] }

struct Root { tree: Node = {} }
```

<!-- run: check recursive.stav -->

```text
error[E0410]: `Node` contains itself
  ┌─ recursive.stav:1:15
  │
1 │ struct Node { children: List<Node> = [] }
  │               ^^^^^^^^
  │
  = note: cycle: Node -> Node
  = help: values must be finite trees, so refer to other values by label with `Ref<T>`
```

## Sum types

A sum type lists its variants. A variant is either a bare name or a name with named payload fields. Payload fields are declared like function parameters, with optional defaults and refinements.

<!-- file: sums.stav -->

```stav
type Level = debug | info | warn | error

type Ready =
  | immediate
  | notify(fd: Int where 0..=1023)
  | check(cmd: List<Str>, interval: Int = 1)

struct Service {
  level: Level = info
  ready: Ready = immediate
}

struct Root { services: Map<Str, Service> = {} block service }

service a { ready = notify(3) }
service b { ready = check(cmd = ["probe"]) }
service c {
  level = Level.warn
  ready = Ready.notify(fd = 4)
}
service d { }
```

<!-- run: eval -o yaml sums.stav -->

```yaml
services:
  a:
    level: info
    ready:
      notify:
        fd: 3
  b:
    level: info
    ready:
      check:
        cmd:
          - probe
        interval: 1
  c:
    level: warn
    ready:
      notify:
        fd: 4
  d:
    level: info
    ready: immediate
```

- Variant names are unique within one sum type. Different sum types may reuse them.
- A variant without payload is written as a bare name, such as `info`, where a value of its sum type is expected.
- A variant with payload is constructed with call syntax. Arguments may be positional, in the order of the declaration, or named. Payload fields with defaults may be left out.
- The qualified forms `Level.info` and `Ready.notify(fd = 3)` work everywhere, and so does `alias.Ready.notify(fd = 3)` for a type imported as `alias`.
- A variant without payload renders as its name. A variant with payload renders as a map from the variant name to its payload fields.

## Options

`Option<T>` holds either `none` or a value of `T`. A value of `T` fits wherever an `Option<T>` is expected and is wrapped implicitly, so there is no constructor for the present case. An `Option` of an `Option` is [E0413](/reference/error-codes#e0413).

Reading an option uses `?.`, `??` or a [`match`](/reference/expressions#match):

<!-- file: options.stav -->

```stav
struct Tls {
  cert: Str = "/etc/tls/cert.pem"
  port: Int = 443
}

struct Root {
  tls:   Option<Tls>
  plain: Option<Tls>
  limit: Option<Int>
  port:  Int = 0
}

tls { port = 8443 }
limit = 5
port  = self.tls?.port ?? 80
```

<!-- run: eval options.stav -->

```json
{
  "tls": {
    "cert": "/etc/tls/cert.pem",
    "port": 8443
  },
  "plain": null,
  "limit": 5,
  "port": 8443
}
```

In TOML output an attribute that is `none` is left out, because TOML has no null value.

## Expected types and inference

Type checking is bidirectional. Where a position requires a type, that expected type is pushed into the expression: the right side of an attribute, function arguments, struct fields, map values, list elements and function results. Where nothing is expected, the type is inferred from the expression itself, as for a `let` without an annotation, the operands of operators and the value a `match` inspects.

- The parameters and the result of a `fn` declaration must be annotated. The parameters of a lambda must be annotated, and its result type may be left out.
- A `let` binding takes the type of its value unless it is annotated.
- A list literal has the type of its first element, and every other element is checked against it.
- `[]`, `{}` and `none` have no type of their own and need an expected type ([E0430](/reference/error-codes#e0430)).
- A brace literal without a type name is a struct literal where a struct is expected and a map literal everywhere else.
- The branches of `if` and `match` must all have one type, the expected type when there is one, or else the type of the first branch.

<!-- file: inference.stav -->

```stav
struct Root { hosts: List<Str> = [] }

let empty = []
let typed: List<Str> = []

hosts = typed
```

<!-- run: check inference.stav -->

```text
error[E0430]: cannot infer the type of `[]`
  ┌─ inference.stav:3:13
  │
3 │ let empty = []
  │             ^^
  │
  = help: add an annotation: `let xs: List<Str> = []`
```

## Coercions

A value changes its type implicitly only where an expected type asks for it, and only in these ways:

| From | To | Rule |
|---|---|---|
| `T` | `Option<T>` | wrapped implicitly |
| a list literal | `Set<T>` | elements checked against `T`, duplicates removed |
| an integer literal | `Float` | literals only, never a computed `Int` |
| `T` | `T where p` | accepted, and the predicate is checked when the value is known |
| `T where p` | `T` | always |
| an identifier | a sum type | the variant of that name |
| an identifier or a symbol word | a symbol type | a symbol literal |
| an identifier or a string literal | `Ref<T>` | a reference to that label |
| a string literal | a symbol type | only when the host allows strings for that type |
| any value that can be rendered | `Data` | see [`Data`](#data) |

There are no other conversions. An `Int` never becomes a `Str`, and arithmetic never mixes `Int` and `Float` ([E0431](/reference/error-codes#e0431)). Use [`toFloat`, `toInt` and `toStr`](/reference/builtins) to convert explicitly.

<!-- file: coercions.stav -->

```stav
struct Root {
  ratio: Float = 0.0
  avg:   Float = 0.0
}

let n = 3

ratio = n
avg   = 1.5 * 2
```

<!-- run: check coercions.stav -->

```text
error[E0401]: expected `Float`, found `Int`
  ┌─ coercions.stav:8:9
  │
8 │ ratio = n
  │         ^
  │
  = help: convert it with `toFloat(...)`

error[E0431]: cannot mix `Int` and `Float` in arithmetic
  ┌─ coercions.stav:9:13
  │
9 │ avg   = 1.5 * 2
  │             ^
  │
  = help: convert the `Int` with `toFloat(...)`
```

## Name resolution

A bare identifier in an expression is resolved before the program runs, in this order:

1. A lexical binding: a `let`, a parameter, a `for` or comprehension variable, a pattern binding, a top-level `let` or `fn` of the file, an import alias or a declaration of the host's prelude. When the expected type is a symbol type or `Ref<T>`, a binding whose type cannot be accepted there is skipped, as described below.
2. A variant, when the expected type is a sum type that has a variant of that name. In call position, `name(...)` is the variant with payload of that name.
3. A symbol, when the expected type is a symbol type.
4. A reference, when the expected type is `Ref<T>`.
5. Otherwise the name is unknown ([E0302](/reference/error-codes#e0302)), and the diagnostic suggests the closest names in scope and the variants of the expected type.

Where a symbol type or `Ref<T>` is expected, the type of the binding decides. If the binding's type can be accepted there, the binding wins: a `let` of type `Ref<Unit>` passed where a `Ref<Unit>` is expected is that `let`. If it cannot, the binding could not have been meant, so the identifier is a symbol or a reference of the expected type. A function, a part, a module alias or a value of another type therefore does not hide a symbol or a reference of the same name. A top-level `let` used before its declaration is never skipped, whatever its type, so the use is [E0305](/reference/error-codes#e0305). No warning is reported, because a name that is no real symbol or label is still caught: by the host's validation of symbols, or by [E0802](/reference/error-codes#e0802) for references.

<!-- file: past.stav -->

```stav
struct Unit { after: List<Ref<Unit>> = [] }

struct Root { units: Map<Str, Unit> = {} block unit }

fn db() -> Part<Root> = part {
  unit db {}
}

let primary: Ref<Unit> = "cache"

include db()

unit cache {}

unit api {
  after = [db, primary]
}
```

<!-- run: eval -o toml past.stav -->

```toml
[units.api]
after = ["db", "cache"]

[units.cache]
after = []

[units.db]
after = []
```

`db` is a function, which cannot be a `Ref<Unit>`, so it is the reference to the block `db`. `primary` has the type `Ref<Unit>`, so it is the binding, and its value is the label `cache`.

Variants follow a different rule: a lexical binding always wins over a variant of the same name, whatever its type. Because shadowing a variant is almost always a mistake, the checker warns about it with [W0302](/reference/error-codes#w0302):

<!-- file: shadow.stav -->

```stav
type Level = debug | info | warn

struct Root { level: Level = info }

let warn = 1

level = warn
```

<!-- run: check shadow.stav -->

```text
warning[W0302]: `warn` refers to a binding, not to the variant of `Level`
  ┌─ shadow.stav:7:9
  │
7 │ level = warn
  │         ^^^^
  │
  = help: rename the binding, or write `Level.warn` for the variant

error[E0401]: expected `Level`, found `Int`
  ┌─ shadow.stav:7:9
  │
7 │ level = warn
  │         ^^^^
```

## References

`Ref<T>` refers to a labeled block of the struct `T` elsewhere in the same root. A reference is written as an identifier or a string literal, and its value is the label.

- When the program is checked, Stav finds the target map of `T`: the one field of type `Map<Str, T>` with a `block` clause that can be reached from the root struct through struct fields. If there is no such field, or more than one, every use of `Ref<T>` is [E0414](/reference/error-codes#e0414).
- After merging, every reference is checked against the keys of the target map, and against any extra labels the host declares. A label that names no block is [E0802](/reference/error-codes#e0802).
- A reference renders as its label.

<!-- file: refs.stav -->

```stav
struct Unit {
  exec:  List<Str> where nonEmpty
  after: List<Ref<Unit>> = []
}

struct Root { units: Map<Str, Unit> = {} block unit }

unit db { exec = ["/usr/bin/db"] }
unit cache { exec = ["/usr/bin/cache"] }
unit api {
  exec  = ["/usr/bin/api"]
  after = [db, "cahce"]
}
```

<!-- run: eval refs.stav -->

```text
error[E0802]: `units.api.after` refers to `cahce`, which is not a declared `unit`
   ┌─ refs.stav:12:16
   │
12 │   after = [db, "cahce"]
   │                ^^^^^^^
   │
   = note: references to `Unit` name the blocks of `unit`
   = help: did you mean `cache`?
```

## Data

`Data` is the type of any value that can be rendered. It lets a schema accept arbitrary structured content, such as the settings of another program.

- Every value whose type contains no function and no part fits `Data`.
- Where `Data` is expected, literals are checked element by element against `Data`, so lists and maps may mix types. The identifier keys of such a map literal are strings.
- `none` is a valid `Data` value.
- A `Data` value can be stored, merged, compared and rendered, and nothing else. It has no fields and no operators, and it cannot be matched.
- Two `Data` values merge by their shape: maps merge per key, lists concatenate and scalars follow priorities. The details are on the [merging](/reference/merging) page.

<!-- file: data.stav -->

```stav
struct Root { settings: Data = none }

settings = { port = 80, hosts = ["a", "b"], tls = { enabled = true, ciphers = none } }
```

<!-- run: eval -o toml data.stav -->

```toml
[settings]
hosts = ["a", "b"]
port = 80

[settings.tls]
enabled = true
```

## Function types

`Fn(A, B) -> R` is the type of a function that takes an `A` and a `B` and returns an `R`.

- A top-level `fn` used as a value has a function type without its parameter defaults. A call through a function value passes every argument, positionally.
- A lambda captures the bindings around it.
- [Builtins](/reference/builtins) are not values. Wrap one in a lambda to pass it on, as in `map(xs, fn(x: Int) = toStr(x))`.
- Function types cannot be rendered and cannot appear in struct fields.

<!-- file: functions.stav -->

```stav
struct Root { labels: List<Str> = [] }

fn label(n: Int, prefix: Str = "#") -> Str = "#{prefix}#{n}"

fn apply(f: Fn(Int, Str) -> Str, xs: List<Int>) -> List<Str> =
  map(xs, fn(x: Int) = f(x, "n"))

labels = apply(label, [1, 2]) ++ [label(3)]
```

<!-- run: eval functions.stav -->

```json
{
  "labels": [
    "n1",
    "n2",
    "#3"
  ]
}
```

## Refinements

`T where p1 && p2` narrows the values of `T` with one or more predicates.

| Predicate | Applies to | Meaning |
|---|---|---|
| `a..b`, `a..=b`, `a..`, `..b`, `..=b` | `Int`, `Float` | a range, where `..` excludes the end and `..=` includes it |
| the same ranges | `Str`, `List`, `Set`, `Map` | a range on the length |
| `nonEmpty` | `Str`, `List`, `Set`, `Map` | the length is at least 1 |
| `matches(r"...")` | `Str` | the regular expression matches the whole string |
| `f` or `f(args)` | any type `T` | a function in scope of type `Fn(T) -> Bool`, or `Fn(T, ...) -> Bool` with the other arguments given |

- `matches` always matches the whole string, as if the pattern were anchored at both ends. Regular expressions run in linear time. An invalid pattern is [E0415](/reference/error-codes#e0415) when the program is checked.
- Refinements are checked on the final, merged values of struct fields, on function arguments when the function is called, on variant payloads when the variant is constructed, and on `let` values with an annotated refined type.
- A failed check is [E0803](/reference/error-codes#e0803). It shows where the refinement is declared and what is wrong with the value.

<!-- file: refinements.stav -->

```stav
fn even(n: Int) -> Bool = n % 2 == 0

fn multiple(n: Int, of: Int) -> Bool = n % of == 0

struct Root {
  id:    Str where matches(r"[a-z]+")
  short: Str where 1..=8
  slots: Int where even && multiple(3) = 6
  ratio: Float where 0.0..1.0 = 0.5
  tags:  List<Str> where ..=3 = []
}

id    = "abc1"
short = ""
slots = 4
ratio = 1
tags  = ["a", "b"]
```

<!-- run: eval refinements.stav -->

```text
error[E0803]: value violates a refinement
   ┌─ refinements.stav:13:9
   │
 6 │   id:    Str where matches(r"[a-z]+")
   │          ---------------------------- constraint declared here
   ·
13 │ id    = "abc1"
   │         ^^^^^^
   │         │
   │         "abc1" does not match `[a-z]+`

error[E0803]: value violates a refinement
   ┌─ refinements.stav:14:9
   │
 7 │   short: Str where 1..=8
   │          --------------- constraint declared here
   ·
14 │ short = ""
   │         ^^
   │         │
   │         the length 0 is not in 1..=8

error[E0803]: value violates a refinement
   ┌─ refinements.stav:15:9
   │
 8 │   slots: Int where even && multiple(3) = 6
   │          ----------------------------- constraint declared here
   ·
15 │ slots = 4
   │         ^
   │         │
   │         `multiple` is false for 4

error[E0803]: value violates a refinement
   ┌─ refinements.stav:16:9
   │
 9 │   ratio: Float where 0.0..1.0 = 0.5
   │          -------------------- constraint declared here
   ·
16 │ ratio = 1
   │         ^
   │         │
   │         1.0 is not in 0.0..1.0
```

## Symbol types

A host program that embeds Stav can register symbol types: domain values, such as package references, that Stav carries without understanding them. A symbol type has a name, a constructor builtin that converts a `Str`, and a check that the host runs on every literal. Where a symbol type is expected, an identifier is a literal of it, unless a binding of that name has a type that can be accepted there ([name resolution](#name-resolution)). Symbol words such as `name@1.2:static` and string literals are literals of it when the host allows them.

A symbol value is its type and its canonical text. It renders as that text and interpolates as that text. Stav does not check that a symbol names anything real. The host receives every symbol of the final value, with the place that wrote it and the path of the value that holds it, and validates them itself.

The `stav` command registers no symbol types. See [Embedding in Rust](/guide/embedding) for how a host declares them.
