//! Whether an active goal is on track: the pace rules.
//!
//! Pace is judged in whole local days against a straight line from the
//! goal's start (its start date, at its starting value) to its target (its
//! target date, at its target value). There are no tolerances: a goal on or
//! ahead of the line is on track, one behind it is at risk, and one that has
//! not reached its target once the target date has passed is late. Every
//! comparison is exact, in integers (see [`Quantity`]).
//!
//! The line, for a goal with `span` days from start to target date, on a day
//! `elapsed` days after the start (clamped to `0..=span`), expects
//! `distance * elapsed / span` of the way done. So:
//!
//! - **before or on the start date** nothing is expected yet;
//! - **on the target date** the whole distance is expected, and a goal short
//!   of it is at risk, not yet late;
//! - **the day after the target date** a goal short of it is late;
//! - **a target date on the start date** (`span` = 0) expects everything
//!   from the start.
//!
//! By measure:
//!
//! - **A total:** the current value is the start plus every check-in's value
//!   dated on or before today.
//! - **A level:** the current value is the latest check-in's value dated on
//!   or before today (by date, then by when it was logged), or the start.
//!   Progress counts in the direction that is better.
//! - **Milestones:** a milestone is reached once a check-in dated on or
//!   before today marks it. An unreached milestone past its own date makes
//!   the goal late. Otherwise the count reached is held to the line. A goal
//!   without a target date is on track if any milestone has a date (it has a
//!   schedule, and is keeping it) and unpaced if none does. A goal with no
//!   milestones is unpaced until its target date passes.
//! - **Done or not:** late once the target date has passed, otherwise on
//!   track (unpaced without a target date). There is no line to fall behind.
//!
//! A number goal that has reached its target is on track. Goals without a
//! target date are unpaced, except as above. Only active goals have a pace.

use jiff::civil::Date;

use crate::domain::{CheckIn, Goal, Measure, Milestone};
use crate::quantity::Quantity;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pace {
    OnTrack,
    AtRisk,
    Late,
    /// Nothing to judge pace against (no target date, or no milestones).
    Unpaced,
}

/// How far a goal has come, as of a day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Progress {
    Milestones {
        reached: usize,
        total: usize,
    },
    Number {
        current: Quantity,
        /// Current is at or past the target, in the better direction.
        reached: bool,
    },
    /// Done-or-not goals: done is the goal's status.
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reading {
    pub progress: Progress,
    /// `None` for a goal that is not active.
    pub pace: Option<Pace>,
}

/// Read a goal's progress and pace on `today`. `milestones` and `check_ins`
/// are the goal's own; others are ignored.
pub fn read(goal: &Goal, milestones: &[Milestone], check_ins: &[CheckIn], today: Date) -> Reading {
    let check_ins: Vec<&CheckIn> = check_ins
        .iter()
        .filter(|c| c.goal_id == goal.id && c.on <= today)
        .collect();
    let line = Line::new(goal, today);
    let (progress, pace) = match &goal.measure {
        Measure::Total { start, target, .. } => {
            let sum: i128 = check_ins
                .iter()
                .filter_map(|c| c.value)
                .map(|v| i128::from(v.units()))
                .sum();
            number(i128::from(start.units()) + sum, *start, *target, 1, &line)
        }
        Measure::Level {
            start,
            target,
            better,
            ..
        } => {
            let latest = check_ins
                .iter()
                .filter(|c| c.value.is_some())
                .max_by(|a, b| (a.on, a.created_at, &a.id).cmp(&(b.on, b.created_at, &b.id)))
                .and_then(|c| c.value)
                .unwrap_or(*start);
            number(
                i128::from(latest.units()),
                *start,
                *target,
                better.sign(),
                &line,
            )
        }
        Measure::Milestones => {
            let own: Vec<&Milestone> = milestones.iter().filter(|m| m.goal_id == goal.id).collect();
            let is_reached = |m: &Milestone| {
                check_ins
                    .iter()
                    .any(|c| c.milestone_id.as_deref() == Some(m.id.as_str()))
            };
            let total = own.len();
            let reached = own.iter().filter(|m| is_reached(m)).count();
            let overdue = own
                .iter()
                .any(|m| !is_reached(m) && m.due.is_some_and(|due| due < today));
            let pace = if total > 0 && reached == total {
                Pace::OnTrack
            } else if overdue || line.passed() {
                Pace::Late
            } else if total == 0 {
                Pace::Unpaced
            } else if line.span.is_none() {
                if own.iter().any(|m| m.due.is_some()) {
                    Pace::OnTrack
                } else {
                    Pace::Unpaced
                }
            } else {
                line.judge(reached as i128, total as i128)
            };
            (Progress::Milestones { reached, total }, pace)
        }
        Measure::Done => {
            let pace = if line.passed() {
                Pace::Late
            } else if line.span.is_none() {
                Pace::Unpaced
            } else {
                Pace::OnTrack
            };
            (Progress::Done, pace)
        }
    };
    Reading {
        progress,
        pace: goal.status.is_active().then_some(pace),
    }
}

