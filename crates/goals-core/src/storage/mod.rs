//! SQLite storage. The running app is the only writer to its database; other
//! programs (the `goals` command, Matters) go through its local interface.

mod db;
mod records;
mod schema;

pub use db::{Database, Summary};
