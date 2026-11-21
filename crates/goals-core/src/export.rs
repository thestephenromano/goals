//! Export everything to JSON or CSV (goals#2).
//!
//! The export format is its own, versioned schema ([`FORMAT`], [`VERSION`]),
//! not the storage tables, so it stays stable as storage changes and can be
//! read back ([`Export::from_json`]). Amounts are written exactly, in their
//! shortest decimal text (`5.25`); dates are ISO 8601 (`2026-01-31`), and
//! timestamps RFC 3339 in UTC.
//!
//! - **JSON:** one document. Each goal carries its milestones (in order),
//!   check-ins and reviews.
//! - **CSV:** one file per kind of record (`goals.csv`, `milestones.csv`,
//!   `check_ins.csv`, `reviews.csv`) with IDs to join them. Cells are written
//!   as they are, so a note that starts with `=` is text a spreadsheet may
//!   treat as a formula; the export is the user's own data, and altering it
//!   would break the exactness above.

use std::io::{Read, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::domain::{Direction, Goal, Measure, Status};
use crate::error::{GoalsError, Result};
use crate::storage::Database;

pub const FORMAT: &str = "goals-export";
pub const VERSION: u32 = 1;

/// The CSV files [`Export::write_csv`] writes into a directory.
pub const CSV_FILES: [&str; 4] = [
    "goals.csv",
    "milestones.csv",
    "check_ins.csv",
    "reviews.csv",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Export {
    pub format: String,
    pub version: u32,
    pub exported_at: String,
    pub goals: Vec<ExportedGoal>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportedGoal {
    pub id: String,
    pub title: String,
    pub done_when: String,
    pub why: String,
    pub start_date: String,
    pub target_date: Option<String>,
    pub measure: ExportedMeasure,
    pub status: ExportedStatus,
    pub created_at: String,
    pub updated_at: String,
    pub milestones: Vec<ExportedMilestone>,
    pub check_ins: Vec<ExportedCheckIn>,
    pub reviews: Vec<ExportedReview>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExportedMeasure {
    Milestones,
    Total {
        start: String,
        target: String,
        unit: String,
    },
    Level {
        start: String,
        target: String,
        unit: String,
        /// `up` or `down`.
        better: String,
    },
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ExportedStatus {
    Active,
    Achieved { on: String },
    Abandoned { on: String, reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportedMilestone {
    pub id: String,
    /// 1 for the first milestone.
    pub position: usize,
    pub title: String,
    pub due: Option<String>,
    pub reached_on: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportedCheckIn {
    pub id: String,
    pub on: String,
    pub note: String,
    pub value: Option<String>,
    pub milestone_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportedReview {
    pub id: String,
    pub on: String,
    pub committed: bool,
    pub on_track: bool,
    pub next_step: String,
    pub created_at: String,
}

fn timestamp(ms: i64) -> String {
    jiff::Timestamp::from_millisecond(ms)
        .map(|t| t.to_string())
        .unwrap_or_else(|_| ms.to_string())
}

impl Database {
    /// Everything in the database, as of now.
    pub fn export(&self) -> Result<Export> {
        let goals = self
            .goals()?
            .into_iter()
            .map(|goal| self.export_goal(goal))
            .collect::<Result<_>>()?;
        Ok(Export {
            format: FORMAT.into(),
            version: VERSION,
            exported_at: jiff::Timestamp::now().to_string(),
            goals,
        })
    }

    fn export_goal(&self, goal: Goal) -> Result<ExportedGoal> {
        let milestones = self
            .milestones(&goal.id)?
            .into_iter()
            .enumerate()
            .map(|(i, m)| ExportedMilestone {
                id: m.id,
                position: i + 1,
                title: m.title,
                due: m.due.map(|d| d.to_string()),
                reached_on: m.reached_on.map(|d| d.to_string()),
            })
            .collect();
        let check_ins = self
            .check_ins(&goal.id)?
            .into_iter()
            .map(|c| ExportedCheckIn {
                id: c.id,
                on: c.on.to_string(),
                note: c.note,
                value: c.value.map(|v| v.to_string()),
                milestone_id: c.milestone_id,
                created_at: timestamp(c.created_at),
                updated_at: timestamp(c.updated_at),
            })
            .collect();
        let reviews = self
            .reviews(&goal.id)?
            .into_iter()
            .map(|r| ExportedReview {
                id: r.id,
                on: r.on.to_string(),
                committed: r.committed,
                on_track: r.on_track,
                next_step: r.next_step,
                created_at: timestamp(r.created_at),
            })
            .collect();
        let measure = match goal.measure {
            Measure::Milestones => ExportedMeasure::Milestones,
            Measure::Done => ExportedMeasure::Done,
            Measure::Total {
                start,
                target,
                unit,
            } => ExportedMeasure::Total {
                start: start.to_string(),
                target: target.to_string(),
                unit,
            },
            Measure::Level {
                start,
                target,
                unit,
                better,
            } => ExportedMeasure::Level {
                start: start.to_string(),
                target: target.to_string(),
                unit,
                better: match better {
                    Direction::Up => "up",
                    Direction::Down => "down",
                }
                .into(),
            },
        };
        let status = match goal.status {
            Status::Active => ExportedStatus::Active,
            Status::Achieved { on } => ExportedStatus::Achieved { on: on.to_string() },
            Status::Abandoned { on, reason } => ExportedStatus::Abandoned {
                on: on.to_string(),
                reason,
            },
        };
        Ok(ExportedGoal {
            id: goal.id,
            title: goal.title,
            done_when: goal.done_when,
            why: goal.why,
            start_date: goal.start_date.to_string(),
            target_date: goal.target_date.map(|d| d.to_string()),
            measure,
            status,
            created_at: timestamp(goal.created_at),
            updated_at: timestamp(goal.updated_at),
            milestones,
            check_ins,
            reviews,
        })
    }
}

impl Export {
    pub fn write_json(&self, out: impl Write) -> Result<()> {
        serde_json::to_writer_pretty(out, self).map_err(json_error)
    }

    /// Read an export back. Refuses another format or a newer version.
    pub fn from_json(input: impl Read) -> Result<Self> {
        let export: Export = serde_json::from_reader(input).map_err(json_error)?;
        if export.format != FORMAT {
            return Err(GoalsError::Validation(format!(
                "not a Goals export (format {:?})",
                export.format
            )));
        }
        if export.version > VERSION {
            return Err(GoalsError::Validation(format!(
                "export version {} is newer than this app's {VERSION}",
                export.version
            )));
        }
        Ok(export)
    }

    /// Write [`CSV_FILES`] into `dir`, replacing files of those names.
    pub fn write_csv(&self, dir: &Path) -> Result<()> {
        let open = |name: &str| csv::Writer::from_path(dir.join(name)).map_err(csv_error);
        let [goals_file, milestones_file, check_ins_file, reviews_file] = CSV_FILES;
        let mut goals = open(goals_file)?;
        let mut milestones = open(milestones_file)?;
        let mut check_ins = open(check_ins_file)?;
        let mut reviews = open(reviews_file)?;
        goals
            .write_record([
                "id",
                "title",
                "done_when",
                "why",
                "start_date",
                "target_date",
                "measure",
                "start",
                "target",
                "unit",
                "better",
                "status",
                "status_on",
                "abandoned_reason",
                "created_at",
                "updated_at",
            ])
            .map_err(csv_error)?;
        milestones
            .write_record(["id", "goal_id", "position", "title", "due", "reached_on"])
            .map_err(csv_error)?;
        check_ins
            .write_record([
                "id",
                "goal_id",
                "on",
                "note",
                "value",
                "milestone_id",
                "created_at",
                "updated_at",
            ])
            .map_err(csv_error)?;
        reviews
            .write_record([
                "id",
                "goal_id",
                "on",
                "committed",
                "on_track",
                "next_step",
                "created_at",
            ])
            .map_err(csv_error)?;
        let text = |o: &Option<String>| o.clone().unwrap_or_default();
        for g in &self.goals {
            let (kind, start, target, unit, better) = match &g.measure {
                ExportedMeasure::Milestones => ("milestones", "", "", "", ""),
                ExportedMeasure::Done => ("done", "", "", "", ""),
                ExportedMeasure::Total {
                    start,
                    target,
                    unit,
                } => ("total", start.as_str(), target.as_str(), unit.as_str(), ""),
                ExportedMeasure::Level {
                    start,
                    target,
                    unit,
                    better,
                } => (
                    "level",
                    start.as_str(),
                    target.as_str(),
                    unit.as_str(),
                    better.as_str(),
                ),
            };
            let (status, on, reason) = match &g.status {
                ExportedStatus::Active => ("active", "", ""),
                ExportedStatus::Achieved { on } => ("achieved", on.as_str(), ""),
                ExportedStatus::Abandoned { on, reason } => {
                    ("abandoned", on.as_str(), reason.as_str())
                }
            };
            goals
                .write_record([
                    g.id.as_str(),
                    &g.title,
                    &g.done_when,
                    &g.why,
                    &g.start_date,
                    &text(&g.target_date),
                    kind,
                    start,
                    target,
                    unit,
                    better,
                    status,
                    on,
                    reason,
                    &g.created_at,
                    &g.updated_at,
                ])
                .map_err(csv_error)?;
            for m in &g.milestones {
                milestones
                    .write_record([
                        m.id.as_str(),
                        &g.id,
                        &m.position.to_string(),
                        &m.title,
                        &text(&m.due),
                        &text(&m.reached_on),
                    ])
                    .map_err(csv_error)?;
            }
            for c in &g.check_ins {
                check_ins
                    .write_record([
                        c.id.as_str(),
                        &g.id,
                        &c.on,
                        &c.note,
                        &text(&c.value),
                        &text(&c.milestone_id),
                        &c.created_at,
                        &c.updated_at,
                    ])
                    .map_err(csv_error)?;
            }
            for r in &g.reviews {
                reviews
                    .write_record([
                        r.id.as_str(),
                        &g.id,
                        &r.on,
                        &r.committed.to_string(),
                        &r.on_track.to_string(),
                        &r.next_step,
                        &r.created_at,
                    ])
                    .map_err(csv_error)?;
            }
        }
        for mut w in [goals, milestones, check_ins, reviews] {
            w.flush()?;
        }
        Ok(())
    }
}

fn json_error(e: serde_json::Error) -> GoalsError {
    match e.classify() {
        serde_json::error::Category::Io => GoalsError::Io(e.into()),
        _ => GoalsError::Validation(format!("not a readable export: {e}")),
    }
}

fn csv_error(e: csv::Error) -> GoalsError {
    if e.is_io_error() {
        match e.into_kind() {
            csv::ErrorKind::Io(io) => GoalsError::Io(io),
            other => GoalsError::Validation(format!("{other:?}")),
        }
    } else {
        GoalsError::Validation(format!("CSV: {e}"))
    }
}
