//! Serde ⇄ JavaScript, mirroring `rquickjs_serde::{to_value, from_value}`.
//! Goes through JSON, which is also the representation rquickjs-serde
//! produces for JSON-representable data.

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::{Error, Result};
use crate::runtime::Ctx;
use crate::value::Value;

pub fn to_value<'js, T: Serialize>(ctx: Ctx<'js>, value: T) -> Result<Value<'js>> {
    let json = serde_json::to_string(&value)?;
    if json == "null" {
        return Ok(Value::new_null(ctx));
    }
    ctx.json_parse(json)
}

pub fn from_value<'js, T: DeserializeOwned>(value: Value<'js>) -> Result<T> {
    let json = value
        .to_json()?
        .ok_or_else(|| Error::new_from_js(value.type_name(), core::any::type_name::<T>()))?;
    Ok(serde_json::from_str(&json)?)
}
