#include "root.h"

namespace Zig {

JSC::JSObject* GlobalObject::moduleLoaderCreateImportMetaProperties(JSGlobalObject* globalObject,
    JSModuleLoader* loader,
    JSValue key,
    JSModuleRecord* record,
    RefPtr<JSC::ScriptFetcher>)
{
    return Zig::ImportMetaObject::create(globalObject, key);
}

} // namespace Zig
