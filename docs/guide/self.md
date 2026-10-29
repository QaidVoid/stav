# Late-bound `self`

Stav evaluates statements eagerly and in order, with one exception. `self` is the **final, merged value** of an enclosing block. A right-hand side that reads `self` is not evaluated where it is written. It is kept aside and evaluated during merging, once the attributes it reads have their final values.

That lets one attribute be derived from another, even when the other one is set or overridden somewhere else.

## Deriving one attribute from another

<!-- file: tools.stav -->

```stav
struct Tool {
  version: Str
  url:     Str = "https://example.org/releases/tool-#{self.version}.tar.gz"
}

struct Root {
  tools: Map<Str, Tool> = {} block tool
}

tool query {
  version = "1.7.1"
}

include "./pin.stav"
```

<!-- file: pin.stav -->

```stav
tool query {
  version = force("1.8.0")
}
```

<!-- run: eval tools.stav -->

```json
{
  "tools": {
    "query": {
      "version": "1.8.0",
      "url": "https://example.org/releases/tool-1.8.0.tar.gz"
    }
  }
}
```

The default of `url` reads `self.version`. Because it is read after merging, it sees the forced `1.8.0` from `pin.stav`, not the `1.7.1` written next to it. A field default that reads `self` is the most common use: the schema describes how one value follows from another, and every instance gets it.

## Which block `self` means

`self.f` refers to the nearest enclosing block whose struct declares a field `f`. The search starts at the innermost block and moves outward. Inside a field default, it starts at the struct being defaulted.

<!-- file: outward.stav -->

```stav
struct Source {
  url:    Str
  sha256: Str
}

struct Package {
  version: Str
  source:  Source
}

struct Root {
  packages: Map<Str, Package> = {} block package
}

package codec {
  version = "2.4"
  source {
    url    = "https://example.org/codec-#{self.version}.tar.gz"
    sha256 = "6b1d2f0c"
  }
}
```

<!-- run: eval -o toml outward.stav -->

```toml
[packages.codec]
version = "2.4"

[packages.codec.source]
url = "https://example.org/codec-2.4.tar.gz"
sha256 = "6b1d2f0c"
```

`Source` has no field `version`, so `self.version` inside `source { }` resolves to the enclosing `Package`. When no enclosing block declares the field, the program does not check:

<!-- file: unknown.stav -->

```stav
struct Root {
  name:  Str = "inventory"
  title: Str = ""
}

title = "#{self.label}"
```

<!-- run: check unknown.stav -->

```text
error[E0303]: no enclosing block has a field `label`
  ┌─ unknown.stav:6:17
  │
6 │ title = "#{self.label}"
  │                 ^^^^^
```

Inside a part or a part file, the search stops at the struct of the part. A part cannot reach the blocks around the place where it is included.

## `let` bindings that read `self`

A `let` statement in a body may read `self` as well. Every right-hand side that uses the binding is then late too:

<!-- file: lets.stav -->

```stav
struct Root {
  release: Str = "stable"
  channel: Str = ""
  banner:  Str = ""
}

let channel = "#{self.release}-channel"

channel = channel
banner  = "built for #{channel}"

release = "testing"
```

<!-- run: eval lets.stav -->

```json
{
  "release": "testing",
  "channel": "testing-channel",
  "banner": "built for testing-channel"
}
```

## Where `self` is allowed

`self` may appear in the right-hand side of an attribute, in a `let` statement inside a body, and in a field default. It may not appear where Stav needs a value before merging starts:

- the condition of an `if` statement and the collection of a `for` statement,
- block labels and `include` expressions,
- the body of a function or a lambda.

These decide which contributions exist, or they would let a function depend on where it is called. Using `self` there is an error:

<!-- file: structural.stav -->

```stav
struct Root {
  debug: Bool = false
  level: Str = "info"
}

if self.debug {
  level = "debug"
}
```

<!-- run: check structural.stav -->

```text
error[E0606]: `self` cannot be used in an `if` condition
  ┌─ structural.stav:6:4
  │
6 │ if self.debug {
  │    ^^^^^^^^^^
  │
  = help: pass the value as a parameter instead
```

## Cycles

A late value can read another late value, but the reads must not go around in a circle, or neither value could ever be computed. Stav checks this twice. While type checking, it rejects a cycle that exists inside one body whatever else is merged into it:

<!-- file: cycle.stav -->

```stav
struct Root {
  version: Str = ""
  tag:     Str = ""
}

version = "#{self.tag}"
tag     = "v#{self.version}"
```

<!-- run: check cycle.stav -->

```text
error[E0607]: `self` dependencies form a cycle
  ┌─ cycle.stav:6:11
  │
6 │ version = "#{self.tag}"
  │           ^^^^^^^^^^^^^
  │           │
  │           `Root.version` reads `Root.tag`
7 │ tag     = "v#{self.version}"
  │           ------------------ `Root.tag` reads `Root.version`
  │
  = note: cycle: Root.version -> Root.tag -> Root.version
```

After merging, it checks again, precisely. The final check looks at block instances, not at types, and only at the contributions that won the merge. Two instances of the same struct may therefore derive their fields in opposite directions:

<!-- file: instances.stav -->

```stav
struct Package {
  version: Str = ""
  tag:     Str = ""
}

struct Root {
  packages: Map<Str, Package> = {} block package
}

package a {
  tag     = "v1.0"
  version = "#{self.tag}-release"
}

package b {
  version = "2.0"
  tag     = "v#{self.version}"
}
```

<!-- run: eval -o toml instances.stav -->

```toml
[packages.a]
version = "v1.0-release"
tag = "v1.0"

[packages.b]
version = "2.0"
tag = "v2.0"
```

A contribution that loses the merge adds no dependency either. When an overlay sets `url = force("...")` to a constant, the default `url` that read `self.version` is discarded, and so is its dependency on `version`.

## How it fits with eager evaluation

Late binding is narrow on purpose. Everything that decides the shape of the value, such as conditions, loops, labels and includes, is evaluated eagerly and cannot read `self`. So the set of contributions is fixed before merging, and only the values of some attributes wait for it. Each late value is evaluated once, after the values it reads, along a dependency graph that is known to be acyclic. Evaluation still always terminates.

## Next

[Imports and modules](./modules) shows how to share declarations between files.
