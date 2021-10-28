-- Add migration script here
ALTER TABLE room ADD COLUMN locked_types text[] NOT NULL DEFAULT '{}';
