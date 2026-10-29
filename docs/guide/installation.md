# Installation

Stav comes in two pieces. The `stav` command formats, checks and evaluates programs. The `stav` crate is the whole language as a Rust library, for programs that embed it.

## The `stav` command

The command is published on crates.io as `stav-cli`. Install it with cargo:

```sh
cargo install stav-cli
```

Building it needs Rust 1.89 or newer. If your toolchain is older, update it with `rustup update stable` first.

Check that the command is on your `PATH`:

```sh
stav --version
```

```text
stav 0.1.0
```

### From a checkout

To build the latest development version, clone the repository and install the command from it:

```sh
git clone https://github.com/QaidVoid/stav
cd stav
cargo install --path crates/stav-cli
```

## The library

To embed Stav in a Rust program, add the `stav` crate. Add `serde` with its `derive` feature as well if you want to read values into your own types:

```sh
cargo add stav
cargo add serde --features derive
```

The library performs no I/O of its own and has a small dependency tree. [Embedding in Rust](./embedding) shows how to use it.

## Editor setup

Stav source files use the `.stav` extension, UTF-8 encoding and LF line endings. Indent with spaces. `stav fmt` lays files out in their canonical form, so it is worth running on save or before every commit:

```sh
stav fmt .
```

With a directory, `stav fmt` formats every `.stav` file below it. [The `stav` command](../reference/cli) describes every option.

## Next

[Write a first program](./first-program).
