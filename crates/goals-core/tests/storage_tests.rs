use goals_core::{
    Database, Direction, GoalsError, Measure, NewCheckIn, NewGoal, NewReview, Pace, Progress,
    Quantity, ReviewPolicy, Status,
};
use jiff::civil::{date, Date};

const START: Date = date(2026, 1, 1);
const TARGET: Date = date(2026, 4, 11); // 100 days after START

fn day(n: i64) -> Date {
    START.checked_add(jiff::Span::new().days(n)).unwrap()
}

fn q(s: &str) -> Quantity {
    s.parse().unwrap()
}

fn new_goal(measure: Measure) -> NewGoal {
    NewGoal {
        title: "  Run 100 km  ".into(),
        done_when: "100 km logged".into(),
        why: "".into(),
        start_date: START,
        target_date: Some(TARGET),
        measure,
    }
}

fn total(target: &str) -> Measure {
    Measure::Total {
        start: Quantity::ZERO,
        target: q(target),
        unit: " km ".into(),
    }
}

fn note(on: Date, text: &str) -> NewCheckIn {
    NewCheckIn {
        on,
        note: text.into(),
        value: None,
        milestone_id: None,
    }
}

fn value(on: Date, v: &str) -> NewCheckIn {
    NewCheckIn {
        value: Some(q(v)),
        ..note(on, "")
    }
}

fn marks(on: Date, milestone: &str) -> NewCheckIn {
    NewCheckIn {
        milestone_id: Some(milestone.into()),
        ..note(on, "")
    }
}

fn validation<T: std::fmt::Debug>(result: Result<T, GoalsError>) -> String {
    match result {
        Err(GoalsError::Validation(message)) => message,
        other => panic!("expected a validation error, got {other:?}"),
    }
}

#[test]
fn goals_of_every_measure_round_trip() {
    let db = Database::open_in_memory().unwrap();
    for measure in [
        Measure::Milestones,
        total("100"),
        Measure::Level {
            start: q("90.5"),
            target: q("80"),
            unit: "kg".into(),
            better: Direction::Down,
        },
        Measure::Done,
    ] {
        let created = db.create_goal(&new_goal(measure.clone())).unwrap();
        assert_eq!(created.title, "Run 100 km");
        assert_eq!(created.status, Status::Active);
        assert_eq!(created.start_date, START);
        assert_eq!(created.target_date, Some(TARGET));
        let expected = match measure {
            Measure::Total { start, target, .. } => Measure::Total {
                start,
                target,
                unit: "km".into(),
            },
            other => other,
        };
        assert_eq!(created.measure, expected);
        assert_eq!(db.goal(&created.id).unwrap(), created);
    }
    assert_eq!(db.goals().unwrap().len(), 4);
}

#[test]
fn a_goal_must_say_what_done_looks_like_and_have_a_sensible_measure() {
    let db = Database::open_in_memory().unwrap();
    let mut g = new_goal(Measure::Done);
    g.title = " ".into();
    validation(db.create_goal(&g));
    let mut g = new_goal(Measure::Done);
    g.done_when = "".into();
    validation(db.create_goal(&g));
    let mut g = new_goal(Measure::Done);
    g.target_date = Some(day(-1));
    validation(db.create_goal(&g));
    validation(db.create_goal(&new_goal(total("0"))));
    validation(db.create_goal(&new_goal(Measure::Level {
        start: q("80"),
        target: q("90"),
        unit: "kg".into(),
        better: Direction::Down,
    })));
    // Holding a level is a goal.
    db.create_goal(&new_goal(Measure::Level {
        start: q("80"),
        target: q("80"),
        unit: "kg".into(),
        better: Direction::Down,
    }))
    .unwrap();
    assert_eq!(db.goals().unwrap().len(), 1);
}

#[test]
fn goals_list_active_first_then_by_target_date() {
    let db = Database::open_in_memory().unwrap();
    let mut undated = new_goal(Measure::Done);
    undated.target_date = None;
    let undated = db.create_goal(&undated).unwrap();
    let mut later = new_goal(Measure::Done);
    later.target_date = Some(day(200));
    let later = db.create_goal(&later).unwrap();
    let soon = db.create_goal(&new_goal(Measure::Done)).unwrap();
    let done = db.create_goal(&new_goal(Measure::Done)).unwrap();
    db.set_status(&done.id, &Status::Achieved { on: day(3) })
        .unwrap();
    let ids: Vec<_> = db.goals().unwrap().into_iter().map(|g| g.id).collect();
    assert_eq!(ids, [soon.id, later.id, undated.id, done.id]);
}

