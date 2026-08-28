//! Ported from rquickjs-core `src/value/{object,array,string,bigint,symbol}.rs`,
//! `src/value.rs`, `src/value/convert/{into,from}.rs` and
//! `src/value/atom/predefined.rs`.

mod common;

use common::test_with;
use rbun::prelude::*;
use rbun::{Array, Atom, BigInt, Context, Function, IntoAtom, Object, PredefinedAtom, Runtime, String as JsString, Symbol, Type, Value};
use std::string::String as StdString;

// ─── object.rs ───

#[test]
fn object_from_javascript() {
    test_with(|ctx| {
        let val: Object = ctx
            .eval(
                r#"
            let obj = {};
            obj['a'] = 3;
            obj[3] = 'a';
            obj
        "#,
            )
            .unwrap();

        let text: StdString = val.get(3).unwrap();
        assert_eq!(text, "a");
        let int: i32 = val.get("a").unwrap();
        assert_eq!(int, 3);
        let int: StdString = val.get(3).unwrap();
        assert_eq!(int, "a");
        val.set("hallo", "foo").unwrap();
        let text: StdString = val.get("hallo").unwrap();
        assert_eq!(text, "foo".to_string());
        val.remove("hallo").unwrap();
        let text: Option<StdString> = val.get("hallo").unwrap();
        assert_eq!(text, None);
    });
}

#[test]
fn object_types() {
    test_with(|ctx| {
        let val: Object = ctx
            .eval(
                r#"
            let array_3 = [];
            array_3[3] = "foo";
            array_3[99] = 4;
            ({
                array_1: [0,1,2,3,4,5],
                array_2: [0,"foo",{},undefined,4,5],
                array_3: array_3,
                func_1: () => 1,
                func_2: function(){ return "foo"},
                obj_1: {
                    a: 1,
                    b: "foo",
                },
            })
            "#,
            )
            .unwrap();
        assert!(val.get::<_, Object>("array_1").unwrap().is_array());
        assert!(val.get::<_, Object>("array_2").unwrap().is_array());
        assert!(val.get::<_, Object>("array_3").unwrap().is_array());
        assert!(val.get::<_, Object>("func_1").unwrap().is_function());
        assert!(val.get::<_, Object>("func_2").unwrap().is_function());
        assert!(!val.get::<_, Object>("obj_1").unwrap().is_function());
        assert!(!val.get::<_, Object>("obj_1").unwrap().is_array());
    })
}

#[test]
fn own_keys_iter() {
    test_with(|ctx| {
        let val: Object = ctx
            .eval(
                r#"
               ({
                 123: 123,
                 str: "abc",
                 arr: [],
                 '': undefined,
               })
            "#,
            )
            .unwrap();
        let keys = val.keys().collect::<rbun::Result<Vec<StdString>>>().unwrap();
        assert_eq!(keys.len(), 4);
        assert_eq!(keys[0], "123");
        assert_eq!(keys[1], "str");
        assert_eq!(keys[2], "arr");
        assert_eq!(keys[3], "");
    })
}

#[test]
fn own_props_iter() {
    test_with(|ctx| {
        let val: Object = ctx
            .eval(
                r#"
               ({
                 123: "",
                 str: "abc",
                 '': "def",
               })
            "#,
            )
            .unwrap();
        let pairs = val.props().collect::<rbun::Result<Vec<(StdString, StdString)>>>().unwrap();
        assert_eq!(pairs.len(), 3);
        assert_eq!(pairs[0].0, "123");
        assert_eq!(pairs[0].1, "");
        assert_eq!(pairs[1].0, "str");
        assert_eq!(pairs[1].1, "abc");
        assert_eq!(pairs[2].0, "");
        assert_eq!(pairs[2].1, "def");
    })
}

#[test]
fn object_into_iter() {
    test_with(|ctx| {
        let val: Object = ctx
            .eval(
                r#"
               ({
                 123: 123,
                 str: "abc",
                 arr: [],
                 '': undefined,
               })
            "#,
            )
            .unwrap();
        let pairs = val.into_iter().collect::<rbun::Result<Vec<_>>>().unwrap();
        assert_eq!(pairs.len(), 4);
        assert_eq!(pairs[0].0.clone().to_string().unwrap(), "123");
        assert_eq!(i32::from_js(&ctx, pairs[0].1.clone()).unwrap(), 123);
        assert_eq!(pairs[1].0.clone().to_string().unwrap(), "str");
        assert_eq!(StdString::from_js(&ctx, pairs[1].1.clone()).unwrap(), "abc");
        assert_eq!(pairs[2].0.clone().to_string().unwrap(), "arr");
        assert_eq!(Array::from_js(&ctx, pairs[2].1.clone()).unwrap().len(), 0);
        assert_eq!(pairs[3].0.clone().to_string().unwrap(), "");
        assert_eq!(Undefined::from_js(&ctx, pairs[3].1.clone()).unwrap(), Undefined);
    })
}

