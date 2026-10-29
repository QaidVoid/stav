# Diagnostics

Stav reports every problem it finds as a diagnostic: an error or a warning with a stable code, a message and the places in the source it is about. The same diagnostics come from `stav check`, `stav eval` and `stav fmt`, and from the Rust library.

The [error code index](/reference/error-codes) explains every code with an example.

## Reading a diagnostic

This program misspells a variant and a block kind.

<!-- file: server.stav -->

```stav
type Level = debug | info | warn | error

struct Server {
  host:  Str
  port:  Int where 1..=65535 = 80
  level: Level = info
}

struct Root { servers: Map<Str, Server> = {} block server }

server web {
  host  = "web.example.org"
  level = wran
}

servre api { host = "api.example.org" }
```

`stav check server.stav` reports both mistakes.

<!-- run: check server.stav -->

```text
error[E0302]: unknown name `wran`, which is not a variant of `Level`
   ┌─ server.stav:13:11
   │
13 │   level = wran
   │           ^^^^
   │
   = help: did you mean `warn`?

error[E0440]: `Root` has no block `servre`
   ┌─ server.stav:16:1
   │
16 │ servre api { host = "api.example.org" }
   │ ^^^^^^
   │
   = help: did you mean `server` or `servers`?
```

Each diagnostic has the same parts.

- **Severity and code.** `error[E0302]` is an error with the code `E0302`. A code names one kind of problem and keeps its meaning across releases.
- **Message.** One line that says what is wrong. Messages start with a lowercase letter and end without a period.
- **Location.** The file, line and column of the primary span, after `┌─`. Lines and columns count from 1, and columns count characters.
- **Source excerpt.** The lines around the problem. The primary span is underlined with `^`.
- **Labels.** Other places that explain the problem are underlined with `-` and carry a short message. They may be in other files.
- **Notes.** Lines starting with `= note:` add context that belongs to no single place, such as the cycle a value depends on.
- **Help.** A line starting with `= help:` suggests a fix whenever one can be given.

A diagnostic with several labels shows each of them. Here two plain assignments conflict, so both are labeled with their value and priority.

<!-- file: ports.stav -->

```stav
struct Root { port: Int = 80 }

port = 8080
port = 9090
```

<!-- run: eval ports.stav -->

```text
error[E0501]: conflicting values for `port`
  ┌─ ports.stav:3:8
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

## Errors and warnings

An error means that the program is invalid. Nothing is printed on standard output when an error is reported, and the command fails.

A warning means that the program is valid but probably does not say what its author meant. Warnings are printed on standard error and the command still succeeds. Here a binding named `warn` hides the variant `warn` of `Level`.

<!-- file: levels.stav -->

```stav
type Level = debug | info | warn | error

struct Root { level: Level = info }

let warn: Level = Level.error

level = warn
```

<!-- run: check levels.stav -->

```text
warning[W0302]: `warn` refers to a binding, not to the variant of `Level`
  ┌─ levels.stav:7:9
  │
7 │ level = warn
  │         ^^^^
  │
  = help: rename the binding, or write `Level.warn` for the variant
