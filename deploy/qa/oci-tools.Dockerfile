FROM docker:27.5.1-dind-rootless AS client
FROM rust:1.88.0-bookworm
COPY --from=client /usr/local/bin/docker /usr/local/bin/docker
COPY --from=client /usr/local/libexec/docker/cli-plugins/docker-buildx /usr/local/libexec/docker/cli-plugins/docker-buildx
COPY --from=client /usr/local/libexec/docker/cli-plugins/docker-compose /usr/local/bin/docker-compose
