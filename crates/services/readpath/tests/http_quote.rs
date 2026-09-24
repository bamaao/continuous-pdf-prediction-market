//! HTTP/WSS contract: response equals `crates/math` on the same θ.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use futures_util::StreamExt;
use http_body_util::BodyExt;
use math::uniform_prior;
use math::Q64;
use readpath::router;
use readpath::store::{MarketProj, MemoryStore};
use std::sync::Arc;
use tower::ServiceExt;

fn seeded() -> (Arc<MemoryStore>, String) {
    let n = 8u16;
    let p0: Vec<i128> = uniform_prior(n as usize).into_iter().map(|q| q.raw()).collect();
    let mut theta = vec![0i128; n as usize];
    let mut exposure = vec![0i128; n as usize];
    theta[0] = Q64::from_int(10).raw();
    exposure[0] = Q64::from_int(10).raw();
    let market = "Seed111111111111111111111111111111111111111".to_string();
    let store = Arc::new(MemoryStore::new());
    store.upsert(MarketProj {
        market: market.clone(),
        family: 1,
        status: 1,
        n,
        beta: Q64::from_int(100).raw(),
        p0,
        theta,
        exposure,
        trading_revenue: 4,
        premium_payable: 0,
        c_m: 5,
        c_r: 0,
        slot: 7,
    });
    (store, market)
}

#[tokio::test]
async fn http_quote_matches_math_on_same_theta() {
    let (store, market) = seeded();
    let row = store.get(&market).unwrap();
    let book = row.book();
    let mut cell0 = vec![false; 8];
    cell0[0] = true;
    let expect = book.view(&cell0, Q64::from_int(1));

    let app = router(store);
    let res = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/markets/{market}/quote?mask=01&shares=1"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["p_s_raw"].as_str().unwrap(), expect.p_s.raw().to_string());
    assert_eq!(v["c_s_raw"].as_str().unwrap(), expect.c_s.raw().to_string());
    assert_eq!(v["coverage_bps"].as_u64().unwrap(), quote::q_bps(expect.coverage));
    assert_eq!(v["rho_hat_bps"].as_u64().unwrap(), quote::q_bps(expect.rho_hat));
    assert_eq!(v["l_max_usdc"].as_u64().unwrap(), expect.l_max_usdc);
    assert_eq!(v["c_max_usdc"].as_u64().unwrap(), expect.c_max_usdc);
    assert_eq!(v["slot"].as_u64().unwrap(), 7);
}

#[tokio::test]
async fn pdf_is_not_exposure() {
    let (store, market) = seeded();
    let app = router(store);
    let res = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/markets/{market}/pdf"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let cells = v["cells"].as_array().unwrap();
    assert_eq!(cells.len(), 8);
    let p0 = cells[0]["p_bps"].as_u64().unwrap();
    let e0 = cells[0]["e"].as_u64().unwrap();
    assert!(p0 > 1250, "buy on cell 0 must raise p_0 above 1/8");
    assert_eq!(e0, 10);
    assert_ne!(p0, e0);
}

#[test]
fn project_prefers_board_and_filled_c_r() {
    let p0 = vec![Q64::from_ratio(1, 4).raw(); 4];
    let z = vec![0i128; 4];
    let row = readpath::project(
        "M".into(),
        1,
        1,
        4,
        Q64::from_int(100).raw(),
        p0,
        z.clone(),
        z,
        1,
        9,
        Some((40, 5, 2)),
        Some(7),
        11,
    )
    .unwrap();
    assert_eq!(row.trading_revenue, 40);
    assert_eq!(row.c_m, 5);
    assert_eq!(row.premium_payable, 2);
    assert_eq!(row.c_r, 7);
    assert_eq!(row.slot, 11);
    assert_eq!(row.book().r_net(), 38);
}

#[tokio::test]
async fn health_and_list() {
    let (store, market) = seeded();
    let app = router(store);
    let health = app
        .clone()
        .oneshot(Request::builder().uri("/v1/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(health.status(), StatusCode::OK);
    let list = app
        .oneshot(Request::builder().uri("/v1/markets").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    let bytes = list.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v[0]["market"], market);
}

#[tokio::test]
async fn ws_emits_when_theta_changes() {
    let (store, market) = seeded();
    let app = router(store.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let url = format!("ws://{addr}/v1/markets/{market}/ws");
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    let first = match ws.next().await.unwrap().unwrap() {
        tokio_tungstenite::tungstenite::Message::Text(t) => t.to_string(),
        other => panic!("want text, got {other:?}"),
    };
    let v: serde_json::Value = serde_json::from_str(&first).unwrap();
    assert_eq!(v["slot"].as_u64().unwrap(), 7);
    let mut row = store.get(&market).unwrap();
    row.theta[1] = Q64::from_int(5).raw();
    row.slot = 8;
    store.upsert(row);
    let second = match ws.next().await.unwrap().unwrap() {
        tokio_tungstenite::tungstenite::Message::Text(t) => t.to_string(),
        other => panic!("want text, got {other:?}"),
    };
    assert_ne!(first, second);
    let v2: serde_json::Value = serde_json::from_str(&second).unwrap();
    assert_eq!(v2["slot"].as_u64().unwrap(), 8);
}
