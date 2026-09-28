import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { t, type Lang, type LangSetting } from "./i18n.svelte";
import { toast } from "./ui/toast.svelte";

export type Recording = {
  id: string; title: string; source: string; created_at: string;
  duration: number; status: "queued" | "processing" | "done" | "error"; error: string;
  rule_id: number | null;
  /** Папка библиотеки. */
  folder_id: number | null;
  archived: boolean;
  has_protocol: boolean;
};
export type Folder = { id: number; name: string };
export type Speaker = { id: string; name: string; person_id: number | null; similarity: number | null };
export type Utterance = { id: number; speaker: string; start: number; end: number; raw: string; text: string; clean: string };
export type AsrModel = "gigaam" | "whisper-turbo";
/** Движок диаризации: без разделения, pyannote 3.0, pyannote community-1, NVIDIA Nemotron 3. */
export type DiarModel = "off" | "pyannote3" | "community1" | "nemotron3";
export type Transcript = {
  id: string; title: string; source: string; created_at: string; duration: number; asr_model: string; diar_model: string;
  /** Язык записи: "ru", "en"…; пусто — русский. */
  language: string;
  speakers: Speaker[]; utterances: Utterance[]; protocol?: Record<string, unknown> | null;
};
/** `pending` — находка LLM, которую пользователь ещё не подтвердил. */
export type Term = { id: number | null; term: string; aliases: string; definition: string; pending: boolean };
export type Person = {
  id: number | null; name: string; aliases: string; role: string; org: string; voiceprints: number; pending: boolean;
};
export type Directory = "terms" | "people";
export type PendingCounts = Record<Directory, number>;
export type ImportReport = { total: number; added: number; updated: number };
/** Образец голоса: `audio` пусто — послушать нельзя (запись удалена вместе с аудио). */
export type VoiceSample = {
  id: number; title: string; uploaded: boolean; date: string; audio: string; start: number; end: number; text: string;
};
/** Отслеживаемая папка; `docx` — рядом с Markdown сохранять документы Word. */
export type Rule = {
  id: number | null; input_dir: string; audio_dir: string; transcript_dir: string;
  protocol_dir: string; protocol: boolean; docx: boolean; enabled: boolean;
};
export type Theme = "system" | "light" | "dark";
export type Settings = {
  asr_model: AsrModel;
  /** Язык записей для Whisper: код языка или "auto". */
  speech_language: string;
  denoise: boolean; level_volume: boolean;
  llm_enabled: boolean; llm_provider: "builtin" | "api"; llm_local_model: string; llm_base_url: string; llm_model: string; llm_api_key: string;
  diar_model: DiarModel; cluster_threshold: number; voice_threshold: number; archive_kbps: number;
  auto_accept_suggestions: boolean; transcript_template: string; protocol_template: string;
  theme: Theme; language: LangSetting; notifications: boolean; developer_mode: boolean;
};
/** `required` — без модели нельзя расшифровывать при текущих настройках. */
export type Model = {
  name: string; kind: "core" | "asr" | "diar" | "llm" | "denoise"; asr: AsrModel | null; diar: DiarModel | null;
  title: string; about: string; publisher: string; license: string;
  size_mb: number; installed: boolean; required: boolean;
};
/** `nemotron_runtime` — собрана ли библиотека NeMo-Speech.cpp, без неё Nemotron 3 недоступен. */
export type AppInfo = {
  version: string; data_dir: string; templates_dir: string; models: Model[]; nemotron_runtime: boolean;
  /** Язык системы, если ядро смогло его узнать. */
  system_language: Lang | null;
};
export type JobKind = "transcribe" | "protocol" | "export";
/** `stage` — код этапа, `title` — его название на языке интерфейса. */
export type JobEvent = {
  recording_id: string; job: JobKind; status: Recording["status"];
  stage: string; title: string; progress: number; message: string;
};
/** Текст, который появляется по ходу работы: фрагмент речи, черновик расшифровки, кусок протокола. */
export type LiveEvent = { recording_id: string } & (
  | { kind: "text"; start: number; text: string }
  | { kind: "draft" }
  | { kind: "protocol"; text: string; reset: boolean }
);
/** Задача в работе и всё, что она успела показать. */
export type LiveState = {
  recording_id: string; job: JobKind; stage: string; title: string; progress: number;
  lines: { start: number; text: string }[]; drafts: number; protocol: string;
};
export type ModelEvent = { name: string; progress: number; error: string };
export type Stats = { recordings: number; seconds: number; terms: number; people: number; voices: number; rules: number };
export type Hit = { id: string; snippet: string };
export type Doc = "transcript" | "protocol";
export type Format = "md" | "docx";

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  legal: (kind: "license" | "notices") => invoke<string>("legal", { kind }),
  installModels: (names: string[] = []) => invoke<void>("install_models", { names }),
  deleteModel: (name: string) => invoke<void>("delete_model", { name }),
  recordings: () => invoke<Recording[]>("list_recordings"),
  importFiles: (paths: string[]) => invoke<string[]>("import_files", { paths }),
  retry: (id: string, asr: AsrModel | null = null, diar: DiarModel | null = null) => invoke<void>("retry", { id, asr, diar }),
  deleteRecording: (id: string) => invoke<void>("delete_recording", { id }),
  renameRecording: (id: string, title: string) => invoke<void>("rename_recording", { id, title }),
  archiveRecordings: (ids: string[], archived: boolean) => invoke<void>("archive_recordings", { ids, archived }),
  moveRecordings: (ids: string[], folder: number | null) => invoke<void>("move_recordings", { ids, folder }),
  folders: () => invoke<Folder[]>("list_folders"),
  saveFolder: (id: number | null, name: string) => invoke<number>("save_folder", { id, name }),
  deleteFolder: (id: number) => invoke<void>("delete_folder", { id }),
  search: (query: string) => invoke<Hit[]>("search", { query }),
  stats: () => invoke<Stats>("stats"),
  transcript: (id: string) => invoke<Transcript>("get_transcript", { id }),
  live: () => invoke<LiveState | null>("live"),
  saveTranscript: (transcript: Transcript) => invoke<void>("save_transcript", { transcript }),
  assignSpeaker: (id: string, speaker: string, personId: number | null, name: string) =>
    invoke<Transcript>("assign_speaker", { id, speaker, personId, name }),
  makeProtocol: (id: string) => invoke<void>("make_protocol", { id }),
  /** Собирает документ и возвращает путь к файлу; без `to` файл остаётся в папке записи. */
  exportFile: (id: string, doc: Doc, format: Format, to: string | null, verbatim = false) =>
    invoke<string>("export_file", { id, doc, format, to, verbatim }),
  exportText: (id: string, doc: Doc, verbatim = false) => invoke<string>("export_text", { id, doc, verbatim }),
  clipboardText: () => invoke<string>("clipboard_text"),
  terms: () => invoke<Term[]>("list_terms"),
  saveTerm: (term: Term) => invoke<number>("save_term", { term }),
  deleteTerm: (id: number) => invoke<void>("delete_term", { id }),
  people: () => invoke<Person[]>("list_people"),
  savePerson: (person: Person) => invoke<number>("save_person", { person }),
  deletePerson: (id: number) => invoke<void>("delete_person", { id }),
  pendingCounts: () => invoke<PendingCounts>("pending_counts"),
  exportDirectory: (kind: Directory, path: string) => invoke<number>("export_directory", { kind, path }),
  importDirectory: (kind: Directory, path: string) => invoke<ImportReport>("import_directory", { kind, path }),
  voices: (personId: number) => invoke<VoiceSample[]>("list_voices", { personId }),
  addVoice: (personId: number, path: string) => invoke<void>("add_voice", { personId, path }),
  deleteVoice: (id: number) => invoke<void>("delete_voice", { id }),
  rules: () => invoke<Rule[]>("list_rules"),
  saveRule: (rule: Rule) => invoke<number>("save_rule", { rule }),
  deleteRule: (id: number) => invoke<void>("delete_rule", { id }),
  settings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),
  setLanguage: (language: Lang) => invoke<void>("set_language", { language }),
  testLlm: (settings: Settings) => invoke<string>("test_llm", { settings }),
  reveal: (path: string, open = false) => invoke<void>("reveal", { path, open }),
  onJob: (cb: (e: JobEvent) => void) => listen<JobEvent>("job", (e) => cb(e.payload)),
  onLive: (cb: (e: LiveEvent) => void) => listen<LiveEvent>("live", (e) => cb(e.payload)),
  onModels: (cb: (e: ModelEvent) => void) => listen<ModelEvent>("models", (e) => cb(e.payload)),
  /** Пункт строки меню приложения, который выполняет окно. */
  onMenu: (cb: (action: "about" | "settings" | "add" | "folder") => void) => listen<"about" | "settings" | "add" | "folder">("menu", (e) => cb(e.payload)),
};

