//! Download options dialog (ticket 21). The pure logic is [`model`] /
//! [`rule_builder`] / [`units`]; this module adds the gpui dialog view that
//! renders the three pages (`Media Settings` / `Additional Files` /
//! `Download Settings`) with a live preview bar at the bottom — the port of
//! `src/gui/dialog/download_options/dialog.py`.

pub mod model;
pub mod rule_builder;
pub mod units;

use gpui_kit::base::Disableable;
use gpui_kit::base::IndexPath;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::searchable_list::SearchableVec;
use gpui_kit::component::select::{Select, SelectState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::theme::ActiveTheme;
use gpui_kit::component::WindowExt;
use gpui_kit::{
    div, px, rgb, AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Window,
};

use config::ConfigStore;
use naming::naming_rules::RuleStore;
use naming::task_naming::NamingState;
use parsers::preview::{AudioQueryInfo, ChoiceData, PreviewInfo, VideoQueryInfo};

pub use model::{AcceptCheck, DownloadOptionsModel, TAG_COLORS};

/// Dialog page index — the pivot's current item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Media,
    Additional,
    Download,
    RuleBuilder,
}

/// The dialog view: owns the model plus the gpui control entities that need
/// `&mut Window` at construction (selects, inputs). Created **before**
/// `open_dialog` per the builder contract (entities made inside the builder
/// are replaced on every rebuild, losing editing state).
pub struct DownloadOptionsDialog {
    pub model: DownloadOptionsModel,
    pub page: Page,
    /// Episode identity for the source line (`update_source_description`).
    pub info: PreviewInfo,
    /// Naming rules for the filename preview.
    pub rule_store: RuleStore,
    /// Rule Builder page state, seeded from the first rule choice's rule.
    pub rule_builder: rule_builder::RuleBuilderModel,
    /// Shared text-fragment editor input (Rule Builder page).
    pub text_input: Entity<InputState>,
    pub video_quality: Entity<SelectState<SearchableVec<String>>>,
    pub audio_quality: Entity<SelectState<SearchableVec<String>>>,
    pub video_codec: Entity<SelectState<SearchableVec<String>>>,
    pub container: Entity<SelectState<SearchableVec<String>>>,
    pub danmaku_type: Entity<SelectState<SearchableVec<String>>>,
    pub subtitle_type: Entity<SelectState<SearchableVec<String>>>,
    pub path: Entity<InputState>,
}

impl DownloadOptionsDialog {
    pub fn new(
        cfg: &ConfigStore,
        info: &PreviewInfo,
        type_ids: &[i64],
        store: &RuleStore,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let info = info.clone();
        let model = DownloadOptionsModel::open(cfg, &info, type_ids, store, (200, 30300, 20));

        let mk =
            |items: Vec<String>, selected: usize, window: &mut Window, cx: &mut Context<Self>| {
                cx.new(|cx| {
                    SelectState::new(
                        SearchableVec::new(items),
                        Some(IndexPath::new(selected)),
                        window,
                        cx,
                    )
                })
            };
        let vq = mk(
            choice_strings(&model.media.video_quality_choices),
            0,
            window,
            cx,
        );
        let aq = mk(
            choice_strings(&model.media.audio_quality_choices),
            0,
            window,
            cx,
        );
        let vc = mk(
            choice_strings(&model.media.video_codec_choices),
            0,
            window,
            cx,
        );
        let container_items = vec!["mp4".to_string(), "mkv".to_string()];
        let container_sel = usize::from(model.download.video_container == "mkv");
        // Python card formats (card.py:259-345); labels are the raw values,
        // same as the cover-format buttons below.
        let danmaku_items = vec!["xml".into(), "ass".into(), "json".into()];
        let danmaku_sel = danmaku_items
            .iter()
            .position(|s| *s == model.additional.danmaku_type)
            .unwrap_or(0);
        let danmaku_type = mk(danmaku_items, danmaku_sel, window, cx);
        let subtitle_items = vec![
            "srt".into(),
            "lrc".into(),
            "txt".into(),
            "ass".into(),
            "json".into(),
        ];
        let subtitle_sel = subtitle_items
            .iter()
            .position(|s| *s == model.additional.subtitle_type)
            .unwrap_or(0);
        let subtitle_type = mk(subtitle_items, subtitle_sel, window, cx);
        let container = mk(container_items, container_sel, window, cx);
        let path_value = model.download.download_path.clone();
        let path = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_value(path_value, window, cx);
            state
        });

        let mut rule_builder = rule_builder::RuleBuilderModel::default();
        if let Some(type_id) = model.download.rule_choices.first().map(|c| c.type_id) {
            if let Some(rule) = model.download.preview_rule(store, type_id) {
                rule_builder.set_rule(&rule);
            }
        }

        let text_input = cx.new(|cx| InputState::new(window, cx));
        let this = Self {
            model,
            page: Page::Media,
            info: info.clone(),
            rule_store: store.clone(),
            rule_builder,
            text_input,
            video_quality: vq,
            audio_quality: aq,
            video_codec: vc,
            container,
            danmaku_type,
            subtitle_type,
            path,
        };
        subscribe_selects(&this, cx);
        this
    }

    /// Async media-info results landed (`init_media_info` +
    /// `on_query_video_info`/`on_query_audio_info`): refresh the choice
    /// lists, keep the current selection when still offered, and re-render
    /// the descriptions.
    pub fn apply_media_choices(
        &mut self,
        video_choices: ChoiceData,
        audio_choices: ChoiceData,
        video_info: Option<VideoQueryInfo>,
        audio_info: Option<AudioQueryInfo>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keep_selection = |select: &Entity<SelectState<SearchableVec<String>>>,
                              choices: &ChoiceData,
                              current_id: i64,
                              window: &mut Window,
                              cx: &mut Context<Self>| {
            let label = choices
                .iter()
                .find(|(_, id)| *id == current_id)
                .map(|(l, _)| l.clone());
            select.update(cx, |s, cx| {
                s.set_items(SearchableVec::new(choice_strings(choices)), window, cx);
                if let Some(label) = &label {
                    s.set_selected_value(label, window, cx);
                }
            });
        };

        let vq = self.video_quality.clone();
        keep_selection(
            &vq,
            &video_choices,
            self.model.media.video_quality_id,
            window,
            cx,
        );
        let aq = self.audio_quality.clone();
        keep_selection(
            &aq,
            &audio_choices,
            self.model.media.audio_quality_id,
            window,
            cx,
        );

        self.model.media.video_quality_choices = video_choices;
        self.model.media.audio_quality_choices = audio_choices;
        self.model.media.video_info = video_info;
        self.model.media.audio_info = audio_info;
        cx.notify();
    }
}

