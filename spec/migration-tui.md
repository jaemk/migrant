# Migration TUI

tui subcommand with interactive migration list and apply controls.

## TUI-1

`migrant tui` launches a terminal UI listing all managed migrations with applied status.

## TUI-2

Navigation: `j`/`k` or Up/Down arrows move the selection.

## TUI-3

Apply controls: `u` applies the next up migration, `d` applies the next down migration,
`a` applies all up, `D` applies all down.

## TUI-4

`r` refreshes migration status; `q`, Esc, or Ctrl+C exits.

Coverage: unit tests in `src/tui.rs` against an in-memory sqlite config (TUI-1 rendered
list and applied marks via ratatui's `TestBackend`, TUI-2 navigation and selection
clamping, TUI-3 all four apply keys plus already-complete messages, TUI-4 refresh and all
quit keys), and `tests/migrant.rs::tui_requires_an_interactive_terminal` for the non-tty
guard.
