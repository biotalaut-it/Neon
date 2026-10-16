# Keamanan dan batas isolasi NEON

NEON sengaja tidak memberi model akses alat shell. Model menerima prompt lokal dan mengembalikan teks/paket JSON; pencarian DDG merupakan perintah eksplisit. Backend model hanya dibuka pada loopback `127.0.0.1` dan aplikasi tidak memakai provider model cloud.

## Batas keamanan yang diterapkan

- Workspace menerima path relatif, menolak traversal `..`, menolak symlink, membatasi file ke 1 MiB, menampilkan diff dan meminta persetujuan sebelum mengganti file.
- Output file model tidak otomatis diterapkan atau dijalankan. Pengguna memasukkan perubahan melalui subcommand workspace.
- Eksekusi hanya dua perintah yang di-allowlist, `cargo test --workspace` dan `cargo check --workspace`, dengan konfirmasi dan batas waktu 120 detik.
- Request model dibatasi waktu dan ukuran response; agent calls, context prompt, output dan ukuran paket dibatasi.
- Memori SQLite hanya masuk setelah persetujuan pengguna dan memiliki operasi daftar/cari/edit/hapus.
- Hasil pencarian DuckDuckGo difilter sebagai URL HTTP/HTTPS; respons HTML dan jumlah hasil dibatasi.

## Bukan sandbox OS

Workspace NEON **tidak mengisolasi proses** memakai container, namespace, seccomp, SELinux profile, atau permission Android terpisah. `cargo test` dan `cargo check` dapat menjalankan build script/proc macro dari dependency dengan hak akun Termux yang sama dan akses jaringan/file yang sama. Konfirmasi CLI bukan jaminan bahwa source aman. Jangan jalankan build pada source yang tidak dipercaya. Isolasi kuat memerlukan lingkungan OS terpisah yang belum disediakan oleh kode ini.

## Privasi dan data

- Database berada di `NEON_DATA_DIR` (default `$HOME/.local/share/neon`) dalam direktori permission Unix `0700`; isi SQLite tidak dienkripsi.
- Penghapusan menghapus record secara logis melalui SQLite; ini **bukan secure erase forensik** dari media/WAL/backup.
- Tidak ada percakapan yang disimpan otomatis dan tidak ada fine-tuning otomatis. Hanya item yang disetujui pengguna yang disimpan sebagai memori.
- Perintah `/search`/`neon search` mengirim teks kueri ke DuckDuckGo melalui HTTPS. Jangan masukkan rahasia pada kueri. Prompt lokal tidak ikut dikirim oleh NEON.
- Model GGUF dan `llama-server` adalah komponen pihak ketiga; lisensi, provenance, dan keamanan model/backend perlu diperiksa oleh pengguna.

Laporkan dugaan kerentanan tanpa menaruh data pribadi atau token di issue publik.