fn number(
    current: i128,
    start: Quantity,
    target: Quantity,
    sign: i128,
    line: &Line,
) -> (Progress, Pace) {
    let progress = sign * (current - i128::from(start.units()));
    let distance = sign * (i128::from(target.units()) - i128::from(start.units()));
    let reached = progress >= distance;
    let pace = if reached {
        Pace::OnTrack
    } else if line.span.is_none() {
        Pace::Unpaced
    } else if line.passed() {
        Pace::Late
    } else {
        line.judge(progress, distance)
    };
    // `current` is a sum of i64 amounts; a total outside i64 can only come
    // from more than 2^63 / 10^19 ≈ 1 check-in at the extremes, so clamp
    // for display rather than wrap.
    let current = Quantity::from_units(current.clamp(i64::MIN.into(), i64::MAX.into()) as i64);
    (Progress::Number { current, reached }, pace)
}

/// The straight line from start to target, on one day.
struct Line {
    /// Days from start date to target date; `None` without a target date.
    span: Option<i64>,
    /// Days from the start date to today, clamped to `0..=span`.
    elapsed: i64,
    /// Today is after the target date.
    passed: bool,
}

impl Line {
    fn new(goal: &Goal, today: Date) -> Self {
        let days = |from: Date, to: Date| i64::from((to - from).get_days());
        match goal.target_date {
            None => Line {
                span: None,
                elapsed: 0,
                passed: false,
            },
            Some(target) => {
                let span = days(goal.start_date, target).max(0);
                Line {
                    span: Some(span),
                    elapsed: days(goal.start_date, today).clamp(0, span),
                    passed: today > target,
                }
            }
        }
    }

    fn passed(&self) -> bool {
        self.passed
    }

    /// On track when `progress` is at least the line's share of `distance`:
    /// `progress / distance >= elapsed / span`, cross-multiplied.
    fn judge(&self, progress: i128, distance: i128) -> Pace {
        let span = self.span.unwrap_or(0);
        let on_line = if span == 0 {
            progress >= distance
        } else {
            // Saturation keeps the comparison's direction; the products
            // only approach i128's range for absurd sums.
            progress.saturating_mul(span.into()) >= distance.saturating_mul(self.elapsed.into())
        };
        if on_line {
            Pace::OnTrack
        } else {
            Pace::AtRisk
        }
    }
}

#[cfg(test)]
mod tests {
    use jiff::civil::date;

    use super::*;
    use crate::domain::{Direction, Status};

    const START: Date = date(2026, 1, 1);
    /// 100 days after START.
    const TARGET: Date = date(2026, 4, 11);

    fn q(s: &str) -> Quantity {
        s.parse().unwrap()
    }

    fn goal(measure: Measure, target_date: Option<Date>) -> Goal {
        Goal {
            id: "g".into(),
            title: "t".into(),
            done_when: "d".into(),
            why: String::new(),
            start_date: START,
            target_date,
            measure,
            status: Status::Active,
            created_at: 0,
            updated_at: 0,
        }
    }

    fn total(target: &str) -> Measure {
        Measure::Total {
            start: Quantity::ZERO,
            target: q(target),
            unit: "km".into(),
        }
    }

    fn level(start: &str, target: &str, better: Direction) -> Measure {
        Measure::Level {
            start: q(start),
            target: q(target),
            unit: "kg".into(),
            better,
        }
    }

