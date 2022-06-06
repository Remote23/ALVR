use alvr_common::prelude::*;
use alvr_session::{CodecType, MediacodecDataType};
use jni::{
    objects::{GlobalRef, JObject},
    sys::jobject,
    AttachGuard, JNIEnv, JavaVM,
};
use ndk::{
    media::media_codec::{MediaCodec, MediaCodecDirection, MediaFormat},
    native_window::NativeWindow,
};
use ndk_sys as sys;
use std::{ffi::CString, ptr::NonNull, sync::Arc, time::Duration};

const MODE_PRIVATE: i32 = 0;
const CONFIG_KEY: &str = "config";
const PREF_NAME: &str = "alvr-pref";
const MICROPHONE_PERMISSION: &str = "android.permission.RECORD_AUDIO";

pub fn vm() -> JavaVM {
    unsafe { JavaVM::from_raw(ndk_context::android_context().vm().cast()).unwrap() }
}

pub fn context() -> jobject {
    ndk_context::android_context().context().cast()
}

// ASurfaceTexture is avaliable only on API level 28. Use a custom Java wrapper instead.
struct SyncSurface {
    inner: GlobalRef,
}

impl SyncSurface {
    fn new(gl_texture: i32) -> Self {
        let vm = vm();
        let env = vm.attach_current_thread().unwrap();

        let jsync_surface = env
            .new_object(
                "com/polygraphene/alvr/SyncSurfaceInner",
                "(I)V",
                &[gl_texture.into()],
            )
            .unwrap();

        let inner = env.new_global_ref(jsync_surface).unwrap();

        SyncSurface { inner }
    }

    fn native_window(&self) -> NativeWindow {
        let vm = vm();
        let env = vm.attach_current_thread().unwrap();

        let surface = env
            .call_method(&self.inner, "getSurface", "()Landroid/view/Surface;", &[])
            .unwrap()
            .l()
            .unwrap();

        unsafe { NativeWindow::from_surface(env.get_native_interface(), *surface).unwrap() }
    }

    // Write image data to the texture passed to the constructor. Returns image timestamp, or None
    // if timeout.
    fn wait_next_image(&self, timeout: Duration) -> Option<Duration> {
        let vm = vm();
        let env = vm.attach_current_thread().unwrap();

        let timeout_nanos = timeout.as_nanos() as i64;
        let timestamp = env
            .call_method(
                &self.inner,
                "waitNextImage",
                "(J)J",
                &[timeout_nanos.into()],
            )
            .unwrap()
            .j()
            .unwrap();

        if timestamp != -1 {
            Some(Duration::from_nanos(timestamp as _))
        } else {
            None
        }
    }
}

pub fn try_get_microphone_permission() {
    let vm = vm();
    let env = vm.attach_current_thread().unwrap();

    let mic_perm_jstring = env.new_string(MICROPHONE_PERMISSION).unwrap();

    let permission_status = env
        .call_method(
            context(),
            "checkSelfPermission",
            "(Ljava/lang/String;)I",
            &[mic_perm_jstring.into()],
        )
        .unwrap()
        .i()
        .unwrap();

    if permission_status != 0 {
        let string_class = env.find_class("java/lang/String").unwrap();
        let perm_array = env
            .new_object_array(1, string_class, mic_perm_jstring)
            .unwrap();

        env.call_method(
            context(),
            "requestPermissions",
            "([Ljava/lang/String;I)V",
            &[perm_array.into(), 0.into()],
        )
        .unwrap();

        // todo: handle case where permission is rejected
    }
}

pub fn load_asset(fname: &str) -> Vec<u8> {
    let vm = vm();
    let env = vm.attach_current_thread().unwrap();

    let asset_manager = unsafe {
        let jasset_manager = env
            .call_method(
                context(),
                "getAssets",
                "()Landroid/content/res/AssetManager;",
                &[],
            )
            .unwrap()
            .l()
            .unwrap();
        let asset_manager_ptr =
            sys::AAssetManager_fromJava(env.get_native_interface(), jasset_manager.cast());

        ndk::asset::AssetManager::from_ptr(NonNull::new(asset_manager_ptr).unwrap())
    };

    let fname_cstring = CString::new(fname).unwrap();
    let mut asset = asset_manager.open(fname_cstring.as_c_str()).unwrap();
    asset.get_buffer().unwrap().to_vec()
}

pub fn load_config_string() -> String {
    let vm = vm();
    let env = vm.attach_current_thread().unwrap();

    let pref_name = env.new_string(PREF_NAME).unwrap();
    let shared_preferences = env
        .call_method(
            context(),
            "getSharedPreferences",
            "(Ljava/lang/String;I)Landroid/content/SharedPreferences;",
            &[pref_name.into(), MODE_PRIVATE.into()],
        )
        .unwrap()
        .l()
        .unwrap();

    let key = env.new_string(CONFIG_KEY).unwrap();
    let default = env.new_string("").unwrap();

    let config = env
        .call_method(
            shared_preferences,
            "getString",
            "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
            &[key.into(), default.into()],
        )
        .unwrap();

    env.get_string(config.l().unwrap().into()).unwrap().into()
}

