/** Действия над записями и папками библиотеки — общие для бокового списка, заголовка записи и меню. */

import { save } from "@tauri-apps/plugin-dialog";
import { api, copyText, recordingDir, revealLabel, showError, type AsrModel, type DiarModel, type Folder, type Recording } from "./api";
import { t, tn } from "./i18n.svelte";
import { live } from "./live.svelte";
import { app } from "./state.svelte";
import { confirm, prompt } from "./ui/dialog.svelte";
import type { MenuItem } from "./ui/menu.svelte";
import { toast } from "./ui/toast.svelte";

const ids = (rs: Recording[]) => rs.map((r) => r.id);
const busy = (r: Recording) => r.status === "processing" || r.status === "queued" || !!live.of(r.id);

async function done(work: Promise<unknown>) {
  await work.catch(showError);
  await app.refresh();
}

// ---------- записи ----------

export async function rename(r: Recording) {
  const title = await prompt({ title: t("rec.renameTitle"), input: r.title, confirm: t("common.rename") });
  if (title && title !== r.title) await done(api.renameRecording(r.id, title));
}

export async function remove(rs: Recording[]) {
  const ok = await confirm({
    title: rs.length === 1 ? t("rec.deleteTitle", { title: rs[0].title }) : tn("rec.deleteManyTitle", rs.length),
    text: rs.every((r) => r.archived) ? t("rec.deleteText") : `${t("rec.deleteText")} ${t("rec.deleteArchiveHint")}`,
    confirm: t("common.delete"),
    danger: true,
  });
  if (!ok) return;
  await done(Promise.all(rs.map((r) => api.deleteRecording(r.id))));
}

export async function archive(rs: Recording[], archived: boolean) {
  await done(api.archiveRecordings(ids(rs), archived));
  if (archived) {
    toast(rs.length === 1 ? t("rec.archived") : tn("rec.archivedMany", rs.length), "info", {
      label: t("common.undo"),
      run: () => done(api.archiveRecordings(ids(rs), false)),
    });
  }
}

export const moveTo = (rs: Recording[], folder: number | null) => done(api.moveRecordings(ids(rs), folder));

export async function redo(r: Recording, asr: AsrModel | null = null, diar: DiarModel | null = null) {
  const ok = await confirm({ title: t("rec.redoTitle"), text: t("rec.redoText"), confirm: t("rec.redoConfirm") });
  if (ok) await done(api.retry(r.id, asr, diar));
}

export const makeProtocol = (r: Recording) => done(api.makeProtocol(r.id));

export type ExportKind = "transcript" | "protocol";
export type ExportFormat = "md" | "docx";

