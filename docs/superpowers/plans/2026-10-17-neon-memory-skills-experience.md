# NEON Memory, Skills, and Experience Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Tambahkan retrieval lintas memori, ringkasan sesi privat, prosedur skill yang dapat dipakai ulang, dan pengalaman terverifikasi yang disimpan secara lokal dengan persetujuan pengguna.

**Architecture:** Pertahankan `MemoryStore` dan database SQLite saat ini, lakukan migrasi `user_version` tambahan yang idempoten, serta pecah domain baru ke submodul `memory/`. Semua record dari hasil model melewati proposal/approval; FTS5 mencari record terpilih tanpa menyimpan chat mentah.

**Tech Stack:** Rust 2021, rusqlite 0.32 bundled SQLite/FTS5, serde, LocalModel loopback yang sudah ada.

**Spec:** `docs/specs/NEON-INTELLIGENCE-FEATURES-SPEC.md`, bagian Smart Memory, Skill Library, Experience Loop.

## Global Constraints

- Gunakan satu backend model GGUF lokal; jangan tambah provider cloud.
- Database tetap lokal di `NEON_DATA_DIR/neon.sqlite3`; migrasi harus mempertahankan data `memories` yang lama.
- Jangan simpan seluruh percakapan secara otomatis; buffer sesi hanya in-memory dan dibatasi maksimal 8 giliran/24 KiB.
- Simpan ringkasan, pengalaman, dan skill buatan model hanya setelah konfirmasi eksplisit; purge harus membersihkan FTS.
- Semua record teks maksimal 8 KiB; query retrieval mengembalikan maksimal 8 item dan 12 KiB secara default.
- Skill adalah teks/checklist tervalidasi, bukan skrip atau kode yang dijalankan.

## Review Focus

1. Kueri FTS kosong/berisi operator khusus harus menghasilkan hasil aman, bukan SQL/FTS injection — tes A1.
2. Restart/migrasi dari database lama harus mempertahankan fakta dan hasil FTS — tes A1.
3. Penolakan proposal, penghapusan item, dan purge tidak boleh meninggalkan record FTS — tes A1/A4.
4. Buffer sesi yang penuh tidak boleh mengembangkan RAM tanpa batas atau persist otomatis — tes A3.
5. Pengalaman lama yang tak relevan/bertentangan tidak boleh menjadi klaim terverifikasi — tes A4/A2.

---

### Task 1: Migrasi record dan FTS lintas domain

**Files:**
- Modify: `crates/neon/src/memory.rs`
- Create: `crates/neon/src/memory/sessions.rs`
- Create: `crates/neon/src/memory/experiences.rs`
- Create: `crates/neon/src/memory/skills.rs`
- Test: `crates/neon/tests/memory.rs`

**Interfaces:**
- Produces `MemoryStore::schema_version() -> Result<i64, String>` untuk diagnosis.

- [ ] Tambah tes `migration_preserves_existing_memories_and_rebuilds_fts`, `migration_is_idempotent`, dan `fts_tracks_update_delete_and_purge`; buktikan tes migrasi mempertahankan record yang dibuat memakai schema lama.
- [ ] Jalankan `cargo test -p neon --test memory migration_ -- --nocapture`; harapkan gagal karena tabel/migrasi baru belum tersedia.
- [ ] Implementasikan migrasi SQLite transactional/versioned untuk `session_summaries`, `experiences`, `skills` dan tabel FTS5 terpisah dengan trigger insert/update/delete.
- [ ] Jalankan `cargo test -p neon --test memory`; harapkan semua tes memori lulus dan database lama tetap bisa dibuka.

### Task 2: Retrieval memory yang relevan dan terbatas

**Files:**
- Modify: `crates/neon/src/memory.rs`
- Create: `crates/neon/src/memory/context.rs`
- Test: `crates/neon/tests/memory.rs`

