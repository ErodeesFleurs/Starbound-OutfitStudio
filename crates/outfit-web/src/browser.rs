use crate::state::{Epoch, InputKey, InputSource, Latest, Reuse};
#[path = "character.rs"]
mod character;
#[path = "recolor.rs"]
mod recolor;
use futures_util::future::{FutureExt, LocalBoxFuture, Shared};
use js_sys::{Array, Function, Reflect, Uint8Array};
use outfit_core::{
    Garment, GarmentEncoding, HatMetadata, HatRarity, Layer, OptimizedEncoding, ProfileChoice,
    RgbaImage, Sheet, Strategy,
};
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::{Clamped, JsCast, JsValue, closure::Closure};
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{
    Blob, BlobPropertyBag, CanvasRenderingContext2d, Document, DragEvent, Event, EventTarget, File,
    HtmlAnchorElement, HtmlButtonElement, HtmlCanvasElement, HtmlElement, HtmlInputElement,
    HtmlSelectElement, HtmlTextAreaElement, ImageData,
};

type EncodingFuture = Shared<LocalBoxFuture<'static, Result<Rc<EditorEncoding>, String>>>;
enum EditorEncoding {
    Outfit(GarmentEncoding),
    Hat(OptimizedEncoding),
}
impl EditorEncoding {
    fn result(&self) -> &OptimizedEncoding {
        match self {
            Self::Outfit(value) => &value.result,
            Self::Hat(value) => value,
        }
    }
}
type ClipboardFuture = Shared<LocalBoxFuture<'static, Result<bool, String>>>;
type Application = Rc<RefCell<App>>;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditorMode {
    Outfit(Garment),
    Hat,
    Character,
    Recolor,
}

impl EditorMode {
    pub fn key(self) -> &'static str {
        match self {
            Self::Outfit(garment) => garment.key(),
            Self::Hat => "hat",
            Self::Character => "character",
            Self::Recolor => "recolor",
        }
    }
}

const SHEETS: [Sheet; 5] = [
    Sheet::Pants,
    Sheet::Body,
    Sheet::FrontSleeves,
    Sheet::BackSleeves,
    Sheet::Back,
];
const HAT_SLOT: usize = 5;
const SLOT_IDS: [&str; 12] = [
    "pants",
    "body",
    "front",
    "rear",
    "back",
    "hat",
    "character-body",
    "character-front",
    "character-rear",
    "character-head",
    "character-hair",
    "character-emotes",
];
const MODES: [(&str, EditorMode); 7] = [
    ("pants", EditorMode::Outfit(Garment::Pants)),
    ("hidingPants", EditorMode::Outfit(Garment::HidingPants)),
    ("chest", EditorMode::Outfit(Garment::Chest)),
    ("back", EditorMode::Outfit(Garment::Back)),
    ("hat", EditorMode::Hat),
    ("character", EditorMode::Character),
    ("recolor", EditorMode::Recolor),
];

struct Upload {
    id: u64,
    image: Rc<RgbaImage>,
}

struct ReadPng {
    image: RgbaImage,
    source_width: usize,
    source_height: usize,
}

struct Slot {
    input: HtmlInputElement,
    clear: HtmlButtonElement,
    dropzone: HtmlElement,
    canvas: HtmlCanvasElement,
    status: HtmlElement,
    epoch: Epoch,
    upload: Option<Upload>,
}

struct Generated {
    text: Rc<String>,
    filename: String,
    command: bool,
}

struct EncodingInput {
    key: InputKey,
    images: EncodingImages,
}

enum EncodingImages {
    Outfit {
        garment: Garment,
        profile: ProfileChoice,
        layers: Vec<(Sheet, Rc<RgbaImage>)>,
    },
    Hat {
        image: Rc<RgbaImage>,
    },
}

struct HatUi {
    name: HtmlInputElement,
    description: HtmlTextAreaElement,
    rarity: HtmlSelectElement,
    remove_hair: HtmlInputElement,
    hide_body: HtmlInputElement,
    frame: HtmlCanvasElement,
    alignment: HtmlCanvasElement,
    character: HtmlCanvasElement,
    hair: HtmlCanvasElement,
    geometry: HtmlElement,
}

struct Ui {
    document: Document,
    modes: Vec<HtmlButtonElement>,
    panels: Vec<HtmlElement>,
    profile: HtmlSelectElement,
    strategy: HtmlSelectElement,
    palette: HtmlInputElement,
    passes_note: HtmlElement,
    profile_note: HtmlElement,
    hat: HatUi,
    character: character::CharacterUi,
    recolor: recolor::RecolorUi,
    export_buttons: Vec<HtmlButtonElement>,
    garment_name: HtmlElement,
    notice: HtmlElement,
    result: HtmlElement,
    stats: HtmlElement,
    output: HtmlTextAreaElement,
    download_last: HtmlButtonElement,
    copy_last: HtmlButtonElement,
}

struct App {
    ui: Ui,
    slots: Vec<Slot>,
    mode: EditorMode,
    hat_metadata: HatMetadata,
    profile: ProfileChoice,
    palette: bool,
    strategy: Strategy,
    request: Epoch,
    next_image_id: u64,
    cache: Latest<EncodingFuture>,
    character_cache: Latest<character::CharacterFuture>,
    character_head: Option<character::DerivedHead>,
    recolor_cache: Latest<recolor::RecolorFuture>,
    recolor_pairs: Vec<recolor::Pair>,
    next_pair_id: u64,
    generated: Option<Generated>,
    clipboard_tail: Option<ClipboardFuture>,
}

fn element<T: JsCast>(document: &Document, id: &str) -> Result<T, JsValue> {
    document
        .get_element_by_id(id)
        .ok_or_else(|| JsValue::from_str(&format!("缺少页面元素：{id}")))?
        .dyn_into::<T>()
        .map_err(|_| JsValue::from_str(&format!("页面元素类型不正确：{id}")))
}

