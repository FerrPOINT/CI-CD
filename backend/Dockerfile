FROM rust:1.86-slim AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock* ./
RUN mkdir src && printf 'fn main() {}\n' > src/main.rs && printf '' > src/lib.rs && /usr/local/cargo/bin/cargo build --release && rm -rf src
COPY src ./src
RUN /usr/local/cargo/bin/cargo clean && /usr/local/cargo/bin/cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends curl git git-lfs && rm -rf /var/lib/apt/lists/* && useradd --create-home --uid 10001 cicd
COPY --from=build /app/target/release/cicd-server /usr/local/bin/cicd-server
RUN mkdir -p /var/lib/forge/git /var/lib/forge/artifacts && chown -R cicd:cicd /var/lib/forge
USER cicd
EXPOSE 22801
CMD ["cicd-server"]
