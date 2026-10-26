// Synthetic tests only. Neither method is included in an installed connector.
impl Journal {
    pub(crate) fn pressure_fixture_usage(&self) -> (i64, i64, i64) {
        self.db.query_row("SELECT identities,result_bytes,(SELECT coalesce(sum(length(result)),0) FROM encrypted_requests) FROM encrypted_usage WHERE id=1", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap()
    }
    pub(crate) fn pressure_fixture_row(
        &self,
        id: &str,
    ) -> (String, String, Option<String>, bool, f64, f64) {
        self.db.query_row("SELECT digest,header,result,delivered,created_at,updated_at FROM encrypted_requests WHERE request_id=?", [id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).unwrap()
    }
}