```

`stav check levels.stav` prints the warning and exits with status 0. `stav eval levels.stav` prints the same warning and then the value, in which `level` is `"error"`. The warning goes to standard error and the value to standard output, so redirecting the output keeps only the value.

## When diagnostics are reported

Stav checks a program in stages, and each stage reports everything it can before the program moves on.

1. **Lexing and parsing** report malformed source, such as an unterminated string. Parsing recovers after an error, so several syntax errors are reported at once. A file with syntax errors is not type checked.
2. **Type checking** reports unknown names, type mismatches, non-exhaustive matches, recursion and every other static problem, across all files. Errors that only follow from an earlier error are not reported, so every error points at a real mistake.
3. **Evaluation** stops at the first runtime error, such as a division by zero or a crossed limit.
4. **Merging and the final checks** report every conflict, every missing required field, every violated refinement and every reference to an undeclared block.

`stav check` runs the first two stages, and `stav eval` runs all four.

## Suggestions

When a name, field, variant, block kind or label is unknown, the help suggests the closest names that would be valid in that place, as in ``did you mean `warn`?``. A suggestion is at most three names, and only names that differ from the misspelled one by a few edits are offered. When nothing is close, the diagnostic has no help.

## JSON output

`--message-format json` writes each diagnostic to standard error as one JSON object on a line of its own, for editors and other programs. It applies to every command.

<!-- run: --message-format json check server.stav -->

```text
{"code":"E0302","severity":"error","message":"unknown name `wran`, which is not a variant of `Level`","labels":[{"file":"server.stav","start":{"line":13,"col":11},"end":{"line":13,"col":15},"message":"","primary":true}],"notes":[],"help":"did you mean `warn`?"}
{"code":"E0440","severity":"error","message":"`Root` has no block `servre`","labels":[{"file":"server.stav","start":{"line":16,"col":1},"end":{"line":16,"col":7},"message":"","primary":true}],"notes":[],"help":"did you mean `server` or `servers`?"}
```

Laid out for reading, the first object is:

```json
{
  "code": "E0302",
  "severity": "error",
  "message": "unknown name `wran`, which is not a variant of `Level`",
  "labels": [
    {
      "file": "server.stav",
      "start": { "line": 13, "col": 11 },
      "end": { "line": 13, "col": 15 },
      "message": "",
      "primary": true
    }
  ],
  "notes": [],
  "help": "did you mean `warn`?"
}
```

| Field | Meaning |
| --- | --- |
| `code` | The stable code, such as `E0302` or `W0302`. |
| `severity` | `error` or `warning`. |
| `message` | What is wrong, in one line. |
| `labels` | The places the diagnostic is about. The first one is the primary span and has `primary` set to `true`. The others are secondary labels with their own messages. |
| `start`, `end` | The span of a label, as one-based lines and columns. `end` is the position just after the last character. |
| `notes` | Further explanations, possibly empty. |
| `help` | A suggested fix, or `null`. |

A problem that belongs to no place in the source has no labels. Here the attribute that `--path` names does not exist.

<!-- file: valid.stav -->

```stav
struct Root { port: Int = 80 }
```

<!-- run: --message-format json eval --path nope valid.stav -->

```text
{"code":"E0805","severity":"error","message":"there is no attribute `nope`","labels":[],"notes":[],"help":null}
```

## Colors

Diagnostics for people are colored when standard error is a terminal and the `NO_COLOR` environment variable is unset or empty. `--color always` colors them even when `NO_COLOR` is set or the output is redirected, and `--color never` turns colors off. JSON output is never colored.

## Exit status

| Status | Meaning |
| --- | --- |
| `0` | Success. Warnings may have been printed. |
| `1` | At least one diagnostic is an error, or `stav fmt --check` found a file that would change. |
| `2` | The command line is not valid, for example an unknown flag. |
| `3` | A file or a stream could not be read or written outside of diagnostics, for example a missing entry file. |

## Diagnostics in Rust

A program that embeds Stav receives diagnostics as a `stav::Diagnostics` value. `stav::check`, `Checked::eval` and `Evaluated::get` return it as their error, and `Checked::warnings` returns the warnings of a program that checks.

`Diagnostics` holds every diagnostic in the order it was found, together with the names and texts of the files they refer to.

- `iter()` yields each `FileDiagnostic`, which pairs a `Diagnostic` with the file it is about.
- A `Diagnostic` has the public fields `code`, `severity`, `message`, `span`, `help`, `labels` and `notes`, the same parts that the command prints.
- `has_errors()` tells whether at least one diagnostic is an error, and `file_name(...)` and `source(...)` return the name and text of a file.
- Its `Display` form writes each diagnostic as a line `error[CODE]: message`, followed by indented lines for the location as `--> file:line:column`, each label, each note and the help.

[Embedding in Rust](/guide/embedding) shows a complete host that checks, evaluates and reports diagnostics.
