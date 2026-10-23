import { homedir } from "node:os";
import { isAbsolute, join } from "node:path";

const args = process.argv.slice(2);
const at = args.indexOf("--config");
if (at >= 0 && !args[at + 1]) throw new Error("--config 缺少路径");
const config = at < 0
  ? join(process.env.XDG_CONFIG_HOME || join(homedir(), ".config"), "micnext/config.toml")
  : args[at + 1];
const file = Bun.file(config);
if (at >= 0 || await file.exists()) {
  const parsed = Bun.TOML.parse(await file.text()) as { core?: { data_dir?: string } };
  const dir = parsed.core?.data_dir;
  if (dir !== undefined) {
    if (typeof dir !== "string" || (!isAbsolute(dir) && !dir.startsWith("~/"))) {
      throw new Error("core.data_dir 必须为绝对路径或 ~/ 开头");
    }
    console.log(join(dir.startsWith("~/") ? join(homedir(), dir.slice(2)) : dir, "micnext.db"));
    process.exit(0);
  }
}
console.log(join(process.env.XDG_DATA_HOME || join(homedir(), ".local/share"), "micnext/micnext.db"));
