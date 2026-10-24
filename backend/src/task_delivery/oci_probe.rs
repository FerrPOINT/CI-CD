//! HTTP readback inside an already validated, owner-local transport helper.
use super::Owner;
use crate::domain::task_delivery::{DeliveryProbe, DeliveryStatus, digest};
use chrono::Utc;
use serde::Deserialize;
use std::net::IpAddr;

// Existing compatibility maximum; the extra byte and metadata fit 4 MiB /tmp.
const MAX_BODY_BYTES: u64 = 1024 * 1024;
const MAX_METADATA_BYTES: usize = 256;

// The producer records curl's status even when head closes the pipe. This also
// bounds unknown-length responses on curl versions predating streaming limits.
const PROBE_SCRIPT: &str = r#"set -uf
url=$1
limit=$2
cap=$((limit + 1))
tmp=$(mktemp -d /tmp/forge-probe.XXXXXX) || exit 1
trap 'rm -f -- "$tmp/body" "$tmp/http" "$tmp/exit"; rmdir -- "$tmp"' 0
trap 'exit 1' HUP INT TERM
{
    {
        curl -q --silent --noproxy '*' --proxy '' --proto '=http' \
            --connect-timeout 1 --max-time 3 --max-redirs 0 \
            --max-filesize "$limit" -H 'cache-control:no-cache' \
            -w '%{stderr}%{http_code}' -- "$url" 2>"$tmp/http"
        result=$?
        printf '%s' "$result" >"$tmp/exit"
    } | head -c "$cap" >"$tmp/body"
} &
pipeline=$!
wait "$pipeline" || exit 1
result=$(cat "$tmp/exit") || exit 1
http=$(cat "$tmp/http") || exit 1
case "$http" in
    000) http=0 ;;
    [1-5][0-9][0-9]) ;;
    *) exit 1 ;;
esac
size=$(wc -c <"$tmp/body") || exit 1
hash=''
if [ "$result" -eq 0 ] && [ "$http" -eq 200 ] && [ "$size" -le "$limit" ]; then
    sum=$(sha256sum "$tmp/body") || exit 1
    hash=${sum%% *}
fi
printf '{"curl_exit":%s,"http_status":%s,"body_bytes":%s,"body_sha256":"%s"}\n' \
    "$result" "$http" "$size" "$hash"
"#;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProbeMetadata {
    curl_exit: u8,
    http_status: u16,
    body_bytes: u64,
    body_sha256: String,
}

fn unavailable() -> DeliveryProbe {
    DeliveryProbe {
        status: DeliveryStatus::Unavailable,
        http_status: None,
        body_sha256: None,
        observed_at: Utc::now(),
    }
}

fn exec_args(helper: &str, ip: IpAddr, path: &str, limit: u64) -> Option<Vec<String>> {
    if limit > MAX_BODY_BYTES
        || !path.starts_with('/')
        || path.contains("//")
        || path.contains("..")
        || path.len() > 128
        || !path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/-_.".contains(&b))
    {
        return None;
    }
    let host = match ip {
        IpAddr::V4(ip) => ip.to_string(),
        IpAddr::V6(ip) => format!("[{ip}]"),
    };
    Some(
        [
            "exec".to_owned(),
            "--".to_owned(),
            helper.to_owned(),
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            PROBE_SCRIPT.to_owned(),
            "forge-probe".to_owned(),
            format!("http://{host}:8000{path}"),
            limit.to_string(),
        ]
        .into(),
    )
}

fn read_metadata(output: &[u8], expected: &str, limit: u64) -> DeliveryProbe {
    let mut probe = unavailable();
    if output.len() > MAX_METADATA_BYTES || limit > MAX_BODY_BYTES || !digest(expected) {
        return probe;
    }
    let Ok(meta) = serde_json::from_slice::<ProbeMetadata>(output) else {
        return probe;
    };
    if (meta.http_status != 0 && !(100..=599).contains(&meta.http_status))
        || meta.body_bytes > limit + 1
        || (!meta.body_sha256.is_empty() && !digest(&meta.body_sha256))
    {
        return probe;
    }
    probe.http_status = (meta.http_status != 0).then_some(meta.http_status);
    if meta.body_bytes > limit || meta.curl_exit == 63 {
        probe.status = DeliveryStatus::Failed;
    } else if meta.curl_exit != 0 || meta.http_status == 0 {
        // A matching prefix is not proof of a complete HTTP response.
        return probe;
    } else if meta.http_status != 200 {
        probe.status = DeliveryStatus::Failed;
    } else if digest(&meta.body_sha256) {
        probe.status = if meta.body_sha256 == expected {
            DeliveryStatus::Verified
        } else {
            DeliveryStatus::Failed
        };
        probe.body_sha256 = Some(meta.body_sha256);
    }
    probe
}

