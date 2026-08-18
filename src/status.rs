//! `migrant status`: report the state of every managed migration in either a
//! human-readable text form or machine-readable JSON.
//!
//! The rendering is factored out of `main` into pure functions over a
//! serializable [`StatusReport`] so both formats are unit-testable without a
//! live database.

use migrant_lib::MigrationStatus;
use serde::Serialize;

/// A single migration's tag, whether it is currently applied, and (for
/// repeatable migrations) whether it is due to re-run.
#[derive(Debug, Clone, Serialize)]
pub struct StatusRow {
    pub tag: String,
    pub applied: bool,
    pub repeatable: bool,
    pub stale: bool,
}

impl StatusRow {
    /// The status mark: applied, pending, or applied but due to re-run.
    fn mark(&self) -> char {
        match (self.applied, self.stale) {
            (true, true) => '~',
            (true, false) => '✓',
            (false, _) => ' ',
        }
    }

    /// The trailing annotation, empty for versioned migrations. A repeatable
    /// migration that has never run "will run"; only one with a recorded row it
    /// no longer matches "will re-run".
    fn note(&self) -> &'static str {
        match (self.repeatable, self.stale, self.applied) {
            (false, _, _) => "",
            (true, true, true) => "  (repeatable, will re-run)",
            (true, true, false) => "  (repeatable, will run)",
            (true, false, _) => "  (repeatable)",
        }
    }
}

/// The full migration-table status: per-migration rows plus summary counts.
#[derive(Debug, Clone, Serialize)]
pub struct StatusReport {
    pub total: usize,
    pub applied: usize,
    pub pending: usize,
    /// Applied repeatable migrations that will re-run on the next `Up` run.
    ///
    /// Note this is narrower than the per-row `stale` flag, which is true for
    /// anything that would run next (including a versioned migration with no
    /// row yet). Those are counted in `pending` instead, so `applied + pending`
    /// still equals `total` and no migration is counted twice.
    pub stale: usize,
    pub migrations: Vec<StatusRow>,
}

impl StatusReport {
    /// Build a report from the library's migration statuses, computing the
    /// summary counts.
    pub fn from_statuses(statuses: &[MigrationStatus]) -> Self {
        let migrations: Vec<StatusRow> = statuses
            .iter()
            .map(|s| StatusRow {
                tag: s.tag().to_string(),
                applied: s.applied(),
                repeatable: s.repeatable(),
                stale: s.stale(),
            })
            .collect();
        let applied = migrations.iter().filter(|r| r.applied).count();
        let stale = migrations.iter().filter(|r| r.applied && r.stale).count();
        StatusReport {
            total: migrations.len(),
            applied,
            pending: migrations.len() - applied,
            stale,
            migrations,
        }
    }

    /// Render the report as human-readable text: a summary line followed by one
    /// `[✓]`/`[ ]`/`[~]` row per migration.
    pub fn render_text(&self) -> String {
        let mut out = format!(
            "Migration status: {} applied, {} pending",
            self.applied, self.pending
        );
        // Only mentioned when there is something to report, so the summary line
        // is unchanged for projects with no repeatable migrations.
        if self.stale > 0 {
            out.push_str(&format!(", {} stale", self.stale));
        }
        out.push_str(&format!(" ({} total)", self.total));
        for row in &self.migrations {
            out.push_str(&format!("\n  [{}] {}{}", row.mark(), row.tag, row.note()));
        }
        out
    }

    /// Render the report as pretty-printed JSON.
    pub fn render_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows() -> Vec<StatusRow> {
        vec![
            StatusRow {
                tag: "20170812145327_initial".to_string(),
                applied: true,
                repeatable: false,
                stale: false,
            },
            StatusRow {
                tag: "20171126194042_second".to_string(),
                applied: false,
                repeatable: false,
                stale: true,
            },
        ]
    }

    fn build(migrations: Vec<StatusRow>) -> StatusReport {
        let applied = migrations.iter().filter(|r| r.applied).count();
        let stale = migrations.iter().filter(|r| r.applied && r.stale).count();
        StatusReport {
            total: migrations.len(),
            applied,
            pending: migrations.len() - applied,
            stale,
            migrations,
        }
    }

