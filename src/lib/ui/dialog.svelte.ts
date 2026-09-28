/** Диалоги приложения (вместо системных): подтверждение, ввод строки, сообщение. Рисует `Dialogs.svelte`. */

export type DialogSpec = {
  title: string;
  text?: string;
  /** Поле ввода: начальное значение. Без него — просто подтверждение. */
  input?: string;
  placeholder?: string;
  confirm?: string;
  cancel?: string | null;
  danger?: boolean;
};

type Open = DialogSpec & { resolve: (v: string | boolean | null) => void };

let queue = $state<Open[]>([]);

export const dialogs = {
  get current() {
    return queue[0] ?? null;
  },
  answer(v: string | boolean | null) {
    queue.shift()?.resolve(v);
  },
};

function ask<T>(spec: DialogSpec) {
  return new Promise<T>((resolve) => queue.push({ ...spec, resolve: resolve as Open["resolve"] }));
}

/** Подтверждение действия; `true` — согласие. */
export const confirm = (spec: DialogSpec) => ask<boolean>(spec).then(Boolean);

/** Ввод строки; `null` — отмена. */
export const prompt = (spec: DialogSpec) => ask<string | null>({ input: "", ...spec }).then((v) => (typeof v === "string" ? v : null));
