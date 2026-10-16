# NEON Termux Agent Implementation Plan

> **For agentic workers:** Implementasikan setelah rencana NeonAtom selesai dan semua tesnya lulus. Ikuti `docs/NEON-SPEC.md`; tandai checkbox per langkah.

**Goal:** Membangun agen CLI Rust NEON untuk Termux yang menjalankan model GGUF lokal melalui `llama.cpp`, menyimpan memori personal yang disetujui pengguna, mencari dengan DuckDuckGo bila diminta, dan menyediakan lab coding ber-batas.

**Architecture:** Satu Cargo workspace berisi crate DSL `neonatom`, CLI NeonAtom, dan crate aplikasi `neon`. NEON mengelola proses backend inference lokal, memory SQLite/FTS5, search DDG, dan workspace tools. NeonAtom dipakai hanya untuk workflow DSL murni dengan limit; bukan untuk mengeksekusi source model sebagai kode host.

**Tech Stack (implemented):** Rust stable/Cargo workspace, `reqwest` blocking + rustls, `serde`/`serde_json`, `rusqlite` bundled dengan FTS5, `scraper`, crate lokal `neonatom`. Backend lokal `llama.cpp` (`llama-server`) dan model GGUF disediakan pengguna. CLI memakai parser argumen kecil buatan proyek; belum ada `clap`, `tokio`, `tracing`, atau TOML config.

**Spec:** `docs/NEON-SPEC.md`; prasyarat bahasa: `docs/NEONATOM-SPEC.md`.

## Global Constraints

- Implementasikan dan uji NeonAtom lebih dulu, sesuai `docs/superpowers/plans/2026-10-16-neonatom-language.md`.
- Target Termux Android RAM 8 GB/storage 128 GB; kapasitas bebas aktual diukur.
- Tanpa provider model cloud/API key; prompt inference hanya ke proses loopback lokal.
- NEON bukan ASI; memori personal adalah retrieval dari data yang pengguna setujui, bukan pelatihan bobot.
- NeonAtom mewakili pola berulang, bukan kompresi universal atau storage 1 MB menjadi 1 TB.
- Sintaks NeonAtom tidak mengecilkan bobot AI atau membuat model lebih pintar; model awal sekitar 1.5B–4B adalah kandidat untuk benchmark, bukan janji kecakapan.
- Tanpa auto-download model, auto-run kode model, shell arbitrer atau klaim sandbox kernel.
- DDG hanya dipanggil untuk pencarian eksplisit; memori/riwayat tidak dimasukkan ke query otomatis.
- Tujuh spesialis adalah peran logis yang menggunakan satu model lokal secara sekuensial; jangan memuat tujuh set bobot atau mengklaim setiap peran sebagai model terpisah.
- Komunikasi agen berupa paket typed JSON/enum Rust yang ringkas, bukan machine code/assembly; Leader menyampaikan jawaban natural dalam Bahasa Indonesia.
- Runtime nyata tanpa placeholder/mock/TODO/stub. Test dapat menggunakan server lokal deterministik.
- Build dan inference diuji pada perangkat Termux ARM64 nyata sebelum dukungan diklaim.

## Review Focus

- Host inference non-loopback — validasi gagal sebelum request dan backend hanya bind ke loopback (Task 1/3).
- Backend/model hilang, port bentrok, readiness timeout — penanganan error tanpa proses yatim (Task 3).
- Memori sensitif atau penghapusan tidak tuntas — tes persetujuan, redaksi kategori sensitif dan purge (Task 4).
- Instruksi jahat hasil DDG atau memori privat — query eksplisit saja dan data web tidak dieksekusi (Task 5/6).
- Traversal/symlink dan perintah berbahaya — path ditolak dan executable di luar allowlist ditolak (Task 6).

---

## Workspace dan peta file

Root `Cargo.toml` adalah Cargo workspace bersama. Crate NeonAtom berada pada `crates/neonatom`, CLI bahasanya `crates/neonatom-cli`, aplikasi ini berada pada `crates/neon` dan dependency-nya adalah crate path `neonatom`. Rencana bahasa membuat crate dan tesnya; rencana ini tidak mengganti atau menyalin implementasi tersebut.

