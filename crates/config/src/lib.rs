//! bilidown 的配置存取：读写 Python 版写入的同一个 config.json，
//!
//! 语义对齐 qfluentwidgets 的 QConfig：
//! * 键序（保存格式）与默认值来自内置 defaults.json（由 Python 端真实保存路径
//!   生成，字节即基准）；
//! * 读取时未知分组/键被忽略；校验器纠正非法值（Range 夹取、Options 取首项、
//!   Bool 纠正为 true）；枚举反序列化失败则中止本次 load（其后的键保持默认）；
//! * 保存是先写临时文件再原子替换。
//!
//! `naming_rule_list` 以原始 JSON 值透传 —— schema 归 naming 模块管。

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde_json::{Map, Value};

/// 当前配置格式版本（与 Python 端 app_config_version 一致）。
pub const APP_CONFIG_VERSION: i64 = 2200;

/// 应用名，也是 User Data Directory 下的目录名（旧名 "Bili23 Downloader"
/// 的目录在定位时一次性改名迁移）。
pub const APP_NAME: &str = "bilidown";

/// Python 版（Bili23 Downloader）的数据目录名。
const LEGACY_APP_NAME: &str = "Bili23 Downloader";

/// `Advanced/user_agent` 缺省值（与 defaults.json / Python 端一致，Edge 变体）。
pub const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/147.0.0.0 Safari/537.36 Edg/147.0.0.0";

/// defaults.json 由 Python 端真实保存路径生成：解析一次即得键序与全部默认值，
/// 无需手抄约一百条默认项。
const DEFAULTS: &str = include_str!("defaults.json");

/// 2.20.0 迁移要剔除的旧默认 CDN host。
const AKAMAI_HOST: &str = "upos-hz-mirrorakam.akamaized.net";

/// Qt AppDataLocation 的无 Qt 等价（与 util/common/mcp/stdio_bridge.py 一致）。
/// 目录从 "Bili23 Downloader" 改名为 bilidown 后，旧目录整体移动一次。
fn default_config_dir() -> Option<PathBuf> {
    let dir = dirs::data_dir().map(|d| d.join(APP_NAME))?;
    if dir.is_dir() {
        return Some(dir);
    }
    let legacy = dir.with_file_name(LEGACY_APP_NAME);
    if legacy.is_dir() {
        std::fs::rename(&legacy, &dir).ok();
    }
    Some(dir)
}

/// 配置存储：有序的 group -> item -> JSON 值，初值即默认值。
// ponytail: save 无锁 —— 单线程调用；跨线程共享时由调用方加锁（tmp 文件同名会交错）
pub struct ConfigStore {
    file: PathBuf,
    values: Map<String, Value>,
    /// 2.20.0 迁移置位：命名规则被整表重置，界面要提示用户。
    naming_rules_reset: bool,
}

