# NeonAtom Language Implementation Plan

> **For agentic workers:** Implementasikan berurutan dan tandai checkbox; rujuk `docs/NEONATOM-SPEC.md`.

**Goal:** Membuat bahasa ekspresi/stream kecil bernama NeonAtom dalam Rust yang ringkas, deterministik, lazy, dan dibatasi resource sebagai fondasi sebelum NEON.

**Architecture:** Workspace Cargo berisi library parser/evaluator dan binary CLI. Source diparse ke AST; evaluator typed dengan fuel/nesting/output limits dan stream lazy tanpa akses host. CLI dapat menjalankan file, menampilkan hasil ringkas, dan mengeluarkan sejumlah elemen yang dibatasi.

**Tech Stack:** Rust stable, Cargo, std library terlebih dahulu; dependencies tambahan hanya bila membuktikan perlu dan tidak membahayakan target ukuran. Tests dengan `cargo test`, tanpa mock runtime.

**Spec:** `docs/NEONATOM-SPEC.md`

## Global Constraints

- NeonAtom merepresentasikan pola berulang, bukan kompresi universal: 1 MB tidak secara fisik setara 1 TB.
- Target executable stripped <1 MiB diukur, bukan dijamin sebelum build.
- Tidak ada akses host filesystem/network/proses pada evaluator; tidak memakai `eval` host language atau unsafe evaluator.
- Default limits: fuel 1.000.000 langkah, nesting 128, source 1 MiB, output 16 MiB, waktu 2 detik.
- Checked integer arithmetic; error eksplisit untuk overflow, pembagian nol, batas dan parse.
- Tidak ada placeholder/mock/TODO/stub sebagai fitur runtime.

## Review Focus

- Ekspresi nested atau input besar menyebabkan stack/resource exhaustion — tes nesting/source/fuel/output limit (Task 2–4).
- Overflow dan pembagian nol memberikan nilai salah — tes arithmetic checked dan diagnostic (Task 3).
- Stream besar/tak terbatas termaterialisasi — tes `take` memproduksi jumlah terbatas dan sum/count mematuhi fuel (Task 4).
- Filter tidak pernah yield atau fungsi rekursif menghindari limit — tes fuel menghentikan proses secara deterministik (Task 4).
- Klaim ukuran executable/rasio representasi tidak terverifikasi — acceptance mencatat ukuran biner dan membedakan representasi dari output fisik (Task 5).

---

## Peta file

- `Cargo.toml`, `Cargo.lock`: workspace crate `neonatom` library dan binary.
- `crates/neonatom/src/lib.rs`: public API `parse`, `eval`, `Limits`, `Value`.
- `crates/neonatom/src/lexer.rs`: tokenisasi dan lokasi sumber.
- `crates/neonatom/src/parser.rs`: grammar dan AST.
- `crates/neonatom/src/value.rs`: typed values dan lazy stream.
- `crates/neonatom/src/eval.rs`: evaluator, fuel, built-ins.
- `crates/neonatom/src/diagnostic.rs`: error lokasi/jenis.
- `crates/neonatom-cli/src/main.rs`: `run`, `check`, `emit`.
- `tests/`: parser, evaluator, limits, CLI.
- `README.md`: sintaks, batas, benchmark dan build.

### Task 1: Workspace crate dan diagnostic source

**Files:** `Cargo.toml`, `Cargo.lock`, `crates/neonatom/Cargo.toml`, `crates/neonatom/src/lib.rs`, `crates/neonatom/src/diagnostic.rs`, `crates/neonatom-cli/Cargo.toml`, `crates/neonatom-cli/src/main.rs`, `tests/diagnostics.rs`

**Interfaces:** `Diagnostic { kind: DiagnosticKind, message: String, span: Span }`; `Span { start: usize, end: usize }`; crate library public types dan error ini.

- [ ] Tulis tes diagnostik untuk rentang byte yang valid, input UTF-8, display error dan kode keluar CLI.
- [ ] Jalankan `cargo test -p neonatom`; verifikasi gagal sebelum implementasi.
- [ ] Implementasikan workspace dan tipe diagnostic/span; CLI memiliki command `run`, `check`, `emit` dengan usage/error yang nyata.
- [ ] Jalankan `cargo test -p neonatom`; seluruh tes lulus.
- [ ] Commit: `feat: initialize NeonAtom workspace and diagnostics`.

### Task 2: Lexer dan parser grammar inti

**Files:** `crates/neonatom/src/lexer.rs`, `crates/neonatom/src/parser.rs`, `tests/parser.rs`

**Interfaces:** `lex(source: &str) -> Result<Vec<Token>, Diagnostic>`; `parse(source: &str) -> Result<Program, Diagnostic>`; AST `Program`, `Expr`, `Function`, `Span`.

