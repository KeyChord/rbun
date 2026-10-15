#include "root.h"

namespace Zig {

// [rbun patch] import.meta embedder callback. Set by the host through
// `bun_embed_vm_set_import_meta_callback` (src/runtime/embed.rs); null when no
// host installed one, which is the case for the normal bun executable.
extern "C" void (*bun_embed_import_meta_callback)(void* globalObject, JSC::EncodedJSValue key, JSC::EncodedJSValue meta) = nullptr;

JSC::JSObject* GlobalObject::moduleLoaderCreateImportMetaProperties(JSGlobalObject* globalObject,
    JSModuleLoader* loader,
    JSValue key,
    JSModuleRecord* record,
    RefPtr<JSC::ScriptFetcher>)
{
    auto* importMeta = Zig::ImportMetaObject::create(globalObject, key);
    importMeta->setModuleGraph(globalObject->vm(), Bun::moduleGraphOfLoader(globalObject, loader));
    // [rbun patch] JSC caches whatever this hook returns as the module's
    // `import.meta`, and JavaScriptCore exposes no public getter for that
    // cached object afterwards (unlike QuickJS' `JS_GetImportMeta`, which
    // rquickjs' `Module::meta` wraps). Handing the fresh object to the
    // embedder here is therefore the only place a host can attach its own
    // properties to a module's real `import.meta`.
    if (bun_embed_import_meta_callback != nullptr) {
        bun_embed_import_meta_callback(
            reinterpret_cast<void*>(globalObject),
            JSC::JSValue::encode(key),
            JSC::JSValue::encode(JSC::JSValue(importMeta)));
    }
    return importMeta;
}

} // namespace Zig
