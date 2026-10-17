# Status Implementasi NEON

**Status:** prototipe lokal runnable dan teruji pada Linux x86_64; belum divalidasi pada Android/Termux ARM64 dengan model nyata.

## Kode yang tersedia

| Area | Implementasi saat ini |
|---|---|
| NeonAtom | Parser/evaluator Rust, lambda, aritmetika checked, let-binding, stream `range/map/filter/take`, agregasi, limit fuel/nesting/source/output/waktu, CLI `check/run/emit`. |
| Adaptive Reasoning | Permintaan umum sederhana memakai satu panggilan Leader; tugas coding/debug memilih analisis, permintaan pencarian memilih jalur search, dan permintaan tes/verifikasi memilih jalur test. Pemilihan berbasis sinyal tugas; jalur search/test tetap memerlukan tool/planner dan bukti aktual. |
| Confidence / Uncertainty | Estimasi Wilson lower bound 95% dari evidence `verified`/`failed` aktual; `not_run` dikecualikan. `neon confidence <plan-id>` menghitung bukti command Cargo yang tersimpan, meminta bukti bila sampel belum ada, atau menyarankan cek independen bila batas bawah di bawah ambang. Tidak memakai angka confidence dari keluaran model. |
| Hybrid Intelligence Router | Local adalah default. `neon ask --remote` meminta consent tiap tugas analisis dan hanya mengirim teks pertanyaan. Provider OpenAI-compatible butuh HTTPS untuk host luar; API key dibaca dari secret vault encrypted age. Tidak ada auto-fallback. |
| Model | Menjalankan `llama-server` sebagai child process; loopback only, health check, timeout, chat-completions lokal, bounded request/response, cleanup saat drop. Remote client juga bounded/no-redirect dan token request hanya di memori terzeroize. |
| Agen | Mike (Router), Susan (Programmer), Fani (Maintenance), Deby (Debugger), Dyor (Decomposer), Jordan (Summarizer), dan Naon (Leader). Satu backend dipakai sekuensial; CLI menampilkan roster dan jalur peran aktif; jawaban akhir diberi label Naon. |
| Smart Memory / Skill / Experience | SQLite/FTS5 untuk fakta, sesi ringkas, pengalaman, dan skills; retrieval dibatasi; record hasil inferensi memerlukan approval. Skill personal dapat dibuat, diedit dengan approval, dan dihapus; built-in terlindungi. Hasil plan terverifikasi ditawarkan sebagai experience opt-in. Tidak ada automatic chat-history logging atau weight fine-tuning. |
| RAG / Knowledge Graph | Reindex lokal FTS5 dengan citation path/baris/hash, exclusion rahasia/binary/build artifacts/symlink, caps scan; parser AST Rust untuk simbol dan dependensi. BFS graph menelusuri edge `mod`/`use` yang ter-resolve pada file terindeks dengan depth 1..8, edge max 200; dependency eksternal tetap unresolved. |
| Planner / Long-running tasks / Universal Tools | Dyor menghasilkan plan JSON strict memakai model lokal; parser menguji tiap tool terhadap enum allowlist; TaskStore SQLite menyimpan DAG/checkpoint/status serta resume setelah restart. Dispatcher menyatukan operasi typed untuk workspace, search, graph, Git, NeonAtom, dan Cargo. |
| Secure Sandbox | Linux Cargo execution menggunakan `bubblewrap`+`prlimit`: user/PID/network namespaces, source workspace read-only, target/tmpfs 512 MiB, CPU 180 s, address space 4 GiB, file 1 GiB, 512 process, command timeout 120 s dan output 16 KiB. Timeout membunuh seluruh process group. Tanpa bwrap/prlimit, command ditolak; Termux fail-closed. |
| Automated Test & Repair | `neon workspace test/check --repair`: maksimal 2 test attempts dan 1 proposal replacement file; exact diff perlu approval baru sebelum sandbox retest. `neon ask` untuk permintaan test meminta approval lalu menjalankan Cargo sandbox; `NotRun` tidak memicu repair. |
| Plugin & Skill Registry | Skills data-only yang sudah ada tetap tersedia. Plugin manifest data-only memiliki semantic version, API version, permission minimum per target allowlist; inactive by default, user confirm untuk register/enable, registry diverifikasi saat resolve, alias `plugins run` hanya melalui typed dispatcher dan menggunakan approval normal. |
| Benchmark | `neon benchmark` menjalankan 3 prompt lokal, menyimpan JSONL lokal, pass marker, latensi, token backend bila tersedia, RSS/HWM dari `/proc` bila tersedia dan membandingkan laporan model yang sama. Belum diukur pada model GGUF/Termux nyata. |
| Privacy / Recovery | Snapshot SQLite konsisten dienkripsi age dengan passphrase, restore autentikasi + SQLite integrity check sebelum replacement, file mode 0600, passphrase tanpa echo dan zeroize. Secret vault menyimpan tiap value dalam ciphertext age dan menjadi sumber API key remote; backup/purge mencakup database/vault. Purge juga menghapus WAL/SHM/history setelah approval; secure erase fisik flash tidak dijamin. |
| Verification | Evidence JSON untuk exit code, timeout, output truncation, readback file, citation, dan read-only tool result; model prose tidak mengubah status. Step hanya terselesaikan sesudah evidence verified. |
| Context / Coordinator | Byte budget prompt diturunkan secara konservatif dari context/response token (bukan exact tokenizer), 12 item context maksimum, 6 giliran recent maksimum; pemangkasan item/byte dilaporkan; satu LocalModel dipakai secara sekuensial dengan batas call. `neon resources` menampilkan batas, thread CPU terkonfigurasi, dan ukuran SQLite/WAL/SHM; tidak mengklaim mengukur RSS Android. |
| DDG | Perintah on-demand melalui HTTPS, parsing HTML, URL hasil divalidasi, batas kueri/hasil/body/timeout. |
| Tools/Lab | Workspace file terpisah, path relatif, traversal/symlink ditolak, file dibatasi 1 MiB, proposal replacement + approval, dan typed dispatcher tanpa shell generik. Cargo di Linux memakai sandbox OS bwrap/prlimit; host non-Linux menolak eksekusi sandboxed. |
| Dokumentasi | README Termux, panduan konfigurasi model lokal, keamanan dan batas klaim. |
| Termux launcher | `scripts/install-termux.sh` mem-build/memasang Rust binary; `scripts/run-termux.sh` memeriksa `llama-server` dan GGUF lalu menjalankan chat interaktif. |

