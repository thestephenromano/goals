//! Milestones, check-ins and reviews.

use jiff::civil::Date;
use rusqlite::{params, Connection, OptionalExtension, Row};

use super::db::{date, goal_in, new_id, not_found, now, optional_date, Database};
use crate::domain::{
    invalid, required, CheckIn, Goal, Measure, Milestone, NewCheckIn, NewReview, Review,
};
use crate::error::Result;
use crate::quantity::Quantity;

const MILESTONE_COLUMNS: &str = "m.id, m.goal_id, m.title, m.due, m.sort_order,
     (SELECT MIN(c.on_date) FROM check_ins c WHERE c.milestone_id = m.id)";

const CHECK_IN_COLUMNS: &str =
    "id, goal_id, on_date, note, value, milestone_id, created_at, updated_at";

const REVIEW_COLUMNS: &str = "id, goal_id, on_date, committed, on_track, next_step, created_at";

/// Smallest gap between neighbouring order keys that still takes a midpoint;
/// closer neighbours renumber the list.
const MIN_GAP: f64 = 1e-9;

impl Database {
    /// Append a milestone to a milestones goal.
    pub fn add_milestone(
        &self,
        goal_id: &str,
        title: &str,
        due: Option<Date>,
    ) -> Result<Milestone> {
        required(title, "a milestone needs a title")?;
        let conn = self.conn();
        let goal = goal_in(&conn, goal_id)?;
        if goal.measure != Measure::Milestones {
            return Err(invalid("only a milestones goal has milestones"));
        }
        let last: Option<f64> = conn.query_row(
            "SELECT MAX(sort_order) FROM milestones WHERE goal_id = ?1",
            [goal_id],
            |r| r.get(0),
        )?;
        let id = new_id();
        let now = now();
        conn.execute(
            "INSERT INTO milestones (id, goal_id, title, due, sort_order, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![
                id,
                goal_id,
                title.trim(),
                due.map(|d| d.to_string()),
                last.map_or(1.0, |l| l.floor() + 1.0),
                now
            ],
        )?;
        milestone_in(&conn, &id)
    }

    pub fn edit_milestone(&self, id: &str, title: &str, due: Option<Date>) -> Result<Milestone> {
        required(title, "a milestone needs a title")?;
        let conn = self.conn();
        let changed = conn.execute(
            "UPDATE milestones SET title = ?2, due = ?3, updated_at = ?4 WHERE id = ?1",
            params![id, title.trim(), due.map(|d| d.to_string()), now()],
        )?;
        if changed == 0 {
            return Err(not_found(id));
        }
        milestone_in(&conn, id)
    }

    /// Move a milestone to follow `after`, or to come first when `after` is
    /// `None`. It takes the midpoint of its new neighbours' keys; when they
    /// are too close (or tied), the goal's milestones are renumbered in
    /// their new order.
    pub fn move_milestone(&self, id: &str, after: Option<&str>) -> Result<()> {
        if after == Some(id) {
            return Err(invalid("a milestone can't follow itself"));
        }
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let moved = milestone_in(&tx, id)?;
        let mut order: Vec<(String, f64)> = milestones_in(&tx, &moved.goal_id)?
            .into_iter()
            .filter(|m| m.id != id)
            .map(|m| (m.id, m.sort_order))
            .collect();
        let at = match after {
            None => 0,
            Some(after) => {
                order
                    .iter()
                    .position(|(m, _)| m == after)
                    .ok_or_else(|| invalid("that milestone belongs to another goal"))?
                    + 1
            }
        };
        let before = at.checked_sub(1).map(|i| order[i].1);
        let next = order.get(at).map(|(_, key)| *key);
        let key = match (before, next) {
            (None, None) => Some(1.0),
            (None, Some(n)) => Some(n - 1.0),
            (Some(b), None) => Some(b.floor() + 1.0),
            (Some(b), Some(n)) if n - b > MIN_GAP => Some(b + (n - b) / 2.0),
            _ => None,
        };
        let stamp = now();
        match key {
            Some(key) => {
                tx.execute(
                    "UPDATE milestones SET sort_order = ?2, updated_at = ?3 WHERE id = ?1",
                    params![id, key, stamp],
                )?;
            }
            None => {
                order.insert(at, (id.to_owned(), 0.0));
                for (i, (m, _)) in order.iter().enumerate() {
                    tx.execute(
                        "UPDATE milestones SET sort_order = ?2, updated_at = ?3 WHERE id = ?1",
                        params![m, (i + 1) as f64, stamp],
                    )?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Delete a milestone. Check-ins that marked it keep their notes.
    pub fn delete_milestone(&self, id: &str) -> Result<()> {
        if self
            .conn()
            .execute("DELETE FROM milestones WHERE id = ?1", [id])?
            == 0
        {
            return Err(not_found(id));
        }
        Ok(())
    }

    /// A goal's milestones, in order.
    pub fn milestones(&self, goal_id: &str) -> Result<Vec<Milestone>> {
        milestones_in(&self.conn(), goal_id)
    }

    pub fn add_check_in(&self, goal_id: &str, new: &NewCheckIn) -> Result<CheckIn> {
        let conn = self.conn();
        let goal = goal_in(&conn, goal_id)?;
        check(&conn, &goal, new, None)?;
        let id = new_id();
        let now = now();
        conn.execute(
            "INSERT INTO check_ins (id, goal_id, on_date, note, value, milestone_id,
                 created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            params![
                id,
                goal_id,
                new.on.to_string(),
                new.note.trim(),
                new.value.map(Quantity::units),
                new.milestone_id,
                now
            ],
        )?;
        check_in_in(&conn, &id)
    }

    /// Replace a check-in's date, note, value and milestone.
    pub fn edit_check_in(&self, id: &str, new: &NewCheckIn) -> Result<CheckIn> {
        let conn = self.conn();
        let old = check_in_in(&conn, id)?;
        let goal = goal_in(&conn, &old.goal_id)?;
        check(&conn, &goal, new, Some(id))?;
        conn.execute(
            "UPDATE check_ins SET on_date = ?2, note = ?3, value = ?4, milestone_id = ?5,
                 updated_at = ?6
             WHERE id = ?1",
            params![
                id,
                new.on.to_string(),
                new.note.trim(),
                new.value.map(Quantity::units),
                new.milestone_id,
                now()
            ],
        )?;
        check_in_in(&conn, id)
    }

    pub fn delete_check_in(&self, id: &str) -> Result<()> {
        if self
            .conn()
            .execute("DELETE FROM check_ins WHERE id = ?1", [id])?
            == 0
        {
            return Err(not_found(id));
        }
        Ok(())
    }

    /// A goal's check-ins, oldest first (by date, then when logged).
    pub fn check_ins(&self, goal_id: &str) -> Result<Vec<CheckIn>> {
        let conn = self.conn();
        goal_in(&conn, goal_id)?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {CHECK_IN_COLUMNS} FROM check_ins WHERE goal_id = ?1
             ORDER BY on_date, created_at, id"
        ))?;
        let rows = stmt.query_map([goal_id], map_check_in)?;
        rows.map(|r| r?).collect()
    }

    pub fn add_review(&self, goal_id: &str, new: &NewReview) -> Result<Review> {
        let conn = self.conn();
        goal_in(&conn, goal_id)?;
        let id = new_id();
        conn.execute(
            "INSERT INTO reviews (id, goal_id, on_date, committed, on_track, next_step, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                goal_id,
                new.on.to_string(),
                new.committed,
                new.on_track,
                new.next_step.trim(),
                now()
            ],
        )?;
        conn.query_row(
            &format!("SELECT {REVIEW_COLUMNS} FROM reviews WHERE id = ?1"),
            [&id],
            map_review,
        )?
    }

    /// A goal's reviews, oldest first.
    pub fn reviews(&self, goal_id: &str) -> Result<Vec<Review>> {
        let conn = self.conn();
        goal_in(&conn, goal_id)?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {REVIEW_COLUMNS} FROM reviews WHERE goal_id = ?1
             ORDER BY on_date, created_at, id"
        ))?;
        let rows = stmt.query_map([goal_id], map_review)?;
        rows.map(|r| r?).collect()
    }

    /// The date of the latest review on or before `today`.
    pub(crate) fn last_review(&self, goal_id: &str, today: Date) -> Result<Option<Date>> {
        let last: Option<String> = self.conn().query_row(
            "SELECT MAX(on_date) FROM reviews WHERE goal_id = ?1 AND on_date <= ?2",
            params![goal_id, today.to_string()],
            |r| r.get(0),
        )?;
        optional_date(last)
    }
}

