# Error codes

Every diagnostic that Stav reports carries a stable code. An error code starts with `E` and a warning code with `W`. A code never changes its meaning once it has been released, and a retired code is never reused. This page lists every code, explains when it is reported, shows a small program that triggers it together with the real output of the `stav` command, and says how to fix it.

[Diagnostics](/reference/diagnostics) explains how to read the output itself.

The first digits of a code name the area it belongs to.

| Range | Area |
| --- | --- |
| `E01xx` | [Lexical structure](#lexical-errors) |
| `E02xx` | [Syntax](#syntax-errors) |
| `E03xx` | [Names and declarations](#names-and-declarations) |
| `E04xx` | [Types](#types) |
| `E05xx` | [Merging](#merging) |
| `E06xx` | [Evaluation, `self`, totality and limits](#evaluation) |
| `E07xx` | [Files, imports and roots](#files-imports-and-roots) |
| `E08xx` | [Checks on the final value and rendering](#final-value-and-rendering) |
| `W0xxx` | [Warnings](#warnings), numbered like the errors of the same area |

## Summary

| Code | Meaning |
| --- | --- |
| [E0101](#e0101) | invalid UTF-8 or a byte order mark |
| [E0102](#e0102) | a whitespace character other than space and tab |
| [E0103](#e0103) | a number directly followed by letters |
| [E0104](#e0104) | an identifier longer than 255 bytes |
| [E0105](#e0105) | `@` outside a symbol word |
| [E0107](#e0107) | a number out of range |
| [E0108](#e0108) | an unterminated string |
| [E0109](#e0109) | an unknown escape sequence |
| [E0110](#e0110) | an opening `"""` not followed by a line break |
| [E0111](#e0111) | a closing `"""` that is not the first text on its line |
| [E0112](#e0112) | a line of a multi-line string without the common indentation |
| [E0113](#e0113) | an unexpected character |
| [E0114](#e0114) | a carriage return |
| [E0201](#e0201) | a statement that is neither an attribute nor a block |
| [E0202](#e0202) | a block inside a literal |
| [E0203](#e0203) | a positional argument after a named one |
| [E0204](#e0204) | an argument given twice |
| [E0205](#e0205) | a type name that starts with a lowercase letter |
| [E0206](#e0206) | `name:Type` without a space after the colon |
| [E0207](#e0207) | chained comparisons |
| [E0208](#e0208) | a declaration or import below the top level |
| [E0209](#e0209) | a statement in a library |
| [E0210](#e0210) | a syntax error |
| [E0211](#e0211) | nesting deeper than the parser allows |
| [E0212](#e0212) | interpolation in an import path or an attribute key |
| [E0213](#e0213) | an import after another item |
| [E0214](#e0214) | a declaration in a part file |
| [E0301](#e0301) | a declaration named like a built-in type |
| [E0302](#e0302) | an unknown name |
| [E0303](#e0303) | `self` without a field that an enclosing block declares |
| [E0304](#e0304) | a declaration that the prelude already makes |
| [E0305](#e0305) | a binding used in its own definition or before it |
| [E0306](#e0306) | a name, field or key given more than once |
| [E0401](#e0401) | a type mismatch |
| [E0410](#e0410) | a recursive type |
| [E0411](#e0411) | a type that cannot be a map key or a set element |
| [E0412](#e0412) | a function or a part in a field |
| [E0413](#e0413) | a nested `Option` |
| [E0414](#e0414) | `Ref<T>` without exactly one target map |
| [E0415](#e0415) | an invalid regular expression literal |
| [E0420](#e0420) | a symbol word where no symbol type is expected |
| [E0421](#e0421) | a value that cannot be turned into a string |
| [E0430](#e0430) | a type that cannot be inferred |
| [E0431](#e0431) | `Int` and `Float` mixed in arithmetic |
| [E0432](#e0432) | a variant pattern that does not fit the variant |
| [E0433](#e0433) | alternatives that bind different names |
| [E0434](#e0434) | a `match` that is not exhaustive |
| [E0435](#e0435) | a `part` whose struct is unknown |
| [E0436](#e0436) | an attribute path through a value without attributes |
| [E0437](#e0437) | a block without the label it needs |
| [E0438](#e0438) | a label on a block that takes none |
| [E0439](#e0439) | a tagged block label that names no variant |
| [E0440](#e0440) | an unknown block kind |
| [E0441](#e0441) | `default` or `force` inside an expression |
| [E0442](#e0442) | a part included into a body of another struct |
| [E0443](#e0443) | a `block` clause on a field of the wrong type |
| [E0444](#e0444) | wrong arguments in a call or a type |
| [E0501](#e0501) | conflicting values at equal priority |
| [E0502](#e0502) | tagged blocks that select different variants |
| [E0503](#e0503) | `Data` values of different shapes at equal priority |
| [E0600](#e0600) | an internal error |
| [E0601](#e0601) | integer overflow |
| [E0602](#e0602) | a float that is not finite |
| [E0603](#e0603) | division by zero |
| [E0604](#e0604) | an index out of range, a missing key or an unknown variant |
| [E0605](#e0605) | a duplicate key produced by a comprehension |
| [E0606](#e0606) | `self` where it cannot be used |
| [E0607](#e0607) | a cycle of `self` dependencies |
| [E0608](#e0608) | `fail(...)` was called |
| [E0609](#e0609) | an invalid computed regular expression |
| [E0610](#e0610) | a recursive function |
| [E0611](#e0611) | the step limit or the nesting limit was exceeded |
| [E0612](#e0612) | a size limit was exceeded |
| [E0613](#e0613) | a symbol whose text the host rejects |
| [E0701](#e0701) | a file or module that does not exist or cannot be read |
| [E0702](#e0702) | a path that leaves its import root |
| [E0703](#e0703) | an import or include cycle |
| [E0704](#e0704) | an unsupported import or include path |
| [E0705](#e0705) | a path that is not a regular file |
| [E0706](#e0706) | a source file larger than 4 MiB |
| [E0707](#e0707) | an import in a prelude file |
| [E0708](#e0708) | an entry file listed twice |
| [E0710](#e0710) | no root struct |
| [E0801](#e0801) | a required field without a value |
| [E0802](#e0802) | a reference to a block that does not exist |
| [E0803](#e0803) | a value that violates a refinement |
| [E0804](#e0804) | a value that cannot be rendered in the requested format |
| [E0805](#e0805) | an attribute that does not exist or does not convert |
| [W0302](#w0302) | a binding that hides a variant |
| [W0434](#w0434) | a `match` arm that can never match |
| [W0703](#w0703) | a part file included twice into one body |

## Lexical errors

These are reported while a file is split into tokens. A file with a lexical error is not type checked.

### E0101

The file is not valid UTF-8, or it starts with a byte order mark. Stav source files are UTF-8 without a byte order mark.

To fix it, save the file as UTF-8 without a byte order mark.

### E0102

The file contains a whitespace character other than a space, a tab or a line feed, outside of strings and comments. The message names the character by its code point, for example `U+00A0` for a non-breaking space, which is easy to type by accident and invisible in most editors.

To fix it, replace the character with a plain space.

### E0103

A number is directly followed by letters, so the text is neither a number nor a name.

<!-- file: e0103.stav -->

```stav
struct Root { size: Int = 0 }

size = 10mb
```

<!-- run: check e0103.stav -->

```text
error[E0103]: `10mb` is not a valid token
  ┌─ e0103.stav:3:8
  │
3 │ size = 10mb
  │        ^^^^
  │
  = help: separate the number from the name, or start the name with a letter
```

Separate the number from the name, or start the name with a letter. Units are not part of the language, so write `size = 10` and name the field after its unit, as in `size_mb`.

### E0104

An identifier is longer than 255 bytes. The message says how long it is.

Shorten the identifier to at most 255 bytes.

### E0105

An `@` appears outside a symbol word. The only place `@` may appear is directly between a name and a version, without whitespace on either side, as in `llvm@19`.

<!-- file: e0105.stav -->

```stav
struct Root { tool: Str = "" }

tool = llvm @19
```

<!-- run: check e0105.stav -->

```text
error[E0105]: `@` is only allowed inside a symbol word
  ┌─ e0105.stav:3:13
  │
3 │ tool = llvm @19
  │             ^
  │
  = help: write the name and version without spaces, like `name@1.2`
```

Remove the whitespace. A symbol word is only accepted where the host expects a symbol type, see [E0420](#e0420).

### E0107

An integer literal does not fit in a signed 64-bit integer, or a float literal is too large to be finite.

<!-- file: e0107.stav -->

```stav
struct Root { big: Int = 0 }

big = 9223372036854775808
```

<!-- run: check e0107.stav -->

```text
error[E0107]: integer `9223372036854775808` does not fit in a signed 64-bit integer
  ┌─ e0107.stav:3:7
  │
3 │ big = 9223372036854775808
  │       ^^^^^^^^^^^^^^^^^^^
  │
  = help: the largest integer is 9223372036854775807
```

Use a smaller number. The largest `Int` is 9223372036854775807.

### E0108

A string is not closed before the end of its line. Single-line strings cannot contain a line break.

<!-- file: e0108.stav -->

```stav
struct Root { name: Str = "" }

name = "unfinished
```

<!-- run: check e0108.stav -->

```text
error[E0108]: unterminated string
  ┌─ e0108.stav:3:8
  │
3 │ name = "unfinished
  │        ^
  │
  = help: close the string with `"` before the end of the line
```

Close the string on the same line. For text that spans lines, use a multi-line string that starts with `"""`.

### E0109

A single-line string contains a backslash escape that Stav does not know. The escapes are `\\`, `\"`, `\n`, `\t`, `\r`, `\0`, `\#` and `\u{...}`.

<!-- file: e0109.stav -->

```stav
struct Root { pattern: Str = "" }

pattern = "\d+"
```

<!-- run: check e0109.stav -->

```text
error[E0109]: unknown escape `\d`
  ┌─ e0109.stav:3:12
  │
3 │ pattern = "\d+"
  │            ^^
  │
  = help: write `\\` for a literal backslash
```

Write `\\` for a literal backslash, or use a raw string such as `r"\d+"`, which has no escapes at all.

### E0110

The opening `"""` of a multi-line string is not followed directly by a line break. The same code is reported when a multi-line string starts inside a single-line string.

<!-- file: e0110.stav -->

```stav
struct Root { script: Str = "" }

script = """echo hi
  """
```

<!-- run: check e0110.stav -->

```text
error[E0110]: the opening `"""` must be followed directly by a line break
  ┌─ e0110.stav:3:10
  │
3 │ script = """echo hi
  │          ^^^^^^^^^^
  │
  = help: start the content of the string on the next line
```

Start the content of the string on the line after the opening `"""`.

### E0111

The closing `"""` of a multi-line string is not the first text on its line.

<!-- file: e0111.stav -->

```stav
struct Root { script: Str = "" }

script = """
  echo hi"""
```

<!-- run: check e0111.stav -->

```text
error[E0111]: the closing `"""` must be the first text on its line
  ┌─ e0111.stav:4:10
  │
4 │   echo hi"""
  │          ^^^
  │
  = help: move the closing `"""` to a line of its own
```

Put the closing `"""` on a line of its own. The whitespace before it sets the indentation that is removed from every line.

### E0112

A line of a multi-line string does not start with the indentation of the closing `"""`. Every line must start with exactly that whitespace, unless it is blank.

<!-- file: e0112.stav -->

```stav
struct Root { script: Str = "" }

script = """
  echo one
 echo two
  """
```

<!-- run: check e0112.stav -->

```text
error[E0112]: line does not start with the indentation of the closing `"""`
  ┌─ e0112.stav:5:1
  │
5 │  echo two
  │ ^^
  │
  = help: start every line of the string with 2 spaces
```

Indent every line at least as far as the closing `"""`, or move the closing `"""` further left.

### E0113

The file contains a character that starts no token, such as a single `&`. When a likely meaning exists, the help says what to write instead.

<!-- file: e0113.stav -->

```stav
struct Root { a: Int = 0 }

a = 1 & 2
```

<!-- run: check e0113.stav -->

```text
error[E0113]: unexpected character `&`
  ┌─ e0113.stav:3:7
  │
3 │ a = 1 & 2
  │       ^
  │
  = help: write `&&` for logical and
```

Remove the character or replace it as the help suggests.

### E0114

The file contains a carriage return. Stav files end their lines with a line feed only, so a carriage return is an error anywhere, even inside a string.

Save the file with LF line endings.

## Syntax errors

These are reported while a file is parsed. Parsing recovers after an error, so one run reports as many independent syntax errors as it can.

### E0201

A statement starts with a name that is followed by neither `=` nor `{`. A statement in a body either assigns an attribute or opens a block.

<!-- file: e0201.stav -->

```stav
struct Root { port: Int = 0 }

port 8080
```

<!-- run: check e0201.stav -->

```text
error[E0201]: expected `=` or `{` after `port`, found `8080`
  ┌─ e0201.stav:3:6
  │
3 │ port 8080
  │      ^^^^
```

Write `port = 8080` to set an attribute, or `name { ... }` to open a block. A block label must also be followed by `{`, and an attribute key by `=`.

### E0202

A block appears inside a list, map or struct literal. Blocks are statements, and literals contain values.

<!-- file: e0202.stav -->

```stav
struct Server { port: Int = 80 }

struct Root { servers: List<Server> = [] }

servers = [{ server { port = 1 } }]
```

<!-- run: check e0202.stav -->

```text
error[E0202]: a block cannot appear inside a literal
  ┌─ e0202.stav:5:14
  │
5 │ servers = [{ server { port = 1 } }]
  │              ^^^^^^
  │
  = help: write `key = { ... }` to set a nested value
```

Write the nested value as a literal, for example `servers = [{ port = 1 }]`.

### E0203

A positional argument follows a named argument.

<!-- file: e0203.stav -->

```stav
struct Root { a: Int = 0 }

fn add(x: Int, y: Int) -> Int = x + y

a = add(x = 1, 2)
```

<!-- run: check e0203.stav -->

```text
error[E0203]: positional argument after a named argument
  ┌─ e0203.stav:5:16
  │
5 │ a = add(x = 1, 2)
  │                ^
  │
  = help: move positional arguments before named ones
```

Put every positional argument before the named ones, as in `add(2, y = 1)`.

### E0204

A call names the same argument twice, or names an argument that a positional argument already gave.

<!-- file: e0204.stav -->

```stav
struct Root { a: Int = 0 }

fn add(x: Int, y: Int) -> Int = x + y

a = add(1, y = 2, y = 3)
```

<!-- run: check e0204.stav -->

```text
error[E0204]: argument `y` is given more than once
  ┌─ e0204.stav:5:19
  │
5 │ a = add(1, y = 2, y = 3)
  │                   ^
  │
  = help: remove one of the arguments
```

Give each argument once.

### E0205

The name of a `struct` or `type` declaration starts with a lowercase letter. Type names start with an uppercase letter, which is how Stav tells types from values.

<!-- file: e0205.stav -->

```stav
struct server { port: Int }

struct Root { a: Int = 0 }
```

<!-- run: check e0205.stav -->

```text
error[E0205]: type name `server` must start with an uppercase letter
  ┌─ e0205.stav:1:8
  │
1 │ struct server { port: Int }
  │        ^^^^^^
  │
  = help: rename it to `Server`
```

Rename the type as the help suggests.

### E0206

A field or parameter is written `name:Type` without a space. Without the space, `port:Int` reads as a single symbol word.

<!-- file: e0206.stav -->

```stav
struct Root {
  port:Int = 80
}
```

<!-- run: check e0206.stav -->

```text
error[E0206]: expected `name: Type`, found `port:Int` without a space
  ┌─ e0206.stav:2:3
  │
2 │   port:Int = 80
  │   ^^^^^^^^
  │
  = help: write `port: Int` with a space after `:`
```

Write `port: Int` with a space after the colon.

### E0207

Comparison operators are chained, as in `a < b < c`. A comparison gives a `Bool`, which cannot be compared again.

<!-- file: e0207.stav -->

```stav
struct Root { ok: Bool = false }

ok = 1 < 2 < 3
```

<!-- run: check e0207.stav -->

```text
error[E0207]: comparison operators cannot be chained
  ┌─ e0207.stav:3:12
  │
3 │ ok = 1 < 2 < 3
  │            ^
  │
  = help: join the comparisons with `&&`, as in `a < b && b < c`
```

Join the comparisons with `&&`, as in `1 < 2 && 2 < 3`.

### E0208

A declaration (`struct`, `type`, `fn`) or an `import` appears inside a body or a block. Declarations and imports are only allowed at the top level of a file.

<!-- file: e0208.stav -->

```stav
struct Root { a: Int = 0 }

fn f() -> Int = 1

if true {
  struct Inner { b: Int }
}
```

<!-- run: check e0208.stav -->

```text
error[E0208]: `struct` is only allowed at the top level of a file
  ┌─ e0208.stav:6:3
  │
6 │   struct Inner { b: Int }
  │   ^^^^^^
  │
  = help: move it to the top level of the file
```

Move the declaration to the top level of the file.

### E0209

A library, that is a file loaded through `import`, contains statements. A library holds declarations, top-level `let` bindings and imports. Only entry files and part files contribute values.

`e0209.stav`:

<!-- file: e0209.stav -->

```stav
import "./e0209-lib.stav" as lib

struct Root { port: Int = lib.port }
```

`e0209-lib.stav`:

<!-- file: e0209-lib.stav -->

```stav
let port = 80

port = 8080
```

<!-- run: check e0209.stav -->

```text
error[E0209]: a library cannot contain statements
  ┌─ e0209-lib.stav:3:1
  │
3 │ port = 8080
  │ ^^^^
  │
  = help: move the statement into an entry file or a part file
```

Move the statement into the entry file, or into a part file that the entry file includes.

### E0210

The parser found a token it did not expect. The message says what it expected and what it found. Common causes are a lowercase type name, an `if` expression without `else`, an unclosed bracket, a `..` that is not the last item of a variant pattern, and an `if`, `match`, `let` or `fn` expression used as an operand without parentheses.

<!-- file: e0210.stav -->

```stav
struct Root { port: int = 80 }
```

<!-- run: check e0210.stav -->

```text
error[E0210]: expected a type, found `int`
  ┌─ e0210.stav:1:21
  │
1 │ struct Root { port: int = 80 }
  │                     ^^^
  │
  = help: type names start with an uppercase letter
```

Fix the syntax as the message and the help describe.

### E0211

The source nests deeper than the parser allows, for example with hundreds of nested brackets. The limit protects the parser from running out of stack.

Split the expression into smaller parts with `let` bindings.

### E0212

An import path or an attribute key contains `#{}` interpolation. Both must be known before anything is evaluated, so they are plain strings.

<!-- file: e0212.stav -->

```stav
struct Root { env: Map<Str, Str> = {} }

let name = "HOME"

env."#{name}" = "/root"
```

<!-- run: check e0212.stav -->

```text
error[E0212]: an attribute key cannot contain interpolation
  ┌─ e0212.stav:5:6
  │
5 │ env."#{name}" = "/root"
  │      ^^^^^^^
  │
  = help: write the key as a plain string
```

Write the key as a plain string. To build a map with computed keys, use a map comprehension, or a block whose label is an interpolated string.

### E0213

An `import` comes after another item of the file.

`e0213.stav`:

<!-- file: e0213.stav -->

```stav
struct Root { a: Int = lib.a }

import "./e0213-lib.stav" as lib
```

`e0213-lib.stav`:

<!-- file: e0213-lib.stav -->

```stav
let a = 1
```

<!-- run: check e0213.stav -->

```text
error[E0213]: `import` must come before every other item
  ┌─ e0213.stav:3:1
  │
3 │ import "./e0213-lib.stav" as lib
  │ ^^^^^^
  │
  = help: move the import to the start of the file
```

Move every import to the start of the file.

### E0214

A part file declares a `struct`, `type` or `fn`. Part files contain statements, `let` bindings and imports, and are type checked against the body that includes them.

`e0214.stav`:

<!-- file: e0214.stav -->

```stav
struct Root { a: Int = 0 }

include "./e0214-part.stav"
```

`e0214-part.stav`:

<!-- file: e0214-part.stav -->

```stav
fn helper() -> Int = 1

a = helper()
```

<!-- run: check e0214.stav -->

```text
error[E0214]: a part file cannot contain a `fn` declaration
  ┌─ e0214-part.stav:1:1
  │
1 │ fn helper() -> Int = 1
  │ ^^
  │
  = help: move the declaration into a library and import it
```

Move the declaration into a library and import it in the part file.

## Names and declarations

### E0301

A `struct` or `type` declaration uses the name of a built-in type: `Bool`, `Int`, `Float`, `Str`, `Path`, `Data`, `List`, `Set`, `Map`, `Option`, `Ref`, `Part` or `Fn`.

<!-- file: e0301.stav -->

```stav
struct Path { dir: Str }

struct Root { a: Int = 0 }
```

<!-- run: check e0301.stav -->

```text
error[E0301]: `Path` is the name of a built-in type
  ┌─ e0301.stav:1:8
  │
1 │ struct Path { dir: Str }
  │        ^^^^
  │
  = help: choose another name
```

Choose another name.

### E0302

A name does not resolve. It is neither a binding in scope, nor a declaration, nor a variant of the expected sum type. The same code covers a missing field of a struct, a missing variant of a sum type, a missing parameter in a named argument, an unknown import alias and a missing member of an imported library. When a close name exists, the help suggests it.

<!-- file: e0302.stav -->

```stav
type Level = debug | info | warn | error

struct Root { level: Level = info }

level = debgu
```

<!-- run: check e0302.stav -->

```text
error[E0302]: unknown name `debgu`, which is not a variant of `Level`
  ┌─ e0302.stav:5:9
  │
5 │ level = debgu
  │         ^^^^^
  │
  = help: did you mean `debug`?
```

Fix the spelling, or declare the name.

### E0303

`self` is not followed by a field that an enclosing block declares. `self.f` looks for `f` in the struct of the innermost enclosing block, then in the blocks around it. A part stops the search at its own struct. The same code is reported for a bare `self` that is not followed by a field.

<!-- file: e0303.stav -->

```stav
struct Server { port: Int = 80, url: Str = "" }

struct Root { server: Server = {} }

server { url = "http://localhost:#{self.prt}" }
```

<!-- run: check e0303.stav -->

```text
error[E0303]: no enclosing block has a field `prt`
  ┌─ e0303.stav:5:41
  │
5 │ server { url = "http://localhost:#{self.prt}" }
  │                                         ^^^
  │
  = help: did you mean `port`?
```

Name a field that the block or an enclosing block declares. [Late-bound self](/guide/self) explains the search.

### E0304

A file declares a name that the host's prelude already declares. Prelude declarations are in scope in every file, so a second declaration with the same name would hide one of them. The `stav` command has no prelude, so only programs that a host checks can report this.

Rename the declaration.

### E0305

A `let` binding uses a binding that is defined after it, or itself. Bindings are evaluated in order, and each one only sees the bindings before it. The same code is reported when top-level bindings and functions depend on each other in a cycle.

<!-- file: e0305.stav -->

```stav
struct Root { a: Int = 0 }

let a = b + 1
let b = 2
```

<!-- run: check e0305.stav -->

```text
error[E0305]: `b` is used in its own definition or before it
  ┌─ e0305.stav:3:9
  │
3 │ let a = b + 1
  │         ^
  │
  = help: a `let` can only use the declarations before it
```

Move the binding that is used before the one that uses it.

### E0306

A name is declared twice in one place: two declarations with the same name in a file, two fields or two parameters with the same name, two `block` clauses with the same kind in one struct, the same key twice in a map or struct literal, or the same field twice in a variant pattern.

<!-- file: e0306.stav -->

```stav
struct Root {
  port: Int = 80
  port: Int = 443
}
```

<!-- run: check e0306.stav -->

```text
error[E0306]: `port` is declared more than once
  ┌─ e0306.stav:3:3
  │
2 │   port: Int = 80
  │   ---- first declared here
3 │   port: Int = 443
  │   ^^^^
  │
  = help: rename or remove one of the declarations
```

Rename or remove one of them.

## Types

### E0401

A value does not have the type its position requires. The message names the expected and the found type, or says which operation does not apply. It covers mismatched assignments and arguments, branches of different types, operators applied to the wrong operands, calls of something that is not a function, field access on a value without fields and builtins given the wrong kind of collection.

<!-- file: e0401.stav -->

```stav
struct Root { port: Int = 80 }

port = "8080"
```

<!-- run: check e0401.stav -->

```text
error[E0401]: expected `Int`, found `Str`
  ┌─ e0401.stav:3:8
  │
3 │ port = "8080"
  │        ^^^^^^
```

Give a value of the expected type. Stav never converts implicitly between `Int` and `Str`, so write `8080` here, or `parseInt(...)` and `toStr(...)` when a conversion is meant.

### E0410

A type contains itself, directly or through lists, sets, maps, options or variant payloads. Every value is a finite tree, so types cannot be recursive.

<!-- file: e0410.stav -->

```stav
struct Node { children: List<Node> = [] }

struct Root { tree: Node = {} }
```

<!-- run: check e0410.stav -->

```text
error[E0410]: `Node` contains itself
  ┌─ e0410.stav:1:15
  │
1 │ struct Node { children: List<Node> = [] }
  │               ^^^^^^^^
  │
  = note: cycle: Node -> Node
  = help: values must be finite trees, so refer to other values by label with `Ref<T>`
```

Refer to other values by label instead, with `Ref<T>` and a map of labeled blocks.

### E0411

A type is used as a map key or a set element although it cannot be one. Map keys are `Str` or a sum type whose variants have no payload. Set elements may also be `Bool`, `Int`, `Path`, symbol types and `Ref<T>`. `Float`, collections, structs and sum types with payloads cannot be keys.

<!-- file: e0411.stav -->

```stav
struct Root { weights: Map<Float, Str> = {} }
```

<!-- run: check e0411.stav -->

```text
error[E0411]: `Float` cannot be a map key
  ┌─ e0411.stav:1:28
  │
1 │ struct Root { weights: Map<Float, Str> = {} }
  │                            ^^^^^
  │
  = help: map keys are `Str` or a sum type whose variants have no fields
```

Use a key type, for example the text of the number as a `Str`.

### E0412

A struct field or a variant payload has a function type or a `Part` type, at any depth. Functions and parts are not data and cannot be rendered.

<!-- file: e0412.stav -->

```stav
struct Root { handler: Fn(Int) -> Int }
```

<!-- run: check e0412.stav -->

```text
error[E0412]: a field cannot hold `Fn(Int) -> Int`
  ┌─ e0412.stav:1:24
  │
1 │ struct Root { handler: Fn(Int) -> Int }
  │                        ^^^^^^^^^^^^^^
  │
  = help: functions and parts are not data, so pass them as parameters instead
```

Keep functions and parts in `let` bindings, and pass them to functions as parameters.

### E0413

An `Option` is nested in another `Option`.

<!-- file: e0413.stav -->

```stav
struct Root { name: Option<Option<Str>> }
```

<!-- run: check e0413.stav -->

```text
error[E0413]: options cannot be nested
  ┌─ e0413.stav:1:21
  │
1 │ struct Root { name: Option<Option<Str>> }
  │                     ^^^^^^^^^^^^^^^^^^^
  │
  = help: use a single `Option`
```

Use a single `Option`.

### E0414

`Ref<T>` is used, but the root has no unique target map for `T`. A reference names a labeled block, so the root must have exactly one field of type `Map<Str, T>` with a `block` clause, reachable from the root struct through struct fields. The message says whether there is none or more than one.

<!-- file: e0414.stav -->

```stav
struct Unit { exec: Str }

struct Root {
  first: Ref<Unit>
}
```

<!-- run: check e0414.stav -->

```text
error[E0414]: `Ref<Unit>` has no target map
  ┌─ e0414.stav:4:10
  │
4 │   first: Ref<Unit>
  │          ^^^^^^^^^
  │
  = help: declare exactly one field `Map<Str, Unit>` with a `block` clause reachable from the root struct
```

Declare one field such as `units: Map<Str, Unit> = {} block unit`.

### E0415

A regular expression literal is invalid, either in a `matches(...)` refinement or as the pattern of the `matches` builtin. Literal patterns are checked before evaluation. Patterns that compile to more than 1 MiB are rejected too.

<!-- file: e0415.stav -->

```stav
struct Root { name: Str where matches(r"[a-z") = "a" }
```

<!-- run: check e0415.stav -->

```text
error[E0415]: invalid regular expression: unclosed character class
  ┌─ e0415.stav:1:39
  │
1 │ struct Root { name: Str where matches(r"[a-z") = "a" }
  │                                       ^^^^^^^
```

Fix the pattern. Remember that `matches` checks the whole string, so anchors are not needed.

### E0420

A symbol word such as `ripgrep:static` or `llvm@19` appears where no symbol type is expected, or where the expected symbol type does not accept symbol words. Symbol types come from the host, and the `stav` command has none.

<!-- file: e0420.stav -->

```stav
struct Root { tool: Str = "" }

tool = ripgrep:static
```

<!-- run: check e0420.stav -->

```text
error[E0420]: the symbol word `ripgrep:static` is only valid where a symbol type is expected
  ┌─ e0420.stav:3:8
  │
3 │ tool = ripgrep:static
  │        ^^^^^^^^^^^^^^
```

Write a string instead, or use the symbol where the host expects its symbol type.

### E0421

A value is interpolated into a string or passed to `toStr`, but its type has no text form. Strings, numbers, booleans, variants without payload, symbols and references can be turned into text. Lists, maps, structs, options and variants with payload cannot.

<!-- file: e0421.stav -->

```stav
struct Root { line: Str = "" }

let ports = [80, 443]

line = "ports: #{ports}"
```

<!-- run: check e0421.stav -->

```text
error[E0421]: cannot interpolate a value of type `List<Int>`
  ┌─ e0421.stav:5:18
  │
5 │ line = "ports: #{ports}"
  │                  ^^^^^
  │
  = help: join the elements with `join(...)`
```

Turn the value into a string first, for example `join(map(ports, fn(p: Int) = toStr(p)), ", ")`.

### E0430

The type of an expression cannot be inferred, because nothing gives it one. This happens for an empty list `[]`, an empty map `{}`, `none` and `fail(...)` when no type is expected.

<!-- file: e0430.stav -->

```stav
struct Root { a: Int = 0 }

let names = []
```

<!-- run: check e0430.stav -->

```text
error[E0430]: cannot infer the type of `[]`
  ┌─ e0430.stav:3:13
  │
3 │ let names = []
  │             ^^
  │
  = help: add an annotation: `let xs: List<Str> = []`
```

Annotate the binding, as in `let names: List<Str> = []`.

### E0431

An arithmetic operator mixes an `Int` and a `Float`. Stav only converts integer literals to `Float`, never computed integers.

<!-- file: e0431.stav -->

```stav
struct Root { ratio: Float = 0.0 }

let count = 3

ratio = count * 0.5
```

<!-- run: check e0431.stav -->

```text
error[E0431]: cannot mix `Int` and `Float` in arithmetic
  ┌─ e0431.stav:5:15
  │
5 │ ratio = count * 0.5
  │               ^
  │
  = help: convert the `Int` with `toFloat(...)`
```

Convert the integer with `toFloat(count)`.

### E0432

A variant pattern does not fit its variant. Without `..`, a pattern for a variant with payload must mention every payload field. A pattern also cannot give fields to a variant without payload, or match a variant with payload by its bare name.

<!-- file: e0432.stav -->

```stav
type Ready = immediate | check(cmd: Str, interval: Int = 1)

struct Root { a: Int = 0 }

fn interval(r: Ready) -> Int = match r {
  immediate       => 0
  check(interval) => interval
}
```

<!-- run: check e0432.stav -->

```text
error[E0432]: the pattern does not mention the fields `cmd`
  ┌─ e0432.stav:7:3
  │
7 │   check(interval) => interval
  │   ^^^^^^^^^^^^^^^
  │
  = help: add `..` to ignore the other fields
```

Mention every field, or add `..` to ignore the rest, as in `check(interval, ..)`.

### E0433

The alternatives of an or-pattern bind different names, or the same name with different types. Whichever alternative matches, the arm must see the same bindings.

<!-- file: e0433.stav -->

```stav
type Shape = circle(r: Int) | square(side: Int)

struct Root { a: Int = 0 }

fn size(s: Shape) -> Int = match s {
  circle(r) | square(side) => 1
}
```

<!-- run: check e0433.stav -->

```text
error[E0433]: the alternatives of a pattern must bind the same names with the same types
  ┌─ e0433.stav:6:15
  │
6 │   circle(r) | square(side) => 1
  │               ^^^^^^^^^^^^
```

Bind the same names in every alternative, or split the alternatives into separate arms.

### E0434

A `match` does not cover every possible value. The arms without guards must cover every variant of a sum type, both values of `Bool` and `none` of an `Option`. Numbers, strings and collections need a `_` or binding arm. The note lists what is missing. A `match` with so many combinations of payload patterns that checking it would take too long is reported with this code too.

<!-- file: e0434.stav -->

```stav
type Level = debug | info | warn | error

struct Root { a: Bool = false }

fn verbose(l: Level) -> Bool = match l {
  debug => true
  info  => true
}
```

<!-- run: check e0434.stav -->

```text
error[E0434]: non-exhaustive `match` on `Level`
  ┌─ e0434.stav:5:32
  │
5 │ fn verbose(l: Level) -> Bool = match l {
  │                                ^^^^^^^
  │
  = note: missing: `warn`, `error`
  = help: add the missing arms or a `_ => ...` arm
```

Add the missing arms, or a final `_ => ...` arm.

### E0435

A `part { ... }` expression has no expected type, so the struct its body is checked against is unknown.

<!-- file: e0435.stav -->

```stav
struct Root { a: Int = 0 }

let p = part { a = 1 }
```

<!-- run: check e0435.stav -->

```text
error[E0435]: the struct of this `part` is unknown
  ┌─ e0435.stav:3:9
  │
3 │ let p = part { a = 1 }
  │         ^^^^^^^^^^^^^^
  │
  = help: give the binding or the function a type such as `Part<S>`
```

Give the binding or the function a type, as in `let p: Part<Root> = part { a = 1 }`.

### E0436

An attribute path goes through a value that has no attributes. Attribute paths walk through struct fields, options of structs and map keys.

<!-- file: e0436.stav -->

```stav
struct Root { port: Int = 80 }

port.value = 8080
```

<!-- run: check e0436.stav -->

```text
error[E0436]: `port` is a value of type `Int`, which has no attribute `value`
  ┌─ e0436.stav:3:6
  │
3 │ port.value = 8080
  │      ^^^^^
  │
  = help: attribute paths go through struct fields and map keys
```

Shorten the path to the attribute itself.

### E0437

A block needs a label but has none. Blocks that add an entry to a map, through a `block` clause or a map field, are written with a label, as in `server api { ... }`.

<!-- file: e0437.stav -->

```stav
struct Server { port: Int = 80 }

struct Root { servers: Map<Str, Server> = {} block server }

server { port = 8080 }
```

<!-- run: check e0437.stav -->

```text
error[E0437]: the `server` block needs a label
  ┌─ e0437.stav:5:1
  │
5 │ server { port = 8080 }
  │ ^^^^^^
  │
  = help: write `server <label> { ... }`
```

Add a label.

### E0438

A block that fills a struct field has a label. Such a block stands for the field itself, so there is nothing to label.

<!-- file: e0438.stav -->

```stav
struct Tls { port: Int = 443 }

struct Root { tls: Tls = {} }

tls main { port = 8443 }
```

<!-- run: check e0438.stav -->

```text
error[E0438]: the `tls` block takes no label
  ┌─ e0438.stav:5:5
  │
5 │ tls main { port = 8443 }
  │     ^^^^
  │
  = help: write `tls { ... }`
```

Remove the label.

### E0439

The label of a tagged block names no variant of the field's sum type. A tagged block selects a variant by its label and sets its payload fields. When the map of a `block` clause is keyed by a sum type, its labels must name variants too.

<!-- file: e0439.stav -->

```stav
type Build = native(features: List<Str> = []) | script(target: Str = "all")

struct Root { build: Build }

build nativ { features = ["simd"] }
```

<!-- run: check e0439.stav -->

```text
error[E0439]: `nativ` is not a variant of `Build`
  ┌─ e0439.stav:5:7
  │
5 │ build nativ { features = ["simd"] }
  │       ^^^^^
  │
  = help: did you mean `native`?
```

Name one of the variants.

### E0440

A block names neither a field nor a `block` clause of the struct it appears in.

<!-- file: e0440.stav -->

```stav
struct Server { port: Int = 80 }

struct Root { servers: Map<Str, Server> = {} block server }

servre api { port = 8080 }
```

<!-- run: check e0440.stav -->

```text
error[E0440]: `Root` has no block `servre`
  ┌─ e0440.stav:5:1
  │
5 │ servre api { port = 8080 }
  │ ^^^^^^
  │
  = help: did you mean `server` or `servers`?
```

Use one of the block kinds the struct offers. The help lists the closest ones.

### E0441

`default(...)` or `force(...)` appears inside an expression. They are priority markers, not functions, and are valid only as the whole right side of an attribute statement.

<!-- file: e0441.stav -->

```stav
struct Root { port: Int = 80 }

port = force(8080) + 1
```

<!-- run: check e0441.stav -->

```text
error[E0441]: `force` is a priority marker, valid only as the whole right side of an attribute
  ┌─ e0441.stav:3:8
  │
3 │ port = force(8080) + 1
  │        ^^^^^^^^^^^
  │
  = help: write `attribute = force(...)`
```

Compute the value first and mark the whole right side, as in `port = force(8081)`.

### E0442

A part is included into a body of a different struct. `include` in a body of struct `S` needs a `Part<S>`.

<!-- file: e0442.stav -->

```stav
struct Server { name: Str }

struct Root { server: Server, port: Int = 80 }

fn tuned() -> Part<Root> = part { port = 8080 }

server {
  name = "web"
  include tuned()
}
```

<!-- run: check e0442.stav -->

```text
error[E0442]: cannot include a `Part<Root>` into a body of `Server`
  ┌─ e0442.stav:9:11
  │
9 │   include tuned()
  │           ^^^^^^^
  │
  = help: include it into a `Root` block, or make it a `Part<Server>`
```

Include the part into a body of its own struct, or change its type.

### E0443

A field has a `block` clause, but its type is neither a map of structs nor a sum type.

<!-- file: e0443.stav -->

```stav
struct Root { count: Int = 0 block counter }
```

<!-- run: check e0443.stav -->

```text
error[E0443]: a `block` clause needs a map of structs or a sum type, not `Int`
  ┌─ e0443.stav:1:15
  │
1 │ struct Root { count: Int = 0 block counter }
  │               ^^^^^
  │
  = help: declare the field as `Map<Str, T>` with a struct `T`
```

Give the field a type such as `Map<Str, Counter>`, or remove the clause.

### E0444

A call or a type has the wrong arguments: missing or extra arguments, named arguments where only positional ones are allowed, a variant constructed with or without payload when the opposite is needed, type arguments on a type that takes none, a default on a lambda parameter, or an `Fn` type without parameter and return types.

<!-- file: e0444.stav -->

```stav
struct Root { a: Int = 0 }

fn add(x: Int, y: Int) -> Int = x + y

a = add(1)
```

<!-- run: check e0444.stav -->

```text
error[E0444]: `add` is missing the argument `y`
  ┌─ e0444.stav:5:5
  │
5 │ a = add(1)
  │     ^^^^^^
```

Pass the arguments the function or type expects.

## Merging

These are reported after evaluation, when the contributions of every file are merged. [Merging rules](/reference/merging) explains the rules behind them.

### E0501

Two or more contributions give different values to the same attribute at the same priority. For scalars, the contributions with the highest priority must agree. Every winner is listed with its priority.

<!-- file: e0501.stav -->

```stav
struct Root { port: Int = 80 }

port = 8080
port = 9090
```

<!-- run: eval e0501.stav -->

```text
error[E0501]: conflicting values for `port`
  ┌─ e0501.stav:3:8
  │
3 │ port = 8080
  │        ^^^^
  │        │
  │        8080 (plain)
4 │ port = 9090
  │        ---- 9090 (plain)
  │
  = help: keep one of them, or mark the one that should win with `force(...)`
```

Keep one of the values, or mark the one that should win with `force(...)`, or the others with `default(...)`.

### E0502

Tagged blocks of the same priority select different variants of one sum type.

<!-- file: e0502.stav -->

```stav
type Build = native(features: List<Str> = []) | script(target: Str = "all")

struct Root { build: Build }

build native { }
build script { }
```

<!-- run: eval e0502.stav -->

```text
error[E0502]: tagged blocks select different variants of `build`
  ┌─ e0502.stav:5:1
  │
5 │ build native { }
  │ ^^^^^^^^^^^^^^^^
  │ │
  │ `native` (plain)
6 │ build script { }
  │ ---------------- `script` (plain)
  │
  = help: keep the blocks of one variant, or select the variant with `force(...)`
```

Keep the blocks of one variant, or select the variant with a higher priority, as in `build = force(script(target = "all"))`.

### E0503

`Data` contributions of the same priority have different shapes, for example a map and a list. Maps merge per key and lists concatenate, but a map and a list cannot merge.

<!-- file: e0503.stav -->

```stav
struct Root { settings: Data = none }

settings = { port = 80 }
settings = ["a", "b"]
```

<!-- run: eval e0503.stav -->

```text
error[E0503]: `Data` values of different shapes for `settings`
  ┌─ e0503.stav:3:12
  │
3 │ settings = { port = 80 }
  │            ^^^^^^^^^^^^^
  │            │
  │            a map (plain)
4 │ settings = ["a", "b"]
  │            ---------- a list (plain)
  │
  = help: a higher priority contribution replaces the others: mark one with `force(...)`, or the others with `default(...)`
```

Give one of them a higher priority. A `force(...)` or `default(...)` resolves every conflict of this kind.

## Evaluation

These are reported while the program is evaluated, or while `self` is resolved after merging. Evaluation stops at the first of them. The checks for `self` cycles and recursion run before evaluation.

### E0600

An internal error: the checker accepted a program that the evaluator cannot evaluate. This is a bug in Stav. Please report it together with the program.

### E0601

Integer arithmetic overflows the range of `Int`. This includes `abs` of the smallest `Int` and `toInt` of a float that does not fit.

<!-- file: e0601.stav -->

```stav
struct Root { big: Int = 0 }

big = 9223372036854775807 + 1
```

<!-- run: eval e0601.stav -->

```text
error[E0601]: integer overflow in `9223372036854775807 + 1`
  ┌─ e0601.stav:3:27
  │
3 │ big = 9223372036854775807 + 1
  │                           ^
```

Keep the numbers within the 64-bit range.

### E0602

A float operation produces a value that is not finite: an infinity or a value that is not a number.

<!-- file: e0602.stav -->

```stav
struct Root { huge: Float = 0.0 }

huge = 1.0e308 * 10.0
```

<!-- run: eval e0602.stav -->

```text
error[E0602]: the result is not a finite number
  ┌─ e0602.stav:3:16
  │
3 │ huge = 1.0e308 * 10.0
  │                ^
```

Keep the numbers finite.

### E0603

An integer is divided by zero, with `/` or `%`.

<!-- file: e0603.stav -->

```stav
struct Root { share: Int = 0 }

let workers = 0

share = 100 / workers
```

<!-- run: eval e0603.stav -->

```text
error[E0603]: division by zero
  ┌─ e0603.stav:5:13
  │
5 │ share = 100 / workers
  │             ^
```

Check the divisor before dividing, for example with `if workers == 0 { 0 } else { 100 / workers }`.

### E0604

An index is out of range for a list, a map has no entry for a key, or a computed map key names no variant of the map's sum key type.

<!-- file: e0604.stav -->

```stav
struct Root { third: Int = 0 }

let ports = [80, 443]

third = ports[2]
```

<!-- run: eval e0604.stav -->

```text
error[E0604]: index 2 is out of range for a list of 2 elements
  ┌─ e0604.stav:5:9
  │
5 │ third = ports[2]
  │         ^^^^^^^^
```

Use `get(ports, 2)`, which returns an `Option`, and give a fallback with `??`.

### E0605

A map comprehension produces the same key twice. The same code is reported for duplicate keys in a map literal that only evaluation can detect.

<!-- file: e0605.stav -->

```stav
struct Root { by_name: Map<Str, Int> = {} }

let names = ["web", "api", "web"]

by_name = {for n in names -> n = len(n)}
```

<!-- run: eval e0605.stav -->

```text
error[E0605]: the comprehension produces the key `web` more than once
  ┌─ e0605.stav:5:30
  │
5 │ by_name = {for n in names -> n = len(n)}
  │                              ^
```

Make the keys unique, for example with `unique(names)`.

### E0606

`self` is read where it cannot be used. `self` is resolved after merging, so it cannot decide which contributions exist. It is not allowed in the condition of `if`, the collection of `for`, block labels, `include` expressions, function bodies and lambdas. A binding that reads `self` cannot be used there either, and cannot be used from the body of another block.

<!-- file: e0606.stav -->

```stav
struct Root { version: Str = "1", beta: Bool = false }

if self.version == "2" {
  beta = true
}
```

<!-- run: check e0606.stav -->

```text
error[E0606]: `self` cannot be used in an `if` condition
  ┌─ e0606.stav:3:4
  │
3 │ if self.version == "2" {
  │    ^^^^^^^^^^^^
  │
  = help: pass the value as a parameter instead
```

Compute the condition from values that are known without merging, such as a `let` binding with a constant value.

### E0607

`self` dependencies form a cycle, so a value would depend on itself. A cycle inside one body is found during checking. Cycles that only appear once files are merged are found after merging, per block instance, using only the contributions that won.

<!-- file: e0607.stav -->

```stav
struct Package {
  version: Str
  tag:     Str
}

struct Root { package: Package }

package {
  version = "#{self.tag}"
  tag     = "v#{self.version}"
}
```

<!-- run: check e0607.stav -->

```text
error[E0607]: `self` dependencies form a cycle
   ┌─ e0607.stav:9:13
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

Break the cycle by setting one of the attributes without `self`. [Late-bound self](/guide/self) explains how the graph is built.

### E0608

The program called `fail(...)`. The message is the text passed to `fail`.

<!-- file: e0608.stav -->

```stav
struct Root { port: Int = 0 }

let env = "staging"

port = if env == "prod" { 443 } else { fail("unknown environment #{env}") }
```

<!-- run: eval e0608.stav -->

```text
error[E0608]: evaluation failed: unknown environment staging
  ┌─ e0608.stav:5:40
  │
5 │ port = if env == "prod" { 443 } else { fail("unknown environment #{env}") }
  │                                        ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
```

Change the configuration so that the failing branch is not taken.

### E0609

A regular expression computed during evaluation is invalid. Literal patterns are checked earlier and report [E0415](#e0415).

<!-- file: e0609.stav -->

```stav
struct Root { ok: Bool = false }

let pattern = "[a-"

ok = matches("abc", pattern)
```

<!-- run: eval e0609.stav -->

```text
error[E0609]: `[a-` is not a valid regular expression
  ┌─ e0609.stav:5:21
  │
5 │ ok = matches("abc", pattern)
  │                     ^^^^^^^
```

Fix the pattern.

### E0610

A function calls itself, directly or through other functions. Passing a function to itself as a value counts as a call. Evaluation must terminate, so functions cannot recurse.

<!-- file: e0610.stav -->

```stav
struct Root { a: Int = fact(5) }

fn fact(n: Int) -> Int = if n == 0 { 1 } else { n * fact(n - 1) }
```

<!-- run: check e0610.stav -->

```text
error[E0610]: `fact` calls itself
  ┌─ e0610.stav:3:53
  │
3 │ fn fact(n: Int) -> Int = if n == 0 { 1 } else { n * fact(n - 1) }
  │                                                     ^^^^
  │
  = note: cycle: `fact` -> `fact`
  = note: evaluation must terminate, so functions cannot recurse
  = help: iterate with `fold` over `range(...)`, for example `fold(range(1, n + 1), 1, fn(acc: Int, i: Int) = acc * i)`
```

Iterate over a finite collection instead, as the help shows with `fold` over `range(...)`.

### E0611

Evaluation needs more steps than its step limit allows, or nests calls and expressions deeper than the evaluator allows. The default step limit is 50 000 000. `stav eval --fuel` sets another one.

<!-- file: e0611.stav -->

```stav
struct Root { total: Int = 0 }

total = fold(range(0, 1000), 0, fn(acc: Int, i: Int) = acc + i)
```

<!-- run: eval --fuel 1000 e0611.stav -->

```text
error[E0611]: evaluation needs more than 1000 steps
  ┌─ e0611.stav:3:14
  │
3 │ total = fold(range(0, 1000), 0, fn(acc: Int, i: Int) = acc + i)
  │              ^^^^^^^^^^^^^^
  │
  = help: make the computation smaller, or raise the step limit
```

Make the computation smaller, or raise the limit with `--fuel`.

### E0612

A value grows larger than a size limit. One string may hold 16 MiB, one collection 10 000 000 elements, and all values built together 512 MiB. Values nested more than 128 levels deep and attribute paths longer than 128 steps are reported too.

<!-- file: e0612.stav -->

```stav
struct Root { count: Int = 0 }

count = len(range(0, 20000000))
```

<!-- run: eval e0612.stav -->

```text
error[E0612]: a collection has more than 10000000 elements
  ┌─ e0612.stav:3:13
  │
3 │ count = len(range(0, 20000000))
  │             ^^^^^^^^^^^^^^^^^^
  │
  = help: make the value smaller, or raise the size limits
```

Make the value smaller. A host can raise the limits through `Limits`.

### E0613

A host's symbol type rejects the text of a symbol, for example a package name with a space in it. The message includes the host's reason. The `stav` command has no symbol types, so only programs that a host evaluates can report this.

Write the symbol in a form the host accepts.

## Files, imports and roots

### E0701

A file or module cannot be found or read: an imported or included file that does not exist, a `std/` module the host does not serve, or a module that is missing from a namespace of host modules.

<!-- file: e0701.stav -->

```stav
import "./missing.stav" as missing

struct Root { a: Int = 0 }
```

<!-- run: check e0701.stav -->

```text
error[E0701]: `missing.stav` does not exist
  ┌─ e0701.stav:1:8
  │
1 │ import "./missing.stav" as missing
  │        ^^^^^^^^^^^^^^^^
```

Fix the path, or create the file. The `stav` command serves `std/` modules only with `--std <DIR>`.

### E0702

A path leaves its import root. Imports, part files and path literals are resolved relative to the file that contains them, and the result must stay inside the import root. The `stav` command uses the directory of the entry file, or the one `--root-dir` names. A file that leaves the root through a symbolic link, and an entry file outside every import root, are reported too.

<!-- file: e0702.stav -->

```stav
struct Root { patch: Path }

patch = ../outside.patch
```

<!-- run: check e0702.stav -->

```text
error[E0702]: the path `../outside.patch` leaves its import root
  ┌─ e0702.stav:3:9
  │
3 │ patch = ../outside.patch
  │         ^^^^^^^^^^^^^^^^
```

Move the file inside the import root, or widen the root with `--root-dir`.

### E0703

Imports or part file includes form a cycle. The note shows the cycle.

`e0703.stav`:

<!-- file: e0703.stav -->

```stav
struct Root { a: Int = 0 }

include "./e0703-part.stav"
```

`e0703-part.stav`:

<!-- file: e0703-part.stav -->

```stav
a = 1

include "./e0703-part.stav"
```

<!-- run: check e0703.stav -->

```text
error[E0703]: `e0703-part.stav` imports or includes itself
  ┌─ e0703-part.stav:3:1
  │
3 │ include "./e0703-part.stav"
  │ ^^^^^^^^^^^^^^^^^^^^^^^^^^^
  │
  = note: cycle: e0703-part.stav -> e0703-part.stav
```

Remove one of the imports or includes in the cycle. Shared declarations belong in a library that both files import.

### E0704

An import or include path has a form Stav does not support. Relative paths start with `./` or `../` and end in `.stav`. `std/<name>` names a standard module and `<namespace>/<module>` a host module. Absolute paths and URLs are not supported, and sources that the host passes in directly cannot use relative paths.

<!-- file: e0704.stav -->

```stav
import "./lib" as lib

struct Root { a: Int = 0 }
```

<!-- run: check e0704.stav -->

```text
error[E0704]: the relative import path `./lib` does not end in `.stav`
  ┌─ e0704.stav:1:8
  │
1 │ import "./lib" as lib
  │        ^^^^^^^
  │
  = help: write `./lib.stav`
```

Write the path as the help suggests.

### E0705

A path names something that is not a regular file, such as a directory.

Point the import or include at a `.stav` file.

### E0706

A source file is larger than 4 MiB.

Split the file into smaller files.

### E0707

A prelude file imports a module. Prelude files are passed in by a host and must stand on their own.

Declare what the prelude needs in the prelude itself.

### E0708

A host lists the same entry file more than once, possibly through two different paths to the same file.

List every entry file once.

### E0710

The root struct does not exist. The `stav` command looks for a struct named `Root` in the entry file, or the struct `--root` names.

<!-- file: e0710.stav -->

```stav
struct Config { port: Int = 80 }
```

<!-- run: check e0710.stav -->

```text
error[E0710]: there is no root struct `Root`
  ┌─ e0710.stav:1:1
  │
1 │ struct Config { port: Int = 80 }
  │ ^
  │
  = help: declare `struct Root { ... }` in the entry file
```

Declare `struct Root { ... }`, or pass `--root Config`.

## Final value and rendering

These are reported once the value is merged, when it is checked as a whole, and when it is rendered or converted.

### E0801

A required field has no value. A field is required when it has no default and is not an `Option`. Every missing field of the final value is reported.

<!-- file: e0801.stav -->

```stav
struct Server {
  name: Str
  port: Int = 80
}

struct Root { server: Server }

server { port = 8080 }
```

<!-- run: eval e0801.stav -->

```text
error[E0801]: the required field `server.name` has no value
  ┌─ e0801.stav:8:1
  │
8 │ server { port = 8080 }
  │ ^^^^^^^^^^^^^^^^^^^^^^
  │
  = help: set it, or give the field a default
```

Set the field, or give it a default in the struct.

### E0802

A `Ref<T>` names a label that no block of the target map declares. The help suggests the closest declared label.

<!-- file: e0802.stav -->

```stav
struct Unit {
  exec:  Str
  after: List<Ref<Unit>> = []
}

struct Root { units: Map<Str, Unit> = {} block unit }

unit db { exec = "/usr/bin/db" }
unit api {
  exec  = "/usr/bin/api"
  after = [dbb]
}
```

<!-- run: eval e0802.stav -->

```text
error[E0802]: `units.api.after` refers to `dbb`, which is not a declared `unit`
   ┌─ e0802.stav:11:12
   │
11 │   after = [dbb]
   │            ^^^
   │
   = note: references to `Unit` name the blocks of `unit`
   = help: did you mean `db`?
```

Fix the label, or declare the block it refers to.

### E0803

A value violates a refinement. Refinements of fields are checked on the final value, and refinements of parameters, payload fields and annotated `let` bindings when the value is passed or bound. The diagnostic points at the value and at the declaration of the constraint.

<!-- file: e0803.stav -->

```stav
struct Root { port: Int where 1..=65535 = 80 }
port = 70000
```

<!-- run: eval e0803.stav -->

```text
error[E0803]: value violates a refinement
  ┌─ e0803.stav:2:8
  │
1 │ struct Root { port: Int where 1..=65535 = 80 }
  │                     ------------------- constraint declared here
2 │ port = 70000
  │        ^^^^^
  │        │
  │        70000 is not in 1..=65535
```

Give a value that meets the constraint.

### E0804

A value cannot be expressed in the requested output format. TOML needs a table at the top, so a list or a scalar cannot be rendered on its own, and a list cannot mix tables and other values.

<!-- file: e0804.stav -->

```stav
struct Root { ports: List<Int> = [80, 443] }
```

<!-- run: eval -o toml --path ports e0804.stav -->

```text
error[E0804]: cannot render `ports` as TOML: TOML needs a table at the top
  ┌─ e0804.stav:1:34
  │
1 │ struct Root { ports: List<Int> = [80, 443] }
  │                                  ^^^^^^^^^
```

Render a struct or a map, or choose JSON or YAML.

### E0805

An attribute path names no attribute of the value, or the attribute does not convert to the Rust type a host asks for. `stav eval --path` reports the first kind.

<!-- file: e0805.stav -->

```stav
struct Root { port: Int = 80 }
```

<!-- run: eval --path prot e0805.stav -->

```text
error[E0805]: there is no attribute `prot`
```

Fix the path. In a host, make the Rust type match the shape of the value.

## Warnings

Warnings do not stop checking or evaluation. `stav check` and `stav eval` print them and still succeed.

### W0302

A `let` binding or parameter has the name of a variant of the expected sum type. The binding wins, which is rarely what was meant.

<!-- file: w0302.stav -->

```stav
type Level = debug | info | warn | error

struct Root { level: Level = info }

let warn: Level = Level.info

level = warn
```

<!-- run: check w0302.stav -->

```text
warning[W0302]: `warn` refers to a binding, not to the variant of `Level`
  ┌─ w0302.stav:7:9
  │
7 │ level = warn
  │         ^^^^
  │
  = help: rename the binding, or write `Level.warn` for the variant
```

Rename the binding, or write the variant qualified, as in `Level.warn`.

### W0434

A `match` arm can never match, because the arms before it already cover every value it would match.

<!-- file: w0434.stav -->

```stav
type Level = debug | info | warn | error

struct Root { a: Int = 0 }

fn rank(l: Level) -> Int = match l {
  _     => 0
  debug => 1
}
```

<!-- run: check w0434.stav -->

```text
warning[W0434]: this arm can never match
  ┌─ w0434.stav:7:3
  │
7 │   debug => 1
  │   ^^^^^
  │
  = help: remove it, or move it before the arms that cover it
```

Remove the arm, or move it before the arms that cover it.

### W0703

The same part file is included twice into one body. Its contributions would be added twice, which concatenates lists twice.

`w0703.stav`:

<!-- file: w0703.stav -->

```stav
struct Root { tags: List<Str> = [] }

include "./w0703-part.stav"
include "./w0703-part.stav"
```

`w0703-part.stav`:

<!-- file: w0703-part.stav -->

```stav
tags = ["web"]
```

<!-- run: check w0703.stav -->

```text
warning[W0703]: `w0703-part.stav` is already included into this body
  ┌─ w0703.stav:4:1
  │
3 │ include "./w0703-part.stav"
  │ --------------------------- first included here
4 │ include "./w0703-part.stav"
  │ ^^^^^^^^^^^^^^^^^^^^^^^^^^^
  │
  = help: remove one of the includes
```

Remove one of the includes.
