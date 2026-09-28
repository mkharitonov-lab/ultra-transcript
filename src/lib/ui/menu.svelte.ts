/** Контекстное меню приложения: одно на всё окно, рисует его `ContextMenu.svelte`. */

export type MenuItem =
  | { separator: true }
  | { heading: string }
  | {
      label: string;
      icon?: string;
      /** Подсказка справа: сочетание клавиш или пояснение. */
      hint?: string;
      danger?: boolean;
      disabled?: boolean;
      checked?: boolean;
      action?: () => void;
      /** Вложенное меню. */
      items?: MenuItem[];
    };

export type MenuEntry = Extract<MenuItem, { label: string }>;

type Open = { x: number; y: number; items: MenuItem[] };

let open = $state<Open | null>(null);

export const menu = {
  get current() {
    return open;
  },
  close() {
    open = null;
  },
};

/** Пустые разделы не показываем: разделитель в начале, в конце или два подряд. */
function tidy(items: MenuItem[]): MenuItem[] {
  const out: MenuItem[] = [];
  for (const it of items) {
    if ("separator" in it && (!out.length || "separator" in out[out.length - 1])) continue;
    out.push("label" in it && it.items ? { ...it, items: tidy(it.items) } : it);
  }
  while (out.length && "separator" in out[out.length - 1]) out.pop();
  return out;
}

/** Открыть меню у курсора (правый щелчок) или под элементом (кнопка «ещё»). */
export function openMenu(e: MouseEvent, items: MenuItem[]) {
  e.preventDefault();
  e.stopPropagation();
  const list = tidy(items);
  if (!list.length) return;
  if (e.type === "contextmenu" || !(e.currentTarget instanceof HTMLElement)) {
    open = { x: e.clientX, y: e.clientY, items: list };
  } else {
    const r = e.currentTarget.getBoundingClientRect();
    open = { x: r.left, y: r.bottom + 4, items: list };
  }
}
