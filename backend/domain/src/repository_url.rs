//! Validation of clone URLs at the project write boundary; no network access.

const INVALID_URL: &str = "repository_url must be an HTTP(S), SSH or Git URL with a host and repository path, or user@host:path; passwords, query and fragment are not allowed";

pub fn validate_repository_url(input: &str) -> Result<(), &'static str> {
    let input = input.trim();
    if input.is_empty()
        || input.chars().any(|c| c.is_whitespace() || c.is_control())
        || input.contains(['\\', '?', '#'])
    {
        return Err(INVALID_URL);
    }
    let parsed = if let Some((scheme, rest)) = input.split_once("://") {
        // Reject forms that the URL parser would repair (e.g. https:///repo).
        if !matches!(scheme, "http" | "https" | "ssh" | "git") || rest.starts_with('/') {
            return Err(INVALID_URL);
        }
        url::Url::parse(input).map_err(|_| INVALID_URL)?
    } else {
        // SCP-style SSH uses user@host:path (including bracketed IPv6).
        let (user, rest) = input.split_once('@').ok_or(INVALID_URL)?;
        if user.is_empty()
            || !user
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        {
            return Err(INVALID_URL);
        }
        let separator = if rest.starts_with('[') {
            rest.find("]:").map(|index| index + 1)
        } else {
            rest.find(':')
        }
        .ok_or(INVALID_URL)?;
        let (host, path) = rest.split_at(separator);
        let path = &path[1..];
        if host.is_empty() || path.is_empty() || host.contains(['/', '@']) || path.starts_with(':')
        {
            return Err(INVALID_URL);
        }
        url::Url::parse(&format!("ssh://{user}@{host}/{path}")).map_err(|_| INVALID_URL)?
    };
    // SSH uses opaque URL hosts: reparse with the network-host rules and reject
    // option-like names that Git/OpenSSH refuse before connecting.
    let host = url::Host::parse(parsed.host_str().ok_or(INVALID_URL)?).map_err(|_| INVALID_URL)?;
    // Git decodes SSH usernames before rejecting option-like user@host arguments.
    let username = parsed.username();
    if matches!(&host, url::Host::Domain(name) if name.is_empty() || name.starts_with('-'))
        || (parsed.scheme() == "ssh"
            && (username.starts_with('-')
                || username.starts_with("%2D")
                || username.starts_with("%2d")))
        || parsed.path().trim_matches('/').is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(INVALID_URL);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_repository_url;

    #[test]
    fn repository_url_accepts_supported_clone_transports() {
        for url in [
            "https://example.test/team/repo.git",
            "http://localhost:7711/git/demo.git",
            "http://backend:22801/git/demo.git",
            "ssh://git@example.test:2222/team/repo.git",
            "ssh://git@[::1]:2222/team/repo.git",
            "git@example.test:team/repo.git",
            "git@[::1]:team/repo.git",
            "git://example.test/team/repo.git",
            " https://example.test/team/repo ",
        ] {
            assert!(validate_repository_url(url).is_ok(), "{url}");
        }
    }

    #[test]
    fn repository_url_rejects_invalid_or_unsupported_inputs() {
        for url in [
            "",
            "   ",
            "not-a-repository-url",
            "--upload-pack=evil",
            "/tmp/repo.git",
            "file:///tmp/repo.git",
            "javascript:alert(1)",
            "ftp://example.test/repo.git",
            "https://",
            "https:///repo.git",
            "https://example.test",
            "https://example.test/",
            "https:example.test/repo.git",
            "https://example.test/repo git",
            "https://example.test/repo\ngit",
            "https://example.test/repo\\git",
            "https://example.test/repo.git#main",
            "https://example.test/repo.git?token=secret",
            "https://user:secret@example.test/repo.git",
            "https://example.test:bad/repo.git",
            "git@example.test:",
            "git@:repo.git",
            "git@example.test",
            "git@example.test:repo.git#main",
            "host:repo.git",
        ] {
            assert!(validate_repository_url(url).is_err(), "{url}");
        }
    }

    #[test]
    fn repository_url_rejects_git_invalid_hosts() {
        for url in [
            "ssh://-bad/team/repo.git",
            "git@-bad:team/repo.git",
            "ssh://bad%host/team/repo.git",
            "git@bad%host:team/repo.git",
            "ssh://%2Dbad/team/repo.git",
            "git@%2Dbad:team/repo.git",
        ] {
            assert!(validate_repository_url(url).is_err(), "{url}");
        }
    }

    #[test]
    fn repository_url_preserves_local_and_alias_hosts() {
        for url in [
            "ssh://git@localhost/team/repo.git",
            "git@backend:team/repo.git",
            "git@build_server:team/repo.git",
            "ssh://git@[2001:db8::1]:2222/team/repo.git",
            "git@[2001:db8::1]:team/repo.git",
        ] {
            assert!(validate_repository_url(url).is_ok(), "{url}");
        }
    }

    #[test]
    fn repository_url_rejects_option_like_ssh_usernames() {
        for url in [
            "ssh://-bad@example.test/team/repo.git",
            "-bad@example.test:team/repo.git",
            "ssh://%2Dbad@example.test/team/repo.git",
            "ssh://%2dbad@example.test/team/repo.git",
            "ssh://-@example.test/team/repo.git",
            "-@example.test:team/repo.git",
        ] {
            assert!(validate_repository_url(url).is_err(), "{url}");
        }
        for url in [
            "ssh://build-user@example.test/team/repo.git",
            "build-user@example.test:team/repo.git",
            "https://-user@example.test/team/repo.git",
            "http://%2Duser@example.test/team/repo.git",
        ] {
            assert!(validate_repository_url(url).is_ok(), "{url}");
        }
    }
}
