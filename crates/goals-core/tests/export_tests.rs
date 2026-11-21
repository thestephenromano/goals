use goals_core::export::{ExportedMeasure, ExportedStatus, CSV_FILES, FORMAT, VERSION};
use goals_core::{
    Database, Direction, Export, GoalsError, Measure, NewCheckIn, NewGoal, NewReview, Quantity,
    Status,
};
use jiff::civil::{date, Date};

const START: Date = date(2026, 1, 1);

fn q(s: &str) -> Quantity {
    s.parse().unwrap()
}

fn goal(title: &str, measure: Measure) -> NewGoal {
    NewGoal {
        title: title.into(),
        done_when: "it's done, \"really\"".into(),
        why: "line one\nline two, with a comma".into(),
        start_date: START,
        target_date: Some(date(2026, 6, 30)),
        measure,
    }
}

/// One goal of every measure and status, with something recorded on each.
fn populated() -> Database {
    let db = Database::open_in_memory().unwrap();
    let run = db
        .create_goal(&goal(
            "Run",
            Measure::Total {
                start: Quantity::ZERO,
                target: q("500"),
                unit: "km".into(),
            },
        ))
        .unwrap();
    for (on, v) in [(date(2026, 1, 2), "5.25"), (date(2026, 1, 3), "0.0001")] {
        db.add_check_in(
            &run.id,
            &NewCheckIn {
                on,
                note: "=SUM(A1)".into(),
                value: Some(q(v)),
                milestone_id: None,
            },
        )
        .unwrap();
    }
    db.add_review(
        &run.id,
        &NewReview {
            on: date(2026, 1, 8),
            committed: true,
            on_track: false,
            next_step: "two runs, then rest".into(),
        },
    )
    .unwrap();
    let weight = db
        .create_goal(&goal(
            "Weight",
            Measure::Level {
                start: q("90"),
                target: q("-1.5"),
                unit: "kg".into(),
                better: Direction::Down,
            },
        ))
        .unwrap();
    db.set_status(
        &weight.id,
        &Status::Abandoned {
            on: date(2026, 2, 1),
            reason: "changed focus".into(),
        },
    )
    .unwrap();
    let exam = db.create_goal(&goal("B1", Measure::Milestones)).unwrap();
    let first = db.add_milestone(&exam.id, "A2 passed", None).unwrap();
    let second = db
        .add_milestone(&exam.id, "B1 passed", Some(date(2026, 6, 1)))
        .unwrap();
    db.move_milestone(&second.id, None).unwrap();
    db.add_check_in(
        &exam.id,
        &NewCheckIn {
            on: date(2026, 3, 1),
            note: String::new(),
            value: None,
            milestone_id: Some(first.id.clone()),
        },
    )
    .unwrap();
    let book = db.create_goal(&goal("Book", Measure::Done)).unwrap();
    db.set_status(
        &book.id,
        &Status::Achieved {
            on: date(2026, 5, 5),
        },
    )
    .unwrap();
    db
}

#[test]
fn json_round_trips_every_measure_and_status_exactly() {
    let db = populated();
    let export = db.export().unwrap();
    assert_eq!((export.format.as_str(), export.version), (FORMAT, VERSION));
    let mut bytes = Vec::new();
    export.write_json(&mut bytes).unwrap();
    let back = Export::from_json(bytes.as_slice()).unwrap();
    assert_eq!(back, export);

    let by_title = |t: &str| export.goals.iter().find(|g| g.title == t).unwrap();
    let run = by_title("Run");
    assert_eq!(
        run.measure,
        ExportedMeasure::Total {
            start: "0".into(),
            target: "500".into(),
            unit: "km".into()
        }
    );
    let values: Vec<_> = run.check_ins.iter().map(|c| c.value.clone()).collect();
    assert_eq!(values, [Some("5.25".into()), Some("0.0001".into())]);
    assert_eq!(run.reviews[0].next_step, "two runs, then rest");
    assert!(run.created_at.ends_with('Z'), "{}", run.created_at);

    let weight = by_title("Weight");
    assert!(
        matches!(&weight.measure, ExportedMeasure::Level { target, better, .. } if target == "-1.5" && better == "down")
    );
    assert_eq!(
        weight.status,
        ExportedStatus::Abandoned {
            on: "2026-02-01".into(),
            reason: "changed focus".into()
        }
    );

    let exam = by_title("B1");
    let titles: Vec<_> = exam
        .milestones
        .iter()
        .map(|m| (m.position, m.title.as_str(), m.reached_on.as_deref()))
        .collect();
    assert_eq!(
        titles,
        [(1, "B1 passed", None), (2, "A2 passed", Some("2026-03-01"))]
    );
    assert_eq!(
        by_title("Book").status,
        ExportedStatus::Achieved {
            on: "2026-05-05".into()
        }
    );

    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["goals"][0]["measure"]["kind"], "total");
    assert_eq!(json["goals"][0]["status"]["state"], "active");
}

