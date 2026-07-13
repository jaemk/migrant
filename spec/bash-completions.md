# Bash Completions

self bash-completions subcommand generating or installing completion scripts.

## BASHCO-1

`migrant self bash-completions` writes a bash completion script to stdout.

## BASHCO-2

`migrant self bash-completions install --path <path>` writes the completion script to the
given path.

Coverage: `tests/bash_completions.rs` (BASHCO-1 stdout output; BASHCO-2 install to a path,
plus declined-confirmation writes nothing). Implemented in `src/main.rs`/`src/cli.rs`.
