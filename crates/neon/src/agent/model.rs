use crate::agent::AgentRole;
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Debug, Clone)]
pub struct ModelConfig {
    pub server_bin: PathBuf,
    pub model_path: PathBuf,
    pub host: IpAddr,
    pub port: u16,
    pub context_tokens: u32,
    pub threads: usize,
    pub startup_timeout: Duration,
    pub request_timeout: Duration,
    pub max_response_bytes: usize,
}
impl ModelConfig {
    pub fn from_env() -> Result<Self, String> {
        Self::from_env_for_role(None)
    }

    pub fn from_env_for_role(role: Option<AgentRole>) -> Result<Self, String> {
        let server_bin = std::env::var_os("NEON_LLAMA_SERVER")
            .map(PathBuf::from)
            .ok_or("NEON_LLAMA_SERVER must point to llama-server")?;
        let model_path = resolve_model_path(role)?;
        let port = parse_env("NEON_PORT", 8089u16)?;
        let context_tokens = parse_env("NEON_CONTEXT", 4096u32)?;
        let threads = parse_env(
            "NEON_THREADS",
            std::thread::available_parallelism()
                .map(usize::from)
                .unwrap_or(2)
                .min(4),
        )?;
        if port < 1024 {
            return Err("NEON_PORT must be in 1024..=65535".into());
        }
        if !(256..=32768).contains(&context_tokens) {
            return Err("NEON_CONTEXT must be in 256..=32768".into());
        }
        if threads == 0 || threads > 16 {
            return Err("NEON_THREADS must be in 1..=16".into());
        }
        Ok(Self {
            server_bin,
            model_path,
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port,
            context_tokens,
            threads,
            startup_timeout: Duration::from_secs(45),
            request_timeout: Duration::from_secs(180),
            max_response_bytes: 4 * 1024 * 1024,
        })
    }
}

fn resolve_model_path(role: Option<AgentRole>) -> Result<PathBuf, String> {
    let candidates: Vec<&str> = match role {
        Some(AgentRole::Leader) => vec!["NEON_MODEL_LEADER", "NEON_MODEL_PATH"],
        Some(AgentRole::Programmer) => vec![
            "NEON_MODEL_SUSAN",
            "NEON_MODEL_PROGRAMMER",
            "NEON_MODEL_PATH",
        ],
        Some(AgentRole::Maintenance) => vec!["NEON_MODEL_FANI", "NEON_MODEL_PATH"],
        Some(AgentRole::Debugger) => vec!["NEON_MODEL_DEBY", "NEON_MODEL_PATH"],
        Some(AgentRole::Decomposer) => vec!["NEON_MODEL_DYOR", "NEON_MODEL_PATH"],
        Some(AgentRole::Router) => vec!["NEON_MODEL_MIKE", "NEON_MODEL_PATH"],
        Some(AgentRole::Summarizer) => vec!["NEON_MODEL_JORDAN", "NEON_MODEL_PATH"],
        None => vec!["NEON_MODEL_PATH"],
    };

    for name in candidates {
        if let Some(value) = std::env::var_os(name) {
            let path = PathBuf::from(value);
            if path.as_os_str().is_empty() {
                continue;
            }
            return Ok(path);
        }
    }

    Err(match role {
        Some(role) => format!(
            "{} model not configured; set NEON_MODEL_PATH or a role-specific override like NEON_MODEL_SUSAN",
            role.display_name()
        ),
        None => "NEON_MODEL_PATH must point to a local GGUF file".to_string(),
    })
}
fn parse_env<T: std::str::FromStr>(name: &str, default: T) -> Result<T, String> {
    match std::env::var(name) {
        Ok(s) => s.parse().map_err(|_| format!("invalid {name}")),
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(e) => Err(format!("cannot read {name}: {e}")),
    }
}

#[derive(Debug)]
pub struct ModelError(pub String);
impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ModelError {}

#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}
#[derive(Debug, Clone)]
pub struct Completion {
    pub text: String,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
}
#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    usage: Option<Usage>,
}
#[derive(Deserialize)]
struct Choice {
    message: ResponseMessage,
}
#[derive(Deserialize)]
struct ResponseMessage {
    content: String,
}
#[derive(Deserialize)]
struct Usage {
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
}