#[test]
fn status_changes_record_their_date_and_an_abandoned_goal_its_reason() {
    let db = Database::open_in_memory().unwrap();
    let g = db.create_goal(&new_goal(Measure::Done)).unwrap();
    let achieved = db
        .set_status(&g.id, &Status::Achieved { on: day(9) })
        .unwrap();
    assert_eq!(achieved.status, Status::Achieved { on: day(9) });
    validation(db.set_status(
        &g.id,
        &Status::Abandoned {
            on: day(10),
            reason: "  ".into(),
        },
    ));
    let abandoned = db
        .set_status(
            &g.id,
            &Status::Abandoned {
                on: day(10),
                reason: " moved away ".into(),
            },
        )
        .unwrap();
    assert_eq!(
        abandoned.status,
        Status::Abandoned {
            on: day(10),
            reason: "moved away".into()
        }
    );
    assert_eq!(
        db.set_status(&g.id, &Status::Active).unwrap().status,
        Status::Active
    );
    assert!(matches!(
        db.set_status("nope", &Status::Active),
        Err(GoalsError::NotFound(_))
    ));
}

#[test]
fn a_measure_changes_kind_only_while_nothing_is_recorded() {
    let db = Database::open_in_memory().unwrap();
    let g = db.create_goal(&new_goal(total("100"))).unwrap();
    // Same kind, new figures: fine at any time.
    db.add_check_in(&g.id, &value(day(1), "5")).unwrap();
    let edited = db.edit_goal(&g.id, &new_goal(total("200"))).unwrap();
    assert!(matches!(edited.measure, Measure::Total { target, .. } if target == q("200")));
    assert!(validation(db.edit_goal(&g.id, &new_goal(Measure::Done))).contains("kind"));

    let m = db.create_goal(&new_goal(Measure::Milestones)).unwrap();
    db.add_milestone(&m.id, "first", None).unwrap();
    validation(db.edit_goal(&m.id, &new_goal(Measure::Done)));

    let empty = db.create_goal(&new_goal(Measure::Milestones)).unwrap();
    let changed = db.edit_goal(&empty.id, &new_goal(total("10"))).unwrap();
    assert_eq!(changed.measure.kind(), "total");
}

#[test]
fn deleting_a_goal_deletes_what_was_recorded_against_it() {
    let db = Database::open_in_memory().unwrap();
    let g = db.create_goal(&new_goal(Measure::Milestones)).unwrap();
    let m = db.add_milestone(&g.id, "first", None).unwrap();
    let c = db.add_check_in(&g.id, &marks(day(1), &m.id)).unwrap();
    db.add_review(
        &g.id,
        &NewReview {
            on: day(2),
            committed: true,
            on_track: true,
            next_step: String::new(),
        },
    )
    .unwrap();
    db.delete_goal(&g.id).unwrap();
    assert!(matches!(db.goal(&g.id), Err(GoalsError::NotFound(_))));
    assert!(matches!(
        db.delete_check_in(&c.id),
        Err(GoalsError::NotFound(_))
    ));
    assert!(matches!(
        db.delete_milestone(&m.id),
        Err(GoalsError::NotFound(_))
    ));
    assert!(matches!(
        db.delete_goal(&g.id),
        Err(GoalsError::NotFound(_))
    ));
}

#[test]
fn milestones_belong_to_milestones_goals_and_keep_their_order() {
    let db = Database::open_in_memory().unwrap();
    let other = db.create_goal(&new_goal(Measure::Done)).unwrap();
    validation(db.add_milestone(&other.id, "x", None));
    let g = db.create_goal(&new_goal(Measure::Milestones)).unwrap();
    validation(db.add_milestone(&g.id, " ", None));
    let ids: Vec<String> = ["a", "b", "c", "d"]
        .iter()
        .map(|t| db.add_milestone(&g.id, t, None).unwrap().id)
        .collect();
    let titles = || -> Vec<String> {
        db.milestones(&g.id)
            .unwrap()
            .into_iter()
            .map(|m| m.title)
            .collect()
    };
    assert_eq!(titles(), ["a", "b", "c", "d"]);
    db.move_milestone(&ids[3], None).unwrap();
    assert_eq!(titles(), ["d", "a", "b", "c"]);
    db.move_milestone(&ids[0], Some(&ids[2])).unwrap();
    assert_eq!(titles(), ["d", "b", "c", "a"]);
    db.move_milestone(&ids[1], Some(&ids[0])).unwrap();
    assert_eq!(titles(), ["d", "c", "a", "b"]);
    validation(db.move_milestone(&ids[1], Some(&ids[1])));
    let foreign = db.create_goal(&new_goal(Measure::Milestones)).unwrap();
    let foreign = db.add_milestone(&foreign.id, "x", None).unwrap();
    validation(db.move_milestone(&ids[1], Some(&foreign.id)));
    let edited = db.edit_milestone(&ids[2], "c2", Some(day(5))).unwrap();
    assert_eq!((edited.title.as_str(), edited.due), ("c2", Some(day(5))));
}

