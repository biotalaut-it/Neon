use super::{
    model::{ChatMessage, LocalModel, ModelError},
    router::{Router, TaskPacket},
    AgentPacket, AgentRole, PacketStatus,
};
use neonatom::{
    eval as eval_neonatom, parse as parse_neonatom, Limits as AtomLimits, Value as AtomValue,
};
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct AgentBudget {
    pub max_calls: usize,
    pub max_tokens_per_call: u32,
    pub max_context_bytes: usize,
}
impl Default for AgentBudget {
    fn default() -> Self {
        Self {
            max_calls: 8,
            max_tokens_per_call: 768,
            max_context_bytes: 24 * 1024,
        }
    }
}
#[derive(Debug, Clone)]
pub struct AgentResponse {
    pub answer: String,
    pub roles_used: Vec<AgentRole>,
    pub calls_used: usize,
    pub evidence: Vec<String>,
}
pub struct Supervisor;

impl Supervisor {
    pub fn run(
        model: &LocalModel,
        input: &str,
        memories: &[String],
        budget: AgentBudget,
    ) -> Result<AgentResponse, ModelError> {
        if input.trim().is_empty() {
            return Err(ModelError("request must not be empty".into()));
        }
        if !(3..=12).contains(&budget.max_calls) {
            return Err(ModelError("agent call budget must be in 3..=12".into()));
        }
        let workflow_slots = workflow_slots(budget.max_calls)?;
        let context = memories.iter().take(8).cloned().collect::<Vec<_>>();
        let context_bytes = input
            .len()
            .saturating_add(context.iter().map(String::len).sum::<usize>());
        if context_bytes > budget.max_context_bytes {
            return Err(ModelError(
                "agent input and memory exceed configured context byte budget".into(),
            ));
        }
        let task = TaskPacket {
            task: input.to_string(),
            context: context.clone(),
        };
        let roles = Router::route(&task, model)?;
        let mut calls = 1usize;
        let mut packets = Vec::new();
        let mut roles_used = vec![AgentRole::Router];
        if roles.contains(&AgentRole::Decomposer) {
            if calls >= workflow_slots.saturating_sub(1) {
                return Err(ModelError(
                    "agent call budget has no decomposition slot".into(),
                ));
            }
            let packet = run_packet(
                model,
                AgentRole::Decomposer,
                input,
                &context,
                &mut calls,
                &budget,
            )?;
            let plan = parse_decomposition(packet.result.as_deref().unwrap_or_default())?;
            packets.push(packet);
            roles_used.push(AgentRole::Decomposer);
            let mut specialist_context = context.clone();
            specialist_context.push(format!("Original request: {input}"));
            for subtask in plan {
                if calls >= workflow_slots.saturating_sub(1) {
                    break;
                }
                let packet = run_packet(
                    model,
                    subtask.role,
                    &subtask.task,
                    &specialist_context,
                    &mut calls,
                    &budget,
                )?;
                roles_used.push(subtask.role);
                packets.push(packet);
            }
            if roles_used.len() < 2 {
                return Err(ModelError(
                    "agent call budget left no specialist after decomposition".into(),
                ));
            }
        } else {
            for role in roles.iter().copied() {
                if role == AgentRole::Decomposer || calls >= workflow_slots.saturating_sub(1) {
                    break;
                }
                let packet = run_packet(model, role, input, &context, &mut calls, &budget)?;
                roles_used.push(role);
                packets.push(packet);
            }
        }
        if packets.is_empty() {
            return Err(ModelError("agent budget left no specialist call".into()));
        }
        let summaries = packets
            .iter()
            .map(AgentPacket::encode)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| ModelError(e.to_string()))?;
        let mut evidence = packets
            .iter()
            .flat_map(|p| p.evidence.clone())
            .collect::<Vec<_>>();
        let summary = if packets.len() > 1 {
            if calls >= workflow_slots {
                return Err(ModelError(
                    "agent call budget exhausted before synthesis".into(),
                ));
            }
            let packet = run_packet(
                model,
                AgentRole::Summarizer,
                "Rangkum temuan. Pertahankan bukti, konflik, dan ketidakpastian.",
                &summaries,
                &mut calls,
                &budget,
            )?;
            roles_used.push(AgentRole::Summarizer);
            evidence.extend(packet.evidence.clone());
            packet.result.unwrap_or_default()
        } else {
            summaries.join("\n")
        };
        if calls >= workflow_slots {
            return Err(ModelError(
                "agent call budget exhausted before Leader response".into(),
            ));
        }
        let leader = run_packet(
            model,
            AgentRole::Leader,
            input,
            &[summary],
            &mut calls,
            &budget,
        )?;
        roles_used.push(AgentRole::Leader);
        let answer = leader
            .result
            .ok_or_else(|| ModelError("Leader returned no answer".into()))?;
        Ok(AgentResponse {
            answer,
            roles_used,
            calls_used: calls,
            evidence,
        })
    }

    pub fn propose_task_plan(model: &LocalModel, goal: &str) -> Result<String, ModelError> {
        if goal.trim().is_empty() || goal.len() > 2048 {
            return Err(ModelError("plan goal must contain 1..=2048 bytes".into()));
        }
        let response = model.complete(
            &[
                ChatMessage {
                    role: "system".into(),
                    content: "Kamu Dyor, perencana tugas NEON. Hasilkan hanya JSON valid, tanpa markdown, dengan bentuk {\"goal\":string,\"steps\":[{\"title\":string,\"instruction\":string,\"tool\":{\"tool\":string,\"arguments\":object},\"depends_on\":[integer],\"acceptance\":string}]}. Buat 2 sampai 8 langkah berurutan, depends_on berisi indeks langkah sebelumnya mulai 0. Tool yang diizinkan: workspace_list(arguments:{}), workspace_read({path}), workspace_search({query}), symbols_search({query}), dependencies_search({query}), git_status({}), git_diff({}), neonatom_eval({source}), workspace_propose_write({path,content}), cargo_test({}), cargo_check({}). Jangan buat tool lain, jangan sertakan field approval/command bebas/URL. Semua path harus relatif di workspace. Operasi penulisan dan cargo hanya proposal; manusia akan diminta persetujuan saat eksekusi. Jangan sertakan tool pencarian web; pencarian DuckDuckGo harus diminta langsung oleh pengguna.".into(),
                },
                ChatMessage { role: "user".into(), content: goal.trim().to_string() },
            ],
            2048,
        )?;
        let plan = response.text.trim();
        if plan.is_empty() || plan.len() > 64 * 1024 {
            return Err(ModelError(
                "local planner output must contain 1..=64 KiB".into(),
            ));
        }
        Ok(plan.to_string())
    }

    pub fn summarize_session(
        model: &LocalModel,
        turns: &[(String, String)],
        max_input_bytes: usize,
    ) -> Result<String, ModelError> {
        let rendered = render_session_turns(turns, max_input_bytes)?;
        let response = model.complete(
            &[
                ChatMessage {
                    role: "system".into(),
                    content: "Kamu Jordan, Summarizer NEON. Ringkas sesi dalam Bahasa Indonesia dengan bagian: keputusan, preferensi/konteks proyek, pekerjaan dan hasil terverifikasi, serta langkah terbuka. Pertahankan identifier dan status bukti. Jangan salin percakapan mentah, jangan mengarang fakta, dan jangan menyertakan rahasia yang tidak diperlukan. Maksimal 1500 karakter.".into(),
                },
                ChatMessage {
                    role: "user".into(),
                    content: rendered,
                },
            ],
            512,
        )?;
        let summary = response.text.trim();
        if summary.is_empty() || summary.len() > 8192 {
            return Err(ModelError(
                "session summary must contain 1..=8192 bytes".into(),
            ));
        }
        Ok(summary.to_string())
    }
}

