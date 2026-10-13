// Read a property the host attached to this module's `import.meta` via
// `Runtime::set_import_meta_hook`, proving the hook reaches a module Bun
// imported from disk (which `Module::meta` cannot).
export const injected = (import.meta as unknown as { injected?: string }).injected ?? "MISSING";
export const specifier = import.meta.path;
