/** Загрузка моделей: одна на всё окно, чтобы ход скачивания был виден в любом разделе. */

import { api, showError, type Model, type ModelEvent } from "./api";
import { fmtSize, t } from "./i18n.svelte";
import { app } from "./state.svelte";
import { confirm } from "./ui/dialog.svelte";

let progress = $state<Record<string, number>>({});
/** Что сделать, когда модель скачается (например, выбрать её в настройках). */
const after: Record<string, () => void> = {};

export const models = {
  /** Доля скачанного; `undefined` — модель сейчас не скачивается. */
  progress: (name: string): number | undefined => progress[name],
  get busy() {
    return Object.keys(progress).length > 0;
  },

  download(names: string[], then?: () => void) {
    for (const n of names) progress[n] = 0;
    if (then && names.length) after[names[names.length - 1]] = then;
    api.installModels(names).catch(showError);
  },

  async remove(m: Model) {
    const ok = await confirm({
      title: t("models.deleteTitle", { name: m.title }),
      text: t("models.deleteText", { size: fmtSize(m.size_mb) }),
      confirm: t("common.delete"),
      danger: true,
    });
    if (!ok) return;
    await api.deleteModel(m.name).catch(showError);
    await app.refreshInfo();
  },

  onEvent(e: ModelEvent) {
    if (e.error) {
      // Сбой останавливает всю очередь загрузки.
      progress = {};
      for (const k of Object.keys(after)) delete after[k];
      showError(`${t("models.failed")}: ${e.error}`);
      return;
    }
    if (e.progress < 1) {
      progress[e.name] = e.progress;
      return;
    }
    delete progress[e.name];
    app.refreshInfo().then(() => {
      after[e.name]?.();
      delete after[e.name];
    });
  },
};

/** Модели, без которых нельзя расшифровывать при текущих настройках. */
export const required = () => app.info?.models.filter((m) => m.required) ?? [];
export const ready = () => required().every((m) => m.installed);
/** Что скачать для начала работы: обязательные модели и, если включено шумоподавление, его модель. */
export const starter = () => app.info?.models.filter((m) => m.required || (m.kind === "denoise" && app.settings?.denoise)) ?? [];
