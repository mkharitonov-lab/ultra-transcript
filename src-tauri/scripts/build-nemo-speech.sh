#!/usr/bin/env bash
# Собирает NeMo-Speech.cpp (NVIDIA) — библиотеку для диаризации Nemotron 3 — в src-tauri/target/nemo-speech.
# Приложение подгружает её при выборе движка; без неё Nemotron 3 просто недоступен.
# Готовых сборок с Nemotron 3 пока нет (v0.1.0 вышла раньше модели), поэтому из исходников.
# Нужны: git, cmake ≥ 3.26, ninja (brew install cmake ninja), Xcode Command Line Tools.
set -euo pipefail

NEMO_COMMIT=97a15afa5caa9bce5baaa86c1184103877af4101        # 2026-09-24: Nemotron 3 Diarization
SENTENCEPIECE_COMMIT=17d7580d6407802f85855d2cc9190634e2c95624 # та же, что в scripts/build_sentencepiece_static.sh

# Минимальная macOS для библиотек (на других системах не действует).
MACOS_MIN=13.3

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK="$ROOT/target/nemo-speech-build"
PREFIX="$ROOT/target/nemo-speech"
mkdir -p "$WORK"

# В исходниках есть файлы Git LFS (для TTS), а git-lfs может быть не установлен — они не нужны.
git_() { git -c filter.lfs.process= -c filter.lfs.smudge=cat -c filter.lfs.clean=cat -c filter.lfs.required=false "$@"; }

fetch() { # fetch REPO_URL DIR COMMIT
    if [ ! -d "$2/.git" ]; then
        git_ init -q "$2"
        git_ -C "$2" remote add origin "$1"
    fi
    git_ -C "$2" fetch -q --depth 1 origin "$3"
    git_ -C "$2" checkout -q --force "$3"
}

fetch https://github.com/NVIDIA/NeMo-Speech.cpp.git "$WORK/src" "$NEMO_COMMIT"
git_ -C "$WORK/src" submodule update -q --init --depth 1 ggml

# SentencePiece статически: библиотека не должна зависеть от Homebrew на машине пользователя.
fetch https://github.com/google/sentencepiece.git "$WORK/sentencepiece" "$SENTENCEPIECE_COMMIT"
cmake -G Ninja -S "$WORK/sentencepiece" -B "$WORK/sentencepiece-build" -DCMAKE_BUILD_TYPE=Release \
    -DSPM_BUILD_TEST=OFF -DSPM_ENABLE_SHARED=OFF -DSPM_ENABLE_TCMALLOC=OFF \
    -DCMAKE_POSITION_INDEPENDENT_CODE=ON -DCMAKE_POLICY_VERSION_MINIMUM=3.5 \
    -DCMAKE_OSX_DEPLOYMENT_TARGET="$MACOS_MIN" >/dev/null
cmake --build "$WORK/sentencepiece-build" --target sentencepiece-static

case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) PRESET=metal-diar ;;
    *) PRESET=cpu-diar ;;
esac
cd "$WORK/src"
scripts/configure.sh "$PRESET" \
    -DSENTENCEPIECE_LIB="$WORK/sentencepiece-build/src/libsentencepiece.a" \
    -DSENTENCEPIECE_INCLUDE_DIR="$WORK/sentencepiece/src" \
    -DNEMO_SPEECH_BUILD_CLI=OFF -DNEMO_SPEECH_BUILD_MIC_CAPTURE=OFF \
    -DCMAKE_OSX_DEPLOYMENT_TARGET="$MACOS_MIN"
cmake --build --preset "$PRESET"
rm -rf "$PREFIX"
cmake --install "build/$PRESET" --prefix "$PREFIX" >/dev/null
echo "Готово: $PREFIX/lib"
