//! Ported from rquickjs-core `src/value/object/property.rs`.
//!
//! Error texts come from JavaScriptCore ("Attempted to assign to readonly
//! property.") rather than QuickJS ("'key' is read-only").

mod common;

use common::test_with;
use rbun::object::{Accessor, Filter, Property};
use rbun::{CatchResultExt, Error, Exception, FromJs, Object};
use std::cell::RefCell;
use std::rc::Rc;
use std::string::String as StdString;

#[test]
fn property_with_undefined() {
    test_with(|ctx| {
        let obj = Object::new(ctx.clone()).unwrap();
        obj.prop("key", ()).unwrap();

        let _: () = obj.get("key").unwrap();

        if let Err(Error::Exception) = obj.set("key", "") {
            let exception = Exception::from_js(&ctx, ctx.catch()).unwrap();
            assert!(exception.message().unwrap_or_default().contains("readonly"));
        } else {
            panic!("Should fail");
        }
    });
}

#[test]
fn property_with_value() {
    test_with(|ctx| {
        let obj = Object::new(ctx.clone()).unwrap();
        obj.prop("key", "str").unwrap();

        let s: StdString = obj.get("key").unwrap();
        assert_eq!(s, "str");

        if let Err(Error::Exception) = obj.set("key", "") {
            let exception = Exception::from_js(&ctx, ctx.catch()).unwrap();
            assert!(exception.message().unwrap_or_default().contains("readonly"));
        } else {
            panic!("Should fail");
        }
    });
}

#[test]
fn property_with_data_descriptor() {
    test_with(|ctx| {
        let obj = Object::new(ctx).unwrap();
        obj.prop("key", Property::from("str")).unwrap();

        let s: StdString = obj.get("key").unwrap();
        assert_eq!(s, "str");
    });
}

#[test]
#[should_panic(expected = "readonly")]
fn property_with_data_descriptor_readonly() {
    test_with(|ctx| {
        let obj = Object::new(ctx.clone()).unwrap();
        obj.prop("key", Property::from("str")).unwrap();
        obj.set("key", "text").catch(&ctx).map_err(|error| panic!("{}", error)).unwrap();
    });
}

#[test]
fn property_with_data_descriptor_writable() {
    test_with(|ctx| {
        let obj = Object::new(ctx).unwrap();
        obj.prop("key", Property::from("str").writable()).unwrap();
        obj.set("key", "text").unwrap();
    });
}

#[test]
#[should_panic(expected = "readonly")]
fn property_with_data_descriptor_not_configurable() {
    test_with(|ctx| {
        let obj = Object::new(ctx.clone()).unwrap();
        obj.prop("key", Property::from("str")).unwrap();
        obj.prop("key", Property::from(39)).catch(&ctx).map_err(|error| panic!("{}", error)).unwrap();
    });
}

#[test]
fn property_with_data_descriptor_configurable() {
    test_with(|ctx| {
        let obj = Object::new(ctx).unwrap();
        obj.prop("key", Property::from("str").configurable()).unwrap();
        obj.prop("key", Property::from(39)).unwrap();
    });
}

#[test]
fn property_with_data_descriptor_not_enumerable() {
    test_with(|ctx| {
        let obj = Object::new(ctx).unwrap();
        obj.prop("key", Property::from("str")).unwrap();
        let keys: Vec<StdString> = obj.own_keys(Filter::new().string()).collect::<rbun::Result<_>>().unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(&keys[0], "key");
        let keys: Vec<StdString> = obj.keys().collect::<rbun::Result<_>>().unwrap();
        assert_eq!(keys.len(), 0);
    });
}

#[test]
fn property_with_data_descriptor_enumerable() {
    test_with(|ctx| {
        let obj = Object::new(ctx).unwrap();
        obj.prop("key", Property::from("str").enumerable()).unwrap();
        let keys: Vec<StdString> = obj.keys().collect::<rbun::Result<_>>().unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(&keys[0], "key");
    });
}

#[test]
fn property_with_getter_only() {
    test_with(|ctx| {
        let obj = Object::new(ctx.clone()).unwrap();
        obj.prop("key", Accessor::from(|| "str")).unwrap();

        let s: StdString = obj.get("key").unwrap();
        assert_eq!(s, "str");

        if let Err(Error::Exception) = obj.set("key", "") {
            let exception = Exception::from_js(&ctx, ctx.catch()).unwrap();
            assert!(exception.message().unwrap_or_default().contains("readonly"));
        } else {
            panic!("Should fail");
        }
    });
}

#[test]
fn property_with_getter_and_setter() {
    test_with(|ctx| {
        let val = Rc::new(RefCell::new(StdString::new()));
        let obj = Object::new(ctx).unwrap();
        obj.prop(
            "key",
            Accessor::from({
                let val = val.clone();
                move || val.borrow().clone()
            })
            .set({
                let val = val.clone();
                move |s: StdString| {
                    *val.borrow_mut() = s;
                }
            }),
        )
        .unwrap();

        let s: StdString = obj.get("key").unwrap();
        assert_eq!(s, "");

        obj.set("key", "str").unwrap();
        assert_eq!(val.borrow().clone(), "str");

        let s: StdString = obj.get("key").unwrap();
        assert_eq!(s, "str");

        obj.set("key", "").unwrap();
        let s: StdString = obj.get("key").unwrap();
        assert_eq!(s, "");
        assert_eq!(val.borrow().clone(), "");
    });
}