- `crates/neon/src/main.rs`, `crates/neon/src/cli.rs`: bootstrap dan REPL.
- `crates/neon/src/config.rs`: path backend/model, limits, workspace, DDG; tanpa konfigurasi provider.
- `crates/neon/src/model/{mod.rs,llama_server.rs}`: lifecycle process backend dan klien loopback.
- `crates/neon/src/memory.rs`: SQLite/FTS5 dan API CRUD memori.
- `crates/neon/src/search/{mod.rs,duckduckgo.rs}`: DDG dan parsing hasil.
- `crates/neon/src/lab/{mod.rs,workspace.rs,commands.rs}`: filesystem workspace dan perintah allowlist.
- `crates/neon/src/agent.rs`: entry point agent dan deklarasi submodul.
- `crates/neon/src/agent/{roles.rs,protocol.rs,router.rs,supervisor.rs}`: tujuh role, kontrak paket, klasifikasi/routing dan alur kerja/budget.
- `crates/neon/tests/`: integration tests.
- `README.md`, `SECURITY.md`: dokumentasi bersama; koordinasikan penambahan tanpa menimpa dokumentasi NeonAtom.

### Task 1: Tambahkan crate aplikasi dan konfigurasi tervalidasi

**Files:** `Cargo.toml` (workspace root yang dibuat Task 1 NeonAtom), `crates/neon/Cargo.toml`, `crates/neon/src/main.rs`, `crates/neon/src/config.rs`, `crates/neon/tests/config.rs`

**Interfaces:** `Config::load() -> Result<Config, ConfigError>`; `Config` berisi `llama_server_bin: PathBuf`, `model_path: PathBuf`, `host: IpAddr`, `port: u16`, `context_tokens: u32`, `threads: usize`, `workspace: PathBuf`, `max_response_bytes: usize`; host diwajibkan loopback. Crate memakai `neonatom` dengan path dependency.

- [ ] Pastikan plan NeonAtom selesai, crate tersedia, `cargo test --workspace` lulus, lalu tambahkan `crates/neon` sebagai member tanpa mengubah source bahasa.
- [ ] Tulis tes konfigurasi untuk backend/model hilang, path model tidak valid, host non-loopback, context/thread/port di luar rentang dan Debug yang tidak membocorkan data privat.
- [ ] Jalankan `cargo test -p neon`; pastikan tes konfigurasi gagal sebelum implementasi.
- [ ] Implementasikan parsing TOML/environment dengan precedence environment > file > default aman; backend/model wajib dipilih, endpoint cloud ditolak.
- [ ] Jalankan `cargo test -p neon`; semua tes lulus.
- [ ] Commit: `feat: add NEON workspace crate and local configuration`.

### Task 2: CLI, SQLite, dan histori sesi lokal

**Files:** `crates/neon/src/main.rs`, `crates/neon/src/cli.rs`, `crates/neon/src/memory.rs`, `crates/neon/tests/cli.rs`

**Interfaces:** `Cli::parse() -> Cli`; `SessionStore::open(path: &Path) -> Result<SessionStore>`; `append_turn(session_id: &str, role: Role, text: &str) -> Result<()>`; `recent_turns(session_id: &str, limit: usize) -> Result<Vec<Turn>>`.

- [ ] Tulis tes `/help`, `/exit`, input kosong, histori sesi persisten, dan skema tanpa kolom/provider credential.
- [ ] Jalankan `cargo test -p neon --test cli`; pastikan gagal sebelum implementasi.
- [ ] Implementasikan REPL `clap` dan SQLite lokal. Definisikan routing `/memory` dan `/forget` ke API Task 4.
- [ ] Jalankan tes CLI; semua tes lulus.
- [ ] Commit: `feat: add NEON CLI and local session history`.

### Task 3: Lifecycle inference lokal llama.cpp

**Files:** `crates/neon/src/model/mod.rs`, `crates/neon/src/model/llama_server.rs`, `crates/neon/tests/local_model.rs`

**Interfaces:** `LocalModel::start(config: &ModelConfig) -> Result<LocalModel>`; `complete(&self, messages: &[Message]) -> Result<Completion, ModelError>`; `shutdown(&mut self) -> Result<()>`. Konfigurasi wajib memungkinkan pilihan model/quant dan context yang dapat diuji dalam RAM bebas.

- [ ] Susun set benchmark lokal Bahasa Indonesia/koding yang mencakup percakapan alami, penjelasan teknis, terjemahan istilah dengan identifier terjaga, debugging, implementasi fitur kecil dan tugas yang diverifikasi dengan test; tetapkan rubric akurasi, instruksi, ketidakpastian, dan kualitas kode.
- [ ] Tulis tes server lokal deterministik untuk chat success, HTTP error/JSON malformed, body besar, timeout, endpoint non-loopback dan shutdown child process.
- [ ] Tulis tes backend binary/model tidak ada, port dipakai, readiness timeout; pastikan cleanup dan tiada proses yatim.
- [ ] Jalankan `cargo test -p neon --test local_model`; pastikan gagal sebelum implementasi.
- [ ] Implementasikan peluncuran `llama-server` tanpa shell, model/context/thread, bind `127.0.0.1`, health check, request/response limit, timeout, shutdown proses dan redaksi log. Dokumentasikan kandidat model/quant ukuran kecil, tetapi jangan memilih hanya dari ukuran atau mengklaim unggul tanpa benchmark.
- [ ] Jalankan tes lokal dan manual dengan llama-server serta GGUF sungguhan; seluruh tes lulus.
- [ ] Commit: `feat: run local GGUF inference through llama.cpp`.

