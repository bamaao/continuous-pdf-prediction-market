//! HTTP/WSS contract: response equals `crates/math` on the same θ.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine;
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
        c_m: 0,
        c_r: 0,
        fee_bps: 0,
        fee_timing: 0,
        slot: 7,
        traders: 2,
        tickets: 3,
        stake_usdc: 12,
        board_phase: 1,
        rho_raw: Q64::ONE.raw(),
        settle_cell: 0,
        liability: 10,
        c_p_board: 0,
        c_p_alloc: 0,
        close_ts: 0,
        risk_lock_ts: 0,
        report_window_secs: 0,
        extra_a: 0,
        extra_b: 0,
        extra_u2: 0,
    });
    store.replace_positions(vec![readpath::PositionRow {
        position: "Pos111111111111111111111111111111111111111".into(),
        owner: "Owner1111111111111111111111111111111111111".into(),
        market: market.clone(),
        set_hash: "aa".into(),
        q_raw: Q64::from_int(2).raw(),
        shares: 2,
        cost_paid: 1,
        claimed: true,
        paid_usdc: 2,
    }]);
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
async fn info_desk_is_implied_pdf_not_e() {
    let (store, market) = seeded();
    let app = router(store);
    let res = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/markets/{market}/info"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["traders"].as_u64().unwrap(), 2);
    assert_eq!(v["tickets"].as_u64().unwrap(), 3);
    assert_eq!(v["stake_usdc"].as_u64().unwrap(), 12);
    assert_eq!(v["c_r"].as_u64().unwrap(), 0);
    assert_eq!(v["c_m"].as_u64().unwrap(), 0);
    assert!(v["l_max_usdc"].as_u64().unwrap() >= 10);
    let cells = v["cells"].as_array().unwrap();
    assert_eq!(cells.len(), 8);
    let p0 = cells[0]["p_bps"].as_u64().unwrap();
    let e0 = cells[0]["e"].as_u64().unwrap();
    assert!(p0 > 1250);
    assert_eq!(e0, 10);
    assert_ne!(p0, e0);
    assert_eq!(v["rho_bps"].as_u64().unwrap(), 10_000);
    assert_eq!(v["peak_risk"]["payout_usdc"].as_u64().unwrap(), v["l_max_usdc"].as_u64().unwrap());
    assert_eq!(v["peak_risk"]["cell"].as_u64().unwrap(), 0);
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
fn project_prefers_board_revenue_and_filled_c_r() {
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
        0,
        Some((40, 5, 2)),
        Some(7),
        11,
    )
    .unwrap();
    assert_eq!(row.trading_revenue, 40);
    assert_eq!(row.c_m, 0);
    assert_eq!(row.premium_payable, 2);
    assert_eq!(row.c_r, 7);
    assert_eq!(row.slot, 11);
    assert_eq!(row.book().r_net(), 38);
}

#[tokio::test]
async fn book_snapshot_feeds_wasm_preview() {
    let (store, market) = seeded();
    let app = router(store);
    let res = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/markets/{market}/book"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let snap = math_wasm::BookSnap {
        beta_raw: v["beta_raw"].as_str().unwrap().into(),
        p0_raw: v["p0_raw"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().into()).collect(),
        theta_raw: v["theta_raw"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().into()).collect(),
        exposure_raw: v["exposure_raw"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().into()).collect(),
        trading_revenue: v["trading_revenue"].as_u64().unwrap(),
        premium_payable: v["premium_payable"].as_u64().unwrap(),
        c_m: v["c_m"].as_u64().unwrap(),
        c_r: v["c_r"].as_u64().unwrap(),
        slot: v["slot"].as_u64().unwrap(),
        fee_bps: v["fee_bps"].as_u64().unwrap() as u16,
    };
    let prev = math_wasm::preview_mask(&snap, "01", 1).unwrap();
    assert_eq!(prev.slot, 7);
    assert_eq!(prev.n, 8);
    assert!(!prev.p_s_raw.is_empty());
}

#[tokio::test]
async fn compose_deposit_is_client_instruction() {
    let owner = solana_sdk::pubkey::Pubkey::new_unique();
    let want = client::deposit(owner, 9);
    let app = router(std::sync::Arc::new(readpath::MemoryStore::new()));
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/compose")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"op":"deposit","owner": owner.to_string(), "amount": 9}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["program_id"], want.program_id.to_string());
    let data = base64::engine::general_purpose::STANDARD.decode(v["data_b64"].as_str().unwrap()).unwrap();
    assert_eq!(data, want.data);
}

