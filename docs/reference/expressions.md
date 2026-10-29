# Expressions and statements

An expression computes a value. A statement contributes values to the attributes of a struct. This page describes both, starting with the operators and ending with the rules that map a block to the attribute it fills.

## Operators

Binary operators, from the loosest to the tightest binding:

| Precedence | Operators | Associativity |
|---|---|---|
| 1 | `??` | right |
| 2 | `\|\|` | left |
| 3 | `&&` | left |
| 4 | `==` `!=` `<` `<=` `>` `>=` | none |
| 5 | `+` `-` `++` | left |
| 6 | `*` `/` `%` | left |

The prefix operators `!` and `-` bind tighter than every binary operator, and the postfix forms `.field`, `?.field`, `[index]` and calls bind tightest of all.

| Operator | Operands | Result |
|---|---|---|
| `+ - * /` | two `Int` or two `Float` | arithmetic. `Int` division truncates toward zero. Division by zero is [E0603](/reference/error-codes#e0603), and `Int` overflow is [E0601](/reference/error-codes#e0601). |
| `%` | two `Int` | the remainder, with the sign of the dividend |
| `++` | two `Str` or two `List<T>` | concatenation |
| `==` `!=` | two values of one type, other than functions and parts | structural equality |
| `<` `<=` `>` `>=` | two `Int`, two `Float`, two `Str` compared byte by byte, or two variants of a sum type without payloads, in declaration order | `Bool` |
| `&&` `\|\|` | two `Bool` | short-circuit logic |
| `!` | `Bool` | negation |
| unary `-` | `Int` or `Float` | negation |
| `a.f` | a struct, or an import alias | the field, or the declaration of the module |
| `a?.f` | an `Option` of a struct | the field wrapped in an `Option`, or `none` when `a` is `none` |
| `a ?? b` | `Option<T>` and `T` | the value inside `a`, or `b` when `a` is `none` |
| `a[i]` | `List<T>` and `Int`, or `Map<K, V>` and `K` | the element. An index out of range or a missing key is [E0604](/reference/error-codes#e0604). Use [`get`](/reference/builtins) for an `Option` instead. |

Mixing `Int` and `Float` in arithmetic is [E0431](/reference/error-codes#e0431). Convert with `toFloat` or `toInt` first. Comparisons cannot be chained: `a < b < c` is [E0207](/reference/error-codes#e0207), and `a < b && b < c` says what it means.

<!-- file: operators.stav -->

```stav
type Level = debug | info | warn | error

struct Point {
  x: Int = 0
  y: Int = 0
}

struct Root {
  ints:   List<Int> = []
  floats: List<Float> = []
  bools:  List<Bool> = []
  text:   Str = ""
  items:  List<Str> = []
  picked: List<Int> = []
}

let maybe: Option<Point> = none
let origin = Point {}

ints   = [1 + 2 * 3, (1 + 2) * 3, 7 / 2, -7 / 2, 7 % 3, -7 % 3, -(-5)]
floats = [1.5 + 2.0, 1.0 / 4.0, toFloat(7) / 2.0]
bools  = [
  "apple" < "banana",
  Level.info < Level.warn,
  [1, 2] == [1, 2],
  origin == Point { x = 0 },
  !(1 == 1) || true && false,
]
text   = "con" ++ "cat"
items  = ["a"] ++ ["b", "c"]
picked = [maybe?.x ?? 7, origin.y, [10, 20, 30][1], { "a" = 4 }["a"]]
```

<!-- run: eval operators.stav -->

```json
{
  "ints": [
    7,
    9,
    3,
    -3,
    1,
    -1,
    5
  ],
  "floats": [
    3.5,
    0.25,
    3.5
  ],
  "bools": [
    true,
    true,
    true,
    true,
    false
  ],
  "text": "concat",
  "items": [
    "a",
    "b",
    "c"
  ],
  "picked": [
    7,
    0,
    20,
    4
  ]
}
```

`if`, `match`, `let`, `fn` and `part` expressions cannot be the operand of an operator without parentheses ([E0210](/reference/error-codes#e0210)). Write `1 + (if c { 1 } else { 2 })`.

## Literals

| Literal | Example | Type |
|---|---|---|
| boolean | `true` | `Bool` |
| integer | `42`, `0x1f` | `Int` |
| float | `0.5`, `1e-3` | `Float` |
| string | `"text #{x}"`, `"""..."""`, `r"\d+"` | `Str` |
| path | `./fix.patch` | `Path` |
| absence | `none` | an `Option` that the context decides |
| list | `[1, 2, 3]` | `List<T>`, or `Set<T>` where a set is expected |
| map | `{ a = 1, "b c" = 2 }` | `Map<K, V>` |
| struct | `Point { x = 1 }`, or `{ x = 1 }` where a `Point` is expected | the struct |

The lexical forms of each literal are on the [lexical structure](/reference/lexical-structure) page. Entries of lists, maps and struct literals are separated by commas or line breaks. Giving the same map key or struct field twice in one literal is [E0306](/reference/error-codes#e0306).

## Calls

A call names a function, a builtin or a variant with payload. Arguments are positional, named or both, and named arguments come after positional ones ([E0203](/reference/error-codes#e0203)). An argument given twice is [E0204](/reference/error-codes#e0204). Parameters with defaults may be left out. A call through a function value, such as a parameter of type `Fn(Int) -> Int`, passes every argument positionally.

```stav
let a = local(9000)
let b = local(port = 9000)
let c = Ready.check(["probe"], timeout = 5)
```

Functions cannot call themselves, directly or through other functions. The [totality](/reference/totality) page explains why and what to write instead.

## Conditionals

`if` in an expression needs an `else`, and every branch has the same type. Branches are written in braces, and `else if` chains further conditions.

```stav
let port = if tls { 443 } else { 80 }
let size = if n > 1000 { "large" } else if n > 10 { "medium" } else { "small" }
```

## Match

`match` inspects a value and evaluates the first arm whose pattern fits it. An arm is a pattern, an optional guard `if condition`, `=>` and an expression.

<!-- file: match.stav -->

```stav
type Ready =
  | immediate
  | notify(fd: Int where 0..=1023)
  | check(cmd: List<Str>, interval: Int = 1, timeout: Int = 60)

struct Root { described: List<Str> = [] }

fn describe(r: Ready) -> Str = match r {
  immediate => "ready at start"
  notify(fd = 0 | 1 | 2) => "notifies on a standard stream"
  notify(fd) => "notifies on fd #{fd}"
  check(cmd, interval = i, ..) if i > 10 => "slow check: #{join(cmd, " ")}"
  check(cmd, ..) => "checks with #{join(cmd, " ")}"
}

fn sign(n: Int) -> Str = match n {
  0       => "zero"
  -1 | -2 => "small negative"
  _       => if n > 0 { "positive" } else { "negative" }
}

fn port(p: Option<Int>) -> Str = match p {
  none     => "no port"
  80 | 443 => "a web port"
  other    => "port #{other}"
}

fn greet(name: Str) -> Str = match name {
  "root" => "careful"
  n      => "hello #{n}"
}

described = [
  describe(immediate),
  describe(notify(2)),
  describe(notify(fd = 7)),
  describe(check(cmd = ["probe", "--fast"], interval = 30)),
  describe(check(["probe"])),
  sign(0),
  sign(-2),
  sign(5),
  port(none),
  port(443),
  port(8080),
  greet("root"),
  greet("ada"),
]
```

<!-- run: eval match.stav -->

```json
{
  "described": [
    "ready at start",
    "notifies on a standard stream",
    "notifies on fd 7",
    "slow check: probe --fast",
    "checks with probe",
    "zero",
    "small negative",
    "positive",
    "no port",
    "a web port",
    "port 8080",
    "careful",
    "hello ada"
  ]
}
```

### Patterns

| Pattern | Matches |
|---|---|
| `_` | anything |
| `name` | anything, and binds it to `name`, unless the inspected type has a variant called `name` |
| `true`, `false`, `42`, `-1`, `"text"` | that literal |
| `none` | an absent `Option` |
| `variant` | a variant without payload |
| `variant(field, field = pattern, ..)` | a variant with payload |
| `p \| q` | either pattern |

- In a variant pattern, `field` binds the payload field to a name of the same name, and `field = pattern` matches the field against a nested pattern. `..` ignores the remaining fields and must come last. Without `..`, every payload field must be mentioned ([E0432](/reference/error-codes#e0432)).
- An identifier is a variant when the inspected type has a variant of that name, and a new binding otherwise. The qualified form `Ready.immediate` is always a variant.
- For an `Option<T>`, `none` matches absence, and every other pattern matches the value inside.
- Both sides of `p | q` must bind the same names with the same types ([E0433](/reference/error-codes#e0433)).

### Exhaustiveness

The arms without guards must cover every possible value. A sum type needs every variant, `Bool` needs both values, an `Option<T>` needs `none` and the cases of `T`, and `Int`, `Float`, `Str` and collections need a `_` or a binding arm. A missing case is [E0434](/reference/error-codes#e0434), and the diagnostic names what is missing. An arm that can never match because earlier arms cover it is warning [W0434](/reference/error-codes#w0434).

<!-- file: exhaustive.stav -->

```stav
type Level = debug | info | warn | error

struct Root { verbose: Bool = false }

fn verbose(l: Level) -> Bool = match l {
  debug | info => true
}
```

<!-- run: check exhaustive.stav -->

```text
error[E0434]: non-exhaustive `match` on `Level`
  ┌─ exhaustive.stav:5:32
  │
5 │ fn verbose(l: Level) -> Bool = match l {
  │                                ^^^^^^^
  │
  = note: missing: `warn`, `error`
  = help: add the missing arms or a `_ => ...` arm
```

## Let expressions

`let` binds names for one expression. Bindings are evaluated in order, and each one sees the bindings before it. A binding cannot refer to itself or to a later binding ([E0305](/reference/error-codes#e0305)).

<!-- file: let.stav -->

```stav
struct Root { url: Str = "" }

fn url(host: Str, tls: Bool) -> Str =
  let scheme = if tls { "https" } else { "http" },
      port   = if tls { 443 } else { 80 }
  in "#{scheme}://#{host}:#{port}"

url = url("example.org", true)
```

<!-- run: eval let.stav -->

```json
{
  "url": "https://example.org:443"
}
```

## Lambdas

`fn(parameters) -> Type = expression` is an anonymous function. Its parameters need types, and its result type may be left out, in which case it is inferred from the body. A lambda captures the bindings around it. Lambda parameters cannot have defaults ([E0444](/reference/error-codes#e0444)).

<!-- file: lambdas.stav -->

```stav
struct Root { scaled: List<Int> = [] }

let factor = 10
let scale = fn(x: Int) = x * factor

scaled = map([1, 2, 3], scale)
```

<!-- run: eval lambdas.stav -->

```json
{
  "scaled": [
    10,
    20,
    30
  ]
}
```

## Comprehensions

A list comprehension builds a list from a collection, and a map comprehension builds a map. An optional `if` keeps only some elements.

```stav
let doubled = [for x in xs if x > 0 -> x * 2]
let pairs = [for k, v in m -> "#{k}=#{v}"]
let homes = {for u in users -> "#{u.name}" = u.home}
```

- The source must be a `List`, a `Set` or a `Map`. A set is visited in its canonical order. A map binds a key and a value and is visited in key order.
- The key of a map comprehension is a string or a variant without payload. Producing one key twice is [E0605](/reference/error-codes#e0605).

<!-- file: comprehensions.stav -->

```stav
type Level = debug | info | warn

struct Root {
  squares: Map<Str, Int> = {}
  env:     List<Str> = []
  quotas:  Map<Level, Int> = {}
}

let vars = { HOME = "/home/ada", LANG = "C.UTF-8" }

squares = {for i in range(1, 5) if i != 2 -> "n#{i}" = i * i}
env     = [for k, v in vars -> "#{k}=#{v}"]
quotas  = {for l in [Level.debug, info, warn] -> l = len("#{l}")}
```

<!-- run: eval comprehensions.stav -->

```json
{
  "squares": {
    "n1": 1,
    "n3": 9,
    "n4": 16
  },
  "env": [
    "HOME=/home/ada",
    "LANG=C.UTF-8"
  ],
  "quotas": {
    "debug": 5,
    "info": 4,
    "warn": 4
  }
}
```

## Part expressions

`part { statements }` is a value of type `Part<S>`, a fragment of a body of the struct `S` that can be included elsewhere. `S` comes from the expected type, so a part needs an annotation or a declared result type ([E0435](/reference/error-codes#e0435)). Parts are described in [Structuring configuration](/guide/structuring).

```stav
fn web(name: Str) -> Part<Root> = part {
  server "#{name}" { replicas = default(2) }
}
```

## Statements

A body is a sequence of statements checked against a struct. The top level of an entry file is a body of the root struct, the inside of a block is a body of the struct the block fills, and a part is a body of its struct.

| Statement | Effect |
|---|---|
| `a.b.c = e` | contributes `e` to the attribute at that path |
| `kind label { ... }` | a block, which contributes its statements to the attribute it maps to |
| `let x = e` | binds `x` for the rest of the body and every body inside it |
| `include e` | contributes the statements of a part here, unless the same part is already included into the same block |
| `if c { ... } else { ... }` | contributes the statements of the branch that `c` selects |
| `for x in e { ... }` | contributes its statements once for every element of `e` |

Statements run eagerly and in order. They do not assign: every statement adds contributions, and the contributions to one attribute are combined afterwards by the [merging rules](/reference/merging). The priority markers `default(...)` and `force(...)` are only valid as the whole right side of an attribute statement ([E0441](/reference/error-codes#e0441)).

### Attribute paths

An attribute path walks through struct fields, through `Option` fields, which a contribution makes present, and through the keys of `Map<Str, V>` fields. A key that is not an identifier is quoted. A path through any other type is [E0436](/reference/error-codes#e0436).

### Control statements

`if` and `for` work on statements the same way their expression forms work on values. A `for` over a map binds the key and the value. The conditions of `if`, the collections of `for` and the labels of blocks are evaluated immediately and cannot read `self` ([E0606](/reference/error-codes#e0606)), so which contributions exist never depends on merged values.

<!-- file: statements.stav -->

```stav
type Level = debug | info | warn

type Fetch =
  | git(url: Str, rev: Str = "main")
  | archive(url: Str)

struct Limit { max: Int }

struct Tls {
  cert: Str
  port: Int = 443
}

struct Root {
  limits: Map<Level, Limit> = {} block limit
  source: Fetch block from
  tls:    Option<Tls>
  sysctl: Map<Str, Int> = {}
  hosts:  List<Str> = []
}

limit warn { max = 3 }
from git { url = "https://example.org/repo.git" }
tls.cert               = "/etc/tls/cert.pem"
sysctl."vm.swappiness" = 10

let domain = "example.org"

for name, port in { db = 5432, cache = 6379 } {
  hosts = ["#{name}.#{domain}:#{port}"]
}
if len(domain) > 20 {
  hosts = ["long"]
} else if len(domain) > 5 {
  hosts = ["medium"]
}
```

<!-- run: eval statements.stav -->

```json
{
  "limits": {
    "warn": {
      "max": 3
    }
  },
  "source": {
    "git": {
      "url": "https://example.org/repo.git",
      "rev": "main"
    }
  },
  "tls": {
    "cert": "/etc/tls/cert.pem",
    "port": 443
  },
  "sysctl": {
    "vm.swappiness": 10
  },
  "hosts": [
    "cache.example.org:6379",
    "db.example.org:5432",
    "medium"
  ]
}
```

## Blocks

A block statement `kind label { body }` or `kind { body }` in a body of the struct `S` fills one attribute of `S`. The first of these rules that applies decides which:

1. **Block clause.** `S` has a field `f: Map<K, T> block kind`. The block needs a label ([E0437](/reference/error-codes#e0437)), it fills the entry `f.<label>`, and its body is a body of `T`. When `K` is a sum type, the label must be one of its variants.
2. **Struct field.** `S` has a field named `kind` whose type is a struct, or an `Option` of a struct. The block takes no label ([E0438](/reference/error-codes#e0438)) and its body is a body of that struct.
3. **Map field.** `S` has a field named `kind` of type `Map<K, V>`. The block takes no label ([E0438](/reference/error-codes#e0438)), and the attributes of its body are entries of the map, as in `env { RUST_LOG = "info" }`.
4. **Tagged block.** `S` has a field named `kind`, or a field with `block kind`, whose type is a sum type. The label must name a variant of that type ([E0439](/reference/error-codes#e0439)), and the attributes of the body are the payload fields of that variant, as in `build native { features = ["simd"] }`.
5. Otherwise the block is [E0440](/reference/error-codes#e0440), with a suggestion for the closest block kind.

A `block` clause belongs on a map of structs or on a sum type ([E0443](/reference/error-codes#e0443)), and one struct cannot declare the same block kind twice ([E0306](/reference/error-codes#e0306)). A label is an identifier or a string literal. A string label may interpolate, but not read `self`.

A tagged block selects its variant and contributes each payload field separately, so several blocks with the same label merge field by field. How selections and payloads merge is described on the [merging](/reference/merging) page.

<!-- file: blocks.stav -->

```stav
type Build =
  | native(features: List<Str> = [])
  | script(target: Str = "all")

struct Gc { keep: Int where 2.. = 10 }

struct Step { run: Str }

struct Job {
  build: Build
  gc:    Gc = {}
  env:   Map<Str, Str> = {}
  steps: Map<Str, Step> = {} block step
}

struct Root { job: Job }

job {
  build native { features = ["simd"] }
  gc { keep = 5 }
  env { RUST_LOG = "info" }
  step lint { run = "lint --strict" }
  step test { run = "test" }
}
```

<!-- run: eval -o toml blocks.stav -->

```toml
[job.build.native]
features = ["simd"]

[job.gc]
keep = 5

[job.env]
RUST_LOG = "info"

[job.steps.lint]
run = "lint --strict"

[job.steps.test]
run = "test"
```

Each rule has its own error when a block does not fit:

<!-- file: bad-blocks.stav -->

```stav
struct Gc { keep: Int = 10 }

struct Step { run: Str }

struct Root {
  gc:    Gc = {}
  steps: Map<Str, Step> = {} block step
}

gc main { keep = 1 }
step { run = "x" }
stpe x { run = "y" }
```

<!-- run: check bad-blocks.stav -->

```text
error[E0438]: the `gc` block takes no label
   ┌─ bad-blocks.stav:10:4
   │
10 │ gc main { keep = 1 }
   │    ^^^^
   │
   = help: write `gc { ... }`

error[E0437]: the `step` block needs a label
   ┌─ bad-blocks.stav:11:1
   │
11 │ step { run = "x" }
   │ ^^^^
   │
   = help: write `step <label> { ... }`

error[E0440]: `Root` has no block `stpe`
   ┌─ bad-blocks.stav:12:1
   │
12 │ stpe x { run = "y" }
   │ ^^^^
   │
   = help: did you mean `step`?
```

## Where self may appear

`self` is the final, merged value of an enclosing block. It may appear on the right side of attribute statements, in `let` statements inside bodies and in the defaults of struct fields. It cannot appear in a function body, in a lambda, in an `if` condition, in a `for` collection, in a block label or in an `include` expression ([E0606](/reference/error-codes#e0606)). [Late-bound self](/guide/self) explains how `self` is resolved.

<!-- file: self.stav -->

```stav
struct Root {
  name: Str = "web"
  url:  Str = ""
}

fn host() -> Str = self.name

url = "https://#{self.name}.example.org"
if self.name == "web" {
  url = "https://www.example.org"
}
```

<!-- run: check self.stav -->

```text
error[E0606]: `self` cannot be used in a function body
  ┌─ self.stav:6:20
  │
6 │ fn host() -> Str = self.name
  │                    ^^^^^^^^^
  │
  = help: pass the value as a parameter instead

error[E0606]: `self` cannot be used in an `if` condition
  ┌─ self.stav:9:4
  │
9 │ if self.name == "web" {
  │    ^^^^^^^^^
  │
  = help: pass the value as a parameter instead
```
