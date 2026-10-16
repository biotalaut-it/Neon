# NEON — Spesifikasi Produk dan Arsitektur

## Tujuan dan batas klaim
NEON adalah agen CLI Rust untuk Termux Android yang menjalankan model GGUF open-weight secara lokal, menyediakan memori personal opt-in, pencarian DuckDuckGo saat diminta, serta lab coding terbatas. NEON bukan Artificial Superintelligence. “Belajar dari keseharian” berarti menyimpan dan mengambil memori yang disetujui pengguna, bukan melatih ulang bobot model. Sebelum agen NEON dibuat, bahasa DSL fondasi **NeonAtom** harus diimplementasikan dan lulus tes sesuai `docs/NEONATOM-SPEC.md` dan `docs/superpowers/plans/2026-10-16-neonatom-language.md`.

## Strategi model lokal dan kapasitas
Inference lokal memakai `llama.cpp` dan GGUF yang dipilih pengguna; tidak ada provider cloud/API model. Pengurangan bobot dilakukan dengan memilih model dasar yang lebih kecil dan kuantisasi bobot yang didukung. Kuantisasi mengurangi ukuran tetapi dapat menurunkan akurasi. Ukuran file bukan total RAM: inference juga membutuhkan memori runtime, cache konteks, dan memori Android. Dokumentasi llama.cpp memberi contoh model 8B tertentu berukuran 4.58 GiB pada Q4_K_M serta menjelaskan trade-off kuantisasi [1]. Angka itu contoh, bukan jaminan untuk model lain atau kecukupan pada HP.

Target perangkat RAM fisik 8 GB/storage 128 GB. Benchmark model open-weight multilingual/coding yang relatif kecil dalam format GGUF terkuantisasi pada perangkat pengguna. Utamakan varian 1.5B–4B sebagai kandidat awal, bukan janji performa/kompatibilitas. Model lebih besar hanya dicoba bila RAM bebas setelah sistem dan context mencukupi. Jangan auto-download, dan periksa lisensi, ukuran, checksum serta metadata. Model card Qwen2.5-7B-Instruct melaporkan 7.61B parameter dan kemampuan coding/matematika serta dukungan multilingual 29+ bahasa; informasi itu tidak membuktikan kualitas Bahasa Indonesia secara spesifik [2].

Bahasa ringkas tidak mengecilkan bobot model. NeonAtom meringkas workflow/pola data, bukan parameter, embedding, tokenizer, atau RAM inference. Prompt tidak diubah menjadi notasi matematika tanpa uji kualitas. Tidak ada rumus umum yang menjamin 1 MB menampung model 1 TB.

## Bahasa dan kualitas
Bahasa percakapan NEON adalah Bahasa Indonesia. Sistem prompt dan memori retrieval bisa membantu konsistensi, tetapi tidak menjamin kefasihan atau kebenaran. Model dievaluasi dengan set uji lokal untuk percakapan Indonesia, penjelasan teknis, terjemahan yang mempertahankan identifier, debugging, implementasi fitur, penjelasan error, dan tugas lintas file. Ukur akurasi, kepatuhan, halusinasi, latensi, RAM dan hasil kode yang lolos tes. NEON tidak menjanjikan dapat menyelesaikan semua masalah; ia harus mengakui ketidakpastian dan meminta informasi atau menjalankan tes bila perlu.

## Agen spesialis dan leader
NEON menyediakan tujuh peran agen logis dengan nama tetap: **Susan (Programmer)** menghasilkan/perbaiki kode lintas bahasa pemrograman; **Fani (Maintenance)** menangani pemeliharaan, dependensi, refactor, tes, kompatibilitas dan keamanan; **Deby (Debugger/Pemecah masalah)** menganalisis gejala, error dan reproduksi; **Dyor (Decomposer)** memecah permintaan menjadi submasalah terurut; **Mike (Router)** mengategorikan pekerjaan dan memilih peran yang sesuai; **Jordan (Summarizer)** merangkum konteks, temuan, alternatif dan solusi; **Naon (Leader)** mengendalikan alur dan menyampaikan jawaban akhir yang jelas dalam Bahasa Indonesia.

