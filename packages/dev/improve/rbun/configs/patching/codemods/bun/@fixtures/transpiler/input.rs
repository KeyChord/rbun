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
        !bun_paths::is_absolute(specifier)
            && bun_core::strings::index_of_char_usize(specifier, b':').is_some()
    }
}
