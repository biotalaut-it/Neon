# NEON RAG and Codebase Intelligence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Cari dokumentasi dan source code workspace secara lokal dengan FTS5, kutipan path/baris, dan indeks simbol Rust yang dapat diperbarui.

**Architecture:** Tambahkan `KnowledgeIndex` pada database lokal yang sama, scanner read-only yang menegakkan root/exclusion/resource caps, chunk FTS untuk seluruh teks yang diizinkan, dan tabel simbol/dependensi yang diekstrak secara sintaks untuk Rust. Supervisor menerima hanya hasil retrieval terbatas; tidak mengirim seluruh repositori ke model.

**Tech Stack:** Rust 2021, rusqlite/FTS5, `syn` full AST untuk Rust, sistem filesystem standard library.

**Spec:** `docs/specs/NEON-INTELLIGENCE-FEATURES-SPEC.md`, bagian RAG teks lokal dan Codebase Intelligence.

## Global Constraints

- RAG awal lexical FTS5 saja; jangan menambah embedding/vector server/provider.
- Root harus canonical dan seluruh file berada di dalam workspace; tolak symlink dan traversal.
- Batas: 10.000 file, total 64 MiB teks, 1 MiB/file, chunk 1.800 byte; batas terlampaui dilaporkan.
- Abaikan `.git`, `target`, `node_modules`, cache/build, binary, file tersembunyi rahasia (`.env*`, credential/key/token/private key).
- Setiap kutipan memuat relative path, line start/end, content hash; model tidak boleh memalsukan sumber.
- Indeks awal memahami deklarasi/dependensi Rust; bahasa lain tetap mendapat full-text search tanpa klaim indeks simbol semantik.

## Review Focus

1. Symlink atau path yang berubah saat indexing tidak boleh membuat pembacaan keluar workspace — tes B1.
2. File rahasia, binary, oversize, dan total workspace di atas cap tidak boleh masuk diam-diam — tes B1.
3. File berubah/hilang harus menghapus chunk dan simbol lama dari hasil — tes B2/B3.
4. UTF-8 multibyte di batas chunk tidak boleh menghasilkan data rusak atau rentang baris salah — tes B2.
5. Query yang tidak cocok harus mengembalikan tidak ada citation, bukan bukti buatan — tes B4.

---

### Task 1: Scanner workspace aman dan resource-capped

**Files:**
- Modify: `crates/neon/Cargo.toml` (add `sha2` for content fingerprints)
- Create: `crates/neon/src/knowledge.rs`
- Create: `crates/neon/src/knowledge/scanner.rs`
- Modify: `crates/neon/src/lib.rs`
- Test: `crates/neon/tests/knowledge.rs`

**Interfaces:**
- Produces `IndexLimits { max_files: usize, max_total_bytes: usize, max_file_bytes: usize }` dengan default 10_000, 64 MiB, 1 MiB.
- Produces `scan_workspace(root: &Path, limits: &IndexLimits) -> Result<Vec<ScannedFile>, KnowledgeError>`; `ScannedFile` mencatat relative path, modified time, byte length, SHA-256 content hash, UTF-8 text.

- [ ] Tulis tes `scanner_rejects_symlinks_and_outside_root`, `scanner_skips_secret_and_binary_files`, `scanner_reports_each_resource_cap`, dan `scanner_accepts_utf8_text` menggunakan temporary directories dan file sungguhan.
- [ ] Jalankan `cargo test -p neon --test knowledge scanner_`; harapkan gagal karena scanner belum ada.
- [ ] Implementasikan traversal deterministik, canonical root check, daftar exclusion, deteksi binary/UTF-8, hash file, dan pelaporan error/cap tanpa mengikuti symlink.
- [ ] Jalankan `cargo test -p neon --test knowledge scanner_`; harapkan semua traversal/cap tests lulus.

### Task 2: Chunking, FTS index, pembaruan dan citation

**Files:**
- Create: `crates/neon/src/knowledge/index.rs`
- Modify: `crates/neon/src/knowledge.rs`
- Test: `crates/neon/tests/knowledge.rs`

