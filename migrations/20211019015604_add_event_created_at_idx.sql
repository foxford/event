CREATE INDEX IF NOT EXISTS event_created_at_idx ON event USING BRIN (created_at);
