use dioxus::prelude::*;
use wasm_bindgen::prelude::*;
use web_sys::{MessageEvent, WebSocket};

#[wasm_bindgen(start)]
pub fn start() {
    dioxus::launch(app);
}

// Utilidad para construir la URL ws://host:3000/websocket respetando http/https
fn ws_url() -> String {
    let window = web_sys::window().unwrap();
    let loc = window.location();
    let protocol = loc.protocol().unwrap(); // "http:" o "https:"
    let hostname = loc.hostname().unwrap();
    let port = loc.port().unwrap(); // puede estar vacío
    let ws_proto = if protocol == "https:" { "wss" } else { "ws" };
    if port.is_empty() {
        format!("{ws_proto}://{hostname}/websocket")
    } else {
        format!("{ws_proto}://{hostname}:{port}/websocket")
    }
}

#[component]
fn app() -> Element {
    let messages = use_signal(|| Vec::<String>::new());

    // Abre el WebSocket una sola vez
    use_effect(move || {
        let ws = WebSocket::new(&ws_url()).expect("failed to open ws");

        {
            let mut messages = messages.clone();
            let onmessage = Closure::<dyn FnMut(MessageEvent)>::new(move |evt: MessageEvent| {
                if let Ok(txt) = evt.data().dyn_into::<js_sys::JsString>() {
                    // push y re-render
                    messages.write().push(txt.as_string().unwrap_or_default());
                } else {
                    messages.write().push("[binary message]".to_string());
                }
            });
            ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
            onmessage.forget(); // mantener vivo el callback
        }

        // opcional: onerror/onopen para feedback
        // let onerror = ...
        // ws.set_onerror(Some(...));

        (move || {
            // cleanup opcional: cerrar el ws al desmontar
            let _ = ws.close();
        })
            ()
    });

    rsx! {
        div { class: "container",
            h1 { "Live feed" }
            p { "Mensajes recibidos del backend vía WebSocket:" }
            ul {
                for m in messages.read().iter() {
                    li { "{m}" }
                }
            }
        }
    }
}

fn main() {}