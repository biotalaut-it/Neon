# NEON Planner, Tools, and Verification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Jalankan rencana tugas yang tersimpan dan dapat di-resume melalui dispatcher tool bertipe dengan approval, lalu laporkan verifikasi hanya berdasarkan hasil proses nyata.

**Architecture:** Model hanya mengusulkan JSON tool/plan; Rust mem-parse ke enum tertutup, memvalidasi path/argumen/batas, dan memanggil fungsi yang di-allowlist. SQLite mencatat plan/step transition; operasi file mutasi dan Cargo hanya berjalan setelah pengguna menyetujui pada CLI. Verification evidence berasal dari return code/output nyata.

**Tech Stack:** Rust 2021, serde/serde_json, rusqlite/SQLite, Workspace/lab yang ada, NeonAtom, std::process dengan executable + argumen eksplisit.

**Spec:** `docs/specs/NEON-INTELLIGENCE-FEATURES-SPEC.md`, bagian Task Planner, Tool Calling, Verification Engine.

## Global Constraints

- Tidak ada `sh -c`, shell generik, arbitrary command, arbitrary URL fetch, atau eksekusi kode hasil model.
- Tool protocol menolak unknown tool/field, invalid enum, payload lebih 8 KiB, dan path di luar workspace.
- `workspace.write` hanya membuat diff proposal; persetujuan berasal dari input pengguna di CLI, bukan JSON/model.
- `cargo test --workspace`/`cargo check --workspace` dan git mutating command tidak boleh berjalan tanpa persetujuan eksplisit.
- Maksimum 12 steps per plan, masing-masing maksimal 4 KiB; retry verifikasi gagal maksimal satu kali per step per resume run.
- Evidence status hanya `verified`, `failed`, atau `not_run`; klaim model tidak mengubah status.

## Review Focus

1. JSON tidak valid, unknown field/tool, duplicate args, atau payload oversize harus ditolak sebelum I/O — tes C1.
2. Path traversal, symlink race, target binary, dan root di luar workspace harus ditolak — tes C1/C2.
3. Model tidak dapat memberikan approval atau memanggil shell lewat plan/tool — tes C1/C2.
4. Crash/restart di tengah step tidak boleh menyatakan step pending sebagai verified atau mengulang step verified — tes C2.
5. Exit status nonzero, timeout, atau truncated output tidak boleh dilaporkan sukses — tes C3.

---

### Task 1: Protocol ToolRequest dan dispatcher read-only

**Files:**
- Create: `crates/neon/src/tools.rs`
- Modify: `crates/neon/src/lib.rs`
- Modify: `crates/neon/src/agent/model.rs` hanya untuk shared bounded result jika diperlukan
- Test: `crates/neon/tests/tools.rs`

**Interfaces:**
- `ToolRequest` closed enum: `WorkspaceList`, `WorkspaceRead { path }`, `WorkspaceSearch { query }`, `GitStatus`, `GitDiff`, `NeonAtomEval { source }`, `DuckDuckGoSearch { query }`.
- `ToolRequest::parse_json(input: &str) -> Result<ToolRequest, ToolError>` menolak unknown fields dan bytes >8 KiB.
- `ToolDispatcher::execute_read_only(&self, request: ToolRequest) -> Result<ToolResult, ToolError>` tidak menerima command bebas atau approval boolean dari model.

- [ ] Tulis tes `tool_parser_rejects_unknown_or_oversize_json`, `workspace_read_rejects_traversal_and_symlink`, `git_tools_are_read_only_allowlisted`, dan `neonatom_eval_uses_fuel_limit`.
- [ ] Jalankan `cargo test -p neon --test tools`; harapkan gagal sebelum protocol/dispatcher tersedia.
- [ ] Implementasikan enum serde `deny_unknown_fields`, batas ukuran/query, operasi workspace nyata melalui `Workspace`, git executable allowlist (binary `git`, argv tetap, timeout/output cap), dan evaluasi NeonAtom bounded.
- [ ] Jalankan `cargo test -p neon --test tools`; harapkan tak ada child process untuk input invalid dan tak ada shell subprocess.

### Task 2: Tool mutasi, approval, dan plan persisten

**Files:**
- Create: `crates/neon/src/planner.rs`
- Modify: `crates/neon/src/tools.rs`
- Modify: `crates/neon/src/lab.rs`
- Modify: `crates/neon/src/config.rs`
- Test: `crates/neon/tests/planner.rs`
- Test: `crates/neon/tests/tools.rs`

**Interfaces:**
- `Plan { id: i64, goal: String, status: PlanStatus, steps: Vec<PlanStep> }`; `PlanStep { id: i64, ordinal: u8, title: String, instruction: String, tool: ToolRequest, depends_on: Vec<i64>, acceptance: String, status: StepStatus }`; step states `pending|running|verified|failed|blocked`.
- `TaskStore::open(db_path: &Path)`, `create_plan(goal, steps)`, `claim_next_step(plan_id)`, `finish_step(step_id, evidence)`, `fail_step(step_id, error)`, `load_plan(plan_id)`; every transition transaction-safe.
- `ToolRequest::WorkspaceProposeWrite { path, content }` returns `NeedsApproval(ChangeDiff)`; separate `ToolDispatcher::apply_approved_write(diff, user_confirmation: UserApproval)` is invoked only by terminal UI after exact diff display and `y`.
- `ToolRequest::CargoTest|CargoCheck` returns `NeedsApproval`; no plan field can encode approval.

- [ ] Tulis tes malformed plan/too many steps, valid DAG/order, atomic status transitions, resume from failed step, no re-run of verified step, file diff not applied without CLI approval, Cargo not started before approval.
- [ ] Jalankan `cargo test -p neon --test planner --test tools`; harapkan gagal untuk persistence/approval flows.
- [ ] Implementasikan tables versioned dalam database lokal, JSON plan validation (2..=12 steps; each <=4 KiB), explicit dependency ordering, step checkpoints, resume, proposal/approval UI, dan Cargo run memakai executable/argv allowlist dengan timeout/output cap.
- [ ] Jalankan tes; harapkan write/Cargo terhenti ketika approval ditolak dan resume mengerjakan hanya `pending|failed` steps.

### Task 3: Verification Engine dan provenance evidence

**Files:**
- Create: `crates/neon/src/verification.rs`
- Modify: `crates/neon/src/planner.rs`
- Modify: `crates/neon/src/tools.rs`
- Modify: `crates/neon/src/main.rs`
- Test: `crates/neon/tests/verification.rs`

**Interfaces:**
- `EvidenceKind` enum: `CommandExit`, `FileDiff`, `IndexCitation`, `ManualApproval`.
- `VerificationEvidence { kind, status: VerificationStatus, summary, source, exit_code, recorded_at }`.
- `VerificationEngine::verify_tool_result(request, result) -> VerificationEvidence`; model-generated prose never enters this function as success evidence.
- Final answer builder receives the collected evidence and marks untested assertions `not_run`.

- [ ] Tulis tes `zero_exit_marks_command_verified`, `nonzero_timeout_and_spawn_error_are_not_verified`, `file_diff_verifies_only_after_readback`, `model_text_cannot_upgrade_status`, dan `truncated_output_is_labeled`.
- [ ] Jalankan `cargo test -p neon --test verification`; harapkan gagal sebelum verifier tersedia.
- [ ] Implementasikan conversion result→evidence, readback hash untuk file yang disetujui, dan evidence attach per `PlanStep` transaction.
- [ ] Jalankan `cargo fmt --all -- --check && cargo test -p neon && cargo clippy -p neon --all-targets -- -D warnings`; harapkan semua output status didasarkan pada return nyata.
