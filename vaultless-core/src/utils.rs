use crate::error::{VaultlessError};
use sqlx::{Executor, FromRow};
use std::marker::PhantomData;

pub struct PaginationMeta {
    pub total_count: i64,
    pub total_pages: i64,
    pub page: i64,
    pub page_size: i64,
}

pub async fn paginate<'c, E, T>(
    exec: E,
    base_query: &str,
    count_query: &str,
    page: i64,
    page_size: i64,
) -> Result<(Vec<T>, PaginationMeta), sqlx::Error>
where
    E: Executor<'c, Database = sqlx::Postgres> + Clone,
    T: for<'r> FromRow<'r, sqlx::postgres::PgRow> + Send + Unpin,
{
    let offset = (page - 1) * page_size;

    // Count
    let total_count: i64 = sqlx::query_scalar(count_query)
        .fetch_one(exec.clone())
        .await?;

    // Data
    let query = format!("{base_query} LIMIT $1 OFFSET $2");

    let rows = sqlx::query_as::<_, T>(&query)
        .bind(page_size)
        .bind(offset)
        .fetch_all(exec)
        .await?;

    let total_pages = (total_count as f64 / page_size as f64).ceil() as i64;

    Ok((
        rows,
        PaginationMeta {
            total_count,
            total_pages,
            page,
            page_size,
        },
    ))
}