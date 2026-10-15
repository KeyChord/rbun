#include "root.h"

namespace Zig {

JSC::JSObject* GlobalObject::moduleLoaderCreateImportMetaProperties(JSGlobalObject* globalObject,
    JSModuleLoader* loader,
    JSValue key,
    JSModuleRecord* record,
    RefPtr<JSC::ScriptFetcher>)
{
    auto* importMeta = Zig::ImportMetaObject::create(globalObject, key);
    importMeta->setModuleGraph(globalObject->vm(), Bun::moduleGraphOfLoader(globalObject, loader));
    return importMeta;
}

} // namespace Zig