pub fn store_config_string(config: String) {
    let vm = vm();
    let env = vm.attach_current_thread().unwrap();

    let pref_name = env.new_string(PREF_NAME).unwrap();
    let shared_preferences = env
        .call_method(
            context(),
            "getSharedPreferences",
            "(Ljava/lang/String;I)Landroid/content/SharedPreferences;",
            &[pref_name.into(), MODE_PRIVATE.into()],
        )
        .unwrap()
        .l()
        .unwrap();

    let editor = env
        .call_method(
            shared_preferences,
            "edit",
            "()Landroid/content/SharedPreferences$Editor;",
            &[],
        )
        .unwrap()
        .l()
        .unwrap();

    let key = env.new_string(CONFIG_KEY).unwrap();
    let value = env.new_string(config).unwrap();
    env.call_method(
        editor,
        "putString",
        "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/SharedPreferences$Editor;",
        &[key.into(), value.into()],
    )
    .unwrap();

    env.call_method(editor, "apply", "()V", &[]).unwrap();
}

pub fn device_name() -> String {
    let vm = vm();
    let env = vm.attach_current_thread().unwrap();

    let jbrand_name = env
        .get_static_field("android/os/Build", "BRAND", "Ljava/lang/String;")
        .unwrap()
        .l()
        .unwrap();
    let brand_name_raw = env.get_string(jbrand_name.into()).unwrap();
    let brand_name = brand_name_raw.to_string_lossy().as_ref().to_owned();
    // Capitalize first letter
    let mut brand_name_it = brand_name.chars();
    let brand_name = brand_name_it
        .next()
        .unwrap()
        .to_uppercase()
        .chain(brand_name_it)
        .collect::<String>();

    let jdevice_name = env
        .get_static_field("android/os/Build", "MODEL", "Ljava/lang/String;")
        .unwrap()
        .l()
        .unwrap();
    let device_name_raw = env.get_string(jdevice_name.into()).unwrap();
    let device_name = device_name_raw.to_string_lossy().as_ref().to_owned();

    format!("{brand_name} {device_name}")
}

pub struct VideoDecoderEnqueuer {
    inner: Arc<MediaCodec>,
}

unsafe impl Send for VideoDecoderEnqueuer {}

impl VideoDecoderEnqueuer {
    // Block until the buffer has been written or timeout is reached. Returns false if timeout.
    pub fn push_frame_nal(
        &self,
        timestamp: Duration,
        data: &[u8],
        timeout: Duration,
    ) -> StrResult<bool> {
        if let Some(mut buffer) = self.inner.dequeue_input_buffer(timeout).map_err(err!())? {
            buffer.buffer_mut()[..data.len()].copy_from_slice(data);

            // NB: the function expects the timestamp in micros, but nanos is used to have complete
            // precision, so when converted back to Duration it can compare correctly to other
            // Durations
            self.inner
                .queue_input_buffer(buffer, 0, data.len(), timestamp.as_nanos() as _, 0)
                .map_err(err!())?;

            Ok(true)
        } else {
            Ok(false)
        }
    }
}

pub struct VideoDecoderDequeuer {
    inner: Arc<MediaCodec>,
    surface: SyncSurface,
}

unsafe impl Send for VideoDecoderDequeuer {}

impl VideoDecoderDequeuer {
    pub fn dequeue_frame(&self, timeout: Duration) -> StrResult<Option<Duration>> {
        if let Some(buffer) = self.inner.dequeue_output_buffer(timeout).map_err(err!())? {
            self.inner
                .release_output_buffer(buffer, true)
                .map_err(err!())?;

            Ok(self.surface.wait_next_image(timeout))
        } else {
            Ok(None)
        }
    }
}

pub fn video_decoder_split(
    codec_type: CodecType,
    csd_0: &[u8],
    extra_options: &[(String, MediacodecDataType)],
    output_texture: i32, // GLuint
) -> StrResult<(VideoDecoderEnqueuer, VideoDecoderDequeuer)> {
    let surface = SyncSurface::new(output_texture);

    let mime = match codec_type {
        CodecType::H264 => "video/avc",
        CodecType::HEVC => "video/hevc",
    };

    let format = MediaFormat::new();
    format.set_str("mime", mime);
    format.set_i32("width", 512);
    format.set_i32("height", 1024);
    format.set_buffer("csd-0", csd_0);

    for (key, value) in extra_options {
        match value {
            MediacodecDataType::Float(value) => format.set_f32(key, *value),
            MediacodecDataType::Int32(value) => format.set_i32(key, *value),
            MediacodecDataType::Int64(value) => format.set_i64(key, *value),
            MediacodecDataType::String(value) => format.set_str(key, value),
        }
    }

    let decoder = Arc::new(MediaCodec::from_decoder_type(mime).ok_or_else(enone!())?);
    decoder
        .configure(
            &format,
            &surface.native_window(),
            MediaCodecDirection::Decoder,
        )
        .map_err(err!())?;
    decoder.start().map_err(err!())?;

    Ok((
        VideoDecoderEnqueuer {
            inner: Arc::clone(&decoder),
        },
        VideoDecoderDequeuer {
            inner: Arc::clone(&decoder),
            surface,
        },
    ))
}
