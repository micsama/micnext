import { defineConfig, type Plugin } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";
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

// 文件图标沿用 VS Code 的 Material Icon Theme：构建时取其扩展名/文件名映射，只打包映射用到的图标，按需加载。
function fileIcons(): Plugin {
  const id = "virtual:file-icons";
  return {
    name: "file-icons",
    resolveId: (source) => (source === id ? `\0${id}` : null),
    load(resolved) {
      if (resolved !== `\0${id}`) return null;
      const m = generateManifest();
      const maps = {
        extensions: m.fileExtensions ?? {},
        names: m.fileNames ?? {},
        lightExtensions: m.light?.fileExtensions ?? {},
        lightNames: m.light?.fileNames ?? {},
      };
      const icons = [...new Set([m.file!, ...Object.values(maps).flatMap(Object.values)])];
      const imports = icons.map(
        (icon, i) => `import i${i} from "material-icon-theme/icons/${m.iconDefinitions![icon]!.iconPath.split("/").at(-1)}?no-inline";`,
      );
      const exports = Object.entries(maps).map(([k, v]) => `export const ${k} = ${JSON.stringify(v)};`);
      const urls = icons.map((icon, i) => `${JSON.stringify(icon)}: i${i}`).join(",");
      return [...imports, ...exports, `export const fallback = ${JSON.stringify(m.file)};`, `export const urls = {${urls}};`].join("\n");
    },
  };
}

export default defineConfig({
  plugins: [katexWoff2Only(), fileIcons(), svelte(), tailwindcss()],
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
