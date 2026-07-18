//! Project-owned `SQLite` migration mechanism and schema foundation.

use rusqlite::{Connection, OptionalExtension, Transaction};

/// Current schema version understood by this executable.
pub const CURRENT_SCHEMA_VERSION: i64 = 1;

const MIGRATION_1: &str = include_str!("../../../migrations/0001_initial.sql");

/// Applies every pending migration transactionally.
///
/// # Errors
///
/// Returns [`StorageError::FutureSchema`] for an incompatible future database or
/// [`StorageError::Database`] when schema inspection or a migration fails.
pub fn migrate(connection: &mut Connection) -> Result<(), StorageError> {
    connection.execute_batch("PRAGMA foreign_keys = ON;")?;
    let existing: Option<i64> = connection
        .query_row(
            "SELECT version FROM schema_metadata WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .or_else(|error| match error {
            rusqlite::Error::SqliteFailure(_, Some(ref message))
                if message.contains("no such table") =>
            {
                Ok(None)
            }
            other => Err(other),
        })?;
    if let Some(version) = existing
        && version > CURRENT_SCHEMA_VERSION
    {
        return Err(StorageError::FutureSchema(version));
    }
    if existing.unwrap_or(0) < 1 {
        apply(connection.transaction()?, 1, MIGRATION_1)?;
    }
    Ok(())
}

fn apply(transaction: Transaction<'_>, version: i64, sql: &str) -> Result<(), StorageError> {
    transaction.execute_batch(sql)?;
    transaction.execute(
        "INSERT INTO schema_metadata(singleton, version) VALUES(1, ?1)
        ON CONFLICT(singleton) DO UPDATE SET version = excluded.version",
        [version],
    )?;
    transaction.commit()?;
    Ok(())
}

/// Persistence foundation failures.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("database schema version {0} is newer than this executable supports")]
    FutureSchema(i64),
    #[error("database error")]
    Database(#[from] rusqlite::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_creates_schema_and_is_idempotent() {
        let mut connection = Connection::open_in_memory().unwrap();
        migrate(&mut connection).unwrap();
        migrate(&mut connection).unwrap();
        let version: i64 = connection
            .query_row("SELECT version FROM schema_metadata", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, CURRENT_SCHEMA_VERSION);
        let tables: i64 = connection.query_row("SELECT count(*) FROM sqlite_schema WHERE type='table' AND name IN ('roots','source_files','revisions','segments')", [], |r| r.get(0)).unwrap();
        assert_eq!(tables, 4);
    }

    #[test]
    fn future_schema_is_rejected() {
        let mut connection = Connection::open_in_memory().unwrap();
        migrate(&mut connection).unwrap();
        connection
            .execute("UPDATE schema_metadata SET version = 999", [])
            .unwrap();
        assert!(matches!(
            migrate(&mut connection),
            Err(StorageError::FutureSchema(999))
        ));
    }
}
