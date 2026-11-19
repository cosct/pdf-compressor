//! Native GPUI desktop shell over the shared pdf-core engine.

pub mod app;
#[cfg(test)]
mod spike;

fn main() {
    // `application()` registers no asset source on its own; without the bundled
    // icon set every `Icon` paints nothing, which leaves the title bar's
    // minimize/maximize/close buttons clickable but invisible.
    let app = gpui_kit::application().with_assets(gpui_kit::assets::Assets);
    app.run(|cx| {
        gpui_kit::init(cx);
        let opened = gpui_kit::open_window(
            app::view::main_window_options(cx),
            cx,
            app::view::AppView::build,
        );
        if let Err(error) = opened {
            eprintln!("failed to open the main window: {error:#}");
            cx.quit();
            return;
        }
        cx.activate(true);
    });
}
