//! `migrant status`: report the state of every managed migration in either a
//! human-readable text form or machine-readable JSON.
//!
//! The rendering is factored out of `main` into pure functions over a
//! serializable [`StatusReport`] so both formats are unit-testable without a
//! live database.

use migrant_lib::MigrationStatus;
use serde::Serialize;

/// A single migration's tag and whether it is currently applied.
#[derive(Debug, Clone, Serialize)]
pub struct StatusRow {
    pub tag: String,
    pub applied: bool,
}

/// The full migration-table status: per-migration rows plus summary counts.
#[derive(Debug, Clone, Serialize)]
pub struct StatusReport {
    pub total: usize,
    pub applied: usize,
    pub pending: usize,
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
            })
            .collect();
        let applied = migrations.iter().filter(|r| r.applied).count();
        StatusReport {
            total: migrations.len(),
            applied,
            pending: migrations.len() - applied,
            migrations,
        }
    }

    /// Render the report as human-readable text: a summary line followed by one
    /// `[✓]`/`[ ]` row per migration.
    pub fn render_text(&self) -> String {
        let mut out = format!(
            "Migration status: {} applied, {} pending ({} total)",
            self.applied, self.pending, self.total
        );
        for row in &self.migrations {
            out.push_str(&format!(
                "\n  [{}] {}",
                if row.applied { '✓' } else { ' ' },
                row.tag
            ));
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
            },
            StatusRow {
                tag: "20171126194042_second".to_string(),
                applied: false,
            },
        ]
    }

    fn report() -> StatusReport {
        let migrations = rows();
        let applied = migrations.iter().filter(|r| r.applied).count();
        StatusReport {
            total: migrations.len(),
            applied,
            pending: migrations.len() - applied,
            migrations,
        }
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
        assert_eq!(value["migrations"][0]["tag"], "20170812145327_initial");
        assert_eq!(value["migrations"][0]["applied"], true);
        assert_eq!(value["migrations"][1]["applied"], false);
    }
}
