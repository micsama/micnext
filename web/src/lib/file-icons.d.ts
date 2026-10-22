declare module "virtual:file-icons" {
  type IconMap = Record<string, string>;
  export const extensions: IconMap;
  export const names: IconMap;
  export const lightExtensions: IconMap;
  export const lightNames: IconMap;
  export const fallback: string;
  export const urls: IconMap;
}
