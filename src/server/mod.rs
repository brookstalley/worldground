pub mod protocol;

use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, RwLock};
use tokio_tungstenite::tungstenite::Message;
use tracing::{error, info, warn};

use crate::simulation::statistics::TickStatistics;
use crate::world::tile::Season;
use crate::world::Tile;
use crate::world::weather_systems::PressureSystem;
use protocol::{
    compute_tile_diffs, HealthStatus, PressureSystemSnapshot, TickDiff, TickStatSummary,
    WorldSnapshot,
};

/// Shared server state accessible from all connection handlers and the simulation loop.
pub struct ServerState {
    /// Current world snapshot message (JSON string, ready to send).
    pub snapshot_json: RwLock<String>,
    /// Broadcast channel for tick diffs.
    pub tick_sender: broadcast::Sender<String>,
    /// Health data updated each tick.
    pub health: RwLock<HealthData>,
}

/// Data needed for the health endpoint.
pub struct HealthData {
    pub tick: u64,
    pub season: Season,
    pub tile_count: u32,
    pub diversity_index: f32,
    pub rule_errors: u32,
    pub last_snapshot_tick: u64,
    pub recent_tick_durations_ms: Vec<f32>,
}

impl HealthData {
    pub fn tick_rate(&self) -> f32 {
        if self.recent_tick_durations_ms.is_empty() {
            return 0.0;
        }
        let avg_ms: f32 =
            self.recent_tick_durations_ms.iter().sum::<f32>() / self.recent_tick_durations_ms.len() as f32;
        if avg_ms <= 0.0 {
            return 0.0;
        }
        1000.0 / avg_ms
    }
}

impl ServerState {
    pub fn new(initial_snapshot_json: String) -> Self {
        let (tx, _) = broadcast::channel(64);
        ServerState {
            snapshot_json: RwLock::new(initial_snapshot_json),
            tick_sender: tx,
            health: RwLock::new(HealthData {
                tick: 0,
                season: Season::Spring,
                tile_count: 0,
                diversity_index: 0.0,
                rule_errors: 0,
                last_snapshot_tick: 0,
                recent_tick_durations_ms: Vec::new(),
            }),
        }
    }

    /// Update server state after a tick completes.
    /// Called by the simulation loop with the new snapshot, diff, and statistics.
    pub async fn on_tick(
        &self,
        new_snapshot_json: Option<String>,
        diff_json: String,
        stats: &TickStatistics,
        tick: u64,
        season: Season,
        tile_count: u32,
        last_snapshot_tick: u64,
    ) {
        // Update snapshot for new connections (only when a full rebuild is provided)
        if let Some(json) = new_snapshot_json {
            *self.snapshot_json.write().await = json;
        }

        // Broadcast diff to all connected clients
        // Ignore send error (no receivers is fine)
        let _ = self.tick_sender.send(diff_json);

        // Update health data
        let mut health = self.health.write().await;
        health.tick = tick;
        health.season = season;
        health.tile_count = tile_count;
        health.diversity_index = stats.diversity_index;
        health.rule_errors = stats.rule_errors;
        health.last_snapshot_tick = last_snapshot_tick;
        health.recent_tick_durations_ms.push(stats.tick_duration_ms);
        // Keep only the last 100 tick durations for rate calculation
        if health.recent_tick_durations_ms.len() > 100 {
            health.recent_tick_durations_ms.remove(0);
        }
    }
}

/// Build the JSON diff message for a tick.
pub fn build_diff_json(
    before_tiles: &[Tile],
    after_tiles: &[Tile],
    tick: u64,
    season: Season,
    stats: &TickStatistics,
    pressure_systems: &[PressureSystem],
) -> String {
    let changed_tiles = compute_tile_diffs(before_tiles, after_tiles);
    let diff = TickDiff {
        message_type: "TickDiff",
        tick,
        season,
        changed_tiles,
        statistics: TickStatSummary::from_statistics(stats),
        pressure_systems: pressure_systems
            .iter()
            .map(PressureSystemSnapshot::from_system)
            .collect(),
    };
    serde_json::to_string(&diff).unwrap_or_else(|_| "{}".to_string())
}

