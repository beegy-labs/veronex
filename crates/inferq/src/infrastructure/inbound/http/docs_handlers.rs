use axum::extract::Query;
use axum::http::header;
use axum::response::{Html, IntoResponse, Response};
use serde::Deserialize;
use serde_json::Value;

const SPEC: &str = include_str!("openapi.json");
const OVERLAY_KO: &str = include_str!("openapi.overlay.ko.json");
const OVERLAY_JA: &str = include_str!("openapi.overlay.ja.json");

#[derive(Deserialize)]
pub struct LangQuery {
    lang: Option<String>,
}

/// Recursively merge `overlay` into `base`.
/// - Objects: overlay keys are merged (deep).
/// - Arrays / scalars: overlay value replaces base.
fn merge_json(base: &mut Value, overlay: &Value) {
    match (base, overlay) {
        (Value::Object(base_map), Value::Object(overlay_map)) => {
            for (key, val) in overlay_map {
                let base_val = base_map.entry(key.clone()).or_insert(Value::Null);
                merge_json(base_val, val);
            }
        }
        (base, overlay) => {
            *base = overlay.clone();
        }
    }
}

/// GET /docs/openapi.json?lang=ko|ja|en — serve the embedded OpenAPI spec,
/// optionally merged with a locale overlay.
pub async fn openapi_json(Query(q): Query<LangQuery>) -> Response {
    let overlay_src = match q.lang.as_deref() {
        Some("ko") => Some(OVERLAY_KO),
        Some("ja") => Some(OVERLAY_JA),
        _ => None,
    };

    match overlay_src {
        Some(overlay_str) => {
            let mut spec: Value = serde_json::from_str(SPEC).unwrap_or(Value::Null);
            if let Ok(overlay) = serde_json::from_str::<Value>(overlay_str) {
                merge_json(&mut spec, &overlay);
            }
            let body = serde_json::to_string(&spec).unwrap_or_default();
            ([(header::CONTENT_TYPE, "application/json")], body).into_response()
        }
        None => ([(header::CONTENT_TYPE, "application/json")], SPEC).into_response(),
    }
}

/// GET /docs/swagger — Swagger UI (CDN). Lang param forwarded to spec URL.
pub async fn swagger_ui() -> impl IntoResponse {
    Html(r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Veronex API — Swagger UI</title>
  <link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css" />
  <style>
    body { margin: 0; }
    .swagger-ui .topbar { background-color: #78350f; }
    .swagger-ui .topbar .download-url-wrapper .select-label select { border: 2px solid #c2710a; }
  </style>
</head>
<body>
  <div id="swagger-ui"></div>
  <script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
  <script>
    SwaggerUIBundle({
      url: '/docs/openapi.json',
      dom_id: '#swagger-ui',
      presets: [SwaggerUIBundle.presets.apis, SwaggerUIBundle.SwaggerUIStandalonePreset],
      layout: 'BaseLayout',
      deepLinking: true,
      tryItOutEnabled: true,
      persistAuthorization: true,
    });
  </script>
</body>
</html>"#)
}

/// GET /docs/redoc — ReDoc viewer (CDN).
pub async fn redoc_ui() -> impl IntoResponse {
    Html(r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Veronex API — ReDoc</title>
  <style>
    body { margin: 0; padding: 0; }
  </style>
</head>
<body>
  <redoc spec-url="/docs/openapi.json" expand-responses="200,201"></redoc>
  <script src="https://cdn.jsdelivr.net/npm/redoc@latest/bundles/redoc.standalone.js"></script>
</body>
</html>"#)
}