#[test]
fn iter_take() {
    test_with(|ctx| {
        let val: Object = ctx
            .eval(
                r#"
               ({
                 123: 123,
                 str: "abc",
                 arr: [],
                 '': undefined,
               })
            "#,
            )
            .unwrap();
        let keys = val.keys().take(1).collect::<rbun::Result<Vec<StdString>>>().unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0], "123");
    })
}

#[test]
fn object_collect_js() {
    test_with(|ctx| {
        let object = [("a", "bc"), ("$_", ""), ("", "xyz")]
            .iter()
            .cloned()
            .collect_js::<Object>(&ctx)
            .unwrap();
        assert_eq!(StdString::from_js(&ctx, object.get("a").unwrap()).unwrap(), "bc");
        assert_eq!(StdString::from_js(&ctx, object.get("$_").unwrap()).unwrap(), "");
        assert_eq!(StdString::from_js(&ctx, object.get("").unwrap()).unwrap(), "xyz");
    })
}

// ─── array.rs ───

#[test]
fn array_from_javascript() {
    test_with(|ctx| {
        let val: Array = ctx
            .eval(
                r#"
            let a = [1,2,3,4,10,"b"]
            a[6] = {}
            a[10] = () => {"hallo"};
            a
            "#,
            )
            .unwrap();
        assert_eq!(val.len(), 11);
        assert_eq!(val.get::<i32>(3).unwrap(), 4);
        assert_eq!(val.get::<i32>(4).unwrap(), 10);
        let _six: Object = val.get(6).unwrap();
    });
}

#[test]
fn array_into_object() {
    test_with(|ctx| {
        let val: Array = ctx
            .eval(
                r#"
            let a2 = [1,2,3];
            a2
        "#,
            )
            .unwrap();
        let object = val.into_object();
        assert_eq!(object.get::<_, i32>(0).unwrap(), 1);
    })
}

#[test]
fn array_into_iter() {
    test_with(|ctx| {
        let val: Array = ctx
            .eval(
                r#"
                  [1,'abcd',true]
                "#,
            )
            .unwrap();
        let elems: Vec<_> = val.into_iter().collect::<rbun::Result<_>>().unwrap();
        assert_eq!(elems.len(), 3);
        assert_eq!(i8::from_js(&ctx, elems[0].clone()).unwrap(), 1);
        assert_eq!(StdString::from_js(&ctx, elems[1].clone()).unwrap(), "abcd");
        assert!(bool::from_js(&ctx, elems[2].clone()).unwrap());
    })
}

#[test]
fn array_iter() {
    test_with(|ctx| {
        let val: Array = ctx
            .eval(
                r#"
                  ["a", 'b', '', "cdef"]
                "#,
            )
            .unwrap();
        let elems: Vec<StdString> = val.iter().collect::<rbun::Result<_>>().unwrap();
        assert_eq!(elems.len(), 4);
        assert_eq!(elems[0], "a");
        assert_eq!(elems[1], "b");
        assert_eq!(elems[2], "");
        assert_eq!(elems[3], "cdef");
    })
}

#[test]
fn array_collect_js() {
    test_with(|ctx| {
        let array = [1i32, 2, 3].iter().cloned().collect_js::<Array>(&ctx).unwrap();
        assert_eq!(array.len(), 3);
        assert_eq!(i32::from_js(&ctx, array.get(0).unwrap()).unwrap(), 1);
        assert_eq!(i32::from_js(&ctx, array.get(1).unwrap()).unwrap(), 2);
        assert_eq!(i32::from_js(&ctx, array.get(2).unwrap()).unwrap(), 3);
    })
}

// ─── string.rs ───

#[test]
fn string_from_javascript() {
    test_with(|ctx| {
        let s: JsString = ctx.eval(" 'foo bar baz' ").unwrap();
        assert_eq!(s.to_string().unwrap(), "foo bar baz");
    });
}

#[test]
fn string_to_javascript() {
    test_with(|ctx| {
        let string = JsString::from_str(ctx.clone(), "foo").unwrap();
        let func: Function = ctx.eval("x =>  x + 'bar'").unwrap();
        let text: StdString = (string,).apply(&func).unwrap();
        assert_eq!(text, "foobar".to_string());
    });
}

// ─── value.rs ───

#[test]
fn type_matches() {
    assert!(Type::Bool.interpretable_as(Type::Bool));

    assert!(Type::Object.interpretable_as(Type::Object));
    assert!(Type::Array.interpretable_as(Type::Object));
    assert!(Type::Function.interpretable_as(Type::Object));

    assert!(!Type::Object.interpretable_as(Type::Array));
    assert!(!Type::Object.interpretable_as(Type::Function));

    assert!(!Type::Bool.interpretable_as(Type::Int));
}

