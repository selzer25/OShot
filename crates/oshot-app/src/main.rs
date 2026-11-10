use std::sync::Arc;

use futures::StreamExt;
use oshot_app::assets::Assets;
use oshot_app::commands::handle_event;
use oshot_app::platform::MacPlatform;
use oshot_app::state::{Command, Paths};

fn main() {
    let app = gpui_platform::application().with_assets(Assets);
    app.on_open_urls(|urls| {
        for url in urls {
            if let Some(path) = url.strip_prefix("file://") {
                let path = std::path::PathBuf::from(path.replace("%20", " "));
                oshot_app::platform::queue(Command::OpenFile(path));
            }
        }
    });
    app.run(|cx| {
        let (platform, mut events) = MacPlatform::new();
        oshot_app::init(Paths::standard(), Arc::new(platform), cx);
        cx.spawn(async move |cx| {
            while let Some(event) = events.next().await {
                cx.update(|cx| handle_event(event, cx));
            }
        })
        .detach();
    });
}