pub(crate) fn start() -> Result<(), JsValue> {
    let document = web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| JsValue::from_str("无法访问浏览器页面。"))?;
    populate_slots(&document)?;
    populate_templates(&document)?;
    let mut slots = Vec::new();
    for id in SLOT_IDS {
        slots.push(Slot {
            input: element(&document, &format!("input-{id}"))?,
            clear: element(&document, &format!("clear-{id}"))?,
            dropzone: element(&document, &format!("drop-{id}"))?,
            canvas: element(&document, &format!("preview-{id}"))?,
            status: element(&document, &format!("status-{id}"))?,
            epoch: Epoch::default(),
            upload: None,
        });
    }
    let ui = Ui {
        modes: MODES
            .iter()
            .map(|(id, _)| element(&document, &format!("mode-{id}")))
            .collect::<Result<_, _>>()?,
        panels: [
            "pants-panel",
            "chest-panel",
            "back-panel",
            "hat-panel",
            "character-panel",
            "recolor-panel",
        ]
        .iter()
        .map(|id| element(&document, id))
        .collect::<Result<_, _>>()?,
        profile: element(&document, "asset-profile")?,
        strategy: element(&document, "optimization-strategy")?,
        palette: element(&document, "palette-enabled")?,
        passes_note: element(&document, "passes-note")?,
        profile_note: element(&document, "profile-note")?,
        hat: HatUi {
            name: element(&document, "hat-name")?,
            description: element(&document, "hat-description")?,
            rarity: element(&document, "hat-rarity")?,
            remove_hair: element(&document, "hat-remove-hair")?,
            hide_body: element(&document, "hat-hide-body")?,
            frame: element(&document, "hat-frame")?,
            alignment: element(&document, "hat-alignment")?,
            character: preview_sprite(&document, include_bytes!("../assets/hatter-character.png"))?,
            hair: preview_sprite(&document, include_bytes!("../assets/hatter-hair.png"))?,
            geometry: element(&document, "hat-geometry")?,
        },
        character: character::CharacterUi::new(&document)?,
        recolor: recolor::RecolorUi::new(&document)?,
        export_buttons: [
            "download-json",
            "copy-json",
            "download-command",
            "copy-command",
        ]
        .iter()
        .map(|id| element(&document, id))
        .collect::<Result<_, _>>()?,
        garment_name: element(&document, "garment-name")?,
        notice: element(&document, "notice")?,
        result: element(&document, "result")?,
        stats: element(&document, "stats")?,
        output: element(&document, "output")?,
        download_last: element(&document, "download-last")?,
        copy_last: element(&document, "copy-last")?,
        document: document.clone(),
    };
    let profile = ui
        .profile
        .value()
        .parse::<ProfileChoice>()
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    let app = Rc::new(RefCell::new(App {
        ui,
        slots,
        mode: EditorMode::Outfit(Garment::Pants),
        hat_metadata: HatMetadata::default(),
        profile,
        palette: true,
        strategy: Strategy::Text,
        request: Epoch::default(),
        next_image_id: 0,
        cache: Latest::new(),
        character_cache: Latest::new(),
        character_head: None,
        recolor_cache: Latest::new(),
        recolor_pairs: Vec::new(),
        next_pair_id: 0,
        generated: None,
        clipboard_tail: None,
    }));
    attach_events(&app)?;
    character::attach_events(&app)?;
    recolor::attach_events(&app)?;
    recolor::add_pair(&app)?;
    app.borrow_mut().refresh_mode();
    app.borrow()
        .draw_hat_preview()
        .map_err(|error| JsValue::from_str(&error))?;
    element::<HtmlElement>(&document, "startup")?.set_hidden(true);
    element::<HtmlElement>(&document, "application")?.set_hidden(false);
    Ok(())
}

fn listen(
    target: &EventTarget,
    event: &str,
    callback: impl FnMut(Event) + 'static,
) -> Result<(), JsValue> {
    let closure = Closure::<dyn FnMut(Event)>::new(callback);
    target.add_event_listener_with_callback(event, closure.as_ref().unchecked_ref())?;
    // These are the fixed page-lifetime listeners, not one listener per render/upload.
    closure.forget();
    Ok(())
}

fn attach_events(app: &Application) -> Result<(), JsValue> {
    for (index, (_, mode)) in MODES.iter().enumerate() {
        let target: EventTarget = app.borrow().ui.modes[index].clone().into();
        let app = Rc::clone(app);
        let mode = *mode;
        listen(&target, "click", move |_| {
            let mut app = app.borrow_mut();
            if app.mode != mode {
                app.mode = mode;
                app.invalidate();
                app.refresh_mode();
            }
        })?;
    }
    for id in ["asset-profile", "optimization-strategy", "palette-enabled"] {
        let target: EventTarget = app
            .borrow()
            .ui
            .document
            .get_element_by_id(id)
            .ok_or_else(|| JsValue::from_str("缺少设置控件。"))?
            .into();
        let app = Rc::clone(app);
        listen(&target, "change", move |_| {
            let mut app = app.borrow_mut();
            let profile = app.ui.profile.value().parse::<ProfileChoice>();
            let strategy = app.ui.strategy.value().parse::<Strategy>();
            match (profile, strategy) {
                (Ok(profile), Ok(strategy)) => {
                    app.profile = profile;
                    app.strategy = strategy;
                    app.palette = app.ui.palette.checked();
                    app.invalidate();
                    app.refresh_settings();
                }
                _ => app.show_notice("无法识别所选优化设置。", true),
            }
        })?;
    }
    for index in 0..SLOT_IDS.len() {
        let (input, clear, dropzone) = {
            let app = app.borrow();
            let slot = &app.slots[index];
            (
                slot.input.clone(),
                slot.clear.clone(),
                slot.dropzone.clone(),
            )
        };
        let input_app = Rc::clone(app);
        let input_target: EventTarget = input.clone().into();
        listen(&input_target, "change", move |_| {
            if let Some(file) = input.files().and_then(|files| files.get(0)) {
                upload(&input_app, index, file);
            } else {
                clear_slot(&input_app, index);
            }
        })?;
        let clear_app = Rc::clone(app);
        listen(&clear.into(), "click", move |_| {
            clear_slot(&clear_app, index)
        })?;
        let over_target: EventTarget = dropzone.clone().into();
        let over_zone = dropzone.clone();
        listen(&over_target, "dragover", move |event| {
            event.prevent_default();
            over_zone.set_class_name("upload-card drag-over");
        })?;
        let leave_zone = dropzone.clone();
        listen(&over_target, "dragleave", move |_| {
            leave_zone.set_class_name("upload-card")
        })?;
        let drop_app = Rc::clone(app);
        listen(&dropzone.clone().into(), "drop", move |event| {
            event.prevent_default();
            dropzone.set_class_name("upload-card");
            if let Some(file) = event
                .dyn_ref::<DragEvent>()
                .and_then(|event| event.data_transfer())
                .and_then(|transfer| transfer.files())
                .and_then(|files| files.get(0))
            {
                upload(&drop_app, index, file);
            }
        })?;
    }
    for id in [
        "hat-name",
        "hat-description",
        "hat-rarity",
        "hat-remove-hair",
        "hat-hide-body",
    ] {
        let target: EventTarget = app
            .borrow()
            .ui
            .document
            .get_element_by_id(id)
            .ok_or_else(|| JsValue::from_str("缺少帽子控件。"))?
            .into();
        let app = Rc::clone(app);
        let event = if matches!(id, "hat-name" | "hat-description") {
            "input"
        } else {
            "change"
        };
        listen(&target, event, move |_| {
            let mut app = app.borrow_mut();
            let Ok(rarity) = app.ui.hat.rarity.value().parse::<HatRarity>() else {
                app.show_notice("无法识别帽子稀有度。", true);
                return;
            };
            app.hat_metadata = HatMetadata {
                shortdescription: app.ui.hat.name.value(),
                description: app.ui.hat.description.value(),
                rarity,
                remove_hair: app.ui.hat.remove_hair.checked(),
                hide_body: app.ui.hat.hide_body.checked(),
            };
            // Descriptor edits retire old exports, not reusable pixel encoding.
            app.request.advance();
            app.reset_result();
            if let Err(error) = app.draw_hat_preview() {
                app.show_notice(&error, true);
            }
        })?;
    }
    for (index, (command, copy)) in [(false, false), (false, true), (true, false), (true, true)]
        .into_iter()
        .enumerate()
    {
        let target: EventTarget = app.borrow().ui.export_buttons[index].clone().into();
        let app = Rc::clone(app);
        listen(&target, "click", move |_| export(&app, command, copy))?;
    }
    let target: EventTarget = app.borrow().ui.download_last.clone().into();
    let download_app = Rc::clone(app);
    listen(&target, "click", move |_| {
        use_generated(&download_app, false)
    })?;
    let target: EventTarget = app.borrow().ui.copy_last.clone().into();
    let copy_app = Rc::clone(app);
    listen(&target, "click", move |_| use_generated(&copy_app, true))?;
    // Prevent dropped files outside a card from navigating away and losing all work.
    let target: EventTarget = app.borrow().ui.document.clone().into();
    listen(&target, "dragover", |event| event.prevent_default())?;
    listen(&target, "drop", |event| event.prevent_default())?;
    Ok(())
}

