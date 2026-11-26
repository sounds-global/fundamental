use std::time::Duration;

use bollard::Docker;
use bollard::container::LogOutput;
use bollard::exec::StartExecResults;
use bollard::models::{ContainerInspectResponse, ExecConfig};
use futures::StreamExt;
use tracing::{debug, info, instrument};

use crate::container_recipe::ContainerRecipe;
use crate::start_container::start_container;
use crate::test_container_error::TestContainerError;

const POLL: Duration = Duration::from_millis(250);

/// A container a program owns rather than shares: started from its own recipe under its own
/// name, so stopping or restarting it disturbs no suite.
///
/// A restart publishes new host ports, so every start reads them again.
#[derive(Debug)]
pub(crate) struct DedicatedContainer {
    recipe: ContainerRecipe,
    ports: Vec<u16>,
}

impl DedicatedContainer {
    pub(crate) async fn start(recipe: ContainerRecipe) -> Result<Self, TestContainerError> {
        let started = start_container(&recipe).await?;
        Ok(Self {
            recipe,
            ports: started.ports,
        })
    }

    pub(crate) fn ports(&self) -> &[u16] {
        &self.ports
    }

    pub(crate) fn name(&self) -> &str {
        &self.recipe.name
    }

    #[instrument(skip_all, fields(container = %self.recipe.name), err(Debug))]
    pub(crate) async fn restart(&mut self) -> Result<(), TestContainerError> {
        let docker = self.docker()?;
        docker
            .restart_container(&self.recipe.name, None)
            .await
            .map_err(|error| self.failed(&error.to_string()))?;
        self.ports = self.published(&docker).await?;
        info!(container = %self.recipe.name, ports = ?self.ports, "test containers: restarted");
        Ok(())
    }

    #[instrument(skip_all, fields(container = %self.recipe.name), err(Debug))]
    pub(crate) async fn stop(&self) -> Result<(), TestContainerError> {
        self.docker()?
            .stop_container(&self.recipe.name, None)
            .await
            .map_err(|error| self.failed(&error.to_string()))?;
        info!(container = %self.recipe.name, "test containers: stopped");
        Ok(())
    }

    #[instrument(skip_all, fields(container = %self.recipe.name), err(Debug))]
    pub(crate) async fn start_again(&mut self) -> Result<(), TestContainerError> {
        let docker = self.docker()?;
        docker
            .start_container(&self.recipe.name, None)
            .await
            .map_err(|error| self.failed(&error.to_string()))?;
        self.ports = self.published(&docker).await?;
        info!(container = %self.recipe.name, ports = ?self.ports, "test containers: started again");
        Ok(())
    }

    pub(crate) async fn until_healthy(&self) -> Result<(), TestContainerError> {
        let docker = self.docker()?;
        let deadline = tokio::time::Instant::now() + self.recipe.startup_timeout;
        let mut last = String::from("never attempted");
        while tokio::time::Instant::now() < deadline {
            match exec(&docker, &self.recipe.name, &self.recipe.healthcheck).await {
                Ok(()) => return Ok(()),
                Err(error) => {
                    debug!(container = %self.recipe.name, %error, "test containers: not healthy yet");
                    last = error;
                }
            }
            tokio::time::sleep(POLL).await;
        }
        Err(TestContainerError::NotReady {
            engine: self.recipe.image,
            endpoint: self.recipe.name.clone(),
            waited_secs: self.recipe.startup_timeout.as_secs(),
            detail: last,
        })
    }

    fn docker(&self) -> Result<Docker, TestContainerError> {
        Docker::connect_with_defaults().map_err(|error| self.failed(&error.to_string()))
    }

    fn failed(&self, detail: &str) -> TestContainerError {
        TestContainerError::Start {
            engine: self.recipe.image,
            name: self.recipe.name.clone(),
            detail: detail.to_owned(),
        }
    }

    async fn published(&self, docker: &Docker) -> Result<Vec<u16>, TestContainerError> {
        let deadline = tokio::time::Instant::now() + self.recipe.startup_timeout;
        loop {
            let inspected = docker
                .inspect_container(&self.recipe.name, None)
                .await
                .map_err(|error| self.failed(&error.to_string()))?;
            if let Some(ports) = host_ports(&inspected, &self.recipe.ports) {
                return Ok(ports);
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(TestContainerError::NoHostPort {
                    engine: self.recipe.image,
                    name: self.recipe.name.clone(),
                    port: self.recipe.ports.first().copied().unwrap_or_default(),
                    detail: "the restarted container published no host port".to_owned(),
                });
            }
            tokio::time::sleep(POLL).await;
        }
    }
}

fn host_ports(inspected: &ContainerInspectResponse, exposed: &[u16]) -> Option<Vec<u16>> {
    let bindings = inspected.network_settings.as_ref()?.ports.as_ref()?;
    exposed
        .iter()
        .map(|port| {
            bindings
                .get(&format!("{port}/tcp"))?
                .as_ref()?
                .iter()
                .find_map(|binding| binding.host_port.as_deref()?.parse().ok())
        })
        .collect()
}

async fn exec(docker: &Docker, container: &str, command: &[&str]) -> Result<(), String> {
    let config = ExecConfig {
        attach_stdout: Some(true),
        attach_stderr: Some(true),
        cmd: Some(command.iter().map(|part| (*part).to_owned()).collect()),
        ..ExecConfig::default()
    };
    let created = docker
        .create_exec(container, config)
        .await
        .map_err(|error| error.to_string())?;
    let started = docker
        .start_exec(&created.id, None)
        .await
        .map_err(|error| error.to_string())?;
    let mut said = String::new();
    if let StartExecResults::Attached { mut output, .. } = started {
        while let Some(chunk) = output.next().await {
            if let Ok(LogOutput::StdErr { message } | LogOutput::StdOut { message }) = chunk {
                said.push_str(&String::from_utf8_lossy(&message));
            }
        }
    }
    let inspected = docker
        .inspect_exec(&created.id)
        .await
        .map_err(|error| error.to_string())?;
    match inspected.exit_code {
        Some(0) => Ok(()),
        code => Err(format!("exited {code:?}: {}", said.trim())),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use bollard::models::{NetworkSettings, PortBinding};

    use super::*;

    fn inspected(bindings: &[(&str, &str)]) -> ContainerInspectResponse {
        let ports: HashMap<String, Option<Vec<PortBinding>>> = bindings
            .iter()
            .map(|(exposed, host)| {
                (
                    (*exposed).to_owned(),
                    Some(vec![PortBinding {
                        host_ip: Some("127.0.0.1".to_owned()),
                        host_port: Some((*host).to_owned()),
                    }]),
                )
            })
            .collect();
        ContainerInspectResponse {
            network_settings: Some(NetworkSettings {
                ports: Some(ports),
                ..NetworkSettings::default()
            }),
            ..ContainerInspectResponse::default()
        }
    }

    #[test]
    fn the_host_ports_are_read_in_the_recipes_order() {
        let found = inspected(&[("9042/tcp", "32801"), ("5433/tcp", "32800")]);
        assert_eq!(host_ports(&found, &[5433, 9042]), Some(vec![32800, 32801]));
    }

    #[test]
    fn a_port_not_yet_published_is_none() {
        let found = inspected(&[("5433/tcp", "32800")]);
        assert_eq!(host_ports(&found, &[5433, 9042]), None);
    }
}