### Task 4: Memori personal opt-in

**Files:** `crates/neon/src/memory.rs`, `crates/neon/tests/memory.rs`

**Interfaces:** `MemoryStore::propose(text: &str, category: MemoryCategory) -> Result<MemoryProposal>`; `approve(proposal_id: &str) -> Result<MemoryId>`; `search(query: &str, limit: usize) -> Result<Vec<Memory>>`; `update(id: MemoryId, text: &str) -> Result<()>`; `forget(id: MemoryId) -> Result<()>`; `forget_all() -> Result<()>`.

- [ ] Tulis tes proposal tidak tersimpan sebelum persetujuan, pencarian FTS5, edit, hapus item/semua, dan kategori sensitif tidak tersimpan otomatis.
- [ ] Jalankan `cargo test -p neon --test memory`; pastikan gagal sebelum implementasi.
- [ ] Implementasikan SQLite/FTS5, transaksi dan API proposal/approval; tampilkan teks memori sebelum simpan. Sediakan operasi `/memory` dan `/forget`.
- [ ] Jalankan tes memory; semua tes lulus.
- [ ] Commit: `feat: add user-approved local personal memory`.

### Task 5: Pencarian DDG on-demand

**Files:** `crates/neon/src/search/mod.rs`, `crates/neon/src/search/duckduckgo.rs`, `crates/neon/tests/duckduckgo.rs`

**Interfaces:** `SearchResult { title: String, url: Url, snippet: Option<String> }`; `DuckDuckGo::search(query: &str, limit: usize) -> Result<Vec<SearchResult>, SearchError>`.

- [ ] Tulis tes fixture HTML untuk hasil, encoding, redirect, empty/malformed, URL http/https dan limit.
- [ ] Tulis tes bahwa request hanya memakai query eksplisit dan tidak melampirkan histori atau memory.
- [ ] Jalankan `cargo test -p neon --test duckduckgo`; pastikan gagal sebelum implementasi.
- [ ] Implementasikan HTTP DDG ber-timeout, batas body, URL encoding, ekstraksi/sanitasi URL sumber; hasil dianggap data, bukan instruksi.
- [ ] Jalankan tes; semua lulus.
- [ ] Commit: `feat: add explicit DuckDuckGo search`.

### Task 6: Multi-agent workflow, NeonAtom dan lab coding

**Files:** `crates/neon/src/agent.rs`, `crates/neon/src/agent/{roles.rs,protocol.rs,router.rs,supervisor.rs}`, `crates/neon/src/lab/mod.rs`, `crates/neon/src/lab/workspace.rs`, `crates/neon/src/lab/commands.rs`, `crates/neon/tests/protocol.rs`, `crates/neon/tests/routing.rs`, `crates/neon/tests/agent.rs`, `crates/neon/tests/workspace.rs`

**Interfaces:** `enum AgentRole { Programmer, Maintenance, Debugger, Decomposer, Router, Summarizer, Leader }`; `AgentPacket { role: AgentRole, task: String, context: Vec<String>, constraints: Vec<String>, result: Option<String>, evidence: Vec<String>, status: PacketStatus, confidence: Option<f32> }`; `AgentProtocol::encode(&AgentPacket) -> Result<String>` dan `decode(&str) -> Result<AgentPacket>`; `Router::route(&TaskPacket, model: &LocalModel) -> Result<Vec<AgentRole>>` dengan validasi enum kategori dan fallback aman untuk ambiguitas; `Supervisor::run(input: &str, budget: AgentBudget) -> Result<AgentResponse>`. Supervisor reuses one `LocalModel` serially; NeonAtom workflow uses `neonatom::parse` then `neonatom::eval` with explicit `Limits`. Lab interfaces: `Workspace::open(root: PathBuf) -> Result<Workspace>`; `read_file(relative: &Path) -> Result<Vec<u8>>`; `propose_write(relative: &Path, content: &[u8]) -> Result<ChangeDiff>`; `apply_write(diff_id: &str, approved: bool) -> Result<()>`; `CommandPolicy::validate(argv: &[OsString]) -> Result<ApprovedCommand>`.