/// Build the JSON diff from lightweight layer snapshots (avoids full tile clone).
pub fn build_diff_json_from_layers(
    before_layers: &[(crate::world::tile::WeatherLayer, crate::world::tile::ConditionsLayer, crate::world::tile::BiomeLayer, crate::world::tile::ResourceLayer)],
    after_tiles: &[Tile],
    tick: u64,
    season: Season,
    stats: &TickStatistics,
    pressure_systems: &[PressureSystem],
) -> String {
    let mut changed_tiles = Vec::new();
    for (i, tile) in after_tiles.iter().enumerate() {
        if let Some((bw, bc, bb, br)) = before_layers.get(i) {
            let weather_changed = *bw != tile.weather;
            let conditions_changed = *bc != tile.conditions;
            let biome_changed = *bb != tile.biome;
            let resources_changed = *br != tile.resources;

            if weather_changed || conditions_changed || biome_changed || resources_changed {
                changed_tiles.push(protocol::TileChange {
                    id: tile.id,
                    weather: if weather_changed { Some(tile.weather.clone()) } else { None },
                    conditions: if conditions_changed { Some(tile.conditions.clone()) } else { None },
                    biome: if biome_changed { Some(tile.biome.clone()) } else { None },
                    resources: if resources_changed { Some(tile.resources.clone()) } else { None },
                });
            }
        }
    }
    let diff = protocol::TickDiff {
        message_type: "TickDiff",
        tick,
        season,
        changed_tiles,
        statistics: protocol::TickStatSummary::from_statistics(stats),
        pressure_systems: pressure_systems
            .iter()
            .map(protocol::PressureSystemSnapshot::from_system)
            .collect(),
    };
    serde_json::to_string(&diff).unwrap_or_else(|_ | "{}".to_string())
}

/// Build the JSON snapshot message for a world.
pub fn build_snapshot_json(world: &crate::world::World) -> String {
    let snapshot = WorldSnapshot::from_world(world);
    serde_json::to_string(&snapshot).unwrap_or_else(|_| "{}".to_string())
}

/// Start the WebSocket + HTTP server on the given address.
/// Returns a handle that can be used to stop the server.
pub async fn start_server(
    state: Arc<ServerState>,
    addr: SocketAddr,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = TcpListener::bind(addr).await?;
    info!(%addr, "Server listening — viewer at http://{}", addr);

    loop {
        let (stream, peer) = listener.accept().await?;
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, peer, state).await {
                error!(%peer, "Connection error: {}", e);
            }
        });
    }
}

/// Handle an incoming TCP connection — route to WebSocket or HTTP.
async fn handle_connection(
    stream: TcpStream,
    peer: SocketAddr,
    state: Arc<ServerState>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Peek at the first bytes to determine if this is a WebSocket upgrade or HTTP request
    let mut buf = [0u8; 512];
    let n = stream.peek(&mut buf).await?;
    let request_line = String::from_utf8_lossy(&buf[..n]).to_lowercase();

    if request_line.contains("upgrade: websocket") {
        handle_websocket(stream, peer, state).await
    } else if request_line.contains("get /health") {
        handle_health_request(stream, state).await
    } else {
        // Serve the viewer for any other HTTP request (GET /, GET /index.html, etc.)
        handle_viewer_request(stream).await
    }
}

