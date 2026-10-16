# Status Implementasi NEON

**Status:** prototipe lokal runnable dan teruji pada Linux x86_64; belum divalidasi pada Android/Termux ARM64 dengan model nyata.

## Kode yang tersedia

| Area | Implementasi saat ini |
|---|---|
| NeonAtom | Parser/evaluator Rust, lambda, aritmetika checked, let-binding, stream `range/map/filter/take`, agregasi, limit fuel/nesting/source/output/waktu, CLI `check/run/emit`. |
| Model | Menjalankan `llama-server` sebagai child process; loopback only, health check, timeout, chat-completions lokal, bounded request/response, cleanup saat drop. |
| Agen | Mike (Router), Susan (Programmer), Fani (Maintenance), Deby (Debugger), Dyor (Decomposer), Jordan (Summarizer), dan Naon (Leader). Satu backend dipakai sekuensial; CLI menampilkan roster dan jalur peran aktif; jawaban akhir diberi label Naon. |
| Smart Memory / Skill / Experience | SQLite/FTS5 untuk fakta, sesi ringkas, pengalaman, dan skills; retrieval dibatasi; record hasil inferensi memerlukan approval. Skill personal dapat dibuat, diedit dengan approval, dan dihapus; built-in terlindungi. Hasil plan terverifikasi ditawarkan sebagai experience opt-in. Tidak ada automatic chat-history logging atau weight fine-tuning. |
| RAG / Codebase | Reindex lokal FTS5 dengan citation path/baris/hash, exclusion rahasia/binary/build artifacts/symlink, caps scan; parser AST Rust untuk simbol dan dependensi. Hasil lokal diambil ke prompt dengan ContextManager. |
| Planner / Tools | Dyor menghasilkan plan JSON strict memakai model lokal; parser menguji tiap tool kembali terhadap enum allowlist termasuk symbols/dependencies; TaskStore SQLite menyimpan DAG/checkpoint/status. `neon plan run` menjalankan read-only tools dan meminta approval sebelum write/Cargo; maksimal satu retry. |
| Verification | Evidence JSON untuk exit code, timeout, output truncation, readback file, citation, dan read-only tool result; model prose tidak mengubah status. Step hanya terselesaikan sesudah evidence verified. |
| Context / Coordinator | Byte budget prompt diturunkan secara konservatif dari context/response token (bukan exact tokenizer), 12 item context maksimum, 6 giliran recent maksimum; pemangkasan item/byte dilaporkan; satu LocalModel dipakai secara sekuensial dengan batas call. `neon resources` menampilkan batas, thread CPU terkonfigurasi, dan ukuran SQLite/WAL/SHM; tidak mengklaim mengukur RSS Android. |
| DDG | Perintah on-demand melalui HTTPS, parsing HTML, URL hasil divalidasi, batas kueri/hasil/body/timeout. |
| Tools/Lab | Workspace file terpisah, path relatif, path traversal/symlink ditolak, file dibatasi 1 MiB, proposal replacement + approval, Cargo test/check allowlist dengan konfirmasi, timeout/output cap. Ini bukan isolasi OS. Tidak ada arbitrary shell tool. |
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

Semua **66 test otomatis** `cargo test --workspace` lulus, termasuk context-byte budget, strict JSON plan, checkpoint/resume/retry cap, CLI persistent plan, resource diagnostics, approval sebelum file write/Cargo, readback evidence, RAG/codebase symbol tools, memory/experience approval, skill FTS update, dan tes HTTP lokal untuk kontrak client model. `cargo clippy --workspace --all-targets -- -D warnings` dan `cargo fmt --all -- --check` lulus. Release binary Linux x86_64 terukur: `neon` 7,028,152 byte; `neonatom` 357,536 byte. Smoke test `neon --help`, `neon resources`, dan `neon team` lulus tanpa model; sintaks kedua skrip Termux valid. Query langsung DuckDuckGo pernah berhasil pada sandbox, tetapi tidak diuji ulang pada perubahan ini.

## Batas dan pekerjaan lanjutan yang belum diverifikasi

1. **Termux ARM64:** belum ada ponsel/NDK/Termux yang tersedia/terdaftar untuk sesi ini; installer/launcher tersedia tetapi build dan chat belum dieksekusi pada perangkat Android.
2. **Inference nyata:** tidak ada path `llama-server` atau bobot GGUF yang disediakan pada sesi ini. Protokol request diuji dengan server HTTP lokal deterministik; inference kualitas model dan lifecycle backend nyata belum diuji.
3. **Benchmark bahasa Indonesia/koding:** belum dijalankan. Kemampuan bergantung pada model GGUF, kuantisasi, context, dan RAM bebas aktual; NEON bukan ASI.
4. **DDG langsung:** pencarian live berhasil pada sandbox, tetapi jaringan eksternal dapat tidak tersedia pada Termux/perangkat pengguna.
5. **Sandbox OS:** workspace hanya membatasi operasi file melalui API NEON. `cargo test/check` tetap berjalan dengan izin akun Termux yang sama setelah konfirmasi; build scripts dapat menjalankan kode. Jangan pakai untuk source tidak tepercaya.
6. **Riwayat chat:** tidak ada session-history persistence agar percakapan tidak direkam otomatis. Memori personal berdiri sendiri dan opt-in.
7. **Konfigurasi:** environment variables dan default aman; file konfigurasi TOML belum dibuat.
8. **Inference end-to-end:** belum tersedia path GGUF/backend di lingkungan ini; alur `neon plan create` yang meminta model lokal perlu diuji pada model nyata. Parser, approval, plan storage, dan eksekusi tools telah diuji terpisah dengan data/proses lokal.

Perintah uji/pengukuran berikutnya pada perangkat pengguna harus dicatat bersama model/quant, versi llama.cpp/toolchain, RAM yang tersedia, context/thread, temperatur perangkat, hasil code tests, latensi dan throughput. Tidak ada model yang diunduh otomatis.
