use super::{
    model::{ChatMessage, LocalModel, ModelError},
    AgentRole,
};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskIntent {
    Compute,
    Coding,
    Debugging,
    General,
}

impl TaskIntent {
    pub fn classify(task: &str) -> Result<Self, ModelError> {
        let task = task.to_ascii_lowercase();
        let has_compute_signal = task.contains("hitung")
            || task.contains("jumlah")
            || task.contains("total")
            || task.contains("kuadrat")
            || task.contains("akar")
            || task.contains("sum")
            || task.contains("range")
            || task.contains("map")
            || task.contains("filter")
            || task.contains("take")
            || task.contains("compute")
            || task.contains("calculate")
            || task.contains("+ ")
            || task.contains(" - ")
            || task.contains(" * ")
            || task.contains(" / ")
            || task.contains("%")
            || task.contains("=");
        let has_coding_signal = task.contains("kode")
            || task.contains("code")
            || task.contains("rust")
            || task.contains("implement")
            || task.contains("function")
            || task.contains("refactor")
            || task.contains("patch")
            || task.contains("write code");
        let has_debug_signal = task.contains("debug")
            || task.contains("bug")
            || task.contains("error")
            || task.contains("panic")
            || task.contains("traceback")
            || task.contains("fix")
            || task.contains("crash");
        let has_concept_signal = task.contains("apa itu")
            || task.contains("apa arti")
            || task.contains("jelaskan")
            || task.contains("bagaimana")
            || task.contains("mengapa")
            || task.contains("what is");

        if has_concept_signal {
            return Ok(Self::General);
        }
        if has_debug_signal {
            return Ok(Self::Debugging);
        }
        if has_coding_signal {
            return Ok(Self::Coding);
        }
        if has_compute_signal {
            return Ok(Self::Compute);
        }
        Ok(Self::General)
    }
}

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
