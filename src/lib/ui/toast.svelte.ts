/** Короткие сообщения в углу окна: что получилось и что пошло не так. Рисует `Toasts.svelte`. */

export type Toast = { id: number; kind: "info" | "ok" | "error"; text: string; action?: { label: string; run: () => void } };

let items = $state<Toast[]>([]);
let seq = 0;

export const toasts = {
  get items() {
    return items;
  },
  dismiss(id: number) {
    items = items.filter((t) => t.id !== id);
  },
};

/** Сообщение исчезает само; ошибке и сообщению с действием времени даётся больше. */
export function toast(text: string, kind: Toast["kind"] = "info", action?: Toast["action"]) {
  const id = ++seq;
  // Одинаковые сообщения подряд (например, сбой в цикле) не копим.
  items = [...items.filter((t) => t.text !== text).slice(-3), { id, kind, text, action }];
  setTimeout(() => toasts.dismiss(id), kind === "error" ? 12_000 : action ? 8000 : 3500);
  return id;
}
