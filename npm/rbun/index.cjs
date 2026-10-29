const path = require("node:path");
const { rbun } = require("./package.json");

exports.libDir = __dirname;
exports.libPath = path.join(__dirname, "libbun_embed.dylib");
exports.bunVersion = rbun.bunVersion;
exports.bunRevision = rbun.bunRevision;