fn choice_strings(choices: &[(String, i64)]) -> Vec<String> {
    choices.iter().map(|(label, _)| label.clone()).collect()
}

/// Sync the model's selection ids when a select confirms (`on_current_*`
/// handlers); the container select also drives the embed gating via
/// [`DownloadOptionsModel::set_video_container`].
fn subscribe_selects(dialog: &DownloadOptionsDialog, cx: &mut Context<DownloadOptionsDialog>) {
    use gpui_kit::component::select::SelectEvent;

    let video_quality = dialog.video_quality.clone();
    cx.subscribe(&video_quality, |this, _, evt: &SelectEvent<_>, cx| {
        if let SelectEvent::Confirm(Some(label)) = evt {
            if let Some((_, id)) = this
                .model
                .media
                .video_quality_choices
                .iter()
                .find(|(l, _)| l == label)
            {
                this.model.media.video_quality_id = *id;
            }
            cx.notify();
        }
    })
    .detach();

    let audio_quality = dialog.audio_quality.clone();
    cx.subscribe(&audio_quality, |this, _, evt: &SelectEvent<_>, cx| {
        if let SelectEvent::Confirm(Some(label)) = evt {
            if let Some((_, id)) = this
                .model
                .media
                .audio_quality_choices
                .iter()
                .find(|(l, _)| l == label)
            {
                this.model.media.audio_quality_id = *id;
            }
            cx.notify();
        }
    })
    .detach();

    let video_codec = dialog.video_codec.clone();
    cx.subscribe(&video_codec, |this, _, evt: &SelectEvent<_>, cx| {
        if let SelectEvent::Confirm(Some(label)) = evt {
            if let Some((_, id)) = this
                .model
                .media
                .video_codec_choices
                .iter()
                .find(|(l, _)| l == label)
            {
                this.model.media.video_codec_id = *id;
            }
            cx.notify();
        }
    })
    .detach();

    let container = dialog.container.clone();
    cx.subscribe(&container, |this, _, evt: &SelectEvent<_>, cx| {
        if let SelectEvent::Confirm(Some(label)) = evt {
            this.model.set_video_container(label.to_string());
            cx.notify();
        }
    })
    .detach();

    // Changing the format re-evaluates the embed gating (xml/srt/… can't
    // be embedded): the setter only records the value; `cx.notify()`
    // re-renders the disabled states. No reset — Python keeps the embed
    // switches checked and silently skips at runtime.
    let danmaku_type = dialog.danmaku_type.clone();
    cx.subscribe(&danmaku_type, |this, _, evt: &SelectEvent<_>, cx| {
        if let SelectEvent::Confirm(Some(label)) = evt {
            this.model.additional.danmaku_type = label.to_string();
            cx.notify();
        }
    })
    .detach();

    let subtitle_type = dialog.subtitle_type.clone();
    cx.subscribe(&subtitle_type, |this, _, evt: &SelectEvent<_>, cx| {
        if let SelectEvent::Confirm(Some(label)) = evt {
            this.model.additional.subtitle_type = label.to_string();
            cx.notify();
        }
    })
    .detach();

    let text_input = dialog.text_input.clone();
    cx.subscribe(
        &text_input,
        |this, _, evt: &gpui_kit::component::input::InputEvent, cx| {
            if !matches!(evt, gpui_kit::component::input::InputEvent::Change) {
                return;
            }
            if let Some((li, fi)) = this.rule_builder.editing {
                let text = this.text_input.read(cx).value().to_string();
                this.rule_builder.edit_text(li, fi, text);
                cx.notify();
            }
        },
    )
    .detach();
}

