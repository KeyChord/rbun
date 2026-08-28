//! Ported from rquickjs-core `src/value/typed_array.rs` and
//! `src/value/array_buffer.rs`.

mod common;

use common::test_with;
use rbun::{ArrayBuffer, Object, TypedArray, Value};

#[test]
fn typed_from_javascript_i8() {
    test_with(|ctx| {
        let val: TypedArray<i8> = ctx.eval("new Int8Array([0, -5, 1, 11])").unwrap();
        assert_eq!(val.len(), 4);
        assert_eq!(val.as_ref() as &[i8], &[0i8, -5, 1, 11]);
    });
}

#[test]
fn typed_into_javascript_i8() {
    test_with(|ctx| {
        let val = TypedArray::<i8>::new(ctx.clone(), [-1i8, 0, 22, 5]).unwrap();
        ctx.globals().set("v_i8", val).unwrap();
        let res: i8 = ctx
            .eval(
                r#"
                    v_i8.length != 4 ? 1 :
                    v_i8[0] != -1 ? 2 :
                    v_i8[1] != 0 ? 3 :
                    v_i8[2] != 22 ? 4 :
                    v_i8[3] != 5 ? 5 :
                    0
                "#,
            )
            .unwrap();
        assert_eq!(res, 0);
    })
}

#[test]
fn typed_from_javascript_f32() {
    test_with(|ctx| {
        let val: TypedArray<f32> = ctx.eval("new Float32Array([0.5, -5.25, 123.125])").unwrap();
        assert_eq!(val.len(), 3);
        assert_eq!(val.as_ref() as &[f32], &[0.5, -5.25, 123.125]);
    });
}

#[test]
fn typed_into_javascript_f32() {
    test_with(|ctx| {
        let val = TypedArray::<f32>::new(ctx.clone(), [-1.5, 0.0, 2.25]).unwrap();
        ctx.globals().set("v_f32", val).unwrap();
        let res: i8 = ctx
            .eval(
                r#"
                    v_f32.length != 3 ? 1 :
                    v_f32[0] != -1.5 ? 2 :
                    v_f32[1] != 0 ? 3 :
                    v_f32[2] != 2.25 ? 4 :
                    0
                "#,
            )
            .unwrap();
        assert_eq!(res, 0);
    })
}

#[test]
fn typed_as_bytes() {
    test_with(|ctx| {
        let val: TypedArray<u32> = ctx.eval("new Uint32Array([0xCAFEDEAD,0xFEEDBEAD])").unwrap();
        let mut res = [0; 8];
        res[..4].copy_from_slice(&0xCAFEDEADu32.to_ne_bytes());
        res[4..].copy_from_slice(&0xFEEDBEADu32.to_ne_bytes());
        assert_eq!(val.as_bytes().unwrap(), &res)
    });
}

#[test]
fn is_typed_array() {
    test_with(|ctx| {
        let val: Value = ctx.eval("new Uint32Array([0xCAFEDEAD, 0xFEEDBEAD])").unwrap();
        let obj = val.into_object().unwrap();
        assert!(obj.is_typed_array::<u32>());
        assert!(!obj.is_typed_array::<i32>());

        let obj = Object::new(ctx).unwrap();
        assert!(!obj.is_typed_array::<u8>());
    });
}

// ─── array_buffer.rs ───

#[test]
fn buffer_from_javascript_i8() {
    test_with(|ctx| {
        let val: ArrayBuffer = ctx.eval("new Int8Array([0, -5, 1, 11]).buffer").unwrap();
        assert_eq!(val.len(), 4);
        assert_eq!(val.as_ref() as &[i8], &[0i8, -5, 1, 11]);
    });
}

#[test]
fn buffer_into_javascript_i8() {
    test_with(|ctx| {
        let val = ArrayBuffer::new(ctx.clone(), [-1i8, 0, 22, 5]).unwrap();
        ctx.globals().set("a_i8", val).unwrap();
        let res: i8 = ctx
            .eval(
                r#"
                    let va = new Int8Array(a_i8);
                    va.length != 4 ? 1 :
                    va[0] != -1 ? 2 :
                    va[1] != 0 ? 3 :
                    va[2] != 22 ? 4 :
                    va[3] != 5 ? 5 :
                    0
                "#,
            )
            .unwrap();
        assert_eq!(res, 0);
    })
}

#[test]
fn buffer_from_javascript_f32() {
    test_with(|ctx| {
        let val: ArrayBuffer = ctx.eval("new Float32Array([0.5, -5.25, 123.125]).buffer").unwrap();
        assert_eq!(val.len(), 12);
        assert_eq!(val.as_ref() as &[f32], &[0.5f32, -5.25, 123.125]);
    });
}

#[test]
fn buffer_into_javascript_f32() {
    test_with(|ctx| {
        let val = ArrayBuffer::new(ctx.clone(), [-1.5f32, 0.0, 2.25]).unwrap();
        ctx.globals().set("a_f32", val).unwrap();
        let res: i8 = ctx
            .eval(
                r#"
                    let vb = new Float32Array(a_f32);
                    a_f32.byteLength != 12 ? 1 :
                    vb.length != 3 ? 2 :
                    vb[0] != -1.5 ? 3 :
                    vb[1] != 0 ? 4 :
                    vb[2] != 2.25 ? 5 :
                    0
                "#,
            )
            .unwrap();
        assert_eq!(res, 0);
    })
}

#[test]
fn buffer_as_bytes() {
    test_with(|ctx| {
        let val: ArrayBuffer = ctx.eval("new Uint32Array([0xCAFEDEAD,0xFEEDBEAD]).buffer").unwrap();
        let mut res = [0; 8];
        res[..4].copy_from_slice(&0xCAFEDEADu32.to_ne_bytes());
        res[4..].copy_from_slice(&0xFEEDBEADu32.to_ne_bytes());
        assert_eq!(val.as_bytes().unwrap(), &res)
    });
}
