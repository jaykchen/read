use html2text::from_read;
use http_req::request;
use wry::{
    application::{
        event::{Event, WindowEvent},
        event_loop::{ControlFlow, EventLoop},
        window::{Window, WindowBuilder},
    },
    webview::WebViewBuilder,
};

const RETURN_OUTER_HTML: &str = r#"
window.onload = function() {
  let html = document.documentElement.textContent;
  window.ipc.postMessage(html);
}
"#;

// use wry lib as a headless browser to render dynamic webpage
 fn main() -> wry::Result<()> {
    enum UserEvents {
        CloseWindow,
    }

    let event_loop = EventLoop::<UserEvents>::with_user_event();

    let url = "https://2023.fossy.us/";
    let url = "https://github.com/tauri-apps/tauri/tree/dev/tooling/webdriver";

    let mut writer = Vec::new(); //container for body of a response
    let _ = request::get(url, &mut writer).unwrap();
    let html = String::from_utf8(writer).unwrap();

    // println!("{:?}", html.clone());
    let window = WindowBuilder::new()
        .with_title("Render HTML to string")
        .with_visible(false)
        .build(&event_loop)?;

    let proxy = event_loop.create_proxy();

    use std::sync::{Arc, Mutex};

    let inner_html = Arc::new(Mutex::new(String::new()));

    let handler = {
        let inner_html = Arc::clone(&inner_html);
        move |_window: &Window, html: String| {
            let mut inner_html = inner_html.lock().unwrap();
            let head = html.lines().take(10).collect::<Vec<&str>>().join(" ");
            // println!("{:?}", head);
            *inner_html = html.clone();

            let output = from_read(inner_html.to_string().as_bytes(), 80);

            println!("{:}", output);
            let _ = proxy.send_event(UserEvents::CloseWindow);
        }
    };

    let _webview = WebViewBuilder::new(window)?
        .with_html(html)?
        .with_initialization_script(RETURN_OUTER_HTML)
        .with_ipc_handler(handler)
        .build()?;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            }
            | Event::UserEvent(UserEvents::CloseWindow) => *control_flow = ControlFlow::Exit,
            _ => (),
        }
    });
}