#[test]
fn big_int() {
    test_with(|ctx| {
        let val: Value = ctx.eval(r#"1n"#).unwrap();
        assert_eq!(val.type_of(), Type::BigInt);
        let val: Value = ctx.eval(r#"999999999999999999999n"#).unwrap();
        assert_eq!(val.type_of(), Type::BigInt);
        let val = Value::new_big_int(ctx.clone(), 1245);
        assert_eq!(val.type_of(), Type::BigInt);
        let val = Value::new_big_int(ctx, 9999999999999999);
        assert_eq!(val.type_of(), Type::BigInt);
    });
}

// ─── bigint.rs ───

#[test]
fn bigint_from_javascript() {
    test_with(|ctx| {
        let s: BigInt = ctx.eval(format!("{}n", i64::MAX)).unwrap();
        assert_eq!(s.to_i64().unwrap(), i64::MAX);
    })
}

#[test]
fn bigint_to_javascript() {
    test_with(|ctx| {
        let bigint = BigInt::from_i64(ctx.clone(), i64::MAX).unwrap();
        let func: Function = ctx
            .eval(format!(
                "x => {{
            if( x != {}n){{
                throw 'error'
            }}
        }}",
                i64::MAX
            ))
            .unwrap();
        func.call::<_, ()>((bigint,)).unwrap();
    })
}

// ─── symbol.rs ───

#[test]
fn symbol_description() {
    test_with(|ctx| {
        let s: Symbol<'_> = ctx.eval("Symbol('foo bar baz')").unwrap();
        assert_eq!(
            s.description().unwrap().into_string().unwrap().to_string().unwrap(),
            "foo bar baz"
        );

        let s: Symbol<'_> = ctx.eval("Symbol()").unwrap();
        assert!(s.description().unwrap().is_undefined());
    });
}

// ─── convert/into.rs ───

#[test]
fn char_to_js() {
    common::js_thread(|| {
        let runtime = Runtime::new().unwrap();
        let ctx = Context::full(&runtime).unwrap();

        let c = 'a';

        ctx.with(|ctx| {
            let globs = ctx.globals();
            globs.set("char", c.into_js(&ctx).unwrap()).unwrap();
            let res: char = ctx.eval("globalThis.char").unwrap();
            assert_eq!(c, res);

            let rt = ctx.eval::<char, _>("''");
            assert!(rt.is_err());
            let rt = ctx.eval::<char, _>("'a'");
            assert!(rt.is_ok());
            let rt = ctx.eval::<char, _>("'ab'");
            assert!(rt.is_err());
        });
    })
}

#[test]
fn system_time_to_js() {
    use std::time::{Duration, SystemTime};

    common::js_thread(|| {
        let runtime = Runtime::new().unwrap();
        let ctx = Context::full(&runtime).unwrap();

        let ts = SystemTime::now();
        let millis = ts.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis();

        ctx.with(|ctx| {
            let globs = ctx.globals();
            globs.set("ts", ts.into_js(&ctx).unwrap()).unwrap();
            let res: i64 = ctx.eval("ts.getTime()").unwrap();
            assert_eq!(millis as i64, res);
        });

        let ts = SystemTime::UNIX_EPOCH - Duration::from_millis(123456);
        let millis = SystemTime::UNIX_EPOCH.duration_since(ts).unwrap().as_millis();

        ctx.with(|ctx| {
            let globs = ctx.globals();
            globs.set("ts", ts.into_js(&ctx).unwrap()).unwrap();
            let res: i64 = ctx.eval("ts.getTime()").unwrap();
            assert_eq!(-(millis as i64), res);
        });
    })
}

// ─── convert/from.rs ───

#[test]
fn js_to_system_time() {
    use std::time::{Duration, SystemTime};

    common::js_thread(|| {
        let runtime = Runtime::new().unwrap();
        let ctx = Context::full(&runtime).unwrap();

        ctx.with(|ctx| {
            let res: SystemTime = ctx.eval("new Date(123456789)").unwrap();
            assert_eq!(Duration::from_millis(123456789), res.duration_since(SystemTime::UNIX_EPOCH).unwrap());

            let res: SystemTime = ctx.eval("new Date(-123456789)").unwrap();
            assert_eq!(Duration::from_millis(123456789), SystemTime::UNIX_EPOCH.duration_since(res).unwrap());
        });
    })
}

// ─── atom/predefined.rs ───

#[test]
fn predefined_atoms() {
    test_with(|ctx| {
        static ALL_PREDEFS: &[PredefinedAtom] = &[
            PredefinedAtom::Null,
            PredefinedAtom::Length,
            PredefinedAtom::Message,
            PredefinedAtom::Stack,
            PredefinedAtom::Name,
            PredefinedAtom::ToString,
            PredefinedAtom::ValueOf,
            PredefinedAtom::Prototype,
            PredefinedAtom::Constructor,
            PredefinedAtom::Then,
            PredefinedAtom::Empty,
        ];
        for predef in ALL_PREDEFS {
            let atom = Atom::from_predefined(ctx.clone(), *predef);
            assert_eq!(atom.to_string().unwrap(), predef.as_str());
            let atom2 = (*predef).into_atom(&ctx).unwrap();
            assert_eq!(atom2.to_string().unwrap(), predef.as_str());
        }
        let obj = Object::new(ctx.clone()).unwrap();
        obj.set(PredefinedAtom::Length, 3).unwrap();
        assert_eq!(obj.get::<_, i32>("length").unwrap(), 3);
    })
}
