//! Подсказка у значка в строке меню (macOS). Системная подсказка (toolTip) у значка перестаёт
//! появляться, после того как приложение побывало в доке и ушло в фон: известная ошибка AppKit
//! (rdar://24027003, electron#3599), а в фоне статус как раз и нужен. Поэтому рисуем свою: панель
//! под значком, пока на нём курсор, с текстом, который обновляется на ходу. Всё — в главном потоке.

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{msg_send, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSFont, NSPanel, NSStatusItem, NSStatusWindowLevel, NSTextField,
    NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString, NSTimer};
use std::cell::RefCell;
use std::ptr::NonNull;
use tauri::tray::{TrayIcon, TrayIconEvent};

/// Как у системных подсказок: не сразу, чтобы не мигать, когда курсор проходит мимо, с.
const DELAY: f64 = 0.5;
/// Поля вокруг текста, pt.
const PAD: NSSize = NSSize::new(8.0, 5.0);
/// Отступ от строки меню и от края экрана, pt.
const GAP: f64 = 4.0;

#[derive(Default)]
struct Hint {
    text: String,
    /// Значок под курсором; None — курсор не на значке.
    item: Option<Retained<NSStatusItem>>,
    timer: Option<Retained<NSTimer>>,
    view: Option<(Retained<NSPanel>, Retained<NSTextField>)>,
    shown: bool,
}

thread_local! {
    static HINT: RefCell<Hint> = RefCell::new(Hint::default());
}

/// События значка: курсор навели — подсказка, увели или щёлкнули — убрать.
pub fn on_tray_event(tray: &TrayIcon, event: TrayIconEvent) {
    let _ = match event {
        TrayIconEvent::Enter { .. } => tray.with_inner_tray_icon(|t| {
            if let Some(item) = t.ns_status_item() {
                enter(item);
            }
        }),
        TrayIconEvent::Leave { .. } | TrayIconEvent::Click { .. } | TrayIconEvent::DoubleClick { .. } => {
            tray.with_inner_tray_icon(|_| leave())
        }
        _ => Ok(()),
    };
}

/// Новый текст подсказки; если она на экране — сразу обновить.
pub fn set_text(text: &str) {
    HINT.with_borrow_mut(|h| {
        h.text = text.to_string();
        if h.shown {
            show(h);
        }
    });
}

fn enter(item: Retained<NSStatusItem>) {
    HINT.with_borrow_mut(|h| {
        h.item = Some(item);
        if let Some(timer) = h.timer.take() {
            timer.invalidate();
        }
        let fire = RcBlock::new(|_: NonNull<NSTimer>| {
            HINT.with_borrow_mut(|h| {
                h.timer = None;
                show(h);
            })
        });
        // SAFETY: таймер сам держит блок; срабатывает в главном потоке, когда HINT свободен.
        h.timer = Some(unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(DELAY, false, &fire) });
    });
}

fn leave() {
    HINT.with_borrow_mut(|h| {
        h.item = None;
        h.shown = false;
        if let Some(timer) = h.timer.take() {
            timer.invalidate();
        }
        if let Some((panel, _)) = &h.view {
            panel.orderOut(None);
        }
    });
}

fn show(h: &mut Hint) {
    let (Some(mtm), Some(item)) = (MainThreadMarker::new(), h.item.as_ref()) else { return };
    // Окно значка в строке меню: подсказка встаёт под ним.
    let Some(bar) = item.button(mtm).and_then(|button| button.window()) else { return };
    let (panel, label) = h.view.get_or_insert_with(|| make(mtm));
    label.setStringValue(&NSString::from_str(&h.text));
    label.sizeToFit();
    label.setFrameOrigin(NSPoint::new(PAD.width, PAD.height));
    let text = label.frame().size;
    let size = NSSize::new(text.width + 2.0 * PAD.width, text.height + 2.0 * PAD.height);
    let icon = bar.frame();
    let mut x = icon.origin.x;
    if let Some(screen) = bar.screen() {
        let area = screen.visibleFrame();
        x = x.min(area.origin.x + area.size.width - size.width - GAP).max(area.origin.x + GAP);
    }
    panel.setFrame_display(NSRect::new(NSPoint::new(x, icon.origin.y - GAP - size.height), size), true);
    panel.invalidateShadow();
    panel.orderFrontRegardless();
    h.shown = true;
}

/// Панель, которая не забирает фокус и не ловит мышь; фон и шрифт — как у системных подсказок.
fn make(mtm: MainThreadMarker) -> (Retained<NSPanel>, Retained<NSTextField>) {
    let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
        NSPanel::alloc(mtm),
        NSRect::ZERO,
        NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
        NSBackingStoreType::Buffered,
        false,
    );
    // SAFETY: панель живёт в HINT до конца работы; её не закрывают, только прячут.
    unsafe { panel.setReleasedWhenClosed(false) };
    panel.setLevel(NSStatusWindowLevel);
    panel.setIgnoresMouseEvents(true);
    panel.setOpaque(false);
    panel.setBackgroundColor(Some(&NSColor::clearColor()));
    panel.setHasShadow(true);
    panel.setHidesOnDeactivate(false);
    panel.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle
            | NSWindowCollectionBehavior::FullScreenAuxiliary,
    );

    let background = NSVisualEffectView::new(mtm);
    background.setMaterial(NSVisualEffectMaterial::ToolTip);
    background.setState(NSVisualEffectState::Active);
    background.setWantsLayer(true);
    // SAFETY: у слоя вида (CALayer) есть cornerRadius и masksToBounds.
    unsafe {
        let layer: Option<Retained<AnyObject>> = msg_send![&*background, layer];
        if let Some(layer) = layer {
            let _: () = msg_send![&*layer, setCornerRadius: 5.0f64];
            let _: () = msg_send![&*layer, setMasksToBounds: true];
        }
    }
    let label = NSTextField::labelWithString(&NSString::new(), mtm);
    label.setFont(Some(&NSFont::toolTipsFontOfSize(0.0)));
    background.addSubview(&label);
    panel.setContentView(Some(&background));
    (panel, label)
}
