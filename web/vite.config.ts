import { defineConfig, type Plugin } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";
import { readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { brotliCompressSync, constants } from "node:zlib";
import { generateManifest } from "material-icon-theme";

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

// 文件图标沿用 VS Code 的 Material Icon Theme：构建时取其扩展名/文件名映射；映射用到的图标打成一个按需加载的 data URL 模块。
function fileIcons(): Plugin {
  const maps = "virtual:file-icons";
  const svgs = "virtual:file-icon-svgs";
  const dir = fileURLToPath(new URL("./node_modules/material-icon-theme/icons/", import.meta.url));
  const manifest = generateManifest();
  const table = {
    extensions: manifest.fileExtensions ?? {},
    names: manifest.fileNames ?? {},
    lightExtensions: manifest.light?.fileExtensions ?? {},
    lightNames: manifest.light?.fileNames ?? {},
  };
  return {
    name: "file-icons",
    resolveId: (source) => (source === maps || source === svgs ? `\0${source}` : null),
    load(resolved) {
      if (resolved === `\0${maps}`) {
        const exports = Object.entries(table).map(([k, v]) => `export const ${k} = ${JSON.stringify(v)};`);
        return [...exports, `export const fallback = ${JSON.stringify(manifest.file)};`].join("\n");
      }
      if (resolved === `\0${svgs}`) {
        const icons = new Set([manifest.file!, ...Object.values(table).flatMap(Object.values)]);
        const urls = Object.fromEntries(
          [...icons].map((icon) => {
            const svg = readFileSync(join(dir, manifest.iconDefinitions![icon]!.iconPath.split("/").at(-1)!), "utf8");
            return [icon, `data:image/svg+xml,${encodeURIComponent(svg)}`];
          }),
        );
        return `export default ${JSON.stringify(urls)};`;
      }
      return null;
    },
  };
}

// 文本产物预压缩为 brotli 并删除原件：二进制只嵌一份，网关带 `Content-Encoding: br` 原样返回。
function brotliText(): Plugin {
  return {
    name: "brotli-text",
    apply: "build",
    writeBundle({ dir }, bundle) {
      for (const file of Object.keys(bundle)) {
        if (!/\.(html|js|css|svg|json)$/.test(file)) continue;
        const path = join(dir!, file);
        const params = { [constants.BROTLI_PARAM_QUALITY]: constants.BROTLI_MAX_QUALITY };
        writeFileSync(`${path}.br`, brotliCompressSync(readFileSync(path), { params }));
        rmSync(path);
      }
    },
  };
}

export default defineConfig({
  plugins: [katexWoff2Only(), fileIcons(), svelte(), tailwindcss(), brotliText()],
  resolve: {
    alias: { $lib: decodeURIComponent(new URL("./src/lib", import.meta.url).pathname) },
  },
  server: {
    proxy: { "/api": "http://127.0.0.1:7878" },
  },
  build: {
    reportCompressedSize: false,
    cssCodeSplit: false,
    chunkSizeWarningLimit: 1000,
    rolldownOptions: {
      output: {
        // 应用代码、Markdown 渲染（含 KaTeX、highlight.js）、其余依赖、文件图标各一个 chunk。
        codeSplitting: {
          groups: [
            { name: "file-icons", test: /file-icon-svgs/, priority: 3 },
            {
              name: "markdown",
              test: /node_modules[\\/](markdown-it|katex|highlight\.js|linkify-it|mdurl|uc\.micro|entities|punycode)/,
              priority: 2,
            },
            { name: "vendor", test: /node_modules/, priority: 1 },
          ],
        },
      },
    },
  },
});
