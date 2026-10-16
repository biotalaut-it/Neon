# Spesifikasi Fitur Intelijen NEON

## Tujuan

Menambahkan memori jangka panjang yang dapat dikendalikan pengguna, pencarian pengetahuan lokal berbasis teks, perencanaan tugas bertahap, pemanggilan alat aman, verifikasi berbasis bukti, indeks codebase, pengelolaan konteks/RAM, koordinasi tujuh agen, pustaka skill, dan penyimpanan pengalaman. Semua inferensi tetap memakai satu model GGUF lokal secara sekuensial; NEON tidak melatih bobot Qwen dan tidak memakai provider model cloud.

## Batas produk

- Platform utama Android Termux ARM64, Rust stable, Rust edition 2021, SQLite + FTS5 yang sudah dipakai proyek.
- Alur offline harus tetap berfungsi untuk semua data lokal setelah model/backend tersedia. DuckDuckGo hanya dipanggil saat pengguna meminta pencarian web.
- Tidak ada kode fitur produksi berupa mock, placeholder, skeleton, TODO, atau jalur pura-pura berhasil. Tes harus menguji perilaku nyata menggunakan database/file temporer dan proses/endpoint lokal yang dibatasi.
- Satu instance backend lokal digunakan bergantian. Tidak ada tujuh model paralel.
- Tindakan model melewati Rust dispatcher bertipe; model tidak memperoleh shell generik, akses jaringan sewenang-wenang, atau hak menyetujui perubahan sendiri.
- Memori percakapan mentah tidak disimpan otomatis. Ringkasan, pengalaman, skill personal, dan memori yang berasal dari percakapan hanya dipersistenkan setelah persetujuan eksplisit.
- File workspace yang diindeks dibaca saja. Pengecualian default: `.git`, `target`, `node_modules`, build/cache, file tersembunyi yang diketahui rahasia (`.env*`, credential/key/token, private key), binary, symlink, dan file di luar root.
- Batas awal indeks: maksimal 10.000 file dan total 64 MiB teks per workspace; file individual mengikuti batas 1 MiB yang sudah digunakan NEON; chunk maksimal 1.800 byte dengan rentang baris sumber. Semua batas dapat ditolak dengan pesan jelas, bukan diam-diam melewati batas.
- Pencarian semantik/embedding tidak masuk rilis pertama. Rilis pertama memakai FTS5, metadata, dan ranking lexical.
- Pengguna dapat melihat, mencari, mengekspor, dan menghapus data memori/indeks/task/experience/skill lokal.

## Definisi fitur dan perilaku

### 1. Smart Memory

Perluas SQLite/FTS5 yang ada untuk fakta/preferensi, keputusan, konteks proyek, ringkasan sesi, dan hasil terverifikasi. Ambil beberapa entri relevan dengan query pengguna dan batas byte/hasil. Sediakan proposal dan persetujuan sebelum menambah informasi yang disimpulkan dari chat; sediakan edit, hapus, dan purge. Ringkasan sesi dibuat dari buffer percakapan volatile yang terbatas, bukan arsip chat penuh.

### 2. RAG teks lokal

Indeks manual/on-demand untuk file workspace yang diizinkan. Pecah teks menjadi chunk yang menyimpan path, rentang baris, hash konten dan waktu modifikasi; FTS5 mengembalikan kutipan relevan. Perubahan/hapus file memperbarui indeks. Hasil selalu menyertakan sumber lokal dan tidak boleh dinyatakan sebagai bukti yang lebih luas dari chunk yang ditemukan.

### 3. Task Planner

Terima rencana berstruktur dengan langkah berurutan, tujuan, dependensi dan kriteria verifikasi. Validasi jumlah/ukuran langkah dan tool yang diizinkan; simpan rencana serta status setiap langkah secara atomik; lanjutkan hanya dari langkah yang belum terverifikasi setelah kegagalan/restart. Penyusunan rencana tidak berarti persetujuan untuk menulis file atau menjalankan proses.

### 4. Tool Calling

Model mengeluarkan permintaan tool JSON bertipe dan terbatas melalui plan Dyor. Dispatcher awal menyediakan operasi baca/cari workspace, pencarian simbol/dependensi, `git status`/`git diff` baca-saja, hitung/evaluasi NeonAtom, `cargo test`/`cargo check` yang meminta konfirmasi, dan pencarian DuckDuckGo eksplisit hanya untuk permintaan langsung pengguna. Perubahan file menghasilkan proposal replacement dan memerlukan persetujuan manusia sebelum diterapkan. Nama tool/field ekstra, path traversal, symlink, payload besar, argumen tak valid, dan shell arbitrary ditolak.

### 5. Verification Engine

Simpan bukti terstruktur untuk hasil nyata: command yang dijalankan, exit status, output terbatas, file readback/diff yang diperiksa, citation, dan tes yang lulus/gagal. Bedakan `verified`, `failed`, dan `not_run`; kategori evidence mencakup `command_exit`, `file_diff`, `index_citation`, `manual_approval`, dan `read_only_tool`. Jangan izinkan kalimat model mengubah status. Verifikasi tidak boleh menganggap kompilasi atau tes berhasil tanpa exit status sukses dari proses nyata atau file readback aktual.

### 6. Codebase Intelligence

