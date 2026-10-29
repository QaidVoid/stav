# Builtins

Stav has 45 built-in functions. They are in scope in every file without an import, and they are the only generic functions in the language: the types of their earlier arguments decide what the later ones are checked against. Every builtin is total. Where a result may not exist, such as the first element of an empty list, the builtin returns an `Option` instead of failing.

## Calling builtins

Builtins take positional arguments only. A named argument or a wrong number of arguments is [`E0444`](/reference/error-codes#e0444).

<!-- file: named.stav -->

```stav
struct Root { text: Str = join(["a", "b"], sep = ",") }
```

<!-- run: check named.stav -->

```text
error[E0444]: `join` takes positional arguments only
  ┌─ named.stav:1:44
  │
1 │ struct Root { text: Str = join(["a", "b"], sep = ",") }
  │                                            ^^^^^^^^^
```

Builtins are not values. They cannot be stored in a `let`, passed as an argument or returned. Wrap one in a lambda to pass it on:

<!-- file: lambda.stav -->

```stav
struct Root { texts: List<Str> = map([1, 2], fn(n: Int) = toStr(n)) }
```

<!-- run: eval -o toml lambda.stav -->

```toml
texts = ["1", "2"]
```

Writing `map([1, 2], toStr)` instead is [`E0302`](/reference/error-codes#e0302), because `toStr` is not a name in scope.

The signatures below use type parameters such as `A` and `K` to show how the types of the arguments and the result relate. They are notation only. User functions cannot declare type parameters.

## Priority markers and failure

| Signature | Description |
|---|---|
| `default<T>(x: T) -> T` | Gives an attribute statement the `default` priority. |
| `force<T>(x: T) -> T` | Gives an attribute statement the `force` priority. |
| `fail<T>(message: Str) -> T` | Stops evaluation with [`E0608`](/reference/error-codes#e0608) and the message. |

`default` and `force` are valid only as the whole right side of an attribute statement, as in `port = default(8080)`. Anywhere else they are [`E0441`](/reference/error-codes#e0441). [Merging rules](/reference/merging) explains what the priorities do.

`fail` has the type its position expects, so it fits in any branch of an `if` or a `match`. Where nothing is expected, its type cannot be inferred ([`E0430`](/reference/error-codes#e0430)). Evaluation stops at the first `fail` it reaches, and the diagnostic shows the calls that led there.

<!-- file: fail.stav -->

```stav
type Mode = dev | prod

struct Root {
  mode: Mode = prod
  port: Int = port_for(self.mode)
}

fn port_for(mode: Mode) -> Int = match mode {
  dev  => 8080
  prod => fail("production needs an explicit port")
}
```

<!-- run: eval fail.stav -->

```text
error[E0608]: evaluation failed: production needs an explicit port
   ┌─ fail.stav:10:11
   │
 5 │   port: Int = port_for(self.mode)
   │               ------------------- in this call of `port_for`
   ·
10 │   prod => fail("production needs an explicit port")
   │           ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
```

## Conversion

| Signature | Description |
|---|---|
| `toStr<T>(x: T) -> Str` | The text of `x`, the same text that `#{x}` interpolates. `T` is `Str`, `Int`, `Float`, `Bool`, a sum type without payloads, a symbol type or `Ref<U>`. |
| `toFloat(x: Int) -> Float` | The nearest `Float`. |
| `toInt(x: Float) -> Int` | `x` truncated toward zero. A result outside the range of `Int` is [`E0601`](/reference/error-codes#e0601). |
| `parseInt(s: Str) -> Option<Int>` | The decimal integer in `s`, or `none`. |

`toStr` writes integers in decimal, floats in the shortest form that reads back as the same number, booleans as `true` or `false`, variants by name, symbols by their canonical text and references by their label. Any other type is [`E0421`](/reference/error-codes#e0421).

`parseInt` accepts ASCII digits with an optional leading `-` and nothing else: no `+`, no spaces, no separators and no other bases. A number outside the range of `Int` gives `none`.

<!-- file: conversion.stav -->

```stav
type Level = debug | info | warn

struct Root { results: Map<Str, Data> = {} }

results {
  toStr    = [toStr(42), toStr(1.5), toStr(true), toStr(Level.warn), toStr(1e-7)]
  toFloat  = toFloat(3)
  toInt    = [toInt(2.7), toInt(-2.7)]
  parseInt = [
    parseInt("42"),
    parseInt("-7"),
    parseInt("+7"),
    parseInt(" 1"),
    parseInt("99999999999999999999"),
  ]
}
```

<!-- run: eval -o yaml conversion.stav -->

```yaml
results:
  parseInt:
    - 42
    - -7
    - null
    - null
    - null
  toFloat: 3.0
  toInt:
    - 2
    - -2
  toStr:
    - "42"
    - "1.5"
    - "true"
    - warn
    - "1e-7"
```

## Strings

| Signature | Description |
|---|---|
| `len(s: Str) -> Int` | The number of Unicode scalar values. Also counts the elements of a list, set or map. |
| `nonEmpty(s: Str) -> Bool` | Whether `len(s)` is at least 1. Also takes a list, set or map. |
| `join(xs: List<Str>, sep: Str) -> Str` | The elements of `xs` with `sep` between them. |
| `split(s: Str, sep: Str where nonEmpty) -> List<Str>` | The pieces of `s` between occurrences of `sep`, empty pieces included. |
| `replace(s: Str, from: Str where nonEmpty, to: Str) -> Str` | `s` with every occurrence of `from` replaced by `to`. |
| `trim(s: Str) -> Str` | `s` without ASCII whitespace at either end. |
| `lower(s: Str) -> Str` | `s` with ASCII letters in lowercase. Other characters are kept. |
| `upper(s: Str) -> Str` | `s` with ASCII letters in uppercase. Other characters are kept. |
| `startsWith(s: Str, prefix: Str) -> Bool` | Whether `s` starts with `prefix`. |
| `endsWith(s: Str, suffix: Str) -> Bool` | Whether `s` ends with `suffix`. |
| `contains(s: Str, needle: Str) -> Bool` | Whether `needle` occurs in `s`. Also tests membership in a list or a set. |
| `matches(s: Str, pattern: Str) -> Bool` | Whether the regular expression `pattern` matches the whole of `s`. |
| `lines(s: Str) -> List<Str>` | The lines of `s`, split at each LF. A final empty line is dropped. |
| `indent(s: Str, n: Int where 0..=64) -> Str` | `s` with `n` spaces before every line that is not empty. |

An empty `sep` for `split`, an empty `from` for `replace` and an `n` outside `0..=64` for `indent` stop evaluation with [`E0803`](/reference/error-codes#e0803), like any other argument that violates the refinement of its parameter:

<!-- file: split.stav -->

```stav
struct Root { parts: List<Str> = [] }

let sep = ""

parts = split("a,b", sep)
```

<!-- run: eval split.stav -->

```text
error[E0803]: value violates a refinement
  ┌─ split.stav:5:22
  │
5 │ parts = split("a,b", sep)
  │                      ^^^
  │                      │
  │                      the string is empty
  │
  = help: the argument `sep` of `split` must not be empty
```

`matches` tests the whole string, as if the pattern were anchored at both ends, so `matches("abc", r"b")` is `false`. Write `r".*b.*"` to search anywhere. Patterns use a syntax with linear time matching: no backreferences and no lookaround. The compiled form of a pattern may take at most 1 MiB. A pattern written as a literal is checked before evaluation and an invalid one is [`E0415`](/reference/error-codes#e0415). A pattern computed during evaluation is checked when it is used, and an invalid one is [`E0609`](/reference/error-codes#e0609):

<!-- file: regex.stav -->

```stav
struct Root { ok: Bool = false }

let pattern = "[a-z"

ok = matches("abc", pattern)
```

<!-- run: eval regex.stav -->

```text
error[E0609]: `[a-z` is not a valid regular expression
  ┌─ regex.stav:5:21
  │
5 │ ok = matches("abc", pattern)
  │                     ^^^^^^^
```

`nonEmpty` and `matches` also work as refinement predicates in a type, as in `Str where nonEmpty` and `Str where matches(r"[a-z]+")`. See [types](/reference/types).

<!-- file: strings.stav -->

```stav
struct Root { results: Map<Str, Data> = {} }

results {
  len        = len("h\u{e9}llo")
  join       = join(["a", "b", "c"], "-")
  split      = split("a,b,,c", ",")
  replace    = replace("a.b.c", ".", "::")
  trim       = trim("  padded\t")
  lower      = lower("MiXed")
  upper      = upper("MiXed")
  startsWith = startsWith("prefix", "pre")
  endsWith   = endsWith("suffix", "fix")
  contains   = contains("haystack", "st")
  matches    = [matches("abc123", r"[a-z]+\d+"), matches("abc", r"b")]
  lines      = lines("x\ny\n")
  indent     = indent("one\n\ntwo\n", 2)
  nonEmpty   = [nonEmpty(""), nonEmpty("x")]
}
```

<!-- run: eval -o toml strings.stav -->

```toml
[results]
contains = true
endsWith = true
indent = "  one\n\n  two\n"
join = "a-b-c"
len = 5
lines = ["x", "y"]
lower = "mixed"
matches = [true, false]
nonEmpty = [false, true]
replace = "a::b::c"
split = ["a", "b", "", "c"]
startsWith = true
trim = "padded"
upper = "MIXED"
```

## Lists and sets

| Signature | Description |
|---|---|
| `map<A, B>(xs: List<A>, f: Fn(A) -> B) -> List<B>` | `f` applied to every element, in order. |
| `filter<A>(xs: List<A>, f: Fn(A) -> Bool) -> List<A>` | The elements for which `f` returns `true`, in order. |
| `fold<A, B>(xs: List<A>, init: B, f: Fn(B, A) -> B) -> B` | A left fold: `f` is called once per element, with the result so far and the element. |
| `flatten<A>(xss: List<List<A>>) -> List<A>` | The inner lists concatenated. |
| `sort<A>(xs: List<A>) -> List<A>` | The elements in ascending order. The sort is stable. |
| `unique<A>(xs: List<A>) -> List<A>` | The first occurrence of every element, in order. |
| `reverse<A>(xs: List<A>) -> List<A>` | The elements in reverse order. |
| `range(start: Int, end: Int) -> List<Int>` | The integers from `start` up to `end`, without `end`. Empty when `end <= start`. |
| `any<A>(xs: List<A>, f: Fn(A) -> Bool) -> Bool` | Whether `f` returns `true` for some element. Stops at the first one. |
| `all<A>(xs: List<A>, f: Fn(A) -> Bool) -> Bool` | Whether `f` returns `true` for every element. Stops at the first `false`. |
| `first<A>(xs: List<A>) -> Option<A>` | The first element, or `none` for an empty list. |
| `last<A>(xs: List<A>) -> Option<A>` | The last element, or `none` for an empty list. |
| `get<A>(xs: List<A>, i: Int) -> Option<A>` | The element at index `i`, counting from 0, or `none` when there is none. Also looks up a key in a map. |
| `len<A>(xs: List<A>) -> Int` | The number of elements. Also takes a set or a map. |
| `contains<A>(xs: List<A>, x: A) -> Bool` | Whether `x` is an element. Also takes a set. |
| `toList<A>(s: Set<A>) -> List<A>` | The elements of the set in canonical order. |
| `toSet<A>(xs: List<A>) -> Set<A>` | The elements as a set, without duplicates. |

The higher order builtins `map`, `filter`, `fold`, `any` and `all` take lists. Turn a set into a list with `toList` first. The function argument is usually a lambda, whose parameters must be annotated: `fn(x: Int) = x * 2`.

`sort` needs elements that can be ordered: `Int`, `Float`, `Str` (compared byte by byte) or a sum type without payloads (in declaration order). `unique` and `toSet` need a key type: `Bool`, `Int`, `Str`, `Path`, a sum type without payloads, a symbol type or `Ref<T>`. Anything else is [`E0401`](/reference/error-codes#e0401).

The first argument of a builtin has no expected type, so a list of variants needs a qualified first element, as in `sort([Level.warn, debug])`.

<!-- file: lists.stav -->

```stav
struct Root { results: Map<Str, Data> = {} }

let xs = [3, 1, 4, 1, 5]
let empty: List<Int> = []
let total = fold(xs, 0, fn(acc: Int, x: Int) = acc + x)

results {
  map      = map(xs, fn(x: Int) = x * 10)
  filter   = filter(xs, fn(x: Int) = x % 2 == 1)
  fold     = total
  flatten  = flatten([[1], [], [2, 3]])
  sort     = sort(xs)
  unique   = unique(xs)
  reverse  = reverse(xs)
  range    = [range(2, 5), range(5, 2)]
  any      = any(xs, fn(x: Int) = x > 4)
  all      = all(xs, fn(x: Int) = x > 4)
  first    = first(xs) ?? -1
  last     = last(empty) ?? -1
  get      = [get(xs, 2) ?? -1, get(xs, 9) ?? -1]
  len      = len(xs)
  contains = contains(xs, 4)
  toSet    = toSet(["b", "a", "b"])
  toList   = toList(toSet(["b", "a"]))
}
```

<!-- run: eval -o toml lists.stav -->

```toml
[results]
all = false
any = true
contains = true
filter = [3, 1, 1, 5]
first = 3
flatten = [1, 2, 3]
fold = 14
get = [4, -1]
last = -1
len = 5
map = [30, 10, 40, 10, 50]
range = [[2, 3, 4], []]
reverse = [5, 1, 4, 1, 3]
sort = [1, 1, 3, 4, 5]
toList = ["a", "b"]
toSet = ["a", "b"]
unique = [3, 1, 4, 5]
```

## Maps

| Signature | Description |
|---|---|
| `keys<K, V>(m: Map<K, V>) -> List<K>` | The keys in canonical order. |
| `values<K, V>(m: Map<K, V>) -> List<V>` | The values in the order of their keys. |
| `hasKey<K, V>(m: Map<K, V>, k: K) -> Bool` | Whether `m` has the key `k`. |
| `get<K, V>(m: Map<K, V>, k: K) -> Option<V>` | The value of `k`, or `none` when `m` has no such key. |
| `mapValues<K, V, W>(m: Map<K, V>, f: Fn(K, V) -> W) -> Map<K, W>` | The same keys, each with `f` applied to the key and its value. |
| `filterMap<K, V>(m: Map<K, V>, f: Fn(K, V) -> Bool) -> Map<K, V>` | The entries for which `f` returns `true`. |
| `union<K, V>(a: Map<K, V>, b: Map<K, V>) -> Map<K, V>` | Every entry of `a` and `b`. Where both have a key, the value of `b` wins. |

The canonical order of `Str` keys is byte order, and keys of a sum type follow the declaration order of its variants.

`union` is shallow and simple: it never looks inside the values and knows nothing of priorities. It is not the merge that combines contributions, which [merging rules](/reference/merging) describes.

<!-- file: maps.stav -->

```stav
struct Root { results: Map<Str, Data> = {} }

let m = { b = 2, a = 1, c = 3 }

results {
  keys      = keys(m)
  values    = values(m)
  hasKey    = [hasKey(m, "a"), hasKey(m, "z")]
  get       = get(m, "c") ?? 0
  len       = len(m)
  mapValues = mapValues(m, fn(k: Str, v: Int) = "#{k}=#{v}")
  filterMap = filterMap(m, fn(k: Str, v: Int) = v > 1)
  union     = union(m, { a = 10, d = 4 })
}
```

<!-- run: eval -o toml maps.stav -->

```toml
[results]
get = 3
hasKey = [true, false]
keys = ["a", "b", "c"]
len = 3
values = [1, 2, 3]

[results.filterMap]
b = 2
c = 3

[results.mapValues]
a = "a=1"
b = "b=2"
c = "c=3"

[results.union]
a = 10
b = 2
c = 3
d = 4
```

## Numbers

| Signature | Description |
|---|---|
| `min(a: Int, b: Int) -> Int` | The smaller of `a` and `b`. Also takes two `Float` values. |
| `max(a: Int, b: Int) -> Int` | The larger of `a` and `b`. Also takes two `Float` values. |
| `abs(x: Int) -> Int` | The absolute value of `x`. |

`min` and `max` take two numbers of the same type. The smallest `Int`, `-9223372036854775808`, has no positive counterpart, so `abs` of it is [`E0601`](/reference/error-codes#e0601). Arithmetic itself is described in [expressions and statements](/reference/expressions).

<!-- file: numbers.stav -->

```stav
struct Root { results: Map<Str, Data> = {} }

results {
  min = [min(3, 8), min(-1, 1)]
  max = [max(3, 8), max(1.5, 0.5)]
  abs = [abs(-4), abs(4)]
}
```

<!-- run: eval -o toml numbers.stav -->

```toml
[results]
abs = [4, 4]
max = [8, 1.5]
min = [3, -1]
```
