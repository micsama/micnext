import { defineConfig, type Plugin } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";

// KaTeX 的样式为每种字体列了 woff2/woff/ttf 三份；现代浏览器只用 woff2，其余两份删掉免得打进二进制。
function katexWoff2Only(): Plugin {
  return {
    name: "katex-woff2-only",
    enforce: "pre",
    transform(code, id) {
      if (!/katex(\.min)?\.css$/.test(id)) return null;
      return code.replace(/,\s*url\([^)]*\.woff\) format\("woff"\)|,\s*url\([^)]*\.ttf\) format\("truetype"\)/g, "");
    },
  };
}

export default defineConfig({
  plugins: [katexWoff2Only(), svelte(), tailwindcss()],
  resolve: {
    alias: { $lib: decodeURIComponent(new URL("./src/lib", import.meta.url).pathname) },
  },
  server: {
    proxy: { "/api": "http://127.0.0.1:7878" },
  },
  build: {
    chunkSizeWarningLimit: 1500,
  },
});
