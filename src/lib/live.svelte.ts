/**
 * Ход обработки записей в реальном времени: этап, распознанный текст, протокол по мере написания,
 * а у записи с микрофона — секунды и громкость.
 */

import { api, type JobEvent, type LiveEvent, type LiveState, type RecordEvent } from "./api";

let jobs = $state<Record<string, LiveState>>({});
/** Громкость микрофона (0…1) по записям, которые идут сейчас. */
let peaks = $state<Record<string, number>>({});

export const live = {
  /** Что сейчас происходит с записью; `undefined` — она не в работе. */
  of: (id: string): LiveState | undefined => jobs[id],
  peak: (id: string): number => peaks[id] ?? 0,
  /** Запись с микрофона, которая идёт сейчас. */
  get recording(): LiveState | undefined {
    return Object.values(jobs).find((j) => j.job === "record");
  },

  /** Окно открыли посреди обработки — забираем у ядра то, что уже накопилось. */
  async restore() {
    const states = await api.live().catch(() => [] as LiveState[]);
    for (const state of states) if (!jobs[state.recording_id]) jobs[state.recording_id] = state;
  },

  onRecord(e: RecordEvent) {
    const cur = jobs[e.recording_id];
    if (cur) cur.seconds = e.seconds;
    peaks[e.recording_id] = e.peak;
  },

  /** Возвращает `true`, когда задача началась или закончилась — пора обновить библиотеку. */
  onJob(e: JobEvent): boolean {
    if (e.status !== "processing") {
      delete jobs[e.recording_id];
      delete peaks[e.recording_id];
      return true;
    }
    const cur = jobs[e.recording_id];
    if (cur && cur.job === e.job) {
      cur.stage = e.stage;
      cur.title = e.title;
      cur.progress = e.progress;
      return false;
    }
    jobs[e.recording_id] = {
      recording_id: e.recording_id, job: e.job, stage: e.stage, title: e.title, progress: e.progress,
      lines: [], drafts: 0, protocol: "", seconds: 0,
    };
    return true;
  },

  onLive(e: LiveEvent) {
    const cur = jobs[e.recording_id];
    if (!cur) return;
    if (e.kind === "text") cur.lines.push({ start: e.start, text: e.text });
    else if (e.kind === "draft") cur.drafts += 1;
    else if (e.kind === "protocol") cur.protocol = e.reset ? e.text : cur.protocol + e.text;
  },
};

/**
 * Разбирает JSON, который ещё пишется: незакрытые строки, массивы и объекты закрываются,
 * недописанный хвост (ключ без значения, обрывок числа) отбрасывается.
 */
export function parsePartialJson(text: string): unknown {
  const stack: ("{" | "[")[] = [];
  let inString = false, escaped = false, isKey = false;
  // Где в последний раз значение было дописано целиком — и что тогда было открыто.
  let safe = 0, safeDepth = 0;
  let expectKey = false;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (inString) {
      if (escaped) escaped = false;
      else if (c === "\\") escaped = true;
      else if (c === '"') {
        inString = false;
        if (!isKey) { safe = i + 1; safeDepth = stack.length; }
      }
      continue;
    }
    if (c === '"') {
      inString = true;
      isKey = expectKey;
      expectKey = false;
    } else if (c === "{" || c === "[") {
      stack.push(c);
      expectKey = c === "{";
      safe = i + 1;
      safeDepth = stack.length;
    } else if (c === "}" || c === "]") {
      stack.pop();
      expectKey = false;
      safe = i + 1;
      safeDepth = stack.length;
    } else if (c === ",") {
      // Значение перед запятой завершено (число или литерал — тоже).
      safe = i;
      safeDepth = stack.length;
      expectKey = stack[stack.length - 1] === "{";
    }
  }
  const close = (depth: number) => stack.slice(0, depth).reverse().map((c) => (c === "{" ? "}" : "]")).join("");
  const attempts: string[] = [];
  if (inString && !isKey) {
    // Строка-значение пишется прямо сейчас: показываем её как есть, без оборванной экранирующей последовательности.
    let body = escaped ? text.slice(0, -1) : text;
    const cut = /(\\+)u[0-9a-fA-F]{0,3}$/.exec(body);
    if (cut && cut[1].length % 2 === 1) body = body.slice(0, cut.index + cut[1].length - 1);
    attempts.push(`${body}"${close(stack.length)}`);
  } else if (!inString) attempts.push(text + close(stack.length));
  attempts.push(text.slice(0, safe) + close(safeDepth));
  for (const a of attempts) {
    try {
      return JSON.parse(a);
    } catch {
      // пробуем более короткий вариант
    }
  }
  return null;
}
