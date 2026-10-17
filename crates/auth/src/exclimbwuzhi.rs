//! ExClimbWuzhi activation payload — port of
//! `util/common/data/exclimbwuzhi.py`.
//!
//! Body shape: a JSON object whose `payload` value is the *stringified*
//! inner JSON (`dumps({"payload": dumps(payload)})`). Inner JSON uses
//! stdlib separators (`", "` / `": "`) with insertion order preserved.

use serde_json::{json, Map, Value};

/// Serialization matching Python `json.dumps(obj)` default separators:
/// `", "` between items, `": "` after keys, insertion order.
pub fn dumps_python(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => dumps_string(s),
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(dumps_python).collect();
            format!("[{}]", parts.join(", "))
        }
        Value::Object(map) => {
            let parts: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}: {}", dumps_string(k), dumps_python(v)))
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
    }
}

fn dumps_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `https://api.bilibili.com/x/internal/gaia-gateway/ExClimbWuzhi`.
pub const EXCLIMBWUZHI_API: &str = "https://api.bilibili.com/x/internal/gaia-gateway/ExClimbWuzhi";

/// `get_exclimbwuzhi_payload(user_agent, uuid)` — the full request body
/// as a JSON string.
pub fn exclimbwuzhi_payload(user_agent: &str, uuid: &str, timestamp_ms: i64) -> String {
    let mut inner = Map::new();
    inner.insert("3064".to_owned(), json!(1));
    inner.insert("5062".to_owned(), json!(timestamp_ms.to_string()));
    inner.insert(
        "03bf".to_owned(),
        json!("https%3A%2F%2Fwww.bilibili.com%2F"),
    );
    inner.insert("39c8".to_owned(), json!("333.1007.fp.risk"));
    inner.insert("34f1".to_owned(), json!(""));
    inner.insert("d402".to_owned(), json!(""));
    inner.insert("654a".to_owned(), json!(""));
    inner.insert("6e7c".to_owned(), json!("1699x794"));
    inner.insert("3c43".to_owned(), device_info(user_agent));
    inner.insert("54ef".to_owned(), json!(AB_TEST_PAYLOAD));
    inner.insert("8b94".to_owned(), json!(""));
    inner.insert("df35".to_owned(), json!(uuid));
    inner.insert("07a4".to_owned(), json!("zh-CN"));
    inner.insert("5f45".to_owned(), Value::Null);
    inner.insert("db46".to_owned(), json!(0));

    let mut outer = Map::new();
    outer.insert(
        "payload".to_owned(),
        Value::String(dumps_python(&Value::Object(inner))),
    );
    dumps_python(&Value::Object(outer))
}

/// The `54ef` field — a JSON blob as a plain string.
const AB_TEST_PAYLOAD: &str = "{\"b_ut\":\"\",\"home_version\":\"V8\",\"in_new_ab\":true,\"ab_version\":{\"for_ai_home_version\":\"V8\",\"in_theme_version\":\"OPEN\",\"enable_web_push\":\"DISABLE\",\"enable_ai_floor_api\":\"ENABLE\",\"enable_shortcut_key\":\"DISABLE\",\"rcmd_timeout_config\":\"550\",\"home_performance_opt\":\"ssr_fetch_opt\",\"infra_projection\":\"OFF\"},\"ab_split_num\":{\"for_ai_home_version\":54,\"in_theme_version\":30,\"enable_web_push\":10,\"enable_ai_floor_api\":137,\"enable_shortcut_key\":54,\"rcmd_timeout_config\":49,\"home_performance_opt\":49,\"infra_projection\":49},\"uniq_page_id\":\"1671272756362\",\"is_modern\":true}";

