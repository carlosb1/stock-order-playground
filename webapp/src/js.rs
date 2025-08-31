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

pub fn init_uplot_now(container_id: &str) {
    let js = format!(r#"
        (function() {{
            if (!window.uPlot) return;
            var el = document.getElementById("{container_id}");
            if (!el || window.depthChart) return;

            el.style.background = "white"; // fondo claro por si el tema es oscuro

            var opts = {{
                title: "Order Book Depth",
                width: el.clientWidth || 900,
                height: el.clientHeight || 420,
                axes: [
                  {{ label: "Price" }},
                  {{ label: "Cum. Size" }},
                ],
                scales: {{ x: {{ time: false }} }},
                series: [
                  {{}}, // x
                  {{
                    label: "Bids",
                    spanGaps: false,
                    stroke: "rgb(34,139,34)",
                    width: 3,
                    points: {{ show: true, size: 3 }},
                    // paths: uPlot.paths.stepped({{ align: 1 }}), // opcional
                  }},
                  {{
                    label: "Asks",
                    spanGaps: false,
                    stroke: "rgb(220,20,60)",
                    width: 3,
                    points: {{ show: true, size: 3 }},
                    // paths: uPlot.paths.stepped({{ align: 1 }}), // opcional
                  }},
                ],
                hooks: {{
                  draw: [
                    (u) => {{
                      const xs = u.data[0], b = u.data[1], a = u.data[2];
                      if (xs.length) {{
                        console.debug("draw n=", xs.length, "x0", xs[0], "xN", xs[xs.length-1], 
                                      "bN", b[b.length-1], "aN", a[a.length-1]);
                      }}
                    }}
                  ]
                }}
            }};

            window.depthChart = new uPlot(opts, [[],[],[]], el);

            window.updateDepth = function(xsAny, bidsAny, asksAny) {{
                // Fuerza a números finitos
                const toNum = v => {{
                    const n = Number(v);
                    return Number.isFinite(n) ? n : NaN;
                }};
                const xs   = Array.from(xsAny, toNum);
                const bids = Array.from(bidsAny, toNum);
                const asks = Array.from(asksAny, toNum);

                // Debug rápido
                const badX = xs.filter(v => !Number.isFinite(v)).length;
                const badB = bids.filter(v => !Number.isFinite(v)).length;
                const badA = asks.filter(v => !Number.isFinite(v)).length;
                if (badX || badB || badA) {{
                  console.warn("Non-finite values:", {{badX, badB, badA}});
                }}
                if (!(xs.length >= 2 && bids.length === xs.length && asks.length === xs.length)) {{
                  console.warn("Lens mismatch", xs.length, bids.length, asks.length);
                  return;
                }}

                // Fija escalas para visibilidad
                let minX = xs[0], maxX = xs[xs.length-1];
                if (minX === maxX) maxX = minX + 1e-6;

                let minY = 0;
                let maxY = Math.max(
                    ...bids.filter(Number.isFinite),
                    ...asks.filter(Number.isFinite),
                    1e-9
                );
                if (!Number.isFinite(maxY) || maxY <= 0) maxY = 1;

                window.depthChart.setData([xs, bids, asks]);
                window.depthChart.setScale('x', {{ min: minX, max: maxX }});
                window.depthChart.setScale('y', {{ min: minY, max: maxY*1.05 }});
            }};
        }})();
    "#);
    let _ = js_sys::Function::new_no_args(&js).call0(&JsValue::NULL);
}
