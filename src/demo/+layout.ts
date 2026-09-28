// Маршруты для `pnpm build:demo`: интерфейс одним файлом, всегда на выдуманных данных.
import { install } from "$lib/dev/mock";

export function load() {
  if (!("__TAURI_INTERNALS__" in window)) install();
}
