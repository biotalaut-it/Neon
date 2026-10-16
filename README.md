# NEON — agen AI lokal untuk Termux

NEON adalah runtime agen Rust yang menjalankan **satu model GGUF open-weight lokal** melalui `llama-server`. Tim memiliki tujuh nama tetap: **Naon (Leader), Susan (Programmer), Fani (Maintenance), Deby (Debugger/Pemecah Masalah), Dyor (Decomposer), Mike (Router), dan Jordan (Summarizer)**. Semua peran dijalankan **bergiliran oleh model yang sama**—bukan tujuh model paralel. Tidak ada API model/cloud provider.

> NEON bukan ASI dan tidak membuktikan kecerdasan melampaui manusia. Model lokal membatasi mutu, pengetahuan, bahasa, dan kemampuan coding. Bahasa ringkas, kuantisasi, maupun rumus matematika tidak mengubah 1 MB menjadi 1 TB secara fisik.

## Yang sudah diimplementasikan

- **NeonAtom**: parser dan evaluator Rust tanpa dependency runtime, operasi aritmetika checked, lambda `x => expr`, stream lazy `range/map/filter/take`, batas fuel/nesting/waktu/byte, dan CLI `check`, `run`, `emit`.
- **NEON agent loop**: tujuh peran dengan prompt berbeda dan paket internal JSON bertipe; router kategori tervalidasi; decomposer menghasilkan 2–5 sub-tugas tervalidasi; summarizer dan Leader menyusun jawaban akhir Bahasa Indonesia.
- **Model lokal**: child process `llama-server` hanya bind `127.0.0.1`, readiness check, endpoint OpenAI-compatible lokal, timeout, limit response, dan perkiraan byte prompt yang diturunkan dari context/response tokens. Estimasi byte bukan tokenizer tepat. Tidak ada fallback ke cloud.
- **Smart Memory**: SQLite + FTS5 untuk preferensi/fakta, ringkasan sesi, skill, dan pengalaman. Memori hasil inferensi perlu persetujuan; ringkasan berasal dari buffer volatile terbatas; sesi mentah tidak disimpan otomatis; tidak ada fine-tuning otomatis.
- **RAG dan indeks codebase**: indeks manual lokal dengan FTS5, citation path/baris/hash, batas scan, pengecualian rahasia/binary/build-output/symlink, serta indeks AST Rust untuk simbol dan relasi `use`/`mod`. Hasil diambil otomatis ke konteks Naon dengan cap byte.
- **Planner dan tool calling**: `neon plan create` meminta Dyor menyusun JSON strict melalui model lokal; Rust memvalidasi semua step/tool lalu menyimpannya di SQLite. `neon plan run` menjalankan tool allowlist; write/Cargo meminta konfirmasi per tindakan; progress dapat dilanjutkan melalui checkpoint.
- **Verification Engine**: status `verified`/`failed`/`not_run` berasal dari exit code, timeout, truncation, atau readback file aktual. Kalimat model tidak dapat mengubah bukti. Plan yang selesai menawarkan pencatatan Experience Loop dengan persetujuan pengguna.
- **Context Manager**: anggaran input konservatif mengikuti `NEON_CONTEXT`, respons maksimum, batas item, dan enam giliran terbaru; satu backend/model dipakai sekuensial, bukan tujuh proses model paralel.
- **DuckDuckGo**: pencarian HTML atas perintah pengguna, dengan batas ukuran, hasil, dan timeout. Pencarian mengirim kueri ke DuckDuckGo; isi prompt/model tidak dikirim ke sana.
- **Lab file**: workspace terpisah dengan path relatif, penolakan `..`/symlink, limit file 1 MiB, preview/perubahan penuh, dan persetujuan sebelum menulis. Perintah test/check meminta konfirmasi dan dibatasi waktu/output.

## Batas jujur untuk ponsel 8 GB RAM / 128 GB storage

NEON hanya mengatur batas runtime perangkat lunak: konteks default 4.096 token, respons HTTP dibatasi 4 MiB, satu backend yang dimuat, maksimal empat thread secara default, dan maksimal delapan panggilan model per permintaan. **Ini bukan jaminan model muat**: RAM Android yang tersedia untuk aplikasi lebih kecil dari angka RAM fisik dan juga dipakai OS. Pilih file GGUF terkuantisasi yang sesuai; ukuran bobot, cache KV, context, dan implementasi backend semuanya memengaruhi pemakaian RAM. Uji dengan perangkat sendiri sebelum memakai model lebih besar. Storage 128 GB cukup untuk source/binary dan satu/lebih model yang cocok, tetapi bobot tidak dibundel di repositori.

Memori pengguna hanya retrieval lokal dan eksplisit, bukan pelatihan bobot model. Jadi NEON dapat memakai fakta/preferensi yang Anda setujui untuk dimasukkan, tetapi tidak otomatis mengubah kecerdasan dasar model.

## Instalasi Termux (Android ARM64)

1. Pasang toolchain Termux:

   ```sh
   pkg update
   pkg install rust clang cmake make git
   ```

2. Build `llama-server` lokal. Perintah contoh berikut memakai upstream `llama.cpp`; opsi build dapat berubah mengikuti versi/toolchain Android:

   ```sh
   cd "$HOME"
   git clone https://github.com/ggml-org/llama.cpp.git
   cmake -S llama.cpp -B llama.cpp/build -DGGML_OPENMP=ON -DLLAMA_CURL=OFF
   cmake --build llama.cpp/build --target llama-server -j2
   ```

