//! JNI exposes only a metadata patch, never arbitrary filesystem writes.
use jni::{
    JNIEnv,
    objects::JClass,
    sys::{jint, jstring},
};
fn reply(env: JNIEnv, result: Result<serde_json::Value, String>) -> jstring {
    let v = result.unwrap_or_else(|e| serde_json::json!({"error":e}));
    env.new_string(v.to_string())
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_it_ckiikc_musiccraft_RatingTags_inspectNative(
    env: JNIEnv,
    _: JClass,
    fd: jint,
) -> jstring {
    let result = (|| {
        let mut f = clone_fd(fd).map_err(|_| "Cannot read audio metadata")?;
        Ok(serde_json::to_value(super::inspect(&mut f)).unwrap())
    })();
    reply(env, result)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_it_ckiikc_musiccraft_RatingTags_patchNative(
    env: JNIEnv,
    _: JClass,
    fd: jint,
    rating: jint,
    scale: jint,
) -> jstring {
    let result = (|| {
        let mut f = clone_fd(fd).map_err(|_| "Cannot read audio metadata")?;
        let scale = match scale {
            5 => super::Scale::Five,
            100 => super::Scale::Hundred,
            _ => return Err("Invalid rating scale".into()),
        };
        let rating = u8::try_from(rating).map_err(|_| "Invalid rating")?;
        let patch = super::patch(&mut f, rating, scale)?;
        Ok(serde_json::to_value(patch).unwrap())
    })();
    reply(env, result)
}

fn clone_fd(fd: i32) -> std::io::Result<std::fs::File> {
    use std::os::fd::BorrowedFd;
    if fd < 0 {
        return Err(std::io::ErrorKind::InvalidInput.into());
    }
    // Borrow only during this JNI call; dup retains Android's granted descriptor access.
    let borrowed = unsafe { BorrowedFd::borrow_raw(fd) };
    Ok(std::fs::File::from(borrowed.try_clone_to_owned()?))
}

/// The sync planner accepts JSON and returns a preview; JNI cannot open or modify a path.
#[unsafe(no_mangle)]
pub extern "system" fn Java_it_ckiikc_musiccraft_FolderSync_planNative(
    mut env: JNIEnv,
    _: JClass,
    input: jni::objects::JString,
) -> jstring {
    let output = env
        .get_string(&input)
        .map(|s| musiccraft_sync::json(&String::from(s)))
        .unwrap_or_else(|_| "{\"error\":\"Invalid sync request\"}".into());
    env.new_string(output)
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}
