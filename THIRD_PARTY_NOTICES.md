# Third-party notices · Сведения о сторонних компонентах

Ultra Transcript («Ультра Транскрибатор») is licensed under the PolyForm Noncommercial License 1.0.0
(see `LICENSE`). That license covers the application's own code only. Everything listed below belongs
to its authors and is used under its own license.

Лицензия приложения (PolyForm Noncommercial 1.0.0, файл `LICENSE`) относится только к его собственному
коду. Всё перечисленное ниже принадлежит своим авторам и используется по их лицензиям.

## 1. Models are not part of the application · Модели не входят в состав приложения

The application does not contain, bundle or redistribute any speech or language models. A model is
downloaded only when the user asks for it, directly from its publisher's servers (GitHub releases of the
sherpa-onnx project, Hugging Face), and is stored in the user's data folder. Each model is licensed to the
user by its publisher under the model's own license; by downloading a model the user accepts that license
and is responsible for complying with it. The application's license does not apply to the models, and the
models' licenses do not apply to the application.

Приложение не содержит и не распространяет модели распознавания речи и языковые модели. Модель
скачивается только по запросу пользователя — напрямую с серверов её разработчика (выпуски проекта
sherpa-onnx на GitHub, Hugging Face) — и хранится в папке данных пользователя. Право использовать модель
пользователь получает от её разработчика по лицензии самой модели: скачивая модель, пользователь
принимает эту лицензию и сам отвечает за её соблюдение. Лицензия приложения на модели не
распространяется, а лицензии моделей не распространяются на приложение.

| Model | Publisher | License | Source |
|---|---|---|---|
| GigaAM v3 | Sber (SaluteDevices) | MIT | github.com/k2-fsa/sherpa-onnx (ONNX export), huggingface.co/ai-sage/GigaAM-v3 |
| Whisper large-v3-turbo | OpenAI | MIT | github.com/k2-fsa/sherpa-onnx (ONNX export), github.com/openai/whisper |
| Parakeet TDT 0.6B v3 | NVIDIA | CC BY 4.0 | github.com/k2-fsa/sherpa-onnx (ONNX export), huggingface.co/nvidia/parakeet-tdt-0.6b-v3 |
| GLM-ASR-Nano-2512 | Zhipu AI | Apache 2.0 | huggingface.co/zai-org/GLM-ASR-Nano-2512, huggingface.co/concedo/GLM-ASR-Nano-2512-GGUF (GGUF) |
| Silero VAD | Silero Team | MIT | github.com/snakers4/silero-vad |
| pyannote segmentation 3.0 | pyannote (Hervé Bredin) | MIT | huggingface.co/pyannote/segmentation-3.0 |
| WeSpeaker ResNet34-LM (VoxCeleb) | WeSpeaker | CC BY 4.0 | huggingface.co/Wespeaker/wespeaker-voxceleb-resnet34-LM |
| pyannote community-1 (PLDA, embeddings) | pyannote, ONNX export by speakrs | CC BY 4.0 | huggingface.co/pyannote-community/speaker-diarization-community-1, huggingface.co/avencera/speakrs-models |
| Nemotron 3 Diarization | NVIDIA | OpenMDW License Agreement 1.1 | huggingface.co/nvidia/Nemotron-3-Diarization |
| DPDFNet | Ceva | Apache 2.0 | huggingface.co/Ceva-IP/DPDFNet |
| GigaChat 3.1 Lightning | Sber | MIT | huggingface.co/ai-sage/GigaChat3.1-10B-A1.8B-GGUF |
| T-lite 2.1 | T-Bank | Apache 2.0 | huggingface.co/t-tech/T-lite-it-2.1-GGUF |
| YandexGPT-5 Lite 8B | Yandex | YandexGPT-5 Lite license: free for research and non-commercial use; commercial use is limited (10 million output tokens per month, then an agreement with Yandex is required) | huggingface.co/yandex/YandexGPT-5-Lite-8B-instruct-GGUF |
| Qwen3 4B Instruct | Alibaba Cloud, GGUF by Unsloth | Apache 2.0 | huggingface.co/unsloth/Qwen3-4B-Instruct-2507-GGUF |

Models under CC BY 4.0 require attribution: the table above names their authors.
The licenses shown are those stated by the publishers on 28 September 2026; check the source page before
any use that the application's own license or a model's license may restrict.

## 2. Programs that are not part of the application · Программы, которые не входят в состав приложения

**FFmpeg** (ffmpeg.org, LGPL 2.1+ / GPL 2+ depending on the build). The application does not include
FFmpeg. It runs the `ffmpeg` program installed on the user's computer as a separate process to read
audio and video files.

**NeMo-Speech.cpp** (github.com/NVIDIA/NeMo-Speech.cpp, Apache 2.0, with ggml — MIT and SentencePiece —
Apache 2.0). Optional and not included: needed only for Nemotron 3 and built by the user from source with
`src-tauri/scripts/build-nemo-speech.sh`. Anyone who distributes a build that includes this library must
ship its license files with it.

## 3. Libraries built into the application · Библиотеки в составе приложения

| Component | License |
|---|---|
| Tauri, wry, tao and Tauri plugins | MIT or Apache 2.0 |
| Svelte, SvelteKit | MIT |
| cpal (microphone capture) | Apache 2.0 |
| sherpa-onnx | Apache 2.0 |
| ONNX Runtime (inside sherpa-onnx) | MIT |
| llama.cpp (through llama-cpp-2) | MIT |
| SQLite (through rusqlite) | Public domain / MIT |
| minijinja | Apache 2.0 |
| reqwest, rustls, ring | MIT, Apache 2.0, ISC |
| calamine, rust_xlsxwriter, csv, zip, tar, bzip2 | MIT or Apache 2.0 (bzip2 library: bzip2 license) |
| cssparser, selectors, dtoa-short, option-ext | MPL 2.0 (used unmodified; source: crates.io) |
| Unicode data (ICU4X) | Unicode License v3 |
| Mozilla CA certificate list (webpki-roots) | CDLA Permissive 2.0 |
| Other Rust crates (about 600) | MIT, Apache 2.0, BSD, ISC, Zlib, Unlicense, CC0 |

Interface icons are drawn after the Feather (MIT) and Lucide (ISC) icon sets.

The full list of Rust crates with their licenses can be produced with
`cargo metadata --format-version 1` in `src-tauri/`; JavaScript packages are listed in `pnpm-lock.yaml`.
None of the built-in components is licensed under the GPL or AGPL.
