use crate::agent::model::ModelConfig;
use std::{
    net::{IpAddr, Ipv4Addr},
    path::PathBuf,
    time::Duration,
};

#[derive(Debug, Clone)]
pub struct Config {
    pub model: ModelConfig,
    pub workspace: PathBuf,
    pub data_dir: PathBuf,
    pub max_agent_calls: usize,
}
impl Config {
    pub fn from_env() -> Result<Self, String> {
        let workspace = workspace_dir()?;
        let data_dir = data_dir()?;
        let mut model = ModelConfig::from_env()?;
        model.host = IpAddr::V4(Ipv4Addr::LOCALHOST);
        model.startup_timeout = Duration::from_secs(45);
        let max_agent_calls = std::env::var("NEON_AGENT_CALLS")
            .ok()
            .map(|s| {
                s.parse::<usize>()
                    .map_err(|_| "invalid NEON_AGENT_CALLS".to_string())
            })
            .transpose()?
            .unwrap_or(8);
        if !(3..=12).contains(&max_agent_calls) {
            return Err("NEON_AGENT_CALLS must be in 3..=12".into());
        }
        Ok(Self {
            model,
            workspace,
            data_dir,
            max_agent_calls,
        })
    }
}
pub fn workspace_dir() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or("HOME is not set")?;
    let path = std::env::var_os("NEON_WORKSPACE")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("neon-workspace"));
    std::fs::create_dir_all(&path).map_err(|e| format!("cannot create workspace: {e}"))?;
    Ok(path)
}
pub fn data_dir() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or("HOME is not set")?;
    let path = std::env::var_os("NEON_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share/neon"));
    std::fs::create_dir_all(&path)
        .map_err(|e| format!("cannot create NEON data directory: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("cannot secure data directory permissions: {e}"))?;
    }
    Ok(path)
}