**Interfaces:**
- Produces `KnowledgeIndex::open(db_path: &Path)`, `reindex(root: &Path, limits: &IndexLimits) -> Result<IndexReport, KnowledgeError>`, dan `search(query: &str, max_hits: usize, max_bytes: usize) -> Result<Vec<KnowledgeHit>, KnowledgeError>`.
- `KnowledgeHit` fields: `path: PathBuf`, `start_line: u32`, `end_line: u32`, `content_hash: String`, `text: String`.

- [ ] Tulis tes `chunker_preserves_line_ranges_and_utf8`, `search_returns_path_lines_and_matching_text`, `reindex_updates_changed_files_and_removes_deleted_files`, dan `search_obeys_hit_and_byte_caps`.
- [ ] Jalankan tes baru; harapkan gagal pada schema/index/query belum tersedia.
- [ ] Implementasikan chunk 1.800 byte dengan overlap nol dan pemotongan hanya di batas karakter/baris yang valid; migrasikan tabel files/chunks/FTS dengan transaction dan replace stale records.
- [ ] Jalankan `cargo test -p neon --test knowledge`; harapkan FTS selalu sejalan dengan file index dan citation akurat.

### Task 3: Indeks simbol dan dependensi Rust

**Files:**
- Modify: `crates/neon/Cargo.toml` (add `syn` with `full` and `proc-macro2` with `span-locations`)
- Create: `crates/neon/src/knowledge/symbols.rs`
- Modify: `crates/neon/src/knowledge/index.rs`
- Test: `crates/neon/tests/codebase.rs`

**Interfaces:**
- Produces `SymbolHit { path, name, kind, signature, line }` dan `DependencyEdge { from_path, to_name, kind, line }`.
- `KnowledgeIndex::search_symbols(query: &str, max_hits: usize) -> Result<Vec<SymbolHit>, KnowledgeError>`.
- Parse hanya ekstensi `.rs` dengan `syn`; files lain tetap di chunk FTS dan tidak diklaim memiliki resolusi simbol.

- [ ] Tulis tes `rust_index_finds_functions_types_traits_and_modules`, `rust_index_records_use_and_mod_edges`, `invalid_rust_stays_searchable_as_text_but_has_no_symbols`, dan `reindex_removes_stale_symbols`.
- [ ] Jalankan `cargo test -p neon --test codebase`; harapkan gagal sebelum parser/tables dibuat.
- [ ] Implementasikan traversal AST untuk item tingkat modul, signatures dan line numbers (`proc-macro2` span-locations), edge extraction untuk `use`/`mod`; indeks ulang secara transaksional.
- [ ] Jalankan `cargo test -p neon --test codebase` dan `cargo clippy -p neon --all-targets -- -D warnings`; harapkan source invalid tetap dapat dicari teks dan tidak menghasilkan symbol palsu.

### Task 4: CLI search/index dan retrieval terbatas ke Supervisor

**Files:**
- Modify: `crates/neon/src/main.rs`
- Modify: `crates/neon/src/agent/supervisor.rs`
- Modify: `README.md`
- Test: `crates/neon/tests/cli.rs`

**Interfaces:**
- Tambah `neon knowledge index [relative-path]`, `neon knowledge search <query>`, dan `neon symbols search <query>`.
- `Supervisor::run_with_knowledge(model, input, memories, hits, budget)` menerima `KnowledgeHit` yang sudah dibatasi; jawaban internal wajib menyertakan path dan line range sebagai evidence.

- [ ] Tulis tes CLI untuk index/search offline, path citation, kosong ketika tidak ada hasil, dan tanpa model untuk command index/search.
- [ ] Jalankan tes CLI baru; pastikan gagal sebelum dispatcher command ada.
- [ ] Implementasikan command, pencetakan citation, dan integrasi retrieval yang memang hanya melewatkan hasil FTS relevant ke model, bukan isi repo seluruhnya.
- [ ] Jalankan `cargo fmt --all -- --check && cargo test -p neon`; harapkan index, symbols, citation, exclusion, dan caps semuanya terverifikasi.
