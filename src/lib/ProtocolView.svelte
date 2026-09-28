<script lang="ts">
  /** Протокол: поля шаблона по порядку. `writing` — он ещё пишется: пустое поле не значит, что сказать нечего. */
  let { data, writing = false }: { data: Record<string, unknown>; writing?: boolean } = $props();

  function label(key: string) {
    const s = key.replaceAll("_", " ");
    return s ? s[0].toUpperCase() + s.slice(1) : s;
  }
  const asText = (v: unknown): string =>
    typeof v === "string" ? v : Array.isArray(v) ? v.map(asText).join(", ") : v == null ? "" : typeof v === "object" ? Object.values(v).map(asText).join(" — ") : String(v);
  const isTable = (v: unknown): v is Record<string, unknown>[] => Array.isArray(v) && v.some((x) => x && typeof x === "object" && !Array.isArray(x));
  const columns = (rows: Record<string, unknown>[]) => [...new Set(rows.flatMap((r) => (r && typeof r === "object" ? Object.keys(r) : [])))];
  const none = $derived(writing ? "" : "—");

  const entries = $derived(Object.entries(data));
  /** Тема выносится в заголовок. */
  const subject = $derived(entries.find(([k, v]) => ["тема", "topic", "название", "title"].includes(k) && typeof v === "string" && v.trim()));
  const isFact = (v: unknown) => typeof v === "string" && v.length <= 160 && !v.includes("\n");
  const rest = $derived(entries.filter(([k]) => k !== subject?.[0]));
  /** Короткие сведения в начале (дата, участники) — строками; с первого длинного поля идут разделы. */
  const split = $derived(rest.findIndex(([, v]) => !isFact(v)));
  const facts = $derived(split < 0 ? rest : rest.slice(0, split));
  const sections = $derived(split < 0 ? [] : rest.slice(split));
</script>

<article>
  {#if subject}<h1>{subject[1]}</h1>{/if}
  {#each facts as [k, v] (k)}
    <p class="fact"><span class="muted">{label(k)}:</span> {v || none}</p>
  {/each}
  {#each sections as [k, v] (k)}
    <section>
      <h2>{label(k)}</h2>
      {#if isTable(v)}
        <table>
          <thead><tr>{#each columns(v) as c (c)}<th>{label(c)}</th>{/each}</tr></thead>
          <tbody>
            {#each v as row}<tr>{#each columns(v) as c (c)}<td>{asText(row?.[c])}</td>{/each}</tr>{/each}
          </tbody>
        </table>
      {:else if Array.isArray(v)}
        {#if v.length}<ul>{#each v as item}<li>{asText(item)}</li>{/each}</ul>{:else}<p class="faint">{none}</p>{/if}
      {:else}
        <p>{asText(v) || none}</p>
      {/if}
    </section>
  {/each}
</article>

<style>
  article { max-width: 800px; user-select: text; -webkit-user-select: text; cursor: text; }
  h1 { font-size: 19px; margin: 0 0 10px; }
  h2 { font-size: var(--fs-md); margin: 0 0 6px; }
  section { margin-top: 18px; }
  p, li { font-size: 14px; line-height: 1.55; margin: 0; white-space: pre-wrap; }
  .fact { margin-top: 2px; }
  ul { margin: 0; padding-left: 20px; }
  table { border-collapse: collapse; width: 100%; font-size: var(--fs-md); }
  th, td { border: 1px solid var(--border); padding: 6px 9px; text-align: left; vertical-align: top; }
  th { background: var(--bg-card); font-weight: 600; }
</style>