    fn check_in(on: Date, value: Option<&str>, logged: i64) -> CheckIn {
        CheckIn {
            id: format!("c{logged}"),
            goal_id: "g".into(),
            on,
            note: String::new(),
            value: value.map(q),
            milestone_id: None,
            created_at: logged,
            updated_at: logged,
        }
    }

    fn marks(milestone: &str, on: Date) -> CheckIn {
        CheckIn {
            milestone_id: Some(milestone.into()),
            ..check_in(on, None, 0)
        }
    }

    fn milestone(id: &str, due: Option<Date>) -> Milestone {
        Milestone {
            id: id.into(),
            goal_id: "g".into(),
            title: id.into(),
            due,
            reached_on: None,
            sort_order: 0.0,
        }
    }

    fn day(n: i64) -> Date {
        START.checked_add(jiff::Span::new().days(n)).unwrap()
    }

    fn opposite(better: Direction) -> Direction {
        match better {
            Direction::Up => Direction::Down,
            Direction::Down => Direction::Up,
        }
    }

    fn pace(g: &Goal, m: &[Milestone], c: &[CheckIn], today: Date) -> Pace {
        read(g, m, c, today).pace.unwrap()
    }

    #[test]
    fn target_is_100_days_after_start() {
        assert_eq!(day(100), TARGET);
    }

    #[test]
    fn a_total_exactly_on_the_line_is_on_track_and_one_unit_short_is_at_risk() {
        let g = goal(total("100"), Some(TARGET));
        // Day 37 expects exactly 37 km.
        let on = [check_in(day(3), Some("37"), 1)];
        assert_eq!(pace(&g, &[], &on, day(37)), Pace::OnTrack);
        let short = [check_in(day(3), Some("36.9999"), 1)];
        assert_eq!(pace(&g, &[], &short, day(37)), Pace::AtRisk);
    }

    #[test]
    fn nothing_is_expected_before_or_on_the_start_date() {
        let g = goal(total("100"), Some(TARGET));
        assert_eq!(pace(&g, &[], &[], day(-5)), Pace::OnTrack);
        assert_eq!(pace(&g, &[], &[], START), Pace::OnTrack);
        assert_eq!(pace(&g, &[], &[], day(1)), Pace::AtRisk);
    }

    #[test]
    fn short_on_the_target_date_is_at_risk_and_late_the_day_after() {
        let g = goal(total("100"), Some(TARGET));
        let c = [check_in(day(1), Some("99"), 1)];
        assert_eq!(pace(&g, &[], &c, TARGET), Pace::AtRisk);
        assert_eq!(pace(&g, &[], &c, day(101)), Pace::Late);
        let done = [check_in(day(1), Some("100"), 1)];
        assert_eq!(pace(&g, &[], &done, TARGET), Pace::OnTrack);
        assert_eq!(pace(&g, &[], &done, day(400)), Pace::OnTrack);
    }

    #[test]
    fn a_target_date_on_the_start_date_expects_everything_at_once() {
        let g = goal(total("10"), Some(START));
        assert_eq!(pace(&g, &[], &[], day(-1)), Pace::AtRisk);
        assert_eq!(pace(&g, &[], &[], START), Pace::AtRisk);
        assert_eq!(pace(&g, &[], &[], day(1)), Pace::Late);
        let c = [check_in(START, Some("10"), 1)];
        assert_eq!(pace(&g, &[], &c, START), Pace::OnTrack);
    }

    #[test]
    fn a_total_counts_its_start_and_only_check_ins_up_to_today() {
        let g = goal(
            Measure::Total {
                start: q("20"),
                target: q("120"),
                unit: "km".into(),
            },
            Some(TARGET),
        );
        let c = [
            check_in(day(1), Some("30"), 1),
            check_in(day(2), None, 2),
            check_in(day(60), Some("70"), 3),
        ];
        let r = read(&g, &[], &c, day(30));
        assert_eq!(
            r.progress,
            Progress::Number {
                current: q("50"),
                reached: false
            }
        );
        assert_eq!(r.pace, Some(Pace::OnTrack));
        assert_eq!(pace(&g, &[], &c, day(31)), Pace::AtRisk);
        let r = read(&g, &[], &c, day(60));
        assert_eq!(
            r.progress,
            Progress::Number {
                current: q("120"),
                reached: true
            }
        );
    }