fn device_info(user_agent: &str) -> Value {
    let mut c = Map::new();
    c.insert("2673".to_owned(), json!(0));
    c.insert("5766".to_owned(), json!(32));
    c.insert("6527".to_owned(), json!(0));
    c.insert("7003".to_owned(), json!(1));
    c.insert("807e".to_owned(), json!(1));
    c.insert("b8ce".to_owned(), json!(user_agent));
    c.insert("641c".to_owned(), json!(0));
    c.insert("07a4".to_owned(), json!("zh-CN"));
    c.insert("1c57".to_owned(), json!(32));
    c.insert("0bd0".to_owned(), json!(20));
    c.insert("748e".to_owned(), json!([960, 1707]));
    c.insert("d61f".to_owned(), json!([912, 1707]));
    c.insert("fc9d".to_owned(), json!(-480));
    c.insert("6aa9".to_owned(), json!("Asia/Shanghai"));
    c.insert("75b8".to_owned(), json!(1));
    c.insert("3b21".to_owned(), json!(1));
    c.insert("8a1c".to_owned(), json!(0));
    c.insert("d52f".to_owned(), json!("not available"));
    c.insert("adca".to_owned(), json!("Win32"));
    c.insert("80c9".to_owned(), pdf_viewers());
    c.insert("13ab".to_owned(), json!("EPQAAAAASUVORK5CYII="));
    c.insert(
        "bfe9".to_owned(),
        json!("//TgNIfAAAAAZJREFUAwBde+3wgcxEHQAAAABJRU5ErkJggg=="),
    );
    c.insert("a3c1".to_owned(), webgl_report());
    c.insert(
        "6bc5".to_owned(),
        json!("Google Inc. (NVIDIA)~ANGLE (NVIDIA, NVIDIA GeForce RTX 4060 Laptop GPU (0x000028E0) Direct3D11 vs_5_0 ps_5_0, D3D11)"),
    );
    c.insert("ed31".to_owned(), json!(0));
    c.insert("72bd".to_owned(), json!(0));
    c.insert("097b".to_owned(), json!(0));
    c.insert("52cd".to_owned(), json!([0, 0, 0]));
    c.insert("a658".to_owned(), fonts());
    c.insert("d02f".to_owned(), json!("124.04347527516074"));
    Value::Object(c)
}

fn pdf_viewers() -> Value {
    json!([
        [
            "PDF Viewer",
            "Portable Document Format",
            [["application/pdf", "pdf"], ["text/pdf", "pdf"]]
        ],
        [
            "Chrome PDF Viewer",
            "Portable Document Format",
            [["application/pdf", "pdf"], ["text/pdf", "pdf"]]
        ],
        [
            "Chromium PDF Viewer",
            "Portable Document Format",
            [["application/pdf", "pdf"], ["text/pdf", "pdf"]]
        ],
        [
            "Microsoft Edge PDF Viewer",
            "Portable Document Format",
            [["application/pdf", "pdf"], ["text/pdf", "pdf"]]
        ],
        [
            "WebKit built-in PDF",
            "Portable Document Format",
            [["application/pdf", "pdf"], ["text/pdf", "pdf"]]
        ],
    ])
}

