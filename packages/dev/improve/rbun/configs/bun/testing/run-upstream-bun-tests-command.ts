import { defineCommand } from "citty";
import {
  printClassifications,
  runSuite,
  suiteDescriptions,
  syncCheckout,
} from "./run-upstream-bun-tests.ts";

const suites = suiteDescriptions();
const suiteHelp = Object.entries(suites)
  .map(([name, description]) => `${name} — ${description}`)
  .join("; ");

export const runUpstreamBunTestsCommand = defineCommand({
  meta: {
    name: "_run-upstream-bun-tests",
    description:
      "Initialize, classify, and differentially run Bun's upstream runtime tests from the pinned Bun source submodule.",
  },
  default: "run",
  subCommands: {
    sync: defineCommand({
      meta: {
        name: "sync",
        description: "Initialize the Bun source submodule or sync RBUN_UPSTREAM_BUN_DIR to the pinned revision.",
      },
      run() {
        syncCheckout();
      },
    }),
    classify: defineCommand({
      meta: {
        name: "classify",
        description: "Classify discovered upstream test files (in-process, runtime-subprocess, cli, mixed).",
      },
      run() {
        return printClassifications();
      },
    }),
    run: defineCommand({
      meta: {
        name: "run",
        description: "Differentially run an upstream compatibility suite against reference Bun and rbun-test-host.",
      },
      args: {
        suite: {
          type: "positional",
          description: `Suite to run (${suiteHelp})`,
          default: "image",
          required: false,
        },
        filter: {
          type: "positional",
          description: "Optional substring filter applied to selected suite file paths",
          required: false,
        },
      },
      async run({ args }) {
        await runSuite(args.suite, args.filter || undefined);
      },
    }),
  },
});
