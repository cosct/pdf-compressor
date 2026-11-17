//! GPUI desktop shell for the pdf-core engine.
//!
//! Phase-0 feasibility spike: one window with a self-drawn title bar, a file
//! drop target, the native file picker, and a background analysis run that
//! streams progress back to the view. It exists to answer whether the stack
//! builds and behaves on all three desktop platforms before the Vue frontend
//! is ported; none of the real queue/state logic lives here yet.

mod spike;

use gpui::{Bounds, WindowBounds, WindowDecorations, WindowOptions, px, size};
use gpui_component::TitleBar;

fn main() {
    // `application()` registers no asset source on its own; without the bundled
    // icon set every `Icon` paints nothing, which leaves the title bar's
    // minimize/maximize/close buttons clickable but invisible.
    let app = gpui_kit::application().with_assets(gpui_kit::assets::Assets);
    app.run(|cx| {
        gpui_kit::init(cx);

        // Same geometry as the Tauri window (tauri.conf.json: 1200×860, min
        // 1200×800) so layout findings carry over to the real port.
        let bounds = Bounds::centered(None, size(px(1200.), px(860.)), cx);
        let opened = gpui_kit::open_window(
            WindowOptions {
                app_id: Some("pdf-compressor".into()),
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(1200.), px(800.))),
                // Linux only (ignored elsewhere): without client-side
                // decorations the compositor draws its own title bar above
                // ours, and `TitleBar` hides its window controls.
                window_decorations: Some(WindowDecorations::Client),
                ..TitleBar::window_options()
            },
            cx,
            spike::SpikeView::build,
        );
        // A failed window must end the process visibly, not leave it running
        // with nothing on screen.
        if let Err(error) = opened {
            eprintln!("failed to open the main window: {error:#}");
            cx.quit();
            return;
        }
        cx.activate(true);
    });
}