/// Handle a WebSocket connection: send snapshot, then stream diffs.
async fn handle_websocket(
    stream: TcpStream,
    peer: SocketAddr,
    state: Arc<ServerState>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ws_stream = tokio_tungstenite::accept_async(stream).await?;
    info!(%peer, "WebSocket connected");

    let (mut write, mut read) = futures_util::StreamExt::split(ws_stream);

    // Send current snapshot
    let snapshot = state.snapshot_json.read().await.clone();
    futures_util::SinkExt::send(&mut write, Message::Text(snapshot.into())).await?;

    // Subscribe to tick diffs
    let mut rx = state.tick_sender.subscribe();

    // Stream diffs until client disconnects
    loop {
        tokio::select! {
            diff = rx.recv() => {
                match diff {
                    Ok(json) => {
                        if futures_util::SinkExt::send(&mut write, Message::Text(json.into())).await.is_err() {
                            break; // Client disconnected
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(%peer, lagged = n, "Client lagged behind on diffs");
                        // Continue — client missed some diffs but will stay connected
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        break; // Server shutting down
                    }
                }
            }
            msg = futures_util::StreamExt::next(&mut read) => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(_)) => break,
                    _ => {} // Ignore other messages from client
                }
            }
        }
    }

    info!(%peer, "WebSocket disconnected");
    Ok(())
}

/// Handle an HTTP request by serving the embedded viewer.
async fn handle_viewer_request(
    mut stream: TcpStream,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncWriteExt;

    // Read and discard the full HTTP request
    let mut buf = vec![0u8; 4096];
    let _ = stream.read(&mut buf).await?;

    const VIEWER_HTML: &str = include_str!("../../viewer/index.html");
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-cache\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        VIEWER_HTML.len(),
        VIEWER_HTML
    );

    stream.write_all(response.as_bytes()).await?;
    stream.shutdown().await?;

    Ok(())
}

