# Spec

Every feature is documented here before or as it lands, with its status.

## Feature status

Status values: `done` (implemented and covered by tests), `pending` (documented,
not yet built; the default), `research` (needs investigation or design before it
can be built). Keep each row's status current with `spec.py set`.

| Feature | Status | Spec |
|---------|--------|------|
| CLI Project Setup | done | [cli-project-setup.md](cli-project-setup.md) |
| CLI Migration Management | done | [cli-migration-management.md](cli-migration-management.md) |
| Database Shell | done | [database-shell.md](database-shell.md) |
| Self Update | done | [self-update.md](self-update.md) |
| Bash Completions | done | [bash-completions.md](bash-completions.md) |
| Migration TUI | done | [migration-tui.md](migration-tui.md) |
| Library Config API | done | [library-config-api.md](library-config-api.md) |
| Migration Types | done | [migration-types.md](migration-types.md) |
| Migrator API | done | [migrator-api.md](migrator-api.md) |
| Transactional Migrations | done | [transactional-migrations.md](transactional-migrations.md) |
| Advisory Locking | done | [advisory-locking.md](advisory-locking.md) |
| Settings Builders | done | [settings-builders.md](settings-builders.md) |
| In-Memory SQLite | done | [in-memory-sqlite.md](in-memory-sqlite.md) |
| Database Backends | done | [database-backends.md](database-backends.md) |
| Config File and Env Resolution | done | [config-file-and-env-resolution.md](config-file-and-env-resolution.md) |
| Error Handling API | done | [error-handling-api.md](error-handling-api.md) |
| Distribution | done | [distribution.md](distribution.md) |

## Conventions

- Each normative statement carries a stable ID (e.g. `FEAT-1`, `API-3`). IDs are
  append-only: retire an ID by marking it removed, never reuse the number.
- Specs are document-first: a feature is documented (status `pending`, or
  `research` if it needs design work) before implementation begins. Flip to
  `done` only once implemented and verified.
- Spec files are named `<slug>.md` and linked from the table above.
