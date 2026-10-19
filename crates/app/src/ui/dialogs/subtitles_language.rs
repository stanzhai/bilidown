//! Subtitle-language picker dialog (ticket 31): port of
//! `src/gui/dialog/setting/subtitles_language.py`. Radio between
//! "download all" and "download only selected", plus a checkable list
//! of the 158-entry language table from
//! `src/util/common/data/subtitles.py`. Persists
//! `Additional.subtitle_language` (`{download_specified, specified_language}`),
//! the exact shape `extras::subtitles` reads.

use serde_json::{json, Value};

use gpui_kit::component::button::Button;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::label::Label;
use gpui_kit::component::radio::{Radio, RadioGroup};
use gpui_kit::component::WindowExt;
use gpui_kit::{div, prelude::*, px, ParentElement, Window};

use super::active_catalog;

const CTX: &str = "SubtitlesLanguageDialog";

/// `subtitles_language_list` verbatim (`(lan, doc_zh)` pairs, same order).
pub const SUBTITLES_LANGUAGE_LIST: [(&str, &str); 158] = [
    ("ai-zh", "中文（自动生成）"),
    ("ai-en", "英语（自动生成）"),
    ("zh", "中文"),
    ("zh-Hant", "中文（繁体）"),
    ("zh-HK", "中文（中国香港）"),
    ("zh-TW", "中文（中国台湾）"),
    ("zh-SG", "中文（新加坡）"),
    ("en", "英语"),
    ("ja", "日语"),
    ("ko", "韩语"),
    ("es", "西班牙语"),
    ("ar", "阿拉伯语"),
    ("pt", "葡萄牙语"),
    ("fr", "法语"),
    ("de", "德语"),
    ("ru", "俄语"),
    ("th", "泰语"),
    ("vi", "越南语"),
    ("yue", "粤语"),
    ("nan", "闽南语"),
    ("hak", "客家语"),
    ("it", "意大利语"),
    ("hi", "印地语"),
    ("ms", "马来语"),
    ("id", "印度尼西亚语"),
    ("tr", "土耳其语"),
    ("fil", "菲律宾语"),
    ("da", "丹麦语"),
    ("nl", "荷兰语"),
    ("eo", "世界语"),
    ("hu", "匈牙利语"),
    ("is", "冰岛语"),
    ("ga", "爱尔兰语"),
    ("ia", "国际语"),
    ("lo", "老挝语"),
    ("la", "拉丁语"),
    ("my", "缅甸语"),
    ("pl", "波兰语"),
    ("sv", "瑞典语"),
    ("uk", "乌克兰语"),
    ("sr", "塞尔维亚语"),
    ("sk", "斯洛伐克语"),
    ("no", "挪威语"),
    ("mn", "蒙古语"),
    ("mi", "毛利语"),
    ("hr", "克罗地亚语"),
    ("iw", "希伯来语"),
    ("fi", "芬兰语"),
    ("el", "希腊语"),
    ("bn", "孟加拉语"),
    ("be", "白俄罗斯语"),
    ("az", "阿塞拜疆语"),
    ("aa", "阿法尔语"),
    ("ab", "阿布哈西亚语"),
    ("af", "南非荷兰语"),
    ("am", "阿姆哈拉语"),
    ("arc", "阿拉米语"),
    ("as", "阿萨姆语"),
    ("ay", "艾马拉语"),
    ("ba", "巴什基尔语"),
    ("bg", "保加利亚语"),
    ("bh", "比哈尔语"),
    ("bi", "比斯拉马语"),
    ("br", "布列塔尼语"),
    ("bs", "波斯尼亚语"),
    ("ca", "加泰罗尼亚语"),
    ("cho", "乔克托语"),
    ("chr", "切罗基语"),
    ("co", "科西嘉语"),
    ("cs", "捷克语"),
    ("cy", "威尔士语"),
    ("dz", "宗卡语"),
    ("et", "爱沙尼亚语"),
    ("eu", "巴斯克语"),
    ("fa", "波斯语"),
    ("ff", "富拉语"),
    ("fj", "斐济语"),
    ("fo", "法罗语"),
    ("fy", "西弗里西亚语"),
    ("gd", "苏格兰盖尔语"),
    ("gl", "加利西亚语"),
    ("gn", "瓜拉尼语"),
    ("gu", "古吉拉特语"),
    ("ha", "豪萨语"),
    ("hy", "亚美尼亚语"),
    ("ig", "伊博语"),
    ("ik", "伊努皮克语"),
    ("iu", "因纽特语"),
    ("jv", "爪哇语"),
    ("ka", "格鲁吉亚语"),
    ("kk", "哈萨克语"),
    ("kl", "格陵兰语"),
    ("km", "高棉语"),
    ("kn", "卡纳达语"),
    ("ks", "克什米尔语"),
    ("ku", "库尔德语"),
    ("ky", "柯尔克孜语"),
    ("lb", "卢森堡语"),
    ("ln", "林加拉语"),
    ("lt", "立陶宛语"),
    ("lus", "米佐语"),
    ("lv", "拉脱维亚语"),
    ("mas", "马赛语"),
    ("mg", "马拉加斯语"),
    ("mk", "马其顿语"),
    ("ml", "马拉雅拉姆语"),
    ("mo", "摩尔多瓦语"),
    ("mr", "马拉地语"),
    ("mt", "马耳他语"),
    ("na", "瑙鲁语"),
    ("ne", "尼泊尔语"),
    ("nv", "纳瓦霍语"),
    ("oc", "奥克语"),
    ("om", "奥罗莫语"),
    ("or", "奥里亚语"),
    ("pa", "旁遮普语"),
    ("ps", "普什图语"),
    ("qu", "克丘亚语"),
    ("rm", "罗曼什语"),
    ("rn", "隆迪语"),
    ("ro", "罗马尼亚语"),
    ("rw", "卢旺达语"),
    ("sa", "梵语"),
    ("scn", "西西里语"),
    ("sd", "信德语"),
    ("sdp", "Sherdukpen"),
    ("sg", "桑戈语"),
    ("sh", "塞尔维亚-克罗地亚语"),
    ("si", "僧伽罗语"),
    ("sl", "斯洛文尼亚语"),
    ("sm", "萨摩亚语"),
    ("sn", "绍纳语"),
    ("so", "索马里语"),
    ("sq", "阿尔巴尼亚语"),
    ("ss", "斯瓦蒂语"),
    ("st", "南索托语"),
    ("su", "巽他语"),
    ("sw", "斯瓦希里语"),
    ("ta", "泰米尔语"),
    ("te", "泰卢固语"),
    ("tg", "塔吉克语"),
    ("ti", "提格利尼亚语"),
    ("tk", "土库曼语"),
    ("tl", "他加禄语"),
    ("tn", "茨瓦纳语"),
    ("to", "汤加语"),
    ("ts", "聪加语"),
    ("tt", "鞑靼语"),
    ("tw", "契维语"),
    ("ug", "维吾尔语"),
    ("ur", "乌尔都语"),
    ("uz", "乌兹别克语"),
    ("vo", "沃拉普克语"),
    ("wo", "沃洛夫语"),
    ("xh", "科萨语"),
    ("yi", "意第绪语"),
    ("yo", "约鲁巴语"),
    ("zu", "祖鲁语"),
];

