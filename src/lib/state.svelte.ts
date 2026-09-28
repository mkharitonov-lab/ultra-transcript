/** Общее состояние окна: что открыто, библиотека, настройки. */

import { api, type AppInfo, type Folder, type PendingCounts, type Recording, type Settings } from "./api";
import { i18n } from "./i18n.svelte";

export type View = "home" | "recording" | "dictionary" | "people" | "settings";
export type SettingsSection =
  | "general" | "recognition" | "speakers" | "audio" | "llm" | "documents" | "watch" | "models" | "developer" | "about";

class AppState {
  info = $state<AppInfo | null>(null);
  settings = $state<Settings | null>(null);
  recordings = $state<Recording[]>([]);
  folders = $state<Folder[]>([]);
  /** Находки LLM, ждущие проверки, — счётчики у «Словаря» и «Кто есть кто». */
  pending = $state<PendingCounts>({ terms: 0, people: 0 });

  view = $state<View>("home");
  /** Открытая запись. */
  selected = $state<string | null>(null);
  section = $state<SettingsSection>("general");

  current = $derived(this.recordings.find((r) => r.id === this.selected) ?? null);

  async init() {
    [this.info, this.settings] = await Promise.all([api.appInfo(), api.settings()]);
    this.applyLanguage();
    await this.refresh();
  }

  async refresh() {
    [this.recordings, this.folders, this.pending] = await Promise.all([api.recordings(), api.folders(), api.pendingCounts()]);
    if (this.selected && !this.recordings.some((r) => r.id === this.selected)) this.go("home");
  }

  async refreshInfo() {
    this.info = await api.appInfo();
  }

  /** Сохраняет настройки целиком; язык и тема применяются сразу. */
  async saveSettings() {
    if (!this.settings) return;
    this.applyLanguage();
    await api.saveSettings($state.snapshot(this.settings));
    // Названия моделей и этапов приходят из ядра уже на выбранном языке.
    await this.refreshInfo();
  }

  private applyLanguage() {
    if (!this.settings) return;
    const lang = i18n.apply(this.settings.language, this.info?.system_language);
    document.title = lang === "ru" ? "Ультра Транскрибатор" : "Ultra Transcript";
    api.setLanguage(lang);
  }

  open(id: string) {
    this.selected = id;
    this.view = "recording";
  }

  go(view: Exclude<View, "recording">, section?: SettingsSection) {
    this.view = view;
    this.selected = null;
    if (section) this.section = section;
  }

  /** Режим разработчика: открывает раздел настроек с параметрами для отладки. */
  get developer() {
    return this.settings?.developer_mode ?? false;
  }
}

export const app = new AppState();