Bangun indeks simbol dan dependensi lokal yang diturunkan dari source, selain FTS chunk. Rilis pertama mengurai deklarasi Rust dengan parser sintaks Rust dan menyimpan fungsi, struct, enum, trait, modul, `use`/mod dependency, nama file dan baris. File bahasa lain tetap bisa dicari secara teks dan tidak boleh diberi label sebagai indeks semantik Rust. Simbol stale dihapus saat source berubah/hilang.

### 7. Context & RAM Manager

Susun konteks dari instruksi wajib, pesan terbaru, task aktif, memori relevan, skill, pengalaman, dan RAG dalam urutan prioritas yang jelas. `ContextManager` memakai perkiraan konservatif byte dari context/response tokens, maksimum 12 item dan 6 giliran baru; ini bukan hitungan tokenizer tepat. Model client juga menolak prompt di atas byte estimate dan cap absolut. Potong kandidat berprioritas rendah saat byte budget penuh; hentikan siklus setelah batas retry/step/call.

### 8. Multi-Agent Coordinator

Gunakan nama peran tetap: Mike (Router), Dyor (Decomposer), Susan (Programmer), Fani (Maintenance), Deby (Debugger), Jordan (Summarizer), Naon (Leader). Pilih role minimum untuk tugas; sederhana dapat langsung ke Leader atau satu spesialis, kompleks memakai Dyor, dan kebutuhan alat lewat dispatcher. Model sama, satu pemanggilan pada satu waktu, batas `NEON_AGENT_CALLS` tetap berlaku. Jawaban mencantumkan peran yang benar-benar dijalankan.

### 9. Skill Library

Simpan prosedur berulang sebagai teks/checklist terstruktur, bukan kode yang dieksekusi otomatis. Sertakan skill bawaan nyata dan teruji untuk debugging, tes Rust, analisis repositori, dan review diff; cari skill via FTS5. Skill buatan pengguna harus lolos validasi ukuran dan persetujuan, dapat diedit/dinonaktifkan/dihapus. Tidak ada instalasi kode/skrip dari model.

### 10. Experience Loop

Simpan pengalaman sebagai tugas ringkas, hasil, penyebab kegagalan, solusi, dan bukti verifikasi. Pengalaman yang diusulkan dari sebuah run memerlukan persetujuan sebelum persistensi. Sebelum mengulang tugas, ambil pengalaman relevan dan tunjukkan sumbernya; pengalaman tidak menjadi fakta tepercaya bila bukti lama bertentangan dengan kondisi codebase saat ini.

## Rancangan penyimpanan dan antarmuka

- Gunakan migrasi SQLite versioned yang idempoten; jangan menghapus/mengubah data schema memori yang telah ada.
- Gunakan tabel FTS terpisah untuk `memories`, `session_summaries`, `knowledge_chunks`, `experiences`, dan `skills`, dengan trigger/sinkronisasi delete/update yang diuji.
- File lokal, task plan, step, tool request, tool result, dan verification evidence memiliki tipe Rust eksplisit dan batas ukuran.
- Perintah CLI: `neon resources` menampilkan byte/call/storage caps (bukan RSS OS); `neon memory`, `neon summary`, `neon knowledge index|search`, `neon symbols search|dependencies`, `neon plan create|list|show|run|resume`, `neon skills`, `neon experience`. Chat slash commands tersedia untuk inspeksi memori/skill/pengalaman serta approval ringkasan.
- Setiap command dapat digunakan tanpa akses internet kecuali `neon search`/tool DDG yang diminta.

## Kriteria penerimaan

1. `cargo fmt --all -- --check`, `cargo test --workspace`, dan `cargo clippy --workspace --all-targets -- -D warnings` lulus.
2. Migrasi mempertahankan memori lama; pencarian FTS, update, delete, purge, dan reindex konsisten.
3. Pengindeksan menolak traversal/symlink/rahasia/binary/oversize dan menghapus hit/simbol stale.
4. Setiap potongan RAG/simbol menunjukkan path dan lokasi; konteks yang diteruskan dibatasi dan terpotong secara deterministik.
5. Tool request malformed/unknown/oversize dan argumen berbahaya ditolak; dispatcher tidak menjalankan shell arbitrary.
6. Tool mutasi/Cargo tidak berjalan tanpa approval eksplisit; hasil verification mencerminkan exit code nyata.
7. Rencana tersimpan dapat di-resume setelah step gagal/restart tanpa mengulang step yang telah terverifikasi.
8. Ringkasan, experience, dan skill hasil model tidak tersimpan tanpa persetujuan; purge menghapus record beserta indeks FTS.
9. Batas panggilan model dan request dibuktikan dengan tes; setiap backend call menggunakan satu LocalModel yang sama dan terjadi sekuensial.
10. Uji beban lokal menggunakan database/file berisi data nyata terkontrol memverifikasi cap 10.000 file, 64 MiB indeks, 1 MiB/file, byte konteks dan output tanpa mengklaim performa Termux sebelum diuji di perangkat.

## Pembagian subproyek

- **A — Memory, Skills, Experience:** item 1, 9, 10.
- **B — Knowledge RAG dan Codebase Index:** item 2, 6.
- **C — Task Planner, Tools, Verification:** item 3, 4, 5.
- **D — Context/RAM dan Coordinator:** item 7, 8; integrasi dengan A–C.

Spesifikasi ini tidak menjanjikan kemampuan ASI, pembelajaran bobot otomatis, atau penyelesaian semua masalah. Kualitas tetap bergantung pada model lokal yang digunakan.
