mod js;
mod models;

use dioxus::prelude::*;
use js_sys::{Array, Float64Array};
use wasm_bindgen::prelude::*;
use web_sys::{MessageEvent, WebSocket};
use crate::js::{ensure_container, init_uplot_if_needed, inject_uplot};
use crate::models::{Book, Kind, Msg, UpdateWrap};

#[wasm_bindgen(start)]
pub fn start() {
    dioxus::launch(app);
}

// load websocket

#[component]
fn app() -> Element {
    let book = use_signal(Book::default);
    let status = use_signal(|| "connecting...".to_string());

    use_effect(move || {
        let window = web_sys::window().unwrap();
        let document = window.document().unwrap();
        inject_uplot(&document);
        let _container = ensure_container(&document);

        // intenta inicializar el chart (si el script aún no cargó, reintenta más tarde)
        {
            let closure = Closure::<dyn FnMut()>::new(move || {
                init_uplot_if_needed("depth");
            });
            // ejecuta ahora
            closure.as_ref().unchecked_ref::<js_sys::Function>().call0(&JsValue::NULL).ok();
            // y cada 500ms por si el script aún no estaba
            let _ = window.set_interval_with_callback_and_timeout_and_arguments_0(
                closure.as_ref().unchecked_ref(),
                500,
            );
            closure.forget(); // mantener vivo
        }

        // WebSocket
        let ws = WebSocket::new(&ws_url()).expect("open ws");

        {
            let mut book = book.clone();
            let mut status = status.clone();

            let onopen = Closure::<dyn FnMut()>::new(move || status.set("connected".into()));
            ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
            onopen.forget();

            let onmessage = Closure::<dyn FnMut(MessageEvent)>::new(move |evt: MessageEvent| {
                if let Ok(txt) = evt.data().dyn_into::<js_sys::JsString>() {
                    if let Some(json) = extract_json(&txt.as_string().unwrap_or_default()) {
                        if let Ok(m) = serde_json::from_str::<Msg>(&json) {
                            if let Kind::OrderBook { OrderBook: UpdateWrap::Update(up) } = m.Market.Item.kind {
                                book.write().apply(&up);

                                // preparar data y llamar a window.updateDepth(xs,bids,asks)
                                let (xs, yb, ya) = book.read().to_depth_union(80);

                                let win = web_sys::window().unwrap();
                                if let Ok(f) = js_sys::Reflect::get(&win, &"updateDepth".into()) {
                                    if let Some(func) = f.dyn_ref::<js_sys::Function>() {
                                        let xs = Float64Array::from(xs.as_slice());
                                        let yb = Float64Array::from(yb.as_slice());
                                        let ya = Float64Array::from(ya.as_slice());
                                        let args = Array::new();
                                        args.push(&xs);
                                        args.push(&yb);
                                        args.push(&ya);
                                        let _ = func.apply(&JsValue::NULL, &args);
                                    }
                                }
                            }
                        }
                    }
                }
            });
            ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
            onmessage.forget();

            let onerror = Closure::<dyn FnMut()>::new(move || status.set("error".into()));
            ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
            onerror.forget();
        }
    });

    rsx! {
        div { class: "p-3",
            h3 { "Order Book depth" }
            p { "{status.read().as_str()}" }
            // el canvas se añade al body, si prefieres dentro, usa un nodo ref y cámbiale el append
        }
    }
}

fn ws_url() -> String {
    let w = web_sys::window().unwrap();
    let loc = w.location();
    let ws_proto = if loc.protocol().unwrap() == "https:" { "wss" } else { "ws" };
    format!("{ws_proto}://{}/websocket", loc.host().unwrap())
}
fn extract_json(s: &str) -> Option<String> {
    let a = s.find('{')?;
    let b = s.rfind('}')?;
    (b > a).then(|| s[a..=b].to_string())
}


fn main() {}