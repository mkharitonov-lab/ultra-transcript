/** Обновления приложения из GitHub Releases: `latest.json` последнего релиза, подпись своим ключом
 *  (плагин updater). Найденное обновление показывает `UpdatePrompt.svelte` — с коротким списком изменений
 *  из CHANGELOG.md, который CI кладёт в `latest.json`. */

import { isTauri } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";

type Status = "idle" | "checking" | "latest" | "available" | "downloading" | "error";

class Updater {
  status = $state<Status>("idle");
  /** Найденное обновление: версия и список изменений. */
  update = $state.raw<Update | null>(null);
  /** Скачано, доля 0..1 (если сервер сообщил размер). */
  progress = $state(0);
  error = $state("");
  /** Предложение закрыто кнопкой «Позже» — до следующего запуска или ручной проверки. */
  dismissed = $state(false);

  /** Проверить наличие обновления. `quiet` — при запуске: ошибки (нет сети) не показываем. */
  async check(quiet = false) {
    if (!isTauri() || this.status === "checking" || this.status === "downloading") return;
    this.status = "checking";
    this.error = "";
    try {
      this.update = await check();
      this.status = this.update ? "available" : "latest";
      if (this.update) this.dismissed = false;
    } catch (e) {
      this.status = quiet ? "idle" : "error";
      this.error = String(e);
    }
  }

  /** Скачать, установить и перезапустить приложение. */
  async install() {
    const update = this.update;
    if (!update) return;
    this.status = "downloading";
    this.progress = 0;
    let total = 0;
    let done = 0;
    try {
      await update.downloadAndInstall((e) => {
        if (e.event === "Started") total = e.data.contentLength ?? 0;
        else if (e.event === "Progress") {
          done += e.data.chunkLength;
          if (total) this.progress = Math.min(1, done / total);
        }
      });
      await relaunch();
    } catch (e) {
      this.status = "error";
      this.error = String(e);
    }
  }

  /** Список изменений: строки `- …` из раздела CHANGELOG.md; остальной текст — как есть. */
  get notes(): string[] {
    const body = this.update?.body?.trim() ?? "";
    if (!body) return [];
    return body
      .split("\n")
      .map((l) => l.trim().replace(/^[-*•]\s*/, ""))
      .filter(Boolean);
  }
}

export const updater = new Updater();
