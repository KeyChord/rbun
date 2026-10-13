# Vendored Bun

- `packages/com/github/oven-sh/bun/` contains third-party Bun source and generated build input. Exclude it from routine searches, reviews, linting, and formatting unless the task explicitly requires work on Bun itself.
- Keep the pinned `src` submodule and tracked package/build files intact. Make rbun's Bun customizations in `packages/dev/improve/rbun/configs/patching/` and regenerate the vendor tree through the existing build workflow.