fn render_session_turns(
    turns: &[(String, String)],
    max_input_bytes: usize,
) -> Result<String, ModelError> {
    if turns.is_empty() || turns.len() > 8 {
        return Err(ModelError("session summary requires 1..=8 turns".into()));
    }
    let rendered = serde_json::to_string(turns).map_err(|e| ModelError(e.to_string()))?;
    if rendered.len() > max_input_bytes || rendered.len() > 24 * 1024 {
        return Err(ModelError(
            "session summary input exceeds its byte budget".into(),
        ));
    }
    Ok(rendered)
}

#[derive(Deserialize)]
struct DecompositionPlan {
    tasks: Vec<DecomposedTask>,
}
#[derive(Deserialize)]
struct DecomposedTask {
    category: String,
    task: String,
}
struct PlannedTask {
    role: AgentRole,
    task: String,
}
fn parse_decomposition(input: &str) -> Result<Vec<PlannedTask>, ModelError> {
    if input.len() > 16 * 1024 {
        return Err(ModelError("decomposition plan exceeds 16 KiB".into()));
    }
    let plan: DecompositionPlan = serde_json::from_str(input)
        .map_err(|e| ModelError(format!("decomposer did not return valid task JSON: {e}")))?;
    if !(2..=5).contains(&plan.tasks.len()) {
        return Err(ModelError("decomposer must return 2..=5 subtasks".into()));
    }
    plan.tasks
        .into_iter()
        .map(|task| {
            if task.task.trim().is_empty() || task.task.len() > 4096 {
                return Err(ModelError(
                    "decomposition task must contain 1..=4096 bytes".into(),
                ));
            }
            let role = AgentRole::parse_label(&task.category)
                .map_err(|e| ModelError(format!("decomposer category rejected: {e}")))?;
            if !matches!(
                role,
                AgentRole::Programmer | AgentRole::Maintenance | AgentRole::Debugger
            ) {
                return Err(ModelError(
                    "decomposition may route only to programmer, maintenance, or debugger".into(),
                ));
            }
            Ok(PlannedTask {
                role,
                task: task.task,
            })
        })
        .collect()
}

