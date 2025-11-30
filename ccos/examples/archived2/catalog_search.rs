use ccos::catalog::{CatalogEntryKind, CatalogFilter};
use ccos::CCOS;
use clap::Parser;
use std::error::Error;

#[derive(Parser, Debug)]
#[command(
    name = "catalog_search",
    about = "Search the CCOS catalog for reusable plans and capabilities"
)]
struct Args {
    /// Search query text
    query: String,

    /// Use semantic (embedding) search instead of keyword matching
    #[arg(long)]
    semantic: bool,

    /// Limit number of results
    #[arg(long, default_value_t = 10)]
    limit: usize,

    /// Restrict search to plans
    #[arg(long)]
    plans_only: bool,

    /// Restrict search to capabilities
    #[arg(long)]
    capabilities_only: bool,

    /// Only refresh catalog indices without printing results
    #[arg(long)]
    ingest_only: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    if args.plans_only && args.capabilities_only {
        eprintln!("--plans-only and --capabilities-only cannot be used together");
        std::process::exit(1);
    }

    let ccos = CCOS::new().await?;
    let catalog = ccos.get_catalog();

    println!("📦 Ingesting marketplace capabilities...");
    let marketplace = ccos.get_capability_marketplace();
    catalog.ingest_marketplace(marketplace.as_ref()).await;

    println!("🗃️  Ingesting plan archive...");
    let plan_archive = ccos.get_plan_archive();
    catalog.ingest_plan_archive(plan_archive.as_ref());

    if args.ingest_only {
        println!("✅ Catalog ingestion complete");
        return Ok(());
    }

    let filter = if args.plans_only {
        Some(CatalogFilter::for_kind(CatalogEntryKind::Plan))
    } else if args.capabilities_only {
        Some(CatalogFilter::for_kind(CatalogEntryKind::Capability))
    } else {
        None
    };

    let limit = if args.limit == 0 { 10 } else { args.limit };

    let hits = if args.semantic {
        println!(
            "🔍 Performing semantic search (limit = {}) for \"{}\"",
            limit, args.query
        );
        catalog.search_semantic(&args.query, filter.as_ref(), limit)
    } else {
        println!(
            "🔍 Performing keyword search (limit = {}) for \"{}\"",
            limit, args.query
        );
        catalog.search_keyword(&args.query, filter.as_ref(), limit)
    };

    if hits.is_empty() {
        println!("⚠️  No catalog entries matched.");
        return Ok(());
    }

    println!("\nTop {} result(s):", hits.len());
    println!("────────────────────────────────────────────────────────────");

    for (rank, hit) in hits.iter().enumerate() {
        let entry = &hit.entry;
        println!(
            "[{}] {:<8} | score {:>6.3} | {}",
            rank + 1,
            format!("{:?}", entry.kind),
            hit.score,
            entry.id
        );
        if let Some(name) = &entry.name {
            println!("    name        : {}", name);
        }
        if let Some(desc) = &entry.description {
            println!("    description : {}", desc);
        }
        if let Some(goal) = &entry.goal {
            println!("    goal        : {}", goal);
        }
        println!("    source      : {:?}", entry.source);
        if let Some(provider) = &entry.provider {
            println!("    provider    : {}", provider);
        }
        if let Some(location) = &entry.location {
            println!("    location    : {:?}", location);
        }
        if !entry.tags.is_empty() {
            println!("    tags        : {}", entry.tags.join(", "));
        }
        if !entry.inputs.is_empty() {
            println!("    inputs      : {}", entry.inputs.join(", "));
        }
        if !entry.outputs.is_empty() {
            println!("    outputs     : {}", entry.outputs.join(", "));
        }
        if !entry.capability_refs.is_empty() {
            println!("    capabilities: {}", entry.capability_refs.join(", "));
        }
        println!("────────────────────────────────────────────────────────────");
    }

    Ok(())
}
