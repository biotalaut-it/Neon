use serde::{Deserialize, Serialize};
use std::{error::Error, fmt};

pub mod model;
pub mod router;
pub mod supervisor;

const MAX_PACKET_BYTES: usize = 64 * 1024;
const MAX_FIELD_BYTES: usize = 32 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRole {
    Programmer,
    Maintenance,
    Debugger,
    Decomposer,
    Router,
    Summarizer,
    Leader,
}
impl AgentRole {
    pub const fn all() -> [Self; 7] {
        [
            Self::Leader,
            Self::Programmer,
            Self::Maintenance,
            Self::Debugger,
            Self::Decomposer,
            Self::Router,
            Self::Summarizer,
        ]
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Leader => "Naon",
            Self::Programmer => "Susan",
            Self::Maintenance => "Fani",
            Self::Debugger => "Deby",
            Self::Decomposer => "Dyor",
            Self::Router => "Mike",
            Self::Summarizer => "Jordan",
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::Leader => "Leader",
            Self::Programmer => "Programmer",
            Self::Maintenance => "Maintenance",
            Self::Debugger => "Debugger / Pemecah Masalah",
            Self::Decomposer => "Decomposer",
            Self::Router => "Router",
            Self::Summarizer => "Summarizer",
        }
    }

    pub fn parse_label(label: &str) -> Result<Self, ProtocolError> {
        match label.trim().to_ascii_lowercase().as_str() {
            "programmer" | "coding" | "programming" | "susan" => Ok(Self::Programmer),
            "maintenance" | "maintain" | "fani" => Ok(Self::Maintenance),
            "debugger" | "debugging" | "problem_solver" | "deby" => Ok(Self::Debugger),
            "decomposer" | "decompose" | "dyor" => Ok(Self::Decomposer),
            "router" | "routing" | "mike" => Ok(Self::Router),
            "summarizer" | "summarize" | "summary" | "jordan" => Ok(Self::Summarizer),
            "leader" | "naon" => Ok(Self::Leader),
            other => Err(ProtocolError(format!(
                "unknown agent role/category: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PacketStatus {
    Complete,
    NeedsInput,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentPacket {
    pub role: AgentRole,
    pub task: String,
    #[serde(default)]
    pub context: Vec<String>,
    #[serde(default)]
    pub constraints: Vec<String>,
    pub result: Option<String>,
    #[serde(default)]
    pub evidence: Vec<String>,
    pub status: PacketStatus,
    pub confidence: Option<f32>,
}
impl AgentPacket {
    pub fn encode(&self) -> Result<String, ProtocolError> {
        self.validate()?;
        let encoded = serde_json::to_string(self).map_err(|e| ProtocolError(e.to_string()))?;
        if encoded.len() > MAX_PACKET_BYTES {
            return Err(ProtocolError("agent packet exceeds 64 KiB".into()));
        }
        Ok(encoded)
    }
    pub fn decode(input: &str) -> Result<Self, ProtocolError> {
        if input.len() > MAX_PACKET_BYTES {
            return Err(ProtocolError("agent packet exceeds 64 KiB".into()));
        }
        let packet: Self = serde_json::from_str(input)
            .map_err(|e| ProtocolError(format!("invalid agent packet: {e}")))?;
        packet.validate()?;
        Ok(packet)
    }
    fn validate(&self) -> Result<(), ProtocolError> {
        if self.task.trim().is_empty() {
            return Err(ProtocolError("packet task must not be empty".into()));
        }
        for field in std::iter::once(&self.task)
            .chain(self.context.iter())
            .chain(self.constraints.iter())
            .chain(self.result.iter())
            .chain(self.evidence.iter())
        {
            if field.len() > MAX_FIELD_BYTES {
                return Err(ProtocolError("packet field exceeds 32 KiB".into()));
            }
        }
        if self
            .confidence
            .is_some_and(|v| !v.is_finite() || !(0.0..=1.0).contains(&v))
        {
            return Err(ProtocolError("confidence must be between 0 and 1".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolError(pub String);
impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl Error for ProtocolError {}

pub fn route_categories(labels: &[&str]) -> Result<Vec<AgentRole>, ProtocolError> {
    if labels.is_empty() {
        return Err(ProtocolError("router returned no categories".into()));
    }
    let mut roles = Vec::new();
    for label in labels {
        let role = AgentRole::parse_label(label)?;
        if matches!(
            role,
            AgentRole::Router | AgentRole::Leader | AgentRole::Summarizer
        ) {
            continue;
        }
        if !roles.contains(&role) {
            roles.push(role);
        }
    }
    if roles.is_empty() {
        return Err(ProtocolError(
            "router returned no executable specialist category".into(),
        ));
    }
    Ok(roles)
}

#[cfg(test)]
mod names_tests {
    use super::AgentRole;

    #[test]
    fn all_roles_have_the_requested_names_and_name_aliases_parse() {
        let names = AgentRole::all().map(AgentRole::display_name);
        assert_eq!(
            names,
            ["Naon", "Susan", "Fani", "Deby", "Dyor", "Mike", "Jordan"]
        );
        assert_eq!(AgentRole::parse_label("naon").unwrap(), AgentRole::Leader);
        assert_eq!(
            AgentRole::parse_label("susan").unwrap(),
            AgentRole::Programmer
        );
        assert_eq!(
            AgentRole::parse_label("fani").unwrap(),
            AgentRole::Maintenance
        );
        assert_eq!(AgentRole::parse_label("deby").unwrap(), AgentRole::Debugger);
        assert_eq!(
            AgentRole::parse_label("dyor").unwrap(),
            AgentRole::Decomposer
        );
        assert_eq!(AgentRole::parse_label("mike").unwrap(), AgentRole::Router);
        assert_eq!(
            AgentRole::parse_label("jordan").unwrap(),
            AgentRole::Summarizer
        );
    }
}
