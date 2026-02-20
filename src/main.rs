use clap::{Parser, Subcommand};
use std::path::Path;
use tracing::error;
use tracing_subscriber::EnvFilter;

use worldground::cli::commands;
use worldground::config::generation::GenerationParams;
use worldground::config::simulation::SimulationConfig;
use worldground::persistence;
use worldground::world::generation::{generate_world, print_world_summary};

#[derive(Parser)]
#[command(name = "worldground")]
#[command(about = "A perpetual world simulation engine with configurable terrain evolution rules")]
#[command(version)]
struct Cli {
    /// Path to the configuration file
    #[arg(short, long, default_value = "config.toml")]
    config: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate a new world from procedural parameters
    Generate {
        /// Path to world generation config file
        #[arg(short, long, default_value = "worldgen.toml")]
        worldgen: String,

        /// Output snapshot directory
        #[arg(short, long, default_value = "snapshots")]
        output: String,
    },

    /// Start the simulation server
    Run {
        /// Path to a snapshot file to resume from (omit to generate a fresh world)
        #[arg(short, long)]
        world: Option<String>,

        /// Path to world generation config file (used when generating fresh)
        #[arg(long, default_value = "worldgen.toml")]
        worldgen: String,

        /// Override tick rate (Hz) from config
        #[arg(long)]
        tick_rate: Option<f32>,

        /// Override WebSocket port from config
        #[arg(long)]
        port: Option<u16>,

        /// Override log level from config
        #[arg(long)]
        log_level: Option<String>,
    },

    /// Inspect world or tile state
    Inspect {
        /// Tile ID to inspect
        #[arg(short, long)]
        tile: Option<u32>,

        /// Show world-level summary statistics
        #[arg(long)]
        world: bool,
    },

    /// Manage world snapshots
    Snapshots {
        #[command(subcommand)]
        action: SnapshotAction,
    },
}

#[derive(Subcommand)]
enum SnapshotAction {
    /// List available snapshots
    List {
        /// Snapshot directory
        #[arg(short, long, default_value = "snapshots")]
        dir: String,
    },

    /// Restore and display a world from a snapshot file
    Restore {
        /// Path to the snapshot file
        file: String,
    },
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Generate { worldgen, output } => {
            let params = match GenerationParams::from_file(Path::new(&worldgen)) {
                Ok(p) => p,
                Err(e) => {
                    error!("Error loading generation config: {}", e);
                    std::process::exit(1);
                }
            };
            println!("Generating world from {}...", worldgen);
            let world = generate_world(&params);
            print_world_summary(&world);

            let snapshot_dir = Path::new(&output);
            match persistence::save_snapshot(&world, snapshot_dir) {
                Ok(path) => println!("\nWorld saved to {}", path.display()),
                Err(e) => {
                    error!("Cannot save snapshot: {}", e);
                    std::process::exit(1);
                }
            }
        }

        Commands::Run { world, worldgen, tick_rate, port, log_level } => {
            let mut config = match SimulationConfig::from_file(Path::new(&cli.config)) {
                Ok(c) => c,
                Err(e) => {
                    error!("Error loading config: {}", e);
                    std::process::exit(1);
                }
            };

            // Apply CLI overrides
            if let Some(rate) = tick_rate {
                config.tick_rate_hz = rate;
            }
            if let Some(p) = port {
                config.websocket_port = p;
            }
            if let Some(level) = log_level {
                config.log_level = level;
            }

            // Determine world source: explicit snapshot path or generate fresh
            let initial_world = if let Some(ref path) = world {
                commands::WorldSource::Snapshot(path.clone())
            } else {
                commands::WorldSource::Generate(worldgen)
            };

            if let Err(e) = commands::run_simulation(&config, initial_world).await {
                error!("Simulation error: {}", e);
                std::process::exit(1);
            }
        }

        Commands::Inspect { tile, world } => {
            let config = match SimulationConfig::from_file(Path::new(&cli.config)) {
                Ok(c) => c,
                Err(e) => {
                    error!("Error loading config: {}", e);
                    std::process::exit(1);
                }
            };

            if let Err(e) = commands::inspect(&config, tile, world) {
                error!("{}", e);
                std::process::exit(1);
            }
        }

        Commands::Snapshots { action } => match action {
            SnapshotAction::List { dir } => {
                let snapshot_dir = Path::new(&dir);
                match persistence::list_snapshots(snapshot_dir) {
                    Ok(snapshots) => {
                        if snapshots.is_empty() {
                            println!("No snapshots found in {}", snapshot_dir.display());
                        } else {
                            println!(
                                "{:<40} {:>8} {:>12}",
                                "File", "Tick", "Size"
                            );
                            println!("{}", "-".repeat(62));
                            for s in &snapshots {
                                let name = s
                                    .path
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("?");
                                let size_kb = s.file_size / 1024;
                                println!(
                                    "{:<40} {:>8} {:>9} KB",
                                    name, s.tick_count, size_kb
                                );
                            }
                            println!(
                                "\n{} snapshot(s) in {}",
                                snapshots.len(),
                                snapshot_dir.display()
                            );
                        }
                    }
                    Err(e) => {
                        error!("Error listing snapshots: {}", e);
                        std::process::exit(1);
                    }
                }
            }
            SnapshotAction::Restore { file } => {
                let path = Path::new(&file);
                match persistence::load_snapshot(path) {
                    Ok(world) => {
                        println!("Restored world from {}", path.display());
                        print_world_summary(&world);
                    }
                    Err(e) => {
                        error!("Error restoring snapshot: {}", e);
                        std::process::exit(1);
                    }
                }
            }
        },
    }
}
