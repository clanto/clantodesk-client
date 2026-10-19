// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

use hbb_common::anyhow::{anyhow, Result};
use std::time::Duration;

pub(super) fn update(text: Option<&str>) {
    if let Err(err) = update_inner(text) {
        hbb_common::throttled_log!(
            Duration::from_secs(60),
            warn,
            "Failed to update the Android host watermark: {err}"
        );
    }
}

fn update_inner(text: Option<&str>) -> Result<()> {
    use jni::objects::{JObject, JValue};

    scrap::android::with_application_context(|jvm, context| {
        let mut env = jvm
            .attach_current_thread()
            .map_err(|err| anyhow!("failed to attach to the Android JVM: {err}"))?;
        let class = host_watermark_class(&mut env, context.as_obj())?;
        let text = match text {
            Some(text) => JObject::from(
                env.new_string(text)
                    .map_err(|err| android_jni_error(&mut env, "create the watermark text", err))?,
            ),
            None => JObject::null(),
        };
        env.call_static_method(
            class,
            "update",
            "(Landroid/content/Context;Ljava/lang/String;)V",
            &[JValue::Object(context.as_obj()), JValue::Object(&text)],
        )
        .map_err(|err| android_jni_error(&mut env, "update the Android host watermark", err))?;
        Ok(())
    })
    .ok_or_else(|| anyhow!("Android application context is not initialized"))?
}

fn host_watermark_class<'local>(
    env: &mut jni::JNIEnv<'local>,
    context: &jni::objects::JObject,
) -> Result<jni::objects::JClass<'local>> {
    use jni::objects::{JClass, JObject, JValue};

    let loader = env
        .call_method(context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
        .and_then(|value| value.l())
        .map_err(|err| android_jni_error(env, "get the Android application class loader", err))?;
    let name = JObject::from(
        env.new_string("it.clanto.clantodesk.clanto.ClantoHostWatermark")
            .map_err(|err| android_jni_error(env, "create the Android class name", err))?,
    );
    let class = env
        .call_method(
            loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&name)],
        )
        .and_then(|value| value.l())
        .map_err(|err| android_jni_error(env, "load the Android host watermark class", err))?;
    Ok(JClass::from(class))
}

fn android_jni_error(
    env: &mut jni::JNIEnv,
    action: &str,
    err: jni::errors::Error,
) -> hbb_common::anyhow::Error {
    match env.exception_clear() {
        Ok(()) => anyhow!("failed to {action}: {err}"),
        Err(clear_err) => {
            anyhow!("failed to {action}: {err}; failed to clear the Java exception: {clear_err}")
        }
    }
}