**Interfaces:**
- Mengonsumsi schema Task 1.
- Produces `MemoryStore::retrieve_context(query: &str, limit: usize, max_bytes: usize) -> Result<Vec<ContextEntry>, String>`.
- `ContextEntry` memiliki `kind`, `source_id`, `category`, `text`, dan `created_at`; tidak berisi embedding.
- `retrieve_context` mengurutkan exact FTS match lebih tinggi, membatasi jumlah dan byte secara deterministik, dan mengembalikan empty vector untuk query kosong.

- [ ] Tambah tes `retrieval_returns_relevant_kinds_with_source_ids`, `retrieval_obeys_item_and_byte_caps`, dan `malformed_fts_input_is_rejected_or_safely_quoted`.
- [ ] Jalankan tes baru dan pastikan batas gagal sesuai ekspektasi sebelum implementasi.
- [ ] Implementasikan query terparameterisasi ke FTS, sanitasi token sesuai perilaku yang diuji, metadata sumber, serta pemotongan berbasis prioritas tanpa mengubah record asli.
- [ ] Jalankan `cargo test -p neon --test memory`; harapkan tidak ada hasil melampaui cap dan tidak ada input FTS menyebabkan panic.

### Task 3: Ringkasan sesi percakapan dengan persetujuan eksplisit

**Files:**
- Modify: `crates/neon/src/main.rs`
- Modify: `crates/neon/src/agent/supervisor.rs`
- Modify: `crates/neon/src/memory.rs`
- Test: `crates/neon/tests/memory.rs`
- Test: `crates/neon/tests/cli.rs`

**Interfaces:**
- Tambahkan `SessionTurn { user: String, assistant: String }` dengan limit masing-masing 8 KiB.
- Tambahkan `MemoryStore::propose_summary(project: &str, text: &str) -> Result<MemoryProposal, String>` dan `approve_summary(id: &str) -> Result<i64, String>`.
- REPL menyimpan maksimal 8 giliran/24 KiB dalam RAM proses; tidak ada persistensi sampai command `/summary` dan jawaban persetujuan `y`.

- [ ] Tulis tes untuk buffer eviction, proposal yang ditolak tidak tersimpan, persetujuan searchable, dan `/summary` tidak merekam pesan mentah.
- [ ] Jalankan tes baru; harapkan gagal sebelum buffer/proposal tersedia.
- [ ] Implementasikan `Supervisor::summarize_session` memakai peran Jordan pada LocalModel yang sama, tampilkan ringkasan untuk review, lalu simpan hanya setelah pengguna menyetujui.
- [ ] Jalankan `cargo test -p neon --test memory --test cli`; harapkan ringkasan yang ditolak tidak ditemukan di FTS dan ringkasan yang disetujui dapat dicari.

### Task 4: Skill Library dan Experience Loop

**Files:**
- Modify: `crates/neon/src/memory/skills.rs`
- Modify: `crates/neon/src/memory/experiences.rs`
- Modify: `crates/neon/src/main.rs`
- Test: `crates/neon/tests/memory.rs`
- Test: `crates/neon/tests/cli.rs`

**Interfaces:**
- Tambahkan `MemoryStore::propose_experience(task, outcome, cause, solution, verification)`, `approve_experience(id)`, `propose_skill(name, triggers, procedure)`, `approve_skill(id)`, dan operasi `list/search/delete` yang tervalidasi.
- Seed skill bawaan sebagai data prosedur nyata: `debugging`, `rust-test`, `repository-analysis`, `diff-review`; seed harus idempoten, tidak berisi command yang otomatis dijalankan.
- Retrieval pengalaman memberi flag `historical=true`; hasil lama bukan bukti run saat ini.

- [ ] Tambah tes untuk proposal/approval, seed idempoten, retrieval FTS, batas ukuran, edit/hapus/purge, dan pembedaan historical evidence.
- [ ] Jalankan tes baru dan verifikasi gagal pada API/data belum dibuat.
- [ ] Implementasikan CRUD/proposal skill dan experience, slash/CLI commands `/skills`, `/experience`, serta ekspor/hapus lokal tanpa eksekusi prosedur otomatis.
- [ ] Jalankan `cargo fmt --all -- --check && cargo test -p neon`; harapkan semua tes lulus dan Clippy tidak memiliki warning.
