//! Unit tests for vector module using mock HTTP servers.

use wiremock::{
    matchers::{method, path, path_regex, body_string_contains},
    Mock, MockServer, ResponseTemplate,
};

use super::vespa_client::VespaClient;
use super::selector::EmbedClient;

// ── VespaClient tests ──────────────────────────────────────────────────────────

#[tokio::test]
async fn vespa_feed_ok() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path_regex(r"/document/v1/mcp_tools/mcp_tools/docid/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "id:mcp_tools:mcp_tools::svc:srv:tool",
            "pathId": "/document/v1/mcp_tools/mcp_tools/docid/svc%3Asrv%3Atool"
        })))
        .mount(&server)
        .await;

    let client = VespaClient::new(&server.uri());
    let doc = super::vespa_client::McpToolDoc {
        tool_id:       "test-deploy:svc:srv:tool".into(),
        environment: "test-deploy".into(),
        tenant_id:    "svc".into(),
        server_id:     "srv".into(),
        server_name:   "test_server".into(),
        tool_name:     "tool".into(),
        description:   "A test tool".into(),
        input_schema:  "{}".into(),
        embedding:     vec![0.1; 1024],
    };
    assert!(client.feed(&doc).await.is_ok());
}

#[tokio::test]
async fn vespa_feed_server_error_returns_err() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path_regex(r"/document/v1/.*"))
        .respond_with(ResponseTemplate::new(500).set_body_string("internal error"))
        .mount(&server)
        .await;

    let client = VespaClient::new(&server.uri());
    let doc = super::vespa_client::McpToolDoc {
        tool_id: "d:s:r:t".into(), environment: "d".into(), tenant_id: "s".into(),
        server_id: "r".into(), server_name: "s".into(), tool_name: "t".into(),
        description: "d".into(), input_schema: "{}".into(), embedding: vec![0.0; 1024],
    };
    assert!(client.feed(&doc).await.is_err());
}

#[tokio::test]
async fn vespa_search_returns_hits() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/search/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "root": {
                "id": "toplevel",
                "relevance": 1.0,
                "fields": { "totalCount": 2 },
                "children": [
                    {
                        "id": "id:mcp_tools:mcp_tools::test-deploy:svc:srv:get_weather",
                        "relevance": 0.92,
                        "fields": {
                            "tool_id": "test-deploy:svc:srv:get_weather",
                            "environment": "test-deploy",
                            "tenant_id": "svc",
                            "server_id": "srv",
                            "tool_name": "get_weather",
                            "description": "Get current weather",
                            "input_schema": "{\"type\":\"object\"}"
                        }
                    },
                    {
                        "id": "id:mcp_tools:mcp_tools::test-deploy:svc:srv:get_forecast",
                        "relevance": 0.85,
                        "fields": {
                            "tool_id": "test-deploy:svc:srv:get_forecast",
                            "environment": "test-deploy",
                            "tenant_id": "svc",
                            "server_id": "srv",
                            "tool_name": "get_forecast",
                            "description": "Get weather forecast",
                            "input_schema": "{\"type\":\"object\"}"
                        }
                    }
                ]
            }
        })))
        .mount(&server)
        .await;

    let client = VespaClient::new(&server.uri());
    let embedding = vec![0.1_f32; 1024];
    let hits = client.search(&embedding, "test-deploy", "svc", 8).await.unwrap();

    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].tool_name, "get_weather");
    assert!((hits[0].relevance - 0.92).abs() < 1e-3);
    assert_eq!(hits[1].tool_name, "get_forecast");
}

#[tokio::test]
async fn vespa_search_uses_contains_for_string_attributes() {
    // Regression: YQL `=` is a numeric range op; using it on a string-typed
    // attribute with a hyphenated value (e.g. `local-dev`) makes Vespa raise
    // `'local-dev' is not an int item expression: Illegal embedded sign character`.
    // The query must use `contains`, which is the YQL idiom for string-attribute
    // exact match.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/search/"))
        .and(body_string_contains("environment contains \\\"local-dev\\\""))
        .and(body_string_contains("tenant_id contains \\\"acct-123\\\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "root": { "id": "toplevel", "relevance": 1.0, "fields": { "totalCount": 0 } }
        })))
        .mount(&server)
        .await;

    let client = VespaClient::new(&server.uri());
    // If the YQL still used `=`, wiremock body match would fail and the call
    // would 404 — i.e. an Err return. `Ok(_)` proves the request body matched.
    assert!(
        client.search(&vec![0.0_f32; 1024], "local-dev", "acct-123", 8).await.is_ok(),
        "search must use `contains` for hyphenated string-attribute filters"
    );
}

#[tokio::test]
async fn vespa_search_empty_result() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/search/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "root": { "id": "toplevel", "relevance": 1.0, "fields": { "totalCount": 0 } }
        })))
        .mount(&server)
        .await;

    let client = VespaClient::new(&server.uri());
    let hits = client.search(&vec![0.0_f32; 1024], "test-deploy", "svc", 8).await.unwrap();
    assert!(hits.is_empty());
}

#[tokio::test]
async fn vespa_delete_server_ok() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path_regex(r"/document/v1/mcp_tools/mcp_tools/docid/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "documentCount": 5
        })))
        .mount(&server)
        .await;

    let client = VespaClient::new(&server.uri());
    assert!(client.delete_server("test-deploy", "svc", "srv").await.is_ok());
}

// ── EmbedClient tests ─────────────────────────────────────────────────────────

#[tokio::test]
async fn embed_client_single() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/embed"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "vector": vec![0.1_f32; 1024],
            "dims": 1024
        })))
        .mount(&server)
        .await;

    let client = EmbedClient::new(&server.uri());
    let vec = client.embed("서울 날씨").await.unwrap();
    assert_eq!(vec.len(), 1024);
}

#[tokio::test]
async fn embed_client_batch() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/embed/batch"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "vectors": [vec![0.1_f32; 1024], vec![0.2_f32; 1024]],
            "dims": 1024
        })))
        .mount(&server)
        .await;

    let client = EmbedClient::new(&server.uri());
    let vecs = client.embed_batch(&["text a", "text b"]).await.unwrap();
    assert_eq!(vecs.len(), 2);
    assert_eq!(vecs[0].len(), 1024);
}

#[tokio::test]
async fn embed_client_error_propagates() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/embed"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;

    let client = EmbedClient::new(&server.uri());
    assert!(client.embed("test").await.is_err());
}
