use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use serde_json::json;
use std::fs;
use talos_storage::{
    config::STORE_PATH,
    handlers::{
        create_category, delete_entry, decrypt_secret, encrypt_and_save, list_tree, TreeNode,
    },
    init::init_storage,
};
use tempfile::tempdir;
use tower::ServiceExt;
use wiremock::{matchers::{method, path}, Mock, MockServer, ResponseTemplate};

// Helper to create the app router and a mock bunker
async fn setup_test_env() -> (Router, MockServer) {
    let bunker_mock = MockServer::start().await;

    // Set env vars for the test. This works because tests run in separate processes.
    std::env::set_var("BUNKER_URL", bunker_mock.uri());
    let temp_dir = tempdir().unwrap();
    std::env::set_var("PASSWORD_STORE_DIR", temp_dir.path().to_str().unwrap());

    // We need to "forget" the temp_dir so it's not dropped at the end of this function,
    // otherwise the directory will be deleted before tests can use it.
    // The OS will clean it up when the test process exits.
    std::mem::forget(temp_dir);

    // The init_storage function will now use the temp dir from the env var
    init_storage().await;

    let app = Router::new()
        .route("/api/tree", axum::routing::get(list_tree))
        .route("/api/decrypt", axum::routing::post(decrypt_secret))
        .route("/api/save", axum::routing::post(encrypt_and_save))
        .route("/api/delete", axum::routing::post(delete_entry))
        .route("/api/create_category", axum::routing::post(create_category));

    (app, bunker_mock)
}

#[tokio::test]
async fn test_create_list_and_delete_category() {
    let (app, _bunker) = setup_test_env().await;
    let category_path = "TestCategory";

    // 1. Create a category
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/create_category")
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"path": "{}"}}"#, category_path)))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert!(std::path::Path::new(&*STORE_PATH).join(category_path).exists());

    // 2. List tree and verify category exists
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/tree")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let tree: Vec<TreeNode> = serde_json::from_slice(&body).unwrap();

    assert_eq!(tree.len(), 1);
    assert_eq!(tree[0].name, category_path);
    assert!(tree[0].is_dir);

    // 3. Delete the empty category
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/delete")
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"path": "{}"}}"#, category_path)))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert!(!std::path::Path::new(&*STORE_PATH).join(category_path).exists());
}

#[tokio::test]
async fn test_full_secret_lifecycle() {
    let (app, bunker) = setup_test_env().await;

    let secret_path = "Social/twitter";
    let secret_content = "MyTwitterPassword\nUser: myuser";
    let encrypted_content = "-----BEGIN PGP MESSAGE-----\nENCRYPTED\n-----END PGP MESSAGE-----";

    // --- 1. Mock Bunker for encryption ---
    Mock::given(method("POST"))
        .and(path("/process"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": encrypted_content
        })))
        .mount(&bunker)
        .await;

    // --- 2. Save a secret ---
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/save")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"path": secret_path, "content": secret_content}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let secret_file_path = std::path::Path::new(&*STORE_PATH).join(format!("{}.gpg", secret_path));
    assert!(secret_file_path.exists());
    let file_content = fs::read_to_string(secret_file_path).unwrap();
    assert_eq!(file_content, encrypted_content);

    // --- 3. Mock Bunker for decryption ---
    Mock::given(method("POST"))
        .and(path("/process"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": secret_content
        })))
        .mount(&bunker)
        .await;

    // --- 4. Decrypt the secret ---
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/decrypt")
                .header("content-type", "application/json")
                .body(Body::from(json!({"path": secret_path}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let decrypted: String = serde_json::from_slice(&body).unwrap();
    assert_eq!(decrypted, secret_content);

    // --- 5. Delete the secret ---
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/delete")
                .header("content-type", "application/json")
                .body(Body::from(json!({"path": secret_path}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert!(!std::path::Path::new(&*STORE_PATH)
        .join(format!("{}.gpg", secret_path))
        .exists());
}