export const recordingDir = (dataDir: string, id: string) => `${dataDir}/recordings/${id}`;
export const audioUrl = (dataDir: string, id: string) => convertFileSrc(`${recordingDir(dataDir, id)}/audio.ogg`);

/** Форматы, которые умеет открывать ядро (через ffmpeg). */
export const mediaFilter = () => ({
  name: t("io.media"),
  extensions: ["mp3", "m4a", "wav", "aac", "ogg", "opus", "flac", "wma", "amr", "aiff", "caf", "mp4", "mov", "mkv", "avi", "webm", "wmv", "m4v", "mpeg", "mpg"],
});

export const isMac = navigator.platform.toLowerCase().includes("mac");
/** «Показать в Finder» / «Показать в Проводнике». */
export const revealLabel = () => t(isMac ? "common.revealMac" : "common.revealWin");

export function fmtTime(sec: number): string {
  const s = Math.max(0, Math.floor(sec));
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), r = s % 60;
  return h ? `${h}:${String(m).padStart(2, "0")}:${String(r).padStart(2, "0")}` : `${m}:${String(r).padStart(2, "0")}`;
}

const PALETTE = ["#5b5bd6", "#d6409f", "#12a594", "#e5484d", "#f76b15", "#3e63dd", "#8e4ec6", "#29a383"];
export const speakerColor = (id: string) => PALETTE[(parseInt(id.replace(/\D/g, "")) || 1) % PALETTE.length];

/** Текст — в буфер обмена. Запасной путь — для WebView, где `navigator.clipboard` недоступен. */
export async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const area = document.createElement("textarea");
    area.value = text;
    area.style.cssText = "position:fixed;opacity:0";
    document.body.append(area);
    area.select();
    const ok = document.execCommand("copy");
    area.remove();
    if (!ok) throw new Error(t("common.copyFailed"));
  }
  toast(t("common.copied"), "ok");
}

/** Сообщение об ошибке — в углу окна. */
export function showError(e: unknown) {
  const text = e instanceof Error ? e.message : String(e);
  toast(text ? text[0].toUpperCase() + text.slice(1) : t("common.error"), "error");
}