3. Sediakan sendiri sebuah file model **GGUF** yang diizinkan lisensinya untuk penggunaan Anda. Tidak ada bobot model yang diunduh otomatis oleh NEON. Pilih model instruct/coding kecil terkuantisasi dan periksa lisensi serta kebutuhan memori dari sumber model.

4. Ambil source NEON ke ponsel, lalu pasang dependency/build. Anda bisa memakai skrip yang memeriksa bahwa ia berjalan di Termux:

   ```sh
   cd /path/ke/neon
   pkg install coreutils
   bash scripts/install-termux.sh
   ```

5. Atur konfigurasi (ganti lokasi dengan file yang benar-benar tersedia di perangkat):

   ```sh
   export NEON_LLAMA_SERVER="$HOME/llama.cpp/build/bin/llama-server"
   export NEON_MODEL_PATH="$HOME/models/model-instruct-q4.gguf"
   export NEON_CONTEXT=4096
   export NEON_THREADS=4
   # Opsional:
   export NEON_WORKSPACE="$HOME/neon-workspace"
   export NEON_DATA_DIR="$HOME/.local/share/neon"
   ```

6. Mulai chat interaktif dengan Naon (NEON akan menyalakan `llama-server` lokal sebagai child process):

   ```sh
   bash scripts/run-termux.sh
   # atau, setelah environment disetel:
   neon chat
   neon team
   neon resources
   neon ask "Bantu debug error Rust ini: ..."
   neon search "dokumentasi Rust ownership"
   neon knowledge index
   neon knowledge search "ownership"
   neon symbols search "borrow"
   neon plan create "Baca proyek, perbaiki bug, lalu jalankan tes"
   neon plan list
   neon plan run 1
   neonatom run examples/sum_squares.na
   ```

Build Android belum dijalankan pada perangkat Termux fisik di sesi ini; keberhasilan tes Linux tidak menjamin semua kombinasi Android/llama.cpp/model kompatibel. Jika build gagal pada ponsel, simpan pesan error lengkap untuk diagnosis.

## Perintah

```text
neon chat
neon team
neon resources                            # lihat byte budget, call cap, ukuran DB lokal
neon ask <pertanyaan>
neon search <kueri DuckDuckGo>
neon knowledge index [subdirektori-workspace]
neon knowledge search <kueri lokal>
neon symbols search <nama>
neon symbols dependencies <path-atau-modul>
neon plan create <tujuan>                 # Dyor menyusun draft memakai model lokal
neon plan list
neon plan show <id>
neon plan run <id>                        # approval diminta saat write/Cargo
neon plan resume <id>                     # maks. satu retry setelah gagal/interupsi
neon memory list
neon memory search <kueri>
neon memory add <kategori> <fakta>      # konfirmasi sebelum simpan
neon memory edit <id> <teks-baru>       # tampilkan payload lalu konfirmasi
neon memory forget <id|all>             # konfirmasi sebelum hapus
neon summary list|search <kueri>|edit <id> <teks>|delete <id>
neon skills list|search <kueri>|add <nama> <pemicu> <prosedur>|delete <id>
neon experience list|search <kueri>|add 'task|hasil|sebab|solusi|verifikasi'|delete <id>
neon workspace list
neon workspace read <path-relatif>
neon workspace write <path-relatif>     # masukkan isi, akhiri baris .end, tinjau diff, setujui
neon workspace test                     # konfirmasi; cargo test --workspace
neon workspace check                    # konfirmasi; cargo check --workspace
```

`neon knowledge index` mengindeks root `NEON_WORKSPACE`; jalankan lagi setelah perubahan besar/hapus file. Pencarian lokal tetap offline. `neon plan create` memerlukan backend+GGUF lokal; `plan run/list/show/resume` tidak memerlukan model. Setiap plan dibatasi 2–12 langkah, tool JSON 8 KiB, output tool 16 KiB, waktu command, dan maksimum satu retry per step. `neon resources` melaporkan byte budget heuristik dan batas lokal tetapi tidak mengukur RSS Android. Membaca/list/search dan kalkulasi NeonAtom tidak menjalankan shell; mutasi file dan Cargo membutuhkan konfirmasi eksplisit pada prompt Termux.

Saat `neon chat` dimulai, daftar seluruh nama tampil. Prompt terminal menggunakan `kamu>` dan jawaban akhir ditampilkan sebagai `Naon>`, dengan nama spesialis yang terlibat pada baris `[Tim: ...]`. Dalam chat tersedia `/help`, `/team`, `/summary`, `/skills`, `/experience`, `/search <kueri>`, `/memory`, `/forget all`, `/workspace`, `/exit`.

## Workspace Rust

```text
crates/neonatom/       parser/evaluator bahasa NeonAtom
crates/neonatom-cli/   CLI bahasa NeonAtom
crates/neon/           CLI/runtime agen lokal NEON
```

Periksa source dan tes:

```sh
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Catatan keamanan penting

Workspace adalah **pembatasan akses file di level aplikasi, bukan sandbox OS/container**. NEON tidak memberi model shell generik atau menjalankan kode arbitrer dari JSON model; dispatcher hanya menerima tool bertipe. Namun `cargo test/check` dan build script berjalan sebagai pengguna Termux Anda setelah konfirmasi; kode dalam workspace dapat memakai izin akun Android/Termux yang sama. Jalankan hanya source tepercaya. Pencarian DuckDuckGo hanya melalui permintaan langsung pengguna dan mengirimkan kueri web ke DuckDuckGo. Lihat [SECURITY.md](SECURITY.md).