    #[test]
    fn a_level_follows_the_latest_check_in_by_date_then_by_when_it_was_logged() {
        let g = goal(level("90", "80", Direction::Down), Some(TARGET));
        let c = [
            // Logged later but dated earlier: superseded.
            check_in(day(5), Some("95"), 9),
            check_in(day(10), Some("88"), 1),
            check_in(day(10), Some("89"), 2),
        ];
        let r = read(&g, &[], &c, day(20));
        assert_eq!(
            r.progress,
            Progress::Number {
                current: q("89"),
                reached: false
            }
        );
        // Day 10 expects 1 kg off: 89 is exactly on the line.
        assert_eq!(pace(&g, &[], &c, day(10)), Pace::OnTrack);
        assert_eq!(pace(&g, &[], &c, day(11)), Pace::AtRisk);
    }

    #[test]
    fn a_level_counts_in_its_better_direction() {
        for better in [Direction::Up, Direction::Down] {
            let (start, target, good, bad) = match better {
                Direction::Up => ("100", "140", "130", "99"),
                Direction::Down => ("140", "100", "110", "141"),
            };
            let g = goal(level(start, target, better), Some(TARGET));
            let c = [check_in(day(1), Some(good), 1)];
            assert_eq!(pace(&g, &[], &c, day(50)), Pace::OnTrack, "{better:?}");
            let c = [check_in(day(1), Some(bad), 1)];
            assert_eq!(pace(&g, &[], &c, day(1)), Pace::AtRisk, "{better:?}");
            // The same figures read the other way round are behind.
            let flipped = goal(level(target, start, opposite(better)), Some(TARGET));
            let c = [check_in(day(1), Some(target), 1)];
            assert_eq!(pace(&flipped, &[], &c, day(50)), Pace::AtRisk);
        }
    }

    #[test]
    fn a_level_that_falls_back_after_reaching_its_target_is_no_longer_reached() {
        let g = goal(level("90", "80", Direction::Down), Some(TARGET));
        let c = [
            check_in(day(10), Some("80"), 1),
            check_in(day(20), Some("85"), 2),
        ];
        assert_eq!(pace(&g, &[], &c, day(15)), Pace::OnTrack);
        let r = read(&g, &[], &c, day(20));
        assert!(matches!(
            r.progress,
            Progress::Number { reached: false, .. }
        ));
        assert_eq!(r.pace, Some(Pace::OnTrack));
        assert_eq!(pace(&g, &[], &c, day(51)), Pace::AtRisk);
    }

    #[test]
    fn holding_a_level_is_reached_while_at_or_better_than_the_start() {
        let g = goal(level("75", "75", Direction::Down), Some(TARGET));
        assert_eq!(pace(&g, &[], &[], day(50)), Pace::OnTrack);
        let c = [check_in(day(10), Some("75.1"), 1)];
        assert_eq!(pace(&g, &[], &c, day(50)), Pace::AtRisk);
        assert_eq!(pace(&g, &[], &c, day(101)), Pace::Late);
    }

    #[test]
    fn number_goals_without_a_target_date_are_unpaced_until_reached() {
        let g = goal(total("10"), None);
        assert_eq!(pace(&g, &[], &[], day(5000)), Pace::Unpaced);
        let c = [check_in(day(1), Some("10"), 1)];
        assert_eq!(pace(&g, &[], &c, day(5000)), Pace::OnTrack);
    }

    #[test]
    fn milestones_are_held_to_the_line_by_count() {
        let g = goal(Measure::Milestones, Some(TARGET));
        let m: Vec<_> = ["a", "b", "c", "d"]
            .iter()
            .map(|id| milestone(id, None))
            .collect();
        // Four milestones over 100 days: one expected by day 25.
        let one = [marks("a", day(3))];
        assert_eq!(pace(&g, &m, &one, day(25)), Pace::OnTrack);
        assert_eq!(pace(&g, &m, &one, day(26)), Pace::AtRisk);
        assert_eq!(
            read(&g, &m, &one, day(26)).progress,
            Progress::Milestones {
                reached: 1,
                total: 4
            }
        );
        // A check-in dated after today does not count yet.
        let later = [marks("a", day(30))];
        assert_eq!(pace(&g, &m, &later, day(25)), Pace::AtRisk);
    }