/// Headless view-model of `Additional.subtitle_language`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SubtitlesLanguageState {
    pub download_specified: bool,
    pub specified_language: Vec<String>,
}

impl SubtitlesLanguageState {
    pub fn load(cfg: &config::ConfigStore) -> Self {
        let value = cfg
            .get("Additional", "subtitle_language")
            .unwrap_or(Value::Null);
        Self::from_value(&value)
    }

    /// `{download_specified: bool, specified_language: [lan]}`; unknown
    /// shapes fall back to "download all, none selected".
    pub fn from_value(value: &Value) -> Self {
        Self {
            download_specified: value
                .get("download_specified")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            specified_language: value
                .get("specified_language")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    pub fn to_value(&self) -> Value {
        json!({
            "download_specified": self.download_specified,
            "specified_language": self.specified_language,
        })
    }

    pub fn save(&self, cfg: &mut config::ConfigStore) -> anyhow::Result<()> {
        cfg.set("Additional", "subtitle_language", self.to_value());
        cfg.save()
    }
}

/// Open the dialog; OK persists and closes.
pub fn open_subtitles_language(
    cfg: config::ConfigStore,
    window: &mut Window,
    cx: &mut gpui_kit::App,
) {
    let catalog = active_catalog(&cfg);
    let title = catalog.tr(CTX, "Customize Subtitle Languages").to_owned();
    let all_label = catalog
        .tr(CTX, "Download all available subtitles")
        .to_owned();
    let specified_label = catalog
        .tr(CTX, "Download only selected subtitles")
        .to_owned();
    let no_label = catalog.tr(CTX, "No.").to_owned();
    let lang_label = catalog.tr(CTX, "Language").to_owned();
    let code_label = catalog.tr(CTX, "Code").to_owned();
    let ok = "OK".to_owned();

    let state = std::rc::Rc::new(std::cell::RefCell::new(SubtitlesLanguageState::load(&cfg)));
    let cfg_cell = std::rc::Rc::new(std::cell::RefCell::new(cfg));

    window.open_dialog(cx, move |dialog, _, _| {
        let st = state.borrow();
        let download_specified = st.download_specified;
        drop(st);

        dialog
            .title(title.clone())
            .w(px(650.))
            .content({
                let state = state.clone();
                let all_label = all_label.clone();
                let specified_label = specified_label.clone();
                let no_label = no_label.clone();
                let lang_label = lang_label.clone();
                let code_label = code_label.clone();
                move |content, _, _| {
                    let st = state.borrow();
                    let selected: Vec<bool> = SUBTITLES_LANGUAGE_LIST
                        .iter()
                        .map(|(lan, _)| st.specified_language.iter().any(|s| s == lan))
                        .collect();
                    drop(st);

                    content.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                RadioGroup::new("subtitle-mode")
                                    .selected_index(if download_specified {
                                        Some(1)
                                    } else {
                                        Some(0)
                                    })
                                    .child(
                                        Radio::new("download-all")
                                            .checked(!download_specified)
                                            .label(all_label.clone())
                                            .on_click({
                                                let state = state.clone();
                                                move |checked, _, _| {
                                                    if *checked {
                                                        state.borrow_mut().download_specified =
                                                            false;
                                                    }
                                                }
                                            }),
                                    )
                                    .child(
                                        Radio::new("download-specified")
                                            .checked(download_specified)
                                            .label(specified_label.clone())
                                            .on_click({
                                                let state = state.clone();
                                                move |checked, _, _| {
                                                    if *checked {
                                                        state.borrow_mut().download_specified =
                                                            true;
                                                    }
                                                }
                                            }),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .pl_2()
                                    .child(div().w(px(40.)).child(Label::new(no_label.clone())))
                                    .child(div().flex_1().child(Label::new(lang_label.clone())))
                                    .child(div().w(px(70.)).child(Label::new(code_label.clone()))),
                            )
                            .child(
                                div()
                                    .id("subtitle-language-list")
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .max_h(px(300.))
                                    .overflow_y_scroll()
                                    .children(SUBTITLES_LANGUAGE_LIST.iter().enumerate().map(
                                        |(i, (lan, doc))| {
                                            let checked = selected[i];
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_2()
                                                .child(
                                                    div()
                                                        .w(px(28.))
                                                        .child(Label::new((i + 1).to_string())),
                                                )
                                                .child(
                                                    Checkbox::new(gpui_kit::SharedString::from(
                                                        format!("lang-{lan}"),
                                                    ))
                                                    .checked(checked)
                                                    .on_click({
                                                        let state = state.clone();
                                                        let lan = (*lan).to_owned();
                                                        move |checked, _, _| {
                                                            let mut st = state.borrow_mut();
                                                            if *checked {
                                                                if !st
                                                                    .specified_language
                                                                    .iter()
                                                                    .any(|s| s == &lan)
                                                                {
                                                                    st.specified_language
                                                                        .push(lan.clone());
                                                                }
                                                            } else {
                                                                st.specified_language
                                                                    .retain(|s| s != &lan);
                                                            }
                                                        }
                                                    }),
                                                )
                                                .child(div().flex_1().child(Label::new(*doc)))
                                                .child(div().w(px(70.)).child(Label::new(*lan)))
                                        },
                                    )),
                            ),
                    )
                }
            })
            .footer(
                DialogFooter::new().child(Button::new("confirm").label(ok.clone()).on_click({
                    let state = state.clone();
                    let cfg_cell = cfg_cell.clone();
                    move |_, window, cx| {
                        let result = state.borrow().save(&mut cfg_cell.borrow_mut());
                        if let Err(e) = result {
                            window.push_notification(
                                gpui_kit::component::notification::Notification::error(
                                    e.to_string(),
                                ),
                                cx,
                            );
                            return;
                        }
                        window.close_dialog(cx);
                    }
                })),
            )
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Unique throwaway config dir per test.
    fn scratch_cfg() -> config::ConfigStore {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "t31-sub-lang-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        config::ConfigStore::with_paths(&dir, "")
    }

    #[test]
    fn table_has_all_158_entries_with_ends_matching_python() {
        assert_eq!(SUBTITLES_LANGUAGE_LIST.len(), 158);
        assert_eq!(SUBTITLES_LANGUAGE_LIST[0], ("ai-zh", "中文（自动生成）"));
        assert_eq!(SUBTITLES_LANGUAGE_LIST[157], ("zu", "祖鲁语"));
    }

    #[test]
    fn table_codes_are_unique() {
        let mut codes: Vec<&str> = SUBTITLES_LANGUAGE_LIST.iter().map(|(l, _)| *l).collect();
        codes.sort_unstable();
        let len = codes.len();
        codes.dedup();
        assert_eq!(codes.len(), len);
    }

    #[test]
    fn state_round_trips_through_the_config_section() {
        let mut cfg = scratch_cfg();
        let mut st = SubtitlesLanguageState::load(&cfg);
        st.download_specified = true;
        st.specified_language = vec!["zh".into(), "ai-zh".into(), "yue".into()];
        st.save(&mut cfg).unwrap();

        let reloaded = SubtitlesLanguageState::load(&cfg);
        assert_eq!(reloaded, st);
        assert_eq!(
            cfg.get("Additional", "subtitle_language").unwrap(),
            serde_json::json!({
                "download_specified": true,
                "specified_language": ["zh", "ai-zh", "yue"],
            })
        );
    }

    #[test]
    fn from_value_defaults_to_download_all() {
        let st = SubtitlesLanguageState::from_value(&Value::Null);
        assert!(!st.download_specified);
        assert!(st.specified_language.is_empty());
    }
}
