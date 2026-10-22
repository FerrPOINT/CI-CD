//! Fixed human proof on pooled SQLx connections, including ordinary autocommit writes.
//! Role, application, native delegation and namespace decisions stay at ingress.
//! Migration 0104 checks the original timestamp after row/constraint waits at COMMIT.
use sqlx::{PgConnection, postgres::PgPoolOptions};

async fn stamp(connection: &mut PgConnection) -> Result<(), sqlx::Error> {
    if let Some(clock) = crate::pipeline_authority::current_clock() {
        let until = clock
            .valid_until()
            .map_err(|_| sqlx::Error::Protocol("Original human authority expired".into()))?;
        let remaining = (until - chrono::Utc::now()).num_milliseconds().max(1);
        // PostgreSQL 17 bounds the whole explicit/implicit transaction as well as
        // each statement. This includes deferred work ordered after the clock
        // trigger; HTTP future cancellation alone cannot cancel a server COMMIT.
        // Preserve any stricter connection defaults instead of weakening them.
        sqlx::query("SELECT set_config('forge.request_valid_until',$1,false),
            set_config('statement_timeout',(CASE WHEN s.setting::bigint=0 THEN $2::bigint ELSE LEAST(s.setting::bigint,$2::bigint) END)::text,false),
            set_config('transaction_timeout',(CASE WHEN t.setting::bigint=0 THEN $2::bigint ELSE LEAST(t.setting::bigint,$2::bigint) END)::text,false)
            FROM pg_settings s CROSS JOIN pg_settings t
            WHERE s.name='statement_timeout' AND t.name='transaction_timeout'")
            .bind(until.to_rfc3339()).bind(remaining)
            .fetch_one(connection).await?;
    } else {
        sqlx::query("RESET forge.request_valid_until")
            .execute(connection)
            .await?;
    }
    Ok(())
}

/// Every serving SQLx pool must use this factory. A connection is stamped on both
/// first open and reuse; a background/machine checkout clears prior human state.
/// The timestamp never becomes a permission and never gets a fresh lifetime.
pub fn pool_options() -> PgPoolOptions {
    PgPoolOptions::new()
        .after_connect(|connection, _| Box::pin(stamp(connection)))
        .before_acquire(|connection, _| Box::pin(async move {
            stamp(connection).await?;
            Ok(true)
        }))
        .after_release(|connection, _| Box::pin(async move {
            sqlx::query("SELECT set_config('forge.request_valid_until','',false), set_config('statement_timeout',s.reset_val,false), set_config('transaction_timeout',t.reset_val,false) FROM pg_settings s CROSS JOIN pg_settings t WHERE s.name='statement_timeout' AND t.name='transaction_timeout'")
                .execute(connection).await?;
            Ok(true)
        }))
}
