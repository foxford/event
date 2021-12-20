ALTER TABLE room
    ADD COLUMN validate_whiteboard_access BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN whiteboard_access jsonb NOT NULL DEFAULT '{}'::jsonb;
