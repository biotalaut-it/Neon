use crate::agent::model::{Completion, LocalModel};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::Path,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkCase {
    pub id: String,
    pub prompt: String,
    pub expected_markers: Vec<String>,
    pub max_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkCaseResult {
    pub id: String,
    pub passed: bool,
    pub latency_ms: u128,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub resident_kib: Option<u64>,
    pub peak_kib: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkReport {
    pub recorded_at: i64,
    pub model_id: String,
    pub cases: Vec<BenchmarkCaseResult>,
}
impl BenchmarkReport {
    pub fn passed(&self) -> usize {
        self.cases.iter().filter(|case| case.passed).count()
    }

    pub fn average_latency_ms(&self) -> Option<u128> {
        let measured = self
            .cases
            .iter()
            .filter(|case| case.error.is_none())
            .map(|case| case.latency_ms)
            .collect::<Vec<_>>();
        (!measured.is_empty()).then(|| measured.iter().sum::<u128>() / measured.len() as u128)
    }

    pub fn total_tokens(&self) -> Option<u64> {
        self.cases.iter().try_fold(0u64, |total, case| {
            total
                .checked_add(case.prompt_tokens?)
                .and_then(|total| total.checked_add(case.completion_tokens?))
        })
    }
}

pub struct BenchmarkRunner;
impl BenchmarkRunner {
    pub fn default_suite() -> Vec<BenchmarkCase> {
        vec![
            BenchmarkCase {
                id: "arithmetic-exact".into(),
                prompt: "Hitung 19 + 23. Jawab angka hasilnya.".into(),
                expected_markers: vec!["42".into()],
                max_tokens: 64,
            },
            BenchmarkCase {
                id: "uncertainty-no-evidence".into(),
                prompt: "Apakah kode lokal saya sudah lulus tes? Tidak ada hasil tes atau akses repositori yang diberikan. Jawab bahwa bukti belum tersedia.".into(),
                expected_markers: vec!["bukti".into(), "belum".into()],
                max_tokens: 128,
            },
            BenchmarkCase {
                id: "indonesian-explanation".into(),
                prompt: "Jelaskan singkat dalam Bahasa Indonesia bahwa timeout menghentikan operasi yang melewati batas waktu.".into(),
                expected_markers: vec!["waktu".into()],
                max_tokens: 128,
            },
        ]
    }

    pub fn run(model: &LocalModel, model_id: &str) -> BenchmarkReport {
        let mut cases = Vec::new();
        for case in Self::default_suite() {
            let started = Instant::now();
            let result = model.complete(
                &[crate::agent::model::ChatMessage {
                    role: "user".into(),
                    content: case.prompt.clone(),
                }],
                case.max_tokens,
            );
            let latency_ms = started.elapsed().as_millis();
            let memory = model.process_memory_kib();
            cases.push(match result {
                Ok(completion) => score_case(&case, &completion, latency_ms, memory),
                Err(error) => BenchmarkCaseResult {
                    id: case.id,
                    passed: false,
                    latency_ms,
                    prompt_tokens: None,
                    completion_tokens: None,
                    resident_kib: memory.map(|value| value.0),
                    peak_kib: memory.map(|value| value.1),
                    error: Some(error.to_string()),
                },
            });
        }
        BenchmarkReport {
            recorded_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_secs() as i64)
                .unwrap_or(0),
            model_id: model_id.into(),
            cases,
        }
    }
}

fn score_case(
    case: &BenchmarkCase,
    completion: &Completion,
    latency_ms: u128,
    memory: Option<(u64, u64)>,
) -> BenchmarkCaseResult {
    let answer = completion.text.to_lowercase();
    let passed = case
        .expected_markers
        .iter()
        .all(|marker| answer.contains(&marker.to_lowercase()));
    BenchmarkCaseResult {
        id: case.id.clone(),
        passed,
        latency_ms,
        prompt_tokens: completion.prompt_tokens,
        completion_tokens: completion.completion_tokens,
        resident_kib: memory.map(|value| value.0),
        peak_kib: memory.map(|value| value.1),
        error: None,
    }
}

pub fn append_report(path: &Path, report: &BenchmarkReport) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|error| error.to_string())?;
    serde_json::to_writer(&mut file, report).map_err(|error| error.to_string())?;
    file.write_all(b"\n").map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())
}

pub fn latest_report(path: &Path) -> Result<Option<BenchmarkReport>, String> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let mut latest = None;
    for line in BufReader::new(file).lines() {
        let line = line.map_err(|error| error.to_string())?;
        let report = serde_json::from_str(&line)
            .map_err(|error| format!("invalid benchmark history record: {error}"))?;
        latest = Some(report);
    }
    Ok(latest)
}

#[cfg(test)]
mod tests {
    use super::{score_case, BenchmarkCase, BenchmarkReport};
    use crate::agent::model::Completion;

    #[test]
    fn benchmark_scores_expected_markers_and_aggregates_measured_metrics() {
        let case = BenchmarkCase {
            id: "known".into(),
            prompt: "question".into(),
            expected_markers: vec!["42".into(), "answer".into()],
            max_tokens: 16,
        };
        let result = score_case(
            &case,
            &Completion {
                text: "The answer is 42".into(),
                prompt_tokens: Some(3),
                completion_tokens: Some(4),
            },
            9,
            Some((128, 256)),
        );
        assert!(result.passed);
        let report = BenchmarkReport {
            recorded_at: 1,
            model_id: "local".into(),
            cases: vec![result],
        };
        assert_eq!(report.passed(), 1);
        assert_eq!(report.average_latency_ms(), Some(9));
        assert_eq!(report.total_tokens(), Some(7));
    }
}
