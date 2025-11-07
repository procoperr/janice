use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use std::process;

use janice::{diff_scans, scan_directory, sync_changes, SyncOptions};

#[derive(Parser)]
#[command(
    name = "jan",
    version,
    about = "Beautifully fast, simple & reliable file syncing"
)]
struct Cli {
    /// Source directory
    source: PathBuf,

    /// Destination directory
    dest: PathBuf,

    /// Dry run (show changes without applying)
    #[arg(short = 'n', long)]
    dry_run: bool,

    /// Delete files in dest not in source
    #[arg(short, long)]
    delete: bool,

    /// Skip confirmation prompt
    #[arg(short = 'y')]
    yes: bool,

    /// Quiet mode (no progress)
    #[arg(short, long)]
    quiet: bool,

    /// Verbose output
    #[arg(short, long)]
    verbose: bool,

    /// Number of threads (default: CPU count)
    #[arg(short = 'j', long)]
    threads: Option<usize>,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {e:#}");
        process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    // Configure thread pool if specified
    if let Some(t) = cli.threads {
        rayon::ThreadPoolBuilder::new()
            .num_threads(t)
            .build_global()
            .map_err(|e| anyhow::anyhow!("Failed to configure thread pool: {e}"))?;
    }

    // Validate paths
    if !cli.source.exists() {
        anyhow::bail!("Source does not exist: {}", cli.source.display());
    }
    if !cli.dest.exists() {
        anyhow::bail!("Destination does not exist: {}", cli.dest.display());
    }

    // Scan source
    if cli.verbose && !cli.quiet {
        println!("Scanning: {}", cli.source.display());
    }
    let src = scan_directory(&cli.source)?;

    if cli.verbose && !cli.quiet {
        println!("{} files, {}", src.files.len(), format_bytes(src.total_size()));
    }

    // Scan destination
    if cli.verbose && !cli.quiet {
        println!("Scanning: {}", cli.dest.display());
    }
    let dst = scan_directory(&cli.dest)?;

    if cli.verbose && !cli.quiet {
        println!("{} files, {}", dst.files.len(), format_bytes(dst.total_size()));
    }

    // Compute diff
    let diff = diff_scans(&src, &dst)?;

    // Check if there are any changes
    let changes = diff.added.len() + diff.modified.len() + diff.renamed.len();
    if changes == 0 && (!cli.delete || diff.removed.is_empty()) {
        if !cli.quiet {
            println!("In sync");
        }
        return Ok(());
    }

    // Display summary
    if !cli.quiet {
        print_diff_summary(&diff, cli.delete, cli.verbose);
    }

    // Dry run - exit after showing changes
    if cli.dry_run {
        if !cli.quiet {
            println!("(dry run)");
        }
        return Ok(());
    }

    // Confirm
    if !cli.yes && !cli.quiet {
        print!("Proceed? [y/N] ");
        std::io::Write::flush(&mut std::io::stdout())?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            return Ok(());
        }
    }

    // Sync
    sync_changes(
        &cli.source,
        &cli.dest,
        &diff,
        &SyncOptions {
            delete_removed: cli.delete,
            preserve_timestamps: true,
            verify_after_copy: false,
        },
    )?;

    if !cli.quiet {
        println!("Done");
    }

    Ok(())
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "K", "M", "G", "T"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;

    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }

    if unit_idx == 0 {
        format!("{}{}", bytes, UNITS[0])
    } else {
        format!("{:.2}{}", size, UNITS[unit_idx])
    }
}

fn print_diff_summary(diff: &janice::DiffResult, delete: bool, verbose: bool) {
    let mut parts = Vec::new();

    if !diff.added.is_empty() {
        parts.push(format!("{} new", diff.added.len()));
    }
    if !diff.modified.is_empty() {
        parts.push(format!("{} modified", diff.modified.len()));
    }
    if !diff.renamed.is_empty() {
        parts.push(format!("{} renamed", diff.renamed.len()));
    }
    if delete && !diff.removed.is_empty() {
        parts.push(format!("{} deleted", diff.removed.len()));
    }

    println!("{}", parts.join(", "));

    if verbose {
        if !diff.added.is_empty() {
            println!("New:");
            for file in diff.added.iter().take(5) {
                println!("  {}", file.path.display());
            }
            if diff.added.len() > 5 {
                println!("  ... {} more", diff.added.len() - 5);
            }
        }

        if !diff.modified.is_empty() {
            println!("Modified:");
            for file in diff.modified.iter().take(5) {
                println!("  {}", file.path.display());
            }
            if diff.modified.len() > 5 {
                println!("  ... {} more", diff.modified.len() - 5);
            }
        }

        if !diff.renamed.is_empty() {
            println!("Renamed:");
            for (old, new) in diff.renamed.iter().take(5) {
                println!("  {} -> {}", old.path.display(), new.path.display());
            }
            if diff.renamed.len() > 5 {
                println!("  ... {} more", diff.renamed.len() - 5);
            }
        }

        if delete && !diff.removed.is_empty() {
            println!("Deleted:");
            for file in diff.removed.iter().take(5) {
                println!("  {}", file.path.display());
            }
            if diff.removed.len() > 5 {
                println!("  ... {} more", diff.removed.len() - 5);
            }
        }
    }
}