fn run_packet(
    model: &LocalModel,
    role: AgentRole,
    task: &str,
    context: &[String],
    calls: &mut usize,
    budget: &AgentBudget,
) -> Result<AgentPacket, ModelError> {
    let role_name = serde_json::to_string(&role).map_err(|e| ModelError(e.to_string()))?;
    let system = format!("Nama agenmu {}. Peran: {}. Role id JSON harus tepat {role_name}. {} Balas hanya satu objek JSON valid dengan field role, task, context, constraints, result, evidence, status, confidence. status salah satu complete/needs_input/failed, confidence angka 0..1 atau null. Gunakan Bahasa Indonesia ringkas kecuali pengguna meminta bahasa lain. Jangan mengeksekusi alat. Jangan mengarang bukti.", role.display_name(), role.title(), role_instruction(role));
    let user = serde_json::json!({"task":task,"context":context,"constraints":["jawaban berdasarkan informasi yang diberikan","jika tidak yakin nyatakan ketidakpastian"]}).to_string();
    if system.len().saturating_add(user.len()) > budget.max_context_bytes {
        return Err(ModelError(
            "agent prompt exceeds context byte budget".into(),
        ));
    }
    *calls += 1;
    let mut answer = model.complete(
        &[
            ChatMessage {
                role: "system".into(),
                content: system.clone(),
            },
            ChatMessage {
                role: "user".into(),
                content: user.clone(),
            },
        ],
        budget.max_tokens_per_call,
    )?;
    let mut packet = AgentPacket::decode(answer.text.trim());
    if packet.is_err() && *calls < budget.max_calls {
        *calls += 1;
        let repair = format!("Keluaran sebelumnya tidak valid. Kembalikan satu objek JSON valid saja dengan field role, task, context, constraints, result, evidence, status, confidence. Data tidak valid: {}", answer.text.chars().take(3000).collect::<String>());
        answer = model.complete(
            &[
                ChatMessage {
                    role: "system".into(),
                    content: system,
                },
                ChatMessage {
                    role: "user".into(),
                    content: repair,
                },
            ],
            budget.max_tokens_per_call,
        )?;
        packet = AgentPacket::decode(answer.text.trim());
    }
    let packet =
        packet.map_err(|e| ModelError(format!("agent returned invalid protocol packet: {e}")))?;
    if packet.role != role {
        return Err(ModelError(
            "agent packet role does not match assigned role".into(),
        ));
    }
    if packet.status == PacketStatus::Failed {
        return Err(ModelError(format!("{} agent reported failure", role_name)));
    }
    if packet.result.as_ref().is_none_or(|s| s.trim().is_empty()) {
        return Err(ModelError("agent returned an empty result".into()));
    }
    Ok(packet)
}

