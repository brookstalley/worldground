use clap::{Parser, Subcommand};
use std::path::Path;

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
        /// Path to a specific world snapshot to load
        #[arg(short, long)]
        world: Option<String>,
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
    let cli = Cli::parse();

    match cli.command {
        Commands::Generate { worldgen, output } => {
            let params = match GenerationParams::from_file(Path::new(&worldgen)) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("Error loading generation config: {}", e);
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
                    eprintln!("Cannot save snapshot: {}", e);
                    std::process::exit(1);
                }
            }
        }

        Commands::Run { world } => {
            let config = match SimulationConfig::from_file(Path::new(&cli.config)) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error loading config: {}", e);
                    std::process::exit(1);
                }
            };

            if let Err(e) = commands::run_simulation(&config, world.as_deref()).await {
                eprintln!("Simulation error: {}", e);
                std::process::exit(1);
            }
        }

        Commands::Inspect { tile, world } => {
            let config = match SimulationConfig::from_file(Path::new(&cli.config)) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error loading config: {}", e);
                    std::process::exit(1);
                }
            };

            if let Err(e) = commands::inspect(&config, tile, world) {
                eprintln!("Error: {}", e);
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
                        eprintln!("Error listing snapshots: {}", e);
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
                        eprintln!("Error restoring snapshot: {}", e);
                        std::process::exit(1);
                    }
                }
            }
        },
    }
}