    #[test]
    fn a_milestone_past_its_own_date_makes_the_goal_late_the_day_after() {
        let g = goal(Measure::Milestones, Some(TARGET));
        let m = [milestone("a", Some(day(10))), milestone("b", None)];
        assert_eq!(pace(&g, &m, &[], day(10)), Pace::AtRisk);
        assert_eq!(pace(&g, &m, &[], day(11)), Pace::Late);
        let c = [marks("a", day(11))];
        assert_eq!(pace(&g, &m, &c, day(11)), Pace::OnTrack);
    }

    #[test]
    fn milestones_short_after_the_target_date_are_late_and_all_reached_is_on_track() {
        let g = goal(Measure::Milestones, Some(TARGET));
        let m = [milestone("a", None), milestone("b", None)];
        let one = [marks("a", day(1))];
        assert_eq!(pace(&g, &m, &one, TARGET), Pace::AtRisk);
        assert_eq!(pace(&g, &m, &one, day(101)), Pace::Late);
        let both = [marks("a", day(1)), marks("b", day(2))];
        assert_eq!(pace(&g, &m, &both, day(500)), Pace::OnTrack);
    }

    #[test]
    fn milestones_without_a_target_date_are_paced_only_by_their_own_dates() {
        let g = goal(Measure::Milestones, None);
        let undated = [milestone("a", None)];
        assert_eq!(pace(&g, &undated, &[], day(900)), Pace::Unpaced);
        let dated = [milestone("a", Some(day(10))), milestone("b", None)];
        assert_eq!(pace(&g, &dated, &[], day(10)), Pace::OnTrack);
        assert_eq!(pace(&g, &dated, &[], day(11)), Pace::Late);
    }

    #[test]
    fn a_goal_with_no_milestones_is_unpaced_until_its_target_date_passes() {
        let g = goal(Measure::Milestones, Some(TARGET));
        assert_eq!(pace(&g, &[], &[], day(50)), Pace::Unpaced);
        assert_eq!(pace(&g, &[], &[], day(101)), Pace::Late);
    }

    #[test]
    fn other_goals_milestones_and_check_ins_are_ignored() {
        let g = goal(Measure::Milestones, Some(TARGET));
        let mut foreign = milestone("x", Some(day(1)));
        foreign.goal_id = "other".into();
        let mine = milestone("a", None);
        let mut foreign_check_in = marks("a", day(1));
        foreign_check_in.goal_id = "other".into();
        let r = read(&g, &[mine, foreign], &[foreign_check_in], day(20));
        assert_eq!(
            r.progress,
            Progress::Milestones {
                reached: 0,
                total: 1
            }
        );
        assert_eq!(r.pace, Some(Pace::AtRisk));
    }

    #[test]
    fn done_or_not_is_late_only_after_its_target_date() {
        let g = goal(Measure::Done, Some(TARGET));
        assert_eq!(pace(&g, &[], &[], TARGET), Pace::OnTrack);
        assert_eq!(pace(&g, &[], &[], day(101)), Pace::Late);
        assert_eq!(
            pace(&goal(Measure::Done, None), &[], &[], day(900)),
            Pace::Unpaced
        );
    }

    #[test]
    fn only_active_goals_have_a_pace() {
        let mut g = goal(total("10"), Some(TARGET));
        g.status = Status::Achieved { on: day(5) };
        let r = read(&g, &[], &[], day(200));
        assert_eq!(r.pace, None);
        g.status = Status::Abandoned {
            on: day(5),
            reason: "r".into(),
        };
        assert_eq!(read(&g, &[], &[], day(200)).pace, None);
    }

    #[test]
    fn extreme_amounts_compare_without_overflow() {
        let max = Quantity::from_units(i64::MAX);
        let g = goal(
            Measure::Total {
                start: Quantity::from_units(i64::MIN),
                target: max,
                unit: String::new(),
            },
            Some(date(9999, 12, 31)),
        );
        let c: Vec<_> = (0..4)
            .map(|i| CheckIn {
                value: Some(max),
                ..check_in(START, None, i)
            })
            .collect();
        let r = read(&g, &[], &c, day(1));
        assert_eq!(r.pace, Some(Pace::OnTrack));
        assert!(matches!(
            r.progress,
            Progress::Number { current, reached: true } if current == max
        ));
    }
}
