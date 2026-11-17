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

// ---------- starting emulator apps ----------

fn call_string(method: &str, sig: &str, args: &[&str]) -> Result<String, String> {
    with_activity(|env, act| {
        let strings: Vec<JObject> = args.iter().map(|a| env.new_string(a).map(JObject::from)).collect::<Result<_, _>>()?;
        let vals: Vec<JValue> = strings.iter().map(JValue::Object).collect();
        let r = env.call_method(act, method, sig, &vals)?.l()?;
        string(env, r)
    })
    .map(Option::unwrap_or_default)
}

fn json_list(json: &str) -> Vec<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(json).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default()
}

/// Every app's package.
fn installed() -> Vec<String> {
    json_list(&call_string("apps", "()Ljava/lang/String;", &[]).unwrap_or_default()).iter().filter_map(|a| a["pkg"].as_str().map(String::from)).collect()
}

fn activities(pkg: &str) -> Vec<String> {
    json_list(&call_string("activities", "(Ljava/lang/String;)Ljava/lang/String;", &[pkg]).unwrap_or_default()).iter().filter_map(|a| a.as_str().map(String::from)).collect()
}

static STARTED: std::sync::Mutex<Option<(String, std::sync::mpsc::Receiver<Result<(), String>>)>> = std::sync::Mutex::new(None);

/// Starts a console's game in its emulator app, off the frame; `poll` tells how it went.
pub fn start(console: &rustyshelf_core::systems::Console, sc: &rustyshelf_core::config::SystemConfig, game: &rustyshelf_core::scanner::Game) {
    let (tx, rx) = std::sync::mpsc::channel();
    let (console, sc, game) = (console.clone(), sc.clone(), game.clone());
    let id = game.id.clone();
    std::thread::spawn(move || {
        let r = rustyshelf_core::android::launch_request(&console, &sc, &game, &installed(), &activities).and_then(|req| {
            let said = call_string("launch", "(Ljava/lang/String;)Ljava/lang/String;", &[&serde_json::to_string(&req).unwrap_or_default()])?;
            if said.is_empty() { Ok(()) } else { Err(said) }
        });
        let _ = tx.send(r);
    });
    *STARTED.lock().unwrap() = Some((id, rx));
}

/// A game started (its launch counted) or why it didn't.
pub fn poll(mut toast: bevy::prelude::ResMut<crate::hud::Toast>, mut plays: bevy::prelude::ResMut<crate::play::Plays>, mut sounds: bevy::prelude::ResMut<crate::sound::Sounds>, time: bevy::prelude::Res<bevy::prelude::Time>) {
    let mut started = STARTED.lock().unwrap();
    let Some(r) = started.as_ref().and_then(|(_, rx)| rx.try_recv().ok()) else { return };
    let (id, _) = started.take().unwrap();
    match r {
        Ok(()) => plays.0.touch(&id),
        Err(why) => {
            toast.show(why, time.elapsed_secs(), 6.0);
            sounds.play("error", 1);
        }
    }
}
