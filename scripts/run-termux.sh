#!/data/data/com.termux/files/usr/bin/bash
set -euo pipefail

expected_prefix="/data/data/com.termux/files/usr"
if [[ "${PREFIX:-}" != "$expected_prefix" ]]; then
  echo "Skrip ini hanya untuk Termux Android. PREFIX saat ini: ${PREFIX:-<kosong>}" >&2
  exit 2
fi

NEON_LLAMA_SERVER="${NEON_LLAMA_SERVER:-$HOME/llama.cpp/build/bin/llama-server}"
NEON_MODEL_PATH="${NEON_MODEL_PATH:-$HOME/models/model-instruct-q4.gguf}"
export NEON_LLAMA_SERVER NEON_MODEL_PATH
export NEON_WORKSPACE="${NEON_WORKSPACE:-$HOME/neon-workspace}"
export NEON_DATA_DIR="${NEON_DATA_DIR:-$HOME/.local/share/neon}"

if [[ ! -x "$NEON_LLAMA_SERVER" ]]; then
  echo "llama-server tidak ditemukan atau tidak executable: $NEON_LLAMA_SERVER" >&2
  echo 'Bangun llama-server sesuai README.md atau set NEON_LLAMA_SERVER ke path yang benar.' >&2
  exit 2
fi
if [[ ! -r "$NEON_MODEL_PATH" ]]; then
  echo "File GGUF tidak ditemukan/tidak dapat dibaca: $NEON_MODEL_PATH" >&2
  echo 'Set NEON_MODEL_PATH ke model GGUF lokal yang lisensinya Anda izinkan.' >&2
  exit 2
fi

mkdir -p "$NEON_WORKSPACE" "$NEON_DATA_DIR"
if ! command -v neon >/dev/null 2>&1; then
  echo 'Binary neon belum terpasang. Jalankan bash scripts/install-termux.sh dari checkout source.' >&2
  exit 2
fi
exec neon chat
