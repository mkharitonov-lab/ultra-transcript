# Ultra Transcript

Локальная расшифровка аудио и видео с фокусом на русский язык — в духе MacWhisper, плюс:
словарь терминов, справочник «кто есть кто», распознавание голосов, автопополнение справочников,
очищенная расшифровка по спикерам, протокол договорённостей по шаблону .docx и фоновая обработка папок.

Всё работает на компьютере пользователя. LLM подключается через OpenAI-совместимый API:
локально (Ollama, LM Studio, llama-server) или по любому custom endpoint.

## Архитектура

```
UI (Svelte, src/) ──invoke/events──► Ядро (Rust, src-tauri/src/)
                                       service.rs   очередь задач, фоновый обработчик, опрос папок
                                       pipeline.rs  Ingest → ASR → Diarize → Identify → Correct → Enrich → Export
                                       speech.rs    sherpa-onnx: GigaAM v3, Silero VAD, pyannote, WeSpeaker
                                       llm.rs       OpenAI-совместимый чат (локально или API)
                                       docx.rs      движок .docx-шаблонов
                                       store.rs     SQLite + голосовые отпечатки (AES-256-GCM)
                                       media.rs     ffmpeg: любые форматы → 16 кГц; архив в Opus
```

- Каноническая модель — `transcript.json` (`transcript.rs`). Очищенный текст, .docx и протокол — её представления.
- Шаблоны .docx редактируются в Word. Метки: `{{поле}}`; `{{список}}` в пункте списка повторяется для каждого
  элемента; `{{таблица.колонка}}` в строке таблицы повторяет строку. **Набор меток шаблона = схема для LLM**:
  добавили в шаблон `{{риски}}` — LLM заполнит и это поле.
- Модели (~205 МБ) скачиваются внутри приложения при первом запуске.

## Запуск

```bash
pnpm install
pnpm tauri dev                      # приложение в режиме разработки
pnpm tauri build --bundles app      # сборка .app
```

Нужен ffmpeg (`brew install ffmpeg`); в поставку его нужно будет положить рядом с бинарником.

Проверка конвейера без интерфейса:

```bash
cd src-tauri && cargo run --release --bin ut -- запись.m4a [--protocol]
```

Данные: `~/Library/Application Support/app.ultratranscript/` (библиотека, модели, шаблоны).

## Минимальные требования

| | macOS | Windows (предварительно) |
|---|---|---|
| Расшифровка и голоса | Apple Silicon M1 Pro, 16 ГБ | x64, 4 ядра с AVX2, 8 ГБ ОЗУ |
| + локальная LLM 7–8B | те же 16 ГБ | 16 ГБ ОЗУ + NVIDIA 8 ГБ VRAM, или LLM по API |