impl ConfigStore {
    /// 生产构造：User Data Directory/bilidown/config.json。
    /// download_path 默认取系统下载目录（拿不到则退回家目录）。
    pub fn open() -> anyhow::Result<Self> {
        let dir = default_config_dir().context("无法定位 User Data Directory")?;
        let downloads = dirs::download_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from(""));
        Ok(Self::with_paths(dir, downloads))
    }

    /// 测试/自定义构造：配置文件在 `config_dir` 下，download_path 默认值注入
    /// （金样字节里它是生成夹具时的 /home/test/Downloads）。
    pub fn with_paths(config_dir: impl Into<PathBuf>, download_dir: impl Into<PathBuf>) -> Self {
        let mut store = Self {
            file: config_dir.into().join("config.json"),
            values: defaults_map(),
            naming_rules_reset: false,
        };
        store.set_item(
            "Download",
            "download_path",
            Value::String(download_dir.into().to_string_lossy().into_owned()),
        );
        store.load_and_migrate();
        store
    }

    /// Python 导入期流程：qconfig.load -> check_need_patch -> patch_config。
    fn load_and_migrate(&mut self) {
        let Ok(bytes) = fs::read(&self.file) else {
            return; // 文件不存在：不加载、不迁移、不落盘
        };
        let data: Map<String, Value> = serde_json::from_slice::<Value>(&bytes)
            .ok()
            .and_then(|v| match v {
                Value::Object(m) => Some(m),
                _ => None,
            })
            .unwrap_or_default(); // 解析失败：按空处理，仍走迁移

        let version = data
            .get("Application")
            .and_then(|a| a.get("config_version"))
            .and_then(Value::as_i64);

        // 2.13.0 门禁读盘上旧数据（proxy_enabled 是已删除的键，叠加时本就被忽略）
        let proxy_enabled = data
            .get("Advanced")
            .and_then(|a| a.get("proxy_enabled"))
            .is_some_and(python_truthy);

        // 只叠加已知的 (group, item)；校验失败按 Python 语义中止整个 load
        'outer: for (group, items) in &data {
            let Some(dst) = self.values.get_mut(group).and_then(Value::as_object_mut) else {
                continue; // 未知分组
            };
            let Some(items) = items.as_object() else {
                continue;
            };
            for (item, value) in items {
                if let Some(slot) = dst.get_mut(item) {
                    match correct(group, item, slot, value) {
                        Some(v) => *slot = v,
                        None => break 'outer, // 反序列化异常：剩余键保持默认
                    }
                }
            }
        }

        if version.is_none_or(|v| v < APP_CONFIG_VERSION) {
            self.patch(version.unwrap_or(0), proxy_enabled);
            let _ = self.save(); // patch_config 末尾的 save()，失败不致命
        }
    }

    /// patch_config 逐步移植。门禁版本读盘上旧值，列表读当前配置值。
    fn patch(&mut self, version: i64, proxy_enabled: bool) {
        if version < 2130 && proxy_enabled {
            // 旧版开着代理开关 → 迁移为手动；关着则保持已加载的三态
            self.set_item("Advanced", "proxy_mode", Value::from("manual"));
        }

        if version < 2140 {
            // 画质优先级补入 122（SDR 增强）：有 4K(120) 插其前，否则追加
            let mut priority = self
                .get_item("Download", "video_quality_priority")
                .cloned()
                .unwrap_or(Value::Array(vec![]));
            if let Some(list) = priority.as_array_mut() {
                if !list.iter().any(|q| q.as_i64() == Some(122)) {
                    match list.iter().position(|q| q.as_i64() == Some(120)) {
                        Some(pos) => list.insert(pos, Value::from(122)),
                        None => list.push(Value::from(122)),
                    }
                }
            }
            self.set_item("Download", "video_quality_priority", priority);
        }

        if version < 2200 {
            // 命名规则整表换新默认（用户改过/自建的一并舍弃，置标志提示）
            self.set_item(
                "File Naming",
                "naming_rule_list",
                defaults_map()
                    .get("File Naming")
                    .and_then(|g| g.get("naming_rule_list"))
                    .cloned()
                    .unwrap_or(Value::Array(vec![])),
            );
            self.naming_rules_reset = true;

            // 海外 CDN 列表剔除 Akamai：只按 host 精确匹配，畸形项原样保留，
            // 长度不变则不写回（QConfig.set 的等值 no-op）
            if let Some(entries) = self
                .get_item("Advanced", "ov_cdn_server_list")
                .and_then(Value::as_array)
                .cloned()
            {
                let filtered: Vec<Value> = entries
                    .iter()
                    .filter(|e| e.get("host").and_then(Value::as_str) != Some(AKAMAI_HOST))
                    .cloned()
                    .collect();
                if filtered.len() != entries.len() {
                    self.set_item("Advanced", "ov_cdn_server_list", Value::Array(filtered));
                }
            }
        }

        self.set_item(
            "Application",
            "config_version",
            Value::from(APP_CONFIG_VERSION),
        );
    }

    fn get_item(&self, group: &str, item: &str) -> Option<&Value> {
        self.values.get(group)?.get(item)
    }

    fn set_item(&mut self, group: &str, item: &str, value: Value) {
        if let Some(v) = self.values.get_mut(group).and_then(|g| g.get_mut(item)) {
            *v = value;
        }
    }

    /// 原子保存：写临时文件后 rename。失败仅返回错误（调用方记日志）。
    pub fn save(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.file.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("创建目录失败: {}", parent.display()))?;
        }
        let tmp = self.file.with_extension("json.tmp");
        let mut bytes = Vec::with_capacity(16 * 1024);
        write_pretty(&Value::Object(self.values.clone()), &mut bytes);
        fs::write(&tmp, &bytes).with_context(|| format!("写入临时文件失败: {}", tmp.display()))?;
        match fs::rename(&tmp, &self.file) {
            Ok(()) => Ok(()),
            Err(e) => {
                let _ = fs::remove_file(&tmp);
                Err(e).with_context(|| format!("替换配置文件失败: {}", self.file.display()))
            }
        }
    }

    /// 读一个配置项（迁移后的当前值）。
    pub fn get(&self, group: &str, item: &str) -> Option<&Value> {
        self.get_item(group, item)
    }

    /// 写一个配置项（不校验、不落盘；未知键忽略）。
    pub fn set(&mut self, group: &str, item: &str, value: Value) {
        self.set_item(group, item, value);
    }

    /// 2.20.0 迁移是否重置了命名规则（界面提示用）。
    pub fn naming_rules_reset(&self) -> bool {
        self.naming_rules_reset
    }

    /// 配置文件路径。
    pub fn path(&self) -> &Path {
        &self.file
    }
}