impl Owner {
    /// The caller must validate the helper receipt and inspected application IP.
    pub(super) async fn probe_remote(
        &self,
        helper: &str,
        ip: IpAddr,
        path: &str,
        expected: &str,
        limit: u64,
    ) -> DeliveryProbe {
        if !digest(expected) {
            return unavailable();
        }
        let Some(args) = exec_args(helper, ip, path, limit) else {
            return unavailable();
        };
        match self.run(&self.policy.docker_bin, &args, None).await {
            Ok(output) => read_metadata(&output, expected, limit),
            Err(_) => unavailable(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    const HASH: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn metadata(exit: u8, http: u16, bytes: u64, hash: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "curl_exit": exit, "http_status": http,
            "body_bytes": bytes, "body_sha256": hash,
        }))
        .unwrap()
    }

    // No sockets or real curl: the shell function models curl's decoded body,
    // write-out channel and exit status, including old curl's unbounded stdout.
    fn script_fixture(body: &str, exit: u8, http: &str, limit: u64) -> Vec<u8> {
        let root =
            std::env::temp_dir().join(format!("forge-oci-probe-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let shell = if cfg!(windows) {
            std::path::PathBuf::from(std::env::var_os("ProgramFiles").unwrap())
                .join("Git/bin/sh.exe")
        } else {
            "/bin/sh".into()
        };
        let script = format!(
            r#"mktemp() {{ command mktemp -d "$FIXTURE_TMP/forge-probe.XXXXXX"; }}
curl() {{
    printf '%s' "$FIXTURE_HTTP" >&2
    if [ "$FIXTURE_BODY" = overflow ]; then
        head -c 2097152 /dev/zero
    else
        printf '%s' "$FIXTURE_BODY"
    fi
    return "$FIXTURE_EXIT"
}}
{PROBE_SCRIPT}"#
        );
        let output = Command::new(shell)
            .args([
                "-c",
                &script,
                "forge-probe",
                "http://192.0.2.1:8000/health",
                &limit.to_string(),
            ])
            .env("FIXTURE_BODY", body)
            .env("FIXTURE_EXIT", exit.to_string())
            .env("FIXTURE_HTTP", http)
            .env("FIXTURE_TMP", &root)
            .output();
        let cleanup = std::fs::remove_dir(&root);
        let output = output.expect("fixture requires /bin/sh and coreutils, never real curl");
        cleanup.expect("probe must clean up its own files and directory");
        assert!(output.status.success(), "probe script failed");
        assert!(output.stderr.is_empty(), "fixture leaked stderr");
        assert!(!output.stdout.is_empty(), "fixture emitted no metadata");
        assert!(output.stdout.len() <= MAX_METADATA_BYTES);
        output.stdout
    }

    #[test]
    fn daemon_probe_arguments_are_fixed_and_ipv6_is_bracketed() {
        for (ip, host) in [("192.0.2.7", "192.0.2.7"), ("2001:db8::7", "[2001:db8::7]")] {
            for path in [
                "/.forge/version",
                "/compatibility",
                "/health",
                "/acceptance",
            ] {
                let args = exec_args("owned-helper", ip.parse().unwrap(), path, 64).unwrap();
                assert_eq!(&args[..5], ["exec", "--", "owned-helper", "/bin/sh", "-c"]);
                assert_eq!(args.len(), 9);
                assert_eq!(args[5], PROBE_SCRIPT);
                assert_eq!(args[6], "forge-probe");
                assert_eq!(args[7], format!("http://{host}:8000{path}"));
                assert_eq!(args[8], "64");
            }
        }
    }

    #[test]
    fn daemon_probe_rejects_nonfixed_paths_and_excessive_limits() {
        let ip = "192.0.2.7".parse().unwrap();
        for path in [
            "health", "//host", "/../x", "/x?q=1", "/x#y", "/x;y", "/$x", "/a b", "/%61", "/x\n",
        ] {
            assert!(exec_args("helper", ip, path, 64).is_none(), "{path:?}");
        }
        assert!(exec_args("helper", ip, &format!("/{}", "a".repeat(128)), 64).is_none());
        assert!(exec_args("helper", ip, "/health", MAX_BODY_BYTES + 1).is_none());
        assert!(exec_args("helper", ip, "/health", u64::MAX).is_none());
    }

    #[test]
    fn daemon_probe_script_preserves_transport_and_stream_bounds() {
        for required in [
            "curl -q --silent --noproxy '*' --proxy '' --proto '=http'",
            "--connect-timeout 1 --max-time 3 --max-redirs 0",
            "--max-filesize \"$limit\"",
            "-H 'cache-control:no-cache'",
            "-w '%{stderr}%{http_code}'",
            "result=$?",
            "wait \"$pipeline\"",
            "head -c \"$cap\"",
            "cap=$((limit + 1))",
            "trap '",
            "sha256sum \"$tmp/body\"",
            "wc -c <\"$tmp/body\"",
        ] {
            assert!(PROBE_SCRIPT.contains(required), "missing {required}");
        }
        for forbidden in [
            "--location",
            " -L",
            "PIPESTATUS",
            "<( ",
            "python",
            "--show-error",
            "cat \"$tmp/body\"",
        ] {
            assert!(!PROBE_SCRIPT.contains(forbidden), "unexpected {forbidden}");
        }
    }

    #[test]
    fn daemon_probe_metadata_requires_complete_success() {
        let verified = read_metadata(&metadata(0, 200, 3, HASH), HASH, 3);
        assert_eq!(verified.status, DeliveryStatus::Verified);
        assert_eq!(verified.http_status, Some(200));
        assert_eq!(verified.body_sha256.as_deref(), Some(HASH));
        for exit in [7, 18, 23, 28, 56, 141] {
            let probe = read_metadata(&metadata(exit, 200, 3, HASH), HASH, 3);
            assert_eq!(probe.status, DeliveryStatus::Unavailable, "exit {exit}");
            assert!(probe.body_sha256.is_none());
        }
        for (exit, http, bytes) in [
            (0, 404, 3),
            (0, 302, 3),
            (0, 500, 3),
            (23, 200, 4),
            (28, 200, 4),
            (63, 200, 0),
        ] {
            assert_eq!(
                read_metadata(&metadata(exit, http, bytes, ""), HASH, 3).status,
                DeliveryStatus::Failed
            );
        }
        assert_eq!(
            read_metadata(&metadata(0, 200, 3, &"a".repeat(64)), HASH, 3).status,
            DeliveryStatus::Failed
        );
        assert_eq!(
            read_metadata(&metadata(0, 0, 0, ""), HASH, 3).status,
            DeliveryStatus::Unavailable
        );
    }

    #[test]
    fn daemon_probe_metadata_is_strict_and_bounded() {
        for output in [
            b"{}".to_vec(),
            b"null".to_vec(),
            b"{\"curl_exit\":0,\"http_status\":200,\"body_bytes\":0}".to_vec(),
            b"{\"curl_exit\":null,\"http_status\":200,\"body_bytes\":0,\"body_sha256\":\"\"}"
                .to_vec(),
            vec![0xff],
            metadata(0, 99, 3, HASH),
            metadata(0, 600, 3, HASH),
            metadata(0, 200, 5, HASH),
            metadata(0, 200, 3, "secret"),
            format!("{}x", String::from_utf8(metadata(0, 200, 3, HASH)).unwrap()).into_bytes(),
            format!(
                "{{\"extra\":0,{}",
                &String::from_utf8(metadata(0, 200, 3, HASH)).unwrap()[1..]
            )
            .into_bytes(),
            format!(
                "{{\"curl_exit\":0,{}",
                &String::from_utf8(metadata(0, 200, 3, HASH)).unwrap()[1..]
            )
            .into_bytes(),
            vec![b' '; MAX_METADATA_BYTES + 1],
        ] {
            assert_eq!(
                read_metadata(&output, HASH, 3).status,
                DeliveryStatus::Unavailable
            );
        }
        assert_eq!(
            read_metadata(&metadata(0, 200, 3, HASH), "bad", 3).status,
            DeliveryStatus::Unavailable
        );
    }

    #[test]
    fn daemon_probe_shell_exact_limit_and_hash_mismatch() {
        let output = script_fixture("abc", 0, "200", 3);
        assert_eq!(
            read_metadata(&output, HASH, 3).status,
            DeliveryStatus::Verified
        );
        assert_eq!(
            read_metadata(&output, &"a".repeat(64), 3).status,
            DeliveryStatus::Failed
        );
    }

    #[test]
    fn daemon_probe_shell_bounds_old_curl_unknown_length() {
        let output = script_fixture("overflow", 23, "200", 3);
        let meta: ProbeMetadata = serde_json::from_slice(&output).unwrap();
        assert_eq!(meta.body_bytes, 4);
        assert_eq!(meta.curl_exit, 23);
        assert!(meta.body_sha256.is_empty());
        assert_eq!(
            read_metadata(&output, HASH, 3).status,
            DeliveryStatus::Failed
        );
    }

    #[test]
    fn daemon_probe_shell_partial_matching_body_never_verifies() {
        for exit in [18, 28, 56] {
            let output = script_fixture("abc", exit, "200", 3);
            let meta: ProbeMetadata = serde_json::from_slice(&output).unwrap();
            assert_eq!(meta.curl_exit, exit);
            assert!(meta.body_sha256.is_empty());
            assert_eq!(
                read_metadata(&output, HASH, 3).status,
                DeliveryStatus::Unavailable
            );
        }
    }

    #[test]
    fn daemon_probe_shell_non200_and_declared_oversize_fail() {
        for (body, exit, http) in [("abc", 0, "302"), ("abc", 0, "404"), ("", 63, "200")] {
            let output = script_fixture(body, exit, http, 3);
            assert_eq!(
                read_metadata(&output, HASH, 3).status,
                DeliveryStatus::Failed
            );
        }
    }

    #[test]
    fn daemon_probe_shell_zero_limit_and_unreachable() {
        let empty_hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let output = script_fixture("", 0, "200", 0);
        assert_eq!(
            read_metadata(&output, empty_hash, 0).status,
            DeliveryStatus::Verified
        );
        let output = script_fixture("a", 0, "200", 0);
        assert_eq!(
            read_metadata(&output, empty_hash, 0).status,
            DeliveryStatus::Failed
        );
        let output = script_fixture("", 7, "000", 3);
        assert_eq!(
            read_metadata(&output, HASH, 3).status,
            DeliveryStatus::Unavailable
        );
    }
}