impl App {
    fn invalidate(&mut self) {
        self.request.advance();
        self.cache.clear();
        self.character_cache.clear();
        self.recolor_cache.clear();
        self.reset_result();
        self.refresh_character_head();
        self.refresh_actions();
        self.ui
            .recolor
            .validation
            .set_text_content(Some("输入变化后需重新生成；所有配对将联合检查完整 RGBA。"));
        if self.mode == EditorMode::Hat
            && let Err(error) = self.draw_hat_preview()
        {
            self.show_notice(&error, true);
        }
        if self.mode == EditorMode::Character
            && let Err(error) = self.draw_character_preview()
        {
            self.show_notice(&error, true);
        }
    }

    fn reset_result(&mut self) {
        self.generated = None;
        self.ui.result.set_hidden(true);
        self.ui.output.set_value("");
        self.ui.notice.set_hidden(true);
        self.ui.notice.set_text_content(None);
        self.ui.download_last.set_disabled(true);
        self.ui.copy_last.set_disabled(true);
    }

    fn show_notice(&self, text: &str, failed: bool) {
        self.ui
            .notice
            .set_class_name(if failed { "notice error" } else { "notice" });
        self.ui.notice.set_text_content(Some(text));
        self.ui.notice.set_hidden(false);
    }

    fn selected_indices(&self) -> &'static [usize] {
        match self.mode {
            EditorMode::Outfit(Garment::Pants | Garment::HidingPants) => &[0],
            EditorMode::Outfit(Garment::Chest) => &[1, 2, 3],
            EditorMode::Outfit(Garment::Back) => &[4],
            EditorMode::Hat => &[HAT_SLOT],
            EditorMode::Character => &[6, 7, 8, 9, 10, 11],
            EditorMode::Recolor => &[],
        }
    }

    fn refresh_actions(&self) {
        let available = if self.mode == EditorMode::Recolor {
            self.recolor_ready()
        } else if self.mode == EditorMode::Character {
            self.character_ready()
        } else {
            self.selected_indices()
                .iter()
                .any(|&index| self.slots[index].upload.is_some())
        };
        let command_available = available
            && (self.mode != EditorMode::Character || self.ui.character.allows_command_export());
        // JSON actions precede command actions in Ui::export_buttons.
        for (index, button) in self.ui.export_buttons.iter().enumerate() {
            button.set_hidden(self.mode == EditorMode::Recolor);
            button.set_disabled(if index < 2 {
                !available
            } else {
                !command_available
            });
        }
        for button in &self.ui.recolor.actions {
            button.set_disabled(self.mode != EditorMode::Recolor || !available);
        }
    }

    fn refresh_mode(&mut self) {
        for (index, (_, mode)) in MODES.iter().enumerate() {
            let selected = self.mode == *mode;
            self.ui.modes[index].set_class_name(if selected { "mode selected" } else { "mode" });
            self.ui.modes[index]
                .set_attribute("aria-pressed", if selected { "true" } else { "false" })
                .expect("static aria-pressed attribute");
        }
        let panel = match self.mode {
            EditorMode::Outfit(Garment::Pants | Garment::HidingPants) => 0,
            EditorMode::Outfit(Garment::Chest) => 1,
            EditorMode::Outfit(Garment::Back) => 2,
            EditorMode::Hat => 3,
            EditorMode::Character => 4,
            EditorMode::Recolor => 5,
        };
        for (index, element) in self.ui.panels.iter().enumerate() {
            element.set_hidden(index != panel);
        }
        self.ui
            .garment_name
            .set_text_content(Some(editor_name(self.mode)));
        self.refresh_settings();
        self.refresh_actions();
        self.ui
            .character
            .help
            .set_hidden(self.mode != EditorMode::Character);
        self.ui.character.gear_help.set_hidden(matches!(
            self.mode,
            EditorMode::Character | EditorMode::Recolor
        ));
        self.ui
            .recolor
            .help
            .set_hidden(self.mode != EditorMode::Recolor);
        if self.mode == EditorMode::Character
            && let Err(error) = self.draw_character_preview()
        {
            self.show_notice(&error, true);
        }
    }

    fn input(&self) -> Option<EncodingInput> {
        let (source, images) = match self.mode {
            EditorMode::Outfit(garment) => {
                let mut identities = Vec::new();
                let mut layers = Vec::new();
                for &index in self.selected_indices() {
                    if let Some(upload) = &self.slots[index].upload {
                        identities.push((upload.id, SHEETS[index]));
                        layers.push((SHEETS[index], Rc::clone(&upload.image)));
                    }
                }
                if layers.is_empty() {
                    return None;
                }
                (
                    InputSource::Outfit {
                        garment,
                        profile: self.profile,
                        layers: identities,
                    },
                    EncodingImages::Outfit {
                        garment,
                        profile: self.profile,
                        layers,
                    },
                )
            }
            EditorMode::Hat => {
                let upload = self.slots[HAT_SLOT].upload.as_ref()?;
                (
                    InputSource::Hat { image: upload.id },
                    EncodingImages::Hat {
                        image: Rc::clone(&upload.image),
                    },
                )
            }
            EditorMode::Character | EditorMode::Recolor => return None,
        };
        Some(EncodingInput {
            key: InputKey {
                source,
                palette: self.palette,
                strategy: self.strategy,
            },
            images,
        })
    }

    fn refresh_settings(&self) {
        let independent = !matches!(self.mode, EditorMode::Outfit(_));
        let direct = self.mode == EditorMode::Recolor;
        self.ui.strategy.set_disabled(direct);
        self.ui.profile.set_disabled(independent);
        self.ui.profile_note.set_hidden(!independent);
        self.ui
            .palette
            .set_disabled(direct || self.strategy == Strategy::Passes);
        self.ui
            .passes_note
            .set_hidden(direct || self.strategy != Strategy::Passes);
    }

    fn draw_hat_preview(&self) -> Result<(), String> {
        let draw = || -> Result<(), JsValue> {
            let upload = self.slots[HAT_SLOT].upload.as_ref();
            let geometry = upload
                .map(|upload| outfit_core::hat_geometry(&upload.image))
                .transpose()
                .map_err(|error| JsValue::from_str(&error.to_string()))?;
            self.ui.hat.frame.set_hidden(geometry.is_none());
            self.ui.hat.geometry.set_text_content(Some(&match geometry {
                Some(g) if g.cropped => {
                    "86 × 215 精灵表：自动选取右上 43 × 43 帧（x=43，y=0）；其余帧不编码。"
                        .to_owned()
                }
                Some(g) => format!(
                    "实际编码帧：{} × {}；不裁切、不缩放 PNG 像素。",
                    g.width, g.height
                ),
                None => "选择 PNG 后显示实际编码帧；86 × 215 自动选取右上 43 × 43。".to_owned(),
            }));
            let extent = geometry.map_or(86, |g| 86.max(g.width).max(g.height));
            let canvas = &self.ui.hat.alignment;
            // Two backing pixels per logical pixel keep Hatter's half-pixel
            // centering exact for both odd and even PNG dimensions.
            canvas.set_width((extent * 2) as u32);
            canvas.set_height((extent * 2) as u32);
            let context = canvas_context(canvas)?;
            context.set_image_smoothing_enabled(false);
            context.scale(2.0, 2.0)?;
            let origin = (extent as f64 - 43.0) / 2.0;
            context.set_stroke_style_str("#557da6");
            context.set_line_width(0.5);
            context.stroke_rect(origin, origin, 43.0, 43.0);
            if !self.hat_metadata.hide_body {
                context.draw_image_with_html_canvas_element(
                    &self.ui.hat.character,
                    origin,
                    origin,
                )?;
                if !self.hat_metadata.remove_hair {
                    context.draw_image_with_html_canvas_element(
                        &self.ui.hat.hair,
                        origin,
                        origin,
                    )?;
                }
            }
            if let Some(g) = geometry {
                let frame = &self.ui.hat.frame;
                frame.set_width(g.width as u32);
                frame.set_height(g.height as u32);
                let frame_context = canvas_context(frame)?;
                frame_context.set_image_smoothing_enabled(false);
                frame_context
                    .draw_image_with_html_canvas_element_and_sw_and_sh_and_dx_and_dy_and_dw_and_dh(
                        &self.slots[HAT_SLOT].canvas,
                        g.offset_x as f64,
                        g.offset_y as f64,
                        g.width as f64,
                        g.height as f64,
                        0.0,
                        0.0,
                        g.width as f64,
                        g.height as f64,
                    )?;
                context.draw_image_with_html_canvas_element(
                    frame,
                    (extent - g.width) as f64 / 2.0,
                    (extent - g.height) as f64 / 2.0,
                )?;
            }
            Ok(())
        };
        draw().map_err(|error| format!("无法绘制帽子定位示意：{}", js_error(error)))
    }
}