#[test]
fn moving_between_neighbours_too_close_for_a_midpoint_renumbers_the_list() {
    let db = Database::open_in_memory().unwrap();
    let g = db.create_goal(&new_goal(Measure::Milestones)).unwrap();
    let ids: Vec<String> = ["a", "b", "c"]
        .iter()
        .map(|t| db.add_milestone(&g.id, t, None).unwrap().id)
        .collect();
    // Halving the gap between "a" and the slot after it, again and again,
    // eventually runs out of room; every move must still land in place.
    for i in 0..80 {
        let mover = if i % 2 == 0 { &ids[2] } else { &ids[1] };
        db.move_milestone(mover, Some(&ids[0])).unwrap();
        let order: Vec<String> = db
            .milestones(&g.id)
            .unwrap()
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(order[1], *mover, "move {i}");
        assert_eq!(order[0], ids[0], "move {i}");
    }
}

#[test]
fn a_check_in_marks_a_milestone_reached_on_its_date() {
    let db = Database::open_in_memory().unwrap();
    let g = db.create_goal(&new_goal(Measure::Milestones)).unwrap();
    let a = db.add_milestone(&g.id, "a", Some(day(10))).unwrap();
    let b = db.add_milestone(&g.id, "b", None).unwrap();
    assert_eq!(a.reached_on, None);
    let c = db.add_check_in(&g.id, &marks(day(4), &a.id)).unwrap();
    assert_eq!(db.milestones(&g.id).unwrap()[0].reached_on, Some(day(4)));
    let again = validation(db.add_check_in(&g.id, &marks(day(5), &a.id)));
    assert!(again.contains("already"));
    // Editing the check-in that marks it may keep marking it.
    db.edit_check_in(&c.id, &marks(day(6), &a.id)).unwrap();
    assert_eq!(db.milestones(&g.id).unwrap()[0].reached_on, Some(day(6)));
    // Moving the mark to another milestone frees the first.
    db.edit_check_in(&c.id, &marks(day(6), &b.id)).unwrap();
    let ms = db.milestones(&g.id).unwrap();
    assert_eq!((ms[0].reached_on, ms[1].reached_on), (None, Some(day(6))));
    // Deleting the milestone keeps the check-in and its note.
    db.edit_check_in(
        &c.id,
        &NewCheckIn {
            note: "passed".into(),
            ..marks(day(6), &b.id)
        },
    )
    .unwrap();
    db.delete_milestone(&b.id).unwrap();
    let kept = &db.check_ins(&g.id).unwrap()[0];
    assert_eq!(
        (kept.note.as_str(), kept.milestone_id.clone()),
        ("passed", None)
    );
}

#[test]
fn check_ins_carry_only_what_their_goal_measures() {
    let db = Database::open_in_memory().unwrap();
    let done = db.create_goal(&new_goal(Measure::Done)).unwrap();
    validation(db.add_check_in(&done.id, &value(day(1), "3")));
    validation(db.add_check_in(&done.id, &note(day(1), "  ")));
    db.add_check_in(&done.id, &note(day(1), "booked the course"))
        .unwrap();

    let number = db.create_goal(&new_goal(total("100"))).unwrap();
    db.add_check_in(&number.id, &note(day(1), "rest day"))
        .unwrap();
    let ms = db.create_goal(&new_goal(Measure::Milestones)).unwrap();
    let m = db.add_milestone(&ms.id, "a", None).unwrap();
    assert!(
        validation(db.add_check_in(&number.id, &marks(day(1), &m.id))).contains("another goal")
    );
    validation(db.add_check_in(&ms.id, &value(day(1), "1")));
    assert!(matches!(
        db.add_check_in("nope", &note(day(1), "x")),
        Err(GoalsError::NotFound(_))
    ));
}