fn defaults_map() -> Map<String, Value> {
    serde_json::from_str::<Value>(DEFAULTS)
        .expect("内置 defaults.json 必须是对象")
        .as_object()
        .expect("内置 defaults.json 必须是对象")
        .clone()
}

/// Python 的真值语义：False/None/0/""/[]/{} 为假，其余为真。
fn python_truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

// ---------- 读取校验：Python 各校验器的等价物 ----------

/// 校验并纠正一个待叠加的值。`None` = 反序列化异常（Python 会中止整个 load）。
/// 选项集与纠偏目标（首项）按 src/util/common/enum.py 的定义顺序核对过。
fn correct(group: &str, item: &str, slot: &Value, incoming: &Value) -> Option<Value> {
    let key = (group, item);
    match key {
        ("Interface", "language") => options_str(incoming, &["zh_CN", "zh_TW", "en_US", "Auto"]),
        ("Interface", "display_scaling") => {
            options_str(incoming, &["1", "1.25", "1.5", "1.75", "2", "Auto"])
        }
        ("Advanced", "area") => options_str(incoming, &["cn", "ov"]),
        ("Advanced", "proxy_mode") => options_str(incoming, &["disabled", "system", "manual"]),
        ("Advanced", "proxy_type") => options_str(incoming, &["http"]),
        ("Advanced", "ffmpeg_source") => options_str(incoming, &["bundled", "system", "custom"]),
        ("Download", "video_container") => options_str(incoming, &["mp4", "mkv"]),
        ("Additional", "danmaku_type") => options_str(incoming, &["xml", "ass", "json"]),
        ("Additional", "subtitle_type") => {
            options_str(incoming, &["srt", "lrc", "txt", "ass", "json"])
        }
        ("Additional", "cover_type") => options_str(incoming, &["jpg", "png", "avif", "webp"]),
        ("Additional", "metadata_type") => options_str(incoming, &["nfo", "json"]),
        ("Behavior", "when_close_window") => options_int(incoming, &[1, 2, 3]),
        ("Behavior", "file_conflict_resolution") => options_int(incoming, &[1, 2]),
        ("Behavior", "duplicate_download_resolution") => options_int(incoming, &[0, 1, 2]),
        ("Behavior", "auto_select_mode_") => options_int(incoming, &[0, 1, 2]),
        ("File Naming", "numbering_type") => options_int(incoming, &[0, 1, 2]),
        ("Download", "keep_original_files_type") => options_int(incoming, &[0, 1, 2]),
        ("Download", "download_thread") => range(incoming, 1, 10),
        ("Download", "download_parallel") => range(incoming, 1, 10),
        ("Download", "auto_retry_max_count") => range(incoming, 1, 20),
        ("MCP", "mcp_port") => range(incoming, 1024, 65535),
        ("QFluentWidgets", "ThemeColor") => Some(color(incoming)),
        // 默认值是 bool 的项带 BoolValidator：1/0 原样放行（Python 1==True），其余纠正为 true
        _ if slot.is_boolean() => Some(match incoming {
            Value::Bool(_) => incoming.clone(),
            v if v.as_i64() == Some(0) || v.as_i64() == Some(1) => v.clone(),
            _ => Value::Bool(true),
        }),
        // 其余项无校验器：透传
        _ => Some(incoming.clone()),
    }
}

/// 枚举序列化项：字符串必须严格命中，否则反序列化抛异常（中止 load）。
/// 命中后由 OptionsValidator 确认 —— 集合相同，必过。
fn options_str(v: &Value, allowed: &[&str]) -> Option<Value> {
    match v.as_str() {
        Some(s) if allowed.contains(&s) => Some(v.clone()),
        _ => None,
    }
}

