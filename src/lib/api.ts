import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ask, message } from "@tauri-apps/plugin-dialog";

export type Recording = {
  id: string; title: string; source: string; created_at: string;
  duration: number; status: "queued" | "processing" | "done" | "error"; error: string;
  rule_id: number | null;
};
export type Speaker = { id: string; name: string; person_id: number | null; similarity: number | null };
export type Utterance = { id: number; speaker: string; start: number; end: number; raw: string; text: string; clean: string };
export type Transcript = {
  id: string; title: string; source: string; created_at: string; duration: number;
  speakers: Speaker[]; utterances: Utterance[]; protocol?: Record<string, unknown> | null;
};
export type Term = { id: number | null; term: string; aliases: string; definition: string };
export type Person = { id: number | null; name: string; aliases: string; role: string; org: string; voiceprints: number };
export type Suggestion = { id: number; kind: "term" | "person"; value: string; detail: string; recording_id: string };
export type Rule = {
  id: number | null; input_dir: string; audio_dir: string; transcript_dir: string;
  protocol_dir: string; protocol: boolean; enabled: boolean;
};
export type Settings = {
  llm_enabled: boolean; llm_base_url: string; llm_model: string; llm_api_key: string;
  cluster_threshold: number; voice_threshold: number; archive_kbps: number;
  auto_accept_suggestions: boolean; transcript_template: string; protocol_template: string;
};
export type Model = { name: string; title: string; size_mb: number; installed: boolean };
export type AppInfo = { data_dir: string; templates_dir: string; models: Model[] };
export type JobEvent = { recording_id: string; status: Recording["status"]; stage: string; progress: number; message: string };
export type ModelEvent = { name: string; progress: number; error: string };

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  installModels: () => invoke<void>("install_models"),
  recordings: () => invoke<Recording[]>("list_recordings"),
  importFiles: (paths: string[]) => invoke<string[]>("import_files", { paths }),
  retry: (id: string) => invoke<void>("retry", { id }),
  deleteRecording: (id: string) => invoke<void>("delete_recording", { id }),
  transcript: (id: string) => invoke<Transcript>("get_transcript", { id }),
  saveTranscript: (transcript: Transcript) => invoke<void>("save_transcript", { transcript }),
  assignSpeaker: (id: string, speaker: string, personId: number | null, name: string) =>
    invoke<Transcript>("assign_speaker", { id, speaker, personId, name }),
  makeProtocol: (id: string) => invoke<void>("make_protocol", { id }),
  terms: () => invoke<Term[]>("list_terms"),
  saveTerm: (term: Term) => invoke<number>("save_term", { term }),
  deleteTerm: (id: number) => invoke<void>("delete_term", { id }),
  people: () => invoke<Person[]>("list_people"),
  savePerson: (person: Person) => invoke<number>("save_person", { person }),
  deletePerson: (id: number) => invoke<void>("delete_person", { id }),
  deleteVoiceprints: (personId: number) => invoke<void>("delete_voiceprints", { personId }),
  suggestions: () => invoke<Suggestion[]>("list_suggestions"),
  resolveSuggestion: (id: number, accept: boolean) => invoke<void>("resolve_suggestion", { id, accept }),
  rules: () => invoke<Rule[]>("list_rules"),
  saveRule: (rule: Rule) => invoke<number>("save_rule", { rule }),
  deleteRule: (id: number) => invoke<void>("delete_rule", { id }),
  settings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),
  testLlm: (settings: Settings) => invoke<string>("test_llm", { settings }),
  reveal: (path: string, open = false) => invoke<void>("reveal", { path, open }),
  onJob: (cb: (e: JobEvent) => void) => listen<JobEvent>("job", (e) => cb(e.payload)),
  onModels: (cb: (e: ModelEvent) => void) => listen<ModelEvent>("models", (e) => cb(e.payload)),
};

export const recordingDir = (dataDir: string, id: string) => `${dataDir}/recordings/${id}`;
export const audioUrl = (dataDir: string, id: string) => convertFileSrc(`${recordingDir(dataDir, id)}/audio.ogg`);

export function fmtTime(sec: number): string {
  const s = Math.max(0, Math.floor(sec));
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), r = s % 60;
  return h ? `${h}:${String(m).padStart(2, "0")}:${String(r).padStart(2, "0")}` : `${m}:${String(r).padStart(2, "0")}`;
}

const PALETTE = ["#5b5bd6", "#d6409f", "#12a594", "#e5484d", "#f76b15", "#3e63dd", "#8e4ec6", "#29a383"];
export const speakerColor = (id: string) => PALETTE[(parseInt(id.replace(/\D/g, "")) || 1) % PALETTE.length];

export const confirmDialog = (text: string) => ask(text, { title: "Ultra Transcript", kind: "warning", okLabel: "Да", cancelLabel: "Отмена" });
export const showError = (e: unknown) => message(String(e), { title: "Ошибка", kind: "error" });
