pub struct PluginRunner;

impl PluginRunner {
    /// Cheap pre-filter that rules
    /// out `./` / `../` / absolute paths before hitting the resolve hook.
    pub fn could_be_plugin(specifier: &[u8]) -> bool {
        if let Some(last_dot) = bun_core::strings::last_index_of_char(specifier, b'.') {
            let ext = &specifier[last_dot + 1..];
            // '.' followed by either a letter or a non-ascii character
            // maybe there are non-ascii file extensions?
            // we mostly want to cheaply rule out "../" and ".." and "./"
            if !ext.is_empty()
                && (ext[0].is_ascii_lowercase() || ext[0].is_ascii_uppercase() || ext[0] > 127)
            {
                return true;
            }
        }
        // [rbun patch] upstream only forwards `ns:` specifiers here, so a bare
        // name such as `chord` never reaches a runtime `onResolve` hook.
        // Embedders using rbun's `Resolver` (modelled on rquickjs, whose
        // resolvers see every specifier) need bare names too, so forward
        // everything that is neither absolute nor relative. The hook still
        // returns `undefined` for names it does not own, falling through to
        // bun's regular resolution.
        !bun_paths::is_absolute(specifier)
            && !(specifier.starts_with(b"./")
                || specifier.starts_with(b"../")
                || specifier == b"."
                || specifier == b"..")
    }
}
