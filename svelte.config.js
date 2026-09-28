// Tauri doesn't have a Node.js server to do proper SSR
// so we use adapter-static with a fallback to index.html to put the site in SPA mode
// See: https://svelte.dev/docs/kit/single-page-apps
// See: https://v2.tauri.app/start/frontend/sveltekit/ for more info
import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";
import process from "node:process";

// `pnpm build:demo` — интерфейс одним файлом demo/index.html на выдуманных данных (src/lib/dev/mock.ts):
// открывается в браузере прямо с диска, без ядра и без сервера.
const demo = process.env.UT_DEMO === "1";

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),
  kit: {
    adapter: adapter({
      fallback: "index.html",
      ...(demo ? { pages: "demo", assets: "demo" } : {}),
    }),
    ...(demo ? { files: { routes: "src/demo" }, output: { bundleStrategy: "inline" }, router: { type: "hash" } } : {}),
  },
};

export default config;
