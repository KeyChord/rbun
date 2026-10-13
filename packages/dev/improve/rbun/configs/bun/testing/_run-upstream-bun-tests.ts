#!/usr/bin/env bun
import { runMain } from "citty";
import { runUpstreamBunTestsCommand } from "./run-upstream-bun-tests-command.ts";

runMain(runUpstreamBunTestsCommand);