fn clear_slot(app: &Application, index: usize) {
    let mut app = app.borrow_mut();
    let slot = &mut app.slots[index];
    slot.epoch.advance();
    slot.upload = None;
    slot.input.set_value("");
    slot.canvas.set_hidden(true);
    slot.status.set_class_name("file-status");
    slot.status
        .set_text_content(Some("尚未选择 PNG；也可拖放文件到此处。"));
    app.invalidate();
}

fn upload(app: &Application, index: usize, file: File) {
    let ticket = {
        let mut app = app.borrow_mut();
        let slot = &mut app.slots[index];
        let ticket = slot.epoch.advance();
        slot.upload = None;
        // Allow choosing the very same file again after either success or failure.
        slot.input.set_value("");
        slot.canvas.set_hidden(true);
        slot.status.set_class_name("file-status");
        slot.status
            .set_text_content(Some(&format!("正在本地读取 {}…", file.name())));
        app.invalidate();
        ticket
    };
    let app = Rc::clone(app);
    spawn_local(async move {
        let result = read_png(&file, &app, index, ticket).await;
        let mut app = app.borrow_mut();
        if !app.slots[index].epoch.accepts(ticket) {
            return;
        }
        match result {
            Ok(Some(read)) => {
                let ReadPng {
                    image,
                    source_width,
                    source_height,
                } = read;
                if let Err(error) = draw_preview(&app.slots[index].canvas, &image) {
                    app.slots[index].status.set_class_name("file-status error");
                    app.slots[index].status.set_text_content(Some(&error));
                    return;
                }
                app.next_image_id += 1;
                let id = app.next_image_id;
                let slot = &mut app.slots[index];
                slot.status.set_class_name("file-status");
                let status = if (source_width, source_height) == (image.width, image.height) {
                    format!(
                        "{} · {} × {} · 本地已载入",
                        file.name(),
                        image.width,
                        image.height
                    )
                } else {
                    format!(
                        "{} · {} × {} → {} × {}，已自动去除右侧/底部透明边 · 本地已载入",
                        file.name(),
                        source_width,
                        source_height,
                        image.width,
                        image.height
                    )
                };
                slot.status.set_text_content(Some(&status));
                slot.upload = Some(Upload {
                    id,
                    image: Rc::new(image),
                });
                app.invalidate();
            }
            Ok(None) => {}
            Err(error) => {
                let slot = &app.slots[index];
                slot.status.set_class_name("file-status error");
                slot.status.set_text_content(Some(&error));
            }
        }
    });
}