#[tokio::test]
async fn preview_uses_math_wasm_on_same_theta() {
    let (store, market) = seeded();
    let app = router(store);
    let res = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/markets/{market}/preview?mask=01&shares=1"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["n"].as_u64().unwrap(), 8);
    assert!(!v["p_s_raw"].as_str().unwrap().is_empty());
    assert_eq!(v["fee_bps"].as_u64().unwrap(), 0);
    assert_eq!(v["pay_usdc"].as_u64().unwrap(), v["c_s_usdc"].as_u64().unwrap());
    assert_eq!(v["payout_if_miss_usdc"].as_u64().unwrap(), 0);
    assert_eq!(v["face_usdc"].as_u64().unwrap(), 1);
}

#[tokio::test]
async fn preview_ticket_is_display_only() {
    let (store, market) = seeded();
    let app = router(store);
    let res = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/markets/{market}/preview?mask=01&shares=1"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let pay = v["pay_usdc"].as_u64().unwrap();
    let hit = v["payout_if_hit_usdc"].as_u64().unwrap();
    assert_eq!(v["net_if_hit"].as_i64().unwrap(), hit as i64 - pay as i64);
    assert_eq!(v["payout_if_miss_usdc"].as_u64().unwrap(), 0);
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
    let health_bytes = health.into_body().collect().await.unwrap().to_bytes();
    let health_v: serde_json::Value = serde_json::from_slice(&health_bytes).unwrap();
    assert_eq!(health_v["pg"], false);
    let list = app
        .oneshot(Request::builder().uri("/v1/markets").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    let bytes = list.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["total"].as_u64().unwrap(), 1);
    assert_eq!(v["page"].as_u64().unwrap(), 1);
    assert_eq!(v["items"][0]["market"], market);
    assert_eq!(v["items"][0]["status"].as_u64().unwrap(), 1);
}

#[tokio::test]
async fn owner_positions_show_paid_pnl() {
    let (store, market) = seeded();
    let app = router(store);
    let res = app
        .oneshot(
            Request::builder()
                .uri("/v1/owners/Owner1111111111111111111111111111111111111/positions")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["total"].as_u64().unwrap(), 1);
    assert_eq!(v["items"][0]["market"], market);
    assert_eq!(v["items"][0]["prompt"], "paid");
    assert_eq!(v["items"][0]["paid_usdc"].as_u64().unwrap(), 2);
    assert_eq!(v["items"][0]["net_usdc"].as_i64().unwrap(), 1);
    assert_eq!(v["paid_tickets"].as_u64().unwrap(), 1);
}

