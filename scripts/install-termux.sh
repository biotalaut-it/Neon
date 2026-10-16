#!/data/data/com.termux/files/usr/bin/bash
set -euo pipefail

expected_prefix="/data/data/com.termux/files/usr"
if [[ "${PREFIX:-}" != "$expected_prefix" ]]; then
  echo "Skrip ini hanya untuk Termux Android. PREFIX saat ini: ${PREFIX:-<kosong>}" >&2
  exit 2
fi

missing=()
for command_name in cargo clang cmake make install; do
  if ! command -v "$command_name" >/dev/null 2>&1; then
    missing+=("$command_name")
  fi
done
if ((${#missing[@]})); then
  printf 'Tool belum tersedia: %s\n' "${missing[*]}" >&2
  echo 'Jalankan: pkg update && pkg install rust clang cmake make coreutils' >&2
  exit 2
fi

project_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$project_dir"
echo "Membangun NEON untuk Android/Termux di: $project_dir"
cargo build --release --workspace
install -Dm755 target/release/neon "$PREFIX/bin/neon"
install -Dm755 target/release/neonatom "$PREFIX/bin/neonatom"
printf '\nTerpasang:\n  %s\n  %s\n' "$PREFIX/bin/neon" "$PREFIX/bin/neonatom"
echo 'Berikutnya: siapkan llama-server dan file GGUF, lalu jalankan bash scripts/run-termux.sh.'
