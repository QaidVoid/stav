# Totality and limits

Evaluation of a Stav program that type checks always terminates. No program can loop forever, recurse without end or wait on anything outside itself. This page lists the rules that make this true, explains why they are enough, and describes the limits that stop programs which would terminate, but only after doing far too much work.

## The rules

The checker enforces each of these rules before anything is evaluated, except the last check of `self` dependencies, which runs once the merge has picked its winners.

| Rule | Violation |
|---|---|
| Functions cannot call themselves, directly or through other functions. | [`E0610`](/reference/error-codes#e0610) |
| A `let` cannot use itself or a binding that comes after it. | [`E0305`](/reference/error-codes#e0305) |
| Types cannot contain themselves. | [`E0410`](/reference/error-codes#e0410) |
| Loops and comprehensions run only over collections that already exist. | not expressible |
| Imports and part files cannot form a cycle. | [`E0703`](/reference/error-codes#e0703) |
| `self` dependencies cannot form a cycle. | [`E0607`](/reference/error-codes#e0607) |

### No recursion

The call graph has an edge from a function to every named function whose name appears in its body, whether it is called there or passed on as a value. The graph must be acyclic, across modules too.

<!-- file: recursion.stav -->

```stav
struct Root { result: Int = fact(5) }

fn fact(n: Int) -> Int = if n == 0 { 1 } else { n * fact(n - 1) }
```

<!-- run: check recursion.stav -->

```text
error[E0610]: `fact` calls itself
  ┌─ recursion.stav:3:53
  │
3 │ fn fact(n: Int) -> Int = if n == 0 { 1 } else { n * fact(n - 1) }
  │                                                     ^^^^
  │
  = note: cycle: `fact` -> `fact`
  = note: evaluation must terminate, so functions cannot recurse
  = help: iterate with `fold` over `range(...)`, for example `fold(range(1, n + 1), 1, fn(acc: Int, i: Int) = acc * i)`
```

Functions that call each other in a cycle are rejected the same way, and the note lists the whole cycle:

<!-- file: mutual.stav -->

```stav
struct Root { even: Bool = is_even(4) }

fn is_even(n: Int) -> Bool = if n == 0 { true } else { is_odd(n - 1) }

fn is_odd(n: Int) -> Bool = if n == 0 { false } else { is_even(n - 1) }
```

<!-- run: check mutual.stav -->

```text
error[E0610]: functions may not call each other in a cycle
  ┌─ mutual.stav:3:56
  │
3 │ fn is_even(n: Int) -> Bool = if n == 0 { true } else { is_odd(n - 1) }
  │                                                        ^^^^^^
  │
  = note: cycle: `is_even` -> `is_odd` -> `is_even`
  = note: evaluation must terminate, so functions cannot recurse
  = help: iterate with `fold` over `range(...)`, for example `fold(range(1, n + 1), 1, fn(acc: Int, i: Int) = acc * i)`
```

A lambda has no name, so it cannot refer to itself either. Builtins are not values and cannot be used to build a loop.

### No recursive bindings

A `let` sees only the bindings before it. A binding that uses itself, or one declared later, is rejected:

<!-- file: letrec.stav -->

```stav
struct Root { n: Int = 0 }

let a = b + 1
let b = a + 1

n = a
```

<!-- run: check letrec.stav -->

```text
error[E0305]: `b` is used in its own definition or before it
  ┌─ letrec.stav:3:9
  │
3 │ let a = b + 1
  │         ^
  │
  = help: a `let` can only use the declarations before it
```

### No recursive types

A struct or sum type cannot contain itself, directly or through `List`, `Set`, `Map`, `Option` or the payload of a variant. Every value is therefore a finite tree whose depth the types bound. A configuration that needs a graph refers to other values by label with `Ref<T>`, which [types](/reference/types) describes.

<!-- file: rectype.stav -->

```stav
struct Node {
  name:     Str
  children: List<Node> = []
}

struct Root { tree: Option<Node> }
```

<!-- run: check rectype.stav -->

```text
error[E0410]: `Node` contains itself
  ┌─ rectype.stav:3:3
  │
3 │   children: List<Node> = []
  │   ^^^^^^^^
  │
  = note: cycle: Node -> Node
  = help: values must be finite trees, so refer to other values by label with `Ref<T>`
```

### Finite iteration

`for` statements, comprehensions and the builtins `map`, `filter`, `fold`, `any`, `all`, `mapValues` and `filterMap` all iterate over a collection that has already been computed. `range(start, end)` builds a finite list from two integers. There is no loop whose end depends on a condition, so every iteration has a known number of steps before it starts.

### Acyclic files

Imports and part files form a graph that must be acyclic. A file that includes itself, directly or through other files, is rejected:

<!-- file: includes.stav -->

```stav
struct Root { tags: List<Str> = [] }

include "./a.stav"
```

<!-- file: a.stav -->

```stav
include "./b.stav"
```

<!-- file: b.stav -->

```stav
include "./a.stav"
```

<!-- run: check includes.stav -->

```text
error[E0703]: imports and part files form a cycle
  ┌─ b.stav:1:1
  │
1 │ include "./a.stav"
  │ ^^^^^^^^^^^^^^^^^^
  │
  = note: cycle: a.stav -> b.stav -> a.stav
```

### Acyclic `self`

An attribute computed from `self` is evaluated after the attributes it reads. Two attributes that read each other would leave both undefined. The checker rejects a cycle within one body as soon as it sees it, and the evaluator checks the winning contributions of every block instance again after merging, before it evaluates any of them. [Late-bound self](/guide/self) explains both checks.

<!-- file: selfcycle.stav -->

```stav
struct Package {
  version: Str
  tag:     Str
}

struct Root { packages: Map<Str, Package> = {} block package }

package codec {
  version = "#{self.tag}"
  tag     = "v#{self.version}"
}
```

<!-- run: check selfcycle.stav -->

```text
error[E0607]: `self` dependencies form a cycle
   ┌─ selfcycle.stav:9:13
   │
 9 │   version = "#{self.tag}"
   │             ^^^^^^^^^^^^^
   │             │
   │             `Package.version` reads `Package.tag`
10 │   tag     = "v#{self.version}"
   │             ------------------ `Package.tag` reads `Package.version`
   │
   = note: cycle: Package.version -> Package.tag -> Package.version
```

## Why evaluation terminates

Take any program that passes these checks. Because the call graph is acyclic, every call to a named function can be replaced by the body of that function, and the replacing stops after finitely many steps. What remains is one large expression built from values, operators, lambdas, conditionals and iteration over finite collections.

A lambda could only call itself if it received itself as an argument, and the type of such a parameter would have to contain itself. Types cannot, so no lambda can reach itself. The only repetition left is iteration, which runs a fixed number of times over collections that already exist. Each expression thus reaches a value in a finite number of steps.

Statements add a finite amount of work each. A `for` runs once per element. Imports and includes unfold finitely, because their graphs are acyclic. Merging visits each contribution once, and each deferred `self` value is evaluated once, in an order that exists because the `self` graph is acyclic. The whole evaluation therefore ends.

## Bounded iteration

Where a recursive definition would be natural, a Stav program iterates instead. `fold` over a `range` covers counting loops, and comprehensions cover the rest:

<!-- file: bounded.stav -->

```stav
struct Root {
  factorial: Int = factorial(10)
  ports:     List<Int> = ports(8000, 4)
  triangle:  List<Int> = []
}

fn factorial(n: Int where 0..=20) -> Int =
  fold(range(1, n + 1), 1, fn(acc: Int, i: Int) = acc * i)

fn ports(base: Int, count: Int where 1..=100) -> List<Int> =
  [for i in range(0, count) -> base + i]

triangle = [for n in range(1, 6) -> fold(range(1, n + 1), 0, fn(acc: Int, i: Int) = acc + i)]
```

<!-- run: eval -o toml bounded.stav -->

```toml
factorial = 3628800
ports = [8000, 8001, 8002, 8003]
triangle = [1, 3, 6, 10, 15]
```

The refinement `0..=20` on `factorial` keeps the result inside the range of `Int`. Past `20!`, the multiplication would overflow, which is [`E0601`](/reference/error-codes#e0601).

## Limits

Terminating is not the same as finishing quickly. Nested folds can build a string that doubles in length at every step, and a program can be total and still need more memory than any machine has. Evaluation therefore runs within limits, and crossing one stops it with an error that points at the expression responsible.

| Limit | Default | Error |
|---|---|---|
| evaluation steps | 50 000 000 | [`E0611`](/reference/error-codes#e0611) |
| total size of the strings and collections built | 512 MiB | [`E0612`](/reference/error-codes#e0612) |
| length of one string | 16 MiB | [`E0612`](/reference/error-codes#e0612) |
| number of elements of one list, set or map | 10 000 000 | [`E0612`](/reference/error-codes#e0612) |

The size limit counts every value evaluation builds, intermediate ones included, so it bounds the memory that live values can take.

Evaluation counts one step for:

- every expression and every statement it evaluates,
- every element of every collection it builds, and every 64 bytes of the strings and collections it builds,
- every key it checks for duplicates in a map literal or a map comprehension,
- every entry of the `Data` maps it merges,
- every byte of every regular expression it compiles, and every 64 bytes of every string matched against one.

Two more bounds keep evaluation from exhausting the stack. Expressions, calls and the computing of merged attributes may nest at most 384 levels deep, beyond which evaluation stops with `E0611`. Values may nest at most 128 levels deep, beyond which it stops with `E0612`. Typed values never come close, so these bounds only matter for `Data` values built in loops.

### Step limit

The `stav` command takes the step limit from `--fuel`. A program that embeds Stav sets every limit through `Limits`, as [embedding in Rust](/guide/embedding) shows.

<!-- file: steps.stav -->

```stav
struct Root { total: Int = 0 }

total = fold(range(0, 1000), 0, fn(acc: Int, i: Int) = acc + i)
```

<!-- run: eval --fuel 1000 steps.stav -->

```text
error[E0611]: evaluation needs more than 1000 steps
  ┌─ steps.stav:3:14
  │
3 │ total = fold(range(0, 1000), 0, fn(acc: Int, i: Int) = acc + i)
  │              ^^^^^^^^^^^^^^
  │
  = help: make the computation smaller, or raise the step limit
```

With the default limit, the same program evaluates at once:

<!-- run: eval steps.stav -->

```json
{
  "total": 499500
}
```

### Size limits

This fold doubles a string forty times. It would end, but with a string of about a thousand billion bytes. The string limit stops it within milliseconds:

<!-- file: doubling.stav -->

```stav
struct Root { text: Str = "" }

text = fold(range(0, 40), "x", fn(s: Str, i: Int) = s ++ s)
```

<!-- run: eval doubling.stav -->

```text
error[E0612]: a string is longer than 16777216 bytes
  ┌─ doubling.stav:3:53
  │
3 │ text = fold(range(0, 40), "x", fn(s: Str, i: Int) = s ++ s)
  │        ---------------------------------------------^^^^^^-
  │        │                                             
  │        in this call of `fn`
  │
  = help: make the value smaller, or raise the size limits
```

## Purity and determinism

Evaluation reads nothing except the source files the host serves. It has no access to environment variables, the clock, the network, randomness or any other file. Path literals name files, but Stav never reads them.

The same files therefore always give the same value, byte for byte. Maps and sets are kept in canonical order, contributions are ordered by their place in the source, and nothing depends on hashing or memory addresses. The order of diagnostics is fixed in the same way. Canonical JSON, printed by `stav eval --canonical`, is a stable form of a value to compare or hash. See [the stav command](/reference/cli).