impl Render for DownloadOptionsDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let page = self.page;
        let tags = self.model.preview_tags();

        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_4()
            .child(
                TabBar::new("options-pages")
                    .child(
                        Tab::new()
                            .label(cx_of_catalog().tr("DownloadOptionsDialog", "Media Settings")),
                    )
                    .child(
                        Tab::new()
                            .label(cx_of_catalog().tr("DownloadOptionsDialog", "Additional Files")),
                    )
                    .child(
                        Tab::new().label(
                            cx_of_catalog().tr("DownloadOptionsDialog", "Download Settings"),
                        ),
                    )
                    .child(
                        Tab::new()
                            .label(cx_of_catalog().tr("DownloadOptionsDialog", "Rule Builder")),
                    )
                    .selected_index(page as usize)
                    .on_click(cx.listener(|this, ix: &usize, _, cx| {
                        this.page = match ix {
                            0 => Page::Media,
                            1 => Page::Additional,
                            2 => Page::Download,
                            _ => Page::RuleBuilder,
                        };
                        cx.notify();
                    })),
            )
            .child(div().flex_1().child(match page {
                Page::Media => self.render_media(cx).into_any_element(),
                Page::Additional => self.render_additional(cx).into_any_element(),
                Page::Download => self.render_download(cx).into_any_element(),
                Page::RuleBuilder => self.render_rule_builder(cx).into_any_element(),
            }))
            .child(self.render_preview_bar(&tags))
    }
}