async fn read_png(
    file: &File,
    app: &Application,
    index: usize,
    ticket: u64,
) -> Result<Option<ReadPng>, String> {
    let buffer = JsFuture::from(file.array_buffer())
        .await
        .map_err(|error| format!("无法读取 PNG：{}", js_error(error)))?;
    // A newer file or Clear may have won while File.arrayBuffer was pending.
    // Retire that read before copying or decoding its pixels.
    if !app.borrow().slots[index].epoch.accepts(ticket) {
        return Ok(None);
    }
    let bytes = Uint8Array::new(&buffer).to_vec();
    let mut image =
        outfit_core::decode_png(&bytes).map_err(|error| format!("无法解码 PNG：{error}"))?;
    let source_width = image.width;
    let source_height = image.height;
    if index == HAT_SLOT {
        outfit_core::hat_geometry(&image).map_err(|error| format!("帽子 PNG：{error}"))?;
    } else if index >= character::FIRST_SLOT {
        character::normalize_and_validate_image(index, &mut image)?;
    } else {
        let frames = outfit_core::layout(SHEETS[index]);
        let width = frames.first().map_or(0, |row| row.len()) * 43;
        let height = frames.len() * 43;
        outfit_core::normalize_sheet(&mut image, width, height).map_err(|error| {
            format!("PNG 无法安全去除右侧/底部透明边（不能丢弃可见像素）：{error}")
        })?;
    }
    Ok(Some(ReadPng {
        image,
        source_width,
        source_height,
    }))
}

fn canvas_context(canvas: &HtmlCanvasElement) -> Result<CanvasRenderingContext2d, JsValue> {
    Ok(canvas
        .get_context("2d")?
        .ok_or_else(|| JsValue::from_str("浏览器不支持 Canvas 2D。"))?
        .dyn_into::<CanvasRenderingContext2d>()?)
}

fn preview_sprite(document: &Document, png: &[u8]) -> Result<HtmlCanvasElement, JsValue> {
    let image = outfit_core::decode_png(png)
        .map_err(|error| JsValue::from_str(&format!("无法解码人物预览：{error}")))?;
    let canvas = document
        .create_element("canvas")?
        .dyn_into::<HtmlCanvasElement>()?;
    draw_preview(&canvas, &image).map_err(|error| JsValue::from_str(&error))?;
    Ok(canvas)
}

fn draw_preview(canvas: &HtmlCanvasElement, image: &RgbaImage) -> Result<(), String> {
    let draw = || -> Result<(), JsValue> {
        canvas.set_width(image.width as u32);
        canvas.set_height(image.height as u32);
        let context = canvas
            .get_context("2d")?
            .ok_or_else(|| JsValue::from_str("浏览器不支持 Canvas 2D。"))?
            .dyn_into::<CanvasRenderingContext2d>()?;
        context.set_image_smoothing_enabled(false);
        let data = ImageData::new_with_u8_clamped_array_and_sh(
            Clamped(&image.rgba),
            image.width as u32,
            image.height as u32,
        )?;
        context.put_image_data(&data, 0.0, 0.0)?;
        canvas.set_hidden(false);
        Ok(())
    };
    draw().map_err(|error| format!("无法显示精灵表预览：{}", js_error(error)))
}

