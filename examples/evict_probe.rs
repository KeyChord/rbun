//! Probe: `Ctx::evict_module` makes a re-import re-read a disk module.
use rbun::{Context, Object, Runtime};

fn main() {
    let path = std::env::args().nth(1).expect("abs path to js module");
    let rt = Runtime::new().unwrap();
    let ctx = Context::full(&rt).unwrap();
    ctx.with(|ctx| {
        let load = |ctx: &rbun::Ctx| -> String {
            let ns: Object = ctx.import(&path).unwrap().finish().unwrap();
            ns.get("value").unwrap()
        };
        std::fs::write(&path, "export const value = 'one';").unwrap();
        println!("first: {}", load(&ctx));
        std::fs::write(&path, "export const value = 'two';").unwrap();
        println!("cached: {}", load(&ctx));
        ctx.evict_module(&path);
        println!("evicted: {}", load(&ctx));
    });
}