impl DownloadOptionsDialog {
    fn render_media(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let m = &self.model.media;
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                Switch::new("media-video-stream")
                    .checked(m.download_video_stream)
                    .label(
                        cx_of_catalog().tr("MediaOptionsCard", "Download standalone video stream"),
                    )
                    .on_change(cx.listener(|this, v: &bool, _, cx| {
                        this.model
                            .media
                            .set_stream_switches(*v, this.model.media.download_audio_stream);
                        cx.notify();
                    })),
            )
            .child(
                Switch::new("media-audio-stream")
                    .checked(m.download_audio_stream)
                    .label(
                        cx_of_catalog().tr("MediaOptionsCard", "Download standalone audio stream"),
                    )
                    .on_change(cx.listener(|this, v: &bool, _, cx| {
                        this.model
                            .media
                            .set_stream_switches(this.model.media.download_video_stream, *v);
                        cx.notify();
                    })),
            )
            .child(
                Switch::new("media-merge")
                    .checked(m.merge_video_audio)
                    .disabled(!(m.download_video_stream && m.download_audio_stream))
                    .label(cx_of_catalog().tr("MediaOptionsCard", "Merge video and audio"))
                    .on_change(cx.listener(|this, v: &bool, _, cx| {
                        this.model.media.set_merge(*v);
                        cx.notify();
                    })),
            )
            .child(
                Switch::new("media-keep-original")
                    .checked(m.keep_original_files)
                    .disabled(
                        !(m.download_video_stream
                            && m.download_audio_stream
                            && m.merge_video_audio),
                    )
                    .label(cx_of_catalog().tr("MediaOptionsCard", "Keep original files"))
                    .on_change(cx.listener(|this, v: &bool, _, cx| {
                        this.model.media.keep_original_files = *v;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(160.))
                            .child(cx_of_catalog().tr("MediaInfoCard", "Video Quality")),
                    )
                    .child(div().flex_1().child(Select::new(&self.video_quality))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(160.))
                            .child(cx_of_catalog().tr("MediaInfoCard", "Audio Quality")),
                    )
                    .child(div().flex_1().child(Select::new(&self.audio_quality))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(160.))
                            .child(cx_of_catalog().tr("MediaInfoCard", "Video Codec")),
                    )
                    .child(div().flex_1().child(Select::new(&self.video_codec))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(160.))
                            .child(cx_of_catalog().tr("DownloadOptionsDialog", "Container")),
                    )
                    .child(div().flex_1().child(Select::new(&self.container))),
            )
            .child(
                div().text_color(cx.theme().muted_foreground).child(
                    self.model
                        .media
                        .video_quality_description(self.model.media_type, cx_of_catalog()),
                ),
            )
            .child(
                div().text_color(cx.theme().muted_foreground).child(
                    self.model
                        .media
                        .audio_quality_description(self.model.media_type, cx_of_catalog()),
                ),
            )
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.model.media.video_codec_description(cx_of_catalog())),
            )
            .child(
                div().text_color(cx.theme().muted_foreground).child(
                    self.model
                        .media
                        .source_description(&self.info, cx_of_catalog()),
                ),
            )
    }

    fn render_additional(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let a = &self.model.additional;
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                Switch::new("add-danmaku")
                    .checked(a.download_danmaku)
                    .label(cx_of_catalog().tr("DanmakuSettingCard", "Download Danmaku"))
                    .on_change(cx.listener(|this, v: &bool, _, cx| {
                        this.model.additional.download_danmaku = *v;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(160.))
                            .child(cx_of_catalog().tr("DanmakuSettingCard", "Danmaku Format")),
                    )
                    .child(div().flex_1().child(Select::new(&self.danmaku_type))),
            )
            .child(
                Switch::new("add-danmaku-embed")
                    .checked(a.embed_danmaku)
                    .label(cx_of_catalog().tr("DanmakuSettingCard", "Embed Danmaku"))
                    .disabled(!a.danmaku_embed_enabled())
                    .on_change(cx.listener(|this, v: &bool, _, cx| {
                        this.model.additional.embed_danmaku = *v;
                        cx.notify();
                    })),
            )
            .child(
                Switch::new("add-danmaku-delete")
                    .checked(a.delete_danmaku_after_embed)
                    .label(
                        cx_of_catalog().tr("DanmakuSettingCard", "Delete Danmaku After Embedding"),
                    )
                    .disabled(!a.danmaku_delete_after_embed_enabled())
                    .on_change(cx.listener(|this, v: &bool, _, cx| {
                        this.model.additional.delete_danmaku_after_embed = *v;
                        cx.notify();
                    })),
            )
            .child(
                Switch::new("add-subtitle")
                    .checked(a.download_subtitle)
                    .label(cx_of_catalog().tr("SubtitleSettingCard", "Download Subtitles"))
                    .on_change(cx.listener(|this, v: &bool, _, cx| {
                        this.model.additional.download_subtitle = *v;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(160.))
                            .child(cx_of_catalog().tr("SubtitleSettingCard", "Subtitle Format")),
                    )
                    .child(div().flex_1().child(Select::new(&self.subtitle_type))),
            )
            .child(
                Switch::new("add-subtitle-embed")
                    .checked(a.embed_subtitle)
                    .label(cx_of_catalog().tr("SubtitleSettingCard", "Embed Subtitles"))
                    .disabled(!a.subtitle_embed_enabled())
                    .on_change(cx.listener(|this, v: &bool, _, cx| {
                        this.model.additional.embed_subtitle = *v;
                        cx.notify();
                    })),
            )
            .child(
                Switch::new("add-subtitle-delete")
                    .checked(a.delete_subtitle_after_embed)
                    .label(
                        cx_of_catalog()
                            .tr("SubtitleSettingCard", "Delete Subtitles After Embedding"),
                    )
                    .disabled(!a.subtitle_delete_after_embed_enabled())
                    .on_change(cx.listener(|this, v: &bool, _, cx| {
                        this.model.additional.delete_subtitle_after_embed = *v;
                        cx.notify();
                    })),
            )
            .child(
                Switch::new("add-cover")
                    .checked(a.download_cover)
                    .label(cx_of_catalog().tr("CoverSettingCard", "Download Cover"))
                    .on_change(cx.listener(|this, v: &bool, _, cx| {
                        this.model.additional.set_download_cover(*v);
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(160.))
                            .child(cx_of_catalog().tr("CoverSettingCard", "Cover Format")),
                    )
                    .child(div().flex_1().flex().gap_1().children(
                        ["jpg", "png", "webp", "avif"].map(|t| {
                            let selected = self.model.additional.cover_type == t;
                            let label = t.to_string();
                            Button::new(gpui_kit::SharedString::from(format!("cover-type-{t}")))
                                .label(label.clone())
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.model.additional.set_cover_type(label.clone());
                                    cx.notify();
                                }))
                                .disabled(selected)
                        }),
                    )),
            )
            .child(
                Switch::new("add-cover-attach")
                    .checked(a.attach_cover)
                    .label(cx_of_catalog().tr("CoverSettingCard", "Embed Cover"))
                    .disabled(!a.cover_attach_enabled())
                    .on_change(cx.listener(|this, v: &bool, _, cx| {
                        this.model.additional.set_attach_cover(*v);
                        cx.notify();
                    })),
            )
            .child(
                Switch::new("add-cover-delete")
                    .checked(a.delete_cover_after_attach)
                    .label(cx_of_catalog().tr("CoverSettingCard", "Delete Cover After Embedding"))
                    .disabled(!a.cover_delete_after_attach_enabled())
                    .on_change(cx.listener(|this, v: &bool, _, cx| {
                        this.model.additional.delete_cover_after_attach = *v;
                        cx.notify();
                    })),
            )
            .child(
                Switch::new("add-chapter")
                    .checked(a.embed_chapter)
                    .label(cx_of_catalog().tr("ChapterSettingCard", "Embed Chapters"))
                    .on_change(cx.listener(|this, v: &bool, _, cx| {
                        this.model.additional.embed_chapter = *v;
                        cx.notify();
                    })),
            )
            .child(
                Switch::new("add-metadata")
                    .checked(a.download_metadata)
                    .label(cx_of_catalog().tr("MetadataSettingCard", "Download Metadata"))
                    .on_change(cx.listener(|this, v: &bool, _, cx| {
                        this.model.additional.download_metadata = *v;
                        cx.notify();
                    })),
            )
    }

    fn render_download(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(160.))
                            .child(cx_of_catalog().tr("DownloadPathSettingCard", "Download Path")),
                    )
                    .child(div().flex_1().child(Input::new(&self.path))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div().w(px(160.)).child(
                            cx_of_catalog().tr("DownloadFormatCard", "Output Container Format"),
                        ),
                    )
                    .child(div().flex_1().child(Select::new(&self.container))),
            )
            .child(
                Switch::new("download-m4a")
                    .checked(self.model.download.m4a_to_mp3)
                    .label(cx_of_catalog().tr("DownloadFormatCard", "Convert M4A to MP3"))
                    .on_change(cx.listener(|this, v: &bool, _, cx| {
                        this.model.download.m4a_to_mp3 = *v;
                        cx.notify();
                    })),
            )
            .child(
                Switch::new("download-show-dialog")
                    .checked(self.model.download.show_dialog)
                    .label(
                        cx_of_catalog()
                            .tr("DownloadSettingsPage", "Automatically show this dialog"),
                    )
                    .on_change(cx.listener(|this, v: &bool, _, cx| {
                        this.model.download.show_dialog = *v;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(160.))
                            .child(cx_of_catalog().tr("DownloadOptionsDialog", "Metadata format")),
                    )
                    .child(
                        div()
                            .flex_1()
                            .child(self.model.additional.metadata_type.clone()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(160.))
                            .child(cx_of_catalog().tr("DownloadOptionsDialog", "Naming rules")),
                    )
                    .child(div().flex_1().flex().flex_col().gap_1().children(
                        self.model.download.rule_choices.iter().map(|c| {
                            let chosen = c
                                .entries
                                .iter()
                                .find(|(_, id)| Some(id) == c.selected.as_ref())
                                .map(|(name, _)| name.clone())
                                .unwrap_or_else(|| {
                                    cx_of_catalog()
                                        .tr("NamingConventionCard", "Not available")
                                        .into()
                                });
                            div().child(format!("type {} → {}", c.type_id, chosen))
                        }),
                    )),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(160.))
                            .child(cx_of_catalog().tr("DownloadOptionsDialog", "Filename preview")),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                self.model
                                    .filename_preview(
                                        &self.rule_store,
                                        self.model
                                            .download
                                            .rule_choices
                                            .first()
                                            .map(|c| c.type_id)
                                            .unwrap_or(0),
                                    )
                                    .unwrap_or_default(),
                            ),
                    ),
            )
    }

    /// `RuleBuilderWidget`: one chip row per level. A chip click opens the
    /// fragment editor (spec presets for variables); level buttons add a
    /// directory level or move one; the composed rule sits below. The
    /// unsupported case falls back to the raw rule text.
    fn render_rule_builder(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let editing = self.rule_builder.editing;
        let rows = self.rule_builder.model.levels.clone();
        let level_count = rows.len();
        let level_rows = rows
            .iter()
            .enumerate()
            .map(|(li, level)| {
                let chips: Vec<_> = level
                    .fragments
                    .iter()
                    .enumerate()
                    .map(|(fi, f)| {
                        let label = match &f.variable {
                            Some(v) => format!("{{{v}}}"),
                            None => f.text.clone(),
                        };
                        let frag_count = level.fragments.len();
                        let mut chip = div().flex().items_center().gap_1().child(
                            Button::new(gpui_kit::SharedString::from(format!("frag-{li}-{fi}")))
                                .label(label)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.rule_builder.editing = Some((li, fi));
                                    cx.notify();
                                })),
                        );
                        if fi > 0 {
                            chip =
                                chip.child(
                                    Button::new(gpui_kit::SharedString::from(format!(
                                        "frag-lt-{li}-{fi}"
                                    )))
                                    .label("←")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.rule_builder.move_fragment(li, fi, -1);
                                        cx.notify();
                                    })),
                                );
                        }
                        if fi + 1 < frag_count {
                            chip =
                                chip.child(
                                    Button::new(gpui_kit::SharedString::from(format!(
                                        "frag-gt-{li}-{fi}"
                                    )))
                                    .label("→")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.rule_builder.move_fragment(li, fi, 1);
                                        cx.notify();
                                    })),
                                );
                        }
                        chip
                    })
                    .collect::<Vec<_>>();
                let is_file = li + 1 == level_count;
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(div().w(px(160.)).child(if is_file {
                        cx_of_catalog()
                            .tr("RuleBuilderWidget", "File name")
                            .to_string()
                    } else {
                        let t = cx_of_catalog().tr("RuleBuilderWidget", "directory {index}");
                        t.replace("{index}", &li.to_string())
                    }))
                    .children(chips)
                    .children(if is_file {
                        None
                    } else {
                        Some(
                            div()
                                .flex()
                                .gap_1()
                                .child(
                                    Button::new(gpui_kit::SharedString::from(format!(
                                        "lvl-up-{li}"
                                    )))
                                    .label("↑")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.rule_builder.move_level(li, -1);
                                        cx.notify();
                                    })),
                                )
                                .child(
                                    Button::new(gpui_kit::SharedString::from(format!(
                                        "lvl-rm-{li}"
                                    )))
                                    .label("×")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.rule_builder.remove_level(li);
                                        cx.notify();
                                    })),
                                ),
                        )
                    })
            })
            .collect::<Vec<_>>();

        // Fragment editor for the open chip: variables show spec presets;
        // text fragments get add/remove. Variable choice keeps the parsed
        // variable (changing it wholesale re-parses — out of scope here).
        let editor = editing.map(|(li, fi)| {
            let fragment = self
                .rule_builder
                .model
                .levels
                .get(li)
                .and_then(|l| l.fragments.get(fi));
            let Some(fragment) = fragment else {
                return div();
            };
            let Some(variable) = fragment.variable.clone() else {
                // Text fragment: swap it out for a variable chip slot.
                return div()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .w(px(160.))
                            .child(cx_of_catalog().tr("RuleBuilderWidget", "text fragment")),
                    )
                    .child(
                        Button::new(gpui_kit::SharedString::from(format!(
                            "remove-frag-{li}-{fi}"
                        )))
                        .label(cx_of_catalog().tr("FragmentChip", "Remove"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.rule_builder.remove_fragment(li, fi);
                            cx.notify();
                        })),
                    );
            };
            let presets = rule_builder::presets_for(&variable).to_vec();
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(
                    div().w(px(160.)).child(
                        cx_of_catalog()
                            .tr("RuleBuilderWidget", "format ({variable})")
                            .replace("{variable}", &variable),
                    ),
                )
                .children(presets.iter().enumerate().map(|(pi, spec)| {
                    Button::new(gpui_kit::SharedString::from(format!("spec-{li}-{fi}-{pi}")))
                        .label(if spec.is_empty() {
                            cx_of_catalog()
                                .tr("RuleBuilderWidget", "default")
                                .to_string()
                        } else {
                            spec.to_string()
                        })
                        .on_click({
                            let spec = spec.to_string();
                            let prefix = fragment.prefix.clone();
                            let suffix = fragment.suffix.clone();
                            cx.listener(move |this, _, _, cx| {
                                this.rule_builder.edit_variable(
                                    li,
                                    fi,
                                    spec.clone(),
                                    prefix.clone(),
                                    suffix.clone(),
                                );
                                cx.notify();
                            })
                        })
                }))
                .children(["leaf_title", "parent_title", "p", "bvid"].map(|v| {
                    Button::new(gpui_kit::SharedString::from(format!("var-{li}-{fi}-{v}")))
                        .label(format!("{{{v}}}"))
                        .on_click({
                            let v = v.to_string();
                            cx.listener(move |this, _, _, cx| {
                                this.rule_builder.change_variable(li, fi, v.clone());
                                cx.notify();
                            })
                        })
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(
                            Button::new(gpui_kit::SharedString::from(format!("add-var-{li}-{fi}")))
                                .label(
                                    cx_of_catalog()
                                        .tr("RuleBuilderWidget", "Insert title variable"),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.rule_builder.add_variable(li, "leaf_title".into());
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new(gpui_kit::SharedString::from(format!("add-txt-{li}-{fi}")))
                                .label(cx_of_catalog().tr("RuleLevelRow", "Insert text"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.rule_builder.add_text(li, "-".into());
                                    cx.notify();
                                })),
                        ),
                )
        });

        div()
            .flex()
            .flex_col()
            .gap_2()
            .children(level_rows)
            .children(editor)
            .child(
                Button::new("add-level")
                    .label(cx_of_catalog().tr("RuleBuilderWidget", "Add directory level"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.rule_builder.add_level();
                        cx.notify();
                    })),
            )
            .child(div().text_color(cx.theme().muted_foreground).child(
                match &self.rule_builder.unsupported {
                    Some(reason) => {
                        let t =
                            cx_of_catalog().tr("RuleBuilderWidget", "rule not editable: {reason}");
                        t.replace("{reason}", &reason.reason)
                    }
                    None => self.rule_builder.rule(),
                },
            ))
    }

    /// `DownloadPreviewBar`: colored tags for the selected content kinds,
    /// gray "Nothing selected" otherwise.
    fn render_preview_bar(&mut self, tags: &[(&'static str, bool)]) -> impl IntoElement {
        let mut row = div().flex().items_center().gap_1();
        for (key, on) in tags {
            if !*on {
                continue;
            }
            let color = TAG_COLORS
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, c)| *c)
                .unwrap_or("#8A8A8A");
            let label = match *key {
                "video" => cx_of_catalog().tr("DownloadPreviewBar", "Video"),
                "audio" => cx_of_catalog().tr("DownloadPreviewBar", "Audio"),
                "danmaku" => cx_of_catalog().tr("DownloadPreviewBar", "Danmaku"),
                "cover" => cx_of_catalog().tr("DownloadPreviewBar", "Cover"),
                "chapter" => cx_of_catalog().tr("DownloadPreviewBar", "Chapters"),
                "metadata" => cx_of_catalog().tr("DownloadPreviewBar", "Metadata"),
                _ => cx_of_catalog().tr("DownloadPreviewBar", "Nothing selected"),
            };
            row = row.child(
                div()
                    .px_2()
                    .py(px(1.))
                    .rounded_sm()
                    .text_color(parse_hex(color))
                    .child(label),
            );
        }
        row
    }
}