/** Файл выгрузки собирается по запросу; затем его открывают, показывают в папке или сохраняют в другое место. */
export async function exportFile(r: Recording, kind: ExportKind, format: ExportFormat, how: "open" | "reveal" | "save", verbatim = false) {
  try {
    if (how === "save") {
      const name = `${r.title} — ${t(kind === "protocol" ? "export.protocolName" : "export.transcriptName")}.${format}`;
      const to = await save({
        defaultPath: name.replace(/[/\\:*?"<>|]/g, "_"),
        filters: [format === "md" ? { name: "Markdown", extensions: ["md"] } : { name: "Word", extensions: ["docx"] }],
      });
      if (!to) return;
      await api.exportFile(r.id, kind, format, to, verbatim);
      toast(t("export.saved"), "ok", { label: revealLabel(), run: () => api.reveal(to) });
    } else {
      const path = await api.exportFile(r.id, kind, format, null, verbatim);
      await api.reveal(path, how === "open");
    }
  } catch (e) {
    showError(e);
  }
}

/** Папка записи в Finder / Проводнике: там аудио и расшифровка. */
export function reveal(r: Recording) {
  if (!app.info) return;
  const dir = recordingDir(app.info.data_dir, r.id);
  (r.duration > 0 ? api.reveal(`${dir}/audio.ogg`) : api.reveal(dir, true)).catch(showError);
}

export const copyDoc = (r: Recording, kind: ExportKind, verbatim = false) =>
  api.exportText(r.id, kind, verbatim).then(copyText).catch(showError);

// ---------- папки ----------

/** Создаёт папку и, если переданы записи, сразу кладёт их в неё. */
export async function newFolder(rs: Recording[] = []) {
  const name = await prompt({ title: t("folder.newTitle"), placeholder: t("folder.placeholder"), confirm: t("folder.create") });
  if (!name) return;
  try {
    const id = await api.saveFolder(null, name);
    if (rs.length) await api.moveRecordings(ids(rs), id);
  } catch (e) {
    showError(e);
  }
  await app.refresh();
}

export async function renameFolder(f: Folder) {
  const name = await prompt({ title: t("folder.renameTitle"), input: f.name, confirm: t("common.rename") });
  if (name && name !== f.name) await done(api.saveFolder(f.id, name));
}

export async function removeFolder(f: Folder) {
  const used = app.recordings.some((r) => r.folder_id === f.id);
  if (used && !(await confirm({ title: t("folder.deleteTitle", { name: f.name }), text: t("folder.deleteText"), confirm: t("common.delete"), danger: true }))) return;
  await done(api.deleteFolder(f.id));
}

// ---------- меню ----------

function exportItems(r: Recording, kind: ExportKind): MenuItem[] {
  return [
    { label: t("export.copy"), icon: "copy", action: () => copyDoc(r, kind) },
    { separator: true },
    { label: "Markdown (.md)", icon: "doc", action: () => exportFile(r, kind, "md", "save") },
    { label: "Word (.docx)", icon: "doc", action: () => exportFile(r, kind, "docx", "save") },
  ];
}

/** Меню записи (или нескольких выделенных записей). */
export function recordingMenu(rs: Recording[]): MenuItem[] {
  if (!rs.length) return [];
  const one = rs.length === 1 ? rs[0] : null;
  const ready = one?.status === "done" || (one?.status === "error" && one.duration > 0);
  const archived = rs.every((r) => r.archived);
  const inFolder = rs.some((r) => r.folder_id !== null);
  const models = app.info?.models ?? [];

  const folders: MenuItem[] = [
    ...app.folders.map((f): MenuItem => ({
      label: f.name,
      icon: "folder",
      checked: rs.every((r) => r.folder_id === f.id),
      action: () => moveTo(rs, f.id),
    })),
    { separator: true },
    { label: t("folder.new"), icon: "folder-plus", action: () => newFolder(rs) },
    ...(inFolder ? [{ label: t("rec.moveOut"), icon: "x", action: () => moveTo(rs, null) }] : []),
  ];

  const items: MenuItem[] = [];
  if (one) {
    items.push({ label: t("common.open"), icon: "wave", action: () => app.open(one.id) });
    items.push({ label: t("common.rename"), icon: "edit", action: () => rename(one) });
    items.push({ separator: true });
  } else {
    items.push({ heading: tn("rec.selected", rs.length) }, { separator: true });
  }
  items.push({ label: t("rec.move"), icon: "folder", items: folders });
  items.push(
    archived
      ? { label: t("rec.unarchive"), icon: "archive", action: () => archive(rs, false) }
      : { label: t("rec.archive"), icon: "archive", action: () => archive(rs, true) },
  );
  if (one) {
    items.push({ separator: true });
    if (ready) {
      items.push({ label: t("export.transcript"), icon: "upload", items: exportItems(one, "transcript") });
      if (one.has_protocol) items.push({ label: t("export.protocol"), icon: "upload", items: exportItems(one, "protocol") });
      else items.push({ label: t("rec.makeProtocol"), icon: "sparkle", disabled: busy(one), action: () => makeProtocol(one) });
    }
    items.push({
      label: t("rec.redo"),
      icon: "refresh",
      disabled: busy(one),
      items: models
        .filter((m) => m.kind === "asr" && m.asr)
        .map((m): MenuItem => ({
          label: m.title,
          hint: m.installed ? "" : t("common.notInstalled"),
          disabled: !m.installed,
          action: () => redo(one, m.asr),
        })),
    });
    items.push({ label: revealLabel(), icon: "reveal", action: () => reveal(one) });
  }
  items.push({ separator: true });
  items.push({ label: one ? t("rec.delete") : tn("rec.deleteMany", rs.length), icon: "trash", danger: true, action: () => remove(rs) });
  return items;
}

export function folderMenu(f: Folder): MenuItem[] {
  return [
    { label: t("common.rename"), icon: "edit", action: () => renameFolder(f) },
    { label: t("folder.new"), icon: "folder-plus", action: () => newFolder() },
    { separator: true },
    { label: t("folder.delete"), icon: "trash", danger: true, action: () => removeFolder(f) },
  ];
}
