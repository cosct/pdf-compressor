//! Native GPUI desktop shell over the shared pdf-core engine.

pub mod app;
mod platform;
#[cfg(test)]
mod spike;

fn main() {
    if let Err(error) = platform::check_display_server() {
        eprintln!("{error}");
        std::process::exit(1);
    }
    if !platform::cjk_fonts_available() {
        eprintln!("warning: no CJK font was detected; Chinese text may render incorrectly");
    }

    let pdf_paths = platform::argv_pdf_paths_from_os();
    let (handoff_tx, handoff_rx) = async_channel::unbounded::<Vec<std::path::PathBuf>>();
    let callback_tx = handoff_tx.clone();
    match platform::SingleInstance::forward(&pdf_paths) {
        Ok(true) => return,
        Ok(false) => {}
        Err(error) => eprintln!("single-instance forwarding unavailable: {error}"),
    }
    let _instance = match platform::SingleInstance::acquire(move |paths| {
        let _ = callback_tx.send_blocking(paths);
    }) {
        Ok(instance) => Some(instance),
        Err(error) => {
            eprintln!("single-instance lock unavailable: {error}");
            None
        }
    };

    // `application()` registers no asset source on its own; without the bundled
    // icon set every `Icon` paints nothing, which leaves the title bar's
    // minimize/maximize/close buttons clickable but invisible.
    let app = gpui_kit::application().with_assets(gpui_kit::assets::Assets);
    app.run(move |cx| {
        gpui_kit::init(cx);
        let opened = gpui_kit::open_window(
            app::view::main_window_options(cx),
            cx,
            app::view::AppView::build,
        );
        let Ok((_, view)) = opened else {
            if let Err(error) = opened {
                eprintln!("failed to open the main window: {error:#}");
            }
            cx.quit();
            return;
        };

        let weak = view.downgrade();
        cx.spawn(async move |cx| {
            while let Ok(paths) = handoff_rx.recv().await {
                let _ = weak.update(cx, |view, cx| view.receive_external_paths(paths, cx));
            }
        })
        .detach();

        // Cold-start file associations are delivered through the same path as
        // later second instances, so readiness and queue filtering stay in one
        // place.
        if !pdf_paths.is_empty() {
            let _ = handoff_tx.send_blocking(pdf_paths);
        }
        cx.activate(true);
    });
}