    fn report() -> StatusReport {
        build(rows())
    }

    /// An up-to-date repeatable migration and a stale one, alongside an applied
    /// versioned migration.
    fn repeatable_report() -> StatusReport {
        build(vec![
            StatusRow {
                tag: "20170812145327_initial".to_string(),
                applied: true,
                repeatable: false,
                stale: false,
            },
            StatusRow {
                tag: "20171126194042_seed-roles".to_string(),
                applied: true,
                repeatable: true,
                stale: false,
            },
            StatusRow {
                tag: "20171126194043_refresh-views".to_string(),
                applied: true,
                repeatable: true,
                stale: true,
            },
        ])
    }

    #[test]
    fn counts_reflect_rows() {
        let r = report();
        assert_eq!(r.total, 2);
        assert_eq!(r.applied, 1);
        assert_eq!(r.pending, 1);
    }

    #[test]
    fn text_has_summary_and_a_row_per_migration() {
        let text = report().render_text();
        assert!(
            text.starts_with("Migration status: 1 applied, 1 pending (2 total)"),
            "unexpected summary line: {text}"
        );
        assert!(text.contains("[✓] 20170812145327_initial"));
        assert!(text.contains("[ ] 20171126194042_second"));
        // one summary line + one line per migration
        assert_eq!(text.lines().count(), 3);
    }

    #[test]
    fn empty_report_is_summary_only() {
        let r = StatusReport {
            total: 0,
            applied: 0,
            pending: 0,
            stale: 0,
            migrations: vec![],
        };
        let text = r.render_text();
        assert_eq!(text, "Migration status: 0 applied, 0 pending (0 total)");
    }

    #[test]
    fn json_round_trips_to_the_documented_shape() {
        let json = report().render_json().unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["total"], 2);
        assert_eq!(value["applied"], 1);
        assert_eq!(value["pending"], 1);
        assert_eq!(value["stale"], 0);
        assert_eq!(value["migrations"][0]["tag"], "20170812145327_initial");
        assert_eq!(value["migrations"][0]["applied"], true);
        assert_eq!(value["migrations"][0]["repeatable"], false);
        assert_eq!(value["migrations"][0]["stale"], false);
        assert_eq!(value["migrations"][1]["applied"], false);
    }

    // REPEAT-8
    #[test]
    fn repeatable_rows_are_annotated_and_stale_ones_marked() {
        let text = repeatable_report().render_text();
        // An up-to-date repeatable migration is applied and annotated.
        assert!(
            text.contains("[✓] 20171126194042_seed-roles  (repeatable)"),
            "unexpected up-to-date repeatable row: {text}"
        );
        // A stale one keeps its row but is marked as due to re-run.
        assert!(
            text.contains("[~] 20171126194043_refresh-views  (repeatable, will re-run)"),
            "unexpected stale repeatable row: {text}"
        );
        // Versioned migrations are unannotated.
        assert!(text.contains("[✓] 20170812145327_initial\n"));
    }

    // REPEAT-8
    #[test]
    fn summary_reports_stale_only_when_non_zero() {
        // A stale repeatable migration is applied (it has a row) and counted
        // separately from `pending`, which covers migrations with no row yet.
        let text = repeatable_report().render_text();
        assert!(
            text.starts_with("Migration status: 3 applied, 0 pending, 1 stale (3 total)"),
            "unexpected summary line: {text}"
        );
        // With nothing stale the summary is unchanged from a project with no
        // repeatable migrations at all.
        assert!(report()
            .render_text()
            .starts_with("Migration status: 1 applied, 1 pending (2 total)"));
    }

    // REPEAT-8
    #[test]
    fn json_carries_repeatable_and_stale_fields() {
        let json = repeatable_report().render_json().unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["stale"], 1);
        assert_eq!(value["migrations"][1]["repeatable"], true);
        assert_eq!(value["migrations"][1]["stale"], false);
        assert_eq!(value["migrations"][2]["repeatable"], true);
        assert_eq!(value["migrations"][2]["stale"], true);
        assert_eq!(value["migrations"][2]["applied"], true);
    }
}