Untuk muat di RAM 8 GB, tujuh peran adalah spesialisasi prompt dan modul routing yang memakai **satu model lokal bersama secara bergantian/sekuensial**, bukan tujuh model neural berbobot yang menetap di RAM. Router adalah peran AI lokal yang mengklasifikasikan tugas dan memilih spesialis; hasil klasifikasi divalidasi terhadap enum kategori, lalu tugas ambigu gagal tertutup. Untuk tugas kompleks, Decomposer membuat unit kerja beserta label kategori; supervisor memvalidasi setiap label terhadap enum Router dan mengirim unit ke spesialis yang sesuai. Satu atau beberapa peran spesialis menghasilkan temuan; Summarizer menyatukan; Leader memeriksa konsistensi dan menjawab manusia. Setiap run memiliki batas panggilan, context dan langkah. Leader tidak menyembunyikan konflik/ketidakpastian. Peran boleh dilewati jika tugas sederhana dan budget kecil.

Komunikasi internal menggunakan kontrak data terstruktur yang kompak dan tervalidasi (`AgentPacket`, JSON bertipe/serde atau enum Rust), bukan machine code, assembly, atau bahasa manusia bebas tanpa skema. Paket memuat role, tujuan, konteks yang dibutuhkan, batasan, hasil, bukti, status dan confidence; isi ringkas dalam Bahasa Indonesia atau kode/istilah teknis yang diperlukan. NeonAtom menyatakan urutan workflow deterministik. Tidak ada format rahasia yang diklaim lebih pintar; schema invalid dibatasi retry lalu gagal aman. Leader menerjemahkan hasil teknis ke Bahasa Indonesia alami tanpa mengubah identifier, kode, atau bukti.

## Runtime, internet dan memori
Backend inference `llama-server` hanya bind loopback, dikelola NEON, dan ditutup saat keluar. Context, thread dan output dibatasi. Tidak ada auto-download model. Inferensi/memori dapat berjalan offline setelah model/backend tersedia. DDG dipanggil hanya atas permintaan; query tidak memuat percakapan/memori kecuali pengguna menyetujui eksplisit.

Memori personal SQLite/FTS5: model dapat mengusulkan tetapi pengguna menyetujui sebelum penyimpanan; pengguna dapat lihat, cari, ubah, hapus item atau hapus semua. Jangan menyimpan otomatis seluruh percakapan atau data sensitif. Memori meningkatkan retrieval personal, bukan melatih ulang bobot.

## Lab coding dan keamanan
Lab membaca/menulis hanya dalam workspace, menampilkan diff dan meminta persetujuan sebelum perubahan. Eksekusi hanya executable/argumen allowlist tanpa shell arbitrer. Termux tanpa root bukan sandbox OS bagi kode tak tepercaya; workspace/allowlist/timeout adalah mitigasi aplikasi. Backend, model dan program punya hak akun Termux. Pindahkan kode tidak tepercaya ke VM/sandbox OS terpisah.

Data lokal bukan otomatis terenkripsi dan backup Android dapat mencakupnya. Sediakan hapus/ekspor lokal, permission ketat dan penjelasan privasi. NeonAtom tidak menyimpan output besar secara ajaib dan tidak mengecilkan bobot model.

## Kriteria penerimaan
`cargo fmt --check`, Clippy dan `cargo test --workspace` lulus. Tes mencakup endpoint loopback, offline, memory approval/purge, DDG tanpa memory otomatis, batas NeonAtom, containment workspace, tanpa auto-run kode model, routing kategori, masing-masing tujuh peran, serial reuse model, schema packet rusak, budget agen habis, konflik spesialis, serta jawaban Leader dalam Bahasa Indonesia dengan identifier teknis terjaga. Uji Termux ARM64 mengukur build, model/quant, RAM, context, latensi dan kualitas set uji Indonesia/koding sebelum klaim performa.

## Referensi
[1]: https://github.com/ggml-org/llama.cpp/blob/master/tools/quantize/README.md "llama.cpp quantization documentation"
[2]: https://huggingface.co/Qwen/Qwen2.5-7B-Instruct "Qwen2.5-7B-Instruct model card"
