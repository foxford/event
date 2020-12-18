ALTER TABLE room ADD COLUMN preserve_history BOOLEAN NOT NULL DEFAULT 't';
CREATE INDEX room_preserve_history_idx ON room (preserve_history);