pub struct LocalModel {
    child: Child,
    client: reqwest::blocking::Client,
    base_url: String,
    max_response_bytes: usize,
    context_tokens: u32,
}
impl LocalModel {
    pub fn start(config: &ModelConfig) -> Result<Self, ModelError> {
        if !config.host.is_loopback() {
            return Err(ModelError("inference backend host must be loopback".into()));
        }
        if !Path::new(&config.model_path).is_file() {
            return Err(ModelError(format!(
                "model file does not exist: {}",
                config.model_path.display()
            )));
        }
        let client = reqwest::blocking::Client::builder()
            .timeout(config.request_timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| ModelError(format!("cannot configure local HTTP client: {e}")))?;
        let address = SocketAddr::new(config.host, config.port);
        let mut child = Command::new(&config.server_bin)
            .arg("--model")
            .arg(&config.model_path)
            .arg("--host")
            .arg(config.host.to_string())
            .arg("--port")
            .arg(config.port.to_string())
            .arg("--ctx-size")
            .arg(config.context_tokens.to_string())
            .arg("--threads")
            .arg(config.threads.to_string())
            .arg("--no-webui")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| ModelError(format!("failed to start llama-server: {e}")))?;
        let base_url = format!("http://{address}");
        let deadline = Instant::now() + config.startup_timeout;
        loop {
            if let Some(status) = child
                .try_wait()
                .map_err(|e| ModelError(format!("cannot inspect llama-server: {e}")))?
            {
                return Err(ModelError(format!(
                    "llama-server exited during startup with {status}"
                )));
            }
            let last_error = match client.get(format!("{base_url}/health")).send() {
                Ok(response) if response.status().is_success() => break,
                Ok(response) => format!("health endpoint returned {}", response.status()),
                Err(e) => e.to_string(),
            };
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ModelError(format!(
                    "llama-server readiness timed out: {last_error}"
                )));
            }
            thread::sleep(Duration::from_millis(250));
        }
        Ok(Self {
            child,
            client,
            base_url,
            max_response_bytes: config.max_response_bytes,
            context_tokens: config.context_tokens,
        })
    }

    pub fn complete(
        &self,
        messages: &[ChatMessage],
        max_tokens: u32,
    ) -> Result<Completion, ModelError> {
        complete_at(
            &self.client,
            &self.base_url,
            self.max_response_bytes,
            self.context_tokens,
            messages,
            max_tokens,
        )
    }
}
impl Drop for LocalModel {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn complete_at(
    client: &reqwest::blocking::Client,
    base_url: &str,
    max_response_bytes: usize,
    context_tokens: u32,
    messages: &[ChatMessage],
    max_tokens: u32,
) -> Result<Completion, ModelError> {
    let prompt_bytes = messages
        .iter()
        .map(|message| message.content.len().saturating_add(message.role.len()))
        .sum::<usize>();
    if !(1..=32768).contains(&max_tokens) {
        return Err(ModelError("max_tokens must be in 1..=32768".into()));
    }
    let prompt_budget = context_tokens
        .checked_sub(max_tokens)
        .and_then(|tokens| tokens.checked_sub(512))
        .map(|tokens| (tokens as usize).saturating_mul(2))
        .ok_or_else(|| ModelError("context is too small for response and system reserve".into()))?;
    if prompt_bytes > prompt_budget {
        return Err(ModelError(format!("local model prompt of {prompt_bytes} bytes exceeds conservative context budget of {prompt_budget} bytes (context={context_tokens} tokens, response={max_tokens} tokens)")));
    }
    if prompt_bytes > 64 * 1024 {
        return Err(ModelError(
            "local model request exceeds 64 KiB prompt limit".into(),
        ));
    }
    let body = serde_json::json!({"model":"local","messages":messages,"max_tokens":max_tokens,"stream":false,"temperature":0.2});
    let response = client
        .post(format!("{base_url}/v1/chat/completions"))
        .json(&body)
        .send()
        .map_err(|e| ModelError(format!("local model request failed: {e}")))?;
    let status = response.status();
    let mut bytes = Vec::new();
    response
        .take(max_response_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| ModelError(format!("cannot read local model response: {e}")))?;
    if bytes.len() > max_response_bytes {
        return Err(ModelError(
            "local model response exceeds configured size limit".into(),
        ));
    }
    if !status.is_success() {
        return Err(ModelError(format!("local model returned HTTP {status}")));
    }
    let decoded: ChatResponse = serde_json::from_slice(&bytes)
        .map_err(|e| ModelError(format!("invalid local model response: {e}")))?;
    let choice = decoded
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| ModelError("local model returned no choices".into()))?;
    Ok(Completion {
        text: choice.message.content,
        prompt_tokens: decoded.usage.as_ref().and_then(|u| u.prompt_tokens),
        completion_tokens: decoded.usage.and_then(|u| u.completion_tokens),
    })
}

