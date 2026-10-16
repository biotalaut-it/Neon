# NEON Context and Multi-Agent Coordinator Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Batasi penggunaan konteks/resource dan mengorkestrasi Naon, Mike, Dyor, Susan, Fani, Deby, dan Jordan hanya ketika diperlukan dengan satu model lokal sekuensial.

**Architecture:** Buat ContextAssembler deterministik yang memeringkat user request, state task, hasil tool, source citation, memory, skill, experience, lalu recent context; batas byte/call/step berlaku sebelum prompt dirakit. Supervisor menerima packet terbatas, memanggil role yang perlu saja, menghindari summarizer untuk satu hasil, dan mengakhiri workflow pada retry/call cap.

**Tech Stack:** Rust 2021, LocalModel, typed AgentPacket, SQLite FTS retrieval dari plan A/B, TaskPlanner/ToolDispatcher dari plan C, konfigurasi lingkungan yang ada.

**Spec:** `docs/specs/NEON-INTELLIGENCE-FEATURES-SPEC.md`, bagian Context & RAM Manager dan Multi-Agent Coordinator.

## Global Constraints

- Gunakan satu LocalModel dan urutan pemanggilan serial; tidak ada task thread per agen atau model kedua.
- `NEON_AGENT_CALLS` tetap dalam rentang 3..=12; maksimal 12 langkah planner; retry invalid JSON maksimal satu kali dan tak melebihi budget total.
- Gunakan `max_context_bytes` yang nyata; jangan menyebut byte heuristic sebagai tokenizer token count.
- Urutan prioritas: user request + system policy, active step, tool/verification evidence, RAG citations, relevant memory/skill/experience, session summary, older context.
- Hasil retrieval disisipkan hanya jika muat; penghilangan diberitahukan di final answer.
- Role names dan mappings tidak boleh berubah: Naon Leader; Mike Router; Dyor Decomposer; Susan Programmer; Fani Maintenance; Deby Debugger; Jordan Summarizer.

## Review Focus

1. Konteks user request/system policy tidak boleh hilang ketika cap penuh — tes D1.
2. Retrieval besar tidak boleh melebihi byte budget atau memicu alokasi tak terbatas — tes D1.
3. Model yang terus meminta retry/tool tidak boleh membuat loop melewati budget — tes D2.
4. Kegagalan model/agent/tool harus menghentikan dependensi step dan mengembalikan state resumable — tes D2.
5. Tugas sederhana tidak boleh memanggil Decomposer/Summarizer/spesialis yang tak diperlukan — tes D2.

---

### Task 1: ContextAssembler deterministik dan budget configurable

**Files:**
- Create: `crates/neon/src/context.rs`
- Modify: `crates/neon/src/lib.rs`
- Modify: `crates/neon/src/config.rs`
- Test: `crates/neon/tests/context.rs`

**Interfaces:**
- `ContextSource` enum: `System`, `UserRequest`, `ActiveTask`, `ToolEvidence`, `Knowledge`, `Memory`, `Skill`, `Experience`, `SessionSummary`, `RecentTurn`.
- `ContextItem { source: ContextSource, text: String, citation: Option<String> }`.
- `ContextBudget { max_bytes: usize, max_items: usize }`, `ContextAssembler::fit(items, budget) -> Result<AssembledContext, ContextError>`.
- `AssembledContext` menyimpan urutan final, byte count, serta sumber yang dipangkas.

- [ ] Tulis tes `required_user_and_policy_are_never_dropped`, `low_priority_items_drop_deterministically`, `multibyte_truncation_is_valid_utf8`, `item_and_total_bytes_are_bounded`, dan `dropped_sources_are_reported`.
- [ ] Jalankan `cargo test -p neon --test context`; harapkan gagal sebelum assembler tersedia.
- [ ] Implementasikan stable priority ordering, byte accounting, batas item, valid UTF-8 truncation, dan default configurable yang konsisten dengan ukuran context `NEON_CONTEXT`.
- [ ] Jalankan `cargo test -p neon --test context`; harapkan total byte tidak pernah melewati cap.

### Task 2: Coordinator hemat panggilan dan integrasi context/tool/retrieval

**Files:**
- Modify: `crates/neon/src/agent/supervisor.rs`
- Modify: `crates/neon/src/agent/router.rs`
- Modify: `crates/neon/src/agent.rs`
- Modify: `crates/neon/src/main.rs`
- Test: `crates/neon/tests/routing.rs`
- Test: `crates/neon/tests/orchestration.rs`

**Interfaces:**
- Tambahkan `Coordinator::run(model, request, assembled_context, plan_state, budget) -> Result<AgentResponse, ModelError>` dan pertahankan `Supervisor::run` sebagai wrapper kompatibilitas selama migrasi.
- `AgentResponse` menambah `context_bytes`, `context_items_dropped`, `evidence`, `roles_used`, `calls_used`, `status`.
- Routing decision memuat `direct_to_leader`, optional specialist list, `needs_decomposition`, dan optional typed tool request; setiap field divalidasi sebelum eksekusi.

- [ ] Tulis tes general request memakai Naon tanpa spesialis; coding task memilih Susan/Deby sesuai label; task multi-step memakai Dyor; Summarizer hanya dipanggil bila ada dua atau lebih result; role yang terpakai tampil berurutan.
- [ ] Tulis tes budget 3/8/12 call, retry JSON hanya sekali, tool call berulang dihentikan, dan satu backend dipanggil serial (instrumentasi urutan pada `LocalModel` melalui server loopback nyata test).
- [ ] Jalankan tes orchestration baru; harapkan fail bila routing/call history/context contract belum cocok.
- [ ] Implementasikan coordinator yang menggunakan ContextAssembler, `MemoryStore::retrieve_context`, `KnowledgeIndex`, Skill/Experience dan Planner melalui interface subplan A–C; role yang tidak perlu tidak dipanggil; `ToolRequest` hanya dieksekusi Dispatcher.
- [ ] Jalankan `cargo test -p neon --test routing --test orchestration`; harapkan pilihan role minimal, satu urutan serial, dan semua call/budget terukur.

### Task 3: Batas iterasi, resume, observabilitas dan integrasi chat

**Files:**
- Modify: `crates/neon/src/main.rs`
- Modify: `crates/neon/src/verification.rs`
- Modify: `crates/neon/src/planner.rs`
- Modify: `README.md`
- Test: `crates/neon/tests/orchestration.rs`
- Test: `crates/neon/tests/cli.rs`

**Interfaces:**
- Chat command menunjukkan `calls_used`, specialist names, source citations, status `verified|failed|not_run`, dan konteks yang dipangkas tanpa mencetak isi rahasia.
- `neon resume <plan-id>` memuat checkpoint `TaskStore`, ContextAssembler merakit ulang hanya state/tool evidence yang dibutuhkan untuk langkah aktif.
- Tambah command diagnostik `neon resources` yang menampilkan konfigurasi cap saat ini, ukuran database/index, dan batas jumlah iterasi tanpa membuat klaim presisi tokenizer.

- [ ] Tulis tes CLI `resources_command_shows_caps`, `resume_uses_last_nonverified_step`, `simple_chat_does_not_call_unused_roles`, dan `final_answer_distinguishes_verified_from_not_run`.
- [ ] Jalankan tes baru; harapkan gagal sebelum command dan diagnostik tersedia.
- [ ] Implementasikan indikator status final, checkpoint-aware resume, cap telemetry berbasis data aplikasi, dan redaksi isi memori saat mencetak diagnostics.
- [ ] Jalankan `cargo fmt --all -- --check && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`; harapkan semua cap, resume, dan pilihan role minimum lulus.
