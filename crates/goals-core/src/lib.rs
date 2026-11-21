//! The Goals app's core: goals, how their progress is measured, check-ins,
//! reviews, the pace rules, and SQLite storage. No user interface.
//!
//! A goal is anything the user can reach. It states what done looks like
//! and why it matters, and measures progress in one of three ways
//! ([`Measure`]): milestones, a number (a total or a level), or done or not.
//! Check-ins are dated notes of progress; reviews are the user's regular
//! look at each goal. [`pace`] reads whether an active goal is on track, at
//! risk or late, and [`review`] says when a goal is due a review or has gone
//! quiet.
//!
//! Goals hold outcomes and evidence of progress. There are no tasks or
//! projects here: the work belongs in a task manager such as Matters, whose
//! projects can refer to a goal by its ID.

pub mod domain;
pub mod error;
pub mod export;
pub mod pace;
pub mod quantity;
pub mod review;
pub mod storage;

pub use domain::{
    CheckIn, Direction, Goal, GoalEdit, Measure, Milestone, NewCheckIn, NewGoal, NewReview, Review,
    Status,
};
pub use error::{GoalsError, Result};
pub use export::Export;
pub use pace::{Pace, Progress, Reading};
pub use quantity::Quantity;
pub use review::ReviewPolicy;
pub use storage::{Database, Summary};