## Verifikasi pada sandbox Linux

Perintah yang digunakan pada revisi terakhir:

```sh
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release --workspace
```

Semua **89 test otomatis** `cargo test --workspace` lulus pada revisi ini, termasuk graph traversal, sandbox Cargo tanpa jaringan/child-process leaks, repair cap, consent routing, plugin permissions, benchmark metrics, encrypted backup/restore dan secret vault. `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt -p neon -- --check`, dan `cargo build --release --workspace` lulus. `cargo fmt --all -- --check` belum hijau karena format lama di `crates/neonatom/src/lib.rs`; file itu tidak diformat massal agar perubahan evaluator tetap terbatas.

## Batas dan pekerjaan lanjutan yang belum diverifikasi

1. **Termux ARM64:** belum ada ponsel/NDK/Termux yang tersedia/terdaftar untuk sesi ini; installer/launcher tersedia tetapi build dan chat belum dieksekusi pada perangkat Android.
2. **Inference nyata:** tidak ada path `llama-server` atau bobot GGUF yang disediakan pada sesi ini. Protokol request diuji dengan server HTTP lokal deterministik; inference kualitas model dan lifecycle backend nyata belum diuji.
3. **Benchmark model nyata:** harness tersedia, tetapi belum dijalankan karena sesi ini tidak menyediakan GGUF/llama-server. Kemampuan bergantung pada model/kuantisasi/context/RAM; NEON bukan ASI.
4. **DDG langsung:** pencarian live berhasil pada sandbox, tetapi jaringan eksternal dapat tidak tersedia pada Termux/perangkat pengguna.
5. **Sandbox OS:** di Linux, `bubblewrap`/`prlimit` memisahkan namespace, menonaktifkan jaringan, membuat source workspace read-only dan membatasi scratch/resource. Ini bukan VM; tetap jalankan source tepercaya. Termux non-Linux menolak Cargo sandboxed karena belum ada implementasi OS Android setara.
6. **Riwayat chat:** tidak ada session-history persistence agar percakapan tidak direkam otomatis. Memori personal berdiri sendiri dan opt-in.
7. **Konfigurasi:** environment variables dan default aman; file konfigurasi TOML belum dibuat.
8. **Inference end-to-end:** belum tersedia path GGUF/backend di lingkungan ini; alur `neon plan create` yang meminta model lokal perlu diuji pada model nyata. Parser, approval, plan storage, dan eksekusi tools telah diuji terpisah dengan data/proses lokal.

Perintah uji/pengukuran berikutnya pada perangkat pengguna harus dicatat bersama model/quant, versi llama.cpp/toolchain, RAM yang tersedia, context/thread, temperatur perangkat, hasil code tests, latensi dan throughput. Tidak ada model yang diunduh otomatis.