fn export(app: &Application, command: bool, copy: bool) {
    if app.borrow().mode == EditorMode::Recolor {
        recolor::generate(app, Some(copy));
        return;
    }
    if app.borrow().mode == EditorMode::Character {
        character::export(app, command, copy);
        return;
    }
    let (ticket, key, metadata, future, reuse, generation) = {
        let mut state = app.borrow_mut();
        let ticket = state.request.advance();
        state.reset_result();
        let Some(EncodingInput { key, images }) = state.input() else {
            state.show_notice("请先选择至少一张 PNG。", true);
            return;
        };
        let metadata = (state.mode == EditorMode::Hat).then(|| state.hat_metadata.clone());
        let (future, reuse, generation) = if let Some(found) = state.cache.get(&key) {
            found
        } else {
            let palette = key.palette;
            let strategy = key.strategy;
            let future = async move {
                let result = match images {
                    EncodingImages::Outfit {
                        garment,
                        profile,
                        layers: images,
                    } => {
                        let layers: Vec<_> = images
                            .iter()
                            .map(|(sheet, image)| Layer {
                                image: image.as_ref(),
                                frames: outfit_core::layout(*sheet),
                            })
                            .collect();
                        let candidates = outfit_core::garment_candidates(
                            &layers, garment, profile, palette, strategy,
                        )
                        .map_err(|error| error.to_string())?;
                        let mut estimates = Vec::with_capacity(candidates.candidates.len());
                        for candidate in &candidates.candidates {
                            estimates.push(deflate_size(&candidate.encoding.directives).await?);
                        }
                        EditorEncoding::Outfit(
                            outfit_core::select_garment(candidates, &estimates, strategy)
                                .map_err(|error| error.to_string())?,
                        )
                    }
                    EncodingImages::Hat { image } => {
                        let candidates = outfit_core::hat_candidates(&image, palette, strategy)
                            .map_err(|error| error.to_string())?;
                        let mut estimates = Vec::with_capacity(candidates.len());
                        for candidate in &candidates {
                            estimates.push(deflate_size(&candidate.directives).await?);
                        }
                        EditorEncoding::Hat(
                            outfit_core::select(candidates, &estimates, strategy)
                                .map_err(|error| error.to_string())?,
                        )
                    }
                };
                Ok(Rc::new(result))
            }
            .boxed_local()
            .shared();
            let (generation, reuse) = state.cache.insert(key.clone(), future.clone());
            (future, reuse, generation)
        };
        state.show_notice(
            match reuse {
                Reuse::New => match key.strategy {
                    Strategy::Compressed => "正在用浏览器 Deflate 比较完整指令候选…",
                    Strategy::Text => "正在比较完整指令原长并估算赢家 Deflate…",
                    Strategy::Passes => "正在生成单像素阶段指令并估算赢家 Deflate…",
                },
                Reuse::Pending => "正在复用当前输入的进行中编码…",
                Reuse::Completed => "正在复用当前输入的已完成编码…",
            },
            false,
        );
        (ticket, key, metadata, future, reuse, generation)
    };
    let app = Rc::clone(app);
    spawn_local(async move {
        let result = future.await;
        let generated = {
            let mut state = app.borrow_mut();
            state.cache.finish(generation, result.is_ok());
            if !state.request.accepts(ticket) {
                return;
            }
            match result {
                Ok(result) => {
                    let encoding = &result.result().encoding;
                    let descriptor = match &key.source {
                        InputSource::Outfit { garment, .. } => {
                            outfit_core::descriptor(*garment, &encoding.directives)
                        }
                        InputSource::Hat { .. } => outfit_core::hat_descriptor(
                            &encoding.directives,
                            metadata
                                .as_ref()
                                .expect("hat export snapshots its metadata"),
                        ),
                        InputSource::Character { .. } | InputSource::Recolor { .. } => {
                            unreachable!("independent modes have separate export lanes")
                        }
                    };
                    let text = if command {
                        outfit_core::spawn_command(&descriptor).map_err(|error| error.to_string())
                    } else {
                        serde_json::to_string_pretty(&descriptor).map_err(|error| error.to_string())
                    };
                    match text {
                        Ok(text) => {
                            render_result(&state.ui, &result, &key, reuse, text.len());
                            state.ui.output.set_value(&text);
                            state.ui.result.set_hidden(false);
                            state.ui.download_last.set_disabled(false);
                            state.ui.copy_last.set_disabled(false);
                            let text = Rc::new(text);
                            let filename = format!(
                                "outfit-{}.{}",
                                input_mode(&key.source).key(),
                                if command { "txt" } else { "json" }
                            );
                            state.generated = Some(Generated {
                                text: Rc::clone(&text),
                                filename: filename.clone(),
                                command,
                            });
                            Some((text, filename))
                        }
                        Err(error) => {
                            state.show_notice(&error, true);
                            None
                        }
                    }
                }
                Err(error) => {
                    state.show_notice(&error, true);
                    None
                }
            }
        };
        if let Some((text, filename)) = generated {
            deliver(&app, ticket, text, filename, command, copy).await;
        }
    });
}

fn use_generated(app: &Application, copy: bool) {
    let prepared = {
        let mut state = app.borrow_mut();
        let ticket = state.request.advance();
        state.generated.as_ref().map(|result| {
            (
                ticket,
                Rc::clone(&result.text),
                result.filename.clone(),
                result.command,
            )
        })
    };
    if let Some((ticket, text, filename, command)) = prepared {
        let app = Rc::clone(app);
        spawn_local(async move {
            deliver(&app, ticket, text, filename, command, copy).await;
        });
    }
}

async fn deliver(
    app: &Application,
    ticket: u64,
    text: Rc<String>,
    filename: String,
    command: bool,
    copy: bool,
) {
    if !app.borrow().request.accepts(ticket) {
        return;
    }
    let result = if copy {
        // Serialize actual clipboard writes, not just their notices. A newer copy
        // waits for a previous write to settle, then wins even if it was slower.
        let future = {
            let mut state = app.borrow_mut();
            let previous = state.clipboard_tail.take();
            let weak = Rc::downgrade(app);
            let future = async move {
                if let Some(previous) = previous {
                    let _ = previous.await;
                }
                let current = weak
                    .upgrade()
                    .is_some_and(|app| app.borrow().request.accepts(ticket));
                if !current {
                    return Ok(false);
                }
                write_clipboard(&text).await?;
                Ok(true)
            }
            .boxed_local()
            .shared();
            state.clipboard_tail = Some(future.clone());
            future
        };
        future.await
    } else {
        download(&app.borrow().ui.document, &text, &filename, command).map(|_| true)
    };
    let state = app.borrow();
    if !state.request.accepts(ticket) {
        return;
    }
    match result {
        Ok(true) => state.show_notice(
            match (state.mode, command, copy) {
                (EditorMode::Recolor, _, true) => {
                    if state.generated.as_ref().is_some_and(|result| result.text.is_empty()) {
                        "已复制空字符串：所提供的全部 RGBA 已一致，不需要 directives。"
                    } else {
                        "已复制完整 directives；仅适用于你提供的原版基底、有效帧与图层，不是装备或身份命令。"
                    }
                }
                (EditorMode::Recolor, _, false) => "已下载完整 directives（可能为零字节空文件）；不是装备或身份命令。",
                (EditorMode::Character, true, true) => {
                    "已复制 /run 身份命令；请先在自己的单人存档应用，再进入服务器。"
                }
                (EditorMode::Character, true, false) => {
                    "已生成 /run 身份命令下载；请先在自己的单人存档应用，再进入服务器。"
                }
                (EditorMode::Character, false, true) => "已复制角色身份 JSON。",
                (EditorMode::Character, false, false) => "已生成角色身份 JSON 下载。",
                (_, true, true) => "已复制 /spawnitem 命令。服务器内执行仍需要管理员权限。",
                (_, true, false) => "已生成命令下载。服务器内执行仍需要管理员权限。",
                (_, false, true) => "已复制物品 JSON。",
                (_, false, false) => "已生成物品 JSON 下载。",
            },
            false,
        ),
        Ok(false) => {}
        Err(error) => state.show_notice(
            &format!("{error} 生成结果仍保留在下方，可下载或手动复制。"),
            true,
        ),
    }
}

