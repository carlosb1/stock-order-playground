mod js;
mod models;

use std::rc::Rc;
use dioxus::prelude::*;
use js_sys::{Array, Float64Array};
use wasm_bindgen::prelude::*;
use web_sys::{console, MessageEvent, WebSocket};
use crate::js::{ensure_container, init_uplot_now, inject_uplot};
use crate::models::{Book, Kind, Msg, OrderBookVariant};
use gloo_net::http::Request;

#[wasm_bindgen(start)]
pub fn start() {
    dioxus::launch(app);
}

async fn post_new_strategy(strategy: String) -> bool {
    let payload = vec![(strategy, "".to_string())];
    let resp = Request::post("/workers")
        .header("Content-Type", "application/json")
        .json(&payload)
        .unwrap()
        .send()
        .await;
    resp.is_ok()
}

async fn list_strategies() -> Vec<String> {
    match Request::get("/workers/list").send().await {
        Ok(resp) => resp.json().await.unwrap_or_default(),
        Err(err) => {
            web_sys::console::error_1(&format!("list_strategies error: {err:?}").into());
            Vec::new()
        }
    }
}

async fn delete_strategy(strategy: String) -> bool {
    let payload = vec![strategy];
    let resp = Request::post("/workers/delete")
        .header("Content-Type", "application/json")
        .json(&payload)
        .unwrap()
        .send()
        .await;
    resp.is_ok()
}


#[component]
fn app() -> Element {
    let book = use_signal(Book::default);
    let status = use_signal(|| "connecting...".to_string());
    let mut signed_strategies = use_signal(|| Vec::new());
    let mut selected = use_signal(|| String::from(""));

    let strategies = spawn(async move {
        let new_strategies = list_strategies().await;
        signed_strategies.set(new_strategies.clone());
        if new_strategies.len() > 1 {
            let first = new_strategies.get(0).unwrap();
            selected.set(first.clone());

        }
    });

    use_effect(move || {
        let window = web_sys::window().unwrap();
        let document = window.document().unwrap();
        inject_uplot(&document);
        let _container = ensure_container(&document);
        {
            let closure = Closure::<dyn FnMut()>::new(move || {
                init_uplot_now("depth");
            });
            closure.as_ref().unchecked_ref::<js_sys::Function>().call0(&JsValue::NULL).ok();
            let _ = window.set_interval_with_callback_and_timeout_and_arguments_0(
                closure.as_ref().unchecked_ref(),
                500,
            );
            closure.forget();
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
                        let msg = serde_json::from_str::<Msg>(&json);
                        match msg {
                            Err(e) => {
                                let str_e = format!("{:?}", e);
                                console::log_1(&JsValue::from_str(str_e.as_str()));
                            }
                            Ok(m) => {
                                if let Kind::OrderBook(OrderBookVariant::Update(up), ..) = m.Market.Item.kind {
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
    document::Stylesheet { href: asset!("/assets/main.css") }
    div {
        class: "form-container",
        div {
            class: "card",
            h3 { "Estrategia a incluir" }

            select {
                value: selected(),
                onchange: move |evt| {
                    selected.set(evt.data.value());
                },
                for stra in signed_strategies.iter() {
                    option { value: stra.clone(), "{stra}" }
                }
            }

            div {                         // fila con botones (si quieres en línea)
                style: "display:flex; gap:10px; align-items:center;",
                button {
                    class: "btn-action",
                    onclick: move |_| {
                        spawn(async move {
                            let strategy = selected.read().to_string();
                            post_new_strategy(strategy).await;
                        });
                    },
                    "Activar estrategia"
                }
                button {
                    class: "btn-action",
                    onclick: move |_| {
                        spawn(async move {
                            let strategy = selected.read().to_string();
                            delete_strategy(strategy).await;
                        });
                    },
                    "Eliminar estrategia"
                }
            }

            div {
                h3 { "Order Book depth" }
                p { "{status.read().as_str()}" }
            }
        }
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