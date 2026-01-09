//! Configuration registry for loading and managing configs at runtime.
//!
//! The registry provides a centralized way to load and access chain,
//! protocol, and deployment configurations from the config directory.

use super::{ChainConfig, DeploymentConfig, ProtocolConfig};
use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;
use tracing::{debug, info};

/// Configuration registry for runtime config management.
#[derive(Debug, Default)]
pub struct ConfigRegistry {
    /// Chain configurations indexed by chain ID
    chains: HashMap<u64, ChainConfig>,
    /// Chain configurations indexed by config file name (e.g., "hyperliquid")
    chains_by_name: HashMap<String, u64>,
    /// Protocol configurations indexed by protocol ID
    protocols: HashMap<String, ProtocolConfig>,
    /// Deployment configurations indexed by deployment name
    deployments: HashMap<String, DeploymentConfig>,
}

impl ConfigRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Load all configurations from a directory.
    ///
    /// Expected structure:
    /// ```text
    /// config/
    ///   chains/
    ///     ethereum.toml
    ///     arbitrum.toml
    ///   protocols/
    ///     aave-v3-ethereum.toml
    ///     hyperlend.toml
    ///   deployments/
    ///     hyperlend-prod.toml
    /// ```
    pub fn load_from_dir(config_dir: impl AsRef<Path>) -> Result<Self> {
        let config_dir = config_dir.as_ref();
        info!(config_dir = %config_dir.display(), "Loading configuration registry");

        let mut registry = Self::new();

        // Load chain configs
        let chains_dir = config_dir.join("chains");
        if chains_dir.exists() {
            registry.load_chains(&chains_dir)?;
        }

        // Load protocol configs
        let protocols_dir = config_dir.join("protocols");
        if protocols_dir.exists() {
            registry.load_protocols(&protocols_dir)?;
        }

        // Load deployment configs
        let deployments_dir = config_dir.join("deployments");
        if deployments_dir.exists() {
            registry.load_deployments(&deployments_dir)?;
        }

        info!(
            chains = registry.chains.len(),
            protocols = registry.protocols.len(),
            deployments = registry.deployments.len(),
            "Configuration registry loaded"
        );

        Ok(registry)
    }

    /// Load chain configs from a directory.
    fn load_chains(&mut self, dir: &Path) -> Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().is_some_and(|e| e == "toml") {
                // Get file stem (name without extension) for name-based lookup
                let file_name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string();

                match ChainConfig::from_file(&path) {
                    Ok(mut config) => {
                        config.expand_env_vars();
                        let chain_id = config.chain.chain_id;
                        debug!(
                            chain_id = chain_id,
                            name = %config.chain.name,
                            file = %path.display(),
                            "Loaded chain config"
                        );
                        self.chains.insert(chain_id, config);
                        self.chains_by_name.insert(file_name, chain_id);
                    }
                    Err(e) => {
                        tracing::warn!(
                            file = %path.display(),
                            error = %e,
                            "Failed to load chain config"
                        );
                    }
                }
            }
        }
        Ok(())
    }

    /// Load protocol configs from a directory.
    fn load_protocols(&mut self, dir: &Path) -> Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().is_some_and(|e| e == "toml") {
                match ProtocolConfig::from_file(&path) {
                    Ok(config) => {
                        let id = config.protocol.id.clone();
                        debug!(
                            protocol_id = %id,
                            name = %config.protocol.name,
                            file = %path.display(),
                            "Loaded protocol config"
                        );
                        self.protocols.insert(id, config);
                    }
                    Err(e) => {
                        tracing::warn!(
                            file = %path.display(),
                            error = %e,
                            "Failed to load protocol config"
                        );
                    }
                }
            }
        }
        Ok(())
    }

    /// Load deployment configs from a directory.
    fn load_deployments(&mut self, dir: &Path) -> Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().is_some_and(|e| e == "toml") {
                match DeploymentConfig::from_file(&path) {
                    Ok(config) => {
                        let name = config.deployment.name.clone();
                        debug!(
                            deployment = %name,
                            file = %path.display(),
                            "Loaded deployment config"
                        );
                        self.deployments.insert(name, config);
                    }
                    Err(e) => {
                        tracing::warn!(
                            file = %path.display(),
                            error = %e,
                            "Failed to load deployment config"
                        );
                    }
                }
            }
        }
        Ok(())
    }

    /// Get chain config by chain ID.
    pub fn get_chain(&self, chain_id: u64) -> Option<&ChainConfig> {
        self.chains.get(&chain_id)
    }

    /// Get protocol config by protocol ID.
    pub fn get_protocol(&self, protocol_id: &str) -> Option<&ProtocolConfig> {
        self.protocols.get(protocol_id)
    }

    /// Get deployment config by name.
    pub fn get_deployment(&self, name: &str) -> Option<&DeploymentConfig> {
        self.deployments.get(name)
    }

    /// Get all chain IDs.
    pub fn chain_ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.chains.keys().copied()
    }

    /// Get all protocol IDs.
    pub fn protocol_ids(&self) -> impl Iterator<Item = &str> {
        self.protocols.keys().map(String::as_str)
    }

    /// Get all deployment names.
    pub fn deployment_names(&self) -> impl Iterator<Item = &str> {
        self.deployments.keys().map(String::as_str)
    }

    /// Get protocols for a specific chain.
    pub fn protocols_for_chain(&self, chain_id: u64) -> Vec<&ProtocolConfig> {
        self.protocols
            .values()
            .filter(|p| p.protocol.chain_id == chain_id)
            .collect()
    }

    /// Get chain config by name (file stem).
    pub fn get_chain_by_name(&self, name: &str) -> Option<&ChainConfig> {
        self.chains_by_name
            .get(name)
            .and_then(|id| self.chains.get(id))
    }

    /// Get a full deployment with its chain and protocol configs.
    /// The deployment references chain and protocol by config file name.
    pub fn get_full_deployment(
        &self,
        name: &str,
    ) -> Option<(&DeploymentConfig, &ChainConfig, &ProtocolConfig)> {
        let deployment = self.deployments.get(name)?;
        // Deployment references configs by file name (e.g., "hyperliquid", "hyperlend")
        let chain = self.get_chain_by_name(&deployment.deployment.chain)?;
        let protocol = self.protocols.get(&deployment.deployment.protocol)?;
        Some((deployment, chain, protocol))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_registry() {
        let registry = ConfigRegistry::new();
        assert!(registry.get_chain(1).is_none());
        assert!(registry.get_protocol("test").is_none());
    }
}
