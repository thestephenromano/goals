//! When a goal is due a review, and when it has gone quiet.
//!
//! Both are counted in whole local days and apply only to active goals.
//!
//! - **Review due:** `review_every_days` have passed since the last review,
//!   or since the goal's start date if it has never been reviewed.
//! - **Quiet:** `quiet_after_days` have passed since anything happened: the
//!   last review, the last check-in dated on or before today, or the start
//!   date, whichever is latest.
//!
//! The defaults follow the weekly review of Getting Things Done, which
//! Matters uses for the work towards these goals: a review is due each week,
//! and a goal is flagged quiet after two weeks, a whole missed review cycle
//! beyond the one already due. Both are settings.

use jiff::civil::Date;

use crate::domain::Goal;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReviewPolicy {
    pub review_every_days: u16,
    pub quiet_after_days: u16,
}

impl Default for ReviewPolicy {
    fn default() -> Self {
        ReviewPolicy {
            review_every_days: 7,
            quiet_after_days: 14,
        }
    }
}

fn days_since(from: Date, today: Date) -> i64 {
    i64::from((today - from).get_days())
}

impl ReviewPolicy {
    pub fn review_due(&self, goal: &Goal, last_review: Option<Date>, today: Date) -> bool {
        let since = last_review.unwrap_or(goal.start_date);
        goal.status.is_active() && days_since(since, today) >= i64::from(self.review_every_days)
    }

    /// `last_activity` is from [`last_activity`].
    pub fn quiet(&self, goal: &Goal, last_activity: Date, today: Date) -> bool {
        goal.status.is_active()
            && days_since(last_activity, today) >= i64::from(self.quiet_after_days)
    }
}

/// The latest of the start date, the last review and the last check-in
/// dated on or before today.
pub fn last_activity(
    goal: &Goal,
    last_review: Option<Date>,
    last_check_in: Option<Date>,
    today: Date,
) -> Date {
    [
        Some(goal.start_date),
        last_review,
        last_check_in.filter(|d| *d <= today),
    ]
    .into_iter()
    .flatten()
    .max()
    .unwrap_or(goal.start_date)
}

#[cfg(test)]
mod tests {
    use jiff::civil::date;

    use super::*;
    use crate::domain::{Measure, Status};

    const START: Date = date(2026, 3, 1);

    fn goal() -> Goal {
        Goal {
            id: "g".into(),
            title: "t".into(),
            done_when: "d".into(),
            why: String::new(),
            start_date: START,
            target_date: None,
            measure: Measure::Done,
            status: Status::Active,
            created_at: 0,
            updated_at: 0,
        }
    }

    fn day(n: i64) -> Date {
        START.checked_add(jiff::Span::new().days(n)).unwrap()
    }

    #[test]
    fn a_review_is_due_a_whole_period_after_the_last_or_the_start() {
        let p = ReviewPolicy::default();
        let g = goal();
        assert!(!p.review_due(&g, None, day(6)));
        assert!(p.review_due(&g, None, day(7)));
        assert!(!p.review_due(&g, Some(day(10)), day(16)));
        assert!(p.review_due(&g, Some(day(10)), day(17)));
    }

    #[test]
    fn quiet_counts_from_the_latest_activity_up_to_today() {
        let p = ReviewPolicy::default();
        let g = goal();
        let last = last_activity(&g, Some(day(3)), Some(day(5)), day(5));
        assert_eq!(last, day(5));
        assert!(!p.quiet(&g, last, day(18)));
        assert!(p.quiet(&g, last, day(19)));
        // A check-in dated in the future is not activity yet.
        assert_eq!(last_activity(&g, None, Some(day(30)), day(5)), START);
        // Nor does a backdated review before the start move it back.
        assert_eq!(last_activity(&g, Some(day(-9)), None, day(5)), START);
    }

    #[test]
    fn goals_that_are_not_active_are_never_due_or_quiet() {
        let p = ReviewPolicy::default();
        let mut g = goal();
        g.status = Status::Achieved { on: day(1) };
        assert!(!p.review_due(&g, None, day(100)));
        assert!(!p.quiet(&g, START, day(100)));
    }

    #[test]
    fn the_periods_are_settings() {
        let p = ReviewPolicy {
            review_every_days: 1,
            quiet_after_days: 3,
        };
        let g = goal();
        assert!(p.review_due(&g, None, day(1)));
        assert!(!p.quiet(&g, START, day(2)));
        assert!(p.quiet(&g, START, day(3)));
    }
}
