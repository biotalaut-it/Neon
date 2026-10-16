# NeonAtom — Spesifikasi Bahasa

## Tujuan
NeonAtom adalah bahasa pemrograman kecil berbasis Rust untuk mendeskripsikan pola dan transformasi data dengan aturan ringkas, lalu mengevaluasinya secara bertahap. Prinsip “1 MB ≈ 1 TB” ditafsirkan sebagai sasaran rasio representasi hanya untuk struktur yang sangat berulang atau dapat dihasilkan oleh rumus, bukan kompresi universal, RAM virtual, atau storage tambahan. NeonAtom tidak dapat membuat 1 MB kapasitas fisik menjadi 1 TB. Sintaks ringkas hanya mengurangi ukuran program/workflow; ia tidak memampatkan bobot model AI, mengecilkan RAM inference, atau membuat model lebih pintar. Pengurangan ukuran model memerlukan bobot model lebih kecil atau kuantisasi, dengan trade-off kualitas yang harus diukur.

## Target v0.1
- Interpreter CLI native untuk Termux ARM64 dan Linux, implementasi inti Rust dengan dependency minimal.
- Target ukuran executable stripped < 1 MiB adalah sasaran pengukuran, bukan janji sebelum build; catat toolchain, linker, dan ukuran hasil aktual. Bila Rust standard library membuat target gagal, laporkan angka terukur dan jangan mengganti toolchain/behavior tanpa dokumentasi.
- Source file UTF-8; parser dan evaluator deterministik, tanpa eval host-language.
- Ekspresi integer 64-bit bertanda, boolean, string terbatas, let-binding, fungsi murni terbatas, rentang lazy, generator aritmetika, dan transformasi stream.
- CLI dapat menghitung ringkasan ukuran/count/checksum atau menulis output secara chunked dengan batas jumlah byte/elemen dan konfirmasi. Tanpa batas eksplisit, program tidak boleh mengmaterialisasi stream tak terbatas.
- Batas default: fuel 1.000.000 langkah, nesting 128, input source 1 MiB, output materialized 16 MiB, waktu eksekusi 2 detik yang dapat dikonfigurasi; semua overflow/error dilaporkan.
- Library API untuk integrasi NEON: `parse(source: &str) -> Result<Program, Diagnostic>`, `eval(program: &Program, limits: Limits) -> Result<Value, RuntimeError>`, `Stream::take(n)` dan iterator lazy terbatas.

## Sintaks inti yang direncanakan
`let` binding, literal dan operasi aritmetika/logika, lambda murni satu-argumen `x => expr`, `range(start, end)`, `map(stream, lambda)`, `filter(stream, lambda)`, `take(stream, n)`, `sum(stream)`, `count(stream)`, `checksum(stream)`, `print(value)`. Definisi fungsi bernama tidak termasuk v0.1. Jangan menambahkan I/O sistem, import jaringan, reflection, eval dinamis, atau akses shell.

Contoh representasi (bukan bukti kompresi umum): `range(0, 1000000000000)` mewakili urutan bilangan sebagai aturan ringkas; iterasi satu triliun elemen tetap memerlukan waktu, sedangkan meminta `take(..., 10)` hanya menghasilkan sepuluh elemen. Data acak 1 TB tetap tidak dapat direpresentasikan tanpa menyimpan informasi yang cukup untuk membangunnya kembali.

## Keamanan dan determinisme
Evaluator tidak menjalankan proses host, tidak mengakses filesystem/network, tidak memakai unsafe Rust untuk evaluator, dan mengaplikasikan batas fuel/nesting/ukuran sebelum operasi mahal. Built-in aritmetika memakai checked operations; pembagian nol dan overflow menjadi diagnostic. Interpretasi lazy harus tetap menghabiskan fuel dan menaati limit output.

## Kriteria penerimaan
- Parser, evaluator, stream laziness, fuel, output limit, overflow, pembagian nol, nesting limit, dan kesalahan sintaks mempunyai unit/integration test deterministik.
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, dan `cargo build --release` lulus.
- CLI demonstrasi urutan besar dengan `take` menghitung nilai terbatas tanpa materialisasi seluruh stream; benchmark melaporkan operasi dan waktu, tidak mengklaim materialisasi 1 TB.
- Ukuran executable release-stripped dilaporkan dengan metode reproduktif; target < 1 MiB ditandai tercapai/tidak tercapai sesuai hasil, tidak disamarkan. Build Linux x86_64 yang diuji menghasilkan 357,536 byte (`stat -c %s target/release/neonatom`); ukuran Android ARM64 belum diukur.
- NeonAtom tidak dipakai untuk menjalankan kode agen tak tepercaya dengan hak host. Integrasi NEON hanya untuk evaluasi DSL ber-batas.
- Jangan memakai bahasa matematika padat untuk mengganti prompt Bahasa Indonesia kecuali evaluasi membuktikan kualitasnya tetap baik; model harus diuji terpisah untuk kefasihan Bahasa Indonesia dan tugas pemrograman.
