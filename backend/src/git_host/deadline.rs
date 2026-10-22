//! Original proof lifetime across native Git, ref preparation and response handoff.
use super::*;
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;

#[derive(Clone, Copy)]
pub(crate) struct GitDeadline {
    instant: Instant,
    until: i64,
}
impl GitDeadline {
    pub(crate) fn request(
        authority: &pdlc_auth_core::http_authority::RequestAuthority,
        until: i64,
    ) -> Result<Option<Self>, ApiError> {
        let Some(instant) = authority.original_deadline() else {
            return Ok(None);
        };
        // Wall first: scheduling delay can only shorten the original clock.
        let wall = Utc::now();
        let remaining = instant
            .checked_duration_since(Instant::now())
            .ok_or_else(ApiError::forbidden)?;
        let duration = chrono::Duration::from_std(remaining).map_err(|_| ApiError::forbidden())?;
        let local_until = wall
            .checked_add_signed(duration)
            .ok_or_else(ApiError::forbidden)?
            .timestamp();
        let clock = Self {
            instant,
            until: until.min(local_until),
        };
        clock.require()?;
        Ok(Some(clock))
    }
    pub(crate) fn intersect(self, other: Self) -> Self {
        Self {
            instant: self.instant.min(other.instant),
            until: self.until.min(other.until),
        }
    }
    /// Freshness only, including lock/COMMIT waits. No role or namespace grant.
    pub(crate) async fn bind_commit_clock(
        self,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), ApiError> {
        self.require()?;
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO forge_request_commit_fences(fence_id,valid_until)
            VALUES($1,to_timestamp(($2::bigint)::double precision))",
        )
        .bind(id)
        .bind(self.until)
        .execute(&mut **transaction)
        .await
        .map_err(ApiError::internal)?;
        sqlx::query("DELETE FROM forge_request_commit_fences WHERE fence_id=$1")
            .bind(id)
            .execute(&mut **transaction)
            .await
            .map_err(ApiError::internal)?;
        self.require()
    }
    pub(super) fn identity(source: &pdlc_auth_core::service_bridge::VerifiedCentralToken) -> Self {
        Self {
            instant: source.lifetime.monotonic_deadline(),
            until: source.lifetime.local_valid_until(),
        }
    }
    pub(super) fn product_personal(
        source: &cicd_app::product_token::ConfirmedPersonalToken,
    ) -> Self {
        Self {
            instant: source.owner().monotonic_deadline(),
            until: source.owner().decision.expires_at.timestamp(),
        }
    }
    pub(super) fn admission(source: &pdlc_auth_core::service_bridge::ConfirmedSpaceRead) -> Self {
        Self {
            instant: source.monotonic_deadline(),
            until: source.valid_until(),
        }
    }
    pub(super) fn current(self) -> bool {
        Instant::now() < self.instant && Utc::now().timestamp() < self.until
    }
    pub(crate) fn require(self) -> Result<(), ApiError> {
        if self.current() {
            Ok(())
        } else {
            Err(ApiError::unauthorized())
        }
    }
    /// Translate only the remaining fixed window, conservatively rounded down.
    #[cfg(target_os = "linux")]
    pub(super) fn hook_bounds(self) -> Result<(u64, i64), ApiError> {
        let mut clock = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: this is a live timespec; reading BOOTTIME has no side effects.
        if unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut clock) } != 0 {
            return Err(ApiError::service_unavailable("Git fixed clock unavailable"));
        }
        self.require()?;
        let remaining = self
            .instant
            .saturating_duration_since(Instant::now())
            .as_secs();
        let cap = u64::try_from(clock.tv_sec)
            .ok()
            .and_then(|seconds| seconds.checked_add(remaining))
            .filter(|_| remaining > 0)
            .ok_or_else(|| ApiError::service_unavailable("Git proof window insufficient"))?;
        Ok((cap, self.until))
    }
    #[cfg(not(target_os = "linux"))]
    pub(super) fn hook_bounds(self) -> Result<(u64, i64), ApiError> {
        Err(ApiError::service_unavailable(
            "Git fixed-clock hooks require Linux",
        ))
    }
    pub(super) fn response(self, mut response: Response) -> Response {
        if let Err(error) = self.require() {
            return git_access_error(error);
        }
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("private, no-store"),
        );
        let (parts, body) = response.into_parts();
        use tokio_stream::StreamExt;
        let chunks = body.into_data_stream().map(move |chunk| {
            if !self.current() {
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "git_proof_expired",
                ))
            } else {
                chunk.map_err(std::io::Error::other)
            }
        });
        Response::from_parts(parts, axum::body::Body::from_stream(chunks))
    }
}

