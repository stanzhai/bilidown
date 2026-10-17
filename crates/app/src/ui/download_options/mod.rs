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
use gpui_kit::component::WindowExt;
use gpui_kit::{
    div, px, rgb, AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Window,
};

use config::ConfigStore;
use naming::naming_rules::RuleStore;
use naming::task_naming::NamingState;
use parsers::preview::PreviewInfo;

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
            path,
        };
        subscribe_selects(&this, cx);
        this
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
                    .child(Tab::new().label("Media Settings"))
                    .child(Tab::new().label("Additional Files"))
                    .child(Tab::new().label("Download Settings"))
                    .child(Tab::new().label("Rule Builder"))
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
                    .label("Download standalone video stream")
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
                    .label("Download standalone audio stream")
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
                    .label("Merge video and audio")
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
                    .label("Keep original files")
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
                    .child(div().w(px(160.)).child("Video Quality"))
                    .child(div().flex_1().child(Select::new(&self.video_quality))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().w(px(160.)).child("Audio Quality"))
                    .child(div().flex_1().child(Select::new(&self.audio_quality))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().w(px(160.)).child("Video Codec"))
                    .child(div().flex_1().child(Select::new(&self.video_codec))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().w(px(160.)).child("Container"))
                    .child(div().flex_1().child(Select::new(&self.container))),
            )
            .child(
                div().text_color(rgb(0x8a8a8a)).child(
                    self.model
                        .media
                        .video_quality_description(self.model.media_type, cx_of_catalog()),
                ),
            )
            .child(
                div().text_color(rgb(0x8a8a8a)).child(
                    self.model
                        .media
                        .audio_quality_description(self.model.media_type, cx_of_catalog()),
                ),
            )
            .child(
                div()
                    .text_color(rgb(0x8a8a8a))
                    .child(self.model.media.video_codec_description(cx_of_catalog())),
            )
            .child(
                div().text_color(rgb(0x8a8a8a)).child(
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
                    .label("Download Danmaku"),
            )
            .child(
                Switch::new("add-danmaku-embed")
                    .checked(a.embed_danmaku)
                    .label("Embed Danmaku")
                    .disabled(!a.danmaku_embed_enabled()),
            )
            .child(
                Switch::new("add-danmaku-delete")
                    .checked(a.delete_danmaku_after_embed)
                    .label("Delete Danmaku After Embedding")
                    .disabled(!a.danmaku_delete_after_embed_enabled()),
            )
            .child(
                Switch::new("add-subtitle")
                    .checked(a.download_subtitle)
                    .label("Download Subtitles"),
            )
            .child(
                Switch::new("add-subtitle-embed")
                    .checked(a.embed_subtitle)
                    .label("Embed Subtitles")
                    .disabled(!a.subtitle_embed_enabled()),
            )
            .child(
                Switch::new("add-subtitle-delete")
                    .checked(a.delete_subtitle_after_embed)
                    .label("Delete Subtitles After Embedding")
                    .disabled(!a.subtitle_delete_after_embed_enabled()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().w(px(160.)).child("Cover format"))
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
                    .label("Embed Cover")
                    .disabled(!a.cover_attach_enabled()),
            )
            .child(
                Switch::new("add-cover-delete")
                    .checked(a.delete_cover_after_attach)
                    .label("Delete Cover After Embedding")
                    .disabled(!a.cover_delete_after_attach_enabled()),
            )
            .child(
                Switch::new("add-chapter")
                    .checked(a.embed_chapter)
                    .label("Embed Chapters"),
            )
            .child(
                Switch::new("add-metadata")
                    .checked(a.download_metadata)
                    .label("Download Metadata"),
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
                    .child(div().w(px(160.)).child("Download Path"))
                    .child(div().flex_1().child(Input::new(&self.path))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().w(px(160.)).child("Output Container Format"))
                    .child(div().flex_1().child(Select::new(&self.container))),
            )
            .child(
                Switch::new("download-m4a")
                    .checked(self.model.download.m4a_to_mp3)
                    .label("Convert M4A to MP3")
                    .on_change(cx.listener(|this, v: &bool, _, cx| {
                        this.model.download.m4a_to_mp3 = *v;
                        cx.notify();
                    })),
            )
            .child(
                Switch::new("download-show-dialog")
                    .checked(self.model.download.show_dialog)
                    .label("Automatically show this dialog")
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
                    .child(div().w(px(160.)).child("Metadata format"))
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
                    .child(div().w(px(160.)).child("Naming rules"))
                    .child(div().flex_1().flex().flex_col().gap_1().children(
                        self.model.download.rule_choices.iter().map(|c| {
                            let chosen = c
                                .entries
                                .iter()
                                .find(|(_, id)| Some(id) == c.selected.as_ref())
                                .map(|(name, _)| name.clone())
                                .unwrap_or_else(|| "Not available".into());
                            div().child(format!("type {} → {}", c.type_id, chosen))
                        }),
                    )),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().w(px(160.)).child("Filename preview"))
                    .child(
                        div().flex_1().text_color(rgb(0x8a8a8a)).child(
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
                        "file name".to_string()
                    } else {
                        format!("directory {li}")
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
                    .gap_1()
                    .child(div().w(px(160.)).child("text fragment"))
                    .child(
                        Button::new(gpui_kit::SharedString::from(format!(
                            "remove-frag-{li}-{fi}"
                        )))
                        .label("Remove")
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
                .child(div().w(px(160.)).child(format!("format ({variable})")))
                .children(presets.iter().enumerate().map(|(pi, spec)| {
                    Button::new(gpui_kit::SharedString::from(format!("spec-{li}-{fi}-{pi}")))
                        .label(if spec.is_empty() {
                            "default".to_string()
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
                                .label("Insert title variable")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.rule_builder.add_variable(li, "leaf_title".into());
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new(gpui_kit::SharedString::from(format!("add-txt-{li}-{fi}")))
                                .label("Insert text")
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
                    .label("Add directory level")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.rule_builder.add_level();
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .text_color(rgb(0x8a8a8a))
                    .child(match &self.rule_builder.unsupported {
                        Some(reason) => format!("rule not editable: {}", reason.reason),
                        None => self.rule_builder.rule(),
                    }),
            )
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
                "video" => "Video",
                "audio" => "Audio",
                "danmaku" => "Danmaku",
                "cover" => "Cover",
                "chapter" => "Chapters",
                "metadata" => "Metadata",
                _ => "Nothing selected",
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

/// The catalog the descriptions render against; the app-wide language
/// choice lives with the shell, so this resolves it once per process.
// ponytail: hard "Auto" locale — wire to the shell's Interface.language
// when the settings screen owns the catalog globally.
fn cx_of_catalog() -> &'static services::i18n::Catalog {
    services::i18n::catalog(services::i18n::Language::from_config("Auto").resolve())
}

/// Open the dialog on a window: builds the entity first (per the builder
/// contract — entities made inside the builder are replaced on every rebuild,
/// losing editing state), then hands the dialog shell title/content/footer.
/// The footer buttons are explicit: a plain `Dialog` never renders buttons
/// from `button_props` (those only install the OK/Cancel callbacks), so the
/// previous empty `DialogFooter::new()` left the dialog with no Download
/// button at all.
pub fn open_download_options(
    window: &mut Window,
    cx: &mut Context<crate::ui::Shell>,
    cfg: &ConfigStore,
    info: &PreviewInfo,
    type_ids: &[i64],
    store: &RuleStore,
) -> Entity<DownloadOptionsDialog> {
    let dialog = cx.new(|cx| DownloadOptionsDialog::new(cfg, info, type_ids, store, window, cx));
    let for_content = std::rc::Rc::new(dialog.clone());
    window.open_dialog(cx, move |d, _w, _a| {
        let entity = for_content.clone();
        d.title("Download Options")
            .content(move |content, _w, _a| content.child((*entity).clone()))
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("cancel")
                            .label("Cancel")
                            .on_click(|_, window, cx| {
                                window.close_dialog(cx);
                            }),
                    )
                    .child(Button::new("ok").label("Download").primary().on_click({
                        let entity = for_content.clone();
                        move |_, window, cx| {
                            if confirm_ok(&entity, cx) {
                                window.close_dialog(cx);
                            }
                        }
                    })),
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
