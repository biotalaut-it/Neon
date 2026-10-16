use super::{
    model::{ChatMessage, LocalModel, ModelError},
    AgentRole,
};
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct TaskPacket {
    pub task: String,
    pub context: Vec<String>,
}
#[derive(Debug, Deserialize)]
struct RouteOutput {
    roles: Vec<String>,
    #[serde(default)]
    complex: bool,
}

pub struct Router;
impl Router {
    pub fn route(task: &TaskPacket, model: &LocalModel) -> Result<Vec<AgentRole>, ModelError> {
        let system = "Kamu Mike, Router NEON. Klasifikasikan tugas ke satu atau lebih label yang sesuai dari coding, maintenance, debugging, decompose, summarize. Keluarkan hanya JSON {\"roles\":[...],\"complex\":false}. Jangan tulis kode atau menjalankan tindakan.";
        let prompt = format!(
            "Tugas pengguna:\n{}\nKonteks relevan:\n{}",
            task.task,
            task.context.join("\n")
        );
        let response = model.complete(
            &[
                ChatMessage {
                    role: "system".into(),
                    content: system.into(),
                },
                ChatMessage {
                    role: "user".into(),
                    content: prompt,
                },
            ],
            256,
        )?;
        let parsed = parse_route_output(&response.text)?;
        let mut labels: Vec<&str> = parsed.roles.iter().map(String::as_str).collect();
        if parsed.complex && !labels.contains(&"decompose") {
            labels.push("decompose");
        }
        super::route_categories(&labels)
            .map_err(|e| ModelError(format!("router category rejected: {e}")))
    }
}
pub fn route_categories(labels: &[&str]) -> Result<Vec<AgentRole>, String> {
    super::route_categories(labels).map_err(|e| e.to_string())
}
fn parse_route_output(input: &str) -> Result<RouteOutput, ModelError> {
    if input.len() > 4096 {
        return Err(ModelError("router output exceeds 4 KiB".into()));
    }
    serde_json::from_str(input)
        .map_err(|e| ModelError(format!("router did not return valid JSON categories: {e}")))
}