fn workflow_slots(max_calls: usize) -> Result<usize, ModelError> {
    let source = format!("take(range(0, {max_calls}), {max_calls})");
    let program = parse_neonatom(&source)
        .map_err(|e| ModelError(format!("invalid internal workflow: {e}")))?;
    let value = eval_neonatom(
        &program,
        AtomLimits {
            fuel: 128,
            max_nesting: 32,
            max_output_bytes: 1024,
            timeout: Duration::from_millis(100),
        },
    )
    .map_err(|e| ModelError(format!("workflow budget evaluation failed: {e}")))?;
    let AtomValue::Stream(stream) = value else {
        return Err(ModelError("workflow did not produce a slot stream".into()));
    };
    let slots = stream
        .collect_limited(max_calls)
        .map_err(|e| ModelError(format!("workflow slot limit failed: {e}")))?;
    if slots.len() != max_calls {
        return Err(ModelError(
            "workflow did not produce the configured agent-call slots".into(),
        ));
    }
    Ok(slots.len())
}

fn role_instruction(role: AgentRole) -> &'static str {
    match role {
        AgentRole::Programmer => "Kamu spesialis implementasi perangkat lunak lintas bahasa. Tulis kode konkret yang dapat dikompilasi, jelaskan versi/asumsi, dan sertakan tes yang relevan. Hindari API rekaan.",
        AgentRole::Maintenance => "Kamu spesialis pemeliharaan. Cari perubahan minimal yang aman, kompatibilitas, risiko dependensi, pembaruan, dan regresi; susun langkah migrasi yang dapat diverifikasi.",
        AgentRole::Debugger => "Kamu spesialis debugging. Telusuri gejala ke akar penyebab, pisahkan fakta dari dugaan, usulkan perbaikan minimal, lalu berikan tes reproduksi dan verifikasi.",
        AgentRole::Decomposer => "Kamu spesialis pemecahan masalah kompleks. Pecah permintaan menjadi 2 sampai 5 sub-tugas independen yang jelas dan berurutan. Isi field result dengan JSON string valid berbentuk {\"tasks\":[{\"category\":\"coding|maintenance|debugging\",\"task\":\"instruksi spesifik\"}]}; setiap sub-tugas harus punya kriteria hasil.",
        AgentRole::Router => "Kamu mengklasifikasikan permintaan, tanpa membuat solusi.",
        AgentRole::Summarizer => "Kamu menyatukan keluaran spesialis. Pertahankan bukti, konflik, batasan, dan ketidakpastian; hilangkan pengulangan tanpa mengubah fakta.",
        AgentRole::Leader => "Kamu Naon, pemimpin yang berbicara langsung kepada pengguna. Jawab dalam Bahasa Indonesia alami, gabungkan temuan, soroti asumsi dan langkah praktis, serta jangan mengeklaim kemampuan yang belum dibuktikan.",
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_decomposition, render_session_turns, workflow_slots};
    use crate::agent::AgentRole;

    #[test]
    fn workflow_dsl_yields_exact_bounded_slots() {
        assert_eq!(workflow_slots(3).unwrap(), 3);
        assert_eq!(workflow_slots(8).unwrap(), 8);
        assert_eq!(workflow_slots(12).unwrap(), 12);
    }

    #[test]
    fn decomposer_plan_is_typed_and_rejects_unapproved_roles() {
        let tasks = parse_decomposition(r#"{"tasks":[{"category":"coding","task":"implement parser"},{"category":"debugging","task":"reproduce failure"}]}"#).unwrap();
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].role, AgentRole::Programmer);
        assert!(parse_decomposition(r#"{"tasks":[{"category":"leader","task":"call shell"},{"category":"coding","task":"write code"}]}"#).is_err());
        assert!(
            parse_decomposition(r#"{"tasks":[{"category":"coding","task":"only one"}]}"#).is_err()
        );
    }

    #[test]
    fn session_summary_buffer_is_bounded_and_keeps_turn_pairs() {
        let turns = vec![("apa kabar?".to_string(), "baik".to_string())];
        let encoded = render_session_turns(&turns, 1024).unwrap();
        assert!(encoded.contains("apa kabar?"));
        assert!(encoded.contains("baik"));
        assert!(render_session_turns(&[], 1024).is_err());
        assert!(render_session_turns(&turns, 1).is_err());
        let too_many = (0..9)
            .map(|_| ("u".to_string(), "a".to_string()))
            .collect::<Vec<_>>();
        assert!(render_session_turns(&too_many, 1024).is_err());
    }
}
