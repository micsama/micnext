declare module "markdown-it-texmath" {
  import type { PluginWithOptions } from "markdown-it";
  type Rule = { name: string; rex: RegExp; tmpl: string; tag: string; displayMode?: boolean };
  const texmath: PluginWithOptions<{
    engine: unknown;
    delimiters?: string | string[];
    katexOptions?: Record<string, unknown>;
  }> & { rules: Record<string, { inline: Rule[]; block: Rule[] }> };
  export default texmath;
}
