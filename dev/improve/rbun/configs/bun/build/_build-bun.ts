#!/usr/bin/env bun
import { runMain } from "citty";
import { buildBunCommand } from "./build-bun-command.ts";

runMain(buildBunCommand);
