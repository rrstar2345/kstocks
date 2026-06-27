use anyhow::{anyhow, Result};
use sqlx::{Pool, Sqlite, QueryBuilder};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
// use std::path::Path;
use std::time::Duration;
use tokio::sync::mpsc;

use tracing::{info, error, warn};

#[derive(Debug, Clone)]
pub struct TickRow {
    pub timestamp: String, // you can switch to i64 later if you prefer unix millis
    pub symbol: String,
    pub expiry: String,
    pub strike_price: f64,

    pub ce_last_price: Option<f64>,
    pub ce_change: Option<f64>,
    pub ce_volume: Option<i64>,
    pub ce_bid: Option<f64>,
    pub ce_ask: Option<f64>,

    pub pe_last_price: Option<f64>,
    pub pe_change: Option<f64>,
    pub pe_volume: Option<i64>,
    pub pe_bid: Option<f64>,
    pub pe_ask: Option<f64>,
}

pub type TickSender = mpsc::Sender<TickRow>;

pub async fn init_db(db_path: &std::path::Path) -> Result<Pool<Sqlite>> {
    // let url = format!("sqlite://{}", db_path.display());
    info!("Initialize db at: {}", db_path.display());

    let connection_options = SqliteConnectOptions::new()
    .filename(db_path)
    .create_if_missing(true);
    
    let pool = SqlitePool::connect_with(connection_options).await?;
    info!("Database connected/created successfully.");

    // let pool = SqlitePoolOptions::new()
    //     .max_connections(5)
    //     .min_connections(1)
    //     .acquire_timeout(Duration::from_secs(5))
    //     .connect(&url)
    //     .await?;

    // Performance PRAGMAs for ingestion.
    // If you want safer durability, set synchronous to FULL.
    sqlx::query("PRAGMA journal_mode = WAL;").execute(&pool).await?;
    sqlx::query("PRAGMA synchronous = NORMAL;").execute(&pool).await?;
    sqlx::query("PRAGMA temp_store = MEMORY;").execute(&pool).await?;

    create_schema(&pool).await?;

    Ok(pool)
}

async fn create_schema(pool: &Pool<Sqlite>) -> Result<()> {
    // Adjust columns/types to match how you ingest.
    // (Your schema had timestamp DATETIME; for high-rate ingest consider INTEGER Unix millis.)
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS ticks (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp TEXT NOT NULL,
            symbol TEXT NOT NULL,
            expiry TEXT NOT NULL,
            strike_price REAL NOT NULL,

            ce_last_price REAL,
            ce_change REAL,
            ce_volume INTEGER,
            ce_bid REAL,
            ce_ask REAL,

            pe_last_price REAL,
            pe_change REAL,
            pe_volume INTEGER,
            pe_bid REAL,
            pe_ask REAL,

            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );
        "#,
    )
    .execute(pool)
    .await?;

    // Choose indexes based on your read queries.
    // Keep it minimal to reduce insert overhead.
    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_ticks_symbol_expiry_ts
        ON ticks(symbol, expiry, timestamp);
        "#,
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub fn start_tick_writer(
    pool: Pool<Sqlite>,
) -> (TickSender, tokio::task::JoinHandle<()>) {
    let (tx, mut rx) = mpsc::channel::<TickRow>(10_000);

    let handle = tokio::spawn(async move {
        // Tune these for ~800 msgs/sec:
        let batch_max = 300usize;              // number of rows per batch
        let batch_max_wait = Duration::from_millis(200); // flush at least this often

        let mut buf: Vec<TickRow> = Vec::with_capacity(batch_max);

        loop {
            let first = tokio::select! {
                v = rx.recv() => v,
                _ = tokio::time::sleep(batch_max_wait) => None,
            };

            let Some(row) = first else {
                // If channel closed, flush remaining and exit.
                if !buf.is_empty() {
                    let _ = insert_batch(&pool, &buf).await;
                }
                break;
            };

            buf.push(row);

            // Fill buffer up to batch_max without waiting too long
            while buf.len() < batch_max {
                match tokio::time::timeout(Duration::from_millis(10), rx.recv()).await {
                    Ok(Some(r)) => buf.push(r),
                    _ => break,
                }
            }

            if !buf.is_empty() {
                if let Err(e) = insert_batch(&pool, &buf).await {
                    error!("❌ DB insert batch failed: {}", e);
                }
                buf.clear();
            }
        }
    });

    (tx, handle)
}

async fn insert_batch(pool: &Pool<Sqlite>, rows: &[TickRow]) -> Result<()> {
    // One multi-row INSERT per batch.
    // SQLite supports VALUES (...), (...), ...
    // sqlx requires binding parameters; we build a query with placeholders.
    if rows.is_empty() {
        warn!("No rows to insert");
        return Ok(());
    }

    // ---------- Start a QueryBuilder with a static part ----------
    // The static literal satisfies `SqlSafeStr`.
    let mut builder = QueryBuilder::new(
        r#"
        INSERT INTO ticks (
            timestamp, symbol, expiry, strike_price,
            ce_last_price, ce_change, ce_volume, ce_bid, ce_ask,
            pe_last_price, pe_change, pe_volume, pe_bid, pe_ask
        )
        "#,
    );

    // ---------- Append the dynamic VALUES clause ----------
    // `push_values` creates the `(?, ?, …)` groups and also binds the values.
    builder.push_values(rows, |mut b, row| {
        b
            .push_bind(&row.timestamp)
            .push_bind(&row.symbol)
            .push_bind(&row.expiry)
            .push_bind(row.strike_price)
            
            .push_bind(row.ce_last_price)
            .push_bind(row.ce_change)
            .push_bind(row.ce_volume)
            .push_bind(row.ce_bid)
            .push_bind(row.ce_ask)
            
            .push_bind(row.pe_last_price)
            .push_bind(row.pe_change)
            .push_bind(row.pe_volume)
            .push_bind(row.pe_bid)
            .push_bind(row.pe_ask);
    });

    // ---------- Build the final query and execute ----------
    let query = builder.build();               // -> Query<'static, Sqlite>
    let res = query.execute(pool).await?;

    if res.rows_affected() == 0 {
        return Err(anyhow!("insert_batch inserted 0 rows"));
    }
    Ok(())
}
