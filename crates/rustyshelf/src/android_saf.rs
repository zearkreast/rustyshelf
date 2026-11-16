//! Android: games folders through the Storage Access Framework, by calling MainActivity (JNI): its folder picker, and
//! its listing of a picked folder for rustyshelf_core::android.

use jni::objects::{JObject, JString, JValue};
use jni::JavaVM;
use rustyshelf_core::android::DocFile;

/// Runs `f` with the activity, on this thread attached to Java for the call.
fn with_activity<R>(f: impl FnOnce(&mut jni::JNIEnv, &JObject) -> jni::errors::Result<R>) -> Result<R, String> {
    let ctx = ndk_context::android_context();
    // SAFETY: android-activity filled ndk-context with the process's JavaVM and the activity before the app started
    let vm = unsafe { JavaVM::from_raw(ctx.vm().cast()) }.map_err(|e| e.to_string())?;
    let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
    let activity = unsafe { JObject::from_raw(ctx.context().cast()) };
    let r = f(&mut env, &activity);
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
    r.map_err(|e| e.to_string())
}

fn string(env: &mut jni::JNIEnv, o: JObject) -> jni::errors::Result<Option<String>> {
    if o.is_null() {
        return Ok(None);
    }
    Ok(Some(env.get_string(&JString::from(o))?.into()))
}

/// rustyshelf_core::android's Lister.
pub fn list(tree: &str, exts: &[String], depth: u32) -> Result<Vec<DocFile>, String> {
    let json = with_activity(|env, act| {
        let (t, e) = (env.new_string(tree)?, env.new_string(exts.join(","))?);
        let r = env.call_method(act, "listFiles", "(Ljava/lang/String;Ljava/lang/String;I)Ljava/lang/String;", &[JValue::Object(&t), JValue::Object(&e), JValue::Int(depth as i32)])?.l()?;
        string(env, r)
    })?
    .unwrap_or_default();
    if let Some(why) = json.strip_prefix('!') {
        return Err(why.to_string());
    }
    let v: serde_json::Value = serde_json::from_str(&json).map_err(|e| e.to_string())?;
    let s = |f: &serde_json::Value, k: &str| f[k].as_str().unwrap_or("").to_string();
    Ok(v.as_array()
        .into_iter()
        .flatten()
        .map(|f| DocFile { name: s(f, "name"), uri: s(f, "uri"), doc_id: s(f, "docId"), size: f["size"].as_u64().unwrap_or(0), mtime: f["mtime"].as_u64().unwrap_or(0) })
        .collect())
}

/// Android's folder picker, waited for (on a thread of its own): the picked folder's tree address.
pub fn pick_folder() -> Option<String> {
    with_activity(|env, act| env.call_method(act, "pickFolder", "()V", &[]).map(|_| ())).ok()?;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(200));
        let answer = with_activity(|env, act| {
            let r = env.call_method(act, "takePicked", "()Ljava/lang/String;", &[])?.l()?;
            string(env, r)
        })
        .ok()?;
        if let Some(a) = answer {
            return Some(a).filter(|a| !a.is_empty());
        }
    }
}
