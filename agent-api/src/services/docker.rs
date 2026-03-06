use bollard::Docker;
use bollard::container::{Config, CreateContainerOptions, LogsOptions, RemoveContainerOptions, StopContainerOptions};
use bollard::models::{HostConfig, PortBinding};
use std::collections::HashMap;
use futures_util::StreamExt;

use crate::error::AppError;

pub struct DockerService {
    client: Docker,
}

impl DockerService {
    pub fn new() -> Result<Self, AppError> {
        let client = Docker::connect_with_local_defaults()
            .map_err(|e| AppError::Internal(format!("failed to connect to docker: {e}")))?;
        Ok(Self { client })
    }

    pub async fn run_container(
        &self,
        name: &str,
        image: &str,
        port: u16,
        container_port: u16,
        max_memory_mb: u64,
        max_cpus: u64,
    ) -> Result<String, AppError> {
        let mut port_bindings = HashMap::new();
        port_bindings.insert(
            format!("{container_port}/tcp"),
            Some(vec![PortBinding {
                host_ip: Some("0.0.0.0".into()),
                host_port: Some(port.to_string()),
            }]),
        );

        let host_config = HostConfig {
            port_bindings: Some(port_bindings),
            memory: Some((max_memory_mb * 1024 * 1024) as i64),
            nano_cpus: Some((max_cpus * 1_000_000_000) as i64),
            ..Default::default()
        };

        let container_name = format!("routeroot-{name}");
        let config = Config {
            image: Some(image.to_string()),
            host_config: Some(host_config),
            labels: Some(HashMap::from([
                ("routeroot".into(), "true".into()),
                ("routeroot.name".into(), name.into()),
            ])),
            ..Default::default()
        };

        let response = self.client
            .create_container(
                Some(CreateContainerOptions { name: &container_name, platform: None }),
                config,
            )
            .await?;

        self.client.start_container::<String>(&response.id, None).await?;

        Ok(response.id)
    }

    pub async fn stop_container(&self, container_id: &str) -> Result<(), AppError> {
        self.client
            .stop_container(container_id, Some(StopContainerOptions { t: 10 }))
            .await
            .ok(); // Ignore errors if already stopped

        self.client
            .remove_container(container_id, Some(RemoveContainerOptions { force: true, ..Default::default() }))
            .await
            .ok();

        Ok(())
    }

    pub async fn get_logs(&self, container_id: &str, tail: usize) -> Result<Vec<String>, AppError> {
        let options = LogsOptions::<String> {
            stdout: true,
            stderr: true,
            tail: tail.to_string(),
            ..Default::default()
        };

        let mut stream = self.client.logs(container_id, Some(options));
        let mut lines = Vec::new();

        while let Some(Ok(log)) = stream.next().await {
            lines.push(log.to_string());
        }

        Ok(lines)
    }

    /// Stop and remove a container by its name (e.g. "routeroot-{name}").
    /// Useful for cleaning up orphan containers when we don't have a container_id.
    pub async fn stop_container_by_name(&self, container_name: &str) -> Result<(), AppError> {
        self.client
            .stop_container(container_name, Some(StopContainerOptions { t: 10 }))
            .await
            .ok();

        self.client
            .remove_container(container_name, Some(RemoveContainerOptions { force: true, ..Default::default() }))
            .await
            .ok();

        Ok(())
    }

    pub fn client(&self) -> &Docker {
        &self.client
    }
}