fn parse_hex(hex: &str) -> gpui_kit::Hsla {
    let n = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0x8a8a8a);
    rgb(n).into()
}

/// The catalog the descriptions render against: the app-global
/// `ShellI18n` set at startup and kept current by the language setting
/// (ticket 36 instant apply). No config file read per call — the old
/// `ConfigStore::open()` ran a full file read+parse at every one of the
/// ~30 render sites.
fn cx_of_catalog() -> &'static services::i18n::Catalog {
    services::i18n::catalog(shell_lang().resolve())
}

/// Render language, resolved once per launch and flipped by the settings
/// screen (ticket 36 instant apply) — zero config reads on the render path.
fn shell_lang() -> services::i18n::Language {
    SHELL_LANG.with(std::cell::Cell::get)
}

thread_local! {
    /// Mirror of the `ShellI18n` app global, readable from the plain
    /// helper fns above (they have no `cx`). `Shell::new` installs it;
    /// `apply_language` keeps it current.
    static SHELL_LANG: std::cell::Cell<services::i18n::Language> =
        std::cell::Cell::new(services::i18n::Language::from_config("Auto"));
}

/// Install the render language (`Shell::new` and every language change).
pub fn set_shell_lang(lang: services::i18n::Language) {
    SHELL_LANG.with(|slot| slot.set(lang));
}

