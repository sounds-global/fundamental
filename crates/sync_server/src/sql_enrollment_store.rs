use std::future::Future;
use std::pin::Pin;

use fundamental_auth::UserId;
use fundamental_keys::{DeviceId, KeyError, VerifyingKey};
use fundamental_sql::SqlPool;
use sqlx::Row;

use crate::enrollment_store::EnrollmentStore;
use crate::{DeviceEnrollment, DeviceStatus};

/// A device-enrollment store backed by a SQL pool, shared by several server instances.
#[derive(Debug, Clone)]
pub struct SqlEnrollmentStore {
    /// The framework handle, so `ensure_table`'s DDL goes through [`SqlPool::execute_ddl`] and
    /// its `40001` retries.
    pool: SqlPool,
}

impl SqlEnrollmentStore {
    /// Wraps a [`SqlPool`].
    #[must_use]
    pub fn new(pool: &SqlPool) -> Self {
        Self { pool: pool.clone() }
    }

    /// Creates the `device_enrollments` table if it does not exist.
    ///
    /// For tests and single-binary setups; a migrated deployment uses its migrations.
    ///
    /// # Errors
    /// Returns [`KeyError::Storage`] if the statement fails.
    pub async fn ensure_table(&self) -> Result<(), KeyError> {
        // Two statements: PostgreSQL has no inline `INDEX` clause in `CREATE TABLE`.
        self.pool
            .execute_ddl(
                "CREATE TABLE IF NOT EXISTS device_enrollments (\
                 id UUID PRIMARY KEY DEFAULT gen_random_uuid(),\
                 device_id TEXT NOT NULL,\
                 user_id TEXT NOT NULL,\
                 public_key TEXT NOT NULL,\
                 status TEXT NOT NULL,\
                 enrolled_at INT8 NOT NULL,\
                 grace_until INT8,\
                 revoked_at INT8)",
            )
            .await
            .map_err(ddl)?;
        self.pool
            .execute_ddl(
                "CREATE INDEX IF NOT EXISTS device_enrollments_by_device \
                 ON device_enrollments (device_id)",
            )
            .await
            .map_err(ddl)?;
        Ok(())
    }
}

fn status_text(status: DeviceStatus) -> &'static str {
    match status {
        DeviceStatus::Active => "active",
        DeviceStatus::Grace => "grace",
        DeviceStatus::Revoked => "revoked",
    }
}

/// Parses a stored status; an unknown value is revoked (fail-closed).
fn status_from_text(text: &str) -> DeviceStatus {
    match text {
        "active" => DeviceStatus::Active,
        "grace" => DeviceStatus::Grace,
        _ => DeviceStatus::Revoked,
    }
}

fn storage(error: sqlx::Error) -> KeyError {
    KeyError::Storage(error.to_string())
}

fn ddl(error: fundamental_sql::SqlError) -> KeyError {
    KeyError::Storage(error.to_string())
}

impl EnrollmentStore for SqlEnrollmentStore {
    fn load<'a>(
        &'a self,
        device_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<DeviceEnrollment>, KeyError>> + Send + 'a>> {
        Box::pin(async move {
            let rows = sqlx::query(
                "SELECT device_id, user_id, public_key, status, enrolled_at, grace_until, revoked_at \
                 FROM device_enrollments WHERE device_id = $1",
            )
            .bind(device_id)
            .fetch_all(&self.pool.inner())
            .await
            .map_err(storage)?;

            let mut enrollments = Vec::with_capacity(rows.len());
            for row in &rows {
                let public_key_text: String = row.try_get("public_key").map_err(storage)?;
                let public_key = public_key_text.parse::<VerifyingKey>()?;
                let device: String = row.try_get("device_id").map_err(storage)?;
                let user: String = row.try_get("user_id").map_err(storage)?;
                let status: String = row.try_get("status").map_err(storage)?;
                enrollments.push(DeviceEnrollment {
                    device_id: DeviceId::new(device),
                    user_id: UserId::new(user),
                    public_key,
                    status: status_from_text(&status),
                    enrolled_at: row.try_get("enrolled_at").map_err(storage)?,
                    grace_until: row.try_get("grace_until").map_err(storage)?,
                    revoked_at: row.try_get("revoked_at").map_err(storage)?,
                });
            }
            Ok(enrollments)
        })
    }

    fn insert(
        &self,
        enrollment: DeviceEnrollment,
    ) -> Pin<Box<dyn Future<Output = Result<(), KeyError>> + Send + '_>> {
        Box::pin(async move {
            sqlx::query(
                "INSERT INTO device_enrollments \
                     (device_id, user_id, public_key, status, enrolled_at, grace_until, revoked_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
            )
            .bind(enrollment.device_id.as_str())
            .bind(enrollment.user_id.as_str())
            .bind(enrollment.public_key.to_string())
            .bind(status_text(enrollment.status))
            .bind(enrollment.enrolled_at)
            .bind(enrollment.grace_until)
            .bind(enrollment.revoked_at)
            .execute(&self.pool.inner())
            .await
            .map_err(storage)?;
            Ok(())
        })
    }

    fn grace_active<'a>(
        &'a self,
        device_id: &'a str,
        grace_until: i64,
    ) -> Pin<Box<dyn Future<Output = Result<(), KeyError>> + Send + 'a>> {
        Box::pin(async move {
            sqlx::query(
                "UPDATE device_enrollments SET status = 'grace', grace_until = $2 \
                 WHERE device_id = $1 AND status = 'active'",
            )
            .bind(device_id)
            .bind(grace_until)
            .execute(&self.pool.inner())
            .await
            .map_err(storage)?;
            Ok(())
        })
    }

    fn revoke_all<'a>(
        &'a self,
        device_id: &'a str,
        revoked_at: i64,
    ) -> Pin<Box<dyn Future<Output = Result<(), KeyError>> + Send + 'a>> {
        Box::pin(async move {
            sqlx::query(
                "UPDATE device_enrollments SET status = 'revoked', revoked_at = $2 \
                 WHERE device_id = $1",
            )
            .bind(device_id)
            .bind(revoked_at)
            .execute(&self.pool.inner())
            .await
            .map_err(storage)?;
            Ok(())
        })
    }
}