/// Handle an HTTP health request.
async fn handle_health_request(
    mut stream: TcpStream,
    state: Arc<ServerState>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncWriteExt;

    // Read and discard the full HTTP request
    let mut buf = vec![0u8; 4096];
    let _ = stream.read(&mut buf).await?;

    let health = state.health.read().await;
    let status = HealthStatus {
        tick: health.tick,
        tick_rate: health.tick_rate(),
        diversity_index: health.diversity_index,
        rule_errors: health.rule_errors,
        snapshot_age_ticks: health.tick.saturating_sub(health.last_snapshot_tick),
        tile_count: health.tile_count,
        season: health.season,
    };

    let body = serde_json::to_string(&status)?;
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );

    stream.write_all(response.as_bytes()).await?;
    stream.shutdown().await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::generation::GenerationParams;
    use crate::simulation::statistics::TickStatistics;
    use crate::world::generation::generate_world;
    use crate::world::tile::*;
    use crate::world::World;
    use std::collections::HashMap;
    use std::time::Duration;

    fn default_gen_params(tile_count: u32) -> GenerationParams {
        GenerationParams {
            seed: 42,
            tile_count,
            ocean_ratio: 0.3,
            mountain_ratio: 0.1,
            elevation_roughness: 0.5,
            climate_bands: true,
            resource_density: 0.3,
            initial_biome_maturity: 0.5,
            topology: crate::config::generation::TopologyConfig::default(),
        }
    }

    fn make_small_world() -> World {
        generate_world(&default_gen_params(100))
    }

    fn make_test_stats(tick: u64) -> TickStatistics {
        TickStatistics {
            tick,
            biome_distribution: HashMap::new(),
            avg_temperature: 288.0,
            avg_moisture: 0.4,
            avg_vegetation_health: 0.7,
            weather_coverage: HashMap::new(),
            diversity_index: 0.65,
            rule_errors: 0,
            tick_duration_ms: 100.0,
        }
    }

    #[test]
    fn build_snapshot_json_is_valid() {
        let world = make_small_world();
        let json = build_snapshot_json(&world);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed["message_type"], "WorldSnapshot");
        assert_eq!(parsed["tile_count"], 100);
        assert!(parsed["tiles"].is_array());
        assert_eq!(parsed["tiles"].as_array().unwrap().len(), 100);
    }

    #[test]
    fn build_diff_json_is_valid() {
        let before = vec![
            Tile::new_default(0, vec![], Position::flat(0.0, 0.0)),
            Tile::new_default(1, vec![], Position::flat(1.0, 0.0)),
        ];
        let mut after = before.clone();
        after[0].weather.temperature = 300.0;

        let stats = make_test_stats(1);
        let json = build_diff_json(&before, &after, 1, Season::Spring, &stats, &[]);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed["message_type"], "TickDiff");
        assert_eq!(parsed["tick"], 1);
        let changes = parsed["changed_tiles"].as_array().unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0]["id"], 0);
        assert!(changes[0]["weather"].is_object());
        // conditions/biome/resources should not be in JSON (skip_serializing_if)
        assert!(changes[0].get("conditions").is_none());
    }

    #[test]
    fn build_diff_json_empty_when_no_changes() {
        let tiles = vec![Tile::new_default(0, vec![], Position::flat(0.0, 0.0))];
        let stats = make_test_stats(1);
        let json = build_diff_json(&tiles, &tiles, 1, Season::Spring, &stats, &[]);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert!(parsed["changed_tiles"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn server_state_on_tick_updates_health() {
        let state = ServerState::new("{}".to_string());
        let stats = make_test_stats(5);

        state
            .on_tick(
                Some("new_snapshot".to_string()),
                "diff".to_string(),
                &stats,
                5,
                Season::Summer,
                100,
                3,
            )
            .await;

        let health = state.health.read().await;
        assert_eq!(health.tick, 5);
        assert_eq!(health.season, Season::Summer);
        assert_eq!(health.tile_count, 100);
        assert!((health.diversity_index - 0.65).abs() < 0.01);
        assert_eq!(health.last_snapshot_tick, 3);
        assert_eq!(health.recent_tick_durations_ms.len(), 1);
    }

    #[tokio::test]
    async fn server_state_updates_snapshot_for_new_clients() {
        let state = ServerState::new("initial".to_string());
        assert_eq!(*state.snapshot_json.read().await, "initial");

        let stats = make_test_stats(1);
        state
            .on_tick(
                Some("updated".to_string()),
                "diff".to_string(),
                &stats,
                1,
                Season::Spring,
                100,
                0,
            )
            .await;

        assert_eq!(*state.snapshot_json.read().await, "updated");
    }

    #[tokio::test]
    async fn tick_rate_calculation() {
        let state = ServerState::new("{}".to_string());

        // Simulate 5 ticks at 200ms each → 5 ticks/sec
        for i in 0..5 {
            let mut stats = make_test_stats(i);
            stats.tick_duration_ms = 200.0;
            state
                .on_tick(
                    Some("{}".to_string()),
                    "{}".to_string(),
                    &stats,
                    i,
                    Season::Spring,
                    100,
                    0,
                )
                .await;
        }

        let health = state.health.read().await;
        assert!((health.tick_rate() - 5.0).abs() < 0.1);
    }

    #[tokio::test]
    async fn health_recent_durations_capped_at_100() {
        let state = ServerState::new("{}".to_string());

        for i in 0..150 {
            let stats = make_test_stats(i);
            state
                .on_tick(
                    Some("{}".to_string()),
                    "{}".to_string(),
                    &stats,
                    i,
                    Season::Spring,
                    100,
                    0,
                )
                .await;
        }

        let health = state.health.read().await;
        assert_eq!(health.recent_tick_durations_ms.len(), 100);
    }

    #[tokio::test]
    async fn broadcast_diff_to_subscribers() {
        let state = ServerState::new("{}".to_string());
        let mut rx = state.tick_sender.subscribe();

        let stats = make_test_stats(1);
        state
            .on_tick(
                Some("{}".to_string()),
                "test_diff".to_string(),
                &stats,
                1,
                Season::Spring,
                100,
                0,
            )
            .await;

        let received = rx.recv().await.expect("should receive diff");
        assert_eq!(received, "test_diff");
    }

    #[tokio::test]
    async fn websocket_client_receives_snapshot_and_diff() {
        let world = make_small_world();
        let snapshot_json = build_snapshot_json(&world);
        let state = Arc::new(ServerState::new(snapshot_json));

        // Bind server to ephemeral port
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server_state = Arc::clone(&state);
        let server_handle = tokio::spawn(async move {
            if let Ok((stream, peer)) = listener.accept().await {
                let _ = handle_websocket(stream, peer, server_state).await;
            }
        });

        // Connect as WebSocket client
        let url = format!("ws://127.0.0.1:{}", addr.port());
        let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();

        // Receive snapshot
        let msg = tokio::time::timeout(
            Duration::from_secs(5),
            futures_util::StreamExt::next(&mut ws),
        )
        .await
        .expect("timeout waiting for snapshot")
        .expect("stream ended")
        .expect("message error");

        let text = msg.into_text().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["message_type"], "WorldSnapshot");
        assert_eq!(parsed["tiles"].as_array().unwrap().len(), 100);

        // Send a tick diff via broadcast
        let stats = make_test_stats(1);
        state
            .on_tick(
                Some("{}".to_string()),
                r#"{"message_type":"TickDiff","tick":1}"#.to_string(),
                &stats,
                1,
                Season::Spring,
                100,
                0,
            )
            .await;

        // Receive diff
        let msg = tokio::time::timeout(
            Duration::from_secs(5),
            futures_util::StreamExt::next(&mut ws),
        )
        .await
        .expect("timeout waiting for diff")
        .expect("stream ended")
        .expect("message error");

        let text = msg.into_text().unwrap();
        assert!(text.contains("TickDiff"));

        // Clean disconnect
        futures_util::SinkExt::close(&mut ws).await.unwrap();
        let _ = server_handle.await;
    }

    #[tokio::test]
    async fn health_endpoint_returns_json() {
        let state = Arc::new(ServerState::new("{}".to_string()));

        // Update health data
        let stats = make_test_stats(42);
        state
            .on_tick(
                Some("{}".to_string()),
                "{}".to_string(),
                &stats,
                42,
                Season::Autumn,
                1000,
                40,
            )
            .await;

        // Bind server
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server_state = Arc::clone(&state);
        let server_handle = tokio::spawn(async move {
            if let Ok((stream, _peer)) = listener.accept().await {
                let _ = handle_health_request(stream, server_state).await;
            }
        });

        // Make HTTP GET /health request
        let mut stream = TcpStream::connect(addr).await.unwrap();
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        stream
            .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();

        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.unwrap();
        let response_str = String::from_utf8_lossy(&response);

        assert!(response_str.contains("200 OK"));
        assert!(response_str.contains("application/json"));

        // Parse JSON body (after headers)
        let body_start = response_str.find('{').unwrap();
        let body = &response_str[body_start..];
        let parsed: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(parsed["tick"], 42);
        assert_eq!(parsed["tile_count"], 1000);
        assert_eq!(parsed["snapshot_age_ticks"], 2);
        assert_eq!(parsed["season"], "Autumn");

        let _ = server_handle.await;
    }

    #[tokio::test]
    async fn client_disconnect_does_not_crash_server() {
        let state = Arc::new(ServerState::new(r#"{"message_type":"WorldSnapshot"}"#.to_string()));

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server_state = Arc::clone(&state);
        let server_handle = tokio::spawn(async move {
            if let Ok((stream, peer)) = listener.accept().await {
                // This should complete without error when client drops
                let _ = handle_websocket(stream, peer, server_state).await;
            }
        });

        // Connect and immediately drop
        let url = format!("ws://127.0.0.1:{}", addr.port());
        let (ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        drop(ws); // Abrupt disconnect

        // Server task should complete gracefully
        let result = tokio::time::timeout(Duration::from_secs(5), server_handle).await;
        assert!(result.is_ok(), "Server should handle disconnect within 5s");
    }
}