#[cfg(test)]
mod tests {
    use super::{complete_at, ChatMessage, ModelConfig};
    use crate::agent::AgentRole;
    use std::{
        env,
        io::{Read, Write},
        net::TcpListener,
        thread,
        time::Duration,
    };

    #[test]
    fn role_specific_model_override_uses_susan_model_when_present() {
        let default_path = "/tmp/default-model.gguf";
        let susan_path = "/tmp/qwen2.5-coder-3b.gguf";

        unsafe {
            env::set_var("NEON_LLAMA_SERVER", "/tmp/llama-server");
            env::set_var("NEON_MODEL_PATH", default_path);
            env::set_var("NEON_MODEL_SUSAN", susan_path);
        }

        let cfg = ModelConfig::from_env_for_role(Some(AgentRole::Programmer)).unwrap();
        assert_eq!(cfg.model_path.to_string_lossy(), susan_path);

        unsafe {
            env::remove_var("NEON_MODEL_SUSAN");
            env::remove_var("NEON_MODEL_PATH");
            env::remove_var("NEON_LLAMA_SERVER");
        }
    }

    #[test]
    fn role_specific_model_falls_back_to_default_model_when_no_override_is_set() {
        let default_path = "/tmp/default-model.gguf";

        unsafe {
            env::set_var("NEON_LLAMA_SERVER", "/tmp/llama-server");
            env::set_var("NEON_MODEL_PATH", default_path);
            env::remove_var("NEON_MODEL_SUSAN");
        }

        let cfg = ModelConfig::from_env_for_role(Some(AgentRole::Maintenance)).unwrap();
        assert_eq!(cfg.model_path.to_string_lossy(), default_path);

        unsafe {
            env::remove_var("NEON_MODEL_PATH");
            env::remove_var("NEON_LLAMA_SERVER");
        }
    }

    fn serve_once(status: u16, body: &'static str) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut request = Vec::new();
            let mut chunk = [0u8; 2048];
            loop {
                let read = stream.read(&mut chunk).unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..read]);
                if let Some(header_end) =
                    request.windows(4).position(|window| window == b"\r\n\r\n")
                {
                    let headers = String::from_utf8_lossy(&request[..header_end]);
                    let body_len = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|n| n.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if request.len() >= header_end + 4 + body_len {
                        break;
                    }
                }
            }
            let response = format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            stream.write_all(response.as_bytes()).unwrap();
        });
        (format!("http://{address}"), handle)
    }

    #[test]
    fn local_chat_completion_parses_response_and_usage() {
        let body = r#"{"choices":[{"message":{"content":"jawaban lokal"}}],"usage":{"prompt_tokens":11,"completion_tokens":4}}"#;
        let (url, server) = serve_once(200, body);
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap();
        let messages = [ChatMessage {
            role: "user".into(),
            content: "test".into(),
        }];
        let result = complete_at(&client, &url, 4096, 4096, &messages, 64).unwrap();
        server.join().unwrap();
        assert_eq!(result.text, "jawaban lokal");
        assert_eq!(result.prompt_tokens, Some(11));
        assert_eq!(result.completion_tokens, Some(4));
    }

    #[test]
    fn local_chat_completion_enforces_response_limit() {
        let body = r#"{"choices":[{"message":{"content":"response larger than limit"}}]}"#;
        let (url, server) = serve_once(200, body);
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap();
        let error = complete_at(&client, &url, 16, 4096, &[], 64).unwrap_err();
        server.join().unwrap();
        assert!(error.to_string().contains("size limit"));
    }

    #[test]
    fn local_chat_completion_rejects_oversized_prompt_and_invalid_token_budget() {
        let client = reqwest::blocking::Client::new();
        let huge = [ChatMessage {
            role: "user".into(),
            content: "x".repeat(64 * 1024 + 1),
        }];
        let error = complete_at(&client, "http://127.0.0.1:1", 1024, 65536, &huge, 64).unwrap_err();
        assert!(error.to_string().contains("64 KiB"));
        let error = complete_at(&client, "http://127.0.0.1:1", 1024, 4096, &[], 0).unwrap_err();
        assert!(error.to_string().contains("max_tokens"));
    }

    #[test]
    fn local_chat_completion_rejects_prompt_over_context_byte_estimate() {
        let client = reqwest::blocking::Client::new();
        let messages = [ChatMessage {
            role: "user".into(),
            content: "x".repeat(6000),
        }];
        let error =
            complete_at(&client, "http://127.0.0.1:1", 1024, 4096, &messages, 768).unwrap_err();
        assert!(error.to_string().contains("conservative context budget"));
    }
}
