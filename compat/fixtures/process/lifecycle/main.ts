process.on("beforeExit", code => console.log(`beforeExit:${code}`));
process.on("exit", code => console.log(`exit:${code}`));
console.log("body");
