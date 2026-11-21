//! Goals and what is recorded against them.

use jiff::civil::Date;

use crate::error::{GoalsError, Result};
use crate::quantity::Quantity;

/// Which way a level goal improves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Higher is better (deadlift 140 kg).
    Up,
    /// Lower is better (weigh 75 kg, run 10 km in under 50 minutes).
    Down,
}

impl Direction {
    pub(crate) fn sign(self) -> i128 {
        match self {
            Direction::Up => 1,
            Direction::Down => -1,
        }
    }
}

/// How a goal's progress is measured. One per goal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Measure {
    /// Ordered checkpoints, each reached or not, each with an optional date.
    Milestones,
    /// Check-ins add up: run 500 km, save $10,000. `start` is what was
    /// already done when the goal was set, usually zero.
    Total {
        start: Quantity,
        target: Quantity,
        unit: String,
    },
    /// The latest check-in is the current value: weigh 75 kg. `start` is
    /// the level when the goal was set.
    Level {
        start: Quantity,
        target: Quantity,
        unit: String,
        better: Direction,
    },
    /// Done or not, for goals the user would rather not break down.
    Done,
}

impl Measure {
    pub fn kind(&self) -> &'static str {
        match self {
            Measure::Milestones => "milestones",
            Measure::Total { .. } => "total",
            Measure::Level { .. } => "level",
            Measure::Done => "done",
        }
    }

    pub fn is_number(&self) -> bool {
        matches!(self, Measure::Total { .. } | Measure::Level { .. })
    }

    pub(crate) fn validate(&self) -> Result<()> {
        match self {
            Measure::Total { start, target, .. } if target <= start => Err(invalid(
                "a total's target must be more than what it starts from",
            )),
            Measure::Level {
                start,
                target,
                better,
                ..
            } => {
                // Equal is allowed: holding a level (stay at or under 75 kg).
                let worse = match better {
                    Direction::Up => target < start,
                    Direction::Down => target > start,
                };
                if worse {
                    Err(invalid(
                        "a level's target must not be worse than where it starts",
                    ))
                } else {
                    Ok(())
                }
            }
            _ => Ok(()),
        }
    }
}

/// Where a goal stands, as the user set it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Active,
    Achieved { on: Date },
    Abandoned { on: Date, reason: String },
}

impl Status {
    pub fn is_active(&self) -> bool {
        matches!(self, Status::Active)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Goal {
    pub id: String,
    pub title: String,
    /// What done looks like, stated so it can be checked.
    pub done_when: String,
    /// Why it matters.
    pub why: String,
    /// The day the pace line starts from.
    pub start_date: Date,
    /// The day the pace line reaches the target. Without one, a goal's pace
    /// is not judged (except for milestones past their own dates).
    pub target_date: Option<Date>,
    pub measure: Measure,
    pub status: Status,
    /// Milliseconds since the Unix epoch.
    pub created_at: i64,
    pub updated_at: i64,
}

/// A new goal. It starts active.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewGoal {
    pub title: String,
    pub done_when: String,
    pub why: String,
    pub start_date: Date,
    pub target_date: Option<Date>,
    pub measure: Measure,
}

/// A goal's editable fields, all replaced at once. The measure's kind can
/// change only while nothing has been recorded against the goal.
pub type GoalEdit = NewGoal;

impl NewGoal {
    pub(crate) fn validate(&self) -> Result<()> {
        required(&self.title, "a goal needs a title")?;
        required(
            &self.done_when,
            "a goal needs a statement of what done looks like",
        )?;
        if self.target_date.is_some_and(|t| t < self.start_date) {
            return Err(invalid("the target date is before the start date"));
        }
        self.measure.validate()
    }
}

/// An ordered checkpoint of a milestones goal. It is reached when a
/// check-in marks it, on that check-in's date.
#[derive(Debug, Clone, PartialEq)]
pub struct Milestone {
    pub id: String,
    pub goal_id: String,
    pub title: String,
    pub due: Option<Date>,
    /// The date of the check-in that marked it reached, if any.
    pub reached_on: Option<Date>,
    pub(crate) sort_order: f64,
}

/// A dated note of progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckIn {
    pub id: String,
    pub goal_id: String,
    pub on: Date,
    pub note: String,
    /// For number goals: the amount added (a total) or the new level.
    pub value: Option<Quantity>,
    /// For milestones goals: the milestone this check-in marks reached.
    pub milestone_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A check-in to record, or a check-in's new contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCheckIn {
    pub on: Date,
    pub note: String,
    pub value: Option<Quantity>,
    pub milestone_id: Option<String>,
}

/// The user's regular look at a goal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Review {
    pub id: String,
    pub goal_id: String,
    pub on: Date,
    /// Still committed to the goal.
    pub committed: bool,
    /// The user's own view of whether it is on track (beside [`crate::pace`]'s).
    pub on_track: bool,
    /// What's next.
    pub next_step: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewReview {
    pub on: Date,
    pub committed: bool,
    pub on_track: bool,
    pub next_step: String,
}

pub(crate) fn invalid(message: &str) -> GoalsError {
    GoalsError::Validation(message.into())
}

pub(crate) fn required(text: &str, message: &str) -> Result<()> {
    if text.trim().is_empty() {
        Err(invalid(message))
    } else {
        Ok(())
    }
}
