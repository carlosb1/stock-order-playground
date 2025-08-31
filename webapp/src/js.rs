use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Document, Element, HtmlElement};

pub fn inject_uplot(document: &Document) {
    // CSS
    if document.get_element_by_id("uplot-css").is_none() {
        let link = document.create_element("link").unwrap();
        link.set_id("uplot-css");
        link.set_attribute("rel", "stylesheet").unwrap();
        link.set_attribute("href", "https://unpkg.com/uplot@1.6.30/dist/uPlot.min.css").unwrap();
        document.head().unwrap().append_child(&link).unwrap();
    }
    // JS
    if document.get_element_by_id("uplot-js").is_none() {
        let script = document.create_element("script").unwrap();
        script.set_id("uplot-js");
        script.set_attribute("src", "https://unpkg.com/uplot@1.6.30/dist/uPlot.iife.min.js").unwrap();
        document.body().unwrap().append_child(&script).unwrap();
    }
}
pub fn ensure_container(document: &Document) -> HtmlElement {
    if let Some(el) = document.get_element_by_id("depth") {
        return el.unchecked_into();
    }
    let div: Element = document.create_element("div").unwrap();
    div.set_id("depth");
    div.set_attribute("style", "width: 900px; height: 420px;").unwrap();
    document.body().unwrap().append_child(&div).unwrap();
    div.unchecked_into()
}

pub  fn init_uplot_if_needed(container_id: &str) {
    // Si ya existe window.depthChart, no recreamos
    let win = web_sys::window().unwrap();
    let has = js_sys::Reflect::has(&win, &"depthChart".into()).unwrap_or(false);
    if has { return; }

    // Crea función JS que inicializa uPlot y un método global updateDepth
    let js = format!(r#"
        (function() {{
            if (!window.uPlot) return; // aún no cargó la lib
            var el = document.getElementById("{container_id}");
            if (!el) return;
            var opts = {{
                title: "Order Book Depth",
                width: el.clientWidth || 900,
                height: el.clientHeight || 420,
                axes: [
                  {{ label: "Price" }},
                  {{ label: "Cum. Size" }},
                ],
                series: [
                  {{}}, // x
                  {{ label: "Bids", spanGaps: true }},
                  {{ label: "Asks", spanGaps: true }},
                ],
                scales: {{
                  x: {{ time: false }},
                }},
            }};
            window.depthChart = new uPlot(opts, [[],[],[]], el);
            window.updateDepth = function(xs, bids, asks) {{
                window.depthChart.setData([xs, bids, asks]);
            }};
        }})();
    "#);

    js_sys::Function::new_no_args(&js).call0(&JsValue::NULL).ok();
}