#[tokio::test]
async fn owner_positions_show_listing_title() {
    let (store, market) = seeded();
    store.set_listing(
        &market,
        readpath::ListingMeta {
            title: "US CPI YoY persist check".into(),
            tags: vec!["macro".into()],
            category: "macro".into(),
            topic: "".into(),
            tag: "".into(),
            description: "First official print".into(),
            event: "US CPI YoY first print".into(),
            blocked_regions: vec![],
        },
    );
    let app = router(store);
    let res = app
        .oneshot(
            Request::builder()
                .uri("/v1/owners/Owner1111111111111111111111111111111111111/positions")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let v: serde_json::Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(v["items"][0]["title"], "US CPI YoY persist check");
}

#[tokio::test]
async fn tickets_journal_fills_positions_mask() {
    let (store, market) = seeded();
    let app = router(store);
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/tickets")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "owner": "Owner1111111111111111111111111111111111111",
                        "market": market,
                        "set_hash": "aa",
                        "kind": "mask",
                        "mask": "01",
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let res = app
        .oneshot(
            Request::builder()
                .uri("/v1/owners/Owner1111111111111111111111111111111111111/positions")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let v: serde_json::Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(v["items"][0]["mask"], "01");
    assert_eq!(v["items"][0]["ticket_kind"], "mask");
}

#[tokio::test]
async fn list_markets_search_and_paginate() {
    let (store, first) = seeded();
    for i in 0..5u8 {
        let mut row = store.get(&first).unwrap();
        row.market = format!("Page{i:02}11111111111111111111111111111111111111");
        row.family = if i % 2 == 0 { 0 } else { 1 };
        row.slot = 10 + i as u64;
        store.upsert(row);
    }
    let app = router(store);
    let page1 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/markets?limit=2&page=1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let v1: serde_json::Value =
        serde_json::from_slice(&page1.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(v1["total"].as_u64().unwrap(), 6);
    assert_eq!(v1["pages"].as_u64().unwrap(), 3);
    assert_eq!(v1["items"].as_array().unwrap().len(), 2);

    let skel = app
        .oneshot(
            Request::builder()
                .uri("/v1/markets?q=skellam&limit=20")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let vs: serde_json::Value =
        serde_json::from_slice(&skel.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert!(vs["total"].as_u64().unwrap() >= 1);
    assert!(vs["items"].as_array().unwrap().iter().all(|it| it["family"] == 0));
}

#[tokio::test]
async fn new_read_surfaces_exist() {
    let (store, market) = seeded();
    let app = router(store);
    for uri in [
        "/v1/auctions".to_string(),
        format!("/v1/markets/{market}/layers"),
        "/v1/ops/status".into(),
        "/v1/pool".into(),
        "/v1/owners/Owner1111111111111111111111111111111111111/risk".into(),
    ] {
        let res = app
            .clone()
            .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK, "{uri}");
    }
}

#[tokio::test]
async fn owner_risk_lists_indexed_quotes() {
    let (store, market) = seeded();
    store.replace_risk(
        vec![readpath::QuoteRow {
            quote: "Qte111111111111111111111111111111111111111".into(),
            market: market.clone(),
            lp: "Owner1111111111111111111111111111111111111".into(),
            layer_id: 1,
            capacity: 40,
            filled: 20,
            premium: 2,
            premium_owed: 2,
            profit_share_bps: 1000,
            cancelled: false,
        }],
        vec![readpath::LayerRow {
            layer: "Lay111111111111111111111111111111111111111".into(),
            market: market.clone(),
            layer_id: 1,
            attachment: 0,
            thickness: 40,
            filled: 20,
            quote_count: 1,
        }],
    );
    store.set_pool(15);
    store.patch_tap(&market, 10, 4);
    let app = router(store);
    let risk = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/owners/Owner1111111111111111111111111111111111111/risk")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let v: serde_json::Value =
        serde_json::from_slice(&risk.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(v["total"].as_u64().unwrap(), 1);
    assert_eq!(v["items"][0]["filled"].as_u64().unwrap(), 20);
    assert_eq!(v["items"][0]["weight_sum"].as_u64().unwrap(), 20_000);
    let pool = app
        .oneshot(Request::builder().uri("/v1/pool").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let p: serde_json::Value =
        serde_json::from_slice(&pool.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(p["c_p_pool"].as_u64().unwrap(), 15);
    assert_eq!(p["boards"][0]["c_p_board"].as_u64().unwrap(), 10);
    assert_eq!(p["boards"][0]["c_p_alloc"].as_u64().unwrap(), 4);
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

#[tokio::test]
async fn prior_cpi_milli_peaks_near_survey() {
    let app = router(Arc::new(MemoryStore::new()));
    let res = app
        .oneshot(
            Request::builder()
                .uri("/v1/prior?family=1&milli=true&n=32&x_min=-2000&x_max=12000&mu=2400&sigma=350")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let peak_x = v["peak_x"].as_f64().unwrap();
    assert!(
        peak_x > 1.5 && peak_x < 3.5,
        "CPI N(2.4, 0.35) peak_x={peak_x}"
    );
    assert_eq!(v["units"], "percentage points");
    assert!(!v["intervals"].as_array().unwrap().is_empty());
    assert_eq!(v["n"].as_u64().unwrap(), 32);
}

#[tokio::test]
async fn listings_survive_reload_from_disk() {
    let path = std::env::temp_dir().join(format!("cpm-listings-test-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let a = MemoryStore::new();
    a.persist_listings(&path);
    a.set_listing(
        "Mkt111111111111111111111111111111111111111",
        readpath::ListingMeta {
            title: "US CPI YoY".into(),
            tags: vec!["macro".into()],
            category: "macro".into(),
            topic: "cpi".into(),
            tag: "yoy".into(),
            description: "First official print".into(),
            event: "US CPI YoY".into(),
            blocked_regions: vec![],
        },
    );
    let b = MemoryStore::new();
    b.persist_listings(&path);
    let got = b.listing_of("Mkt111111111111111111111111111111111111111").expect("listing");
    assert_eq!(got.title, "US CPI YoY");
    assert_eq!(got.tags, vec!["macro".to_string()]);
    assert_eq!(got.category, "macro");
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn put_and_get_listing() {
    let (store, market) = seeded();
    let app = router(store);
    let body = serde_json::json!({
        "market": market,
        "title": "US CPI YoY",
        "category": "macro",
        "description": "First official print",
        "event": "CPI first print",
    });
    let post = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/listings")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(post.status(), StatusCode::OK);
    let get = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/listings/{market}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get.status(), StatusCode::OK);
    let row: serde_json::Value =
        serde_json::from_slice(&get.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(row["title"], "US CPI YoY");
    assert_eq!(row["tags"][0], "macro");
    let tagged = serde_json::json!({
        "market": market,
        "title": "阿森纳 vs 切尔西",
        "tags": ["football", "epl"],
        "description": "Premier League match. Settles on regulation full-time score.",
        "event": "Arsenal vs Chelsea",
    });
    let post_tags = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/listings")
                .header("content-type", "application/json")
                .body(Body::from(tagged.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(post_tags.status(), StatusCode::OK);
    let list = app
        .oneshot(Request::builder().uri("/v1/markets?tag=epl").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let page: serde_json::Value =
        serde_json::from_slice(&list.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(page["items"][0]["title"], "阿森纳 vs 切尔西");
    assert_eq!(page["items"][0]["tags"][1], "epl");
}

#[tokio::test]
async fn catalog_tags_add_and_delete_unused_only() {
    let (store, market) = seeded();
    let app = router(store);
    let add = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/tags")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"serie a"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(add.status(), StatusCode::OK);
    let listed = app
        .clone()
        .oneshot(Request::builder().uri("/v1/tags").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let tags: serde_json::Value =
        serde_json::from_slice(&listed.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert!(tags["items"].as_array().unwrap().iter().any(|t| t["name"] == "serie a" && t["used"] == 0));
    let attach = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/listings")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "market": market,
                        "title": "Arsenal vs Chelsea",
                        "tags": ["football", "epl"],
                        "description": "Premier League. Settles on regulation full-time score.",
                        "event": "Arsenal vs Chelsea",
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(attach.status(), StatusCode::OK);
    let used = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/v1/tags/football")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(used.status(), StatusCode::CONFLICT);
    let unused = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/v1/tags/serie%20a")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unused.status(), StatusCode::OK);
}

#[tokio::test]
async fn owner_vault_projects_l1_shape() {
    let app = router(Arc::new(MemoryStore::new()));
    let res = app
        .oneshot(
            Request::builder()
                .uri("/v1/owners/Owner1111111111111111111111111111111111111/vault")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let v: serde_json::Value =
        serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(v["mint"], "Circle SPL USDC");
    assert_eq!(v["source"], "l1");
    assert!(v["exists"].is_boolean());
    assert!(v["available"].is_number());
    assert!(v["free"].is_number());
}

const COMMENT_WALLET: &str = "So11111111111111111111111111111111111111112";

#[tokio::test]
async fn comments_require_indexed_market() {
    let app = router(Arc::new(MemoryStore::new()));
    let missing = "Missing11111111111111111111111111111111111";
    let get = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/markets/{missing}/comments"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get.status(), StatusCode::NOT_FOUND);
    let post = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/markets/{missing}/comments"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "author": COMMENT_WALLET, "body": "hello" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(post.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn comments_post_and_list_oldest_first() {
    let (store, market) = seeded();
    let app = router(store);
    let empty = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/markets/{market}/comments"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(empty.status(), StatusCode::OK);
    let empty_page: serde_json::Value =
        serde_json::from_slice(&empty.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(empty_page["total"], 0);
    assert!(empty_page["items"].as_array().unwrap().is_empty());

    for body in ["first note", "second note"] {
        let post = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/v1/markets/{market}/comments"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "author": COMMENT_WALLET, "body": body }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(post.status(), StatusCode::OK);
        let row: serde_json::Value =
            serde_json::from_slice(&post.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert_eq!(row["author"], COMMENT_WALLET);
        assert_eq!(row["body"], body);
        assert!(row["id"].as_i64().unwrap() > 0);
    }

    let listed = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/markets/{market}/comments"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(listed.status(), StatusCode::OK);
    let page: serde_json::Value =
        serde_json::from_slice(&listed.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(page["total"], 2);
    assert_eq!(page["items"][0]["body"], "first note");
    assert_eq!(page["items"][1]["body"], "second note");
    assert_eq!(page["items"][0]["author"], COMMENT_WALLET);
}

#[tokio::test]
async fn comments_reject_empty_or_oversized() {
    let (store, market) = seeded();
    let app = router(store);
    let empty = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/markets/{market}/comments"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "author": COMMENT_WALLET, "body": "   " }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(empty.status(), StatusCode::BAD_REQUEST);
    let long = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/markets/{market}/comments"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "author": COMMENT_WALLET, "body": "x".repeat(2001) }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(long.status(), StatusCode::BAD_REQUEST);
    let bad_author = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/markets/{market}/comments"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"author":"not-a-key","body":"hello"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bad_author.status(), StatusCode::BAD_REQUEST);
}

const REVIEW_WALLET: &str = "So11111111111111111111111111111111111111112";

fn application_body(title: &str) -> String {
    serde_json::json!({
        "applicant": REVIEW_WALLET,
        "family": 1,
        "title": title,
        "tags": ["macro"],
        "event": "US CPI YoY first print",
        "description": "First official print. Revisions do not settle.",
        "topic": "US_CPI_YOY",
        "tag": "2026-03",
        "blocked_regions": ["CN"],
        "compose": { "op": "create_gaussian", "close_in": 86400, "family": 1 },
    })
    .to_string()
}

#[tokio::test]
async fn review_system_queues_then_reviewer_approves() {
    let app = router(Arc::new(MemoryStore::new()));
    let post = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/listings/applications")
                .header("content-type", "application/json")
                .body(Body::from(application_body("US CPI YoY")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(post.status(), StatusCode::OK);
    let created: serde_json::Value =
        serde_json::from_slice(&post.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(created["status_name"], "pending_review");
    assert_eq!(created["logs"][0]["action"], "submit");
    assert_eq!(created["compose"]["op"], "create_gaussian");
    assert_eq!(created["market"], "");
    let id = created["id"].as_i64().unwrap();

    let dup = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/listings/applications")
                .header("content-type", "application/json")
                .body(Body::from(application_body("us cpi yoy")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(dup.status(), StatusCode::CONFLICT);

    let decide = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/review")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "id": id, "reviewer": REVIEW_WALLET, "action": "approve" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(decide.status(), StatusCode::OK);
    let done: serde_json::Value =
        serde_json::from_slice(&decide.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(done["status_name"], "approved");
    assert!(done["logs"].as_array().unwrap().iter().any(|l| l["action"] == "approve"));

    let opened = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/review")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "id": id,
                        "reviewer": REVIEW_WALLET,
                        "action": "opened",
                        "market": REVIEW_WALLET
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(opened.status(), StatusCode::OK);
    let live: serde_json::Value =
        serde_json::from_slice(&opened.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(live["status_name"], "open");
    assert_eq!(live["market"], REVIEW_WALLET);
}

#[tokio::test]
async fn geo_ip_hides_blocked_listing() {
    let (store, market) = seeded();
    store.set_listing(
        &market,
        readpath::ListingMeta {
            title: "Blocked".into(),
            tags: vec!["macro".into()],
            category: "macro".into(),
            topic: "".into(),
            tag: "".into(),
            description: "First official print".into(),
            event: "US CPI YoY first print".into(),
            blocked_regions: vec!["CN".into()],
        },
    );
    let app = router(store);
    let hidden = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/markets?limit=20")
                .header("cf-ipcountry", "CN")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let page: serde_json::Value =
        serde_json::from_slice(&hidden.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(page["total"], 0);
    let visible = app
        .oneshot(
            Request::builder()
                .uri("/v1/markets?limit=20")
                .header("cf-ipcountry", "US")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let page: serde_json::Value =
        serde_json::from_slice(&visible.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(page["total"], 1);
}

#[tokio::test]
async fn compose_buy_after_close_ts_is_forbidden() {
    let n = 8u16;
    let p0: Vec<i128> = uniform_prior(n as usize).into_iter().map(|q| q.raw()).collect();
    let market_pk = solana_sdk::pubkey::Pubkey::new_unique();
    let market = market_pk.to_string();
    let store = Arc::new(MemoryStore::new());
    store.upsert(MarketProj {
        market: market.clone(),
        family: 1,
        status: 1,
        n,
        beta: Q64::from_int(100).raw(),
        p0,
        theta: vec![0i128; n as usize],
        exposure: vec![0i128; n as usize],
        trading_revenue: 0,
        premium_payable: 0,
        c_m: 0,
        c_r: 0,
        fee_bps: 0,
        fee_timing: 0,
        slot: 1,
        traders: 0,
        tickets: 0,
        stake_usdc: 0,
        board_phase: 0,
        rho_raw: 0,
        settle_cell: 0,
        liability: 0,
        c_p_board: 0,
        c_p_alloc: 0,
        close_ts: 1,
        risk_lock_ts: 1,
        report_window_secs: 0,
        extra_a: 0,
        extra_b: 0,
        extra_u2: 0,
    });
    let owner = solana_sdk::pubkey::Pubkey::new_unique();
    let app = router(store);
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/compose")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "op": "buy_set",
                        "owner": owner.to_string(),
                        "market": market,
                        "mask": "01",
                        "shares": 1,
                        "nonce": 1
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}
