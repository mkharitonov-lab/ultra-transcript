/**
 * Интерфейс без ядра: `pnpm dev` в обычном браузере. Ядро заменяют выдуманные данные —
 * так можно работать над окном, не запуская расшифровку. В сборку приложения этот файл не попадает.
 */

import { emit } from "@tauri-apps/api/event";
import license from "../../../LICENSE?raw";
import notices from "../../../THIRD_PARTY_NOTICES.md?raw";
import { mockConvertFileSrc, mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import type {
  AppInfo, Folder, JobEvent, JobKind, LiveEvent, LiveState, Model, Person, Recording, Rule, Settings, Term, Transcript,
} from "../api";

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const stamp = (daysAgo: number, time = "10:30") => {
  const d = new Date(Date.now() - daysAgo * 86_400_000);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")} ${time}`;
};

let english = false;
const l = (ru: string, en: string) => (english ? en : ru);

const settings: Settings = {
  asr_model: "gigaam", speech_language: "auto", denoise: true, level_volume: true,
  llm_enabled: !new URLSearchParams(location.search).has("fresh"), llm_provider: "builtin", llm_local_model: "gigachat-lightning",
  llm_base_url: "http://localhost:11434/v1", llm_model: "qwen3:8b", llm_api_key: "",
  diar_model: "pyannote3", cluster_threshold: 0.4, voice_threshold: 0.55, archive_kbps: 24,
  auto_accept_suggestions: false, auto_title: true, transcript_template: "", protocol_template: "",
  theme: "system", language: "system", notifications: true, developer_mode: false, input_device: "",
};

// `?fresh` в адресе — первый запуск: модели ещё не скачаны, библиотека пуста.
const fresh = new URLSearchParams(location.search).has("fresh");
const installed = new Set(fresh ? [] : ["gigaam", "vad", "segmentation", "embedding", "denoiser", "gigachat-lightning", "community1"]);
function models(): Model[] {
  const m = (name: string, kind: Model["kind"], title: string, about: string, publisher: string, license: string, size_mb: number, extra: Partial<Model> = {}): Model => ({
    name, kind, asr: null, diar: null, title, about, publisher, license, size_mb, installed: installed.has(name), required: false, ...extra,
  });
  const all = [
    m("gigaam", "asr", "GigaAM v3", l("Русская речь с пунктуацией", "Russian speech with punctuation"), l("Сбер", "Sber"), "MIT", 170, { asr: "gigaam" }),
    m("whisper-turbo", "asr", "Whisper large-v3-turbo", l("Речь на разных языках", "Speech in many languages"), "OpenAI", "MIT", 563, { asr: "whisper-turbo" }),
    m("whisper-large", "asr", "Whisper large-v3", l("Речь на разных языках, точнее и медленнее turbo", "Speech in many languages, more accurate and slower than turbo"), "OpenAI", "MIT", 1068, { asr: "whisper-large" }),
    m("t-one", "asr", "T-one", l("Русская речь, телефонные разговоры", "Russian speech, phone calls"), l("Т-Банк", "T-Bank"), "Apache 2.0", 128, { asr: "t-one" }),
    m("parakeet", "asr", "Parakeet TDT 0.6B v3", l("Речь на 25 европейских языках", "Speech in 25 European languages"), "NVIDIA", "CC BY 4.0", 487, { asr: "parakeet" }),
    m("glm-asr", "asr", "GLM-ASR-Nano", l("Речь на 17 языках, распознаёт языковая модель", "Speech in 17 languages, recognized by a language model"), "Zhipu AI", "Apache 2.0", 1700, { asr: "glm-asr" }),
    m("vad", "core", "Silero VAD", l("Поиск речи в записи", "Finds speech in a recording"), "Silero", "MIT", 1),
    m("segmentation", "core", "pyannote segmentation 3.0", l("Границы реплик", "Finds who speaks when"), "pyannote", "MIT", 7),
    m("embedding", "core", "WeSpeaker ResNet34-LM", l("Голосовые профили", "Voice profiles"), "WeSpeaker", "CC BY 4.0", 27),
    m("denoiser", "denoise", "DPDFNet", l("Подавление шума", "Noise reduction"), "Ceva", "Apache 2.0", 9),
    m("community1", "diar", "pyannote community-1", l("Разделение по спикерам", "Speaker separation"), "pyannote", "CC BY 4.0", 27, { diar: "community1" }),
    m("nemotron3", "diar", "Nemotron 3 Diarization", l("Разделение по спикерам", "Speaker separation"), "NVIDIA", "OpenMDW 1.1", 107, { diar: "nemotron3" }),
    m("gigachat-lightning", "llm", "GigaChat 3.1 Lightning", l("ИИ-помощник", "AI assistant"), l("Сбер", "Sber"), "MIT", 6470),
    m("t-lite", "llm", "T-lite 2.1", l("ИИ-помощник", "AI assistant"), l("Т-Банк", "T-Bank"), "Apache 2.0", 5030),
    m("yandexgpt-lite", "llm", "YandexGPT-5 Lite 8B", l("ИИ-помощник", "AI assistant"), l("Яндекс", "Yandex"), l("лицензия Яндекса, с ограничениями", "Yandex license, restrictions apply"), 4920),
    m("qwen3-4b", "llm", "Qwen3 4B Instruct", l("ИИ-помощник", "AI assistant"), "Alibaba", "Apache 2.0", 2500),
  ];
  for (const x of all) x.required = x.kind === "core" || x.asr === settings.asr_model || x.diar === settings.diar_model;
  return all;
}
const info = (): AppInfo => ({
  version: "0.2.0", data_dir: "/Users/demo/Library/Application Support/app.ultratranscript",
  templates_dir: "/Users/demo/Library/Application Support/app.ultratranscript/templates", models: models(), nemotron_runtime: false, system_language: null,
});

let folders: Folder[] = fresh ? [] : [{ id: 1, name: "Порт — цифровой двойник" }, { id: 2, name: "Интервью" }];
const rec = (id: string, title: string, days: number, duration: number, extra: Partial<Recording> = {}): Recording => ({
  id, title, source: `/Users/demo/Recordings/${title}.m4a`, created_at: stamp(days), duration, status: "done", error: "",
  rule_id: null, folder_id: null, archived: false, has_protocol: false, ...extra,
});
let recordings: Recording[] = [
  rec("r1", "Совещание по заявке в Минпромторг", 0, 2471, { has_protocol: true, folder_id: 1 }),
  rec("r2", "Планёрка 28 сентября", 0, 1312, { error: "Протокол не составлен: языковая модель недоступна" }),
  rec("r3", "Интервью с главным инженером", 1, 3605, { folder_id: 2 }),
  rec("r4", "Лекция: компьютерное зрение", 3, 5400),
  rec("r5", "Созвон с подрядчиком", 6, 940, { folder_id: 1 }),
  rec("r6", "Испорченная запись", 9, 0, { status: "error", error: "ffmpeg: Invalid data found when processing input" }),
  rec("r7", "Стратегическая сессия", 40, 7260),
  rec("r8", "Старый созвон", 95, 600, { archived: true }),
];
if (fresh) recordings = [];

const lines = [
  ["S1", "Добрый день, коллеги. Начнём совещание по проекту «Цифровой двойник порта». На повестке два вопроса: заявка в Минпромторг и смета на следующий квартал.", "Ну, добрый день, коллеги. Начнём, значит, совещание по проекту «Цифровой двойник порта». На повестке два вопроса: заявка в Минпромторг и смета на следующий квартал."],
  ["S2", "По заявке: документы готовы на 80%, осталось согласовать техническое задание с подрядчиком.", "По заявке: документы готовы на 80%, э-э, осталось согласовать техническое задание с подрядчиком."],
  ["S1", "Когда мы сможем подать заявку? Нам важно успеть до 10 октября.", "Хорошо. Ну, когда мы сможем подать заявку? Нам, как бы, важно успеть до 10 октября."],
  ["S2", "Думаю, подадим до 5 октября. Смету подготовит Смирнова Анна к следующей пятнице.", "Думаю, подадим до 5 октября. Смету подготовит Смирнова Анна к следующей пятнице."],
  ["S3", "Смета будет готова в четверг. Нужны актуальные цены на оборудование — запрошу их у поставщика сегодня.", "Смета будет готова в четверг. Нужны актуальные цены на оборудование — запрошу их у поставщика сегодня."],
  ["S1", "Тогда решили: заявку подаём до 5 октября, ответственный — Петров Сергей. Смету готовит Смирнова Анна. Спасибо всем, до встречи.", "Отлично. Тогда решили: заявку подаём до 5 октября, ответственный — Петров Сергей. Смету готовит Смирнова Анна. Спасибо всем, до встречи."],
];
const protocol = {
  тема: "Заявка в Минпромторг и смета на IV квартал",
  дата: stamp(0).slice(0, 10),
  участники: "Петров Сергей Иванович (руководитель проекта), Спикер 2, Смирнова Анна",
  повестка: ["Заявка в Минпромторг", "Смета на следующий квартал"],
  краткое_содержание: "Документы по заявке готовы на 80%: осталось согласовать техническое задание с подрядчиком. Заявку решено подать до 5 октября. Смета будет готова к четвергу, для неё запрашиваются актуальные цены на оборудование.",
  решения: ["Подать заявку в Минпромторг до 5 октября", "Подготовить смету к следующей пятнице"],
  поручения: [
    { что: "Подать заявку в Минпромторг", ответственный: "Петров Сергей", срок: "5 октября" },
    { что: "Подготовить смету", ответственный: "Смирнова Анна", срок: "следующая пятница" },
    { что: "Запросить цены на оборудование у поставщика", ответственный: "Смирнова Анна", срок: "сегодня" },
  ],
  открытые_вопросы: ["Согласование технического задания с подрядчиком"],
};
function transcript(r: Recording, withProtocol = r.has_protocol): Transcript {
  let at = 0;
  return {
    id: r.id, title: r.title, source: r.source, created_at: r.created_at, duration: r.duration || 95,
    asr_model: "GigaAM v3", diar_model: "pyannote 3.0", language: "ru",
    speakers: [
      { id: "S1", name: "Спикер 1", person_id: null, similarity: 0.82, suggested: 1 },
      { id: "S2", name: "Спикер 2", person_id: null, similarity: null },
      { id: "S3", name: "Смирнова Анна", person_id: 2, similarity: null },
    ],
    // Как в ядре: текст — как распознан, LLM его не переписывает.
    utterances: lines.map(([speaker, , text], id) => {
      const start = at;
      at += 6 + text.length / 14;
      return { id, speaker, start, end: at - 1, raw: text.toLowerCase(), text, clean: "" };
    }),
    protocol: withProtocol ? protocol : null,
  };
}
const transcripts: Record<string, Transcript> = {};
const transcriptOf = (id: string) => (transcripts[id] ??= transcript(recordings.find((r) => r.id === id)!));

let terms: Term[] = [
  { id: 1, term: "ГИСП", aliases: "гисп, гис п", definition: "Государственная информационная система промышленности", pending: true },
  { id: 2, term: "Минпромторг", aliases: "минпром торг, мин промторг", definition: "Министерство промышленности и торговли", pending: false },
  { id: 3, term: "Цифровой двойник", aliases: "", definition: "Виртуальная модель порта", pending: false },
  { id: 4, term: "ФРП", aliases: "эф эр пэ", definition: "Фонд развития промышленности", pending: false },
];
let people: Person[] = [
  { id: 3, name: "Козлов Дмитрий", aliases: "", role: "представитель подрядчика", org: "", voiceprints: 0, pending: true },
  { id: 1, name: "Петров Сергей Иванович", aliases: "Сергей Иванович", role: "Руководитель проекта", org: "ЦСР", voiceprints: 3, pending: false },
  { id: 2, name: "Смирнова Анна", aliases: "Аня", role: "Финансовый аналитик", org: "ЦСР", voiceprints: 1, pending: false },
];
let rules: Rule[] = [
  { id: 1, input_dir: "/Users/demo/Documents/Zoom", audio_dir: "", transcript_dir: "/Users/demo/Documents/Расшифровки", protocol_dir: "/Users/demo/Documents/Протоколы", protocol: true, docx: false, enabled: true },
];
let seq = 100;

// ---------- события ----------

let liveState: LiveState | null = null;
function job(id: string, kind: JobKind, status: JobEvent["status"], stage = "", title = "", progress = 0, message = "") {
  if (status === "processing") {
    if (liveState?.recording_id === id && liveState.job === kind) Object.assign(liveState, { stage, title, progress });
    else liveState = { recording_id: id, job: kind, stage, title, progress, lines: [], drafts: 0, protocol: "", seconds: 0 };
  } else if (liveState?.recording_id === id) liveState = null;
  return emit("job", { recording_id: id, job: kind, status, stage, title, progress, message } satisfies JobEvent);
}
function live(e: LiveEvent) {
  if (liveState?.recording_id === e.recording_id) {
    if (e.kind === "text") liveState.lines.push({ start: e.start, text: e.text });
    else if (e.kind === "draft") liveState.drafts += 1;
    else liveState.protocol = e.reset ? e.text : liveState.protocol + e.text;
  }
  return emit("live", e);
}

/** Запись с микрофона: таймер и громкость, фразы раз в несколько секунд — пока не остановят. */
let recorder: { id: string; stop: boolean; keep: boolean } | null = null;
async function record(r: Recording, meeting: boolean) {
  recorder = { id: r.id, stop: false, keep: true };
  await job(r.id, "record", "processing", "load", l("Загрузка моделей", "Loading models"));
  await sleep(1200);
  await job(r.id, "record", "processing", "record", l("Запись", "Recording"));
  const phrases = lines.map((x) => x[2]);
  let seconds = 0, next = 0;
  while (!recorder.stop) {
    await sleep(200);
    seconds += 0.2;
    const peak = 0.05 + Math.random() * 0.5 * (Math.sin(seconds * 1.7) + 1);
    const system = meeting ? 0.05 + Math.random() * 0.4 * (Math.cos(seconds * 1.3) + 1) : null;
    await emit("record", { recording_id: r.id, seconds, peak, system });
    if (seconds > next * 5 + 3 && next < phrases.length) {
      await live({ recording_id: r.id, kind: "text", start: next * 5, text: phrases[next] });
      next++;
    }
  }
  const keep = recorder.keep;
  recorder = null;
  if (!keep) {
    recordings = recordings.filter((x) => x.id !== r.id);
    return job(r.id, "record", "done", "", "", 1);
  }
  r.status = "queued";
  await job(r.id, "record", "done", "", "", 1);
  await transcribe(r);
}
async function stage(id: string, kind: JobKind, code: string, title: string, steps: number, each?: (i: number) => unknown) {
  await job(id, kind, "processing", code, title, 0);
  for (let i = 0; i < steps; i++) {
    await sleep(450);
    await each?.(i);
    await job(id, kind, "processing", code, title, (i + 1) / steps);
  }
}

/** Расшифровка: текст появляется по мере распознавания, затем черновик. */
async function transcribe(r: Recording) {
  await sleep(400);
  r.status = "processing";
  await job(r.id, "transcribe", "processing");
  await stage(r.id, "transcribe", "prepare", l("Подготовка аудио", "Preparing audio"), 2);
  await stage(r.id, "transcribe", "denoise", l("Шумоподавление", "Reducing noise"), 3);
  const full = transcript(r, false);
  await stage(r.id, "transcribe", "recognize", l("Распознавание речи", "Recognizing speech"), full.utterances.length, (i) =>
    live({ recording_id: r.id, kind: "text", start: full.utterances[i].start, text: full.utterances[i].text }),
  );
  await stage(r.id, "transcribe", "diarize", l("Разделение по спикерам", "Separating speakers"), 5);
  r.duration = full.duration;
  await stage(r.id, "transcribe", "terms", l("Исправление терминов", "Fixing terms"), 1);
  transcripts[r.id] = full;
  await live({ recording_id: r.id, kind: "draft" });
  r.status = "done";
  await job(r.id, "transcribe", "done", "", "", 1);
}

/** Протокол пишется на глазах: ответ LLM уходит в окно по кускам. */
async function writeProtocol(r: Recording) {
  await sleep(300);
  await job(r.id, "protocol", "processing", "protocol", l("Составление протокола", "Writing the minutes"));
  await live({ recording_id: r.id, kind: "protocol", text: "", reset: true });
  const text = JSON.stringify(protocol, null, 1);
  for (let i = 0; i < text.length; i += 9) {
    await sleep(45);
    await live({ recording_id: r.id, kind: "protocol", text: text.slice(i, i + 9), reset: false });
  }
  transcriptOf(r.id).protocol = protocol;
  r.has_protocol = true;
  await job(r.id, "protocol", "done", "", "", 1);
}

async function download(names: string[]) {
  for (const name of names.length ? names : models().filter((m) => m.required && !m.installed).map((m) => m.name)) {
    for (let p = 0; p < 1; p += 0.08) {
      await sleep(180);
      await emit("models", { name, progress: p, error: "" });
    }
    installed.add(name);
    await emit("models", { name, progress: 1, error: "" });
  }
}

// ---------- команды ----------

type Args = Record<string, any>;
const commands: Record<string, (a: Args) => unknown> = {
  app_info: info,
  legal: (a) => (a.kind === "license" ? license : notices),
  install_models: (a) => void download(a.names),
  delete_model: (a) => void installed.delete(a.name),
  list_recordings: () => recordings,
  import_files: (a) =>
    (a.paths as string[]).map((p) => {
      const r = rec(`n${++seq}`, p.split("/").pop()!.replace(/\.\w+$/, ""), 0, 0, { status: "queued", created_at: stamp(0, new Date().toTimeString().slice(0, 5)) });
      recordings = [r, ...recordings];
      void transcribe(recordings[0]);
      return r.id;
    }),
  retry: (a) => {
    const r = recordings.find((x) => x.id === a.id)!;
    r.status = "queued";
    void transcribe(r);
  },
  delete_recording: (a) => void (recordings = recordings.filter((r) => r.id !== a.id)),
  rename_recording: (a) => {
    recordings.find((r) => r.id === a.id)!.title = a.title;
    if (transcripts[a.id]) transcripts[a.id].title = a.title;
  },
  archive_recordings: (a) => recordings.forEach((r) => a.ids.includes(r.id) && (r.archived = a.archived)),
  move_recordings: (a) => recordings.forEach((r) => a.ids.includes(r.id) && (r.folder_id = a.folder)),
  list_folders: () => folders,
  save_folder: (a) => {
    if (a.id) folders.find((f) => f.id === a.id)!.name = a.name;
    else folders.push({ id: ++seq, name: a.name });
    return a.id ?? seq;
  },
  delete_folder: (a) => {
    folders = folders.filter((f) => f.id !== a.id);
    recordings.forEach((r) => r.folder_id === a.id && (r.folder_id = null));
  },
  search: (a) =>
    recordings.flatMap((r) => {
      if (r.status !== "done") return [];
      const hit = transcriptOf(r.id).utterances.find((u) => u.text.toLowerCase().includes(a.query));
      return hit ? [{ id: r.id, snippet: hit.text.slice(0, 70) }] : [];
    }),
  stats: () => ({
    recordings: recordings.length, seconds: recordings.reduce((s, r) => s + r.duration, 0),
    terms: terms.filter((t) => !t.pending).length, people: people.filter((p) => !p.pending).length,
    voices: people.filter((p) => p.voiceprints).length, rules: rules.filter((r) => r.enabled).length,
  }),
  get_transcript: (a) => {
    const r = recordings.find((x) => x.id === a.id);
    if (!r || (!transcripts[a.id] && r.status !== "done")) throw l("расшифровка ещё не готова", "the transcript is not ready yet");
    return transcriptOf(a.id);
  },
  live: () => (liveState ? [liveState] : []),
  start_recording: (a) => {
    if (recorder) throw l("запись уже идёт", "a recording is already in progress");
    const now = new Date();
    const stampNow = stamp(0, now.toTimeString().slice(0, 5));
    const meeting = a?.source === "meeting";
    const title = `${meeting ? l("Встреча", "Meeting") : l("Запись", "Recording")} ${stampNow.slice(8, 10)}.${stampNow.slice(5, 7)}.${stampNow.slice(0, 4)} ${stampNow.slice(11)}`;
    const r = rec(`n${++seq}`, title, 0, 0, { status: "recording", created_at: stampNow, source: `/Users/demo/Library/recordings/n${seq}/capture.wav` });
    recordings = [r, ...recordings];
    void record(recordings[0], meeting);
    return r.id;
  },
  stop_recording: (a) => {
    if (!recorder || recorder.id !== a.id) throw l("запись уже остановлена", "the recording has already stopped");
    recorder.keep = a.keep;
    recorder.stop = true;
  },
  input_devices: () => ["MacBook Pro Microphone", "AirPods Pro", "Scarlett 2i2 USB"],
  save_transcript: (a) => void (transcripts[a.transcript.id] = a.transcript),
  assign_speaker: (a) => {
    const t = transcriptOf(a.id);
    const s = t.speakers.find((x) => x.id === a.speaker)!;
    const p = people.find((x) => x.id === a.personId);
    Object.assign(s, { person_id: p?.id ?? (a.name ? ++seq : null), name: p?.name ?? (a.name || `Спикер ${a.speaker.slice(1)}`), similarity: null, suggested: null });
    if (!p && a.name) people.push({ id: seq, name: a.name, aliases: "", role: "", org: "", voiceprints: a.remember ? 1 : 0, pending: false });
    else if (p && a.remember) p.voiceprints += 1;
    return t;
  },
  make_protocol: (a) => void writeProtocol(recordings.find((r) => r.id === a.id)!),
  export_file: (a) => a.to ?? `/Users/demo/recordings/${a.id}/${a.doc}.${a.format}`,
  export_text: (a) => `# ${transcriptOf(a.id).title}\n\n` + transcriptOf(a.id).utterances.map((u) => (a.verbatim ? u.text : u.clean || u.text)).join("\n\n"),
  clipboard_text: () => "текст из буфера обмена",
  list_terms: () => terms,
  save_term: (a) => {
    if (a.term.id) terms = terms.map((t) => (t.id === a.term.id ? a.term : t));
    else terms.push({ ...a.term, id: ++seq });
    return a.term.id ?? seq;
  },
  delete_term: (a) => void (terms = terms.filter((t) => t.id !== a.id)),
  list_people: () => people,
  save_person: (a) => {
    if (a.person.id) people = people.map((p) => (p.id === a.person.id ? a.person : p));
    else people.push({ ...a.person, id: ++seq });
    return a.person.id ?? seq;
  },
  delete_person: (a) => void (people = people.filter((p) => p.id !== a.id)),
  pending_counts: () => ({ terms: terms.filter((t) => t.pending).length, people: people.filter((p) => p.pending).length }),
  export_directory: () => 3,
  import_directory: () => ({ total: 5, added: 3, updated: 1 }),
  list_voices: (a) =>
    Array.from({ length: people.find((p) => p.id === a.personId)?.voiceprints ?? 0 }, (_, i) => ({
      id: a.personId * 10 + i, title: recordings[i]?.title ?? "голос.m4a", uploaded: i === 2, date: stamp(i * 3).slice(0, 10),
      audio: "/demo/audio.ogg", start: 12, end: 28, text: lines[0][1],
    })),
  add_voice: (a) => void people.forEach((p) => p.id === a.personId && p.voiceprints++),
  delete_voice: (a) => void people.forEach((p) => p.id === Math.floor(a.id / 10) && p.voiceprints--),
  list_rules: () => rules,
  save_rule: (a) => {
    if (a.rule.id) rules = rules.map((r) => (r.id === a.rule.id ? a.rule : r));
    else rules.push({ ...a.rule, id: ++seq });
    return a.rule.id ?? seq;
  },
  delete_rule: (a) => void (rules = rules.filter((r) => r.id !== a.id)),
  get_settings: () => settings,
  save_settings: (a) => void Object.assign(settings, a.settings),
  set_language: (a) => void (english = a.language === "en"),
  test_llm: async () => (await sleep(900), '{"ok":true}'),
  reveal: () => {},
  // Системные диалоги выбора файлов: в браузере их нет — «выбран» выдуманный файл.
  "plugin:dialog|open": (a) => (a.options?.directory ? "/Users/demo/Documents/Диктофон" : a.options?.multiple ? ["/Users/demo/Recordings/Новая запись.m4a"] : "/Users/demo/Recordings/Новая запись.m4a"),
  "plugin:dialog|save": (a) => `/Users/demo/Documents/${a.options?.defaultPath ?? "file"}`,
};

export function install() {
  mockWindows("main");
  mockConvertFileSrc("macos");
  mockIPC(
    async (cmd, args) => {
      const run = commands[cmd];
      if (!run) return console.warn("mock: нет команды", cmd, args);
      await sleep(20);
      // Как настоящее ядро: окно получает копию данных, а не сами объекты.
      const out = await run((args ?? {}) as Args);
      return out === undefined ? null : JSON.parse(JSON.stringify(out));
    },
    { shouldMockEvents: true },
  );
  console.info("Ультра Транскрибатор: ядра нет, данные выдуманные (src/lib/dev/mock.ts)");
}
