<script lang="ts">
  import { open, save } from "@tauri-apps/plugin-dialog";
  import Icon from "./Icon.svelte";
  import { api, showError, type Directory } from "./api";
  import { t, tn } from "./i18n.svelte";
  import { openMenu } from "./ui/menu.svelte";
  import { toast } from "./ui/toast.svelte";

  let { kind, onimport }: { kind: Directory; onimport: () => void } = $props();

  let busy = $state(false);

  async function importFile() {
    const path = await open({ filters: [{ name: t("io.tables"), extensions: ["xlsx", "xls", "ods", "csv"] }] });
    if (typeof path !== "string") return;
    busy = true;
    try {
      const r = await api.importDirectory(kind, path);
      toast(r.added || r.updated ? t("io.imported", { added: r.added, updated: r.updated }) : t(r.total ? "io.nothingNew" : "io.emptyFile"), "ok");
      onimport();
    } catch (e) {
      showError(e);
    } finally {
      busy = false;
    }
  }

  async function exportFile(ext: "xlsx" | "csv") {
    const path = await save({
      defaultPath: `${t(kind === "terms" ? "nav.dictionary" : "nav.people")}.${ext}`,
      filters: [ext === "xlsx" ? { name: "Excel", extensions: ["xlsx"] } : { name: "CSV", extensions: ["csv"] }],
    });
    if (!path) return;
    try {
      const n = await api.exportDirectory(kind, path);
      toast(tn(kind === "terms" ? "io.savedTerms" : "io.savedPeople", n), "ok", { label: t("common.show"), run: () => api.reveal(path) });
    } catch (e) {
      showError(e);
    }
  }
</script>

<button class="with-icon" onclick={importFile} disabled={busy} title={t("io.importHint")}>
  <Icon name="download" size={12} /> {t("io.import")}
</button>
<button class="with-icon" title={t(kind === "people" ? "io.exportHintPeople" : "io.exportHint")}
  onclick={(e) => openMenu(e, [
    { label: "Excel (.xlsx)", action: () => exportFile("xlsx") },
    { label: "CSV (.csv)", action: () => exportFile("csv") },
  ])}>
  <Icon name="upload" size={12} /> {t("io.export")} <Icon name="chevron" size={11} />
</button>