async fn write_clipboard(text: &str) -> Result<(), String> {
    let window = web_sys::window().ok_or_else(|| "无法访问浏览器。".to_owned())?;
    let clipboard = Reflect::get(window.navigator().as_ref(), &JsValue::from_str("clipboard"))
        .map_err(js_error)?;
    if clipboard.is_undefined() || clipboard.is_null() {
        return Err(
            "浏览器未提供剪贴板权限。请使用下载按钮，或在 localhost / HTTPS 下打开。".to_owned(),
        );
    }
    let promise = call1(&clipboard, "writeText", &JsValue::from_str(text))
        .map_err(|error| format!("无法写入剪贴板：{}", js_error(error)))?;
    JsFuture::from(js_sys::Promise::resolve(&promise))
        .await
        .map_err(|error| format!("剪贴板写入被拒绝：{}", js_error(error)))?;
    Ok(())
}

fn download(document: &Document, text: &str, filename: &str, command: bool) -> Result<(), String> {
    let save = || -> Result<(), JsValue> {
        let options = BlobPropertyBag::new();
        options.set_type(if command {
            "text/plain;charset=utf-8"
        } else {
            "application/json;charset=utf-8"
        });
        let blob = Blob::new_with_str_sequence_and_options(
            &Array::of1(&JsValue::from_str(text)),
            &options,
        )?;
        let url = web_sys::Url::create_object_url_with_blob(&blob)?;
        let result = (|| -> Result<(), JsValue> {
            let anchor = document
                .create_element("a")?
                .dyn_into::<HtmlAnchorElement>()?;
            anchor.set_href(&url);
            anchor.set_download(filename);
            anchor.set_hidden(true);
            document
                .body()
                .ok_or_else(|| JsValue::from_str("页面不可用。"))?
                .append_child(&anchor)?;
            anchor.click();
            anchor.remove();
            Ok(())
        })();
        web_sys::Url::revoke_object_url(&url)?;
        result
    };
    save().map_err(|error| format!("无法下载文件：{}", js_error(error)))
}

fn call0(object: &JsValue, name: &str) -> Result<JsValue, JsValue> {
    Reflect::get(object, &JsValue::from_str(name))?
        .dyn_into::<Function>()?
        .call0(object)
}

fn call1(object: &JsValue, name: &str, argument: &JsValue) -> Result<JsValue, JsValue> {
    Reflect::get(object, &JsValue::from_str(name))?
        .dyn_into::<Function>()?
        .call1(object, argument)
}

/// Browser measurement: UTF-8 Blob -> CompressionStream
/// ('deflate') -> sum readable chunk byte lengths. No native-compressor fallback.
async fn deflate_size(text: &str) -> Result<usize, String> {
    let unsupported = "当前浏览器不支持 CompressionStream('deflate')，无法提供真实的优化估算。请使用支持此接口的新版浏览器。";
    let constructor = Reflect::get(&js_sys::global(), &JsValue::from_str("CompressionStream"))
        .ok()
        .and_then(|value| value.dyn_into::<Function>().ok())
        .ok_or_else(|| unsupported.to_owned())?;
    let stream = Reflect::construct(&constructor, &Array::of1(&JsValue::from_str("deflate")))
        .map_err(|_| unsupported.to_owned())?;
    let blob = Blob::new_with_str_sequence(&Array::of1(&JsValue::from_str(text)))
        .map_err(|error| format!("无法准备压缩估算：{}", js_error(error)))?;
    let source: JsValue = blob.stream().into();
    let compressed = call1(&source, "pipeThrough", &stream)
        .map_err(|error| format!("无法运行浏览器 Deflate：{}", js_error(error)))?;
    let reader = call0(&compressed, "getReader").map_err(js_error)?;
    let result = async {
        let mut size = 0usize;
        loop {
            let read = call0(&reader, "read").map_err(js_error)?;
            let chunk = JsFuture::from(js_sys::Promise::resolve(&read))
                .await
                .map_err(js_error)?;
            if Reflect::get(&chunk, &JsValue::from_str("done"))
                .map_err(js_error)?
                .as_bool()
                == Some(true)
            {
                return Ok(size);
            }
            let value = Reflect::get(&chunk, &JsValue::from_str("value")).map_err(js_error)?;
            size += Uint8Array::new(&value).byte_length() as usize;
        }
    }
    .await;
    let release = call0(&reader, "releaseLock").map_err(js_error);
    match result {
        Err(error) => Err(error),
        Ok(size) => release.map(|_| size),
    }
}

fn js_error(error: JsValue) -> String {
    error
        .as_string()
        .or_else(|| {
            Reflect::get(&error, &JsValue::from_str("message"))
                .ok()
                .and_then(|value| value.as_string())
        })
        .unwrap_or_else(|| format!("{error:?}"))
}

fn garment_name(garment: Garment) -> &'static str {
    match garment {
        Garment::Pants => "普通裤装",
        Garment::HidingPants => "隐藏身体裤装",
        Garment::Chest => "上衣与袖子",
        Garment::Back => "背饰",
    }
}

fn input_mode(source: &InputSource) -> EditorMode {
    match source {
        InputSource::Outfit { garment, .. } => EditorMode::Outfit(*garment),
        InputSource::Hat { .. } => EditorMode::Hat,
        InputSource::Character { .. } => EditorMode::Character,
        InputSource::Recolor { .. } => EditorMode::Recolor,
    }
}

fn editor_name(mode: EditorMode) -> &'static str {
    match mode {
        EditorMode::Outfit(garment) => garment_name(garment),
        EditorMode::Hat => "帽子",
        EditorMode::Character => "人物重绘",
        EditorMode::Recolor => "原版直接改色",
    }
}

