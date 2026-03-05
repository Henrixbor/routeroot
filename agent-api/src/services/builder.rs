use std::path::{Path, PathBuf};
use std::process::Command;
use crate::error::AppError;

/// Clone a repo and build a Docker image for it.
pub async fn clone_and_build(repo: &str, branch: &str, name: &str) -> Result<(String, u16), AppError> {
    let work_dir = PathBuf::from("/tmp/agentdns-builds").join(name);

    // Clean up any previous build
    if work_dir.exists() {
        std::fs::remove_dir_all(&work_dir).ok();
    }
    std::fs::create_dir_all(&work_dir)
        .map_err(|e| AppError::Internal(format!("failed to create build dir: {e}")))?;

    // Clone
    let clone_status = Command::new("git")
        .args(["clone", "--depth", "1", "--branch", branch, repo, work_dir.to_str().unwrap()])
        .status()
        .map_err(|e| AppError::Internal(format!("git clone failed: {e}")))?;

    if !clone_status.success() {
        return Err(AppError::BadRequest(format!("git clone failed for {repo}@{branch}")));
    }

    // Detect build type and get container port
    let (dockerfile_content, container_port) = detect_and_generate_dockerfile(&work_dir)?;

    // Write Dockerfile if we generated one
    if let Some(content) = dockerfile_content {
        std::fs::write(work_dir.join("Dockerfile"), content)
            .map_err(|e| AppError::Internal(format!("failed to write Dockerfile: {e}")))?;
    }

    // Build Docker image
    let image_tag = format!("agentdns-{name}:latest");
    let build_status = Command::new("docker")
        .args(["build", "-t", &image_tag, "."])
        .current_dir(&work_dir)
        .status()
        .map_err(|e| AppError::Internal(format!("docker build failed: {e}")))?;

    if !build_status.success() {
        return Err(AppError::Internal("docker build failed".into()));
    }

    // Cleanup build dir
    std::fs::remove_dir_all(&work_dir).ok();

    Ok((image_tag, container_port))
}

/// Returns (optional generated Dockerfile content, container port).
/// If the repo already has a Dockerfile, returns (None, 3000) — assumes port 3000 as default.
fn detect_and_generate_dockerfile(work_dir: &Path) -> Result<(Option<String>, u16), AppError> {
    // Already has a Dockerfile — use it as-is
    if work_dir.join("Dockerfile").exists() {
        return Ok((None, 3000));
    }

    // Node.js project
    if work_dir.join("package.json").exists() {
        let pkg = std::fs::read_to_string(work_dir.join("package.json")).unwrap_or_default();
        let port = if pkg.contains("next") { 3000 }
            else if pkg.contains("vite") || pkg.contains("astro") { 4321 }
            else { 3000 };

        let dockerfile = format!(
            r#"FROM node:22-alpine
WORKDIR /app
COPY package*.json ./
RUN npm ci
COPY . .
RUN npm run build 2>/dev/null || true
EXPOSE {port}
CMD ["npm", "start"]
"#
        );
        return Ok((Some(dockerfile), port));
    }

    // Rust project
    if work_dir.join("Cargo.toml").exists() {
        let dockerfile = r#"FROM rust:1.83-bookworm AS builder
WORKDIR /app
COPY . .
RUN cargo build --release
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/* /usr/local/bin/
EXPOSE 3000
CMD ["sh", "-c", "ls /usr/local/bin/ | head -1 | xargs -I{} /usr/local/bin/{}"]
"#;
        return Ok((Some(dockerfile.into()), 3000));
    }

    // Go project
    if work_dir.join("go.mod").exists() {
        let dockerfile = r#"FROM golang:1.23 AS builder
WORKDIR /app
COPY go.* ./
RUN go mod download
COPY . .
RUN CGO_ENABLED=0 go build -o /app/server .
FROM alpine:latest
RUN apk add --no-cache ca-certificates
COPY --from=builder /app/server /server
EXPOSE 3000
CMD ["/server"]
"#;
        return Ok((Some(dockerfile.into()), 3000));
    }

    // Static site (fallback)
    if work_dir.join("index.html").exists() {
        let dockerfile = r#"FROM caddy:alpine
COPY . /srv
EXPOSE 80
"#;
        return Ok((Some(dockerfile.into()), 80));
    }

    Err(AppError::BadRequest(
        "Could not detect project type. Add a Dockerfile to your repo.".into(),
    ))
}
