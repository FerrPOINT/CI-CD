FROM rust:1.86-slim AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock* ./
COPY domain/Cargo.toml domain/Cargo.toml
COPY cli/Cargo.toml cli/Cargo.toml
RUN mkdir -p src domain/src cli/src \
 && printf 'fn main() {}\n' > src/main.rs && printf '' > src/lib.rs \
 && printf '\n' > domain/src/lib.rs \
 && printf 'fn main() {}\n' > cli/src/main.rs \
 && /usr/local/cargo/bin/cargo build --release \
 && /usr/local/cargo/bin/cargo clean -p cicd-server -p cicd-domain -p cicd-cli
COPY src ./src
COPY domain/src domain/src
COPY cli/src cli/src
RUN find src domain cli -name '*.rs' -exec touch {} + \
 && /usr/local/cargo/bin/cargo build --release -p cicd-server -p cicd-cli

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends curl git git-lfs && rm -rf /var/lib/apt/lists/* && useradd --create-home --uid 10001 cicd
COPY --from=build /app/target/release/cicd-server /usr/local/bin/cicd-server
RUN mkdir -p /var/lib/forge/git /var/lib/forge/artifacts && chown -R cicd:cicd /var/lib/forge
USER cicd
EXPOSE 22801
CMD ["cicd-server"]