- [ ] Tulis tes protocol valid encode/decode, schema invalid ditolak, field wajib dan panjang paket terbatas.
- [ ] Tulis tes routing model lokal untuk tujuh kategori peran: coding → Programmer, maintenance/refactor → Maintenance, error/reproduksi → Debugger, tugas kompleks → Decomposer, klasifikasi → Router, sintesis → Summarizer, jawaban pengguna → Leader; tes fixture membuktikan kategori valid dipetakan benar dan output kategori invalid/ambigu gagal tertutup atau meminta klarifikasi.
- [ ] Tulis tes supervisor memastikan Router mengklasifikasikan awal, Decomposer memberi label kategori tervalidasi pada unit tugas, unit dikirim ke spesialis yang sesuai → Summarizer → Leader, shortcut untuk tugas sederhana, shared model hanya dipakai sekuensial, jumlah panggilan/context/fuel dibatasi, schema invalid maksimal retry yang ditentukan, konflik/ketidakpastian tidak dihapus, dan jawaban Leader Bahasa Indonesia mempertahankan code identifiers.
- [ ] Tulis tes containment path normal/traversal/absolute/symlink, file limits, diff/approval, command allowlist dan tanpa shell.
- [ ] Tulis tes integrasi bahwa workflow NeonAtom menjalankan DSL hanya dengan limits, model lokal dipakai bersama (bukan tujuh model simultan), memory disertakan hanya melalui retrieval sesuai permintaan, memory baru butuh approval, hasil DDG tidak dieksekusi, perubahan file butuh approval, tiada loop tanpa batas.
- [ ] Jalankan `cargo test -p neon --test protocol --test routing --test workspace --test agent`; pastikan tes baru gagal sebelum implementasi.
- [ ] Implementasikan protocol serde typed dan validasi panjang/field; prompt Router lokal untuk klasifikasi; konversi hasil ke enum kategori tervalidasi dengan fallback klarifikasi; enam prompt spesialis lain; supervisor sekuensial berbatas dengan satu backend, context reuse terukur, retry schema terbatas, serta sintesis yang menjaga bukti/konflik. Leader menyusun jawaban Indonesia alami.
- [ ] Implementasikan workspace validasi fail-closed, write atomik, proses allowlist tanpa shell/cwd workspace/env minimal/timeout/output limit; panggil API NeonAtom sebagai library ber-limit, tidak sebagai subprocess bebas.
- [ ] Jalankan tes; semua lulus.
- [ ] Commit: `feat: add bounded multi-agent NeonAtom workflow and guarded coding lab`.

### Task 7: Dokumentasi dan verifikasi Termux ARM64

**Files:** `README.md`, `SECURITY.md`, workspace CI/tests.

**Interfaces:** Instruksi lengkap untuk build workspace, memasang `llama.cpp`, memilih GGUF/lisensi, konfigurasi, workflow NeonAtom, memory approval/delete, dan batas keamanan.

- [ ] Tambah regression test offline, DDG offline, model/backend hilang, memory purge, cleanup proses dan DSL limits.
- [ ] Jalankan `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo build --workspace --release`.
- [ ] Uji pada Termux ARM64 nyata: NeonAtom CLI, build NEON, inference offline dengan GGUF yang lisensinya diperiksa, memory approve/forget, DDG eksplisit, workspace/allowlist, dan benchmark set Indonesia/koding pada kandidat model/quant yang dapat dimuat. Catat versi, ukuran model, context/thread, RAM/ruang aktual, latency/token per detik, skor tiap rubric dan tugas coding yang lolos tes.
- [ ] Dokumentasikan bahwa NeonAtom tidak menyimpan output besar secara ajaib, tidak melatih bobot, memori lokal tidak otomatis terenkripsi, dan kontrol lab bukan sandbox OS.
- [ ] Commit: `docs: document fully local NEON and NeonAtom on Termux`.


## Pembaruan pelaksanaan native

Implementasi awal telah dikerjakan langsung atas permintaan pengguna. Untuk keadaan kode dan batas pengujian terbaru lihat [`docs/IMPLEMENTATION-STATUS.md`](../../IMPLEMENTATION-STATUS.md). Source saat ini memakai file aktual `agent/model.rs`, `agent/router.rs`, `agent/supervisor.rs`, `lab.rs`, dan `search.rs` (berbeda dari pemecahan file yang direncanakan awal). Tes workspace dan binary release Linux x86_64 berhasil. Langkah hardware Termux ARM64, inferensi GGUF nyata, benchmark bahasa Indonesia/koding, model/license selection, session-history opt-in, dan penguatan isolasi OS tetap belum selesai/diverifikasi; checkbox rencana yang belum dipenuhi sengaja tidak ditandai selesai. Konfirmasi `neon workspace test/check` bukan sandbox kernel.