#[test]
fn reading_back_refuses_other_formats_and_newer_versions() {
    let export = populated().export().unwrap();
    let mut other = export.clone();
    other.format = "something-else".into();
    let mut newer = export;
    newer.version = VERSION + 1;
    for bad in [other, newer] {
        let mut bytes = Vec::new();
        bad.write_json(&mut bytes).unwrap();
        assert!(matches!(
            Export::from_json(bytes.as_slice()),
            Err(GoalsError::Validation(_))
        ));
    }
    assert!(matches!(
        Export::from_json(&b"{not json"[..]),
        Err(GoalsError::Validation(_))
    ));
}

fn read_csv(path: &std::path::Path) -> (Vec<String>, Vec<Vec<String>>) {
    let mut reader = csv::Reader::from_path(path).unwrap();
    let header = reader.headers().unwrap().iter().map(String::from).collect();
    let rows = reader
        .records()
        .map(|r| r.unwrap().iter().map(String::from).collect())
        .collect();
    (header, rows)
}

#[test]
fn csv_files_join_by_id_and_keep_text_that_needs_quoting() {
    let db = populated();
    let export = db.export().unwrap();
    let dir = tempfile::tempdir().unwrap();
    export.write_csv(dir.path()).unwrap();
    for name in CSV_FILES {
        assert!(dir.path().join(name).is_file(), "{name}");
    }
    let (header, goals) = read_csv(&dir.path().join("goals.csv"));
    assert_eq!(goals.len(), 4);
    let col = |name: &str| header.iter().position(|h| h == name).unwrap();
    let run = goals.iter().find(|g| g[col("title")] == "Run").unwrap();
    assert_eq!(run[col("done_when")], "it's done, \"really\"");
    assert_eq!(run[col("why")], "line one\nline two, with a comma");
    assert_eq!(
        (
            &*run[col("measure")],
            &*run[col("target")],
            &*run[col("unit")]
        ),
        ("total", "500", "km")
    );
    let weight = goals.iter().find(|g| g[col("title")] == "Weight").unwrap();
    assert_eq!(
        (
            &*weight[col("status")],
            &*weight[col("abandoned_reason")],
            &*weight[col("better")]
        ),
        ("abandoned", "changed focus", "down")
    );

    let (header, check_ins) = read_csv(&dir.path().join("check_ins.csv"));
    let c = |name: &str| header.iter().position(|h| h == name).unwrap();
    let run_check_ins: Vec<_> = check_ins
        .iter()
        .filter(|r| r[c("goal_id")] == run[col("id")])
        .collect();
    assert_eq!(run_check_ins.len(), 2);
    assert_eq!(run_check_ins[0][c("value")], "5.25");
    assert_eq!(run_check_ins[0][c("note")], "=SUM(A1)");

    let (_, milestones) = read_csv(&dir.path().join("milestones.csv"));
    let (_, reviews) = read_csv(&dir.path().join("reviews.csv"));
    assert_eq!((milestones.len(), reviews.len()), (2, 1));
    // A milestone reached by a check-in names it.
    let marked: Vec<_> = check_ins
        .iter()
        .filter(|r| !r[c("milestone_id")].is_empty())
        .collect();
    assert_eq!(marked.len(), 1);
    assert!(milestones
        .iter()
        .any(|m| m[0] == marked[0][c("milestone_id")]));
}

#[test]
fn an_empty_database_exports_headers_only() {
    let db = Database::open_in_memory().unwrap();
    let export = db.export().unwrap();
    assert!(export.goals.is_empty());
    let dir = tempfile::tempdir().unwrap();
    export.write_csv(dir.path()).unwrap();
    let (header, rows) = read_csv(&dir.path().join("reviews.csv"));
    assert_eq!(header[0], "id");
    assert!(rows.is_empty());
}

#[test]
fn writing_csv_into_a_missing_directory_is_an_io_error() {
    let export = Database::open_in_memory().unwrap().export().unwrap();
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        export.write_csv(&dir.path().join("missing")),
        Err(GoalsError::Io(_))
    ));
}