/// The rules a check-in's contents must meet on its goal.
fn check(conn: &Connection, goal: &Goal, new: &NewCheckIn, editing: Option<&str>) -> Result<()> {
    if new.value.is_some() && !goal.measure.is_number() {
        return Err(invalid("only a number goal's check-ins have a value"));
    }
    if new.note.trim().is_empty() && new.value.is_none() && new.milestone_id.is_none() {
        return Err(invalid("a check-in needs a note, a value or a milestone"));
    }
    if let Some(milestone) = &new.milestone_id {
        let owner: Option<String> = conn
            .query_row(
                "SELECT goal_id FROM milestones WHERE id = ?1",
                [milestone],
                |r| r.get(0),
            )
            .optional()?;
        if owner.as_deref() != Some(goal.id.as_str()) {
            return Err(invalid("that milestone belongs to another goal"));
        }
        let marked: bool = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM check_ins
                 WHERE milestone_id = ?1 AND id IS NOT ?2)",
            params![milestone, editing],
            |r| r.get(0),
        )?;
        if marked {
            return Err(invalid(
                "another check-in already marks that milestone reached",
            ));
        }
    }
    Ok(())
}

fn milestone_in(conn: &Connection, id: &str) -> Result<Milestone> {
    conn.query_row(
        &format!("SELECT {MILESTONE_COLUMNS} FROM milestones m WHERE m.id = ?1"),
        [id],
        map_milestone,
    )
    .optional()?
    .ok_or_else(|| not_found(id))?
}