/// OptionsValidator：命中（含 1.0==1 这类数值相等）原样保留，否则取首项。
fn options_int(v: &Value, allowed: &[i64]) -> Option<Value> {
    let hit = v.as_f64().is_some_and(|n| allowed.contains(&(n as i64)));
    Some(if hit {
        v.clone()
    } else {
        Value::from(allowed[0])
    })
}

/// RangeValidator：夹取到区间；非数值在 Python 里比较即抛异常 → 中止 load。
fn range(v: &Value, min: i64, max: i64) -> Option<Value> {
    let n = v.as_f64()?;
    let clamped = n.clamp(min as f64, max as f64);
    Some(if v.is_i64() || v.is_u64() {
        Value::from(clamped as i64)
    } else {
        Value::from(clamped)
    })
}

/// QFluentWidgets.ThemeColor：接受 #RGB/#RRGGBB/#AARRGGBB（大小写不限），
/// 归一成小写 #aarrggbb；非法取默认 #ff009faa。
fn color(v: &Value) -> Value {
    let fallback = Value::from("#ff009faa");
    let Some(s) = v.as_str() else { return fallback };
    let hex = s.strip_prefix('#').unwrap_or("");
    let expanded = match hex.len() {
        3 => format!(
            "ff{}{}{}{}{}{}",
            &hex[0..1],
            &hex[0..1],
            &hex[1..2],
            &hex[1..2],
            &hex[2..3],
            &hex[2..3]
        ),
        6 => format!("ff{hex}"),
        8 => hex.to_string(),
        _ => return fallback,
    };
    if expanded.len() == 8 && expanded.bytes().all(|b| b.is_ascii_hexdigit()) {
        Value::from(format!("#{}", expanded.to_ascii_lowercase()))
    } else {
        fallback
    }
}

// ---------- 保存格式：Python json.dumps(indent=4, ensure_ascii=False) 的字节等价 ----------

fn write_pretty(value: &Value, out: &mut Vec<u8>) {
    write_value(value, 0, out);
}

fn write_value(value: &Value, indent: usize, out: &mut Vec<u8>) {
    match value {
        Value::Object(map) if !map.is_empty() => {
            out.push(b'{');
            for (i, (k, v)) in map.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                out.push(b'\n');
                pad(indent + 1, out);
                write_string(k, out);
                out.extend_from_slice(b": ");
                write_value(v, indent + 1, out);
            }
            out.push(b'\n');
            pad(indent, out);
            out.push(b'}');
        }
        Value::Array(items) if !items.is_empty() => {
            out.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                out.push(b'\n');
                pad(indent + 1, out);
                write_value(item, indent + 1, out);
            }
            out.push(b'\n');
            pad(indent, out);
            out.push(b']');
        }
        Value::Object(_) => out.extend_from_slice(b"{}"),
        Value::Array(_) => out.extend_from_slice(b"[]"),
        Value::String(s) => write_string(s, out),
        Value::Number(n) => out.extend_from_slice(n.to_string().as_bytes()),
        Value::Bool(b) => out.extend_from_slice(if *b { b"true" } else { b"false" }),
        Value::Null => out.extend_from_slice(b"null"),
    }
}

fn pad(indent: usize, out: &mut Vec<u8>) {
    for _ in 0..indent * 4 {
        out.push(b' ');
    }
}

/// Python 的转义集：\" \\ \b \f \n \r \t，其余 <0x20 为 \u00xx（小写十六进制）；
/// '/' 与 >=0x7f 原样输出（ensure_ascii=False）。
fn write_string(s: &str, out: &mut Vec<u8>) {
    out.push(b'"');
    for c in s.chars() {
        match c {
            '"' => out.extend_from_slice(b"\\\""),
            '\\' => out.extend_from_slice(b"\\\\"),
            '\u{8}' => out.extend_from_slice(b"\\b"),
            '\u{c}' => out.extend_from_slice(b"\\f"),
            '\n' => out.extend_from_slice(b"\\n"),
            '\r' => out.extend_from_slice(b"\\r"),
            '\t' => out.extend_from_slice(b"\\t"),
            c if (c as u32) < 0x20 => {
                out.extend_from_slice(format!("\\u{:04x}", c as u32).as_bytes());
            }
            c => out.extend_from_slice(c.encode_utf8(&mut [0u8; 4]).as_bytes()),
        }
    }
    out.push(b'"');
}
