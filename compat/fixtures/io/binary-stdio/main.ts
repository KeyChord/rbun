await Bun.write(Bun.stdout, new Uint8Array([0, 255, 65, 13, 10, 66]));
await Bun.write(Bun.stderr, new Uint8Array([254, 0, 67, 13, 10, 68]));
