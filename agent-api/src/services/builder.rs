use std::path::{Path, PathBuf};
use std::process::Command;
use crate::error::AppError;

/// Validate that a repo URL is safe to clone.
pub fn validate_repo_url(repo: &str, allowed_hosts: &[String]) -> Result<(), AppError> {
    // Only allow HTTPS to prevent local file access and SSRF
    if !repo.starts_with("https://") {
        return Err(AppError::BadRequest(
            "only HTTPS repository URLs are allowed (e.g. https://github.com/user/repo)".into()
        ));
    }

    // Block suspicious URL patterns
    if repo.contains("..") || repo.contains('\0') || repo.contains('@') {
        return Err(AppError::BadRequest("invalid repository URL".into()));
    }

    // Validate against allowed hosts
    if !allowed_hosts.is_empty() {
        let host = repo.trim_start_matches("https://").split('/').next().unwrap_or("");
        if !allowed_hosts.iter().any(|h| host == h.as_str()) {
            return Err(AppError::BadRequest(format!(
                "repository host '{host}' not allowed. Allowed: {}",
                allowed_hosts.join(", ")
            )));
        }
    }

    Ok(())
}

/// Clone a repo and build a Docker image for it.
/// Captures and returns build output for debugging.
pub async fn clone_and_build(repo: &str, branch: &str, name: &str) -> Result<(String, u16), AppError> {
    // Validate branch name — alphanumeric, hyphens, dots, underscores, slashes
    if !branch.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '.' || c == '_' || c == '/') {
        return Err(AppError::BadRequest(
            "invalid branch name: only alphanumeric, hyphens, dots, underscores, and slashes allowed".into()
        ));
    }

    let work_dir = PathBuf::from("/tmp/routeroot-builds").join(name);

    // Clean up any previous build
    if work_dir.exists() {
        std::fs::remove_dir_all(&work_dir).ok();
    }
    std::fs::create_dir_all(&work_dir)
        .map_err(|e| AppError::Internal(format!("failed to create build dir: {e}")))?;

    // Clone — capture output for debugging
    tracing::info!(repo = %repo, branch = %branch, name = %name, "cloning repository");
    let clone_output = Command::new("git")
        .args(["clone", "--depth", "1", "--branch", branch, repo, work_dir.to_str().unwrap()])
        .output()
        .map_err(|e| AppError::Internal(format!("git clone failed to execute: {e}")))?;

    if !clone_output.status.success() {
        let stderr = String::from_utf8_lossy(&clone_output.stderr);
        tracing::error!(repo = %repo, branch = %branch, stderr = %stderr, "git clone failed");
        return Err(AppError::BadRequest(format!(
            "git clone failed for {repo}@{branch}: {stderr}"
        )));
    }

    // Detect build type and get container port
    let (dockerfile_content, container_port) = detect_and_generate_dockerfile(&work_dir)?;
    tracing::info!(
        name = %name,
        container_port = container_port,
        generated_dockerfile = dockerfile_content.is_some(),
        "build type detected"
    );

    // Write Dockerfile if we generated one
    if let Some(content) = dockerfile_content {
        std::fs::write(work_dir.join("Dockerfile"), content)
            .map_err(|e| AppError::Internal(format!("failed to write Dockerfile: {e}")))?;
    }

    // Build Docker image — capture output
    let image_tag = format!("routeroot-{name}:latest");
    tracing::info!(image = %image_tag, "building docker image");
    let build_output = Command::new("docker")
        .args(["build", "-t", &image_tag, "."])
        .current_dir(&work_dir)
        .output()
        .map_err(|e| AppError::Internal(format!("docker build failed to execute: {e}")))?;

    if !build_output.status.success() {
        let stderr = String::from_utf8_lossy(&build_output.stderr);
        let stdout = String::from_utf8_lossy(&build_output.stdout);
        // Log last 50 lines of build output for debugging
        let combined = format!("{stdout}\n{stderr}");
        let last_lines: String = combined.lines().rev().take(50).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
        tracing::error!(name = %name, build_output = %last_lines, "docker build failed");
        return Err(AppError::Internal(format!(
            "docker build failed for '{name}':\n{last_lines}"
        )));
    }

    tracing::info!(name = %name, image = %image_tag, "docker build complete");

    // Cleanup build dir
    std::fs::remove_dir_all(&work_dir).ok();

    Ok((image_tag, container_port))
}

/// Returns (optional generated Dockerfile content, container port).
fn detect_and_generate_dockerfile(work_dir: &Path) -> Result<(Option<String>, u16), AppError> {
    // Already has a Dockerfile — use it as-is
    if work_dir.join("Dockerfile").exists() {
        // Try to detect EXPOSE port from existing Dockerfile
        let dockerfile = std::fs::read_to_string(work_dir.join("Dockerfile")).unwrap_or_default();
        let port = dockerfile.lines()
            .find(|l| l.trim().starts_with("EXPOSE"))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(3000);
        return Ok((None, port));
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

    // Python project
    if work_dir.join("requirements.txt").exists() || work_dir.join("pyproject.toml").exists() {
        let pip_install = if work_dir.join("requirements.txt").exists() {
            "RUN pip install --no-cache-dir -r requirements.txt"
        } else {
            "RUN pip install --no-cache-dir ."
        };
        let dockerfile = format!(
            r#"FROM python:3.12-slim
WORKDIR /app
COPY . .
{pip_install}
EXPOSE 8000
CMD ["python", "-m", "uvicorn", "main:app", "--host", "0.0.0.0", "--port", "8000"]
"#
        );
        return Ok((Some(dockerfile), 8000));
    }

    // Static site (fallback)
    if work_dir.join("index.html").exists() {
        let dockerfile = r#"FROM caddy:alpine
COPY . /usr/share/caddy
EXPOSE 80
"#;
        return Ok((Some(dockerfile.into()), 80));
    }

    Err(AppError::BadRequest(
        "Could not detect project type. Supported: Dockerfile, Node.js, Rust, Go, Python, static HTML. Add a Dockerfile to your repo.".into(),
    ))
}
