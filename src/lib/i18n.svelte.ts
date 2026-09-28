/** Язык интерфейса. Строки — в `locales/`; русский словарь задаёт набор ключей. */

import ru from "./locales/ru";
import en from "./locales/en";

export type Lang = "ru" | "en";
export type LangSetting = Lang | "system";
export type Key = keyof typeof ru;
export type Dict = Record<Key, string | readonly string[]>;
type Params = Record<string, string | number>;

const dicts: Record<Lang, Dict> = { ru, en };

/** Язык системы, каким его видит окно: русский — если он есть среди языков системы, иначе английский. */
function browserLang(): Lang {
  const all = globalThis.navigator?.languages ?? [globalThis.navigator?.language ?? ""];
  return all.some((l) => l.toLowerCase().startsWith("ru")) ? "ru" : "en";
}

let lang = $state<Lang>(browserLang());

export const i18n = {
  get lang() {
    return lang;
  },
  /**
   * Применить настройку; возвращает выбранный язык. `system` — язык системы по сведениям ядра:
   * на macOS окно знает только язык, с которым запущено приложение.
   */
  apply(setting: LangSetting, system?: Lang | null): Lang {
    lang = setting === "system" ? (system ?? browserLang()) : setting;
    document.documentElement.lang = lang;
    return lang;
  },
};

const fill = (s: string, params?: Params) => (params ? s.replace(/\{(\w+)\}/g, (_, k) => String(params[k] ?? "")) : s);

/** Строка на языке интерфейса; `{имя}` заменяется значением из `params`. */
export function t(key: Key, params?: Params): string {
  const v = dicts[lang][key] ?? dicts.ru[key] ?? key;
  return fill(typeof v === "string" ? v : v[0], params);
}

/** Номер формы слова для числа: в русском три формы, в английском две. */
function form(n: number): number {
  n = Math.abs(n);
  if (lang === "en") return n === 1 ? 0 : 1;
  const d = n % 10, dd = n % 100;
  if (d === 1 && dd !== 11) return 0;
  return d >= 2 && d <= 4 && (dd < 12 || dd > 14) ? 1 : 2;
}

/** Строка с числом: tn("n.recordings", 5) → «5 записей». В строке доступно `{n}`. */
export function tn(key: Key, n: number, params?: Params): string {
  const v = dicts[lang][key] ?? dicts.ru[key] ?? key;
  const s = typeof v === "string" ? v : (v[form(n)] ?? v[v.length - 1]);
  return fill(s, { n: n.toLocaleString(locale()), ...params });
}

export const locale = () => (lang === "ru" ? "ru-RU" : "en-US");

/** «1,5 ГБ» или «170 МБ». */
export function fmtSize(mb: number): string {
  if (mb >= 1000) return `${(mb / 1000).toLocaleString(locale(), { maximumFractionDigits: 1 })} ${t("unit.gb")}`;
  return `${Math.round(mb)} ${t("unit.mb")}`;
}

const dayStart = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();

/** Дата записи («2026-09-28 21:05») по-человечески: «Сегодня, 21:05», «28 сент., 21:05», «28 сент. 2025 г.». */
export function fmtDate(stamp: string, withTime = true): string {
  const d = new Date(stamp.replace(" ", "T"));
  if (isNaN(d.getTime())) return stamp;
  const now = new Date();
  const days = Math.round((dayStart(now) - dayStart(d)) / 86_400_000);
  const time = d.toLocaleTimeString(locale(), { hour: "2-digit", minute: "2-digit" });
  let day: string;
  if (days === 0) day = t("date.today");
  else if (days === 1) day = t("date.yesterday");
  else if (d.getFullYear() === now.getFullYear()) day = d.toLocaleDateString(locale(), { day: "numeric", month: "short" });
  else return d.toLocaleDateString(locale(), { day: "numeric", month: "short", year: "numeric" });
  return withTime ? `${day}, ${time}` : day;
}

/** Длительность словами: «1 ч 05 мин», «12 мин», «40 с». */
export function fmtDuration(sec: number): string {
  const s = Math.max(0, Math.round(sec));
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60);
  if (h) return `${h} ${t("unit.h")} ${String(m).padStart(2, "0")} ${t("unit.min")}`;
  if (m) return `${m} ${t("unit.min")}`;
  return `${s} ${t("unit.s")}`;
}
