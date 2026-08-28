console.log(
  JSON.stringify({
    importMetaMain: import.meta.main,
    bunMainMatchesPath: Bun.main === import.meta.path,
    argvMainMatchesPath: process.argv[1] === import.meta.path,
    arguments: process.argv.slice(2),
    execArgv: process.execArgv,
  }),
);

process.on("beforeExit", code => console.log(`beforeExit:${code}`));
process.on("exit", code => console.log(`exit:${code}`));
