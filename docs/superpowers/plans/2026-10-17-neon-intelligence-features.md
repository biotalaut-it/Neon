# NEON Intelligence Features Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Menambahkan sepuluh fitur intelijen praktis NEON melalui empat subproyek yang dapat dibangun, diuji, dan ditinjau secara terpisah.

**Architecture:** Pertahankan Rust CLI/Termux dan model GGUF lokal. Bangun pertama fondasi persistence/memory, lalu RAG/index, berikutnya tools/planner/verification, dan terakhir context/coordinator yang menggabungkan interface tiga subproyek sebelumnya. Setiap subproyek tetap menghasilkan kemampuan nyata tanpa placeholder.

**Tech Stack:** Rust 2021, rusqlite 0.32 + FTS5, serde/serde_json, `syn` untuk AST Rust, LocalModel/llama-server yang sudah ada, NeonAtom, Termux ARM64.

**Spec:** `docs/specs/NEON-INTELLIGENCE-FEATURES-SPEC.md`.

**Implementation status (2026-10-17):** Subprojects A–D are implemented in the Rust workspace and pass current Linux workspace tests/Clippy. Plan creation through real GGUF inference and Android Termux ARM64 execution remain unverified because this environment has no target phone/model configured.

## Global Constraints

- Semua inference lokal dan sekuensial; tidak ada provider model cloud atau model paralel.
- Memori, ringkasan, skills personal, dan pengalaman dari model memerlukan approval; chat mentah tidak disimpan otomatis.
- Indeks hanya mencakup file lokal yang diizinkan dan memberikan citation path/baris; FTS5 lebih dulu, tidak ada embedding pada rilis ini.
- Tool adalah enum tertutup; tidak ada shell arbitrary; file mutasi/Cargo memerlukan approval manusia.
- Batas workspace 10.000 file, 64 MiB total teks, 1 MiB per file, 1.800 byte per chunk; byte/call/step limits ditegakkan sebelum kerja.
- Status verifikasi berasal dari exit status/hasil tes/file readback yang nyata, bukan klaim model.
- Peran bernama tetap: Naon, Mike, Dyor, Susan, Fani, Deby, Jordan; gunakan satu model secara bergantian.

## Review Focus

1. Data sensitif/di luar workspace masuk indeks/memori — covered in subplan B Task 1 and A Tasks 1/3.
2. Payload invalid/berbahaya melewati tool validator — covered in subplan C Task 1/2.
3. Oversized retrieval/context membebani RAM atau token — covered in subplan B Task 2/4 and D Task 1.
4. Resume menandai pekerjaan belum dites sebagai berhasil atau mengulang verified step — covered in subplan C Task 2/3 and D Task 3.
5. Jawaban menyatakan tes/hasil sukses tanpa bukti — covered in subplan C Task 3 and D Task 3.

---

### Task 1: Implementasikan fondasi Memory, Skills, Experience

**Files:** subplan `docs/superpowers/plans/2026-10-17-neon-memory-skills-experience.md`; sumber: `memory.rs`, `memory/*`, CLI, tes memori.

**Interfaces:** `MemoryStore::retrieve_context`, session summary proposals, experience and skill CRUD; see subplan.

- [x] Jalankan tes subplan A; migrasi mempertahankan memori lama, mengindeks record baru, dan persetujuan eksplisit teruji.
- [x] Jalankan `cargo test -p neon --test memory --test cli`; memory, summary, skill, experience lulus.

### Task 2: Implementasikan RAG dan Codebase Intelligence

**Files:** subplan `docs/superpowers/plans/2026-10-17-neon-rag-codebase-intelligence.md`; sumber: `knowledge.rs`, `knowledge/*`, CLI, tes indexing.

**Interfaces:** `KnowledgeIndex::reindex/search/search_symbols`, `KnowledgeHit` dan citation path/baris; see subplan.

- [x] Jalankan tes scanner, FTS chunk, symbols/dependencies, dan CLI indexing sesuai subplan B.
- [x] Jalankan `cargo test -p neon --test knowledge --test codebase --test cli`; caps, exclusions, stale reindex, citation lulus.

### Task 3: Implementasikan Task Planner, Tool Calling, Verification

**Files:** subplan `docs/superpowers/plans/2026-10-17-neon-planner-tools-verification.md`; sumber: `tools.rs`, `planner.rs`, `verification.rs`, `lab.rs`, CLI, tests.

**Interfaces:** typed `ToolRequest`, `TaskStore`, persistent `Plan`/`PlanStep`, `VerificationEvidence`; see subplan C.

- [x] Jalankan tests malformed/tool approval, persistence/resume, dan bukti command/file sesuai subplan C.
- [x] Jalankan `cargo test -p neon --test tools --test planner --test verification`; tak ada mutasi/Cargo tanpa token approval dan evidence berasal dari hasil nyata.

### Task 4: Integrasikan Context Manager dan Coordinator

**Files:** subplan `docs/superpowers/plans/2026-10-17-neon-context-multiagent-coordinator.md`; sumber: `context.rs`, `agent/supervisor.rs`, router, CLI.

**Interfaces:** ContextAssembler `fit`, `Coordinator::run`, `AgentResponse` resource/evidence fields; consumes outputs from A–C.

- [x] Implementasikan ContextManager per-model dan integrasi context; coordinator menggunakan satu LocalModel secara sekuensial dan batas call/checkpoint tetap berlaku.
- [x] Jalankan `cargo fmt --all`, `cargo test --workspace`, dan `cargo clippy --workspace --all-targets -- -D warnings`; seluruh pemeriksaan lulus pada sandbox Linux.

### Task 5: Uji packaging Termux pada perangkat target

**Files:** `README.md`, scripts Termux, `docs/IMPLEMENTATION-STATUS.md`; perangkat Android ARM64.

**Interfaces:** instalasi dengan `scripts/install-termux.sh`, konfigurasi `NEON_LLAMA_SERVER`/`NEON_MODEL_PATH`, sesi `neon chat`.

- [ ] Build pada Termux ARM64 dan catat Rust/llama.cpp/model/quant, ukuran RSS dan latency.
- [ ] Jalankan workflow memory approval, `neon knowledge index/search`, satu task multi-step, tool approval, resume setelah gagal, dan verifier di dataset workspace uji.
- [ ] Perbarui status hanya dengan hasil pengukuran perangkat nyata; jangan menyatakan keberhasilan Android berdasarkan Linux.

## Dependency order

1. Plan A (Memory/Skills/Experience) dan Plan B (RAG/Codebase) dapat dikembangkan setelah schema SQLite/API database diputuskan; kedua indeks berbagi database file tetapi tabel/migrasinya terpisah.
2. Plan C (Planner/Tools/Verification) dapat mulai protocol read-only beriringan, tetapi TaskStore dan approval UI harus stabil sebelum coordinator mengintegrasikannya.
3. Plan D bergantung pada interface retrieval A/B dan execution/evidence C; kerjakan setelah tiga subplan tersebut lulus.
4. Termux validation paling akhir dan memerlukan perangkat/model lokal pengguna.
