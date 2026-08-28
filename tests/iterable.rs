//! Ported from rquickjs-core `src/value/iterable.rs`.

mod common;

use common::test_with;
use rbun::{Array, Iterable, JsIterator, Object, Value};

#[test]
fn iterable_spread() {
    test_with(|ctx| {
        let iter = Iterable::from(vec![1i32, 2, 3]);
        ctx.globals().set("myIter", iter).unwrap();
        let result: Vec<i32> = ctx.eval("[...myIter]").unwrap();
        assert_eq!(result, vec![1, 2, 3]);
    });
}

#[test]
fn iterable_for_of() {
    test_with(|ctx| {
        let iter = Iterable::from(vec!["a", "b", "c"]);
        ctx.globals().set("myIter2", iter).unwrap();
        let result: String = ctx
            .eval(
                r#"
            let s_forof = "";
            for (const x of myIter2) { s_forof += x; }
            s_forof
        "#,
            )
            .unwrap();
        assert_eq!(result, "abc");
    });
}

#[test]
fn iterable_from_range() {
    test_with(|ctx| {
        let iter = Iterable::from(0..5);
        ctx.globals().set("myIter3", iter).unwrap();
        let result: Vec<i32> = ctx.eval("[...myIter3]").unwrap();
        assert_eq!(result, vec![0, 1, 2, 3, 4]);
    });
}

#[test]
fn iterable_single_use() {
    test_with(|ctx| {
        let iter = Iterable::from(vec![1i32, 2]);
        ctx.globals().set("myIter4", iter).unwrap();
        let first: Vec<i32> = ctx.eval("[...myIter4]").unwrap();
        assert_eq!(first, vec![1, 2]);
        let second: Vec<i32> = ctx.eval("[...myIter4]").unwrap();
        assert_eq!(second, Vec::<i32>::new());
    });
}

#[test]
fn js_iter_from_array() {
    test_with(|ctx| {
        let iter: JsIterator<i32> = ctx.eval("[1, 2, 3][Symbol.iterator]()").unwrap();
        let values: Vec<i32> = iter.filter_map(|r| r.ok()).collect();
        assert_eq!(values, vec![1, 2, 3]);
    });
}

#[test]
fn js_iter_from_iterable() {
    test_with(|ctx| {
        let iter: JsIterator<i32> = ctx.eval("[4, 5, 6]").unwrap();
        let values: Vec<i32> = iter.filter_map(|r| r.ok()).collect();
        assert_eq!(values, vec![4, 5, 6]);
    });
}

#[test]
fn js_iter_from_generator() {
    test_with(|ctx| {
        let iter: JsIterator<i32> = ctx
            .eval(
                r#"
            (function*() {
                yield 10;
                yield 20;
                yield 30;
            })()
        "#,
            )
            .unwrap();
        let values: Vec<i32> = iter.filter_map(|r| r.ok()).collect();
        assert_eq!(values, vec![10, 20, 30]);
    });
}

#[test]
fn js_iter_roundtrip() {
    test_with(|ctx| {
        let rust_iter = Iterable::from(vec![100i32, 200, 300]);
        ctx.globals().set("myIter5", rust_iter).unwrap();
        let js_iter: JsIterator<i32> = ctx.eval("myIter5").unwrap();
        let values: Vec<i32> = js_iter.filter_map(|r| r.ok()).collect();
        assert_eq!(values, vec![100, 200, 300]);
    });
}

#[test]
fn js_iter_raw_values() {
    test_with(|ctx| {
        let iter: JsIterator<Value> = ctx.eval("[1, 'two', 3]").unwrap();
        let values: Vec<Value> = iter.filter_map(|r| r.ok()).collect();
        assert_eq!(values.len(), 3);
        assert!(values[0].is_int());
        assert!(values[1].is_string());
        assert!(values[2].is_int());
    });
}

#[test]
fn js_iter_typed_conversion() {
    test_with(|ctx| {
        let iter: JsIterator<Value> = ctx.eval("[1, 2, 3]").unwrap();
        let typed = iter.typed::<i32>();
        let values: Vec<i32> = typed.filter_map(|r| r.ok()).collect();
        assert_eq!(values, vec![1, 2, 3]);
    });
}

#[test]
fn js_iter_strings() {
    test_with(|ctx| {
        let iter: JsIterator<String> = ctx.eval("['hello', 'world', 'rust']").unwrap();
        let values: Vec<String> = iter.filter_map(|r| r.ok()).collect();
        assert_eq!(values, vec!["hello", "world", "rust"]);
    });
}

#[test]
fn js_iter_floats() {
    test_with(|ctx| {
        let iter: JsIterator<f64> = ctx.eval("[1.5, 2.7, 3.54]").unwrap();
        let values: Vec<f64> = iter.filter_map(|r| r.ok()).collect();
        assert_eq!(values, vec![1.5, 2.7, 3.54]);
    });
}

#[test]
fn js_iter_bools() {
    test_with(|ctx| {
        let iter: JsIterator<bool> = ctx.eval("[true, false, true]").unwrap();
        let values: Vec<bool> = iter.filter_map(|r| r.ok()).collect();
        assert_eq!(values, vec![true, false, true]);
    });
}

#[test]
fn js_iter_objects() {
    test_with(|ctx| {
        let iter: JsIterator<Object> = ctx.eval("[{a: 1}, {b: 2}]").unwrap();
        let objects: Vec<Object> = iter.filter_map(|r| r.ok()).collect();
        assert_eq!(objects.len(), 2);
        assert_eq!(objects[0].get::<_, i32>("a").unwrap(), 1);
        assert_eq!(objects[1].get::<_, i32>("b").unwrap(), 2);
    });
}

#[test]
fn js_iter_map_entries() {
    test_with(|ctx| {
        let iter: JsIterator<Array> = ctx.eval("new Map([['a', 1], ['b', 2]]).entries()").unwrap();
        let entries: Vec<Array> = iter.filter_map(|r| r.ok()).collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].get::<String>(0).unwrap(), "a");
        assert_eq!(entries[0].get::<i32>(1).unwrap(), 1);
    });
}

#[test]
fn js_iter_set() {
    test_with(|ctx| {
        let iter: JsIterator<i32> = ctx.eval("new Set([1, 2, 3])").unwrap();
        let values: Vec<i32> = iter.filter_map(|r| r.ok()).collect();
        assert_eq!(values, vec![1, 2, 3]);
    });
}