fn milestones_in(conn: &Connection, goal_id: &str) -> Result<Vec<Milestone>> {
    goal_in(conn, goal_id)?;
    let mut stmt = conn.prepare(&format!(
        "SELECT {MILESTONE_COLUMNS} FROM milestones m WHERE m.goal_id = ?1
         ORDER BY m.sort_order, m.created_at, m.id"
    ))?;
    let rows = stmt.query_map([goal_id], map_milestone)?;
    rows.map(|r| r?).collect()
}

fn check_in_in(conn: &Connection, id: &str) -> Result<CheckIn> {
    conn.query_row(
        &format!("SELECT {CHECK_IN_COLUMNS} FROM check_ins WHERE id = ?1"),
        [id],
        map_check_in,
    )
    .optional()?
    .ok_or_else(|| not_found(id))?
}

fn map_milestone(row: &Row) -> rusqlite::Result<Result<Milestone>> {
    let due: Option<String> = row.get(3)?;
    let reached: Option<String> = row.get(5)?;
    let (id, goal_id, title, sort_order) = (row.get(0)?, row.get(1)?, row.get(2)?, row.get(4)?);
    Ok((|| {
        Ok(Milestone {
            id,
            goal_id,
            title,
            due: optional_date(due)?,
            reached_on: optional_date(reached)?,
            sort_order,
        })
    })())
}

fn map_check_in(row: &Row) -> rusqlite::Result<Result<CheckIn>> {
    let on: String = row.get(2)?;
    let value: Option<i64> = row.get(4)?;
    let check_in = CheckIn {
        id: row.get(0)?,
        goal_id: row.get(1)?,
        on: Date::MIN,
        note: row.get(3)?,
        value: value.map(Quantity::from_units),
        milestone_id: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    };
    Ok(date(&on).map(|on| CheckIn { on, ..check_in }))
}

fn map_review(row: &Row) -> rusqlite::Result<Result<Review>> {
    let on: String = row.get(2)?;
    let review = Review {
        id: row.get(0)?,
        goal_id: row.get(1)?,
        on: Date::MIN,
        committed: row.get(3)?,
        on_track: row.get(4)?,
        next_step: row.get(5)?,
        created_at: row.get(6)?,
    };
    Ok(date(&on).map(|on| Review { on, ..review }))
}
