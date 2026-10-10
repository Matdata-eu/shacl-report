# syntax=docker/dockerfile:1

# Build stage: compile the release binary. Web assets are embedded at compile
# time, so the binary is all the runtime image needs.
FROM rust:1-slim-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
# Build the dependencies on their own first so code changes reuse that layer.
RUN mkdir src && echo 'fn main() {}' > src/main.rs && touch src/lib.rs \
    && cargo build --release --locked \
    && rm -rf src
COPY build.rs ./
COPY src ./src
COPY assets ./assets
COPY THIRD_PARTY_NOTICES.md ./
RUN touch src/main.rs src/lib.rs && cargo build --release --locked

# Runtime stage: glibc and nothing else, running as an unprivileged user.
# HTTPS uses rustls with built-in root certificates, so no CA bundle is needed.
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /src/target/release/shacl-report /usr/local/bin/shacl-report
COPY LICENSE THIRD_PARTY_NOTICES.md /usr/share/doc/shacl-report/

# Serve the viewer as a public, multi-user website on port 8080. Every
# SHACL_REPORT_* variable maps to a `serve` flag; see the README.
ENV SHACL_REPORT_BIND=0.0.0.0:8080 \
    SHACL_REPORT_PUBLIC=true
EXPOSE 8080
WORKDIR /data
ENTRYPOINT ["shacl-report"]
CMD ["serve", "--no-open"]
