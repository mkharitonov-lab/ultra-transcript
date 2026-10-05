#!/usr/bin/env bash
# FFmpeg для поставки вместе с приложением (Mac): только звук, лицензия LGPL, без внешних
# зависимостей — libopus собирается из исходников и входит в файл статически.
# Результат: src-tauri/binaries/ffmpeg-<target-triple> — Tauri кладёт его рядом с исполняемым
# файлом приложения (bundle.externalBin), там его и ищет media::ffmpeg.
#
# Нужно: Xcode Command Line Tools. ≈3 минуты.
set -euo pipefail

FFMPEG_VERSION=7.1.1
OPUS_VERSION=1.5.2

here="$(cd "$(dirname "$0")/.." && pwd)"
triple="$(rustc -vV | sed -n 's/^host: //p')"
out="$here/binaries/ffmpeg-$triple"
# Папка сборки; в CI — вне target, чтобы кэш Rust не обходил исходники opus и ffmpeg.
work="${FFMPEG_BUILD_DIR:-$here/target/ffmpeg-build}"
prefix="$work/prefix"
jobs="$(sysctl -n hw.ncpu 2>/dev/null || nproc)"
export MACOSX_DEPLOYMENT_TARGET=14.2

mkdir -p "$work" "$here/binaries"
cd "$work"

if [ ! -f "$prefix/lib/libopus.a" ]; then
  curl -fsSL "https://downloads.xiph.org/releases/opus/opus-$OPUS_VERSION.tar.gz" | tar xz
  (cd "opus-$OPUS_VERSION" &&
    ./configure --prefix="$prefix" --disable-shared --enable-static --disable-doc --disable-extra-programs &&
    make -j"$jobs" && make install)
fi

[ -d "ffmpeg-$FFMPEG_VERSION" ] || curl -fsSL "https://ffmpeg.org/releases/ffmpeg-$FFMPEG_VERSION.tar.xz" | tar xJ
cd "ffmpeg-$FFMPEG_VERSION"
# Читать — любые форматы со звуком (аудио и видео-контейнеры, все встроенные аудиокодеки);
# писать — Opus в Ogg (архив записи) и сырые отсчёты f32 (для распознавания).
PKG_CONFIG_PATH="$prefix/lib/pkgconfig" ./configure \
  --prefix="$work/ffmpeg-out" \
  --pkg-config-flags=--static \
  --extra-cflags="-I$prefix/include" --extra-ldflags="-L$prefix/lib" \
  --disable-everything --disable-autodetect --disable-network --disable-doc \
  --disable-ffplay --disable-ffprobe --disable-avdevice --disable-swscale --disable-postproc \
  --enable-small --enable-libopus \
  --enable-protocol=file,pipe,fd \
  --enable-demuxers --enable-parsers \
  --enable-decoders \
  --enable-encoder=libopus,pcm_f32le,pcm_s16le \
  --enable-muxer=ogg,opus,pcm_f32le,wav \
  --enable-filter=aresample,aformat,anull,atrim,amix,pan
make -j"$jobs"
cp ffmpeg "$out"
strip "$out" 2>/dev/null || true
"$out" -hide_banner -encoders 2>/dev/null | grep -q libopus
echo "готово: $out ($(du -h "$out" | cut -f1))"