- [ ] Tulis tes precedence aritmetika/logika, let binding, fungsi, call, `range/map/filter/take/sum/count/checksum/print`, string/boolean, input kosong dan syntax error dengan span tepat.
- [ ] Jalankan `cargo test -p neonatom --test parser`; verifikasi gagal sebelum implementasi.
- [ ] Implementasikan lexer recursive-descent parser untuk grammar yang dites; tolak source di atas 1 MiB dan nesting di atas 128 secara terkontrol.
- [ ] Jalankan tes parser; seluruh tes lulus.
- [ ] Commit: `feat: parse NeonAtom expression language`.

### Task 3: Evaluator ekspresi dan fungsi murni

**Files:** `crates/neonatom/src/value.rs`, `crates/neonatom/src/eval.rs`, `tests/eval.rs`

**Interfaces:** `Limits { fuel: u64, max_nesting: usize, max_output_bytes: usize, timeout: Duration }`; `Value` typed; `eval(program: &Program, limits: Limits) -> Result<Value, RuntimeError>`; evaluasi let binding/fungsi/built-in murni.

- [ ] Tulis tes nilai integer/boolean/string, lexical scope, fungsi murni, checked add/sub/mul, division-by-zero, integer overflow, type errors, fuel exhaustion dan nesting.
- [ ] Jalankan `cargo test -p neonatom --test eval`; verifikasi gagal sebelum implementasi.
- [ ] Implementasikan evaluator deterministic; kurangi fuel pada setiap node/call, checked arithmetic dan typed errors. Tidak ada eval bahasa host atau akses host.
- [ ] Jalankan tes evaluator; seluruh tes lulus.
- [ ] Commit: `feat: evaluate bounded pure NeonAtom expressions`.

### Task 4: Lazy streams dan batas materialisasi

**Files:** `crates/neonatom/src/value.rs`, `crates/neonatom/src/eval.rs`, `tests/streams.rs`

**Interfaces:** `Stream::range(start: i64, end: i64) -> Stream`; `Stream::take(self, count: u64) -> Stream`; evaluator `map`, `filter`, `sum`, `count`, `checksum` dengan evaluator/fuel yang sama; `emit` wajib menerima output limit.

- [ ] Tulis tes `range(0, 1_000_000_000_000)` + `take(..., 10)` menghasilkan hanya sepuluh nilai; sum/count/checksum stream finite sesuai hasil; filter tanpa yield dihentikan fuel; infinite/unbounded emission memerlukan take; output limit menghentikan emit.
- [ ] Jalankan `cargo test -p neonatom --test streams`; verifikasi gagal sebelum implementasi.
- [ ] Implementasikan stream sebagai iterator composable lazy yang memajukan evaluator per elemen dan memakai fuel serta limit output; jangan mengumpulkan seluruh range ke Vec.
- [ ] Jalankan tes stream; seluruhnya lulus dan pemeriksaan memori menunjukkan `take(10)` tidak mengalokasikan proporsional terhadap panjang range.
- [ ] Commit: `feat: add lazy bounded NeonAtom streams`.

### Task 5: CLI, dokumentasi dan ukuran build terukur

**Files:** `crates/neonatom-cli/src/main.rs`, `tests/cli.rs`, `README.md`, `SECURITY.md`.

**Interfaces:** `neonatom run <file>`; `neonatom check <file>`; `neonatom emit <file> --limit <count>`; command menampilkan diagnostic dan status gagal yang konsisten.

- [ ] Tulis tes CLI untuk source valid/salah, run result, check-only, emit limit, source >1 MiB dan exit code.
- [ ] Jalankan tes CLI; verifikasi gagal sebelum implementasi.
- [ ] Implementasikan CLI dengan pembacaan berkas input berbatas, batas evaluasi default, dan output terkontrol. Dokumentasikan contoh pola dan bahwa hasil 1 TB tidak otomatis disimpan/materialisasi.
- [ ] Jalankan `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo build --release`.
- [ ] Ukur executable stripped di host dan Termux ARM64 bila tersedia; dokumentasikan toolchain/ukuran aktual dan apakah target <1 MiB tercapai. Jangan menyebut <1 MiB bila belum terukur.
- [ ] Commit: `feat: ship NeonAtom CLI with measured footprint`.

## Integrasi sebagai prasyarat NEON
Sebelum mengimplementasikan rencana `docs/superpowers/plans/2026-10-16-neon-termux-agent.md`, crate NeonAtom harus tersedia dan tesnya lulus. NEON memakai `neonatom::parse`/`eval` hanya untuk DSL workflow deklaratif yang ber-batas; model, memori, dan inference tetap mempunyai limit resource sendiri. Jangan gunakan bahasa ini untuk menjalankan source model sebagai kode host atau untuk menyatakan model 1 TB muat dalam 1 MB.