fn webgl_report() -> Value {
    json!([
        "extensions:ANGLE_instanced_arrays;EXT_blend_minmax;EXT_clip_control;EXT_color_buffer_half_float;EXT_depth_clamp;EXT_disjoint_timer_query;EXT_float_blend;EXT_frag_depth;EXT_polygon_offset_clamp;EXT_shader_texture_lod;EXT_texture_compression_bptc;EXT_texture_compression_rgtc;EXT_texture_filter_anisotropic;EXT_texture_mirror_clamp_to_edge;EXT_sRGB;KHR_parallel_shader_compile;OES_element_index_uint;OES_fbo_render_mipmap;OES_standard_derivatives;OES_texture_float;OES_texture_float_linear;OES_texture_half_float;OES_texture_half_float_linear;OES_vertex_array_object;WEBGL_blend_func_extended;WEBGL_color_buffer_float;WEBGL_compressed_texture_s3tc;WEBGL_compressed_texture_s3tc_srgb;WEBGL_debug_renderer_info;WEBGL_debug_shaders;WEBGL_depth_texture;WEBGL_draw_buffers;WEBGL_lose_context;WEBGL_multi_draw;WEBGL_polygon_mode",
        "webgl aliased line width range:[1, 1]",
        "webgl aliased point size range:[1, 1024]",
        "webgl alpha bits:8",
        "webgl antialiasing:yes",
        "webgl blue bits:8",
        "webgl depth bits:24",
        "webgl green bits:8",
        "webgl max anisotropy:16",
        "webgl max combined texture image units:32",
        "webgl max cube map texture size:16384",
        "webgl max fragment uniform vectors:1024",
        "webgl max render buffer size:16384",
        "webgl max texture image units:16",
        "webgl max texture size:16384",
        "webgl max varying vectors:30",
        "webgl max vertex attribs:16",
        "webgl max vertex texture image units:16",
        "webgl max vertex uniform vectors:4095",
        "webgl max viewport dims:[32767, 32767]",
        "webgl red bits:8",
        "webgl renderer:WebKit WebGL",
        "webgl shading language version:WebGL GLSL ES 1.0 (OpenGL ES GLSL ES 1.0 Chromium)",
        "webgl stencil bits:0",
        "webgl vendor:WebKit",
        "webgl version:WebGL 1.0 (OpenGL ES 2.0 Chromium)",
        "webgl unmasked vendor:Google Inc. (NVIDIA)",
        "webgl unmasked renderer:ANGLE (NVIDIA, NVIDIA GeForce RTX 4060 Laptop GPU (0x000028E0) Direct3D11 vs_5_0 ps_5_0, D3D11)",
        "webgl vertex shader high float precision:23",
        "webgl vertex shader high float precision rangeMin:127",
        "webgl vertex shader high float precision rangeMax:127",
        "webgl vertex shader medium float precision:23",
        "webgl vertex shader medium float precision rangeMin:127",
        "webgl vertex shader medium float precision rangeMax:127",
        "webgl vertex shader low float precision:23",
        "webgl vertex shader low float precision rangeMin:127",
        "webgl vertex shader low float precision rangeMax:127",
        "webgl fragment shader high float precision:23",
        "webgl fragment shader high float precision rangeMin:127",
        "webgl fragment shader high float precision rangeMax:127",
        "webgl fragment shader medium float precision:23",
        "webgl fragment shader medium float precision rangeMin:127",
        "webgl fragment shader medium float precision rangeMax:127",
        "webgl fragment shader low float precision:23",
        "webgl fragment shader low float precision rangeMin:127",
        "webgl fragment shader low float precision rangeMax:127",
        "webgl vertex shader high int precision:0",
        "webgl vertex shader high int precision rangeMin:31",
        "webgl vertex shader high int precision rangeMax:30",
        "webgl vertex shader medium int precision:0",
        "webgl vertex shader medium int precision rangeMin:31",
        "webgl vertex shader medium int precision rangeMax:30",
        "webgl vertex shader low int precision:0",
        "webgl vertex shader low int precision rangeMin:31",
        "webgl vertex shader low int precision rangeMax:30",
        "webgl fragment shader high int precision:0",
        "webgl fragment shader high int precision rangeMin:31",
        "webgl fragment shader high int precision rangeMax:30",
        "webgl fragment shader medium int precision:0",
        "webgl fragment shader medium int precision rangeMin:31",
        "webgl fragment shader medium int precision rangeMax:30",
        "webgl fragment shader low int precision:0",
        "webgl fragment shader low int precision rangeMin:31",
        "webgl fragment shader low int precision rangeMax:30",
    ])
}

fn fonts() -> Value {
    json!([
        "Arial",
        "Arial Black",
        "Arial Narrow",
        "Book Antiqua",
        "Bookman Old Style",
        "Calibri",
        "Cambria",
        "Cambria Math",
        "Century",
        "Century Gothic",
        "Century Schoolbook",
        "Comic Sans MS",
        "Consolas",
        "Courier",
        "Courier New",
        "Georgia",
        "Helvetica",
        "Impact",
        "Lucida Bright",
        "Lucida Calligraphy",
        "Lucida Console",
        "Lucida Fax",
        "Lucida Handwriting",
        "Lucida Sans",
        "Lucida Sans Typewriter",
        "Lucida Sans Unicode",
        "Microsoft Sans Serif",
        "Monotype Corsiva",
        "MS Gothic",
        "MS PGothic",
        "MS Reference Sans Serif",
        "MS Sans Serif",
        "MS Serif",
        "Palatino Linotype",
        "Segoe Print",
        "Segoe Script",
        "Segoe UI",
        "Segoe UI Light",
        "Segoe UI Semibold",
        "Segoe UI Symbol",
        "Tahoma",
        "Times",
        "Times New Roman",
        "Trebuchet MS",
        "Verdana",
        "Wingdings",
        "Wingdings 2",
        "Wingdings 3",
    ])
}