#[test]
fn check_ins_are_listed_by_date_and_can_be_edited_and_deleted() {
    let db = Database::open_in_memory().unwrap();
    let g = db.create_goal(&new_goal(total("100"))).unwrap();
    let late = db.add_check_in(&g.id, &value(day(9), "4")).unwrap();
    let early = db.add_check_in(&g.id, &value(day(2), "5.25")).unwrap();
    let listed: Vec<_> = db
        .check_ins(&g.id)
        .unwrap()
        .into_iter()
        .map(|c| c.id)
        .collect();
    assert_eq!(listed, [early.id.clone(), late.id.clone()]);
    let edited = db
        .edit_check_in(
            &early.id,
            &NewCheckIn {
                note: " felt good ".into(),
                ..value(day(3), "6")
            },
        )
        .unwrap();
    assert_eq!(
        (edited.on, edited.note.as_str(), edited.value),
        (day(3), "felt good", Some(q("6")))
    );
    assert_eq!(edited.created_at, early.created_at);
    db.delete_check_in(&late.id).unwrap();
    assert_eq!(db.check_ins(&g.id).unwrap(), [edited]);
    validation(db.edit_check_in(&early.id, &note(day(3), "")));
}

#[test]
fn a_summary_reads_pace_review_and_quiet_on_a_day() {
    let db = Database::open_in_memory().unwrap();
    let policy = ReviewPolicy::default();
    let g = db.create_goal(&new_goal(total("100"))).unwrap();
    db.add_check_in(&g.id, &value(day(1), "10")).unwrap();
    let s = db.summary(&g.id, day(10), &policy).unwrap();
    assert_eq!(s.reading.pace, Some(Pace::OnTrack));
    assert_eq!(
        s.reading.progress,
        Progress::Number {
            current: q("10"),
            reached: false
        }
    );
    assert!(s.review_due);
    assert_eq!(s.last_review, None);
    assert!(!s.quiet);

    db.add_review(
        &g.id,
        &NewReview {
            on: day(10),
            committed: true,
            on_track: true,
            next_step: "long run on Sunday".into(),
        },
    )
    .unwrap();
    // A review dated later than the day read does not count yet.
    db.add_review(
        &g.id,
        &NewReview {
            on: day(40),
            committed: true,
            on_track: false,
            next_step: String::new(),
        },
    )
    .unwrap();
    let s = db.summary(&g.id, day(11), &policy).unwrap();
    assert_eq!(s.reading.pace, Some(Pace::AtRisk));
    assert_eq!(s.last_review, Some(day(10)));
    assert!(!s.review_due);
    let s = db.summary(&g.id, day(24), &policy).unwrap();
    assert!(s.review_due && s.quiet);
    assert_eq!(db.reviews(&g.id).unwrap().len(), 2);

    let done = db.create_goal(&new_goal(Measure::Done)).unwrap();
    db.set_status(&done.id, &Status::Achieved { on: day(2) })
        .unwrap();
    let active: Vec<_> = db
        .active_summaries(day(24), &policy)
        .unwrap()
        .into_iter()
        .map(|s| s.goal.id)
        .collect();
    assert_eq!(active, [g.id]);
}

#[test]
fn the_database_persists_and_refuses_a_newer_schema() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("goals.db");
    let id = {
        let db = Database::open(&path).unwrap();
        assert_eq!(db.schema_version().unwrap(), 1);
        db.create_goal(&new_goal(Measure::Done)).unwrap().id
    };
    let db = Database::open(&path).unwrap();
    assert_eq!(db.goal(&id).unwrap().title, "Run 100 km");
    drop(db);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.pragma_update(None, "user_version", 99).unwrap();
    drop(conn);
    assert!(matches!(Database::open(&path), Err(GoalsError::Damaged(_))));
}

#[test]
fn damaged_rows_are_reported_not_misread() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("goals.db");
    let id = Database::open(&path)
        .unwrap()
        .create_goal(&new_goal(Measure::Done))
        .unwrap()
        .id;
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute("UPDATE goals SET start_date = 'soon' WHERE id = ?1", [&id])
        .unwrap();
    drop(conn);
    let db = Database::open(&path).unwrap();
    assert!(matches!(db.goal(&id), Err(GoalsError::Damaged(_))));
    assert!(matches!(db.goals(), Err(GoalsError::Damaged(_))));
}
