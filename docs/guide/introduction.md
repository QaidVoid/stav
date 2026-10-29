# What Stav is

<p class="lead">
Stav is a statically typed, eager and total configuration language. A program is a set of <code>.stav</code> files written in block syntax. Stav checks the whole program, evaluates it, merges what every file contributes, and hands over one value that renders to JSON, TOML or YAML or converts into Rust types.
</p>

A configuration file starts out small and simple, and then it grows. It gains a second environment, a shared base, a few computed values and an override for one machine. At that point the file is a program, and it deserves what programs have: names, types, functions and reuse. It also needs guarantees that ordinary programs do not give. A configuration must never hang, never depend on where or when it runs, and never fail halfway through with an error that a checker could have reported up front.

Stav is built around those guarantees.

## Three properties

### Static types

Every value in Stav has a type, and the whole program is type checked before any of it is evaluated. Structs declare the shape of a configuration, sum types name its choices, and refinements such as `Int where 1..=65535` narrow its values. A misspelled field, a wrong type, a missing `match` arm or a broken import is reported with its location, and usually with the closest valid name as a hint.

Typing is local and predictable. The type that a position expects flows into the value written there, and functions declare their parameter and return types. An error therefore points at the line that is wrong and not at some distant consumer.

### Totality

Evaluation of a well-typed Stav program always terminates. The language is designed so that it cannot loop forever:

- A function cannot call itself, directly or through other functions.
- A type cannot contain itself, so every value is a finite tree.
- Loops and comprehensions run only over collections that already exist.
- Imports and part files cannot form cycles, and neither can the `self` references between attributes.

Termination alone is not enough, because a program that finishes after a billion years is not useful. Evaluation therefore runs with limits on steps and on memory. A program that crosses one stops with an error that points at the expression responsible.

### No laziness

Stav is eager. Statements are evaluated in order, each once, and every right-hand side is evaluated where it is written. There is one deliberate exception. A right-hand side that reads `self` is evaluated after merging, so that it can see the final value of its block, even when another file changed that value. The dependencies between such attributes are checked to be acyclic before any of them is evaluated.

Eager evaluation keeps errors close to their cause and makes the order of evaluation easy to follow. It is also what makes merging precise: every contribution is known, with its path and its priority, before the merge begins.

## Pure and deterministic

Evaluation reads nothing except the source files that the host serves. There is no access to environment variables, the clock, the network, random numbers or other files. The same files always give the same value, byte for byte, and the canonical JSON output is stable enough to hash.

Paths in a program are values that name files. Stav never opens the files that paths name. What happens to them is up to the program that consumes the value.

## Built for structure

The other half of Stav is about how configuration is organized.

- **Blocks.** A configuration reads like a document of nested blocks, such as `server { port = 8080 }` or `upstream primary { ... }`. Each block maps onto a field of the schema, so its contents are type checked against the struct it fills.
- **Merging.** Every file, block and part contributes values to paths in one final value. Lists concatenate, sets combine, maps merge per key, and single values settle by priority. Two contributions that disagree at the same priority are a reported conflict, never a silent override.
- **Priorities.** A declared default is the weakest value, then `default(...)`, then a plain assignment, then `force(...)`. A shared part can offer defaults that a user overrides without ceremony.
- **Parts.** A part is a typed, reusable fragment of a body. Parts are written as functions that take parameters or as separate files, and they are included wherever their struct fits.

## Where Stav fits

Stav has two kinds of users.

The `stav` command evaluates a program and prints its value as JSON, TOML or YAML. Any application that reads one of these formats gets a typed, checked configuration source without linking anything.

A Rust program can embed the `stav` crate instead. The host program supplies the source files, and it can add a prelude of schemas, standard modules, domain types of its own and validation after evaluation. It then reads the evaluated value straight into its own types through serde.

::: info Version 0.1
Stav is young. Until version 1.0, a minor release may change the language and the API. The changelog lists every change.
:::

## Where to go next

- [Install the `stav` command](./installation).
- [Write a first program](./first-program) and evaluate it.
- [Take the tour](./tour) of the language.
