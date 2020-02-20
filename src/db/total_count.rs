//! Wraps a query into `SELECT COUNT(*) FROM (…) AS sub` to get the total number of rows returned.

use diesel::pg::Pg;
use diesel::prelude::*;
use diesel::query_builder::*;
use diesel::query_dsl::methods::LoadQuery;
use diesel::sql_types::BigInt;

pub(super) trait TotalCount: Sized {
    fn total_count(self) -> TotalCounted<Self>;
}

impl<T> TotalCount for T {
    fn total_count(self) -> TotalCounted<Self> {
        TotalCounted { query: self }
    }
}

#[derive(Debug, Clone, Copy, QueryId)]
pub(super) struct TotalCounted<T> {
    query: T,
}

impl<T> TotalCounted<T> {
    pub(super) fn execute(self, conn: &PgConnection) -> QueryResult<i64>
    where
        Self: LoadQuery<PgConnection, i64>,
    {
        self.load::<i64>(conn)?
            .get(0)
            .map(|x| *x)
            .ok_or(diesel::result::Error::NotFound)
    }
}

impl<T: Query> Query for TotalCounted<T> {
    type SqlType = BigInt;
}

impl<T> RunQueryDsl<PgConnection> for TotalCounted<T> {}

impl<T> QueryFragment<Pg> for TotalCounted<T>
where
    T: QueryFragment<Pg>,
{
    fn walk_ast(&self, mut out: AstPass<Pg>) -> QueryResult<()> {
        out.push_sql("SELECT COUNT(*) FROM (");
        self.query.walk_ast(out.reborrow())?;
        out.push_sql(") AS sub");
        Ok(())
    }
}
