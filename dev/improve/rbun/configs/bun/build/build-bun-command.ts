import { defineCommand } from "citty";
import { buildBun } from "./build-bun.ts";

export const buildBunCommand = defineCommand({
  meta: {
    name: "_build-bun",
    description: "Build the vendored Bun and link libbun_embed.dylib.",
  },
  async run({ rawArgs }) {
    await buildBun(rawArgs);
  },
});