fn render_result(
    ui: &Ui,
    cached: &EditorEncoding,
    key: &InputKey,
    reuse: Reuse,
    text_bytes: usize,
) {
    let result = cached.result();
    let encoding = &result.encoding;
    let strategy = match result.strategy {
        Strategy::Text => "文本最短",
        Strategy::Compressed => "Deflate 估算最小",
        Strategy::Passes => "最少像素替换遍数",
    };
    let reuse = match reuse {
        Reuse::New => "新编码（输入或设置已变化）",
        Reuse::Pending => "复用当前输入的进行中编码",
        Reuse::Completed => "复用当前输入的已完成编码",
    };
    let profile = match cached {
        EditorEncoding::Outfit(value) => value.profile.key(),
        EditorEncoding::Hat(_) => "Hatter 稳定画布",
    };
    let search = match cached {
        EditorEncoding::Outfit(value) => format!(
            "{} 个辅助图；最多 {} 个完整候选；有界搜索，非全局最优；设置 {}",
            value.profile_count,
            value.candidate_limit,
            match &key.source {
                InputSource::Outfit { profile, .. } => profile.key(),
                _ => unreachable!("garment result requires garment input"),
            },
        ),
        EditorEncoding::Hat(_) => "固定帽子画布候选".to_owned(),
    };
    // All strings interpolated here are fixed labels or encoder numbers; uploaded
    // filenames and generated text are always assigned through text/value APIs.
    ui.stats.set_inner_html(&format!(
        "<div><dt>服装 / 辅助图</dt><dd>{} / {profile}</dd></div>\
         <div><dt>编码目标</dt><dd>{strategy}</dd></div>\
         <div><dt>本次编码来源</dt><dd>{reuse}</dd></div>\
         <div><dt>原始像素指令</dt><dd>{} B</dd></div>\
         <div><dt>实际像素指令</dt><dd>{} B</dd></div>\
         <div><dt>已编码像素 / 颜色</dt><dd>{} / {}</dd></div>\
         <div><dt>调色板</dt><dd>{}</dd></div>\
         <div><dt>调色板归并颜色</dt><dd>{}</dd></div>\
         <div><dt>完整导出文本</dt><dd>{text_bytes} B</dd></div>\
         <div><dt>完整指令 Deflate 估算</dt><dd>{} B</dd></div>\
         <div><dt>已比较编码候选</dt><dd>{}</dd></div>\
         <div><dt>搜索范围 / 限制</dt><dd>{search}</dd></div>",
        editor_name(input_mode(&key.source)),
        encoding.baseline_bytes,
        encoding.output_bytes,
        encoding.pixel_count,
        encoding.color_count,
        if encoding.palette_applied {
            "已启用"
        } else {
            "未使用（关闭或无收益）"
        },
        encoding.factored_color_count,
        result.estimated_deflate_bytes,
        result.candidate_count,
    ));
}

fn populate_slots(document: &Document) -> Result<(), JsValue> {
    for (index, title, dimensions, host) in [
        (6, "身体与腿", "387 × 258", "character-uploads"),
        (7, "前臂", "387 × 301", "character-uploads"),
        (8, "后臂", "387 × 301", "character-uploads"),
        (
            9,
            "基础头",
            "43 × 43，或原版 86 × 215（取 normal 格）",
            "character-uploads",
        ),
        (
            11,
            "表情叠加层",
            "387 × 344 · 仅绘制表情差异",
            "character-uploads",
        ),
        (10, "头发", "43 × 43", "character-uploads"),
        (0, "裤装精灵表", "387 × 258", "pants-upload"),
        (2, "前袖", "387 × 301", "chest-uploads"),
        (1, "上衣身体", "86 × 258", "chest-uploads"),
        (3, "后袖", "387 × 301", "chest-uploads"),
        (4, "背饰精灵表", "387 × 301", "back-upload"),
        (
            HAT_SLOT,
            "帽子原始 PNG",
            "宽、高各 1–256 像素",
            "hat-upload",
        ),
    ] {
        let id = SLOT_IDS[index];
        let card = document.create_element("section")?;
        card.set_id(&format!("drop-{id}"));
        card.set_class_name("upload-card");
        card.set_attribute("aria-labelledby", &format!("title-{id}"))?;
        card.set_inner_html(&format!(
            "<h3 id=\"title-{id}\">{title}</h3><p class=\"muted\">{dimensions} PNG · 可拖放文件到这里</p>\
             <label for=\"input-{id}\">选择{title} PNG</label>\
             <input id=\"input-{id}\" type=\"file\" accept=\"image/png,.png\" aria-describedby=\"status-{id}\">\
             <button id=\"clear-{id}\" type=\"button\" class=\"secondary small\" aria-label=\"清除{title} PNG\">清除图片</button>\
             <p id=\"status-{id}\" class=\"file-status\" role=\"status\" aria-live=\"polite\">尚未选择 PNG；也可拖放文件到此处。</p>\
             <div class=\"preview\"><canvas id=\"preview-{id}\" hidden role=\"img\" aria-label=\"已选择的{title}精灵表预览\"></canvas></div>"
        ));
        element::<HtmlElement>(document, host)?.append_child(&card)?;
    }
    Ok(())
}

fn populate_templates(document: &Document) -> Result<(), JsValue> {
    for (host, directory, templates) in [
        (
            "pants-templates",
            "pants",
            vec![
                ("pantsm", "裤装（男性）", "包含男性身体参考帧。", true),
                ("pantsf", "裤装（女性）", "包含女性身体参考帧。", true),
                (
                    "pants_blank",
                    "裤装（空白）",
                    "普通与隐藏身体模式共用。",
                    false,
                ),
            ],
        ),
        (
            "chest-templates",
            "chest",
            vec![
                ("chestm", "上衣（男性）", "包含男性身体参考帧。", true),
                ("chestf", "上衣（女性）", "包含女性身体参考帧。", true),
                ("chest_blank", "上衣（空白）", "只绘制身体部分。", false),
                ("fsleeves", "前袖", "包含前臂动画参考帧。", true),
                ("bsleeves", "后袖", "包含后臂动画参考帧。", true),
                ("sleeves_blank", "袖子（空白）", "可用于前袖或后袖。", false),
            ],
        ),
        (
            "back-templates",
            "back",
            vec![
                ("backm", "背饰（男性）", "包含男性身体参考帧。", true),
                ("backf", "背饰（女性）", "包含女性身体参考帧。", true),
                ("back_blank", "背饰（空白）", "空白背饰动画精灵表。", false),
            ],
        ),
    ] {
        for (file, title, description, pdn) in templates {
            let path = format!("assets/templates/{directory}/{file}");
            let card = document.create_element("article")?;
            card.set_class_name("template-card");
            let pdn = if pdn {
                format!(
                    "<a class=\"button secondary\" href=\"{path}.pdn\" download=\"{file}.pdn\">下载 PDN</a>"
                )
            } else {
                String::new()
            };
            card.set_inner_html(&format!(
                "<div class=\"template-preview\"><img src=\"{path}.png\" alt=\"{title}精灵表模板\" loading=\"lazy\"></div>\
                 <h4>{title}</h4><p class=\"muted\">{description}</p><div class=\"actions\">\
                 <a class=\"button\" href=\"{path}.png\" download=\"{file}.png\">下载 PNG</a>{pdn}</div>"
            ));
            element::<HtmlElement>(document, host)?.append_child(&card)?;
        }
    }
    Ok(())
}
