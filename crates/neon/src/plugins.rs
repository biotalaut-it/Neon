use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

pub const NEON_PLUGIN_API_VERSION: &str = "0.1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginPermission {
    WorkspaceRead,
    WorkspaceSearch,
    GitRead,
    Compute,
    WorkspaceWrite,
    ExecuteTests,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginTool {
    pub name: String,
    pub target_tool: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    pub id: String,
    pub version: String,
    pub api_version: String,
    pub permissions: Vec<PluginPermission>,
    pub tools: Vec<PluginTool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredPlugin {
    pub id: String,
    pub version: String,
    pub api_version: String,
    pub permissions: Vec<PluginPermission>,
    pub enabled: bool,
}

pub struct PluginRegistry {
    conn: Connection,
}
impl PluginRegistry {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create plugin registry directory: {error}"))?;
        }
        let conn = Connection::open(path)
            .map_err(|error| format!("cannot open plugin registry: {error}"))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS neon_plugins(
                id TEXT PRIMARY KEY,
                version TEXT NOT NULL,
                api_version TEXT NOT NULL,
                permissions_json TEXT NOT NULL,
                tools_json TEXT NOT NULL,
                enabled INTEGER NOT NULL CHECK(enabled IN (0,1)),
                created_at INTEGER NOT NULL
            );",
        )
        .map_err(|error| format!("cannot migrate plugin registry: {error}"))?;
        Ok(Self { conn })
    }

    pub fn register(&mut self, manifest: &PluginManifest) -> Result<(), String> {
        validate_manifest(manifest)?;
        let permissions =
            serde_json::to_string(&manifest.permissions).map_err(|error| error.to_string())?;
        let tools = serde_json::to_string(&manifest.tools).map_err(|error| error.to_string())?;
        self.conn
            .execute(
                "INSERT INTO neon_plugins(id,version,api_version,permissions_json,tools_json,enabled,created_at) VALUES(?1,?2,?3,?4,?5,0,?6)",
                params![manifest.id, manifest.version, manifest.api_version, permissions, tools, now_epoch()],
            )
            .map_err(|error| format!("cannot register plugin: {error}"))?;
        Ok(())
    }

    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> Result<(), String> {
        if enabled {
            let stored: Option<(String, String, String, String)> = self
                .conn
                .query_row(
                    "SELECT version,api_version,permissions_json,tools_json FROM neon_plugins WHERE id=?1",
                    [id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .optional()
                .map_err(|error| error.to_string())?;
            let (version, api_version, permissions_json, tools_json) =
                stored.ok_or("plugin id not found")?;
            let manifest = PluginManifest {
                id: id.into(),
                version,
                api_version,
                permissions: serde_json::from_str(&permissions_json)
                    .map_err(|error| format!("invalid stored plugin permissions: {error}"))?,
                tools: serde_json::from_str(&tools_json)
                    .map_err(|error| format!("invalid stored plugin tools: {error}"))?,
            };
            validate_manifest(&manifest)?;
        }
        let changed = self
            .conn
            .execute(
                "UPDATE neon_plugins SET enabled=?1 WHERE id=?2",
                params![enabled, id],
            )
            .map_err(|error| error.to_string())?;
        if changed == 0 {
            return Err("plugin id not found".into());
        }
        Ok(())
    }

    pub fn remove(&mut self, id: &str) -> Result<(), String> {
        let changed = self
            .conn
            .execute("DELETE FROM neon_plugins WHERE id=?1", [id])
            .map_err(|error| error.to_string())?;
        if changed == 0 {
            return Err("plugin id not found".into());
        }
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<RegisteredPlugin>, String> {
        let mut statement = self.conn.prepare("SELECT id,version,api_version,permissions_json,enabled FROM neon_plugins ORDER BY id").map_err(|error|error.to_string())?;
        let rows = statement
            .query_map([], |row| {
                let permissions: String = row.get(3)?;
                let permissions = serde_json::from_str(&permissions).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        3,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?;
                Ok(RegisteredPlugin {
                    id: row.get(0)?,
                    version: row.get(1)?,
                    api_version: row.get(2)?,
                    permissions,
                    enabled: row.get::<_, bool>(4)?,
                })
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }

    pub fn resolve_tool(
        &self,
        id: &str,
        name: &str,
    ) -> Result<Option<(String, Vec<PluginPermission>)>, String> {
        let row: Option<(String, String, String)> = self
            .conn
            .query_row(
                "SELECT version,api_version,permissions_json FROM neon_plugins WHERE id=?1 AND enabled=1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        let Some((version, api_version, permissions_json)) = row else {
            return Ok(None);
        };
        if api_version != NEON_PLUGIN_API_VERSION {
            return Err(format!("plugin API version {api_version} is incompatible"));
        }
        validate_version(&version)?;
        let permissions: Vec<PluginPermission> =
            serde_json::from_str(&permissions_json).map_err(|error| error.to_string())?;
        let tools_json: String = self
            .conn
            .query_row(
                "SELECT tools_json FROM neon_plugins WHERE id=?1",
                [id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let tools: Vec<PluginTool> =
            serde_json::from_str(&tools_json).map_err(|error| error.to_string())?;
        validate_manifest(&PluginManifest {
            id: id.into(),
            version,
            api_version,
            permissions: permissions.clone(),
            tools: tools.clone(),
        })?;
        Ok(tools
            .into_iter()
            .find(|tool| tool.name == name)
            .map(|tool| (tool.target_tool, permissions)))
    }
}

pub fn validate_manifest(manifest: &PluginManifest) -> Result<(), String> {
    validate_identifier(&manifest.id)?;
    validate_version(&manifest.version)?;
    if manifest.api_version != NEON_PLUGIN_API_VERSION {
        return Err(format!(
            "plugin API {} is incompatible; expected {}",
            manifest.api_version, NEON_PLUGIN_API_VERSION
        ));
    }
    if manifest.tools.is_empty() || manifest.tools.len() > 32 {
        return Err("plugin must declare 1..=32 tools".into());
    }
    let permissions = manifest.permissions.iter().copied().collect::<HashSet<_>>();
    if permissions.len() != manifest.permissions.len() {
        return Err("plugin permissions must be unique".into());
    }
    let mut names = HashSet::new();
    for tool in &manifest.tools {
        validate_identifier(&tool.name)?;
        if !names.insert(&tool.name) {
            return Err("plugin tool names must be unique".into());
        }
        if !matches!(
            tool.target_tool.as_str(),
            "workspace_list"
                | "workspace_read"
                | "workspace_search"
                | "symbols_search"
                | "dependencies_search"
                | "knowledge_graph"
                | "git_status"
                | "git_diff"
                | "neonatom_eval"
                | "workspace_propose_write"
                | "cargo_test"
                | "cargo_check"
        ) {
            return Err(format!(
                "plugin target tool is not allowlisted: {}",
                tool.target_tool
            ));
        }
        let required = permission_for_target(&tool.target_tool);
        if !permissions.contains(&required) {
            return Err(format!(
                "plugin tool {} requires permission {required:?}",
                tool.name
            ));
        }
    }
    Ok(())
}

pub fn permission_for_target(target_tool: &str) -> PluginPermission {
    match target_tool {
        "workspace_list" | "workspace_read" => PluginPermission::WorkspaceRead,
        "workspace_search" | "symbols_search" | "dependencies_search" | "knowledge_graph" => {
            PluginPermission::WorkspaceSearch
        }
        "git_status" | "git_diff" => PluginPermission::GitRead,
        "neonatom_eval" => PluginPermission::Compute,
        "workspace_propose_write" => PluginPermission::WorkspaceWrite,
        "cargo_test" | "cargo_check" => PluginPermission::ExecuteTests,
        _ => unreachable!("manifest validation rejects unknown targets"),
    }
}

fn validate_identifier(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(
            "plugin identifiers must be 1..=64 ASCII letters, digits, '.', '_' or '-'".into(),
        );
    }
    Ok(())
}

fn validate_version(value: &str) -> Result<(), String> {
    let parts = value.split('.').collect::<Vec<_>>();
    if parts.len() != 3
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err("plugin version must use numeric MAJOR.MINOR.PATCH form".into());
    }
    Ok(())
}

fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{PluginManifest, PluginPermission, PluginRegistry, PluginTool};
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn db() -> PathBuf {
        std::env::temp_dir().join(format!(
            "neon-plugins-{}-{}.db",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn manifest() -> PluginManifest {
        PluginManifest {
            id: "local-search".into(),
            version: "1.0.0".into(),
            api_version: "0.1".into(),
            permissions: vec![PluginPermission::WorkspaceSearch],
            tools: vec![PluginTool {
                name: "find-source".into(),
                target_tool: "workspace_search".into(),
            }],
        }
    }

    #[test]
    fn plugin_is_disabled_until_approved_and_has_compatible_permissions() {
        let path = db();
        let mut registry = PluginRegistry::open(&path).unwrap();
        registry.register(&manifest()).unwrap();
        assert!(registry
            .resolve_tool("local-search", "find-source")
            .unwrap()
            .is_none());
        registry.set_enabled("local-search", true).unwrap();
        let resolved = registry
            .resolve_tool("local-search", "find-source")
            .unwrap()
            .unwrap();
        assert_eq!(resolved.0, "workspace_search");
        assert_eq!(resolved.1, vec![PluginPermission::WorkspaceSearch]);
        assert!(registry.list().unwrap()[0].enabled);
        drop(registry);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn incompatible_api_versions_and_unlisted_tools_are_rejected() {
        let mut incompatible = manifest();
        incompatible.api_version = "9.0".into();
        assert!(super::validate_manifest(&incompatible).is_err());
        let mut arbitrary = manifest();
        arbitrary.tools[0].target_tool = "run_shell".into();
        assert!(super::validate_manifest(&arbitrary).is_err());
    }
}
