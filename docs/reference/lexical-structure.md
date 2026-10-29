# Lexical structure

This page describes how Stav source text is split into tokens: the encoding of a file, comments, names, numbers, strings and paths, and where a line break ends a statement.

## Source files

A Stav source file has the extension `.stav` and is UTF-8 text.

- A file must not start with a byte order mark. One is reported as [E0101](/reference/error-codes#e0101).
- Lines end with a line feed. A carriage return anywhere in a file, including inside a string, is [E0114](/reference/error-codes#e0114).
- A file that is not valid UTF-8 cannot be read at all. The `stav` command reports it as [E0701](/reference/error-codes#e0701).
- A source file may be at most 4 MiB long ([E0706](/reference/error-codes#e0706)).

The formatter, the checker and the evaluator all work on the same tokens, so a file that lexes cleanly behaves the same in every command.

## Whitespace

Spaces and horizontal tabs separate tokens. Any other Unicode whitespace character outside strings and comments is [E0102](/reference/error-codes#e0102), because it is invisible in most editors and almost always pasted by accident. A character that cannot start any token is [E0113](/reference/error-codes#e0113).

Line feeds are tokens of their own. They separate statements, list items, map entries and match arms. The rules that decide when a line break continues an expression are described under [line breaks](#line-breaks).

## Comments

A `#` outside a string starts a comment that runs to the end of the line.

```stav
# The port the server listens on.
let port = 8080 # Trailing comments are fine too.
```

A comment that starts with `##` followed by a space or the end of the line is a doc comment. Consecutive doc comment lines form one block, which documents the declaration, field or variant that follows it. Doc comments are Markdown. Tools such as a language server or a host program can show them next to the item they describe.

```stav
## A web upstream.
struct Upstream {
  ## The base URL, with its scheme.
  url:    Str
  ## The share of traffic, from 1 to 100.
  weight: Int where 1..=100 = 10
}
```

## Identifiers

An identifier names a binding, a field, a function, a variant or a block. It starts with an ASCII letter or `_`, and continues with letters, digits and `_`.

```text
IDENT       = ident_start { ident_char | "-" ident_char | "." digit { digit } }
ident_start = "a"..."z" | "A"..."Z" | "_"
ident_char  = ident_start | digit
digit       = "0"..."9"
```

Two more characters can appear inside an identifier:

- A `-` belongs to the identifier when a letter, a digit or `_` follows it directly. `xdg-utils` and `on-failure` are single identifiers.
- A `.` belongs to the identifier when a digit follows it directly. `openssl1.1` and `Apache-2.0` are single identifiers, while `a.b` is a field access.

Because of the first rule, a minus between two names needs spaces to be a subtraction. `a - 1` subtracts one from `a`, but `a-1` is the identifier `a-1`. The formatter always writes binary operators with spaces around them.

<!-- file: names.stav -->

```stav
struct Root { names: List<Str> = [] }

let xdg-utils = "kebab case"
let openssl1.1 = "a dotted version"
let a = 5
let b = a - 1

names = [xdg-utils, openssl1.1, "#{b}"]
```

<!-- run: eval names.stav -->

```json
{
  "names": [
    "kebab case",
    "a dotted version",
    "4"
  ]
}
```

Without the spaces, `a-1` is looked up as a name of its own:

<!-- file: minus.stav -->

```stav
struct Root { n: Int = 0 }

let a = 5

n = a-1
```

<!-- run: check minus.stav -->

```text
error[E0302]: unknown name `a-1`
  ┌─ minus.stav:5:5
  │
5 │ n = a-1
  │     ^^^
```

Identifiers are case sensitive and at most 255 bytes long ([E0104](/reference/error-codes#e0104)).

### Naming conventions

The names of structs and sum types must start with an uppercase letter ([E0205](/reference/error-codes#e0205)). Functions, fields, bindings and variants are usually written in `snake_case` or `kebab-case`. Variants may use any identifier, so that values such as `Apache-2.0` can be variants.

## Keywords

These words are reserved and cannot be used as identifiers:

```text
let fn struct type match if else for in import include as part self
true false none where block
```

A keyword cannot name a binding, a field, a variant or a bare label. To use a keyword as a map key or a block label, quote it:

<!-- file: keywords.stav -->

```stav
struct Service { port: Int = 0 }

struct Root {
  words:    Map<Str, Int> = {}
  services: Map<Str, Service> = {} block service
}

words       = { "in" = 1, "match" = 2, plain = 3 }
words."for" = 4

service "in" { port = 1 }
```

<!-- run: eval -o toml keywords.stav -->

```toml
[words]
for = 4
in = 1
match = 2
plain = 3

[services.in]
port = 1
```

The names of the built-in types are not keywords, but they are reserved. Declaring a struct or a sum type named `Bool`, `Int`, `Float`, `Str`, `Path`, `Data`, `List`, `Set`, `Map`, `Option`, `Ref`, `Part` or `Fn` is [E0301](/reference/error-codes#e0301).

## Symbol words

A symbol word is an identifier followed directly, without whitespace, by a version, a variant, or both. Symbol words exist so that a host program can offer compact literals for its own [symbol types](/reference/types#symbol-types), such as references to packages.

```text
SYMWORD = IDENT ( "@" verpart [ ":" varpart ] | ":" varpart )
verpart = verchar { verchar }
verchar = "a"..."z" | "A"..."Z" | digit | "." | "_" | "~"
varpart = "a"..."z" { "a"..."z" | digit | "_" }
```

| Source | Tokens |
|---|---|
| `llvm@19.1:static` | one symbol word with name `llvm`, version `19.1` and variant `static` |
| `ripgrep:static` | one symbol word with name `ripgrep` and variant `static` |
| `openssl1.1` | one identifier |
| `port: Int` | the identifier `port`, the punctuation `:` and the type name `Int` |

The `@` character appears nowhere else in Stav. An `@` next to whitespace is [E0105](/reference/error-codes#e0105):

<!-- file: at.stav -->

```stav
struct Root { tool: Str = "" }

tool = llvm @19
```

<!-- run: check at.stav -->

```text
error[E0105]: `@` is only allowed inside a symbol word
  ┌─ at.stav:3:13
  │
3 │ tool = llvm @19
  │             ^
  │
  = help: write the name and version without spaces, like `name@1.2`
```

A `:` that is followed directly by a lowercase letter makes a symbol word, so a field declaration needs a space after its colon. Writing `port:Int` is [E0206](/reference/error-codes#e0206):

<!-- file: colon.stav -->

```stav
struct Root {
  port:Int = 0
}
```

<!-- run: check colon.stav -->

```text
error[E0206]: expected `name: Type`, found `port:Int` without a space
  ┌─ colon.stav:2:3
  │
2 │   port:Int = 0
  │   ^^^^^^^^
  │
  = help: write `port: Int` with a space after `:`
```

A symbol word is valid only where the expected type is a symbol type whose host accepts symbol words. The `stav` command declares no symbol types, so there every symbol word is [E0420](/reference/error-codes#e0420):

<!-- file: symword.stav -->

```stav
struct Root { tool: Str = "" }

tool = llvm@19
```

<!-- run: check symword.stav -->

```text
error[E0420]: the symbol word `llvm@19` is only valid where a symbol type is expected
  ┌─ symword.stav:3:8
  │
3 │ tool = llvm@19
  │        ^^^^^^^
```

## Numbers

```text
INT   = dec | "0x" hex { hex | "_" } | "0o" oct { oct | "_" } | "0b" bin { bin | "_" }
dec   = digit { digit | "_" }
FLOAT = dec "." dec [ exp ] | dec exp
exp   = ( "e" | "E" ) [ "+" | "-" ] dec
```

- `Int` values are signed 64-bit integers. A literal that does not fit is [E0107](/reference/error-codes#e0107).
- `Float` values are finite IEEE 754 binary64 numbers. A float literal too large to be finite is also [E0107](/reference/error-codes#e0107).
- `_` separates digit groups and is ignored.
- There are no negative literals. `-1` is the unary minus applied to `1`.
- A number followed directly by letters, as in `0ad`, is [E0103](/reference/error-codes#e0103).

<!-- file: numbers.stav -->

```stav
struct Root {
  ints:   List<Int> = []
  floats: List<Float> = []
}

ints   = [42, 1_000_000, 0x1f, 0o755, 0b1010, -7]
floats = [0.5, 2_500.25, 1.0e-3, 2e3]
```

<!-- run: eval numbers.stav -->

```json
{
  "ints": [
    42,
    1000000,
    31,
    493,
    10,
    -7
  ],
  "floats": [
    0.5,
    2500.25,
    0.001,
    2000.0
  ]
}
```

## Strings

Stav has three forms of string literals: single-line strings, multi-line strings and raw strings. All of them produce values of type `Str`.

### Single-line strings

A single-line string is delimited by `"` and must end on the line it starts on ([E0108](/reference/error-codes#e0108)). It understands these escapes:

| Escape | Meaning |
|---|---|
| `\\` | a backslash |
| `\"` | a double quote |
| `\n`, `\t`, `\r`, `\0` | a line feed, a tab, a carriage return, a NUL character |
| `\#` | a literal `#`, so `\#{` is a literal `#{` |
| `\u{X}` | the Unicode scalar value with 1 to 6 hexadecimal digits `X` |

Any other escape is [E0109](/reference/error-codes#e0109). A string interpolates every `#{expression}` it contains, as described under [interpolation](#interpolation). A `$` has no meaning, so `${HOME}` passes through unchanged.

### Multi-line strings

A multi-line string holds file contents and scripts. Apart from interpolation, its content is taken verbatim.

- The opening `"""` must be followed directly by a line break ([E0110](/reference/error-codes#e0110)).
- The closing `"""` must be the first text on its line ([E0111](/reference/error-codes#e0111)). The whitespace before it is the indentation prefix.
- Every content line must start with exactly the indentation prefix, byte for byte, or consist only of whitespace ([E0112](/reference/error-codes#e0112)).
- The value is formed by removing the prefix from every line, turning lines of only whitespace into empty lines, joining the lines with line feeds and adding one final line feed. A string without content lines is empty.
- There are no backslash escapes. `\n`, a trailing `\` and `$` are kept as written. The one exception is `\#{`, which writes a literal `#{`.
- The content cannot contain `"""`.

In this example the closing delimiter is indented by four spaces, so four spaces are removed from every line:

<!-- file: multi.stav -->

```stav
struct Root { text: Str = "" }

text = """
    line one
      indented

    line three
    """
```

<!-- run: eval multi.stav -->

```json
{
  "text": "line one\n  indented\n\nline three\n"
}
```

### Raw strings

`r"..."` is a raw single-line string, and `r"""..."""` is a raw multi-line string with the same layout rules as other multi-line strings. Raw strings have no escapes and no interpolation, which makes them the natural form for regular expressions. A raw single-line string cannot contain `"`.

The three forms side by side:

<!-- file: strings.stav -->

```stav
struct Root {
  text:   Str = ""
  script: Str = ""
  re:     Str = ""
  raw:    Str = ""
}

let user = "deploy"

text   = "tab\there, quote \", backslash \\, hash \#{x}, e-acute \u{e9}"
script = """
  printf '%s\n' "${HOME}"
  echo "#{user}"
  echo \#{literal}
  """
re     = r"\d+\.\d+ #{kept}"
raw    = r"""
  raw #{kept}
    \n stays
  """
```

<!-- run: eval strings.stav -->

```json
{
  "text": "tab\there, quote \", backslash \\, hash #{x}, e-acute é",
  "script": "printf '%s\\n' \"${HOME}\"\necho \"deploy\"\necho #{literal}\n",
  "re": "\\d+\\.\\d+ #{kept}",
  "raw": "raw #{kept}\n  \\n stays\n"
}
```

### Interpolation

`#{expression}` inside a single-line or multi-line string evaluates the expression and inserts its text. The expression is type checked, and its type must be one of these:

| Type | Text |
|---|---|
| `Str` | the string itself |
| `Int` | the decimal number, with `-` for negative values |
| `Float` | the shortest text that reads back as the same number, such as `0.1` or `1e-7` |
| `Bool` | `true` or `false` |
| a variant without payload | the name of the variant |
| a symbol type | the canonical text of the symbol |
| `Ref<T>` | the label the reference names |

Any other type is [E0421](/reference/error-codes#e0421). To insert a list, join it into a string first with [`join`](/reference/builtins). Line breaks inside `#{ }` are ignored, so a long expression may span several lines.

<!-- file: interpolation.stav -->

```stav
type Level = debug | info | warn

struct Root {
  text: Str = ""
  bad:  Str = ""
}

let hosts = ["a", "b"]

text = "#{1} #{-2} #{2.5} #{0.1} #{1e-7} #{true} #{Level.warn} #{join(hosts, ",")}"
bad  = "#{hosts}"
```

<!-- run: check interpolation.stav -->

```text
error[E0421]: cannot interpolate a value of type `List<Str>`
   ┌─ interpolation.stav:11:11
   │
11 │ bad  = "#{hosts}"
   │           ^^^^^
   │
   = help: join the elements with `join(...)`
```

Without the last line, the program evaluates:

<!-- file: interpolation.stav -->

```stav
type Level = debug | info | warn

struct Root { text: Str = "" }

let hosts = ["a", "b"]

text = "#{1} #{-2} #{2.5} #{0.1} #{1e-7} #{true} #{Level.warn} #{join(hosts, ",")}"
```

<!-- run: eval interpolation.stav -->

```json
{
  "text": "1 -2 2.5 0.1 1e-7 true warn a,b"
}
```

## Path literals

```text
PATH     = ( "./" | "../" ) { pathchar }
pathchar = "a"..."z" | "A"..."Z" | digit | "." | "_" | "-" | "+" | "@" | "=" | "," | "/"
```

A path literal starts with `./` or `../` and produces a value of type `Path`.

- A path is relative to the directory of the file that contains it, and `.` and `..` are resolved lexically.
- The resolved path must stay inside the import root of the file ([E0702](/reference/error-codes#e0702)).
- There are no absolute path literals. An absolute path on another system is a string.
- Stav never reads the file a path names. A `Path` value is the import root joined with the normalized path, and the host decides what to do with it.

The `stav` command uses the directory of the entry file, as written on the command line, as the import root. With the entry file given as `paths.stav`, that directory is empty, so the paths render relative to it:

<!-- file: paths.stav -->

```stav
struct Root {
  patch: Path = ./default.patch
  files: List<Path> = []
}

files = [./fixes/one.patch, ./conf/../extra/./two.conf]
```

<!-- run: eval paths.stav -->

```json
{
  "patch": "default.patch",
  "files": [
    "fixes/one.patch",
    "extra/two.conf"
  ]
}
```

## Punctuation and operators

```text
punctuation  { } [ ] ( ) , = . ?. ?? : -> => .. ..= |
operators    + - * / % ++ == != < <= > >= && || !
```

The meaning and precedence of the operators are described in [Expressions and statements](/reference/expressions).

## Line breaks

A line break ends a statement, a list item, a map entry, a struct field or a match arm. A comma does the same, so `a = 1, b = 2` is two statements on one line. A line break is ignored, and the expression continues on the next line, in these cases:

1. Inside parentheses `( ... )` and inside an interpolation `#{ ... }`.
2. After a token that cannot end an expression: `{ [ ( , = . ?. ?? : -> => | + - * / % ++ == != < <= >= && || !` and the keywords `in`, `else`, `where`, `import`, `include` and `as`.
3. Before a token that cannot start a statement: `. ?. ?? && || ++ else | =>`.

A line that starts with `+`, `-` or `*` does not continue the previous line, so break long arithmetic after the operator instead of before it. Other text after a complete statement on the same line is [E0210](/reference/error-codes#e0210).

<!-- file: lines.stav -->

```stav
struct Root {
  total: Int = 0
  url:   Str = ""
  flags: List<Str> = []
}

let base = 1_000

total = base +
  250 +
  (5 * 3)
url = "https://example.org"
  ++ "/api"
  ++ "/v1"
flags = [
  "--verbose"
  "--color"
]
```

<!-- run: eval lines.stav -->

```json
{
  "total": 1265,
  "url": "https://example.org/api/v1",
  "flags": [
    "--verbose",
    "--color"
  ]
}
```