/// Open the dialog on a window: builds the entity first (per the builder
/// contract — entities made inside the builder are replaced on every rebuild,
/// losing editing state), then hands the dialog shell title/content/footer.
/// The footer buttons are explicit: a plain `Dialog` never renders buttons
/// from `button_props` (those only install the OK/Cancel callbacks), so the
/// previous empty `DialogFooter::new()` left the dialog with no Download
/// button at all.
///
/// `on_accept` runs only after the footer Download button commits — the
/// dialog gates whatever follows it (task queueing), so Cancel queues
/// nothing. `FnOnce` + `take()`: the builder may re-run, the continuation
/// must fire at most once.
#[allow(clippy::too_many_arguments)]
pub fn open_download_options(
    window: &mut Window,
    cx: &mut Context<crate::ui::Shell>,
    cfg: &ConfigStore,
    info: &PreviewInfo,
    type_ids: &[i64],
    store: &RuleStore,
    candidates: Vec<parsers::tree::TreeItem>,
    on_accept: impl FnOnce(&mut gpui_kit::App) + 'static,
) -> Entity<DownloadOptionsDialog> {
    let dialog = cx.new(|cx| DownloadOptionsDialog::new(cfg, info, type_ids, store, window, cx));

    // `preview_init` + the dialog's own `query_video_info`/`query_audio_info`:
    // fill the choice lists from a real playurl query. The dialog opens on
    // "auto"-only choices; this walk replaces them with the episode's
    // available qualities as soon as the network answers.
    if !candidates.is_empty() {
        let user_agent = cfg
            .get("Advanced", "user_agent")
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| config::DEFAULT_USER_AGENT.to_owned());
        let priorities = crate::download_pipeline::priorities_from_config(cfg);
        // Ticket 34: playurl preview queries are cookie-gated; read the
        // persisted auth state here (the shell lease is held, so the
        // entity must not be re-read) and seed the worker's client.
        let auth_state = cfg
            .path()
            .parent()
            .map(auth::session::config_path)
            .and_then(|p| auth::AuthState::from_config_file(&p))
            .unwrap_or_default();
        let spawn_dialog = dialog.clone();
        window
            .spawn(cx, async move |cx: &mut gpui_kit::AsyncWindowContext| {
                // No EnterGuard here (see bootstrap::on_runtime): the
                // whole fetch walk runs on the shared tokio runtime.
                let Ok(client) = network::HttpClient::new(network::ClientOptions {
                    user_agent,
                    proxy: network::ProxyConfig::none(),
                    cookies_from_persistence: Vec::new(),
                    signer: None,
                }) else {
                    return;
                };
                auth::seed_client_cookies(&client, &auth_state);
                let apis = parsers::playurl::PlayurlApis::default();
                let m = cx
                    .update(|_, cx| spawn_dialog.read(cx).model.media.clone())
                    .ok();
                let (info, video, audio) = crate::bootstrap::on_runtime(move || async move {
                    let mut preview = parsers::preview::Preview::new(priorities);
                    let mut adopted = false;
                    for item in &candidates {
                        if adopted {
                            break;
                        }
                        let Ok(fetched) =
                            parsers::playurl::fetch_media_info(&client, &apis, item, None).await
                        else {
                            continue;
                        };
                        if preview
                            .start(item.attribute, &item.bvid, item.cid, async { Ok(fetched) })
                            .await
                            .is_ok()
                            && !preview.info.error_occurred
                        {
                            adopted = true;
                        }
                    }
                    if !adopted {
                        return (None, None, None);
                    }
                    let Some(m) = m else {
                        return (None, None, None);
                    };
                    let video = preview
                        .query_video(&client, &apis, m.video_quality_id, m.video_codec_id)
                        .await
                        .ok()
                        .flatten();
                    let audio = preview.query_audio(m.audio_quality_id);
                    (Some(preview.info), video, audio)
                })
                .await
                .map(|(i, v, a)| (i, Some(v), Some(a)))
                .unwrap_or((None, None, None));
                let (Some(info), Some(video), Some(audio)) = (info, video, audio) else {
                    return;
                };
                _ = cx.update(|window, cx| {
                    spawn_dialog.update(cx, |d, cx| {
                        d.apply_media_choices(
                            info.video_quality_choices.clone(),
                            info.audio_quality_choices.clone(),
                            video,
                            audio,
                            window,
                            cx,
                        );
                    });
                });
            })
            .detach();
    }
    let for_content = std::rc::Rc::new(dialog.clone());
    let on_accept = std::rc::Rc::new(std::cell::RefCell::new(Some(on_accept)));
    window.open_dialog(cx, move |d, _w, _a| {
        let entity = for_content.clone();
        d.title(cx_of_catalog().tr("DownloadOptionsDialog", "Download Options"))
            // Python dialog is 750x500 (dialog.py QSize) — the gpui default
            // 448px cramps the label column and the selects.
            .w(px(750.))
            .content(move |content, _w, _a| content.child((*entity).clone()))
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("cancel")
                            .label(cx_of_catalog().tr("DownloadOptionsDialog", "Cancel"))
                            .on_click(|_, window, cx| {
                                window.close_dialog(cx);
                            }),
                    )
                    .child(
                        Button::new("ok")
                            .label(cx_of_catalog().tr("DownloadOptionsDialog", "Download"))
                            .primary()
                            .on_click({
                                let entity = for_content.clone();
                                let on_accept = on_accept.clone();
                                move |_, window, cx| {
                                    if confirm_ok(&entity, cx) {
                                        window.close_dialog(cx);
                                        if let Some(cb) = on_accept.borrow_mut().take() {
                                            cb(cx);
                                        }
                                    }
                                }
                            }),
                    ),
            )
    });
    dialog
}

/// OK handler: commit on `AcceptCheck::Ok`, otherwise leave the config
/// untouched.
fn confirm_ok(dialog: &Entity<DownloadOptionsDialog>, cx: &mut gpui_kit::App) -> bool {
    let model = dialog.read(cx).model.clone();
    if !matches!(model.accept_check(), AcceptCheck::Ok) {
        return false;
    }
    let Ok(mut cfg) = ConfigStore::open() else {
        return false;
    };
    let mut naming = NamingState::default();
    model.commit(&mut cfg, &mut naming);
    cfg.save().ok();
    true
}

#[cfg(test)]
mod tests;
