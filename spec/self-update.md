# Self Update

self update subcommand pulling the latest release binary from GitHub.

## SELFUP-1

`migrant self update` replaces the running binary with the latest GitHub release. Requires
the `update` cargo feature.

## SELFUP-2

Version comparison handles backported releases: an older-versioned release published later
does not replace a newer installed binary.

Coverage: `update_tests` in `src/main.rs` (backport handling, version comparison).