/// The detached native owner retains the lifecycle lease throughout this call.
pub(super) async fn output(
    command: &mut Command,
    body: Bytes,
    proof: Option<GitDeadline>,
) -> Result<std::process::Output, ApiError> {
    if let Some(proof) = proof {
        proof.require()?;
    }
    let expiry = async {
        match proof {
            Some(proof) => tokio::time::sleep_until(proof.instant.into()).await,
            None => std::future::pending::<()>().await,
        }
    };
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.as_std_mut().process_group(0);
    }
    let mut child = command
        .kill_on_drop(true)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| ApiError::internal(sqlx::Error::Io(error)))?;
    let pid = child.id();
    let mut stdin = child.stdin.take().expect("configured Git stdin");
    let mut stdout = child.stdout.take().expect("configured Git stdout");
    let mut stderr = child.stderr.take().expect("configured Git stderr");
    let writer = tokio::spawn(async move {
        stdin.write_all(&body).await?;
        stdin.shutdown().await
    });
    let mut out = tokio::spawn(async move {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).await.map(|_| bytes)
    });
    let mut err = tokio::spawn(async move {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).await.map(|_| bytes)
    });
    let completed = async {
        let status = child.wait().await?;
        // Include inherited pipe writers, even if their parent has already exited.
        let stdout = (&mut out).await.map_err(std::io::Error::other)??;
        let stderr = (&mut err).await.map_err(std::io::Error::other)??;
        Ok::<_, std::io::Error>(std::process::Output {
            status,
            stdout,
            stderr,
        })
    };
    let stop = async {
        if proof.is_none() {
            std::future::pending::<()>().await;
        }
        loop {
            if proof.is_some_and(|proof| !proof.current()) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    let result = tokio::select! {
        biased;
        _=expiry=>None,
        _=stop=>None,
        result=completed=>Some(result),
    };
    let result = match result {
        Some(Ok(output)) => Ok(output),
        other => {
            #[cfg(unix)]
            if let Some(pid) = pid {
                // Let Git's signal handler remove its own ref locks before
                // forcing the remaining owned group down. Never unlink locks
                // by guessing which process created them.
                // SAFETY: this invocation created and owns this process group.
                unsafe {
                    libc::kill(-(pid as i32), libc::SIGTERM);
                }
                let _ = tokio::time::timeout(Duration::from_millis(250), child.wait()).await;
                // A custom hook can ignore TERM even after its Git parent exits.
                // SAFETY: this is still the group owned by this invocation.
                unsafe {
                    libc::kill(-(pid as i32), libc::SIGKILL);
                }
            }
            let _ = child.kill().await;
            let _ = child.wait().await;
            // Handles already consumed by completed must not be polled twice.
            out.abort();
            err.abort();
            match other {
                Some(Err(error)) => Err(ApiError::internal(sqlx::Error::Io(error))),
                _ => Err(ApiError::service_unavailable(
                    "Git proof expired or execution timed out; exact ref readback required",
                )),
            }
        }
    };
    writer.abort();
    let _ = writer.await;
    let output = result?;
    if proof.is_some_and(|proof| !proof.current()) {
        return Err(ApiError::service_unavailable(
            "Git proof expired during execution; exact ref readback required",
        ));
    }
    Ok(output)
}
