/** Пункты меню для текста под курсором: поле ввода, правка расшифровки или просто выделенный текст. */

import { api, isMac, showError } from "../api";
import { t } from "../i18n.svelte";
import type { MenuItem } from "./menu.svelte";

type Field = HTMLInputElement | HTMLTextAreaElement;
const isField = (el: Element | null): el is Field =>
  el instanceof HTMLTextAreaElement ||
  (el instanceof HTMLInputElement && ["text", "search", "password", "email", "url", "number", "tel"].includes(el.type));

const key = (k: string) => (isMac ? `⌘${k}` : `Ctrl+${k}`);

async function paste() {
  const text = await api.clipboardText().catch((e) => (showError(e), ""));
  // insertText сохраняет отмену (⌘Z) и сообщает полю об изменении.
  if (text) document.execCommand("insertText", false, text);
}

export function textItems(e: MouseEvent): MenuItem[] {
  const target = e.target instanceof Element ? e.target : null;
  const field = target?.closest("input, textarea") ?? null;
  const editable = target?.closest<HTMLElement>("[contenteditable]:not([contenteditable='false'])") ?? null;
  const selected = isField(field) ? field.selectionStart !== field.selectionEnd : !!getSelection()?.toString();
  if (isField(field) || editable) {
    const locked = isField(field) && (field.readOnly || field.disabled);
    (isField(field) ? field : editable)?.focus();
    return [
      { label: t("common.cut"), hint: key("X"), disabled: !selected || locked, action: () => document.execCommand("cut") },
      { label: t("common.copy"), hint: key("C"), disabled: !selected, action: () => document.execCommand("copy") },
      { label: t("common.paste"), hint: key("V"), disabled: locked, action: paste },
      { label: t("common.selectAll"), hint: key("A"), action: () => (isField(field) ? field.select() : document.execCommand("selectAll")) },
    ];
  }
  return selected ? [{ label: t("common.copy"), hint: key("C"), action: () => document.execCommand("copy") }] : [];
}
